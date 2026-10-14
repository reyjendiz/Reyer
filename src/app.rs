//! The Reyer application: player lifecycle, input routing, UI coordination, and frame rendering.
use crate::art::ArtGenerator;
use crate::gfx::gl::{Target, Texture};
use crate::gfx::{Gfx, Rect};
use crate::input::{Input, KeyPress, K};
use crate::media::{AUDIO_EXTS, VIDEO_EXTS};
use crate::player::{Player, Section};
use crate::settings::{data_dir, Settings};
use crate::shell::UserEvent;
use crate::speed::{format_speed, PRESETS};
use crate::spring::{Preset, Spring};
use crate::tokens::tokens;
use crate::ui::chrome::{ChromeAction, ChromeInput, ChromeState};
use crate::ui::content::{tile_index, tint_from_seed, ContentState, CoverBackdrop, NowPlaying};
use crate::ui::dock::{DockAction, DockInput, DockState};
use crate::ui::panels::{PanelAction, PanelState};
use crate::ui::popover::{menu, menu_height, MenuItem, Popover};
use crate::ui::theme::Theme;
use crate::ui::toast::ToastState;
use crate::ui::ambient::Ambient;
use crate::ui::{id, Ui, UiState};
use crate::win::SystemPrefs;
use glow::HasContext;
use std::ffi::{c_char, c_void};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use winit::event_loop::EventLoopProxy;
use winit::window::{CursorIcon, ResizeDirection};

/// A bundled file: next to the executable (or a few folders up), then the working directory, then
/// the source tree the binary was built from (development runs).
fn asset_path(rel: &str) -> PathBuf {
    let exe = std::env::current_exe().ok();
    let dirs = exe.iter().flat_map(|e| e.ancestors().skip(1).take(3).map(std::path::Path::to_path_buf));
    dirs.chain([PathBuf::from("."), PathBuf::from(env!("CARGO_MANIFEST_DIR"))])
        .map(|d| d.join(rel))
        .find(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(rel))
}

#[derive(Default, Clone)]
pub struct Cli {
    pub files: Vec<String>,
    pub shot: Option<String>,
    pub shot_after_ms: u64,
    pub window_rect: Option<[i32; 4]>,
    /// (ms, kind, argument): scripted input
    pub script: Vec<(u64, String, String)>,
}

impl Cli {
    pub fn parse() -> Cli {
        let mut cli = Cli { shot_after_ms: 1200, ..Cli::default() };
        let mut args = std::env::args().skip(1);
        while let Some(a) = args.next() {
            match a.as_str() {
                "--shot" => cli.shot = args.next(),
                // "800:click:700,560;1500:key:escape" - scripted input for screenshots and checks
                "--script" => {
                    cli.script = args.next().unwrap_or_default().split(';').filter_map(|step| {
                        let mut p = step.splitn(3, ':');
                        Some((p.next()?.parse().ok()?, p.next()?.to_string(), p.next()?.to_string()))
                    }).collect();
                }
                "--shot-after" => cli.shot_after_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(1200),
                "--rect" => {
                    let v: Vec<i32> = args.next().unwrap_or_default().split(',').filter_map(|p| p.parse().ok()).collect();
                    if v.len() == 4 {
                        cli.window_rect = Some([v[0], v[1], v[2], v[3]]);
                    }
                }
                _ if !a.starts_with("--") => cli.files.push(a),
                _ => {}
            }
        }
        cli
    }
}

#[derive(Clone, Debug)]
pub struct WinState {
    pub size: (f32, f32),
    pub dpi: f32,
    pub fullscreen: bool,
    pub maximized: bool,
    pub focused: bool,
    pub minimized: bool,
    pub hwnd: isize,
    pub rect: Option<[i32; 4]>,
}

pub enum WinCmd {
    ToggleFullscreen,
    ExitFullscreen,
    SetMaximized(bool),
    Minimize,
    DragMove,
    DragResize(ResizeDirection),
    Topmost(bool),
    CursorVisible(bool),
    Cursor(CursorIcon),
    Title(String),
    Quit,
}

pub struct FrameOut {
    pub animating: bool,
    pub deadline: Option<Instant>,
    pub cmds: Vec<WinCmd>,
}

pub struct App {
    pub modifiers: (bool, bool, bool),
    cli: Cli,
    settings: Settings,
    settings_path: PathBuf,
    sys_prefs: SystemPrefs,
    theme: Theme,
    player: Player,
    art_gen: ArtGenerator,
    display: *mut c_void,
    get_proc: extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,

    ui_state: UiState,
    dock_state: DockState,
    chrome_state: ChromeState,
    content_state: ContentState,
    toast_state: ToastState,
    popover: Popover,
    panel_state: PanelState,

    // Springs and timings
    launch: Spring,
    rise_card: Spring,
    rise_dock: Spring,
    chrome_spring: Spring,
    fs_scale: Spring,
    video_size: (i32, i32),
    dock_dim_spring: Spring,
    top_dim_spring: Spring,

    dock_dimmed: bool,
    top_dimmed: bool,
    last_sample_lum: Instant,

    chrome_visible: bool,
    last_activity: Instant,
    last_click: Option<(Instant, (f32, f32))>,
    click_moved: bool,

    cover_texture: Option<glow::Texture>,
    last_cover_rev: u32,

