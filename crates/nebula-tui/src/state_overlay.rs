//! STATE MAP overlay (K10): a repo's state map as a tree you walk with the
//! keys. It is the TUI twin of `desktop/src/lib/state/StateMap.svelte`:
//! roots start open and everything below starts closed, the selection falls
//! back to the first row, and the extractor's note and a "no map yet" state
//! are said in words. The rules are `nebula_core::state_map`'s (`tree`,
//! `line`, `slice`); this file only holds the cursor and draws.
//!
//! Self-contained: the caller reads the map off the key path
//! (`state_map::read`), hands it to [`StateOverlay::new`] or
//! [`StateOverlay::reload`], feeds keys to [`StateOverlay::handle_key`] and
//! acts on the [`Outcome`] (close, re-read, copy the slice).

use crate::theme::Theme;
use crate::ui::{centered_rect_pct, truncate};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use nebula_core::clock::now_ms;
use nebula_core::state_map::{self, StateMap, TreeRow, MAP_PATH};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use std::collections::{HashMap, HashSet};

/// What a key asks of the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    None,
    Close,
    /// Read the map again and call [`StateOverlay::reload`].
    Reload,
    /// Put this text (a node's slice, at most 2 KB) on the clipboard.
    Copy(String),
}

pub struct StateOverlay {
    repo: String,
    read: Result<Option<StateMap>, String>,
    /// A node's open state where the user changed it; else roots are open.
    toggled: HashMap<String, bool>,
    /// The visible rows, rebuilt whenever the map or a toggle changes.
    rows: Vec<TreeRow>,
    cursor: usize,
    scroll: usize,
    /// Rows the body showed last draw: what PageUp and PageDown move by.
    page: usize,
    /// One line of news from the caller (a copy landed), until the next key.
    flash: Option<String>,
    theme: Theme,
}

/// The rows `read` shows: roots open, the rest as `toggled` says.
fn build(read: &Result<Option<StateMap>, String>, toggled: &HashMap<String, bool>) -> Vec<TreeRow> {
    let Ok(Some(map)) = read else {
        return Vec::new();
    };
    let roots: HashSet<&str> = state_map::tree(map, &|_| true)
        .iter()
        .filter(|r| r.depth == 0)
        .map(|r| map.nodes[r.node].id.as_str())
        .collect();
    state_map::tree(map, &|id: &str| {
        toggled
            .get(id)
            .copied()
            .unwrap_or_else(|| roots.contains(id))
    })
}

impl StateOverlay {
    /// `read` is `state_map::read(root)`, done off the key path.
    pub fn new(repo: String, read: Result<Option<StateMap>, String>) -> Self {
        let mut overlay = Self {
            repo,
            read,
            toggled: HashMap::new(),
            rows: Vec::new(),
            cursor: 0,
            scroll: 0,
            page: 10,
            flash: None,
            theme: Theme::default(),
        };
        overlay.rebuild(None);
        overlay
    }

    /// The colours of the running app; the default theme until set.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// Say something in the footer until the next key.
    pub fn flash(&mut self, text: impl Into<String>) {
        self.flash = Some(text.into());
    }

    /// A fresh read after [`Outcome::Reload`]: the selection and the toggles
    /// stay where their ids still exist.
    pub fn reload(&mut self, read: Result<Option<StateMap>, String>) {
        let keep = self.selected_id();
        self.read = read;
        self.flash = None;
        self.rebuild(keep);
    }

    fn rebuild(&mut self, keep: Option<String>) {
        self.rows = build(&self.read, &self.toggled);
        self.cursor = match (&self.read, keep) {
            (Ok(Some(map)), Some(id)) => self
                .rows
                .iter()
                .position(|r| map.nodes[r.node].id == id)
                .unwrap_or(0),
            _ => 0,
        };
    }

    fn selected_id(&self) -> Option<String> {
        let Ok(Some(map)) = &self.read else {
            return None;
        };
        Some(map.nodes.get(self.rows.get(self.cursor)?.node)?.id.clone())
    }

    /// Row `i` has children showing: the row under it sits deeper.
    fn open_at(&self, i: usize) -> bool {
        match (self.rows.get(i), self.rows.get(i + 1)) {
            (Some(row), Some(next)) => next.depth > row.depth,
            _ => false,
        }
    }

