# Orchestrator

<sub>[← README](../README.md) · [Keys](keys.md) · [Commands](commands.md) · [Sessions](sessions.md) · [Configuration](configuration.md) · [How it works](how-it-works.md)</sub>

The ORCHESTRATOR is a chat-first way to run kete. Instead of moving from
project to project and opening each worktree by hand, you keep one pinned agent
session in front of you and ask it to coordinate the rest.

## Starting one

- **`F10` in kete.** With no orchestrator on the machine, `F10` opens a small
  prompt: the harness (Claude by default) and the profile to run it under.
  `Enter` starts it and the view opens on it.
- **`kete orchestrator start`** from your own shell does the same:
  `--kind codex`, `--model`, `--effort`, `--profile work`, `--name`, and
  `--project <id-or-name>` to run it in that project's root worktree instead.
- **"Make orchestrator"** on any session card's menu pins an existing session.
  If it is idle it is handed the orchestrator guidance as its next turn;
  otherwise it gets it with its next cold start. **"Unpin orchestrator"** on the
  orchestrator's card (or `kete orchestrator clear`) turns it back into an
  ordinary session.

There is exactly **one orchestrator per machine**. Starting another is refused
while one is running unless you pass `--replace`; pinning a session demotes
whichever session held the role before (it keeps running, as a worker).
`kete orchestrator status` says which session it is, where it runs and what it
is doing.

**Where it runs.** Without `--project`, the orchestrator gets a workspace of its
own: `<data dir>/orchestrator` (`%LOCALAPPDATA%\kete\data\orchestrator` on
Windows), a git-initialized folder registered once as a project named
`orchestrator`, which shows as its own project tab. Its hooks, notes and scratch
files never land in one of your repos. Every later start reuses it.

## The view

`F10` with an orchestrator opens the ORCHESTRATOR VIEW: its session takes the
whole screen, input locked, so you are typing to it straight away. A strip
across the top holds a chip for every other live session, in the attention
order of `/`, `.` and `,`: needs input first, then running, then finished turns
nobody has read, then the rest. A click on a chip opens the review modal on
that session. Under 60 columns the strip collapses to one chip that counts the
sessions and how many need you.

- `F10` again goes back to the grid; it reaches through a locked agent pane
  (in a terminal tab F10 stays the shell program's, so `^q` first).
- `^q` steps out of the orchestrator to the chips without leaving the view:
  `←`/`→` walk them, `Enter` reviews one, `Esc` goes back to typing, and `^q`
  again leaves for the grid.
- The orchestrator's card wears a `◆ orchestrator` badge on the grid.
- **Open on the orchestrator** (Settings, General, `orchestrator_view`) picks
  whether kete starts on this view when an orchestrator exists: `auto` (the
  default) only in a terminal narrower than 100 columns, `always`, or `never`.

## Review modal

The orchestrator (`kete orchestrator review`), or you from a chip, can open a
tabbed modal over one or more sessions:

- **Terminal** is the session's own pane, live: the modal moves the pane onto
  it, and puts the pane back where it was when the modal closes. `Enter`, or a
  click on the screen, closes the modal into that session with its input locked.
- **Diff** is the uncommitted diff of the session's worktree, every changed file,
  read with the same git loader as the diff viewer.
- **History** is the session's recent conversation from its harness transcript
  when kete knows one, else its cleaned terminal output.
- **PR** is the pull request on the session's checkout: its whole conversation
  once it has been read (`v`), else its headline, or "no PR". `Enter` opens it
  in the browser.

`Tab`/`⇧Tab` and `1`-`4` switch tabs, `←`/`→` (or `[`/`]`) walk the sessions
under review, `↑`/`↓` and `PgUp`/`PgDn` scroll, `Esc` closes. The title names
the session, not its id.

A review the orchestrator opens while you are typing into any session, the
orchestrator itself included (or with another modal up), does not take the
screen: a toast says so, and `F10` shows it (`^q` then `F10` from a terminal
tab). One that opens at once ignores `Enter` and a click on the screen for
three quarters of a second, so a keystroke meant for something else never
drops you into a worker. Closing a review puts the pane back on what it showed,
or empties it if it showed nothing.

## In the desktop window

The orchestrator is pinned at the top of the rail, above the repos, with its
live state (or how many of its sessions need you). With none yet, the rail
reads **Start orchestrator**: a form with harness, profile, model and effort,
each starting on the Agents tab's defaults. `0` (or `F10` outside a shell),
the palette (**Open orchestrator**, **Start orchestrator**, **Unpin
orchestrator**) and a session's right-click menu (**Make orchestrator**,
**Unpin orchestrator**, **Review**) reach the same places. A daemon older than
protocol 54 is never sent these requests: the window says to run `kete reload`.

