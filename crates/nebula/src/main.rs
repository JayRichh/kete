mod ado;
mod browser;
mod cli;
mod desktop;
mod editors;
mod guard;
mod hook;
mod orchestrator_renew;
mod profile;
mod roles;
mod setup;
mod ship;
mod ssh;
mod statusline;
mod studio;
mod studio_note;
mod tunnel;
mod upgrade;
mod usage;
mod wake;

use anyhow::Result;
use cli::{
    Command, ConfigCommand, EditorsCommand, OrchestratorCommand, ProfileCommand, SessionCommand,
    SetupCommand, StudioCommand, WakeAction,
};

use std::path::Path;

fn main() -> Result<()> {
    let cli = cli::parse_as_invoked();
    // A Claude Code too old for exec-form hooks (before 2.1.139) ignores a
    // hook's `args` and runs its `command` — this binary — bare, with the
    // payload on a pipe. Inside a session that is never someone asking for
    // the TUI, which would sit on the hook until it timed out: exit quietly,
    // as `_hook` does where it has nothing to do (`hooks::installer`).
    if cli.command.is_none() && cli.dir.is_none() && run_bare_by_a_hook() {
        return Ok(());
    }
    // Kete was Bro: carry Bro's data and runtime dirs over before anything
    // opens a file in them. A no-op once moved, so the note shows once.
    if opens_state(&cli.command) {
        for note in nebula_core::paths::migrate_legacy_dirs() {
            eprintln!("kete: {note}");
        }
    }
    // A `nebula ssh` / `nebula tunnel` from another machine may have sent its
    // settings along. Merge them before anything reads a setting, and before
    // a thread or a child exists to inherit the variable.
    nebula_tui::bundle::apply_forwarded();
    // Likewise before any thread or child: nothing `nebula` starts — the
    // daemon and everything it runs, or the TUI's own helpers — may
    // inherit the Claude Code session it was typed in.
    let dropped = nebula_core::env::scrub_host_claude_session();
    match cli.command {
        Some(Command::Daemon { foreground, adopt }) => {
            init_daemon_logging(foreground)?;
            if !dropped.is_empty() {
                tracing::info!(vars = ?dropped, "dropped the inherited Claude Code session env");
            }
            log_fatal(
                nebula_daemon::run_daemon(nebula_daemon::DaemonOpts { foreground, adopt }),
                &nebula_core::paths::daemon_log_path(),
            )
        }
        Some(Command::Reload) => upgrade::run_reload(),
        Some(Command::Add { path }) => nebula_tui::run_add_project(path),
        Some(Command::Config { command }) => nebula_tui::run_config(match command {
            ConfigCommand::Path => nebula_tui::ConfigOp::Path,
            ConfigCommand::Export { path } => nebula_tui::ConfigOp::Export { path },
            ConfigCommand::Import { source, dry_run } => {
                nebula_tui::ConfigOp::Import { source, dry_run }
            }
            ConfigCommand::Harnesses => nebula_tui::ConfigOp::Harnesses,
        }),
        Some(Command::Profile { command }) => profile::run_profile(match command {
            ProfileCommand::List { json } => profile::ProfileOp::List { json },
            ProfileCommand::Pick {
                harness,
                project,
                dry_run,
                json,
            } => profile::ProfileOp::Pick {
                harness,
                project,
                dry_run,
                json,
            },
            ProfileCommand::Reserve { name } => profile::ProfileOp::Reserve(name),
            ProfileCommand::Unreserve { name } => profile::ProfileOp::Unreserve(name),
            ProfileCommand::Add {
                name,
                harness,
                use_default,
                default,
                env,
            } => profile::ProfileOp::Add {
                name,
                harness,
                use_default,
                default,
                env,
            },
            ProfileCommand::Remove { name, purge } => profile::ProfileOp::Remove { name, purge },
            ProfileCommand::Rename { old, new } => profile::ProfileOp::Rename { old, new },
            ProfileCommand::Duplicate { from, name } => {
                profile::ProfileOp::Duplicate { from, name }
            }
            ProfileCommand::Spread { mode, threshold } => {
                profile::ProfileOp::Spread { mode, threshold }
            }
            ProfileCommand::Edit {
                name,
                harness,
                use_default,
                own_home,
                default,
                env,
                clear_env,
            } => profile::ProfileOp::Edit {
                name,
                harness,
                use_default: if use_default {
                    Some(true)
                } else if own_home {
                    Some(false)
                } else {
                    None
                },
                default,
                env: if clear_env {
                    Some(Vec::new())
                } else if env.is_empty() {
                    None
                } else {
                    Some(env)
                },
            },
            ProfileCommand::SetProject {
                project,
                profile,
                clear,
            } => profile::ProfileOp::SetProject {
                project,
                profile,
                clear,
            },
            ProfileCommand::Default { name } => profile::ProfileOp::Default { name },
            ProfileCommand::Enable { name } => profile::ProfileOp::Enable { name, on: true },
            ProfileCommand::Disable { name } => profile::ProfileOp::Enable { name, on: false },
            ProfileCommand::Login { name, args } => profile::ProfileOp::Login { name, args },
        }),
        Some(Command::Wake {
            account,
            all,
            force,
            dry_run,
            mode,
            json,
            action,
        }) => wake::run(match action {
            Some(WakeAction::Set {
                account,
                at,
                days,
                keep_rolling,
                no_keep_rolling,
                skip_idle,
                mode,
                wake_pc,
                no_wake_pc,
            }) => wake::WakeOp::Set(wake::SetArgs {
                account,
                at,
                days,
                keep_rolling: flag(keep_rolling, no_keep_rolling),
                skip_idle,
                mode,
                wake_pc: flag(wake_pc, no_wake_pc),
            }),
            Some(WakeAction::Off { account }) => wake::WakeOp::Off { account },
            Some(WakeAction::Status { json }) => wake::WakeOp::Status { json },
            None => wake::WakeOp::Run {
                account,
                all,
                force,
                dry_run,
                mode,
                json,
            },
        }),
        Some(Command::Kill) => nebula_tui::run_kill(),
        Some(Command::Rename { title, force }) => {
            let mode = if force {
                nebula_tui::RenameMode::Force
            } else {
                nebula_tui::RenameMode::Auto
            };
            nebula_tui::run_rename(title.join(" "), mode)
        }
        Some(Command::Worktree { name, base }) => nebula_tui::run_worktree(name.join(" "), base),
        Some(Command::Spawn {
            task,
            kind,
            worktree,
            base,
            profile,
            role,
        }) => nebula_tui::run_spawn(task.join(" "), kind, worktree, base, profile, role),
        // 0.60: studio mode, roles and the usage ledger, each dispatched to
        // its lane's file.
        Some(Command::Studio { command }) => match command {
            StudioCommand::Note {
                text,
                project,
                to,
                about,
                priority,
                triage,
                json,
            } => studio_note::run(studio_note::NoteArgs {
                text: text.join(" "),
                project,
                to,
                about,
                priority,
                triage,
                json,
            }),
            other => studio::run(other),
        },
        Some(Command::Roles { command }) => roles::run(command),
        Some(Command::Usage {
            since,
            until,
            project,
            by,
            csv,
        }) => usage::run(usage::UsageArgs {
            since,
            until,
            project,
            by,
            csv,
        }),
        Some(Command::Guard { session }) => std::process::exit(guard::run(session)),
        Some(Command::Open { files }) => nebula_tui::run_open(files),
        Some(Command::Sessions { json }) => nebula_tui::run_sessions(json),
        Some(Command::Session { command }) => match command {
            SessionCommand::MoveAccount {
                session,
                profile,
                own_login: _,
            } => nebula_tui::run_move_account(&session, profile),
        },
        Some(Command::Read { session, lines }) => nebula_tui::run_read_session(&session, lines),
        Some(Command::Ask {
            session,
            question,
            timeout,
            no_wait,
        }) => nebula_tui::run_ask_session(&session, question.join(" "), timeout, !no_wait),
        Some(Command::Orchestrator { command }) => match command {
            OrchestratorCommand::List { json } => nebula_tui::run_orchestrator_list(json),
            OrchestratorCommand::Start {
                kind,
                model,
                effort,
                profile,
                project,
                name,
                replace,
                role,
            } => nebula_tui::run_orchestrator_start(nebula_tui::ipc::StartArgs {
                kind,
                model,
                effort,
                profile,
                project,
                name,
                replace,
                role_card: role,
            }),
            OrchestratorCommand::Status { json } => nebula_tui::run_orchestrator_status(json),
            OrchestratorCommand::Clear => nebula_tui::run_orchestrator_clear(),
            OrchestratorCommand::Read {
                session,
                lines,
                bytes,
            } => nebula_tui::run_orchestrator_read(&session, lines, bytes),
            OrchestratorCommand::Send {
                session,
                prompt,
                timeout,
                no_wait,
            } => nebula_tui::run_orchestrator_send(&session, prompt.join(" "), timeout, !no_wait),
            OrchestratorCommand::Spawn {
                project,
                worktree,
                base,
                kind,
                name,
                model,
                effort,
                profile,
                role,
                task,
            } => {
                let task = task.join(" ");
                nebula_tui::run_orchestrator_spawn(nebula_tui::ipc::SpawnArgs {
                    project: &project,
                    worktree: worktree.as_deref(),
                    base,
                    kind,
                    name,
                    model,
                    effort,
                    profile,
                    task: &task,
                    role_card: role,
                })
            }
            OrchestratorCommand::Renew {
                from,
                profile,
                pick,
                model,
                keep_old,
                now,
                json,
            } => orchestrator_renew::run(orchestrator_renew::RenewArgs {
                from,
                profile,
                pick,
                model,
                keep_old,
                now,
                json,
            }),
            OrchestratorCommand::Review { sessions, tabs } => {
                nebula_tui::run_orchestrator_review(&sessions, tabs)
            }
        },
        Some(Command::Ship {
            project,
            worktree,
            command,
        }) => ship::run_ship(command, ado::Target { project, worktree }),
        Some(Command::Ado {
            project,
            worktree,
            command,
        }) => ado::run_ado(command, ado::Target { project, worktree }),
        Some(Command::Browser {
            port,
            bind,
            public,
            credential,
            no_open,
        }) => browser::run_browser(browser::BrowserOpts {
            port,
            // --public is --bind 0.0.0.0 with a name; clap keeps the two
            // from being given at once.
            bind: bind.unwrap_or(if public {
                browser::PUBLIC_BIND
            } else {
                browser::DEFAULT_BIND
            }),
            credential,
            open: !no_open,
        }),
        Some(Command::Ssh {
            host,
            path,
            no_sync_config,
        }) => ssh::run_ssh(&host, path.as_deref(), !no_sync_config),
        Some(Command::Tunnel {
            host,
            path,
            port,
            remote_port,
            no_sync_config,
        }) => tunnel::run_tunnel(tunnel::TunnelOpts {
            host,
            path,
            port,
            remote_port,
            sync_config: !no_sync_config,
        }),
        Some(Command::Setup {
            command:
                SetupCommand::Terminal {
                    dry_run,
                    no_font,
                    wt_settings,
                },
        }) => setup::run_terminal(setup::Options {
            dry_run,
            no_font,
            wt_settings,
        }),
        Some(Command::Desktop { args }) => desktop::run_desktop(&args),
        Some(Command::Editors { command }) => match command {
            EditorsCommand::List { json } => editors::run_list(json),
            EditorsCommand::Open { path, with, line } => {
                editors::run_open(&path, with.as_deref(), line)
            }
        },
        Some(Command::Upgrade { force }) => upgrade::run_upgrade(force),
        Some(Command::StaleDaemonNote) => {
            if nebula_daemon::lifecycle::daemon_is_stale() {
                println!("note: the running daemon was built from older code.");
                if nebula_daemon::handoff::daemon_can_restart() {
                    println!("{}", upgrade::RELOAD_HINT);
                } else {
                    println!("{}", upgrade::KILL_HINT);
                }
            }
            Ok(())
        }
        Some(Command::ProtocolVersion) => {
            println!("{}", nebula_core::PROTOCOL_VERSION);
            Ok(())
        }
        Some(Command::ProtocolMinCompatibleVersion) => {
            println!("{}", nebula_core::MIN_COMPATIBLE_PROTOCOL);
            Ok(())
        }
        Some(Command::RestartVersion) => {
            println!("{}", nebula_daemon::handoff::VERSION);
            Ok(())
        }
        Some(Command::Hook { endpoint, event }) => hook::run_hook(&endpoint, &event),
        Some(Command::Statusline) => statusline::run_statusline(),
        None => match cli.dir {
            Some(dir) => nebula_tui::run_add_project(dir),
            None => {
                init_tui_logging()?;
                if !dropped.is_empty() {
                    tracing::info!(vars = ?dropped, "dropped the inherited Claude Code session env");
                }
                let handoff =
                    log_fatal(nebula_tui::run_tui(), &nebula_core::paths::tui_log_path())?;
                match handoff {
                    // Hosts-picker handoff: the TUI quit and restored the
                    // terminal so a fresh `nebula ssh` can exec over us (the
                    // local daemon and its sessions stay up).
                    Some(entry) => {
                        eprintln!("kete: connecting to {}…", entry.host);
                        ssh::run_ssh(&entry.host, entry.path.as_deref(), true)
                    }
                    None => Ok(()),
                }
            }
        },
    }
}

