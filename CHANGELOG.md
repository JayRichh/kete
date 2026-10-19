# Changelog

## 0.57.0

Teammates and checkouts show up on their own, and the usage meter tells the truth.

- **Subagents of a background job appear.** A job started from the terminal kept posting its hooks to a daemon port that no longer existed after a restart, so its subagents never reached the board. The daemon now writes its hook endpoint to a file and the hooks fall back to it, so SubagentStart, status and statusLine events arrive again.
- **Repos and worktrees register themselves.** A checkout that is a linked worktree of a registered repo, or holds one, is added to the repo list on the stamp tick (about every 10 seconds). Remove a repo and it stays removed.
- **Teammates on the kanban.** The orchestrator session has a teammate strip on the board with each subagent's label, model and state, and the board no longer shows its empty state when only the orchestrator exists. Recent checkouts include linked checkouts with no session yet.
- **Subagent pane under the chat (desktop).** Total, running and done counts in the header, a scrolling list, finished rows folded into "N finished", a draggable splitter with a remembered height, and Collapse chat.
- **Usage meter (desktop).** The 5-hour and weekly meters read the account's own usage from Anthropic every 60 seconds per profile and at once on window focus, with the account email beside them. The meter stuck at 7 percent is gone.
- **Upgrade note.** No protocol change. Run `kete kill` and reopen so the daemon starts on this build.

## 0.56.0

Sessions you start from a session, what each subagent runs on, and git state that keeps up.

- **Follow-up as its own session.** In the follow-up box, Start as new session (`Ctrl+Shift+Enter` in the desktop, `Ctrl+N` in the terminal app) starts what you typed as a new session in the same checkout. It nests under the session it came from, and the app opens it.
- **Ask aside.** A session's menu has Ask aside: a side question goes to a new session beside it, with its prompt asking it not to change files. It runs with the same permissions as the session it came from. The question gets its own session, nested under the one it is about, and the main session's turn is left alone.
- **Nested counts.** A session with nested sessions or subagents says how many are working, need you or are done. Finished subagents folded into their `N finished` row count as done.
- **Subagent model and effort.** Subagent rows on the overview, the board and the terminal app's tree and cards show the model and effort a subagent runs at, when Claude reports them (`Explore · haiku · low`).
- **Git state within seconds (desktop).** The desktop notices a commit, checkout, reset, stage or fetch in any shown checkout within 2 seconds, without running git to find out. It re-reads a checkout with a working session every 6 seconds. A row whose git state did not change keeps its state.
- **Session details (desktop).** The session view's Details rail shows where the session runs (its checkout, or its Claude Cloud page), context use, changed files in its checkout, model and effort, and how the sessions and subagents it started stand.
- **Moving a session says what stays behind.** Before a session moves to another checkout, both apps count the uncommitted files and unpushed commits that stay in the old one, and ask first. A clean checkout moves as before.
- **Recent checkouts on the quick prompt.** The quick prompt offers the five checkouts sessions last ran in. Click one in the desktop. In the terminal app, `^R` lists them and `Alt+1` to `Alt+5` aim the launch.
- **Upgrade note.** The protocol moved from 63 to 64. Run `kete kill` and reopen.

## 0.55.0

Subagents show under their session in both apps, and finished ones tidy themselves away.

- **Subagents under their session.** The desktop board and the terminal app's cards and overview list every subagent beneath its session. Finished ones fold into one "N finished" row, with Clear finished beside it. Turn the fold off with `hide_done_subagents` (Settings › Sessions › Subagents).
- **Finished subagents age out.** The daemon drops an ended subagent after an hour by default. `subagent_retention` takes `10m`, `1h`, `4h`, `1d` or `off`.
- **Subagent status is truer.** A subagent counts as waiting only while a background task it launched is still running, so finished ones are no longer shown as waiting. A lost stop no longer leaves a row running for the life of its session, and killing or restarting a session closes its old subagents without touching the new process's.
- **Upgrade note.** The protocol moved from 62 to 63. Run `kete kill` and reopen.

## 0.54.0

Editors, menus, appearance, Files, wake-up scheduling, usage limits and critters ship together in this release.

