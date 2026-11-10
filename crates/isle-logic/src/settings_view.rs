// SPDX-License-Identifier: GPL-3.0-or-later
//! The settings window's content: layout, painting through `Canvas`, and input. Pure logic, so
//! it is tested here; the Windows side only owns the window, the glass behind it and the saving.
//!
//! Coordinates in events are DIPs relative to the window's content rectangle (`W` x `H`).
//! `paint` draws at an offset (the window keeps a margin around the content for the shadow).

use crate::canvas::{Align, Canvas, Color, Rect, fit_text};
use crate::module::{EditKey, Event};
use crate::settings::{Settings, WorldClock};
use crate::text_edit::{Composition, TextEdit};

pub const W: f32 = 860.0;
pub const H: f32 = 540.0;
/// Space around the content for the soft shadow.
pub const MARGIN: f32 = 40.0;
const RADIUS: f32 = 16.0;
const TITLE_H: f32 = 44.0;
const NAV_W: f32 = 196.0;
const FOOT_H: f32 = 40.0;
const PAD: f32 = 22.0;
const ROW_H: f32 = 58.0;
const ORDER_H: f32 = 31.0;
const MAX_CITIES: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Umum,
    Tampilan,
    Halaman,
    Musik,
    Pomodoro,
    Jam,
    Pembaruan,
    Tentang,
}

const SECTIONS: [(Section, &str); 8] = [
    (Section::Umum, "Umum"),
    (Section::Tampilan, "Tampilan"),
    (Section::Halaman, "Halaman"),
    (Section::Musik, "Musik"),
    (Section::Pomodoro, "Pomodoro"),
    (Section::Jam, "Jam dunia"),
    (Section::Pembaruan, "Pembaruan"),
    (Section::Tentang, "Tentang"),
];

/// One editable setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Autostart,
    Material,
    PillW,
    PillH,
    RestAfter,
    Lead,
    Focus,
    Short,
    Long,
    Sessions,
    AutoBreaks,
    AutoFocus,
    AutoUpdate,
    Hours,
    Repo,
    Token,
    City(usize),
    Zone(usize),
    Order,
}

/// Something the window must do that is not a stored value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Close,
    CheckUpdate,
    ClearSkipped,
    OpenData,
    OpenToml,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Ctl {
    Close,
    Nav(usize),
    Toggle(Key),
    Choice(Key, usize),
    Slider(Key, f32, f32, f32),
    Step(Key, f32, f32, f32),
    Text(Key),
    Button(Act),
    OrderRow(usize),
    OrderUp(usize),
    OrderDown(usize),
    OrderReset,
    PomoReset,
    AddCity,
    DelCity(usize),
}

struct Hit {
    r: Rect,
    ctl: Ctl,
    /// Only inside the scrolling pane.
    pane: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub redraw: bool,
    pub changed: Vec<Key>,
    pub acts: Vec<Act>,
    pub copy: Option<String>,
}

struct Palette {
    text: Color,
    mute: Color,
    line: Color,
    ctl: Color,
    ctl_h: Color,
    sel: Color,
    field: Color,
}

const ACCENT: Color = Color(217, 119, 87, 255);

fn palette(light: bool) -> Palette {
    if light {
        Palette { text: Color(20, 22, 26, 255), mute: Color(20, 22, 26, 150), line: Color(20, 22, 26, 26), ctl: Color(20, 22, 26, 15), ctl_h: Color(20, 22, 26, 30), sel: Color(20, 22, 26, 24), field: Color(255, 255, 255, 150) }
    } else {
        Palette { text: Color(241, 243, 247, 255), mute: Color(241, 243, 247, 178), line: Color(255, 255, 255, 26), ctl: Color(255, 255, 255, 20), ctl_h: Color(255, 255, 255, 38), sel: Color(255, 255, 255, 34), field: Color(0, 0, 0, 72) }
    }
}

pub struct SettingsView {
    /// The working copy: what the controls show and edit.
    pub settings: Settings,
    /// Pages that can be reordered, as (id, title), in the current order.
    order: Vec<(String, String)>,
    section: usize,
    hits: Vec<Hit>,
    hover: Option<usize>,
    scroll: f32,
    content_h: f32,
    focus: Option<Key>,
    edit: TextEdit,
    drag_slider: Option<usize>,
    drag_row: Option<usize>,
    version: String,
    restart: bool,
    /// The user changed the order, so it is written to the file (otherwise `page_order` stays as is).
    order_touched: bool,
    origin: (f32, f32),
    /// Where the mouse pressed, to tell a click on a text field from a drag.
    pub caret_at: Option<(f32, f32)>,
}

impl SettingsView {
    pub fn new(settings: Settings, pages: Vec<(String, String)>, version: &str) -> Self {
        Self {
            settings,
            order: pages,
            section: 0,
            hits: Vec::new(),
            hover: None,
            scroll: 0.0,
            content_h: 0.0,
            focus: None,
            edit: TextEdit::new(160),
            drag_slider: None,
            drag_row: None,
            version: version.to_string(),
            restart: false,
            order_touched: false,
            origin: (0.0, 0.0),
            caret_at: None,
        }
    }

    pub fn light(&self) -> bool {
        self.settings.glass && self.settings.glass_light
    }

    pub fn wants_text(&self) -> bool {
        self.focus.is_some()
    }

    /// Page ids in the order the user chose.
    pub fn page_order(&self) -> Vec<String> {
        self.order.iter().map(|(id, _)| id.clone()).collect()
    }

    /// Copies every setting this window edits into `s` (leaving the rest of `s` alone).
    pub fn apply_to(&self, s: &mut Settings) {
        let v = &self.settings;
        s.autostart = v.autostart;
        s.glass = v.glass;
        s.glass_light = v.glass_light;
        s.pill_width = v.pill_width;
        s.pill_height = v.pill_height;
        s.rest_after_secs = v.rest_after_secs;
        s.lyrics_lead_ms = v.lyrics_lead_ms;
        s.pomodoro = v.pomodoro.clone();
        s.world_clocks = v.world_clocks.clone();
        s.auto_update = v.auto_update;
        s.update_check_hours = v.update_check_hours;
        s.update_repo = v.update_repo.clone();
        s.update_token = v.update_token.clone();
        if self.order_touched {
            s.page_order = self.page_order();
        }
    }

    /// Forgets a skipped version (after "Hapus").
    pub fn set_skipped(&mut self, v: &str) {
        self.settings.skipped_version = v.to_string();
    }

    // ---- values ----

    fn num(&self, k: Key) -> f32 {
        let s = &self.settings;
        match k {
            Key::PillW => s.pill_width,
            Key::PillH => s.pill_height,
            Key::RestAfter => s.rest_after_secs as f32,
            Key::Lead => s.lyrics_lead_ms as f32,
            Key::Focus => s.pomodoro.focus_minutes as f32,
            Key::Short => s.pomodoro.short_break_minutes as f32,
            Key::Long => s.pomodoro.long_break_minutes as f32,
            Key::Sessions => s.pomodoro.sessions_before_long_break as f32,
            Key::Hours => s.update_check_hours as f32,
            _ => 0.0,
        }
    }

