//! `kete profile`: the PROFILES list from the command line — list, add,
//! duplicate, rename, remove, pick the default and the spread, and sign
//! one in.
//!
//! The list is `config.json`'s `profiles` key (`nebula_core::profiles`),
//! edited as raw JSON so every other key, and every field a newer kete
//! wrote, survives. The daemon reads the list fresh at each spawn; only a
//! rename (which a session naming the profile refuses) and `set-project`
//! (which looks the project up) ask it anything. `login` runs the harness's own sign-in in this
//! terminal with the profile's environment applied, so the credentials it
//! writes land in the PROFILE HOME (`<data dir>/profiles/<name>/`).

use anyhow::{bail, Context, Result};
use nebula_core::profiles::{self, Profile};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What `kete profile` was asked to do.
pub enum ProfileOp {
    /// `--json` (0.60): every account with its windows, live sessions
    /// and holds; the accounts lane's.
    List {
        json: bool,
    },
    /// `kete profile pick` (0.60): the account a scripted launch takes;
    /// the accounts lane's.
    #[allow(dead_code)]
    Pick {
        harness: Option<String>,
        project: Option<String>,
        dry_run: bool,
        json: bool,
    },
    /// `kete profile reserve` (0.60): the accounts lane's.
    #[allow(dead_code)]
    Reserve(String),
    /// `kete profile unreserve` (0.60): the accounts lane's.
    #[allow(dead_code)]
    Unreserve(String),
    Add {
        name: String,
        harness: String,
        use_default: bool,
        default: bool,
        env: Vec<String>,
    },
    Remove {
        name: String,
        purge: bool,
    },
    /// A new name: its home, project defaults and reading move with it.
    Rename {
        old: String,
        new: String,
    },
    /// A copy of `from` under `name`, with a home of its own.
    Duplicate {
        from: String,
        name: String,
    },
    /// The ACCOUNT SPREAD, and with `threshold` failover's percent.
    Spread {
        mode: String,
        threshold: Option<String>,
    },
    /// Change what is given; the name stays (it names the home).
    Edit {
        name: String,
        harness: Option<String>,
        /// Some(true): adopt the CLI's login; Some(false): a home of its own.
        use_default: Option<bool>,
        default: bool,
        /// Some: replace the variables (empty drops them all).
        env: Option<Vec<String>>,
    },
    /// A project's default account for the profile's harness, or with
    /// `clear` (a harness) none.
    SetProject {
        project: String,
        profile: Option<String>,
        clear: Option<String>,
    },
    Default {
        name: String,
    },
    Enable {
        name: String,
        on: bool,
    },
    Login {
        name: String,
        args: Vec<String>,
    },
}

