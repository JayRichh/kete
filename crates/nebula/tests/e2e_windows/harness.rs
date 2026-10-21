//! The Windows TUI harness: `kete` in a ConPTY of its own, driven with
//! literal key bytes (ConPTY turns them into the console key records the
//! TUI reads) and read back through vt100.
//!
//! Every harness owns a temp root: the runtime and data dirs (never the
//! user's), a home, the repos, and the daemon the TUI starts there.

use super::agent;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const COLS: u16 = 220;
pub const ROWS: u16 = 50;
/// How long one screen state may take to come up: generous, as a cold
/// CI runner starts a daemon, git worktrees and agents.
pub const WAIT: Duration = Duration::from_secs(30);
const POLL_STEP: Duration = Duration::from_millis(50);

pub const ENTER: &[u8] = b"\r";
pub const ESC: &[u8] = &[0x1b];
pub const SHIFT_RIGHT: &[u8] = b"\x1b[1;2C";

type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

/// The home variables every child gets, each under the harness's home:
/// `HOME` for Kete and Git, `USERPROFILE` for Node's `os.homedir()`
/// (where Claude Code finds `.claude` and `.claude.json`), and AppData.
pub const HOME_VARS: [&str; 4] = ["HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA"];
/// Variables naming a config dir a real login lives in, dropped from
/// every child so a shell with a Kete profile or a CLI login lends none.
pub const LOGIN_DIR_VARS: &[&str] = &[
    nebula_core::env::CLAUDE_CONFIG_DIR,
    nebula_core::env::CODEX_HOME,
    nebula_core::profiles::GH_CONFIG_DIR,
    nebula_core::env::CONFIG_FILE,
    "XDG_CONFIG_HOME",
];
/// Variables that are a login themselves, dropped from every child.
pub const LOGIN_TOKEN_VARS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "GH_TOKEN",
    "GITHUB_TOKEN",
];

/// [`HOME_VARS`] under `base`, in order.
fn home_values(base: &Path) -> [PathBuf; 4] {
    let home = base.join("home");
    [
        home.clone(),
        home.clone(),
        home.join("AppData").join("Roaming"),
        home.join("AppData").join("Local"),
    ]
}

/// A `kete` command run beside the TUI, isolated as the TUI is.
fn kete_command(base: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_kete"));
    for (var, value) in HOME_VARS.iter().zip(home_values(base)) {
        cmd.env(var, value);
    }
    for var in LOGIN_DIR_VARS.iter().chain(LOGIN_TOKEN_VARS) {
        cmd.env_remove(var);
    }
    cmd
}

pub struct Tui {
    writer: Writer,
    parser: Arc<Mutex<vt100::Parser>>,
    /// Every byte the TUI's ConPTY sent, for replaying a screen that went
    /// wrong outside the run.
    raw: Arc<Mutex<Vec<u8>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Held, not dropped: dropping it closes the pseudoconsole under the
    /// TUI. Kept so the host's size can be changed under it.
    master: Box<dyn portable_pty::MasterPty + Send>,
    /// What [`Tui::restart`] launches the next TUI with.
    launch: Launch,
    pub runtime_dir: PathBuf,
    pub data_dir: PathBuf,
    pub repos: PathBuf,
    pub agent_log: PathBuf,
    pub echo_log: PathBuf,
    gates: PathBuf,
    _root: tempfile::TempDir,
}

pub struct Options {
    /// The fake agent's script (see `agent`).
    pub script: String,
    /// Scripts by the prompt's first word, over `script`.
    pub scripts: Vec<(&'static str, String)>,
    /// Directories dropped from PATH, for a TUI that must not find a tool.
    pub strip_path: Vec<String>,
    /// Start from a data dir an older release left, with its repos
    /// (`upgrade::seed`), instead of an empty one: `v0.48`.
    pub fixture: Option<&'static str>,
}

/// How a TUI is started in a harness's dirs.
#[derive(Clone)]
struct Launch {
    base: PathBuf,
    options_env: Vec<(String, String)>,
    strip_path: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            script: "stop;echo".into(),
            scripts: Vec::new(),
            strip_path: Vec::new(),
            fixture: None,
        }
    }
}

