// SPDX-License-Identifier: GPL-3.0-or-later
#![windows_subsystem = "windows"]

mod app;
mod backdrop;
mod digioh;
mod d2d_canvas;
mod headphones_win;
mod osd_win;
mod input_win;
mod shell_win;
mod lrclib;
mod media_win;
mod notif_win;
mod phone;
mod modules;
mod power;
mod renderer;
mod settings_io;
mod settings_win;
mod system;
mod tray;
mod updater;
mod usage;
mod ui;
mod window;

/// Appends a line to `%TEMP%\buginotch.log` (the exe has no console).
pub(crate) fn log(line: &str) {
    use std::io::Write;
    let path = std::env::temp_dir().join("buginotch.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| log(&format!("panic: {info}"))));
    if !system::acquire_single_instance() {
        return;
    }
    if let Err(e) = window::run() {
        log(&format!("error: {e}"));
    }
    // An update the user chose to install when quitting.
    updater::run_pending_install();
    // Leave at once. Even ExitProcess crashed here (the detach of the graphics DLLs), and a
    // crashing process keeps its exe locked, which broke updates. See `window::run`.
    unsafe {
        use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
        let _ = TerminateProcess(GetCurrentProcess(), 0);
    }
}