/// Run with no arguments from inside a Kete session with stdin a pipe:
/// what an exec-form hook looks like to a CLI that drops its `args`.
fn run_bare_by_a_hook() -> bool {
    use std::io::IsTerminal;
    nebula_core::env::non_empty(nebula_core::env::AGENT_ID).is_some()
        && !std::io::stdin().is_terminal()
}

/// A `--x` / `--no-x` pair as a change: None when neither was given.
fn flag(on: bool, off: bool) -> Option<bool> {
    match (on, off) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    }
}

/// Whether `command` reads or writes the data or runtime dirs itself, so
/// Bro's must be carried over first. The rest (a remote's, the desktop
/// window's, a hook relaying to the daemon over its endpoint) leave them
/// to whichever process does.
fn opens_state(command: &Option<Command>) -> bool {
    !matches!(
        command,
        Some(
            Command::Browser { .. }
                | Command::Ssh { .. }
                | Command::Tunnel { .. }
                | Command::Setup { .. }
                | Command::Desktop { .. }
                | Command::Hook { .. }
                // Runs on every shell command a role session makes: it
                // reads its settings and the studio map, nothing to carry.
                | Command::Guard { .. }
                | Command::ProtocolVersion
                | Command::ProtocolMinCompatibleVersion
                | Command::RestartVersion
        )
    )
}