impl Tui {
    pub fn spawn(options: Options) -> Self {
        let root = tempfile::Builder::new()
            .prefix("kete-e2e-")
            .tempdir()
            .unwrap();
        let base = root.path().canonicalize().unwrap();
        let base = PathBuf::from(
            base.display()
                .to_string()
                .trim_start_matches(r"\\?\")
                .to_string(),
        );
        let runtime_dir = base.join("rt");
        let data_dir = base.join("data");
        let repos = base.join("repos");
        for dir in [&runtime_dir, &data_dir, &repos]
            .into_iter()
            .cloned()
            .chain(home_values(&base))
        {
            std::fs::create_dir_all(dir).unwrap();
        }
        // The look the assertions read (the indexed default theme, no file
        // icons, no tips), and the fake agent as the `claude` harness's program:
        // the quick prompt launches Claude, and the daemon installs
        // Claude's hooks for it and passes it Claude's argv.
        let me = std::env::current_exe().unwrap();
        if let Some(name) = options.fixture {
            super::upgrade::seed(&base, name, &me);
        } else {
            std::fs::write(
                data_dir.join(nebula_core::paths::CONFIG_FILE_NAME),
                serde_json::json!({
                    "theme": "default",
                    "icons": "off",
                    "quick_prompt_new_worktree": true,
                    // No tips or What's new page over the screens the tests read.
                    "show_tips": false,
                    "whats_new_seen": "999.0",
                    "harnesses": { "claude": { "program": me.display().to_string() } },
                })
                .to_string(),
            )
            .unwrap();
        }

        let agent_log = base.join("agent.log");
        let echo_log = base.join("echo.log");
        let gates = base.join("gates");
        std::fs::create_dir_all(&gates).unwrap();
        let mut options_env = vec![
            (agent::SCRIPT.to_string(), options.script.clone()),
            (agent::LOG.to_string(), agent_log.display().to_string()),
            (agent::ECHO_LOG.to_string(), echo_log.display().to_string()),
            (agent::GATES.to_string(), gates.display().to_string()),
        ];
        for (word, script) in &options.scripts {
            options_env.push((
                format!("{}_{}", agent::SCRIPT, word.to_ascii_uppercase()),
                script.clone(),
            ));
        }
        let launch = Launch {
            base: base.clone(),
            options_env,
            strip_path: options.strip_path.clone(),
        };
        let Started {
            writer,
            parser,
            raw,
            child,
            master,
        } = launch.start();
        Self {
            writer,
            parser,
            raw,
            child,
            master,
            launch,
            runtime_dir,
            data_dir,
            repos,
            agent_log,
            echo_log,
            gates,
            _root: root,
        }
    }

    /// The harness's home: what `HOME` and `USERPROFILE` are for the TUI,
    /// its daemon and [`Tui::kete`].
    pub fn home(&self) -> PathBuf {
        self.launch.base.join("home")
    }

    /// `kete <args>` run from the command line beside this TUI: its dirs
    /// and home, the fake agent's environment (a daemon it has to start
    /// runs the fake too), and never the machine's Task Scheduler.
    pub fn kete(&self, args: &[&str]) -> std::process::Output {
        self.kete_with(args, &[])
    }

    /// [`Tui::kete`] with `env` on top: `NEBULA_AGENT_ID` names the
    /// session an orchestrator verb runs as.
    pub fn kete_with(&self, args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
        let mut cmd = kete_command(&self.launch.base);
        cmd.args(args)
            .env(nebula_core::env::RUNTIME_DIR, &self.runtime_dir)
            .env(nebula_core::env::DATA_DIR, &self.data_dir)
            .env(nebula_core::env::WAKE_NO_TASK, "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .env(agent::ROLE, "1")
            .stdin(std::process::Stdio::null());
        for var in nebula_core::env::AGENT_SESSION_VARS {
            cmd.env_remove(var);
        }
        for (key, value) in &self.launch.options_env {
            cmd.env(key, value);
        }
        cmd.envs(env.iter().copied());
        cmd.output().unwrap()
    }

    /// Quit this TUI (its daemon stays up, as it does when a user quits)
    /// and start a fresh one in the same dirs: the next launch after an
    /// upgrade.
    pub fn restart(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let started = self.launch.start();
        self.writer = started.writer;
        self.parser = started.parser;
        self.raw = started.raw;
        self.child = started.child;
        self.master = started.master;
    }

    /// A committed repo named `name` with a bare repo beside it as its
    /// `origin` (its `main` pushed), so the base is `origin/main`.
    pub fn make_repo(&self, name: &str) -> PathBuf {
        self.make_repo_with_origin(name, false)
    }

    /// `make_repo`, with `origin` named as a GitHub URL so Kete takes the
    /// repo for a GitHub one, while pushes go to the local bare repo
    /// (`pushInsteadOf`): nothing reaches the network but what asks
    /// GitHub itself.
    pub fn make_github_repo(&self, name: &str) -> PathBuf {
        self.make_repo_with_origin(name, true)
    }

    fn make_repo_with_origin(&self, name: &str, github: bool) -> PathBuf {
        let repo = self.repos.join(name);
        let bare = self.repos.join(format!("{name}-origin.git"));
        std::fs::create_dir_all(&repo).unwrap();
        git(
            &self.repos,
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                &bare.display().to_string(),
            ],
        );
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "e2e@kete.dev"]);
        git(&repo, &["config", "user.name", "kete-e2e"]);
        git(&repo, &["config", "core.autocrlf", "false"]);
        std::fs::write(repo.join("README.md"), "e2e\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        let bare = bare.display().to_string();
        if github {
            let url = format!("https://github.com/kete-e2e/{name}.git");
            git(&repo, &["remote", "add", "origin", &url]);
            git(
                &repo,
                &["config", &format!("url.{bare}.pushInsteadOf"), &url],
            );
        } else {
            git(&repo, &["remote", "add", "origin", &bare]);
        }
        git(&repo, &["push", "-q", "-u", "origin", "main"]);
        git(&repo, &["remote", "set-head", "origin", "main"]);
        repo
    }

