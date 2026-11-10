// SPDX-License-Identifier: GPL-3.0-or-later
//! The Ayat page: which passage of the whole Bible to show in each half hour, and how to read
//! and tidy a chapter fetched from the internet. No Bible text is stored here or anywhere in
//! this repository; the app asks an online Bible for it.

/// A new passage every 30 minutes.
pub const SLOT_MS: i64 = 30 * 60_000;

pub struct Book {
    /// As the online Bible spells it (also used in the request).
    pub name: &'static str,
    /// For the reference line: "Yoh 3:16".
    pub short: &'static str,
    pub chapters: u32,
}

const fn b(name: &'static str, short: &'static str, chapters: u32) -> Book {
    Book { name, short, chapters }
}

pub const BOOKS: [Book; 66] = [
    b("Kejadian", "Kej", 50), b("Keluaran", "Kel", 40), b("Imamat", "Im", 27), b("Bilangan", "Bil", 36), b("Ulangan", "Ul", 34),
    b("Yosua", "Yos", 24), b("Hakim-hakim", "Hak", 21), b("Rut", "Rut", 4), b("1 Samuel", "1Sam", 31), b("2 Samuel", "2Sam", 24),
    b("1 Raja-raja", "1Raj", 22), b("2 Raja-raja", "2Raj", 25), b("1 Tawarikh", "1Taw", 29), b("2 Tawarikh", "2Taw", 36),
    b("Ezra", "Ezr", 10), b("Nehemia", "Neh", 13), b("Ester", "Est", 10), b("Ayub", "Ayb", 42), b("Mazmur", "Mzm", 150),
    b("Amsal", "Ams", 31), b("Pengkhotbah", "Pkh", 12), b("Kidung Agung", "Kid", 8), b("Yesaya", "Yes", 66), b("Yeremia", "Yer", 52),
    b("Ratapan", "Rat", 5), b("Yehezkiel", "Yeh", 48), b("Daniel", "Dan", 12), b("Hosea", "Hos", 14), b("Yoel", "Yl", 3),
    b("Amos", "Am", 9), b("Obaja", "Ob", 1), b("Yunus", "Yun", 4), b("Mikha", "Mi", 7), b("Nahum", "Nah", 3),
    b("Habakuk", "Hab", 3), b("Zefanya", "Zef", 3), b("Hagai", "Hag", 2), b("Zakharia", "Za", 14), b("Maleakhi", "Mal", 4),
    b("Matius", "Mat", 28), b("Markus", "Mrk", 16), b("Lukas", "Luk", 24), b("Yohanes", "Yoh", 21), b("Kisah Para Rasul", "Kis", 28),
    b("Roma", "Rm", 16), b("1 Korintus", "1Kor", 16), b("2 Korintus", "2Kor", 13), b("Galatia", "Gal", 6), b("Efesus", "Ef", 6),
    b("Filipi", "Flp", 4), b("Kolose", "Kol", 4), b("1 Tesalonika", "1Tes", 5), b("2 Tesalonika", "2Tes", 3), b("1 Timotius", "1Tim", 6),
    b("2 Timotius", "2Tim", 4), b("Titus", "Tit", 3), b("Filemon", "Flm", 1), b("Ibrani", "Ibr", 13), b("Yakobus", "Yak", 5),
    b("1 Petrus", "1Ptr", 5), b("2 Petrus", "2Ptr", 3), b("1 Yohanes", "1Yoh", 5), b("2 Yohanes", "2Yoh", 1), b("3 Yohanes", "3Yoh", 1),
    b("Yudas", "Yud", 1), b("Wahyu", "Why", 22),
];

pub fn total_chapters() -> u32 {
    BOOKS.iter().map(|b| b.chapters).sum()
}

pub fn slot_of(now_ms: i64) -> i64 {
    now_ms.div_euclid(SLOT_MS)
}

/// When the passage after the one for `now_ms` starts.
pub fn next_change_ms(now_ms: i64) -> i64 {
    (slot_of(now_ms) + 1) * SLOT_MS
}

/// How far through the current half hour we are, 0..1.
pub fn progress(now_ms: i64) -> f32 {
    now_ms.rem_euclid(SLOT_MS) as f32 / SLOT_MS as f32
}

