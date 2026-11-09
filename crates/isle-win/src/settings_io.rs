// SPDX-License-Identifier: GPL-3.0-or-later
//! Reading and read-modify-writing `%APPDATA%\IsleWin\settings.toml`, so independent
//! features never overwrite each other's changes with a stale copy.

use crate::system::data_path;
use isle_logic::settings::Settings;

pub fn load() -> Settings {
    std::fs::read_to_string(data_path("settings.toml")).map(|t| Settings::from_toml(&t)).unwrap_or_default()
}

pub fn update(f: impl FnOnce(&mut Settings)) {
    let mut s = load();
    f(&mut s);
    let path = data_path("settings.toml");
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, s.to_toml());
}