    topmost: bool,
    quit: bool,
    started: bool,
    pre_fs_size: Option<(f32, f32)>,
    fs_transition: bool,
    save_geo_at: Option<Instant>,
    last_normal_rect: Option<[i32; 4]>,
    last_title: String,
    ambient: Ambient,
    tiles: std::collections::HashMap<u32, glow::Texture>,
    cover_bg: CoverBackdrop,
    track_toast: Option<(Instant, &'static str)>,
    info_sections: Vec<Section>,
    /// Tiny blurred copy of the current frame that fills the letterbox bars.
    bgext: Option<[Target; 2]>,
}

impl App {
    pub fn new(
        cli: &Cli,
        proxy: EventLoopProxy<UserEvent>,
        display: *mut c_void,
        get_proc: extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    ) -> App {
        let settings_path = data_dir().join("settings.json");
        let settings = Settings::load(&settings_path);
        let sys_prefs = SystemPrefs::read();
        let theme = Theme::new(&settings, &sys_prefs, [-0.55, -0.83]);
        let player = Player::new(&settings, proxy.clone());
        let art_gen = ArtGenerator::new(proxy);

        App {
            modifiers: (false, false, false),
            cli: cli.clone(),
            settings,
            settings_path,
            sys_prefs,
            theme,
            player,
            art_gen,
            display,
            get_proc,

            ui_state: UiState::new(),
            dock_state: DockState::new(),
            chrome_state: ChromeState::new(),
            content_state: ContentState::new(),
            toast_state: ToastState::new(),
            popover: Popover::new(),
            panel_state: PanelState::new(),

            launch: Spring::new(Preset::Smooth),
            rise_card: Spring::new(Preset::Smooth),
            rise_dock: Spring::new(Preset::Smooth),
            chrome_spring: Spring::at(Preset::Smooth, 1.0),
            fs_scale: Spring::at(Preset::Smooth, 1.0),
            video_size: (0, 0),
            dock_dim_spring: Spring::new(Preset::Smooth),
            top_dim_spring: Spring::new(Preset::Smooth),

            dock_dimmed: false,
            top_dimmed: false,
            last_sample_lum: Instant::now(),

            chrome_visible: true,
            last_activity: Instant::now(),
            last_click: None,
            click_moved: false,

            cover_texture: None,
            last_cover_rev: 0,

            topmost: false,
            quit: false,
            started: false,
            pre_fs_size: None,
            fs_transition: false,
            save_geo_at: None,
            last_normal_rect: None,
            last_title: String::new(),
            ambient: Ambient::new(),
            tiles: std::collections::HashMap::new(),
            cover_bg: CoverBackdrop::new(),
            track_toast: None,
            info_sections: Vec::new(),
            bgext: None,
        }
    }

    pub fn script_step(&self, i: usize) -> Option<(u64, String, String)> {
        self.cli.script.get(i).cloned()
    }

    pub fn script_pending(&self, i: usize) -> bool {
        i < self.cli.script.len()
    }

    pub fn first_frame_presented(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let _ = self.player.start(&self.settings, self.display, self.get_proc);
        if !self.cli.files.is_empty() {
            self.player.open_paths(&self.cli.files, &mut self.settings);
        }

        self.launch.snap(0.0);
        self.launch.set_target(1.0, false);
        self.rise_card.snap(0.0);
        self.rise_card.set_target(1.0, false);
        self.rise_dock.snap(0.0);
        self.rise_dock.set_target(1.0, false);
    }

    pub fn player_events(&mut self) {
        let ch = self.player.poll(&mut self.settings);
        while let Some(msg) = self.player.toasts.pop_front() {
            self.toast_state.show(&msg, Instant::now());
        }
        if ch.speed {
            self.dock_state.speed_changed(self.player.speed());
            if self.player.has_media() {
                self.toast_state.show(&format_speed(self.player.speed()), Instant::now());
            }
        }
        if ch.tracks || ch.media {
            self.info_sections = self.player.media_info();
        }
        if self.player.art_wanted {
            self.player.art_wanted = false;
            self.art_gen.request(
                &self.player.art_key(),
                &self.player.title(),
                &self.player.artist(),
                &self.player.album(),
            );
        }
        if self.player.closing {
            self.quit = true;
        }
    }

    pub fn frame_ready(&mut self) {
        self.player.frame_ready();
    }

    pub fn window_changed(&mut self, win: &WinState) {
        if !win.fullscreen && !win.maximized && !win.minimized {
            if let Some(r) = win.rect {
                self.last_normal_rect = Some(r);
                self.save_geo_at = Some(Instant::now() + Duration::from_millis(tokens().timings.geometry_save_debounce as u64));
            }
        }
        if win.fullscreen != (self.pre_fs_size.is_some()) {
            if win.fullscreen {
                self.pre_fs_size = Some(win.size);
                self.fs_scale.snap(0.96);
                self.fs_scale.set_target(1.0, false);
            } else {
                self.pre_fs_size = None;
                self.fs_scale.snap(0.96);
                self.fs_scale.set_target(1.0, false);
            }
        }
    }

    pub fn focus_changed(&mut self, focused: bool) {
        if focused {
            self.sys_prefs = SystemPrefs::read();
            self.theme = Theme::new(&self.settings, &self.sys_prefs, self.theme.light_dir);
        }
    }

    pub fn request_close(&mut self) {
        self.player.remember_position(&mut self.settings, false);
        if let Some(r) = self.last_normal_rect {
            self.settings.window_rect = Some(r);
        }
        let _ = self.settings.save(&self.settings_path);
        self.player.shutdown();
        self.quit = true;
    }

    pub fn quit_requested(&self) -> bool {
        self.quit
    }

    pub fn shutdown(&mut self) {
        self.player.shutdown();
        self.art_gen.cancel();
    }

    pub fn shot_request(&self, elapsed: Duration) -> Option<String> {
        let path = self.cli.shot.clone()?;
        (elapsed.as_millis() as u64 >= self.cli.shot_after_ms).then_some(path)
    }

    fn activity(&mut self) {
        self.last_activity = Instant::now();
        self.chrome_visible = true;
    }

