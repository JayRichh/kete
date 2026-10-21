# Triage

<sub>[← README](../README.md) · [Studio mode](studio.md) · [Roles](roles.md) · [Orchestrator](orchestrator.md) · [Commands](commands.md) · [Keys](keys.md)</sub>

The triage desk files a note from one line. You type the line. Kete writes it as an inbox note in the repo. Then it asks the repo's triage desk, in one turn, to say where the note goes. The desk sets the note's recipient, subject, priority and status, and answers in one line, which Kete shows you.

```sh
kete studio note "storm sky bands at dusk" --project my-game --triage
```

The same line can be typed under the orchestrator in the desktop app or in the terminal app. All three do the same thing. Without `--triage` the command only files the note.

## Where the inbox is

Kete reads the inbox folder from two places, in this order:

1. `studio_inbox` for the repo in `config.local.json`. It is set on this machine only.
2. `inbox` in the repo's committed `.kete/studio.toml`. See [Studio mode](studio.md#ketestudiotoml).

The folder is relative to the repo root. An absolute path, or one that uses `..`, is refused with "the inbox `<path>` must be a folder inside the repo". A repo with neither names no inbox, and the note is not filed. Kete makes the folder when it is missing.

## The note file

A note is `<inbox>/<id>.md`.

The id is the UTC date and a slug of the first 6 words of the text: `2026-10-22-storm-sky-bands-at-dusk`. A slug is lower case. Letters of any script are kept, so `Māori` stays `māori`. An apostrophe is dropped, so `HUD's` becomes `huds`. Any other character separates words. A text with no words gets the slug `note`.

A `-2` suffix is added only when a resolved note already holds the id (`-3` after that). An open note holding the id is a duplicate, and nothing is written.

Kete writes these frontmatter keys, in this order:

| Key | Value |
|---|---|
| `id` | The id above. |
| `status` | `new`. |
| `to` | `--to`, else `triage`. |
| `about` | `--about`. Left out when not given. |
| `priority` | `--priority`. Left out when not given, and a reader takes `normal`. |
| `title` | The first line of the text, with runs of whitespace made one space, cut on a word boundary at 160 characters. A cut title ends with `…`. |
| `at` | When it was filed, in UTC. |
| `by` | `jay`. |

The body is the full text you typed. A value that a reader could take for a comment, a quote or a key is put in double quotes, and a newline in a value becomes a space, so a value can never add a key of its own. An empty note is refused. So is one over 8 KiB (8,192 bytes).

The file is written under a dot name (`.<id>.md.tmp`) and renamed into place. The inbox never lists a half-written note, and a failed write leaves no temp file behind.

### A note as written

```
---
id: 2026-10-22-storm-sky-bands-at-dusk
status: new
to: triage
title: storm sky bands at dusk
at: 2026-10-22T01:22:00Z
by: jay
---
storm sky bands at dusk
```

After the desk's turn it holds, for example, `to: art-and-3d`, `about: World and weather`, `priority: high` and `status: open`. Those edits are the desk's.

## The duplicate rule

A new note is refused as a duplicate when an open note:

- has the same id, or
- shares the first 5 words of its title. Words are compared in lower case, letters and digits only, so `STORM sky-bands at Dusk!` matches `storm sky bands at dusk`.

A note counts as open when its status is `new` or `open`. A resolved, done, closed or rejected note never blocks. A file in the inbox that Kete cannot read, and that holds the id, blocks too, because a rename would replace it.

A refused note names the existing note. The command exits with code 3 and prints the path of the note it repeats.

## Where the note is sent

Once the note is on disk, Kete sends one turn to the first of these that exists:

1. **A live desk.** A session that is alive, not archived, in the repo's root worktree, and running under the `triage` role card. If there are several with under 60 percent of their context used, the one with the least used gets the turn.
2. **A new desk.** When no live desk has room and the repo's committed `.kete/roles.toml` defines a `triage` role, Kete starts a session named `triage desk` in the root worktree with that card, and its first prompt is the turn.
3. **The orchestrator.** When the repo defines no `triage` role, the orchestrator gets the same turn with `[triage] ` in front.