    fn set_num(&mut self, k: Key, v: f32) {
        let s = &mut self.settings;
        match k {
            Key::PillW => s.pill_width = v,
            Key::PillH => s.pill_height = v,
            Key::RestAfter => s.rest_after_secs = v as i64,
            Key::Lead => s.lyrics_lead_ms = v as i64,
            Key::Focus => s.pomodoro.focus_minutes = v as u32,
            Key::Short => s.pomodoro.short_break_minutes = v as u32,
            Key::Long => s.pomodoro.long_break_minutes = v as u32,
            Key::Sessions => s.pomodoro.sessions_before_long_break = v as u32,
            Key::Hours => s.update_check_hours = v as u32,
            _ => {}
        }
    }

    fn flag(&self, k: Key) -> bool {
        let s = &self.settings;
        match k {
            Key::Autostart => s.autostart,
            Key::AutoBreaks => s.pomodoro.auto_start_breaks,
            Key::AutoFocus => s.pomodoro.auto_start_focus,
            Key::AutoUpdate => s.auto_update,
            _ => false,
        }
    }

    fn set_flag(&mut self, k: Key, v: bool) {
        let s = &mut self.settings;
        match k {
            Key::Autostart => s.autostart = v,
            Key::AutoBreaks => s.pomodoro.auto_start_breaks = v,
            Key::AutoFocus => s.pomodoro.auto_start_focus = v,
            Key::AutoUpdate => s.auto_update = v,
            _ => {}
        }
    }

    fn material(&self) -> usize {
        match (self.settings.glass, self.settings.glass_light) {
            (false, _) => 0,
            (true, false) => 1,
            (true, true) => 2,
        }
    }