    fn set_open(&mut self, open: bool) {
        if let Some(id) = self.selected_id() {
            self.toggled.insert(id.clone(), open);
            self.rebuild(Some(id));
        }
    }

    /// Right: open the node, or step into its first child when it is open.
    fn open_or_enter(&mut self) {
        if !self.rows.get(self.cursor).is_some_and(|r| r.has_children) {
            return;
        }
        if self.open_at(self.cursor) {
            self.cursor += 1;
        } else {
            self.set_open(true);
        }
    }

    /// Left: close the node, or step to its parent when closed or a leaf.
    fn close_or_parent(&mut self) {
        let Some(row) = self.rows.get(self.cursor) else {
            return;
        };
        let (has_children, depth) = (row.has_children, row.depth);
        if has_children && self.open_at(self.cursor) {
            self.set_open(false);
        } else if let Some(parent) = self.rows[..self.cursor]
            .iter()
            .rposition(|r| r.depth < depth)
        {
            self.cursor = parent;
        }
    }

    fn toggle(&mut self) {
        if self.rows.get(self.cursor).is_some_and(|r| r.has_children) {
            let open = self.open_at(self.cursor);
            self.set_open(!open);
        }
    }

    fn copy(&self) -> Outcome {
        let (Ok(Some(map)), Some(id)) = (&self.read, self.selected_id()) else {
            return Outcome::None;
        };
        state_map::slice(map, &id, now_ms()).map_or(Outcome::None, Outcome::Copy)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        if key.kind == KeyEventKind::Release
            || key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return Outcome::None;
        }
        self.flash = None;
        let last = self.rows.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return Outcome::Close,
            KeyCode::Char('r') => return Outcome::Reload,
            KeyCode::Char('c') => return self.copy(),
            KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.cursor = (self.cursor + 1).min(last),
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(self.page),
            KeyCode::PageDown => self.cursor = (self.cursor + self.page).min(last),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = last,
            KeyCode::Right | KeyCode::Char('l') => self.open_or_enter(),
            KeyCode::Left | KeyCode::Char('h') => self.close_or_parent(),
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle(),
            _ => {}
        }
        Outcome::None
    }

    fn title(&self) -> String {
        match &self.read {
            Ok(Some(map)) => format!(
                " State map of {}, built {} ",
                self.repo,
                state_map::ago(now_ms(), map.built_at)
            ),
            _ => format!(" State map of {} ", self.repo),
        }
    }

    /// Keep the cursor on the screen: the body is `height` rows tall.
    fn follow(&mut self, height: u16) {
        self.page = usize::from(height).max(1);
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + self.page {
            self.scroll = self.cursor + 1 - self.page;
        }
        self.scroll = self.scroll.min(self.rows.len().saturating_sub(self.page));
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect) {
        let th = self.theme;
        let rect = centered_rect_pct(area, 96, 90);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(th.muted))
            .title(Span::styled(
                self.title(),
                Style::default().fg(th.text).add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(rect);
        f.render_widget(block, rect);
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let width = usize::from(inner.width);
        let has_rows = !self.rows.is_empty();
        let note = match &self.read {
            Ok(Some(map)) => map.note.clone().filter(|n| !n.trim().is_empty()),
            _ => None,
        };
        // The note gets up to two wrapped rows, the body keeps at least one.
        let note_h = note
            .as_ref()
            .map_or(0, |n| {
                (n.chars().count() + 1).div_ceil(width).clamp(1, 2) as u16
            })
            .min(inner.height.saturating_sub(2));
        let [body, note_area, foot] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(note_h),
            Constraint::Length(1),
        ])
        .areas(inner);
        self.follow(body.height);

        match &self.read {
            Err(e) => say(f, body, e, Style::default().fg(th.warn)),
            Ok(None) => say(
                f,
                body,
                &format!("No state map yet. The daemon writes {MAP_PATH} for a studio repo."),
                Style::default().fg(th.muted),
            ),
            Ok(Some(_)) if !has_rows => say(
                f,
                body,
                "This map has no nodes.",
                Style::default().fg(th.muted),
            ),
            Ok(Some(map)) => {
                let lines: Vec<Line> = self
                    .rows
                    .iter()
                    .enumerate()
                    .skip(self.scroll)
                    .take(usize::from(body.height))
                    .map(|(i, row)| {
                        let node = &map.nodes[row.node];
                        let mark = if !row.has_children {
                            "  "
                        } else if self.open_at(i) {
                            "\u{25be} "
                        } else {
                            "\u{25b8} "
                        };
                        let indent = " ".repeat((row.depth * 2).min(width / 2));
                        let lead = if i == self.cursor { "> " } else { "  " };
                        let text = truncate(
                            &format!("{lead}{indent}{mark}{}", state_map::line(node)),
                            width,
                        );
                        let mut style = Style::default();
                        // Ember means needs you, and only it, as on the
                        // desktop: a session waiting or an open question. The
                        // line says both in words, so colour is never the only sign.
                        if node.status == "needs_feedback" || node.questions > 0 {
                            style = style.fg(th.err);
                        }
                        if i == self.cursor {
                            style = style.bg(th.sel_bg).add_modifier(Modifier::BOLD);
                        }
                        Line::styled(text, style)
                    })
                    .collect();
                f.render_widget(Paragraph::new(lines), body);
            }
        }

        if let Some(note) = note.filter(|_| note_area.height > 0) {
            f.render_widget(
                Paragraph::new(Span::styled(
                    format!(" {note}"),
                    Style::default().fg(th.muted),
                ))
                .wrap(Wrap { trim: false }),
                note_area,
            );
        }

        let footer = match &self.flash {
            Some(text) => format!(" {text}"),
            None if has_rows => {
                " j/k move  l/h open/close  c copy slice  r reload  esc close".to_string()
            }
            None => " r reload  esc close".to_string(),
        };
        f.render_widget(
            Paragraph::new(Span::styled(
                truncate(&footer, width),
                Style::default().fg(th.dim),
            )),
            foot,
        );
    }
}

