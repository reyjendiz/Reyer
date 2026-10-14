//! The 2D renderer: batched rounded rectangles, shadows, text, icons and images, plus the dedicated
//! passes (glass, edge bands, lens knobs, blur, copy). The UI draws in logical pixels; a transform
//! stack (scale, offset, alpha) applies to everything drawn until it is popped.
use super::geom::Rect;
use super::gl::{Gl, Prog, Target};
use super::icons::{Icon, Icons};
use super::text::{Atlas, TextEngine, Weight};
use glow::HasContext;

pub type Rgba = [f32; 4];

const FLOATS: usize = 20;
const MAX_VERTS: usize = 6 * 6000;

#[derive(Clone, Copy)]
struct Xf {
    s: f32,
    tx: f32,
    ty: f32,
    alpha: f32,
}

/// Uniform values for one glass container, in logical px where lengths are involved.
pub struct GlassParams {
    /// (rect, corner radius, glow 0..1); rects with zero width are skipped. At most six.
    pub shapes: Vec<(Rect, f32, f32)>,
    pub smooth_union: f32,
    pub shadow_opacity: f32,
    /// press shimmer position 0..1, negative = off
    pub shimmer: f32,
    pub frosted: bool,
    /// 0 = fine blur .. 1 = coarse blur
    pub blur_mix: f32,
    pub refraction: f32,
    pub saturation: f32,
    pub lift: f32,
    pub tint: Rgba,
    pub light_dir: [f32; 2],
    pub dim: f32,
    pub rim_strength: f32,
    pub frost: Rgba,
    pub rim_width: f32,
    pub rim_high: f32,
    pub rim_low: f32,
    pub border_alpha: f32,
}

pub struct Renderer {
    pub gl: Gl,
    ui: Prog,
    glass: Prog,
    edge: Prog,
    lens: Prog,
    blur: Prog,
    copy: Prog,
    ui_vao: glow::VertexArray,
    ui_vbo: glow::Buffer,
    quad_vao: glow::VertexArray,
    quad_vbo: glow::Buffer,
    empty_vao: glow::VertexArray,
    verts: Vec<f32>,
    batch_tex: Option<glow::Texture>,
    pub atlas: Atlas,
    pub text: TextEngine,
    icons: Icons,
    xf: Vec<Xf>,
    clips: Vec<(i32, i32, i32, i32)>,
    /// Physical size of the current render target.
    pub view: (f32, f32),
    pub dpi: f32,
    /// Blurred copies of the content layer (fine, coarse), sampled by glass and edge bands.
    /// The content layer itself (sharp) and its blurred copies (fine, coarse, heavy).
    pub backdrop: Option<[glow::Texture; 4]>,
    pub draw_calls: u32,
}

fn premul(c: Rgba, alpha: f32) -> Rgba {
    let a = c[3] * alpha;
    [c[0] * a, c[1] * a, c[2] * a, a]
}

