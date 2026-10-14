//! Immediate-mode UI: every frame the app describes the interface; persistent per-widget state
//! (springs, focus, drags) lives in `UiState`. Layout is plain arithmetic on logical pixels.
pub mod ambient;
pub mod chrome;
pub mod content;
pub mod dock;
pub mod panels;
pub mod popover;
pub mod theme;
pub mod toast;

use crate::gfx::{GlassParams, Icon, Rect, Renderer, Rgba, Weight};
use crate::input::{Input, KeyPress, K};
use crate::spring::{rubber_band, Preset, Spring};
use crate::tokens::tokens;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use theme::{Role, Theme, Variant};

pub fn id(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub fn id_n(s: &str, n: u64) -> u64 {
    id(s) ^ n.wrapping_add(0x9e37_79b9_7f4a_7c15).wrapping_mul(0x0000_0100_0000_01b3)
}

/// `3:05` or `1:03:05`.
pub fn format_time(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    let (h, m, r) = (s / 3600, s % 3600 / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{r:02}")
    } else {
        format!("{m}:{r:02}")
    }
}

const LONG_PRESS: Duration = Duration::from_millis(450);
const TOOLTIP_DELAY: Duration = Duration::from_millis(650);

/// Persistent state of one widget.
pub struct Widget {
    pub hover: Spring,
    pub press: Spring,
    pub ring: Spring,
    pub thick: Spring,
    pub lens: Spring,
    pub knob: Spring,
    pub settling: bool,
    pub drag: f32,
    vel: f32,
    last_x: f32,
    last_t: Option<Instant>,
    long_fired: bool,
}

impl Widget {
    fn new() -> Widget {
        Widget {
            hover: Spring::new(Preset::Smooth),
            press: Spring::new(Preset::Snappy),
            ring: Spring::new(Preset::Smooth),
            thick: Spring::new(Preset::Snappy),
            lens: Spring::new(Preset::Snappy),
            knob: Spring::new(Preset::Snappy),
            settling: false,
            drag: 0.0,
            vel: 0.0,
            last_x: 0.0,
            last_t: None,
            long_fired: false,
        }
    }
}

#[derive(Default)]
struct Tip {
    pending: Option<(u64, String, Rect, Instant)>,
    suppress: u64,
    touched: bool,
    shown_id: u64,
    text: String,
    anchor: Rect,
}

pub struct UiState {
    widgets: HashMap<u64, Widget>,
    /// The widget being pressed with the mouse.
    pub active: u64,
    /// The widget that has keyboard focus.
    pub focus: u64,
    /// Focus rings show only while the keyboard is being used to move around (like :focus-visible).
    pub nav: bool,
    order: Vec<u64>,
    tip: Tip,
    tip_spring: Spring,
    /// Last frame's widget rects by id, for anchoring popovers.
    pub rects: HashMap<u64, Rect>,
    press_at: Instant,
}

impl UiState {
    pub fn new() -> UiState {
        UiState {
            widgets: HashMap::new(),
            active: 0,
            focus: 0,
            nav: false,
            order: Vec::new(),
            tip: Tip::default(),
            tip_spring: Spring::new(Preset::Smooth),
            rects: HashMap::new(),
            press_at: Instant::now(),
        }
    }

    pub fn widget(&mut self, id: u64) -> &mut Widget {
        self.widgets.entry(id).or_insert_with(Widget::new)
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

/// What a widget reports back after interaction.
#[derive(Default, Clone, Copy)]
pub struct Resp {
    pub hovered: bool,
    pub down: bool,
    pub clicked: bool,
    pub right_clicked: bool,
    pub long_pressed: bool,
    pub focused: bool,
    /// 0..1, springy hover amount
    pub hover: f32,
    /// Press scale spring (1 at rest)
    pub scale: f32,
}

#[derive(Clone, Copy)]
pub struct Interact<'a> {
    pub enabled: bool,
    pub focusable: bool,
    /// Space / Enter activate the widget when it has focus.
    pub keys: bool,
    pub long_press: bool,
    pub press_scale: f32,
    pub label: &'a str,
}

impl Default for Interact<'_> {
    fn default() -> Self {
        Interact { enabled: true, focusable: true, keys: true, long_press: false, press_scale: 0.94, label: "" }
    }
}

pub struct GlassSpec {
    pub role: Role,
    /// (rect, corner radius, glow 0..1)
    pub shapes: Vec<(Rect, f32, f32)>,
    /// Dimming layer for clear glass over bright content (0..0.35).
    pub dim: f32,
    /// Press shimmer sweep position 0..1, < 0 off.
    pub shimmer: f32,
    /// Glass over glass: more opaque so stacked text stays legible.
    pub overlap: f32,
}

pub struct Ui<'a> {
    pub r: &'a mut Renderer,
    pub st: &'a mut UiState,
    pub input: &'a Input,
    pub th: &'a Theme,
    pub dt: f32,
    pub now: Instant,
    /// Some spring or transition is still moving: keep drawing frames.
    pub animating: bool,
    /// Earliest time something needs a redraw even if nothing moves.
    pub wake: Option<Instant>,
    /// Input is blocked below a modal layer.
    pub blocked: bool,
    /// The pointer is over a chrome element, so the video area must not react.
    pub over_chrome: bool,
    used: Vec<bool>,
}

