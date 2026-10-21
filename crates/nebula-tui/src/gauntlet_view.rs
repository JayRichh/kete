//! The GAUNTLET VIEW overlay (K5): every verdict by kind, newest first,
//! with its evidence and the Studio app link.
//!
//! Read-only (decision 12): it reads the repo's sign-off files through
//! `nebula_core::studio::gauntlet` and links into the Studio app; ruling
//! on a verdict stays there. The reads are BACKGROUND READS: a job on the
//! blocking pool drops what it found into the view's inbox and wakes the
//! loop with a redraw, and the next draw or key lands it ([`sync`]). A
//! view with no `Jobs` (the unit tests) reads inline.
//!
//! Keys: `Tab` / `⇧Tab` kinds, `↑` `↓` rows, Enter the detail, `o` Open in
//! Studio, `/` filter, `r` reloads, Esc or `q` closes. A click on a row
//! is Enter on it (`activate`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::app::{App, Overlay};
use crate::filter_list::{FilterList, FilterListKey};
use crate::view_jobs::{Answer, Jobs};
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use nebula_core::studio::gauntlet::{self, EvidenceEntry, KindVerdicts};
use nebula_core::studio::manifest::{self, StudioManifest};
use nebula_core::studio::verdicts::Verdict;
use nebula_core::{ClientRequest, ProjectId};
use ratatui::layout::{Position, Rect};

/// What one read of the repo found.
#[derive(Debug, Default)]
pub struct Loaded {
    pub manifest: Option<StudioManifest>,
    /// Why there is nothing to show: no manifest, or one that does not
    /// parse.
    pub error: Option<String>,
    pub kinds: Vec<KindVerdicts>,
}

/// A finished read, waiting in the view's inbox.
#[derive(Debug)]
enum Landed {
    List(Loaded),
    Evidence(String, Vec<EvidenceEntry>),
}

/// The gauntlet overlay's state.
#[derive(Debug, Clone, Default)]
pub struct GauntletView {
    /// The repo it shows; None: the first studio repo.
    pub repo: Option<ProjectId>,
    /// The verdict it opened on, as (kind, id); taken by the first read.
    pub item: Option<(String, String)>,
    /// Where it last drew, for outside clicks.
    pub area: Rect,
    /// The repo's name, for the title.
    pub name: String,
    /// The repo's root; None when there is no studio repo to show.
    pub root: Option<PathBuf>,
    /// None until the first read lands. Shared, so the per-frame clone of
    /// the overlay copies a pointer, not every verdict.
    pub data: Option<Arc<Loaded>>,
    /// The kind tab that is on.
    pub tab: usize,
    /// The kind's rows: indices into its verdicts, newest first.
    pub list: FilterList<usize>,
    /// `/` was pressed: keys type into the filter.
    pub filtering: bool,
    /// The selected verdict's detail is up in place of the rows.
    pub detail: bool,
    pub detail_scroll: u16,
    /// Evidence listings by id; None while one is being read.
    pub evidence: HashMap<String, Option<Vec<EvidenceEntry>>>,
    /// The tab strip's cells and its row, as last drawn.
    pub tab_hits: Vec<(u16, u16)>,
    pub tabs_y: u16,
    /// Rows the body showed, for paging.
    pub page: u16,
    inbox: Arc<Mutex<Vec<Landed>>>,
}

impl GauntletView {
    /// The kind tab that is on.
    pub fn kind(&self) -> Option<&KindVerdicts> {
        self.data.as_ref()?.kinds.get(self.tab)
    }

    /// The verdict under the cursor.
    pub fn selected(&self) -> Option<&Verdict> {
        let i = *self.list.selected()?;
        self.kind()?.verdicts.get(i)
    }

    /// The Studio link for the selected verdict, when it has one.
    pub fn studio_link(&self) -> Option<String> {
        let v = self.selected()?;
        gauntlet::studio_link(self.data.as_ref()?.manifest.as_ref()?, &v.kind, &v.id)
    }

    /// Fill the rows from the kind that is on, through the filter.
    fn refilter(&mut self) {
        let labels: Vec<String> = self
            .kind()
            .map(|k| k.verdicts.iter().map(label).collect())
            .unwrap_or_default();
        self.list.set_items((0..labels.len()).collect());
        self.list.apply_filter_by(labels, |i| i);
    }