- **Open in your own editor.** VS Code installations each have their own menu row; Visual Studio appears for checkouts with a solution or project. Open files and folders, choose the default installation, show a checkout in Explorer, or dock an editor beside Kete. The apps launch installed editors as separate windows.
- **Context menus throughout both apps.** Right-click and the verbs menu follow the selected session, worktree, project, account, file or folder. Open in, Copy, Pull request, Move and View submenus group related actions. State determines what is available, and the desktop explains disabled actions. Menus support filtering, keyboard navigation and focus return.
- **The desktop's browser menu is gone.** Fields, terminals and code viewers have Kete's own editing menus. Installed builds disable browser print and refresh accelerators while preserving terminal and text editing keys.
- **Files beside Diff.** Browse tracked and untracked, non-ignored files in a collapsible tree, filter paths, open files in Kete or an installed editor, and drag paths onto sessions or terminals without submitting them. File menus include diff, stage, unstage and discard actions where applicable.
- **Appearance shared with your tools.** Workshop, One Dark, One Light, live VS Code and imported VS Code palettes are available. Pick interface, code and terminal fonts and sizes, or reset appearance. The terminal app shares the VS Code theme loader. Settings keeps Appearance beside Editors in the Desktop tab.
- **Whakaoho.** An optional per-account wake-up schedule, timeline and usage bars help plan Claude usage windows. Choose an hour, preview windows, stagger accounts and inspect the decision log. Windows Task Scheduler can run the hidden launcher. Scheduling is off by default; an actual wake uses the selected account's subscription. Restart the daemon after upgrading to use hidden interactive wake sessions.
- **Usage limits and effort.** Cards distinguish a running session waiting for its usage window, a session needing sign-in or another answer, and a finished turn with an error. Effort changes follow the session. Background work is checked against Claude's reported tasks, and Windows hooks invoke Kete directly.
- **Critters in both apps.** Optional pixel visitors wander, meet, react to sessions and keep a local collection log. Pet, feed, name, drag, throw or shoo them from their own menu. Discover them in Settings, through the Critters action, the Konami code, or five clicks on the desktop mark. Reduced motion is respected.
- **Navigation and fixes.** Accounts and scheduling are consolidated without dropping their actions. The command palette runs the selected action again. Theme contrast, narrow settings layouts, menu scrolling and pixel scaling under interface zoom are corrected. Returning to the board refreshes commit counts immediately, even when a turn finished while it was away. The 0.53.1 Windows paste fix is included.

## 0.53.1

- **Pastes on Windows no longer send early.** When the console paused partway through a large paste, as it can on a busy or slow machine, the closing line break could arrive on its own after the pause. The terminal app then treated it as a typed Enter, so the agent got the paste up to that point and the rest followed as a new message. For a second after a large paste, keys that arrive pressed and released together now count as part of the paste. An Enter you type yourself still sends.

## 0.53.0

No new features. Upgrading from 0.47/0.48 is tested on a real old data folder, the desktop has end-to-end tests, and the bugs these tests found are fixed.

- **Upgrading from 0.47 or 0.48.** A data folder made by 0.48 now opens in 0.53 with your projects, worktrees, sessions, order, accounts, rebound keys and review marks intact. Fixes from that testing:
  - The What's new page shows once per release instead of again on every later release.
  - Files you marked reviewed in 0.48 stay marked in the diff against the base, as long as their diff hasn't changed. Unmarking one sticks.
  - The terminal app says when it is talking to an older, still-running daemon, and what to run. `install.ps1` replaces the files but not the running daemon, and `kete reload` doesn't work on Windows.
- **Committed Claude settings.** Before 0.52, an agent's `git add -A` could commit `.claude/settings.local.json`, the file Kete writes for Claude's hooks, which holds local paths. Once per checkout, the terminal app now names each one that tracks it and gives the command to untrack it (`git rm --cached .claude/settings.local.json`, then commit). It never untracks the file itself.
- **Desktop: a finished session's commits show at once.** The window re-reads a worktree's git state when a session there changes status. Before, it re-read every 30 seconds, so for up to 30 seconds a new Review card showed no commits and Ship refused it.
- **Tested end to end on both apps.**
  - The desktop now has tests that drive the real window against a real daemon. They cover the board, drag to Ship and to Archived, the review split, the scope chip, rebinding a key, and the `?` menu, and they run in CI.
  - The Windows terminal-app tests now also cover the 0.48 upgrade and check that every line an agent prints reaches the pane.
