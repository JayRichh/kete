//! Rebindable application hotkeys.
//!
//! Every key the grid reacts to is an [`Action`] here, with a default
//! chord list. The Hotkeys tab of the settings overlay writes overrides
//! into the shared config JSON (`keybindings: { "<action id>": "j, down" }`),
//! and the event loop dispatches through [`Keymap::lookup`] instead of
//! matching raw `KeyCode`s.
//!
//! Two things this module knows that a naive keymap wouldn't:
//!
//! * **Normalization.** Terminals disagree about how a key arrives —
//!   `Shift+j` may come through as `Char('J')` with or without the shift
//!   bit, `Ctrl+]` is spelled `Ctrl+5` in the legacy encoding, `BackTab`
//!   is really `Shift+Tab`. [`KeyChord::from_event`] folds all of that into
//!   one canonical form so a binding matches whatever the emulator sends.
//!
//! * **Host reachability.** nebula runs *inside* Terminal.app / Ghostty /
//!   tmux, which eat chords before we ever see them — every `⌘` combo, most
//!   `^⇧` ones, `^←`. [`host_warning`] flags those at bind time so the user
//!   finds out at the moment of choosing, not the next time the key does
//!   nothing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::BTreeMap;
use std::fmt;

pub use nebula_config::actions::{index_of, spec_at, spec_of, Action, ActionSpec, Scope, ACTIONS};

// ---- chords ----

/// A single key press: one key plus the modifiers held with it, in the one
/// canonical spelling [`KeyChord::from_event`] produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

/// The modifiers a binding can name. META/HYPER are dropped: no terminal
/// reports them for a key a user could reasonably choose.
const KEPT_MODS: KeyModifiers = KeyModifiers::CONTROL
    .union(KeyModifiers::ALT)
    .union(KeyModifiers::SHIFT)
    .union(KeyModifiers::SUPER);

impl KeyChord {
    /// Canonicalize a crossterm event into a chord, so the same physical
    /// press compares equal however the emulator spelled it:
    ///
    /// * `Char('J')` and `shift + Char('j')` both become `shift+j`;
    /// * shift is dropped from punctuation and digits, where the glyph
    ///   already carries it (`?` is `?`, never `shift+?`);
    /// * `BackTab` becomes `shift+tab`;
    /// * `ctrl+5` — the legacy encoding's name for byte 0x1D — becomes
    ///   `ctrl+]`, which is what the user actually pressed.
    pub fn from_event(key: &KeyEvent) -> Self {
        let mut mods = key.modifiers & KEPT_MODS;
        let mut code = key.code;
        match code {
            KeyCode::Char(c) => {
                if c.is_alphabetic() {
                    if c.is_uppercase() {
                        mods |= KeyModifiers::SHIFT;
                    }
                    code = KeyCode::Char(c.to_lowercase().next().unwrap_or(c));
                } else {
                    mods.remove(KeyModifiers::SHIFT);
                }
            }
            KeyCode::BackTab => {
                code = KeyCode::Tab;
                mods |= KeyModifiers::SHIFT;
            }
            _ => {}
        }
        if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('5') {
            code = KeyCode::Char(']');
        }
        Self { code, mods }
    }

    /// Parse a config-file spelling (`"ctrl+shift+f"`, `"shift+tab"`, `"/"`).
    /// Unknown key names return None, which the loader reports and skips.
    pub fn parse(spec: &str) -> Option<Self> {
        let spec = spec.trim();
        if spec.is_empty() {
            return None;
        }
        // '+' is both the separator and a bindable key, so a chord ending
        // in one — "+" alone, or "ctrl++" — names the plus key itself.
        let (mod_part, name) = match spec.strip_suffix('+') {
            Some(rest) => (rest.trim_end_matches('+'), "+".to_string()),
            None => match spec.rfind('+') {
                Some(i) => (&spec[..i], spec[i + 1..].trim().to_lowercase()),
                None => ("", spec.to_lowercase()),
            },
        };
        let mut mods = KeyModifiers::NONE;
        for part in mod_part.split('+').filter(|p| !p.trim().is_empty()) {
            mods |= match part.trim().to_lowercase().as_str() {
                "ctrl" | "control" | "^" => KeyModifiers::CONTROL,
                "alt" | "opt" | "option" | "meta" => KeyModifiers::ALT,
                "shift" => KeyModifiers::SHIFT,
                "cmd" | "super" | "win" => KeyModifiers::SUPER,
                _ => return None,
            };
        }
        let code = parse_key_name(&name)?;
        Some(Self::from_event(&KeyEvent::new(code, mods)))
    }

    /// Config-file spelling. Round-trips through [`KeyChord::parse`].
    pub fn spec(&self) -> String {
        let mut out = String::new();
        if self.mods.contains(KeyModifiers::CONTROL) {
            out.push_str("ctrl+");
        }
        if self.mods.contains(KeyModifiers::ALT) {
            out.push_str("alt+");
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            out.push_str("shift+");
        }
        if self.mods.contains(KeyModifiers::SUPER) {
            out.push_str("cmd+");
        }
        out.push_str(&key_name(self.code));
        out
    }

    /// Compact on-screen spelling: `^q`, `⇧Tab`, `⌥p`, `⌘k`, `↓`.
    pub fn display(&self) -> String {
        let mut out = String::new();
        if self.mods.contains(KeyModifiers::CONTROL) {
            out.push('^');
        }
        if self.mods.contains(KeyModifiers::ALT) {
            out.push('⌥');
        }
        if self.mods.contains(KeyModifiers::SUPER) {
            out.push('⌘');
        }
        let shift = self.mods.contains(KeyModifiers::SHIFT);
        match self.code {
            // A shifted letter reads best as the capital itself.
            KeyCode::Char(c) if shift && c.is_alphabetic() => {
                out.push('⇧');
                out.extend(c.to_uppercase());
            }
            _ => {
                if shift {
                    out.push('⇧');
                }
                out.push_str(&key_display(self.code));
            }
        }
        out
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display())
    }
}