fn mix(mut x: u64) -> u64 {
    // splitmix64: the same slot always gives the same passage, even after a restart.
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// The book index, chapter and a seed for choosing the verse, for a slot. `attempt` gives
/// another chapter when the first one has nothing readable (or could not be fetched).
pub fn chapter_for(slot: i64, attempt: u32) -> (usize, u32, u64) {
    let seed = mix((slot as u64) ^ mix(attempt as u64 + 1));
    let mut n = (seed % total_chapters() as u64) as u32;
    for (i, book) in BOOKS.iter().enumerate() {
        if n < book.chapters {
            return (i, n + 1, mix(seed));
        }
        n -= book.chapters;
    }
    (0, 1, mix(seed))
}

/// The passage name the online Bible understands: "1 Samuel 3".
pub fn chapter_query(book: usize, chapter: u32) -> String {
    format!("{} {}", BOOKS[book].name, chapter)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verse {
    pub number: u32,
    pub text: String,
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        match rest.find(';').filter(|&e| e <= 10) {
            Some(e) => {
                let ent = &rest[1..e];
                let ch = match ent {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    _ => ent
                        .strip_prefix("#x")
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                        .and_then(char::from_u32),
                };
                match ch {
                    Some(c) => {
                        out.push(c);
                        rest = &rest[e + 1..];
                    }
                    None => {
                        out.push('&');
                        rest = &rest[1..];
                    }
                }
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let a = s.find(open)? + open.len();
    let b = s[a..].find(close)? + a;
    Some(&s[a..b])
}

/// Reads the online Bible's reply for one chapter (`<verse><number>..</number><text>..</text></verse>`).
pub fn parse_chapter(xml: &str) -> Vec<Verse> {
    xml.split("<verse>")
        .skip(1)
        .filter_map(|chunk| {
            let chunk = chunk.split("</verse>").next()?;
            let number = between(chunk, "<number>", "</number>")?.trim().parse().ok()?;
            let text = tidy(&decode_entities(&strip_tags(between(chunk, "<text>", "</text>")?)));
            (!text.is_empty()).then_some(Verse { number, text })
        })
        .collect()
}

/// Removes the Psalms' cross-numbering "(34-2)" with any heading before it, collapses spaces
/// and drops stray dashes at the ends.
pub fn tidy(s: &str) -> String {
    let mut text = s.to_string();
    // The last "(12-3)" marker: what follows is the verse itself; before it a psalm heading.
    let mut cut = None;
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'(' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j < b.len() && b[j] == b'-' {
                let mut k = j + 1;
                while k < b.len() && b[k].is_ascii_digit() {
                    k += 1;
                }
                if k > j + 1 && k < b.len() && b[k] == b')' {
                    cut = Some(k + 1);
                    i = k;
                }
            }
        }
        i += 1;
    }
    if let Some(c) = cut {
        text = text[c..].to_string();
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = text.trim_matches(|c: char| c == '-' || c == ',' || c == ';' || c.is_whitespace()).to_string();
    let mut chars = text.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
        None => text,
    }
}

/// Whether a passage reads well on its own: not too short or long, not just a list of names.
pub fn readable(text: &str) -> bool {
    let len = text.chars().count();
    if !(40..=230).contains(&len) {
        return false;
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 7 {
        return false;
    }
    let caps = words.iter().filter(|w| w.chars().next().is_some_and(char::is_uppercase)).count();
    (caps as f32) / (words.len() as f32) < 0.4
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    /// "Yoh 3:16" or "Mzm 23:1-2".
    pub reference: String,
    pub text: String,
}

/// Picks a readable passage from a chapter, starting at a place derived from `seed`. A very
/// short verse is joined with the next one. `None` when the chapter has nothing readable.
pub fn choose(book: usize, verses: &[Verse], seed: u64, chapter: u32) -> Option<Passage> {
    let n = verses.len();
    for step in 0..n {
        let i = (seed as usize % n + step) % n;
        let mut text = verses[i].text.clone();
        let mut last = verses[i].number;
        let mut j = i;
        while text.chars().count() < 40 && j + 1 < n && j < i + 2 {
            j += 1;
            text.push(' ');
            text.push_str(&verses[j].text);
            last = verses[j].number;
        }
        if readable(&text) {
            let first = verses[i].number;
            let range = if last == first { format!("{first}") } else { format!("{first}-{last}") };
            return Some(Passage { reference: format!("{} {}:{}", BOOKS[book].short, chapter, range), text });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_books_add_up_to_the_whole_bible() {
        assert_eq!(BOOKS.len(), 66);
        assert_eq!(total_chapters(), 1189);
    }

    #[test]
    fn a_slot_always_gives_the_same_chapter_in_range() {
        for slot in [0i64, 1, 12345, 59_000_000] {
            let (b, c, seed) = chapter_for(slot, 0);
            assert_eq!((b, c, seed), chapter_for(slot, 0));
            assert!(c >= 1 && c <= BOOKS[b].chapters);
        }
        assert_ne!(chapter_for(100, 0), chapter_for(101, 0));
        assert_ne!(chapter_for(100, 0), chapter_for(100, 1), "another attempt, another chapter");
    }

    #[test]
    fn chapters_are_spread_over_the_whole_bible() {
        let mut seen_books = std::collections::HashSet::new();
        for slot in 0..4000 {
            seen_books.insert(chapter_for(slot, 0).0);
        }
        assert!(seen_books.len() >= 60, "{} books seen", seen_books.len());
    }

    #[test]
    fn half_hours() {
        let t = 5 * SLOT_MS + 12 * 60_000;
        assert_eq!(slot_of(t), 5);
        assert_eq!(next_change_ms(t), 6 * SLOT_MS);
        assert!((progress(t) - 0.4).abs() < 1e-4);
        assert_eq!(slot_of(t + 20 * 60_000), 6);
    }

    #[test]
    fn query_names() {
        assert_eq!(chapter_query(8, 3), "1 Samuel 3");
    }

    const XML: &str = "<?xml version=\"1.0\"?><bible><book name=\"Contoh\"><chapter><chap>2</chap><verses>\
        <verse><number>1</number><title>Judul bagian</title><text>Ini contoh kalimat pertama yang cukup panjang untuk dibaca &amp; dipahami &quot;baik&quot;.</text></verse>\
        <verse><number>2</number><text>Pendek saja.</text></verse>\
        <verse><number>3</number><text>Lanjutan kalimat contoh yang membuatnya cukup panjang.</text></verse>\
        </verses></chapter></book></bible>";

    #[test]
    fn a_chapter_reply_is_read() {
        let v = parse_chapter(XML);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].number, 1);
        assert_eq!(v[0].text, "Ini contoh kalimat pertama yang cukup panjang untuk dibaca & dipahami \"baik\".");
        assert!(parse_chapter("not xml").is_empty());
    }

    #[test]
    fn entities_decode() {
        assert_eq!(decode_entities("a &#8220;b&#x201D; &lt;c&gt; &bogus; & d"), "a \u{201c}b\u{201d} <c> &bogus; & d");
    }

    #[test]
    fn tidying() {
        assert_eq!(tidy("Contoh judul. (34-2) isi ayat   itu;"), "Isi ayat   itu".split_whitespace().collect::<Vec<_>>().join(" "));
        assert_eq!(tidy("(46-3) Sebab itu"), "Sebab itu");
        assert_eq!(tidy("--sebab begitu--"), "Sebab begitu");
        assert_eq!(tidy("tanpa tanda"), "Tanpa tanda");
    }

    #[test]
    fn readable_rejects_lists_of_names_and_extremes() {
        assert!(readable("Ini contoh kalimat biasa yang cukup panjang untuk dibaca dengan nyaman."));
        assert!(!readable("Terlalu pendek."));
        assert!(!readable("Anak Abraham Ishak, Anak Ishak Yakub, Anak Yakub Yehuda, Anak Yehuda Peres dan Zerah."));
        assert!(!readable(&"kata ".repeat(80)));
    }

    #[test]
    fn choose_joins_short_verses_and_skips_the_unreadable() {
        let v = parse_chapter(XML);
        let p = choose(0, &v, 1, 2).unwrap();
        assert_eq!(p.reference, "Kej 2:2-3");
        assert!(p.text.starts_with("Pendek saja. Lanjutan"));
        let p0 = choose(0, &v, 0, 2).unwrap();
        assert_eq!(p0.reference, "Kej 2:1");
        assert_eq!(choose(0, &[Verse { number: 1, text: "Pendek.".into() }], 0, 1), None);
    }
}