impl Renderer {
    pub fn new(gl: Gl) -> Result<Renderer, String> {
        let sh = |vs: &str, fs: &str| Prog::new(&gl, vs, fs);
        let ui = sh(include_str!("../../assets/shaders/ui.vert"), include_str!("../../assets/shaders/ui.frag"))?;
        let glass = sh(include_str!("../../assets/shaders/quad.vert"), include_str!("../../assets/shaders/glass.frag"))?;
        let edge = sh(include_str!("../../assets/shaders/quad.vert"), include_str!("../../assets/shaders/edge.frag"))?;
        let lens = sh(include_str!("../../assets/shaders/quad.vert"), include_str!("../../assets/shaders/lens.frag"))?;
        let blur = sh(include_str!("../../assets/shaders/fullscreen.vert"), include_str!("../../assets/shaders/blur.frag"))?;
        let copy = sh(include_str!("../../assets/shaders/fullscreen.vert"), include_str!("../../assets/shaders/copy.frag"))?;
        unsafe {
            let ui_vao = gl.create_vertex_array()?;
            let ui_vbo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(ui_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(ui_vbo));
            gl.buffer_data_size(glow::ARRAY_BUFFER, (MAX_VERTS * FLOATS * 4) as i32, glow::STREAM_DRAW);
            let stride = (FLOATS * 4) as i32;
            for (loc, size, off) in [(0u32, 2, 0), (1, 2, 2), (2, 2, 4), (3, 4, 6), (4, 4, 10), (5, 4, 14), (6, 2, 18)] {
                gl.enable_vertex_attrib_array(loc);
                gl.vertex_attrib_pointer_f32(loc, size, glow::FLOAT, false, stride, off * 4);
            }
            let quad_vao = gl.create_vertex_array()?;
            let quad_vbo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(quad_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(quad_vbo));
            gl.buffer_data_size(glow::ARRAY_BUFFER, 6 * 4 * 4, glow::STREAM_DRAW);
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 16, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 16, 8);
            let empty_vao = gl.create_vertex_array()?;
            gl.bind_vertex_array(None);
            let atlas = Atlas::new(&gl, 2048);
            Ok(Renderer {
                gl,
                ui,
                glass,
                edge,
                lens,
                blur,
                copy,
                ui_vao,
                ui_vbo,
                quad_vao,
                quad_vbo,
                empty_vao,
                verts: Vec::with_capacity(MAX_VERTS * FLOATS),
                batch_tex: None,
                atlas,
                text: TextEngine::new(),
                icons: Icons::default(),
                xf: vec![Xf { s: 1.0, tx: 0.0, ty: 0.0, alpha: 1.0 }],
                clips: Vec::new(),
                view: (1.0, 1.0),
                dpi: 1.0,
                backdrop: None,
                draw_calls: 0,
            })
        }
    }

    // ---- targets and state -------------------------------------------------------------------------
    /// Binds `target` (or the window) and resets the transform and clip stacks.
    pub fn begin(&mut self, target: Option<&Target>, size: (i32, i32), dpi: f32) {
        self.flush();
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, target.map(|t| t.fbo));
            self.gl.viewport(0, 0, size.0, size.1);
            self.gl.disable(glow::DEPTH_TEST);
            self.gl.disable(glow::CULL_FACE);
            self.gl.disable(glow::SCISSOR_TEST);
            self.gl.enable(glow::BLEND);
            self.gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
        }
        self.view = (size.0 as f32, size.1 as f32);
        self.dpi = dpi;
        self.xf.truncate(1);
        self.xf[0] = Xf { s: 1.0, tx: 0.0, ty: 0.0, alpha: 1.0 };
        self.clips.clear();
        self.draw_calls = 0;
    }

    pub fn clear(&mut self, c: Rgba) {
        unsafe {
            self.gl.clear_color(c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
    }

    /// Scale about `pivot`, then offset, with a multiplicative alpha; applies until `pop`.
    pub fn push(&mut self, pivot: (f32, f32), scale: f32, offset: (f32, f32), alpha: f32) {
        let p = *self.xf.last().expect("transform stack");
        let local_t = (pivot.0 * (1.0 - scale) + offset.0, pivot.1 * (1.0 - scale) + offset.1);
        self.xf.push(Xf { s: p.s * scale, tx: p.s * local_t.0 + p.tx, ty: p.s * local_t.1 + p.ty, alpha: p.alpha * alpha });
    }

    pub fn push_alpha(&mut self, alpha: f32) {
        self.push((0.0, 0.0), 1.0, (0.0, 0.0), alpha);
    }

    pub fn pop(&mut self) {
        if self.xf.len() > 1 {
            self.xf.pop();
        }
    }

    pub fn alpha(&self) -> f32 {
        self.xf.last().map_or(1.0, |x| x.alpha)
    }

    /// Maps a logical rect through the transform stack, back to logical window coordinates.
    pub fn transformed(&self, r: Rect) -> Rect {
        let x = self.xf.last().expect("transform stack");
        Rect::new(r.x * x.s + x.tx, r.y * x.s + x.ty, r.w * x.s, r.h * x.s)
    }

    /// The inverse of the current transform: a window point to local coordinates.
    pub fn untransform(&self, p: (f32, f32)) -> (f32, f32) {
        let x = self.xf.last().expect("transform stack");
        ((p.0 - x.tx) / x.s, (p.1 - x.ty) / x.s)
    }

    pub fn push_clip(&mut self, r: Rect) {
        self.flush();
        let t = self.transformed(r);
        let d = self.dpi;
        let (mut x0, mut y0, mut x1, mut y1) = ((t.x * d).floor() as i32, (t.y * d).floor() as i32, (t.right() * d).ceil() as i32, (t.bottom() * d).ceil() as i32);
        if let Some(&(px0, py0, px1, py1)) = self.clips.last() {
            x0 = x0.max(px0);
            y0 = y0.max(py0);
            x1 = x1.min(px1);
            y1 = y1.min(py1);
        }
        self.clips.push((x0, y0, x1.max(x0), y1.max(y0)));
        self.apply_clip();
    }

    pub fn pop_clip(&mut self) {
        self.flush();
        self.clips.pop();
        self.apply_clip();
    }

    fn apply_clip(&self) {
        unsafe {
            match self.clips.last() {
                Some(&(x0, y0, x1, y1)) => {
                    self.gl.enable(glow::SCISSOR_TEST);
                    self.gl.scissor(x0, self.view.1 as i32 - y1, x1 - x0, y1 - y0);
                }
                None => self.gl.disable(glow::SCISSOR_TEST),
            }
        }
    }

    // ---- batched geometry --------------------------------------------------------------------------
    pub fn flush(&mut self) {
        if self.verts.is_empty() {
            return;
        }
        let gl = &self.gl;
        self.ui.use_it(gl);
        self.ui.f2(gl, "u_view", self.view.0, self.view.1);
        self.ui.i1(gl, "u_tex", 0);
        unsafe {
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, self.batch_tex.or(Some(self.atlas.tex)));
            gl.bind_vertex_array(Some(self.ui_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.ui_vbo));
            gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes_of(&self.verts));
            gl.draw_arrays(glow::TRIANGLES, 0, (self.verts.len() / FLOATS) as i32);
        }
        self.draw_calls += 1;
        self.verts.clear();
    }

    fn use_texture(&mut self, tex: Option<glow::Texture>) {
        let want = tex.or(Some(self.atlas.tex));
        let have = self.batch_tex.or(Some(self.atlas.tex));
        if want != have || self.verts.len() / FLOATS + 6 > MAX_VERTS {
            self.flush();
        }
        self.batch_tex = tex;
    }

    #[allow(clippy::too_many_arguments)]
    fn quad_ui(&mut self, rect: (f32, f32, f32, f32), expand: f32, uv: [f32; 4], radius: f32, border: f32, sigma: f32, mode: f32, top: Rgba, bottom: Rgba, border_col: Rgba) {
        let (x0, y0, x1, y1) = rect;
        let (cx, cy, hw, hh) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0) / 2.0, (y1 - y0) / 2.0);
        let (ex0, ey0, ex1, ey1) = (x0 - expand, y0 - expand, x1 + expand, y1 + expand);
        let du = if x1 > x0 { (uv[2] - uv[0]) / (x1 - x0) } else { 0.0 };
        let dv = if y1 > y0 { (uv[3] - uv[1]) / (y1 - y0) } else { 0.0 };
        let corner = |x: f32, y: f32, fill: Rgba| -> [f32; FLOATS] {
            [
                x, y, x - cx, y - cy, hw, hh, radius, border, sigma, mode, fill[0], fill[1], fill[2], fill[3], border_col[0], border_col[1], border_col[2],
                border_col[3], uv[0] + (x - x0) * du, uv[1] + (y - y0) * dv,
            ]
        };
        let (a, b, c, d) = (corner(ex0, ey0, top), corner(ex1, ey0, top), corner(ex1, ey1, bottom), corner(ex0, ey1, bottom));
        for v in [a, b, c, a, c, d] {
            self.verts.extend_from_slice(&v);
        }
    }

    fn phys(&self, r: Rect) -> (f32, f32, f32, f32) {
        let x = self.xf.last().expect("transform stack");
        let d = self.dpi;
        ((r.x * x.s + x.tx) * d, (r.y * x.s + x.ty) * d, ((r.x + r.w) * x.s + x.tx) * d, ((r.y + r.h) * x.s + x.ty) * d)
    }

    fn unit(&self) -> f32 {
        self.xf.last().map_or(1.0, |x| x.s) * self.dpi
    }

    pub fn rrect(&mut self, r: Rect, radius: f32, color: Rgba) {
        self.rrect_gradient(r, radius, color, color);
    }

    pub fn rrect_gradient(&mut self, r: Rect, radius: f32, top: Rgba, bottom: Rgba) {
        if r.is_empty() {
            return;
        }
        let a = self.alpha();
        self.use_texture(None);
        let u = self.unit();
        self.quad_ui(self.phys(r), 1.0, [0.0; 4], radius * u, 0.0, 0.0, 0.0, premul(top, a), premul(bottom, a), [0.0; 4]);
    }

    /// Outline only.
    pub fn rrect_stroke(&mut self, r: Rect, radius: f32, width: f32, color: Rgba) {
        self.rrect_bordered(r, radius, [0.0; 4], width, color);
    }

    pub fn rrect_bordered(&mut self, r: Rect, radius: f32, fill: Rgba, border_w: f32, border: Rgba) {
        if r.is_empty() {
            return;
        }
        let a = self.alpha();
        self.use_texture(None);
        let u = self.unit();
        self.quad_ui(self.phys(r), 1.0, [0.0; 4], radius * u, border_w * u, 0.0, 0.0, premul(fill, a), premul(fill, a), premul(border, a));
    }

    /// Soft shadow of a rounded rectangle (`sigma` in logical px).
    pub fn shadow(&mut self, r: Rect, radius: f32, sigma: f32, dy: f32, color: Rgba) {
        if r.is_empty() {
            return;
        }
        let a = self.alpha();
        self.use_texture(None);
        let u = self.unit();
        self.quad_ui(self.phys(r.offset(0.0, dy)), sigma * u * 1.5 + 2.0, [0.0; 4], radius * u, 0.0, sigma * u, 3.0, premul(color, a), premul(color, a), [0.0; 4]);
    }

    pub fn circle(&mut self, center: (f32, f32), radius: f32, color: Rgba) {
        self.rrect(Rect::new(center.0 - radius, center.1 - radius, 2.0 * radius, 2.0 * radius), radius, color);
    }

    /// An image (premultiplied texture) clipped to a rounded rectangle. `uv` = u0, v0, u1, v1.
    pub fn image(&mut self, tex: glow::Texture, r: Rect, uv: [f32; 4], radius: f32, alpha: f32) {
        if r.is_empty() || alpha <= 0.0 {
            return;
        }
        let a = self.alpha() * alpha;
        self.use_texture(Some(tex));
        let u = self.unit();
        self.quad_ui(self.phys(r), 0.0, uv, radius * u, 0.0, 0.0, 2.0, [0.0, 0.0, 0.0, a], [0.0, 0.0, 0.0, a], [0.0; 4]);
    }

    /// Draws an offscreen target (rendered with this renderer's y-down projection) back onto the screen.
    pub fn target_image(&mut self, tex: glow::Texture, r: Rect, alpha: f32) {
        self.image(tex, r, [0.0, 1.0, 1.0, 0.0], 0.0, alpha);
    }

    // ---- text and icons -----------------------------------------------------------------------------
    pub fn text_width(&mut self, s: &str, size: f32, weight: Weight) -> f32 {
        let px = size * self.dpi;
        self.text.width(&self.gl, &mut self.atlas, s, weight, px) / self.dpi
    }

    /// Height of a line box, in logical px.
    pub fn line_height(&self, size: f32) -> f32 {
        let (asc, desc) = self.text.line_metrics(size * self.dpi);
        (asc - desc) / self.dpi
    }

    /// Draws one line with its box top-left at (x, y). Returns the width.
    pub fn text(&mut self, s: &str, x: f32, y: f32, size: f32, weight: Weight, color: Rgba) -> f32 {
        let px = size * self.dpi;
        let (glyphs, width) = self.text.layout(&self.gl, &mut self.atlas, s, weight, px);
        let (asc, _) = self.text.line_metrics(px);
        let a = self.alpha();
        let col = premul(color, a);
        let xf = *self.xf.last().expect("transform stack");
        let d = self.dpi;
        let (ox, oy) = ((x * xf.s + xf.tx) * d, (y * xf.s + xf.ty) * d + asc * xf.s);
        self.use_texture(None);
        let atlas = self.atlas.size as f32;
        for p in glyphs {
            let g = p.glyph;
            if g.w == 0 || g.h == 0 {
                continue;
            }
            let gx = (ox + (p.x + g.left) * xf.s).round();
            let gy = (oy + g.top * xf.s).round();
            let (gw, gh) = (g.w as f32 * xf.s, g.h as f32 * xf.s);
            let uv = [g.ax as f32 / atlas, g.ay as f32 / atlas, (g.ax + g.w) as f32 / atlas, (g.ay + g.h) as f32 / atlas];
            self.quad_ui((gx, gy, gx + gw, gy + gh), 0.0, uv, 0.0, 0.0, 0.0, 1.0, col, col, [0.0; 4]);
        }
        width / d
    }

    /// Text clipped to `max_w` with an ellipsis.
    pub fn elide(&mut self, s: &str, size: f32, weight: Weight, max_w: f32) -> String {
        if self.text_width(s, size, weight) <= max_w {
            return s.to_string();
        }
        let chars: Vec<char> = s.chars().collect();
        let (mut lo, mut hi) = (0usize, chars.len());
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let cand: String = chars[..mid].iter().collect::<String>() + "…";
            if self.text_width(&cand, size, weight) <= max_w {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        chars[..lo].iter().collect::<String>().trim_end().to_string() + "…"
    }

    /// Greedy line wrapping. `anywhere` also breaks inside words (long paths, hashes).
    pub fn wrap(&mut self, s: &str, size: f32, weight: Weight, max_w: f32, anywhere: bool) -> Vec<String> {
        let mut lines = Vec::new();
        for para in s.split('\n') {
            let mut line = String::new();
            let mut tokens: Vec<String> = Vec::new();
            for word in para.split_inclusive(' ') {
                tokens.push(word.to_string());
            }
            for tok in tokens {
                let cand = format!("{line}{tok}");
                if self.text_width(cand.trim_end(), size, weight) <= max_w {
                    line = cand;
                    continue;
                }
                if !line.is_empty() {
                    lines.push(line.trim_end().to_string());
                    line = String::new();
                }
                if self.text_width(tok.trim_end(), size, weight) <= max_w || !anywhere && !tok.contains(char::is_alphanumeric) {
                    line = tok;
                    continue;
                }
                // a single token wider than the line: break it by characters
                for ch in tok.chars() {
                    let cand = format!("{line}{ch}");
                    if self.text_width(&cand, size, weight) > max_w && !line.is_empty() {
                        lines.push(std::mem::take(&mut line));
                    }
                    line.push(ch);
                }
            }
            lines.push(line.trim_end().to_string());
        }
        lines
    }

    /// An icon centered on `center`, `size` logical px square.
    pub fn icon(&mut self, icon: Icon, center: (f32, f32), size: f32, color: Rgba) {
        let xf = *self.xf.last().expect("transform stack");
        let px = (size * xf.s * self.dpi).round().max(4.0) as i32;
        let Some(g) = self.icons.get(&self.gl, &mut self.atlas, icon, px) else { return };
        let col = premul(color, xf.alpha);
        self.use_texture(None);
        let d = self.dpi;
        let x = ((center.0 * xf.s + xf.tx) * d - px as f32 / 2.0).round();
        let y = ((center.1 * xf.s + xf.ty) * d - px as f32 / 2.0).round();
        let a = self.atlas.size as f32;
        let uv = [g.ax as f32 / a, g.ay as f32 / a, (g.ax + g.size) as f32 / a, (g.ay + g.size) as f32 / a];
        self.quad_ui((x, y, x + px as f32, y + px as f32), 0.0, uv, 0.0, 0.0, 0.0, 1.0, col, col, [0.0; 4]);
    }

    // ---- dedicated passes ---------------------------------------------------------------------------
    fn draw_quad(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let v = [x0, y0, 0.0, 0.0, x1, y0, 1.0, 0.0, x1, y1, 1.0, 1.0, x0, y0, 0.0, 0.0, x1, y1, 1.0, 1.0, x0, y1, 0.0, 1.0];
        unsafe {
            self.gl.bind_vertex_array(Some(self.quad_vao));
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.quad_vbo));
            self.gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes_of(&v));
            self.gl.draw_arrays(glow::TRIANGLES, 0, 6);
        }
        self.draw_calls += 1;
    }

    fn bind_backdrop(&self, prog: &Prog) -> bool {
        let Some([sharp, fine, coarse, heavy]) = self.backdrop else { return false };
        // uniforms go to the program in use: select it before setting the sampler units
        prog.use_it(&self.gl);
        prog.i1(&self.gl, "sharp", 3);
        prog.i1(&self.gl, "heavy", 4);
        prog.i1(&self.gl, "fine", 1);
        prog.i1(&self.gl, "coarse", 2);
        unsafe {
            self.gl.active_texture(glow::TEXTURE1);
            self.gl.bind_texture(glow::TEXTURE_2D, Some(fine));
            self.gl.active_texture(glow::TEXTURE2);
            self.gl.bind_texture(glow::TEXTURE_2D, Some(coarse));
            self.gl.active_texture(glow::TEXTURE3);
            self.gl.bind_texture(glow::TEXTURE_2D, Some(sharp));
            self.gl.active_texture(glow::TEXTURE4);
            self.gl.bind_texture(glow::TEXTURE_2D, Some(heavy));
            self.gl.active_texture(glow::TEXTURE0);
        }
        true
    }

    /// One Liquid Glass container (see glass.frag).
    pub fn glass(&mut self, g: &GlassParams) {
        self.flush();
        let shapes: Vec<_> = g.shapes.iter().filter(|(r, _, _)| r.w > 0.0 && r.h > 0.0).take(6).collect();
        if shapes.is_empty() || !self.bind_backdrop(&self.glass) {
            return;
        }
        let xf = *self.xf.last().expect("transform stack");
        let d = self.dpi;
        let u = xf.s * d;
        let phys: Vec<(f32, f32, f32, f32)> = shapes.iter().map(|(r, _, _)| self.phys(*r)).collect();
        let margin = 44.0 * u;
        let x0 = phys.iter().map(|p| p.0).fold(f32::MAX, f32::min) - margin;
        let y0 = phys.iter().map(|p| p.1).fold(f32::MAX, f32::min) - margin;
        let x1 = phys.iter().map(|p| p.2).fold(f32::MIN, f32::max) + margin;
        let y1 = phys.iter().map(|p| p.3).fold(f32::MIN, f32::max) + margin;
        let gl = &self.gl;
        let p = &self.glass;
        p.use_it(gl);
        p.f2(gl, "u_view", self.view.0, self.view.1);
        p.f2(gl, "u_origin", x0, y0);
        p.f2(gl, "u_size", x1 - x0, y1 - y0);
        p.f1(gl, "u_dpi", u);
        p.f1(gl, "u_opacity", xf.alpha);
        let names = ["shape0", "shape1", "shape2", "shape3", "shape4", "shape5"];
        let mut radii = [0.0f32; 6];
        let mut glows = [0.0f32; 6];
        for (i, name) in names.iter().enumerate() {
            match phys.get(i) {
                Some(s) => {
                    p.f4(gl, name, [s.0 - x0, s.1 - y0, s.2 - s.0, s.3 - s.1]);
                    radii[i] = shapes[i].1 * u;
                    glows[i] = shapes[i].2;
                }
                None => p.f4(gl, name, [0.0; 4]),
            }
        }
        p.f4(gl, "radiiA", [radii[0], radii[1], radii[2], radii[3]]);
        p.f4(gl, "radiiB", [radii[4], radii[5], g.smooth_union * u, g.shadow_opacity]);
        p.f4(gl, "glowA", [glows[0], glows[1], glows[2], glows[3]]);
        p.f4(gl, "glowB", [glows[4], glows[5], g.shimmer, if g.frosted { 1.0 } else { 0.0 }]);
        p.f4(gl, "optics", [g.blur_mix, if g.frosted { 0.0 } else { g.refraction * u }, g.saturation, g.lift]);
        p.f4(gl, "tint", g.tint);
        p.f4(gl, "light", [g.light_dir[0], g.light_dir[1], g.dim, g.rim_strength]);
        p.f4(gl, "frostColor", g.frost);
        p.f4(gl, "rimParams", [g.rim_width * u, g.rim_high, g.rim_low, g.border_alpha]);
        self.draw_quad(x0, y0, x1, y1);
    }

    /// Progressive blur + dim band along the top or bottom window edge.
    pub fn edge_band(&mut self, r: Rect, from_top: bool, dim: f32, opacity: f32) {
        self.flush();
        if opacity <= 0.001 || !self.bind_backdrop(&self.edge) {
            return;
        }
        let (x0, y0, x1, y1) = self.phys(r);
        let gl = &self.gl;
        let p = &self.edge;
        p.use_it(gl);
        p.f2(gl, "u_view", self.view.0, self.view.1);
        p.f1(gl, "u_fromTop", if from_top { 1.0 } else { 0.0 });
        p.f1(gl, "u_dim", dim);
        p.f1(gl, "u_opacity", opacity * self.alpha());
        self.draw_quad(x0, y0, x1, y1);
    }

    /// The slider knob: a flat dot at rest, a refracting glass lens while touched.
    #[allow(clippy::too_many_arguments)]
    pub fn lens(&mut self, center: (f32, f32), radius: f32, magnify: f32, strength: f32, half_thick: f32, accent: Rgba, track: Rgba, light: [f32; 4]) {
        self.flush();
        let xf = *self.xf.last().expect("transform stack");
        let u = xf.s * self.dpi;
        let (cx, cy) = ((center.0 * xf.s + xf.tx) * self.dpi, (center.1 * xf.s + xf.ty) * self.dpi);
        let r = radius * u;
        let gl = &self.gl;
        let p = &self.lens;
        p.use_it(gl);
        p.f2(gl, "u_view", self.view.0, self.view.1);
        p.f1(gl, "u_opacity", xf.alpha);
        p.f1(gl, "u_radius", r);
        p.f1(gl, "magnify", magnify);
        p.f1(gl, "strength", strength);
        p.f1(gl, "halfThick", half_thick * u);
        p.f4(gl, "accent", accent);
        p.f4(gl, "trackColor", track);
        p.f4(gl, "knobColor", [1.0, 1.0, 1.0, 1.0]);
        p.f4(gl, "light", light);
        self.draw_quad(cx - r - 1.0, cy - r - 1.0, cx + r + 1.0, cy + r + 1.0);
    }

    /// One separable blur pass `src -> dst`; `dir` is the uv step.
    pub fn blur_pass(&mut self, src: glow::Texture, dst: &Target, dir: (f32, f32)) {
        self.flush();
        let gl = &self.gl;
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(dst.fbo));
            gl.viewport(0, 0, dst.w, dst.h);
            gl.disable(glow::BLEND);
            gl.disable(glow::SCISSOR_TEST);
        }
        let p = &self.blur;
        p.use_it(gl);
        p.i1(gl, "u_src", 0);
        p.f2(gl, "u_dir", dir.0, dir.1);
        unsafe {
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(src));
            gl.bind_vertex_array(Some(self.empty_vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.enable(glow::BLEND);
        }
        self.draw_calls += 1;
    }

    /// Copies (a sub-rectangle of) `src` into `dst`, scaling with linear filtering, with a colour grade.
    pub fn copy_pass(&mut self, src: glow::Texture, dst: &Target, uv_rect: [f32; 4], brightness: f32, saturation: f32) {
        self.flush();
        let gl = &self.gl;
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(dst.fbo));
            gl.viewport(0, 0, dst.w, dst.h);
            gl.disable(glow::BLEND);
            gl.disable(glow::SCISSOR_TEST);
        }
        let p = &self.copy;
        p.use_it(gl);
        p.i1(gl, "u_src", 0);
        p.f4(gl, "u_rect", uv_rect);
        p.f4(gl, "u_grade", [brightness, saturation, 0.0, 0.0]);
        unsafe {
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(src));
            gl.bind_vertex_array(Some(self.empty_vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.enable(glow::BLEND);
        }
        self.draw_calls += 1;
    }
}

fn bytes_of(v: &[f32]) -> &[u8] {
    // SAFETY: f32 has no padding and any bit pattern is a valid u8 sequence
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}
