//! The sidebar's now-playing panel for WACUP (and Winamp): reads the
//! player and sends its transport buttons through the Winamp-compatible
//! window-message API, from outside the player. Nothing is injected into
//! WACUP and no plug-in is installed in it.
//!
//! The player's main window has the class `Winamp v1.x`. Over it:
//! - `WM_USER` (`WM_WA_IPC`) with `IPC_ISPLAYING` (104) answers 1 playing,
//!   3 paused, 0 stopped; `IPC_GETOUTPUTTIME` (105) answers the position in
//!   ms (mode 0) and the track length in ms (mode 2) or seconds (mode 1).
//! - `WM_COMMAND` with 40044 (previous), 40045 (play), 40046 (pause), 40047
//!   (stop) and 40048 (next) press the transport buttons.
//! - `IPC_GETPLAYLISTTITLE` returns a pointer inside the player's own
//!   process and the SDK marks it plug-in only, so the title is read from
//!   the window's caption instead (`3. Artist - Title - WACUP`).
//!
//! `docs/wacup.md` says what was checked against a real WACUP and what was
//! not. The logic here runs over [`Bus`]; the tests give it a fake one, and
//! [`sys`] is the Windows one. Built with the `wacup` feature (on by
//! default) and only on Windows; elsewhere the commands answer
//! "unsupported" and the panel stays hidden.

#![cfg_attr(not(all(windows, feature = "wacup")), allow(dead_code))]

use serde::Serialize;
use std::path::{Path, PathBuf};

/// The window class of Winamp's main window, kept by WACUP.
pub const CLASS: &str = "Winamp v1.x";
const WM_WA_IPC: u32 = 0x0400;
const WM_COMMAND: u32 = 0x0111;
const IPC_ISPLAYING: isize = 104;
const IPC_GETOUTPUTTIME: isize = 105;
const BUTTON_PREVIOUS: usize = 40044;
const BUTTON_PLAY: usize = 40045;
const BUTTON_PAUSE: usize = 40046;
const BUTTON_STOP: usize = 40047;
const BUTTON_NEXT: usize = 40048;

/// The player found: its main window and the program it runs in.
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub hwnd: isize,
    /// The executable's file name, lowercase (`wacup.exe`).
    pub process: String,
}

/// What the module needs of the system: find the window, send it a
/// message with a timeout (a hung player must not hang the window), read
/// its caption.
pub trait Bus {
    fn find(&self) -> Option<Player>;
    /// `WM_WA_IPC` with `wparam` and the IPC id; None when the player did
    /// not answer in time.
    fn ipc(&self, player: &Player, wparam: usize, ipc: isize) -> Option<isize>;
    /// `WM_COMMAND` with the button id; false when it was not delivered.
    fn command(&self, player: &Player, id: usize) -> bool;
    fn caption(&self, player: &Player) -> String;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    Playing,
    Paused,
    Stopped,
}

impl Playback {
    fn word(self) -> &'static str {
        match self {
            Playback::Playing => "playing",
            Playback::Paused => "paused",
            Playback::Stopped => "stopped",
        }
    }
}

/// What the panel draws.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct State {
    /// This build can talk to a player (Windows, `wacup` feature).
    pub supported: bool,
    pub running: bool,
    /// `WACUP`, `Winamp`, or the program's file name.
    pub player: String,
    /// `playing`, `paused` or `stopped`.
    pub playback: String,
    pub title: String,
    pub position_ms: i64,
    /// 0 when the player does not know (a stream).
    pub length_ms: i64,
    /// `wacup.exe` was found, so Launch can start it.
    pub can_launch: bool,
}

impl State {
    #[cfg_attr(all(windows, feature = "wacup"), allow(dead_code))]
    pub fn unsupported() -> Self {
        State {
            supported: false,
            ..State::idle(false)
        }
    }

    /// The player is not running.
    pub fn idle(can_launch: bool) -> Self {
        State {
            supported: true,
            running: false,
            player: String::new(),
            playback: Playback::Stopped.word().into(),
            title: String::new(),
            position_ms: 0,
            length_ms: 0,
            can_launch,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Previous,
    PlayPause,
    Stop,
    Next,
}

impl Transport {
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "previous" => Some(Self::Previous),
            "play_pause" => Some(Self::PlayPause),
            "stop" => Some(Self::Stop),
            "next" => Some(Self::Next),
            _ => None,
        }
    }
}