    fn text_of(&self, k: Key) -> String {
        let s = &self.settings;
        match k {
            Key::Repo => s.update_repo.clone(),
            Key::Token => s.update_token.clone(),
            Key::City(i) => s.world_clocks.get(i).map(|c| c.label.clone()).unwrap_or_default(),
            Key::Zone(i) => s.world_clocks.get(i).map(|c| c.zone.clone()).unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn set_text(&mut self, k: Key, v: &str) {
        let s = &mut self.settings;
        match k {
            Key::Repo => s.update_repo = v.to_string(),
            Key::Token => s.update_token = v.to_string(),
            Key::City(i) => {
                if let Some(c) = s.world_clocks.get_mut(i) {
                    c.label = v.to_string();
                }
            }
            Key::Zone(i) => {
                if let Some(c) = s.world_clocks.get_mut(i) {
                    c.zone = v.to_string();
                }
            }
            _ => {}
        }
    }

    fn needs_restart(k: Key) -> bool {
        matches!(k, Key::Lead | Key::Focus | Key::Short | Key::Long | Key::Sessions | Key::AutoBreaks | Key::AutoFocus | Key::City(_) | Key::Zone(_))
    }

    // ---- painting ----

    pub fn paint(&mut self, c: &mut dyn Canvas, ox: f32, oy: f32) {
        self.origin = (ox, oy);
        let p = palette(self.light());
        let light = self.light();
        let shape = Rect { x: ox, y: oy, w: W, h: H };
        shadow(c, shape, light);
        c.fill_round_rect(shape, RADIUS, if light { Color(246, 247, 250, 128) } else { Color(18, 20, 26, 128) });
        c.push_clip(shape, RADIUS);
        // A sheen that fades down the window, then thin lit edges.
        c.fill_round_rect(shape, 0.0, Color(255, 255, 255, if light { 60 } else { 5 }));
        for i in 0..25 {
            let bottom = H * 0.6 * (i + 1) as f32 / 25.0;
            c.fill_round_rect(Rect { x: ox, y: oy, w: W, h: bottom }, 0.0, Color(255, 255, 255, if light { 3 } else { 1 }));
        }
        c.pop_clip();
        c.push_clip_out(Rect { x: ox + 1.0, y: oy + 1.0, w: W - 2.0, h: H - 2.0 }, RADIUS - 1.0);
        c.fill_round_rect(shape, RADIUS, Color(255, 255, 255, if light { 200 } else { 40 }));
        c.pop_clip();

        let mut hits: Vec<Hit> = Vec::new();
        let hover = self.hover;

        // title bar
        c.fill_round_rect(Rect { x: ox, y: oy + TITLE_H - 1.0, w: W, h: 1.0 }, 0.0, p.line);
        c.fill_round_rect(Rect { x: ox + 16.0, y: oy + 13.0, w: 18.0, h: 18.0 }, 5.0, ACCENT);
        c.fill_round_rect(Rect { x: ox + 21.0, y: oy + 13.0, w: 8.0, h: 5.0 }, 2.0, Color(20, 22, 26, 255));
        text(c, "Pengaturan Buginotch", ox + 44.0, oy, 300.0, TITLE_H, 13.0, 500, p.text, Align::Left);
        let close = Rect { x: ox + W - 42.0, y: oy + 10.0, w: 30.0, h: 24.0 };
        if hover.is_some_and(|i| i == hits.len()) {
            c.fill_round_rect(close, 6.0, p.ctl_h);
        }
        text(c, "✕", close.x, close.y, close.w, close.h, 12.0, 400, p.mute, Align::Center);
        hits.push(Hit { r: close, ctl: Ctl::Close, pane: false });

        // navigation
        c.fill_round_rect(Rect { x: ox + NAV_W - 1.0, y: oy + TITLE_H, w: 1.0, h: H - TITLE_H - FOOT_H }, 0.0, p.line);
        for (i, (_, name)) in SECTIONS.iter().enumerate() {
            let r = Rect { x: ox + 8.0, y: oy + TITLE_H + 10.0 + i as f32 * 36.0, w: NAV_W - 17.0, h: 34.0 };
            let selected = i == self.section;
            if selected {
                c.fill_round_rect(r, 8.0, p.sel);
            } else if hover.is_some_and(|h| h == hits.len()) {
                c.fill_round_rect(r, 8.0, p.ctl);
            }
            c.fill_round_rect(Rect { x: r.x + 12.0, y: r.y + 12.0, w: 10.0, h: 10.0 }, 3.0, if selected { ACCENT } else { p.mute });
            text(c, name, r.x + 32.0, r.y, r.w - 36.0, r.h, 13.0, if selected { 500 } else { 400 }, if selected { p.text } else { p.mute }, Align::Left);
            hits.push(Hit { r, ctl: Ctl::Nav(i), pane: false });
        }

        // footer
        c.fill_round_rect(Rect { x: ox, y: oy + H - FOOT_H, w: W, h: 1.0 }, 0.0, p.line);
        c.fill_circle(ox + 24.0, oy + H - FOOT_H / 2.0, 3.5, Color(95, 208, 138, 255));
        let note = if self.restart { "Tersimpan. Perubahan Pomodoro, lirik dan jam dunia berlaku setelah Buginotch dimulai ulang." } else { "Tersimpan otomatis ke settings.toml" };
        text(c, note, ox + 36.0, oy + H - FOOT_H, W - 60.0, FOOT_H, 12.0, 400, p.mute, Align::Left);

        // the scrolling pane
        let pane = Rect { x: ox + NAV_W, y: oy + TITLE_H, w: W - NAV_W, h: H - TITLE_H - FOOT_H };
        c.push_clip(pane, 0.0);
        let mut y = pane.y + 16.0 - self.scroll;
        let x = pane.x + PAD;
        let w = pane.w - PAD * 2.0;
        let start_y = y;
        let (sec, title) = SECTIONS[self.section];
        text(c, title, x, y, w, 26.0, 18.0, 600, p.text, Align::Left);
        y += 28.0;
        let sub = match sec {
            Section::Umum => "Perilaku dasar aplikasi.",
            Section::Tampilan => "Bentuk dan bahan notch.",
            Section::Halaman => "Seret baris atau pakai panah. Halaman Pembaruan muncul sendiri saat ada pembaruan.",
            Section::Musik => "Penyesuaian lirik di notch.",
            Section::Pomodoro => "Durasi sesi pada halaman Timer.",
            Section::Jam => "Kota-kota pada halaman Widget (tiga pertama tampil).",
            Section::Pembaruan => "Tidak ada yang terpasang tanpa klik Anda.",
            Section::Tentang => "",
        };
        text(c, sub, x, y, w, 18.0, 12.0, 400, p.mute, Align::Left);
        y += 28.0;

        let mut cx = Cx { c, p: &p, hits: &mut hits, hover, x, w, light };
        match sec {
            Section::Umum => {
                y = self.group(&mut cx, y, &[
                    Row { name: "Mulai dengan Windows", desc: "Buginotch menyala setelah masuk ke Windows.", key: "autostart", ctl: RowCtl::Toggle(Key::Autostart) },
                    Row { name: "Mata berkedip saat diam", desc: "Detik tanpa aktivitas sebelum jam berganti mata. 0 mematikannya.", key: "rest_after_secs", ctl: RowCtl::Slider(Key::RestAfter, 0.0, 300.0, 5.0, "dtk") },
                ]);
            }
            Section::Tampilan => {
                y = self.preview(&mut cx, y);
                y = self.group(&mut cx, y, &[
                    Row { name: "Bahan notch", desc: "Hitam pekat, atau kaca yang menembus latar (Windows 11).", key: "glass, glass_light", ctl: RowCtl::Choice(Key::Material, &["Hitam", "Kaca gelap", "Kaca terang"]) },
                    Row { name: "Lebar pill", desc: "Ukuran notch saat tertutup, piksel logis.", key: "pill_width", ctl: RowCtl::Slider(Key::PillW, 80.0, 180.0, 2.0, "") },
                    Row { name: "Tinggi pill", desc: "", key: "pill_height", ctl: RowCtl::Slider(Key::PillH, 22.0, 40.0, 1.0, "") },
                ]);
            }
            Section::Halaman => {
                y = self.order_list(&mut cx, y);
                let r = Rect { x, y: y + 4.0, w: 190.0, h: 30.0 };
                button(&mut cx, r, "Kembalikan urutan awal", Ctl::OrderReset, false);
                y += 40.0;
            }
            Section::Musik => {
                y = self.group(&mut cx, y, &[Row { name: "Lirik lebih cepat", desc: "Pemutar melapor terlambat; naikkan jika lirik masih telat.", key: "lyrics_lead_ms", ctl: RowCtl::Slider(Key::Lead, 0.0, 1500.0, 50.0, "ms") }]);
            }
            Section::Pomodoro => {
                y = self.group(&mut cx, y, &[
                    Row { name: "Fokus", desc: "Menit.", key: "pomodoro.focus_minutes", ctl: RowCtl::Step(Key::Focus, 1.0, 180.0, 1.0) },
                    Row { name: "Istirahat pendek", desc: "Menit.", key: "pomodoro.short_break_minutes", ctl: RowCtl::Step(Key::Short, 1.0, 60.0, 1.0) },
                    Row { name: "Istirahat panjang", desc: "Menit.", key: "pomodoro.long_break_minutes", ctl: RowCtl::Step(Key::Long, 1.0, 90.0, 1.0) },
                    Row { name: "Sesi sebelum istirahat panjang", desc: "", key: "pomodoro.sessions_before_long_break", ctl: RowCtl::Step(Key::Sessions, 2.0, 12.0, 1.0) },
                    Row { name: "Istirahat mulai sendiri", desc: "Begitu sesi fokus selesai.", key: "pomodoro.auto_start_breaks", ctl: RowCtl::Toggle(Key::AutoBreaks) },
                    Row { name: "Fokus berikutnya mulai sendiri", desc: "Begitu istirahat selesai.", key: "pomodoro.auto_start_focus", ctl: RowCtl::Toggle(Key::AutoFocus) },
                ]);
                button(&mut cx, Rect { x, y: y + 4.0, w: 170.0, h: 30.0 }, "Kembali ke 25 / 5 / 15", Ctl::PomoReset, false);
                y += 40.0;
            }
            Section::Jam => {
                y = self.cities(&mut cx, y);
            }
            Section::Pembaruan => {
                let skipped = if self.settings.skipped_version.is_empty() { "Tidak ada.".to_string() } else { format!("Dilewati: {}", self.settings.skipped_version) };
                y = self.group(&mut cx, y, &[
                    Row { name: "Perbarui otomatis", desc: "Periksa rilis baru secara berkala.", key: "auto_update", ctl: RowCtl::Toggle(Key::AutoUpdate) },
                    Row { name: "Jarak pemeriksaan", desc: "Jam antar pemeriksaan.", key: "update_check_hours", ctl: RowCtl::Step(Key::Hours, 1.0, 168.0, 1.0) },
                    Row { name: "Repositori rilis", desc: "owner/nama", key: "update_repo", ctl: RowCtl::Text(Key::Repo, false) },
                    Row { name: "Token baca-saja", desc: "Untuk repositori private. Hanya disimpan di komputer ini.", key: "update_token", ctl: RowCtl::Text(Key::Token, true) },
                    Row { name: "Versi yang dilewati", desc: &skipped, key: "skipped_version", ctl: RowCtl::Button("Hapus", Act::ClearSkipped) },
                ]);
                button(&mut cx, Rect { x, y: y + 4.0, w: 150.0, h: 30.0 }, "Periksa sekarang", Ctl::Button(Act::CheckUpdate), true);
                y += 40.0;
            }
            Section::Tentang => {
                let g = Rect { x, y, w, h: 96.0 };
                cx.c.fill_round_rect(g, 12.0, p.ctl);
                cx.c.fill_round_rect(Rect { x: x + 16.0, y: y + 16.0, w: 64.0, h: 64.0 }, 14.0, Color(255, 255, 255, 255));
                cx.c.fill_round_rect(Rect { x: x + 35.0, y: y + 16.0, w: 26.0, h: 14.0 }, 6.0, Color(20, 22, 26, 255));
                cx.c.fill_round_rect(Rect { x: x + 30.0, y: y + 44.0, w: 36.0, h: 18.0 }, 3.0, ACCENT);
                text(cx.c, &format!("Buginotch  {}", self.version), x + 96.0, y + 16.0, w - 110.0, 24.0, 16.0, 600, p.text, Align::Left);
                text(cx.c, "Notch bergaya pulau untuk Windows 11. Sebagian turunan dari Isle (GPL-3.0-or-later).", x + 96.0, y + 42.0, w - 110.0, 18.0, 12.0, 400, p.mute, Align::Left);
                text(cx.c, "Data di %APPDATA%\\IsleWin", x + 96.0, y + 62.0, w - 110.0, 18.0, 12.0, 400, p.mute, Align::Left);
                y += 108.0;
                button(&mut cx, Rect { x, y, w: 140.0, h: 30.0 }, "Buka folder data", Ctl::Button(Act::OpenData), false);
                button(&mut cx, Rect { x: x + 150.0, y, w: 150.0, h: 30.0 }, "Buka settings.toml", Ctl::Button(Act::OpenToml), false);
                y += 40.0;
            }
        }
        c.pop_clip();
        self.content_h = y - start_y + 32.0;
        let max = (self.content_h - pane.h).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
        if max > 0.0 {
            let track = pane.h - 16.0;
            let thumb = (pane.h / self.content_h * track).max(24.0);
            let top = pane.y + 8.0 + self.scroll / max * (track - thumb);
            c.fill_round_rect(Rect { x: pane.x + pane.w - 8.0, y: top, w: 3.0, h: thumb }, 1.5, p.mute);
        }
        // Hit rectangles of pane controls were laid out in scrolled coordinates already; drop
        // those that scrolled out of the pane so they cannot be clicked through the title bar.
        self.hits = hits;
        let pane_rect = pane;
        self.hits.retain(|h| !h.pane || (h.r.y + h.r.h > pane_rect.y && h.r.y < pane_rect.y + pane_rect.h));
        if self.hover.is_some_and(|i| i >= self.hits.len()) {
            self.hover = None;
        }
        // Focused text field: caret (drawn after the field so it sits on top).
        self.caret_at = None;
    }

    fn group(&mut self, cx: &mut Cx, y: f32, rows: &[Row]) -> f32 {
        let h = rows.iter().map(|r| row_h(r)).sum::<f32>();
        cx.c.fill_round_rect(Rect { x: cx.x, y, w: cx.w, h }, 12.0, cx.p.ctl);
        let mut ry = y;
        for (i, r) in rows.iter().enumerate() {
            let rh = row_h(r);
            if i > 0 {
                cx.c.fill_round_rect(Rect { x: cx.x, y: ry, w: cx.w, h: 1.0 }, 0.0, cx.p.line);
            }
            let ctl_w = self.control_width(cx, r);
            let label_w = cx.w - 28.0 - ctl_w - 16.0;
            text(cx.c, r.name, cx.x + 14.0, ry + 9.0, label_w, 18.0, 13.0, 500, cx.p.text, Align::Left);
            let mut ty = ry + 27.0;
            if !r.desc.is_empty() {
                let d = fit_text(cx.c, r.desc, 12.0, 400, label_w);
                text(cx.c, &d, cx.x + 14.0, ty, label_w, 16.0, 12.0, 400, cx.p.mute, Align::Left);
                ty += 15.0;
            }
            text(cx.c, r.key, cx.x + 14.0, ty, label_w, 13.0, 10.0, 400, Color(cx.p.mute.0, cx.p.mute.1, cx.p.mute.2, cx.p.mute.3 / 2), Align::Left);
            let right = cx.x + cx.w - 14.0;
            let mid = ry + rh / 2.0;
            self.draw_control(cx, r, right, mid, ctl_w);
            ry += rh;
        }
        y + h + 12.0
    }

    fn control_width(&self, cx: &mut Cx, r: &Row) -> f32 {
        match &r.ctl {
            RowCtl::Toggle(_) => 38.0,
            RowCtl::Choice(_, opts) => opts.iter().map(|o| cx.c.measure_text(o, 12.5, 400) + 24.0).sum::<f32>() + 6.0,
            RowCtl::Slider(..) => 150.0 + 64.0,
            RowCtl::Step(..) => 26.0 * 2.0 + 48.0,
            RowCtl::Text(..) => 200.0,
            RowCtl::Button(..) => 72.0,
        }
    }

    fn draw_control(&mut self, cx: &mut Cx, r: &Row, right: f32, mid: f32, w: f32) {
        let p = cx.p;
        match &r.ctl {
            RowCtl::Toggle(k) => {
                let on = self.flag(*k);
                let t = Rect { x: right - 38.0, y: mid - 11.0, w: 38.0, h: 22.0 };
                cx.c.fill_round_rect(t, 11.0, if on { ACCENT } else { p.ctl_h });
                let kx = if on { t.x + 19.0 } else { t.x + 3.0 };
                cx.c.fill_circle(kx + 8.0, mid, 8.0, if on { Color(255, 255, 255, 255) } else { Color(p.text.0, p.text.1, p.text.2, 210) });
                cx.hit(Rect { x: t.x - 4.0, y: t.y - 4.0, w: t.w + 8.0, h: t.h + 8.0 }, Ctl::Toggle(*k));
            }
            RowCtl::Choice(k, opts) => {
                let sel = self.material();
                let bar = Rect { x: right - w, y: mid - 15.0, w, h: 30.0 };
                cx.c.fill_round_rect(bar, 9.0, p.field);
                let mut x = bar.x + 3.0;
                for (i, o) in opts.iter().enumerate() {
                    let ow = cx.c.measure_text(o, 12.5, 400) + 24.0;
                    let cell = Rect { x, y: bar.y + 3.0, w: ow, h: 24.0 };
                    if i == sel {
                        cx.c.fill_round_rect(cell, 7.0, p.sel);
                    }
                    text(cx.c, o, cell.x, cell.y, cell.w, cell.h, 12.5, if i == sel { 500 } else { 400 }, if i == sel { p.text } else { p.mute }, Align::Center);
                    cx.hit(cell, Ctl::Choice(*k, i));
                    x += ow;
                }
            }
            RowCtl::Slider(k, min, max, step, unit) => {
                let v = self.num(*k);
                let track = Rect { x: right - 150.0, y: mid - 2.0, w: 150.0, h: 4.0 };
                cx.c.fill_round_rect(track, 2.0, p.ctl_h);
                let f = ((v - min) / (max - min)).clamp(0.0, 1.0);
                cx.c.fill_round_rect(Rect { w: track.w * f, ..track }, 2.0, ACCENT);
                cx.c.fill_circle(track.x + track.w * f, mid, 8.0, Color(255, 255, 255, 255));
                let label = if *k == Key::RestAfter && v == 0.0 { "mati".to_string() } else if unit.is_empty() { format!("{}", v as i64) } else { format!("{} {}", v as i64, unit) };
                text(cx.c, &label, track.x - 66.0, mid - 10.0, 58.0, 20.0, 12.0, 400, p.mute, Align::Right);
                cx.hit(Rect { x: track.x - 8.0, y: mid - 14.0, w: track.w + 16.0, h: 28.0 }, Ctl::Slider(*k, *min, *max, *step));
            }
            RowCtl::Step(k, min, max, step) => {
                let v = self.num(*k);
                let minus = Rect { x: right - w, y: mid - 13.0, w: 26.0, h: 26.0 };
                let plus = Rect { x: right - 26.0, ..minus };
                for (b, sym, dir) in [(minus, "−", -1.0), (plus, "+", 1.0)] {
                    cx.c.fill_round_rect(b, 7.0, p.ctl_h);
                    text(cx.c, sym, b.x, b.y, b.w, b.h, 15.0, 500, p.text, Align::Center);
                    cx.hit(b, Ctl::Step(*k, *min, *max, *step * dir));
                }
                text(cx.c, &format!("{}", v as i64), minus.x + 26.0, minus.y, 48.0, 26.0, 13.0, 500, p.text, Align::Center);
            }
            RowCtl::Text(k, secret) => {
                let f = Rect { x: right - w, y: mid - 14.0, w, h: 28.0 };
                self.text_field(cx, f, *k, *secret);
            }
            RowCtl::Button(label, act) => {
                let b = Rect { x: right - w, y: mid - 14.0, w, h: 28.0 };
                button(cx, b, label, Ctl::Button(*act), false);
            }
        }
    }

    fn text_field(&mut self, cx: &mut Cx, f: Rect, k: Key, secret: bool) {
        let p = cx.p;
        let focused = self.focus == Some(k);
        cx.c.fill_round_rect(f, 8.0, p.field);
        if focused {
            cx.c.push_clip_out(Rect { x: f.x + 1.5, y: f.y + 1.5, w: f.w - 3.0, h: f.h - 3.0 }, 6.5);
            cx.c.fill_round_rect(f, 8.0, ACCENT);
            cx.c.pop_clip();
        }
        let raw = if focused { self.edit.display_text() } else { self.text_of(k) };
        let shown: String = if secret { raw.chars().map(|_| '•').collect() } else { raw };
        let inner = f.w - 20.0;
        cx.c.push_clip(Rect { x: f.x + 2.0, y: f.y, w: f.w - 4.0, h: f.h }, 6.0);
        if focused {
            let prefix_raw = self.edit.caret_prefix();
            let prefix: String = if secret { prefix_raw.chars().map(|_| '•').collect() } else { prefix_raw };
            let pw = cx.c.measure_text(&prefix, 12.5, 400);
            // Scroll the text so the caret stays inside the field.
            let shift = (pw - inner).max(0.0);
            text(cx.c, &shown, f.x + 10.0 - shift, f.y, 4000.0, f.h, 12.5, 400, p.text, Align::Left);
            cx.c.fill_round_rect(Rect { x: f.x + 10.0 + pw - shift, y: f.y + 6.0, w: 1.5, h: f.h - 12.0 }, 0.0, p.text);
        } else {
            let t = fit_text(cx.c, &shown, 12.5, 400, inner);
            text(cx.c, &t, f.x + 10.0, f.y, inner, f.h, 12.5, 400, p.text, Align::Left);
        }
        cx.c.pop_clip();
        cx.hit(f, Ctl::Text(k));
    }

    fn preview(&mut self, cx: &mut Cx, y: f32) -> f32 {
        let area = Rect { x: cx.x, y, w: cx.w, h: 84.0 };
        cx.c.push_clip(area, 12.0);
        let bands = [Color(255, 138, 92, 255), Color(224, 64, 154, 255), Color(106, 92, 255, 255), Color(18, 181, 165, 255)];
        let bw = area.w / bands.len() as f32;
        for (i, b) in bands.iter().enumerate() {
            cx.c.fill_round_rect(Rect { x: area.x + i as f32 * bw, y: area.y, w: bw + 1.0, h: area.h }, 0.0, *b);
        }
        for i in 0..18 {
            cx.c.fill_round_rect(Rect { x: area.x + i as f32 * 40.0 - 10.0, y: area.y, w: 12.0, h: area.h }, 0.0, Color(255, 255, 255, 28));
        }
        let pw = self.settings.pill_width.clamp(80.0, 180.0) + 60.0;
        let ph = self.settings.pill_height.clamp(22.0, 40.0) + 8.0;
        let n = Rect { x: area.x + (area.w - pw) / 2.0, y: area.y - 12.0, w: pw, h: ph + 12.0 };
        let (fill, fg) = match self.material() {
            0 => (Color(0, 0, 0, 255), Color(255, 255, 255, 255)),
            1 => (Color(18, 20, 26, 150), Color(255, 255, 255, 255)),
            _ => (Color(246, 247, 250, 175), Color(20, 22, 26, 255)),
        };
        cx.c.fill_round_rect(n, 13.0, fill);
        if self.material() != 0 {
            cx.c.push_clip_out(Rect { x: n.x + 1.0, y: n.y, w: n.w - 2.0, h: n.h - 1.0 }, 12.0);
            cx.c.fill_round_rect(n, 13.0, Color(255, 255, 255, if self.material() == 2 { 200 } else { 45 }));
            cx.c.pop_clip();
        }
        text(cx.c, "12:41", n.x, n.y + 12.0, n.w, ph, 12.0, 600, fg, Align::Center);
        cx.c.pop_clip();
        y + area.h + 12.0
    }

    fn order_list(&mut self, cx: &mut Cx, y: f32) -> f32 {
        let h = self.order.len() as f32 * ORDER_H + 12.0;
        cx.c.fill_round_rect(Rect { x: cx.x, y, w: cx.w, h }, 12.0, cx.p.ctl);
        for (i, (id, title)) in self.order.iter().enumerate() {
            let r = Rect { x: cx.x + 6.0, y: y + 6.0 + i as f32 * ORDER_H, w: cx.w - 12.0, h: ORDER_H - 3.0 };
            let dragging = self.drag_row == Some(i);
            cx.c.fill_round_rect(r, 8.0, if dragging { cx.p.sel } else { cx.p.ctl });
            if dragging {
                cx.c.push_clip_out(Rect { x: r.x + 1.5, y: r.y + 1.5, w: r.w - 3.0, h: r.h - 3.0 }, 6.5);
                cx.c.fill_round_rect(r, 8.0, ACCENT);
                cx.c.pop_clip();
            }
            text(cx.c, "⋮⋮", r.x + 8.0, r.y, 18.0, r.h, 13.0, 400, cx.p.mute, Align::Center);
            let tile = Rect { x: r.x + 34.0, y: r.y + (r.h - 22.0) / 2.0, w: 22.0, h: 22.0 };
            cx.c.fill_round_rect(tile, 6.0, tile_color(id));
            let letter: String = title.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
            text(cx.c, &letter, tile.x, tile.y, tile.w, tile.h, 11.0, 600, Color(255, 255, 255, 255), Align::Center);
            text(cx.c, title, r.x + 66.0, r.y, r.w - 140.0, r.h, 13.0, 400, cx.p.text, Align::Left);
            let up = Rect { x: r.x + r.w - 56.0, y: r.y + 4.0, w: 24.0, h: r.h - 8.0 };
            let down = Rect { x: r.x + r.w - 30.0, ..up };
            cx.hit(r, Ctl::OrderRow(i));
            for (b, sym, ctl) in [(up, "▲", Ctl::OrderUp(i)), (down, "▼", Ctl::OrderDown(i))] {
                cx.hit(b, ctl);
                text(cx.c, sym, b.x, b.y, b.w, b.h, 10.0, 400, cx.p.mute, Align::Center);
            }
        }
        y + h + 8.0
    }

    fn cities(&mut self, cx: &mut Cx, y: f32) -> f32 {
        let n = self.settings.world_clocks.len();
        let h = n as f32 * 44.0 + 12.0;
        cx.c.fill_round_rect(Rect { x: cx.x, y, w: cx.w, h }, 12.0, cx.p.ctl);
        for i in 0..n {
            let ry = y + 8.0 + i as f32 * 44.0;
            self.text_field(cx, Rect { x: cx.x + 12.0, y: ry, w: 150.0, h: 30.0 }, Key::City(i), false);
            self.text_field(cx, Rect { x: cx.x + 172.0, y: ry, w: cx.w - 172.0 - 56.0, h: 30.0 }, Key::Zone(i), false);
            let d = Rect { x: cx.x + cx.w - 40.0, y: ry + 1.0, w: 28.0, h: 28.0 };
            text(cx.c, "✕", d.x, d.y, d.w, d.h, 12.0, 400, cx.p.mute, Align::Center);
            cx.hit(d, Ctl::DelCity(i));
        }
        let mut y2 = y + h + 8.0;
        if n < MAX_CITIES {
            button(cx, Rect { x: cx.x, y: y2, w: 130.0, h: 30.0 }, "+ Tambah kota", Ctl::AddCity, false);
            y2 += 38.0;
        }
        text(cx.c, "Zona waktu memakai nama Windows, misalnya Tokyo Standard Time (tzutil /l).", cx.x, y2, cx.w, 18.0, 12.0, 400, cx.p.mute, Align::Left);
        y2 + 24.0
    }

    // ---- input ----

    fn hit_at(&self, x: f32, y: f32) -> Option<usize> {
        let (ox, oy) = self.origin;
        let pane = Rect { x: ox + NAV_W, y: oy + TITLE_H, w: W - NAV_W, h: H - TITLE_H - FOOT_H };
        let inside = |r: &Rect| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h;
        (0..self.hits.len()).rev().find(|&i| {
            let h = &self.hits[i];
            inside(&h.r) && (!h.pane || inside(&pane))
        })
    }

    pub fn on_event(&mut self, e: &Event) -> Outcome {
        let mut out = Outcome::default();
        match e {
            Event::PointerMove(x, y) => {
                let (ox, oy) = self.origin;
                let h = self.hit_at(x + ox, y + oy);
                if h != self.hover {
                    self.hover = h;
                    out.redraw = true;
                }
            }
            Event::PointerDown(x, y) => {
                let (ox, oy) = self.origin;
                let h = self.hit_at(x + ox, y + oy);
                let was = self.focus;
                if !matches!(h.map(|i| self.hits[i].ctl), Some(Ctl::Text(_))) {
                    self.end_edit();
                }
                if let Some(i) = h {
                    let ctl = self.hits[i].ctl;
                    self.press(i, ctl, x + ox, &mut out);
                }
                out.redraw = out.redraw || was != self.focus || h.is_some();
            }
            Event::PointerDrag(x, y) => {
                let (ox, oy) = self.origin;
                if let Some(i) = self.drag_slider {
                    if let Ctl::Slider(k, min, max, step) = self.hits[i].ctl {
                        let r = self.hits[i].r;
                        let f = ((x + ox - (r.x + 8.0)) / (r.w - 16.0)).clamp(0.0, 1.0);
                        let v = ((min + f * (max - min)) / step).round() * step;
                        self.change_num(k, v.clamp(min, max), &mut out);
                    }
                } else if let Some(from) = self.drag_row {
                    // The row under the cursor takes the dragged row's place.
                    if let Some(j) = self.hit_at(x + ox, y + oy) {
                        if let Ctl::OrderRow(to) = self.hits[j].ctl {
                            if to != from {
                                let item = self.order.remove(from);
                                self.order.insert(to, item);
                                self.drag_row = Some(to);
                                self.order_touched = true;
                                out.changed.push(Key::Order);
                                out.redraw = true;
                            }
                        }
                    }
                }
            }
            Event::PointerUp(..) => {
                if self.drag_slider.take().is_some() || self.drag_row.take().is_some() {
                    out.redraw = true;
                }
            }
            Event::PointerWheel(steps) => {
                let max = (self.content_h - (H - TITLE_H - FOOT_H)).max(0.0);
                let before = self.scroll;
                self.scroll = (self.scroll - steps * 48.0).clamp(0.0, max);
                out.redraw = self.scroll != before;
            }
            Event::Edit { key, shift } => self.edit_key(*key, *shift, &mut out),
            Event::Text(t) => {
                if let Some(k) = self.focus {
                    self.edit.insert(t);
                    self.commit_text(k, &mut out);
                }
            }
            Event::Shortcut(c) => {
                if let Some(k) = self.focus {
                    match c {
                        'a' => self.edit.select_all(),
                        'c' => out.copy = self.edit.selected_text(),
                        'x' => {
                            out.copy = self.edit.cut();
                            self.commit_text(k, &mut out);
                        }
                        _ => {}
                    }
                    out.redraw = true;
                }
            }
            Event::Compose(c) => {
                if self.focus.is_some() {
                    self.edit.set_composition(c.clone().map(|c: Composition| c));
                    out.redraw = true;
                }
            }
        }
        out
    }

    fn press(&mut self, i: usize, ctl: Ctl, x: f32, out: &mut Outcome) {
        match ctl {
            Ctl::Close => out.acts.push(Act::Close),
            Ctl::Nav(n) => {
                self.section = n;
                self.scroll = 0.0;
                self.end_edit();
            }
            Ctl::Toggle(k) => {
                let v = !self.flag(k);
                self.set_flag(k, v);
                self.touched(k, out);
            }
            Ctl::Choice(Key::Material, n) => {
                self.settings.glass = n > 0;
                self.settings.glass_light = n == 2;
                self.touched(Key::Material, out);
            }
            Ctl::Choice(..) => {}
            Ctl::Slider(k, min, max, step) => {
                let r = self.hits[i].r;
                let f = ((x - (r.x + 8.0)) / (r.w - 16.0)).clamp(0.0, 1.0);
                let v = ((min + f * (max - min)) / step).round() * step;
                self.change_num(k, v.clamp(min, max), out);
                self.drag_slider = Some(i);
            }
            Ctl::Step(k, min, max, delta) => {
                let v = (self.num(k) + delta).clamp(min, max);
                self.change_num(k, v, out);
            }
            Ctl::Text(k) => {
                if self.focus != Some(k) {
                    self.end_edit();
                    self.focus = Some(k);
                    let t = self.text_of(k);
                    self.edit.set_text(&t);
                    self.edit.end(false);
                }
            }
            Ctl::Button(a) => out.acts.push(a),
            Ctl::OrderRow(n) => self.drag_row = Some(n),
            Ctl::OrderUp(n) => self.move_row(n, -1, out),
            Ctl::OrderDown(n) => self.move_row(n, 1, out),
            Ctl::OrderReset => {
                let mut v = self.order.clone();
                v.sort_by_key(|(id, _)| default_rank(id));
                if v != self.order {
                    self.order = v;
                    self.order_touched = true;
                    out.changed.push(Key::Order);
                }
            }
            Ctl::PomoReset => {
                self.settings.pomodoro = crate::pomodoro::PomodoroConfig::default();
                self.restart = true;
                out.changed.push(Key::Focus);
            }
            Ctl::AddCity => {
                if self.settings.world_clocks.len() < MAX_CITIES {
                    self.settings.world_clocks.push(WorldClock { label: "Kota".into(), zone: "UTC".into() });
                    self.touched(Key::City(0), out);
                }
            }
            Ctl::DelCity(n) => {
                if n < self.settings.world_clocks.len() {
                    self.end_edit();
                    self.settings.world_clocks.remove(n);
                    self.touched(Key::City(0), out);
                }
            }
        }
    }

    fn move_row(&mut self, i: usize, d: i32, out: &mut Outcome) {
        let j = i as i32 + d;
        if j >= 0 && (j as usize) < self.order.len() {
            self.order.swap(i, j as usize);
            self.order_touched = true;
            out.changed.push(Key::Order);
        }
    }

    fn change_num(&mut self, k: Key, v: f32, out: &mut Outcome) {
        if self.num(k) != v {
            self.set_num(k, v);
            self.touched(k, out);
        }
    }

    fn touched(&mut self, k: Key, out: &mut Outcome) {
        if k == Key::Order {
            self.order_touched = true;
        }
        if Self::needs_restart(k) {
            self.restart = true;
        }
        out.changed.push(k);
        out.redraw = true;
    }

    fn commit_text(&mut self, k: Key, out: &mut Outcome) {
        let t = self.edit.text().to_string();
        self.set_text(k, &t);
        self.touched(k, out);
    }

    fn end_edit(&mut self) {
        self.focus = None;
        self.edit.set_composition(None);
    }

    fn edit_key(&mut self, key: EditKey, shift: bool, out: &mut Outcome) {
        let Some(k) = self.focus else { return };
        match key {
            EditKey::Enter | EditKey::Escape => self.end_edit(),
            EditKey::Backspace => {
                self.edit.backspace();
                self.commit_text(k, out);
            }
            EditKey::Delete => {
                self.edit.delete();
                self.commit_text(k, out);
            }
            EditKey::Left => self.edit.move_left(shift),
            EditKey::Right => self.edit.move_right(shift),
            EditKey::Home => self.edit.home(shift),
            EditKey::End => self.edit.end(shift),
        }
        out.redraw = true;
    }

    /// Where the caret is (content coordinates), for the IME window.
    pub fn text_anchor(&self) -> Option<(f32, f32)> {
        let k = self.focus?;
        let (ox, oy) = self.origin;
        self.hits.iter().find(|h| h.ctl == Ctl::Text(k)).map(|h| (h.r.x - ox + 10.0, h.r.y - oy + h.r.h))
    }
}

// ---- drawing helpers ----

struct Cx<'a> {
    c: &'a mut dyn Canvas,
    p: &'a Palette,
    hits: &'a mut Vec<Hit>,
    hover: Option<usize>,
    x: f32,
    w: f32,
    #[allow(dead_code)]
    light: bool,
}

impl Cx<'_> {
    fn hit(&mut self, r: Rect, ctl: Ctl) {
        self.hits.push(Hit { r, ctl, pane: true });
    }
}

