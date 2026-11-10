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

1. Shell + Charging  2. Music  3. Focus timer  4. Lyrics  5. Headphones
6. Text input + To-do  8. Shelf  9. Clipboard
10. Water, Notifications, Widgets, Resting: done.

7. JSON cards: planned (see `docs/superpowers/specs`).

## Auto update

IsleWin looks for a newer GitHub release at start and every 6 hours (tray menu: "Periksa
pembaruan…", "Perbarui otomatis"). A new version appears on the **Pembaruan** page and as a
short peek in the pill; nothing is installed without a click. The installer is downloaded,
checked against its SHA-256, then run silently, and IsleWin starts again by itself.

Settings in `%APPDATA%\IsleWin\settings.toml`:

```toml
auto_update = true
update_check_hours = 6
update_repo = "mmdkcxx/islenotch-winver"
update_token = ""   # read-only GitHub token, only needed while the repository is private
```

The token can also be given as the `ISLE_UPDATE_TOKEN` environment variable (nothing on
disk). Each release needs `IsleWin-Setup-x.y.z.exe`; the release workflow also uploads its
`.sha256`.

## Credits and licence

GPL-3.0-or-later. `pomodoro.rs` and `lrc.rs` are adapted from Isle (GPL-3.0-or-later);
see `NOTICE.md`. Lyrics come from [LRCLIB](https://lrclib.net). The reference product
Nusa inspired some features; no assets or code were copied.
