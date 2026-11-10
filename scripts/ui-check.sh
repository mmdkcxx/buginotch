#!/usr/bin/env bash
# UI cross-check: shows every page in solid, dark-glass and light-glass looks, over a flat and a
# striped backdrop, then checks the shots and writes a contact sheet to target/ui-check/.
#   scripts/ui-check.sh                 build, capture, check
#   scripts/ui-check.sh --no-build      reuse target/.../debug/buginotch.exe
#   scripts/ui-check.sh --exe PATH      check another exe (a Windows-readable path under /mnt/c)
# Needs WSL with Windows interop. It runs a private copy of the exe, so it refuses to start while
# Buginotch is already running (they would sit on top of each other); stop it first or pass
# --stop-installed. The shots contain your real todo/clipboard/notification text: keep them local.
set -euo pipefail
cd "$(dirname "$0")/.."
BUILD=1; EXE=""; STOP=0
while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) BUILD=0 ;;
    --exe) EXE="$2"; shift ;;
    --stop-installed) STOP=1 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
running() { powershell.exe -NoProfile -Command "@(Get-Process buginotch,isle-win -ErrorAction SilentlyContinue).Count" | tr -d '\r'; }
if [ "$(running)" != "0" ]; then
  if [ "$STOP" = 1 ]; then
    powershell.exe -NoProfile -Command "Get-Process buginotch,isle-win -ErrorAction SilentlyContinue | Stop-Process -Force" >/dev/null
    sleep 2
  else
    echo "Buginotch is running. Quit it first (tray menu > Keluar) or pass --stop-installed." >&2
    exit 3
  fi
fi
if [ -z "$EXE" ]; then
  [ "$BUILD" = 1 ] && cargo build -p isle-win --target x86_64-pc-windows-gnu
  EXE=target/x86_64-pc-windows-gnu/debug/buginotch.exe
fi
WTEMP=$(powershell.exe -NoProfile -Command '$env:TEMP' | tr -d '\r')
LTEMP=$(wslpath "$WTEMP")
WORK="$LTEMP/buginotch-uicheck"; mkdir -p "$WORK"
cp "$EXE" "$WORK/buginotch-uicheck.exe"
cp scripts/ui-check.ps1 "$WORK/ui-check.ps1"
OUT=target/ui-check; rm -rf "$OUT"; mkdir -p "$OUT/shots"
for look in solid dark light; do
  for backdrop in flat stripes; do
    echo "capturing $look / $backdrop"
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$WTEMP\\buginotch-uicheck\\ui-check.ps1" \
      -Exe "$WTEMP\\buginotch-uicheck\\buginotch-uicheck.exe" -Out "$WTEMP\\buginotch-uicheck\\shots" -Look $look -Backdrop $backdrop >/dev/null
    sleep 2
  done
done
cp "$WORK"/shots/*.png "$OUT/shots/"
python3 -I scripts/ui_check.py "$OUT"
