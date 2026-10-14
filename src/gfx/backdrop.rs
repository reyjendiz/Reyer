//! Blurred copies of the content layer that every glass element samples:
//!   fine   - quarter resolution, two gaussian iterations   (~9 px at full size: Clear glass)
//!   coarse - continues at 1/8 and 1/16 resolution          (~30 px at full size: Regular glass)
//!   heavy  - two more at 1/32                               (~60 px: the content-layer material)
//! Every pass samples neighbouring texels (spread 1), never with gaps: a wide kernel stretched over a
//! small texture copies each edge several times and the blur turns into stripes.
use super::gl::Target;
use super::renderer::Renderer;
use glow::HasContext;

struct Step {
    mid: Target,
    out: Target,
}

pub struct Backdrop {
    half: Target,
    grab: Target,
    steps: [Step; 7],
    size: (i32, i32),
}

/// Divisors of the five gaussian steps (relative to the logical window size).
const DIVS: [f32; 7] = [4.0, 4.0, 8.0, 16.0, 16.0, 32.0, 32.0];

fn level(logical: (f32, f32), div: f32) -> (i32, i32) {
    (((logical.0 / div).round() as i32).max(4), ((logical.1 / div).round() as i32).max(4))
}

impl Backdrop {
    pub fn new(gl: &glow::Context, logical: (f32, f32)) -> Backdrop {
        let mk = |d: f32| {
            let (w, h) = level(logical, d);
            Target::new(gl, w, h, true)
        };
        Backdrop {
            half: mk(2.0),
            grab: mk(4.0),
            steps: DIVS.map(|d| Step { mid: mk(d), out: mk(d) }),
            size: (logical.0 as i32, logical.1 as i32),
        }
    }

    pub fn fine(&self) -> glow::Texture {
        self.steps[1].out.tex
    }

    pub fn coarse(&self) -> glow::Texture {
        self.steps[4].out.tex
    }

    pub fn heavy(&self) -> glow::Texture {
        self.steps[6].out.tex
    }

    pub fn resize(&mut self, gl: &glow::Context, logical: (f32, f32)) {
        let size = (logical.0 as i32, logical.1 as i32);
        if size == self.size {
            return;
        }
        self.size = size;
        let (w, h) = level(logical, 2.0);
        self.half.resize(gl, w, h);
        let (w, h) = level(logical, 4.0);
        self.grab.resize(gl, w, h);
        for (step, d) in self.steps.iter_mut().zip(DIVS) {
            let (w, h) = level(logical, d);
            step.mid.resize(gl, w, h);
            step.out.resize(gl, w, h);
        }
    }

    /// Recomputes the whole chain from `scene`. The caller re-binds its own target afterwards.
    pub fn update(&mut self, r: &mut Renderer, scene: &Target) {
        r.copy_pass(scene.tex, &self.half, [0.0, 0.0, 1.0, 1.0], 0.0, 1.0);
        r.copy_pass(self.half.tex, &self.grab, [0.0, 0.0, 1.0, 1.0], 0.0, 1.0);
        let mut input = self.grab.tex;
        for step in &self.steps {
            r.blur_pass(input, &step.mid, (1.0 / step.mid.w as f32, 0.0));
            r.blur_pass(step.mid.tex, &step.out, (0.0, 1.0 / step.out.h as f32));
            input = step.out.tex;
        }
    }

    /// Mean luminance of normalized rects (x, y, w, h, y down) of the coarse backdrop.
    pub fn luminance(&self, gl: &glow::Context, rects: &[[f32; 4]]) -> Vec<f32> {
        let t = &self.steps[4].out;
        let mut px = vec![0u8; (t.w * t.h * 4) as usize];
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(t.fbo));
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            gl.read_pixels(0, 0, t.w, t.h, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(&mut px)));
        }
        rects
            .iter()
            .map(|r| {
                let x0 = ((r[0] * t.w as f32).floor() as i32).clamp(0, t.w - 1);
                let x1 = (((r[0] + r[2]) * t.w as f32).ceil() as i32).clamp(x0 + 1, t.w);
                // texture rows run bottom to top
                let ytop = ((r[1] * t.h as f32).floor() as i32).clamp(0, t.h - 1);
                let ybot = (((r[1] + r[3]) * t.h as f32).ceil() as i32).clamp(ytop + 1, t.h);
                let (mut sum, mut n) = (0.0f32, 0);
                for y in ytop..ybot {
                    let row = t.h - 1 - y;
                    for x in x0..x1 {
                        let i = ((row * t.w + x) * 4) as usize;
                        sum += 0.2126 * px[i] as f32 + 0.7152 * px[i + 1] as f32 + 0.0722 * px[i + 2] as f32;
                        n += 1;
                    }
                }
                if n == 0 {
                    0.0
                } else {
                    sum / (n as f32 * 255.0)
                }
            })
            .collect()
    }
}
