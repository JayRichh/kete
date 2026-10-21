//! LEARN IN PLACE: the copy both apps teach with, kept here beside the
//! verbs so the two read the same and name the same keys.
//!
//! - [`TIPS`]: one-line FIRST-USE TIPS, each shown once the first time a
//!   user meets its concept (the `?` menu, the scope chip, the Review and
//!   Ship lanes, drag on the board, the review). Seen-state is the
//!   config's `tips_seen`, shared by both apps; `show_tips` turns them all
//!   off. [`tip_due`] says whether one may show now.
//! - [`WHATS_NEW`] and [`KEYS_CHANGED`]: the WHAT'S NEW page, shown once
//!   after an upgrade ([`whats_new_due`], `whats_new_seen` in the config)
//!   and reachable from Help and the palette after.
//! - [`EMPTY`]: what an empty lane, list or view says, and the one verb
//!   that fills it.
//!
//! Copy names keys as `{verb_id}` (or `{verb_id|words}`): each app fills
//! in the chord the user has bound to that registry verb ([`fill`]), or
//! the words after `|` when it has none, so a rebind never leaves a tip
//! naming the wrong key.

use crate::Config;

/// One first-use tip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tip {
    /// Stable: stored in the config's `tips_seen`. Never rename one.
    pub id: &'static str,
    /// One short line, `{verb}` placeholders filled by [`fill`].
    pub text: &'static str,
}

/// The `?` verbs menu, the first time a row or card is selected.
pub const TIP_VERBS: &str = "verbs";
/// The scope chip, the first time a scope is set or the board opens over
/// more than one repo.
pub const TIP_SCOPE: &str = "scope";
/// The Review and Ship lanes, the first time a card reaches Review.
pub const TIP_LANES: &str = "lanes";
/// Drag on the board, the first time it opens with two cards or more.
pub const TIP_DRAG: &str = "drag";
/// The review unit, the first time one opens.
pub const TIP_REVIEW: &str = "review";

/// The longest a tip runs, its keys filled in: what the terminal app's
/// footer row holds at 80 columns beside its two buttons.
pub const TIP_MAX: usize = 54;

pub const TIPS: &[Tip] = &[
    Tip {
        id: TIP_VERBS,
        text: "{verbs|The verbs menu} lists what you can do here, with keys.",
    },
    Tip {
        id: TIP_SCOPE,
        text: "The scope chip narrows the board; clear it to see all.",
    },
    Tip {
        id: TIP_LANES,
        text: "Review: finished work. Drag it to Ship to open a PR.",
    },
    Tip {
        id: TIP_DRAG,
        text: "Drag cards across lanes: Ship opens a PR, Done merges.",
    },
    Tip {
        id: TIP_REVIEW,
        text: "The row above the diff: reply, PR, merge, archive.",
    },
];

/// The tip with this id.
pub fn tip(id: &str) -> Option<&'static Tip> {
    TIPS.iter().find(|t| t.id == id)
}

/// Whether the tip `id` may show now: tips are on, it hasn't been shown,
/// and nothing else is up (`busy`: another tip, or the What's new page).
/// A tip that waits on `busy` is not marked seen, so it shows next time
/// its concept comes round.
pub fn tip_due(cfg: &Config, id: &str, busy: bool) -> bool {
    cfg.show_tips && !busy && !cfg.tips_seen.contains(id) && tip(id).is_some()
}

/// `text` with each `{id}` / `{id|words}` replaced: by `key(id)`, the
/// user's chord for that registry verb, else by `words` (else the id).
pub fn fill(text: &str, key: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let inner = &rest[open + 1..open + close];
        let (id, words) = inner.split_once('|').unwrap_or((inner, inner));
        out.push_str(&key(id).unwrap_or_else(|| words.to_string()));
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

// ---- What's new ----

/// One release's part of the WHAT'S NEW page.
#[derive(Debug, Clone, Copy)]
pub struct Release {
    /// `0.52`: major.minor.
    pub version: &'static str,
    /// The release in a line.
    pub title: &'static str,
    /// What changed, a line each, the most useful first.
    pub items: &'static [&'static str],
}