    fn show_tab(&mut self, tab: usize) {
        let n = self.data.as_ref().map_or(0, |d| d.kinds.len());
        if n == 0 {
            return;
        }
        self.tab = tab % n;
        self.list.cursor = 0;
        self.list.scroll = 0;
        self.detail = false;
        self.refilter();
    }

    /// Put the cursor on verdict `id` of the kind that is on.
    fn select_id(&mut self, id: &str) -> bool {
        let at = self
            .kind()
            .and_then(|k| k.verdicts.iter().position(|v| v.id == id));
        at.is_some_and(|i| self.list.move_to_item(i)) || self.selected().is_some_and(|v| v.id == id)
    }
}

/// What the filter matches on: id, word, by and the note's first line.
fn label(v: &Verdict) -> String {
    let note = v
        .note
        .as_deref()
        .and_then(|n| n.lines().next())
        .unwrap_or("");
    format!(
        "{} {} {} {note}",
        v.id,
        v.verdict,
        v.by.as_deref().unwrap_or("")
    )
}

/// A fresh view on `repo`, or on the first studio repo.
fn build(app: &App, repo: Option<ProjectId>, item: Option<(String, String)>) -> GauntletView {
    let project = match &repo {
        Some(id) => app.tree.projects.iter().find(|p| &p.id == id),
        None => {
            let studio = app.studio_repos.get(&app.tree.projects);
            app.tree.projects.iter().find(|p| studio.contains(&p.id))
        }
    };
    GauntletView {
        repo: project.map(|p| p.id.clone()).or(repo),
        item,
        name: project.map(|p| p.name.clone()).unwrap_or_default(),
        root: project.map(|p| p.repo_path.clone()),
        ..GauntletView::default()
    }
}

/// Open the gauntlet view, on `repo` when the verb came from one.
pub fn open(app: &mut App, repo: Option<ProjectId>) {
    let view = build(app, repo, None);
    show(app, view);
}

/// Open the gauntlet view on one verdict (the board's verdict cell).
pub fn open_on(app: &mut App, repo: Option<ProjectId>, kind: &str, id: &str) {
    let view = build(app, repo, Some((kind.to_string(), id.to_string())));
    show(app, view);
}

fn show(app: &mut App, view: GauntletView) {
    app.modals.overlay = Some(Overlay::Gauntlet(view));
    read_list(app);
    app.chrome.dirty = true;
}

/// The wake-up a job answers with: a "slow" notice on a ticket nobody
/// waits on touches nothing and redraws.
fn redraw() -> Answer {
    Answer::Slow {
        ticket: crate::view_jobs::ticket(),
    }
}

/// Run `work` on the blocking pool when there is one, else now; what it
/// returns goes to `inbox`.
fn run(
    jobs: Option<Jobs>,
    inbox: &Arc<Mutex<Vec<Landed>>>,
    work: impl FnOnce() -> Landed + Send + 'static,
) {
    let inbox = inbox.clone();
    let put = move |l: Landed| {
        if let Ok(mut q) = inbox.lock() {
            q.push(l);
        }
    };
    match jobs {
        Some(jobs) => jobs.run(move || {
            put(work());
            Some(redraw())
        }),
        None => put(work()),
    }
}

/// Read the repo's verdicts (again).
fn read_list(app: &mut App) {
    let jobs = app.jobs.view_jobs.clone();
    let Some(Overlay::Gauntlet(view)) = &mut app.modals.overlay else {
        return;
    };
    let Some(root) = view.root.clone() else {
        return;
    };
    run(jobs.clone(), &view.inbox, move || {
        Landed::List(match manifest::load(&root) {
            Ok(Some(m)) => Loaded {
                kinds: gauntlet::list(&root, &m),
                manifest: Some(m),
                error: None,
            },
            Ok(None) => Loaded {
                error: Some(format!("This repo has no {}.", manifest::MANIFEST_PATH)),
                ..Loaded::default()
            },
            Err(e) => Loaded {
                error: Some(e),
                ..Loaded::default()
            },
        })
    });
    sync(jobs, view);
}

