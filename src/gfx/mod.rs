pub mod backdrop;
pub mod geom;
pub mod gl;
pub mod icons;
pub mod renderer;
pub mod text;

pub use geom::Rect;
pub use icons::Icon;
pub use renderer::{GlassParams, Renderer, Rgba};
pub use text::Weight;

use backdrop::Backdrop;
use gl::{Gl, Target};

/// Everything GPU-side the app draws with: the renderer, the content layer (`scene`), mpv's output
/// (`video`) and the blurred copies of the scene that glass samples.
pub struct Gfx {
    pub r: Renderer,
    pub scene: Target,
    pub video: Target,
    pub backdrop: Backdrop,
    /// Window size in physical px.
    pub size: (i32, i32),
    pub dpi: f32,
}

impl Gfx {
    pub fn new(gl: Gl, size: (i32, i32), dpi: f32) -> Result<Gfx, String> {
        let size = (size.0.max(1), size.1.max(1));
        let scene = Target::new(&gl, size.0, size.1, false);
        let video = Target::new(&gl, size.0, size.1, false);
        let logical = (size.0 as f32 / dpi, size.1 as f32 / dpi);
        let backdrop = Backdrop::new(&gl, logical);
        let r = Renderer::new(gl)?;
        Ok(Gfx { r, scene, video, backdrop, size, dpi })
    }

    /// Logical window size.
    pub fn logical(&self) -> (f32, f32) {
        (self.size.0 as f32 / self.dpi, self.size.1 as f32 / self.dpi)
    }

    pub fn resize(&mut self, size: (i32, i32), dpi: f32) {
        let size = (size.0.max(1), size.1.max(1));
        self.size = size;
        self.dpi = dpi;
        self.scene.resize(&self.r.gl, size.0, size.1);
        self.video.resize(&self.r.gl, size.0, size.1);
        self.backdrop.resize(&self.r.gl, self.logical());
    }

    /// Starts drawing the content layer.
    pub fn begin_scene(&mut self) {
        let (size, dpi) = (self.size, self.dpi);
        self.r.begin(Some(&self.scene), size, dpi);
    }

    /// Blurs the content layer for the glass elements, then switches to the window.
    pub fn finish_scene(&mut self) {
        self.r.flush();
        self.backdrop.update(&mut self.r, &self.scene);
        self.r.backdrop = Some([self.scene.tex, self.backdrop.fine(), self.backdrop.coarse(), self.backdrop.heavy()]);
        let (size, dpi) = (self.size, self.dpi);
        self.r.begin(None, size, dpi);
        let (w, h) = self.logical();
        self.r.target_image(self.scene.tex, Rect::new(0.0, 0.0, w, h), 1.0);
    }
}
