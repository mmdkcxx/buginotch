// SPDX-License-Identifier: GPL-3.0-or-later
//! Music: what is playing (SMTC), with transport controls and a scrub bar.

use crate::media_win::MediaWatcher;
use crate::system::epoch_ms;
use isle_logic::canvas::{Align, Canvas, Color, ImageId, Rect, fit_text};
use isle_logic::media::MediaInfo;
use isle_logic::module::{Event, Module, PillItem, Reaction};
use isle_logic::lyrics::{Lyrics, current_index, window};
use isle_logic::music_layout::{MusicHit, PLAY_RADIUS, SKIP_RADIUS, bar_fraction, hit_for, layout_p, lyric_rows};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

const WHITE: Color = Color(245, 246, 248, 255);
const DIM: Color = Color(139, 144, 153, 255);
const TRACK: Color = Color(43, 47, 54, 255);

/// What the lyrics column shows for the current track.
enum LyricsState {
    Loading,
    Ready(Lyrics),
}

type Slot = Arc<Mutex<Option<(String, Lyrics)>>>;

pub struct MusicModule {
    hwnd: isize,
    lyrics_on: bool,
    /// Slide of the lyrics column: (start ms, from, to), eased over `COLUMN_MS`.
    column_anim: (i64, f32, f32),
    lyrics: LyricsState,
    /// Track the lyrics were requested for.
    lyrics_track: String,
    slot: Slot,
    /// Current synced line as last drawn, and when it last advanced by one (for the scroll).
    lyric_idx: Option<usize>,
    /// Fraction under the thumb while the scrub bar is being dragged.
    scrub: Option<f32>,
    /// A just-released seek: shown until the player reports the new position.
    seek_hold: Option<(f32, i64)>,
    lyric_anim_start: Option<i64>,
    watcher: MediaWatcher,
    info: Option<MediaInfo>,
    cover: Option<ImageId>,
    cover_key: Option<u64>,
    /// Shown in the pill's left slot while a session exists.
    had_session: bool,
}

impl MusicModule {
    pub fn new(watcher: MediaWatcher, hwnd: isize) -> Self {
        Self { hwnd, lyrics_on: false, column_anim: (0, 0.0, 0.0), lyrics: LyricsState::Loading, lyrics_track: String::new(), slot: Arc::default(), lyric_idx: None, lyric_anim_start: None, scrub: None, seek_hold: None, watcher, info: None, cover: None, cover_key: None, had_session: false }
    }

    /// Eased 0..1 progress of the lyrics column at `now_ms`.
    fn column_p(&self, now_ms: i64) -> f32 {
        let (start, from, to) = self.column_anim;
        let t = ((now_ms - start) as f32 / COLUMN_MS as f32).clamp(0.0, 1.0);
        from + (to - from) * (1.0 - (1.0 - t).powi(3))
    }

    fn track_key(info: &MediaInfo) -> String {
        format!("{}\u{1}{}\u{1}{}", info.title, info.artist, info.duration_ms / 1000)
    }

    /// Starts a lookup on a worker thread when lyrics are on and the track changed.
    fn request_lyrics(&mut self) {
        let Some(info) = self.info.clone().filter(|_| self.had_session) else { return };
        let key = Self::track_key(&info);
        if !self.lyrics_on || key == self.lyrics_track {
            return;
        }
        self.lyrics_track = key.clone();
        self.lyric_idx = None;
        self.lyric_anim_start = None;
        self.lyrics = LyricsState::Loading;
        let (slot, hwnd) = (self.slot.clone(), self.hwnd);
        std::thread::spawn(move || {
            let found = crate::lrclib::lookup(&info.title, &info.artist, &info.album, info.duration_ms);
            *slot.lock().unwrap() = Some((key, found));
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), crate::window::WM_APP_LYRICS, WPARAM(0), LPARAM(0));
            }
        });
    }

    fn playing(&self) -> bool {
        self.info.as_ref().is_some_and(|i| i.playing)
    }
}