    /// Let the agents waiting on `gate:<name>` go on.
    pub fn open_gate(&self, name: &str) {
        std::fs::write(self.gates.join(name), "").unwrap();
    }

    /// Narrow the host by a column and widen it back, as a user dragging
    /// the window would: the TUI repaints every cell (`on_host_resize`),
    /// so a screen that heals after this had lost cells on the way to the
    /// host, not in the TUI's own state.
    pub fn host_repaint(&self) {
        for cols in [COLS - 1, COLS] {
            self.master
                .resize(PtySize {
                    rows: ROWS,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .unwrap();
            std::thread::sleep(Duration::from_millis(300));
        }
    }

    /// Write every byte the host was sent so far to a file that outlives
    /// the run, and say where.
    pub fn save_raw(&self, name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("kete-e2e-{name}-{}.vt", std::process::id()));
        let _ = std::fs::write(&path, &*self.raw.lock().unwrap());
        path
    }

    pub fn send(&self, bytes: &[u8]) {
        let mut w = self.writer.lock().unwrap();
        w.write_all(bytes).unwrap();
        w.flush().unwrap();
    }

    pub fn type_str(&self, s: &str) {
        self.send(s.as_bytes());
    }

    pub fn screen_text(&self) -> String {
        screen_to_text(self.parser.lock().unwrap().screen())
    }

    pub fn wait_for(&self, what: &str, pred: impl Fn(&vt100::Screen) -> bool) {
        let deadline = Instant::now() + WAIT;
        loop {
            if pred(self.parser.lock().unwrap().screen()) {
                return;
            }
            if Instant::now() > deadline {
                panic!(
                    "timed out waiting for: {what}\n--- screen ---\n{}\n--- tui.log tail ---\n{}\n--- agent.log ---\n{}",
                    self.screen_text(),
                    tail(&self.data_dir.join("state").join("tui.log"), 40),
                    tail(&self.agent_log, 40),
                );
            }
            std::thread::sleep(POLL_STEP);
        }
    }

    /// Print the screen, for working out what a new flow shows.
    #[allow(dead_code)]
    pub fn dump(&self, label: &str) {
        println!("--- {label} ---\n{}", self.screen_text());
    }

    pub fn wait_for_text(&self, needle: &str) {
        self.wait_for(&format!("text {needle:?}"), |s| {
            screen_to_text(s).contains(needle)
        });
    }

    pub fn wait_for_gone(&self, needle: &str) {
        self.wait_for(&format!("text {needle:?} to disappear"), |s| {
            !screen_to_text(s).contains(needle)
        });
    }

    pub fn try_wait_for_text(&self, needle: &str, window: Duration) -> bool {
        let deadline = Instant::now() + window;
        loop {
            if self.screen_text().contains(needle) {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(POLL_STEP);
        }
    }

    /// `wait_for` that gives up after `window`, saying whether it held.
    pub fn try_wait_for(&self, window: Duration, pred: impl Fn(&vt100::Screen) -> bool) -> bool {
        let deadline = Instant::now() + window;
        loop {
            if pred(self.parser.lock().unwrap().screen()) {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(POLL_STEP);
        }
    }

    /// Wait until the agent's log has a line containing `needle`.
    pub fn wait_for_agent(&self, needle: &str) {
        self.wait_for(&format!("agent log {needle:?}"), |_| {
            std::fs::read_to_string(&self.agent_log).is_ok_and(|t| t.contains(needle))
        });
    }

    /// The session id of the fake agent that took `prompt`, off its log:
    /// the process that logged the prompt, and the id it logged at boot.
    pub fn agent_id(&self, prompt: &str) -> String {
        let log = std::fs::read_to_string(&self.agent_log).unwrap();
        let pid_of = |line: &str| line.split(']').next().unwrap_or_default().to_string();
        let pid = log
            .lines()
            .find(|l| l.contains(&format!("prompt {prompt:?}")))
            .map(pid_of)
            .unwrap_or_else(|| panic!("no agent took {prompt:?}:\n{log}"));
        log.lines()
            .filter(|l| pid_of(l) == pid)
            .find_map(|l| l.split("] agent ").nth(1)?.split(' ').next())
            .expect("the agent logs its id at boot")
            .to_string()
    }

    /// Where `needle` first appears: (row, col) in cells.
    pub fn find(&self, needle: &str) -> Option<(u16, u16)> {
        screen_to_text(self.parser.lock().unwrap().screen())
            .lines()
            .enumerate()
            .find_map(|(row, line)| {
                let at = line.find(needle)?;
                Some((row as u16, line[..at].chars().count() as u16))
            })
    }

    /// Open the project at `path` with `o`, and wait for its name.
    pub fn add_project(&self, path: &Path, name: &str) {
        self.send(b"o");
        self.wait_for_text("Open project");
        // Typed in one write, the path reads as a paste (the TUI's PASTE
        // BURST): let it land before Enter, or the Enter joins the paste.
        let typed = path.display().to_string();
        self.type_str(&typed);
        // A long path shows as `> …` plus its tail, so wait on the tail.
        let tail: String = {
            let chars: Vec<char> = typed.chars().collect();
            chars[chars.len().saturating_sub(40)..].iter().collect()
        };
        self.wait_for_text(&tail);
        self.send(ENTER);
        self.wait_for_gone("Open project");
        self.wait_for_text(name);
    }
}

impl Launch {
    /// `kete` in a fresh ConPTY over these dirs, and the thread reading it
    /// into a screen.
    fn start(&self) -> Started {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let base = &self.base;
        let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_kete"));
        cmd.env(nebula_core::env::RUNTIME_DIR, base.join("rt"));
        cmd.env(nebula_core::env::DATA_DIR, base.join("data"));
        cmd.env(nebula_core::env::WORKTREE_SYNC_MS, "100");
        cmd.env(nebula_core::env::UPDATE_CHECK_SECS, "0");
        cmd.env(nebula_core::env::LOG, "debug");
        for (var, value) in HOME_VARS.iter().zip(home_values(base)) {
            cmd.env(var, value);
        }
        for var in LOGIN_DIR_VARS.iter().chain(LOGIN_TOKEN_VARS) {
            cmd.env_remove(var);
        }
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        cmd.env("GCM_INTERACTIVE", "never");
        cmd.env_remove("NO_COLOR");
        cmd.env_remove("FORCE_COLOR");
        for var in nebula_core::env::AGENT_SESSION_VARS {
            cmd.env_remove(var);
        }
        cmd.env(agent::ROLE, "1");
        for (key, value) in &self.options_env {
            cmd.env(key, value);
        }
        if !self.strip_path.is_empty() {
            let path = std::env::var("PATH").unwrap_or_default();
            let kept: Vec<&str> = path
                .split(';')
                .filter(|dir| {
                    !self
                        .strip_path
                        .iter()
                        .any(|gone| dir.trim_end_matches('\\').eq_ignore_ascii_case(gone))
                })
                .collect();
            cmd.env("PATH", kept.join(";"));
        }
        cmd.cwd(base.join("repos"));
        let child = pty.slave.spawn_command(cmd).unwrap();
        drop(pty.slave);

        let mut reader = pty.master.try_clone_reader().unwrap();
        let writer: Writer = Arc::new(Mutex::new(pty.master.take_writer().unwrap()));
        let master = pty.master;

        let parser = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLS, 0)));
        let raw = Arc::new(Mutex::new(Vec::new()));
        {
            let parser = parser.clone();
            let raw = raw.clone();
            let writer = writer.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 16384];
                let mut tail: Vec<u8> = Vec::new();
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    raw.lock().unwrap().extend_from_slice(&buf[..n]);
                    let mut parser = parser.lock().unwrap();
                    parser.process(&buf[..n]);
                    // ConPTY asks where the cursor is before it draws a
                    // thing (and the TUI may ask again): answer as a
                    // terminal would, or it waits.
                    tail.extend_from_slice(&buf[..n]);
                    let asks = tail.windows(4).filter(|w| w == b"\x1b[6n").count();
                    if asks > 0 {
                        let (row, col) = parser.screen().cursor_position();
                        let reply = format!("\x1b[{};{}R", row + 1, col + 1);
                        let mut w = writer.lock().unwrap();
                        for _ in 0..asks {
                            let _ = w.write_all(reply.as_bytes());
                        }
                        let _ = w.flush();
                        tail.clear();
                    } else if tail.len() > 3 {
                        tail.drain(..tail.len() - 3);
                    }
                }
            });
        }
        Started {
            writer,
            parser,
            raw,
            child,
            master,
        }
    }
}

