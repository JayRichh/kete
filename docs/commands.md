# Commands

<sub>[← README](../README.md) · [Keys](keys.md) · [Commands](commands.md) · [Sessions](sessions.md) · [Configuration](configuration.md) · [How it works](how-it-works.md)</sub>

The `kete` CLI. Every command carries its own help: `kete <command> --help` is the full page,
flags and examples included, and `-h` is the one-screen reminder. `kete --version` (short `-V`)
prints the version of the binary you're running (`kete 0.56.0`), the same version the TUI's
FOOTER carries at its left edge (with `⇡ vX.Y.Z` beside it once a newer release is published), and
what to check after `kete upgrade`. This page is the same surface in one place. Commands marked *(agents run this)* are the ones a coding agent invokes on
your behalf, see [How it works](how-it-works.md).

```
kete                      open the TUI (auto-starts the daemon)
kete desktop              open the desktop window: kanban, terminals, diffs
kete add <dir>            register a git checkout as a project
kete daemon               run the daemon that owns every session
kete kill                 shut the running daemon down (stops all sessions)
kete reload               move the daemon onto the installed binary, keeping every session
kete rename <title>       title the session this runs inside          (agents run this)
kete worktree [name]      move this session into a worktree           (agents run this)
kete spawn <task>         start another agent session beside it       (agents run this)
kete open <file>…         show files/images/diagrams in file tabs     (agents run this)
kete sessions             list every session across every project     (agents run this)
kete session move-account restart a session fresh on another account
kete read <session>       read recent context from another session    (agents run this)
kete ask <session> <q>    ask another session and print its answer    (agents run this)
kete orchestrator <cmd>   start the one orchestrator; its own verbs   (see Orchestrator)
kete ado <cmd>            Azure DevOps tasks, task worktrees, PRs     (see Azure DevOps)
kete studio <cmd>         studio mode: inbox, autonomy, queue, notes  (see Studio mode)
kete roles <cmd>          what each role card gets; bypass acceptance (see Studio mode)
kete usage                tokens, estimated cost and turns per session per day
kete config <cmd>         back up, restore or locate this machine's settings
kete profile <cmd>        several accounts per harness: add, sign in, rename, spread, remove
kete wake <cmd>           start accounts' 5-hour windows early at an hour you pick (Whakaoho)
kete editors <cmd>        list the VS Code and Visual Studio installs Kete finds; open a path in one
kete setup terminal       install a Nerd Font and add a "Kete" Windows Terminal profile
kete browser              serve this TUI in a web browser via ttyd
kete ssh <host>           open kete on a remote host over ssh
kete tunnel <host>        open a remote host's kete in a tab here
kete upgrade              install the latest published kete
```

## The TUI

```sh
kete                    # launch the TUI (auto-starts the daemon). With no project yet, a
                          # launch from inside a git repo offers it on the splash: Enter
                          # opens it; anywhere else, `o` browses for one
kete desktop            # the same sessions in a native window (kete-desktop, built from
                          # desktop/; looked for beside kete, then on PATH). After `--`:
                          # --view board|session|diff, --session <name>, --theme light|dark
```

## Projects and the daemon

```sh
kete add <dir>          # add a repo as a project, named after its root directory
kete add .              # same, for the repo you're in (bare `kete <dir>` / `kete .` also work),
                          # but a directory whose name collides with a subcommand needs the long
                          # form (`kete add browser`) or a `./` prefix, or bare `kete browser`
                          # serves the TUI over ttyd instead of adding the directory
                          # If an existing project's old folder has gone missing, use the TUI's
                          # Locate project prompt on that project to relink it; `kete add` still
                          # registers the path as a project and does not guess which stranded row
                          # should move.
kete daemon             # run the daemon (normally auto-spawned)
kete daemon --foreground  # daemon with logs to stdout, for debugging
kete kill               # stop the daemon and all sessions cleanly
kete reload             # restart the daemon in place onto the binary this runs from: every
                          # session keeps its process, scrollback and status, and agents' hooks
                          # keep reaching it. Open TUIs drop their connection: relaunch `kete`
                          # to pick the sessions back up. The cut-over after `make install`; a
                          # daemon from before reload existed can only be restarted by `kill`
```