    fn open_dialog(&mut self, subtitles: bool) {
        let mut dialog = rfd::FileDialog::new();
        if !self.settings.last_folder.is_empty() {
            dialog = dialog.set_directory(&self.settings.last_folder);
        }
        if subtitles {
            dialog = dialog.add_filter("Subtitles", &["srt", "ass", "ssa", "vtt", "sub"]);
            if let Some(path) = dialog.pick_file() {
                self.player.add_subtitle(&path.to_string_lossy());
                self.toast_state.show("Added subtitle file", Instant::now());
            }
        } else {
            let mut exts = Vec::new();
            exts.extend_from_slice(VIDEO_EXTS);
            exts.extend_from_slice(AUDIO_EXTS);
            dialog = dialog.add_filter("Media Files", &exts);
            if let Some(paths) = dialog.pick_files() {
                let list: Vec<String> = paths.into_iter().map(|p| p.to_string_lossy().into_owned()).collect();
                self.player.open_paths(&list, &mut self.settings);
            }
        }
    }

    /// Opens a popover from `anchor`; asking for the one that is already open closes it instead.
    #[allow(clippy::too_many_arguments)]
    fn toggle_popover(&mut self, g: &mut Gfx, input: &Input, dt: f32, now: Instant, which: &'static str, anchor: u64, size: (f32, f32), window: (f32, f32)) {
        let mut ui = Ui::new(&mut g.r, &mut self.ui_state, input, &self.theme, dt, now);
        if self.popover.is_open() && self.popover.which == which {
            self.popover.hide(&mut ui);
        } else {
            self.popover.show(&ui, which, anchor, size, window, 0);
        }
    }

    pub fn frame(&mut self, g: &mut Gfx, input: &mut Input, win: &WinState, dt: f32, now: Instant) -> FrameOut {
        let mut cmds = Vec::new();
        let (w, h) = g.logical();

        // Check geometry save debounce
        if let Some(deadline) = self.save_geo_at {
            if now >= deadline {
                self.save_geo_at = None;
                if let Some(r) = self.last_normal_rect {
                    self.settings.window_rect = Some(r);
                    let _ = self.settings.save(&self.settings_path);
                }
            }
        }

        if let Some((at, kind)) = self.track_toast {
            if now >= at {
                self.track_toast = None;
                let list = if kind == "audio" { self.player.audio_tracks() } else { self.player.subtitle_tracks() };
                let sel = list.into_iter().find(|t| t.selected).map(|t| t.label).unwrap_or_else(|| "Off".into());
                self.toast_state.show(&format!("{}: {sel}", if kind == "audio" { "Audio" } else { "Subtitles" }), now);
            }
        }

        // Poll background artwork generator
        if let Some((_key, path)) = self.art_gen.poll() {
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    let rgba = img.to_rgba8();
                    self.player.set_cover(Some(crate::player::Cover {
                        w: rgba.width(),
                        h: rgba.height(),
                        rgba: rgba.into_raw(),
                        color: [0.5, 0.5, 0.5],
                    }));
                }
            }
        }

        // Update cover GL texture
        if self.player.cover_rev != self.last_cover_rev {
            self.last_cover_rev = self.player.cover_rev;
            if let Some(old) = self.cover_texture.take() {
                unsafe { g.r.gl.delete_texture(old) };
            }
            if let Some(cov) = &self.player.cover {
                let tex = Texture::from_rgba(&g.r.gl, cov.w, cov.h, &cov.rgba, false);
                self.cover_texture = Some(tex.id);
            }
        }

        // empty-state backgrounds load one per frame, never all at startup
        if !self.player.has_media() {
            self.ambient.load_next(&g.r.gl, asset_path);
        }

        // Mouse activity & parallax light direction
        if input.moved {
            let (mx, my) = input.mouse;
            self.theme.light_dir = [
                -0.55 + (mx / w.max(1.0) - 0.5) * 0.35,
                -0.83 + (my / h.max(1.0) - 0.5) * 0.25,
            ];
        }
        // moving the pointer elsewhere does not bring the controls back; hovering them does (below)
        if input.pressed || input.wheel != 0.0 || !input.keys.is_empty() {
            self.activity();
        }

        // Drag & drop files
        if !input.files.is_empty() {
            let files = std::mem::take(&mut input.files);
            self.player.open_paths(&files, &mut self.settings);
            self.dock_state.breathe_once();
            self.activity();
        }

        // Render mpv frame into g.video target
        // a resized target is blank until mpv draws again, which it won't do for a paused frame by itself
        if self.video_size != (g.video.w, g.video.h) {
            self.video_size = (g.video.w, g.video.h);
            self.player.request_render();
        }
        self.player.render(&g.video);

        // Letterbox "background extension": a 24 px copy of the picture, blurred, behind the video
        let vr = self.player.video_rect();
        let bg_ext = self.player.has_video() && self.settings.background_extension && (vr[2] < 0.995 || vr[3] < 0.995);
        if bg_ext {
            let (vw, vh) = (vr[2] * w, vr[3] * h);
            let (tw, th) = (24, ((24.0 * vh / vw.max(1.0)).round() as i32).max(6));
            let gl = g.r.gl.clone();
            let ts = self.bgext.get_or_insert_with(|| [Target::new(&gl, tw, th, true), Target::new(&gl, tw, th, true)]);
            ts[0].resize(&gl, tw, th);
            ts[1].resize(&gl, tw, th);
            // the video target is sampled bottom-up, hence the flipped v range
            g.r.copy_pass(g.video.tex, &ts[0], [vr[0], 1.0 - (vr[1] + vr[3]), vr[2], vr[3]], -0.32, 1.1);
            for _ in 0..3 {
                g.r.blur_pass(ts[0].tex, &ts[1], (1.0 / tw as f32, 0.0));
                g.r.blur_pass(ts[1].tex, &ts[0], (0.0, 1.0 / th as f32));
            }
        }

