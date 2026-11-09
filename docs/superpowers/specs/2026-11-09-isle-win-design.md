# Isle for Windows — Design

Date: 2026-11-09. Status: draft v4 (full feature scope minus Prayer, staged; paged panel; modules; local repo), awaiting approval.

## Intent and why

Port the idea of Isle (https://github.com/zickrian/isle, GPL-3.0, a notch-style
island for GNOME) to Windows as a **new, from-scratch Rust program**. The user
wants a notch at the top centre of the screen that is as light as possible and
fully native, shipped as an installer. They also want to **learn native Windows
programming** by building it, so clarity of code matters as much as the result.
Isle's GNOME code (JS extension + D-Bus/MPRIS Rust daemon) cannot run on Windows,
so nothing is run as-is; only platform-neutral logic is adapted.

## Goals

- A borderless, transparent, always-on-top black notch, centred at the top of the
  primary monitor, hidden from the taskbar and Alt+Tab.
- Rests as a slim pill (96 x 26 px, about the height of a taskbar clock) showing the time (and what is going on). While the timer runs, the clock is replaced by the countdown (mm:ss, green). Hover opens it
  with spring animation into a panel; moving the pointer away closes it.
- Very low footprint: single exe of a few MB, idle RAM in the low tens of MB,
  ~0% CPU at rest (no redraw loop when nothing animates).
- Correct on Windows: per-monitor DPI, multi-monitor, ClearType-quality text.
- Delivered as one NSIS installer (`.exe`), built by GitHub Actions.

## Non-goals

- No WebView, no GUI framework (no Tauri, Slint, egui, iced).
- No Linux/macOS support.
- No embedded scripting language (Lua/Rhai) for now; rejected as over-built.
- Not a line-by-line translation of Isle; not every Isle feature in v1.
- No voice assistant, AI integrations or phone/QR transfer. No Prayer times.

## Anti-goals (failure even if it "works")

- Idle CPU/GPU use from a constant render loop.
- Stealing focus or blocking clicks on windows beneath the notch when closed.
- Code so tangled that it can't be read as a learning project.

## Constraints

- Language: Rust (stable, edition 2024). Windows APIs via the `windows` crate.
- Isle is GPL-3.0-or-later, so this project is GPL-3.0-or-later too; any code
  adapted from Isle keeps its attribution.
- Developed in WSL (editing, `cargo check --target x86_64-pc-windows-gnu`
  where possible); **running and testing happens on the user's Windows** side.
  CI (GitHub Actions, `windows-latest`) builds the release and installer.

## Approaches considered

1. **A. Win32 + Direct2D/DirectWrite, drawn by hand** — lightest and most
   native; every widget is ours; most work. **Chosen by the user**, with a
   staged rollout so text-input features come last.
2. B. Slint (software/Skia) — faster to build, ~15–25 MB more RAM, UI limited
   to what Slint offers. Rejected: user wants max-lite and to learn native.
3. C. egui/iced — GPU-based, heavy, flaky transparent windows on Windows.
   Rejected.

## Design

### Workspace layout
```
isle-win/
  crates/
    isle-logic/   # pure Rust, no Windows deps, unit-testable on Linux:
                  # pomodoro timer state machine, LRC lyrics parser,
                  # spring animation math, layout/hit-test maths
    isle-win/     # the exe: window, rendering, Windows integrations
  installer/      # NSIS script
  .github/workflows/
```
Logic is adapted from Isle's `isle-core` (`pomodoro.rs`, `lrc.rs`) where it fits;
everything platform-neutral lives in `isle-logic` so it can be tested in WSL.

### Window
- One top-level `WS_POPUP` window with `WS_EX_LAYERED`/composition alpha,
  `WS_EX_TOOLWINDOW` (no taskbar/Alt+Tab), `WS_EX_TOPMOST`, `WS_EX_NOACTIVATE`
  (never takes focus on hover).
- Rendered with **DirectComposition + Direct2D** (swap chain with premultiplied
  alpha) for smooth rounded transparent edges; text via DirectWrite.