pub fn run_profile(op: ProfileOp) -> Result<()> {
    let base = nebula_core::paths::config_path();
    let local = nebula_core::paths::config_local_path();
    let config = profiles_layer(&base, &local)?;
    // An edit of the local layer also holds config.json's lock: the
    // settings writers (`Config::save`, the window's profile edits) take
    // that one for both layers. The base layer's edit locks it itself.
    let _lock = match &op {
        // A project default edits the `projects` key: its own layer.
        // A rename and a spread take the settings locks themselves.
        ProfileOp::List { .. }
        | ProfileOp::Pick { .. }
        | ProfileOp::Reserve(_)
        | ProfileOp::Unreserve(_)
        | ProfileOp::Login { .. }
        | ProfileOp::SetProject { .. }
        | ProfileOp::Rename { .. }
        | ProfileOp::Spread { .. } => None,
        _ if config == local => Some(
            nebula_core::settings::lock(&base)
                .with_context(|| format!("lock {}", base.display()))?,
        ),
        _ => None,
    };
    match op {
        ProfileOp::List { json: false } => list(),
        // 0.60: placeholders from the foundation; the accounts lane fills them.
        ProfileOp::List { json: true } => crate::not_in_this_build("kete profile list --json"),
        ProfileOp::Pick { .. } => crate::not_in_this_build("kete profile pick"),
        ProfileOp::Reserve(_) => crate::not_in_this_build("kete profile reserve"),
        ProfileOp::Unreserve(_) => crate::not_in_this_build("kete profile unreserve"),
        ProfileOp::Add {
            name,
            harness,
            use_default,
            default,
            env,
        } => {
            check_harness(&harness)?;
            let vars = env_vars(&env)?;
            let profile = Profile {
                name: name.trim().to_string(),
                harness: harness.trim().to_string(),
                env: vars,
                default,
                use_default,
                disabled: false,
                reserved: false,
            };
            profiles::add_to_file(&config, &profile).map_err(anyhow::Error::msg)?;
            match profile.home() {
                Some(home) => {
                    profile
                        .prepare(None)
                        .with_context(|| format!("create {}", home.display()))?;
                    println!(
                        "added profile `{}` ({}), home {}",
                        profile.name,
                        profile.harness,
                        home.display()
                    );
                    if profiles::login_args(&profile.harness).is_some() {
                        println!("sign it in with: kete profile login {}", profile.name);
                    }
                }
                None => println!(
                    "added profile `{}` ({}) on the CLI's default login",
                    profile.name, profile.harness
                ),
            }
            Ok(())
        }
        ProfileOp::Remove { name, purge } => {
            // The home holds the login: kept unless asked, so adding the
            // name back signs nothing in again. Never an adopted login's —
            // that is the CLI's own default config, and not ours.
            let removed =
                profiles::remove_in_file(&config, &profiles::profiles_root(), &name, purge)
                    .map_err(anyhow::Error::msg)?;
            println!("removed profile `{}`", name.trim());
            match removed.home {
                Some(home) if removed.purged => println!("deleted {}", home.display()),
                Some(home) => println!(
                    "its home stays at {} (kete profile remove --purge deletes it)",
                    home.display()
                ),
                None => {}
            }
            Ok(())
        }
        ProfileOp::Rename { old, new } => rename(&base, &local, &old, &new),
        ProfileOp::Duplicate { from, name } => {
            let copy =
                profiles::duplicate_in_file(&config, &from, &name).map_err(anyhow::Error::msg)?;
            match copy.home() {
                Some(home) => {
                    copy.prepare(None)
                        .with_context(|| format!("create {}", home.display()))?;
                    println!(
                        "added profile `{}` ({}), a copy of `{}`, home {}",
                        copy.name,
                        copy.harness,
                        from.trim(),
                        home.display()
                    );
                }
                None => println!(
                    "added profile `{}` ({}), a copy of `{}`, on the CLI's default login",
                    copy.name,
                    copy.harness,
                    from.trim()
                ),
            }
            if let Some(command) = nebula_core::accounts::login_command(Some(&copy), &copy.harness)
            {
                if nebula_core::accounts::login_state(Some(&copy), &copy.harness)
                    != nebula_core::accounts::LoginState::LoggedIn
                {
                    println!("sign it in with: {command}");
                }
            }
            Ok(())
        }
        ProfileOp::Spread { mode, threshold } => spread(&mode, threshold.as_deref()),
        ProfileOp::Edit {
            name,
            harness,
            use_default,
            default,
            env,
        } => {
            let held = nebula_core::settings::read_object(&config)
                .map_err(anyhow::Error::msg)?
                .unwrap_or_default();
            let Some(old) = profiles::from_settings(&held)
                .into_iter()
                .find(|p| p.name.trim() == name.trim())
            else {
                bail!("no profile named `{name}`: kete profile list shows them");
            };
            if let Some(harness) = &harness {
                check_harness(harness)?;
            }
            let profile = edited(old, harness, use_default, default, env.as_deref())?;
            profiles::update_in_file(&config, &profile).map_err(anyhow::Error::msg)?;
            if let Some(home) = profile.home() {
                profile
                    .prepare(None)
                    .with_context(|| format!("create {}", home.display()))?;
            }
            println!("saved profile `{}` ({})", profile.name, profile.harness);
            if let Some(hint) =
                nebula_core::accounts::login_command(Some(&profile), &profile.harness)
            {
                if nebula_core::accounts::login_state(Some(&profile), &profile.harness)
                    == nebula_core::accounts::LoginState::NeedsLogin
                {
                    println!("sign it in with: {hint}");
                }
            }
            Ok(())
        }
        ProfileOp::SetProject {
            project,
            profile,
            clear,
        } => set_project(
            &base,
            &local,
            &project,
            profile.as_deref(),
            clear.as_deref(),
        ),
        ProfileOp::Default { name } => {
            profiles::set_default_in_file(&config, &name).map_err(anyhow::Error::msg)?;
            println!("`{}` is now its harness's default profile", name.trim());
            Ok(())
        }
        ProfileOp::Enable { name, on } => {
            profiles::set_enabled_in_file(&config, &name, on).map_err(anyhow::Error::msg)?;
            println!(
                "`{}` is {}",
                name.trim(),
                if on {
                    "enabled"
                } else {
                    "disabled: launches that pick no profile skip it"
                }
            );
            Ok(())
        }
        ProfileOp::Login { name, args } => login(&name, &args),
    }
}

