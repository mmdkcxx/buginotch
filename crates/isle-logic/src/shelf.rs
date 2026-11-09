// SPDX-License-Identifier: GPL-3.0-or-later
//! The Shelf: files parked on the notch. Files are referenced by path, never copied.

/// Most files kept on the shelf.
pub const MAX_ITEMS: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: u64,
    pub path: String,
}

#[derive(Debug, Default)]
pub struct Shelf {
    items: Vec<Item>,
    next_id: u64,
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The file name without its folder (works for `\` and `/`).
pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Upper-case extension without the dot ("PDF"), or "" for none. At most 4 chars.
pub fn extension(path: &str) -> String {
    let name = file_name(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => ext.chars().take(4).collect::<String>().to_uppercase(),
        _ => String::new(),
    }
}

impl Shelf {
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Adds files to the front, in the order given. Paths already on the shelf are not added
    /// twice; when the shelf is full the oldest files drop off. Returns how many were new.
    pub fn add(&mut self, paths: &[String]) -> usize {
        let mut added = 0;
        for p in paths.iter().rev() {
            if p.trim().is_empty() || self.items.iter().any(|i| same(&i.path, p)) {
                continue;
            }
            self.next_id += 1;
            self.items.insert(0, Item { id: self.next_id, path: p.clone() });
            added += 1;
        }
        self.items.truncate(MAX_ITEMS);
        added
    }

    pub fn remove(&mut self, id: u64) {
        self.items.retain(|i| i.id != id);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn paths(&self) -> Vec<String> {
        self.items.iter().map(|i| i.path.clone()).collect()
    }

    /// Drops files for which `exists` is false. Returns whether any went.
    pub fn retain_existing(&mut self, exists: impl Fn(&str) -> bool) -> bool {
        let before = self.items.len();
        self.items.retain(|i| exists(&i.path));
        self.items.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn dropped_files_go_on_top_keeping_their_order() {
        let mut s = Shelf::default();
        s.add(&[p("C:\\a.txt")]);
        assert_eq!(s.add(&[p("C:\\b.txt"), p("C:\\c.txt")]), 2);
        assert_eq!(s.paths(), ["C:\\b.txt", "C:\\c.txt", "C:\\a.txt"]);
    }

    #[test]
    fn the_same_file_is_not_added_twice_whatever_its_case() {
        let mut s = Shelf::default();
        s.add(&[p("C:\\Foto\\a.PNG")]);
        assert_eq!(s.add(&[p("c:\\foto\\a.png"), p("  ")]), 0);
        assert_eq!(s.items().len(), 1);
    }

    #[test]
    fn a_full_shelf_drops_the_oldest() {
        let mut s = Shelf::default();
        for i in 0..MAX_ITEMS + 3 {
            s.add(&[format!("C:\\f{i}.txt")]);
        }
        assert_eq!(s.items().len(), MAX_ITEMS);
        assert_eq!(s.items()[0].path, format!("C:\\f{}.txt", MAX_ITEMS + 2));
        assert!(!s.paths().contains(&p("C:\\f0.txt")));
    }

    #[test]
    fn remove_and_clear_and_missing_files() {
        let mut s = Shelf::default();
        s.add(&[p("C:\\a.txt"), p("C:\\b.txt")]);
        let id = s.items()[0].id;
        s.remove(id);
        assert_eq!(s.paths(), ["C:\\b.txt"]);
        s.add(&[p("C:\\gone.txt")]);
        assert!(s.retain_existing(|p| !p.contains("gone")));
        assert!(!s.retain_existing(|_| true));
        s.clear();
        assert!(s.items().is_empty());
    }

    #[test]
    fn names_and_extensions_are_read_from_the_path() {
        assert_eq!(file_name("C:\\Users\\a\\Laporan Q3.pdf"), "Laporan Q3.pdf");
        assert_eq!(file_name("/home/x/y.png"), "y.png");
        assert_eq!(extension("C:\\x\\Laporan.pdf"), "PDF");
        assert_eq!(extension("C:\\x\\arsip.tar.gz"), "GZ");
        assert_eq!(extension("C:\\x\\.gitignore"), "");
        assert_eq!(extension("C:\\x\\README"), "");
        assert_eq!(extension("C:\\x\\a.markdown"), "MARK");
    }
}