With the Orchestrator target chosen (desktop) the note goes to the orchestrator and to nothing else. With no desk and no orchestrator, Kete says so: "no triage desk and no orchestrator: the repo's roles file defines no `triage` card, and none is running". The note is still on disk.

### The 60 percent rule

A desk at or over 60 percent of its context is left alone, and the next note starts a fresh desk. The percent used is the larger of two numbers:

- the share of the model's context window the session has used;
- the share of the role card's `context_budget` that its tokens make up, when the card sets one.

A session whose usage is not known counts as 0 percent.

### The prompt

The desk gets this one line, with the note's absolute path. The path uses `/` separators so it reads the same from any worktree.

```
Triage the inbox note C:/Code/my-game/notes/inbox/2026-10-22-storm-sky-bands-at-dusk.md: read it, then in its frontmatter set to (a recipient, a department slug, or jay), about, priority (high, normal or low) and status: open. Change nothing else. Answer in one line: where it goes and why.
```

The orchestrator gets the same line after `[triage] `.

### The wait

Kete waits up to 120 seconds for the desk's turn to end, then reads the session back. The line shown is the last `Assistant:` line of the transcript, else the last line with text, cut at 200 characters.

If the desk has not answered in 120 seconds, the strip shows:

```
filed 2026-10-22-storm-sky-bands-at-dusk: the desk is busy, so the note waits in the inbox
```

The note stays in the inbox and the desk may still finish it.

## The `triage` role card

Define the card in the repo's `.kete/roles.toml` and commit it. Kete reads the committed file only. See [Roles](roles.md).

```toml
[roles.triage]
label = "Triage"
model = "sonnet"
effort = "low"
permission = "prompt"
tools = ["Read", "Edit", "Glob", "Grep"]
allow = ["Edit(notes/inbox/*.md)"]
context_budget = 100000
prompt = """
You are the triage desk. Each turn names one inbox note. Read it.
Edit only that note's frontmatter: set to, about, priority and status: open.
Never change the body. Answer in one line: where the note goes and why.
"""
```

The `allow` rule pre-approves edits to the inbox's notes, so the desk does not stop to ask you before each one. A desk that does ask shows as needs you, and its 120 second wait keeps running; past it the strip says the desk is busy. Use the repo's own inbox folder in the rule.

Keep the inbox out of the roles file's `governance` list. A governance path is denied to every role session, and the desk could not edit its notes.

A card edit reaches new desks only. A desk that is already running keeps the prompt it started with. When you change the card, the next fresh desk uses it, which happens at the 60 percent rule.

## Three ways to file

### CLI

```sh
kete studio note "<text>" [--project P] [--to R] [--about A] [--priority high|normal|low] [--triage] [--json]
```

| Flag | Does |
|---|---|
| `--project P` | The repo to file into. |
| `--to R` | The recipient, in place of `triage`. |
| `--about A` | The note's subject. |
| `--priority` | `high`, `normal` or `low`. |
| `--triage` | Also sends the turn to the desk. Without it the command only files. |
| `--json` | Prints the result as JSON. |

Exit code 3 means a duplicate. See [Commands](commands.md).

### Terminal app

In the orchestrator view, press `^Q` to unlock the keys to the chip strip. Then:

| Key | Does |
|---|---|
| `t` | Opens a one-line field on the bottom row. |
| Enter | Files the note and sends it. |
| Esc | Closes the field. |

After Enter the row shows the route line until your next key. The `triage_note` verb opens the same field. It has no key by default. Bind one in [Keys](keys.md).

### Desktop app

Under the orchestrator's session there is a strip with:

- a one-line field;
- a target chip, Triage desk or Orchestrator;
- the repo the note files into;
- the route line from the last note;
- the last 3 notes filed, as links.

Enter files and sends. The `triage_note` verb focuses the field. It has no key by default.

## What it does not do

- It does not watch the inbox. A note written by hand is not triaged until you send it to someone.
- It never edits a note after it files it. Kete writes the file once.
- It does not enforce what the desk does. The desk's edits are the desk's, within its role card's tools and rules.
- It does not start work on a note. The [orchestrator](orchestrator.md) reads open notes as [Studio mode](studio.md) describes.

## Pending Jay

- The id's date is UTC. The alternative is the local date. Default: UTC, so one note has one id on every machine.