- **The view** is a split: on the left, every session the orchestrator started
  in any repo, as one tree with a repo / branch chip per row, the counts of
  what in that tree needs you, and below it what wants you elsewhere; on the
  right, the orchestrator's own session, so the chat stays primary. A click on
  a row opens the review sheet. The quick prompt under it sends to the
  orchestrator as a follow-up (its target reads **Orchestrator**), until you
  pick a repo.
- **The overview** keeps each session in its own repo, and a session started
  from another repo says so: a `from ◆ Orchestrator` chip that opens the
  orchestrator. The orchestrator's row and session header wear the badge.
- **Reviews** the orchestrator asks for arrive as a notice, *Orchestrator wants
  you to review X*, that never takes the focus; **Open** shows the review
  sheet with Terminal, Diff, History and PR tabs (the Azure DevOps review when
  the repo lives there, else the GitHub link). `[`/`]` walk the sessions, `Esc`
  closes.
- **Open on** (Settings, Desktop tab) starts the window on the orchestrator;
  `--view orchestrator` does it for one run.

## Renewal

An orchestrator's context fills up. At 60 percent of its budget it writes a
handover note into its workspace (what is in flight, what is next, what waits
on you) and runs `kete orchestrator renew --from <note>`. You can ask for the
same from your own shell, the TUI (**Renew orchestrator** on the
orchestrator's session menu, which asks for the note path) or the desktop
(**Renew orchestrator…** on its session menu, a form with the note, an
account, **Keep old session** and **Renew now**).

```sh
kete orchestrator renew --from <note> [--profile P | --pick] [--model M] [--keep-old] [--now] [--json]
```

1. **The checks come first.** The note must be a file inside the
   orchestrator's workspace (`<data dir>/orchestrator`) or inside a registered
   project or one of its checkouts, UTF-8, 1 to 65,536 bytes. A bad note, no
   live orchestrator, or a renewal already waiting is refused before anything
   changes. The daemon answers **Scheduled**.
2. **It waits for the turn to end.** The swap happens once the old
   orchestrator's turn ends (its status reads finished), at most 300 s, or at
   once with `--now` or when it is not mid-turn. A turn that never ends fails
   the renewal and changes nothing.
3. **The swap.** A fresh orchestrator starts on the same project with the old
   one's harness, model and effort (or `--model`), role card and account (or
   `--profile`; `--pick` takes the account `kete profile pick` would name and
   holds it, and refuses with the reason when no account is eligible). It
   holds the role from its first spawn, so its system prompt carries the
   guidance, and its first prompt is:

   ```text
   [from kete renew] You are the orchestrator, renewed from <old name> at <HH:MM>. Your handover note is <path>; it is quoted below. Read it, then carry on.
   ```

   followed by the note.
4. **The new one must report.** Once its first prompt reaches Kete through
   its hooks, the old session is archived and stopped (`--keep-old` leaves it
   running as a worker) and the renewal is **Done**. If the new session sends
   no hook within 60 s, or exits first, it is stopped and archived and the old
   one holds the role again, untouched: it is never typed the guidance as a
   turn. The renewal is **Failed** with the reason.
5. **Every client hears.** The TUI's orchestrator view moves onto the new
   session and flashes *Orchestrator renewed from <note>* (or the failure);
   the desktop shows a notice with **Open note** (or the reason).

Each renewal ends in one line of `<data dir>/orchestrator-renewals.jsonl`:

```json
{"at":1792590537978,"old":"01M5FABXZNVAN04SVYT9CYNSA2","new":"01M5FABYC4MBSCNQ3XF98J4VSS","note":"C:\\Code\\game\\handover.md","state":"done","reason":null}
```

`at` is epoch milliseconds; `state` is `done` or `failed`; `new` is null when
no new session was started; `reason` says why a renewal failed.

On Windows a command line holds at most 32,767 characters and the note rides
the new CLI's command line beside the guidance, so a note well inside the
65,536-byte cap can still be too long there (a 40,000-byte note is). The
start fails and the old orchestrator stays in charge.

## Agent-facing commands

The orchestrator's guidance is injected by role on every cold start: the
harness's system-prompt flag where it has one (Claude's
`--append-system-prompt`), or the opening of the first prompt for a harness
without one (Codex, Cursor, ...). It is never handed a prewarmed session that
booted as a worker. The commands use the same CLI-over-socket pattern as
`kete spawn`, but the daemon refuses them unless the calling session is the
persisted orchestrator:

```sh
kete orchestrator list [--json]
kete orchestrator read <session> [--lines N] [--bytes N]
kete orchestrator send <session> <prompt> [--no-wait] [--timeout S]
kete orchestrator spawn --project <project> [--worktree <branch-or-id>] [--base <ref>]
                        [--kind K] [--name N] [--model M] [--effort E] [--profile P] <task>
kete orchestrator review <session>... [--tab terminal|diff|history|pr]...
kete orchestrator renew --from <note> [--profile P | --pick] [--model M] [--keep-old] [--now] [--json]
```