        self.theme.glass_opacity = self.settings.glass_opacity; // live while the slider moves

        // Auto-hide chrome logic
        let d = self.dock_state.rect;
        let (mx, my) = input.mouse;
        // the dock's footprint plus a margin, down to the window edge: hovering it reveals and holds the controls
        let over_dock = input.mouse_in && mx >= d.x - 24.0 && mx <= d.x + d.w + 24.0 && my >= d.y - 24.0;
        let hold_chrome = over_dock
            || !self.player.has_media()
            || self.player.paused()
            || self.popover.is_open()
            || self.dock_state.busy
            || self.chrome_state.hovered; // keys count as activity, so keyboard use never pins the controls

        let auto_hide_dur = Duration::from_millis(tokens().timings.auto_hide as u64);
        let chrome_target = if hold_chrome || now.duration_since(self.last_activity) < auto_hide_dur {
            1.0
        } else {
            0.0
        };
        let mut anim = false;
        let chrome_t = self.chrome_spring.to(chrome_target, dt, self.theme.reduce_motion, &mut anim);
        let launch = self.launch.to(1.0, dt, self.theme.reduce_motion, &mut anim);
        let rise_c = self.rise_card.to(1.0, dt, self.theme.reduce_motion, &mut anim);
        let rise_d = self.rise_dock.to(1.0, dt, self.theme.reduce_motion, &mut anim);
        let fs = self.fs_scale.to(1.0, dt, self.theme.reduce_motion, &mut anim);
        // subtitles ride above the dock while it is showing
        let video_bottom = (vr[1] + vr[3]) * h;
        let covered = (video_bottom - (self.dock_state.rect.y - 8.0)).max(0.0) * chrome_t;
        self.player.set_sub_lift(covered * 720.0 / h.max(1.0));

        // Cursor visibility
        let cursor_show = chrome_t > 0.01 || !self.player.has_media() || input.hovering_files;
        cmds.push(WinCmd::CursorVisible(cursor_show));

        // Sample video luminance for clear glass dimming hysteresis
        if self.player.has_video() && !self.player.paused() && chrome_t > 0.05 && now.duration_since(self.last_sample_lum) >= Duration::from_millis(250) {
            self.last_sample_lum = now;
            // mean luminance behind the dock, the window buttons and the title pill (last frame's backdrop)
            let (dim_on, dim_off) = (tokens().glass.dim_on_luminance, tokens().glass.dim_off_luminance);
            let m = tokens().spacing.m;
            let norm = |r: Rect| [r.x / w.max(1.0), r.y / h.max(1.0), r.w / w.max(1.0), r.h / h.max(1.0)];
            let cluster = Rect::new(w - 142.0 - m, m, 142.0, tokens().sizes.chrome_height);
            let lum = g.backdrop.luminance(&g.r.gl, &[norm(self.dock_state.rect), norm(cluster), norm(self.chrome_state.pill)]);
            let top = lum[1].max(if self.player.title().is_empty() { 0.0 } else { lum[2] });
            let hyst = |on: bool, l: f32| if on { l > dim_off } else { l > dim_on };
            self.dock_dimmed = hyst(self.dock_dimmed, lum[0]);
            self.top_dimmed = hyst(self.top_dimmed, top);
        }
        let dock_dim = self.dock_dim_spring.to(if self.dock_dimmed && self.player.has_video() { tokens().glass.dimming } else { 0.0 }, dt, self.theme.reduce_motion, &mut anim);
        let top_dim = self.top_dim_spring.to(if self.top_dimmed && self.player.has_video() { tokens().glass.dimming } else { 0.0 }, dt, self.theme.reduce_motion, &mut anim);

        // Now Playing art: the embedded cover, or a generated tile picked by a hash of the title
        let mut art_tex = None;
        if self.player.is_audio() {
            let key = match self.cover_texture {
                Some(_) => self.player.cover_rev as u64,
                None => (1u64 << 40) | tile_index(self.player.art_seed() as i64) as u64,
            };
            art_tex = self.cover_texture.or_else(|| {
                let i = tile_index(self.player.art_seed() as i64);
                let gl = &g.r.gl;
                Some(*self.tiles.entry(i).or_insert_with(|| {
                    let img = std::fs::read(asset_path(&format!("assets/art/tile_{i:02}.jpg"))).ok().and_then(|b| image::load_from_memory(&b).ok());
                    match img.map(|i| i.to_rgba8()) {
                        Some(rgba) => Texture::from_rgba(gl, rgba.width(), rgba.height(), &rgba, true).id,
                        None => Texture::from_rgba(gl, 1, 1, &[60, 60, 70, 255], false).id,
                    }
                }))
            });
            if let Some(t) = art_tex {
                self.cover_bg.update(g, t, key);
            }
        }

        // springs drawn in the content layer must keep frames coming too
        let mut content_wake: Option<Instant> = None;

        // 1. Begin Content Layer (Scene)
        g.begin_scene();
        let bg_color = if self.player.has_video() {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            self.theme.c.content_background.0
        };
        g.r.clear(bg_color);
        // entering or leaving full screen, the content eases from slightly smaller to the new size
        g.r.push((w / 2.0, h / 2.0), fs, (0.0, 0.0), 1.0);

