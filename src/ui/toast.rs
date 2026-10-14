//! A brief message that emerges from the title pill, holds, and compresses back into it.
//! A new toast while one is showing cross-fades the text instead of stacking.
use super::theme::Role;
use super::{GlassSpec, Ui};
use crate::gfx::{Rect, Weight};
use crate::spring::{Preset, Spring};
use crate::tokens::tokens;
use std::time::{Duration, Instant};

const SIZE: f32 = 14.0;
const HEIGHT: f32 = 44.0;

pub struct ToastState {
    grow: Spring,
    swap: Spring,
    message: String,
    previous: String,
    hide_at: Option<Instant>,
}

impl ToastState {
    pub fn new() -> ToastState {
        ToastState { grow: Spring::new(Preset::Snappy), swap: Spring::at(Preset::Snappy, 1.0), message: String::new(), previous: String::new(), hide_at: None }
    }

    pub fn show(&mut self, text: &str, now: Instant) {
        if self.grow.value > 0.02 && self.message != text {
            self.previous = std::mem::take(&mut self.message);
            self.swap.snap(0.0);
        }
        self.message = text.to_string();
        self.hide_at = Some(now + Duration::from_millis(tokens().timings.toast_hold as u64));
    }

    pub fn visible(&self) -> bool {
        self.hide_at.is_some() || self.grow.value > 0.003
    }

    /// `from` is the title pill's rect; the toast grows out of it and shrinks back.
    pub fn draw(&mut self, ui: &mut Ui, window: (f32, f32), from: Rect) {
        let now = ui.now;
        let holding = match self.hide_at {
            Some(t) if now >= t => {
                self.hide_at = None;
                false
            }
            Some(t) => {
                ui.wake_at(t);
                true
            }
            None => false,
        };
        let k = self.grow.to(if holding { 1.0 } else { 0.0 }, ui.dt, ui.th.reduce_motion, &mut ui.animating).max(0.0);
        let sw = self.swap.to(1.0, ui.dt, ui.th.reduce_motion, &mut ui.animating);
        if k <= 0.003 {
            return;
        }
        let tw = ui.r.text_width(&self.message, SIZE, Weight::DemiBold);
        let w = (tw + 40.0).min(window.0 - 32.0);
        let dst = Rect::new((window.0 - w) / 2.0, tokens().spacing.m + 4.0, w, HEIGHT);
        let src = Rect::new(from.x + from.w * 0.1, from.y + from.h * 0.1, (from.w * 0.8).max(1.0), (from.h * 0.8).max(1.0));
        let shape = src.lerp(dst, k);
        ui.r.push_alpha((k * 2.5).min(1.0));
        ui.glass(GlassSpec { role: Role::Regular, shapes: vec![(shape, shape.h / 2.0, 0.0)], dim: 0.0, shimmer: -1.0, overlap: 0.0 });
        ui.r.pop();

        let text_a = ((k - 0.4) / 0.6).clamp(0.0, 1.0);
        if text_a > 0.0 {
            ui.r.push_alpha(text_a);
            ui.r.push_clip(dst);
            let lh = ui.r.line_height(SIZE);
            let max_w = window.0 - 72.0;
            let color = ui.th.c.text_primary.0;
            if sw < 0.999 && !self.previous.is_empty() {
                let text = ui.r.elide(&self.previous, SIZE, Weight::DemiBold, max_w);
                let tw = ui.r.text_width(&text, SIZE, Weight::DemiBold);
                ui.r.push_alpha(1.0 - sw);
                ui.r.text(&text, dst.x + (dst.w - tw) / 2.0, dst.y + (dst.h - lh) / 2.0 - 6.0 * sw, SIZE, Weight::DemiBold, color);
                ui.r.pop();
            }
            let text = ui.r.elide(&self.message, SIZE, Weight::DemiBold, max_w);
            let tw = ui.r.text_width(&text, SIZE, Weight::DemiBold);
            ui.r.push_alpha(sw);
            ui.r.text(&text, dst.x + (dst.w - tw) / 2.0, dst.y + (dst.h - lh) / 2.0 + 6.0 * (1.0 - sw), SIZE, Weight::DemiBold, color);
            ui.r.pop();
            ui.r.pop_clip();
            ui.r.pop();
        }
    }
}

impl Default for ToastState {
    fn default() -> Self {
        Self::new()
    }
}
