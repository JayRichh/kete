//! The desktop window's own preferences, kept under one `desktop` object
//! in `config.json` that [`crate::Config`] has no field for: an unknown key
//! rides through the TUI's saves untouched, and the window never writes a
//! TUI key for them — its light/dark choice is not the TUI's `theme`. The
//! TUI reads the editor choices here and writes one thing: a repo's
//! `.kete.json` Open command the user allowed ([`open_allowed_in`]), the
//! same approval the window keeps, so a command allowed in one app runs in
//! the other.
//!
//! Most preferences are a word off a fixed list ([`choices`]). The look
//! has three more shapes, each checked as strictly: a font family (free
//! text, [`crate::look::font_family_ok`], empty for Kete's own), an
//! imported theme (`import:<id>`, which must be in [`DesktopPrefs::themes`]),
//! and the imported themes themselves, kept as the palettes they were
//! derived into ([`crate::look::Palette`]), never as the theme files.

use crate::look::{self, Palette};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// The top-level key the preferences live under.
pub const KEY: &str = "desktop";

/// The window theme: Kete's Workshop following the OS, or pinned light or
/// dark; One Dark and One Light; VS Code's own theme, followed live. An
/// imported theme is `import:<id>` ([`IMPORT_PREFIX`]) besides these.
pub const THEMES: &[&str] = &["system", "light", "dark", "one-dark", "one-light", "vscode"];
/// What an imported theme's `theme` value starts with.
pub const IMPORT_PREFIX: &str = "import:";
/// The most imported themes kept: each is a few KiB of config.json.
pub const MAX_IMPORTED: usize = 12;
/// The window's text size, 14 its own: the whole window scales to it.
pub const UI_SIZES: &[&str] = &["12", "13", "14", "15", "16"];
/// The terminal's font size.
pub const TERM_SIZES: &[&str] = &[
    "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20",
];
/// The diff and file viewer's font size; 12.5 the window's own.
pub const EDITOR_SIZES: &[&str] = &["10", "11", "12", "12.5", "13", "14", "15", "16", "17", "18"];
/// The terminal's line height, as a multiple of its font size.
pub const TERM_LINES: &[&str] = &["1", "1.12", "1.2", "1.3", "1.4", "1.5"];
/// The look's fixed-list keys, in the order the Appearance group shows
/// them, with their defaults.
const SIZE_DEFAULTS: [(&str, &str); 4] = [
    ("ui_size", "14"),
    ("term_size", "13"),
    ("editor_size", "12.5"),
    ("term_line", "1.12"),
];
/// The font preferences: a family name, or empty for Kete's own.
pub const FONT_KEYS: [&str; 3] = ["font_ui", "font_mono", "font_term"];
/// The sidebar's narrowest and widest width in px; the window clamps a
/// drag to them and a hand edit of the saved width is clamped here.
pub const SIDEBAR_MIN: u16 = 200;
pub const SIDEBAR_MAX: u16 = 480;
/// The sidebar's width with none saved: the 220 px it was, and 24 more.
pub const SIDEBAR_DEFAULT: u16 = 244;
/// The Diff view's layout for a repo with none saved yet.
pub const DIFF_MODES: &[&str] = &["side", "inline", "replacement"];
/// What the main column shows with no view open.
pub const HOMES: &[&str] = &["overview", "board"];
/// What the window opens on: its home view, or the orchestrator.
pub const STARTUPS: &[&str] = &["home", "orchestrator"];
/// What opens a folder (a checkout, a repo): the editor found (VS Code
/// first), VS Code, Visual Studio, or the project's own Open command.
pub const EDITOR_FOLDERS: &[&str] = &["auto", "vscode", "visual-studio", "command"];
/// What opens a file: the editor found, VS Code, Visual Studio, or Kete's
/// own viewer.
pub const EDITOR_FILES: &[&str] = &["auto", "vscode", "visual-studio", "kete"];
/// Whether VS Code opens a folder in the window it last used (`-r`)
/// rather than a window of its own.
pub const EDITOR_REUSE: &[&str] = &["off", "on"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopPrefs {
    pub theme: String,
    pub diff_mode: String,
    pub home: String,
    /// Open in editor commands from a repo's .kete.json the user has let
    /// run, by repo path: the same command doesn't ask again.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub approved_open: BTreeMap<String, Vec<String>>,
    /// What the window opens on (`home` or `orchestrator`).
    pub startup: String,
    /// Overview groups the user folded: `repo:<project id>` and
    /// `wt:<worktree id>`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub collapsed: Vec<String>,
    /// The sidebar's width in px, [`SIDEBAR_MIN`] to [`SIDEBAR_MAX`]; 0 is
    /// [`SIDEBAR_DEFAULT`], and is not written.
    #[serde(skip_serializing_if = "is_zero")]
    pub sidebar_width: u16,
    /// The sidebar is folded to its icon rail.
    #[serde(skip_serializing_if = "is_false")]
    pub sidebar_collapsed: bool,
    /// What Open in editor uses on a folder ([`EDITOR_FOLDERS`]).
    pub editor_folder: String,
    /// What Open in editor uses on a file ([`EDITOR_FILES`]).
    pub editor_file: String,
    /// VS Code's "reuse window" ([`EDITOR_REUSE`]).
    pub editor_reuse: String,
    /// The solution Visual Studio last opened from a repo with several, by
    /// repo path: its path from the checkout's root, picked first next time
    /// in any of the repo's worktrees.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub solution: BTreeMap<String, String>,
    /// The interface's font family; empty is Geist.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub font_ui: String,
    /// Paths, ports and counts in the chrome; empty is Geist Mono.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub font_mono: String,
    /// Terminals and the diff and file viewers; empty is JetBrains Mono.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub font_term: String,
    /// [`UI_SIZES`]; written only when it is not the default.
    #[serde(skip_serializing_if = "is_default_ui_size")]
    pub ui_size: String,
    /// [`TERM_SIZES`].
    #[serde(skip_serializing_if = "is_default_term_size")]
    pub term_size: String,
    /// [`EDITOR_SIZES`].
    #[serde(skip_serializing_if = "is_default_editor_size")]
    pub editor_size: String,
    /// [`TERM_LINES`].
    #[serde(skip_serializing_if = "is_default_term_line")]
    pub term_line: String,
    /// Imported themes by id, as the palettes they were derived into.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub themes: BTreeMap<String, Palette>,
    /// Keys a newer window wrote, kept through a save.
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

impl Default for DesktopPrefs {
    fn default() -> Self {
        Self {
            theme: THEMES[0].into(),
            diff_mode: DIFF_MODES[0].into(),
            home: HOMES[0].into(),
            approved_open: BTreeMap::new(),
            startup: STARTUPS[0].into(),
            collapsed: Vec::new(),
            sidebar_width: 0,
            sidebar_collapsed: false,
            editor_folder: EDITOR_FOLDERS[0].into(),
            editor_file: EDITOR_FILES[0].into(),
            editor_reuse: EDITOR_REUSE[0].into(),
            solution: BTreeMap::new(),
            font_ui: String::new(),
            font_mono: String::new(),
            font_term: String::new(),
            ui_size: size_default("ui_size").into(),
            term_size: size_default("term_size").into(),
            editor_size: size_default("editor_size").into(),
            term_line: size_default("term_line").into(),
            themes: BTreeMap::new(),
            other: BTreeMap::new(),
        }
    }
}

/// The default of one of the look's fixed-list keys.
pub fn size_default(key: &str) -> &'static str {
    SIZE_DEFAULTS
        .iter()
        .find(|(k, _)| *k == key)
        .map_or("", |(_, v)| v)
}

fn is_zero(v: &u16) -> bool {
    *v == 0
}
fn is_false(v: &bool) -> bool {
    !*v
}
fn is_default_ui_size(v: &str) -> bool {
    v == size_default("ui_size")
}
fn is_default_term_size(v: &str) -> bool {
    v == size_default("term_size")
}
fn is_default_editor_size(v: &str) -> bool {
    v == size_default("editor_size")
}
fn is_default_term_line(v: &str) -> bool {
    v == size_default("term_line")
}

/// The choices a preference offers, by key; None for a key there is none.
pub fn choices(key: &str) -> Option<&'static [&'static str]> {
    match key {
        "theme" => Some(THEMES),
        "diff_mode" => Some(DIFF_MODES),
        "home" => Some(HOMES),
        "startup" => Some(STARTUPS),
        "editor_folder" => Some(EDITOR_FOLDERS),
        "editor_file" => Some(EDITOR_FILES),
        "editor_reuse" => Some(EDITOR_REUSE),
        "ui_size" => Some(UI_SIZES),
        "term_size" => Some(TERM_SIZES),
        "editor_size" => Some(EDITOR_SIZES),
        "term_line" => Some(TERM_LINES),
        _ => None,
    }
}