## What agents run for you

```sh
kete rename <title>     # title the current session (agents run this; --force to retitle)
kete worktree [name] [--base <ref>]  # move the current session into a worktree of its project,
                          # creating the branch if it's new (agents run this when you ask for a
                          # worktree; no name invents one; --base picks a new branch's start point,
                          # a branch name meaning origin's fetched copy (main is origin/main)
                          # but is ignored when the branch already exists; without it the
                          # worktree_base_branch setting applies, else origin's default)
kete spawn <task> [--kind <claude|codex|cursor|pi|muse|grok|opencode|copilot>] [--worktree <branch> [--base <ref>]]
           [--profile <name>] [--role <id>]
                          # start a new agent session beside the current one, in the same worktree,
                          # opening on <task> (agents run this when you ask for a new kete session;
                          # --kind defaults to this session's harness; custom harnesses launch from
                          # the TUI picker and presets, not --kind); --worktree starts it in the
                          # project's worktree on <branch> instead, creating it when the branch has
                          # none (--base picks a new branch's start point and is refused when the
                          # branch already exists), and leaves this session where it is;
                          # --profile runs it on that account (one for its harness; default:
                          # this session's account on this harness, else the harness's default;
                          # needs daemon protocol 58, an older daemon is asked to update);
                          # --role launches it with a role card from the repo's
                          # .kete/roles.toml: its model, tools, prompt and permissions (needs
                          # daemon protocol 66; see Studio mode)
kete open <file>…       # show the files in this kete's FILE TABS, a modal with one tab per
                          # file, the focused one previewed, Enter editing it (agents run this only
                          # when you ask to see a file, image, mockup or diagram; text files,
                          # supported local images and Mermaid diagrams render inline, other
                          # binaries are refused and the agent names the path instead)
kete sessions [--json]  # list every session across every project: id, name, project, worktree
                          # branch/path, harness, status and one-line recent summary. Agents run
                          # this when you name another session and they need an exact id.
kete read <session> [--lines N]
                          # print clean recent context from another session, resolving by id or
                          # unambiguous fuzzy name. Claude transcript text is preferred; otherwise
                          # the daemon strips ANSI from bounded PTY scrollback.
kete ask <session> "<question>" [--timeout S] [--no-wait]
                          # send a labelled question into another local agent, wait for that turn
                          # to finish, and print the answer. A busy target is queued until idle by
                          # default; --no-wait fails fast instead.
kete orchestrator start [--kind K] [--model M] [--effort E] [--profile P] [--project X]
                        [--name N] [--replace] [--role R]
                          # start the machine's one orchestrator (F10 in kete does the same),
                          # in its own workspace under the data dir unless --project names one.
                          # --role launches it with a role card from that project's
                          # .kete/roles.toml (needs daemon protocol 66).
kete orchestrator status [--json]
                          # which session is the orchestrator, where it runs, what it is doing.
kete orchestrator clear   # unpin it; the session keeps running as a worker.
kete orchestrator list|read|send|spawn|review
                          # the orchestrator session's own verbs, refused for every other caller.
                          # See docs/orchestrator.md.
kete orchestrator spawn --project P [--worktree W] [--base R] [--kind K] [--name N] [--model M]
                        [--effort E] [--profile P] [--role R] <task>
                          # --role launches the worker with a role card from the project's
                          # .kete/roles.toml (needs daemon protocol 66). In a repo with studio
                          # mode on, autonomy A0 refuses the spawn.
kete orchestrator renew --from <note> [--profile P | --pick] [--model M] [--keep-old] [--now] [--json]
                          # hand the orchestrator over to a fresh session. The new one starts on
                          # the handover <note> (a file in the orchestrator's workspace or a
                          # registered project) as its first prompt once the current turn ends,
                          # takes the role, and the old one is archived. --profile or --pick
                          # (kete's pick, as `kete profile pick`) chooses the account, --model
                          # the model; both default to the old session's. --keep-old leaves the
                          # old session running as a worker. --now swaps at once. Run by the
                          # orchestrator itself, or by you from your own shell.
```

