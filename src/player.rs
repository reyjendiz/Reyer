//! The playback engine: libmpv state mirrored for the UI, commands, playlist, resume positions,
//! cover art, file information and the Dolby/DTS passthrough fallback.
use crate::gfx::gl::Target;
use crate::media::{self, looks_protected};
use crate::mpv::{self, Event, Mpv, RenderContext, END_REASON_ERROR, RENDER_UPDATE_FRAME};
use crate::playlist::Playlist;
use crate::settings::Settings;
use crate::shell::UserEvent;
use crate::speed::{step_speed, MAX_SPEED, MIN_SPEED};
use crate::volume::{from_mpv, to_mpv, MAX_PERCENT};
use serde_json::Value;
use std::collections::VecDeque;
use std::ffi::{c_char, c_void};
use std::path::Path;
use winit::event_loop::EventLoopProxy;

/// Seconds played (and left) before a position is worth remembering.
const RESUME_MIN: f64 = 10.0;
/// Formats sent untouched to a receiver, which decodes them itself (this is how Atmos / DTS:X reach it).
pub const PASSTHROUGH: &str = "ac3,eac3,dts-hd,truehd";

/// Reply id of asynchronous seeks.
const SEEK_REPLY: u64 = 1000;

const OBSERVED: [&str; 14] = [
    "time-pos", "duration", "pause", "volume", "mute", "speed", "track-list", "aid", "sid", "chapter-list", "metadata", "media-title",
    "eof-reached", "osd-dimensions",
];

#[derive(Default, Clone, Copy)]
pub struct Changes {
    pub media: bool,
    pub tracks: bool,
    pub speed: bool,
    pub volume: bool,
    pub cover: bool,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub id: i64,
    pub label: String,
    pub selected: bool,
}

pub struct Cover {
    pub rgba: Vec<u8>,
    pub w: u32,
    pub h: u32,
    pub color: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct Section {
    pub title: &'static str,
    pub rows: Vec<(String, String)>,
}

extern "C" fn on_wakeup(data: *mut c_void) {
    // SAFETY: `data` is a leaked EventLoopProxy that lives for the whole process
    let proxy = unsafe { &*(data as *const EventLoopProxy<UserEvent>) };
    let _ = proxy.send_event(UserEvent::MpvWake);
}

extern "C" fn on_frame(data: *mut c_void) {
    let proxy = unsafe { &*(data as *const EventLoopProxy<UserEvent>) };
    let _ = proxy.send_event(UserEvent::MpvFrame);
}

pub struct Player {
    mpv: Option<Mpv>,
    render: Option<RenderContext>,
    proxy: *const EventLoopProxy<UserEvent>,
    pub playlist: Playlist,
    pub loaded: bool,
    pub closing: bool,
    pub toasts: VecDeque<String>,
    pub cover: Option<Cover>,
    pub cover_rev: u32,
    needs_render: bool,
    speed_target: f32,
    volume_target: f32,
    passthrough_rejected: bool,
    seek_in_flight: bool,
    seek_queued: Option<(f64, bool)>,
    /// Where a running seek is going; shown as the position until mpv gets there.
    seek_target: Option<f64>,
    // mirrored mpv properties
    time_pos: f64,
    duration: f64,
    paused: bool,
    volume: f64,
    muted: bool,
    speed: f64,
    track_list: Vec<Value>,
    chapters: Vec<f64>,
    metadata: serde_json::Map<String, Value>,
    media_title: String,
    eof: bool,
    osd: Value,
    /// Set when an audio file is playing and ComfyUI art generation could be requested.
    pub art_wanted: bool,
}

impl Player {
    pub fn new(settings: &Settings, proxy: EventLoopProxy<UserEvent>) -> Player {
        Player {
            mpv: None,
            render: None,
            proxy: Box::into_raw(Box::new(proxy)),
            playlist: Playlist::default(),
            loaded: false,
            closing: false,
            toasts: VecDeque::new(),
            cover: None,
            cover_rev: 0,
            needs_render: false,
            speed_target: 1.0,
            volume_target: settings.volume,
            passthrough_rejected: false,
            seek_in_flight: false,
            seek_queued: None,
            seek_target: None,
            time_pos: 0.0,
            duration: 0.0,
            paused: true,
            volume: to_mpv(settings.volume) as f64,
            muted: settings.muted,
            speed: 1.0,
            track_list: vec![],
            chapters: vec![],
            metadata: Default::default(),
            media_title: String::new(),
            eof: false,
            osd: Value::Null,
            art_wanted: false,
        }
    }