        if self.player.has_video() {
            if let (true, Some(ts)) = (bg_ext, self.bgext.as_ref()) {
                // aspect-fill the window with the blurred copy, centred on the picture
                let (vw, vh) = (vr[2] * w, vr[3] * h);
                let fill = (w / vw.max(1.0)).max(h / vh.max(1.0)) * 1.08;
                let (dw, dh) = (vw * fill, vh * fill);
                let (cx, cy) = ((vr[0] + vr[2] / 2.0) * w, (vr[1] + vr[3] / 2.0) * h);
                g.r.target_image(ts[0].tex, Rect::new(cx - dw / 2.0, cy - dh / 2.0, dw, dh), 1.0);
            }
            // Live video frame
            g.r.target_image(g.video.tex, Rect::new(0.0, 0.0, w, h), 1.0);
        } else if self.player.is_audio() {
            let seed = self.player.art_seed() as i64;
            let tint = tint_from_seed(seed);
            let mood = match &self.player.cover {
                Some(cov) => [cov.color[0], cov.color[1], cov.color[2], 1.0],
                None => tint,
            };
            let mut ui = Ui::new(&mut g.r, &mut self.ui_state, input, &self.theme, dt, now);
            let (bg, bg_front) = self.cover_bg.layers(&mut ui);
            let np = NowPlaying {
                title: &self.player.title(),
                artist: &self.player.artist(),
                album: &self.player.album(),
                cover: art_tex,
                has_cover: self.player.cover.is_some(),
                mood,
                tint,
                pill: self.chrome_state.pill,
                revision: self.player.cover_rev as u64,
                bg,
                bg_front,
            };
            self.content_state.draw_now_playing(&mut ui, (w, h), true, &np);
            anim |= ui.animating;
        } else {
            let running = win.focused && !win.minimized && !self.sys_prefs.battery_saver;
            let mut ui = Ui::new(&mut g.r, &mut self.ui_state, input, &self.theme, dt, now);
            self.ambient.draw(&mut ui, (w, h), launch > 0.6, running);
            anim |= ui.animating;
            content_wake = ui.wake;
        }

        g.r.pop();

        // 2. Finish Scene -> build backdrop blur and render scene texture to backbuffer
        g.finish_scene();

        // empty-state card and drop highlight sit above the content layer (they never blur themselves)
        {
            let mut ui = Ui::new(&mut g.r, &mut self.ui_state, input, &self.theme, dt, now);
            self.content_state.draw_empty(&mut ui, (w, h), self.player.has_media(), rise_c);
            self.content_state.draw_drop_glow(&mut ui, (w, h), input.hovering_files, win.fullscreen);
            anim |= ui.animating;
        }


        // 3. Decor and Edge Bands
        if self.player.has_video() && chrome_t > 0.01 {
            g.r.edge_band(Rect::new(0.0, 0.0, w, 96.0), true, 0.18, chrome_t);
            g.r.edge_band(Rect::new(0.0, h - 132.0, w, 132.0), false, 0.18, chrome_t);
        }

        // 4. UI Layer: Top Chrome, Dock, Overlay Popovers
        let title = self.player.title();
        let subtitle = if self.player.is_audio() {
            let art = self.player.artist();
            let alb = self.player.album();
            if !art.is_empty() && !alb.is_empty() {
                format!("{art} — {alb}")
            } else if !art.is_empty() {
                art
            } else {
                alb
            }
        } else {
            String::new()
        };

        let chrome_in = ChromeInput {
            window: (w, h),
            chrome_t,
            dim: top_dim,
            title: &title,
            subtitle: &subtitle,
            topmost: self.topmost,
            maximized: win.maximized,
            fullscreen: win.fullscreen,
        };

        let dock_in = DockInput {
            window: (w, h),
            chrome_t,
            rise: rise_d,
            dim: dock_dim,
            unified: self.settings.dock_style == "unified",
            has_media: self.player.has_media(),
            paused: self.player.paused(),
            muted: self.player.muted(),
            volume: self.player.volume(),
            volume_max: crate::volume::MAX_PERCENT,
            position: self.player.position(),
            duration: self.player.duration(),
            chapters: self.player.chapters().iter().map(|&c| if self.player.duration() > 0.0 { (c / self.player.duration()) as f32 } else { 0.0 }).collect(),
            speed: self.player.speed(),
            has_audio_tracks: !self.player.audio_tracks().is_empty(),
            subtitle_active: self.player.subtitle_tracks().iter().any(|t| t.selected),
            fullscreen: win.fullscreen,
        };

        let audio_tracks = self.player.audio_tracks();
        let subtitle_tracks = self.player.subtitle_tracks();
        let has_media = self.player.has_media();
        let current_speed = self.player.speed();
        let info_secs = if self.info_sections.is_empty() { self.player.media_info() } else { self.info_sections.clone() };

        struct UiEvents {
            clicked_bg_pause: bool,
            clicked_bg_open: bool,
            chrome_acts: Vec<ChromeAction>,
            dock_acts: Vec<DockAction>,
            pop_chosen_audio: Option<usize>,
            pop_chosen_sub: Option<usize>,
            pop_open_dialog_sub: bool,
            pop_chosen_speed: Option<usize>,
            pop_open_info: bool,
            pop_panel_acts: Vec<PanelAction>,
            pop_hide: bool,
            pop_tools: Option<&'static str>,
            unused_keys: Vec<KeyPress>,
            animating: bool,
            wake: Option<Instant>,
        }

        let mut events = UiEvents {
            clicked_bg_pause: false,
            clicked_bg_open: false,
            chrome_acts: Vec::new(),
            dock_acts: Vec::new(),
            pop_chosen_audio: None,
            pop_chosen_sub: None,
            pop_open_dialog_sub: false,
            pop_chosen_speed: None,
            pop_open_info: false,
            pop_panel_acts: Vec::new(),
            pop_hide: false,
            pop_tools: None,
            unused_keys: Vec::new(),
            animating: anim,
            wake: content_wake,
        };