- Closed state: the window is only as big as the pill, so it never covers other
  windows. It grows when open. Hover is detected with `TrackMouseEvent`
  (`WM_MOUSEMOVE`/`WM_MOUSELEAVE`), with a short dwell delay so passing over it
  doesn't open it (as in Isle).
- Frames are drawn only while an animation runs or content changes
  (timer tick once per second for the clock); otherwise the message loop blocks.

### Animation
- Critically/under-damped spring on width, height and corner radius, stepped
  by a vsync-aligned timer (`DwmFlush`/`WM_TIMER` at ~60 Hz, only while moving).

### Feature modules (extensibility, the user's own features)
Every feature, built-in or the user's own, is a **module** implementing one small
Rust trait (name is a builder call): `pill_slot()` (what it shows in the closed
pill, if anything, and its priority), `draw_panel()` (its page in the open panel),
`on_event()` (mouse/keys), `tick()` (when it next needs a redraw, so idle stays
at 0% CPU). Clock, Music, Timer and Lyrics are modules like any other; adding a
feature = one new file + one line to register it, then `cargo build`. Reference
example of a user module to keep in mind: **Claude token usage** (data source to
be investigated later; local Claude Code logs may or may not allow computing
remaining quota).

### Features (chosen by the user: everything on the Nusa list except Prayer)
Scope: Music, Lyrics, Focus timer, To-do, Charging, Headphones, Clipboard, Shelf,
Water, Notifications, Widgets, idle "Resting" state. **Not included: Prayer times.**
Every one is a module (see above), built in stages; each stage ships an installer
and the user can reorder stages 8-10.

Panel model (changed after the Nusa reference): the open panel is **paged**, not
tabbed: left/right arrows and a row of dots at the bottom; each page is a module's
`draw_panel()`. Open size 540 x 184 px (660 wide while lyrics are shown); closed
pill 96 x 26 px (140 wide with music or charging). Header shows the page title and
date + time. Mockup: https://claude.ai/artifact/Y17oTeKAB8FA7as2WS6aXD (covers
stages 1-4 and To-do; later pages get mocked up before their stage).

Stages:
1. **Shell + Charging:** window, hover open/close with spring, clock, tray icon
   (Quit, "Start with Windows"), paging. Charging peek: when the charger is plugged
   in, the closed pill shows a bolt + battery % for ~3 s (`GetSystemPowerStatus`,
   power-broadcast message).
2. **Music:** Windows media sessions (SMTC): title, artist, cover, play/pause/
   next/prev, progress. Closed pill shows a mini cover + equaliser on the left.
3. **Focus timer:** Pomodoro (work/break lengths, ring, Start/Reset). While it
   runs, the closed pill's clock is replaced by the countdown (mm:ss, green).
4. **Lyrics:** synced lyrics from LRCLIB (HTTP on a worker thread). A Lyrics
   button on the Music page widens the notch and shows 3 lines, current one large;
   click a line to seek.
5. **Headphones:** a short peek in the closed pill when headphones/earbuds connect
   or disconnect (device name, battery % where Windows exposes it), via the audio
   endpoint / Bluetooth device watchers.
6. **Text input component, then To-do:** hand-built single-line text box (caret,
   selection, clipboard paste, IME composition) and a To-do page: "done/total today"
   counter, Add, Edit (delete), tick to complete, strikethrough, persisted to
   `%APPDATA%\IsleWin\todo.json`. Due-time reminders pop in the closed pill.