    // ---- lifecycle -----------------------------------------------------------------------------------
    /// Loads libmpv and creates the player and its render context. The GL context must be current.
    pub fn start(&mut self, settings: &Settings, display: *mut c_void, get_proc: extern "C" fn(*mut c_void, *const c_char) -> *mut c_void) -> Result<(), String> {
        let path = mpv::find_libmpv().ok_or("libmpv-2.dll not found")?;
        let m = Mpv::load(&path)?;
        let hwdec = std::env::var("REYER_HWDEC").unwrap_or_else(|_| if settings.hwdec { "auto-safe".into() } else { "no".into() });
        let opts: [(&str, String); 36] = [
            ("vo", "libmpv".into()),
            ("keep-open", "yes".into()),
            ("idle", "yes".into()),
            ("osc", "no".into()),
            ("osd-level", "0".into()),
            ("config", "no".into()),
            ("ytdl", "no".into()),
            ("audio-client-name", "Reyer".into()), // the name Windows shows in the volume mixer
            ("ao", "wasapi".into()),               // never fall through to other drivers
            ("load-scripts", "no".into()),
            ("load-stats-overlay", "no".into()),
            ("load-console", "no".into()),
            ("load-osd-console", "no".into()),
            ("load-auto-profiles", "no".into()),
            ("load-select", "no".into()),
            ("load-positioning", "no".into()),
            ("load-commands", "no".into()),
            ("load-context-menu", "no".into()),
            ("input-default-bindings", "no".into()),
            ("input-vo-keyboard", "no".into()),
            ("audio-display", "no".into()),
            ("hwdec", hwdec),
            ("sub-auto", "fuzzy".into()),
            ("audio-pitch-correction", "yes".into()),
            ("tone-mapping", "auto".into()),
            ("hdr-compute-peak", "auto".into()),
            // transparent bars: the content layer draws its own background extension behind the video
            ("border-background", "color".into()),
            ("background-color", "#00000000".into()),
            ("volume", format!("{}", to_mpv(settings.volume))),
            ("mute", if settings.muted { "yes" } else { "no" }.into()),
            ("volume-max", format!("{}", to_mpv(MAX_PERCENT))),
            // TrueHD and DTS-HD bitstreams need the device to themselves (WASAPI exclusive mode)
            ("audio-spdif", if settings.audio_passthrough { PASSTHROUGH } else { "" }.into()),
            ("audio-exclusive", if settings.audio_passthrough { "yes" } else { "no" }.into()),
            ("terminal", "no".into()),
            ("msg-level", "all=error".into()),
            ("cache", "yes".into()),
        ];
        for (k, v) in &opts {
            m.option(k, v);
        }
        if !m.initialize() {
            return Err("mpv failed to initialize".into());
        }
        m.log_messages("error");
        for profile in ["gpu-hq", "high-quality"] {
            if m.command(&["apply-profile", profile]) {
                break;
            }
        }
        for (i, name) in OBSERVED.iter().enumerate() {
            m.observe(name, i as u64 + 1);
        }
        m.set_wakeup(on_wakeup, self.proxy as *mut c_void);
        let rc = m.create_render_context(get_proc, display)?;
        rc.set_update_callback(Some(on_frame), self.proxy as *mut c_void);
        self.mpv = Some(m);
        self.render = Some(rc);
        self.needs_render = true;
        Ok(())
    }

    pub fn started(&self) -> bool {
        self.mpv.is_some()
    }

    /// Stops playback and frees mpv. Resume positions are stored by the caller beforehand.
    pub fn shutdown(&mut self) {
        self.render = None; // the render context must go before the player
        self.mpv = None;
    }