## Azure DevOps

Run in a checkout whose git remote is on Azure DevOps: org, project and repo
come from that remote, the team from the desktop's Tasks view (else the
project's default team). Sign-in is never interactive: Git Credential Manager
with `credential.interactive=never`, then `az account get-access-token`. When
neither has a token the command says to sign in from the desktop app or run
`az login`.

From anywhere else, name the repo: `--project <kete project>` (its name or
id) goes on any `kete ado` command, before or after the verb, and acts in that
project's main checkout; add `--worktree <branch>` for one of its worktrees
(`pr create` pushes that checkout's branch, so with `--project` it needs
`--worktree` or `--source`; a source that is the target or the repo's default
branch is refused before anything is cleaned or pushed). The orchestrator, whose own
workspace has no remote, always passes it.

```sh
kete ado --project ledger-api tasks
kete ado --project ledger-api --worktree jari/gst-rounding pr create --task 4211
kete ado tasks [--all | --open] [--json]
                          # your tasks in the current sprint, by parent (--all: everyone's
                          # open items in it; --open: everything open assigned to you)
kete ado start-task <id> [--agent H] [--no-agent] [--branch B] [--base R]
                         [--no-assign] [--no-activate] [--prompt P] [--model M] [--effort E]
                         [--profile P] [--json]
                          # the desktop's Start task: <initials>/<slug> branch from the
                          # repo's default branch, a worktree, the task linked, assigned and
                          # In Progress, an agent on it (the orchestrator's child when run there)
kete ado prs [--json]     # your active PRs, and those waiting on your review
kete ado pr <id> [--json] # one PR: reviewers and votes, policies, comment threads
kete ado pr create [--title T] [--summary S] [--testing T] [--target B] [--source B]
                   [--task <id> | --no-task] [--reviewer WHO]... [--no-reviewer] [--publish]
                          # push this branch and open a draft PR with the composer's rules:
                          # co-author lines cleaned from unpushed commits, a task and a
                          # reviewer unless waived, an existing PR reported, not doubled
kete ado vote <id> approve|approve-with-suggestions|wait|reject|reset
kete ado reply <id> <thread> <text>
```

## Shipping a worktree