/// The page, newest first, rewritten from CHANGELOG.md for
/// reading in the app.
pub const WHATS_NEW: &[Release] = &[
    Release {
        version: "0.60",
        title: "A studio of agents",
        items: &[
            "kete studio enable <repo> lets the orchestrator work through an inbox when it is idle, within the autonomy level you set (A0 to A3).",
            "Role cards in .kete/roles.toml set a session's model, tools, prompt and permissions; kete roles check shows them.",
            "The Studio board, state map and Gauntlet view read your repo's notes and verdicts; the triage strip files a note in one line.",
            "kete usage --csv exports tokens, estimated cost and tool uses per session per day.",
            "kete orchestrator renew --from <note> hands the orchestrator over to a fresh session.",
        ],
    },
    Release {
        version: "0.56",
        title: "Sessions from sessions",
        items: &[
            "Start a follow-up as its own session, or Ask aside: both nest under the session they came from.",
            "Parent rows count their nested sessions and subagents: working, needs you, done.",
            "Subagent rows show the model and effort they run at, when Claude reports them.",
            "Moving a session says which uncommitted files and unpushed commits stay behind.",
            "The quick prompt offers recent checkouts; the desktop's git state and Details rail keep up within seconds.",
        ],
    },
    Release {
        version: "0.54",
        title: "Your tools, your menus, your day",
        items: &[
            "Open files and checkouts in installed VS Code or Visual Studio, or dock an editor beside Kete.",
            "Right-click for contextual actions and searchable submenus; browse Files and drag paths onto sessions.",
            "Choose Workshop, One Dark, One Light or VS Code themes, with your own fonts and sizes.",
            "Whakaoho schedules optional account wake-ups beside usage windows; cards show limits and sign-in needs.",
            "Discover optional critters in Settings or the palette; pet, feed, name and collect visitors.",
        ],
    },
    Release {
        version: "0.52",
        title: "Learn in place",
        items: &[
            "A one-line tip the first time you meet the ? menu, the scope chip, the Review and Ship lanes, drag, or the review. Settings, General, Show tips turns them off.",
            "Every empty lane and list says what goes there and the one thing to do.",
            "The footer and the verb bar show the selection's top verbs and ? more. The desktop's Help (F1) groups keys by task.",
            "A session's changes are counted per turn, so two sessions in one worktree no longer mix.",
        ],
    },
    Release {
        version: "0.51",
        title: "Which session made a change",
        items: &[
            "The review switches between This session and All in this worktree.",
            "A worktree remembers the branch it was cut from; the review and the lanes use it.",
            "Large pastes on Windows arrive in seconds, with pasting... N KB meanwhile.",
            "Export and import settings, delete merged worktrees, and vote on ADO PRs from the apps.",
        ],
    },
    Release {
        version: "0.50",
        title: "The board follows the work",
        items: &[
            "Lanes: Running, Needs you, Review, Ship, Done. Review is finished work no PR carries; Ship has an open PR.",
            "Drops do what the lane means: Review to Ship opens the PR form, Ship to Done merges after a confirm.",
            "The scope chip (all, repo, worktree) narrows the board, attention and quick prompt.",
            "Open a Review or Ship card to dock its diff beside the board, with Reply, PR, Merge, Archive.",
        ],
    },
    Release {
        version: "0.49",
        title: "One list of verbs",
        items: &[
            "? on anything shows its verbs with their keys; type to filter, Enter to run. F1 is every key.",
            "The footer and the desktop's verb bar follow the selection.",
            "Rebind the desktop's keys in Settings, Hotkeys.",
            "The diff keeps an agent's committed work: it compares against the base branch.",
        ],
    },
];

/// One row of the page's keys-changed table (0.49).
#[derive(Debug, Clone, Copy)]
pub struct KeyChange {
    pub verb: &'static str,
    pub tui: &'static str,
    pub desktop: &'static str,
}

