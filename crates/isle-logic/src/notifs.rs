// SPDX-License-Identifier: GPL-3.0-or-later
//! Windows notifications as the Notifications page and the pill see them.

use std::collections::HashSet;

/// How many recent notifications the page keeps.
pub const KEEP: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub struct Notif {
    pub id: u32,
    pub app: String,
    pub title: String,
    pub body: String,
    /// When it arrived (epoch ms).
    pub at_ms: i64,
}

#[derive(Debug, Default)]
pub struct Feed {
    seen: HashSet<u32>,
    items: Vec<Notif>,
    primed: bool,
}

impl Feed {
    /// Takes the notifications Windows holds now. Returns the ones that arrived since the last
    /// call, newest first. The first call only remembers what is already there.
    pub fn update(&mut self, mut all: Vec<Notif>) -> Vec<Notif> {
        all.sort_by(|a, b| b.at_ms.cmp(&a.at_ms));
        let fresh: Vec<Notif> = if self.primed { all.iter().filter(|n| !self.seen.contains(&n.id)).cloned().collect() } else { Vec::new() };
        self.primed = true;
        self.seen = all.iter().map(|n| n.id).collect();
        all.truncate(KEEP);
        self.items = all;
        fresh
    }

    /// The most recent notifications, newest first (at most `KEEP`).
    pub fn items(&self) -> &[Notif] {
        &self.items
    }
}

/// One line for the pill: "Dina · Jadi makan malam?". Line breaks become spaces.
pub fn snippet(n: &Notif) -> String {
    let flat = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ");
    let (title, body) = (flat(&n.title), flat(&n.body));
    match (title.is_empty(), body.is_empty()) {
        (false, false) => format!("{title} · {body}"),
        (false, true) => title,
        _ => body,
    }
}

/// The first letter or digit of the app name, upper-cased, for its icon tile.
pub fn letter(app: &str) -> char {
    app.chars().find(|c| c.is_alphanumeric()).and_then(|c| c.to_uppercase().next()).unwrap_or('?')
}

/// A tile colour for an app, the same every time.
pub fn tile_color(app: &str) -> (u8, u8, u8) {
    const PALETTE: [(u8, u8, u8); 6] = [(34, 165, 89), (217, 119, 87), (59, 130, 246), (155, 89, 182), (225, 160, 40), (20, 160, 190)];
    let h = app.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
    PALETTE[h as usize % PALETTE.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(id: u32, at: i64) -> Notif {
        Notif { id, app: "WhatsApp".into(), title: "Dina".into(), body: "Halo".into(), at_ms: at }
    }

    #[test]
    fn the_first_update_reports_nothing_new() {
        let mut f = Feed::default();
        assert!(f.update(vec![n(1, 10), n(2, 20)]).is_empty());
        assert_eq!(f.items().iter().map(|i| i.id).collect::<Vec<_>>(), vec![2, 1]);
    }

    #[test]
    fn later_updates_report_only_unseen_ones_newest_first() {
        let mut f = Feed::default();
        f.update(vec![n(1, 10)]);
        let fresh = f.update(vec![n(1, 10), n(2, 20), n(3, 30)]);
        assert_eq!(fresh.iter().map(|i| i.id).collect::<Vec<_>>(), vec![3, 2]);
        assert!(f.update(vec![n(1, 10), n(2, 20), n(3, 30)]).is_empty());
    }

    #[test]
    fn a_dismissed_notification_leaves_the_list_without_counting_as_new() {
        let mut f = Feed::default();
        f.update(vec![n(1, 10), n(2, 20)]);
        assert!(f.update(vec![n(2, 20)]).is_empty());
        assert_eq!(f.items().len(), 1);
    }

    #[test]
    fn keeps_only_the_latest_few() {
        let mut f = Feed::default();
        f.update((0..20).map(|i| n(i, i as i64)).collect());
        assert_eq!(f.items().len(), KEEP);
        assert_eq!(f.items()[0].id, 19);
    }

    #[test]
    fn snippet_joins_title_and_body_on_one_line() {
        let mut x = n(1, 0);
        x.body = "Jadi makan\nmalam?".into();
        assert_eq!(snippet(&x), "Dina · Jadi makan malam?");
        x.body.clear();
        assert_eq!(snippet(&x), "Dina");
        x.title.clear();
        x.body = "Halo".into();
        assert_eq!(snippet(&x), "Halo");
    }

    #[test]
    fn letter_is_the_first_alphanumeric_uppercased() {
        assert_eq!(letter("whatsapp"), 'W');
        assert_eq!(letter("  3D Viewer"), '3');
        assert_eq!(letter("…"), '?');
        assert_eq!(letter(""), '?');
    }

    #[test]
    fn tile_color_is_stable_per_app() {
        assert_eq!(tile_color("WhatsApp"), tile_color("WhatsApp"));
        assert_ne!(tile_color("WhatsApp"), tile_color("Gmail"));
    }
}
