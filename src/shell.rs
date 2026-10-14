//! The window shell: winit window, OpenGL context, event translation and the frame loop.
use crate::app::{App, Cli, FrameOut, WinCmd, WinState};
use crate::gfx::Gfx;
use crate::input::{Input, KeyPress, K};
use crate::win;
use glow::HasContext;
use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version};
use glutin::display::{Display, GetGlDisplay, GlDisplay};
use glutin::prelude::*;
use glutin::surface::{Surface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::{c_char, c_void, CStr};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{CursorIcon, Fullscreen, ResizeDirection, Window, WindowId};

#[derive(Debug)]
pub enum UserEvent {
    /// libmpv has events waiting.
    MpvWake,
    /// libmpv has a new video frame.
    MpvFrame,
    /// A background job (cover generation) finished.
    Wake,
}

struct Running {
    window: Window,
    hwnd: isize,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    display: Box<Display>,
    gfx: Gfx,
    app: App,
    input: Input,
    state: WinState,
    last_frame: Instant,
    started: Instant,
    first_frame_done: bool,
    redraw_at: Option<Instant>,
    animating: bool,
    shot_done: bool,
    cursor: CursorIcon,
    cursor_visible: bool,
    script_idx: usize,
    /// Frame durations in ms, collected when REYER_PERF is set (written on exit).
    perf: Option<Vec<f32>>,
    release_next: bool,
}

pub struct Shell {
    proxy: EventLoopProxy<UserEvent>,
    cli: Cli,
    running: Option<Running>,
    exit_code: i32,
}

pub fn run(cli: Cli) -> i32 {
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(l) => l,
        Err(e) => {
            win::message_box(&format!("Reyer could not start: {e}"), "Reyer");
            return 1;
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut shell = Shell { proxy: event_loop.create_proxy(), cli, running: None, exit_code: 0 };
    let _ = event_loop.run_app(&mut shell);
    shell.exit_code
}

extern "C" fn get_proc(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    // SAFETY: `ctx` is the boxed Display owned by `Running`, alive as long as mpv's render context
    unsafe {
        let display = &*(ctx as *const Display);
        display.get_proc_address(CStr::from_ptr(name)) as *mut c_void
    }
}

fn map_key(ev: &winit::event::KeyEvent, ctrl: bool, shift: bool, alt: bool) -> Option<KeyPress> {
    let key = match &ev.logical_key {
        Key::Named(n) => match n {
            NamedKey::Space => K::Space,
            NamedKey::Enter => K::Enter,
            NamedKey::Escape => K::Escape,
            NamedKey::ArrowLeft => K::Left,
            NamedKey::ArrowRight => K::Right,
            NamedKey::ArrowUp => K::Up,
            NamedKey::ArrowDown => K::Down,
            NamedKey::Tab => K::Tab,
            NamedKey::PageUp => K::PageUp,
            NamedKey::PageDown => K::PageDown,
            NamedKey::Home => K::Home,
            NamedKey::End => K::End,
            NamedKey::Backspace => K::Backspace,
            NamedKey::ContextMenu => K::Menu,
            _ => return None,
        },
        Key::Character(s) => {
            // with Ctrl held the logical key is a control character on some layouts; use the text-less key
            let ch = s.chars().next()?;
            K::Char(ch.to_lowercase().next().unwrap_or(ch))
        }
        _ => return None,
    };
    Some(KeyPress { key, ctrl, shift, alt, repeat: ev.repeat })
}

impl Shell {
    fn create(&mut self, el: &ActiveEventLoop) -> Result<Running, String> {
        let (areas, primary) = win::work_areas();
        let settings_rect = crate::settings::Settings::load(&crate::settings::data_dir().join("settings.json")).window_rect;
        let rect = self.cli.window_rect.or_else(|| Some(crate::geometry::restore_rect(settings_rect, &areas, primary))).unwrap();
        let attrs = Window::default_attributes()
            .with_title("Reyer")
            .with_inner_size(PhysicalSize::new(rect[2] as u32, rect[3] as u32))
            .with_position(PhysicalPosition::new(rect[0], rect[1]))
            .with_min_inner_size(PhysicalSize::new(520, 300))
            .with_decorations(false)
            .with_undecorated_shadow(true)
            .with_visible(false)
            .with_window_icon(crate::icon::window_icon());
        let template = ConfigTemplateBuilder::new().with_alpha_size(0);
        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(attrs))
            .build(el, template, |mut configs| configs.next().expect("no GL config"))
            .map_err(|e| e.to_string())?;
        let window = window.ok_or("no window")?;
        let hwnd = match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
            RawWindowHandle::Win32(h) => h.hwnd.get(),
            _ => 0,
        };
        let display = config.display();
        let ctx_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .build(Some(window.window_handle().map_err(|e| e.to_string())?.as_raw()));
        let not_current = unsafe { display.create_context(&config, &ctx_attrs) }.map_err(|e| format!("OpenGL 3.3 is not available: {e}"))?;
        let surface_attrs = window.build_surface_attributes(SurfaceAttributesBuilder::<WindowSurface>::new()).map_err(|e| e.to_string())?;
        let surface = unsafe { display.create_window_surface(&config, &surface_attrs) }.map_err(|e| e.to_string())?;
        let context = not_current.make_current(&surface).map_err(|e| e.to_string())?;
        let _ = surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::new(1).unwrap()));
        let gl = Rc::new(unsafe { glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s)) });
        let size = window.inner_size();
        let dpi = window.scale_factor() as f32;
        let gfx = Gfx::new(gl, (size.width as i32, size.height as i32), dpi)?;
        win::set_rounded(hwnd, true);
        win::install_resize_edges(hwnd);
        let display = Box::new(display);
        let state = WinState {
            size: gfx.logical(),
            dpi,
            fullscreen: false,
            maximized: false,
            focused: true,
            minimized: false,
            hwnd,
            rect: Some([rect[0], rect[1], rect[2], rect[3]]),
        };
        let app = App::new(&self.cli, self.proxy.clone(), &*display as *const Display as *mut c_void, get_proc);
        Ok(Running {
            window,
            hwnd,
            surface,
            context,
            display,
            gfx,
            app,
            input: Input::default(),
            state,
            last_frame: Instant::now(),
            started: Instant::now(),
            first_frame_done: false,
            redraw_at: None,
            animating: true,
            shot_done: false,
            cursor: CursorIcon::Default,
            cursor_visible: true,
            script_idx: 0,
            perf: std::env::var_os("REYER_PERF").map(|_| Vec::new()),
            release_next: false,
        })
    }
}