- **More reliable CI.** Three tests that failed under load are fixed without being disabled. One of them was a real race: a test changed the process-wide PATH while other tests started git.

## 0.52.0

The new board, review and keys are now taught in the app, attribution is exact per turn, and the flows are tested end to end on Windows.

- **Tips the first time.** A one-line tip appears the first time you meet the `?` verbs menu, the scope chip, the Review and Ship lanes, drag on the board, or the docked review. It names your own keys. Choose "Got it" or "Don't show tips"; Settings → General → Show tips turns them back on.
- **What's new.** After an upgrade, a page summarises 0.49 to 0.52, including the keys that changed. It shows once and stays in Help (`w` in the terminal app's Help, a button in the desktop's) and the palette.
- **Empty screens say what to do.** Every empty lane, worktree, repo and search result says what goes there and offers its one action with your key.
- **Less to read.** Footers show at most seven items: `? more`, how to move, and the selection's top verbs. The desktop's Help (`F1`) groups keys by task (Navigate, Act on a session, Review & ship, Window), with rare ones behind Show all.
- **"This session" is exact per turn.** Kete snapshots each turn's start and end. Other sessions' work between a session's turns drops out of its diff, even in a file both touched. It still counts edits another session makes during this session's turn, and it shows more rather than less when git can't merge a gap. Snapshot refs now live under `refs/kete/d/<daemon id>/` and are swept at startup, when a project is added or removed, and daily (migration 35).
- **`kete ship`** uses the worktree's recorded base, as the apps do.
- **Fixes:**
  - A path pasted or typed into Open project on Windows replaces the `~/` prefill instead of being appended to it.
  - Claude's `.claude/settings.local.json`, which Kete writes for its hooks, is excluded from git, so an agent's `git add -A` no longer commits it.
- **Tested end to end on Windows.** A new suite drives the real `kete` binaries through ConPTY with a scripted agent:
  - start, Running, Needs you, reply, Review with its commit;
  - `?` verbs, scope, and the diff against base;
  - the PR form's fix for a missing `gh`;
  - archive and the worktree delete, by keys and by drag;
  - a 20 KB paste arriving as one paste.

  It runs in CI.
- **Pastes on Windows:** the console layer converts input at about 23 KB/s. A 110 KB paste reaches the agent in about 4–5 seconds, and Kete itself no longer adds to that.

## 0.51.0

See which session made a change, paste large text quickly on Windows, and do the rest of the command line's jobs from either app.

- **Which session made which change.** Kete snapshots a session's worktree when the session starts and at the end of each turn, without touching your index or files, and keeps the snapshots under `refs/kete/sessions/`. In the review, "This session / All in this worktree" switches the diff (`Ctrl+l` in the terminal app). "This session" means the changes between the session's start and its latest turn, so another session's edits made during its turn are counted too. Needs a 0.51 daemon (protocol 59).
- **A worktree remembers its base.** Kete records the branch and commit a worktree was cut from. The review diff and the Review/Ship lanes use the recorded base, even after the base branch moves on or the setting changes. Older worktrees still work it out as before.
- **Large pastes on Windows arrive quickly.** The terminal app now reads console input in bulk. A 110 KB paste reaches the agent in about 3 seconds instead of about 15, and the status bar shows "pasting… N KB" meanwhile. Characters not on your keyboard (`—`, curly quotes) no longer drop out of pastes.
- **Command-line jobs, now in the apps:**
  - Vote on and reply to Azure DevOps PRs in the terminal app (`⇧B`, then `v` or `c`).
  - Start the orchestrator with a model, effort, project and name in both apps.
  - "Delete merged & clean worktrees…" in both apps' repo menus.
  - Export and import settings from Settings in both apps (`⇧E` / `⇧I` in the terminal app); import shows what will change first. New: `kete config import --dry-run`.
  - Rename an account that sessions still use; the sessions follow the new name (protocol 60). A running session keeps its old sign-in folder until it restarts.