impl<'a> Ui<'a> {
    pub fn new(r: &'a mut Renderer, st: &'a mut UiState, input: &'a Input, th: &'a Theme, dt: f32, now: Instant) -> Ui<'a> {
        st.order.clear();
        st.tip.touched = false;
        Ui { r, st, input, th, dt, now, animating: false, wake: None, blocked: false, over_chrome: false, used: vec![false; input.keys.len()] }
    }

    pub fn wake_at(&mut self, t: Instant) {
        self.wake = Some(self.wake.map_or(t, |w| w.min(t)));
    }

    /// The pointer in the current (transformed) local space.
    pub fn mouse(&self) -> (f32, f32) {
        self.r.untransform(self.input.mouse)
    }

    pub fn pointer_in(&self, rect: Rect) -> bool {
        !self.blocked && self.input.mouse_in && {
            let m = self.mouse();
            rect.contains(m.0, m.1)
        }
    }

    // ---- springs and tweens ---------------------------------------------------------------------------
    pub fn spring(&mut self, s: &mut Spring, target: f32) -> f32 {
        s.to(target, self.dt, self.th.reduce_motion, &mut self.animating)
    }

    // ---- keys ----------------------------------------------------------------------------------------
    /// Takes the first unused key press of `k` (so only one widget reacts to it).
    pub fn take_key(&mut self, k: K) -> Option<KeyPress> {
        let i = self.input.keys.iter().enumerate().position(|(i, p)| p.key == k && !self.used[i])?;
        self.used[i] = true;
        Some(self.input.keys[i])
    }

    /// Key presses nobody consumed, for the global shortcuts.
    pub fn unused_keys(&self) -> Vec<KeyPress> {
        self.input.keys.iter().zip(&self.used).filter(|(_, u)| !**u).map(|(k, _)| *k).collect()
    }

    pub fn has_focus(&self, id: u64) -> bool {
        self.st.focus == id
    }

    // ---- interaction ---------------------------------------------------------------------------------
    pub fn interact(&mut self, id: u64, rect: Rect, o: Interact) -> Resp {
        let enabled = o.enabled && !self.blocked;
        let hovered = enabled && self.pointer_in(rect) && (self.st.active == 0 || self.st.active == id);
        if o.focusable && o.enabled {
            self.st.order.push(id);
        }
        let mut resp = Resp { hovered, ..Resp::default() };
        if hovered && self.input.pressed {
            self.st.active = id;
            if o.focusable {
                self.st.focus = id;
            }
            self.st.nav = false;
            self.st.press_at = self.now;
            self.st.widget(id).long_fired = false;
        }
        let active = self.st.active == id;
        resp.down = active && self.input.down;
        if active && self.input.released && hovered {
            resp.clicked = true;
        }
        if hovered && self.input.right_released {
            resp.right_clicked = true;
        }
        if o.long_press && active && resp.down && hovered {
            let started = self.st.press_at;
            if self.now.duration_since(started) >= LONG_PRESS {
                if !self.st.widget(id).long_fired {
                    self.st.widget(id).long_fired = true;
                    resp.long_pressed = true;
                    resp.clicked = false;
                }
            } else {
                self.wake_at(started + LONG_PRESS);
            }
        }
        if resp.clicked && self.st.widget(id).long_fired {
            resp.clicked = false;
        }
        resp.focused = self.st.focus == id && enabled;
        if resp.focused && o.keys {
            if self.take_key(K::Space).is_some() || self.take_key(K::Enter).is_some() {
                resp.clicked = true;
            }
            if self.take_key(K::Menu).is_some() {
                resp.right_clicked = true;
            }
        }
        // tooltips: a short hover delay, cancelled by pressing or leaving
        if !o.label.is_empty() && hovered && self.st.tip.suppress != id {
            self.st.tip.touched = true;
            if self.input.pressed {
                self.st.tip.suppress = id;
                self.st.tip.pending = None;
                self.st.tip.shown_id = 0;
            } else if self.st.tip.pending.as_ref().map(|p| p.0) != Some(id) && self.st.tip.shown_id != id {
                let anchor = self.r.transformed(rect);
                self.st.tip.pending = Some((id, o.label.to_string(), anchor, self.now));
            }
        }
        let show_ring_hover = hovered || (resp.focused && self.st.nav);
        let reduce = self.th.reduce_motion;
        let dt = self.dt;
        let mut anim = false;
        let w = self.st.widget(id);
        resp.hover = w.hover.to(if show_ring_hover { 1.0 } else { 0.0 }, dt, reduce, &mut anim);
        resp.scale = w.press.to(if resp.down { o.press_scale } else { 1.0 }, dt, reduce, &mut anim);
        self.animating |= anim;
        self.st.rects.insert(id, self.r.transformed(rect));
        resp
    }

    /// Ends the frame's interaction bookkeeping: releases, Tab navigation, tooltips.
    pub fn finish(&mut self) {
        if self.input.released || !self.input.down {
            self.st.active = 0;
        }
        if self.input.moved {
            self.st.tip.suppress = if self.st.tip.suppress != 0 && self.st.tip.touched { self.st.tip.suppress } else { 0 };
        }
        // Tab / Shift+Tab walk the focusable widgets in drawing order
        let shift = self.input.keys.iter().any(|k| k.key == K::Tab && k.shift);
        if self.take_key(K::Tab).is_some() && !self.st.order.is_empty() {
            let order = self.st.order.clone();
            let pos = order.iter().position(|i| *i == self.st.focus);
            let next = match (pos, shift) {
                (Some(p), false) => (p + 1) % order.len(),
                (Some(p), true) => (p + order.len() - 1) % order.len(),
                (None, false) => 0,
                (None, true) => order.len() - 1,
            };
            self.st.focus = order[next];
            self.st.nav = true;
        }
        if !self.st.tip.touched {
            self.st.tip.pending = None;
            self.st.tip.shown_id = 0;
            self.st.tip.suppress = 0;
        }
        if let Some((tid, text, anchor, since)) = self.st.tip.pending.clone() {
            if self.now.duration_since(since) >= TOOLTIP_DELAY {
                self.st.tip.shown_id = tid;
                self.st.tip.text = text;
                self.st.tip.anchor = anchor;
                self.st.tip.pending = None;
            } else {
                self.wake_at(since + TOOLTIP_DELAY);
            }
        }
    }

    /// Marks keyboard use for focus rings (called for navigation keys by the app).
    pub fn set_nav(&mut self, on: bool) {
        self.st.nav = on;
    }

    // ---- drawing helpers -----------------------------------------------------------------------------
    pub fn focus_ring(&mut self, id: u64, rect: Rect, radius: f32, inset: f32) {
        let on = self.st.focus == id && self.st.nav && !self.blocked;
        let (dt, reduce) = (self.dt, self.th.reduce_motion);
        let mut anim = false;
        let k = self.st.widget(id).ring.to(if on { 1.0 } else { 0.0 }, dt, reduce, &mut anim);
        self.animating |= anim;
        if k <= 0.01 {
            return;
        }
        let m = inset - (1.0 - k) * 4.0;
        let mut c = self.th.accent;
        c[3] *= k;
        self.r.rrect_stroke(rect.inset(m), radius, 2.0, c);
    }

    /// One Liquid Glass container: shapes that melt into each other, one backdrop sample per pixel.
    pub fn glass(&mut self, g: GlassSpec) {
        let t = &tokens().glass;
        let variant = self.th.variant_for(g.role);
        let clear = variant == Variant::Clear;
        let material = variant == Variant::Material;
        let params = if clear { t.clear } else { t.regular };
        let pal = if clear { self.th.cv } else { self.th.c };
        let tint_src = if clear { pal.glass_tint.0 } else { pal.glass_tint_regular.0 };
        let mut tint = tint_src;
        // one transparency for dock, pill, menus and toasts; the slider in Settings moves it
        let o = self.th.glass_opacity.clamp(0.0, 1.0);
        let unified = !material;
        if unified {
            tint[3] = 0.02 + 0.55 * o;
        }
        tint[3] = (tint[3] + g.overlap * t.overlap_opacity_boost * if unified { 0.3 } else { 1.0 }).min(1.0);
        let frosted = self.th.reduce_transparency || self.r.backdrop.is_none();
        let dim = if self.th.increase_contrast && clear { t.dimming } else { g.dim };
        self.r.glass(&GlassParams {
            shapes: g.shapes,
            smooth_union: t.smooth_union,
            // the content-layer material is a plain background blur: no shadow, no rim, no lens
            shadow_opacity: if material { 0.0 } else { t.shadow_opacity },
            shimmer: g.shimmer,
            frosted,
            // Clear shows mostly the sharp content (light lensing at the rim), Regular a soft blur
            blur_mix: match variant {
                Variant::Material => 0.4,
                _ => -0.35 + 1.35 * o,
            },
            refraction: if material { 0.0 } else { params.refraction },
            saturation: params.saturation,
            lift: params.lift,
            tint,
            light_dir: self.th.light_dir,
            dim,
            rim_strength: if material { 0.0 } else { 1.0 },
            frost: pal.frost.0,
            rim_width: t.rim_width,
            rim_high: t.rim_high,
            rim_low: t.rim_low,
            border_alpha: pal.border.0[3],
        });
    }

    // ---- buttons -------------------------------------------------------------------------------------
    /// A control that sits on a glass capsule. Press scales with a snappy spring, hover lifts brightness.
    /// `content` draws inside the pressed transform, given the content color.
    pub fn button(&mut self, id: u64, rect: Rect, style: BtnStyle, content: impl FnOnce(&mut Ui, Rect, Rgba)) -> Resp {
        let o = Interact { enabled: style.enabled, label: style.label, long_press: style.long_press, press_scale: style.press_scale, ..Interact::default() };
        let resp = self.interact(id, rect, o);
        let c = rect.center();
        self.r.push(c, resp.scale * (1.0 + 0.04 * resp.hover), (0.0, 0.0), if style.enabled { 1.0 } else { 0.4 });
        let hl = Rect::new(c.0, c.1, 0.0, 0.0);
        let hw = if style.wide { rect.w - 4.0 } else { rect.w.min(rect.h) - 4.0 };
        let hl = Rect::new(hl.x - hw / 2.0, c.1 - (rect.h - 4.0) / 2.0, hw, rect.h - 4.0);
        let base = if style.danger {
            self.th.cv.danger.0
        } else if style.on_regular && self.th.light {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            [1.0, 1.0, 1.0, 1.0]
        };
        let mut hc = base;
        hc[3] = resp.hover * if style.danger { 0.95 } else { 0.16 };
        if hc[3] > 0.003 {
            self.r.rrect(hl, hl.h / 2.0, hc);
        }
        let color = if style.danger && resp.hovered {
            [1.0, 1.0, 1.0, 1.0]
        } else if style.active {
            self.th.accent
        } else if style.on_regular {
            self.th.c.text_primary.0
        } else {
            self.th.cv.text_primary.0
        };
        content(self, rect, color);
        self.r.pop();
        self.focus_ring(id, rect, rect.h / 2.0, -3.0);
        resp
    }

    pub fn icon_button(&mut self, id: u64, rect: Rect, icon: &'static str, size: f32, style: BtnStyle) -> Resp {
        self.button(id, rect, style, move |ui, r, c| ui.r.icon(Icon::Named(icon), r.center(), size, c))
    }

    pub fn text_button(&mut self, id: u64, rect: Rect, text: &str, size: f32, style: BtnStyle) -> Resp {
        let style = BtnStyle { wide: true, ..style };
        let text = text.to_string();
        self.button(id, rect, style, move |ui, r, c| {
            let w = ui.r.text_width(&text, size, Weight::Medium);
            let h = ui.r.line_height(size);
            ui.r.text(&text, r.x + (r.w - w) / 2.0, r.y + (r.h - h) / 2.0, size, Weight::Medium, c);
        })
    }

    // ---- sliders -------------------------------------------------------------------------------------
    /// Seek / volume slider. The knob is a flat dot at rest and a small glass lens (magnifying the track)
    /// while touched. Dragging follows the pointer 1:1 with rubber-banding past the ends; on release the
    /// knob settles with the pointer's velocity.
    pub fn slider(&mut self, id: u64, rect: Rect, o: SliderOpts) -> SliderResp {
        let hit = Rect::new(rect.x, rect.y - 6.0, rect.w, rect.h + 12.0);
        let r = self.interact(id, hit, Interact { enabled: o.enabled, press_scale: 1.0, keys: false, ..Interact::default() });
        let mut out = SliderResp::default();
        let m = self.mouse();
        let w = rect.w.max(1.0);
        out.hovered = r.hovered;
        out.hover_value = ((m.0 - rect.x) / w).clamp(0.0, 1.0);
        let dragging = r.down && self.st.active == id;
        let now = self.now;
        let (dt, reduce) = (self.dt, self.th.reduce_motion);
        if o.enabled && self.st.active == id && (self.input.pressed || (dragging && self.input.moved)) {
            let x = m.0 - rect.x;
            let wd = self.st.widget(id);
            if self.input.pressed {
                wd.last_x = x;
                wd.last_t = Some(now);
                wd.vel = 0.0;
            }
            let dts = wd.last_t.map_or(0.016, |t| now.duration_since(t).as_secs_f32().max(0.001));
            wd.vel = 0.7 * wd.vel + 0.3 * (x - wd.last_x) / dts;
            wd.last_x = x;
            wd.last_t = Some(now);
            wd.knob.snap(rubber_band(x, 0.0, w, 24.0)); // 1:1 with the pointer, no spring lag
            wd.settling = false;
            wd.drag = (x / w).clamp(0.0, 1.0);
            out.moved = Some(wd.drag);
        }
        if self.st.active == id && self.input.released && o.enabled {
            let wd = self.st.widget(id);
            out.committed = Some(wd.drag);
            wd.settling = true;
            wd.knob.nudge(wd.vel);
            wd.knob.set_target(wd.drag * w, reduce);
        }
        // keyboard: arrows adjust, Page keys take bigger steps
        if r.focused && o.enabled {
            let mut d = 0.0;
            if self.take_key(K::Left).is_some() || self.take_key(K::Down).is_some() {
                d = -o.step;
            } else if self.take_key(K::Right).is_some() || self.take_key(K::Up).is_some() {
                d = o.step;
            } else if self.take_key(K::PageDown).is_some() {
                d = -o.big_step;
            } else if self.take_key(K::PageUp).is_some() {
                d = o.big_step;
            }
            if d != 0.0 {
                let v = (o.value + d).clamp(0.0, 1.0);
                out.moved = Some(v);
                out.committed = Some(v);
                self.st.nav = true;
            }
        }
        let engaged = r.hovered || dragging || (r.focused && self.st.nav);
        let wd = self.st.widget(id);
        out.dragging = dragging;
        out.drag_value = wd.drag;
        let mut anim = false;
        let thick = wd.thick.to(if engaged { 6.0 } else { 4.0 }, dt, reduce, &mut anim);
        let lens = wd.lens.to(if engaged { 1.0 } else { 0.0 }, dt, reduce, &mut anim);
        if wd.settling {
            let was = wd.knob.value;
            wd.knob.to(wd.drag * w, dt, reduce, &mut anim);
            let _ = was;
            if wd.knob.settled() {
                wd.settling = false;
            }
        }
        let shown = if dragging { wd.drag } else { o.value };
        let knob_pos = if dragging || wd.settling { wd.knob.value } else { shown * w };
        self.animating |= anim;
        out.shown = shown;
        // track
        let cy = rect.y + rect.h / 2.0;
        let t = Rect::new(rect.x, cy - thick / 2.0, w, thick.max(1.0));
        self.r.rrect(t, t.h / 2.0, self.th.cv.track.0);
        let fill_w = knob_pos.clamp(t.h, w);
        self.r.rrect(Rect::new(t.x, t.y, fill_w, t.h), t.h / 2.0, self.th.accent);
        for &tick in o.ticks {
            if tick > 0.002 && tick < 0.998 {
                self.r.rrect(Rect::new(rect.x + tick * w - 1.0, t.y, 2.0, t.h), 0.0, [0.0, 0.0, 0.0, 0.45]);
            }
        }
        // the lens knob: grows 12 -> ~24 px and refracts the track below it
        let size = 12.0 + 12.0 * lens;
        let strength = if self.th.reduce_transparency { 0.0 } else { lens };
        let ld = self.th.light_dir;
        self.r.lens((rect.x + knob_pos, cy), size / 2.0, 1.0 + 0.5 * lens, strength, t.h / 2.0, self.th.accent, self.th.cv.track.0, [ld[0], ld[1], 1.0, 0.08]);
        self.focus_ring(id, Rect::new(rect.x, rect.y, rect.w, rect.h), 8.0, -4.0);
        out
    }
}

