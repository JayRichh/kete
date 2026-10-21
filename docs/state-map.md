# State map

<sub>[← README](../README.md) · [Studio mode](studio.md) · [Studio board](studio-board.md) · [Roles](roles.md) · [Commands](commands.md) · [Keys](keys.md)</sub>

The state map is one graph for a studio repo. It holds what exists in the repo and who is on it: departments, systems, subsystems, decisions, lanes, pull requests, files and agents. Edges say who owns what, what depends on what, what is touched by whom and what is blocked by what.

The graph has two layers. The repo layer comes from a script the repo keeps. The live layer is what Kete already knows: the repo's worktrees, its open pull requests, its sessions and their subagents. Kete merges the two and writes the result as a file. The apps read the file.

The map is for you and for agents. You read it as a tree and a small graph. An agent reads a slice of it: a short text about one node and what joins it, under 2 KB.

## The file

The daemon writes the map to `.kete/state-map.json` in the repo's main checkout (`crates/nebula-core/src/state_map.rs:24`). The file stays out of git. It is rebuilt all the time, and it holds your local worktree paths. Kete adds the line `.kete/state-map.json*` to the checkout's `.git/info/exclude` before it writes, as it does for its other per-checkout files. That line also covers the temp file. Nothing is added to the repo's `.gitignore`.

The file is one line of JSON. Kete writes it to `state-map.json.tmp` and renames it into place, so a reader never sees half of it. A file over 8 MiB is not read.

| Key | What it holds |
|---|---|
| `repo` | The repo root, with `/` separators. |
| `built_at` | When the map was built, in epoch milliseconds. |
| `nodes` | Every node. Sorted by kind, then id. |
| `edges` | Every edge, as `from`, `to` and `kind`. |
| `note` | Why the repo layer is missing, when it is. Otherwise null. |

The map is built only for a repo in [studio mode](studio.md) on this machine, which means a `kete studio enable` entry in `config.local.json`. A committed manifest alone builds nothing, and runs nothing (`crates/nebula-daemon/src/state_map.rs:13`). The repo must also be registered in Kete.

## What a node holds

| Field | Meaning |
|---|---|
| `id` | The node's id. The repo's own id where it has one, such as `D-012`. |
| `kind` | One of `department`, `system`, `subsystem`, `decision`, `lane`, `pr`, `file`, `agent`. |
| `label` | The name to show. |
| `status` | A word, such as `open`, `active` or `needs_feedback`. |
| `owner` | Who owns it, or null. |
| `changed_at` | The last change, in epoch milliseconds, or null. |
| `questions` | The count of open questions on it. |
| `links` | Repo-relative paths or URLs. |

An edge is `from`, `to` and `kind`. The kind is one of `owns`, `depends_on`, `touched_by` and `blocked_by`. `from` owns, depends on, is touched by or is blocked by `to`.

## The repo layer: the extractor

The repo says how to build its layer. Name a command under `[state_map]` in `.kete/studio.toml` (`state_map.rs:135`).

```toml
version = 1

[state_map]
extractor = ["python", "Tools/Studio/state_map.py"]
```

| Key | What it is |
|---|---|
| `extractor` | A list of strings. The first is the program. The rest are its arguments. |

Kete runs the command in the repo root and reads its output. The command prints one JSON object on stdout.

```json
{
  "nodes": [
    {"id": "dept:design", "kind": "department", "label": "Design"},
    {"id": "SYS-1", "kind": "system", "label": "Combat", "status": "building", "owner": "Design lead"},
    {"id": "D-012", "kind": "decision", "label": "Combat pacing", "status": "open",
     "owner": "Design lead", "questions": 2, "links": ["docs/decisions/D-012.md"]}
  ],
  "edges": [
    {"from": "dept:design", "to": "SYS-1", "kind": "owns"},
    {"from": "SYS-1", "to": "D-012", "kind": "owns"},
    {"from": "D-012", "to": "lane:lane-a", "kind": "blocked_by"}
  ]
}
```

Only `id` and `kind` are needed on a node. Only `from`, `to` and `kind` are needed on an edge. Every other field may be left out. A key Kete does not know is ignored.

### What counts as bad

The rules are in `parse_layer` (`state_map.rs:154`).