enum RowCtl<'a> {
    Toggle(Key),
    Choice(Key, &'a [&'a str]),
    Slider(Key, f32, f32, f32, &'a str),
    Step(Key, f32, f32, f32),
    Text(Key, bool),
    Button(&'a str, Act),
}

struct Row<'a> {
    name: &'a str,
    desc: &'a str,
    key: &'a str,
    ctl: RowCtl<'a>,
}

fn row_h(r: &Row) -> f32 {
    if r.desc.is_empty() { ROW_H - 14.0 } else { ROW_H }
}

#[allow(clippy::too_many_arguments)]
fn text(c: &mut dyn Canvas, s: &str, x: f32, y: f32, w: f32, h: f32, size: f32, weight: u16, col: Color, align: Align) {
    if !s.is_empty() {
        c.draw_text(s, Rect { x, y, w, h }, size, weight, col, align);
    }
}

fn button(cx: &mut Cx, r: Rect, label: &str, ctl: Ctl, primary: bool) {
    let hovered = cx.hits.len() < usize::MAX && cx.hover.is_some_and(|h| h == cx.hits.len());
    let fill = if primary { if hovered { Color(228, 134, 102, 255) } else { ACCENT } } else if hovered { cx.p.sel } else { cx.p.ctl_h };
    cx.c.fill_round_rect(r, 8.0, fill);
    text(cx.c, label, r.x, r.y, r.w, r.h, 12.5, 500, if primary { Color(255, 255, 255, 255) } else { cx.p.text }, Align::Center);
    cx.hit(r, ctl);
}

