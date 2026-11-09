# Isle for Windows

A small native notch/island at the top-centre of the screen: clock, music (system
media sessions), synced lyrics, a focus timer and charging peeks. Written from
scratch in Rust with Win32, DirectComposition, Direct2D and DirectWrite: no WebView,
no GUI framework; the exe is a few hundred KB and idles at ~0% CPU.

Inspired by [Isle](https://github.com/zickrian/isle) (GNOME) by zickrian.

## Install

Run `IsleWin-Setup-<version>.exe` (per-user, no admin rights). Hover the top of the
screen to open the notch; the tray icon has "Mulai dengan Windows" and "Keluar".

## Build

In WSL/Linux with `mingw-w64` and `nsis`:

```
rustup target add x86_64-pc-windows-gnu
cargo test -p isle-logic                # pure logic, runs anywhere
scripts/build-installer.sh              # dist/IsleWin-Setup-<version>.exe
```

## Stages

1. Shell + Charging  2. Music  3. Focus timer  4. Lyrics: done.
5. Headphones  6. Text input + To-do  7. JSON cards  8. Shelf  9. Clipboard
10. Water, Notifications, Widgets, Resting: planned (see `docs/superpowers/specs`).

## Credits and licence

GPL-3.0-or-later. `pomodoro.rs` and `lrc.rs` are adapted from Isle (GPL-3.0-or-later);
see `NOTICE.md`. Lyrics come from [LRCLIB](https://lrclib.net). The reference product
Nusa inspired some features; no assets or code were copied.
