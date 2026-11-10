// SPDX-License-Identifier: GPL-3.0-or-later
//! Platform-independent logic for Buginotch. Nothing here touches Windows APIs,
//! so everything is unit-tested on Linux.

pub mod airpods;
pub mod clipboard;
pub mod digioh;
pub mod digioh_layout;
pub mod usage;
pub mod canvas;
pub mod glass;
pub mod chrome;
pub mod equaliser;
pub mod headphones;
pub mod layout;
pub mod lrc;
pub mod lyrics;
pub mod media;
pub mod module;
pub mod music_layout;
pub mod notifs;
pub mod osd;
pub mod peek;
pub mod phone;
pub mod rest;
pub mod pomodoro;
pub mod settings;
pub mod settings_view;
pub mod spring;
pub mod timer_store;
pub mod track_card;
pub mod text_edit;
pub mod todo;
pub mod update;
pub mod water;
pub mod widgets;
pub mod todo_layout;
pub mod timefmt;
pub mod timer_layout;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod page_transition;
pub mod shelf;
pub mod shelf_layout;
