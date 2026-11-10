#!/usr/bin/env bash
# Builds the release exe and the NSIS installer into dist/.
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
cargo build --release --target x86_64-pc-windows-gnu -p isle-win
mkdir -p dist
makensis -DVERSION="$VERSION" installer/buginotch.nsi
ls -l "dist/Buginotch-Setup-$VERSION.exe"