fn friendly(process: &str) -> String {
    match process.trim_end_matches(".exe") {
        "wacup" => "WACUP".into(),
        "winamp" => "Winamp".into(),
        other => other.into(),
    }
}

/// The track a player's caption names: `3. Artist - Title - WACUP`, with
/// the number, the program name and a `[Paused]` or `[Stopped]` mark taken
/// off. A caption that is only the program's name has no track.
pub fn clean_title(caption: &str) -> String {
    let mut t = caption.trim();
    for mark in ["[Paused]", "[Stopped]", "[paused]", "[stopped]"] {
        t = t.trim_end_matches(mark).trim_end();
    }
    for name in [" - WACUP", " - Winamp", " - Winamp (WACUP)"] {
        if let Some(rest) = t.strip_suffix(name) {
            t = rest.trim_end();
            break;
        }
    }
    if matches!(t, "WACUP" | "Winamp") {
        return String::new();
    }
    // "12. " at the front is the playlist number.
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        if let Some(rest) = t[digits..].strip_prefix(". ") {
            t = rest;
        }
    }
    t.trim().to_string()
}

fn millis(v: Option<isize>) -> i64 {
    v.filter(|n| *n > 0).map_or(0, |n| n as i64)
}

/// The player's state now; `can_launch` rides along for the idle one.
pub fn read(bus: &dyn Bus, can_launch: bool) -> State {
    let Some(player) = bus.find() else {
        return State::idle(can_launch);
    };
    let playback = match bus.ipc(&player, 0, IPC_ISPLAYING) {
        Some(1) => Playback::Playing,
        Some(3) => Playback::Paused,
        _ => Playback::Stopped,
    };
    let (position_ms, length_ms) = if playback == Playback::Stopped {
        (0, 0)
    } else {
        let position = millis(bus.ipc(&player, 0, IPC_GETOUTPUTTIME));
        // Mode 2 is the length in ms where the player has it; older ones
        // answer mode 1 in seconds only.
        let length = match millis(bus.ipc(&player, 2, IPC_GETOUTPUTTIME)) {
            0 => millis(bus.ipc(&player, 1, IPC_GETOUTPUTTIME)) * 1000,
            ms => ms,
        };
        (position, length)
    };
    State {
        supported: true,
        running: true,
        player: friendly(&player.process),
        playback: playback.word().into(),
        title: clean_title(&bus.caption(&player)),
        position_ms,
        length_ms,
        can_launch,
    }
}

/// Press a transport button. Play/pause is the pause button while a track
/// is loaded (it pauses and resumes) and the play button when stopped.
pub fn send(bus: &dyn Bus, what: Transport) -> Result<(), String> {
    let player = bus.find().ok_or("WACUP is not running")?;
    let id = match what {
        Transport::Previous => BUTTON_PREVIOUS,
        Transport::Next => BUTTON_NEXT,
        Transport::Stop => BUTTON_STOP,
        Transport::PlayPause => match bus.ipc(&player, 0, IPC_ISPLAYING) {
            Some(1) | Some(3) => BUTTON_PAUSE,
            _ => BUTTON_PLAY,
        },
    };
    if bus.command(&player, id) {
        Ok(())
    } else {
        Err("WACUP did not answer".into())
    }
}

/// Where `wacup.exe` may be, best first: the registry's App Paths, then
/// the Program Files folders.
pub fn exe_candidates(app_paths: &[PathBuf], program_files: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = app_paths.to_vec();
    out.extend(
        program_files
            .iter()
            .map(|p| p.join("WACUP").join("wacup.exe")),
    );
    out
}

/// The first candidate that exists.
pub fn find_exe(candidates: &[PathBuf], exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    candidates.iter().find(|p| exists(p)).cloned()
}

#[cfg(all(windows, feature = "wacup"))]
mod sys {
    //! The Windows bus: `FindWindowW`, `SendMessageTimeoutW` (so a hung
    //! player costs a quarter second, not the window), `GetWindowTextW`;
    //! the process name through `QueryFullProcessImageNameW`; the install
    //! path from the registry's App Paths, read only.

    use super::{Bus, Player, CLASS, WM_COMMAND, WM_WA_IPC};
    use std::path::PathBuf;
    use windows::core::{w, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ,
        RRF_SUBKEY_WOW6432KEY, RRF_SUBKEY_WOW6464KEY,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowTextW, GetWindowThreadProcessId, SendMessageTimeoutW,
        SMTO_ABORTIFHUNG,
    };

    const TIMEOUT_MS: u32 = 250;