/// Record a fatal top-level error in the log file before it goes to stderr —
/// the TUI's stderr disappears with the terminal, the daemon's is /dev/null.
fn log_fatal<T>(result: Result<T>, log_path: &Path) -> Result<T> {
    if let Err(err) = &result {
        nebula_core::crashlog::append(log_path, &format!("FATAL {err:#}"));
    }
    result
}

fn log_filter() -> tracing_subscriber::EnvFilter {
    tracing_subscriber::EnvFilter::try_from_env(nebula_core::env::LOG)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
}

/// Route tracing to `log_path` (created on demand, appended, no ANSI) —
/// neither binary can log to the terminal: the TUI owns it and the daemon
/// has no stderr.
fn init_file_logging(log_path: &Path) -> Result<()> {
    std::fs::create_dir_all(nebula_core::paths::log_dir())?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    tracing_subscriber::fmt()
        .with_env_filter(log_filter())
        .with_writer(file)
        .with_ansi(false)
        .init();
    Ok(())
}

fn init_daemon_logging(foreground: bool) -> Result<()> {
    // The daemon runs detached with stderr on /dev/null — without this hook a
    // panic (on any thread, tokio workers included) leaves no trace.
    let log_path = nebula_core::paths::daemon_log_path();
    nebula_core::crashlog::install_panic_hook(log_path.clone());
    if foreground {
        tracing_subscriber::fmt()
            .with_env_filter(log_filter())
            .init();
        return Ok(());
    }
    init_file_logging(&log_path)
}

fn init_tui_logging() -> Result<()> {
    // Panic output to stderr dies with the alternate screen — capture it to
    // the log file. The TUI later wraps this hook with its terminal-restore,
    // so the chain on panic is: restore terminal → log to file → stderr.
    let log_path = nebula_core::paths::tui_log_path();
    nebula_core::crashlog::install_panic_hook(log_path.clone());
    // stdout belongs to the UI — log to file only.
    init_file_logging(&log_path)
}