    // ---- video output --------------------------------------------------------------------------------
    /// Called when mpv announced a frame: asks whether it is a new picture.
    pub fn frame_ready(&mut self) {
        if let Some(rc) = &self.render {
            if rc.update() & RENDER_UPDATE_FRAME != 0 {
                self.needs_render = true;
            }
        }
    }

    pub fn request_render(&mut self) {
        self.needs_render = true;
    }

    /// Draws the picture into `video` when there is something new. Returns true if it did.
    pub fn render(&mut self, video: &Target) -> bool {
        if !self.needs_render {
            return false;
        }
        let Some(rc) = &self.render else { return false };
        self.needs_render = false;
        rc.render(video.fbo.0.get(), video.w, video.h);
        true
    }

    // ---- events --------------------------------------------------------------------------------------
    pub fn poll(&mut self, settings: &mut Settings) -> Changes {
        let mut ch = Changes::default();
        loop {
            let Some(ev) = self.mpv.as_ref().and_then(|m| m.poll()) else { break };
            match ev {
                Event::Property { name, value } => self.property(&name, value, settings, &mut ch),
                Event::FileLoaded => self.file_loaded(settings, &mut ch),
                Event::EndFile { reason } if reason == END_REASON_ERROR => self.on_error(),
                Event::Log { prefix, level } => {
                    if prefix.starts_with("ao") && (level == "error" || level == "fatal") {
                        self.passthrough_fallback(settings);
                    }
                }
                Event::Shutdown => self.closing = true,
                Event::CommandReply { id } if id == SEEK_REPLY => {
                    self.seek_in_flight = false;
                    match self.seek_queued.take() {
                        Some((t, exact)) => self.seek(t, exact),
                        None => self.seek_target = None,
                    }
                }
                _ => {}
            }
        }
        ch
    }

    fn property(&mut self, name: &str, v: Value, settings: &mut Settings, ch: &mut Changes) {
        match name {
            "time-pos" => self.time_pos = v.as_f64().unwrap_or(0.0),
            "duration" => self.duration = v.as_f64().unwrap_or(0.0),
            "pause" => self.paused = v.as_bool().unwrap_or(true),
            "volume" => {
                if let Some(x) = v.as_f64() {
                    self.volume = x;
                    settings.volume = (from_mpv(x as f32) * 10.0).round() / 10.0;
                }
                ch.volume = true;
            }
            "mute" => {
                if let Some(m) = v.as_bool() {
                    self.muted = m;
                    settings.muted = m;
                }
                ch.volume = true;
            }
            "speed" => {
                self.speed = v.as_f64().unwrap_or(1.0);
                ch.speed = true;
            }
            "track-list" => {
                self.track_list = v.as_array().cloned().unwrap_or_default();
                ch.tracks = true;
                ch.media = true;
            }
            "aid" | "sid" => ch.tracks = true,
            "chapter-list" => {
                self.chapters = v.as_array().map(|a| a.iter().filter_map(|c| c.get("time").and_then(Value::as_f64)).collect()).unwrap_or_default();
                ch.tracks = true;
            }
            "metadata" => {
                self.metadata = v
                    .as_object()
                    .map(|m| m.iter().map(|(k, v)| (k.to_lowercase(), v.clone())).collect())
                    .unwrap_or_default();
                ch.media = true;
            }
            "media-title" => {
                self.media_title = v.as_str().unwrap_or("").to_string();
                ch.media = true;
            }
            "eof-reached" => {
                let reached = v.as_bool().unwrap_or(false);
                if reached && !self.eof {
                    self.eof = true;
                    self.remember_position(settings, true);
                    if !self.next(settings, ch) {
                        self.set("pause", "yes");
                    }
                } else {
                    self.eof = reached;
                }
            }
            "osd-dimensions" => self.osd = v,
            _ => {}
        }
    }

    fn file_loaded(&mut self, settings: &Settings, ch: &mut Changes) {
        self.loaded = true;
        if let Some(v) = self.mpv.as_ref().and_then(|m| m.get("track-list")) {
            self.track_list = v.as_array().cloned().unwrap_or_default();
        }
        self.load_cover(settings);
        ch.media = true;
        ch.tracks = true;
        ch.cover = true;
    }

