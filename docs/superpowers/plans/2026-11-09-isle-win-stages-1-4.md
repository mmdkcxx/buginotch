# Isle for Windows, Stages 1-4 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A native Windows notch app (shell + Charging, Music, Focus timer, Lyrics) shipped as an NSIS installer, built so later features are one-file modules.

**Architecture:** Two crates. `isle-logic` is pure Rust with no Windows dependencies (spring maths, layout, Pomodoro, LRC parsing, settings, module trait, a `Canvas` drawing trait) and is unit-tested in WSL. `isle-win` is the exe: a Win32 window rendered with DirectComposition + Direct2D/DirectWrite behind the `Canvas` trait, plus Windows integrations (SMTC, power, tray). Frames are drawn only while something animates or changes.

**Tech Stack:** Rust stable (edition 2024), `windows` crate, `serde` + `toml`, `ureq` (LRCLIB), NSIS, GitHub Actions workflow (inert until a remote exists).

**Spec:** `docs/superpowers/specs/2026-11-09-isle-win-design.md`. Later stages (5-10: Headphones, text input + To-do, JSON cards, Shelf, Clipboard, Water/Notifications/Widgets) get their own plans.

## Global Constraints

- Rust stable, edition 2024; Windows APIs via the `windows` crate; no WebView, no GUI framework (no Tauri, Slint, egui, iced).
- License GPL-3.0-or-later. Code adapted from Isle (`pomodoro.rs`, `lrc.rs`, Copyright (C) 2026 zickrian) keeps its SPDX header and attribution.
- Minimum Windows 10 1903; font "Segoe UI Variable" with fallback "Segoe UI".
- Closed pill 96 x 26 px (140 wide with music or charging); open panel 540 x 184 px (660 wide with lyrics); bottom corner radius 13 closed, 30 open (DIPs, scaled by monitor DPI).
- Hover opens after 150 ms dwell, closes 220 ms after the pointer leaves.
- Window styles: `WS_POPUP`, `WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`; never takes focus; window is only as large as the current notch so it never covers other windows.
- Idle target: ~0% CPU, no render loop; a 1 s timer only drives the clock/timer/fullscreen check.
- Hidden while a fullscreen app is foreground (`SHQueryUserNotificationState`, checked on the 1 s tick).
- Settings in `%APPDATA%\IsleWin\settings.toml`; lyrics cache in `%APPDATA%\IsleWin\lyrics\`.
- Installer: NSIS, per-user, installs to `%LOCALAPPDATA%\Programs\IsleWin`, Start Menu shortcut, uninstaller, no admin.
- Release profile: `opt-level = "z"`, `lto = true`, `panic = "abort"`, `strip = true`.
- Dev loop: edit in WSL; `cargo test -p isle-logic` in WSL; `cargo build --release --target x86_64-pc-windows-gnu -p isle-win` for the exe; run it on Windows from WSL by copying to `/mnt/c/Users/Public/isle-win/` and `powershell.exe -NoProfile -Command Start-Process 'C:\Users\Public\isle-win\isle-win.exe'`.
- UI copy is Indonesian where the app speaks to the user ("Lirik", "Mulai", "Jeda", "Tidak ada lirik").

## Review Focus

1. Media player closes or no session exists: Music page shows an empty state ("Tidak ada musik"), the pill's music slot disappears, nothing crashes. (Task 11)
2. Monitor unplugged, resolution/DPI change, or taskbar/primary monitor change: notch re-centres on the primary monitor and rescales. (Task 6)
3. LRCLIB offline, 404 or slow: Lyrics button says "Tidak ada lirik", UI never freezes (network on a worker thread, 5 s timeout). (Task 13)
4. Very long titles and non-Latin text (CJK, Arabic, emoji): ellipsised to fit, DirectWrite font fallback, never overflows the panel. (Task 11)
5. Sleep/resume or system clock change with a running timer: remaining time comes from wall-clock epoch ms, not accumulated ticks, so it is right after resume. (Task 12)

---

### Task 1: Workspace, toolchain and adapted Isle logic

**Files:**
- Create: `Cargo.toml` (workspace), `crates/isle-logic/Cargo.toml`, `crates/isle-logic/src/lib.rs`, `crates/isle-logic/src/pomodoro.rs`, `crates/isle-logic/src/lrc.rs`, `LICENSE` (GPL-3.0 text), `NOTICE.md`, `.gitignore`
- Test: tests live inside the two copied modules (`#[cfg(test)]`)