impl DesktopPrefs {
    /// The preferences in the layers, `local` over `path`. A missing or
    /// unreadable object is the defaults, and a word off its list is that
    /// preference's default — never an error the window has to show.
    pub fn load_at(path: &Path, local: &Path) -> Self {
        Self::from_raw(crate::read_raw_key(path, local, KEY))
    }

    /// The preferences a stored `desktop` value holds, on [`Self::load_at`]'s
    /// terms.
    pub fn from_raw(raw: Option<serde_json::Value>) -> Self {
        let mut prefs: DesktopPrefs = raw
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        let fallback = DesktopPrefs::default();
        // Only whole palettes, under ids a slug could be, and no more of
        // them than are kept: a hand edit can't paint half a theme.
        prefs
            .themes
            .retain(|id, palette| look::slug(id) == *id && palette.valid());
        while prefs.themes.len() > MAX_IMPORTED {
            prefs.themes.pop_last();
        }
        for key in [
            "diff_mode",
            "home",
            "startup",
            "editor_folder",
            "editor_file",
            "editor_reuse",
            "ui_size",
            "term_size",
            "editor_size",
            "term_line",
        ] {
            let ok = choices(key).is_some_and(|list| list.contains(&prefs.get(key)));
            if !ok {
                prefs.set_unchecked(key, fallback.get(key));
            }
        }
        if prefs.sidebar_width != 0 {
            prefs.sidebar_width = prefs.sidebar_width.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
        }
        if !prefs.theme_ok(&prefs.theme) {
            prefs.theme = fallback.theme;
        }
        for key in FONT_KEYS {
            let held = prefs.get(key).trim().to_string();
            let ok = held.is_empty() || look::font_family_ok(&held);
            prefs.set_unchecked(key, if ok { &held } else { "" });
        }
        prefs
    }