    pub struct Win;

    fn hwnd(p: &Player) -> HWND {
        HWND(p.hwnd as *mut core::ffi::c_void)
    }

    fn process_name(window: HWND) -> String {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        if pid == 0 {
            return String::new();
        }
        let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
        else {
            return String::new();
        };
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = unsafe {
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
        };
        let _ = unsafe { CloseHandle(handle) };
        if ok.is_err() {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/'])
            .next()
            .unwrap_or_default()
            .to_lowercase()
    }

    fn send(p: &Player, msg: u32, wparam: usize, lparam: isize) -> Option<isize> {
        let mut result = 0usize;
        let sent = unsafe {
            SendMessageTimeoutW(
                hwnd(p),
                msg,
                WPARAM(wparam),
                LPARAM(lparam),
                SMTO_ABORTIFHUNG,
                TIMEOUT_MS,
                Some(&mut result),
            )
        };
        // A 32-bit player's answer is a signed int: -1 comes back as
        // 0xFFFFFFFF in a 64-bit usize.
        (sent.0 != 0).then_some(result as u32 as i32 as isize)
    }

    impl Bus for Win {
        fn find(&self) -> Option<Player> {
            let class: Vec<u16> = CLASS.encode_utf16().chain(Some(0)).collect();
            let window = unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) }.ok()?;
            Some(Player {
                hwnd: window.0 as isize,
                process: process_name(window),
            })
        }

        fn ipc(&self, player: &Player, wparam: usize, ipc: isize) -> Option<isize> {
            send(player, WM_WA_IPC, wparam, ipc)
        }

        fn command(&self, player: &Player, id: usize) -> bool {
            send(player, WM_COMMAND, id & 0xFFFF, 0).is_some()
        }

        fn caption(&self, player: &Player) -> String {
            let mut buf = [0u16; 512];
            let n = unsafe { GetWindowTextW(hwnd(player), &mut buf) };
            String::from_utf16_lossy(&buf[..n.max(0) as usize])
        }
    }

    fn app_path(
        root: HKEY,
        view: windows::Win32::System::Registry::REG_ROUTINE_FLAGS,
    ) -> Option<PathBuf> {
        let mut buf = [0u16; 520];
        let mut bytes = (buf.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                root,
                w!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\wacup.exe"),
                PCWSTR::null(),
                RRF_RT_REG_SZ | view,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        };
        if status.0 != 0 {
            return None;
        }
        let chars = (bytes as usize / 2).saturating_sub(1).min(buf.len());
        let text = String::from_utf16_lossy(&buf[..chars]);
        let text = text.trim().trim_matches('"');
        (!text.is_empty()).then(|| PathBuf::from(text))
    }

    /// The App Paths default value for `wacup.exe`, in either registry view
    /// of both hives.
    pub fn registered() -> Vec<PathBuf> {
        let mut out = Vec::new();
        for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            for view in [RRF_SUBKEY_WOW6464KEY, RRF_SUBKEY_WOW6432KEY] {
                if let Some(p) = app_path(root, view) {
                    if !out.contains(&p) {
                        out.push(p);
                    }
                }
            }
        }
        out
    }

    pub fn program_files() -> Vec<PathBuf> {
        ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
            .iter()
            .filter_map(|k| std::env::var_os(k).map(PathBuf::from))
            .fold(Vec::new(), |mut v, p| {
                if !v.contains(&p) {
                    v.push(p);
                }
                v
            })
    }
}

#[cfg(all(windows, feature = "wacup"))]
fn installed() -> Option<PathBuf> {
    let candidates = exe_candidates(&sys::registered(), &sys::program_files());
    find_exe(&candidates, |p| p.is_file())
}

/// The panel's poll: the player's state, or "unsupported" off Windows.
#[tauri::command]
pub async fn wacup_state() -> State {
    #[cfg(all(windows, feature = "wacup"))]
    {
        tokio::task::spawn_blocking(|| {
            let found = sys::Win.find().is_some();
            // The install is looked up only when there is nothing running.
            let can_launch = !found && installed().is_some();
            read(&sys::Win, can_launch)
        })
        .await
        .unwrap_or_else(|_| State::idle(false))
    }
    #[cfg(not(all(windows, feature = "wacup")))]
    {
        State::unsupported()
    }
}