**Interfaces:**
- Produces: `isle_logic::pomodoro::{Phase, PomodoroConfig, Pomodoro, PomodoroView, Transition, TimerState}` and `isle_logic::lrc::{LyricLine, parse, plain}`, unchanged from Isle's `isle-core` (`Pomodoro::{new, start, pause, stop, skip, tick(now_ms), view(now_ms), remaining(now_ms)}`; `LyricLine { time_ms: i64, text: String }`).

- [ ] **Step 1: Install build tools (the user runs this; needs sudo)**

Ask the user to run `! sudo apt install -y mingw-w64 nsis`. Verify: `x86_64-w64-mingw32-gcc --version` and `makensis -VERSION` both print a version. The `x86_64-pc-windows-gnu` rust target is already installed.

- [ ] **Step 2: Create the workspace** with members `crates/isle-logic` (and `crates/isle-win` later), `resolver = "3"`, `[workspace.package]` edition 2024, license `GPL-3.0-or-later`, and the release profile from Global Constraints.

- [ ] **Step 3: Copy `pomodoro.rs` and `lrc.rs`** from `/tmp/claude-1000/-home-doh-rog/d8d219f8-690a-4502-9b21-53d825dd7ec7/scratchpad/isle-src/crates/isle-core/src/` into `crates/isle-logic/src/`, keep their SPDX headers and in-file tests, `pub mod` both in `lib.rs`. `NOTICE.md` states they are adapted from https://github.com/zickrian/isle (GPL-3.0-or-later).

- [ ] **Step 4: Run `cargo test -p isle-logic`**
Expected: all copied tests PASS.

- [ ] **Step 5: Commit** `chore: workspace and adapted pomodoro/lrc from Isle`

---

### Task 2: Spring animation

**Files:**
- Create: `crates/isle-logic/src/spring.rs`
- Test: same file, `#[cfg(test)]`

**Interfaces:**
- Produces: `pub struct Spring { pub value: f32, pub velocity: f32, pub target: f32, stiffness: f32, damping: f32 }`; `Spring::new(value: f32, stiffness: f32, damping: f32) -> Spring`; `Spring::step(&mut self, dt: f32)` (semi-implicit Euler, substeps of at most 1/240 s); `Spring::is_settled(&self) -> bool` (|value-target| < 0.05 and |velocity| < 0.05); `Spring::snap(&mut self)` sets value = target, velocity = 0.

- [ ] **Step 1: Write failing tests** `converges_to_target` (stiffness 300, damping 26, target 100 from 0, step 1/60 for 120 frames, asserts `is_settled()` and `(value-100).abs() < 0.05`), `overshoots_when_underdamped` (damping 8, max value seen > 100), `does_not_overshoot_when_critically_damped` (damping = 2*sqrt(stiffness), max value <= 100.01), `large_dt_stays_finite` (dt = 0.5 gives finite value).
- [ ] **Step 2: Run** `cargo test -p isle-logic spring` Expected: FAIL (not defined).
- [ ] **Step 3: Implement** as above in `spring.rs`, `pub mod spring;` in `lib.rs`.
- [ ] **Step 4: Run** the same command. Expected: PASS.
- [ ] **Step 5: Commit** `feat(logic): spring animation`

---

### Task 3: Layout and hover state machine

**Files:**
- Create: `crates/isle-logic/src/layout.rs`
- Test: same file

**Interfaces:**
- Produces:
  - `pub struct Size { pub w: f32, pub h: f32 }`, `pub struct NotchTarget { pub size: Size, pub radius: f32 }`
  - `pub enum PillKind { Idle, Wide }`
  - `pub fn target(open: bool, wide_pill: bool, lyrics: bool) -> NotchTarget` returning exactly the Global Constraints sizes (96x26 r13; 140x26 r13; 540x184 r30; 660x184 r30 when open && lyrics).
  - `pub fn notch_x(monitor_width_px: i32, notch_width_px: i32) -> i32` (centred).
  - `pub struct Hover { .. }` with `Hover::new()`, `Hover::pointer_inside(&mut self, inside: bool, now_ms: i64)`, `Hover::update(&mut self, now_ms: i64) -> bool` returning whether the notch should be open: opens when inside continuously for 150 ms, closes when outside continuously for 220 ms.