    fn on_error(&mut self) {
        let path = self.playlist.current().unwrap_or("").to_string();
        let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(path.clone());
        if !path.is_empty() && Path::new(&path).is_file() && looks_protected(Path::new(&path)) {
            self.toasts.push_back(format!("“{name}” is copy-protected (iTunes DRM) and can’t be played"));
        } else {
            self.toasts.push_back(format!("Can’t play “{name}”"));
        }
    }

    // ---- cover art -----------------------------------------------------------------------------------
    fn load_cover(&mut self, settings: &Settings) {
        self.art_wanted = false;
        let Some(path) = self.playlist.current().map(str::to_string) else {
            self.set_cover(None);
            return;
        };
        if self.has_video() || path.contains("://") {
            self.set_cover(None);
            return;
        }
        let tags = media::read_tags(Path::new(&path));
        let mut image = tags.cover.as_deref().and_then(decode_cover);
        if image.is_none() {
            if let Some(cached) = crate::art::cached(&self.art_key()) {
                image = std::fs::read(cached).ok().and_then(|b| decode_cover(&b));
            }
        }
        let missing = image.is_none();
        self.set_cover(image);
        self.art_wanted = missing && settings.generate_artwork;
    }

    pub fn set_cover(&mut self, image: Option<Cover>) {
        self.cover = image;
        self.cover_rev += 1;
    }

    // ---- derived properties --------------------------------------------------------------------------
    pub fn has_media(&self) -> bool {
        self.playlist.current().is_some()
    }

    pub fn has_video(&self) -> bool {
        self.track_list.iter().any(|t| t["type"] == "video" && !t["albumart"].as_bool().unwrap_or(false))
    }

    pub fn is_audio(&self) -> bool {
        self.loaded && !self.has_video()
    }

    fn meta(&self, keys: &[&str]) -> String {
        keys.iter()
            .find_map(|k| self.metadata.get(*k).and_then(Value::as_str).filter(|s| !s.is_empty()))
            .unwrap_or("")
            .to_string()
    }

    pub fn title(&self) -> String {
        let Some(path) = self.playlist.current() else { return String::new() };
        let stem = || Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if self.has_video() {
            if self.media_title.is_empty() {
                stem()
            } else {
                self.media_title.clone()
            }
        } else {
            let t = self.meta(&["title"]);
            if t.is_empty() {
                stem()
            } else {
                t
            }
        }
    }

    pub fn artist(&self) -> String {
        self.meta(&["artist", "album_artist", "albumartist", "composer"])
    }

    pub fn album(&self) -> String {
        self.meta(&["album"])
    }

    pub fn art_key(&self) -> String {
        format!("{}|{}|{}", self.artist(), self.album(), self.title())
    }

    pub fn art_seed(&self) -> u32 {
        crate::art::crc32(self.title().as_bytes()) & 0x7FFF_FFFF
    }

