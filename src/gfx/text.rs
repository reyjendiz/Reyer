//! Text: system fonts (Segoe UI with CJK fallbacks), glyph rasterization into a shared atlas, measuring,
//! eliding and wrapping. Sizes are logical pixels; rasterization happens at physical size.
use fontdue::{Font, FontSettings};
use glow::HasContext;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Weight {
    Regular,
    Medium,
    DemiBold,
    Bold,
}

impl Weight {
    fn slot(self) -> usize {
        match self {
            Weight::Regular | Weight::Medium => 0,
            Weight::DemiBold => 1,
            Weight::Bold => 2,
        }
    }
}

/// A shelf-packed RGBA texture shared by glyphs and icons (alpha is the coverage mask).
pub struct Atlas {
    pub tex: glow::Texture,
    pub size: i32,
    x: i32,
    y: i32,
    row: i32,
    /// Bumped every time the atlas is cleared, so cached entries know to rasterize again.
    pub generation: u32,
}

impl Atlas {
    pub fn new(gl: &glow::Context, size: i32) -> Atlas {
        unsafe {
            let tex = gl.create_texture().expect("atlas");
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, size, size, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(None));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            Atlas { tex, size, x: 1, y: 1, row: 0, generation: 0 }
        }
    }

    /// Reserves a `w` x `h` slot (with a 1px gutter). `None` when the atlas is full.
    fn reserve(&mut self, w: i32, h: i32) -> Option<(i32, i32)> {
        if w + 2 > self.size || h + 2 > self.size {
            return None;
        }
        if self.x + w + 1 > self.size {
            self.x = 1;
            self.y += self.row + 1;
            self.row = 0;
        }
        if self.y + h + 1 > self.size {
            return None;
        }
        let at = (self.x, self.y);
        self.x += w + 1;
        self.row = self.row.max(h);
        Some(at)
    }

    /// Uploads an alpha mask (one byte per pixel) as white-with-alpha. Returns its top-left in the atlas.
    /// When full, the atlas is cleared (callers re-rasterize through `generation`).
    pub fn put_alpha(&mut self, gl: &glow::Context, w: i32, h: i32, alpha: &[u8]) -> Option<(i32, i32)> {
        let mut rgba = Vec::with_capacity(alpha.len() * 4);
        for &a in alpha {
            rgba.extend_from_slice(&[255, 255, 255, a]);
        }
        self.put_rgba(gl, w, h, &rgba)
    }

    pub fn put_rgba(&mut self, gl: &glow::Context, w: i32, h: i32, rgba: &[u8]) -> Option<(i32, i32)> {
        let (x, y) = match self.reserve(w, h) {
            Some(p) => p,
            None => {
                self.clear();
                self.reserve(w, h)?
            }
        };
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tex));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, x, y, w, h, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(Some(rgba)));
        }
        Some((x, y))
    }

    fn clear(&mut self) {
        self.x = 1;
        self.y = 1;
        self.row = 0;
        self.generation += 1;
    }
}

#[derive(Clone, Copy)]
pub struct Glyph {
    /// Atlas rectangle in pixels.
    pub ax: i32,
    pub ay: i32,
    pub w: i32,
    pub h: i32,
    /// Offset of the bitmap's top-left from the pen position on the baseline, physical px (y down).
    pub left: f32,
    pub top: f32,
    pub advance: f32,
    generation: u32,
}

fn fonts_dir() -> PathBuf {
    let root = std::env::var_os("SystemRoot").or_else(|| std::env::var_os("WINDIR")).map(PathBuf::from);
    root.unwrap_or_else(|| PathBuf::from(r"C:\Windows")).join("Fonts")
}

fn load(name: &str, index: u32) -> Option<Font> {
    let bytes = std::fs::read(fonts_dir().join(name)).ok()?;
    Font::from_bytes(bytes, FontSettings { collection_index: index, ..FontSettings::default() }).ok()
}

/// Fonts tried, in order, for characters the main font lacks (loaded only when needed).
const FALLBACKS: &[(&str, u32)] = &[
    ("segoeui.ttf", 0),
    ("seguisym.ttf", 0),
    ("msyh.ttc", 0),
    ("YuGothM.ttc", 0),
    ("malgun.ttf", 0),
    ("simsun.ttc", 0),
    ("arial.ttf", 0),
];

pub struct TextEngine {
    main: [Option<Font>; 3],
    fallbacks: Vec<Option<Font>>, // lazily loaded, parallel to FALLBACKS
    glyphs: HashMap<(u8, u16, u32), Glyph>,
    font_for_char: HashMap<(usize, char), u8>,
}