/// Land what the inbox holds, then ask for the evidence the open detail
/// shows and nobody has read yet. Every draw and key runs it.
pub(crate) fn sync(jobs: Option<Jobs>, view: &mut GauntletView) {
    let landed: Vec<Landed> = view
        .inbox
        .lock()
        .map(|mut q| q.drain(..).collect())
        .unwrap_or_default();
    for l in landed {
        match l {
            Landed::List(loaded) => land_list(view, loaded),
            Landed::Evidence(id, entries) => {
                view.evidence.insert(id, Some(entries));
            }
        }
    }
    if !view.detail {
        return;
    }
    let (Some(root), Some(data), Some(v)) = (view.root.clone(), view.data.clone(), view.selected())
    else {
        return;
    };
    let Some(m) = data.manifest.clone() else {
        return;
    };
    let wanted: Vec<String> = v
        .evidence
        .iter()
        .filter(|id| !view.evidence.contains_key(*id))
        .cloned()
        .collect();
    for id in wanted {
        view.evidence.insert(id.clone(), None);
        let (root, m) = (root.clone(), m.clone());
        run(jobs.clone(), &view.inbox, move || {
            let entries = gauntlet::evidence(&root, &m, &id);
            Landed::Evidence(id, entries)
        });
    }
    // An inline read (no jobs) has landed already: take it now.
    if jobs.is_none() {
        let landed: Vec<Landed> = view
            .inbox
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default();
        for l in landed {
            if let Landed::Evidence(id, entries) = l {
                view.evidence.insert(id, Some(entries));
            }
        }
    }
}

/// A read landed: keep the kind and verdict that were on, or go to the
/// one the view opened on.
fn land_list(view: &mut GauntletView, loaded: Loaded) {
    let was = view
        .kind()
        .map(|k| k.kind.clone())
        .zip(view.selected().map(|v| v.id.clone()));
    let opened_on = view.item.take();
    let want = opened_on.clone().or(was);
    view.data = Some(Arc::new(loaded));
    view.evidence.clear();
    let tab = want.as_ref().and_then(|(kind, _)| {
        view.data
            .as_ref()?
            .kinds
            .iter()
            .position(|k| &k.kind == kind)
    });
    view.tab = tab.unwrap_or(view.tab);
    if view
        .data
        .as_ref()
        .is_some_and(|d| view.tab >= d.kinds.len())
    {
        view.tab = 0;
    }
    view.refilter();
    if let Some((_, id)) = &want {
        let found = tab.is_some() && view.select_id(id);
        // Opened on a verdict: its detail is what was asked for.
        if opened_on.is_some() {
            view.detail = found;
        }
    }
}

/// Enter on the selected row, or a click on it: its detail comes up, or
/// goes back to the rows.
pub(crate) fn activate(app: &mut App) {
    let jobs = app.jobs.view_jobs.clone();
    let Some(Overlay::Gauntlet(view)) = &mut app.modals.overlay else {
        return;
    };
    if view.selected().is_none() {
        return;
    }
    view.detail = !view.detail;
    view.detail_scroll = 0;
    sync(jobs, view);
    app.chrome.dirty = true;
}

/// `o`: the selected verdict's page in the Studio app, in the browser.
/// Only a link to this machine is built (`gauntlet::studio_link`).
pub(crate) fn open_studio(app: &mut App, out: &mut Vec<ClientRequest>) {
    let Some(Overlay::Gauntlet(view)) = &app.modals.overlay else {
        return;
    };
    match view.studio_link() {
        Some(url) => crate::event_loop::open_link(app, &url, out),
        None => app.chrome.flash = Some("This verdict has no Studio link".into()),
    }
}