fn fmt_time(ms: i64) -> String {
    let s = (ms.max(0) / 1000) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

fn triangle(c: &mut dyn Canvas, cx: f32, cy: f32, r: f32, right: bool, col: Color) {
    let d = if right { 1.0 } else { -1.0 };
    c.fill_polygon(&[(cx - d * r * 0.6, cy - r), (cx - d * r * 0.6, cy + r), (cx + d * r * 0.8, cy)], col);
}

impl Module for MusicModule {
    fn id(&self) -> &'static str {
        "music"
    }
    fn title(&self) -> &'static str {
        "Musik"
    }
    fn pill_left(&self) -> Option<PillItem> {
        None
    }
    fn pill_items(&self) -> Vec<PillItem> {
        if !self.had_session {
            return Vec::new();
        }
        let mut items = Vec::new();
        if let Some(id) = self.cover {
            items.push(PillItem::MiniCover(id));
        }
        items.push(PillItem::Equaliser { active: self.playing() });
        items
    }
    fn wide_pill(&self) -> bool {
        self.had_session
    }
    fn poll(&mut self, _now_ms: i64) -> bool {
        let latest = self.watcher.latest();
        let had = latest.as_ref().is_some_and(|i| !i.title.is_empty() || !i.artist.is_empty());
        let changed = had != self.had_session
            || match (&self.info, &latest) {
                (Some(a), Some(b)) => a.title != b.title || a.artist != b.artist || a.playing != b.playing || a.cover.as_ref().map(|c| c.serial) != b.cover.as_ref().map(|c| c.serial),
                (None, None) => false,
                _ => true,
            };
        self.had_session = had;
        self.info = latest;
        self.request_lyrics();
        let arrived = self.slot.lock().unwrap().take();
        let mut changed = changed;
        if let Some((key, found)) = arrived {
            if key == self.lyrics_track {
                self.lyrics = LyricsState::Ready(found);
                changed = true;
            }
        }
        changed
    }
    fn wants_wide_panel(&self) -> bool {
        self.lyrics_on && self.had_session
    }
    fn prepare(&mut self, c: &mut dyn Canvas) {
        let Some(info) = &self.info else { return };
        let key = info.cover.as_ref().map(|cv| cv.serial);
        if key != self.cover_key {
            self.cover = info.cover.as_ref().map(|cv| c.load_image(&cv.rgba, cv.width, cv.height));
            self.cover_key = key;
        }
    }
    fn draw_panel(&mut self, c: &mut dyn Canvas, area: Rect, now_ms: i64) {
        let Some(info) = self.info.clone().filter(|_| self.had_session) else {
            c.draw_text("Tidak ada musik", area, 13.0, 400, DIM, Align::Center);
            return;
        };
        let p = self.column_p(now_ms);
        let l = layout_p(area, p);
        match self.cover {
            Some(id) => c.draw_image(id, l.cover, 14.0),
            None => c.fill_round_rect(l.cover, 14.0, Color(232, 116, 79, 255)),
        }
        let title = fit_text(c, &info.title, 14.0, 650, l.title.w);
        c.draw_text(&title, l.title, 14.0, 650, WHITE, Align::Left);
        let sub = if info.app.is_empty() { info.artist.clone() } else { format!("{} · {}", info.artist, info.app) };
        let sub = fit_text(c, &sub, 12.0, 400, l.artist.w);
        c.draw_text(&sub, l.artist, 12.0, 400, DIM, Align::Left);

        let mut pos = info.position_at(now_ms);
        let shown = self.scrub.or(self.seek_hold.filter(|(_, until)| now_ms < *until).map(|(f, _)| f));
        if let (Some(f), true) = (shown, info.duration_ms > 0) {
            pos = (info.duration_ms as f32 * f) as i64;
        }
        c.draw_text(&fmt_time(pos), l.elapsed, 10.5, 400, DIM, Align::Left);
        if info.duration_ms > 0 {
            c.draw_text(&fmt_time(info.duration_ms), l.total, 10.5, 400, DIM, Align::Right);
        }
        let mix = |a: Color, b: Color| Color(lerp(a.0 as f32, b.0 as f32, p) as u8, lerp(a.1 as f32, b.1 as f32, p) as u8, lerp(a.2 as f32, b.2 as f32, p) as u8, 255);
        let (btn_bg, btn_fg) = (mix(TRACK, WHITE), mix(WHITE, Color(0, 0, 0, 255)));
        c.fill_round_rect(l.lyrics_btn, 9.0, btn_bg);
        c.draw_text("Lirik", l.lyrics_btn, 11.0, 600, btn_fg, Align::Center);
        if p > 0.01 {
            self.draw_lyrics(c, area, pos, now_ms, p, l.total.x + l.total.w + 12.0);
        }
        c.fill_round_rect(l.bar, 2.0, TRACK);
        if info.duration_ms > 0 {
            let f = (pos as f32 / info.duration_ms as f32).clamp(0.0, 1.0);
            c.fill_round_rect(Rect { w: (l.bar.w * f).max(4.0), ..l.bar }, 2.0, WHITE);
            if self.scrub.is_some() {
                c.fill_circle(l.bar.x + l.bar.w * f, l.bar.y + l.bar.h / 2.0, 5.0, WHITE);
            }
        }

        for (center, left) in [(l.prev, true), (l.next, false)] {
            let r = 5.5;
            triangle(c, center.0 + if left { 2.0 } else { -2.0 }, center.1, r, !left, WHITE);
            let bx = if left { center.0 - r - 2.5 } else { center.0 + r + 0.5 };
            c.fill_round_rect(Rect { x: bx, y: center.1 - r, w: 2.0, h: r * 2.0 }, 1.0, WHITE);
            let _ = SKIP_RADIUS;
        }
        c.fill_circle(l.play.0, l.play.1, PLAY_RADIUS, WHITE);
        let black = Color(0, 0, 0, 255);
        if info.playing {
            c.fill_round_rect(Rect { x: l.play.0 - 5.5, y: l.play.1 - 6.0, w: 4.0, h: 12.0 }, 1.0, black);
            c.fill_round_rect(Rect { x: l.play.0 + 1.5, y: l.play.1 - 6.0, w: 4.0, h: 12.0 }, 1.0, black);
        } else {
            triangle(c, l.play.0 + 1.0, l.play.1, 6.5, true, black);
        }
    }
    fn on_event(&mut self, e: &Event, area: Rect, now_ms: i64) -> Reaction {
        if !self.had_session {
            return Reaction::None;
        }
        // Players that cannot seek (Apple Music) get a plain progress bar.
        let duration = self.info.as_ref().filter(|i| i.can_seek).map_or(0, |i| i.duration_ms);
        match e {
            Event::PointerDrag(x, _) if self.scrub.is_some() => {
                self.scrub = Some(bar_fraction(area, self.lyrics_on, *x));
                return Reaction::Redraw;
            }
            Event::PointerUp(x, _) => {
                let Some(_) = self.scrub.take() else { return Reaction::None };
                let f = bar_fraction(area, self.lyrics_on, *x);
                if duration > 0 {
                    self.watcher.seek((duration as f32 * f) as i64);
                    self.seek_hold = Some((f, now_ms + 800));
                }
                return Reaction::Redraw;
            }
            _ => {}
        }
        let Event::PointerDown(x, y) = e else { return Reaction::None };
        match hit_for(area, self.lyrics_on, *x, *y) {
            Some(MusicHit::PlayPause) => self.watcher.play_pause(),
            Some(MusicHit::Prev) => self.watcher.previous(),
            Some(MusicHit::Next) => self.watcher.next(),
            // Press on the bar starts a scrub; the seek happens on release (click = press + release).
            Some(MusicHit::Seek(f)) => {
                if duration > 0 {
                    self.scrub = Some(f);
                }
            }
            Some(MusicHit::LyricsToggle) => {
                let from = self.column_p(now_ms);
                self.lyrics_on = !self.lyrics_on;
                self.column_anim = (now_ms, from, if self.lyrics_on { 1.0 } else { 0.0 });
                self.request_lyrics();
            }
            Some(MusicHit::LyricRow(row)) => {
                let LyricsState::Ready(Lyrics::Synced(lines)) = &self.lyrics else { return Reaction::None };
                let pos = self.info.as_ref().map(|i| i.position_at(now_ms)).unwrap_or(0);
                let [prev, cur, next] = window(lines.len(), current_index(lines, pos));
                if let Some(i) = [prev, cur, next][row] {
                    self.watcher.seek(lines[i].time_ms);
                }
            }
            None => return Reaction::None,
        }
        Reaction::Redraw
    }
    /// Animates the pill's equaliser and the scrub bar while playing.
    fn next_wake_ms(&self, now_ms: i64) -> Option<i64> {
        let now = now_ms.max(epoch_ms());
        if now - self.column_anim.0 < COLUMN_MS + 20 {
            return Some(now + 8); // the lyrics column is sliding
        }
        if !self.playing() {
            return None;
        }
        let mut wake = now + 120;
        if self.lyrics_on {
            if self.lyric_anim_start.is_some_and(|s| now - s < SCROLL_MS + 20) {
                wake = now + 8; // keep the scroll smooth
            } else if let (LyricsState::Ready(Lyrics::Synced(lines)), Some(info)) = (&self.lyrics, &self.info) {
                // Wake exactly when the next line starts.
                let pos = info.position_at(now);
                if let Some(next) = lines.iter().find(|l| l.time_ms > pos) {
                    wake = wake.min(now + (next.time_ms - pos).max(1));
                }
            }
        }
        Some(wake)
    }
}