`kete ship` takes a worktree's branch to a pull request and on to merged, on
whichever forge its remote is: GitHub through `gh`, Azure DevOps through its
API. Sign-in is never interactive. A missing one says to run `gh auth login`
(or `az login`, or sign in from the desktop app). It runs in the checkout you
are in. `--project <kete project>` acts in that project's main checkout from
anywhere, and `--worktree <branch>` picks one of its worktrees (without
`--project`, one of the current checkout's project).

```sh
kete ship status [--json]  # forge and remote, branch and base, unpushed commits, the branch's PR
                           # (open first, else its newest); a sign-in it needs is said, not asked
kete ship pr [--title T] [--body B | --body -] [--draft] [--base B] [--json]
                           # clean co-author lines from unpushed commits, push with upstream,
                           # open the PR into the review base (worktree_base_branch, else the
                           # default branch); no --title: suggested from the session's name or
                           # prompt and the commits since the base; an open PR is updated by the
                           # push, never opened twice
kete ship merge [--method squash|merge|rebase] [--delete-branch] [--json]
                           # merge the open PR (GitHub pinned to the head it read; Azure DevOps
                           # completed through its API, or the PR page when its policies hold it);
                           # --delete-branch deletes it on the remote only, the worktree stays
kete ship --project ledger-api --worktree jr/fix-gst pr --draft
```

## Settings

```sh
kete config path              # where config.json, config.local.json, agent_presets.json and
                                # ssh_hosts.json live
kete config export [path]     # one JSON file of this machine's settings: stdout, a file, or
                                # nebula-settings.json inside a folder. Never config.local.json
kete config import <source>   # merge a backup in: an export, a bare config.json /
                                # agent_presets.json / ssh_hosts.json, a folder holding any of
                                # them, or - for stdin. Keys it sets replace this machine's, keys
                                # it lacks stay, and config.local.json is never written
kete config harnesses         # print the effective harness registry: every harness with
                                # the program, flags, resume shape, hook dialect and defaults a
                                # launch uses. Copy a row into config.json `harnesses` to override it
```

See [Configuration](configuration.md#backup-restore-and-other-machines).

## Profiles

```sh
kete profile add <name> --harness <h> [--use-default] [--default] [--env K=V]…
                                # a named account for one harness; --use-default adopts the
                                # CLI's existing login instead of a home of its own
kete profile login <name> [-- <args>]
                                # run the harness's sign-in here under the profile:
                                # claude auth login --claudeai, codex login, copilot login
kete profile list [--json]    # accounts: 5h and weekly usage bars, defaults, login state;
                                # --json adds each account's windows, live sessions and holds,
                                # and whether it is reserved
kete profile pick [--harness H] [--project P] [--dry-run] [--json]
                                # the account a scripted launch should take, and why: prints its
                                # name (--json: the name, the reason and every candidate).
                                # Reserved and switched-off accounts are never candidates, and
                                # the spread `manual` counts as `least_used`. It holds the
                                # account for 10 minutes so the next pick spreads;
                                # --dry-run shows the candidates and holds nothing. --harness
                                # defaults to claude. Exits 3 when no account is eligible
kete profile reserve <name>   # keep it for your own sessions: still the default for launches
                                # you start, never picked for ones the orchestrator or a script starts
kete profile unreserve <name> # let scripts pick it again
kete profile default <name>   # launches that pick no profile take it
kete profile enable <name>    # pickable again by launches that name no profile
kete profile disable <name>   # never picked unless named
kete profile remove <name> [--purge]
                                # drop it; --purge deletes its home (and the login) too
kete profile duplicate <src> <new>
                                # a copy: same harness, variables and kind of login, its own home
kete profile rename <old> <new>
                                # its home, project defaults and usage reading follow; refused
                                # while a session (running or stopped) runs on it
kete profile spread <manual|failover|least-used> [--threshold N]
                                # the account a launch naming none runs on; N is 50, 70, 80, 90, 95
kete session move-account <session> [<profile> | --own-login]
                                # restart a session fresh on another account of its harness
```

See [Profiles](profiles.md).

## Studio mode

A repo opts in with `.kete/studio.toml` and you switch it on per machine. `<project>` is a project's name or id. Only you change the autonomy level or the studio settings: the daemon refuses `enable`, `disable`, `autonomy`, `exclude` and `include` from inside an agent session. See [Studio mode](studio.md).

```sh
kete studio enable <project> [--inbox DIR]
                                # switch studio mode on for the repo at autonomy A1 (Plan); the
                                # inbox is the manifest's `inbox` unless --inbox (relative to
                                # the repo root) names one
kete studio disable <project> # switch it off; the repo's entry is removed
kete studio status [--json]   # every studio repo: autonomy, inbox, open notes and notes for
                                # you, the next notes, why the runner is holding, the last idle note
kete studio autonomy <project> [A0|A1|A2|A3]
                                # print the level, or set it: A0 Hold, A1 Plan (default),
                                # A2 Deliver, A3 Release
kete studio exclude <project> <id-or-about>
                                # leave a note (by id) or every note with that `about` out of the queue
kete studio include <project> <id-or-about>
                                # hand it back
kete studio queue [--project P] [--now] [--json]
                                # what the queue runner would send the orchestrator now;
                                # --now sends it if every condition for an idle note holds
kete studio note <text>… [--project P] [--to R] [--about A] [--priority high|normal|low]
                         [--triage] [--json]
                                # file one inbox note; the id is the date and the first words.
                                # --to is a recipient, a department slug or jay (default: triage),
                                # --triage hands the note to the triage desk to route.
                                # Exits 3 when an open note already says the same
```

```sh
kete roles check [--project P] [--role R] [--json]
                                # what each role in the repo's committed .kete/roles.toml gets:
                                # the flags and the settings file, as a launch would apply them
                                # (--project defaults to the project you are in)
kete roles accept-bypass <profile>
                                # run Claude in bypass mode under that account, here, so you answer
                                # its one-time dialog; roles marked `unattended` run only on
                                # accounts that did. Refused inside a kete session
kete usage [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--project P] [--by session|day|role] [--csv]
                                # the usage ledger: tokens, estimated cost, tool uses and turns,
                                # one row per session per local day. Today by session unless told
                                # otherwise; both dates are inclusive; --csv prints every row as CSV
```

`--role <id>` is also a flag of `kete spawn`, `kete orchestrator spawn` and `kete orchestrator start`, and `kete orchestrator renew` hands the orchestrator over (see [What agents run for you](#what-agents-run-for-you)). More: [Studio queue](studio-queue.md), [Roles](roles.md), [Usage ledger](usage-ledger.md), [Triage](triage.md).

## Wake-up (Whakaoho)

At the hour you pick, Kete sends one tiny prompt ("hi", to haiku) under an account, so its five-hour window starts then and resets earlier in your day: an anchor of 05:00 gives a window to 10:00, and you start at 09:00 with a fresh one an hour away. It uses your own subscription, is off by default, and is set per account (`@claude` is Claude's own login; any other name is a profile).

```sh
kete wake set <account> --at HH:MM [--days all|mon-fri|weekends|mon,wed,…]
              [--keep-rolling] [--skip-idle N] [--mode auto|interactive|print] [--wake-pc]
                                # switch it on; options left out keep their values.
                                # --at suggested takes the hour that fits your usual day
kete wake off <account>       # switch it off; the hour and options are kept
kete wake status [--json]     # schedules, next wake-up, live window, usual hours,
                                # the suggested hour, and the last decision
kete wake <account> --dry-run # what would happen now; runs nothing
kete wake <account> [--force] [--mode auto|interactive|print]
                                # run it now if the rules allow (--force: whatever they say)
kete wake --all [--dry-run]   # every switched-on account: what the scheduled task runs
```

A heartbeat goes out only when the switch is on, it's one of its days and the hour has come, the account is switched on and logged in, it has no live five-hour window, and that hour hasn't had one. If the PC slept through the hour, it still goes out on waking, but only while it's earlier than your usual start (an hour's grace while Kete doesn't know your usual hours yet). `--keep-rolling` also starts the next window at each 5-hour step inside your usual hours, so resets stay on the anchor's grid. `--skip-idle N` leaves an account alone after N days without use.

- **Auto** (the default) runs interactive, and falls back to print when the daemon can't run it (one older than this kete, until `kete kill` lets it restart, or none to reach); the log says `fired (print fallback: old_daemon)`. Naming `--mode interactive` pins it: no fallback.
- **Interactive** runs a hidden Claude session in the daemon, in Kete's empty `wake` folder under the data dir: never listed, killed when the reply lands or after two minutes. Kete's status line reads the new reset time off it.
- **Print** runs `claude -p` instead. It needs no daemon, but it may bill your plan's API credits rather than start the window.

On Windows, `kete wake set` and `off` keep one Task Scheduler task, `Kete\Whakaoho`, that runs `kete wake --all` at every switched-on hour (Kete open or not; `--wake-pc` lets it wake the PC). The task runs `kete-w.exe wake --all`: kete with no console window, installed beside `kete.exe`, so a wake-up never opens a window or takes the focus (without it, from an older install, each run shows a console and `kete wake status` says so). It is removed when no account is on. On macOS and Linux, `kete wake set` prints the crontab lines to add instead. Schedules live in this machine's `config.local.json` under `wake` (never exported or sent to remotes), and every decision is a line in `<data dir>/wake-log.jsonl`, with the reset time before and after, so a heartbeat that didn't start a window shows as one.

## Editors

The same VS Code and Visual Studio the menus' Open in rows use: VS Code and its forks (Insiders, VSCodium, Cursor, Windsurf…) found by their own install, and Visual Studio through its installer (Community, Professional and Enterprise alike, previews too). Each opens directly and detached, without Kete's own environment.

```sh
kete editors list [--json]             # what is installed, the default of each kind first
kete editors open .                    # this checkout in VS Code (a window already on it is focused)
kete editors open src/main.rs --line 42
kete editors open . --with vs          # its solution or project in Visual Studio
kete editors open Api.sln              # a .sln, .slnx, .csproj or .vcxproj opens in Visual Studio
kete editors open . --with cursor      # an install by its id from the list
```

## Windows Terminal

```sh
kete setup terminal           # install JetBrainsMono Nerd Font for this user (from the latest
                              # nerd-fonts release, into %LOCALAPPDATA%\Microsoft\Windows\Fonts,
                              # registered under HKCU) and add or update a "Kete" profile in
                              # Windows Terminal's settings.json: this kete.exe, Kete's icon, the
                              # font, a "One Dark" color scheme (one already there is kept)
kete setup terminal --dry-run # say all of it and print the diff of settings.json; do nothing
kete setup terminal --no-font # only the profile
```

`settings.json` is backed up beside itself (`settings.json.kete-backup-<seconds>`) before it is
written, and the profile and scheme are spliced in: its comments, key order and layout stay. A second
run finds them and writes nothing. A "Bro" profile made by `bro setup terminal` is the same profile
(Kete keeps its GUID), so it is renamed and pointed at `kete` rather than joined by a second one. On macOS and Linux it says how to set up a Nerd Font by hand. The
font is what `nerd` [file icons](configuration.md) need.

## Other machines, other screens

```sh
kete ssh <host> [dir]   # open kete on a remote machine over ssh (installs it there if
                          # missing); destinations are remembered for the TUI's HOSTS PICKER
                          # (`Shift+H`). Needs the OpenSSH client (`ssh`) on PATH here. This
                          # machine's config.json and agent presets ride along, and the remote
                          # kete merges them into its own settings (its config.local.json
                          # still wins); --no-sync-config or the ssh_sync_config setting leaves
                          # them here
kete tunnel <host> [dir] [--port N] [--remote-port N]
                          # that host's kete in a browser tab here, over one ssh tunnel: installs
                          # kete there if missing, runs `kete browser` on its loopback, forwards
                          # the port, and opens the local URL. Nothing is exposed on the remote's
                          # network (the tunnel is the only way in) so it needs no --credential.
                          # If that host already has a `kete browser` on the port, the tunnel
                          # reuses it instead of failing on the clash (a --credential one will ask
                          # for it in the tab).
                          # Needs the OpenSSH client (`ssh`) on PATH here and ttyd on the remote;
                          # Ctrl+C takes both ends down. --port is the local end (same rules as
                          # `kete browser`), --remote-port the far end when something there
                          # already holds that number. Settings ride along as they do for
                          # `kete ssh` (--no-sync-config leaves them here)
kete browser [--port N] [--bind ADDR | --public] [--credential USER:PASSWORD] [--no-open]
                          # serve this TUI in a browser tab via ttyd and open it; needs ttyd on
                          # PATH. With no --port it takes 7681 when that's free and a free port
                          # otherwise, saying which, so one per checkout can serve at once.
                          # --port 0 always picks a free one; --port N is that port or an error,
                          # which is what you want behind an ssh tunnel. Listens on 127.0.0.1
                          # unless --bind names an interface address or --public takes them all
                          # (0.0.0.0), for a kete on a remote box, where the access control
                          # is the firewall/security group in front of the port. That serves a
                          # live, writable terminal, so put something in front of it and use
                          # --credential to add ttyd's HTTP basic auth on top. --no-open serves
                          # without launching a desktop browser, for a box that has none
kete upgrade            # install the latest release (--force on a dev build). Swapping the
                          # binary doesn't touch a running daemon, so afterwards it shuts an idle
                          # one (no live sessions) down for you and the next launch starts on the
                          # new binary. With sessions live it restarts the daemon in place onto
                          # the new binary (what `kete reload` does), and they keep running.
                          # A daemon from before in-place restarts is left up (its sessions
                          # would die with it) and it says to run `kete kill` when you're
                          # ready. When the new build speaks another protocol, and so can't
                          # attach to that daemon, it says that too and offers the restart then
                          # and there
```