impl Running {
    fn physical_rect(&self) -> Option<[i32; 4]> {
        let p = self.window.outer_position().ok()?;
        let s = self.window.inner_size();
        Some([p.x, p.y, s.width as i32, s.height as i32])
    }

    fn sync_state(&mut self) {
        let size = self.window.inner_size();
        self.state.dpi = self.window.scale_factor() as f32;
        self.state.size = (size.width as f32 / self.state.dpi, size.height as f32 / self.state.dpi);
        self.state.fullscreen = self.window.fullscreen().is_some();
        self.state.maximized = self.window.is_maximized();
        self.state.minimized = self.window.is_minimized().unwrap_or(false);
        if !self.state.fullscreen && !self.state.maximized && !self.state.minimized {
            self.state.rect = self.physical_rect();
        }
        win::set_fullscreen_flag(self.state.fullscreen);
    }

    fn redraw(&mut self, el: &ActiveEventLoop) {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 || self.state.minimized {
            return;
        }
        self.sync_state();
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        if let Some(p) = self.perf.as_mut() {
            p.push(now.duration_since(self.last_frame).as_secs_f32() * 1000.0);
        }
        self.last_frame = now;
        if self.gfx.size != (size.width as i32, size.height as i32) || (self.gfx.dpi - self.state.dpi).abs() > 1e-3 {
            self.gfx.resize((size.width as i32, size.height as i32), self.state.dpi);
        }
        self.run_script();
        let out: FrameOut = self.app.frame(&mut self.gfx, &mut self.input, &self.state, dt, now);
        self.input.end_frame();
        if let Some(path) = self.app.shot_request(self.started.elapsed()) {
            if !self.shot_done {
                self.shot_done = true;
                self.save_shot(&path);
                self.write_perf();
                self.app.request_close();
                el.exit();
                return;
            }
        }
        let _ = self.surface.swap_buffers(&self.context);
        if !self.first_frame_done {
            self.first_frame_done = true;
            self.window.set_visible(true);
            self.window.focus_window();
            self.app.first_frame_presented();
        }
        self.animating = out.animating;
        self.redraw_at = out.deadline;
        self.apply(out.cmds, el);
    }