pub const KEYS_CHANGED: &[KeyChange] = &[
    KeyChange {
        verb: "Verbs menu",
        tui: "? (help moved to F1)",
        desktop: "? (help moved to F1)",
    },
    KeyChange {
        verb: "Diff",
        tui: "d (g still works)",
        desktop: "d (Ctrl+Shift+D still works)",
    },
    KeyChange {
        verb: "Delete",
        tui: "Del only",
        desktop: "Del",
    },
    KeyChange {
        verb: "Reply to the agent",
        tui: "Space",
        desktop: "Space (was f)",
    },
    KeyChange {
        verb: "Open a session",
        tui: "Enter",
        desktop: "Enter (Space no longer opens)",
    },
    KeyChange {
        verb: "Filter",
        tui: "f",
        desktop: "f or /",
    },
    KeyChange {
        verb: "Digits",
        tui: "repos 1-9",
        desktop: "repos 1-9 in rail order",
    },
    KeyChange {
        verb: "Full screen",
        tui: "z",
        desktop: "z",
    },
];

/// This build's release as the page and `whats_new_seen` spell it:
/// major.minor of nebula-config's version, which both apps link.
pub fn current_version() -> String {
    minor_of(env!("CARGO_PKG_VERSION"))
}

/// `0.52.3` → `0.52`; a version with no dot is itself.
pub fn minor_of(version: &str) -> String {
    let mut parts = version.trim().splitn(3, '.');
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor)) => format!("{major}.{minor}"),
        _ => version.trim().to_string(),
    }
}

fn parse(version: &str) -> Option<(u32, u32)> {
    let v = minor_of(version);
    let (a, b) = v.split_once('.')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

/// Whether the WHAT'S NEW page shows: this is an upgrade, and the page
/// has a part for a release after the one last shown (`seen`), up to
/// `current`. `fresh`: no settings file was there before this run,
/// so this is a first install, not an upgrade: nothing shows (the caller
/// records `current` as seen). An unreadable `seen` (a hand edit) counts
/// as older; an empty one, on an existing install, as before 0.49.
pub fn whats_new_due(seen: &str, current: &str, fresh: bool) -> bool {
    if fresh {
        return false;
    }
    let Some(now) = parse(current) else {
        return false;
    };
    // A part newer than the one last shown, and not after this release:
    // a release with no part of its own (0.53 over 0.52) shows nothing.
    let was = parse(seen);
    WHATS_NEW
        .iter()
        .any(|r| parse(r.version).is_some_and(|v| v <= now && was.is_none_or(|was| was < v)))
}

/// Whether none of `marks` exists: a first run, not an upgrade. See
/// [`fresh_install`] for the marks an earlier run leaves.
pub fn first_run(marks: &[&std::path::Path]) -> bool {
    marks.iter().all(|p| !p.exists())
}

/// Whether this machine has never run kete: no settings file in either
/// layer and no daemon database. A user who never changed a setting has
/// no config.json, but every earlier run left the database; checked
/// before the daemon is reached, so its first start doesn't count.
pub fn fresh_install() -> bool {
    let (path, local) = Config::layer_paths();
    first_run(&[&path, &local, &nebula_core::paths::db_path()])
}

// ---- Empty states ----

/// What an empty lane, list or view says: one line, and the verb that
/// fills it (its key and button named by each app), if there is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Empty {
    /// Stable id each app looks its copy up by.
    pub id: &'static str,
    pub text: &'static str,
    /// The registry verb that fills it, by id.
    pub verb: Option<&'static str>,
    /// What that verb does here, in a word or two, for its button or key
    /// hint (`p: new session`). Empty with no verb.
    pub does: &'static str,
}

