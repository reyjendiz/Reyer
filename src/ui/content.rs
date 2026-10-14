//! The content layer's own screens: the empty-state card, the drop highlight and the audio
//! "Now Playing" screen. These are standard materials (blur, tint), never Liquid Glass lenses.
//! Video itself is drawn by the shell from mpv's render target.
use super::theme::Role;
use super::{GlassSpec, Ui};
use crate::gfx::gl::Target;
use crate::gfx::{Gfx, Rect, Weight};
use crate::spring::{Ease, Preset, Spring, Tween};
use crate::tokens::tokens;

fn darker(c: [f32; 4], factor: f32) -> [f32; 4] {
    [c[0] / factor, c[1] / factor, c[2] / factor, c[3]]
}

/// Hue tint derived from a hash, like `Qt.hsla(seed % 360 / 360, 0.65, 0.5)`.
pub fn tint_from_seed(seed: i64) -> [f32; 4] {
    let h = (seed.unsigned_abs() % 360) as f32 / 60.0;
    let (s, l) = (0.65f32, 0.5f32);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r + m, g + m, b + m, 1.0]
}

/// Which of the 12 generated tiles an audio file without cover uses.
pub fn tile_index(seed: i64) -> u32 {
    (seed.unsigned_abs() % 12) as u32 + 1
}

pub struct NowPlaying<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
    /// Cover (or generated tile) texture; the app loads and owns it.
    pub cover: Option<glow::Texture>,
    pub has_cover: bool,
    pub mood: [f32; 4],
    pub tint: [f32; 4],
    /// Title pill rect: the cover grows out of it on first open.
    pub pill: Rect,
    /// Bumped by the app whenever the track (and so the cover) changes.
    pub revision: u64,
    /// The blurred, colour-matched background layers (back, front) and the front's opacity.
    pub bg: [Option<glow::Texture>; 2],
    pub bg_front: f32,
}

/// The Now Playing background: a thick, softly blurred, desaturated copy of the cover, cross-fading
/// (400 ms) between the previous and the new one. Rendered once per cover change at 64 px.
pub struct CoverBackdrop {
    layers: Option<[Target; 3]>,
    front: usize,
    key: u64,
    fade: Tween,
}

const BG_SIZE: i32 = 64;

impl CoverBackdrop {
    pub fn new() -> CoverBackdrop {
        CoverBackdrop { layers: None, front: 0, key: u64::MAX, fade: Tween::new(1.0) }
    }

    /// Re-renders when `key` (the current art) changes. Must run outside the scene pass.
    pub fn update(&mut self, g: &mut Gfx, src: glow::Texture, key: u64) {
        if key == self.key {
            return;
        }
        self.key = key;
        let gl = g.r.gl.clone();
        let layers = self.layers.get_or_insert_with(|| [0, 1, 2].map(|_| Target::new(&gl, BG_SIZE, BG_SIZE, true)));
        self.front = 1 - self.front;
        let (dst, tmp) = (&layers[self.front], &layers[2]);
        g.r.copy_pass(src, dst, [0.0, 0.0, 1.0, 1.0], -0.18, 0.25);
        let step = 1.0 / BG_SIZE as f32;
        for _ in 0..4 {
            g.r.blur_pass(dst.tex, tmp, (step, 0.0));
            g.r.blur_pass(tmp.tex, dst, (0.0, step));
        }
        self.fade.snap(0.0);
    }

    /// (back, front) textures and the front's opacity for this frame.
    pub fn layers(&mut self, ui: &mut Ui) -> ([Option<glow::Texture>; 2], f32) {
        let Some(l) = &self.layers else { return ([None, None], 0.0) };
        let ms = ui.th.fade(tokens().timings.track_crossfade);
        let k = self.fade.to(1.0, ms, Ease::OutCubic, ui.dt, &mut ui.animating);
        ([Some(l[1 - self.front].tex), Some(l[self.front].tex)], k)
    }
}