/// The named keys, each with its config spelling, its on-screen glyph, and
/// the alternate config spellings `parse` also accepts — one table so the
/// three lookups below can never disagree about a key. Letters, function
/// keys and BackTab are handled by the arms around the lookups instead.
const KEY_NAMES: &[KeyRow] = &[
    (KeyCode::Up, "up", "↑", &[]),
    (KeyCode::Down, "down", "↓", &[]),
    (KeyCode::Left, "left", "←", &[]),
    (KeyCode::Right, "right", "→", &[]),
    (KeyCode::Enter, "enter", "Enter", &["return", "cr"]),
    (KeyCode::Tab, "tab", "Tab", &[]),
    (KeyCode::Esc, "esc", "Esc", &["escape"]),
    (KeyCode::Char(' '), "space", "Space", &[]),
    (KeyCode::Backspace, "backspace", "⌫", &["bs"]),
    (KeyCode::Delete, "delete", "Del", &["del"]),
    (KeyCode::Insert, "insert", "Ins", &["ins"]),
    (KeyCode::Home, "home", "Home", &[]),
    (KeyCode::End, "end", "End", &[]),
    (KeyCode::PageUp, "pgup", "PgUp", &["pageup"]),
    (KeyCode::PageDown, "pgdn", "PgDn", &["pagedown"]),
];

/// One `KEY_NAMES` row: `(code, config spelling, glyph, alternate spellings)`.
type KeyRow = (KeyCode, &'static str, &'static str, &'static [&'static str]);

/// The `KEY_NAMES` row for `code`, with BackTab folded onto Tab — the
/// shift bit carries the difference, so both spell and display as `tab`.
fn key_row(code: KeyCode) -> Option<&'static KeyRow> {
    let code = if code == KeyCode::BackTab {
        KeyCode::Tab
    } else {
        code
    };
    KEY_NAMES.iter().find(|(c, ..)| *c == code)
}

fn parse_key_name(name: &str) -> Option<KeyCode> {
    if name == "backtab" {
        return Some(KeyCode::BackTab);
    }
    if let Some((code, ..)) = KEY_NAMES
        .iter()
        .find(|(_, canonical, _, aliases)| *canonical == name || aliases.contains(&name))
    {
        return Some(*code);
    }
    if let Some(n) = name.strip_prefix('f') {
        if let Ok(n) = n.parse::<u8>() {
            if (1..=20).contains(&n) {
                return Some(KeyCode::F(n));
            }
        }
    }
    let mut chars = name.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(KeyCode::Char(c))
}

fn key_name(code: KeyCode) -> String {
    if let Some((_, name, ..)) = key_row(code) {
        return (*name).into();
    }
    match code {
        KeyCode::Char(c) => c.to_lowercase().to_string(),
        KeyCode::F(n) => format!("f{n}"),
        other => format!("{other:?}").to_lowercase(),
    }
}

fn key_display(code: KeyCode) -> String {
    if let Some((_, _, glyph, _)) = key_row(code) {
        return (*glyph).into();
    }
    match code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::F(n) => format!("F{n}"),
        other => format!("{other:?}"),
    }
}

// ---- host-terminal reachability ----

/// How likely a chord is to actually reach nebula from inside the user's
/// terminal emulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Every emulator delivers it.
    Fine,
    /// Arrives in some setups only — worth saying so, still bindable.
    Risky,
    /// Effectively never arrives: the emulator or the OS takes it first.
    Blocked,
}

impl Reach {
    pub fn is_fine(self) -> bool {
        self == Reach::Fine
    }
}

/// Ctrl+letter chords whose legacy control byte is another key's, with the
/// warning each earns — a table so adding one can't drift the wording.
/// The message returns as `&'static str`, hence full strings per entry.
const CTRL_COLLISIONS: &[(char, &str)] = &[
    (
        'm',
        "^m is the same byte as Enter in terminals without the kitty protocol",
    ),
    (
        'i',
        "^i is the same byte as Tab in terminals without the kitty protocol",
    ),
    (
        '[',
        "^[ is the same byte as Esc in terminals without the kitty protocol",
    ),
    (
        'h',
        "^h is the same byte as ⌫ in terminals without the kitty protocol",
    ),
];

