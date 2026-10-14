//! Vector icons on a 24x24 grid, rasterized with tiny-skia at the needed size into the shared atlas,
//! so they stay crisp at any DPI. Animated icons (play/pause, speaker) are cached per quantized state.
use super::text::Atlas;
use std::collections::HashMap;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

/// name -> (svg path data, stroked?)
const ICONS: &[(&str, &str, bool)] = &[
    ("previous", "M6 5h2.2v14H6z M19.5 5.6v12.8L9.4 12z", false),
    ("next", "M15.8 5H18v14h-2.2z M4.5 5.6v12.8L14.6 12z", false),
    ("open", "M3.5 7.5A1.5 1.5 0 0 1 5 6h4l2 2h8a1.5 1.5 0 0 1 1.5 1.5v8A1.5 1.5 0 0 1 19 19H5a1.5 1.5 0 0 1-1.5-1.5z", true),
    ("fullscreen", "M4.5 9V4.5H9 M19.5 9V4.5H15 M4.5 15v4.5H9 M19.5 15v4.5H15", true),
    ("exitFullscreen", "M9 4.5V9H4.5 M15 4.5V9h4.5 M9 19.5V15H4.5 M15 19.5V15h4.5", true),
    ("subtitles", "M5 5.5h14a1.5 1.5 0 0 1 1.5 1.5v10a1.5 1.5 0 0 1-1.5 1.5H5A1.5 1.5 0 0 1 3.5 17V7A1.5 1.5 0 0 1 5 5.5z M7 14.5h4 M13.5 14.5H17 M7 11h2.5 M12 11h5", true),
    ("audio", "M4.5 10v4 M8.25 7v10 M12 4.5v15 M15.75 8v8 M19.5 10.5v3", true),
    ("settings", "M4 7.5h9 M17 7.5h3 M4 16.5h3 M11 16.5h9 M15 5.5v4 M9 14.5v4", true),
    ("more", "M4.4 12a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0-3.2 0z M10.4 12a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0-3.2 0z M16.4 12a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0-3.2 0z", false),
    ("pin", "M9 4.5h6 M10.2 4.5v5.2L7.5 13.5h9l-2.7-3.8V4.5 M12 13.5v6", true),
    ("minimize", "M6.5 12h11", true),
    ("maximize", "M7 6.5h10a.5.5 0 0 1 .5.5v10a.5.5 0 0 1-.5.5H7a.5.5 0 0 1-.5-.5V7a.5.5 0 0 1 .5-.5z", true),
    ("restore", "M6.5 9.5h8v8h-8z M9.5 6.5h8v8", true),
    ("close", "M7 7l10 10 M17 7L7 17", true),
    ("check", "M5.5 12.5l4 4 9-9", true),
];

const SPEAKER_BODY: &str = "M3.5 9.2h3.3L11.5 5v14l-4.7-4.2H3.5z";
const WAVE_1: &str = "M14.6 9.6a3.4 3.4 0 0 1 0 4.8";
const WAVE_2: &str = "M17 7.2a6.8 6.8 0 0 1 0 9.6";
const SLASH: &str = "M15.5 9.5l5 5 M20.5 9.5l-5 5";

/// Steps used to quantize animated icon state.
pub const STATES: u32 = 16;

pub fn play_pause_state(t: f32) -> u32 {
    (t.clamp(0.0, 1.0) * STATES as f32).round() as u32
}

