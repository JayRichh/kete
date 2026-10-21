//! Windows end-to-end flows of the 0.60 ledger lane, every one named `ledger_*`.
//!
//! The fake agent's `usage:` step writes an assistant message to its
//! transcript and reports the session's cost on the status line, as Claude
//! does; the daemon's ledger adds each turn up, and `kete usage --csv`
//! prints what it holds.

use super::harness::{Options, Tui, ENTER, WAIT};
use std::time::{Duration, Instant};

pub const TESTS: &[(&str, fn())] = &[(
    "ledger_two_turns_make_one_row_off_adds_nothing_and_the_tui_exports_it",
    ledger_two_turns_make_one_row_off_adds_nothing_and_the_tui_exports_it,
)];

/// Two turns of one session sum into today's one row: 1,500 input and
/// 300 output tokens, the cost estimate Claude reported last (0.08), two
/// turns. With `usage_ledger` off, a third turn adds nothing. The TUI
/// exports the same rows.
fn ledger_two_turns_make_one_row_off_adds_nothing_and_the_tui_exports_it() {
    let tui = Tui::spawn(Options {
        script: "usage:1000,200,0.05;stop;\
                 gate:second;prompt:the second turn;usage:500,100,0.08;stop;\
                 gate:third;prompt:the third turn;usage:300,30,0.11;stop;echo"
            .into(),
        ..Options::default()
    });
    let repo = tui.make_repo("ledger-proj");
    tui.wait_for_text("create your first project");
    tui.add_project(&repo, "ledger-proj");
    launch(&tui, "count the tokens");

    let row = wait_for_row(&tui, |r| r.turns == "1");
    assert_eq!((row.input.as_str(), row.output.as_str()), ("1000", "200"));
    assert_eq!(row.cost, "0.0500");

    tui.open_gate("second");
    let row = wait_for_row(&tui, |r| r.turns == "2" && r.cost == "0.0800");
    assert_eq!((row.input.as_str(), row.output.as_str()), ("1500", "300"));
    assert_eq!(row.project, "ledger-proj");
    let csv = usage_csv(&tui);
    eprintln!("ledger CSV after two turns:\n{csv}");
    assert_eq!(csv.lines().count(), 2, "one row under the header:\n{csv}");

    // Off: the third turn runs to its end and the ledger keeps what it had.
    let config = tui.data_dir.join(nebula_core::paths::CONFIG_FILE_NAME);
    let mut cfg: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    cfg["usage_ledger"] = false.into();
    std::fs::write(&config, cfg.to_string()).unwrap();
    tui.open_gate("third");
    let start = Instant::now();
    while std::fs::read_to_string(&tui.agent_log)
        .unwrap_or_default()
        .matches("hook Stop:")
        .count()
        < 3
    {
        assert!(start.elapsed() < WAIT, "the third turn never ended");
        std::thread::sleep(Duration::from_millis(200));
    }
    // The Stop hook has returned; give the late status line and the turn
    // end the time they take.
    std::thread::sleep(Duration::from_secs(3));
    let after = usage_csv(&tui);
    assert_eq!(after, csv, "the ledger is off: nothing new");

    // The terminal app's Export usage CSV, from the command palette, writes
    // the same rows to the exports folder without holding the screen.
    tui.send(&[0x0b]);
    tui.type_str("usage csv");
    tui.wait_for_text("Export usage CSV");
    tui.send(ENTER);
    tui.wait_for_text("Usage CSV written");
    let day = nebula_core::usage_ledger::local_day(nebula_core::clock::now_secs() as i64);
    let file = tui
        .data_dir
        .join("exports")
        .join(format!("usage-{day}.csv"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), csv);
}

/// The quick prompt, `task` typed and sent.
fn launch(tui: &Tui, task: &str) {
    tui.send(b"p");
    tui.wait_for_text("what should the agent do?");
    tui.type_str(task);
    tui.wait_for_text(&format!("│ {task}"));
    tui.send(ENTER);
    tui.wait_for_gone("what should the agent do?");
}

/// `kete usage --csv`'s output.
fn usage_csv(tui: &Tui) -> String {
    let out = tui.kete(&["usage", "--csv"]);
    assert!(
        out.status.success(),
        "kete usage --csv: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n")
}

/// The columns these flows read from one CSV row.
#[derive(Debug)]
struct Row {
    project: String,
    input: String,
    output: String,
    cost: String,
    turns: String,
}

/// The first `kete usage --csv` row `pred` holds for, polled until it does.
fn wait_for_row(tui: &Tui, pred: impl Fn(&Row) -> bool) -> Row {
    let start = Instant::now();
    loop {
        let csv = usage_csv(tui);
        let row = csv.lines().nth(1).map(|line| {
            // No field in these flows needs quoting.
            let f: Vec<&str> = line.split(',').collect();
            Row {
                project: f[1].into(),
                input: f[7].into(),
                output: f[8].into(),
                cost: f[11].into(),
                turns: f[13].into(),
            }
        });
        match row {
            Some(row) if pred(&row) => return row,
            last => assert!(
                start.elapsed() < WAIT,
                "no ledger row as expected; last {last:?}\n{csv}"
            ),
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}