- **Narrow windows and small terminals.** The desktop's quick prompt, breadcrumb, header, review bar, diff toolbar, verb bar, worktree rows, Accounts and Settings no longer clip or scroll sideways at 640–900px. The terminal app's status bar drops a hint whole instead of cutting it mid-word. Below about 150 columns, the docked review takes the whole board.
- **Desktop:** Create pull request is greyed on the base branch, with the reason.

## 0.50.0

The board now follows the work from start to merge: scope it to what you're working on, review an agent's changes beside the board, and open or merge the PR without leaving either app.

- **Board lanes follow the work.** Running → Needs you → Review → Ship → Done.
  - **Review** holds sessions that finished unread, or that left changes no PR carries yet: uncommitted work or commits ahead of the base.
  - **Ship** holds sessions with an open or draft PR.
  - **Done** holds the rest. Cards show "N commits" and the PR state.
  - Both apps use the same rules, computed once in the shared core.
- **Drops do what the lane means.** Review → Ship opens the PR form. Ship → Done merges after a confirm (squash, merge or rebase, with an option to delete the remote branch). Review → Done marks the card seen. Done → Archived archives it and offers the safe worktree delete (linked worktrees only, when it held the last session). A drop that can't happen dims the lane and shows the reason while you're still dragging; on the base branch it says so instead of offering a PR.
- **Scope chip.** `all ▸ repo ▸ worktree @account` sits in both apps' headers. The board, the attention strip and next-attention keys, the overview, the quick prompt's target and the diff's default worktree all follow it. Set it with a rail repo or a digit, or with "Focus here" on a repo, worktree or card. Each lane foot says "N more outside scope", and ✕ (or Esc in the terminal app) clears it. In the terminal app `Tab` still flips between this project and every project.
- **Review beside the board.** Open a Review or Ship card to dock its worktree's diff against the base next to the board. A row above the diff has Reply (`Space`), Open or Create pull request (`⌥P`), Merge (`⌥M`), Archive (`⌥A`) and Delete worktree (`⌥W`), the same in both apps. Closing it returns you to the same card.
- **Pull requests from the app.** The PR form starts from the session's name, prompt and commits since the base, for GitHub (via `gh`) or Azure DevOps. If something fails it shows the fix as a button: sign in, install `gh`, or open the PR in ADO when a policy blocks the merge. The same verbs are on `?`, the menus and the palette. New CLI: `kete ship status|pr|merge`.
- **No credential pop-ups.** Kete's own pushes and fetches never open a Git Credential Manager window or an ssh passphrase prompt. A sign-in failure shows as "sign in" with a button.
- **`kete spawn --profile <name>`** starts a sibling session on another account. It needs a 0.50 daemon (protocol 58); older clients and daemons still connect.

## 0.49.0

Both apps now share one list of verbs and one key table. You learn by pointing at a thing instead of memorising keys, and the diff keeps an agent's committed work.