7. **Cards from JSON (user's own features):** a generic page that shows cards
   (title, value, progress, colour) read from `%APPDATA%\IsleWin\cards\`, watched
   for changes, so any script can feed it without rebuilding (e.g. a Claude token
   usage script; data source to be investigated then).
8. **Shelf:** drop files on the notch (OLE drop target; hovering with a drag opens
   it), thumbnails, drag them back out into any app (OLE drop source) or click to
   copy. Files are referenced, not copied, unless the user chooses otherwise.
9. **Clipboard history:** text, links, images, files via the clipboard-format
   listener; search (uses the text component), pin, click to re-copy, drag out.
   Privacy: local only, size/age limits, skips content apps mark as excluded from
   history (password managers), one-click pause and clear.
10. **Water, Notifications, Widgets, Resting state:** water goal with reminders
    and chart (+ button, no text); "Resting" eyes animation when idle; Widgets
    page (two small widgets at a time: world clock, battery, countdown, ...);
    Windows notifications shown as a short pill. **Risk:** reading other apps'
    notifications through `UserNotificationListener` normally needs package
    identity and a user permission, which an unpackaged installer app may not get;
    to be spiked first, and may end up limited or dropped.

Optional extras: system overlays (volume, brightness, Caps Lock, keyboard language).

Reference for ideas only: Nusa (https://rama2555.github.io/nusa/), a separate
product; no assets or code are copied.

### Threading
UI thread owns the window and all drawing. Worker threads (SMTC events, HTTP)
send messages to it with `PostMessage` and a small channel. No async runtime.

### Settings
A small `settings.toml` in `%APPDATA%\IsleWin`, read at start (timer lengths,
autostart, which monitor). No settings UI in v1.

### Packaging
`cargo build --release` with `opt-level="z"`, `lto`, `panic="abort"`, `strip`.
NSIS script installs to `%LOCALAPPDATA%\Programs\IsleWin` (per-user, no admin),
adds Start Menu shortcut and uninstaller. GitHub Actions builds on tag push and
attaches the installer to the release.

### Testing
- `isle-logic`: ordinary `cargo test` (run in WSL and CI).
- Window/rendering: manual checklist on Windows (hover, DPI 100/150%, two
  monitors, fullscreen game/video behaviour), plus a CI build that must compile.

## User decisions (confirmed)
- Approach A (Win32 + Direct2D, no framework), modules in code now, JSON cards
  later (stage 6); local repo first.
- Features: all of Nusa's list except Prayer (Music, Lyrics, Focus timer, To-do,
  Charging, Headphones, Clipboard, Shelf, Water, Notifications, Widgets, Resting).

## Left to the builder
Exact spring constants and colours (start from Isle's look: pure black pill,
white text), font choice (Segoe UI Variable), icon art, exact crate versions,
tray icon implementation details, NSIS script details.

## Calls I made (not the user's decisions)
- MVP scope = shell, music, timer, lyrics.
- NSIS per-user installer; `windows` crate; `ureq` for HTTP; TOML settings.
- DirectComposition over plain layered window (`UpdateLayeredWindow`) for
  smoother alpha; fall back to layered window if DComp proves troublesome.
- Project lives in `~/isle-win` (WSL) as a **local git repo** (user's decision:
  no GitHub repo yet). CI workflow is written but only runs once a remote exists.

## Answers to the builder check

- **Panel layout (my call, following Isle):** the open panel has a row of page
  tabs (Music, Timer; more later) and shows one page at a time. The closed pill
  shows the clock plus, by priority: playing music (mini cover) on one side and a
  running timer (small ring/time left) on the other, as Isle does.
- **Fullscreen (my call):** hide the notch while a fullscreen app/game is in the
  foreground, using `SHQueryUserNotificationState`, checked on the existing
  once-per-second clock tick (no extra polling source, no idle cost).
- **Testing loop:** WSL interop works on this machine (Windows 11, build 26300),
  so builds can be launched on Windows directly from WSL for quick checks;
  the user confirms visual behaviour. Windows target for local builds:
  `x86_64-pc-windows-gnu` (mingw) or `cargo-xwin` (MSVC), chosen when setting up.
- **Releases (my call):** CI + installer are set up with stage 1 (shell), so every
  stage ships an installer; first tagged release `v0.1.0` after stage 1.
- **Minimum Windows (my call):** Windows 10 1903+; font Segoe UI Variable with
  fallback to Segoe UI.
- **GitHub repo:** user decided: local only for now. The installer is built locally
  (NSIS via mingw/WSL or on Windows) until a remote is added.
