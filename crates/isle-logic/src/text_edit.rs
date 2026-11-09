// SPDX-License-Identifier: GPL-3.0-or-later
//! State of a single-line text box: text, caret, selection and an IME composition.
//! Positions are `char` indices. Drawing and measuring live with the platform code.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composition {
    pub text: String,
    /// Caret position inside the composition, in chars.
    pub cursor: usize,
}

#[derive(Debug, Clone)]
pub struct TextEdit {
    text: String,
    caret: usize,
    anchor: Option<usize>,
    composition: Option<Composition>,
    max_chars: usize,
}

impl Default for TextEdit {
    fn default() -> Self {
        Self::new(200)
    }
}

fn byte_at(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map_or(s.len(), |(i, _)| i)
}

/// Newlines and tabs become spaces, other control characters are dropped.
fn clean(s: &str) -> String {
    s.chars().filter_map(|c| match c {
        '\n' | '\r' | '\t' => Some(' '),
        c if c.is_control() => None,
        c => Some(c),
    }).collect()
}

impl TextEdit {
    pub fn new(max_chars: usize) -> Self {
        Self { text: String::new(), caret: 0, anchor: None, composition: None, max_chars }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn clear(&mut self) {
        *self = Self::new(self.max_chars);
    }

    pub fn set_text(&mut self, s: &str) {
        self.clear();
        self.insert(s);
    }

    /// The selected range as (start, end) in chars, when non-empty.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.caret).then(|| (a.min(self.caret), a.max(self.caret)))
    }

    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selection()?;
        Some(self.text[byte_at(&self.text, a)..byte_at(&self.text, b)].to_string())
    }

    fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        self.text.replace_range(byte_at(&self.text, a)..byte_at(&self.text, b), "");
        self.caret = a;
        self.anchor = None;
        true
    }

    /// Types or pastes `s` at the caret, replacing the selection. Stops at the length limit.
    pub fn insert(&mut self, s: &str) {
        self.composition = None;
        self.delete_selection();
        let room = self.max_chars.saturating_sub(self.len());
        let s: String = clean(s).chars().take(room).collect();
        if s.is_empty() {
            return;
        }
        self.text.insert_str(byte_at(&self.text, self.caret), &s);
        self.caret += s.chars().count();
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() || self.caret == 0 {
            return;
        }
        let (from, to) = (byte_at(&self.text, self.caret - 1), byte_at(&self.text, self.caret));
        self.text.replace_range(from..to, "");
        self.caret -= 1;
    }

    pub fn delete(&mut self) {
        if self.delete_selection() || self.caret >= self.len() {
            return;
        }
        let (from, to) = (byte_at(&self.text, self.caret), byte_at(&self.text, self.caret + 1));
        self.text.replace_range(from..to, "");
    }

    fn go(&mut self, to: usize, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = to.min(self.len());
    }

    pub fn move_left(&mut self, extend: bool) {
        match (self.selection(), extend) {
            (Some((a, _)), false) => self.go(a, false),
            _ => self.go(self.caret.saturating_sub(1), extend),
        }
    }

    pub fn move_right(&mut self, extend: bool) {
        match (self.selection(), extend) {
            (Some((_, b)), false) => self.go(b, false),
            _ => self.go(self.caret + 1, extend),
        }
    }

    pub fn home(&mut self, extend: bool) {
        self.go(0, extend);
    }

    pub fn end(&mut self, extend: bool) {
        self.go(self.len(), extend);
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.len();
    }

    /// Removes and returns the selected text (for Ctrl+X).
    pub fn cut(&mut self) -> Option<String> {
        let t = self.selected_text()?;
        self.delete_selection();
        Some(t)
    }

    /// Moves the caret to char index `i` (a click), optionally extending the selection (a drag).
    pub fn set_caret(&mut self, i: usize, extend: bool) {
        self.go(i, extend);
    }

    /// The char index whose boundary is nearest to `x`, given `measure` for the width of a prefix.
    pub fn index_at(&self, x: f32, measure: &mut dyn FnMut(&str) -> f32) -> usize {
        let n = self.len();
        let mut best = (0usize, x.abs());
        for i in 1..=n {
            let d = (measure(&self.text[..byte_at(&self.text, i)]) - x).abs();
            if d < best.1 {
                best = (i, d);
            }
        }
        best.0
    }

    /// An IME is composing `text` at the caret (`None` ends the composition).
    pub fn set_composition(&mut self, c: Option<Composition>) {
        if c.is_some() {
            self.delete_selection();
        }
        self.composition = c.filter(|c| !c.text.is_empty());
    }

    pub fn composition(&self) -> Option<&Composition> {
        self.composition.as_ref()
    }

    /// The text as drawn: the composition shows inside it at the caret.
    pub fn display_text(&self) -> String {
        match &self.composition {
            Some(c) => {
                let at = byte_at(&self.text, self.caret);
                format!("{}{}{}", &self.text[..at], c.text, &self.text[at..])
            }
            None => self.text.clone(),
        }
    }

    /// Composition range inside `display_text()`, in chars (for the underline).
    pub fn composition_range(&self) -> Option<(usize, usize)> {
        self.composition.as_ref().map(|c| (self.caret, self.caret + c.text.chars().count()))
    }

    /// Text up to where the caret is drawn (inside the composition while one is active).
    pub fn caret_prefix(&self) -> String {
        let shown = self.display_text();
        let at = self.caret + self.composition.as_ref().map_or(0, |c| c.cursor.min(c.text.chars().count()));
        shown[..byte_at(&shown, at)].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(s: &str) -> TextEdit {
        let mut t = TextEdit::new(50);
        t.insert(s);
        t
    }

    #[test]
    fn typing_inserts_at_the_caret() {
        let mut t = with("helo");
        t.move_left(false);
        t.insert("l");
        assert_eq!((t.text(), t.caret()), ("hello", 4));
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut t = with("hello world");
        t.home(false);
        for _ in 0..5 {
            t.move_right(true);
        }
        assert_eq!(t.selected_text().as_deref(), Some("hello"));
        t.insert("bye");
        assert_eq!((t.text(), t.caret(), t.selection()), ("bye world", 3, None));
    }

    #[test]
    fn backspace_and_delete_remove_a_char_or_the_selection() {
        let mut t = with("abc");
        t.backspace();
        assert_eq!(t.text(), "ab");
        t.home(false);
        t.delete();
        assert_eq!(t.text(), "b");
        t.select_all();
        t.backspace();
        assert_eq!((t.text(), t.caret()), ("", 0));
        t.backspace();
        t.delete();
        assert_eq!(t.text(), "");
    }

    #[test]
    fn arrows_collapse_a_selection_to_its_edge() {
        let mut t = with("abcd");
        t.home(false);
        t.move_right(true);
        t.move_right(true);
        t.move_left(false);
        assert_eq!((t.caret(), t.selection()), (0, None));
        t.end(true);
        t.move_right(false);
        assert_eq!((t.caret(), t.selection()), (4, None));
    }

    #[test]
    fn shift_arrows_grow_and_shrink_the_selection() {
        let mut t = with("abcd");
        t.move_left(true);
        t.move_left(true);
        assert_eq!(t.selection(), Some((2, 4)));
        t.move_right(true);
        assert_eq!(t.selection(), Some((3, 4)));
        t.move_right(true);
        assert_eq!(t.selection(), None);
    }

    #[test]
    fn cut_returns_the_selection_and_removes_it() {
        let mut t = with("abcdef");
        t.set_caret(1, false);
        t.set_caret(4, true);
        assert_eq!(t.cut().as_deref(), Some("bcd"));
        assert_eq!(t.text(), "aef");
        assert_eq!(t.cut(), None);
    }

    #[test]
    fn newlines_become_spaces_and_control_chars_vanish() {
        let t = with("a\nb\u{7}c\td");
        assert_eq!(t.text(), "a bc d");
    }

    #[test]
    fn the_length_limit_stops_typing_and_paste() {
        let mut t = TextEdit::new(5);
        t.insert("abc");
        t.insert("defgh");
        assert_eq!(t.text(), "abcde");
        t.insert("z");
        assert_eq!(t.text(), "abcde");
    }

    #[test]
    fn multibyte_text_is_edited_by_char() {
        let mut t = with("日本語é");
        assert_eq!(t.len(), 4);
        t.backspace();
        t.move_left(false);
        t.insert("x");
        assert_eq!((t.text(), t.caret()), ("日本x語", 3));
        t.home(false);
        t.delete();
        assert_eq!(t.text(), "本x語");
    }

    #[test]
    fn a_click_lands_on_the_nearest_boundary() {
        let t = with("abcd");
        let mut ten_per_char = |s: &str| s.chars().count() as f32 * 10.0;
        assert_eq!(t.index_at(-5.0, &mut ten_per_char), 0);
        assert_eq!(t.index_at(14.0, &mut ten_per_char), 1);
        assert_eq!(t.index_at(16.0, &mut ten_per_char), 2);
        assert_eq!(t.index_at(500.0, &mut ten_per_char), 4);
    }

    #[test]
    fn a_composition_shows_inside_the_text_without_changing_it() {
        let mut t = with("ab");
        t.move_left(false);
        t.set_composition(Some(Composition { text: "にほ".into(), cursor: 2 }));
        assert_eq!(t.display_text(), "aにほb");
        assert_eq!(t.text(), "ab");
        assert_eq!(t.composition_range(), Some((1, 3)));
        assert_eq!(t.caret_prefix(), "aにほ");
        t.set_composition(Some(Composition { text: "にほ".into(), cursor: 1 }));
        assert_eq!(t.caret_prefix(), "aに");
    }

    #[test]
    fn committing_text_ends_the_composition() {
        let mut t = with("a");
        t.set_composition(Some(Composition { text: "に".into(), cursor: 1 }));
        t.insert("日");
        assert_eq!((t.composition(), t.text()), (None, "a日"));
        t.set_composition(Some(Composition { text: String::new(), cursor: 0 }));
        assert_eq!(t.composition(), None);
    }
}