#[derive(Clone, Copy)]
pub struct BtnStyle<'a> {
    pub label: &'a str,
    pub active: bool,
    pub danger: bool,
    /// Sits on Regular glass (menus): follows the light/dark palette.
    pub on_regular: bool,
    pub enabled: bool,
    pub long_press: bool,
    pub wide: bool,
    pub press_scale: f32,
}

impl Default for BtnStyle<'_> {
    fn default() -> Self {
        BtnStyle { label: "", active: false, danger: false, on_regular: false, enabled: true, long_press: false, wide: false, press_scale: 0.94 }
    }
}

pub struct SliderOpts<'a> {
    pub value: f32,
    pub ticks: &'a [f32],
    pub step: f32,
    pub big_step: f32,
    pub enabled: bool,
}

#[derive(Default, Clone, Copy)]
pub struct SliderResp {
    /// During a drag (or a key press).
    pub moved: Option<f32>,
    /// Drag released / key press.
    pub committed: Option<f32>,
    pub dragging: bool,
    pub hovered: bool,
    pub hover_value: f32,
    pub drag_value: f32,
    pub shown: f32,
}

impl Ui<'_> {
    /// Draws the shared tooltip (last, above everything).
    pub fn draw_tooltip(&mut self, window: (f32, f32)) {
        let shown = self.st.tip.shown_id != 0;
        let (dt, reduce) = (self.dt, self.th.reduce_motion);
        let mut anim = false;
        let k = self.st.tip_spring.to(if shown { 1.0 } else { 0.0 }, dt, reduce, &mut anim);
        self.animating |= anim;
        if k < 0.02 || self.st.tip.text.is_empty() {
            return;
        }
        let text = self.st.tip.text.clone();
        let a = self.st.tip.anchor;
        let (tw, th) = (self.r.text_width(&text, 12.0, Weight::Medium), 26.0);
        let w = tw + 20.0;
        let below = a.y < 60.0;
        let x = (a.x + a.w / 2.0 - w / 2.0).clamp(8.0, (window.0 - w - 8.0).max(8.0));
        // clear of the glass the button sits on, rising slightly towards its place as it appears
        let gap = 16.0;
        let rise = (1.0 - k) * 4.0;
        let y = if below { a.bottom() + gap - rise } else { a.y - th - gap + rise };
        let bubble = Rect::new(x, y, w, th);
        self.r.push(bubble.center(), 0.92 + 0.08 * k.min(1.0), (0.0, 0.0), k.min(1.0));
        self.glass(GlassSpec { role: Role::Regular, shapes: vec![(bubble, th / 2.0, 0.0)], dim: 0.0, shimmer: -1.0, overlap: 0.0 });
        let lh = self.r.line_height(12.0);
        self.r.text(&text, x + 10.0, y + (th - lh) / 2.0, 12.0, Weight::Medium, self.th.c.text_primary.0);
        self.r.pop();
    }
}