pub const EMPTY: &[Empty] = &[
    Empty {
        id: "lane_running",
        text: "Agents at work show here.",
        verb: Some("quick_prompt"),
        does: "new session",
    },
    Empty {
        id: "lane_needs",
        text: "Nothing needs you.",
        verb: None,
        does: "",
    },
    Empty {
        id: "lane_review",
        text: "Finished work to read lands here.",
        verb: None,
        does: "",
    },
    Empty {
        id: "lane_ship",
        text: "Cards with an open PR. Drag one in from Review.",
        verb: None,
        does: "",
    },
    Empty {
        id: "lane_done",
        text: "Merged and read work rests here.",
        verb: None,
        does: "",
    },
    Empty {
        id: "board",
        text: "No sessions yet: start one and it shows on the board.",
        verb: Some("quick_prompt"),
        does: "new session",
    },
    Empty {
        id: "no_repos",
        text: "Add a repo to start: its worktrees and sessions show here.",
        verb: Some("add_project"),
        does: "add a repo",
    },
    Empty {
        id: "no_sessions",
        text: "No sessions in this worktree yet.",
        verb: Some("quick_prompt"),
        does: "new session",
    },
    Empty {
        id: "overview",
        text: "Every session you start shows here under its repo, the ones waiting on you first.",
        verb: Some("quick_prompt"),
        does: "new session",
    },
    Empty {
        id: "accounts",
        text: "No accounts yet: add one to run sessions on another login.",
        verb: Some("accounts"),
        does: "accounts",
    },
    Empty {
        id: "search",
        text: "Nothing matches. Try fewer letters.",
        verb: None,
        does: "",
    },
];