| Case | What Kete does |
|---|---|
| A node with an unknown `kind`, no `id` or a blank `id` | Leaves that node out. The rest are kept. |
| A node or edge with a field of the wrong type | Leaves that entry out. |
| An edge with an unknown `kind` or a missing end | Leaves that edge out. |
| An edge to an id that is not a node, or from a node to itself | Dropped in the merge. |
| The same edge twice | Kept once. |
| The same node id twice in the layer | The first one is kept. |
| Output that is not JSON, or not a JSON object | The whole repo layer is refused. |
| A missing `nodes` or `edges` list | Taken as empty. |

When the whole repo layer is refused, the live layer is still written. The map's `note` says why, in one line, and the desktop panel shows it as "Repo layer: ...". The note also covers a missing manifest, a manifest with no extractor, a program that cannot start, a non-zero exit and a timeout.

### How the extractor is run

The extractor is repo code that runs without anyone watching. These rules hold (`crates/nebula-daemon/src/state_map.rs:330` and `:366`).

| Rule | Value |
|---|---|
| Shell | None. The argv runs as given. |
| Working directory | The repo root. |
| Stdin | Closed. |
| Stderr | Dropped. |
| Output cap | 4 MiB (4,194,304 bytes). Over it, the output is refused. |
| Timeout | 30 seconds. Past it, the process is killed. |
| Exit code | Must be 0. |
| Frequency | At most once a minute per repo. A failure is kept for that minute too. |

## The live layer

Kete builds the live layer itself (`state_map.rs:269`). Its ids are fixed, so an extractor can point edges at them.

| Node | Id | Label | Status |
|---|---|---|---|
| Department | `dept:<slug>` | The department's label from the manifest's `departments` table | None |
| Lane | `lane:<branch>` | The branch | `active` while a session on it runs or needs feedback, else `idle` |
| Agent | `agent:<id>` | The session's name | The session's status, such as `running` or `needs_feedback` |
| Subagent | `agent:<id>/<subagent id>` | Its description | Its status |
| Pull request | `pr:<number>` | `#<number> <title>` | `draft` or `open` |