/// Whether the host terminal is likely to swallow `chord` before nebula
/// sees it, and why. nebula is always a guest inside Terminal.app, Ghostty,
/// iTerm, tmux or an ssh session, and each of those claims keys for itself
/// — so the honest answer here is a probability, not a fact. Nothing is
/// rejected on the strength of it; the settings overlay just says so.
pub fn host_warning(chord: &KeyChord) -> (Reach, Option<&'static str>) {
    let m = chord.mods;
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let shift = m.contains(KeyModifiers::SHIFT);
    let alt = m.contains(KeyModifiers::ALT);

    if m.contains(KeyModifiers::SUPER) {
        return (
            Reach::Blocked,
            Some("⌘ chords never reach a TUI: Terminal.app swallows them, Ghostty binds its own"),
        );
    }
    // Mission Control owns these on stock macOS (they switch Spaces).
    if ctrl && matches!(chord.code, KeyCode::Left | KeyCode::Right) && !shift && !alt {
        return (
            Reach::Risky,
            Some("macOS Mission Control takes ^← / ^→ unless you turn its Spaces shortcuts off"),
        );
    }
    // Legacy encoding has no shifted Enter: without the kitty protocol the
    // terminal sends the same \r a bare Enter does.
    if shift && !ctrl && !alt && chord.code == KeyCode::Enter {
        return (
            Reach::Risky,
            Some("⇧Enter needs the kitty keyboard protocol: Ghostty/kitty send it, Terminal.app sends a plain Enter and tmux flattens it to one; ⇧O and ⌥Enter arrive everywhere"),
        );
    }
    if ctrl && shift {
        return (
            Reach::Risky,
            Some("^⇧ needs the kitty keyboard protocol: Ghostty/kitty send it, Terminal.app won't"),
        );
    }
    if ctrl {
        // Legacy control bytes that collide with a key of their own.
        if let Some((_, why)) = CTRL_COLLISIONS
            .iter()
            .find(|(c, _)| chord.code == KeyCode::Char(*c))
        {
            return (Reach::Risky, Some(why));
        }
        match chord.code {
            KeyCode::Enter | KeyCode::Tab | KeyCode::Backspace | KeyCode::Esc => {
                return (
                    Reach::Risky,
                    Some("ctrl + this key needs the kitty keyboard protocol to be distinguishable"),
                )
            }
            // Only a handful of punctuation has a control byte at all.
            KeyCode::Char(c)
                if !c.is_ascii_alphabetic()
                    && !matches!(c, ' ' | '@' | '[' | '\\' | ']' | '^' | '_' | '/') =>
            {
                return (
                    Reach::Risky,
                    Some("most terminals have no encoding for ctrl + this key"),
                )
            }
            _ => {}
        }
    }
    if alt {
        return (
            Reach::Risky,
            Some("⌥ only arrives if your terminal sends Option as Meta (Terminal.app: off by default)"),
        );
    }
    if let KeyCode::F(n) = chord.code {
        if n >= 13 {
            return (Reach::Risky, Some("few terminals emit F13 and above"));
        }
    }
    (Reach::Fine, None)
}

// ---- the map ----

/// What an action with no chord shows in place of one, everywhere a chord
/// list is rendered — the hotkeys tab and the help labels must agree.
pub const UNBOUND: &str = "-";

/// Resolved bindings: one chord list per entry of [`ACTIONS`], in the same
/// order, so an index is a stable handle for both the UI and the config.
#[derive(Debug, Clone)]
pub struct Keymap {
    binds: Vec<Vec<KeyChord>>,
    /// Rows the user set (in the config or the Hotkeys tab), as against
    /// rows on their defaults, which may have given a chord up to one of
    /// theirs: only these are written back.
    mine: Vec<bool>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            binds: ACTIONS.iter().map(|s| parse_list(s.defaults)).collect(),
            mine: vec![false; ACTIONS.len()],
        }
    }
}

impl Keymap {
    /// Defaults with the config's `keybindings` overrides applied. An
    /// override naming an unknown action or an unparseable chord is logged
    /// and skipped, so one bad line can't strand the user without a keymap.
    /// An empty string is a deliberate unbind.
    pub fn from_overrides(overrides: &BTreeMap<String, String>) -> Self {
        let mut map = Self::default();
        let mut mine = vec![false; ACTIONS.len()];
        for (id, spec) in overrides {
            // The desktop window's keys, beside these in the same map.
            if id.starts_with(nebula_config::actions::DESKTOP_PREFIX) {
                continue;
            }
            let Some(index) = ACTIONS.iter().position(|s| s.id == id) else {
                tracing::warn!("ignoring keybinding for unknown action {id:?}");
                continue;
            };
            mine[index] = true;
            if spec.trim().is_empty() {
                map.binds[index].clear();
                continue;
            }
            let mut chords = Vec::new();
            // `ctrl+,` and a lone `,` are chords, not separators.
            for part in nebula_config::actions::split_chords(spec) {
                match KeyChord::parse(&part) {
                    Some(chord) if !chords.contains(&chord) => chords.push(chord),
                    Some(_) => {}
                    None => tracing::warn!("ignoring unparseable keybinding {part:?} for {id}"),
                }
            }
            map.binds[index] = chords;
        }
        // A chord the user gave a row is theirs: a row still on its
        // defaults gives it up, so a default added in a later release
        // (`d` on the diff, `z` on full screen) never shadows a binding
        // saved before it.
        for (index, spec) in ACTIONS.iter().enumerate() {
            if mine[index] {
                continue;
            }
            let taken: Vec<KeyChord> = ACTIONS
                .iter()
                .enumerate()
                .filter(|(i, s)| mine[*i] && s.scope == spec.scope)
                .flat_map(|(i, _)| map.binds[i].clone())
                .collect();
            map.binds[index].retain(|c| !taken.contains(c));
        }
        map.mine = mine;
        map
    }

    /// Only what the user set and differs from the defaults, for writing
    /// back to the config. A default row that gave a chord up to one of
    /// theirs stays out: it gives it up again on the next load, and gets
    /// it back once theirs lets it go.
    pub fn overrides(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for (index, spec) in ACTIONS.iter().enumerate() {
            if self.mine[index] && self.binds[index] != parse_list(spec.defaults) {
                out.insert(spec.id.to_string(), self.spec_list(index));
            }
        }
        out
    }

    /// The action `chord` triggers in `scope`, if any.
    pub fn lookup(&self, scope: Scope, chord: &KeyChord) -> Option<Action> {
        ACTIONS
            .iter()
            .enumerate()
            .find(|(i, s)| s.scope == scope && self.binds[*i].contains(chord))
            .map(|(_, s)| s.action)
    }

