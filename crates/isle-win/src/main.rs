// SPDX-License-Identifier: GPL-3.0-or-later
#![windows_subsystem = "windows"]

mod window;

fn main() -> windows::core::Result<()> {
    window::run()
}