Sessions are named by id, `terminal:<id>`, or an unambiguous fragment of their
name, project or branch.

- **list** prints every live session in attention order with a normalized
  state (`needs_feedback`, `running`, `unread`, `idle`, `terminated`,
  `disconnected`), its project, branch, harness, parent session and open
  subagents. `--json` keeps the tree it always printed (`projects`,
  `worktrees`, `agents`, `terminals`, `links`) and adds `sessions`, the ordered
  rows.
- **read** is transcript-aware with the cleaned terminal tail as fallback;
  `--bytes` reads the raw bounded tail instead.
- **send** labels the turn `[from orchestrator]`, waits for a busy target to go
  idle (or refuses with `--no-wait`), and refuses the orchestrator itself.
- **spawn** starts a worker on a self-contained task, in a fresh worktree when
  `--worktree` names a branch that has none.
- **review** opens the review modal in every attached kete over all the named
  sessions.
- **renew** hands the orchestrator over to a fresh session that starts on a
  handover note, once this turn ends (see [Renewal](#renewal)). End the turn
  after running it.

The everyday `kete sessions`, `kete read` and `kete ask` work in the
orchestrator too.

### Azure DevOps

For a repo on Azure DevOps the guidance teaches `kete ado` (see
[Commands](commands.md#azure-devops)) and one flow, naming the repo with
`--project` because its own workspace has no remote: `kete ado --project P
tasks`, pick a task with the user, `kete ado --project P start-task <id>` (a branch and worktree for it,
the task linked, assigned and moved to In Progress, and a worker on it that is
the orchestrator's child, through `OrchestratorSpawnWith` on the new worktree),
watch the worker with `list` and `read`, `review` it for the user, then
`kete ado --project P --worktree <its branch> pr create` (draft, co-author lines cleaned,
a task and a reviewer unless waived; with `--project`, `--worktree` or `--source <branch>`
is required, and the default branch is never pushed). Votes and replies only when the user asks.
Sign-in is never interactive: a missing credential is reported with "sign in
from the desktop app or run `az login`", and the orchestrator passes that on
rather than trying to sign in.

## Permission model

The socket is same-user only, and that is the real boundary: any process
running as you can talk to the daemon. On top of it, orchestrator commands get
extra checks, because they can message other agents and open review UI in every
client. Every orchestrator request carries the caller's `NEBULA_AGENT_ID`, and
the daemon checks the caller exists, is not archived, and holds the
`orchestrator` role in its database. Prompt text is never trusted.

Granting the role is checked too:

- `CreateAgent` can no longer create an orchestrator; only `StartOrchestrator`
  and `SetOrchestrator` grant the role.
- Those requests, and `ClearOrchestrator`, are refused when the client process
  runs inside a live agent session (the daemon walks the process table from the
  client's pid up to a session's PTY child). That stops a worker's own shell
  from minting or taking the role by accident or on a prompt's say-so.
- The orchestrator verbs trust the caller id the client sends, so the same
  process check backs them: a verb whose client runs inside a *different*
  agent's session than the orchestrator it names (a worker that set
  `NEBULA_AGENT_ID` to the orchestrator's id) is refused.
- A renewal (`RenewOrchestrator`) is the orchestrator's own or a person's.
  Naming a caller, it passes the caller checks above and only the persisted,
  unarchived orchestrator may renew itself; a worker is refused. Naming none,
  it is refused from inside any agent session, like provisioning. Its note
  must lie in the orchestrator's workspace or a registered project (or one of
  its checkouts), so a renewal cannot quote an arbitrary file into the new
  orchestrator's first prompt.
- Provisioning is serialized, so two starts cannot both win, and a partial
  unique index on the role makes a second orchestrator row impossible from any
  path.

These checks are **defence in depth**, not a sealed boundary. Known gaps:

- Terminal tabs are exempt. A process inside a kete terminal tab counts as your
  own shell and passes. Anything with socket access can open a terminal tab
  and send it input, and a command run that way gets the same pass.
- A client the OS reports no pid for passes.
- A same-user process outside every session (your own shell, or anything else
  running as you) is trusted, as it could drive any session anyway.

The orchestrator does not approve permission prompts for other agents. Its loop
is tell-then-ask: report that a session is waiting and offer to open it in
review.

## Attention feed

The design is **poll-on-demand plus visible chips**, not injected prompts. The
daemon broadcasts status changes to every client; the strip updates from them at
once, and the orchestrator calls `kete orchestrator list` whenever it needs a
fresh read. Nothing is written into its chat box unasked.

## Follow-ups

- Azure DevOps pipelines in `kete ado`.

Shipped in 0.60: idle notes for an idle orchestrator, within the studio's
autonomy level ([Studio](studio.md), K2), and orchestrator
renewal ([Renewal](#renewal), K9).