    /// `theme` is one the window can paint: off the fixed list, or an
    /// imported theme still kept.
    pub fn theme_ok(&self, theme: &str) -> bool {
        THEMES.contains(&theme) || self.imported(theme).is_some()
    }

    /// The imported palette a `theme` value names, if it is one kept.
    pub fn imported(&self, theme: &str) -> Option<&Palette> {
        self.themes.get(theme.strip_prefix(IMPORT_PREFIX)?)
    }

    /// Keep `palette` as an imported theme and return the `theme` value
    /// that picks it. One of the same label is replaced (an import again
    /// updates it); a new one takes its label's slug, numbered past one
    /// already taken. Refused once [`MAX_IMPORTED`] are kept.
    pub fn import_theme(&mut self, palette: Palette) -> Result<String, String> {
        if !palette.valid() {
            return Err("that theme did not map onto a whole palette".into());
        }
        let same = self
            .themes
            .iter()
            .find(|(_, p)| p.label.eq_ignore_ascii_case(&palette.label))
            .map(|(id, _)| id.clone());
        let id = match same {
            Some(id) => id,
            None => {
                if self.themes.len() >= MAX_IMPORTED {
                    return Err(format!(
                        "{MAX_IMPORTED} imported themes are kept at most: remove one first"
                    ));
                }
                let base = look::slug(&palette.label);
                let mut id = base.clone();
                let mut n = 2;
                while self.themes.contains_key(&id) {
                    id = format!("{base}-{n}");
                    n += 1;
                }
                id
            }
        };
        self.themes.insert(id.clone(), palette);
        Ok(format!("{IMPORT_PREFIX}{id}"))
    }

