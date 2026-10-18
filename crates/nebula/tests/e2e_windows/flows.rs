//! The flows, one test each: the 0.49–0.51 board, review and ship work as
//! a user drives it, on the real binaries.

use super::harness::*;
use std::path::{Path, PathBuf};
use std::time::Duration;

type Test = fn();

pub const TESTS: &[(&str, Test)] = &[
    ("boots_to_the_empty_grid", boots_to_the_empty_grid),
    (
        "quick_prompt_session_runs_needs_you_and_lands_in_review",
        quick_prompt_session_runs_needs_you_and_lands_in_review,
    ),
    (
        "review_card_verbs_focus_scope_and_diff",
        review_card_verbs_focus_scope_and_diff,
    ),
    (
        "ship_without_gh_shows_the_install_fix",
        ship_without_gh_shows_the_install_fix,
    ),
    (
        "archive_offers_the_worktree_delete",
        archive_offers_the_worktree_delete,
    ),
    (
        "dragging_a_done_card_onto_archived_offers_the_delete",
        dragging_a_done_card_onto_archived_offers_the_delete,
    ),
    (
        "a_20_kb_paste_reaches_the_agent_as_one_paste",
        a_20_kb_paste_reaches_the_agent_as_one_paste,
    ),
    (
        "a_paste_with_a_pause_before_its_last_line_break_never_submits",
        a_paste_with_a_pause_before_its_last_line_break_never_submits,
    ),
    (
        "the_grid_pane_shows_every_line_the_agent_printed",
        the_grid_pane_shows_every_line_the_agent_printed,
    ),
    (
        "the_pane_keeps_every_line_across_board_and_grid",
        the_pane_keeps_every_line_across_board_and_grid,
    ),
    (
        "a_048_data_dir_upgrades_in_place",
        super::upgrade::a_048_data_dir_upgrades_in_place,
    ),
    (
        "a_usage_limit_reads_resumes_on_the_card_until_claude_carries_on",
        a_usage_limit_reads_resumes_on_the_card_until_claude_carries_on,
    ),
    (
        "a_wake_up_runs_a_hidden_session_no_list_shows",
        a_wake_up_runs_a_hidden_session_no_list_shows,
    ),
    (
        "the_session_tree_names_a_subagents_model_and_effort",
        the_session_tree_names_a_subagents_model_and_effort,
    ),
];

/// A subagent the session runs, its Agent call naming haiku and its own
/// hooks saying low effort: the session tree (Shift+S) lists it under
/// the session with its task, type, model and effort. It stays running,
/// so no finished fold hides it.
fn the_session_tree_names_a_subagents_model_and_effort() {
    let tui = Tui::spawn(Options {
        script: "subagent-open:Explore:haiku:low:Map the hooks;stop;echo".into(),
        ..Options::default()
    });
    let repo = tui.make_repo("tree-proj");
    tui.wait_for_text("create your first project");
    tui.add_project(&repo, "tree-proj");
    launch(&tui, "map the hooks");
    tui.wait_for_agent("step stop");
    tui.wait_for_text("agent-1  done");
    tui.send(b"S");
    tui.wait_for_text("Map the hooks");
    tui.wait_for_text("Explore · haiku · low");
}

