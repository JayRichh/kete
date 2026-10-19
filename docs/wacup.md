# WACUP panel

The desktop window's sidebar has a now-playing panel for [WACUP](https://getwacup.com) on Windows: title,
previous, play or pause, next, and elapsed over length. Where WACUP is not running it says so and
offers Launch. It is hidden on other platforms. It polls once a second while it is on screen (the
sidebar open and the window visible).

The Windows side is `desktop/src-tauri/src/wacup.rs`, built by the `wacup` cargo feature (on by
default; `--no-default-features` leaves it out).

## What it talks to

WACUP keeps Winamp's main window and its window messages. Nothing is injected into WACUP and no
plug-in is installed in it.

| Need | Call | Checked |
|---|---|---|
| Find the player | `FindWindowW("Winamp v1.x")`; the program is read from the window's process (`wacup.exe` is named WACUP, `winamp.exe` Winamp) | Real WACUP, 2026-10-19 |
| Playing, paused, stopped | `WM_USER` with `IPC_ISPLAYING` (104): 1, 3, 0 | Real WACUP, 2026-10-19 |
| Position, length | `IPC_GETOUTPUTTIME` (105): mode 0 position in ms, mode 2 length in ms, mode 1 length in s | Length and mode 2: real WACUP. Position: always 0 on the test machine (see below) |
| Transport | `WM_COMMAND` 40044 previous, 40045 play, 40046 pause, 40047 stop, 40048 next | Play, pause (toggles), stop: real WACUP. Previous and next: not exercised (the test list had one track) |
| Title | The window caption, `3. Artist - Title - Winamp` | Real WACUP: its caption ends ` - Winamp`, not ` - WACUP`; the panel strips either |

Each message goes through `SendMessageTimeoutW` with a quarter-second timeout, so a hung player
does not hang the window.

Sources:

- The Winamp SDK header `wa_ipc.h` (mirror: https://git.txlyre.website/txlyre/winamp/raw/main/Src/Winamp/wa_ipc.h, read 2026-10-19) defines `IPC_ISPLAYING` 104, `IPC_GETOUTPUTTIME` 105 with its three modes, `WM_WA_IPC` as `WM_USER`, `WINAMP_BUTTON1` to `5` as 40044 to 40048, and the `FindWindow("Winamp v1.x")` convention. The header calls the button ids "the five main control buttons" without naming them; previous, play, pause, stop, next is the long-standing order and the play, pause and stop ones match WACUP.
- `IPC_GETPLAYLISTTITLE` (212) is marked "only usable from plug-ins (not external apps)": it returns a pointer inside the player's process. That is why the title comes from the caption.
- https://getwacup.com (read 2026-10-19; public preview build 25130, 30 September 2026) documents neither the window API nor a command line. It lists a plug-in SDK as coming "eventually".
- WACUP's command line is unofficial: its developer says in the WACUP forum (https://getwacup.com/community/index.php?topic=1389, read 2026-10-19) that command-line handling is "not being an officially supported feature". The panel does not use it. Launch starts `wacup.exe` with no arguments.
- No HTTP or JSON interface of WACUP was found in the site, its forum or a web search (2026-10-19). Unverified that none exists.

## Launch

Launch starts the first `wacup.exe` it finds, read only: the `App Paths\wacup.exe` registry key (both
hives, both views), then `Program Files\WACUP` and `Program Files (x86)\WACUP`. The button shows only
when one is found.

## Manual test

1. Open the desktop window with WACUP closed. The panel at the bottom of the sidebar says
   "WACUP not running" and shows Launch.
2. Press Launch. WACUP opens; within a second the panel shows its state.
3. Play a track in WACUP. The panel shows the title (a long one slides to show its end), the play
   button turns to pause, and the clock runs against the length.
4. Press pause and play in the panel; WACUP follows. Press next and previous with a few tracks in
   the playlist.
5. Pause in WACUP itself; within a second the panel's button turns to play.
6. Close WACUP. The panel goes back to "WACUP not running".
7. Fold the sidebar (`Ctrl+B`): the panel is not shown and no polling happens. Open it again and it
   resumes.

The same read and transport against a running WACUP, from the repo:

```
cd desktop
cargo test wacup_live -- --ignored --nocapture
```

The first test prints the state; the second pauses, resumes, stops and plays the loaded track.
WACUP must be running with a track loaded; to stay quiet, use a silent file.

## Not verified

- The clock: on the test machine `IPC_GETOUTPUTTIME` mode 0 stayed 0 while a silent WAV played, and
  WACUP's own title bar showed `[00:00/0:20]`, so the position was not seen advancing. The length
  was right (20000 ms).
- Previous and next, and a stream (length 0).
- 64-bit WACUP. The test build was 32-bit; the messages are the same across bitness.