/// Keys over the view.
pub fn handle_key(app: &mut App, key: KeyEvent, out: &mut Vec<ClientRequest>) {
    let jobs = app.jobs.view_jobs.clone();
    let Some(Overlay::Gauntlet(view)) = &mut app.modals.overlay else {
        return;
    };
    sync(jobs.clone(), view);
    app.chrome.dirty = true;
    let page = i64::from(view.page.max(1));
    if view.filtering {
        match key.code {
            KeyCode::Enter => view.filtering = false,
            _ => match view.list.handle_standard_key(&key, page) {
                FilterListKey::Close => view.filtering = false,
                FilterListKey::Cleared => {
                    view.filtering = false;
                    view.refilter();
                }
                FilterListKey::QueryChanged => view.refilter(),
                FilterListKey::Moved | FilterListKey::None => {}
            },
        }
        return;
    }
    match key.code {
        KeyCode::Esc if view.detail => view.detail = false,
        KeyCode::Esc if !view.list.query.is_empty() => {
            view.list.query.clear();
            view.refilter();
        }
        KeyCode::Esc | KeyCode::Char('q') => app.modals.overlay = None,
        KeyCode::Char('/') => {
            view.filtering = true;
            view.detail = false;
        }
        KeyCode::Tab => view.show_tab(view.tab + 1),
        KeyCode::BackTab => {
            let n = view.data.as_ref().map_or(1, |d| d.kinds.len().max(1));
            view.show_tab(view.tab + n - 1);
        }
        KeyCode::Up | KeyCode::Char('k') => step(view, -1),
        KeyCode::Down | KeyCode::Char('j') => step(view, 1),
        KeyCode::PageUp if view.detail => {
            view.detail_scroll = view.detail_scroll.saturating_sub(view.page)
        }
        KeyCode::PageDown if view.detail => {
            view.detail_scroll = view.detail_scroll.saturating_add(view.page)
        }
        KeyCode::PageUp => step(view, -page),
        KeyCode::PageDown => step(view, page),
        KeyCode::Home => step(view, i64::MIN / 2),
        KeyCode::End => step(view, i64::MAX / 2),
        KeyCode::Enter => activate(app),
        KeyCode::Char('o') => open_studio(app, out),
        KeyCode::Char('r') => read_list(app),
        _ => {}
    }
    if let Some(Overlay::Gauntlet(view)) = &mut app.modals.overlay {
        sync(jobs, view);
    }
}

fn step(view: &mut GauntletView, by: i64) {
    view.list.step(by);
    view.detail_scroll = 0;
}