#[derive(Clone, Copy)]
pub struct Placed {
    pub glyph: Glyph,
    /// Pen x of this glyph in physical px, relative to the start of the line.
    pub x: f32,
}

impl TextEngine {
    pub fn new() -> TextEngine {
        let main = [load("segoeui.ttf", 0), load("seguisb.ttf", 0).or_else(|| load("segoeuib.ttf", 0)), load("segoeuib.ttf", 0)];
        TextEngine {
            main,
            fallbacks: FALLBACKS.iter().map(|_| None).collect(),
            glyphs: HashMap::new(),
            font_for_char: HashMap::new(),
        }
    }

    /// Resolves which font draws `ch`: 0..=2 are the main weights, 3.. the fallbacks.
    fn pick(&mut self, ch: char, weight: Weight) -> u8 {
        let slot = weight.slot();
        if let Some(&id) = self.font_for_char.get(&(slot, ch)) {
            return id;
        }
        let has = |f: &Option<Font>| f.as_ref().is_some_and(|f| f.lookup_glyph_index(ch) != 0);
        let id = if has(&self.main[slot]) || ch.is_whitespace() || ch.is_control() {
            slot as u8
        } else {
            let mut found = slot as u8;
            for i in 0..FALLBACKS.len() {
                if self.fallbacks[i].is_none() {
                    self.fallbacks[i] = load(FALLBACKS[i].0, FALLBACKS[i].1);
                }
                if has(&self.fallbacks[i]) {
                    found = 3 + i as u8;
                    break;
                }
            }
            found
        };
        self.font_for_char.insert((slot, ch), id);
        id
    }

    fn font(&self, id: u8) -> Option<&Font> {
        if id < 3 {
            self.main[id as usize].as_ref().or(self.main[0].as_ref())
        } else {
            self.fallbacks[id as usize - 3].as_ref()
        }
    }

    pub fn line_metrics(&self, px: f32) -> (f32, f32) {
        self.main[0]
            .as_ref()
            .and_then(|f| f.horizontal_line_metrics(px))
            .map(|m| (m.ascent, m.descent))
            .unwrap_or((px * 0.95, -px * 0.25))
    }

    fn glyph(&mut self, gl: &glow::Context, atlas: &mut Atlas, ch: char, weight: Weight, px: f32) -> Option<Glyph> {
        let id = self.pick(ch, weight);
        let font = self.font(id)?;
        let index = font.lookup_glyph_index(ch);
        let key = (id, index, (px * 4.0).round() as u32);
        if let Some(g) = self.glyphs.get(&key) {
            if g.generation == atlas.generation {
                return Some(*g);
            }
        }
        let (m, bitmap) = font.rasterize_indexed(index, px);
        let (ax, ay) = if m.width > 0 && m.height > 0 { atlas.put_alpha(gl, m.width as i32, m.height as i32, &bitmap)? } else { (0, 0) };
        let g = Glyph {
            ax,
            ay,
            w: m.width as i32,
            h: m.height as i32,
            left: m.xmin as f32,
            top: -(m.ymin as f32 + m.height as f32),
            advance: m.advance_width,
            generation: atlas.generation,
        };
        self.glyphs.insert(key, g);
        Some(g)
    }

    /// Lays a single line out. `px` is the physical size.
    pub fn layout(&mut self, gl: &glow::Context, atlas: &mut Atlas, text: &str, weight: Weight, px: f32) -> (Vec<Placed>, f32) {
        let mut out = Vec::with_capacity(text.len());
        let mut x = 0.0;
        let mut prev: Option<(u8, u16)> = None;
        for ch in text.chars() {
            if ch == '\n' || ch == '\r' {
                continue;
            }
            let Some(g) = self.glyph(gl, atlas, ch, weight, px) else { continue };
            let id = self.pick(ch, weight);
            let idx = self.font(id).map(|f| f.lookup_glyph_index(ch)).unwrap_or(0);
            if let (Some((pid, pidx)), Some(font)) = (prev, self.font(id)) {
                if pid == id {
                    x += font.horizontal_kern_indexed(pidx, idx, px).unwrap_or(0.0);
                }
            }
            out.push(Placed { glyph: g, x });
            x += g.advance;
            prev = Some((id, idx));
        }
        (out, x)
    }

    /// Width of a line in physical px (no atlas upload needed beyond the glyph cache).
    pub fn width(&mut self, gl: &glow::Context, atlas: &mut Atlas, text: &str, weight: Weight, px: f32) -> f32 {
        self.layout(gl, atlas, text, weight, px).1
    }
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}