/// Refuse a harness id the registry does not know.
fn check_harness(harness: &str) -> Result<()> {
    let registry = nebula_tui::config::Config::load().harness_registry();
    if !registry.iter().any(|h| h.id == harness.trim()) {
        bail!(
            "unknown harness `{harness}`: expected one of {}",
            registry
                .iter()
                .map(|h| h.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}

/// `--env` pairs as a map.
fn env_vars(pairs: &[String]) -> Result<BTreeMap<String, String>> {
    let mut vars = BTreeMap::new();
    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            bail!("--env takes KEY=VALUE, not `{pair}`");
        };
        if key.trim().is_empty() {
            bail!("--env `{pair}` names no variable");
        }
        vars.insert(key.trim().to_string(), value.to_string());
    }
    Ok(vars)
}

/// `old` with what `kete profile edit` was given: the name and the
/// switched-off flag always stay, the rest only where given.
fn edited(
    mut old: Profile,
    harness: Option<String>,
    use_default: Option<bool>,
    default: bool,
    env: Option<&[String]>,
) -> Result<Profile> {
    if let Some(harness) = harness {
        old.harness = harness.trim().to_string();
    }
    if let Some(adopt) = use_default {
        old.use_default = adopt;
    }
    if default {
        old.default = true;
    }
    if let Some(env) = env {
        old.env = env_vars(env)?;
    }
    if let Some(problem) = old.problem() {
        bail!("{problem}");
    }
    Ok(old)
}

/// `kete profile set-project`: the project found in the daemon's tree (by
/// name, id or checkout path), its default account for a harness set or
/// cleared under its repo path exactly as the daemon stores it, in the
/// layer whose `projects` key is in force.
fn set_project(
    base: &Path,
    local: &Path,
    query: &str,
    profile: Option<&str>,
    clear: Option<&str>,
) -> Result<()> {
    let (harness, name) = match (profile, clear) {
        (Some(name), _) => {
            let harness = profiles::project_default_harness(&profiles::load(), name)
                .map_err(anyhow::Error::msg)?;
            (harness, Some(name.trim().to_string()))
        }
        (None, Some(harness)) => (harness.trim().to_string(), None),
        (None, None) => bail!("name a profile, or --clear <harness>"),
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (projects, worktrees) = rt.block_on(fetch_projects())?;
    let project = find_project(&projects, &worktrees, query)?;
    let repo = project.repo_path.to_string_lossy().into_owned();
    let layer =
        profiles::layer_for(base, local, profiles::PROJECTS_KEY).map_err(anyhow::Error::msg)?;
    // The local layer's edit also holds config.json's lock, as every
    // settings writer takes that one for both layers.
    let _lock = if layer == local {
        Some(
            nebula_core::settings::lock(base)
                .with_context(|| format!("lock {}", base.display()))?,
        )
    } else {
        None
    };
    profiles::set_project_default_in_file(&layer, &repo, &harness, name.as_deref())
        .map_err(anyhow::Error::msg)?;
    match name {
        Some(name) => println!(
            "{harness} sessions in {} that name no profile now run on `{name}`",
            project.name
        ),
        None => println!(
            "{} has no default {harness} account now: the account spread picks",
            project.name
        ),
    }
    Ok(())
}

/// The daemon's projects and worktrees, over one connection (the daemon
/// is started when none runs, as `kete ado` does).
async fn fetch_projects() -> Result<(Vec<nebula_core::Project>, Vec<nebula_core::Worktree>)> {
    let (projects, worktrees, _) = fetch_tree().await?;
    Ok((projects, worktrees))
}

/// The daemon's projects, worktrees and sessions, from one snapshot.
async fn fetch_tree() -> Result<(
    Vec<nebula_core::Project>,
    Vec<nebula_core::Worktree>,
    Vec<nebula_core::Agent>,
)> {
    use nebula_core::codec::{read_frame, write_frame};
    use nebula_core::{ClientRequest, ServerEvent};
    let mut conn = nebula_tui::ipc::connect_or_spawn().await?;
    write_frame(&mut conn.stream, &ClientRequest::Subscribe).await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Snapshot {
                projects,
                worktrees,
                agents,
                ..
            }) => return Ok((projects, worktrees, agents)),
            Some(_) => continue,
            None => bail!("the daemon closed the connection before sending its projects"),
        }
    }
}