    /// Forget the imported theme `id`; the window goes back to Workshop
    /// if it was wearing it.
    pub fn remove_theme(&mut self, id: &str) -> Result<(), String> {
        if self.themes.remove(id).is_none() {
            return Err(format!("no imported theme {id}"));
        }
        if self.theme == format!("{IMPORT_PREFIX}{id}") {
            self.theme = THEMES[0].into();
        }
        Ok(())
    }

    /// Kete's own look: Workshop following the OS, Geist, Geist Mono and
    /// JetBrains Mono at their sizes. The imported themes stay, to pick
    /// again.
    pub fn reset_look(&mut self) {
        self.theme = THEMES[0].into();
        for key in FONT_KEYS {
            self.set_unchecked(key, "");
        }
        for (key, value) in SIZE_DEFAULTS {
            self.set_unchecked(key, value);
        }
    }

    pub fn load() -> Self {
        let (path, local) = crate::layer_paths();
        Self::load_at(&path, &local)
    }

    pub fn get(&self, key: &str) -> &str {
        match key {
            "theme" => &self.theme,
            "diff_mode" => &self.diff_mode,
            "home" => &self.home,
            "startup" => &self.startup,
            "editor_folder" => &self.editor_folder,
            "editor_file" => &self.editor_file,
            "editor_reuse" => &self.editor_reuse,
            "font_ui" => &self.font_ui,
            "font_mono" => &self.font_mono,
            "font_term" => &self.font_term,
            "ui_size" => &self.ui_size,
            "term_size" => &self.term_size,
            "editor_size" => &self.editor_size,
            "term_line" => &self.term_line,
            _ => "",
        }
    }

    fn set_unchecked(&mut self, key: &str, value: &str) {
        match key {
            "theme" => self.theme = value.into(),
            "diff_mode" => self.diff_mode = value.into(),
            "home" => self.home = value.into(),
            "startup" => self.startup = value.into(),
            "editor_folder" => self.editor_folder = value.into(),
            "editor_file" => self.editor_file = value.into(),
            "editor_reuse" => self.editor_reuse = value.into(),
            "font_ui" => self.font_ui = value.into(),
            "font_mono" => self.font_mono = value.into(),
            "font_term" => self.font_term = value.into(),
            "ui_size" => self.ui_size = value.into(),
            "term_size" => self.term_size = value.into(),
            "editor_size" => self.editor_size = value.into(),
            "term_line" => self.term_line = value.into(),
            _ => {}
        }
    }

    /// `command` from the repo at `repo`'s .kete.json has been allowed.
    pub fn open_approved(&self, repo: &str, command: &str) -> bool {
        self.approved_open
            .get(repo)
            .is_some_and(|list| list.iter().any(|c| c == command))
    }

    /// Allow `command` for the repo at `repo`.
    pub fn approve_open(&mut self, repo: &str, command: &str) {
        let list = self.approved_open.entry(repo.to_string()).or_default();
        if !list.iter().any(|c| c == command) {
            list.push(command.to_string());
        }
    }

    /// The solution remembered for the repo at `repo`.
    pub fn solution_for(&self, repo: &str) -> Option<&str> {
        self.solution.get(repo).map(String::as_str)
    }