/// The pointer over the view: a tab click turns to that kind, a row click
/// is Enter on it, the wheel moves the cursor. True: it took the event.
pub fn handle_mouse(app: &mut App, mouse: MouseEvent, out: &mut Vec<ClientRequest>) -> bool {
    let _ = out;
    let jobs = app.jobs.view_jobs.clone();
    let Some(Overlay::Gauntlet(view)) = &mut app.modals.overlay else {
        return false;
    };
    sync(jobs, view);
    let pos = Position::new(mouse.column, mouse.row);
    app.chrome.dirty = true;
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if pos.y == view.tabs_y {
                if let Some(i) = view
                    .tab_hits
                    .iter()
                    .position(|(a, b)| (*a..*b).contains(&pos.x))
                {
                    view.show_tab(i);
                }
            } else if !view.detail {
                if let Some(row) = view.list.hit(pos) {
                    view.list.select(row as i64);
                    activate(app);
                }
            }
        }
        MouseEventKind::ScrollDown => step(view, 1),
        MouseEventKind::ScrollUp => step(view, -1),
        _ => {}
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};
    use ratatui::backend::TestBackend;
    use std::path::Path;

    fn put(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    /// A studio repo with two kinds, a reconciled pair and an evidence
    /// folder, as the one project of an app with the view open on it.
    fn fixture() -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        put(
            r,
            ".kete/studio.toml",
            "version = 1\nsignoffs = \"rev/signoffs\"\nevidence = \"rev/evidence\"\n\
             studio_url = \"http://127.0.0.1:5173\"\n[kinds]\nart = \"art-and-3d\"\n[links]\nart = \"/art?id={id}\"\n",
        );
        put(
            r,
            "rev/signoffs/art/C02.json",
            r#"{"verdict":"split","by":"third-reviewer","at":"2026-10-19","note":"too soft\nsecond line","tags":["sky"],"evidence":["EV1"]}"#,
        );
        put(
            r,
            "rev/signoffs/art/C02-reconciled.json",
            r#"{"verdict":"approve"}"#,
        );
        put(
            r,
            "rev/signoffs/art/C05.json",
            r#"{"verdict":"approve","by":"Jay","at":"2026-10-20","note":"good"}"#,
        );
        put(
            r,
            "rev/signoffs/sound/tui.json",
            r#"{"verdict":"rejected","reviewer":"Jay","date":"2026-10-19","note":"not in the room"}"#,
        );
        put(r, "rev/evidence/EV1/shot.png", "12345");
        put(r, "rev/evidence/EV1/notes.txt", "x");
        let mut app = App::new();
        app.tree.projects.push(nebula_core::Project {
            id: ProjectId("p1".into()),
            name: "wheako".into(),
            repo_path: r.to_path_buf(),
            sort_order: 0,
        });
        open(&mut app, Some(ProjectId("p1".into())));
        (dir, app)
    }

    fn view(app: &App) -> &GauntletView {
        match &app.modals.overlay {
            Some(Overlay::Gauntlet(v)) => v,
            _ => panic!("the gauntlet is not open"),
        }
    }

    fn key(app: &mut App, code: KeyCode) {
        let mods = if code == KeyCode::BackTab {
            KeyModifiers::SHIFT
        } else {
            KeyModifiers::NONE
        };
        let ev = KeyEvent {
            code,
            modifiers: mods,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        handle_key(app, ev, &mut Vec::new());
    }

    fn draw(app: &mut App, w: u16, h: u16) -> String {
        let mut terminal = ratatui::Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| crate::ui::gauntlet_view::draw(f, app, f.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn it_lists_each_kind_newest_first_and_the_pair_once() {
        let (_dir, mut app) = fixture();
        let v = view(&app);
        let d = v.data.as_ref().unwrap();
        assert_eq!(
            d.kinds.iter().map(|k| k.kind.as_str()).collect::<Vec<_>>(),
            vec!["art", "sound"]
        );
        assert_eq!(v.selected().unwrap().id, "C05");
        key(&mut app, KeyCode::Down);
        assert_eq!(view(&app).selected().unwrap().id, "C02");
        assert_eq!(view(&app).list.len(), 2, "C02-reconciled folds into C02");
        key(&mut app, KeyCode::Tab);
        assert_eq!(view(&app).selected().unwrap().id, "tui");
        key(&mut app, KeyCode::BackTab);
        assert_eq!(view(&app).kind().unwrap().kind, "art");
    }

    #[test]
    fn enter_shows_the_detail_with_evidence_and_esc_goes_back() {
        let (_dir, mut app) = fixture();
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Enter);
        let v = view(&app);
        assert!(v.detail);
        let ev = v.evidence["EV1"].as_ref().unwrap();
        assert_eq!(
            ev.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec!["rev/evidence/EV1/notes.txt", "rev/evidence/EV1/shot.png"]
        );
        assert_eq!(
            v.studio_link().as_deref(),
            Some("http://127.0.0.1:5173/art?id=C02")
        );
        key(&mut app, KeyCode::Esc);
        assert!(!view(&app).detail);
        key(&mut app, KeyCode::Esc);
        assert!(app.modals.overlay.is_none());
    }

    #[test]
    fn o_opens_a_link_or_says_there_is_none() {
        let (_dir, mut app) = fixture();
        key(&mut app, KeyCode::Char('o'));
        assert!(app
            .chrome
            .flash
            .as_deref()
            .unwrap()
            .contains("127.0.0.1:5173/art?id=C05"));
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Char('o'));
        assert_eq!(
            app.chrome.flash.as_deref(),
            Some("This verdict has no Studio link")
        );
    }

    #[test]
    fn slash_filters_and_q_types_while_filtering() {
        let (_dir, mut app) = fixture();
        key(&mut app, KeyCode::Char('/'));
        for c in "soft".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        assert_eq!(view(&app).list.len(), 1);
        assert_eq!(view(&app).selected().unwrap().id, "C02");
        key(&mut app, KeyCode::Char('q'));
        assert!(app.modals.overlay.is_some(), "q types into the filter");
        key(&mut app, KeyCode::Esc);
        assert!(!view(&app).filtering);
        assert_eq!(view(&app).list.len(), 2, "Esc cleared the filter");
        key(&mut app, KeyCode::Char('q'));
        assert!(app.modals.overlay.is_none());
    }

    #[test]
    fn open_on_selects_that_verdict_with_its_detail() {
        let (dir, mut app) = fixture();
        let _ = dir;
        open_on(&mut app, Some(ProjectId("p1".into())), "art", "C02");
        let v = view(&app);
        assert_eq!(v.kind().unwrap().kind, "art");
        assert_eq!(v.selected().unwrap().id, "C02");
        assert!(v.detail);
        open_on(&mut app, Some(ProjectId("p1".into())), "sound", "tui");
        assert_eq!(view(&app).selected().unwrap().id, "tui");
    }

    #[test]
    fn a_repo_without_a_manifest_or_any_studio_repo_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new();
        app.tree.projects.push(nebula_core::Project {
            id: ProjectId("p1".into()),
            name: "plain".into(),
            repo_path: dir.path().to_path_buf(),
            sort_order: 0,
        });
        open(&mut app, Some(ProjectId("p1".into())));
        assert!(draw(&mut app, 80, 24).contains("no .kete/studio.toml"));
        open(&mut app, None);
        assert!(draw(&mut app, 80, 24).contains("No repo here has studio mode"));
    }

    #[test]
    fn draws_tabs_rows_and_detail_at_80x24_and_120x40() {
        for (w, h) in [(80, 24), (120, 40)] {
            let (_dir, mut app) = fixture();
            let text = draw(&mut app, w, h);
            assert!(text.contains("Gauntlet"), "{text}");
            assert!(text.contains("art 2") && text.contains("sound 1"), "{text}");
            assert!(text.contains("C05") && text.contains("C02"), "{text}");
            assert!(
                text.contains("● split") && text.contains("✓ approve"),
                "{text}"
            );
            assert!(!text.contains("C02-reconciled"), "{text}");
            assert!(text.contains("Enter detail"), "{text}");
            key(&mut app, KeyCode::Down);
            key(&mut app, KeyCode::Enter);
            let text = draw(&mut app, w, h);
            assert!(text.contains("too soft"), "{text}");
            assert!(text.contains("C02-reconciled.json"), "{text}");
            assert!(text.contains("shot.png") && text.contains("5 B"), "{text}");
            assert!(text.contains("sky"), "{text}");
            key(&mut app, KeyCode::Tab);
            let text = draw(&mut app, w, h);
            assert!(
                text.contains("✕ rejected") && text.contains("tui"),
                "{text}"
            );
        }
    }

    #[test]
    fn a_click_on_a_row_is_enter_on_it() {
        let (_a, mut by_key) = fixture();
        let (_b, mut by_click) = fixture();
        draw(&mut by_key, 100, 30);
        let text = draw(&mut by_click, 100, 30);
        key(&mut by_key, KeyCode::Down);
        key(&mut by_key, KeyCode::Enter);
        // The C02 row's cell, wherever the layout put it.
        let (x, y) = text
            .lines()
            .enumerate()
            .find_map(|(y, l)| {
                l.find("C02")
                    .map(|b| (l[..b].chars().count() as u16, y as u16))
            })
            .unwrap();
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        };
        assert!(handle_mouse(&mut by_click, click, &mut Vec::new()));
        let (k, c) = (view(&by_key), view(&by_click));
        assert_eq!(k.selected().map(|v| &v.id), c.selected().map(|v| &v.id));
        assert_eq!((k.detail, k.tab), (c.detail, c.tab));
        assert_eq!(
            k.evidence.keys().collect::<Vec<_>>(),
            c.evidence.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_click_on_a_tab_turns_to_it() {
        let (_dir, mut app) = fixture();
        let text = draw(&mut app, 100, 30);
        let (x, y) = text
            .lines()
            .enumerate()
            .find_map(|(y, l)| {
                l.find("sound 1")
                    .map(|b| (l[..b].chars().count() as u16, y as u16))
            })
            .unwrap();
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x + 1,
            row: y,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut app, click, &mut Vec::new());
        assert_eq!(view(&app).kind().unwrap().kind, "sound");
    }
}