/// The daemon's sessions and the protocol it said at Hello, over the
/// connection a rename then sends `RenameProfileSessions` on.
async fn sessions_and_protocol() -> Result<(nebula_tui::ipc::Connection, Vec<nebula_core::Agent>)> {
    use nebula_core::codec::{read_frame, write_frame};
    use nebula_core::{ClientRequest, ServerEvent};
    let mut conn = nebula_tui::ipc::connect_or_spawn().await?;
    write_frame(&mut conn.stream, &ClientRequest::Subscribe).await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Snapshot { agents, .. }) => return Ok((conn, agents)),
            Some(_) => continue,
            None => bail!("the daemon closed the connection before sending its sessions"),
        }
    }
}

/// Ask the daemon to point every session naming `old` at `new`.
async fn rename_sessions(
    conn: &mut nebula_tui::ipc::Connection,
    old: &str,
    new: &str,
) -> Result<()> {
    use nebula_core::codec::{read_frame, write_frame};
    use nebula_core::{ClientRequest, ServerEvent};
    const REQ_ID: u64 = 1;
    let req = ClientRequest::RenameProfileSessions {
        req_id: REQ_ID,
        old: old.trim().to_string(),
        new: new.trim().to_string(),
    };
    write_frame(&mut conn.stream, &req).await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Ack { req_id, .. }) if req_id == REQ_ID => return Ok(()),
            Some(ServerEvent::Error {
                req_id: Some(REQ_ID),
                message,
            }) => bail!(message),
            Some(_) => continue,
            None => bail!("the daemon closed the connection before answering"),
        }
    }
}

/// `kete profile rename`: the sessions are asked of the daemon, then
/// `profiles::rename` moves the home and rewrites every name in the
/// settings. A daemon that rewrites the sessions' stored name (protocol
/// 59) is then asked to, and no session refuses the rename; an older one
/// cannot, so a session naming the profile refuses it.
fn rename(base: &Path, local: &Path, old: &str, new: &str) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (mut conn, agents) = rt.block_on(sessions_and_protocol())?;
    let follows = conn.protocol_version >= nebula_core::RENAME_PROFILE_PROTOCOL;
    let signed_in = profiles::load()
        .iter()
        .find(|p| p.name.trim() == old.trim())
        .is_some_and(|p| {
            nebula_core::accounts::login_state(Some(p), &p.harness)
                == nebula_core::accounts::LoginState::LoggedIn
        });
    let root = profiles::profiles_root();
    let done = if follows {
        profiles::rename_following(base, local, &root, old, new)
    } else {
        profiles::rename(base, local, &root, old, new, &agents)
    }
    .map_err(anyhow::Error::msg)?;
    println!("renamed `{}` to `{}`", old.trim(), done.profile.name);
    let named = agents
        .iter()
        .filter(|a| a.profile.as_deref().map(str::trim) == Some(old.trim()))
        .count();
    if follows && named > 0 {
        rt.block_on(rename_sessions(&mut conn, old, new))
            .with_context(|| {
                format!(
                    "renamed, but the sessions still name `{}`: move them to an account",
                    old.trim()
                )
            })?;
        println!(
            "{named} session(s) now name `{}`; a running one picks the new home up when it restarts",
            done.profile.name
        );
    }
    if let Some((_, to)) = &done.home {
        println!("its home is now {}", to.display());
    }
    if !done.projects.is_empty() {
        println!(
            "default {} account of {} now names `{}`",
            done.profile.harness,
            done.projects.join(", "),
            done.profile.name
        );
    }
    if !done.archived.is_empty() {
        println!(
            "{} archived session(s) still name `{}` ({}): move one to an account before restoring it",
            done.archived.len(),
            old.trim(),
            done.archived.join(", ")
        );
    }
    // Some sign-ins key their stored login by the config dir's path: say
    // so when a signed-in home reads as signed out once moved.
    let profile = &done.profile;
    if signed_in
        && nebula_core::accounts::login_state(Some(profile), &profile.harness)
            == nebula_core::accounts::LoginState::NeedsLogin
    {
        if let Some(command) = nebula_core::accounts::login_command(Some(profile), &profile.harness)
        {
            println!("it reads as signed out now: sign it in with: {command}");
        }
    }
    Ok(())
}

/// `kete profile spread <mode> [--threshold N]`: the ACCOUNT SPREAD as
/// the Accounts views set it, through the same settings write.
fn spread(mode: &str, threshold: Option<&str>) -> Result<()> {
    let word = parse_spread(mode)?;
    let threshold = threshold.map(parse_threshold).transpose()?;
    nebula_tui::config::Config::update(|c| {
        c.account_spread = word.to_string();
        if let Some(t) = &threshold {
            c.account_spread_threshold = t.clone();
        }
    })
    .context("save the spread")?;
    let at = match &threshold {
        Some(t) => t.clone(),
        None => nebula_core::accounts::load_policy().threshold.to_string(),
    };
    println!(
        "account spread: {}",
        match nebula_core::accounts::Spread::parse(word) {
            nebula_core::accounts::Spread::Manual => "manual (the default profile)".to_string(),
            nebula_core::accounts::Spread::Failover => format!("failover at {at}%"),
            nebula_core::accounts::Spread::LeastUsed => "least used".to_string(),
        }
    );
    Ok(())
}