    /// Set one preference: a word off its list, a theme the window can
    /// paint, or a font family (empty for Kete's own).
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        if key == "theme" {
            if !self.theme_ok(value) {
                return Err(format!(
                    "theme is one of {} or an imported theme, not {value:?}",
                    THEMES.join(", ")
                ));
            }
            self.theme = value.into();
            return Ok(());
        }
        if FONT_KEYS.contains(&key) {
            let value = value.trim();
            if !value.is_empty() && !look::font_family_ok(value) {
                return Err(format!("{value:?} is not a font family name"));
            }
            self.set_unchecked(key, value);
            return Ok(());
        }
        let list = choices(key).ok_or_else(|| format!("no desktop preference {key}"))?;
        if !list.contains(&value) {
            return Err(format!(
                "{key} is one of {}, not {value:?}",
                list.join(", ")
            ));
        }
        self.set_unchecked(key, value);
        Ok(())
    }

    /// Read the stored object fresh, apply `change`, and write the object
    /// back, leaving every other key in both layers alone — one read and
    /// one write under the settings lock. A refused change, or a settings
    /// file that can't be parsed, writes nothing.
    pub fn update_in(
        path: &Path,
        local: &Path,
        change: impl FnOnce(&mut DesktopPrefs) -> Result<(), String>,
    ) -> Result<DesktopPrefs, String> {
        let value = crate::update_raw_key(path, local, KEY, |stored| {
            let mut prefs = Self::from_raw(stored.cloned());
            change(&mut prefs).map_err(std::io::Error::other)?;
            serde_json::to_value(&prefs).map_err(std::io::Error::other)
        })
        .map_err(|e| e.to_string())?;
        Ok(Self::from_raw(Some(value)))
    }

    pub fn update(
        change: impl FnOnce(&mut DesktopPrefs) -> Result<(), String>,
    ) -> Result<DesktopPrefs, String> {
        let (path, local) = crate::layer_paths();
        Self::update_in(&path, &local, change)
    }
}

/// Whether a checkout's Open command may run now, for the repo at `repo`,
/// in the layers at `path` / `local`: one from the user's own setting
/// (`from_file` false) always may; one from a `.kete.json` (anyone who can
/// push to the repo wrote it) only once allowed for that repo. `approved`
/// is the command the app showed and the user said yes to: it allows the
/// command only when it is the one resolved now (the file can change
/// between the ask and the yes), and the approval is kept, so the same
/// command doesn't ask again in either app (a changed one does).
pub fn open_allowed_in(
    path: &Path,
    local: &Path,
    repo: &str,
    command: &str,
    from_file: bool,
    approved: Option<&str>,
) -> Result<bool, String> {
    if !from_file {
        return Ok(true);
    }
    if DesktopPrefs::load_at(path, local).open_approved(repo, command) {
        return Ok(true);
    }
    if approved != Some(command) {
        return Ok(false);
    }
    DesktopPrefs::update_in(path, local, |p| {
        p.approve_open(repo, command);
        Ok(())
    })?;
    Ok(true)
}

