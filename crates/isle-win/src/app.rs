// SPDX-License-Identifier: GPL-3.0-or-later
//! Notch state: hover, open/close, and the springs that animate its size.

use crate::modules::{charging::ChargingModule, clock::ClockModule, timer::TimerModule, todo::TodoModule, clipboard::ClipboardModule, shelf::ShelfModule, water::WaterModule, widgets::WidgetsModule, notifications::NotificationsModule, update::UpdateModule, digioh::DigiohModule, usage::UsageModule};
use crate::system::{epoch_ms, local_time};
use crate::ui::{self, Header};
use isle_logic::canvas::{Align, Canvas, Color, Rect, fit_text};
use isle_logic::notifs::scroll_offset;
use isle_logic::chrome::{ChromeAction, chrome_hit};
use isle_logic::headphones::{Level, battery_anim, level_of};
use isle_logic::module::{Event, PillBattery, PillCard, PillItem, Reaction};
use isle_logic::osd::{self, OsdKind, PillOsd};
use isle_logic::equaliser;
use isle_logic::rest;
use isle_logic::track_card::{self, CARD_H, CARD_RADIUS, CARD_W};
use isle_logic::layout::{Hover, target};
use isle_logic::timefmt::{format_clock, format_date};
use isle_logic::module::Registry;
use isle_logic::page_transition;
use isle_logic::settings::Settings;
use isle_logic::spring::Spring;
use std::time::Instant;

pub struct Poll {
    pub redraw: bool,
    /// The closed pill's width changed: the window must be resized.
    pub relayout: bool,
}

pub struct Step {
    /// Set when this step switched the notch to open.
    pub opened: bool,
    /// The notch finished closing and the window can shrink back to the pill.
    pub fully_closed: bool,
    pub keep_ticking: bool,
}

pub struct App {
    pub registry: Registry,
    pub settings: Settings,
    pub wide: bool,
    pub lyrics: bool,
    w: Spring,
    h: Spring,
    r: Spring,
    hover: Hover,
    inside: bool,
    open: bool,
    /// The window is currently as large as the open panel.
    pub window_big: bool,
    start: Instant,
    /// Equaliser clock (stops while paused), bar height scale 0..1, and the uptime of the last step.
    eq_t: f32,
    eq_level: f32,
    eq_prev: f32,
    last: Option<Instant>,
    last_pill: String,
    /// A page slide in progress: (page we are leaving, direction, start epoch ms).
    trans: Option<(usize, f32, i64)>,
    /// Mouse-wheel page turning: when the last turn happened (a flick turns one page, not five).
    wheel_turned_ms: i64,
    /// The pill is currently grown into the "new song" card.
    card: bool,
    /// Width the closed pill needs for a long centre text (a peek); 0 when the usual width does.
    text_w: f32,
    /// When the pill's centre text last changed (epoch ms), and when it was last drawn scrolling.
    pill_since: i64,
    pill_rolling_ms: i64,
    /// What the card shows; kept while it shrinks back so it can fade out.
    card_last: Option<PillCard>,
    /// The pill shows only the clock, so it may fall asleep; the last time it did not (now_ms).
    rest_ok: bool,
    last_active: i64,
}

impl App {
    pub fn new(mut settings: Settings) -> Self {
        match std::env::var("ISLE_GLASS").as_deref() {
            Ok("1") => settings.glass = true,
            Ok("light") => {
                settings.glass = true;
                settings.glass_light = true;
            }
            _ => {}
        }
        // Trying an order without touching settings.toml: ISLE_PAGE_ORDER=usage,music
        if let Ok(order) = std::env::var("ISLE_PAGE_ORDER") {
            settings.page_order = order.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }
        let (w, h, r) = closed_dims(&settings, false);
        let mut registry = Registry::new();
        registry.add(Box::new(ClockModule));
        registry.add(Box::new(ChargingModule::new()));
                registry.add(Box::new(TimerModule::new(settings.pomodoro.clone())));
        registry.add(Box::new(TodoModule::new()));
        registry.add(Box::new(ClipboardModule::new()));
        registry.add(Box::new(ShelfModule::new()));
        registry.add(Box::new(WaterModule::new()));
        registry.add(Box::new(WidgetsModule::new(&settings.world_clocks)));
        registry.add(Box::new(NotificationsModule::new()));
        registry.add(Box::new(DigiohModule::new()));
        registry.add(Box::new(UsageModule::new()));
        registry.add(Box::new(UpdateModule::new()));
        App {
            registry,
            settings,
            wide: false,
            lyrics: false,
            w: Spring::new(w, 300.0, 22.0),
            h: Spring::new(h, 300.0, 22.0),
            r: Spring::new(r, 260.0, 20.0),
            hover: Hover::new(),
            inside: false,
            open: false,
            window_big: false,
            start: Instant::now(),
            eq_t: 0.0,
            eq_level: 0.0,
            eq_prev: 0.0,
            last: None,
            last_pill: String::new(),
            trans: None,
            wheel_turned_ms: 0,
            card: false,
            text_w: 0.0,
            pill_since: 0,
            pill_rolling_ms: i64::MIN / 2,
            card_last: None,
            rest_ok: false,
            last_active: 0,
        }
    }

