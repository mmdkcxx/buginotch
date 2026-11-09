// SPDX-License-Identifier: GPL-3.0-or-later
#![windows_subsystem = "windows"]

mod app;
mod d2d_canvas;
mod renderer;
mod window;

/// Appends a line to `%TEMP%\isle-win.log` (the exe has no console).
pub(crate) fn log(line: &str) {
    use std::io::Write;
    let path = std::env::temp_dir().join("isle-win.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| log(&format!("panic: {info}"))));
    if let Err(e) = window::run() {
        log(&format!("error: {e}"));
    }
}