pub fn speaker_state(level: f32) -> u32 {
    (level.clamp(0.0, 1.0) * 4.0).round() as u32
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Icon {
    Named(&'static str),
    /// 0..=STATES: 0 = play triangle, STATES = pause bars
    PlayPause(u32),
    /// three 0..=4 levels: first wave, second wave, mute slash
    Speaker(u32, u32, u32),
}

#[derive(Clone, Copy)]
pub struct IconGlyph {
    pub ax: i32,
    pub ay: i32,
    pub size: i32,
    generation: u32,
}

fn path_from_svg(d: &str) -> Option<Path> {
    use svgtypes::{SimplePathSegment as S, SimplifyingPathParser};
    let mut pb = PathBuilder::new();
    for seg in SimplifyingPathParser::from(d).flatten() {
        match seg {
            S::MoveTo { x, y } => pb.move_to(x as f32, y as f32),
            S::LineTo { x, y } => pb.line_to(x as f32, y as f32),
            S::Quadratic { x1, y1, x, y } => pb.quad_to(x1 as f32, y1 as f32, x as f32, y as f32),
            S::CurveTo { x1, y1, x2, y2, x, y } => pb.cubic_to(x1 as f32, y1 as f32, x2 as f32, y2 as f32, x as f32, y as f32),
            S::ClosePath => pb.close(),
        }
    }
    pb.finish()
}

fn polygon(points: &[(f32, f32)]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(points[0].0, points[0].1);
    for p in &points[1..] {
        pb.line_to(p.0, p.1);
    }
    pb.close();
    pb.finish()
}

struct Canvas {
    pixmap: Pixmap,
    scale: f32,
}

impl Canvas {
    fn paint(alpha: f32) -> Paint<'static> {
        let mut paint = Paint::default();
        paint.set_color_rgba8(255, 255, 255, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8);
        paint.anti_alias = true;
        paint
    }

    fn fill(&mut self, path: &Path, alpha: f32) {
        self.pixmap.fill_path(path, &Self::paint(alpha), FillRule::Winding, Transform::from_scale(self.scale, self.scale), None);
    }

    fn stroke(&mut self, path: &Path, width: f32, alpha: f32) {
        let stroke = Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
        self.pixmap.stroke_path(path, &Self::paint(alpha), &stroke, Transform::from_scale(self.scale, self.scale), None);
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn draw(icon: Icon, c: &mut Canvas) {
    match icon {
        Icon::Named(name) => {
            if let Some((_, d, stroked)) = ICONS.iter().find(|(n, _, _)| *n == name) {
                if let Some(path) = path_from_svg(d) {
                    if *stroked {
                        c.stroke(&path, 1.7, 1.0);
                    } else {
                        c.fill(&path, 1.0);
                    }
                }
            }
        }
        Icon::PlayPause(state) => {
            // the triangle splits into two halves that become the bars (same winding)
            let t = state as f32 / STATES as f32;
            let quad = |from: [(f32, f32); 4], to: [(f32, f32); 4]| -> Vec<(f32, f32)> {
                (0..4).map(|i| (lerp(from[i].0, to[i].0, t), lerp(from[i].1, to[i].1, t))).collect()
            };
            let left = quad([(7.0, 5.0), (13.0, 8.5), (13.0, 15.5), (7.0, 19.0)], [(6.5, 5.0), (10.0, 5.0), (10.0, 19.0), (6.5, 19.0)]);
            let right = quad([(13.0, 8.5), (19.5, 12.0), (19.5, 12.0), (13.0, 15.5)], [(14.0, 5.0), (17.5, 5.0), (17.5, 19.0), (14.0, 19.0)]);
            for pts in [left, right] {
                if let Some(path) = polygon(&pts) {
                    c.fill(&path, 1.0);
                    c.stroke(&path, 1.2, 1.0);
                }
            }
        }
        Icon::Speaker(w1, w2, slash) => {
            if let Some(body) = path_from_svg(SPEAKER_BODY) {
                c.fill(&body, 1.0);
                c.stroke(&body, 1.0, 1.0);
            }
            for (d, level) in [(WAVE_1, w1), (WAVE_2, w2), (SLASH, slash)] {
                if level > 0 {
                    if let Some(path) = path_from_svg(d) {
                        c.stroke(&path, 1.7, level as f32 / 4.0);
                    }
                }
            }
        }
    }
}

#[derive(Default)]
pub struct Icons {
    cache: HashMap<(Icon, i32), IconGlyph>,
}

impl Icons {
    /// Rasterizes `icon` at `size` physical px (once) and returns its atlas slot.
    pub fn get(&mut self, gl: &glow::Context, atlas: &mut Atlas, icon: Icon, size: i32) -> Option<IconGlyph> {
        let size = size.max(4);
        if let Some(g) = self.cache.get(&(icon, size)) {
            if g.generation == atlas.generation {
                return Some(*g);
            }
        }
        let mut canvas = Canvas { pixmap: Pixmap::new(size as u32, size as u32)?, scale: size as f32 / 24.0 };
        draw(icon, &mut canvas);
        let alpha: Vec<u8> = canvas.pixmap.data().chunks_exact(4).map(|p| p[3]).collect();
        let (ax, ay) = atlas.put_alpha(gl, size, size, &alpha)?;
        let g = IconGlyph { ax, ay, size, generation: atlas.generation };
        self.cache.insert((icon, size), g);
        Some(g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_path_parses() {
        for (name, d, _) in ICONS {
            assert!(path_from_svg(d).is_some(), "{name}");
        }
        for d in [SPEAKER_BODY, WAVE_1, WAVE_2, SLASH] {
            assert!(path_from_svg(d).is_some());
        }
    }

    #[test]
    fn icons_draw_something() {
        for (name, _, _) in ICONS {
            let mut c = Canvas { pixmap: Pixmap::new(24, 24).unwrap(), scale: 1.0 };
            draw(Icon::Named(name), &mut c);
            assert!(c.pixmap.data().chunks_exact(4).any(|p| p[3] > 0), "{name} is empty");
        }
    }
}