/// `text` wrapped over the body, in `style`.
fn say(f: &mut Frame, body: Rect, text: &str, style: Style) {
    f.render_widget(
        Paragraph::new(Span::styled(format!(" {text}"), style)).wrap(Wrap { trim: false }),
        body,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::state_map::{merge, Edge, EdgeKind, Layer, Node, NodeKind};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn node(id: &str, kind: NodeKind, label: &str, status: &str) -> Node {
        Node {
            id: id.into(),
            kind,
            label: label.into(),
            status: status.into(),
            owner: None,
            changed_at: None,
            questions: 0,
            links: Vec::new(),
        }
    }

    fn owns(from: &str, to: &str) -> Edge {
        Edge {
            from: from.into(),
            to: to.into(),
            kind: EdgeKind::Owns,
        }
    }

    /// A department owning a system owning a file.
    fn fixture() -> StateMap {
        merge(
            "C:/Code/game",
            now_ms(),
            Layer {
                nodes: vec![
                    node("dept:art", NodeKind::Department, "Art", ""),
                    node("SYS-1", NodeKind::System, "Renderer", "active"),
                    node("src/draw.rs", NodeKind::File, "Draw loop", ""),
                ],
                edges: vec![owns("dept:art", "SYS-1"), owns("SYS-1", "src/draw.rs")],
            },
            Layer::default(),
        )
    }

    /// One system owning `n` files with long labels.
    fn hub(n: usize) -> StateMap {
        let mut nodes = vec![node("hub", NodeKind::System, "Hub", "")];
        let mut edges = Vec::new();
        for i in 0..n {
            let id = format!("file-{i:03}");
            nodes.push(node(&id, NodeKind::File, &"a long label ".repeat(20), ""));
            edges.push(owns("hub", &id));
        }
        merge(
            "C:/Code/game",
            now_ms(),
            Layer { nodes, edges },
            Layer::default(),
        )
    }

    fn overlay(map: StateMap) -> StateOverlay {
        StateOverlay::new("game".into(), Ok(Some(map)))
    }

    fn press(o: &mut StateOverlay, code: KeyCode) -> Outcome {
        o.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn id(o: &StateOverlay) -> Option<String> {
        o.selected_id()
    }

    /// The screen after a draw at `w` by `h`, one string per row.
    fn screen(o: &mut StateOverlay, w: u16, h: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                o.draw(f, area)
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect()
    }

    fn text(o: &mut StateOverlay) -> String {
        screen(o, 80, 24).join("\n")
    }

    #[test]
    fn roots_start_open_and_the_rest_closed_at_80_by_24() {
        let mut o = overlay(fixture());
        let shown = text(&mut o);
        assert!(
            shown.contains("State map of game, built just now"),
            "{shown}"
        );
        assert!(shown.contains("department dept:art: Art"), "{shown}");
        assert!(shown.contains("system SYS-1: Renderer (active)"), "{shown}");
        assert!(!shown.contains("src/draw.rs"), "{shown}");
        assert!(shown.contains("esc close"), "{shown}");

        press(&mut o, KeyCode::Down);
        press(&mut o, KeyCode::Char('l'));
        let shown = text(&mut o);
        assert!(shown.contains("file src/draw.rs: Draw loop"), "{shown}");
    }

    #[test]
    fn keys_move_open_close_and_climb_to_the_parent() {
        let mut o = overlay(fixture());
        assert_eq!(id(&o).as_deref(), Some("dept:art"));
        assert_eq!(o.rows.len(), 2);

        press(&mut o, KeyCode::Down);
        assert_eq!(id(&o).as_deref(), Some("SYS-1"));
        press(&mut o, KeyCode::Char('k'));
        assert_eq!(id(&o).as_deref(), Some("dept:art"));
        // l on an open node steps into its first child.
        press(&mut o, KeyCode::Char('l'));
        assert_eq!(id(&o).as_deref(), Some("SYS-1"));
        // l on a closed node opens it and stays.
        press(&mut o, KeyCode::Right);
        assert_eq!((id(&o).as_deref(), o.rows.len()), (Some("SYS-1"), 3));
        press(&mut o, KeyCode::Right);
        assert_eq!(id(&o).as_deref(), Some("src/draw.rs"));
        // A leaf: l does nothing, h climbs to its parent.
        press(&mut o, KeyCode::Right);
        assert_eq!(id(&o).as_deref(), Some("src/draw.rs"));
        press(&mut o, KeyCode::Char('h'));
        assert_eq!(id(&o).as_deref(), Some("SYS-1"));
        // h on an open node closes it, on a closed one climbs.
        press(&mut o, KeyCode::Left);
        assert_eq!((id(&o).as_deref(), o.rows.len()), (Some("SYS-1"), 2));
        press(&mut o, KeyCode::Left);
        assert_eq!(id(&o).as_deref(), Some("dept:art"));
        // A root with nothing above it closes, and Enter and Space toggle.
        press(&mut o, KeyCode::Char('h'));
        assert_eq!(o.rows.len(), 1);
        press(&mut o, KeyCode::Enter);
        assert_eq!(o.rows.len(), 2);
        press(&mut o, KeyCode::Char(' '));
        assert_eq!(o.rows.len(), 1);
        press(&mut o, KeyCode::Enter);

        press(&mut o, KeyCode::End);
        assert_eq!(id(&o).as_deref(), Some("SYS-1"));
        press(&mut o, KeyCode::Home);
        assert_eq!(id(&o).as_deref(), Some("dept:art"));
        press(&mut o, KeyCode::PageDown);
        assert_eq!(id(&o).as_deref(), Some("SYS-1"));
        press(&mut o, KeyCode::PageUp);
        assert_eq!(id(&o).as_deref(), Some("dept:art"));
        // Past the ends stays put.
        press(&mut o, KeyCode::Up);
        assert_eq!(id(&o).as_deref(), Some("dept:art"));

        assert_eq!(press(&mut o, KeyCode::Esc), Outcome::Close);
        assert_eq!(press(&mut o, KeyCode::Char('q')), Outcome::Close);
        assert_eq!(press(&mut o, KeyCode::Char('r')), Outcome::Reload);
        assert_eq!(press(&mut o, KeyCode::Char('x')), Outcome::None);
        // A key with Ctrl held is not ours.
        let ctrl = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert_eq!(o.handle_key(ctrl), Outcome::None);
    }

    #[test]
    fn copy_is_the_selected_nodes_slice_within_2_kb() {
        let map = fixture();
        let mut o = overlay(map.clone());
        press(&mut o, KeyCode::Down);
        let Outcome::Copy(copied) = press(&mut o, KeyCode::Char('c')) else {
            panic!("c should copy");
        };
        assert_eq!(copied, state_map::slice(&map, "SYS-1", now_ms()).unwrap());
        assert!(copied.contains("system SYS-1"), "{copied}");
        assert!(copied.len() <= state_map::SLICE_MAX_BYTES);

        let big = hub(400);
        let mut o = overlay(big.clone());
        let Outcome::Copy(copied) = press(&mut o, KeyCode::Char('c')) else {
            panic!("c should copy");
        };
        assert_eq!(copied, state_map::slice(&big, "hub", now_ms()).unwrap());
        assert!(copied.len() <= 2048, "{} bytes", copied.len());

        let mut none = StateOverlay::new("game".into(), Ok(None));
        assert_eq!(press(&mut none, KeyCode::Char('c')), Outcome::None);
    }

    #[test]
    fn no_map_an_error_and_a_note_are_said_in_words() {
        let mut none = StateOverlay::new("game".into(), Ok(None));
        let shown = text(&mut none);
        assert!(shown.contains("No state map yet."), "{shown}");
        assert!(shown.contains(".kete/state-map.json"), "{shown}");

        let mut failed = StateOverlay::new(
            "game".into(),
            Err("could not read .kete/state-map.json: access denied".into()),
        );
        let shown = text(&mut failed);
        assert!(shown.contains("access denied"), "{shown}");

        let mut map = fixture();
        map.note = Some("no extractor named in .kete/studio.toml".into());
        let shown = text(&mut overlay(map));
        assert!(
            shown.contains("no extractor named in .kete/studio.toml"),
            "{shown}"
        );
        assert!(shown.contains("department dept:art"), "{shown}");
    }

    #[test]
    fn reload_keeps_the_selection_and_toggles_where_ids_survive() {
        let mut o = overlay(fixture());
        press(&mut o, KeyCode::Down);
        press(&mut o, KeyCode::Right);
        press(&mut o, KeyCode::Right);
        assert_eq!(id(&o).as_deref(), Some("src/draw.rs"));

        let mut next = fixture();
        next.built_at += 1;
        next.nodes
            .push(node("SYS-2", NodeKind::System, "Audio", ""));
        next.edges.push(owns("dept:art", "SYS-2"));
        o.reload(Ok(Some(next)));
        assert_eq!(id(&o).as_deref(), Some("src/draw.rs"));
        assert_eq!(o.rows.len(), 4);

        // The selected id is gone: back to the first row.
        let only_dept = merge(
            "C:/Code/game",
            now_ms(),
            Layer {
                nodes: vec![node("dept:art", NodeKind::Department, "Art", "")],
                edges: Vec::new(),
            },
            Layer::default(),
        );
        o.reload(Ok(Some(only_dept)));
        assert_eq!(id(&o).as_deref(), Some("dept:art"));

        o.reload(Err("gone".into()));
        assert_eq!(id(&o), None);
        o.reload(Ok(Some(fixture())));
        assert_eq!(id(&o).as_deref(), Some("dept:art"));
    }

    #[test]
    fn the_cursor_row_stays_on_screen() {
        let mut o = overlay(hub(40));
        press(&mut o, KeyCode::End);
        let shown = text(&mut o);
        assert!(shown.contains("file-039"), "{shown}");
        assert!(!shown.contains("file-000"), "{shown}");
        assert!(shown.contains("> "), "{shown}");
        press(&mut o, KeyCode::Home);
        let shown = text(&mut o);
        assert!(shown.contains("file-000"), "{shown}");
    }

    #[test]
    fn a_tiny_area_never_panics() {
        for (w, h) in [(10, 3), (1, 1), (2, 2), (80, 2), (3, 24)] {
            let mut map = fixture();
            map.note = Some("a note that is longer than the area".into());
            for mut o in [
                overlay(map),
                StateOverlay::new("game".into(), Ok(None)),
                StateOverlay::new("game".into(), Err("bad".into())),
                overlay(hub(30)),
            ] {
                screen(&mut o, w, h);
                press(&mut o, KeyCode::End);
                screen(&mut o, w, h);
            }
        }
    }
}