    pub fn position(&self) -> f64 {
        self.seek_target.unwrap_or(self.time_pos)
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Percent, 0..150.
    pub fn volume(&self) -> f32 {
        from_mpv(self.volume as f32)
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    pub fn speed(&self) -> f32 {
        self.speed as f32
    }

    pub fn chapters(&self) -> &[f64] {
        &self.chapters
    }

    /// Where mpv draws the picture inside the window, normalized 0..1 (letterbox bars excluded): x, y, w, h.
    pub fn video_rect(&self) -> [f32; 4] {
        let g = |k: &str| self.osd.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let (w, h) = (g("w"), g("h"));
        if w <= 0.0 || h <= 0.0 {
            return [0.0, 0.0, 1.0, 1.0];
        }
        let (ml, mr, mt, mb) = (g("ml"), g("mr"), g("mt"), g("mb"));
        [(ml / w) as f32, (mt / h) as f32, ((w - ml - mr) / w) as f32, ((h - mt - mb) / h) as f32]
    }

    fn tracks(&self, kind: &str) -> Vec<Track> {
        self.track_list
            .iter()
            .filter(|t| t["type"] == kind && !t["albumart"].as_bool().unwrap_or(false))
            .map(|t| {
                let s = |k: &str| t[k].as_str().unwrap_or("").to_string();
                let parts = [s("title"), s("lang").to_uppercase(), s("codec").to_uppercase()];
                let label = parts.iter().filter(|p| !p.is_empty()).cloned().collect::<Vec<_>>().join(" · ");
                let id = t["id"].as_i64().unwrap_or(0);
                Track { id, label: if label.is_empty() { format!("Track {id}") } else { label }, selected: t["selected"].as_bool().unwrap_or(false) }
            })
            .collect()
    }

    pub fn audio_tracks(&self) -> Vec<Track> {
        self.tracks("audio")
    }

    pub fn subtitle_tracks(&self) -> Vec<Track> {
        self.tracks("sub")
    }

    // ---- mpv access ----------------------------------------------------------------------------------
    fn set(&self, prop: &str, value: &str) {
        if let Some(m) = &self.mpv {
            m.set(prop, value);
        }
    }

    fn command(&self, args: &[&str]) {
        if let (Some(m), true) = (&self.mpv, self.playlist.current().is_some()) {
            m.command(args);
        }
    }

    // ---- opening and moving through files ------------------------------------------------------------
    pub fn open_paths(&mut self, paths: &[String], settings: &mut Settings) {
        let local: Vec<String> = paths.iter().map(|p| file_url_to_path(p)).filter(|p| !p.is_empty()).collect();
        if local.is_empty() {
            return;
        }
        let playlist = Playlist::for_paths(&local);
        if playlist.current().is_none() {
            self.toasts.push_back("Nothing playable there".into());
            return;
        }
        self.remember_position(settings, false);
        self.playlist = playlist;
        let first = Path::new(&local[0]);
        if first.exists() {
            let dir = if first.is_dir() { first } else { first.parent().unwrap_or(first) };
            settings.last_folder = dir.to_string_lossy().into_owned();
        }
        if let Some(p) = self.playlist.current().map(str::to_string) {
            self.load(&p, settings);
        }
    }

    fn load(&mut self, path: &str, settings: &Settings) {
        if self.mpv.is_none() {
            return;
        }
        self.loaded = false;
        self.seek_queued = None;
        self.seek_target = None;
        self.time_pos = 0.0;
        self.duration = 0.0;
        self.track_list.clear();
        self.metadata.clear();
        self.media_title.clear();
        self.chapters.clear();
        self.eof = false;
        self.set_cover(None);
        let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("m4p")) {
            self.toasts.push_back(format!("“{name}” is copy-protected (iTunes DRM) and can’t be played"));
            return;
        }
        let start = settings.resume_playback.then(|| settings.resume_positions.get(path).copied()).flatten();
        let ok = match start {
            Some(s) => self.mpv.as_ref().is_some_and(|m| m.command(&["loadfile", path, "replace", "-1", &format!("start={s}")])),
            None => self.mpv.as_ref().is_some_and(|m| m.command(&["loadfile", path, "replace"])),
        };
        if ok {
            self.set("pause", "no");
        } else {
            self.toasts.push_back(format!("Can’t open “{name}”"));
        }
    }

    pub fn remember_position(&mut self, settings: &mut Settings, finished: bool) {
        let Some(path) = self.playlist.current().map(str::to_string) else { return };
        if path.contains("://") {
            return;
        }
        settings.resume_positions.shift_remove(&path);
        let (pos, dur) = (self.time_pos, self.duration);
        if !finished && pos > RESUME_MIN && dur - pos > RESUME_MIN && dur > 60.0 {
            settings.resume_positions.insert(path, (pos * 10.0).round() / 10.0); // re-inserted last: the most recent survive trimming
        }
    }

    fn next(&mut self, settings: &Settings, _ch: &mut Changes) -> bool {
        match self.playlist.next().map(str::to_string) {
            Some(p) => {
                self.load(&p, settings);
                true
            }
            None => false,
        }
    }

    pub fn next_track(&mut self, settings: &mut Settings) -> bool {
        self.remember_position(settings, false);
        let mut ch = Changes::default();
        self.next(settings, &mut ch)
    }

    pub fn previous_track(&mut self, settings: &mut Settings) {
        self.remember_position(settings, false);
        let pos = self.time_pos;
        match self.playlist.previous(pos).map(str::to_string) {
            Some(p) => self.load(&p, settings),
            None => self.seek(0.0, true),
        }
    }

    pub fn toggle_pause(&mut self) {
        if self.mpv.is_none() || self.playlist.current().is_none() {
            return;
        }
        if self.eof {
            self.seek(0.0, true);
        }
        self.set("pause", if self.paused { "no" } else { "yes" });
    }

    /// Seeks without blocking the UI. While one seek is still running, only the latest request is kept
    /// and sent when it finishes, so scrubbing never piles up work in mpv.
    pub fn seek(&mut self, seconds: f64, exact: bool) {
        if self.mpv.is_none() || self.playlist.current().is_none() {
            return;
        }
        self.seek_target = Some(seconds.max(0.0));
        if self.seek_in_flight {
            self.seek_queued = Some((seconds, exact));
            return;
        }
        let target = format!("{}", seconds.max(0.0));
        let mode = if exact { "absolute+exact" } else { "absolute+keyframes" };
        if let Some(m) = &self.mpv {
            self.seek_in_flight = m.command_async(SEEK_REPLY, &["seek", &target, mode]);
        }
    }

    pub fn seek_relative(&self, seconds: f64) {
        self.command(&["seek", &format!("{seconds}"), "relative+exact"]);
    }

    pub fn set_volume(&mut self, percent: f32) {
        self.volume_target = percent.clamp(0.0, MAX_PERCENT);
        self.set("volume", &format!("{}", to_mpv(self.volume_target)));
        if self.volume_target > 0.0 && self.muted {
            self.set("mute", "no");
        }
    }

    /// Keys and wheel; returns the new percentage for the toast.
    pub fn nudge_volume(&mut self, delta: f32) -> f32 {
        self.set_volume(self.volume_target + delta);
        self.volume_target
    }

    pub fn toggle_mute(&mut self) {
        self.set("mute", if self.muted { "no" } else { "yes" });
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.speed_target = speed.clamp(MIN_SPEED, MAX_SPEED);
        self.set("speed", &format!("{}", self.speed_target));
    }

    /// Rapid presses step from what was asked, not from mpv's lagging echo.
    pub fn step_speed(&mut self, direction: i32) {
        self.set_speed(step_speed(self.speed_target, direction));
    }

    pub fn select_track(&self, kind: &str, id: i64) {
        let value = if id > 0 { id.to_string() } else { "no".to_string() };
        self.set(if kind == "audio" { "aid" } else { "sid" }, &value);
    }

    pub fn cycle_track(&self, kind: &str) {
        self.command(&["cycle", if kind == "audio" { "aid" } else { "sid" }]);
    }

    pub fn add_subtitle(&self, path: &str) {
        self.command(&["sub-add", &file_url_to_path(path), "select"]);
    }

    /// Live effect of a settings change made in the settings popover.
    pub fn apply_setting(&mut self, name: &str, value: bool) {
        match name {
            "audio_passthrough" => {
                self.passthrough_rejected = false;
                self.set("audio-spdif", if value { PASSTHROUGH } else { "" });
                self.set("audio-exclusive", if value { "yes" } else { "no" });
                self.command(&["ao-reload"]);
            }
            "hwdec" => {
                let on = std::env::var("REYER_HWDEC").unwrap_or_else(|_| "auto-safe".into());
                self.set("hwdec", if value { &on } else { "no" });
            }
            "generate_artwork" => self.art_wanted = value && self.is_audio() && self.cover.is_none(),
            _ => {}
        }
    }

    /// mpv does not fall back by itself: a refused bitstream leaves no audio output and playback stalls.
    /// Decode instead (until the setting is switched again, e.g. after connecting a receiver).
    fn passthrough_fallback(&mut self, settings: &Settings) {
        if !settings.audio_passthrough || self.passthrough_rejected {
            return;
        }
        self.passthrough_rejected = true;
        self.set("audio-spdif", "");
        self.set("audio-exclusive", "no");
        self.command(&["ao-reload"]);
        self.toasts.push_back("This audio output can't take Dolby/DTS directly – decoding it instead".into());
    }

    // ---- file information ----------------------------------------------------------------------------
    /// What mpv knows about the current file, only the fields that are known.
    pub fn media_info(&self) -> Vec<Section> {
        let (Some(m), Some(path)) = (&self.mpv, self.playlist.current()) else { return vec![] };
        let get = |p: &str| m.get(p).filter(|v| !v.is_null());
        let text = |v: Option<Value>| v.and_then(|v| v.as_str().map(str::to_string).or_else(|| (!v.is_null()).then(|| v.to_string())));
        let rate = |bits: Option<f64>| match bits {
            Some(b) if b >= 1e6 => Some(format!("{:.1} Mbit/s", b / 1e6)),
            Some(b) if b > 0.0 => Some(format!("{:.0} kbit/s", b / 1e3)),
            _ => None,
        };
        let mut out = Vec::new();
        let mut section = |title: &'static str, rows: Vec<(&str, Option<String>)>| {
            let rows: Vec<(String, String)> =
                rows.into_iter().filter_map(|(k, v)| v.filter(|v| !v.is_empty() && v != "0").map(|v| (k.to_string(), v))).collect();
            if !rows.is_empty() {
                out.push(Section { title, rows });
            }
        };
        let p = Path::new(path);
        let size = get("file-size").and_then(|v| v.as_f64());
        section(
            "File",
            vec![
                ("Name", p.file_name().map(|n| n.to_string_lossy().into_owned())),
                ("Folder", p.parent().filter(|d| !d.as_os_str().is_empty()).map(|d| d.to_string_lossy().into_owned())),
                ("Size", size.map(|s| format!("{:.1} MB", s / 1048576.0))),
                ("Container", text(get("file-format")).map(|f| f.split(',').next().unwrap_or("").to_uppercase())),
                ("Duration", clock(self.duration)),
                ("Chapters", (!self.chapters.is_empty()).then(|| self.chapters.len().to_string())),
            ],
        );
        let vp = get("video-params").unwrap_or(Value::Null);
        if self.has_video() {
            let gamma = vp["gamma"].as_str().unwrap_or("").to_string();
            let fps = get("container-fps").and_then(|v| v.as_f64()).filter(|f| *f > 0.0);
            let hw = text(get("hwdec-current")).filter(|h| h != "no").unwrap_or_else(|| "Off".into());
            section(
                "Video",
                vec![
                    ("Codec", text(get("video-codec"))),
                    ("Resolution", vp["w"].as_i64().map(|w| format!("{} × {}", w, vp["h"].as_i64().unwrap_or(0)))),
                    ("Frame rate", fps.map(|f| format!("{} fps", format!("{f:.3}").trim_end_matches('0').trim_end_matches('.')))),
                    ("Bitrate", rate(get("video-bitrate").and_then(|v| v.as_f64()))),
                    ("Pixel format", vp["hw-pixelformat"].as_str().or(vp["pixelformat"].as_str()).map(str::to_string)),
                    (
                        "Color",
                        Some(
                            [vp["colormatrix"].as_str(), vp["primaries"].as_str(), Some(gamma.as_str())]
                                .iter()
                                .flatten()
                                .filter(|s| !s.is_empty())
                                .copied()
                                .collect::<Vec<_>>()
                                .join(" / "),
                        ),
                    ),
                    ("HDR", (gamma == "pq" || gamma == "hlg").then(|| "Yes (tone-mapped to SDR)".to_string())),
                    ("HW decoding", Some(hw)),
                ],
            );
        }
        let ap = get("audio-params").unwrap_or(Value::Null);
        section(
            "Audio",
            vec![
                ("Codec", text(get("audio-codec"))),
                ("Sample rate", ap["samplerate"].as_f64().map(|s| format!("{} kHz", s / 1000.0))),
                ("Channels", ap["hr-channels"].as_str().map(str::to_string).or_else(|| ap["channel-count"].as_i64().map(|c| c.to_string()))),
                ("Bitrate", rate(get("audio-bitrate").and_then(|v| v.as_f64()))),
            ],
        );
        let mut tags: Vec<(String, String)> = self
            .metadata
            .iter()
            .filter_map(|(k, v)| v.as_str().filter(|s| !s.is_empty()).map(|s| (title_case(&k.replace('_', " ")), s.to_string())))
            .collect();
        tags.sort();
        section("Tags", tags.iter().map(|(k, v)| (k.as_str(), Some(v.clone()))).collect());
        let join = |t: Vec<Track>| (!t.is_empty()).then(|| t.iter().map(|t| t.label.clone()).collect::<Vec<_>>().join(", "));
        section("Tracks", vec![("Audio", join(self.audio_tracks())), ("Subtitles", join(self.subtitle_tracks()).or(Some("None in this file".into())))]);
        out
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn clock(seconds: f64) -> Option<String> {
    if seconds <= 0.0 {
        return None;
    }
    let s = seconds as u64;
    Some(if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) })
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `file:///C:/x%20y.mp4` -> `C:/x y.mp4`; plain paths and URLs pass through.
pub fn file_url_to_path(s: &str) -> String {
    let s = s.trim().trim_matches('"');
    let Some(rest) = s.strip_prefix("file:///").or_else(|| s.strip_prefix("file://")) else { return s.to_string() };
    let mut out = Vec::new();
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Decodes embedded art, shrinks it to at most 640 px and extracts a tint from it.
pub fn decode_cover(bytes: &[u8]) -> Option<Cover> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width().max(img.height()) > 640 { img.resize(640, 640, image::imageops::FilterType::Triangle) } else { img };
    let rgba = img.to_rgba8();
    let color = dominant_color(&rgba);
    let (w, h) = rgba.dimensions();
    Some(Cover { rgba: rgba.into_raw(), w, h, color })
}

/// Average of the most saturated quarter of an 8x8 thumbnail: a usable tint, never muddy gray.
pub fn dominant_color(img: &image::RgbaImage) -> [f32; 3] {
    let small = image::imageops::resize(img, 8, 8, image::imageops::FilterType::Triangle);
    let mut px: Vec<[f32; 3]> = small.pixels().map(|p| [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]).collect();
    let sat = |c: &[f32; 3]| {
        let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
        if mx <= 0.0 {
            0.0
        } else {
            (mx - mn) / mx
        }
    };
    px.sort_by(|a, b| sat(b).partial_cmp(&sat(a)).unwrap_or(std::cmp::Ordering::Equal));
    let top = &px[..16.min(px.len())];
    let n = top.len().max(1) as f32;
    [top.iter().map(|c| c[0]).sum::<f32>() / n, top.iter().map(|c| c[1]).sum::<f32>() / n, top.iter().map(|c| c[2]).sum::<f32>() / n]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_become_paths() {
        assert_eq!(file_url_to_path("file:///C:/Music/a%20b.mp3"), "C:/Music/a b.mp3");
        assert_eq!(file_url_to_path("\"D:\\x.mp4\""), "D:\\x.mp4");
        assert_eq!(file_url_to_path("https://a/b.m3u8"), "https://a/b.m3u8");
    }

    #[test]
    fn clock_formats() {
        assert_eq!(clock(65.0).as_deref(), Some("1:05"));
        assert_eq!(clock(3725.0).as_deref(), Some("1:02:05"));
        assert_eq!(clock(0.0), None);
    }

    #[test]
    fn dominant_color_prefers_saturated_pixels() {
        let mut img = image::RgbaImage::from_pixel(64, 64, image::Rgba([128, 128, 128, 255]));
        for x in 0..32 {
            for y in 0..64 {
                img.put_pixel(x, y, image::Rgba([220, 20, 20, 255]));
            }
        }
        let c = dominant_color(&img);
        assert!(c[0] > 0.7 && c[1] < 0.3);
    }
}