/// A transport button: `previous`, `play_pause`, `stop` or `next`.
#[tauri::command]
pub async fn wacup_transport(what: String) -> Result<(), String> {
    let what = Transport::parse(&what).ok_or_else(|| format!("no such control: {what}"))?;
    #[cfg(all(windows, feature = "wacup"))]
    {
        tokio::task::spawn_blocking(move || send(&sys::Win, what))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(all(windows, feature = "wacup")))]
    {
        let _ = what;
        Err("WACUP control is Windows only".into())
    }
}

/// Start `wacup.exe` from where it is installed.
#[tauri::command]
pub async fn wacup_launch() -> Result<(), String> {
    #[cfg(all(windows, feature = "wacup"))]
    {
        let exe = installed().ok_or("wacup.exe was not found in Program Files or the registry")?;
        let mut cmd = std::process::Command::new(&exe);
        if let Some(dir) = exe.parent() {
            cmd.current_dir(dir);
        }
        cmd.spawn()
            .map(|_| ())
            .map_err(|e| format!("could not start {}: {e}", exe.display()))
    }
    #[cfg(not(all(windows, feature = "wacup")))]
    {
        Err("WACUP control is Windows only".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A player that answers like WACUP, and records what it was sent.
    struct Fake {
        player: Option<Player>,
        playing: Option<isize>,
        position: Option<isize>,
        length_ms: Option<isize>,
        length_s: Option<isize>,
        caption: &'static str,
        sent: RefCell<Vec<usize>>,
        deaf: bool,
    }

    impl Fake {
        fn running(playing: isize) -> Self {
            Fake {
                player: Some(Player {
                    hwnd: 7,
                    process: "wacup.exe".into(),
                }),
                playing: Some(playing),
                position: Some(83_000),
                length_ms: Some(215_500),
                length_s: Some(215),
                caption: "3. Artist - Song - WACUP",
                sent: RefCell::new(Vec::new()),
                deaf: false,
            }
        }
    }

    impl Bus for Fake {
        fn find(&self) -> Option<Player> {
            self.player.clone()
        }
        fn ipc(&self, _: &Player, wparam: usize, ipc: isize) -> Option<isize> {
            match (ipc, wparam) {
                (IPC_ISPLAYING, _) => self.playing,
                (IPC_GETOUTPUTTIME, 0) => self.position,
                (IPC_GETOUTPUTTIME, 1) => self.length_s,
                (IPC_GETOUTPUTTIME, 2) => self.length_ms,
                _ => None,
            }
        }
        fn command(&self, _: &Player, id: usize) -> bool {
            self.sent.borrow_mut().push(id);
            !self.deaf
        }
        fn caption(&self, _: &Player) -> String {
            self.caption.into()
        }
    }

    #[test]
    fn no_window_is_not_running_and_carries_whether_it_can_launch() {
        let mut bus = Fake::running(1);
        bus.player = None;
        let s = read(&bus, true);
        assert!(s.supported && !s.running && s.can_launch);
        assert_eq!(s.title, "");
        assert!(!read(&bus, false).can_launch);
    }

    #[test]
    fn a_playing_player_reports_title_position_and_length() {
        let s = read(&Fake::running(1), false);
        assert!(s.running);
        assert_eq!(
            (s.player.as_str(), s.playback.as_str(), s.title.as_str()),
            ("WACUP", "playing", "Artist - Song")
        );
        assert_eq!((s.position_ms, s.length_ms), (83_000, 215_500));
    }

    #[test]
    fn paused_is_3_and_stopped_has_no_time() {
        assert_eq!(read(&Fake::running(3), false).playback, "paused");
        let s = read(&Fake::running(0), false);
        assert_eq!(
            (s.playback.as_str(), s.position_ms, s.length_ms),
            ("stopped", 0, 0)
        );
    }

    #[test]
    fn length_falls_back_to_seconds_and_unknown_is_zero() {
        let mut bus = Fake::running(1);
        bus.length_ms = Some(-1);
        assert_eq!(read(&bus, false).length_ms, 215_000);
        bus.length_s = Some(-1);
        assert_eq!(read(&bus, false).length_ms, 0);
        bus.position = Some(-1);
        assert_eq!(read(&bus, false).position_ms, 0);
    }

    #[test]
    fn an_unanswered_player_reads_as_stopped() {
        let mut bus = Fake::running(1);
        bus.playing = None;
        assert_eq!(read(&bus, false).playback, "stopped");
    }

    #[test]
    fn a_winamp_is_named_for_its_program() {
        let mut bus = Fake::running(1);
        bus.player = Some(Player {
            hwnd: 1,
            process: "winamp.exe".into(),
        });
        assert_eq!(read(&bus, false).player, "Winamp");
        bus.player = Some(Player {
            hwnd: 1,
            process: "".into(),
        });
        assert_eq!(read(&bus, false).player, "");
    }

    #[test]
    fn captions_lose_their_number_program_and_mark() {
        assert_eq!(clean_title("12. A - B - WACUP"), "A - B");
        assert_eq!(clean_title("1. A - B - Winamp [Paused]"), "A - B");
        assert_eq!(clean_title("A - B - Winamp [Stopped]"), "A - B");
        assert_eq!(clean_title("WACUP"), "");
        assert_eq!(clean_title("Winamp"), "");
        assert_eq!(clean_title("2020. Song"), "Song");
        assert_eq!(clean_title("2020 Song - X"), "2020 Song - X");
        assert_eq!(clean_title("  "), "");
    }

    #[test]
    fn the_transport_sends_the_five_button_ids() {
        for (what, id) in [
            (Transport::Previous, 40044),
            (Transport::Stop, 40047),
            (Transport::Next, 40048),
        ] {
            let bus = Fake::running(1);
            send(&bus, what).unwrap();
            assert_eq!(*bus.sent.borrow(), vec![id]);
        }
    }

    #[test]
    fn play_pause_pauses_a_loaded_track_and_plays_when_stopped() {
        for (state, id) in [(1, 40046), (3, 40046), (0, 40045)] {
            let bus = Fake::running(state);
            send(&bus, Transport::PlayPause).unwrap();
            assert_eq!(*bus.sent.borrow(), vec![id], "state {state}");
        }
    }

    #[test]
    fn a_transport_with_no_player_or_no_answer_is_an_error() {
        let mut bus = Fake::running(1);
        bus.deaf = true;
        assert!(send(&bus, Transport::Next).is_err());
        bus.player = None;
        assert!(send(&bus, Transport::Next)
            .unwrap_err()
            .contains("not running"));
        assert_eq!(Transport::parse("play_pause"), Some(Transport::PlayPause));
        assert_eq!(Transport::parse("rewind"), None);
    }

    #[test]
    fn the_install_is_the_registry_path_then_program_files() {
        let reg = vec![PathBuf::from(r"D:\Music\WACUP\wacup.exe")];
        let pf = vec![
            PathBuf::from(r"C:\Program Files"),
            PathBuf::from(r"C:\Program Files (x86)"),
        ];
        let c = exe_candidates(&reg, &pf);
        assert_eq!(c[0], reg[0]);
        assert_eq!(
            c[2],
            PathBuf::from(r"C:\Program Files (x86)")
                .join("WACUP")
                .join("wacup.exe")
        );
        let only_x86 = c[2].clone();
        assert_eq!(find_exe(&c, |p| p == only_x86), Some(only_x86.clone()));
        assert_eq!(find_exe(&c, |_| false), None);
        assert_eq!(find_exe(&c, |_| true), Some(reg[0].clone()));
    }
}

/// Against a real player: `cargo test wacup_live -- --ignored --nocapture`
/// with WACUP running (docs/wacup.md).
#[cfg(all(test, windows, feature = "wacup"))]
mod live {
    use super::*;

    #[test]
    #[ignore = "needs WACUP running"]
    fn wacup_live_reads_the_running_player() {
        let state = read(&sys::Win, installed().is_some());
        println!("{state:#?}");
        assert!(state.running, "start WACUP first");
        assert_eq!(state.player, "WACUP");
    }
}

#[cfg(all(test, windows, feature = "wacup"))]
mod live_transport {
    use super::*;

    /// Pauses and resumes the running player, then stops it and plays again.
    #[test]
    #[ignore = "needs WACUP running with a track loaded"]
    fn wacup_live_transport_pauses_resumes_and_stops() {
        let bus = &sys::Win;
        let wait = || std::thread::sleep(std::time::Duration::from_millis(400));
        send(bus, Transport::PlayPause).unwrap();
        wait();
        assert_eq!(read(bus, false).playback, "paused");
        send(bus, Transport::PlayPause).unwrap();
        wait();
        assert_eq!(read(bus, false).playback, "playing");
        send(bus, Transport::Stop).unwrap();
        wait();
        assert_eq!(read(bus, false).playback, "stopped");
        send(bus, Transport::PlayPause).unwrap();
        wait();
        assert_eq!(read(bus, false).playback, "playing");
    }
}