- **`?` shows what you can do here.** Press `?` on any card, worktree, repo, board card, diff file or account to get its verbs with their keys. Type to filter, then Enter to run. The last row opens every key. Full help is on `F1`. Both command palettes (`Ctrl+K`) list the selection's verbs first.
- **A verb bar and footer that follow the selection.** The desktop has a slim bar under the overview, board and worktrees views with the focused row's top verbs as buttons. The terminal app's footer leads with `? more`, then the selection's top four verbs. Every menu row in the terminal app now shows its key.
- **One list of verbs, one key table.** Verbs, their labels and their keys come from one registry used by both apps, so menus read the same in both ("Open", "Open in editor", "Close terminal…", "Remove from Kete…").
- **Keys changed** (keys you've rebound yourself keep working; delete keeps its confirm):

  | Verb | Terminal app | Desktop |
  |---|---|---|
  | Diff | `d` (was `g`, which still works) | `d` anywhere (was `Ctrl+Shift+D`, which still works) |
  | Delete | `Del` only (`d` and `⌫` no longer delete) | `Del` (unchanged) |
  | Reply to the agent | `Space` (unchanged) | `Space` (was `f`) |
  | Open a session | `Enter` (unchanged) | `Enter` (`Space` no longer opens) |
  | Filter | `f` (unchanged) | `f` or `/`: the view's filter, otherwise the palette |
  | Refresh | `⇧R` (unchanged) | `⇧R` refreshes PRs (restart lost its `⇧R` default; it's in the menus) |
  | Digits | repos 1–9 (unchanged) | repos 1–9 in rail order (were Home/Session/Diff/Orchestrator; the orchestrator keeps `F10`) |
  | Full screen | `z` (`Ctrl+F` still works) | `z` (`Ctrl+Shift+M` still works) |
  | Help | `F1` (`?` is now the verbs menu) | `F1` (`?` is now the verbs menu) |
  | Settings | `s` or `Ctrl+,` | `Ctrl+,` |
  | ADO Branches | | `f` filters, fetch moved to `⇧R` |

- **Rebind keys on the desktop.** Settings → Hotkeys lists the window's own keys and you can edit them: add a key, unbind it, reset it, with a warning when a chord is already taken. The terminal app's keys show below as a separate group, which you can't edit. Desktop keys are saved beside the terminal app's under `keybindings` (`"desktop.<id>"`).
- **The diff keeps committed work.** Both apps compare a worktree against where it split from its base branch: the `worktree_base_branch` setting, else `origin/HEAD`, else main or master. An agent's commits stay in the diff. A toggle switches back to HEAD (desktop toolbar button and palette; `Ctrl+g` in the terminal app's diff). Staging and discarding work against HEAD only. A strip lists the commits since the base. The desktop's worktree picker is grouped by repo and asks which worktree instead of guessing.
- **Accounts, finished in the apps.** Rename an account (refused while a session still uses it, with the reason), delete its home on remove, set variables when adding one and clear them when editing, in both apps. Desktop Settings → Profiles now opens the Accounts view. Adopted-login accounts show their sign-in command everywhere. New CLI: `kete profile rename`, `kete profile duplicate`, `kete profile spread <manual|failover|least-used> [--threshold N]`, `kete session move-account`.
- **The apps agree on what needs you.** Board lanes, attention and ordering are computed once in the shared core, so the desktop no longer counts a stopped session as unread.

## 0.48.0

Pastes land whole, accounts are managed in the app, the two apps trade their best views, and
everything can be put in your own order by dragging.

- **Pasting into a session on Windows.** A large paste in the terminal app reaches the agent as one
  bracketed paste (Claude Code shows "[Pasted text +N lines]") instead of thousands of keystrokes with
  a submit per line. The daemon writes a session's input on its own thread, so a big paste never stalls
  the session or other requests, and pastes larger than a frame are split. Very large pastes are still
  slow to arrive through the Windows console.
- **Accounts, managed in the app.** The desktop's Accounts view (`Shift+U`, the rail's Accounts
  heading, the palette) and the terminal app's `Shift+U` list every account, including switched-off,
  broken and own logins, with login state, plan, usage and the sessions and repos using each. Add an
  account (harness, name, adopt your current login) and its sign-in runs straight away in a terminal;
  edit, duplicate, remove, set default, switch on or off, sign in again. The account spread (manual,
  failover, least used) is set at the top.
- **Accounts for work.** Each repo can have a default account per harness (repo menu, Accounts view,
  terminal app Settings → Project, `kete profile set-project`); launches take the named account, then the
  repo's default, then the spread, then the harness default. **Move to account…** on a session (menu, or
  click its account in the session header) restarts it fresh on another account in the same worktree.
  New CLI: `kete profile edit`, `kete profile set-project`. Needs a 0.48 daemon (protocol 57).
- **Drag to order, in both apps.** Repos, worktrees and sessions keep the order you give them, shared by
  both apps: drag repos in the rail or project tabs, worktree groups or bands (the main checkout stays
  first), and sessions within a worktree. `Alt+↑/↓` (terminal app also `Shift+↑/↓`) and Move up / Move
  down menu items do the same from the keyboard. A scope you've never reordered keeps its recency
  order; once you have two or more repos the overview follows the rail's order. Needs a 0.48 daemon
  (protocol 56).
