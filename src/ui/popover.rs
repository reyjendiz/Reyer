//! A Regular-glass popover that grows out of the button that opened it (position, size and corner
//! radius interpolate from the button's shape) and collapses back into it. It is one spring, so
//! reopening mid-close simply turns around with the current velocity.
//!
//! Draw order: the app sets `ui.blocked = popover.is_open()` while drawing the chrome, resets it to
//! `false`, then calls `Popover::draw` last so it sits above everything and receives input.
use super::theme::Role;
use super::{id_n, GlassSpec, Interact, Ui};
use crate::gfx::{Icon, Rect, Weight};
use crate::input::K;
use crate::spring::{Preset, Spring};
use crate::tokens::tokens;

pub struct Popover {
    t: Spring,
    open: bool,
    /// Which popover is showing ("audio", "subtitles", "speed", "settings", "tools", ...).
    pub which: &'static str,
    src: Rect,
    dst: Rect,
    size: (f32, f32),
    /// Widget that gets keyboard focus back when it closes.
    return_focus: u64,
    /// Highlighted row for keyboard navigation in menus.
    pub current: usize,
}

impl Popover {
    pub fn new() -> Popover {
        Popover { t: Spring::new(Preset::Snappy), open: false, which: "", src: Rect::ZERO, dst: Rect::ZERO, size: (0.0, 0.0), return_focus: 0, current: 0 }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// True while open or still collapsing, so the app keeps drawing it.
    pub fn shown(&self) -> bool {
        self.open || self.t.value > 0.002
    }

    pub fn progress(&self) -> f32 {
        self.t.value.max(0.0)
    }

    /// Opens it from the widget `anchor` (a rect remembered by `UiState::rects` last frame).
    pub fn show(&mut self, ui: &Ui, which: &'static str, anchor: u64, size: (f32, f32), window: (f32, f32), current: usize) {
        let Some(&a) = ui.st.rects.get(&anchor) else { return };
        self.which = which;
        self.size = size;
        self.layout(a, window);
        self.return_focus = anchor;
        self.current = current;
        self.open = true;
    }

    /// Above the anchor if it fits, else below it, kept inside the window.
    fn layout(&mut self, a: Rect, window: (f32, f32)) {
        self.src = a;
        let w = self.size.0.min(window.0 - 24.0).max(1.0);
        let h = self.size.1.min(window.1 - 24.0).max(1.0);
        let x = (a.x + a.w / 2.0 - w / 2.0).clamp(12.0, (window.0 - w - 12.0).max(12.0));
        let mut y = a.y - h - 12.0;
        if y < 12.0 {
            y = (a.bottom() + 12.0).min(window.1 - h - 12.0);
        }
        self.dst = Rect::new(x, y.max(12.0), w, h);
    }

    pub fn hide(&mut self, ui: &mut Ui) {
        if self.open {
            self.open = false;
            if self.return_focus != 0 {
                ui.st.focus = self.return_focus;
            }
        }
    }

    /// Draws the shape and calls `content(ui, body, progress)` once the shape is about 30% open.
    /// Clicks outside and Escape close it; clicks inside stay inside.
    pub fn draw(&mut self, ui: &mut Ui, content: impl FnOnce(&mut Ui, &mut usize, Rect, f32)) {
        if self.open {
            if ui.take_key(K::Escape).is_some() {
                self.hide(ui);
            } else if ui.input.pressed && ui.input.mouse_in && !self.dst.contains(ui.input.mouse.0, ui.input.mouse.1) {
                self.hide(ui);
            }
        }
        // follow the anchor when the window is resized or the dock re-lays out
        if let Some(&a) = ui.st.rects.get(&self.return_focus) {
            let window = (ui.r.view.0 / ui.r.dpi, ui.r.view.1 / ui.r.dpi);
            self.layout(a, window);
        }
        let k = ui.spring(&mut self.t, if self.open { 1.0 } else { 0.0 }).max(0.0);
        if !self.shown() {
            return;
        }
        let kc = k.min(1.0);
        let shape = self.src.lerp(self.dst, kc);
        let radius = (self.src.h / 2.0) + (tokens().radii.popover - self.src.h / 2.0) * kc;
        ui.r.push_alpha((k * 3.0).min(1.0));
        ui.glass(GlassSpec { role: Role::Regular, shapes: vec![(Rect::new(shape.x, shape.y, shape.w.max(1.0), shape.h.max(1.0)), radius, 0.0)], dim: 0.0, shimmer: -1.0, overlap: 1.0 });
        ui.r.pop();

        // clicks inside the popover must never reach the video or the chrome below it. This is a flag,
        // not a widget: a catching widget would own `st.active` and starve every control inside.
        if self.open && shape.contains(ui.input.mouse.0, ui.input.mouse.1) {
            ui.over_chrome = true;
        }
        let body_a = ((k - 0.3) / 0.6).clamp(0.0, 1.0);
        if body_a > 0.0 {
            // the content travels with the glass as it grows out of the button, clipped to the current shape
            let c = self.dst.center();
            ui.r.push(c, 0.96 + 0.04 * kc, (shape.x - self.dst.x, shape.y - self.dst.y), body_a);
            ui.r.push_clip(Rect::new(self.dst.x, self.dst.y, shape.w, shape.h));
            // controls stay inert until the popover is nearly open (and while it collapses)
            let was_blocked = ui.blocked;
            ui.blocked = was_blocked || !self.open || k < 0.7;
            content(ui, &mut self.current, self.dst, k);
            ui.blocked = was_blocked;
            ui.r.pop_clip();
            ui.r.pop();
        }
    }
}

impl Default for Popover {
    fn default() -> Self {
        Self::new()
    }
}

pub struct MenuItem {
    pub label: String,
    pub checked: bool,
}

pub const ROW_H: f32 = 34.0;
pub const TITLE_H: f32 = 30.0;

/// Height a menu with `rows` rows (and an optional title) needs inside a popover.
pub fn menu_height(title: bool, rows: usize) -> f32 {
    let s = tokens().spacing.s;
    (if title { TITLE_H } else { 0.0 }) + rows as f32 * ROW_H + 2.0 * s
}

/// A checkable list for track and speed menus. Arrow keys move, Enter/Space choose; rows stagger in
/// with the popover's progress. Returns the index of the chosen row.
pub fn menu(ui: &mut Ui, current: &mut usize, body: Rect, progress: f32, title: &str, items: &[MenuItem]) -> Option<usize> {
    let s = tokens().spacing.s;
    let m = tokens().spacing.m;
    let mut chosen = None;
    let mut y = body.y + s;
    if !title.is_empty() {
        let lh = ui.r.line_height(12.0);
        ui.r.text(title, body.x + s + m, y + (TITLE_H - lh) / 2.0, 12.0, Weight::DemiBold, ui.th.c.text_secondary.0);
        y += TITLE_H;
    }
    if !items.is_empty() {
        *current = (*current).min(items.len() - 1);
    }
    if ui.take_key(K::Up).is_some() {
        *current = current.saturating_sub(1);
        ui.set_nav(true);
    }
    if ui.take_key(K::Down).is_some() && !items.is_empty() {
        *current = (*current + 1).min(items.len() - 1);
        ui.set_nav(true);
    }
    if !items.is_empty() && (ui.take_key(K::Enter).is_some() || ui.take_key(K::Space).is_some()) {
        chosen = Some(*current);
    }
    let radius = tokens().radii.popover - s;
    for (i, item) in items.iter().enumerate() {
        let row = Rect::new(body.x + s, y, body.w - 2.0 * s, ROW_H);
        y += ROW_H;
        let a = ((progress - 0.35 - i as f32 * 0.04) / 0.4).clamp(0.0, 1.0);
        let r = ui.interact(id_n("menu/row", i as u64), row, Interact { focusable: false, keys: false, press_scale: 1.0, ..Interact::default() });
        if r.hovered && ui.input.moved {
            *current = i;
        }
        if r.clicked {
            chosen = Some(i);
        }
        ui.r.push_alpha(a);
        let keyboard = ui.st.nav && *current == i;
        let hl = if r.hovered { 0.10 } else if keyboard { 0.07 } else { 0.0 };
        if hl > 0.0 {
            let base = if ui.th.light { [0.0, 0.0, 0.0, hl] } else { [1.0, 1.0, 1.0, hl] };
            ui.r.rrect(row, radius.min(row.h / 2.0), base);
        }
        if keyboard {
            ui.r.rrect_stroke(row, row.h / 2.0, 2.0, ui.th.accent);
        }
        if item.checked {
            ui.r.icon(Icon::Named("check"), (row.x + m + 8.0, row.y + row.h / 2.0), 16.0, ui.th.accent);
        }
        let text = ui.r.elide(&item.label, 13.0, Weight::Medium, row.w - m * 2.0 - 24.0);
        let lh = ui.r.line_height(13.0);
        ui.r.text(&text, row.x + m + 24.0, row.y + (row.h - lh) / 2.0, 13.0, Weight::Medium, ui.th.c.text_primary.0);
        ui.r.pop();
    }
    chosen
}
