//! Thin helpers over `glow`: shader programs, render targets and textures.
use glow::HasContext;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub type Gl = Rc<glow::Context>;

pub struct Prog {
    pub id: glow::Program,
    locs: RefCell<HashMap<&'static str, Option<glow::UniformLocation>>>,
}

impl Prog {
    pub fn new(gl: &glow::Context, vs: &str, fs: &str) -> Result<Prog, String> {
        unsafe {
            let program = gl.create_program()?;
            let mut shaders = Vec::new();
            for (kind, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
                let sh = gl.create_shader(kind)?;
                gl.shader_source(sh, src);
                gl.compile_shader(sh);
                if !gl.get_shader_compile_status(sh) {
                    let log = gl.get_shader_info_log(sh);
                    return Err(format!("shader compile failed: {log}"));
                }
                gl.attach_shader(program, sh);
                shaders.push(sh);
            }
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                return Err(format!("program link failed: {}", gl.get_program_info_log(program)));
            }
            for sh in shaders {
                gl.detach_shader(program, sh);
                gl.delete_shader(sh);
            }
            Ok(Prog { id: program, locs: RefCell::new(HashMap::new()) })
        }
    }

    pub fn loc(&self, gl: &glow::Context, name: &'static str) -> Option<glow::UniformLocation> {
        self.locs.borrow_mut().entry(name).or_insert_with(|| unsafe { gl.get_uniform_location(self.id, name) }).clone()
    }

    pub fn use_it(&self, gl: &glow::Context) {
        unsafe { gl.use_program(Some(self.id)) };
    }

    pub fn f1(&self, gl: &glow::Context, name: &'static str, v: f32) {
        unsafe { gl.uniform_1_f32(self.loc(gl, name).as_ref(), v) };
    }

    pub fn f2(&self, gl: &glow::Context, name: &'static str, a: f32, b: f32) {
        unsafe { gl.uniform_2_f32(self.loc(gl, name).as_ref(), a, b) };
    }

    pub fn f4(&self, gl: &glow::Context, name: &'static str, v: [f32; 4]) {
        unsafe { gl.uniform_4_f32(self.loc(gl, name).as_ref(), v[0], v[1], v[2], v[3]) };
    }

    pub fn i1(&self, gl: &glow::Context, name: &'static str, v: i32) {
        unsafe { gl.uniform_1_i32(self.loc(gl, name).as_ref(), v) };
    }
}

/// A 2D texture, optionally with a framebuffer to render into.
pub struct Target {
    pub tex: glow::Texture,
    pub fbo: glow::Framebuffer,
    pub w: i32,
    pub h: i32,
    float: bool,
}

impl Target {
    pub fn new(gl: &glow::Context, w: i32, h: i32, float: bool) -> Target {
        let (w, h) = (w.max(1), h.max(1));
        unsafe {
            let tex = gl.create_texture().expect("texture");
            let fbo = gl.create_framebuffer().expect("framebuffer");
            let t = Target { tex, fbo, w, h, float };
            t.allocate(gl);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(tex), 0);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            t
        }
    }

    fn allocate(&self, gl: &glow::Context) {
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tex));
            let (internal, ty) = if self.float { (glow::RGBA16F, glow::HALF_FLOAT) } else { (glow::RGBA8, glow::UNSIGNED_BYTE) };
            gl.tex_image_2d(glow::TEXTURE_2D, 0, internal as i32, self.w, self.h, 0, glow::RGBA, ty, glow::PixelUnpackData::Slice(None));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        }
    }

    pub fn resize(&mut self, gl: &glow::Context, w: i32, h: i32) {
        let (w, h) = (w.max(1), h.max(1));
        if (w, h) != (self.w, self.h) {
            self.w = w;
            self.h = h;
            self.allocate(gl);
        }
    }

    pub fn destroy(self, gl: &glow::Context) {
        unsafe {
            gl.delete_framebuffer(self.fbo);
            gl.delete_texture(self.tex);
        }
    }
}

/// An uploaded image (premultiplied RGBA).
pub struct Texture {
    pub id: glow::Texture,
    pub w: u32,
    pub h: u32,
}

impl Texture {
    /// `rgba` is straight alpha; it is premultiplied on upload.
    pub fn from_rgba(gl: &glow::Context, w: u32, h: u32, rgba: &[u8], mipmaps: bool) -> Texture {
        let mut data = rgba.to_vec();
        for px in data.chunks_exact_mut(4) {
            let a = px[3] as u32;
            if a != 255 {
                for c in &mut px[..3] {
                    *c = ((*c as u32 * a + 127) / 255) as u8;
                }
            }
        }
        unsafe {
            let id = gl.create_texture().expect("texture");
            gl.bind_texture(glow::TEXTURE_2D, Some(id));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, w as i32, h as i32, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(Some(&data)));
            if mipmaps {
                gl.generate_mipmap(glow::TEXTURE_2D);
            }
            let min = if mipmaps { glow::LINEAR_MIPMAP_LINEAR } else { glow::LINEAR };
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, min as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            Texture { id, w, h }
        }
    }

    pub fn destroy(self, gl: &glow::Context) {
        unsafe { gl.delete_texture(self.id) };
    }
}
