// SPDX-License-Identifier: GPL-3.0-or-later
#![windows_subsystem = "windows"]

mod d2d_canvas;
mod renderer;
mod window;

fn main() -> windows::core::Result<()> {
    window::run()
}