    pub fn chords(&self, action: Action) -> &[KeyChord] {
        index_of(action).map_or(&[], |i| self.binds[i].as_slice())
    }

    pub fn chords_at(&self, index: usize) -> &[KeyChord] {
        self.binds.get(index).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// On-screen chord list for a row: `j ↓`, or `—` when unbound.
    pub fn display_at(&self, index: usize) -> String {
        let chords = self.chords_at(index);
        if chords.is_empty() {
            return UNBOUND.into();
        }
        chords
            .iter()
            .map(|c| c.display())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The first chord for an action, for help text and footer hints.
    pub fn first(&self, action: Action) -> Option<KeyChord> {
        self.chords(action).first().copied()
    }

    /// Help-style label for an action: every chord it answers to, or `—`.
    pub fn label(&self, action: Action) -> String {
        index_of(action).map_or_else(|| UNBOUND.into(), |i| self.display_at(i))
    }

    /// The chords Help and the footer print for an action: every one it
    /// answers to but the ⌘ ones. ⌘ never reaches nebula in Terminal.app
    /// or `nebula browser`, and every ⌘ default has a plain key beside it
    /// (`1`–`9`, `+`), so the ⌘ chords stay bound as silent aliases
    /// that only Settings → Hotkeys lists. An action bound to ⌘ chords
    /// alone — a binding of the user's own — still shows them.
    pub fn shown_chords(&self, action: Action) -> Vec<KeyChord> {
        let all = self.chords(action);
        let plain: Vec<KeyChord> = all
            .iter()
            .filter(|c| !c.mods.contains(KeyModifiers::SUPER))
            .copied()
            .collect();
        if plain.is_empty() {
            all.to_vec()
        } else {
            plain
        }
    }

    /// [`Self::label`] without the ⌘ aliases ([`Self::shown_chords`]).
    pub fn shown_label(&self, action: Action) -> String {
        let chords = self.shown_chords(action);
        if chords.is_empty() {
            return UNBOUND.into();
        }
        chords
            .iter()
            .map(|c| c.display())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The first of [`Self::shown_chords`], for a footer hint.
    pub fn shown_first(&self, action: Action) -> Option<KeyChord> {
        self.shown_chords(action).first().copied()
    }

    fn spec_list(&self, index: usize) -> String {
        self.binds[index]
            .iter()
            .map(|c| c.spec())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Other rows that already answer to `chord` in the same scope as
    /// `index`. Cross-scope reuse is fine — the panels and a locked
    /// terminal never read the same keystroke.
    pub fn conflicts(&self, index: usize, chord: &KeyChord) -> Vec<usize> {
        let Some(spec) = ACTIONS.get(index) else {
            return Vec::new();
        };
        ACTIONS
            .iter()
            .enumerate()
            .filter(|(i, s)| *i != index && s.scope == spec.scope && self.binds[*i].contains(chord))
            .map(|(i, _)| i)
            .collect()
    }

    /// Bind `chord` to `index`, replacing its chords (or appending when
    /// `add`). Any conflicting row loses the chord — one keystroke can
    /// only mean one thing, and silently shadowing the loser would be
    /// worse than taking it away visibly.
    pub fn bind(&mut self, index: usize, chord: KeyChord, add: bool) {
        for other in self.conflicts(index, &chord) {
            self.binds[other].retain(|c| *c != chord);
        }
        self.mine[index] = true;
        let slot = &mut self.binds[index];
        if add {
            if !slot.contains(&chord) {
                slot.push(chord);
            }
        } else {
            slot.clear();
            slot.push(chord);
        }
    }

    pub fn reset(&mut self, index: usize) {
        if let Some(spec) = ACTIONS.get(index) {
            self.binds[index] = parse_list(spec.defaults);
            self.mine[index] = false;
        }
    }

    pub fn clear(&mut self, index: usize) {
        if let Some(slot) = self.binds.get_mut(index) {
            slot.clear();
            self.mine[index] = true;
        }
    }

    /// A row sharing a chord with another action in the same scope. The
    /// overlay warns before it lets you make one, so this only comes from a
    /// hand-edited config — but when it does, [`Keymap::lookup`] silently
    /// gives the key to whichever action is declared first, and a row that
    /// looks bound while doing nothing is exactly the confusing state the
    /// duplicate warning exists to prevent.
    pub fn is_ambiguous(&self, index: usize) -> bool {
        self.chords_at(index)
            .iter()
            .any(|c| !self.conflicts(index, c).is_empty())
    }

    /// Who else claims this row's chords, as a readable list.
    pub fn shadowed_by(&self, index: usize) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = self
            .chords_at(index)
            .iter()
            .flat_map(|c| self.conflicts(index, c))
            .filter_map(|i| ACTIONS.get(i).map(|s| s.label))
            .collect();
        out.dedup();
        out
    }

    /// Rows whose chords the host terminal probably eats, with the worst
    /// verdict across the row's chords.
    pub fn reach_at(&self, index: usize) -> Reach {
        self.chords_at(index)
            .iter()
            .map(|c| host_warning(c).0)
            .fold(Reach::Fine, |worst, r| match (worst, r) {
                (Reach::Blocked, _) | (_, Reach::Blocked) => Reach::Blocked,
                (Reach::Risky, _) | (_, Reach::Risky) => Reach::Risky,
                _ => Reach::Fine,
            })
    }
}

fn parse_list(specs: &[&str]) -> Vec<KeyChord> {
    specs.iter().filter_map(|s| KeyChord::parse(s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyChord {
        KeyChord::from_event(&KeyEvent::new(code, mods))
    }

    #[test]
    fn every_default_parses_and_round_trips() {
        for spec in ACTIONS {
            for raw in spec.defaults {
                let chord = KeyChord::parse(raw)
                    .unwrap_or_else(|| panic!("{}: {raw:?} does not parse", spec.id));
                let reparsed = KeyChord::parse(&chord.spec())
                    .unwrap_or_else(|| panic!("{}: {raw:?} does not round-trip", spec.id));
                assert_eq!(chord, reparsed, "{}: {raw:?}", spec.id);
            }
        }
    }

    #[test]
    fn action_ids_are_unique() {
        let mut ids: Vec<&str> = ACTIONS.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate action id");
    }

    #[test]
    fn defaults_do_not_collide_within_a_scope() {
        let map = Keymap::default();
        for (i, _) in ACTIONS.iter().enumerate() {
            for chord in map.chords_at(i) {
                assert!(
                    map.conflicts(i, chord).is_empty(),
                    "{} default {chord} collides with {:?}",
                    ACTIONS[i].id,
                    map.conflicts(i, chord)
                        .iter()
                        .map(|j| ACTIONS[*j].id)
                        .collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn shift_letters_normalize_both_spellings() {
        let upper = ev(KeyCode::Char('J'), KeyModifiers::SHIFT);
        let bare_upper = ev(KeyCode::Char('J'), KeyModifiers::NONE);
        let lower_shift = ev(KeyCode::Char('j'), KeyModifiers::SHIFT);
        assert_eq!(upper, bare_upper);
        assert_eq!(upper, lower_shift);
        assert_eq!(upper.spec(), "shift+j");
        assert_eq!(upper.display(), "⇧J");
        assert_ne!(upper, ev(KeyCode::Char('j'), KeyModifiers::NONE));
    }

    #[test]
    fn punctuation_drops_the_shift_bit() {
        // '?' is shift+/ on a US layout; the glyph already says so.
        assert_eq!(
            ev(KeyCode::Char('?'), KeyModifiers::SHIFT),
            ev(KeyCode::Char('?'), KeyModifiers::NONE)
        );
        assert_eq!(KeyChord::parse("?").unwrap().spec(), "?");
    }

    #[test]
    fn shift_enter_is_its_own_chord_and_flagged_as_kitty_only() {
        let map = Keymap::default();
        let shift_enter = ev(KeyCode::Enter, KeyModifiers::SHIFT);
        assert_eq!(
            map.lookup(Scope::Global, &shift_enter),
            Some(Action::OpenWorktree)
        );
        assert_eq!(
            map.lookup(Scope::Global, &ev(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::Activate)
        );
        assert_eq!(shift_enter.spec(), "shift+enter");
        assert_eq!(host_warning(&shift_enter).0, Reach::Risky);
        // …so a stock terminal gets a letter for it too, and the ESC CR a
        // Shift+Enter mapping (VS Code's /terminal-setup, Option as Meta)
        // sends — which tmux passes through where it flattens the shifted
        // Enter — lands on the same action as Alt+Enter.
        assert_eq!(
            map.lookup(Scope::Global, &ev(KeyCode::Char('O'), KeyModifiers::SHIFT)),
            Some(Action::OpenWorktree)
        );
        assert_eq!(
            map.lookup(Scope::Global, &ev(KeyCode::Enter, KeyModifiers::ALT)),
            Some(Action::OpenWorktree)
        );
    }

    #[test]
    fn backtab_is_shift_tab() {
        assert_eq!(
            ev(KeyCode::BackTab, KeyModifiers::NONE),
            KeyChord::parse("shift+tab").unwrap()
        );
    }

    #[test]
    fn legacy_ctrl_5_is_ctrl_bracket() {
        assert_eq!(
            ev(KeyCode::Char('5'), KeyModifiers::CONTROL),
            KeyChord::parse("ctrl+]").unwrap()
        );
    }

    #[test]
    fn parses_the_literal_plus_key() {
        let plus = KeyChord::parse("+").unwrap();
        assert_eq!(plus.code, KeyCode::Char('+'));
        assert_eq!(KeyChord::parse("ctrl++").unwrap().code, KeyCode::Char('+'));
    }

    #[test]
    fn lookup_respects_scope() {
        let map = Keymap::default();
        let ctrl_q = KeyChord::parse("ctrl+q").unwrap();
        assert_eq!(
            map.lookup(Scope::Terminal, &ctrl_q),
            Some(Action::UnlockTerminal)
        );
        // ^q in the panels is free — the scopes never read the same press.
        assert_eq!(map.lookup(Scope::Global, &ctrl_q), None);
        assert_eq!(
            map.lookup(Scope::Global, &KeyChord::parse("q").unwrap()),
            Some(Action::Quit)
        );
    }

    /// ^d / ^u are panel chords only: a locked pane must keep forwarding
    /// them as the shell's EOF and kill-to-start, so the Terminal scope
    /// never answers to them.
    #[test]
    fn half_page_chords_are_bound_in_the_panels_and_not_the_locked_pane() {
        let map = Keymap::default();
        let ctrl_d = KeyChord::parse("ctrl+d").unwrap();
        let ctrl_u = KeyChord::parse("ctrl+u").unwrap();
        assert_eq!(
            map.lookup(Scope::Global, &ctrl_d),
            Some(Action::HalfPageDown)
        );
        assert_eq!(map.lookup(Scope::Global, &ctrl_u), Some(Action::HalfPageUp));
        assert_eq!(map.lookup(Scope::Terminal, &ctrl_d), None);
        assert_eq!(map.lookup(Scope::Terminal, &ctrl_u), None);
        // Plain, kitty-free control bytes: every emulator delivers them.
        assert!(host_warning(&ctrl_d).0.is_fine());
        assert!(host_warning(&ctrl_u).0.is_fine());
    }

    /// `t` / `⇧T` is a SHIFT PAIR (#93): the lowercase key does it inside
    /// nebula, the shifted one outside — a shell terminal in the pane, a
    /// Ghostty tab. `⇧C`, the Ghostty tab's old key, is free.
    #[test]
    fn t_is_a_terminal_in_nebula_and_shift_t_one_in_ghostty() {
        let map = Keymap::default();
        let at = |spec: &str| map.lookup(Scope::Global, &KeyChord::parse(spec).unwrap());
        assert_eq!(at("t"), Some(Action::NewTerminal));
        assert_eq!(at("shift+t"), Some(Action::OpenGhosttyTab));
        assert_eq!(at("shift+c"), None, "⇧C is free");
        assert_eq!(map.label(Action::NewTerminal), "t");
    }

    /// Help and the footer leave the ⌘ aliases out: they are bound, and
    /// Settings → Hotkeys lists them, but only the key that arrives in
    /// every terminal is printed. A ⌘-only binding still shows.
    #[test]
    fn shown_chords_leave_the_cmd_aliases_out() {
        let mut map = Keymap::default();
        let cmd_p = KeyChord::parse("cmd+p").unwrap();
        assert!(
            map.chords(Action::ProjectDropdown).contains(&cmd_p),
            "still bound"
        );
        assert_eq!(
            map.lookup(Scope::Global, &cmd_p),
            Some(Action::ProjectDropdown),
            "and still answers"
        );
        assert!(
            map.label(Action::ProjectDropdown).contains('⌘'),
            "Hotkeys lists it"
        );
        assert!(!map.shown_label(Action::ProjectDropdown).contains('⌘'));
        assert_eq!(map.shown_label(Action::ProjectDropdown), "+");
        for n in 1..=9u8 {
            let action = Action::SelectProjectTab(n);
            assert_eq!(map.shown_label(action), n.to_string());
            assert_eq!(
                map.first(action).map(|c| c.display()),
                Some(n.to_string()),
                "the digit leads the slot's defaults"
            );
        }
        let help = index_of(Action::Help).unwrap();
        map.bind(help, KeyChord::parse("cmd+k").unwrap(), false);
        assert_eq!(map.shown_label(Action::Help), "⌘k", "⌘ alone still shows");
        assert_eq!(
            map.shown_first(Action::Help).map(|c| c.display()),
            Some("⌘k".to_string())
        );
    }

    #[test]
    fn binding_steals_the_chord_from_its_previous_owner() {
        let mut map = Keymap::default();
        let g = KeyChord::parse("g").unwrap();
        let open_repo = index_of(Action::OpenRepo).unwrap();
        assert_eq!(
            map.conflicts(open_repo, &g),
            vec![index_of(Action::GitDiff).unwrap()]
        );
        map.bind(open_repo, g, false);
        assert_eq!(map.lookup(Scope::Global, &g), Some(Action::OpenRepo));
        assert!(!map.chords(Action::GitDiff).contains(&g));
    }

    #[test]
    fn overrides_round_trip_through_the_config_shape() {
        let mut map = Keymap::default();
        let quit = index_of(Action::Quit).unwrap();
        map.bind(quit, KeyChord::parse("ctrl+x").unwrap(), false);
        let saved = map.overrides();
        assert_eq!(saved.get("quit").map(String::as_str), Some("ctrl+x"));
        // Untouched actions stay out of the file.
        assert!(!saved.contains_key("help"));
        let loaded = Keymap::from_overrides(&saved);
        assert_eq!(
            loaded.lookup(Scope::Global, &KeyChord::parse("ctrl+x").unwrap()),
            Some(Action::Quit)
        );
        assert_eq!(
            loaded.lookup(Scope::Global, &KeyChord::parse("q").unwrap()),
            None
        );
    }

    #[test]
    fn a_saved_binding_outranks_a_default_added_later() {
        // Saved before `d` was the diff's and `z` full screen's: the
        // user's rows keep them, the default rows give them up.
        let map = Keymap::from_overrides(&BTreeMap::from([
            ("delete".into(), "d, delete".into()),
            ("tree_browser".into(), "z".into()),
        ]));
        let d = KeyChord::parse("d").unwrap();
        let z = KeyChord::parse("z").unwrap();
        assert_eq!(map.lookup(Scope::Global, &d), Some(Action::Delete));
        assert_eq!(map.lookup(Scope::Global, &z), Some(Action::TreeBrowser));
        assert_eq!(
            map.chords(Action::GitDiff),
            &[KeyChord::parse("g").unwrap()]
        );
        assert_eq!(
            map.chords(Action::ToggleFullScreen),
            &[KeyChord::parse("ctrl+f").unwrap()]
        );
        // Nothing else moves.
        assert!(!map.is_ambiguous(index_of(Action::Delete).unwrap()));
        assert_eq!(
            map.lookup(Scope::Global, &KeyChord::parse("?").unwrap()),
            Some(Action::Verbs)
        );
    }

    #[test]
    fn a_default_row_that_gave_a_chord_up_is_never_saved() {
        let mut map =
            Keymap::from_overrides(&BTreeMap::from([("delete".into(), "d, delete".into())]));
        map.bind(
            index_of(Action::Help).unwrap(),
            KeyChord::parse("f1").unwrap(),
            false,
        );
        let saved = map.overrides();
        assert!(!saved.contains_key("git_diff"), "{saved:?}");
        assert_eq!(saved["delete"], "d, delete");
        // Once the user's row lets `d` go, the diff has it back.
        let map = Keymap::from_overrides(&BTreeMap::from([("delete".into(), "delete".into())]));
        assert_eq!(
            map.chords(Action::GitDiff),
            parse_list(&["d", "g"]).as_slice()
        );
        // A steal is the stealer's: the row it came from is not saved.
        let mut map = Keymap::default();
        map.bind(
            index_of(Action::Help).unwrap(),
            KeyChord::parse("g").unwrap(),
            false,
        );
        assert_eq!(map.overrides().keys().collect::<Vec<_>>(), ["help"]);
        let loaded = Keymap::from_overrides(&map.overrides());
        assert_eq!(loaded.chords(Action::GitDiff), map.chords(Action::GitDiff));
    }

    #[test]
    fn comma_chords_survive_the_config_shape() {
        let mut map = Keymap::default();
        let settings = index_of(Action::Settings).unwrap();
        let prev = index_of(Action::PrevAttention).unwrap();
        map.bind(settings, KeyChord::parse("f2").unwrap(), true);
        map.bind(prev, KeyChord::parse(";").unwrap(), true);
        let saved = map.overrides();
        assert_eq!(saved["settings"], "s, ctrl+,, f2");
        let loaded = Keymap::from_overrides(&saved);
        assert_eq!(
            loaded.chords(Action::Settings),
            map.chords(Action::Settings)
        );
        assert_eq!(
            loaded.chords(Action::PrevAttention),
            map.chords(Action::PrevAttention)
        );
        assert!(loaded
            .chords(Action::PrevAttention)
            .contains(&KeyChord::parse(",").unwrap()));
    }

    #[test]
    fn an_empty_override_means_unbound() {
        let map = Keymap::from_overrides(&BTreeMap::from([("help".into(), String::new())]));
        assert!(map.chords(Action::Help).is_empty());
        assert_eq!(map.label(Action::Help), "-");
    }

    #[test]
    fn a_broken_override_falls_back_instead_of_stranding_the_user() {
        let map = Keymap::from_overrides(&BTreeMap::from([
            ("quit".into(), "nonsense+zz".into()),
            ("no_such_action".into(), "x".into()),
        ]));
        assert!(map.chords(Action::Quit).is_empty(), "the row is cleared");
        assert_eq!(
            map.lookup(Scope::Global, &KeyChord::parse("?").unwrap()),
            Some(Action::Verbs),
            "the rest of the map is untouched"
        );
    }

    /// The keys a 0.48 user rebound (`crates/nebula/tests/fixtures/
    /// upgrade`) still bind after the 0.49 key table: every id is still a
    /// verb, their chords win over defaults added since (`z` for full
    /// screen), help on their F12 leaves `?` to the verbs menu, and a save
    /// writes the same overrides back.
    #[test]
    fn a_048_users_rebound_keys_still_bind() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../nebula/tests/fixtures/upgrade/v0.48/data/config.json"
        ))
        .unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let theirs: BTreeMap<String, String> =
            serde_json::from_value(json["keybindings"].clone()).unwrap();
        assert_eq!(theirs.len(), 6);
        for id in theirs.keys() {
            assert!(
                ACTIONS.iter().any(|s| s.id == id),
                "{id} is no longer a verb"
            );
        }
        let map = Keymap::from_overrides(&theirs);
        let at = |chord: &str| map.lookup(Scope::Global, &KeyChord::parse(chord).unwrap());
        assert_eq!(at("ctrl+n"), Some(Action::QuickPrompt));
        assert_ne!(at("p"), Some(Action::QuickPrompt));
        assert_eq!(at("z"), Some(Action::Screenshot));
        assert_eq!(at("f12"), Some(Action::Help));
        assert_eq!(at("?"), Some(Action::Verbs));
        assert_eq!(at("f2"), Some(Action::Rename));
        assert_eq!(at("r"), Some(Action::Rename));
        assert_eq!(at("alt+k"), Some(Action::ReorderUp));
        assert!(map.chords(Action::Archive).is_empty());
        assert_eq!(map.overrides(), theirs);
    }

    #[test]
    fn a_hand_edited_duplicate_is_flagged_on_both_rows() {
        // Nothing in the overlay can produce this; a text editor can. (One
        // override alone takes the chord off a row on its defaults.)
        let map = Keymap::from_overrides(&BTreeMap::from([
            ("open_repo".into(), "g".into()),
            ("git_diff".into(), "d, g".into()),
        ]));
        let open_repo = index_of(Action::OpenRepo).unwrap();
        let diff = index_of(Action::GitDiff).unwrap();
        assert!(map.is_ambiguous(open_repo));
        assert!(map.is_ambiguous(diff));
        assert_eq!(map.shadowed_by(open_repo), vec!["Git diff"]);
        // The defaults themselves are always unambiguous.
        assert!(Keymap::default()
            .binds
            .iter()
            .enumerate()
            .all(|(i, _)| !Keymap::default().is_ambiguous(i)));
    }

    #[test]
    fn cmd_chords_are_reported_unreachable() {
        let (reach, why) = host_warning(&KeyChord::parse("cmd+]").unwrap());
        assert_eq!(reach, Reach::Blocked);
        assert!(why.unwrap().contains('⌘'));
    }

    #[test]
    fn risky_chords_are_flagged_but_allowed() {
        for spec in ["ctrl+shift+f", "ctrl+left", "alt+p", "ctrl+m", "f13"] {
            let chord = KeyChord::parse(spec).unwrap();
            let (reach, why) = host_warning(&chord);
            assert_eq!(reach, Reach::Risky, "{spec}");
            assert!(why.is_some(), "{spec}");
        }
    }

    #[test]
    fn ordinary_chords_are_clean() {
        for spec in ["q", "shift+j", "/", "ctrl+q", "down", "enter", "f5"] {
            let chord = KeyChord::parse(spec).unwrap();
            assert!(host_warning(&chord).0.is_fine(), "{spec} should be fine");
        }
    }

    #[test]
    fn every_action_ships_with_a_reachable_chord() {
        // Some defaults are deliberately iffy (^] is a fallback hatch, ⌘P
        // an alias), but no action may be *only* reachable through a chord
        // the host terminal is likely to eat.
        let map = Keymap::default();
        for (i, spec) in ACTIONS.iter().enumerate() {
            // The menus' own verbs ship unbound: a menu row, the verbs
            // menu and the palette reach them, and a user may bind one.
            if spec.defaults.is_empty() {
                assert!(
                    matches!(
                        spec.action,
                        Action::Restart
                            | Action::NewWorktree
                            | Action::PinOrchestrator
                            | Action::MoveToAccount
                            | Action::ToggleRun
                            | Action::DeleteWorktree
                            | Action::RemoveProject
                            | Action::SweepWorktrees
                            | Action::ExportSettings
                            | Action::ImportSettings
                            | Action::CreatePullRequest
                            | Action::MergePullRequest
                            | Action::FocusScope
                            | Action::ScopeAccount
                            | Action::WhatsNew
                            | Action::OpenInVscode
                            | Action::OpenInVisualStudio
                            | Action::DockEditor
                            | Action::RevealInExplorer
                            | Action::OpenWithDefault
                            | Action::OpenFileInKete
                            | Action::CopyPath
                            | Action::CopyRelativePath
                            | Action::CopyBranch
                            | Action::WakeAccount
                            | Action::Critters
                            | Action::MarkSeen
                            | Action::CopyPrUrl
                            | Action::ArchiveDone
                            | Action::WakeSchedule
                            | Action::AskAside
                            // 0.60: the studio verbs, menu and palette rows.
                            | Action::StudioBoard
                            | Action::GauntletView
                            | Action::TriageNote
                            | Action::StudioAutonomy
                            | Action::RenewOrchestrator
                            | Action::UsageCsv
                    ),
                    "{} ships with no chord",
                    spec.id
                );
                continue;
            }
            assert!(
                map.chords_at(i).iter().any(|c| host_warning(c).0.is_fine()),
                "{} has no chord a stock terminal delivers",
                spec.id
            );
        }
    }

    // Pins every named key's config spelling, on-screen glyph, and the
    // alternate spellings `parse` accepts, so a table rewrite can't drift
    // a single string the hotkeys tab or a saved config depends on.
    #[test]
    fn named_keys_keep_their_spellings_and_glyphs() {
        let table: &[(KeyCode, &str, &str, &[&str])] = &[
            (KeyCode::Up, "up", "↑", &[]),
            (KeyCode::Down, "down", "↓", &[]),
            (KeyCode::Left, "left", "←", &[]),
            (KeyCode::Right, "right", "→", &[]),
            (KeyCode::Enter, "enter", "Enter", &["return", "cr"]),
            (KeyCode::Tab, "tab", "Tab", &[]),
            (KeyCode::BackTab, "tab", "Tab", &[]),
            (KeyCode::Esc, "esc", "Esc", &["escape"]),
            (KeyCode::Backspace, "backspace", "⌫", &["bs"]),
            (KeyCode::Delete, "delete", "Del", &["del"]),
            (KeyCode::Insert, "insert", "Ins", &["ins"]),
            (KeyCode::Home, "home", "Home", &[]),
            (KeyCode::End, "end", "End", &[]),
            (KeyCode::PageUp, "pgup", "PgUp", &["pageup"]),
            (KeyCode::PageDown, "pgdn", "PgDn", &["pagedown"]),
            (KeyCode::Char(' '), "space", "Space", &[]),
        ];
        for (code, name, glyph, aliases) in table {
            assert_eq!(key_name(*code), *name, "{code:?}");
            assert_eq!(key_display(*code), *glyph, "{code:?}");
            if *code != KeyCode::BackTab {
                assert_eq!(parse_key_name(name), Some(*code), "{name}");
            }
            for alias in *aliases {
                assert_eq!(parse_key_name(alias), Some(*code), "{alias}");
            }
        }
        assert_eq!(parse_key_name("backtab"), Some(KeyCode::BackTab));
        // The open-ended arms: letters, function keys, and crossterm's
        // Debug spelling for anything else.
        assert_eq!(key_name(KeyCode::Char('Q')), "q");
        assert_eq!(key_display(KeyCode::Char('Q')), "Q");
        assert_eq!(parse_key_name("q"), Some(KeyCode::Char('q')));
        assert_eq!(parse_key_name("qq"), None);
        assert_eq!(key_name(KeyCode::F(12)), "f12");
        assert_eq!(key_display(KeyCode::F(12)), "F12");
        assert_eq!(parse_key_name("f12"), Some(KeyCode::F(12)));
        assert_eq!(parse_key_name("f21"), None);
        assert_eq!(key_name(KeyCode::Null), "null");
        assert_eq!(key_display(KeyCode::Null), "Null");
    }
}
