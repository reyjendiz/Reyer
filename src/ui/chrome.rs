//! Top-left title pill and top-right window buttons, both on one glass container (one shader pass).
use super::theme::Role;
use super::{id, BtnStyle, GlassSpec, Ui};
use crate::gfx::{Rect, Weight};
use crate::spring::{Preset, Spring};
use crate::tokens::tokens;

pub const PIN: &str = "chrome/pin";
pub const MINIMIZE: &str = "chrome/minimize";
pub const MAXIMIZE: &str = "chrome/maximize";
pub const CLOSE: &str = "chrome/close";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChromeAction {
    TogglePin,
    Minimize,
    ToggleMaximize,
    Close,
}

pub struct ChromeInput<'a> {
    pub window: (f32, f32),
    pub chrome_t: f32,
    pub dim: f32,
    pub title: &'a str,
    pub subtitle: &'a str,
    pub topmost: bool,
    pub maximized: bool,
    pub fullscreen: bool,
}

pub struct ChromeState {
    pill_w: Spring,
    swap: Spring,
    shown_title: String,
    shown_sub: String,
    old_title: String,
    old_sub: String,
    /// Title pill rect from the last frame (the toast grows out of it).
    pub pill: Rect,
    pub hovered: bool,
}

impl ChromeState {
    pub fn new() -> ChromeState {
        ChromeState {
            pill_w: Spring::new(Preset::Smooth),
            swap: Spring::at(Preset::Snappy, 1.0),
            shown_title: String::new(),
            shown_sub: String::new(),
            old_title: String::new(),
            old_sub: String::new(),
            pill: Rect::new(12.0, 12.0, 120.0, 40.0),
            hovered: false,
        }
    }

    pub fn draw(&mut self, ui: &mut Ui, inp: &ChromeInput) -> Vec<ChromeAction> {
        let t = tokens();
        let m = t.spacing.m;
        let mut acts = Vec::new();
        let height = t.sizes.chrome_height;
        let cluster_w = 4.0 * 34.0 + 2.0 * 3.0;
        let (win_w, _) = inp.window;
        let band = Rect::new(0.0, 0.0, win_w, height + 2.0 * m);
        // hovering the band keeps the chrome visible; only the glass itself blocks the video area
        self.hovered = ui.pointer_in(band);
        let drift = (1.0 - inp.chrome_t) * -8.0;

        // the text cross-fades with a slight vertical slide whenever the title changes
        if inp.title != self.shown_title || inp.subtitle != self.shown_sub {
            self.old_title = std::mem::replace(&mut self.shown_title, inp.title.to_string());
            self.old_sub = std::mem::replace(&mut self.shown_sub, inp.subtitle.to_string());
            self.swap.snap(0.0);
        }
        let sw = ui.spring(&mut self.swap, 1.0);

        let has_title = !inp.title.is_empty();
        let tw = ui.r.text_width(inp.title, 13.0, Weight::DemiBold).max(ui.r.text_width(inp.subtitle, 11.0, Weight::Medium));
        let target = (tw + 36.0).min(win_w * 0.55).min(win_w - cluster_w - 3.0 * m).max(0.0);
        let pw = ui.spring(&mut self.pill_w, target).max(0.0);
        let ph = height + if inp.subtitle.is_empty() { 0.0 } else { 8.0 };
        let pill = Rect::new(m, m + drift, pw, ph);
        let cluster = Rect::new(win_w - cluster_w - m, m + drift, cluster_w, height);
        self.pill = pill;
        if (has_title && ui.pointer_in(pill)) || ui.pointer_in(cluster) {
            ui.over_chrome = true;
        }

        let ct = inp.chrome_t.clamp(0.0, 1.0);
        ui.r.push((win_w / 2.0, 0.0), 0.97 + 0.03 * ct, (0.0, 0.0), ct);
        ui.glass(GlassSpec {
            role: Role::Cluster,
            shapes: vec![(if has_title { pill } else { Rect::ZERO }, ph / 2.0, 0.0), (cluster, height / 2.0, 0.0)],
            dim: inp.dim,
            shimmer: -1.0,
            overlap: 0.0,
        });

        // ---- title pill ----------------------------------------------------------------------------------
        if has_title && pw > 4.0 {
            ui.r.push_clip(pill);
            let max_w = pw - 36.0;
            let (c1, c2) = (ui.th.cv.text_primary.0, ui.th.cv.text_secondary.0);
            let (lh1, lh2) = (ui.r.line_height(13.0), ui.r.line_height(11.0));
            let block = |ui: &mut Ui, title: &str, sub: &str, dy: f32, a: f32| {
                if a <= 0.0 {
                    return;
                }
                let total = lh1 + if sub.is_empty() { 0.0 } else { lh2 };
                let y = pill.y + (pill.h - total) / 2.0 + dy;
                ui.r.push_alpha(a);
                let s = ui.r.elide(title, 13.0, Weight::DemiBold, max_w);
                ui.r.text(&s, pill.x + 18.0, y, 13.0, Weight::DemiBold, c1);
                if !sub.is_empty() {
                    let s = ui.r.elide(sub, 11.0, Weight::Medium, max_w);
                    ui.r.text(&s, pill.x + 18.0, y + lh1, 11.0, Weight::Medium, c2);
                }
                ui.r.pop();
            };
            if sw < 0.999 {
                block(ui, &self.old_title.clone(), &self.old_sub.clone(), -6.0 * sw, 1.0 - sw);
            }
            block(ui, &self.shown_title.clone(), &self.shown_sub.clone(), 6.0 * (1.0 - sw), sw);
            ui.r.pop_clip();
        }

        // ---- window buttons: pin, minimize, maximize/restore, close -------------------------------------
        let by = cluster.y + 3.0;
        let bx = |i: f32| cluster.x + 3.0 + 34.0 * i;
        let pin_label = if inp.topmost { "Don't keep on top" } else { "Keep on top" };
        if ui.icon_button(id(PIN), Rect::new(bx(0.0), by, 34.0, 34.0), "pin", 16.0, BtnStyle { label: pin_label, active: inp.topmost, ..BtnStyle::default() }).clicked {
            acts.push(ChromeAction::TogglePin);
        }
        if ui.icon_button(id(MINIMIZE), Rect::new(bx(1.0), by, 34.0, 34.0), "minimize", 16.0, BtnStyle { label: "Minimize", ..BtnStyle::default() }).clicked {
            acts.push(ChromeAction::Minimize);
        }
        let restored = inp.maximized || inp.fullscreen;
        let (icon, label) = if restored { ("restore", if inp.maximized { "Restore" } else { "Maximize" }) } else { ("maximize", "Maximize") };
        if ui.icon_button(id(MAXIMIZE), Rect::new(bx(2.0), by, 34.0, 34.0), icon, 15.0, BtnStyle { label, ..BtnStyle::default() }).clicked {
            acts.push(ChromeAction::ToggleMaximize);
        }
        if ui.icon_button(id(CLOSE), Rect::new(bx(3.0), by, 34.0, 34.0), "close", 16.0, BtnStyle { label: "Close", danger: true, ..BtnStyle::default() }).clicked {
            acts.push(ChromeAction::Close);
        }
        ui.r.pop();
        acts
    }
}

impl Default for ChromeState {
    fn default() -> Self {
        Self::new()
    }
}
