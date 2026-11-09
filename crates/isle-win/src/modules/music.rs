// SPDX-License-Identifier: GPL-3.0-or-later
//! Music: what is playing (SMTC), with transport controls and a scrub bar.

use crate::media_win::MediaWatcher;
use crate::system::epoch_ms;
use isle_logic::canvas::{Align, Canvas, Color, ImageId, Rect, fit_text};
use isle_logic::media::MediaInfo;
use isle_logic::module::{Event, Module, PillItem, Reaction};
use isle_logic::lyrics::{Lyrics, current_index, window};
use isle_logic::music_layout::{MusicHit, PLAY_RADIUS, SKIP_RADIUS, hit_for, layout_for, lyric_rows};
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
    lyrics: LyricsState,
    /// Track the lyrics were requested for.
    lyrics_track: String,
    slot: Slot,
    watcher: MediaWatcher,
    info: Option<MediaInfo>,
    cover: Option<ImageId>,
    cover_key: Option<(String, String, usize)>,
    /// Shown in the pill's left slot while a session exists.
    had_session: bool,
}

impl MusicModule {
    pub fn new(watcher: MediaWatcher, hwnd: isize) -> Self {
        Self { hwnd, lyrics_on: false, lyrics: LyricsState::Loading, lyrics_track: String::new(), slot: Arc::default(), watcher, info: None, cover: None, cover_key: None, had_session: false }
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
                (Some(a), Some(b)) => a.title != b.title || a.artist != b.artist || a.playing != b.playing || a.cover.is_some() != b.cover.is_some(),
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
        let key = info.cover.as_ref().map(|cv| (info.title.clone(), info.artist.clone(), cv.rgba.len()));
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
        let lyrics_on = self.lyrics_on;
        let l = layout_for(area, lyrics_on);
        match self.cover {
            Some(id) => c.draw_image(id, l.cover, 14.0),
            None => c.fill_round_rect(l.cover, 14.0, Color(232, 116, 79, 255)),
        }
        let title = fit_text(c, &info.title, 14.0, 650, l.title.w);
        c.draw_text(&title, l.title, 14.0, 650, WHITE, Align::Left);
        let sub = if info.app.is_empty() { info.artist.clone() } else { format!("{} · {}", info.artist, info.app) };
        let sub = fit_text(c, &sub, 12.0, 400, l.artist.w);
        c.draw_text(&sub, l.artist, 12.0, 400, DIM, Align::Left);

        let pos = info.position_at(now_ms);
        c.draw_text(&fmt_time(pos), l.elapsed, 10.5, 400, DIM, Align::Left);
        if info.duration_ms > 0 {
            c.draw_text(&fmt_time(info.duration_ms), l.total, 10.5, 400, DIM, Align::Right);
        }
        let (btn_bg, btn_fg) = if lyrics_on { (WHITE, Color(0, 0, 0, 255)) } else { (TRACK, WHITE) };
        c.fill_round_rect(l.lyrics_btn, 9.0, btn_bg);
        c.draw_text("Lirik", l.lyrics_btn, 11.0, 600, btn_fg, Align::Center);
        if lyrics_on {
            self.draw_lyrics(c, area, pos);
        }
        c.fill_round_rect(l.bar, 2.0, TRACK);
        if info.duration_ms > 0 {
            let f = (pos as f32 / info.duration_ms as f32).clamp(0.0, 1.0);
            c.fill_round_rect(Rect { w: (l.bar.w * f).max(4.0), ..l.bar }, 2.0, WHITE);
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
        let Event::PointerDown(x, y) = e else { return Reaction::None };
        if !self.had_session {
            return Reaction::None;
        }
        match hit_for(area, self.lyrics_on, *x, *y) {
            Some(MusicHit::PlayPause) => self.watcher.play_pause(),
            Some(MusicHit::Prev) => self.watcher.previous(),
            Some(MusicHit::Next) => self.watcher.next(),
            Some(MusicHit::Seek(f)) => {
                if let Some(info) = &self.info {
                    if info.duration_ms > 0 {
                        self.watcher.seek((info.duration_ms as f32 * f) as i64);
                    }
                }
            }
            Some(MusicHit::LyricsToggle) => {
                self.lyrics_on = !self.lyrics_on;
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
        self.playing().then(|| now_ms.max(epoch_ms()) + 120)
    }
}

impl MusicModule {
    fn draw_lyrics(&self, c: &mut dyn Canvas, area: Rect, pos: i64) {
        let rows = lyric_rows(area);
        let message = |c: &mut dyn Canvas, text: &str| c.draw_text(text, rows[1], 12.0, 400, DIM, Align::Left);
        let lyrics = match &self.lyrics {
            LyricsState::Loading => return message(c, "Mencari lirik…"),
            LyricsState::Ready(l) => l,
        };
        let (texts, index): (Vec<&str>, Option<usize>) = match lyrics {
            Lyrics::None => return message(c, "Tidak ada lirik"),
            Lyrics::Synced(lines) => (lines.iter().map(|l| l.text.as_str()).collect(), current_index(lines, pos)),
            Lyrics::Plain(lines) => (lines.iter().map(String::as_str).collect(), None),
        };
        for (slot, line) in window(texts.len(), index).into_iter().enumerate() {
            let Some(i) = line else { continue };
            let (size, weight, color) = if slot == 1 { (15.0, 650, WHITE) } else { (12.0, 400, DIM) };
            let text = fit_text(c, texts[i], size, weight, rows[slot].w);
            c.draw_text(&text, rows[slot], size, weight, color, Align::Left);
        }
    }
}