- **The board, anywhere.** Drag a card to Done to mark it seen, onto the Archived zone that appears
  while dragging to archive it, onto a repo or worktree to move it; drops the agent owns (into Needs you
  or Running) are refused with the reason. Overview | Board and a Diff button sit in every desktop view
  and keep an open session or diff; `Shift+K` reaches the board from any terminal app overlay or a full
  screen session. A diff opened from the board returns to it.
- **Desktop worktrees.** The overview groups each repo's sessions under its worktrees, as the terminal
  app's bands do: main or linked, branch, ahead/behind, changes, PR, ports and run, foldable, with a row
  while git cuts a new one. Worktree buttons (new session or terminal here, diff, switch branch, open in
  editor or folder, copy path, run, delete). A Worktrees view per repo lists them all with sortable
  columns and "Delete merged & clean…". Narrow windows: the rail collapses to icons and the split shows
  one side with a List / Pane switch (minimum width 640).
- **Buttons for everything.** Desktop: palette, help and a New session button in the header; restart,
  follow-up, new terminal, open in editor, diff, full pane (`Ctrl+Shift+M`) and the menu on the session
  header; new session, new worktree, switch branch and editor on repo headers; diff, follow-up, archive
  and menu on board cards; tooltips name each key; dialogs keep Tab inside and return focus. Terminal
  app: footer key hints are clickable, settings and help in the header, a `+` on every band, reply / diff
  / archive / delete / `⋯` on the selected card (board too), diff and menu in the pane header, every help
  row and dialog answer clickable.
- **Desktop features in the terminal app.** A subagent tree (`Shift+S`, and a `⑂` chip on cards and the
  pane header), a command palette over every action (`Ctrl+K`), an attention strip (needs you / failed /
  running / unread, click to jump), Azure DevOps tasks, pull requests and branches (`Shift+B`, ADO repos
  only), and back / forward between places (`Alt+←/→` or `<` `>`).