/// A spread as config.json spells it, from `manual`, `failover`,
/// `least-used` or `least_used`.
fn parse_spread(mode: &str) -> Result<&'static str> {
    let mode = mode.trim().to_ascii_lowercase();
    match mode.as_str() {
        "manual" | "failover" | "least-used" | "least_used" => {
            Ok(nebula_core::accounts::Spread::parse(&mode).as_str())
        }
        _ => bail!("unknown spread `{mode}`: expected manual, failover or least-used"),
    }
}

/// A failover threshold the Accounts views offer (`90` or `90%`).
fn parse_threshold(text: &str) -> Result<String> {
    let t = text.trim().trim_end_matches('%');
    if nebula_core::accounts::THRESHOLDS.contains(&t) {
        return Ok(t.to_string());
    }
    bail!(
        "threshold {text}: expected one of {}",
        nebula_core::accounts::THRESHOLDS.join(", ")
    )
}

/// The project `query` names: its id, its name (any case; refused when two
/// share it), or a path that is one of its checkouts or inside one.
fn find_project<'a>(
    projects: &'a [nebula_core::Project],
    worktrees: &[nebula_core::Worktree],
    query: &str,
) -> Result<&'a nebula_core::Project> {
    let q = query.trim();
    if let Some(p) = projects.iter().find(|p| p.id.0 == q) {
        return Ok(p);
    }
    let named: Vec<&nebula_core::Project> = projects
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case(q))
        .collect();
    match named.len() {
        1 => return Ok(named[0]),
        0 => {}
        n => bail!("{n} projects are named `{q}`: give its path instead"),
    }
    let want = normal_path(Path::new(q));
    let inside = |root: &Path| {
        let root = normal_path(root);
        want == root || want.starts_with(&format!("{root}/"))
    };
    projects
        .iter()
        .find(|p| {
            inside(&p.repo_path)
                || worktrees
                    .iter()
                    .any(|w| w.project_id == p.id && inside(&w.path))
        })
        .ok_or_else(|| {
            anyhow::anyhow!("no kete project named `{q}` or holding that path: `kete add` adds one")
        })
}