    /// Scripted input (`--script`): moves, clicks and key presses at given times.
    fn run_script(&mut self) {
        if self.release_next {
            self.input.release();
            self.release_next = false;
        }
        let ms = self.started.elapsed().as_millis() as u64;
        let Some((at, kind, arg)) = self.app.script_step(self.script_idx).filter(|s| s.0 <= ms) else { return };
        self.script_idx += 1;
        let pos = |a: &str| {
            let v: Vec<f32> = a.split(',').filter_map(|p| p.parse().ok()).collect();
            (v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0))
        };
        let _ = at;
        match kind.as_str() {
            "down" => {
                self.input.mouse = pos(&arg);
                self.input.mouse_in = true;
                self.input.moved = true;
                self.input.press();
            }
            "up" => {
                self.input.mouse = pos(&arg);
                self.input.moved = true;
                self.input.release();
            }
            "move" | "click" => {
                self.input.mouse = pos(&arg);
                self.input.mouse_in = true;
                self.input.moved = true;
                if kind == "click" {
                    self.input.press();
                    self.release_next = true;
                }
            }
            "key" => {
                let (ctrl, name) = match arg.strip_prefix("ctrl+") {
                    Some(n) => (true, n),
                    None => (false, arg.as_str()),
                };
                let key = match name {
                    "space" => K::Space,
                    "escape" => K::Escape,
                    "enter" => K::Enter,
                    "tab" => K::Tab,
                    "left" => K::Left,
                    "right" => K::Right,
                    "up" => K::Up,
                    "down" => K::Down,
                    c => K::Char(c.chars().next().unwrap_or(' ')),
                };
                self.input.keys.push(KeyPress { key, ctrl, shift: false, alt: false, repeat: false });
            }
            _ => {}
        }
    }

    fn write_perf(&self) {
        let (Some(p), Some(path)) = (&self.perf, std::env::var_os("REYER_PERF")) else { return };
        let mut v: Vec<f32> = p.iter().copied().skip(30).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if v.is_empty() {
            return;
        }
        let pct = |q: f32| v[((v.len() - 1) as f32 * q) as usize];
        let _ = std::fs::write(path, format!("frames {} median {:.1} p95 {:.1} max {:.1}
", v.len(), pct(0.5), pct(0.95), v[v.len() - 1]));
    }

    fn save_shot(&mut self, path: &str) {
        let (w, h) = self.gfx.size;
        let mut px = vec![0u8; (w * h * 4) as usize];
        unsafe {
            let gl = &self.gfx.r.gl;
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.read_buffer(glow::BACK);
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            gl.read_pixels(0, 0, w, h, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(&mut px)));
        }
        let row = (w * 4) as usize;
        let mut flipped = Vec::with_capacity(px.len());
        for y in (0..h as usize).rev() {
            for x in 0..w as usize {
                let i = y * row + x * 4;
                flipped.extend_from_slice(&[px[i], px[i + 1], px[i + 2], 255]);
            }
        }
        if let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, flipped) {
            let _ = img.save(path);
        }
    }

    fn apply(&mut self, cmds: Vec<WinCmd>, el: &ActiveEventLoop) {
        for cmd in cmds {
            match cmd {
                WinCmd::ToggleFullscreen => {
                    if self.window.fullscreen().is_some() {
                        self.window.set_fullscreen(None);
                    } else {
                        // from a maximized window the client area stayed at work-area size and left a black strip
                        // where the taskbar was; start from the normal state instead
                        if self.window.is_maximized() {
                            self.window.set_maximized(false);
                        }
                        self.window.set_fullscreen(Some(Fullscreen::Borderless(None)));
                    }
                }
                WinCmd::ExitFullscreen => self.window.set_fullscreen(None),
                WinCmd::SetMaximized(on) => self.window.set_maximized(on),
                WinCmd::Minimize => self.window.set_minimized(true),
                WinCmd::DragMove => {
                    let _ = self.window.drag_window();
                }
                WinCmd::DragResize(d) => {
                    let _ = self.window.drag_resize_window(d);
                }
                WinCmd::Topmost(on) => win::set_topmost(self.hwnd, on),
                WinCmd::CursorVisible(v) => {
                    if v != self.cursor_visible {
                        self.cursor_visible = v;
                        self.window.set_cursor_visible(v);
                    }
                }
                WinCmd::Cursor(c) => {
                    if c != self.cursor {
                        self.cursor = c;
                        self.window.set_cursor(c);
                    }
                }
                WinCmd::Title(t) => self.window.set_title(&t),
                WinCmd::Quit => el.exit(),
            }
        }
    }
}