/// The empty state with this id.
pub fn empty(id: &str) -> Option<&'static Empty> {
    EMPTY.iter().find(|e| e.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config::default()
    }

    #[test]
    fn a_tip_is_due_once_then_seen() {
        let mut c = cfg();
        assert!(tip_due(&c, TIP_VERBS, false), "on by default, not seen");
        c.tips_seen.insert(TIP_VERBS.into());
        assert!(!tip_due(&c, TIP_VERBS, false), "shown once");
        assert!(tip_due(&c, TIP_SCOPE, false), "the others still wait");
    }

    #[test]
    fn a_tip_waits_while_another_is_up_and_is_not_marked() {
        let c = cfg();
        assert!(!tip_due(&c, TIP_DRAG, true));
        assert!(!c.tips_seen.contains(TIP_DRAG));
        assert!(tip_due(&c, TIP_DRAG, false), "due again once nothing is up");
    }

    #[test]
    fn show_tips_off_silences_every_tip() {
        let mut c = cfg();
        c.show_tips = false;
        assert!(TIPS.iter().all(|t| !tip_due(&c, t.id, false)));
    }

    #[test]
    fn an_unknown_tip_is_never_due() {
        assert!(!tip_due(&cfg(), "nope", false));
    }

    #[test]
    fn tip_ids_are_unique_and_fill_cleanly() {
        let mut ids: Vec<&str> = TIPS.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), TIPS.len());
        for t in TIPS {
            let text = fill(t.text, |_| None);
            assert!(!text.contains('{') && !text.contains('|'), "{}", t.id);
            // The terminal app's footer row at 80 columns holds it beside
            // its buttons, its keys filled in.
            let keyed = fill(t.text, |_| Some("?".into()));
            assert!(keyed.chars().count() <= TIP_MAX, "{} is long", t.id);
        }
    }

    #[test]
    fn placeholders_take_every_tip_verb_from_the_registry() {
        for t in TIPS {
            let mut rest = t.text;
            while let Some(open) = rest.find('{') {
                let close = rest[open..].find('}').unwrap();
                let inner = &rest[open + 1..open + close];
                let id = inner.split('|').next().unwrap();
                assert!(
                    crate::actions::verbs().iter().any(|v| v.id == id),
                    "{} names {id}",
                    t.id
                );
                rest = &rest[open + close + 1..];
            }
        }
        for e in EMPTY {
            assert_eq!(e.verb.is_some(), !e.does.is_empty(), "{}", e.id);
            if let Some(v) = e.verb {
                assert!(
                    crate::actions::verbs().iter().any(|x| x.id == v),
                    "{} names {v}",
                    e.id
                );
            }
        }
    }

    #[test]
    fn fill_names_the_bound_key_or_the_fallback() {
        let text = "{verbs|The verbs menu} lists it.";
        assert_eq!(
            fill(text, |id| (id == "verbs").then(|| "?".into())),
            "? lists it."
        );
        assert_eq!(fill(text, |_| None), "The verbs menu lists it.");
        assert_eq!(fill("{d} then", |_| None), "d then");
        assert_eq!(fill("open { brace", |_| None), "open { brace");
    }

    #[test]
    fn whats_new_shows_once_after_an_upgrade() {
        assert!(whats_new_due("0.51", "0.52", false), "an upgrade");
        assert!(
            whats_new_due("", "0.52", false),
            "an install from before the key"
        );
        assert!(!whats_new_due("0.52", "0.52", false), "already shown");
        assert!(!whats_new_due("0.53", "0.52", false), "a downgrade");
        assert!(
            whats_new_due("0.51", "0.52.4", false),
            "patch versions read as their minor"
        );
        assert!(!whats_new_due("0.52", "0.52.4", false));
        assert!(
            whats_new_due("junk", "0.52", false),
            "a bad value counts as older"
        );
        assert!(
            !whats_new_due("0.52", "0.53", false),
            "a release with no part of its own shows nothing new"
        );
        assert!(
            whats_new_due("0.50", "0.53", false),
            "parts newer than the one shown still show"
        );
    }

    /// The settings a 0.48 install left (`crates/nebula/tests/fixtures/
    /// upgrade`): no seen-state, so the page is due once and every tip may
    /// show; seen, it is not due again.
    #[test]
    fn a_048_config_sees_whats_new_once_and_every_tip() {
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../nebula/tests/fixtures/upgrade/v0.48/data/config.json"
        );
        let dir = tempfile::tempdir().unwrap();
        let mut c = Config::load_at(std::path::Path::new(fixture), &dir.path().join("none.json"));
        assert!(c.skipped.is_empty(), "{:?}", c.skipped);
        assert!(whats_new_due(&c.whats_new_seen, "0.53", false));
        for tip in TIPS {
            assert!(tip_due(&c, tip.id, false), "{}", tip.id);
        }
        c.whats_new_seen = "0.53".into();
        assert!(!whats_new_due(&c.whats_new_seen, "0.53", false));
    }

    #[test]
    fn a_first_install_sees_no_whats_new() {
        assert!(!whats_new_due("", "0.52", true));
    }

    #[test]
    fn whats_new_needs_a_page_for_the_release() {
        assert!(
            !whats_new_due("", "0.40", false),
            "nothing at or before 0.40"
        );
    }

    #[test]
    fn versions_read_as_major_minor() {
        assert_eq!(minor_of("0.52.0"), "0.52");
        assert_eq!(minor_of("1.2"), "1.2");
        assert_eq!(minor_of("7"), "7");
        assert_eq!(current_version(), minor_of(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn the_page_runs_newest_first_and_stays_short() {
        let order: Vec<_> = WHATS_NEW.iter().map(|r| parse(r.version)).collect();
        let mut sorted = order.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(order, sorted);
        for r in WHATS_NEW {
            assert!(r.items.len() <= 5, "{} is long", r.version);
        }
        assert!(KEYS_CHANGED.len() <= 10);
    }

    #[test]
    fn first_run_is_no_settings_file_and_no_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        let db = dir.path().join("nebula.db");
        assert!(first_run(&[&path, &local, &db]));
        std::fs::write(&local, "{}").unwrap();
        assert!(!first_run(&[&path, &local, &db]));
        std::fs::remove_file(&local).unwrap();
        std::fs::write(&db, "").unwrap();
        assert!(!first_run(&[&path, &local, &db]), "a database: an upgrade");
    }

    #[test]
    fn the_seen_state_round_trips_and_stays_out_while_empty() {
        let c = cfg();
        let json = serde_json::to_value(&c).unwrap();
        assert!(json.get("tips_seen").is_none());
        assert!(json.get("whats_new_seen").is_none());
        assert_eq!(json.get("show_tips"), Some(&serde_json::Value::Bool(true)));
        let mut c = cfg();
        c.tips_seen.insert(TIP_LANES.into());
        c.whats_new_seen = "0.52".into();
        let back: Config = serde_json::from_value(serde_json::to_value(&c).unwrap()).unwrap();
        assert!(back.tips_seen.contains(TIP_LANES));
        assert_eq!(back.whats_new_seen, "0.52");
        let old: Config = serde_json::from_str("{}").unwrap();
        assert!(old.show_tips, "a config predating the key shows tips");
    }
}