        {
            let mut ui = Ui::new(&mut g.r, &mut self.ui_state, input, &self.theme, dt, now);
            ui.animating |= anim;
            // hidden chrome is not clickable (the first move only brings it back)
            ui.blocked = self.popover.is_open() || chrome_t < 0.05;

            events.chrome_acts = self.chrome_state.draw(&mut ui, &chrome_in);
            events.dock_acts = self.dock_state.draw(&mut ui, &dock_in);

            // Interaction on background video area (if not over chrome)
            let clicked_bg = input.pressed && !ui.over_chrome && !self.popover.is_open();
            if clicked_bg {
                self.click_moved = false;
                self.last_click = Some((now, input.mouse));
            }
            if input.down && !self.click_moved {
                if let Some((_, start)) = self.last_click {
                    let (mx, my) = input.mouse;
                    if (mx - start.0).abs() + (my - start.1).abs() > 5.0 {
                        self.click_moved = true;
                        if !win.fullscreen {
                            cmds.push(WinCmd::DragMove);
                        }
                    }
                }
            }
            if input.released && !self.click_moved {
                if let Some((t, _)) = self.last_click.take() {
                    if now.duration_since(t) < Duration::from_millis(tokens().timings.click_delay as u64) {
                        if has_media {
                            events.clicked_bg_pause = true;
                        } else {
                            events.clicked_bg_open = true;
                        }
                    }
                }
            }
            if input.double_click && !ui.over_chrome && has_media {
                cmds.push(WinCmd::ToggleFullscreen);
            }
            if input.wheel != 0.0 && !ui.over_chrome && !self.popover.is_open() {
                let v = self.player.nudge_volume(if input.wheel > 0.0 { 5.0 } else { -5.0 });
                self.dock_state.poke_volume(false, now);
                self.toast_state.show(&format!("Volume {}%", v.round() as i32), now);
            }

            ui.blocked = false;

            if self.popover.shown() {
                match self.popover.which {
                    "audio" => {
                        let items: Vec<MenuItem> = audio_tracks.iter().map(|t| MenuItem { label: t.label.clone(), checked: t.selected }).collect();
                        let mut chosen = None;
                        self.popover.draw(&mut ui, |ui, cur, body, prog| {
                            chosen = menu(ui, cur, body, prog, "Audio Tracks", &items);
                        });
                        if let Some(idx) = chosen {
                            events.pop_chosen_audio = Some(idx);
                            events.pop_hide = true;
                        }
                    }
                    "subtitles" => {
                        let any_sel = subtitle_tracks.iter().any(|t| t.selected);
                        let mut items = vec![MenuItem { label: "Off".into(), checked: !any_sel }];
                        items.extend(subtitle_tracks.iter().map(|t| MenuItem { label: t.label.clone(), checked: t.selected }));
                        items.push(MenuItem { label: "Add Subtitle File…".into(), checked: false });
                        let mut chosen = None;
                        self.popover.draw(&mut ui, |ui, cur, body, prog| {
                            chosen = menu(ui, cur, body, prog, "Subtitles", &items);
                        });
                        if let Some(idx) = chosen {
                            if idx == items.len() - 1 {
                                events.pop_open_dialog_sub = true;
                            } else {
                                events.pop_chosen_sub = Some(idx);
                            }
                            events.pop_hide = true;
                        }
                    }
                    "speed" => {
                        let items: Vec<MenuItem> = PRESETS.iter().map(|&s| MenuItem {
                            label: format_speed(s),
                            checked: (s - current_speed).abs() < 1e-4,
                        }).collect();
                        let mut chosen = None;
                        self.popover.draw(&mut ui, |ui, cur, body, prog| {
                            chosen = menu(ui, cur, body, prog, "Playback Speed", &items);
                        });
                        if let Some(idx) = chosen {
                            events.pop_chosen_speed = Some(idx);
                            events.pop_hide = true;
                        }
                    }
                    "settings" => {
                        self.popover.draw(&mut ui, |ui, _cur, body, _prog| {
                            events.pop_panel_acts = self.panel_state.settings(ui, body, &mut self.settings, has_media);
                        });
                    }
                    "info" => {
                        self.popover.draw(&mut ui, |ui, _cur, body, _prog| {
                            events.pop_panel_acts = self.panel_state.info(ui, body, &info_secs);
                        });
                    }
                    "tools" => {
                        self.popover.draw(&mut ui, |ui, _cur, body, _prog| {
                            let by = body.y + 8.0;
                            let bx = |i: f32| body.x + 12.0 + 36.0 * i;
                            if ui.icon_button(id("tools/audio"), Rect::new(bx(0.0), by, 34.0, 34.0), "audio", 18.0, crate::ui::BtnStyle { on_regular: true, label: "Audio", ..Default::default() }).clicked {
                                events.pop_tools = Some("audio");
                            }
                            if ui.icon_button(id("tools/sub"), Rect::new(bx(1.0), by, 34.0, 34.0), "subtitles", 18.0, crate::ui::BtnStyle { on_regular: true, label: "Subtitles", ..Default::default() }).clicked {
                                events.pop_tools = Some("subtitles");
                            }
                            if ui.icon_button(id("tools/settings"), Rect::new(bx(2.0), by, 34.0, 34.0), "settings", 18.0, crate::ui::BtnStyle { on_regular: true, label: "Settings", ..Default::default() }).clicked {
                                events.pop_tools = Some("settings");
                            }
                            if ui.icon_button(id("tools/open"), Rect::new(bx(3.0), by, 34.0, 34.0), "open", 18.0, crate::ui::BtnStyle { on_regular: true, label: "Open", ..Default::default() }).clicked {
                                events.pop_tools = Some("open");
                            }
                            if ui.icon_button(id("tools/fs"), Rect::new(bx(4.0), by, 34.0, 34.0), if win.fullscreen { "exitFullscreen" } else { "fullscreen" }, 18.0, crate::ui::BtnStyle { on_regular: true, label: "Fullscreen", ..Default::default() }).clicked {
                                events.pop_tools = Some("fullscreen");
                            }
                        });
                    }
                    _ => {}
                }
            }

            self.toast_state.draw(&mut ui, (w, h), self.chrome_state.pill);
            ui.draw_tooltip((w, h));
            events.unused_keys = ui.unused_keys();
            ui.finish();

            events.animating |= ui.animating;
            events.wake = match (events.wake, ui.wake) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }

        // Apply background click actions
        if events.clicked_bg_pause {
            self.player.toggle_pause();
        } else if events.clicked_bg_open {
            self.open_dialog(false);
        }

        // Process Chrome Actions
        for act in events.chrome_acts {
            match act {
                ChromeAction::TogglePin => {
                    self.topmost = !self.topmost;
                    cmds.push(WinCmd::Topmost(self.topmost));
                    self.toast_state.show(if self.topmost { "Keep on Top" } else { "Normal Window Order" }, now);
                }
                ChromeAction::Minimize => cmds.push(WinCmd::Minimize),
                ChromeAction::ToggleMaximize => {
                    if win.fullscreen {
                        cmds.push(WinCmd::ExitFullscreen);
                    } else {
                        cmds.push(WinCmd::SetMaximized(!win.maximized));
                    }
                }
                ChromeAction::Close => self.request_close(),
            }
        }

        // Process Dock Actions
        for act in events.dock_acts {
            match act {
                DockAction::Previous => self.player.previous_track(&mut self.settings),
                DockAction::Next => { self.player.next_track(&mut self.settings); }
                DockAction::TogglePause => self.player.toggle_pause(),
                DockAction::ToggleMute => {
                    // mpv confirms the change later: the message comes from the state before the toggle
                    let was_muted = self.player.muted();
                    self.player.toggle_mute();
                    self.toast_state.show(if was_muted { "Sound On" } else { "Muted" }, now);
                }
                DockAction::SetVolume(v) => self.player.set_volume(v),
                DockAction::Seek(s, exact) => self.player.seek(s, exact),
                DockAction::StepSpeed(dir) => self.player.step_speed(dir),
                DockAction::ResetSpeed => self.player.set_speed(1.0),
                DockAction::SpeedPresets => {
                    let anchor = id(crate::ui::dock::SPEED);
                    if self.ui_state.rects.contains_key(&anchor) {
                        self.toggle_popover(g, input, dt, now, "speed", anchor, (220.0, menu_height(true, PRESETS.len())), (w, h));
                    }
                }
                DockAction::Popover(kind) => {
                    let (anchor, size) = match kind {
                        "audio" => (id(crate::ui::dock::AUDIO), (260.0, menu_height(true, self.player.audio_tracks().len()))),
                        "subtitles" => (id(crate::ui::dock::SUBTITLES), (260.0, menu_height(true, self.player.subtitle_tracks().len() + 2))),
                        "settings" => (id(crate::ui::dock::SETTINGS), (380.0, 520.0)),
                        "tools" => (id(crate::ui::dock::MORE), (210.0, 52.0)),
                        _ => (0, (240.0, 200.0)),
                    };
                    if anchor != 0 {
                        self.toggle_popover(g, input, dt, now, kind, anchor, size, (w, h));
                    }
                }
                DockAction::Open => self.open_dialog(false),
                DockAction::Fullscreen => cmds.push(WinCmd::ToggleFullscreen),
            }
        }

        // Process Popover Events
        if events.pop_hide {
            self.popover.hide(&mut Ui::new(&mut g.r, &mut self.ui_state, input, &self.theme, dt, now));
        }
        if let Some(idx) = events.pop_chosen_audio {
            if let Some(t) = audio_tracks.get(idx) {
                self.player.select_track("audio", t.id);
                self.toast_state.show(&format!("Audio: {}", t.label), now);
            }
        }
        if let Some(idx) = events.pop_chosen_sub {
            if idx == 0 {
                self.player.select_track("subtitles", 0);
                self.toast_state.show("Subtitles: Off", now);
            } else if let Some(t) = subtitle_tracks.get(idx - 1) {
                self.player.select_track("subtitles", t.id);
                self.toast_state.show(&format!("Subtitles: {}", t.label), now);
            }
        }
        if events.pop_open_dialog_sub {
            self.open_dialog(true);
        }
        if let Some(idx) = events.pop_chosen_speed {
            if let Some(&s) = PRESETS.get(idx) {
                self.player.set_speed(s);
                self.toast_state.show(&format_speed(s), now);
            }
        }
        if let Some(tool) = events.pop_tools {
            match tool {
                "audio" => {
                    self.popover.show(&Ui::new(&mut g.r, &mut self.ui_state, input, &self.theme, dt, now), "audio", id("tools/audio"), (260.0, menu_height(true, audio_tracks.len())), (w, h), 0);
                }
                "subtitles" => {
                    self.popover.show(&Ui::new(&mut g.r, &mut self.ui_state, input, &self.theme, dt, now), "subtitles", id("tools/sub"), (260.0, menu_height(true, subtitle_tracks.len() + 2)), (w, h), 0);
                }
                "settings" => {
                    self.popover.show(&Ui::new(&mut g.r, &mut self.ui_state, input, &self.theme, dt, now), "settings", id("tools/settings"), (380.0, 520.0), (w, h), 0);
                }
                "open" => {
                    self.open_dialog(false);
                    self.popover.hide(&mut Ui::new(&mut g.r, &mut self.ui_state, input, &self.theme, dt, now));
                }
                "fullscreen" => {
                    cmds.push(WinCmd::ToggleFullscreen);
                    self.popover.hide(&mut Ui::new(&mut g.r, &mut self.ui_state, input, &self.theme, dt, now));
                }
                _ => {}
            }
        }
        for act in events.pop_panel_acts {
            match act {
                PanelAction::Applied(k, v) => self.player.apply_setting(k, v),
                PanelAction::Changed => {
                    self.theme = Theme::new(&self.settings, &self.sys_prefs, self.theme.light_dir);
                    let _ = self.settings.save(&self.settings_path);
                }
                PanelAction::InfoRequested => {
                    self.popover.show(&Ui::new(&mut g.r, &mut self.ui_state, input, &self.theme, dt, now), "info", id(crate::ui::dock::SETTINGS), (440.0, 480.0), (w, h), 0);
                }
                PanelAction::Copy(text) => {
                    if let Ok(mut clip) = arboard::Clipboard::new() {
                        let _ = clip.set_text(&text);
                        self.toast_state.show("Copied to clipboard", now);
                    }
                }
            }
        }