/// Soft shadow outside the window: stacked rounded rectangles, clipped out of the window shape.
fn shadow(c: &mut dyn Canvas, shape: Rect, light: bool) {
    c.push_clip_out(shape, RADIUS);
    let (a, n) = if light { (5, 14) } else { (7, 14) };
    for i in 0..n {
        let g = (n - i) as f32 * 1.9;
        c.fill_round_rect(Rect { x: shape.x - g, y: shape.y - g + 10.0, w: shape.w + g * 2.0, h: shape.h + g * 2.0 }, RADIUS + g, Color(0, 0, 0, a));
    }
    c.pop_clip();
}

fn tile_color(id: &str) -> Color {
    match id {
        "music" => Color(224, 64, 154, 255),
        "timer" | "usage" => Color(217, 119, 87, 255),
        "todo" => Color(91, 141, 239, 255),
        "clipboard" => Color(138, 99, 210, 255),
        "shelf" => Color(18, 165, 148, 255),
        "water" => Color(42, 168, 214, 255),
        "widgets" => Color(154, 125, 46, 255),
        "notifications" => Color(214, 86, 63, 255),
        "digioh" => Color(58, 165, 107, 255),
        _ => Color(110, 118, 132, 255),
    }
}

const DEFAULT_ORDER: [&str; 11] = ["music", "timer", "todo", "clipboard", "shelf", "water", "widgets", "notifications", "digioh", "usage", "update"];