/// WHAKAOHO: `kete wake @claude --force` has the daemon run a hidden
/// session — the fake agent, handed haiku and the one-word prompt, in the
/// wake folder — whose status line reading lands on the account, that no
/// session list shows, and that the wake log records with the reset time
/// before and after.
fn a_wake_up_runs_a_hidden_session_no_list_shows() {
    let tui = Tui::spawn(Options {
        scripts: vec![("hi", "statusline:1800000000;stop".into())],
        ..Options::default()
    });
    tui.wait_for_text("create your first project");
    // Claude's own login, as far as the account check can tell.
    let claude = tui.home().join(".claude");
    std::fs::create_dir_all(&claude).unwrap();
    std::fs::write(claude.join(".credentials.json"), r#"{"claudeAiOauth": {}}"#).unwrap();

    let text = |out: &std::process::Output| {
        format!(
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    };
    // Off, so a plain run (and its dry run) skips and runs nothing.
    let dry = tui.kete(&["wake", "@claude", "--dry-run"]);
    assert!(dry.status.success(), "{}", text(&dry));
    assert!(
        String::from_utf8_lossy(&dry.stdout).contains("would skip: wake-up is off"),
        "{}",
        text(&dry)
    );
    assert!(
        !tui.data_dir.join("wake-log.jsonl").exists(),
        "a dry run logs nothing"
    );

    let out = tui.kete(&["wake", "@claude", "--force", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let list: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON out");
    let report = &list[0]["report"];
    assert_eq!(report["submitted"], true, "{report}");
    assert_eq!(report["finished"], true, "{report}");
    assert_eq!(report["read"], true, "{report}");
    assert_eq!(report["resets_after"], 1_800_000_000, "{report}");

    let log = std::fs::read_to_string(tui.data_dir.join("wake-log.jsonl")).unwrap();
    let entries: Vec<serde_json::Value> = log
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(entries.len(), 1, "{log}");
    assert_eq!(entries[0]["account"], "@claude");
    assert_eq!(entries[0]["decision"], "fired");
    assert_eq!(entries[0]["forced"], true);
    assert_eq!(entries[0]["resets_after"], 1_800_000_000);

    // Run as Claude would be: the cheapest model, the one-word prompt, in
    // the wake folder, its turn reported through the hooks Kete put there.
    // (The session is killed the moment its Stop lands, so the agent may
    // not live to log that hook's exit.)
    let agent_log = std::fs::read_to_string(&tui.agent_log).unwrap();
    assert!(agent_log.contains(r#""--model", "haiku""#), "{agent_log}");
    assert!(agent_log.contains(r#"prompt "hi""#), "{agent_log}");
    assert!(
        agent_log.contains("hook UserPromptSubmit: Ok(Some(0))"),
        "{agent_log}"
    );
    assert!(agent_log.contains("statusline: Ok(Some(0))"), "{agent_log}");
    assert!(agent_log.contains("step stop"), "{agent_log}");
    assert!(tui
        .data_dir
        .join("wake")
        .join(".claude")
        .join("settings.local.json")
        .exists());

    // No row: no client's list, board or snapshot has it.
    let (_, _, agents) = super::upgrade::daemon_tree(&tui.runtime_dir);
    assert!(agents.is_empty(), "{agents:?}");
    tui.wait_for_text("create your first project");
    // The reading is the account's, where the CLI and the apps read it.
    let reading =
        nebula_core::usage::read_account_in(&tui.data_dir.join("profiles"), "@claude").unwrap();
    assert_eq!(
        reading
            .window(nebula_core::usage::WindowKind::Session)
            .and_then(|w| w.resets_at),
        Some(1_800_000_000)
    );
}

/// The feature the fake agent builds: a question, a file, a commit.
const FEATURE: &str =
    "ask;edit:src/feature.txt:made by the fake agent;commit:add the feature;stop;echo";

fn boots_to_the_empty_grid() {
    let tui = Tui::spawn(Options::default());
    tui.wait_for_text("create your first project");
}

/// A session from the quick prompt, followed on the board: Running while
/// it works, Needs you (and the header's attention count) on its question,
/// Space to reply, then Finished and in Review with its commit counted.
fn quick_prompt_session_runs_needs_you_and_lands_in_review() {
    let tui = Tui::spawn(Options {
        script: format!("gate:asked;{FEATURE}"),
        ..Options::default()
    });
    let repo = tui.make_repo("life-proj");
    tui.wait_for_text("create your first project");
    tui.add_project(&repo, "life-proj");

    // On the grid, the header's ATTENTION STRIP counts it running, then
    // waiting on you.
    launch(&tui, "build the feature");
    tui.wait_for_text("1 running");
    tui.wait_for_agent("step gate:asked");
    tui.open_gate("asked");
    tui.wait_for_text("1 needs you");
    tui.wait_for_text("agent-1  needs you");

    // On the board, its lane.
    tui.send(b"K");
    tui.wait_for_text("● 1 Needs you");
    reply(&tui, "yes ship it");
    tui.wait_for_agent("reply \"yes ship it\"");
    tui.wait_for_text("● 1 Review");
    tui.wait_for_text("1 commit");
    tui.wait_for_text("agent-1  done");

    // What it printed reached the daemon whole.
    let (_, _, ring) = daemon_tail(&tui.runtime_dir, &tui.agent_id("build the feature"));
    let ring = String::from_utf8_lossy(&ring);
    assert!(ring.contains("committed: add the feature"), "{ring:?}");

    // The work is on the session's own worktree, committed whole: Kete's
    // hooks file is not in it, nor left over as a change.
    let wt = session_worktree(&repo);
    let log = git(&wt, &["log", "--format=%s", "main..HEAD"]);
    assert_eq!(log.trim(), "add the feature");
    let files = git(&wt, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(files.trim(), "src/feature.txt");
    assert_eq!(git(&wt, &["status", "--porcelain"]).trim(), "");

    // Back on the grid, its pane reads all of it.
    tui.send(b"K");
    tui.wait_for_gone("● 1 Review");
    pane_shows_the_feature(&tui);
}

/// On a Review card: `?` offers Create pull request and Worktree; `d`
/// diffs the worktree against its base with the agent's committed file;
/// Focus here narrows the board to the card's worktree, and the lanes
/// count what that leaves outside.
fn review_card_verbs_focus_scope_and_diff() {
    let tui = Tui::spawn(Options {
        script: FEATURE.into(),
        scripts: vec![("tidy", "stop;echo".into())],
        ..Options::default()
    });
    let repo = tui.make_repo("verbs-proj");
    tui.wait_for_text("create your first project");
    tui.add_project(&repo, "verbs-proj");
    tui.send(b"K");
    to_review(&tui);

    tui.send(b"?");
    tui.wait_for_text("Create pull request");
    tui.wait_for_text("Worktree");
    tui.send(ESC);
    tui.wait_for_gone("Create pull request");

    tui.send(b"d");
    tui.wait_for_text("Commits (1)");
    tui.wait_for_text("add the feature");
    tui.wait_for_text("src/feature.txt");
    tui.wait_for_text("+made by the fake agent");
    tui.send(ESC);
    tui.wait_for_gone("Commits (1)");

    // A second session in a worktree of its own, finished with nothing
    // done and watched as it went (the cursor follows it): Done.
    launch(&tui, "tidy the docs");
    tui.wait_for_text("● 1 Done");

    tui.send(b"?");
    tui.wait_for_text("Worktree");
    tui.type_str("focus");
    tui.wait_for_text("Focus here");
    tui.wait_for_text("⌕ focus");
    tui.send(ENTER);
    tui.wait_for_text("● 0 Review");
    // The Review lane's foot counts the card the scope leaves out, in
    // its short form in a lane this narrow.
    tui.wait_for("the Review card counted outside the scope", |s| {
        let text = screen_to_text(s);
        text.contains("1 more outside scope") || text.contains("+1 outside")
    });
    // The chip names the worktree now.
    tui.wait_for_text("all ▸ verbs-proj ▸ ");

    // Esc lets go of the card, then widens the scope back.
    for _ in 0..3 {
        if !tui.try_wait_for_text(" outside", Duration::from_millis(500)) {
            break;
        }
        tui.send(ESC);
    }
    tui.wait_for_gone(" outside");
    tui.wait_for_text("● 1 Review");
}

/// Review → Ship opens the pull request composer; with no `gh` on PATH
/// creating it fails with the fix as a button: install gh.
fn ship_without_gh_shows_the_install_fix() {
    let tui = Tui::spawn(Options {
        script: FEATURE.into(),
        strip_path: gh_dirs(),
        ..Options::default()
    });
    let repo = tui.make_github_repo("ship-proj");
    tui.wait_for_text("create your first project");
    tui.add_project(&repo, "ship-proj");
    tui.send(b"K");
    to_review(&tui);

    tui.send(SHIFT_RIGHT);
    tui.wait_for_text("Pull request: ");
    tui.wait_for_text("Title  add the feature");
    tui.send(ENTER);
    tui.wait_for_text("open cli.github.com to install gh");
    tui.send(ESC);
}

/// Archive: `a` on a Review card archives it alone (its worktree holds
/// work no PR carries); a Done card moved past Done is archived with the
/// offer of its now idle worktree's safe delete, which removes it from
/// disk when taken.
fn archive_offers_the_worktree_delete() {
    let tui = Tui::spawn(Options {
        script: FEATURE.into(),
        scripts: vec![("tidy", "stop;echo".into())],
        ..Options::default()
    });
    let repo = tui.make_repo("arch-proj");
    tui.wait_for_text("create your first project");
    tui.add_project(&repo, "arch-proj");
    tui.send(b"K");
    to_review(&tui);
    tui.send(b"a");
    tui.wait_for_text("Archive agent 'agent-1'?");
    tui.send(ENTER);
    tui.wait_for_text("● 0 Review");
    assert!(
        !tui.try_wait_for_text("delete it from disk", Duration::from_secs(1)),
        "a plain archive offers no delete:\n{}",
        tui.screen_text()
    );

    let before = linked_worktrees(&repo);
    launch(&tui, "tidy the docs");
    tui.wait_for_text("● 1 Done");
    let wt = linked_worktrees(&repo)
        .into_iter()
        .find(|w| !before.contains(w))
        .expect("the tidy session's worktree");
    tui.send(SHIFT_RIGHT);
    tui.wait_for_text("delete it from disk too?");
    tui.send(ENTER);
    tui.wait_for_text("● 0 Done");
    let deadline = std::time::Instant::now() + WAIT;
    while wt.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the worktree was not deleted:\n{}",
            tui.screen_text()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The same archive by mouse: a Done card held, the Archived zone comes
/// up along the board's foot, and letting go over it archives the card
/// with the worktree delete offered.
fn dragging_a_done_card_onto_archived_offers_the_delete() {
    let tui = Tui::spawn(Options::default());
    let repo = tui.make_repo("drag-proj");
    tui.wait_for_text("create your first project");
    tui.add_project(&repo, "drag-proj");
    tui.send(b"K");
    launch(&tui, "tidy the docs");
    tui.wait_for_text("● 1 Done");

    // The card's own line (the pane header names the prompt too).
    let (row, col) = tui.find("┃ › tidy the").expect("the Done card");
    tui.send(&sgr_mouse(0, col, row, false));
    tui.send(&sgr_mouse(32, col, row + 1, false));
    tui.wait_for_text("let go here to archive");
    let (zone_row, zone_col) = tui.find("let go here to archive").unwrap();
    tui.send(&sgr_mouse(32, zone_col, zone_row, false));
    tui.send(&sgr_mouse(0, zone_col, zone_row, true));
    tui.wait_for_text("delete it from disk too?");
    tui.send(ENTER);
    tui.wait_for_text("● 0 Done");
}

/// A 20 KB paste into a live session arrives at the agent as one
/// bracketed paste, its line breaks inside it and nothing submitted early.
fn a_20_kb_paste_reaches_the_agent_as_one_paste() {
    let tui = Tui::spawn(Options::default());
    let repo = tui.make_repo("paste-proj");
    tui.wait_for_text("create your first project");
    tui.add_project(&repo, "paste-proj");
    launch(&tui, "paste here");
    tui.wait_for_text("echo ready");
    tui.send(ENTER);
    tui.wait_for_text("back to the card");

    let mut text = String::new();
    let mut i = 0;
    while text.len() < 20 * 1024 {
        text.push_str(&format!(
            "let value_{i} = compute(\"line {i}\", &[1, 2, 3]);\r"
        ));
        i += 1;
    }
    let paste = [&b"\x1b[200~"[..], text.as_bytes(), b"\x1b[201~"].concat();
    tui.send(&paste);
    tui.wait_for_text(&format!("paste {} bytes", text.len()));
    assert!(
        !tui.try_wait_for_text("submit", Duration::from_secs(1)),
        "the paste submitted:\n{}",
        tui.screen_text()
    );
    let got = std::fs::read(&tui.echo_log).unwrap();
    let starts = count(&got, b"\x1b[200~");
    let ends = count(&got, b"\x1b[201~");
    assert_eq!((starts, ends), (1, 1), "one bracketed paste");
    let inner = &got[6..got.len() - 6];
    assert_eq!(String::from_utf8_lossy(inner), text);
}

/// A large paste the host hands over with a pause before its last line
/// break (a busy console, a slow machine) still arrives as paste and never
/// submits: the tail after the pause is the paste's, not a typed Enter.
fn a_paste_with_a_pause_before_its_last_line_break_never_submits() {
    let tui = Tui::spawn(Options::default());
    let repo = tui.make_repo("pause-proj");
    tui.wait_for_text("create your first project");
    tui.add_project(&repo, "pause-proj");
    launch(&tui, "paste here");
    tui.wait_for_text("echo ready");
    tui.send(ENTER);
    tui.wait_for_text("back to the card");

    let mut text = String::new();
    let mut i = 0;
    while text.len() < 100 * 1024 {
        text.push_str(&format!("let value_{i} = compute(\"line {i}\");\r"));
        i += 1;
    }
    let (head, tail) = text.split_at(text.len() - 1);
    tui.send(&[&b"\x1b[200~"[..], head.as_bytes()].concat());
    std::thread::sleep(Duration::from_millis(400));
    tui.send(&[tail.as_bytes(), b"\x1b[201~"].concat());
    assert!(
        !tui.try_wait_for_text("submit", Duration::from_secs(8)),
        "the paste submitted:\n{}",
        tui.screen_text()
    );
    let got = std::fs::read(&tui.echo_log).unwrap();
    // One paste or two back to back, the agent got every byte and no Enter.
    let text_only = strip(&strip(&got, b"\x1b[200~"), b"\x1b[201~");
    assert_eq!(String::from_utf8_lossy(&text_only), text);
}

fn strip(hay: &[u8], needle: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(hay.len());
    let mut i = 0;
    while i < hay.len() {
        if hay[i..].starts_with(needle) {
            i += needle.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

/// What the agent printed, read in the grid's pane: every line whole, as
/// the daemon's ring has it — not cut short, not missing.
fn the_grid_pane_shows_every_line_the_agent_printed() {
    let tui = Tui::spawn(Options {
        script: format!("gate:asked;{FEATURE}"),
        ..Options::default()
    });
    let repo = tui.make_repo("pane-proj");
    tui.wait_for_text("create your first project");
    tui.add_project(&repo, "pane-proj");
    launch(&tui, "build the feature");
    tui.wait_for_agent("step gate:asked");
    tui.open_gate("asked");
    tui.wait_for_text("1 needs you");
    reply(&tui, "yes ship it");
    tui.wait_for_agent("step echo");
    pane_shows_the_feature(&tui);
}

/// The same session followed across the board and the grid as it works:
/// launched on the board, its question met on the grid, answered on the
/// board, and read back on the grid, every line whole.
fn the_pane_keeps_every_line_across_board_and_grid() {
    let tui = Tui::spawn(Options {
        script: format!("gate:asked;{FEATURE}"),
        ..Options::default()
    });
    let repo = tui.make_repo("swap-proj");
    tui.wait_for_text("create your first project");
    tui.add_project(&repo, "swap-proj");
    tui.send(b"K");
    tui.wait_for_text("Needs you");
    launch(&tui, "build the feature");
    tui.wait_for_text("● 1 Running");
    tui.wait_for_agent("step gate:asked");
    tui.send(b"K");
    tui.wait_for_gone("● 1 Running");
    tui.open_gate("asked");
    tui.wait_for_text("1 needs you");
    tui.send(b"K");
    tui.wait_for_text("● 1 Needs you");
    reply(&tui, "yes ship it");
    tui.wait_for_agent("step echo");
    tui.wait_for_text("● 1 Review");
    tui.send(b"K");
    tui.wait_for_gone("● 1 Review");
    pane_shows_the_feature(&tui);
    // And through a host resize, which repaints every cell.
    tui.host_repaint();
    pane_shows_the_feature(&tui);
}

/// A turn that ends on a usage limit (`StopFailure`) Claude then waits
/// out: running, and its card says when it carries on, in the words the
/// session's screen printed. Claude's resume clears it and the turn ends
/// as any other; a failed login after it waits on you, saying so.
fn a_usage_limit_reads_resumes_on_the_card_until_claude_carries_on() {
    let tui = Tui::spawn(Options {
        script: "fail:rate_limit;say:Continuing automatically at 3:45pm;gate:resumed;\
                 notify:quota_auto_resume_fired;stop;gate:relogin;\
                 fail:authentication_failed;echo"
            .into(),
        ..Options::default()
    });
    let repo = tui.make_repo("limit-proj");
    tui.wait_for_text("create your first project");
    tui.add_project(&repo, "limit-proj");
    launch(&tui, "build the feature");
    tui.wait_for_agent("step gate:resumed");
    tui.wait_for_text("resumes 3:45pm");
    tui.wait_for_text("1 running");
    tui.open_gate("resumed");
    tui.wait_for_gone("resumes 3:45pm");
    tui.wait_for_agent("step gate:relogin");
    tui.wait_for_text("agent-1  done");
    tui.open_gate("relogin");
    tui.wait_for_text("logged out");
    tui.wait_for_text("1 needs you");
}

/// Every line the FEATURE agent printed, whole, in the screen model. On a
/// miss: the screen's right half (the pane, in rows short enough that no
/// byte-counting `cut` of the log chops them), what the daemon holds, and
/// whether a host repaint healed it.
fn pane_shows_the_feature(tui: &Tui) {
    let lines = [
        "fake agent up",
        "working on: build the feature",
        "QUESTION: ship it? (reply and Enter)",
        "got reply: yes ship it",
        "edited src/feature.txt",
        "committed: add the feature",
        "fake agent finished",
        "echo ready",
    ];
    let whole = |s: &vt100::Screen| {
        let text = screen_to_text(s);
        lines.iter().all(|l| text.contains(l))
    };
    if !tui.try_wait_for(Duration::from_secs(5), whole) {
        let id = tui.agent_id("build the feature");
        let (cols, rows, ring) = daemon_tail(&tui.runtime_dir, &id);
        let pane: String = tui
            .screen_text()
            .lines()
            .map(|row| row.chars().skip(usize::from(COLS / 2)).collect::<String>())
            .map(|row| format!("{}\n", row.trim_end()))
            .collect();
        let raw = tui.save_raw("pane");
        // A host resize makes the TUI paint every cell again: a screen it
        // heals lost cells between the TUI and the host; one it leaves
        // wrong is wrong in the TUI's own emulation of the session.
        tui.host_repaint();
        let healed = tui.try_wait_for(Duration::from_secs(5), whole);
        panic!(
            "the pane lost a line (healed by a host repaint: {healed}; host bytes in {}):\n--- the screen's right half ---\n{pane}--- ring ({cols}x{rows}) ---\n{:?}\n--- agent.log ---\n{}",
            raw.display(),
            String::from_utf8_lossy(&ring),
            std::fs::read_to_string(&tui.agent_log).unwrap_or_default(),
        );
    }
}

// ---- steps ----

/// The quick prompt, `task` typed and launched.
fn launch(tui: &Tui, task: &str) {
    tui.send(b"p");
    tui.wait_for_text("what should the agent do?");
    tui.type_str(task);
    tui.wait_for_text(&format!("│ {task}"));
    tui.send(ENTER);
    tui.wait_for_gone("what should the agent do?");
}

/// Space on the card under the cursor, `text` typed and sent.
fn reply(tui: &Tui, text: &str) {
    tui.send(b" ");
    tui.wait_for_text("Follow-up · ");
    tui.type_str(text);
    tui.wait_for_text(&format!("│{text}"));
    tui.send(ENTER);
    tui.wait_for_gone("Follow-up · ");
}

/// A FEATURE session launched, its question answered, and its card in
/// Review with its commit. The board is up.
fn to_review(tui: &Tui) {
    launch(tui, "build the feature");
    tui.wait_for_text("● 1 Needs you");
    reply(tui, "yes ship it");
    tui.wait_for_text("● 1 Review");
    tui.wait_for_text("1 commit");
}

/// The repo's linked worktrees (every checkout but its own).
fn linked_worktrees(repo: &Path) -> Vec<PathBuf> {
    let list = git(repo, &["worktree", "list", "--porcelain"]);
    list.lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .map(PathBuf::from)
        .skip(1)
        .collect()
}

/// The linked worktree Kete cut for the session, the only one.
fn session_worktree(repo: &Path) -> PathBuf {
    let paths = linked_worktrees(repo);
    assert_eq!(paths.len(), 1, "one linked worktree: {paths:?}");
    let wt = paths[0].clone();
    let branch = git(&wt, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_ne!(
        branch.trim(),
        "main",
        "the session runs off the base branch"
    );
    wt
}

/// The PATH directories holding `gh`, for a TUI that must not find it.
fn gh_dirs() -> Vec<String> {
    let out = std::process::Command::new("where").arg("gh").output();
    let Ok(out) = out else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| Path::new(l.trim()).parent())
        .map(|p| p.display().to_string())
        .collect()
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}