impl ApplicationHandler<UserEvent> for Shell {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        match self.create(el) {
            Ok(mut r) => {
                // a hidden window gets no paint messages: draw the first frame by hand, then show it
                r.redraw(el);
                r.window.request_redraw();
                self.running = Some(r);
            }
            Err(e) => {
                win::message_box(&format!("Reyer could not start.\n\n{e}\n\nYour graphics driver needs to support OpenGL 3.3."), "Reyer");
                self.exit_code = 1;
                el.exit();
            }
        }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, event: UserEvent) {
        let Some(r) = self.running.as_mut() else { return };
        match event {
            UserEvent::MpvWake => r.app.player_events(),
            UserEvent::MpvFrame => r.app.frame_ready(),
            UserEvent::Wake => {}
        }
        r.window.request_redraw();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(r) = self.running.as_mut() else { return };
        let dpi = r.window.scale_factor() as f32;
        match event {
            WindowEvent::CloseRequested => {
                r.app.request_close();
                r.window.request_redraw();
            }
            WindowEvent::RedrawRequested => r.redraw(el),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } | WindowEvent::Moved(_) => {
                r.sync_state();
                r.app.window_changed(&r.state);
                r.window.request_redraw();
            }
            WindowEvent::Focused(f) => {
                r.state.focused = f;
                r.app.focus_changed(f);
                r.window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                r.input.mouse = (position.x as f32 / dpi, position.y as f32 / dpi);
                r.input.mouse_in = true;
                r.input.moved = true;
                r.window.request_redraw();
            }
            WindowEvent::CursorLeft { .. } => {
                r.input.mouse_in = false;
                r.window.request_redraw();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                match (button, state) {
                    (MouseButton::Left, ElementState::Pressed) => r.input.press(),
                    (MouseButton::Left, ElementState::Released) => r.input.release(),
                    (MouseButton::Right, ElementState::Pressed) => r.input.right_pressed = true,
                    (MouseButton::Right, ElementState::Released) => r.input.right_released = true,
                    _ => {}
                }
                r.window.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                r.input.wheel += match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 40.0) as f32,
                };
                r.window.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let m = r.app.modifiers;
                if let Some(k) = map_key(&event, m.0, m.1, m.2) {
                    r.input.keys.push(k);
                    r.window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                let s = m.state();
                r.app.modifiers = (s.control_key(), s.shift_key(), s.alt_key());
            }
            WindowEvent::DroppedFile(path) => {
                r.input.files.push(path.to_string_lossy().into_owned());
                r.input.hovering_files = false;
                r.window.request_redraw();
            }
            WindowEvent::HoveredFile(_) => {
                r.input.hovering_files = true;
                r.window.request_redraw();
            }
            WindowEvent::HoveredFileCancelled => {
                r.input.hovering_files = false;
                r.window.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let Some(r) = self.running.as_mut() else { return };
        if r.app.quit_requested() || r.shot_done {
            el.exit();
            return;
        }
        if r.animating || (self.cli.shot.is_some() && !r.shot_done) {
            r.window.request_redraw();
            el.set_control_flow(ControlFlow::Poll);
            // Poll + vsync'd swap_buffers paces redraws to the display
            return;
        }
        match r.redraw_at {
            Some(t) if t <= Instant::now() => {
                r.window.request_redraw();
                el.set_control_flow(ControlFlow::Wait);
            }
            Some(t) => el.set_control_flow(ControlFlow::WaitUntil(t)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        if let Some(mut r) = self.running.take() {
            r.app.shutdown();
            drop(r.gfx);
            let _ = &r.display;
        }
    }
}

#[allow(dead_code)]
fn _unused(_: Duration, _: ResizeDirection) {}