/// One launched TUI: what [`Tui::restart`] replaces together.
struct Started {
    writer: Writer,
    parser: Arc<Mutex<vt100::Parser>>,
    raw: Arc<Mutex<Vec<u8>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = kete_command(&self.launch.base)
            .arg("kill")
            .env(nebula_core::env::RUNTIME_DIR, &self.runtime_dir)
            .env(nebula_core::env::DATA_DIR, &self.data_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        // The daemon and its agents let go of the dirs a moment after.
        std::thread::sleep(Duration::from_millis(300));
    }
}

pub fn screen_to_text(screen: &vt100::Screen) -> String {
    let (rows, cols) = screen.size();
    let mut out = String::new();
    for row in 0..rows {
        for col in 0..cols {
            match screen.cell(row, col) {
                Some(cell) if !cell.contents().is_empty() => out.push_str(cell.contents()),
                Some(cell) if cell.is_wide_continuation() => {}
                _ => out.push(' '),
            }
        }
        out.push('\n');
    }
    out
}

/// An SGR mouse report at 0-based `col`,`row`.
pub fn sgr_mouse(button: u16, col: u16, row: u16, release: bool) -> Vec<u8> {
    format!(
        "\x1b[<{button};{};{}{}",
        col + 1,
        row + 1,
        if release { 'm' } else { 'M' }
    )
    .into_bytes()
}

