//! Empty-state background (content layer): calm generated backgrounds drifting slowly and
//! cross-fading every 18 s. Everything stops while the window is inactive or the background hidden.
use super::Ui;
use crate::gfx::gl::Texture;
use crate::gfx::Rect;
use crate::spring::{Ease, Tween};
use std::time::{Duration, Instant};

const COUNT: usize = 4;
const HOLD: Duration = Duration::from_secs(18);

pub struct Ambient {
    tex: [Option<(glow::Texture, u32, u32)>; COUNT],
    current: usize,
    since: Option<Instant>,
    fade: [Tween; COUNT],
    drift: [f32; COUNT],
    shown: Tween,
}

impl Ambient {
    pub fn new() -> Ambient {
        let mut fade = [Tween::new(0.0); COUNT];
        fade[0] = Tween::new(1.0);
        Ambient { tex: [None; COUNT], current: 0, since: None, fade, drift: [0.0; COUNT], shown: Tween::new(0.0) }
    }

    /// Decodes at most one missing image per call, so startup never waits for all four.
    pub fn load_next(&mut self, gl: &glow::Context, path: impl Fn(&str) -> std::path::PathBuf) {
        let Some(i) = self.tex.iter().position(Option::is_none) else { return };
        let img = std::fs::read(path(&format!("assets/backgrounds/ambient_{}.jpg", i + 1)))
            .ok()
            .and_then(|b| image::load_from_memory(&b).ok())
            .map(|i| i.to_rgba8());
        self.tex[i] = Some(match img {
            Some(rgba) => {
                let (w, h) = rgba.dimensions();
                (Texture::from_rgba(gl, w, h, &rgba, true).id, w, h)
            }
            // a missing file leaves a 1x1 dark texture instead of retrying every frame
            None => (Texture::from_rgba(gl, 1, 1, &[11, 11, 14, 255], false).id, 1, 1),
        });
    }

    /// `visible`: no media is open and the launch has progressed far enough; `running`: window active.
    pub fn draw(&mut self, ui: &mut Ui, window: (f32, f32), visible: bool, running: bool) {
        let dt = ui.dt;
        let fade_in = ui.th.fade(500.0);
        let a = self.shown.to(if visible { 1.0 } else { 0.0 }, if visible { fade_in } else { 0.0 }, Ease::OutCubic, dt, &mut ui.animating);
        if a <= 0.01 {
            self.since = None;
            return;
        }
        if running {
            let since = *self.since.get_or_insert(ui.now);
            let next = (self.current + 1) % COUNT;
            if ui.now.duration_since(since) >= HOLD && self.tex[next].is_some() {
                self.current = next;
                self.since = Some(ui.now);
            }
            ui.wake_at(self.since.unwrap_or(ui.now) + HOLD);
        } else {
            self.since = None;
        }
        let (w, h) = window;
        for i in 0..COUNT {
            let k = self.fade[i].to(if i == self.current { 1.0 } else { 0.0 }, 3000.0, Ease::InOutSine, dt, &mut ui.animating);
            let Some((tex, iw, ih)) = self.tex[i] else { continue };
            if k <= 0.0 {
                continue;
            }
            // paused, not stopped: no jump on resume
            if running && !ui.th.reduce_motion {
                self.drift[i] = (self.drift[i] + dt / (60.0 + i as f32 * 9.0)) % 1.0;
                ui.animating = true;
            }
            let ph = self.drift[i] * std::f32::consts::TAU;
            let (dw, dh) = (w * 1.12, h * 1.12);
            let rect = Rect::new((w - dw) / 2.0 + ph.sin() * w * 0.03, (h - dh) / 2.0 + ph.cos() * h * 0.025, dw, dh);
            ui.r.image(tex, rect, crop_uv(dw / dh.max(1.0), iw as f32 / ih.max(1) as f32), 0.0, k * a);
        }
    }
}

impl Default for Ambient {
    fn default() -> Self {
        Self::new()
    }
}

/// uv rectangle that fills a box of aspect `boxa` with an image of aspect `imga` (centre crop).
pub fn crop_uv(boxa: f32, imga: f32) -> [f32; 4] {
    if boxa > imga {
        let v = imga / boxa;
        [0.0, (1.0 - v) / 2.0, 1.0, (1.0 + v) / 2.0]
    } else {
        let u = boxa / imga;
        [(1.0 - u) / 2.0, 0.0, (1.0 + u) / 2.0, 1.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_keeps_the_centre() {
        assert_eq!(crop_uv(2.0, 1.0), [0.0, 0.25, 1.0, 0.75]);
        assert_eq!(crop_uv(0.5, 1.0), [0.25, 0.0, 0.75, 1.0]);
        assert_eq!(crop_uv(1.0, 1.0), [0.0, 0.0, 1.0, 1.0]);
    }
}