The slug of a department is the one the [studio board](studio-board.md#the-departments-table) uses.

| Edge | Meaning |
|---|---|
| `lane:<branch>` owns `agent:<id>` | A session runs in that worktree. |
| `agent:<id>` owns `agent:<id>/<subagent id>` | A running subagent of that session. |
| `lane:<head branch>` owns `pr:<number>` | The pull request is from that branch. |
| `dept:<slug>` is touched by `agent:<id>` | The session's [role card](roles.md) names that department. |

A lane has the branch's worktree path in its `links`. A lane exists for every worktree of the repo, the main checkout included. An agent has its role card's label as its `owner` and its last status change as `changed_at`. Archived sessions are left out. A subagent shows only while it runs.

Open pull requests come from `gh pr list --state open --limit 100`. Kete asks at most once every 5 minutes for each repo. When `gh` fails, the map has no pull requests.

## How the layers merge

The rules are in `merge` (`state_map.rs:347`).

| Case | Result |
|---|---|
| A node only in one layer | Kept as it is. |
| A node in both layers | The repo's `kind` and `label` stay. The live `status` and `owner` win where the live side has one. `changed_at` is the later of the two. `links` are the union. `questions` come from the repo. |
| An edge | Edges from both layers are joined. An edge whose end is not a node is dropped. So are an edge to itself and a repeat. |

An extractor can use the live ids to join its own nodes to Kete's. In the example above, `D-012` is blocked by `lane:lane-a`. The edge stays while that lane exists. An extractor that names a node `dept:design` gets the live status and owner when the manifest's table has a Design department.

## When it rebuilds

The daemon checks once a second whether a rebuild is due (`state_map.rs:397`, `crates/nebula-daemon/src/state_map.rs:66`).

| Case | Rule |
|---|---|
| Never built since the daemon started | Rebuild now. |
| An event changed the map | Rebuild once 5 seconds have passed since the last build. A burst of events is one rebuild. |
| No event | Rebuild once 60 seconds have passed. |

These events count (`crates/nebula-daemon/src/state_map.rs:44`): a session, worktree or project added or removed, a status change, a session ending, a role change and a subagent starting, changing or ending. A daemon that falls behind on events treats it as a change.

The rebuild runs off the event path. It rebuilds every studio repo and writes each file. A repo whose studio mode went off is no longer built, and its old file stays where it is.

## In the desktop app

The state map panel (`desktop/src/lib/state/StateMap.svelte`) sits on the Studio page, under the board. It reads `.kete/state-map.json` from the repo's checkout when it opens, every 10 seconds while the page is visible, and when you press Reload (`StateMap.svelte:21`). It writes nothing.

The header shows the repo, when the map was built and the node count. A repo layer note shows under it. A map with no file says: "No state map yet. Kete builds one while studio mode is on for this repo." A file that cannot be read shows the reason, marked with a cross.

On the left is the tree. It follows the `owns` edges. The roots are the nodes nothing owns, and they start open. A node owned twice shows under its first owner. Each row shows the label and a line of kind, status and open questions. A row with a session waiting on you or with open questions is marked with `!`.

| Key | What it does |
|---|---|
| `↑` `↓` | Move between rows. |
| `→` or `Enter` | Opens the node. |
| `←` | Closes the node. |

A click on the arrow beside a row opens or closes it. A click on the row selects it.

On the right is the selected node. It shows the owner, the last change and the links. Below that is a graph of the node and the nodes joined to it, with the edge kinds on the lines. The graph draws up to 8 joined nodes. Under it, a list holds every joined node. Click one in the list to select it. The tree opens what hides it.

Copy slice puts the slice for the selected node on the clipboard and says "Copied" or "Could not copy".

## In the TUI and the CLI

Press `s` on the studio board (or pick State map from its menu) to open the TUI overlay (`crates/nebula-tui/src/state_overlay.rs`). It draws the same tree. Roots start open. The footer names the keys. Esc goes back to the board.

| Key | What it does |
|---|---|
| `j` `k` or `↓` `↑` | Move between rows. |
| `l` or `→` | Opens the node, or steps into it when it is open. |
| `h` or `←` | Closes the node, or steps to its parent. |
| `Enter` or `Space` | Opens or closes the node. |
| `Home` `End` `PgUp` `PgDn` | Jump. |
| `c` | Copies the selected node's slice. |
| `r` | Reads the file again. |
| `Esc` or `q` | Goes back to the board. |

| Command | Prints |
|---|---|
| `kete state` | The tree, as indented text. One line for each node. |
| `kete state --json` | The whole map. |
| `kete state --slice <node>` | The slice for the node. |
| `kete state --slice <node> --json` | The node, the nodes joined to it, and those edges, as a map. |

`kete state` reads the map of the repo you are in, from any of its worktrees, or of the repo you name (`kete state ../game`). It reads the file in the repo's main checkout (`crates/nebula/src/state.rs`). With no map it says so and exits 1. Sessions may run it without a prompt, like `kete usage`.

## The slice

A slice is the text an agent reads about one node (`state_map.rs:508`). It is never over 2,048 bytes. It holds the node and the nodes one edge away. It lists no more.

```
State map of C:/Code/game, built 3 min ago.
decision D-012: Combat pacing (open), 2 open questions
Owner: Design lead.
Links: docs/decisions/D-012.md.
Owned by:
- system SYS-1: Combat (building)
Blocked by:
- lane lane:lane-a: lane-a (active)
```

| Line | Holds |
|---|---|
| First | The repo and how long ago the map was built. |
| Second | The node as `kind id: label (status)`, then the open question count when it is not zero. |
| `Owner`, `Changed`, `Links` | Each shown only when the node has it. |
| Groups | `Owned by`, `Owns`, `Depends on`, `Needed by`, `Blocked by`, `Blocks`, `Touched by`, `Touches`, in that order. A group is shown only when it has nodes. |

A label over 100 characters is cut and ends in `...`. When the slice would pass the cap, Kete drops the lines that do not fit and ends with a line that says how many were left out:

```
And 12 lines more. `kete state --slice hub --json` has them all.
```

## More

- [Studio mode](studio.md): the manifest, the departments table and the files Kete keeps.
- [Studio board](studio-board.md): the same repo by department.
- [Roles](roles.md): the role cards whose department places an agent in the map.