/// [`open_allowed_in`] in the layers on disk.
pub fn open_allowed(
    repo: &str,
    command: &str,
    from_file: bool,
    approved: Option<&str>,
) -> Result<bool, String> {
    let (path, local) = crate::layer_paths();
    open_allowed_in(&path, &local, repo, command, from_file, approved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_bad_words_read_as_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        assert_eq!(
            DesktopPrefs::load_at(&path, &local),
            DesktopPrefs::default()
        );
        std::fs::write(
            &path,
            r#"{"desktop": {"theme": "purple", "home": "board"}}"#,
        )
        .unwrap();
        let prefs = DesktopPrefs::load_at(&path, &local);
        assert_eq!(prefs.theme, "system");
        assert_eq!(prefs.home, "board");
    }

    #[test]
    fn a_save_touches_only_the_desktop_object_and_keeps_its_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        std::fs::write(
            &path,
            r#"{"theme": "kete-light", "desktop": {"future": 3}, "x": [1]}"#,
        )
        .unwrap();
        DesktopPrefs::update_in(&path, &local, |p| p.set("theme", "dark")).unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved["theme"], "kete-light",
            "the TUI theme is not the window's"
        );
        assert_eq!(saved["x"], serde_json::json!([1]));
        assert_eq!(saved["desktop"]["theme"], "dark");
        assert_eq!(saved["desktop"]["future"], 3);
        assert!(DesktopPrefs::update_in(&path, &local, |p| p.set("theme", "neon")).is_err());
    }

    #[test]
    fn folded_groups_round_trip_and_an_empty_list_is_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        assert!(DesktopPrefs::load_at(&path, &local).collapsed.is_empty());
        DesktopPrefs::update_in(&path, &local, |p| {
            p.collapsed = vec!["repo:p1".into(), "wt:w2".into()];
            Ok(())
        })
        .unwrap();
        assert_eq!(
            DesktopPrefs::load_at(&path, &local).collapsed,
            vec!["repo:p1".to_string(), "wt:w2".to_string()]
        );
        DesktopPrefs::update_in(&path, &local, |p| {
            p.collapsed.clear();
            Ok(())
        })
        .unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(saved["desktop"].get("collapsed").is_none());
    }

    #[test]
    fn sidebar_width_and_fold_round_trip_clamped_and_default_is_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        let fresh = DesktopPrefs::load_at(&path, &local);
        assert_eq!((fresh.sidebar_width, fresh.sidebar_collapsed), (0, false));
        DesktopPrefs::update_in(&path, &local, |p| {
            p.sidebar_width = 300;
            p.sidebar_collapsed = true;
            Ok(())
        })
        .unwrap();
        let back = DesktopPrefs::load_at(&path, &local);
        assert_eq!((back.sidebar_width, back.sidebar_collapsed), (300, true));
        std::fs::write(&path, r#"{"desktop": {"sidebar_width": 9000}}"#).unwrap();
        assert_eq!(
            DesktopPrefs::load_at(&path, &local).sidebar_width,
            SIDEBAR_MAX
        );
        std::fs::write(&path, r#"{"desktop": {"sidebar_width": 40}}"#).unwrap();
        assert_eq!(
            DesktopPrefs::load_at(&path, &local).sidebar_width,
            SIDEBAR_MIN
        );
        DesktopPrefs::update_in(&path, &local, |p| {
            p.sidebar_width = 0;
            p.sidebar_collapsed = false;
            Ok(())
        })
        .unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(saved["desktop"].get("sidebar_width").is_none());
        assert!(saved["desktop"].get("sidebar_collapsed").is_none());
    }

    #[test]
    fn startup_opens_home_or_the_orchestrator() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        assert_eq!(DesktopPrefs::load_at(&path, &local).startup, "home");
        DesktopPrefs::update_in(&path, &local, |p| p.set("startup", "orchestrator")).unwrap();
        assert_eq!(DesktopPrefs::load_at(&path, &local).startup, "orchestrator");
        std::fs::write(&path, r#"{"desktop": {"startup": "sideways"}}"#).unwrap();
        assert_eq!(DesktopPrefs::load_at(&path, &local).startup, "home");
        assert!(DesktopPrefs::update_in(&path, &local, |p| p.set("startup", "x")).is_err());
    }

    #[test]
    fn editors_default_to_auto_and_take_their_choices() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        let prefs = DesktopPrefs::load_at(&path, &local);
        assert_eq!(
            (prefs.editor_folder.as_str(), prefs.editor_file.as_str()),
            ("auto", "auto")
        );
        DesktopPrefs::update_in(&path, &local, |p| {
            p.set("editor_folder", "visual-studio")?;
            p.set("editor_file", "kete")
        })
        .unwrap();
        let prefs = DesktopPrefs::load_at(&path, &local);
        assert_eq!(prefs.get("editor_folder"), "visual-studio");
        assert_eq!(prefs.get("editor_file"), "kete");
        // Each list is its own: a file is never the Open command, a folder
        // never Kete's viewer.
        assert!(
            DesktopPrefs::update_in(&path, &local, |p| p.set("editor_file", "command")).is_err()
        );
        assert!(
            DesktopPrefs::update_in(&path, &local, |p| p.set("editor_folder", "kete")).is_err()
        );
        std::fs::write(
            &path,
            r#"{"desktop": {"editor_folder": "emacs", "editor_file": "vscode"}}"#,
        )
        .unwrap();
        let prefs = DesktopPrefs::load_at(&path, &local);
        assert_eq!(prefs.editor_folder, "auto");
        assert_eq!(prefs.editor_file, "vscode");
    }

    #[test]
    fn reuse_window_is_off_until_turned_on_and_a_solution_is_remembered_per_repo() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        assert_eq!(DesktopPrefs::load_at(&path, &local).editor_reuse, "off");
        DesktopPrefs::update_in(&path, &local, |p| {
            p.solution
                .insert("C:/Code/api".into(), "src/Api.slnx".into());
            p.set("editor_reuse", "on")
        })
        .unwrap();
        let prefs = DesktopPrefs::load_at(&path, &local);
        assert_eq!(prefs.editor_reuse, "on");
        assert_eq!(prefs.solution_for("C:/Code/api"), Some("src/Api.slnx"));
        assert_eq!(prefs.solution_for("C:/Code/other"), None);
        assert!(
            DesktopPrefs::update_in(&path, &local, |p| p.set("editor_reuse", "always")).is_err()
        );
        std::fs::write(&path, r#"{"desktop": {"editor_reuse": "sometimes"}}"#).unwrap();
        assert_eq!(DesktopPrefs::load_at(&path, &local).editor_reuse, "off");
    }

    #[test]
    fn a_repo_file_open_command_runs_once_allowed_and_asks_again_when_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        let allowed = |cmd: &str, approved: Option<&str>| {
            open_allowed_in(&path, &local, "C:/Code/api", cmd, true, approved).unwrap()
        };
        // The user's own setting never asks.
        assert!(open_allowed_in(&path, &local, "C:/Code/api", "code .", false, None).unwrap());
        assert!(!allowed("code .", None));
        // A yes to another command (the file changed under the dialog)
        // allows nothing.
        assert!(!allowed("code .", Some("rm -rf /")));
        assert!(allowed("code .", Some("code .")));
        assert!(allowed("code .", None), "kept");
        assert!(!allowed("code . && curl x", None), "a changed command asks");
        assert!(!open_allowed_in(&path, &local, "C:/Code/web", "code .", true, None).unwrap());
    }

    fn palette(label: &str) -> Palette {
        let data = crate::vscode::ThemeData {
            colors: [
                ("editor.background", "#282C34"),
                ("editor.foreground", "#ABB2BF"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
            ..Default::default()
        };
        look::from_vscode(&data, false, label)
    }

    fn saved(path: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// The look's sizes and fonts are written only off their defaults, so
    /// a window that never touched them leaves config.json as it was.
    #[test]
    fn the_look_writes_nothing_until_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        DesktopPrefs::update_in(&path, &local, |p| p.set("home", "board")).unwrap();
        let desktop = saved(&path)["desktop"].clone();
        for key in FONT_KEYS
            .iter()
            .chain(["ui_size", "term_size", "editor_size", "term_line", "themes"].iter())
        {
            assert!(desktop.get(key).is_none(), "{key} written at its default");
        }
        let prefs = DesktopPrefs::load_at(&path, &local);
        assert_eq!((prefs.get("ui_size"), prefs.get("term_size")), ("14", "13"));
        assert_eq!(
            (prefs.get("editor_size"), prefs.get("term_line")),
            ("12.5", "1.12")
        );
        DesktopPrefs::update_in(&path, &local, |p| {
            p.set("term_size", "15")?;
            p.set("font_term", " JetBrains Mono ")
        })
        .unwrap();
        let desktop = saved(&path)["desktop"].clone();
        assert_eq!(desktop["term_size"], "15");
        assert_eq!(desktop["font_term"], "JetBrains Mono", "trimmed");
        assert!(DesktopPrefs::update_in(&path, &local, |p| p.set("term_size", "40")).is_err());
    }

    /// A font is a family name or nothing: anything that could end the CSS
    /// value it goes into is refused, and a hand edit holding one reads as
    /// Kete's own font.
    #[test]
    fn a_font_is_a_family_name_or_kete_s_own() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        DesktopPrefs::update_in(&path, &local, |p| p.set("font_ui", "Segoe UI")).unwrap();
        assert_eq!(DesktopPrefs::load_at(&path, &local).font_ui, "Segoe UI");
        let bad = DesktopPrefs::update_in(&path, &local, |p| p.set("font_ui", "x\"; color: red"));
        assert!(bad.is_err());
        DesktopPrefs::update_in(&path, &local, |p| p.set("font_ui", "")).unwrap();
        assert_eq!(DesktopPrefs::load_at(&path, &local).font_ui, "");
        std::fs::write(
            &path,
            r#"{"desktop": {"font_mono": "a}b", "font_term": "Fira Code", "ui_size": "99"}}"#,
        )
        .unwrap();
        let prefs = DesktopPrefs::load_at(&path, &local);
        assert_eq!(prefs.font_mono, "");
        assert_eq!(prefs.font_term, "Fira Code");
        assert_eq!(prefs.ui_size, "14");
    }

    /// An imported theme is kept as its palette under its label's slug,
    /// and picked by `import:<id>`; importing it again replaces it, a
    /// second of the same slug is numbered, and only so many are kept.
    #[test]
    fn imported_themes_are_kept_as_palettes_and_picked_by_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        let mut picked = String::new();
        DesktopPrefs::update_in(&path, &local, |p| {
            picked = p.import_theme(palette("Atom One Dark"))?;
            p.set("theme", &picked)
        })
        .unwrap();
        assert_eq!(picked, "import:atom-one-dark");
        let prefs = DesktopPrefs::load_at(&path, &local);
        assert_eq!(prefs.theme, "import:atom-one-dark");
        assert_eq!(prefs.imported(&prefs.theme).unwrap().label, "Atom One Dark");
        assert!(
            saved(&path)["desktop"]["themes"]["atom-one-dark"]["tokens"]["color-bg"]
                .as_str()
                .is_some(),
            "stored as the derived tokens"
        );

        let mut prefs = DesktopPrefs::load_at(&path, &local);
        assert_eq!(
            prefs.import_theme(palette("atom one dark")).unwrap(),
            "import:atom-one-dark",
            "the same label replaces"
        );
        assert_eq!(
            prefs.import_theme(palette("Atom: One Dark!")).unwrap(),
            "import:atom-one-dark-2"
        );
        for i in prefs.themes.len()..MAX_IMPORTED {
            prefs.import_theme(palette(&format!("Theme {i}"))).unwrap();
        }
        assert!(prefs.import_theme(palette("One too many")).is_err());
        assert!(prefs.set("theme", "import:nope").is_err());

        prefs.theme = "import:atom-one-dark".into();
        prefs.remove_theme("atom-one-dark").unwrap();
        assert_eq!(prefs.theme, "system", "back to Workshop");
        assert!(prefs.remove_theme("atom-one-dark").is_err());
    }

    /// A hand-edited palette that is not whole is dropped, and a theme
    /// naming it reads as Workshop.
    #[test]
    fn a_broken_imported_theme_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        let mut whole = serde_json::to_value(palette("Fine")).unwrap();
        let mut broken = whole.clone();
        broken["tokens"]["color-bg"] = "url(x)".into();
        whole["label"] = "Fine".into();
        std::fs::write(
            &path,
            serde_json::json!({"desktop": {
                "theme": "import:broken",
                "themes": {"broken": broken, "fine": whole, "Not A Slug": whole}
            }})
            .to_string(),
        )
        .unwrap();
        let prefs = DesktopPrefs::load_at(&path, &local);
        assert_eq!(prefs.theme, "system");
        assert_eq!(prefs.themes.keys().collect::<Vec<_>>(), ["fine"]);
    }

    /// Reset appearance puts Workshop and Kete's fonts back and keeps the
    /// imported themes to pick again.
    #[test]
    fn reset_look_keeps_the_imported_themes() {
        let mut prefs = DesktopPrefs::default();
        let picked = prefs.import_theme(palette("Atom One Dark")).unwrap();
        prefs.set("theme", &picked).unwrap();
        prefs.set("font_ui", "Inter").unwrap();
        prefs.set("ui_size", "16").unwrap();
        prefs.set("term_line", "1.3").unwrap();
        prefs.set("home", "board").unwrap();
        prefs.reset_look();
        assert_eq!(prefs.theme, "system");
        assert_eq!(prefs.font_ui, "");
        assert_eq!(
            (prefs.ui_size.as_str(), prefs.term_line.as_str()),
            ("14", "1.12")
        );
        assert_eq!(prefs.home, "board", "not the look's");
        assert_eq!(prefs.themes.len(), 1);
    }
}