pub fn git(cwd: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed in {}: {}",
        cwd.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn tail(path: &Path, lines: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// The daemon's ring of what session `agent` printed, read over its pipe
/// as a client would (`TailOutput`): (cols, rows, bytes).
pub fn daemon_tail(runtime_dir: &Path, agent: &str) -> (u16, u16, Vec<u8>) {
    use nebula_core::codec::{read_frame, write_frame};
    use nebula_core::{AgentId, ClientRequest, ServerEvent, SessionRef, PROTOCOL_VERSION};
    let pipe = nebula_core::paths::pipe_name_for_socket(&runtime_dir.join("daemon.sock"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut stream = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(&pipe)
            .unwrap();
        let hello = ClientRequest::Hello {
            protocol_version: PROTOCOL_VERSION,
        };
        write_frame(&mut stream, &hello).await.unwrap();
        let ask = ClientRequest::TailOutput {
            req_id: 7,
            session: SessionRef::Agent(AgentId(agent.into())),
            max_bytes: 1 << 20,
            after_seq: None,
        };
        write_frame(&mut stream, &ask).await.unwrap();
        loop {
            match read_frame::<ServerEvent, _>(&mut stream).await.unwrap() {
                Some(ServerEvent::OutputTail { tail, .. }) => {
                    let tail = tail.expect("the session's ring");
                    return (tail.cols, tail.rows, tail.data);
                }
                Some(_) => continue,
                None => panic!("the daemon hung up"),
            }
        }
    })
}