- [ ] **Step 1: Write failing tests** `target_sizes_match_spec`, `notch_is_centred` (1920, 540 -> 690), `hover_opens_after_dwell` (inside at t=0: update(100) false, update(150) true), `passing_over_does_not_open` (inside t=0, outside t=100, update(300) false), `closes_after_delay` (open, outside at t=1000: update(1100) true, update(1220) false), `reentering_cancels_close`.
- [ ] **Step 2: Run** `cargo test -p isle-logic layout` Expected: FAIL.
- [ ] **Step 3: Implement** in `layout.rs`; `pub mod layout;`.
- [ ] **Step 4: Run** Expected: PASS.
- [ ] **Step 5: Commit** `feat(logic): notch layout and hover dwell`

---

### Task 4: Settings

**Files:**
- Create: `crates/isle-logic/src/settings.rs`
- Test: same file

**Interfaces:**
- Produces: `#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)] #[serde(default)] pub struct Settings { pub autostart: bool, pub monitor: u32, pub pill_width: f32, pub pill_height: f32, pub pomodoro: PomodoroConfig }` (defaults: false, 0, 96.0, 26.0, default config); `Settings::from_toml(&str) -> Settings` (invalid input returns defaults, never panics); `Settings::to_toml(&self) -> String`. `target()` from Task 3 is NOT changed; pill sizes from settings are applied by `isle-win` (Task 7).

- [ ] **Step 1: Write failing tests** `empty_file_gives_defaults`, `partial_file_keeps_other_defaults` (`pill_width = 80.0` only), `garbage_gives_defaults`, `round_trips`.
- [ ] **Step 2: Run** `cargo test -p isle-logic settings` Expected: FAIL.
- [ ] **Step 3: Implement** with `toml` + `serde`; add both deps to `isle-logic`.
- [ ] **Step 4: Run** Expected: PASS.
- [ ] **Step 5: Commit** `feat(logic): settings`

---

### Task 5: Canvas trait, module trait, page registry

**Files:**
- Create: `crates/isle-logic/src/canvas.rs`, `crates/isle-logic/src/module.rs`, `crates/isle-logic/src/testing.rs` (a `RecordingCanvas` fake, `#[cfg(any(test, feature = "testing"))]`)
- Test: `crates/isle-logic/src/module.rs`