impl Default for CoverBackdrop {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ContentState {
    card_show: Spring,
    enter: Spring,
    old_scale: Spring,
    new_scale: Spring,
    text_in: Spring,
    drop_glow: Spring,
    last_revision: u64,
    was_active: bool,
}

impl ContentState {
    pub fn new() -> ContentState {
        ContentState {
            card_show: Spring::new(Preset::Smooth),
            enter: Spring::new(Preset::Snappy),
            old_scale: Spring::at(Preset::Snappy, 1.0),
            new_scale: Spring::at(Preset::Snappy, 1.0),
            text_in: Spring::at(Preset::Snappy, 1.0),
            drop_glow: Spring::new(Preset::Smooth),
            last_revision: 0,
            was_active: false,
        }
    }

    /// The empty state: a material card with a hint. `rise` is the launch spring (0..1).
    pub fn draw_empty(&mut self, ui: &mut Ui, window: (f32, f32), has_media: bool, rise: f32) {
        let show = ui.spring(&mut self.card_show, if has_media { 0.0 } else { 1.0 });
        let a = show * rise;
        if a <= 0.01 {
            return;
        }
        let (w, h) = (400.0, 136.0);
        let card = Rect::new((window.0 - w) / 2.0, (window.1 - h) / 2.0 + (1.0 - rise) * 12.0, w, h);
        ui.r.push(card.center(), 0.96 + 0.04 * show, (0.0, 0.0), a);
        ui.glass(GlassSpec { role: Role::Material, shapes: vec![(card, 26.0, 0.0)], dim: 0.0, shimmer: -1.0, overlap: 0.0 });
        ui.r.rrect_stroke(card, 26.0, 1.0, [1.0, 1.0, 1.0, 0.10]);
        let (t1, t2) = ("Drop a video or audio file here", "or press O");
        let (l1, l2) = (ui.r.line_height(18.0), ui.r.line_height(13.0));
        let gap = tokens().spacing.s;
        let y = card.y + (card.h - (l1 + gap + l2)) / 2.0;
        let (w1, w2) = (ui.r.text_width(t1, 18.0, Weight::DemiBold), ui.r.text_width(t2, 13.0, Weight::Medium));
        ui.r.text(t1, card.x + (card.w - w1) / 2.0, y, 18.0, Weight::DemiBold, ui.th.cv.text_primary.0);
        ui.r.text(t2, card.x + (card.w - w2) / 2.0, y + l1 + gap, 13.0, Weight::Medium, ui.th.cv.text_secondary.0);
        ui.r.pop();
    }

    /// An accent frame while files are dragged over the window.
    pub fn draw_drop_glow(&mut self, ui: &mut Ui, window: (f32, f32), hovering: bool, fullscreen: bool) {
        let k = ui.spring(&mut self.drop_glow, if hovering { 1.0 } else { 0.0 });
        if k <= 0.01 {
            return;
        }
        let radius = if fullscreen { 0.0 } else { tokens().radii.window };
        let full = Rect::new(0.0, 0.0, window.0, window.1);
        let mut c = ui.th.accent;
        let mut fill = c;
        fill[3] = 0.08 * k;
        c[3] *= k;
        ui.r.rrect(full.inset(3.0), (radius - 3.0).max(0.0), fill);
        ui.r.rrect_stroke(full.inset(1.5), radius, 3.0, c);
    }