/// How long the lyrics column takes to slide in or out.
const COLUMN_MS: i64 = 320;

/// How long a lyric line takes to scroll into place.
const SCROLL_MS: i64 = 380;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl MusicModule {
    fn draw_lyrics(&mut self, c: &mut dyn Canvas, area: Rect, pos: i64, now_ms: i64, p: f32, left_edge: f32) {
        let mut rows = lyric_rows(area);
        for r in rows.iter_mut() {
            // Sits right after the player column, so it can never overlap it while the panel
            // is still widening; the page clip reveals it as the panel grows.
            r.x = left_edge + (1.0 - p) * 18.0;
        }
        let fade = |col: Color| Color(col.0, col.1, col.2, (col.3 as f32 * p) as u8);
        let message = |c: &mut dyn Canvas, text: &str| c.draw_text(text, rows[1], 12.0, 400, fade(DIM), Align::Left);

        // Track the current synced line; start a scroll when it advances by exactly one.
        let idx = match &self.lyrics {
            LyricsState::Ready(Lyrics::Synced(lines)) => current_index(lines, pos),
            _ => None,
        };
        if idx != self.lyric_idx {
            self.lyric_anim_start = match (self.lyric_idx, idx) {
                (Some(a), Some(b)) if b == a + 1 => Some(now_ms),
                _ => None,
            };
            self.lyric_idx = idx;
        }
        let t = self.lyric_anim_start.map_or(1.0, |s| ((now_ms - s) as f32 / SCROLL_MS as f32).clamp(0.0, 1.0));
        let t = 1.0 - (1.0 - t).powi(3); // ease out

        let lyrics = match &self.lyrics {
            LyricsState::Loading => return message(c, "Mencari lirik…"),
            LyricsState::Ready(l) => l,
        };
        if let (Lyrics::Synced(lines), Some(i)) = (lyrics, idx) {
            // Line j sits at slot (j - i + 1) + (1 - t): slot 1 is the big middle row.
            let (mid, step) = (rows[1].y + rows[1].h / 2.0, 32.0);
            for j in i.saturating_sub(2)..=(i + 1).min(lines.len() - 1) {
                let slot = (j as f32 - i as f32 + 1.0) + (1.0 - t);
                let dist = (slot - 1.0).abs();
                let emphasis = (1.0 - dist).clamp(0.0, 1.0);
                let alpha = (2.0 - dist).clamp(0.0, 1.0);
                if alpha <= 0.0 {
                    continue;
                }
                let size = lerp(12.0, 15.0, emphasis);
                let col = Color(
                    lerp(DIM.0 as f32, WHITE.0 as f32, emphasis) as u8,
                    lerp(DIM.1 as f32, WHITE.1 as f32, emphasis) as u8,
                    lerp(DIM.2 as f32, WHITE.2 as f32, emphasis) as u8,
                    (alpha * p * 255.0) as u8,
                );
                let weight = if emphasis > 0.5 { 650 } else { 400 };
                let cy = mid + (slot - 1.0) * step;
                let r = Rect { x: rows[1].x, y: cy - 14.0, w: rows[1].w, h: 28.0 };
                let text = fit_text(c, &lines[j].text, size, weight, r.w);
                c.draw_text(&text, r, size, weight, col, Align::Left);
            }
            return;
        }
        let (texts, index): (Vec<&str>, Option<usize>) = match lyrics {
            Lyrics::None => return message(c, "Tidak ada lirik"),
            Lyrics::Synced(lines) => (lines.iter().map(|l| l.text.as_str()).collect(), None),
            Lyrics::Plain(lines) => (lines.iter().map(String::as_str).collect(), None),
        };
        for (slot, line) in window(texts.len(), index).into_iter().enumerate() {
            let Some(i) = line else { continue };
            let (size, weight, color) = if slot == 1 { (15.0, 650, fade(WHITE)) } else { (12.0, 400, fade(DIM)) };
            let text = fit_text(c, texts[i], size, weight, rows[slot].w);
            c.draw_text(&text, rows[slot], size, weight, color, Align::Left);
        }
    }
}
