// SPDX-License-Identifier: GPL-3.0-or-later
//! Platform-independent logic for Isle for Windows. Nothing here touches Windows APIs,
//! so everything is unit-tested on Linux.

pub mod canvas;
pub mod chrome;
pub mod layout;
pub mod lrc;
pub mod media;
pub mod module;
pub mod music_layout;
pub mod peek;
pub mod pomodoro;
pub mod settings;
pub mod spring;
pub mod timefmt;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