/// A path for comparing: canonical when it exists, `/` separators, no
/// trailing one, and on Windows case folded.
fn normal_path(p: &Path) -> String {
    let p = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let text = p.to_string_lossy();
    let text = text
        .strip_prefix(r"\\?\")
        .unwrap_or(&text)
        .replace('\\', "/");
    let text = text.trim_end_matches('/').to_string();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

fn list() -> Result<()> {
    let all = profiles::load();
    let usage = nebula_core::usage::read_all_in(&profiles::profiles_root());
    let policy = nebula_core::accounts::load_policy();
    let now = nebula_core::clock::now_secs() as i64;
    let lines = render_list(&all, &usage, now, |p, harness| {
        nebula_core::accounts::login_state(p, harness)
    });
    if all.is_empty() {
        println!(
            "no profiles: add one with: kete profile add <name> --harness <claude|codex|copilot>"
        );
    }
    for line in lines {
        println!("{line}");
    }
    if !all.is_empty() {
        let spread = match policy.spread {
            nebula_core::accounts::Spread::Manual => "manual (the default profile)".to_string(),
            nebula_core::accounts::Spread::Failover => {
                format!("failover at {}%", policy.threshold)
            }
            nebula_core::accounts::Spread::LeastUsed => "least used".to_string(),
        };
        println!("\naccount spread: {spread}  (Settings \u{2192} Sessions \u{2192} Accounts)");
    }
    let projects = profiles::load_project_defaults();
    if !projects.is_empty() {
        println!("\nproject defaults (kete profile set-project):");
        for line in project_default_lines(&projects) {
            println!("{line}");
        }
    }
    Ok(())
}

/// `  C:\Code\app  claude: work, codex: gpt` per project with defaults.
fn project_default_lines(projects: &BTreeMap<String, BTreeMap<String, String>>) -> Vec<String> {
    projects
        .iter()
        .map(|(repo, defaults)| {
            let pairs: Vec<String> = defaults.iter().map(|(h, n)| format!("{h}: {n}")).collect();
            format!("  {repo}  {}", pairs.join(", "))
        })
        .collect()
}

/// Width of a usage bar in `kete profile list`.
const BAR: usize = 10;

/// `kete profile list`'s lines: every profile, then each harness's own
/// login that has a reading (`@claude`), with a bar for its session (5h)
/// and weekly use, and whether it is signed in.
fn render_list(
    all: &[Profile],
    usage: &[nebula_core::usage::AccountUsage],
    now: i64,
    login: impl Fn(Option<&Profile>, &str) -> nebula_core::accounts::LoginState,
) -> Vec<String> {
    use nebula_core::accounts::LoginState;
    use nebula_core::usage::{block_bar, pct_text};
    let cell = |w: Option<&nebula_core::usage::UsageWindow>| {
        let p = w.and_then(|w| w.shown_at(now));
        format!("{} {:>4}", block_bar(p, BAR), pct_text(p))
    };
    let mut rows: Vec<(String, String, String, String, String)> = Vec::new();
    let mut push = |name: String, harness: &str, profile: Option<&Profile>| {
        let key = nebula_core::usage::account_key(profile.map(|p| p.name.as_str()), harness);
        let reading = usage.iter().find(|u| u.account == key);
        let [top, bottom] = reading.map_or([None, None], |u| u.bars());
        let state = match profile.and_then(Profile::problem) {
            Some(problem) => format!("! {problem}"),
            None => match login(profile, harness) {
                LoginState::LoggedIn => "signed in".to_string(),
                LoginState::NeedsLogin => {
                    match nebula_core::accounts::login_command(profile, harness) {
                        Some(hint) => format!("needs login: {hint}"),
                        None => "needs login".to_string(),
                    }
                }
                LoginState::Unknown => String::new(),
            },
        };
        rows.push((name, harness.to_string(), cell(top), cell(bottom), state));
    };
    for profile in all {
        let mut name = profile.name.clone();
        if profiles::default_for(all, &profile.harness).is_some_and(|d| d.name == profile.name) {
            name.push_str(" *");
        }
        if profile.disabled {
            name.push_str(" (off)");
        }
        push(name, profile.harness.trim(), Some(profile));
    }
    for u in usage {
        if let Some(harness) = nebula_core::usage::default_login_harness(&u.account) {
            push(format!("{} (own login)", harness), harness, None);
        }
    }
    if rows.is_empty() {
        return Vec::new();
    }
    let mut out = vec![format!(
        "{:<22} {:<8} {:<15} {:<15} {}",
        "ACCOUNT", "HARNESS", "5H / FIRST", "WEEKLY / NEXT", "LOGIN"
    )];
    for (name, harness, top, bottom, state) in rows {
        out.push(
            format!("{name:<22} {harness:<8} {top:<15} {bottom:<15} {state}")
                .trim_end()
                .to_string(),
        );
    }
    out.push("* default".to_string());
    out
}

/// Run the harness's sign-in in this terminal under the profile's
/// environment, and wait for it. `extra` rides after the sign-in's own
/// arguments (`--email you@example.com`, `--device-code`).
fn login(name: &str, extra: &[String]) -> Result<()> {
    let all = profiles::load();
    let Some(profile) = all.iter().find(|p| p.name.trim() == name.trim()) else {
        bail!("no profile named `{name}`: kete profile list shows them");
    };
    if let Some(problem) = profile.problem() {
        bail!("{problem}");
    }
    let Some(args) = profiles::login_args(&profile.harness) else {
        bail!(
            "kete knows no sign-in command for `{}`: sign in from a session under the profile instead",
            profile.harness
        );
    };
    let registry = nebula_tui::config::Config::load().harness_registry();
    let program = registry
        .iter()
        .find(|h| h.id == profile.harness.trim())
        .map(|h| h.program.trim().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| profile.harness.trim().to_string());
    profile.prepare(None).context("prepare the profile home")?;
    let env = profile.launch_env();
    let args: Vec<String> = args
        .iter()
        .map(|a| a.to_string())
        .chain(extra.iter().cloned())
        .collect();
    match profile.home() {
        Some(home) => eprintln!(
            "kete: signing `{}` in with `{program} {}` (home {})",
            profile.name,
            args.join(" "),
            home.display()
        ),
        None => eprintln!(
            "kete: signing `{}` in with `{program} {}` (the CLI's default login)",
            profile.name,
            args.join(" ")
        ),
    }
    let mut command = login_command(&program, &args)?;
    for name in env.removals() {
        command.env_remove(name);
    }
    command.envs(env.set.iter().map(|(k, v)| (k, v)));
    let status = command
        .status()
        .with_context(|| format!("run {program}: is it installed?"))?;
    if !status.success() {
        bail!("{program} exited with {status}");
    }
    Ok(())
}

/// `program args…` as a process: on Windows resolved through PATH and
/// PATHEXT, a `.cmd` or `.ps1` under its host, as a session spawn does.
#[cfg(windows)]
fn login_command(program: &str, args: &[String]) -> Result<std::process::Command> {
    let launch = nebula_core::shell::launch(program, args).map_err(anyhow::Error::msg)?;
    let mut command = std::process::Command::new(&launch.program);
    command
        .args(&launch.args)
        .envs(launch.env.iter().map(|(k, v)| (k, v)));
    Ok(command)
}

#[cfg(unix)]
fn login_command(program: &str, args: &[String]) -> Result<std::process::Command> {
    let mut command = std::process::Command::new(program);
    command.args(args);
    Ok(command)
}

/// The settings layer an edit of the PROFILES list goes to: the one whose
/// `profiles` key is in force. Layers merge key by key, so a
/// `config.local.json` with a `profiles` key replaces `config.json`'s
/// list whole, and an edit there is the only one that shows. A local layer
/// that is there but unreadable is refused: there is no telling which list
/// is in force.
fn profiles_layer(config: &Path, local: &Path) -> Result<PathBuf> {
    let held = nebula_core::settings::read_object(local)
        .map_err(|e| anyhow::anyhow!("{}: {e}: fix it, then try again", local.display()))?;
    Ok(
        if held.is_some_and(|obj| obj.contains_key(profiles::CONFIG_KEY)) {
            local.to_path_buf()
        } else {
            config.to_path_buf()
        },
    )
}

#[cfg(test)]
mod layer_tests {
    use super::*;

    fn layers(dir: &Path) -> (PathBuf, PathBuf) {
        (dir.join("config.json"), dir.join("config.local.json"))
    }

    #[test]
    fn profiles_edit_the_base_layer_unless_the_local_one_holds_them() {
        let dir = tempfile::tempdir().unwrap();
        let (config, local) = layers(dir.path());
        assert_eq!(profiles_layer(&config, &local).unwrap(), config);
        std::fs::write(&local, r#"{"theme": "rose"}"#).unwrap();
        assert_eq!(profiles_layer(&config, &local).unwrap(), config);
        std::fs::write(&local, r#"{"profiles": []}"#).unwrap();
        assert_eq!(profiles_layer(&config, &local).unwrap(), local);
        std::fs::write(&local, "{ not json").unwrap();
        assert!(profiles_layer(&config, &local).is_err());
    }

    /// An add lands where `profiles::load` reads it back from.
    #[test]
    fn an_added_profile_is_one_the_merged_settings_show() {
        let dir = tempfile::tempdir().unwrap();
        let (config, local) = layers(dir.path());
        std::fs::write(
            &config,
            r#"{"profiles": [{"name": "base", "harness": "claude"}]}"#,
        )
        .unwrap();
        std::fs::write(
            &local,
            r#"{"profiles": [{"name": "work", "harness": "claude"}]}"#,
        )
        .unwrap();
        let profile: Profile =
            serde_json::from_value(serde_json::json!({"name": "home", "harness": "claude"}))
                .unwrap();
        profiles::add_to_file(&profiles_layer(&config, &local).unwrap(), &profile).unwrap();
        let merged: Vec<String> =
            profiles::from_settings(&nebula_core::settings::read_object(&local).unwrap().unwrap())
                .into_iter()
                .map(|p| p.name)
                .collect();
        assert_eq!(merged, ["work", "home"]);
        assert!(!std::fs::read_to_string(&config).unwrap().contains("home"));
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;
    use nebula_core::{Project, ProjectId, Worktree, WorktreeId};

    fn work() -> Profile {
        let mut env = BTreeMap::new();
        env.insert("K".to_string(), "v".to_string());
        Profile {
            name: "work".into(),
            harness: "claude".into(),
            env,
            default: false,
            use_default: false,
            disabled: true,
            reserved: false,
        }
    }

    #[test]
    fn an_edit_changes_only_what_is_given() {
        let same = edited(work(), None, None, false, None).unwrap();
        assert_eq!(same, work());
        let p = edited(work(), None, Some(true), true, Some(&[])).unwrap();
        assert!(p.use_default && p.default && p.env.is_empty());
        assert!(p.disabled, "the switch stays");
        assert_eq!(p.name, "work");
        let p = edited(
            work(),
            Some(" codex ".into()),
            Some(false),
            false,
            Some(&["A=1".into()]),
        )
        .unwrap();
        assert_eq!(p.harness, "codex");
        assert_eq!(p.env.get("A").map(String::as_str), Some("1"));
        assert!(edited(work(), None, None, false, Some(&["NOEQ".into()])).is_err());
        assert!(edited(work(), None, None, false, Some(&["=v".into()])).is_err());
        assert!(edited(work(), Some(" ".into()), None, false, None).is_err());
    }

    #[test]
    fn a_project_is_found_by_id_name_or_a_path_in_a_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("app");
        let wt = dir.path().join("app-feature");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(&wt).unwrap();
        let project = |id: &str, name: &str, path: &Path| Project {
            id: ProjectId(id.into()),
            name: name.into(),
            repo_path: path.to_path_buf(),
            sort_order: 0,
        };
        let projects = vec![
            project("p1", "App", &repo),
            project("p2", "twin", Path::new("/x")),
            project("p3", "twin", Path::new("/y")),
        ];
        let worktrees = vec![Worktree {
            id: WorktreeId("w1".into()),
            project_id: ProjectId("p1".into()),
            path: wt.clone(),
            branch: "feature".into(),
            is_main: false,
            sort_order: 0,
        }];
        let find = |q: &str| find_project(&projects, &worktrees, q).map(|p| p.id.0.clone());
        assert_eq!(find("p2").unwrap(), "p2");
        assert_eq!(find("app").unwrap(), "p1", "names match in any case");
        assert_eq!(find(repo.join("src").to_str().unwrap()).unwrap(), "p1");
        assert_eq!(
            find(wt.to_str().unwrap()).unwrap(),
            "p1",
            "a linked checkout"
        );
        assert!(find("twin").unwrap_err().to_string().contains("2 projects"));
        assert!(
            find(dir.path().to_str().unwrap()).is_err(),
            "above every checkout"
        );
        assert!(find("nope").is_err());
    }

    #[test]
    fn project_defaults_list_one_line_per_project() {
        let mut map = BTreeMap::new();
        map.insert(
            "/repo".to_string(),
            BTreeMap::from([
                ("claude".to_string(), "work".to_string()),
                ("codex".to_string(), "gpt".to_string()),
            ]),
        );
        assert_eq!(
            project_default_lines(&map),
            ["  /repo  claude: work, codex: gpt"]
        );
    }
}

#[cfg(test)]
mod list_tests {
    use super::*;
    use nebula_core::accounts::LoginState;
    use nebula_core::usage::{AccountUsage, UsageSource, UsageWindow, WindowKind};

    #[test]
    fn the_list_draws_bars_flags_and_the_login_command() {
        let p = |name: &str, default: bool, disabled: bool| Profile {
            name: name.into(),
            harness: "claude".into(),
            env: BTreeMap::new(),
            default,
            use_default: false,
            disabled,
            reserved: false,
        };
        let all = vec![p("work", true, false), p("home", false, true)];
        let usage = vec![
            AccountUsage {
                account: "work".into(),
                harness: "claude".into(),
                windows: vec![
                    UsageWindow {
                        kind: WindowKind::Session,
                        label: None,
                        percent: 50.0,
                        resets_at: None,
                        read_at: 1,
                        estimated: false,
                    },
                    UsageWindow {
                        kind: WindowKind::Weekly,
                        label: None,
                        percent: 10.0,
                        resets_at: None,
                        read_at: 1,
                        estimated: false,
                    },
                ],
                source: UsageSource::Statusline,
                at: 0,
                plan: None,
            },
            AccountUsage {
                account: "@codex".into(),
                harness: "codex".into(),
                windows: Vec::new(),
                source: UsageSource::Rollout,
                at: 0,
                plan: None,
            },
        ];
        let lines = render_list(&all, &usage, 0, |p, _| match p {
            Some(p) if p.name == "home" => LoginState::NeedsLogin,
            _ => LoginState::LoggedIn,
        });
        assert!(lines[0].starts_with("ACCOUNT"));
        assert!(lines[1].starts_with("work *"), "{}", lines[1]);
        assert!(
            lines[1].contains("\u{25b0}\u{25b0}\u{25b0}\u{25b0}\u{25b0}\u{25b1}"),
            "{}",
            lines[1]
        );
        assert!(lines[1].contains(" 50%") && lines[1].contains(" 10%"));
        assert!(lines[1].ends_with("signed in"));
        assert!(lines[2].starts_with("home (off)"));
        assert!(lines[2].contains("\u{2014}"), "unknown reads as a dash");
        assert!(lines[2].ends_with("needs login: kete profile login home"));
        assert!(lines[3].starts_with("codex (own login)"));
    }
}