        // Global Keyboard Shortcuts
        for k in events.unused_keys {
            match k.key {
                K::Space => self.player.toggle_pause(),
                K::Left => {
                    let d = if k.shift { -1.0 } else if k.ctrl { -30.0 } else { -5.0 };
                    self.player.seek_relative(d);
                }
                K::Right => {
                    let d = if k.shift { 1.0 } else if k.ctrl { 30.0 } else { 5.0 };
                    self.player.seek_relative(d);
                }
                K::Up => {
                    let v = self.player.nudge_volume(5.0);
                    self.dock_state.poke_volume(false, now);
                    self.toast_state.show(&format!("Volume {}%", v.round() as i32), now);
                }
                K::Down => {
                    let v = self.player.nudge_volume(-5.0);
                    self.dock_state.poke_volume(false, now);
                    self.toast_state.show(&format!("Volume {}%", v.round() as i32), now);
                }
                K::Escape => {
                    if self.popover.is_open() {
                        self.popover.hide(&mut Ui::new(&mut g.r, &mut self.ui_state, input, &self.theme, dt, now));
                    } else if win.fullscreen {
                        cmds.push(WinCmd::ExitFullscreen);
                    }
                }
                K::Home => self.player.seek(0.0, true),
                K::PageDown => { self.player.next_track(&mut self.settings); }
                K::PageUp => self.player.previous_track(&mut self.settings),
                K::Backspace => self.player.set_speed(1.0),
                K::Char('f') => cmds.push(WinCmd::ToggleFullscreen),
                K::Char('m') => {
                    // mpv confirms the change later: the message comes from the state before the toggle
                    let was_muted = self.player.muted();
                    self.player.toggle_mute();
                    self.toast_state.show(if was_muted { "Sound On" } else { "Muted" }, now);
                }
                K::Char('o') => self.open_dialog(false),
                K::Char('n') => { self.player.next_track(&mut self.settings); }
                K::Char('p') => self.player.previous_track(&mut self.settings),
                K::Char(']') => self.player.step_speed(1),
                K::Char('[') => self.player.step_speed(-1),
                K::Char('a') => {
                    self.player.cycle_track("audio");
                    // announced once mpv has switched (its track list arrives a moment later)
                    self.track_toast = Some((now + Duration::from_millis(150), "audio"));
                }
                K::Char('s') => {
                    self.player.cycle_track("subtitles");
                    self.track_toast = Some((now + Duration::from_millis(150), "subtitles"));
                }
                K::Char('i') => {
                    if has_media {
                        let anchor = id(crate::ui::dock::SETTINGS);
                        self.toggle_popover(g, input, dt, now, "info", anchor, (440.0, 480.0), (w, h));
                    }
                }
                K::Char('t') => {
                    self.topmost = !self.topmost;
                    cmds.push(WinCmd::Topmost(self.topmost));
                    self.toast_state.show(if self.topmost { "Keep on Top" } else { "Normal Window Order" }, now);
                }
                K::Char('v') if k.ctrl => {
                    if let Ok(mut clip) = arboard::Clipboard::new() {
                        if let Ok(text) = clip.get_text() {
                            let text = text.trim().trim_matches('"');
                            if !text.is_empty() {
                                self.player.open_paths(&[text.to_string()], &mut self.settings);
                            }
                        }
                    }
                }
                K::Char(',') if k.ctrl => {
                    let anchor = id(crate::ui::dock::SETTINGS);
                    self.toggle_popover(g, input, dt, now, "settings", anchor, (380.0, 520.0), (w, h));
                }
                _ => {}
            }
        }

        // Flush 2D rendering commands
        g.r.flush();

        // Window Title
        let desired_title = if self.player.title().is_empty() {
            "Reyer".to_string()
        } else {
            format!("{} – Reyer", self.player.title())
        };
        if desired_title != self.last_title {
            self.last_title = desired_title.clone();
            cmds.push(WinCmd::Title(desired_title));
        }

        let animating = events.animating || self.toast_state.visible() || self.popover.shown() || self.player.has_video();
        // timers that need a frame even when nothing moves
        let hide_at = (self.player.has_media() && !self.player.paused()).then(|| self.last_activity + Duration::from_millis(tokens().timings.auto_hide as u64));
        let deadline = [events.wake, self.track_toast.map(|t| t.0), self.save_geo_at, hide_at].into_iter().flatten().filter(|t| *t > now).min();
        FrameOut { animating, deadline, cmds }
    }
}
