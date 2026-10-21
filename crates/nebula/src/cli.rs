//! The command-line surface: every `nebula` subcommand, its arguments, and the
//! help each one prints. `main.rs` owns only the dispatch and the logging.
//!
//! Two rules keep `--help` readable, and both are load-bearing:
//!
//! * **Every doc comment is two paragraphs.** `clap_derive` takes the first
//!   paragraph as `about` and the whole comment as `long_about`, and the root's
//!   command list only ever renders `about` — so paragraph one is a single
//!   sentence under ~60 characters that has to stand alone, and the prose after
//!   the blank line is what `nebula <command> --help` shows.
//! * **Examples are `after_help`**, which `after_long_help` falls back to, so
//!   one string serves both `-h` and `--help`. Clap runs it through the same
//!   wrapper as everything else: keep every line under ~78 characters or the
//!   hand-aligned columns reflow into a mess on a narrow terminal.
//!
//! Wrapping itself comes from clap's `wrap_help` feature (see Cargo.toml).
//! Without it `StyledStr::wrap` compiles to a no-op and no width setting here
//! does anything at all.

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

/// Parse the command line under the name it was run as: `kete`, or `bro`
/// (the old name, installed beside it), so usage lines say what you typed.
pub(crate) fn parse_as_invoked() -> Cli {
    let name = invoked_name(std::env::args_os().next().as_deref());
    let matches = Cli::command().bin_name(name).get_matches();
    Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit())
}

/// `bro` when argv[0] is a `bro` (the alias, a copy or a symlink), else
/// `kete`.
pub(crate) fn invoked_name(argv0: Option<&std::ffi::OsStr>) -> &'static str {
    let stem = argv0
        .map(std::path::Path::new)
        .and_then(std::path::Path::file_stem);
    match stem.and_then(|s| s.to_str()) {
        Some(s) if s.eq_ignore_ascii_case("bro") => "bro", // rebrand:keep
        _ => "kete",
    }
}

#[derive(Parser)]
#[command(
    name = "kete",
    version,
    max_term_width = 100,
    about = "Run your coding agents side by side, in one place",
    long_about = "Run your coding agents side by side, in one place.\n\n\
        Kete keeps a tree (projects hold worktrees, worktrees hold sessions) \
        and a background daemon owns every PTY in it. \
        Agents keep running after the TUI quits, and their scrollback is replayed \
        when you come back.\n\n\
        A bare `kete` opens the TUI. The commands below drive the same tree from \
        a shell; `rename`, `worktree`, `spawn` and `open` are the ones an agent \
        runs on your behalf from inside a session.",
    after_help = ROOT_EXAMPLES
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
    /// Directory to add as a project: shorthand for `kete add <dir>`.
    ///
    /// A directory whose name collides with a subcommand needs the long form
    /// (`kete add browser`) or a `./` prefix.
    pub(crate) dir: Option<String>,
}

const ROOT_EXAMPLES: &str = "\
Examples:
  kete                            open the TUI (auto-starts the daemon)
  kete add ~/code/my-app          register a project
  kete browser --port 8080        serve this TUI in a browser tab

Run `kete <command> --help` for a command's flags and examples.";