    /// The volume / brightness bar to show now, unless the user turned it off.
    fn osd_now(&self) -> Option<PillOsd> {
        if self.settings.osd { self.registry.pill_osd(epoch_ms()) } else { None }
    }

    /// Width, height and radius of the pill at rest: the "new song" card while it shows.
    fn closed_now(&self) -> (f32, f32, f32) {
        if self.card {
            return (CARD_W, CARD_H, CARD_RADIUS);
        }
        let (w, h, r) = closed_dims(&self.settings, self.wide);
        (w.max(self.text_w), h, r)
    }

    pub fn now_ms(&self) -> i64 {
        self.start.elapsed().as_millis() as i64
    }

    pub fn pointer(&mut self, inside: bool) {
        self.inside = inside;
        let now = self.now_ms();
        self.last_active = now;
        self.hover.pointer_inside(inside, now);
    }

    /// Current animated shape in DIPs: width, height, bottom-corner radius.
    pub fn dims(&self) -> (f32, f32, f32) {
        // The corner radius follows how far the notch has grown (not its own spring), so the
        // shape rounds off in step with its size instead of looking like a box that rounds later.
        let (_, _, base_r) = closed_dims(&self.settings, self.wide);
        let base_h = self.settings.pill_height;
        let open = target(true, self.wide, self.lyrics);
        // Round off through the card's radius on the way up, whatever state flag is set.
        let gf = ((self.h.value - base_h) / (CARD_H - base_h)).clamp(0.0, 1.0);
        let card_r = base_r + (CARD_RADIUS - base_r) * gf;
        let p = ((self.h.value - CARD_H) / (open.size.h - CARD_H)).clamp(0.0, 1.0);
        (self.w.value, self.h.value, card_r + (open.radius - card_r) * p)
    }

    /// Window size (DIPs) for the open or closed state.
    pub fn window_dims(&self, big: bool) -> (f32, f32) {
        if big {
            let t = target(true, self.wide, self.lyrics).size;
            (t.w, t.h)
        } else {
            let (w, h, _) = self.closed_now();
            (w, h)
        }
    }

    /// What the window region must cover now (DIPs): the shape as drawn, and where it is heading.
    pub fn region_dims(&self) -> (f32, f32) {
        let (tw, th) = self.window_dims(self.window_big);
        // Never smaller than the shape as drawn, so a shrinking card is not cropped.
        (tw.max(self.w.value.ceil() + 1.0), th.max(self.h.value.ceil() + 1.0))
    }

    /// The shape to blur behind (width, height, corner radius in DIPs), when the glass look is on.
    pub fn blur_shape(&self) -> Option<(f32, f32, f32)> {
        self.settings.glass.then(|| self.dims())
    }

    /// A soft shadow around the glass (CSS `0 22px 50px rgba(0,0,0,.35)`): stacked translucent
    /// rounded rects, drawn only outside the shape so it never dulls the glass itself.
    fn paint_glass_shadow(&self, c: &mut dyn Canvas, x: f32, w: f32, h: f32, r: f32) {
        let grow = ((h - 26.0) / (184.0 - 26.0)).clamp(0.0, 1.0);
        let reach = 10.0 + 24.0 * grow;
        let drop = 4.0 + 14.0 * grow;
        const LAYERS: i32 = 12;
        let alpha = (3.0 + 2.0 * grow) as u8;
        // Everything outside the glass shape, with its exact rounded corners.
        c.push_clip_out(Rect { x, y: -r, w, h: h + r }, r);
        for i in 0..LAYERS {
            let e = reach * (i + 1) as f32 / LAYERS as f32;
            let rect = Rect { x: x - e, y: -e - r, w: w + 2.0 * e, h: h + r + 2.0 * e + drop };
            c.fill_round_rect(rect, r + e, if self.settings.glass_light { Color(30, 40, 80, alpha.saturating_sub(1)) } else { Color(0, 0, 0, alpha) });
        }
        c.pop_clip();
    }

    pub fn set_glass(&mut self, on: bool, light: bool) {
        self.settings.glass = on;
        self.settings.glass_light = light;
    }

    /// Pages the settings window can reorder: (id, title), in the current order.
    pub fn page_list(&mut self) -> Vec<(String, String)> {
        self.registry.modules_mut().iter().filter(|m| m.has_page() && m.id() != "update").map(|m| (m.id().to_string(), m.title().to_string())).collect()
    }

