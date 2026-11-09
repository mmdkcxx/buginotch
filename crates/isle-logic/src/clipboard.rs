// SPDX-License-Identifier: GPL-3.0-or-later
//! Clipboard history: recent copied text, pins, search, and the privacy limits.

/// Most unpinned entries kept.
pub const MAX_ITEMS: usize = 50;
/// Longest text stored (chars); longer copies are ignored.
pub const MAX_CHARS: usize = 4000;
/// Unpinned entries older than this are dropped.
pub const MAX_AGE_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u64,
    pub text: String,
    pub pinned: bool,
    pub at_ms: i64,
}

impl Entry {
    pub fn kind(&self) -> Kind {
        kind_of(&self.text)
    }
}

pub fn kind_of(text: &str) -> Kind {
    let t = text.trim();
    let link = (t.starts_with("http://") || t.starts_with("https://")) && !t.contains(char::is_whitespace);
    if link { Kind::Link } else { Kind::Text }
}

#[derive(Debug, Default)]
pub struct History {
    entries: Vec<Entry>,
    next_id: u64,
    pub paused: bool,
}

impl History {
    /// Adds a copy. Returns false when it was ignored (paused, blank, too long). A repeat
    /// of an existing entry moves it to the top instead of adding a second one.
    pub fn add(&mut self, text: &str, now_ms: i64) -> bool {
        if self.paused || text.trim().is_empty() || text.chars().count() > MAX_CHARS {
            return false;
        }
        let mut pinned = false;
        if let Some(i) = self.entries.iter().position(|e| e.text == text) {
            pinned = self.entries.remove(i).pinned;
        }
        self.next_id += 1;
        self.entries.insert(0, Entry { id: self.next_id, text: text.to_string(), pinned, at_ms: now_ms });
        self.trim(now_ms);
        true
    }

    /// Drops old and surplus unpinned entries. Returns whether any went.
    pub fn trim(&mut self, now_ms: i64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.pinned || now_ms - e.at_ms <= MAX_AGE_MS);
        let mut kept = 0;
        self.entries.retain(|e| {
            if e.pinned {
                return true;
            }
            kept += 1;
            kept <= MAX_ITEMS
        });
        self.entries.len() != before
    }

    pub fn toggle_pin(&mut self, id: u64) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.pinned = !e.pinned;
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.entries.retain(|e| e.id != id);
    }

    /// Forgets everything that is not pinned.
    pub fn clear(&mut self) {
        self.entries.retain(|e| e.pinned);
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries matching `query` (case-insensitive substring; blank matches all), pinned
    /// first, then newest first.
    pub fn visible(&self, query: &str) -> Vec<&Entry> {
        let q = query.trim().to_lowercase();
        let mut v: Vec<&Entry> = self.entries.iter().filter(|e| q.is_empty() || e.text.to_lowercase().contains(&q)).collect();
        v.sort_by_key(|e| !e.pinned);
        v
    }

    pub fn pinned(&self) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.pinned).collect()
    }

    /// Restores pinned entries saved earlier.
    pub fn restore(&mut self, texts: Vec<String>, now_ms: i64) {
        for t in texts.into_iter().rev() {
            if self.add(&t, now_ms) {
                self.entries[0].pinned = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(h: &History, q: &str) -> Vec<String> {
        h.visible(q).iter().map(|e| e.text.clone()).collect()
    }

    #[test]
    fn newest_copy_is_first_and_a_repeat_moves_up_instead_of_duplicating() {
        let mut h = History::default();
        h.add("a", 0);
        h.add("b", 1);
        h.add("a", 2);
        assert_eq!(texts(&h, ""), ["a", "b"]);
    }

    #[test]
    fn blank_overlong_and_paused_copies_are_ignored() {
        let mut h = History::default();
        assert!(!h.add("   ", 0));
        assert!(!h.add(&"x".repeat(MAX_CHARS + 1), 0));
        assert!(h.add(&"x".repeat(MAX_CHARS), 0));
        h.paused = true;
        assert!(!h.add("secret", 1));
        assert_eq!(h.visible("").len(), 1);
    }

    #[test]
    fn pinned_entries_float_to_the_top_and_survive_clear() {
        let mut h = History::default();
        h.add("old", 0);
        h.add("new", 1);
        let id = h.visible("")[1].id;
        h.toggle_pin(id);
        assert_eq!(texts(&h, ""), ["old", "new"]);
        h.clear();
        assert_eq!(texts(&h, ""), ["old"]);
    }

    #[test]
    fn a_repeat_of_a_pinned_entry_stays_pinned() {
        let mut h = History::default();
        h.add("a", 0);
        let id = h.visible("")[0].id;
        h.toggle_pin(id);
        h.add("a", 5);
        assert!(h.visible("")[0].pinned);
        assert_eq!(h.visible("").len(), 1);
    }

    #[test]
    fn search_ignores_case_and_blank_shows_everything() {
        let mut h = History::default();
        h.add("Rapat Senin", 0);
        h.add("belanja", 1);
        assert_eq!(texts(&h, "rapat"), ["Rapat Senin"]);
        assert_eq!(texts(&h, "  ").len(), 2);
        assert!(texts(&h, "zzz").is_empty());
    }

    #[test]
    fn surplus_and_old_entries_go_but_pinned_ones_stay() {
        let mut h = History::default();
        h.add("keep", 0);
        let id = h.visible("")[0].id;
        h.toggle_pin(id);
        for i in 0..(MAX_ITEMS + 5) {
            h.add(&format!("item {i}"), 1);
        }
        assert_eq!(h.visible("").len(), MAX_ITEMS + 1);
        assert!(h.trim(MAX_AGE_MS + 10));
        assert_eq!(texts(&h, ""), ["keep"]);
    }

    #[test]
    fn links_are_told_apart_from_text() {
        assert_eq!(kind_of("https://example.com/a?b=1"), Kind::Link);
        assert_eq!(kind_of("https://example.com dan teks"), Kind::Text);
        assert_eq!(kind_of("example.com"), Kind::Text);
    }

    #[test]
    fn pinned_texts_restore_pinned_in_their_saved_order() {
        let mut h = History::default();
        h.restore(vec!["first".into(), "second".into()], 0);
        assert_eq!(texts(&h, ""), ["first", "second"]);
        assert!(h.visible("").iter().all(|e| e.pinned));
    }
}