/// `--kind` for `kete spawn`: one of the agent CLIs kete runs. A bare
/// `custom` is never accepted: custom harnesses carry a registry id the
/// flag cannot name, so they launch from the TUI picker and presets.
fn parse_agent_kind(s: &str) -> Result<nebula_core::AgentKind, String> {
    nebula_core::AgentKind::parse(s).ok_or_else(|| {
        format!(
            "unknown harness `{s}`: expected one of {} (custom harnesses launch from the TUI)",
            nebula_core::AgentKind::ALL
                .iter()
                .filter(|k| **k != nebula_core::AgentKind::Custom)
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

fn parse_review_tab(s: &str) -> Result<nebula_core::ReviewTabKind, String> {
    match s {
        "terminal" => Ok(nebula_core::ReviewTabKind::Terminal),
        "diff" => Ok(nebula_core::ReviewTabKind::Diff),
        "history" => Ok(nebula_core::ReviewTabKind::History),
        "pr" | "pull-request" | "pull_request" => Ok(nebula_core::ReviewTabKind::PullRequest),
        _ => Err("expected terminal, diff, history or pr".into()),
    }
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Register a git checkout as a project.
    ///
    /// Adds a directory to the project list, named after the repository's
    /// root directory. Bare `kete <dir>` is the same
    /// command, so `kete .` and `kete add .` do the same thing.
    #[command(after_help = ADD_EXAMPLES)]
    Add {
        /// Path to a git repository (default: the current directory).
        #[arg(default_value = ".")]
        path: String,
    },
    /// Run the daemon that owns every session.
    ///
    /// The daemon holds every PTY, the store, git and agent status. The TUI
    /// auto-spawns it detached, so you rarely run this by hand: reach for it
    /// when you want to watch what the daemon is doing. Set NEBULA_LOG to
    /// change the log level.
    #[command(after_help = DAEMON_EXAMPLES)]
    Daemon {
        /// Stay attached to the terminal instead of logging to file.
        #[arg(long)]
        foreground: bool,
        /// Take over the sessions a restarting daemon wrote to this file.
        #[arg(long, hide = true, value_name = "STATE")]
        adopt: Option<std::path::PathBuf>,
    },
    /// Shut the running daemon down (stops all sessions).
    ///
    /// Asks the daemon to exit cleanly; every session it owns stops with it.
    /// A daemon from a build on another protocol can't take that request, so
    /// it gets SIGTERM instead, which it handles the same clean way.
    /// Quitting the TUI does not do this (the daemon outlives its clients on
    /// purpose) so this is how you stop everything. To move onto a newly
    /// installed binary without stopping anything, use `kete reload`.
    #[command(after_help = KILL_EXAMPLES)]
    Kill,
    /// Move the daemon onto the installed binary, keeping every session.
    ///
    /// Restarts the running daemon in place: it execs the `kete` binary
    /// this command runs from, and every session (agents mid-turn included)
    /// keeps running, its scrollback intact. Open TUIs lose their connection;
    /// relaunch `kete` to pick the sessions back up. `kete upgrade` does
    /// this on its own after installing. Needs a daemon from a build that
    /// has this command; an older one can only be restarted by `kete kill`.
    #[command(after_help = RELOAD_EXAMPLES)]
    Reload,
    /// Title the session this command runs inside.
    ///
    /// Run from inside a kete agent session: it titles that session's row.
    /// Agents run it themselves to auto-title on your first prompt. Without
    /// --force it only fills in a title that is still missing, so an agent
    /// can never overwrite a name you chose.
    #[command(after_help = RENAME_EXAMPLES)]
    Rename {
        /// The new title; multiple words need no quotes.
        #[arg(required = true, num_args = 1..)]
        title: Vec<String>,
        /// Replace an existing title instead of only filling in a missing one.
        #[arg(long)]
        force: bool,
    },
    /// Move this session into a worktree of its project.
    ///
    /// Run from inside a kete agent session; agents run it when you ask them
    /// to work in a worktree. Creates the git worktree when the branch has
    /// none, re-homes the session onto it at once, and restarts the session
    /// resumed inside the new checkout as soon as the current turn ends.
    #[command(after_help = WORKTREE_EXAMPLES)]
    Worktree {
        /// Branch name; several words are joined with hyphens, none at all
        /// gets a random `<adj>-<noun>-<verb>` one.
        name: Vec<String>,
        /// Start point for a new branch (default: the `worktree_base_branch`
        /// setting, else origin's default branch, fetched).
        ///
        /// A branch name origin has means origin's copy of it, fetched first:
        /// `main` is `origin/main`, never this checkout's local branch. A tag,
        /// a SHA or a branch origin lacks is used as named. Refused when the
        /// branch already exists, since it already has a start point.
        #[arg(long, value_name = "REF")]
        base: Option<String>,
    },
    /// Start another agent session beside this one.
    ///
    /// Run from inside a kete agent session; agents run it when you ask for
    /// a new kete session. The new session starts in the same worktree (or
    /// the one `--worktree` names), on the task you name as its first
    /// prompt, and shows up on the grid on its own, this session carries
    /// on untouched.
    #[command(after_help = SPAWN_EXAMPLES)]
    Spawn {
        /// The task the new session starts on; multiple words need no quotes.
        #[arg(required = true, num_args = 1..)]
        task: Vec<String>,
        /// Harness for the new session: claude, codex, cursor, pi, muse,
        /// grok, opencode or copilot.
        ///
        /// Defaults to the harness this session is running.
        #[arg(long, value_name = "KIND", value_parser = parse_agent_kind)]
        kind: Option<nebula_core::AgentKind>,
        /// Start the session in this project's worktree on BRANCH instead,
        /// creating the worktree when the branch has none; spaces become
        /// hyphens.
        #[arg(long, value_name = "BRANCH")]
        worktree: Option<String>,
        /// Start point for a new `--worktree` branch, resolved as
        /// `kete worktree --base` resolves it (default: the
        /// `worktree_base_branch` setting, else origin's default branch).
        /// Refused when the branch already exists.
        #[arg(long, value_name = "REF", requires = "worktree")]
        base: Option<String>,
        /// The account (a PROFILE's name) the new session runs on; it must
        /// be one for the new session's harness.
        ///
        /// Defaults to this session's account on this session's harness,
        /// else the harness's default account. Needs daemon protocol 58.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// The role card (an id in the repo's `.kete/roles.toml`) the new
        /// session launches with: its model, tools, prompt and permissions.
        /// Needs daemon protocol 66.
        #[arg(long, value_name = "ROLE")]
        role: Option<String>,
    },
    /// Show files to the user inside this kete.
    ///
    /// Run from inside a kete agent session; agents run it only when you
    /// ask to see a file, image, mockup or diagram, never unprompted.
    /// Opens text, supported local images and Mermaid diagrams; other
    /// binaries are refused.
    /// The files open in kete's file tabs (a modal with one tab per file,
    /// the focused one previewed, Enter editing it) in every kete
    /// attached to this daemon, and this session carries on untouched.
    #[command(after_help = OPEN_EXAMPLES)]
    Open {
        /// The files to show, relative to the current directory or absolute.
        #[arg(required = true, num_args = 1.., value_name = "FILE")]
        files: Vec<String>,
    },
    /// List every session across every project.
    ///
    /// Prints the session id/name, project, worktree branch, harness, status
    /// and a one-line recent summary. Agents run this when you refer to
    /// another kete session by name and they need its exact id.
    #[command(after_help = SESSIONS_EXAMPLES)]
    Sessions {
        /// Print JSON instead of compact text.
        #[arg(long)]
        json: bool,
    },
    /// One session's account.
    ///
    /// `kete session move-account <session> <profile>` restarts a session
    /// fresh on another account of its harness, as the apps' "Move to
    /// account" does.
    #[command(after_help = SESSION_EXAMPLES)]
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Read another session's recent context.
    ///
    /// Resolves a session by id or unambiguous fuzzy name, then prints clean
    /// recent conversation text. Claude transcript text is preferred when
    /// available; otherwise kete falls back to stripped PTY scrollback.
    #[command(after_help = READ_EXAMPLES)]
    Read {
        /// Session id or unambiguous fuzzy name.
        session: String,
        /// Number of recent text lines to print.
        #[arg(long, default_value_t = 200)]
        lines: u32,
    },
    /// Ask another session a question and print its answer.
    ///
    /// Sends a labelled prompt into the target agent, waits for that turn to
    /// finish, and prints only the answer text captured from the target's
    /// output. If the target is busy, the default is to wait until it is idle
    /// before sending; --no-wait fails fast instead.
    #[command(after_help = ASK_EXAMPLES)]
    Ask {
        /// Target agent id or unambiguous fuzzy name.
        session: String,
        /// Question text; multiple words need no quotes.
        #[arg(required = true, num_args = 1..)]
        question: Vec<String>,
        /// Total seconds to wait for queueing and the answer turn.
        #[arg(long, default_value_t = 120)]
        timeout: u64,
        /// Fail instead of queueing when the target is currently busy.
        #[arg(long)]
        no_wait: bool,
    },
    /// Start the orchestrator, or coordinate projects from it.
    ///
    /// `start`, `status` and `clear` are yours, from your own shell (F10 in
    /// kete does the same). Every other command is for the pinned
    /// orchestrator session only: the daemon refuses it unless the current
    /// NEBULA_AGENT_ID belongs to the persisted orchestrator row.
    #[command(after_help = ORCHESTRATOR_EXAMPLES)]
    Orchestrator {
        #[command(subcommand)]
        command: OrchestratorCommand,
    },
    /// Ship a worktree's branch: its pull request, opened and merged.
    ///
    /// GitHub through `gh`, Azure DevOps through its API, whichever the
    /// checkout's remote is. Sign-in is never interactive: run `gh auth
    /// login` or `az login` (or sign in from the desktop app) when asked.
    #[command(after_help = SHIP_EXAMPLES)]
    Ship {
        /// Act on this kete project (name or id) instead of the checkout
        /// you are in.
        #[arg(long, global = true, value_name = "PROJECT")]
        project: Option<String>,
        /// The worktree on this branch: in --project, else in the project
        /// of the checkout you are in (default: that checkout).
        #[arg(long, global = true, value_name = "BRANCH")]
        worktree: Option<String>,
        #[command(subcommand)]
        command: ShipCommand,
    },
    /// Azure DevOps for this repo: sprint tasks, task worktrees, pull requests.
    ///
    /// Org, project and repo come from the current checkout's Azure DevOps
    /// git remote. Sign-in is never interactive: Git Credential Manager is
    /// asked with `credential.interactive=never`, then `az account
    /// get-access-token`. When neither has a token, sign in from the
    /// desktop app or run `az login`.
    #[command(after_help = ADO_EXAMPLES)]
    Ado {
        /// Act on this kete project (name or id) instead of the checkout
        /// you are in: the orchestrator runs in its own workspace, so it
        /// names the repo here.
        #[arg(long, global = true, value_name = "PROJECT")]
        project: Option<String>,
        /// With --project: the project's worktree on this branch (default:
        /// its main checkout). `pr create` pushes this checkout's branch.
        #[arg(long, global = true, value_name = "BRANCH", requires = "project")]
        worktree: Option<String>,
        #[command(subcommand)]
        command: AdoCommand,
    },
    /// Back up, restore or locate this machine's settings.
    ///
    /// Settings live in `config.json` (the portable file an export, an
    /// import and `kete ssh` carry) with `config.local.json` over it for
    /// what only makes sense on this machine, beside `agent_presets.json` and
    /// `ssh_hosts.json`. An export is one JSON file holding all but the local
    /// layer; an import merges one in. Changes apply without a restart.
    #[command(after_help = CONFIG_EXAMPLES)]
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Manage PROFILES: several accounts per harness.
    ///
    /// A profile is a named account for one harness: a second Claude
    /// subscription, another ChatGPT login for Codex. Sessions launched under
    /// it get its own config dir (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`,
    /// `COPILOT_HOME`) under `<data dir>/profiles/<name>/`, and inherited
    /// credential variables (`ANTHROPIC_API_KEY`, `GH_TOKEN`, …) stripped.
    /// The quick prompt's `^A` picks one per launch.
    #[command(after_help = PROFILE_EXAMPLES)]
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Start accounts' 5-hour windows early (Whakaoho).
    ///
    /// At the hour you pick, one tiny prompt to the cheapest model under an
    /// account starts its five-hour window, so it resets earlier in your
    /// day. It is off by default, set per account, and uses your own
    /// subscription. A Windows scheduled task (`Kete\Whakaoho`) runs `kete
    /// wake --all` at each switched-on hour, whether Kete is open or not. A
    /// heartbeat goes out only when the account has no live window, is on
    /// and logged in, and hasn't had one for that hour. `--dry-run` says
    /// what would happen and runs nothing.
    #[command(
        after_help = WAKE_EXAMPLES,
        args_conflicts_with_subcommands = true,
        subcommand_negates_reqs = true
    )]
    Wake {
        /// A profile name, or @claude for Claude's own login.
        account: Option<String>,
        /// Every switched-on account: what the scheduled task runs.
        #[arg(long)]
        all: bool,
        /// Wake the account now, whatever its schedule says.
        #[arg(long)]
        force: bool,
        /// Say what would happen; run nothing.
        #[arg(long)]
        dry_run: bool,
        /// auto (interactive, print if the daemon can't), interactive, or print.
        #[arg(long, value_name = "MODE")]
        mode: Option<String>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        #[command(subcommand)]
        action: Option<WakeAction>,
    },
    /// Serve this TUI in a web browser via ttyd.
    ///
    /// Runs ttyd in front of a kete TUI and opens a tab on it, so a phone or
    /// another machine can drive this kete. Needs ttyd on PATH
    /// (`brew install ttyd`); Ctrl+C takes the server down. It listens on
    /// loopback unless --bind or --public widens it.
    #[command(after_help = BROWSER_EXAMPLES)]
    Browser {
        /// Port for ttyd to listen on.
        ///
        /// Omit to take 7681 when it's free and a free one otherwise, so a
        /// checkout per worktree can each serve at once. `--port 0` always
        /// picks a free one; a port named explicitly is used or the command
        /// fails, which is what you want behind an ssh tunnel.
        #[arg(long)]
        port: Option<u16>,
        /// Address to listen on (default 127.0.0.1).
        ///
        /// Name a specific interface address to reach this kete from another
        /// host, e.g. `--bind 10.0.1.7`. See --public for every interface.
        #[arg(long, value_name = "ADDR", conflicts_with = "public")]
        bind: Option<std::net::IpAddr>,
        /// Listen on every interface (0.0.0.0).
        ///
        /// For a kete on a remote box. This serves a live, writable terminal
        /// to anything that can reach the port: put a firewall, security
        /// group, or VPN in front of it, and consider --credential.
        #[arg(long)]
        public: bool,
        /// HTTP basic auth for the served terminal, as USER:PASSWORD.
        ///
        /// ttyd asks for it in the browser tab. Worth adding to any bind wider
        /// than loopback, on top of whatever guards the port itself.
        #[arg(long, value_name = "USER:PASSWORD")]
        credential: Option<String>,
        /// Serve the URL but do not open a desktop browser.
        ///
        /// For a machine with no desktop to open it on: `kete tunnel` runs
        /// the remote half this way.
        #[arg(long)]
        no_open: bool,
    },
    /// Open kete on a remote host over ssh.
    ///
    /// Connects with ssh and runs kete there, installing it on the remote
    /// first when it is missing, so what you drive is the remote's own daemon
    /// and sessions. This machine's `config.json` and agent presets ride
    /// along and are merged into the remote's settings, where its own
    /// `config.local.json` still wins. Destinations are remembered for the
    /// TUI's host picker (`Shift+H`).
    #[command(after_help = SSH_EXAMPLES)]
    Ssh {
        /// ssh destination, passed verbatim (e.g. user@server).
        host: String,
        /// Remote directory to start in (default: remote $HOME).
        path: Option<String>,
        /// Leave this machine's settings behind for this connection.
        ///
        /// The `ssh_sync_config` setting turns the forward off for good.
        #[arg(long)]
        no_sync_config: bool,
    },
    /// Open a remote host's kete in a browser tab here.
    ///
    /// One ssh tunnel does the whole thing: it installs kete on the remote
    /// if missing, runs `kete browser` on the remote's own loopback,
    /// forwards the port, and opens the local URL. Nothing is exposed on the
    /// remote's network (the tunnel is the only way in) so it needs no
    /// --credential. A `kete browser` already serving that port is reused
    /// rather than treated as a clash. Needs ttyd on the remote; Ctrl+C takes
    /// both ends down.
    #[command(after_help = TUNNEL_EXAMPLES)]
    Tunnel {
        /// ssh destination, passed verbatim (e.g. user@server).
        host: String,
        /// Remote directory to start in (default: remote $HOME).
        path: Option<String>,
        /// Local end of the tunnel, and the port the browser opens.
        ///
        /// Omit to take 7681 when it is free and a free port otherwise;
        /// `--port 0` always picks a free one.
        #[arg(long)]
        port: Option<u16>,
        /// Port the remote serves on (default: the same number as --port).
        ///
        /// Name one when something on the remote already holds that port.
        #[arg(long, value_name = "PORT")]
        remote_port: Option<u16>,
        /// Leave this machine's settings behind for this connection.
        ///
        /// By default `config.json` and the agent presets ride along, as they
        /// do for `kete ssh`; the `ssh_sync_config` setting turns that off
        /// for good.
        #[arg(long)]
        no_sync_config: bool,
    },
    /// Set up the terminal Kete runs in.
    ///
    /// `kete setup terminal` installs JetBrainsMono Nerd Font for this user
    /// (the FILE ICONS' glyphs need a Nerd Font) and adds a "Kete" profile
    /// to Windows Terminal: `kete` as its command line, that font, a One Dark
    /// color scheme. The settings file is backed up first.
    #[command(after_help = SETUP_EXAMPLES)]
    Setup {
        #[command(subcommand)]
        command: SetupCommand,
    },
    /// Open the desktop window: the kanban, live terminals and diffs.
    ///
    /// Launches `kete-desktop`, the native window over the same daemon this
    /// TUI talks to (it starts the daemon when none is running). It is
    /// looked for beside this `kete` first, then on PATH; when neither has
    /// it, this prints where to get it. Arguments after `--` go to the
    /// window: `--view board|session|diff|orchestrator`, `--session <name>`,
    /// `--theme light|dark|system`, `--diff side|inline|replacement`.
    #[command(after_help = DESKTOP_EXAMPLES)]
    Desktop {
        /// Options for the window, after `--`.
        #[arg(last = true, value_name = "ARGS")]
        args: Vec<String>,
    },
    /// Your editors: list the ones Kete found, open a path in one.
    ///
    /// The same VS Code and Visual Studio the menus open: VS Code and its
    /// forks (Insiders, VSCodium, Cursor, Windsurf…) found by their own
    /// install, Visual Studio through its installer (Community,
    /// Professional and Enterprise alike, previews too). Each is started
    /// directly and detached, without Kete's own environment.
    #[command(after_help = EDITORS_EXAMPLES)]
    Editors {
        #[command(subcommand)]
        command: EditorsCommand,
    },
    /// Install the latest published kete over this one.
    ///
    /// Runs the install script for the newest release, then moves a running
    /// daemon onto the new binary in place (see `kete reload`): every
    /// session keeps running. A daemon from before in-place restarts can't
    /// be moved; its sessions keep running on the old binary until you
    /// restart it with `kete kill` (which stops all sessions), and when the
    /// new build can't attach to it, upgrade says so and offers that restart.
    #[command(after_help = UPGRADE_EXAMPLES)]
    Upgrade {
        /// Upgrade even when running from a local cargo build.
        #[arg(long)]
        force: bool,
    },
    /// Installer hook: print the cutover note only when a live daemon is on
    /// a different build than this binary (see `make install` / install.sh).
    #[command(hide = true, name = "_stale-daemon-note")]
    StaleDaemonNote,
    /// Upgrade hook: print the protocol version this binary speaks, so the
    /// `kete upgrade` that installed it can tell whether it will still
    /// attach to the daemon left running.
    #[command(hide = true, name = "_protocol-version")]
    ProtocolVersion,
    /// Upgrade hook: print the oldest protocol version this binary can
    /// attach to, paired with `_protocol-version` for compatibility checks.
    #[command(hide = true, name = "_protocol-min-compatible-version")]
    ProtocolMinCompatibleVersion,
    /// Reload hook: print the newest restart-state version this binary
    /// reads, so a daemon asked to restart onto it knows it can.
    #[command(hide = true, name = "_restart-version")]
    RestartVersion,
    /// Studio mode: a repo's inbox, autonomy and queue.
    ///
    /// A repo describes itself in `.kete/studio.toml` (its inbox folder,
    /// departments and verdict folders); what this machine allows it (the
    /// inbox the queue reads, the autonomy level A0 to A3, notes to leave
    /// alone) lives in `config.local.json` and only you change it, never a
    /// session. While the orchestrator sits idle, kete hands it a short note
    /// naming new inbox notes.
    #[command(after_help = STUDIO_EXAMPLES)]
    Studio {
        #[command(subcommand)]
        command: StudioCommand,
    },
    /// Role cards: what each role gets, and bypass acceptance.
    ///
    /// A repo's `.kete/roles.toml` (read from its committed HEAD) names
    /// roles with a model, effort, tool set, prompt and permission profile.
    /// `--role <id>` on a spawn applies one.
    #[command(after_help = ROLES_EXAMPLES)]
    Roles {
        #[command(subcommand)]
        command: RolesCommand,
    },
    /// Tokens, cost and turns per session per day.
    ///
    /// The usage ledger: input, output and cache tokens, Claude Code's
    /// estimated cost (a list-price figure, not a bill), tool uses and
    /// turns, one row per session per local day. Today by session unless
    /// told otherwise; `--csv` prints the rows as CSV.
    #[command(after_help = USAGE_EXAMPLES)]
    Usage {
        /// First local date to include, YYYY-MM-DD.
        #[arg(long, value_name = "DATE")]
        since: Option<String>,
        /// Last local date to include, YYYY-MM-DD.
        #[arg(long, value_name = "DATE")]
        until: Option<String>,
        /// One project only (id or name).
        #[arg(long)]
        project: Option<String>,
        /// Group the table by session (default), day or role.
        #[arg(long, value_name = "WHAT", value_parser = ["session", "day", "role"])]
        by: Option<String>,
        /// Print every row as CSV instead of a table.
        #[arg(long)]
        csv: bool,
    },
    /// The state map: what a studio repo holds and who is on it.
    ///
    /// Reads `.kete/state-map.json`, which the daemon rebuilds for a repo in
    /// studio mode: departments, systems, decisions, lanes, pull requests
    /// and agents. Prints the tree, or with --slice the short text (under
    /// 2 KB) an agent reads about one node.
    #[command(after_help = STATE_EXAMPLES)]
    State {
        /// The repo, or any checkout of it (default: the current folder).
        repo: Option<std::path::PathBuf>,
        /// One node's slice instead of the tree.
        #[arg(long, value_name = "NODE")]
        slice: Option<String>,
        /// Print JSON: the whole map, or with --slice the node and its edges.
        #[arg(long)]
        json: bool,
    },
    /// Role sessions' PreToolUse guard: refuse a force push and what the
    /// repo's autonomy denies. Reads the hook payload on stdin; exits 2
    /// with the reason to refuse, 0 to let the command run.
    #[command(hide = true, name = "_guard")]
    Guard {
        /// The session the hook runs for.
        #[arg(long)]
        session: String,
    },
    /// Agent-CLI hook (the Windows hook command): POST the hook payload on
    /// stdin to the daemon's hook receiver, as the unix `curl` one-liner
    /// does. Inert outside a kete agent session; always exits 0.
    #[command(hide = true, name = "_hook")]
    Hook {
        /// The receiver route: claude, codex, cursor, pi, opencode or copilot.
        endpoint: String,
        /// The hook event, as the daemon names it (`Stop`, …).
        event: String,
    },
    /// Claude statusLine command: hand the statusLine JSON on stdin to the
    /// daemon (plan usage, context), then print the user's own statusLine,
    /// or a compact usage line when they have none. Always exits 0.
    #[command(hide = true, name = "_statusline")]
    Statusline,
}

#[derive(Subcommand)]
pub(crate) enum OrchestratorCommand {
    /// Start the machine's one orchestrator session.
    ///
    /// Without --project it runs in a workspace of its own under kete's data
    /// dir, so it never writes into one of your repos. Refused while another
    /// orchestrator is running unless --replace, which demotes that one to an
    /// ordinary session.
    Start {
        /// Harness to run it on.
        #[arg(long, value_name = "KIND", value_parser = parse_agent_kind, default_value = "claude")]
        kind: nebula_core::AgentKind,
        /// Model the harness launches with.
        #[arg(long)]
        model: Option<String>,
        /// Reasoning effort the harness launches with.
        #[arg(long)]
        effort: Option<String>,
        /// PROFILE (account) to run under; default is the harness's own.
        #[arg(long)]
        profile: Option<String>,
        /// Run in this project's root worktree instead (id or name).
        #[arg(long)]
        project: Option<String>,
        /// Session name (default: orchestrator).
        #[arg(long)]
        name: Option<String>,
        /// Replace a running orchestrator; it keeps running as a worker.
        #[arg(long)]
        replace: bool,
        /// Role card (an id in the project's `.kete/roles.toml`) to launch
        /// it with. Needs daemon protocol 66.
        #[arg(long, value_name = "ROLE")]
        role: Option<String>,
    },
    /// Show which session is the orchestrator and what it is doing.
    Status {
        /// Print JSON instead of compact text.
        #[arg(long)]
        json: bool,
    },
    /// Unpin the orchestrator; its session keeps running as a worker.
    Clear,
    /// List every live session in attention order.
    ///
    /// needs_feedback first, then running, then unread (a finished turn
    /// nobody has looked at), then the rest, with role, parent and open
    /// subagents. --json keeps the tree (projects, worktrees, agents,
    /// terminals, links) and adds the ordered `sessions`.
    List {
        /// Print JSON instead of compact text.
        #[arg(long)]
        json: bool,
    },
    /// Read a session's recent context.
    ///
    /// The harness transcript when kete knows it, else the cleaned terminal
    /// tail. --bytes reads the raw bounded terminal tail instead.
    Read {
        /// Session id, terminal:<id>, or unambiguous name.
        session: String,
        /// Number of recent text lines to print.
        #[arg(long, default_value_t = 200)]
        lines: u32,
        /// Read this many bytes of the raw terminal tail instead.
        #[arg(long)]
        bytes: Option<u32>,
    },
    /// Send a follow-up prompt to another agent, labelled [from orchestrator].
    ///
    /// A busy target is waited on until it is idle; --no-wait fails instead.
    /// The orchestrator cannot send to itself.
    Send {
        /// Target agent id or unambiguous name.
        session: String,
        /// Prompt text; multiple words need no quotes.
        #[arg(required = true, num_args = 1..)]
        prompt: Vec<String>,
        /// Seconds to wait for a busy target.
        #[arg(long, default_value_t = 120)]
        timeout: u64,
        /// Fail instead of waiting when the target is busy.
        #[arg(long)]
        no_wait: bool,
    },
    /// Spawn an agent in any project or worktree.
    Spawn {
        /// Project id or visible project name.
        #[arg(long)]
        project: String,
        /// Worktree id or branch. Existing checkouts are reused; a missing
        /// branch is cut using kete's normal worktree base rules.
        #[arg(long)]
        worktree: Option<String>,
        /// Start point when a new worktree has to be cut.
        #[arg(long)]
        base: Option<String>,
        /// Harness for the new session.
        #[arg(long, value_name = "KIND", value_parser = parse_agent_kind, default_value = "claude")]
        kind: nebula_core::AgentKind,
        /// Session name (default: the next free agent-N, auto-titled).
        #[arg(long)]
        name: Option<String>,
        /// Model the harness launches with.
        #[arg(long)]
        model: Option<String>,
        /// Reasoning effort the harness launches with.
        #[arg(long)]
        effort: Option<String>,
        /// PROFILE (account) to run under.
        #[arg(long)]
        profile: Option<String>,
        /// Role card (an id in the project's `.kete/roles.toml`) the worker
        /// launches with. Needs daemon protocol 66.
        #[arg(long, value_name = "ROLE")]
        role: Option<String>,
        /// Prompt text; multiple words need no quotes.
        #[arg(required = true, num_args = 1..)]
        task: Vec<String>,
    },
    /// Hand the orchestrator over to a fresh session.
    ///
    /// The new orchestrator starts on the handover note as its first
    /// prompt once the current turn ends (`--now`: at once), takes the
    /// role, and the old one is archived (`--keep-old` keeps it as a
    /// worker). Run by the orchestrator itself, or by you.
    Renew {
        /// The handover note: a file in the orchestrator's workspace or a
        /// registered project.
        #[arg(long = "from", value_name = "NOTE")]
        from: std::path::PathBuf,
        /// The account the new orchestrator runs on (default: the old one's).
        #[arg(long, conflicts_with = "pick")]
        profile: Option<String>,
        /// Let kete pick the account, as `kete profile pick` would.
        #[arg(long)]
        pick: bool,
        /// The model it runs (default: the old one's).
        #[arg(long)]
        model: Option<String>,
        /// Keep the old session running as a worker.
        #[arg(long)]
        keep_old: bool,
        /// Swap now instead of at the end of the current turn.
        #[arg(long)]
        now: bool,
        /// Print JSON instead of a line.
        #[arg(long)]
        json: bool,
    },
    /// Open the tabbed review modal in every attached TUI.
    Review {
        /// Sessions to review: ids, terminal:<id>, or unambiguous names.
        #[arg(required = true, num_args = 1..)]
        sessions: Vec<String>,
        /// Tabs to open. Repeats allowed; default is terminal, diff, history, pr.
        #[arg(long = "tab", value_parser = parse_review_tab)]
        tabs: Vec<nebula_core::ReviewTabKind>,
    },
}

#[derive(Subcommand)]
pub(crate) enum ShipCommand {
    /// The forge, branch, base, unpushed commits and the branch's pull request.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Push the branch and open its pull request.
    ///
    /// Co-author and attribution lines are cleaned from unpushed commits
    /// and the body first. An open PR from the branch is not opened twice:
    /// the push updates it. Without --title, the title and body are
    /// suggested from the session and the commits since the base.
    Pr {
        #[arg(long)]
        title: Option<String>,
        /// The description; `-` reads it from stdin.
        #[arg(long)]
        body: Option<String>,
        /// Open it as a draft.
        #[arg(long)]
        draft: bool,
        /// The branch it merges into (default: the review base, the
        /// `worktree_base_branch` setting, else the default branch).
        #[arg(long, value_name = "BRANCH")]
        base: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Merge the branch's open pull request.
    ///
    /// GitHub: `gh pr merge`, pinned to the head commit it was read at.
    /// Azure DevOps: the PR is completed through its API; one its policies
    /// hold says so with its page. The local branch and worktree stay.
    Merge {
        /// squash, merge or rebase.
        #[arg(long, default_value = "squash", value_parser = parse_merge_method)]
        method: kete_ado::ship::MergeMethod,
        /// Delete the branch on the remote once merged.
        #[arg(long)]
        delete_branch: bool,
        #[arg(long)]
        json: bool,
    },
}

fn parse_merge_method(s: &str) -> Result<kete_ado::ship::MergeMethod, String> {
    kete_ado::ship::MergeMethod::parse(s)
        .ok_or_else(|| format!("unknown merge method `{s}`: expected squash, merge or rebase"))
}

#[derive(Subcommand)]
pub(crate) enum AdoCommand {
    /// Your tasks in the team's current sprint, grouped by parent.
    ///
    /// The team is the one the desktop's Tasks view remembers for this
    /// repo, else the project's default team. Offline or signed out, the
    /// last list saved for the scope is shown and says so.
    Tasks {
        /// Everyone's open items in the current sprint, not only yours.
        #[arg(long, conflicts_with = "open")]
        all: bool,
        /// Everything open assigned to you, in any sprint.
        #[arg(long)]
        open: bool,
        /// Print JSON instead of compact text.
        #[arg(long)]
        json: bool,
    },
    /// Start a task: a new branch and worktree, linked to it, with an agent.
    ///
    /// As the desktop's Start task: the branch (default `<initials>/<slug
    /// of the title>`) is cut from the repo's Azure DevOps default branch
    /// unless --base, linked to the task, the task assigned to you and moved
    /// to In Progress, then an agent starts in the worktree with the task as
    /// its prompt. Run from the orchestrator, the agent is its child.
    StartTask {
        /// Work item id.
        id: u32,
        /// Harness for the agent (default: the quick prompt's harness).
        #[arg(long, value_name = "HARNESS")]
        agent: Option<String>,
        /// Make the branch and worktree only; start no agent.
        #[arg(long, conflicts_with = "agent")]
        no_agent: bool,
        /// Branch name instead of the suggested one.
        #[arg(long)]
        branch: Option<String>,
        /// Start point instead of the repo's default branch.
        #[arg(long)]
        base: Option<String>,
        /// Leave the task's assignee as it is.
        #[arg(long)]
        no_assign: bool,
        /// Leave the task's state as it is.
        #[arg(long)]
        no_activate: bool,
        /// The agent's opening prompt instead of the task's text.
        #[arg(long)]
        prompt: Option<String>,
        /// Model the agent launches with.
        #[arg(long)]
        model: Option<String>,
        /// Reasoning effort the agent launches with.
        #[arg(long)]
        effort: Option<String>,
        /// PROFILE (account) the agent runs under.
        #[arg(long)]
        profile: Option<String>,
        /// Print JSON instead of compact text.
        #[arg(long)]
        json: bool,
    },
    /// Active pull requests: yours, and those waiting on your review.
    Prs {
        /// Print JSON instead of compact text.
        #[arg(long)]
        json: bool,
    },
    /// One pull request (detail, policies, threads), or `pr create`.
    #[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
    Pr {
        /// Pull request id.
        #[arg(required = true)]
        id: Option<u32>,
        /// Print JSON instead of compact text.
        #[arg(long)]
        json: bool,
        #[command(subcommand)]
        action: Option<AdoPrAction>,
    },
    /// Vote on a pull request.
    Vote {
        /// Pull request id.
        id: u32,
        /// approve, approve-with-suggestions, wait, reject or reset.
        #[arg(value_parser = parse_vote)]
        vote: i8,
    },
    /// Reply on a pull request's comment thread.
    Reply {
        /// Pull request id.
        id: u32,
        /// Thread id (`kete ado pr <id>` lists them).
        thread: u32,
        /// Reply text; multiple words need no quotes.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum AdoPrAction {
    /// Push this branch and open a pull request from it.
    ///
    /// As the desktop's composer: AI co-author and attribution lines are
    /// taken out of unpushed commit messages first, the PR opens as a draft
    /// unless --publish, it needs a linked task (--task, else the branch's
    /// own) unless --no-task and a reviewer unless --no-reviewer. An active
    /// PR from the branch is reported, never doubled. With --project it
    /// needs --worktree (or --source); a source that is the target or the
    /// repo's default branch is refused before anything is pushed.
    Create {
        /// Title (default: the task's title, else the only commit subject).
        #[arg(long)]
        title: Option<String>,
        /// The Summary section of the description.
        #[arg(long)]
        summary: Option<String>,
        /// The Testing section of the description.
        #[arg(long)]
        testing: Option<String>,
        /// Target branch (default: the repo's default branch).
        #[arg(long)]
        target: Option<String>,
        /// Source branch (default: the checked-out one).
        #[arg(long)]
        source: Option<String>,
        /// Work item to link (default: the task the branch was started for).
        #[arg(long, conflicts_with = "no_task")]
        task: Option<u32>,
        /// Open it without a linked task.
        #[arg(long)]
        no_task: bool,
        /// Reviewer by name or email; repeat for more.
        #[arg(long = "reviewer", value_name = "WHO")]
        reviewers: Vec<String>,
        /// Open it without a reviewer.
        #[arg(long, conflicts_with = "reviewers")]
        no_reviewer: bool,
        /// Publish at once instead of opening a draft.
        #[arg(long)]
        publish: bool,
        /// Print JSON instead of compact text.
        #[arg(long)]
        json: bool,
    },
}

/// A vote word as Azure DevOps' number.
pub(crate) fn parse_vote(s: &str) -> Result<i8, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "approve" | "approved" | "10" => Ok(10),
        "approve-with-suggestions" | "suggestions" | "5" => Ok(5),
        "wait" | "wait-for-author" | "-5" => Ok(-5),
        "reject" | "rejected" | "-10" => Ok(-10),
        "reset" | "none" | "0" => Ok(0),
        _ => Err(format!(
            "unknown vote `{s}`: approve, approve-with-suggestions, wait, reject or reset"
        )),
    }
}

const ADO_EXAMPLES: &str = "\
Examples:
  kete ado --project ledger-api tasks            from anywhere, e.g. the orchestrator
  kete ado --project ledger-api --worktree jari/gst-rounding pr create --task 4211
  kete ado tasks                                 your tasks in the current sprint
  kete ado tasks --all --json
  kete ado start-task 4211                       branch, worktree and an agent for it
  kete ado start-task 4211 --agent codex --branch jr/fix-gst
  kete ado prs
  kete ado pr 812 --json                         detail, policies and threads
  kete ado pr create --task 4211 --reviewer sam@example.com
  kete ado vote 812 approve
  kete ado reply 812 3 \"fixed in the latest push\"";

const SHIP_EXAMPLES: &str = "\
Examples:
  kete ship status                               this checkout's branch and its PR
  kete ship pr --draft                           push and open a draft, title suggested
  kete ship pr --title \"Fix the login redirect\" --body - < notes.md
  kete ship --project ledger-api --worktree jr/fix-gst status --json
  kete ship merge --method squash --delete-branch";

const ADD_EXAMPLES: &str = "\
Examples:
  kete add .                     add the repo you are standing in
  kete add ~/code/my-app         add one by path
  kete ~/code/my-app             the same, without the subcommand";

const DAEMON_EXAMPLES: &str = "\
Examples:
  kete daemon --foreground       run it attached, logs on stdout
  NEBULA_LOG=debug kete daemon --foreground
                                   the same, at debug level";

const KILL_EXAMPLES: &str = "\
Examples:
  kete kill                      stop the daemon and every session";

const RELOAD_EXAMPLES: &str = "\
Examples:
  kete reload                    restart the daemon, sessions and all
  make install && kete reload    cut over to a local build";

const RENAME_EXAMPLES: &str = "\
Examples:
  kete rename Fix Login Redirect   title this session
  kete rename --force Auth Rework  replace a title already set";

const WORKTREE_EXAMPLES: &str = "\
Examples:
  kete worktree fix-login-redirect  branch off the configured base and move there
  kete worktree fix login redirect  the same; the words are slugified
  kete worktree                     invent a random branch name
  kete worktree hotfix --base v0.21.0
                                      branch from a named start point";

const SPAWN_EXAMPLES: &str = "\
Examples:
  kete spawn \"port the tests to the new fixture\"
  kete spawn --kind codex \"review the diff on this branch\"
  kete spawn --worktree fix-login \"fix the login redirect\"
                                   start it in that branch's worktree, made if new
  kete spawn --worktree hotfix --base v0.21.0 \"backport the fix\"
  kete spawn --profile work \"triage the failing CI run\"";

const OPEN_EXAMPLES: &str = "\
Examples:
  kete open README.md                one tab
  kete open mockup.png flow.mmd      image and diagram previews
  kete open src/main.rs docs/keys.md a tab each, in this order";

const SESSIONS_EXAMPLES: &str = "\
Examples:
  kete sessions
  kete sessions --json";

const READ_EXAMPLES: &str = "\
Examples:
  kete read research
  kete read 01HZY8QHA4AYM7M4XTA2G3KC9B --lines 80";

const ASK_EXAMPLES: &str = "\
Examples:
  kete ask research \"which library did you recommend?\"
  kete ask \"feature spike\" \"summarize your Redis findings\" --timeout 60
  kete ask research \"are you idle?\" --no-wait";

const ORCHESTRATOR_EXAMPLES: &str = "\
Examples (yours, from your own shell):
  kete orchestrator start                        claude, in its own workspace
  kete orchestrator start --kind codex --profile work
  kete orchestrator status
Examples (the orchestrator session's):
  kete orchestrator list --json
  kete orchestrator read fix-login --lines 80
  kete orchestrator send fix-login \"continue after the test failure\"
  kete orchestrator spawn --project kete --worktree fix-auth --name auth \"fix the login redirect\"
  kete orchestrator review fix-login agent-2 --tab diff --tab terminal";

const STUDIO_EXAMPLES: &str = "\
Examples:
  kete studio enable my-game        studio mode on, at A1 (Plan)
  kete studio status               each studio repo: autonomy, inbox, counts
  kete studio autonomy my-game A2   let it deliver: pushes and pull requests
  kete studio exclude my-game library-sweep-2
  kete studio queue --now          send the idle note now, if it holds
  kete studio note \"the storm sky bands at dusk\" --project my-game";

const ROLES_EXAMPLES: &str = "\
Examples:
  kete roles check --project my-game       what every role gets
  kete roles check --role runner --json
  kete roles accept-bypass max2           answer Claude's bypass dialog";

const USAGE_EXAMPLES: &str = "\
Examples:
  kete usage                       today, by session
  kete usage --since 2026-10-21 --by role
  kete usage --csv --since 2026-10-21 > ledger.csv";

const STATE_EXAMPLES: &str = "\
Examples:
  kete state                       the tree of the repo you are in
  kete state --slice D-012         what an agent reads about one node
  kete state ../game --json        the whole map as JSON";

const BROWSER_EXAMPLES: &str = "\
Examples:
  kete browser                   serve on 127.0.0.1:7681, open a tab
  kete browser --port 8080       take a specific port
  kete browser --no-open         serve only; print the URL
  kete browser --public --credential me:secret
                                   reachable off-box, behind basic auth";

const SSH_EXAMPLES: &str = "\
Examples:
  kete ssh user@server           open the remote's kete
  kete ssh user@server /srv/app  start in a directory there
  kete ssh user@server --no-sync-config
                                   keep this machine's settings here";

const CONFIG_EXAMPLES: &str = "\
Examples:
  kete config path               where each settings file lives
  kete config export ~/backups   write ~/backups/nebula-settings.json
  kete config import ~/backups   merge it back in, here or elsewhere";

const TUNNEL_EXAMPLES: &str = "\
Examples:
  kete tunnel user@server           the remote's TUI in a tab here
  kete tunnel user@server /srv/app  start in a directory there
  kete tunnel user@server --port 9000
                                      pick the local end of the tunnel";

const DESKTOP_EXAMPLES: &str = "\
Examples:
  kete desktop                   open the window
  kete desktop -- --view diff    open it on the diff view
  kete desktop -- --view orchestrator
                                 open it on the orchestrator
  kete desktop -- --theme light  force the light theme";

const EDITORS_EXAMPLES: &str = "\
Examples:
  kete editors list                      what is installed, the default of each kind first
  kete editors open .                    this checkout in VS Code
  kete editors open src/main.rs --line 42
  kete editors open . --with vs          its solution in Visual Studio
  kete editors open Api.sln              a solution opens in Visual Studio
  kete editors open . --with cursor      an install by its id from the list";

const UPGRADE_EXAMPLES: &str = "\
Examples:
  kete upgrade                   install the latest release
  kete upgrade --force           do it over a local cargo build";

const PROFILE_EXAMPLES: &str = "\
Examples:
  kete profile add main --harness claude --use-default --default
  kete profile add max2 --harness claude
  kete profile login max2        sign the new account in
  kete profile duplicate max2 max3      same harness and variables, its own login
  kete profile rename max2 work         its home and project defaults follow
  kete profile remove max3 --purge      its home (the login) goes too
  kete profile spread failover --threshold 80
  kete profile edit max2 --env HTTPS_PROXY=http://proxy:8080
  kete profile set-project . max2       max2 for Claude sessions in this repo
  kete profile set-project app --clear claude
  kete profile list              accounts, homes and last seen usage";

const WAKE_EXAMPLES: &str = "\
Examples:
  kete wake set work --at 05:00        on, every day at 05:00
  kete wake set work --at suggested    the hour that fits your usual day
  kete wake set work --days mon-fri --keep-rolling
  kete wake status               schedules, next and last wake-ups
  kete wake work --dry-run       what would happen now
  kete wake work --force         wake it now, whatever the schedule
  kete wake off work             off; the hour is kept for next time
  kete wake --all                what the scheduled task runs";

#[derive(Subcommand)]
pub(crate) enum WakeAction {
    /// Switch an account's wake-up on, at an hour.
    ///
    /// Options left out keep their values. Saved in this machine's
    /// `config.local.json` (never exported or sent to remotes), and the
    /// scheduled task is rewritten to match.
    Set {
        /// A profile name, or @claude.
        account: String,
        /// HH:MM in 15-minute steps, or `suggested`.
        #[arg(long, value_name = "HH:MM")]
        at: Option<String>,
        /// all, mon-fri, weekends, or a list such as mon,wed,fri.
        #[arg(long)]
        days: Option<String>,
        /// Also start a window at each 5-hour step inside your usual hours.
        #[arg(long, overrides_with = "no_keep_rolling")]
        keep_rolling: bool,
        /// Only the hour itself.
        #[arg(long)]
        no_keep_rolling: bool,
        /// Skip it after N days without use (0: never skip).
        #[arg(long, value_name = "N")]
        skip_idle: Option<u32>,
        /// auto (the default: interactive, falling back to print while the
        /// daemon can't run it), interactive (never falls back), or print.
        #[arg(long, value_name = "MODE")]
        mode: Option<String>,
        /// Let the scheduled task wake the PC from sleep.
        #[arg(long, overrides_with = "no_wake_pc")]
        wake_pc: bool,
        /// Run only while the PC is awake (catching up when it wakes).
        #[arg(long)]
        no_wake_pc: bool,
    },
    /// Switch an account's wake-up off.
    ///
    /// Its hour and options are kept for the next `kete wake set`. The
    /// scheduled task goes when no account is on.
    Off {
        /// A profile name, or @claude.
        account: String,
    },
    /// Schedules, next and last wake-ups, and suggested hours.
    ///
    /// For each Claude account: its schedule, the next wake-up, its live
    /// window, its usual hours from Kete's sessions with the suggested
    /// hour, and the last decision from the wake log.
    Status {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum EditorsCommand {
    /// List VS Code, its forks and Visual Studio as Kete finds them: id,
    /// name, version, edition and path, the default of each kind first.
    #[command(after_help = EDITORS_EXAMPLES)]
    List {
        /// One JSON array instead of the table.
        #[arg(long)]
        json: bool,
    },
    /// Open a folder, a file or a solution in an editor.
    ///
    /// A folder opens in VS Code (focusing a window already open on it);
    /// with --with vs, its solution or project in Visual Studio. A file
    /// opens in its checkout's VS Code window, at --line; in Visual Studio
    /// it goes to the instance already running. A .sln, .slnx, .csproj or
    /// .vcxproj opens in Visual Studio.
    #[command(after_help = EDITORS_EXAMPLES)]
    Open {
        /// The folder, file or solution.
        path: std::path::PathBuf,
        /// vscode (the default), vs, or an install's id from `kete editors list`.
        #[arg(long = "with", value_name = "EDITOR")]
        with: Option<String>,
        /// The line a file opens at (VS Code).
        #[arg(long, value_name = "N")]
        line: Option<u32>,
    },
}

#[derive(Subcommand)]
pub(crate) enum ProfileCommand {
    /// List the accounts: harness, default and off flags, 5h and weekly
    /// usage bars, and whether each is signed in.
    List {
        /// Print JSON: every account with its windows, live sessions and
        /// holds.
        #[arg(long)]
        json: bool,
    },
    /// Name the account a scripted launch should take, and why.
    ///
    /// The pick for automation: reserved and switched-off accounts are
    /// never candidates. It holds the account for 10 minutes so the next
    /// pick spreads; `--dry-run` shows every candidate and holds nothing.
    /// Exits 3 when no account is eligible.
    Pick {
        /// The harness to pick for (default: claude).
        #[arg(long)]
        harness: Option<String>,
        /// The project whose default account counts (id or name).
        #[arg(long)]
        project: Option<String>,
        /// Show the candidates; hold nothing.
        #[arg(long)]
        dry_run: bool,
        /// Print JSON instead of the name.
        #[arg(long)]
        json: bool,
    },
    /// Keep an account for your own sessions.
    ///
    /// A reserved account stays the default for the sessions you start and
    /// is never picked for one the orchestrator or a script starts.
    Reserve { name: String },
    /// Let scripts pick an account you reserved again.
    Unreserve { name: String },
    /// Add a profile.
    ///
    /// Without --use-default it gets a home of its own, empty until
    /// `kete profile login <name>` signs it in.
    Add {
        /// Letters, digits, `-` and `_`; unique across harnesses.
        name: String,
        /// The harness it is an account for: claude, codex, copilot, or
        /// any other harness id (which then only gets --env).
        #[arg(long, value_name = "HARNESS")]
        harness: String,
        /// Adopt the CLI's existing default login (`~/.claude`, `~/.codex`,
        /// …) instead of a home of its own: no dir is moved, nothing is
        /// copied.
        #[arg(long)]
        use_default: bool,
        /// Make it the harness's default: launches that pick no profile
        /// take it.
        #[arg(long)]
        default: bool,
        /// Extra variable for its sessions, `KEY=VALUE`; repeatable.
        #[arg(long, value_name = "KEY=VALUE")]
        env: Vec<String>,
    },
    /// Remove a profile. Its home (and the login in it) stays unless
    /// --purge.
    Remove {
        name: String,
        /// Delete the profile's home too.
        #[arg(long)]
        purge: bool,
    },
    /// Rename a profile: its home moves to the new name, and every
    /// project default naming it follows.
    ///
    /// The sessions on it follow the new name too, archived ones included;
    /// a running one picks up the moved home when it restarts. Against a
    /// daemon too old to move them (before protocol 60) it is refused while
    /// a session (running, or stopped and able to resume) runs on it.
    Rename {
        /// The profile's name now.
        old: String,
        /// Letters, digits, `-` and `_`; unique across harnesses.
        new: String,
    },
    /// Add a profile as a copy of another: its harness, variables and kind
    /// of login, but not its default star or its switch, in a home of its
    /// own (sign it in separately).
    Duplicate {
        /// The profile to copy.
        from: String,
        /// The new profile's name.
        name: String,
    },
    /// Set the account spread: which account a launch naming none runs on.
    ///
    /// manual: the harness's default profile. failover: the default until
    /// its 5h or weekly use reaches --threshold percent, then the next
    /// enabled profile. least-used: the enabled profile with the least use.
    Spread {
        /// manual, failover or least-used.
        #[arg(value_name = "MODE")]
        mode: String,
        /// Percent at which failover moves on: 50, 70, 80, 90 or 95.
        #[arg(long, value_name = "N")]
        threshold: Option<String>,
    },
    /// Change a profile. The name stays: it names the profile's home. Only
    /// what is given changes; --env replaces the variables, --clear-env
    /// drops them.
    Edit {
        name: String,
        /// The harness it is an account for.
        #[arg(long, value_name = "HARNESS")]
        harness: Option<String>,
        /// Adopt the CLI's existing default login.
        #[arg(long, conflicts_with = "own_home")]
        use_default: bool,
        /// Back to a home of its own under the data dir.
        #[arg(long)]
        own_home: bool,
        /// Make it the harness's default.
        #[arg(long)]
        default: bool,
        /// Replace its variables, `KEY=VALUE`; repeatable.
        #[arg(long, value_name = "KEY=VALUE", conflicts_with = "clear_env")]
        env: Vec<String>,
        /// Drop every variable it sets.
        #[arg(long)]
        clear_env: bool,
    },
    /// Make a profile its harness's default.
    Default { name: String },
    /// Make a profile the default account of its harness for one project:
    /// sessions started there naming no profile run under it, ahead of the
    /// account spread and the harness's default.
    ///
    /// The project is its name, its id, or a path inside one of its
    /// checkouts (`.` works). --clear <harness> takes the project's default
    /// for that harness away.
    SetProject {
        /// The project: name, id, or a checkout path.
        project: String,
        /// The profile; its harness is the one it becomes the default of.
        #[arg(required_unless_present = "clear", conflicts_with = "clear")]
        profile: Option<String>,
        /// Clear the project's default account for this harness.
        #[arg(long, value_name = "HARNESS")]
        clear: Option<String>,
    },
    /// Switch a profile on: launches that pick no profile may take it again.
    Enable { name: String },
    /// Switch a profile off: launches that pick no profile skip it (no
    /// default, no spread), and pickers leave it out. Sessions already
    /// under it keep it.
    Disable { name: String },
    /// Sign a profile in: runs `claude auth login --claudeai`, `codex
    /// login` or `copilot login` here, under the profile's environment.
    ///
    /// The CLI opens the browser for its OAuth flow (or prints the URL and
    /// asks for the code); the credentials land in the profile's home.
    /// Anything after `--` is passed on: `kete profile login max2 -- --email
    /// you@example.com`.
    Login {
        name: String,
        /// Extra arguments for the harness's sign-in.
        #[arg(last = true, value_name = "ARGS")]
        args: Vec<String>,
    },
}

/// `kete studio`'s subcommands (0.60, the queue lane's; `note` is the
/// triage lane's).
#[derive(Subcommand)]
pub(crate) enum StudioCommand {
    /// Turn studio mode on for a repo, at autonomy A1 (Plan).
    ///
    /// Writes the repo's entry in this machine's `config.local.json`: the
    /// inbox folder (the manifest's `inbox` unless `--inbox` names one)
    /// and autonomy A1.
    Enable {
        /// The project (id or name).
        project: String,
        /// The inbox folder, relative to the repo root.
        #[arg(long, value_name = "DIR")]
        inbox: Option<String>,
    },
    /// Turn studio mode off for a repo.
    Disable {
        /// The project (id or name).
        project: String,
    },
    /// Each studio repo's autonomy, inbox, counts and queue state.
    Status {
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Print a repo's autonomy level, or set it.
    ///
    /// A0 Hold refuses new sessions in the repo; A1 Plan (the default)
    /// denies pushes, pull requests, ships and tags; A2 Deliver denies tags
    /// and releases; A3 Release denies nothing. Only you change it, from
    /// your own shell or the apps.
    Autonomy {
        /// The project (id or name).
        project: String,
        /// A0, A1, A2 or A3; left out, the current level is printed.
        level: Option<String>,
    },
    /// Leave inbox notes alone, by id or `about` value.
    Exclude {
        /// The project (id or name).
        project: String,
        /// A note's id, or an `about` value.
        value: String,
    },
    /// Hand excluded notes back to the queue.
    Include {
        /// The project (id or name).
        project: String,
        /// A note's id, or an `about` value.
        value: String,
    },
    /// What the queue runner would send the orchestrator now.
    ///
    /// `--now` sends it, when every condition for an idle note holds.
    Queue {
        /// One project only (id or name).
        #[arg(long)]
        project: Option<String>,
        /// Send the idle note now, if the conditions hold.
        #[arg(long)]
        now: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// File one inbox note, and hand it to the triage desk.
    ///
    /// The note's id is the date and the first words; a note that repeats
    /// an open one is refused (exit 3).
    Note {
        /// The note; multiple words need no quotes.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
        /// The project whose inbox it goes in (id or name).
        #[arg(long)]
        project: Option<String>,
        /// Who it is for: a recipient, a department slug, or jay.
        #[arg(long)]
        to: Option<String>,
        /// What it is about.
        #[arg(long)]
        about: Option<String>,
        /// high, normal (default) or low.
        #[arg(long, value_parser = ["high", "normal", "low"])]
        priority: Option<String>,
        /// Send it to the triage desk to route.
        #[arg(long)]
        triage: bool,
        /// Print JSON instead of a line.
        #[arg(long)]
        json: bool,
    },
}

/// `kete roles`'s subcommands (0.60, the roles lane's).
#[derive(Subcommand)]
pub(crate) enum RolesCommand {
    /// What each role gets: its flags and settings file.
    ///
    /// Read from the repo's committed `.kete/roles.toml`, exactly as a
    /// launch would apply it.
    Check {
        /// The project (id or name; default: the one you are in).
        #[arg(long)]
        project: Option<String>,
        /// One role only.
        #[arg(long)]
        role: Option<String>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Answer Claude's bypass-mode dialog for an account, yourself.
    ///
    /// Runs Claude in bypass mode under the account, here, so you accept
    /// its one-time dialog; unattended roles run only on accounts that did.
    /// Refused inside a kete session.
    AcceptBypass {
        /// The account (a PROFILE's name).
        profile: String,
    },
}

const SETUP_EXAMPLES: &str = "\
Examples:
  kete setup terminal --dry-run  say what it would do, and the diff, do nothing
  kete setup terminal            install the font, add the profile
  kete setup terminal --no-font  only the profile";

#[derive(Subcommand)]
pub(crate) enum SetupCommand {
    /// Install JetBrainsMono Nerd Font (per user) and add a "Kete" profile
    /// to Windows Terminal, backing its settings.json up first.
    Terminal {
        /// Say what would be downloaded, installed and written, and do none
        /// of it.
        #[arg(long)]
        dry_run: bool,
        /// Leave the font alone; only add the profile.
        #[arg(long)]
        no_font: bool,
        /// The Windows Terminal settings.json to edit instead of the one
        /// Windows Terminal reads, a copy, to try it on. Hidden: it is
        /// for tests.
        #[arg(long, value_name = "PATH", hide = true)]
        wt_settings: Option<std::path::PathBuf>,
    },
}

const SESSION_EXAMPLES: &str = "\
Examples:
  kete session move-account fix-auth max2
                       restart it fresh on max2
  kete session move-account fix-auth --own-login
                       back on its harness's own login";

#[derive(Subcommand)]
pub(crate) enum SessionCommand {
    /// Restart a session on another account of its harness.
    ///
    /// The session boots again FRESH in the same worktree: each account
    /// keeps its conversations in its own config dir, so the old
    /// conversation stays with the old account. Refused for an archived or
    /// Cloud session, a profile of another harness, and one not defined.
    #[command(after_help = SESSION_EXAMPLES)]
    MoveAccount {
        /// The session: its id, its name, or part of one name.
        #[arg(value_name = "NAME")]
        session: String,
        /// The profile to run on.
        #[arg(required_unless_present = "own_login", conflicts_with = "own_login")]
        profile: Option<String>,
        /// Run on the harness's own login, no profile.
        #[arg(long)]
        own_login: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum ConfigCommand {
    /// Print where each settings file lives.
    ///
    /// `NEBULA_CONFIG_FILE` moves `config.json` alone, into a dotfiles
    /// checkout, say; `NEBULA_DATA_DIR` moves them all.
    #[command(after_help = "Example:\n  kete config path")]
    Path,
    /// Write this machine's settings to one JSON file.
    ///
    /// Carries `config.json`, the agent presets and the ssh host list, never
    /// `config.local.json`. Keys and presets this build doesn't know are
    /// carried as they are, so a newer kete's settings survive the trip.
    #[command(
        after_help = "Examples:\n  kete config export > nebula-settings.json\n  \
                            kete config export ~/backups   writes ~/backups/nebula-settings.json"
    )]
    Export {
        /// File, or existing folder, to write (default: stdout; `-` too).
        #[arg(value_name = "PATH")]
        path: Option<String>,
    },
    /// Merge a settings backup into this machine's settings.
    ///
    /// Takes an export, a bare `config.json`, `agent_presets.json` or
    /// `ssh_hosts.json`, a folder holding any of them, or `-` for stdin. Keys
    /// the file sets replace this machine's and keys it lacks are left alone;
    /// presets merge by name and hosts by destination. `config.local.json` is
    /// never written, and still wins.
    #[command(
        after_help = "Examples:\n  kete config import nebula-settings.json\n  \
                            kete config import ~/dotfiles/nebula   a folder holding config.json\n  \
                            kete config import --dry-run backup.json   what it would change"
    )]
    Import {
        /// The file, the folder, or `-` for stdin.
        #[arg(value_name = "SOURCE")]
        source: String,
        /// Say what the import would change, and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Print the effective harness registry: every harness kete knows (
    /// the built-ins, `custom_harnesses` entries and `harnesses` map ids)
    /// with the program, flags, resume shape, hook dialect and defaults a
    /// launch actually uses. Copy a row into config.json `harnesses` to
    /// override it.
    #[command(after_help = "Example:\n  kete config harnesses")]
    Harnesses,
}