    /// Takes settings edited in the settings window: look, order and the values read live.
    /// Pomodoro, lyrics lead and the world clocks are read once at start and wait for a restart.
    pub fn apply_settings(&mut self, s: isle_logic::settings::Settings) {
        let here = self.registry.current_page_id();
        self.registry.reorder(&s.page_order);
        if let Some(id) = here {
            self.registry.goto_id(id);
        }
        self.settings.glass = s.glass;
        self.settings.glass_light = s.glass_light;
        self.settings.pill_width = s.pill_width;
        self.settings.pill_height = s.pill_height;
        self.settings.rest_after_secs = s.rest_after_secs;
        self.settings.auto_update = s.auto_update;
        self.settings.osd = s.osd;
        self.settings.hide_native_osd = s.hide_native_osd;
        crate::osd_win::set_hide_native(s.osd && s.hide_native_osd);
        self.settings.update_check_hours = s.update_check_hours;
        self.settings.update_repo = s.update_repo;
        self.settings.update_token = s.update_token;
        self.settings.page_order = s.page_order;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Follows the current page's wish for a wide panel (lyrics). Returns true when
    /// the open size changed, so the caller must resize the window.
    pub fn sync_lyrics(&mut self) -> bool {
        let want = self.registry.current_page().is_some_and(|m| m.wants_wide_panel());
        if want == self.lyrics {
            return false;
        }
        self.lyrics = want;
        if self.open {
            // Widening or narrowing the open panel should glide, not bounce past its size.
            self.w.set_damping(self.w.critical_damping());
            let t = target(true, self.wide, self.lyrics);
            self.w.target = t.size.w;
            self.h.target = t.size.h;
            self.r.target = t.radius;
        }
        true
    }

    /// Polls the modules (once a second and on wake-ups). Returns whether anything
    /// visible changed.
    pub fn poll(&mut self) -> Poll {
        let polled = self.registry.poll_all(epoch_ms());
        let (text, _) = self.registry.pill_text("");
        let text_changed = text != self.last_pill;
        // Room for the text between the cover (left) and the equaliser (right).
        let chars = text.chars().count();
        let battery = self.registry.pill_battery(epoch_ms());
        let osd = self.osd_now();
        let text_w = if osd.is_some() { OSD_W } else if chars > 6 || battery.is_some() { (chars as f32 * 7.4 + 72.0).min(PILL_MAX_TEXT_W) + if battery.is_some() { 62.0 } else { 0.0 } } else { 0.0 };
        if text_changed {
            self.pill_since = epoch_ms();
        }
        let width_changed = text_w != self.text_w;
        self.text_w = text_w;
        self.last_pill = text;
        let wide = self.registry.any_wide_pill();
        let card_now = self.registry.pill_card(epoch_ms());
        let want_card = card_now.is_some() && !self.open;
        if card_now.is_some() {
            self.card_last = card_now;
        }
        let relayout = wide != self.wide || want_card != self.card || width_changed;
        if relayout {
            self.wide = wide;
            self.card = want_card;
            if !self.open {
                let (w, h, r) = self.closed_now();
                self.w.target = w;
                self.h.target = h;
                self.r.target = r;
            }
        }
        let resized = self.sync_lyrics();
        let t = local_time();
        self.rest_ok = !self.open && !self.inside && !self.card && battery.is_none() && osd.is_none()
            && self.last_pill == format_clock(t.hour, t.minute)
            && !self.registry.pill_items().iter().any(|i| matches!(i, PillItem::Equaliser { active: true }));
        if !self.rest_ok {
            self.last_active = self.now_ms();
        }
        Poll { redraw: polled || text_changed || self.open || relayout || resized || self.card || self.equaliser_running() || self.resting() > 0.0 || epoch_ms() - self.pill_rolling_ms < 120, relayout: relayout || resized }
    }

    /// When the modules next need a redraw (epoch ms).
    pub fn next_wake_ms(&self) -> Option<i64> {
        let now = epoch_ms();
        // The closed pill's equaliser moves, so it needs a steady (but not full-rate) redraw.
        let eq = self.equaliser_running().then_some(now + 33);
        // Eyes: a steady redraw while they show, and one wake for the moment they should appear.
        let after = self.settings.rest_after_secs * 1000;
        let eyes = if self.rest_ok && after > 0 {
            let idle = self.now_ms() - self.last_active;
            Some(now + if idle >= after { 33 } else { (after - idle).max(1) })
        } else {
            None
        };
        // Long pill text scrolls: one frame per display refresh.
        let rolling = (now - self.pill_rolling_ms < 120).then_some(now + 16);
        [self.registry.earliest_wake(now), eq, eyes, rolling].into_iter().flatten().min()
    }

    /// How far the pill has fallen asleep (0 awake .. 1 eyes fully shown).
    fn resting(&self) -> f32 {
        if !self.rest_ok {
            return 0.0;
        }
        rest::visibility(self.now_ms() - self.last_active, self.settings.rest_after_secs * 1000)
    }

    /// True while the music is playing (the bars dance) or still settling down after a pause.
    fn equaliser_running(&self) -> bool {
        let active = self.registry.pill_items().iter().any(|i| matches!(i, PillItem::Equaliser { active: true }));
        let present = self.registry.pill_items().iter().any(|i| matches!(i, PillItem::Equaliser { .. }));
        !self.open && present && (active || self.eq_level > 0.001)
    }

    /// Advances the equaliser clock: it runs while playing and the bars sink to rest when paused.
    fn step_equaliser(&mut self) {
        let now = self.start.elapsed().as_secs_f32();
        let dt = (now - self.eq_prev).clamp(0.0, 0.25);
        self.eq_prev = now;
        let playing = self.registry.pill_items().iter().any(|i| matches!(i, PillItem::Equaliser { active: true }));
        if playing {
            self.eq_t += dt;
            self.eq_level = (self.eq_level + dt * 3.0).min(1.0);
        } else {
            self.eq_level = (self.eq_level - dt * 3.0).max(0.0);
        }
    }

    /// Passes a pointer event to the current page (coordinates relative to the notch).
    pub fn page_event(&mut self, e: &Event) -> bool {
        self.page_reaction(e) != Reaction::None
    }

    /// True while the current page's text box has keyboard focus.
    pub fn text_focus(&self) -> bool {
        self.registry.current_page().is_some_and(|m| m.wants_text())
    }

    /// Where the text caret is, relative to the notch's top-left (DIPs).
    pub fn ime_anchor(&self) -> Option<(f32, f32)> {
        let (w, h, _) = self.dims();
        let body = ui::page_area(Rect { x: 0.0, y: 0.0, w, h });
        let (cx, cy) = self.registry.current_page()?.text_caret()?;
        Some((body.x + cx, body.y + cy))
    }

    /// A new clipboard text arrived; returns whether to redraw.
    pub fn clipboard_changed(&mut self, text: &str) -> bool {
        self.registry.clipboard_changed(text, epoch_ms())
    }

    /// Files dropped on the notch go to the Shelf.
    pub fn files_dropped(&mut self, paths: &[String]) -> bool {
        self.registry.goto_id("shelf");
        self.registry.set_dragging(false);
        self.registry.files_dropped(paths, epoch_ms())
    }

    /// Files are being dragged over the notch: open it on the Shelf.
    pub fn drag_hover(&mut self, over: bool) {
        if over {
            self.registry.goto_id("shelf");
        }
        self.registry.set_dragging(over);
        self.pointer(over);
    }

    pub fn take_drag(&mut self) -> Option<Vec<String>> {
        self.registry.take_drag()
    }

    pub fn take_copy_files(&mut self) -> Option<Vec<String>> {
        self.registry.take_copy_files()
    }

    /// Text a page asked to copy during the last click.
    pub fn take_copy(&mut self) -> Option<String> {
        self.registry.take_copy()
    }

    pub fn page_reaction(&mut self, e: &Event) -> Reaction {
        if !self.open {
            return Reaction::None;
        }
        let (w, h, _) = self.dims();
        let body = ui::page_area(Rect { x: 0.0, y: 0.0, w, h });
        let now = epoch_ms();
        match self.registry.current_page_mut() {
            Some(page) => page.on_event(e, body, now),
            None => Reaction::None,
        }
    }

    /// Mouse wheel over the notch: the page gets it first (lists scroll); otherwise it turns the
    /// page (wheel down = next). One turn per 150 ms so a trackpad flick moves a single page.
    pub fn wheel(&mut self, steps: f32) -> bool {
        if !self.open || steps == 0.0 {
            return false;
        }
        if self.page_event(&Event::PointerWheel(steps)) {
            return true;
        }
        let now = epoch_ms();
        if now - self.wheel_turned_ms < 150 {
            return false;
        }
        let before = self.registry.current();
        let dir = if steps < 0.0 { 1 } else { -1 };
        self.registry.go(dir);
        if self.registry.current() == before {
            return false;
        }
        self.wheel_turned_ms = now;
        self.trans = Some((before, dir as f32, now));
        true
    }

    /// Handles a left click; coordinates are DIPs relative to the notch's top-left.
    /// Returns whether a redraw is needed.
    pub fn click(&mut self, x: f32, y: f32) -> bool {
        if !self.open {
            return false;
        }
        let (w, h, _) = self.dims();
        let area = Rect { x: 0.0, y: 0.0, w, h };
        let before = self.registry.current();
        let dir = match chrome_hit(area, x, y, self.registry.page_count()) {
            Some(ChromeAction::Prev) => {
                self.registry.go(-1);
                -1.0
            }
            Some(ChromeAction::Next) => {
                self.registry.go(1);
                1.0
            }
            Some(ChromeAction::Dot(i)) => {
                self.registry.set_page(i);
                if i > before { 1.0 } else { -1.0 }
            }
            None => return self.page_event(&Event::PointerDown(x, y)),
        };
        if self.registry.current() != before {
            self.trans = Some((before, dir, epoch_ms()));
        }
        true
    }

    /// Draws the notch shape and its content. `win_w` is the window width in DIPs.
    pub fn paint(&mut self, c: &mut dyn Canvas, win_w: f32) {
        self.step_equaliser();
        let (w, h, r) = self.dims();
        let x = ((win_w - w) / 2.0).max(0.0);
        let shape = Rect { x, y: -r, w, h: h + r };
        if self.settings.glass {
            // Frosted glass: a dark see-through tint over the blur the window asks the system for,
            // a faint sheen that fades down the shape, and thin lit edges.
            // rgba(18,20,26,.5) tint, a sheen fading from white .12 to .02 over the height (stepped
        // bands, since the canvas has no gradient), and 1px edges on the sides and bottom.
        let light = self.settings.glass_light;
        self.paint_glass_shadow(c, x, w, h, r);
        // Dark: rgba(18,20,26,.5) tint, sheen white .12 -> .02. Light: rgba(246,247,250,.5) tint,
        // sheen white .55 -> .25, brighter edges (the artifact's two glass variants).
        c.fill_round_rect(shape, r, if light { Color(246, 247, 250, 128) } else { Color(18, 20, 26, 128) });
        c.push_clip(shape, r);
        // One flat layer, plus 25 thin layers that each end a little earlier: the sum fades down
        // the shape with no visible seams.
        c.fill_round_rect(shape, 0.0, Color(255, 255, 255, if light { 64 } else { 5 }));
        const LAYERS: i32 = 25;
        for i in 0..LAYERS {
            let bottom = h * (i + 1) as f32 / LAYERS as f32;
            c.fill_round_rect(Rect { x, y: 0.0, w, h: bottom }, 0.0, Color(255, 255, 255, if light { 3 } else { 1 }));
        }
        let (edge, lit) = if light { (178, 204) } else { (41, 36) };
        c.fill_round_rect(Rect { x, y: h - 1.0, w, h: 1.0 }, 0.0, Color(255, 255, 255, lit));
        c.fill_round_rect(Rect { x, y: 0.0, w: 1.0, h }, 0.0, Color(255, 255, 255, edge));
        c.fill_round_rect(Rect { x: x + w - 1.0, y: 0.0, w: 1.0, h }, 0.0, Color(255, 255, 255, edge));
        c.pop_clip();
        } else {
            c.fill_round_rect(shape, r, Color(0, 0, 0, 255));
        }
        // Everything drawn from here on may be remapped for the see-through background.
        c.set_glass(self.settings.glass, self.settings.glass_light);

        // Progress of the open/close from the height, which does not change when the lyrics
        // column widens the open panel (a width-based value jumped and flashed the pill).
        let (_, closed_h, _) = self.closed_now();
        let open_h = target(true, self.wide, self.lyrics).size.h;
        let p = ((h - closed_h) / (open_h - closed_h)).clamp(0.0, 1.0);
        let t = local_time();
        let clock = format_clock(t.hour, t.minute);
        self.registry.prepare_all(c);

        let base_h = self.settings.pill_height;
        let gf = ((h - base_h) / (CARD_H - base_h)).clamp(0.0, 1.0);
        if p < 0.98 {
            let idle = (1.0 - gf * 2.0).clamp(0.0, 1.0);
            let a = ((1.0 - p * 1.6).clamp(0.0, 1.0) * idle * 255.0) as u8;
            let sleep = self.resting();
            let a_text = (a as f32 * (1.0 - sleep)) as u8;
            if sleep > 0.0 && a > 0 {
                let ea = (a as f32 * sleep) as u8;
                let e = rest::eyes(self.now_ms());
                let (cy, gap, r) = (base_h / 2.0, 9.0, 5.0);
                for k in [-1.0f32, 1.0] {
                    let cx = x + w / 2.0 + k * (gap + r);
                    let hh = (2.0 * r * e.open).max(1.5);
                    let eye = Rect { x: cx - r, y: cy - hh / 2.0, w: 2.0 * r, h: hh };
                    c.fill_round_rect(eye, hh.min(2.0 * r) / 2.0, Color(245, 246, 248, ea));
                    if e.open > 0.45 {
                        c.push_clip(eye, hh.min(2.0 * r) / 2.0);
                        c.fill_circle(cx + e.look_x * 2.4, cy + e.look_y * 1.8, 2.3, Color(0, 0, 0, ea));
                        c.pop_clip();
                    }
                }
            }
            let osd_now = self.osd_now();
            if let (Some(o), true) = (osd_now, a > 0) {
                draw_osd(c, x, w, base_h, o, a);
            } else if a_text > 0 {
                let (text, accent) = self.registry.pill_text(&clock);
                let color = if accent { Color(52, 211, 153, a_text) } else { Color(245, 246, 248, a_text) };
                let room = Rect { x: x + 30.0, y: 0.0, w: (w - 60.0).max(1.0), h: base_h };
                let full_w = c.measure_text(&text, 12.0, 600);
                match self.registry.pill_battery(epoch_ms()) {
                    Some(b) => {
                        let text = fit_text(c, &text, 12.0, 600, room.w);
                        draw_text_with_battery(c, &text, room, base_h, color, a, b, epoch_ms())
                    }
                    None if full_w > room.w => {
                        // Too long for the pill: glide the text sideways until its end shows.
                        self.pill_rolling_ms = epoch_ms();
                        let dx = scroll_offset(full_w - room.w, epoch_ms() - self.pill_since);
                        c.push_clip(room, 0.0);
                        c.draw_text(&text, Rect { x: room.x - dx, w: full_w + 24.0, ..room }, 12.0, 600, color, Align::Left);
                        c.pop_clip();
                    }
                    None => c.draw_text(&text, room, 12.0, 600, color, Align::Center),
                }
                let alpha = a_text as f32 / 255.0;
                // Left: album art. Right: equaliser. Middle: the text above.
                let items = self.registry.pill_items();
                let has_tile = items.iter().any(|i| matches!(i, PillItem::AppTile { .. }));
                for item in items {
                    match item {
                        // A notification's logo takes the cover's place while it shows.
                        PillItem::MiniCover(_) if has_tile => {}
                        PillItem::AppTile { image, letter, color } => {
                            let r = Rect { x: x + 10.0, y: (base_h - 16.0) / 2.0, w: 16.0, h: 16.0 };
                            match image {
                                Some(id) => {
                                    c.fill_round_rect(r, 4.0, Color(43, 47, 54, a_text));
                                    c.draw_image_alpha(id, r, 4.0, alpha);
                                }
                                None => {
                                    c.fill_round_rect(r, 4.0, Color(color.0, color.1, color.2, a_text));
                                    c.draw_text(&letter.to_string(), Rect { y: r.y + 1.0, h: 14.0, ..r }, 10.0, 700, Color(245, 246, 248, a_text), Align::Center);
                                }
                            }
                        }
                        PillItem::MiniCover(id) => {
                            c.draw_image_alpha(id, Rect { x: x + 10.0, y: (base_h - 15.0) / 2.0, w: 15.0, h: 15.0 }, 4.0, alpha);
                        }
                        PillItem::Equaliser { .. } => {
                            let ea = (a_text as f32 * self.eq_level) as u8;
                            let t = self.eq_t;
                            for i in 0..3 {
                                let bar = 3.0 + 9.0 * equaliser::bar(t, i) * self.eq_level;
                                c.fill_round_rect(Rect { x: x + w - 20.0 + i as f32 * 4.0, y: (base_h + 12.0) / 2.0 - bar, w: 2.0, h: bar }, 1.0, Color(52, 211, 153, ea));
                            }
                            draw_pause_glyph(c, x + w - 20.0, (base_h - 10.0) / 2.0, (a_text as f32 * (1.0 - self.eq_level)) as u8);
                        }
                    }
                }
            }
            if gf > 0.5 && p < 0.05 {
                if let Some(card) = self.card_last.clone() {
                    let ca = (((gf - 0.5) * 2.0).clamp(0.0, 1.0) * 255.0) as u8;
                    let l = track_card::layout(x, w);
                    c.push_clip(Rect { x, y: 0.0, w, h }, 0.0);
                    match card.cover {
                        Some(id) => c.draw_image_alpha(id, l.cover, 9.0, ca as f32 / 255.0),
                        None => c.fill_round_rect(l.cover, 9.0, Color(43, 47, 54, ca)),
                    }
                    let title = fit_text(c, &card.title, 13.0, 600, l.title.w);
                    c.draw_text(&title, l.title, 13.0, 600, Color(245, 246, 248, ca), Align::Left);
                    let sub = fit_text(c, &card.sub, 11.0, 400, l.sub.w);
                    c.draw_text(&sub, l.sub, 11.0, 400, Color(139, 144, 153, ca), Align::Left);
                    let t = self.eq_t;
                    let ea = (ca as f32 * self.eq_level) as u8;
                    for i in 0..3 {
                        let bar = 3.0 + 9.0 * equaliser::bar(t, i) * self.eq_level;
                        c.fill_round_rect(Rect { x: l.eq.x + i as f32 * 4.0, y: l.eq.y + l.eq.h - bar, w: 2.0, h: bar }, 1.0, Color(52, 211, 153, ea));
                    }
                    draw_pause_glyph(c, l.eq.x, l.eq.y + l.eq.h - 11.0, (ca as f32 * (1.0 - self.eq_level)) as u8);
                    c.fill_round_rect(l.bar, 1.5, Color(43, 47, 54, ca));
                    if card.progress > 0.0 {
                        c.fill_round_rect(Rect { w: (l.bar.w * card.progress).max(3.0), ..l.bar }, 1.5, Color(245, 246, 248, ca));
                    }
                    c.pop_clip();
                }
            }
        }
        if p > 0.05 {
            let alpha = ((p - 0.3) / 0.7).clamp(0.0, 1.0);
            let area = Rect { x, y: 0.0, w, h };
            let date = format_date(t.year, t.month, t.day, t.weekday);
            let pages = self.registry.page_count();
            let current = self.registry.current();
            let title = self.registry.current_page().map(|m| m.title()).unwrap_or("");
            ui::draw_chrome(c, area, &Header { title, date: &date, time: &clock }, pages, current, self.trans.filter(|&(_, _, t0)| !page_transition::finished(epoch_ms() - t0)).map(|(b, _, t0)| (b, epoch_ms() - t0)), alpha);
            if alpha > 0.5 {
                let now = epoch_ms();
                let body = ui::page_area(area);
                let slide = self.trans.filter(|&(_, _, t0)| !page_transition::finished(now - t0));
                c.push_clip(body, 0.0);
                if let Some((from, dir, t0)) = slide {
                    let (leaving, arriving) = page_transition::frame(now - t0, dir);
                    if leaving.alpha > 0.0 {
                        c.push_shift(leaving.dx, leaving.alpha);
                        if let Some(page) = self.registry.page_at_mut(from) {
                            page.draw_panel(c, body, now);
                        }
                        c.pop_shift();
                    }
                    if arriving.alpha > 0.0 {
                        c.push_shift(arriving.dx, arriving.alpha);
                        if let Some(page) = self.registry.current_page_mut() {
                            page.draw_panel(c, body, now);
                        }
                        c.pop_shift();
                    }
                } else if let Some(page) = self.registry.current_page_mut() {
                    page.draw_panel(c, body, now);
                }
                c.pop_clip();
            }
        }
    }

    pub fn step(&mut self) -> Step {
        let now = Instant::now();
        let dt = self.last.map(|l| (now - l).as_secs_f32()).unwrap_or(1.0 / 60.0).min(0.05);
        self.last = Some(now);

        let want_open = self.hover.update(self.now_ms()) || self.text_focus();
        let mut opened = false;
        if want_open != self.open {
            self.w.set_damping(22.0);
            self.open = want_open;
            opened = want_open;
            let (w, h, r) = if want_open {
                let t = target(true, self.wide, self.lyrics);
                (t.size.w, t.size.h, t.radius)
            } else {
                self.closed_now()
            };
            self.w.target = w;
            self.h.target = h;
            self.r.target = r;
        }
        self.w.step(dt);
        self.h.step(dt);
        self.r.step(dt);

        let settled = self.w.is_settled() && self.h.is_settled() && self.r.is_settled();
        if settled {
            self.w.snap();
            self.h.snap();
            self.r.snap();
        }
        let fully_closed = settled && !self.open && self.window_big;
        let hover_pending = isle_logic::layout::hover_pending(self.inside, self.open, self.text_focus());
        if !settled {
            // Keep a frame cadence while moving.
        } else {
            self.last = None;
        }
        let sliding = self.open && self.trans.is_some_and(|(_, _, t0)| !page_transition::finished(epoch_ms() - t0));
        if !sliding {
            self.trans = None;
        }
        Step { opened, fully_closed, keep_ticking: !settled || hover_pending || sliding }
    }
}

/// The closed pill grows for a long peek only up to this width; beyond it the text scrolls.
const PILL_MAX_TEXT_W: f32 = 330.0;

/// Width of the closed pill while the volume / brightness bar shows.
const OSD_W: f32 = 236.0;

/// The pill's volume / brightness bar: icon on the left, the bar, then the number.
fn draw_osd(c: &mut dyn Canvas, x: f32, w: f32, pill_h: f32, o: PillOsd, a: u8) {
    let alpha = (a as f32 * o.alpha) as u8;
    let (tint, dim) = match (o.kind, o.muted) {
        (_, true) => ((139, 144, 153), (60, 64, 71)),
        (OsdKind::Volume, false) => ((245, 246, 248), (70, 74, 82)),
        (OsdKind::Brightness, false) => ((245, 246, 248), (70, 74, 82)),
    };
    let col = |t: (u8, u8, u8), al: u8| Color(t.0, t.1, t.2, al);
    let cy = pill_h / 2.0;
    let (ix, iy) = (x + 14.0, cy - 8.0);
    let ic = col(tint, alpha);
    match o.kind {
        OsdKind::Volume => {
            let p = |px: f32, py: f32| (ix + px, iy + py);
            c.fill_polygon(&[p(0.0, 5.5), p(3.5, 5.5), p(8.0, 1.5), p(8.0, 14.5), p(3.5, 10.5), p(0.0, 10.5)], ic);
            if o.muted || o.level <= 0.0 {
                let (x0, y0) = p(10.5, 5.5);
                osd_line(c, x0, y0, x0 + 5.0, y0 + 5.0, 1.6, ic);
                osd_line(c, x0 + 5.0, y0, x0, y0 + 5.0, 1.6, ic);
            } else {
                let on = osd::waves(o.level, false);
                for (i, r) in [3.6f32, 6.4, 9.2].into_iter().enumerate() {
                    let al = if (i as u8) < on { alpha } else { (alpha as f32 * 0.22) as u8 };
                    c.stroke_arc(ix + 7.0, iy + 8.0, r, -42.0, 84.0, 1.5, col(tint, al));
                }
            }
        }
        OsdKind::Brightness => {
            let (cx, cyy) = (ix + 8.0, iy + 8.0);
            let k = 0.55 + 0.45 * o.level;
            c.fill_circle(cx, cyy, 3.0 + 0.9 * k, ic);
            for i in 0..8 {
                let ang = i as f32 * std::f32::consts::FRAC_PI_4;
                let (s, co) = ang.sin_cos();
                let (r0, r1) = (5.6 + 0.4 * k, 6.4 + 1.8 * k);
                osd_line(c, cx + co * r0, cyy + s * r0, cx + co * r1, cyy + s * r1, 1.5, col(tint, (alpha as f32 * (0.4 + 0.6 * o.level)) as u8));
            }
        }
    }
    let pct = format!("{}", if o.muted { 0 } else { (o.level * 100.0).round() as u32 });
    let num = Rect { x: x + w - 14.0 - 30.0, y: 0.0, w: 30.0, h: pill_h };
    c.draw_text(&pct, num, 12.0, 600, Color(245, 246, 248, alpha), Align::Right);
    let bar = Rect { x: ix + 16.0 + 12.0, y: cy - 2.0, w: (num.x - 10.0) - (ix + 28.0), h: 4.0 };
    c.fill_round_rect(bar, 2.0, col(dim, alpha));
    let fill = if o.muted { 0.0 } else { o.level };
    if fill > 0.0 {
        c.fill_round_rect(Rect { w: (bar.w * fill).max(4.0), ..bar }, 2.0, ic);
    }
}

/// A straight line of the given width, as a quad.
fn osd_line(c: &mut dyn Canvas, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, col: Color) {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let (nx, ny) = (-dy / len * width / 2.0, dx / len * width / 2.0);
    c.fill_polygon(&[(x0 + nx, y0 + ny), (x1 + nx, y1 + ny), (x1 - nx, y1 - ny), (x0 - nx, y0 - ny)], col);
}

fn closed_dims(settings: &Settings, wide: bool) -> (f32, f32, f32) {
    let w = if wide { settings.pill_width + 44.0 } else { settings.pill_width };
    (w, settings.pill_height, 13.0)
}

/// Centre text followed by a neat battery icon and a percentage that counts up as the icon fills.
#[allow(clippy::too_many_arguments)]
fn draw_text_with_battery(c: &mut dyn Canvas, text: &str, room: Rect, pill_h: f32, text_color: Color, alpha: u8, b: PillBattery, now: i64) {
    let (shown, fill) = battery_anim(b.percent, now - b.since_ms);
    let tint = match level_of(b.percent) {
        Level::Good => (52, 211, 153),
        Level::Warn => (245, 196, 81),
        Level::Low => (255, 107, 107),
    };
    let tint = Color(tint.0, tint.1, tint.2, alpha);
    let final_label = format!("{}%", b.percent);
    let (tw, pw) = (c.measure_text(text, 12.0, 600).ceil(), c.measure_text(&final_label, 12.0, 600).ceil());
    const ICON_W: f32 = 20.0;
    const NUB: f32 = 2.0;
    let group = tw + 7.0 + ICON_W + NUB + 6.0 + pw;
    let gx = (room.x + (room.w - group) / 2.0).max(room.x).round();
    let iy = ((pill_h - 10.0) / 2.0).round();
    c.draw_text(text, Rect { x: gx, y: 0.0, w: tw + 2.0, h: pill_h }, 12.0, 600, text_color, Align::Left);
    let ix = gx + tw + 7.0;
    // Body: a tinted outline around a black cavity, then the fill, then the terminal nub.
    c.fill_round_rect(Rect { x: ix, y: iy, w: ICON_W, h: 10.0 }, 2.6, tint);
    c.fill_round_rect(Rect { x: ix + 1.3, y: iy + 1.3, w: ICON_W - 2.6, h: 7.4 }, 1.5, Color(0, 0, 0, 255));
    if fill > 0.0 {
        let inner = ICON_W - 4.8;
        c.fill_round_rect(Rect { x: ix + 2.4, y: iy + 2.4, w: (inner * fill).max(1.2), h: 5.2 }, 1.0, tint);
    }
    c.fill_round_rect(Rect { x: ix + ICON_W + 0.6, y: iy + 3.0, w: NUB, h: 4.0 }, 1.0, tint);
    c.draw_text(&format!("{shown}%"), Rect { x: ix + ICON_W + NUB + 6.0, y: 0.0, w: pw + 4.0, h: pill_h }, 12.0, 600, tint, Align::Left);
}

/// Two short bars (a pause symbol) at `x`,`y`, 10 px tall, that the equaliser bars melt into.
fn draw_pause_glyph(c: &mut dyn Canvas, x: f32, y: f32, alpha: u8) {
    if alpha == 0 {
        return;
    }
    let col = Color(139, 144, 153, alpha);
    c.fill_round_rect(Rect { x: x + 1.0, y, w: 3.0, h: 10.0 }, 1.2, col);
    c.fill_round_rect(Rect { x: x + 7.0, y, w: 3.0, h: 10.0 }, 1.2, col);
}