    /// The audio screen: darkened cover colour, cover growing out of the title pill, text sliding in.
    pub fn draw_now_playing(&mut self, ui: &mut Ui, window: (f32, f32), active: bool, np: &NowPlaying) {
        let e = ui.spring(&mut self.enter, if active { 1.0 } else { 0.0 });
        let sw = ui.spring(&mut self.new_scale, 1.0);
        let so = ui.spring(&mut self.old_scale, 0.94);
        let ti = ui.spring(&mut self.text_in, 1.0);
        if np.revision != self.last_revision {
            self.last_revision = np.revision;
            if active && self.was_active {
                self.old_scale.snap(1.0);
                self.new_scale.snap(0.94);
                self.text_in.snap(0.0);
            }
        }
        if active && !self.was_active {
            self.old_scale.snap(0.94);
            self.text_in.snap(0.0);
        }
        self.was_active = active;
        if e <= 0.003 {
            return;
        }
        let _ = so;
        ui.r.push_alpha(e.min(1.0));
        let bg = darker(np.mood, 3.2);
        let full = Rect::new(0.0, 0.0, window.0, window.1);
        ui.r.rrect(full, 0.0, bg);
        // the copied cover is stretched over the window (sampled right side up: copy keeps row order)
        for (i, layer) in np.bg.iter().enumerate() {
            let a = if i == 0 { 1.0 } else { np.bg_front };
            if let Some(tex) = layer {
                ui.r.image(*tex, full, [0.0, 0.0, 1.0, 1.0], 0.0, a);
            }
        }
        if !np.has_cover {
            ui.r.rrect(full, 0.0, [np.tint[0], np.tint[1], np.tint[2], 0.22]);
        }
        // vibrancy-preserving darkening towards the bottom for legible text
        ui.r.rrect_gradient(full, 0.0, [0.0, 0.0, 0.0, 0.15], [0.0, 0.0, 0.0, 0.45]);

        let s = &tokens().spacing;
        let size = (window.1 * 0.52).min(window.0 * 0.34).min(420.0).max(120.0);
        let gap = s.xl.max(window.0 * 0.05);
        let block = size + gap + (window.0 * 0.36).min(380.0);
        let cover = Rect::new((window.0 - block) / 2.0, (window.1 - size) / 2.0 - window.1 * 0.03, size, size);
        let k = e.clamp(0.0, 1.2);
        let live = Rect::new(
            np.pill.x + 8.0 + (cover.x - np.pill.x - 8.0) * k,
            np.pill.y + 6.0 + (cover.y - np.pill.y - 6.0) * k,
            28.0 + (cover.w - 28.0) * k,
            28.0 + (cover.h - 28.0) * k,
        );
        let radius = (live.w * 0.05).max(8.0);
        ui.r.push(live.center(), sw, (0.0, 0.0), 1.0);
        match np.cover {
            Some(tex) => ui.r.image(tex, live, [0.0, 0.0, 1.0, 1.0], radius, 1.0),
            None => ui.r.rrect(live, radius, np.mood),
        }
        if !np.has_cover {
            // generated tiles are tinted with the hue of the title
            ui.r.rrect(live, radius, [np.tint[0], np.tint[1], np.tint[2], 0.3]);
        }
        ui.r.pop();

        // title, artist and album slide in with a stagger
        let x = cover.right() + gap;
        let max_w = (window.0 - x - s.xl).max(40.0);
        let title_size = (window.0 / 26.0).clamp(20.0, 34.0);
        let lines = ui.r.wrap(np.title, title_size, Weight::Bold, max_w, false);
        let lines: Vec<String> = lines.into_iter().take(2).collect();
        let (lt, la, lb) = (ui.r.line_height(title_size), ui.r.line_height(17.0), ui.r.line_height(15.0));
        let mut total = lt * lines.len() as f32;
        if !np.artist.is_empty() {
            total += s.s + la;
        }
        if !np.album.is_empty() {
            total += s.s + lb;
        }
        let mut y = live.y + live.h / 2.0 - total / 2.0;
        let appear = |order: f32| -> (f32, f32) { ((ti * 1.4 - order * 0.12).clamp(0.0, 1.0), (1.0 - (ti * 1.2 - order * 0.1).min(1.0)) * 12.0) };
        let (a0, d0) = appear(0.0);
        ui.r.push_alpha(a0);
        for line in &lines {
            ui.r.text(line, x, y + d0, title_size, Weight::Bold, [1.0, 1.0, 1.0, 1.0]);
            y += lt;
        }
        ui.r.pop();
        if !np.artist.is_empty() {
            y += s.s;
            let (a, d) = appear(1.0);
            ui.r.push_alpha(a);
            let t = ui.r.elide(np.artist, 17.0, Weight::DemiBold, max_w);
            ui.r.text(&t, x, y + d, 17.0, Weight::DemiBold, [1.0, 1.0, 1.0, 0.78]);
            ui.r.pop();
            y += la;
        }
        if !np.album.is_empty() {
            y += s.s;
            let (a, d) = appear(2.0);
            ui.r.push_alpha(a);
            let t = ui.r.elide(np.album, 15.0, Weight::Medium, max_w);
            ui.r.text(&t, x, y + d, 15.0, Weight::Medium, [1.0, 1.0, 1.0, 0.58]);
            ui.r.pop();
        }
        ui.r.pop();
    }
}

impl Default for ContentState {
    fn default() -> Self {
        Self::new()
    }
}