- **Delete confirm wording.** The terminal app's forced-delete confirm reads `[Enter/y/Esc/n] keep it
  [f] delete anyway`.

## 0.47.0

The desktop diff closes properly, worktree deletes stop losing work, and both apps say which account
each session runs on.

- **Desktop diff fixes.** Diff opened from the orchestrator (or a session) has a close button and
  `Esc` closes it even from inside the editor, returning to where it was opened. A hidden diff no
  longer paints over the terminal or takes its clicks, which was what looked like a stretched terminal
  under a diff that wouldn't go away. A terminal shown again re-asserts its size.
- **Worktree deletes keep uncommitted work.** The terminal app's delete confirm reads the checkout
  first: the sessions and terminals it stops, uncommitted changes and unpushed commits. It never forces
  on the first answer. A dirty checkout takes a second confirm whose only yes is `f`. The daemon now
  refuses an unforced delete of a dirty checkout before it stops any session. The desktop's confirm
  counts terminals too and shows the same git state, and its "delete the emptied worktree too" box
  starts unticked when the checkout holds changes.
- **Desktop worktrees and branches.** Deleting the last session in a linked worktree offers to delete
  the checkout (Delete emptied worktree is now a shared setting). A branch switcher for the root
  checkout (`c` on a repo, the repo menu, the palette) with fuzzy local and remote branches, create on
  a new name, and stash / bring / discard for changes. Pull requests are polled for every worktree
  including main, show open, draft, merged and closed, and refresh on demand. Auto-clean merged
  worktrees now works from the desktop too.
- **Desktop alerts and more.** Done and needs-you notifications with the configured sounds while the
  window is in the background; a click opens the session (Windows). `kete open <file>` from a session
  opens a file sheet in the window. `.` and `,` jump to the next and previous session needing you. A
  version plate in the rail shows when an update is out. "Delete all N sessions" on a worktree.
- **Terminal app board and header.** The grid header ends in an `Overview | Board` toggle (`Shift+K`
  as before), and the board shows and flips its scope. Lane titles match the desktop. The session pane
  header shows status, harness@profile, project / branch, context, PR, age and the last prompt, giving
  way a piece at a time on narrow panes.
- **Accounts in both apps.** Every session label names its profile, the default included. The desktop
  quick prompt and start sheets start on "auto" under a failover or least-used spread and show what it
  would pick; the terminal app's quick prompt and `n` picker do the same, and `n` now picks the account.
  The Accounts rail and the terminal app's `Shift+U` list each account's sessions, show broken profiles
  with their error, mark a disabled default, and give every account that needs a login its command; the
  rail can run it in a new terminal. A Claude login whose refresh token has expired reads as expired.
  The rail refreshes on focus. Removing a profile says its sessions won't start again.
- **CI.** The orchestrator's inside-a-session check ignores stale parent links from reused Windows pids;
  the upgrade probe retries a binary that is still busy being written.

## 0.46.0

The desktop app catches up with the terminal app, and both get an orchestrator, account usage and
`kete ado`.

- **Desktop settings.** A Settings view (`Ctrl+,`, or the gear at the bottom of the rail) over the
  same `config.json` the terminal app uses: General, Sessions, Appearance, Agents, Project,
  Experimental and a read-only Hotkeys tab, plus Desktop, Presets and Profiles. Rows that only the
  terminal app uses sit under a "Terminal app" group and stay editable. Reset puts back what the
  window's tabs show and leaves project commands, profiles, harness definitions, hotkeys and hand-added
  keys alone. The window's theme, diff layout, home view and **Open on** are saved under a `desktop`
  object, separate from the terminal app's `theme`.
- **Live settings in the terminal app.** A running `kete` picks up a change to `config.json` or
  `config.local.json` within about three seconds, whoever wrote it, and waits while its own settings
  overlay is open.
- **Desktop launches follow the Agents tab.** New sessions get the configured model and effort, custom
  harnesses launch as themselves, and the harness list follows each harness's Enabled switch,
  `harnesses` overrides and Hide missing CLIs.
- **Desktop session and repo management.** Right-click (or `Shift+F10`) on a session row, board card,
  session header or repo header for the terminal app's card menu: follow-up, restart, duplicate, move,
  rename, mark seen, archive, unarchive and delete, plus run and stop, new terminal, diff, open in
  editor, open remote, new and delete worktree, rename, locate and remove a repo. Row keys follow the
  terminal app's (`r`, `a`, `u`, `m`, `Shift+P`), and `Del` deletes. Archived sessions show with `Shift+A`. Unread
  clears wherever you see the session.
- **Command palette and help.** `Ctrl+K` (or `Ctrl+Shift+P`) jumps to any repo, worktree, session or
  pull request, what needs you first, and runs any window action or menu verb. `?` or `F1` lists every
  window key. `Ctrl+Shift+P` is the palette now; `Ctrl+Shift+N` stays the quick prompt.
- **Desktop quick prompt.** Model, effort, profile and agent preset pickers, a New worktree switch, and
  multi-line tasks (`Shift+Enter` for a new line). It honours **Follow new session**.
- **Presets and profiles in the window.** Settings, Presets edits the terminal app's agent presets;
  Settings, Profiles adds, edits and removes `kete profile` accounts. A profile that needs a login shows
  the `kete profile login <name>` command to copy: the window never signs anything in.
- **Orchestrator.** One pinned orchestrator agent per machine that watches every repo, reads and
  messages other sessions, starts workers in fresh worktrees and asks you to review them. `F10` in the
  terminal app starts it (pick a harness and profile) or opens it full-screen with every other session
  as a chip; a review opens a modal with Terminal, Diff, History and PR tabs. In the desktop app it has
  its own view (`0` or `F10`, or the rail item above Repos): its cross-repo tree beside its chat, review
  notices that open a review sheet, and the quick prompt sending it follow-ups. `kete orchestrator
  start|status|clear` from the CLI. **Open on the orchestrator** (`orchestrator_view`) starts the
  terminal app on it; the window's **Open on** does the same there. `F10` no longer crashes the
  terminal app.
- **`kete ado`.** Azure DevOps from the CLI, for you or the orchestrator: `tasks`, `start-task` (branch,
  worktree and agent in one step), `prs`, `pr`, `pr create` (a draft by default, with the desktop
  composer's rules), `vote` and `reply`. `--project` and `--worktree` name the repo from anywhere. It
  never pushes the default branch and never signs in interactively.
- **Accounts and usage.** Kete reads each account's 5h and weekly use (Copilot's monthly allowances)
  and each session's context, from Claude's status line, Codex's session logs, Copilot's logs and its
  quota. `Shift+U` in the terminal app and the rail's Accounts list in the window show bars, resets,
  login state and an on/off switch per profile; session cards and rows show `ctx 42%` and compactions.
  `kete profile list` draws the same table, and `kete profile enable|disable` switches a profile.
- **Account spread.** Settings, Sessions, Accounts: `failover` moves new sessions to the next enabled
  profile when the default reaches a threshold, `least_used` picks the least used one, `manual` is the
  old behaviour and the default. A named profile is always used as named.
- **Claude status line.** Claude sessions start with `--settings` pointing at a status line in Kete's
  data dir (`kete _statusline`). It feeds the usage readings, then runs your own statusLine, or prints a
  short `ctx · 5h · wk` line. A custom status line takes the place of Claude's footer hints; turn it off
  with Settings, Sessions, **Usage status line**. Nothing is written into your repos.
- **Safer settings files.** Every writer (the terminal app, the window, `kete profile`, settings
  imports) takes a lock on the file and writes through a temp file of its own, and the window writes
  only the keys it changed. A file saved with a byte order mark (Notepad) reads fine. A `config.json`,
  `config.local.json` or `agent_presets.json` that exists but can't be parsed is never overwritten:
  saves fail with a message naming it until you fix or remove it. Before, a broken local layer was
  ignored and a broken presets file was replaced.
- **Open command consent.** In the window, Open in editor shows an open command that comes from a
  repo's `.kete.json` and asks before it first runs, per repo and command.
- **Windows fix.** Hide missing CLIs finds `claude.exe`, `codex.cmd` and the like through `PATHEXT`; it
  used to hide every harness.

Upgrading: the daemon speaks protocol 55 and still accepts 53, so 0.45 and 0.46 clients and daemons
talk to each other. The orchestrator and the usage readings need a 0.46 daemon. On macOS and Linux,
`kete upgrade` moves the running daemon onto the new binary with every session kept (or run
`kete reload` after installing another way). On Windows in-place restart isn't there yet: the 0.45
daemon keeps running after the installer, and `kete kill` (which stops every session) is what moves
you onto 0.46. On Windows the in-app hints say `kete kill`.

Going back to 0.45 works, with two things to know. The database stays on 0.46's schema, which 0.45
opens as it is, so the index that allows one orchestrator stays. And 0.45 doesn't know a profile's
`disabled` flag: it launches disabled profiles, and drops the flag the next time it edits the profile
list.

## 0.45.0

The first Kete release: a fork of [nebula](https://github.com/AgentSystemLabs/nebula), native on
Windows, with a desktop app and Azure DevOps.

- **Windows port.** Runs natively on Windows 10 1809+ and 11, x64 and ARM64, with no WSL or tmux. The
  daemon talks over a per-user named pipe and runs sessions on ConPTY.
- **Rebrand.** Nebula is now Kete: the `kete` binary, with `bro` (the old name) beside it as the same
  program. Bro's data moves to Kete's folders on first run.
- **Desktop app.** `kete desktop` opens a native window over the same daemon: an overview of every
  repo's session tree, nested agents and subagents included, ordered by what needs you; the board;
  live terminals; and a diff view with side by side, inline and replacement modes.
- **Azure DevOps.** Sprint tasks with Start task (branch, worktree and agent in one step), branches
  grouped by person, and pull requests: a draft composer with policy notes, policy status, votes,
  threads and publish. Sign-in comes from Git Credential Manager or the Azure CLI, held in memory.
- **Workshop themes.** A warm, square light and dark palette for the desktop app, the `kete-dark`
  and `kete-light` TUI themes (`kete-dark` is the default), and a Kete Workshop Windows Terminal
  scheme.
- **Profiles.** Several accounts per harness, each with its own config dir.
- **Copilot.** GitHub Copilot CLI joins the supported harnesses.
- **Ports.** Each worktree gets its own block of localhost ports, shown on its band.
- **Installer.** `install.ps1` installs `kete` and `bro` and, on x64, the desktop app, per user with
  no admin prompt.