fn default_rank(id: &str) -> usize {
    DEFAULT_ORDER.iter().position(|x| *x == id).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::RecordingCanvas;

    fn view() -> SettingsView {
        let pages = ["music", "timer", "todo"].iter().map(|i| (i.to_string(), i.to_uppercase())).collect();
        SettingsView::new(Settings::default(), pages, "0.3.7")
    }

    fn click(v: &mut SettingsView, x: f32, y: f32) -> Outcome {
        v.on_event(&Event::PointerMove(x, y));
        let o = v.on_event(&Event::PointerDown(x, y));
        v.on_event(&Event::PointerUp(x, y));
        o
    }

    fn centre(v: &SettingsView, f: impl Fn(&Ctl) -> bool) -> (f32, f32) {
        let h = v.hits.iter().find(|h| f(&h.ctl)).expect("control not painted");
        (h.r.x - v.origin.0 + h.r.w / 2.0, h.r.y - v.origin.1 + h.r.h / 2.0)
    }

    #[test]
    fn toggle_flips_the_setting_and_reports_the_key() {
        let mut v = view();
        let mut c = RecordingCanvas::default();
        v.paint(&mut c, 40.0, 40.0);
        let (x, y) = centre(&v, |c| *c == Ctl::Toggle(Key::Autostart));
        let before = v.settings.autostart;
        let o = click(&mut v, x, y);
        assert_eq!(v.settings.autostart, !before);
        assert_eq!(o.changed, vec![Key::Autostart]);
    }

    #[test]
    fn nav_switches_section_and_material_sets_glass() {
        let mut v = view();
        let mut c = RecordingCanvas::default();
        v.paint(&mut c, 40.0, 40.0);
        let (x, y) = centre(&v, |c| *c == Ctl::Nav(1));
        click(&mut v, x, y);
        v.paint(&mut c, 40.0, 40.0);
        let (x, y) = centre(&v, |c| *c == Ctl::Choice(Key::Material, 2));
        click(&mut v, x, y);
        assert!(v.settings.glass && v.settings.glass_light && v.light());
        let (x, y) = centre(&v, |c| *c == Ctl::Choice(Key::Material, 0));
        click(&mut v, x, y);
        assert!(!v.settings.glass);
    }

    #[test]
    fn stepper_stays_inside_its_range() {
        let mut v = view();
        let mut c = RecordingCanvas::default();
        v.section = 4;
        v.paint(&mut c, 40.0, 40.0);
        let (x, y) = centre(&v, |c| matches!(c, Ctl::Step(Key::Sessions, _, _, d) if *d < 0.0));
        for _ in 0..10 {
            click(&mut v, x, y);
        }
        assert_eq!(v.settings.pomodoro.sessions_before_long_break, 2);
    }

    #[test]
    fn page_order_moves_with_the_arrows_and_the_reset_restores_it() {
        let mut v = view();
        let mut c = RecordingCanvas::default();
        v.section = 2;
        v.paint(&mut c, 40.0, 40.0);
        let (x, y) = centre(&v, |c| *c == Ctl::OrderDown(0));
        let o = click(&mut v, x, y);
        assert_eq!(v.page_order(), vec!["timer", "music", "todo"]);
        assert_eq!(o.changed, vec![Key::Order]);
        v.paint(&mut c, 40.0, 40.0);
        let (x, y) = centre(&v, |c| *c == Ctl::OrderReset);
        click(&mut v, x, y);
        assert_eq!(v.page_order(), vec!["music", "timer", "todo"]);
    }

    #[test]
    fn typing_into_a_text_field_updates_the_setting_and_escape_leaves() {
        let mut v = view();
        let mut c = RecordingCanvas::default();
        v.section = 6;
        v.paint(&mut c, 40.0, 40.0);
        let (x, y) = centre(&v, |c| *c == Ctl::Text(Key::Repo));
        click(&mut v, x, y);
        assert!(v.wants_text());
        v.on_event(&Event::Shortcut('a'));
        v.on_event(&Event::Text("a/b".into()));
        assert_eq!(v.settings.update_repo, "a/b");
        v.on_event(&Event::Edit { key: EditKey::Backspace, shift: false });
        assert_eq!(v.settings.update_repo, "a/");
        v.on_event(&Event::Edit { key: EditKey::Escape, shift: false });
        assert!(!v.wants_text());
    }

    #[test]
    fn apply_to_copies_only_what_the_window_edits() {
        let mut v = view();
        v.settings.glass = true;
        v.settings.skipped_version = "9.9.9".into();
        let mut s = Settings::default();
        s.skipped_version = "1.0.0".into();
        s.page_order = vec!["usage".into()];
        v.apply_to(&mut s);
        assert!(s.glass);
        assert_eq!(s.skipped_version, "1.0.0");
        assert_eq!(s.page_order, vec!["usage"], "untouched order is not written");
        v.order_touched = true;
        v.apply_to(&mut s);
        assert_eq!(s.page_order, vec!["music", "timer", "todo"]);
    }

    #[test]
    fn every_section_paints_without_panicking() {
        let mut v = view();
        for i in 0..SECTIONS.len() {
            v.section = i;
            let mut c = RecordingCanvas::default();
            v.paint(&mut c, 40.0, 40.0);
            assert!(!c.calls.is_empty());
        }
    }
}