**Interfaces:**
- Produces:
  - `pub struct Rect { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }`, `pub struct Color(pub u8, pub u8, pub u8, pub u8)`
  - `pub trait Canvas { fn fill_round_rect(&mut self, r: Rect, radius: f32, c: Color); fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, c: Color); fn stroke_arc(&mut self, cx: f32, cy: f32, r: f32, start_deg: f32, sweep_deg: f32, width: f32, c: Color); fn draw_text(&mut self, text: &str, r: Rect, size: f32, weight: u16, c: Color, align: Align); fn measure_text(&mut self, text: &str, size: f32, weight: u16) -> f32; fn draw_image(&mut self, id: ImageId, r: Rect, radius: f32); fn push_clip(&mut self, r: Rect, radius: f32); fn pop_clip(&mut self); }` with `pub enum Align { Left, Center, Right }`, `pub struct ImageId(pub u32)`.
  - `pub fn fit_text(c: &mut dyn Canvas, text: &str, size: f32, weight: u16, max_w: f32) -> String` (returns text, or the longest prefix + "…" that fits; never panics on empty or multi-byte text).
  - `pub enum Event { PointerMove(f32, f32), PointerDown(f32, f32), PointerWheel(f32), Key(Key) }`
  - `pub trait Module { fn id(&self) -> &'static str; fn title(&self) -> &'static str; fn pill_left(&self) -> Option<PillItem>; fn pill_center(&self) -> Option<String> { None } fn pill_priority(&self) -> u8 { 0 } fn draw_panel(&mut self, c: &mut dyn Canvas, area: Rect, now_ms: i64); fn on_event(&mut self, e: &Event, area: Rect, now_ms: i64) -> Reaction; fn next_wake_ms(&self, now_ms: i64) -> Option<i64>; fn wants_wide_panel(&self) -> bool { false } }`; `pub enum PillItem { MiniCover(ImageId), Equaliser { active: bool } }`; `pub enum Reaction { None, Redraw, Peek(PeekRequest) }`.
  - `pub struct Registry { .. }`: `add(Box<dyn Module>)`, `pages() -> &mut [Box<dyn Module>]`, `current: usize`, `go(delta: i32)` (wraps), `pill_center(now) -> String` (highest-priority module's text, else the clock text passed in), `earliest_wake(now) -> Option<i64>`.

- [ ] **Step 1: Write failing tests** with `RecordingCanvas` (measures 7 px per char): `fit_text_returns_whole_when_fits`, `fit_text_truncates_with_ellipsis` (width of result <= max), `fit_text_handles_cjk_and_emoji` ("日本語のタイトル🎵" never splits a char), `fit_text_empty`, `registry_go_wraps` (3 modules, go(-1) from 0 -> 2), `registry_priority_picks_highest_center_text`, `earliest_wake_is_minimum`.
- [ ] **Step 2: Run** `cargo test -p isle-logic` Expected: FAIL.
- [ ] **Step 3: Implement** the traits, `fit_text` (binary search over `char_indices`), `Registry`, and `RecordingCanvas`.
- [ ] **Step 4: Run** Expected: PASS.
- [ ] **Step 5: Commit** `feat(logic): canvas and module traits, registry`

---

### Task 6: `isle-win` skeleton, Win32 window, DPI, compile gate

**Files:**
- Create: `crates/isle-win/Cargo.toml`, `crates/isle-win/build.rs` (embeds `isle-win.manifest`), `crates/isle-win/isle-win.manifest` (PerMonitorV2 DPI, Windows 10 compat, UTF-8), `crates/isle-win/src/main.rs`, `crates/isle-win/src/window.rs`
- Modify: workspace `Cargo.toml` members

**Interfaces:**
- Produces: `window::Window::create(app: Rc<RefCell<App>>) -> Result<Window>`; `Window::set_size_and_pos(&self, w_px: i32, h_px: i32)` (centred at the top of the primary monitor, `SWP_NOACTIVATE`); `Window::hwnd() -> HWND`; custom messages `WM_APP_WAKE = WM_APP + 1`, `WM_APP_MEDIA = WM_APP + 2`, `WM_APP_LYRICS = WM_APP + 3`; `main()` runs `GetMessage` loop (blocks when idle). The `windows` features list: Win32_Foundation, Win32_Graphics_{Direct2D, Direct2D_Common, Direct3D, Direct3D11, DirectComposition, DirectWrite, Dxgi, Dxgi_Common, Gdi, Imaging}, Win32_System_{LibraryLoader, Power, Registry, Threading, Com}, Win32_UI_{WindowsAndMessaging, HiDpi, Shell, Input_KeyboardAndMouse}, Media_Control, Foundation, Storage_Streams.

- [ ] **Step 1:** Create the window with the styles from Global Constraints, 96 x 26 DIPs scaled by `GetDpiForWindow`, shown with `SW_SHOWNOACTIVATE`, centred with `layout::notch_x`.
- [ ] **Step 2:** Handle `WM_DPICHANGED`, `WM_DISPLAYCHANGE` and `WM_SETTINGCHANGE` by recomputing size/position on the primary monitor (Review Focus 2).
- [ ] **Step 3: Compile gate.** Run: `cargo build --release --target x86_64-pc-windows-gnu -p isle-win` Expected: finishes without errors.
- [ ] **Step 4: Manual check on Windows** (copy exe, `Start-Process`): a window exists at top centre, does not show in the taskbar or Alt+Tab, does not take focus when hovered or clicked, and can be killed from Task Manager. Report the result to the user.
- [ ] **Step 5: Commit** `feat(win): borderless top-most notch window`

---

### Task 7: Renderer (DirectComposition + Direct2D) behind `Canvas`

**Files:**
- Create: `crates/isle-win/src/renderer.rs`, `crates/isle-win/src/d2d_canvas.rs`
- Modify: `crates/isle-win/src/window.rs`

**Interfaces:**
- Consumes: `isle_logic::canvas::{Canvas, Rect, Color, Align, ImageId}` (Task 5).
- Produces: `Renderer::new(hwnd: HWND, w_px: u32, h_px: u32) -> Result<Renderer>`; `Renderer::resize(&mut self, w_px: u32, h_px: u32)`; `Renderer::frame<F: FnOnce(&mut dyn Canvas)>(&mut self, f: F) -> Result<()>` (BeginDraw, clear to transparent, closure, EndDraw, Present; recreate device on `D2DERR_RECREATE_TARGET`); `D2dCanvas: Canvas` implementing every method with DirectWrite (font "Segoe UI Variable Text" falling back to "Segoe UI"), `register_image(&mut self, rgba: &[u8], w: u32, h: u32) -> ImageId`.

- [ ] **Step 1:** Swap chain with `DXGI_ALPHA_MODE_PREMULTIPLIED` attached through a DComp visual; render target at the window's pixel size with DPI scaling.
- [ ] **Step 2:** In `window.rs` draw the black notch shape (`fill_round_rect` with only the bottom corners rounded: draw a rect for the top half plus a rounded rect, or a path) on `WM_PAINT`/`WM_APP_WAKE`; nothing draws unless requested.
- [ ] **Step 3: Compile gate** (same command as Task 6). Expected: no errors.
- [ ] **Step 4: Manual check:** a black pill with smooth rounded bottom corners over a bright window, no white box or fringe, ClearType-quality test text "Isle". Report to the user.
- [ ] **Step 5: Commit** `feat(win): composition renderer and Canvas implementation`

---

### Task 8: Hover, spring-driven open/close, on-demand frames

**Files:**
- Create: `crates/isle-win/src/app.rs`
- Modify: `crates/isle-win/src/window.rs`

**Interfaces:**
- Consumes: `Spring` (Task 2), `layout::{target, Hover, notch_x}` (Task 3), `Renderer` (Task 7).
- Produces: `App { registry: Registry, settings: Settings, springs: {w, h, radius}, hover: Hover, .. }`; `App::on_pointer(&mut self, inside: bool, x: f32, y: f32, now_ms: i64)`; `App::next_frame(&mut self, now_ms: i64) -> FrameRequest` where `FrameRequest { redraw: bool, next_wake_ms: Option<i64> }`. Animation is stepped by a one-shot `SetTimer` (16 ms) that is only armed while any spring is unsettled; a 1000 ms timer runs the clock tick.

- [ ] **Step 1:** Track the pointer with `TrackMouseEvent` (`TME_LEAVE`); while closed the window is exactly the pill size, while open or animating it is resized each frame with `SetWindowPos(SWP_NOACTIVATE | SWP_NOZORDER)` to the spring size (clamped to the larger of current and target while shrinking so content is not clipped).
- [ ] **Step 2:** Use stiffness 300 and damping 22 for width/height, 260/20 for radius (builder may tune by eye; these give a slight overshoot).
- [ ] **Step 3: Compile gate** Expected: no errors.
- [ ] **Step 4: Manual check:** hover opens with spring after ~150 ms, leaving closes after ~220 ms, passing over quickly does nothing, clicks on a window under the closed pill still reach that window, Task Manager shows ~0% CPU when idle. Report to the user.
- [ ] **Step 5: Commit** `feat(win): hover open/close with spring animation`

---

### Task 9: Clock module, paging, header, dots

**Files:**
- Create: `crates/isle-win/src/ui.rs` (header, dots, arrows), `crates/isle-win/src/modules/mod.rs`, `crates/isle-win/src/modules/clock.rs`

**Interfaces:**
- Consumes: `Module`, `Registry`, `Canvas` (Task 5).
- Produces: `ClockModule: Module` (id `"clock"`, no panel page; supplies the closed-pill centre text `HH:MM` from `local_time()`); `ui::draw_chrome(c: &mut dyn Canvas, reg: &Registry, area: Rect, date_text: &str, time_text: &str)`: page title left, date + time right, dots bottom (active dot 18 wide, others 5), arrows at the left/right edges; `ui::hit_chrome(x, y, area) -> Option<ChromeAction>` with `ChromeAction::{Prev, Next, Dot(usize)}`. Pure geometry lives in `isle-logic` as `chrome_hit(area: Rect, x: f32, y: f32, pages: usize) -> Option<ChromeAction>` and is unit-tested there.

- [ ] **Step 1: Write failing tests** in `isle-logic` for `chrome_hit`: `hit_next_arrow`, `hit_prev_arrow`, `hit_third_dot` (3 pages), `miss_returns_none`.
- [ ] **Step 2: Run** `cargo test -p isle-logic chrome` Expected: FAIL; then implement `chrome_hit` in `crates/isle-logic/src/chrome.rs` and run again, Expected: PASS.
- [ ] **Step 3:** Implement `ClockModule`, wire mouse click handling through `chrome_hit` (no keyboard focus is ever taken, so there are no key shortcuts), draw the closed pill (centre text, 12 px, weight 600, `tabular` digits via a fixed-width digit font feature or per-digit placement) and the open chrome.
- [ ] **Step 4: Compile gate** Expected: no errors.
- [ ] **Step 5: Manual check:** closed pill shows the correct local time (updates each minute), open panel shows title/date/time header and dots; arrows and dots change pages (page bodies are empty until later tasks). Report to the user.
- [ ] **Step 6: Commit** `feat: clock, page chrome and paging`

---

### Task 10: Tray, autostart, single instance, fullscreen hide

**Files:**
- Create: `crates/isle-win/src/tray.rs`, `crates/isle-win/src/system.rs`

**Interfaces:**
- Produces: `tray::Tray::add(hwnd) -> Result<Tray>` (Shell_NotifyIcon, menu: "Mulai dengan Windows" (checked), "Keluar"); `system::set_autostart(enabled: bool) -> Result<()>` (writes/removes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\IsleWin` = quoted exe path); `system::is_autostart() -> bool`; `system::acquire_single_instance() -> bool` (named mutex `Local\IsleWin`; second launch exits silently); `system::fullscreen_app_active() -> bool` (`SHQueryUserNotificationState` returns `QUNS_BUSY` or `QUNS_RUNNING_D3D_FULL_SCREEN` or `QUNS_PRESENTATION_MODE`).

- [ ] **Step 1:** Implement the four items; on the 1 s tick, `ShowWindow(SW_HIDE)` while `fullscreen_app_active()` and `SW_SHOWNOACTIVATE` when it ends; load/save `settings.toml` through `Settings` (`autostart` mirrors the registry).
- [ ] **Step 2: Compile gate** Expected: no errors.
- [ ] **Step 3: Manual check:** tray icon appears; "Keluar" quits; launching twice leaves one process; toggling autostart creates/removes the Run key (`reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run`); playing a fullscreen video hides the notch and it returns afterwards. Report to the user.
- [ ] **Step 4: Commit** `feat(win): tray, autostart, single instance, fullscreen hide`

---

### Task 11: Charging peek (Stage 1 finish) and Music (Stage 2)

**Files:**
- Create: `crates/isle-logic/src/peek.rs`, `crates/isle-logic/src/media.rs`, `crates/isle-win/src/power.rs`, `crates/isle-win/src/media_win.rs`, `crates/isle-win/src/modules/charging.rs`, `crates/isle-win/src/modules/music.rs`

**Interfaces:**
- Produces (logic):
  - `pub struct Peek { .. }`: `Peek::show(&mut self, text: String, now_ms: i64, duration_ms: i64)`, `Peek::current(&self, now_ms: i64) -> Option<&str>`, `Peek::next_wake_ms(&self) -> Option<i64>`.
  - `pub struct PowerState { pub on_ac: bool, pub percent: Option<u8> }` and `pub fn charge_event(prev: Option<PowerState>, now: PowerState) -> Option<String>` returning `Some("⚡ 82%")` only when `on_ac` goes false -> true (percent unknown shows "⚡").
  - `pub struct MediaInfo { pub title: String, pub artist: String, pub album: String, pub app: String, pub playing: bool, pub position_ms: i64, pub duration_ms: i64, pub updated_at_ms: i64, pub cover: Option<Vec<u8>> }` with `MediaInfo::position_at(&self, now_ms: i64) -> i64` (interpolates while `playing`, clamped to `duration_ms`; paused returns `position_ms`).
- Produces (win): `power::read() -> PowerState` (`GetSystemPowerStatus`); `media_win::MediaWatcher::spawn(hwnd: HWND) -> MediaWatcher` (worker thread owning the SMTC `GlobalSystemMediaTransportControlsSessionManager`; posts `WM_APP_MEDIA` on session/media/playback/timeline change; `latest() -> Option<MediaInfo>`; commands `play_pause()`, `next()`, `previous()`, `seek(ms: i64)`); `ChargingModule`, `MusicModule` implementing `Module`.

- [ ] **Step 1: Write failing tests** in `isle-logic`: `charge_event_fires_on_plug_in`, `charge_event_silent_when_already_on_ac`, `charge_event_silent_on_unplug`, `peek_expires_after_duration` (show at 0 for 3200 ms: current(3199) Some, current(3200) None), `position_interpolates_while_playing` (position 10 000 at t=0, playing, position_at(2 500) = 12 500), `position_frozen_when_paused`, `position_clamped_to_duration`.
- [ ] **Step 2: Run** `cargo test -p isle-logic peek media` Expected: FAIL; implement; run again, Expected: PASS.
- [ ] **Step 3:** `ChargingModule` polls `power::read()` on the 1 s tick and shows the peek (centre text green, pill widens to the Wide size) for 3200 ms.
- [ ] **Step 4:** `MusicModule`: page layout from the mockup (96 px cover with 14 px radius, title 14/650 and artist 12 on the left column, scrub bar with elapsed/total, prev / play-pause / next); the pill shows the mini cover and a 3-bar equaliser (animates only while playing and a 120 ms wake is requested only then). Cover bytes are decoded with WIC into an RGBA buffer and registered on the canvas. Titles/artists pass through `fit_text` (Review Focus 4). With no session the page shows "Tidak ada musik" and `pill_left()` is `None` (Review Focus 1).
- [ ] **Step 5: Compile gate** Expected: no errors.
- [ ] **Step 6: Manual check on Windows:** play something in Spotify/YouTube; title, artist, cover, progress, play/pause/next/prev work; close the player: empty state, no crash; a title like `日本語のタイトル 🎵 very long ...` is ellipsised; plug/unplug a charger (laptop) shows the peek once. Report to the user.
- [ ] **Step 7: Commit** `feat: charging peek and SMTC music`

---

### Task 12: Focus timer (Stage 3)

**Files:**
- Create: `crates/isle-win/src/modules/timer.rs`
- Modify: `crates/isle-logic/src/lib.rs` (add `pub fn now_ms() -> i64` helper only if not present), `crates/isle-win/src/modules/mod.rs`

**Interfaces:**
- Consumes: `Pomodoro`, `PomodoroConfig`, `Phase` (Task 1), `Settings.pomodoro` (Task 4).
- Produces: `TimerModule: Module` with `pill_center() -> Some("mm:ss")` while running (priority 10, drawn green), page with a progress ring (112 px, stroke 8), remaining time (25/650), Work and Break steppers (work in steps of 5 min, 5-90; break 1-30), Start/Pause and Reset buttons; phase end calls `Reaction::Peek` with "Waktunya istirahat" / "Waktunya fokus". State (`TimerState`) is saved to `%APPDATA%\IsleWin\timer.json` on every change and restored on start. Pure helper in logic: `pub fn format_mmss(ms: i64) -> String` (rounds up, `"24:59"`, `"00:00"`, never negative).

- [ ] **Step 1: Write failing tests** `format_mmss_rounds_up` (1 499 ms -> "00:02", 1 000 -> "00:01", 0 -> "00:00", -5 -> "00:00", 1 500 000 -> "25:00"), `timer_survives_resume` (start at t=0 for 25 min, `view(now = 600_000)` has `remaining_ms = 900_000` regardless of how many `tick` calls happened, Review Focus 5), `state_round_trips_through_json`.
- [ ] **Step 2: Run** `cargo test -p isle-logic timer` Expected: FAIL; implement `format_mmss` (in `crates/isle-logic/src/timefmt.rs`) and the persistence helper; run again, Expected: PASS.
- [ ] **Step 3:** Implement `TimerModule`; request a wake at the next whole second only while running.
- [ ] **Step 4: Compile gate** Expected: no errors.
- [ ] **Step 5: Manual check:** Start: the closed pill's clock becomes a green countdown; Pause/Reset restore the clock; steppers change lengths; quit and relaunch mid-run: remaining time is still correct; put the PC to sleep for a minute: remaining time reflects the elapsed minute. Report to the user.
- [ ] **Step 6: Commit** `feat: focus timer`

---

### Task 13: Lyrics (Stage 4)

**Files:**
- Create: `crates/isle-logic/src/lyrics.rs`, `crates/isle-win/src/lrclib.rs`
- Modify: `crates/isle-win/src/modules/music.rs`, `crates/isle-logic/src/layout.rs` only if the lyrics width needs a new case (it does not; `target(open, wide, lyrics)` already exists)

**Interfaces:**
- Consumes: `lrc::{parse, plain, LyricLine}` (Task 1), `MediaInfo` (Task 11), `layout::target` (Task 3).
- Produces (logic): `pub fn current_index(lines: &[LyricLine], position_ms: i64) -> Option<usize>` (last line whose `time_ms <= position_ms`; `None` before the first line or for empty input); `pub fn window(lines: &[LyricLine], index: Option<usize>) -> [Option<usize>; 3]` (previous, current, next); `pub fn cache_key(title: &str, artist: &str, duration_ms: i64) -> String` (lowercase, trims, replaces characters illegal in Windows file names with `_`, stable). Produces (win): `lrclib::fetch(title: &str, artist: &str, album: &str, duration_s: u32) -> Result<Option<String>>` (GET `https://lrclib.net/api/get?track_name=&artist_name=&album_name=&duration=`, synced lyrics field, 5 s timeout, `Ok(None)` on 404, runs on a worker thread and posts `WM_APP_LYRICS`); cache read/write in `%APPDATA%\IsleWin\lyrics\<cache_key>.lrc` (a cached miss is stored as an empty file for 24 h).

- [ ] **Step 1: Write failing tests** `index_before_first_line_is_none`, `index_at_exact_timestamp`, `index_after_last_line`, `index_empty_lines_is_none`, `window_at_start_has_no_previous`, `window_at_end_has_no_next`, `cache_key_strips_illegal_chars` (`"AC/DC: Back?"` has none of `\/:*?"<>|`), `cache_key_is_stable_case_insensitive`.
- [ ] **Step 2: Run** `cargo test -p isle-logic lyrics` Expected: FAIL; implement; run again, Expected: PASS.
- [ ] **Step 3:** Add the "Lirik" toggle button to the Music page (`wants_wide_panel()` true while on, which widens the panel to 660 px through `target(.., lyrics = true)`); lyric column shows three lines from `window(...)`, current line 16 px bold white, others 13 px dim; clicking a line calls `MediaWatcher::seek(line.time_ms)`.
- [ ] **Step 4:** On track change start `lrclib::fetch` on a worker thread (cache first). While loading the column says "Mencari lirik…"; on `Ok(None)`, a network error or timeout it says "Tidak ada lirik" and the UI stays responsive (Review Focus 3). Plain (unsynced) lyrics are shown without highlighting via `lrc::plain`.
- [ ] **Step 5: Compile gate** Expected: no errors.
- [ ] **Step 6: Manual check:** play a well-known song: lyrics appear and the current line follows playback; click a line to seek; switch off Wi-Fi and change song: "Tidak ada lirik" appears and hovering/animation stays smooth; the second play of the same song loads instantly from cache. Report to the user.
- [ ] **Step 7: Commit** `feat: synced lyrics from LRCLIB`

---

### Task 14: Installer, local release script and CI workflow

**Files:**
- Create: `installer/isle-win.nsi`, `scripts/build-installer.sh`, `.github/workflows/release.yml`, `README.md`

**Interfaces:**
- Consumes: the release exe `target/x86_64-pc-windows-gnu/release/isle-win.exe`.
- Produces: `dist/IsleWin-Setup-<version>.exe`; `scripts/build-installer.sh` builds the release exe, reads the version from `crates/isle-win/Cargo.toml`, runs `makensis -DVERSION=<v> installer/isle-win.nsi`.

- [ ] **Step 1:** NSIS script (per-user, `RequestExecutionLevel user`): installs to `$LOCALAPPDATA\Programs\IsleWin`, Start Menu shortcut, `Uninstall` entry in `HKCU\...\Uninstall\IsleWin`, uninstaller removes files, shortcut, the autostart Run value, and kills a running `isle-win.exe` first; optional "Mulai dengan Windows" checkbox page.
- [ ] **Step 2:** Workflow `release.yml` on tag `v*` (`windows-latest`, `cargo build --release`, install NSIS via choco, build installer, attach to the release). It stays inert until a remote exists.
- [ ] **Step 3: Verify:** `scripts/build-installer.sh` produces `dist/IsleWin-Setup-0.1.0.exe`; file size is a few MB at most (report the actual size); copy it to `/mnt/c/Users/Public/isle-win/` and ask the user to run it: it installs without a UAC prompt, the notch starts, the Start Menu entry works, uninstalling removes everything including the Run value.
- [ ] **Step 4:** README: what it is, how to build/install, the stage list, credits to Isle (GPL-3.0-or-later) and the LRCLIB attribution.
- [ ] **Step 5: Commit** `feat: NSIS installer, release script, CI workflow`; then `git tag v0.1.0` after the user confirms the installer works, and `v0.2.0` once Tasks 11-13 are confirmed.

---

## Self-review notes

- **Spec coverage:** Window/hover/spring/DPI/click-through (Tasks 3, 6-8), modules and extensibility (Task 5), clock, paging, tray, autostart, fullscreen (Tasks 9-10), Charging (11), Music (11), Focus timer incl. countdown in the pill (12), Lyrics (13), settings (4), installer and CI (14), local repo only (no remote steps). Stages 5-10 are deliberately out of this plan.
- **Type consistency:** `Module`/`Registry`/`Canvas` (Task 5) are the only shared interfaces and later tasks use them as defined; `target(open, wide, lyrics)` is defined in Task 3 and used in Tasks 8 and 13; `MediaInfo::position_at` (Task 11) feeds `current_index` (Task 13).
- **Proportion:** the plan states signatures, values and checks; bodies are left to the implementer.
