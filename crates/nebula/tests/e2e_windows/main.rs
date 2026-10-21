//! Windows end to end: the real `kete` binary in a real ConPTY, its own
//! daemon, temp git repos with a local bare repo as `origin`, and a
//! scripted fake agent standing in for Claude Code — the screen read back
//! through vt100, as a user would see it.
//!
//! One binary plays two parts (`harness = false`): run by cargo it is the
//! suite; run by the daemon as a session's CLI (the temp config points the
//! `claude` harness's program at it, and [`agent::ROLE`] rides the
//! environment down) it is the fake agent, which reports status through
//! the hooks the daemon installed in the worktree, exactly as Claude does.
//!
//! `cargo test -p nebula --test e2e_windows [-- <name filter>]`

#[cfg(windows)]
mod accounts;
#[cfg(windows)]
mod agent;
#[cfg(windows)]
mod flows;
#[cfg(windows)]
mod harness;
#[cfg(windows)]
mod ledger;
#[cfg(windows)]
mod queue;
#[cfg(windows)]
mod renew;
#[cfg(windows)]
mod roles;
#[cfg(windows)]
mod triage;
#[cfg(windows)]
mod upgrade;

/// Every flow in the suite: the main thread's, then each 0.60 lane's
/// (`accounts_*`, `queue_*`, `roles_*`, `renew_*`, `ledger_*`,
/// `triage_*`), in that order. The filter, `--exact` and `--list` see
/// all of them.
#[cfg(windows)]
fn all_tests() -> Vec<(&'static str, fn())> {
    [
        flows::TESTS,
        accounts::TESTS,
        queue::TESTS,
        roles::TESTS,
        renew::TESTS,
        ledger::TESTS,
        triage::TESTS,
    ]
    .into_iter()
    .flatten()
    .copied()
    .collect()
}

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    if std::env::var_os(agent::ROLE).is_some() {
        agent::run();
        return;
    }
    run_suite();
}

/// Set to anything: the suite runs nothing (CI's workspace-wide test step,
/// which leaves the suite to a step of its own).
#[cfg(windows)]
const SKIP: &str = "KETE_E2E_SKIP";
/// The longest one test may run before the whole run is failed.
#[cfg(windows)]
const TEST_LIMIT: std::time::Duration = std::time::Duration::from_secs(180);

/// libtest's job, done small: run every test whose name holds the filter
/// (the first argument that is not a flag), one at a time — each brings up
/// a TUI, a daemon and agents of its own — and fail the run if any fails.
#[cfg(windows)]
fn run_suite() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let tests = all_tests();
    if args.iter().any(|a| a == "--list") {
        for (name, _) in &tests {
            println!("{name}: test");
        }
        return;
    }
    // Every test here is an end-to-end run; none is ignored, so a
    // workspace-wide `-- --ignored` runs none of them. CI runs the suite
    // in a step of its own and sets SKIP on the workspace-wide one.
    if args.iter().any(|a| a == "--ignored") || std::env::var_os(SKIP).is_some() {
        println!("\nrunning 0 tests");
        return;
    }
    // A test that hangs somewhere no screen wait covers (a child that
    // never exits, a pipe that never closes) fails the run instead of
    // holding a CI job to its timeout.
    let current = std::sync::Arc::new(std::sync::Mutex::new(("", std::time::Instant::now())));
    {
        let current = current.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let (name, since) = *current.lock().unwrap();
            if since.elapsed() > TEST_LIMIT {
                println!("test {name} ... FAILED (hung past {TEST_LIMIT:?})");
                std::process::exit(101);
            }
        });
    }
    let filter = args.iter().find(|a| !a.starts_with('-')).cloned();
    let exact = args.iter().any(|a| a == "--exact");
    let chosen: Vec<_> = tests
        .iter()
        .filter(|(name, _)| match &filter {
            Some(f) if exact => name == f,
            Some(f) => name.contains(f.as_str()),
            None => true,
        })
        .collect();
    println!("\nrunning {} tests", chosen.len());
    let began = std::time::Instant::now();
    let mut failed = Vec::new();
    for (name, test) in chosen.iter().copied() {
        let start = std::time::Instant::now();
        *current.lock().unwrap() = (name, start);
        let outcome = std::panic::catch_unwind(*test);
        let took = start.elapsed().as_secs_f64();
        match outcome {
            Ok(()) => println!("test {name} ... ok ({took:.1} s)"),
            Err(_) => {
                println!("test {name} ... FAILED ({took:.1} s)");
                failed.push(*name);
            }
        }
    }
    println!(
        "\ntest result: {}. {} passed; {} failed; finished in {:.1} s",
        if failed.is_empty() { "ok" } else { "FAILED" },
        chosen.len() - failed.len(),
        failed.len(),
        began.elapsed().as_secs_f64()
    );
    if !failed.is_empty() {
        for name in &failed {
            println!("    {name}");
        }
        std::process::exit(101);
    }
}
