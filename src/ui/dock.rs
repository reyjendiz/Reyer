//! The transport dock: five capsules on one glass container (one shader pass).
//! [Transport] [Volume] [Seek] [Speed] [Tools]; narrow windows first fold the tools into "…",
//! then stop the volume slider from unfolding. Every width change is a spring, so folding morphs.
use super::theme::Role;
use super::{id, BtnStyle, GlassSpec, SliderOpts, Ui};
use crate::gfx::{Icon, Rect, Weight};
use crate::gfx::icons::{play_pause_state, speaker_state};
use crate::spring::{Ease, Preset, Spring, Tween};
use crate::speed::format_speed;
use crate::tokens::tokens;
use std::time::{Duration, Instant};

pub const PREV: &str = "dock/prev";
pub const PLAY: &str = "dock/play";
pub const NEXT: &str = "dock/next";
pub const MUTE: &str = "dock/mute";
pub const VOLUME: &str = "dock/volume";
pub const SEEK: &str = "dock/seek";
pub const SLOWER: &str = "dock/slower";
pub const SPEED: &str = "dock/speed";
pub const FASTER: &str = "dock/faster";
pub const AUDIO: &str = "dock/audio";
pub const SUBTITLES: &str = "dock/subtitles";
pub const SETTINGS: &str = "dock/settings";
pub const OPEN: &str = "dock/open";
pub const FULLSCREEN: &str = "dock/fullscreen";
pub const MORE: &str = "dock/more";

#[derive(Debug, Clone, PartialEq)]
pub enum DockAction {
    Previous,
    Next,
    TogglePause,
    ToggleMute,
    SetVolume(f32),
    Seek(f64, bool),
    StepSpeed(i32),
    ResetSpeed,
    SpeedPresets,
    Popover(&'static str),
    Open,
    Fullscreen,
}

/// What the dock needs to know about the player and the window each frame.
pub struct DockInput {
    pub window: (f32, f32),
    pub chrome_t: f32,
    pub rise: f32,
    pub dim: f32,
    pub unified: bool,
    pub has_media: bool,
    pub paused: bool,
    pub muted: bool,
    pub volume: f32,
    pub volume_max: f32,
    pub position: f64,
    pub duration: f64,
    pub chapters: Vec<f32>,
    pub speed: f32,
    pub has_audio_tracks: bool,
    pub subtitle_active: bool,
    pub fullscreen: bool,
}

pub struct DockState {
    tools_w: Spring,
    volume_w: Spring,
    speed_nudge: Spring,
    speed_pulse: Spring,
    breathe: Spring,
    merge: Spring,
    glow: [Spring; 5],
    roll: Spring,
    roll_dir: f32,
    prev_speed: String,
    last_speed: f32,
    bubble_x: Spring,
    bubble_show: Spring,
    tools_opacity: Tween,
    play_t: Spring,
    waves: [Tween; 3],
    volume_open: bool,
    volume_idle: Option<Instant>,
    volume_hover: bool,
    shimmer_at: Option<Instant>,
    pulse_at: Option<Instant>,
    pub rect: Rect,
    pub tools_collapsed: bool,
    /// Something in the dock is being used: the chrome must stay visible.
    pub busy: bool,
    pub focus_within: bool,
}

impl DockState {
    pub fn new() -> DockState {
        DockState {
            tools_w: Spring::new(Preset::Smooth),
            volume_w: Spring::new(Preset::Smooth),
            speed_nudge: Spring::at(Preset::Snappy, 0.0),
            speed_pulse: Spring::at(Preset::Snappy, 0.0),
            breathe: Spring::new(Preset::Bouncy),
            merge: Spring::new(Preset::Smooth),
            glow: [Spring::new(Preset::Smooth); 5],
            roll: Spring::new(Preset::Snappy),
            roll_dir: 1.0,
            prev_speed: format_speed(1.0),
            last_speed: 1.0,
            bubble_x: Spring::new(Preset::Follow),
            bubble_show: Spring::new(Preset::Snappy),
            tools_opacity: Tween::new(1.0),
            play_t: Spring::new(Preset::Snappy),
            waves: [Tween::new(0.0); 3],
            volume_open: false,
            volume_idle: None,
            volume_hover: false,
            shimmer_at: None,
            pulse_at: None,
            rect: Rect::ZERO,
            tools_collapsed: false,
            busy: false,
            focus_within: false,
        }
    }

    /// The dock breathes once when something is dropped onto the window.
    pub fn breathe_once(&mut self) {
        self.breathe.nudge(0.9);
    }

    pub fn poke_volume(&mut self, compact: bool, now: Instant) {
        if compact {
            return;
        }
        self.volume_open = true;
        self.volume_idle = Some(now + Duration::from_millis(tokens().timings.volume_collapse as u64));
    }

    /// Called whenever the player's speed changed (a toast and the number rolls).
    pub fn speed_changed(&mut self, speed: f32) {
        let d = if speed > self.last_speed { 1.0 } else { -1.0 };
        self.prev_speed = format_speed(self.last_speed);
        self.roll_dir = d;
        self.roll.snap(0.0);
        self.roll.set_target(1.0, false);
        if (speed - 1.0).abs() < 1e-6 {
            self.speed_pulse.nudge(40.0);
        } else {
            self.speed_nudge.nudge(d * 160.0);
        }
        self.last_speed = speed;
    }

    fn press(&mut self, ui: &Ui) {
        if !ui.th.reduce_motion {
            self.shimmer_at = Some(ui.now);
        }
    }

    pub fn draw(&mut self, ui: &mut Ui, inp: &DockInput) -> Vec<DockAction> {
        let t = tokens();
        let mut acts = Vec::new();
        let height = t.sizes.dock_height;
        let width = t.sizes.dock_max_width.min(inp.window.0 - 2.0 * t.spacing.l);
        let gap = t.spacing.s;
        let pad = (height - 40.0) / 2.0; // concentric: inner radius = outer radius - pad
        let transport_w = 2.0 * pad + 36.0 + 44.0 + 36.0 + 4.0;
        let speed_w = 2.0 * pad + 32.0 + 58.0 + 32.0;
        let tools_full = 2.0 * pad + 5.0 * 36.0;
        let tools_folded = height;
        let volume_slider = 104.0;
        let seek_min = 190.0;
        let fixed_w = transport_w + height + speed_w + 4.0 * gap;
        self.tools_collapsed = width - fixed_w - tools_full < seek_min;
        let volume_compact = width - fixed_w - if self.tools_collapsed { tools_folded } else { tools_full } < seek_min + volume_slider;
        let now = ui.now;

        // the volume slider collapses a moment after the last touch
        if let Some(d) = self.volume_idle {
            if now >= d {
                self.volume_idle = None;
                self.volume_open = self.volume_hover;
                if self.volume_hover {
                    self.volume_idle = Some(now + Duration::from_millis(t.timings.volume_collapse as u64));
                }
            } else {
                ui.wake_at(d);
            }
        }
        let tools_target = if self.tools_collapsed { tools_folded } else { tools_full };
        let volume_target = height + if self.volume_open && !volume_compact { volume_slider } else { 0.0 };
        let (dt, rm) = (ui.dt, ui.th.reduce_motion);
        let mut anim = false;
        let tools_w = self.tools_w.to(tools_target, dt, rm, &mut anim);
        let volume_w = self.volume_w.to(volume_target, dt, rm, &mut anim);
        let speed_nudge = self.speed_nudge.to(0.0, dt, rm, &mut anim);
        let speed_pulse = self.speed_pulse.to(0.0, dt, rm, &mut anim);
        let breathe = self.breathe.to(1.0, dt, rm, &mut anim);
        let merge = self.merge.to(if inp.unified { 1.0 } else { 0.0 }, dt, rm, &mut anim);
        let roll = self.roll.to(1.0, dt, rm, &mut anim);
        ui.animating |= anim;

        let seek_w = (width - transport_w - volume_w - speed_w - tools_w - 4.0 * gap).max(80.0);
        let compact_seek = seek_w < 230.0; // too tight for the time labels: the bar gets the room
        let x_volume = transport_w + gap;
        let x_seek = x_volume + volume_w + gap;
        let x_speed = x_seek + seek_w + gap;
        let x_tools = x_speed + speed_w + gap;
        let ox = (inp.window.0 - width) / 2.0;
        let oy = inp.window.1 - height - t.sizes.dock_bottom_margin + (1.0 - inp.chrome_t) * 14.0 + (1.0 - inp.rise) * 12.0;
        let dock = Rect::new(ox, oy, width, height);
        self.rect = dock;
        // the dock fades with the rest of the chrome (it used to only slide 8 px and stay visible)
        ui.r.push((ox + width / 2.0, oy + height), breathe, (0.0, 0.0), inp.chrome_t.clamp(0.0, 1.0));

        // hover and focus per group, for the glass glow
        let groups = [
            Rect::new(ox, oy, transport_w, height),
            Rect::new(ox + x_volume, oy, volume_w, height),
            Rect::new(ox + x_seek, oy, seek_w, height),
            Rect::new(ox + x_speed, oy, speed_w, height),
            Rect::new(ox + x_tools, oy, tools_w, height),
        ];
        let in_group: [&[&str]; 5] = [&[PREV, PLAY, NEXT], &[MUTE, VOLUME], &[SEEK], &[SLOWER, SPEED, FASTER], &[AUDIO, SUBTITLES, SETTINGS, OPEN, FULLSCREEN, MORE]];
        let mut glows = [0.0f32; 5];
        let mut any_focus = false;
        for i in 0..5 {
            let focused = ui.st.nav && in_group[i].iter().any(|n| ui.has_focus(id(n)));
            any_focus |= in_group[i].iter().any(|n| ui.has_focus(id(n)));
            let on = ui.pointer_in(groups[i]) || focused;
            glows[i] = self.glow[i].to(if on { 1.0 } else { 0.0 }, dt, rm, &mut ui.animating);
        }
        self.focus_within = any_focus;
        self.volume_hover = ui.pointer_in(groups[1]) && volume_w > height + 4.0 || ui.pointer_in(groups[1]);
        let was_hover = self.volume_hover;
        if was_hover && !self.volume_open {
            self.poke_volume(volume_compact, now);
        } else if !was_hover && self.volume_open && self.volume_idle.is_none() {
            self.volume_idle = Some(now + Duration::from_millis(t.timings.volume_collapse as u64));
        }
        let hovering_dock = ui.pointer_in(dock);
        if hovering_dock {
            ui.over_chrome = true;
        }

        // shimmer sweeping across the glass when any control is pressed
        let shimmer = match self.shimmer_at {
            Some(s) => {
                let k = now.duration_since(s).as_secs_f32() / 0.42;
                if k >= 1.0 {
                    self.shimmer_at = None;
                    -1.0
                } else {
                    ui.animating = true;
                    (1.0 - (1.0 - k).powi(3)) * 1.1
                }
            }
            None => -1.0,
        };

        // ---- glass: the transport capsule grows into one pill while the others sink into it ----------
        let sunk = |r: Rect| -> Rect {
            if merge > 0.98 {
                return Rect::ZERO;
            }
            let inset = merge * r.h / 2.0;
            Rect::new(r.x, r.y + inset, r.w, r.h - 2.0 * inset)
        };
        let local = |r: Rect| r.offset(ox, oy);
        let shapes = vec![
            (local(Rect::new(0.0, 0.0, transport_w, height).lerp(Rect::new(0.0, 0.0, width, height), merge)), height / 2.0, 0.0),
            (local(sunk(Rect::new(x_volume, 0.0, volume_w, height))), height / 2.0, 0.0),
            (local(sunk(Rect::new(x_seek, 0.0, seek_w, height))), height / 2.0, 0.0),
            (
                local(sunk(Rect::new(x_speed + speed_nudge - speed_pulse, -speed_pulse, speed_w + 2.0 * speed_pulse, height + 2.0 * speed_pulse))),
                height / 2.0 + speed_pulse,
                0.0,
            ),
            (local(sunk(Rect::new(x_tools, 0.0, tools_w, height))), height / 2.0, 0.0),
        ];
        let mut shapes = shapes;
        if inp.unified {
            let g = glows.iter().cloned().fold(0.0, f32::max) * 0.5;
            for s in shapes.iter_mut() {
                s.2 = g;
            }
        } else {
            for (s, g) in shapes.iter_mut().zip(glows) {
                s.2 = g;
            }
        }
        ui.glass(GlassSpec { role: Role::Dock, shapes, dim: inp.dim, shimmer, overlap: 0.0 });

        // hairline dividers between the groups inside the single pill
        if merge > 0.01 {
            let mut c = ui.th.cv.border.0;
            c[3] *= merge;
            for x in [x_volume, x_seek, x_speed, x_tools] {
                ui.r.rrect(Rect::new(ox + x - gap / 2.0, oy + height * 0.28, 1.0, height * 0.44), 0.0, c);
            }
        }

        let tc = ui.th.cv.text_secondary.0;
        let idn = |s: &str| id(s);

        // ---- transport -----------------------------------------------------------------------------------
        let prev_r = Rect::new(ox + pad, oy + pad, 36.0, 40.0);
        let play_r = Rect::new(ox + pad + 38.0, oy + pad, 44.0, 40.0);
        let next_r = Rect::new(ox + pad + 84.0, oy + pad, 36.0, 40.0);
        if ui.icon_button(idn(PREV), prev_r, "previous", 18.0, BtnStyle { label: "Previous", ..BtnStyle::default() }).clicked {
            self.press(ui);
            acts.push(DockAction::Previous);
        }
        let playing = inp.has_media && !inp.paused;
        let play_label = if inp.paused { "Play" } else { "Pause" };
        let pp = self.play_t.to(if playing { 1.0 } else { 0.0 }, dt, rm, &mut ui.animating);
        let pulse_at = self.pulse_at;
        let play = ui.button(idn(PLAY), play_r, BtnStyle { label: play_label, ..BtnStyle::default() }, |ui, r, c| {
            ui.r.icon(Icon::PlayPause(play_pause_state(pp)), r.center(), 26.0, c);
            if let Some(p) = pulse_at {
                let k = (ui.now.duration_since(p).as_secs_f32() / 0.38).min(1.0);
                if k < 1.0 {
                    let e = 1.0 - (1.0 - k).powi(3);
                    let s = 0.9 + (1.45 - 0.9) * e;
                    let a = (if ui.th.reduce_motion { 0.0 } else { 0.35 }) * (1.0 - e);
                    let (cx, cy) = r.center();
                    ui.r.rrect_stroke(Rect::new(cx - 17.0 * s, cy - 17.0 * s, 34.0 * s, 34.0 * s), 17.0 * s, 1.5, [1.0, 1.0, 1.0, a]);
                    ui.animating = true;
                }
            }
        });
        if play.clicked {
            self.press(ui);
            self.pulse_at = Some(now);
            acts.push(DockAction::TogglePause);
        }
        if ui.icon_button(idn(NEXT), next_r, "next", 18.0, BtnStyle { label: "Next", ..BtnStyle::default() }).clicked {
            self.press(ui);
            acts.push(DockAction::Next);
        }

        // ---- volume: speaker + slider unfolding out of the capsule ----------------------------------------
        let vg = groups[1];
        ui.r.push_clip(vg);
        let mute_r = Rect::new(vg.x + pad, oy + pad, 40.0, 40.0);
        let level = inp.volume / 100.0;
        let mute_label = if inp.muted { "Unmute" } else { "Mute" };
        let muted = inp.muted || inp.volume <= 0.0;
        let wave_targets = [if !muted && level > 0.0 { 1.0 } else { 0.0 }, if !muted && level > 0.5 { 1.0 } else { 0.0 }, if muted { 1.0 } else { 0.0 }];
        let fade = ui.th.fade(140.0);
        let mut waves = [0.0f32; 3];
        for i in 0..3 {
            // the waves fade out one by one when muting, the slash comes in last
            waves[i] = self.waves[i].to(wave_targets[i], fade, Ease::OutCubic, dt, &mut ui.animating);
        }
        let mute = ui.button(idn(MUTE), mute_r, BtnStyle { label: mute_label, ..BtnStyle::default() }, |ui, r, c| {
            ui.r.icon(Icon::Speaker(speaker_state(waves[0]), speaker_state(waves[1]), speaker_state(waves[2])), r.center(), 20.0, c);
        });
        if mute.clicked {
            self.press(ui);
            acts.push(DockAction::ToggleMute);
        }
        let vol_alpha = ((volume_w - height) / volume_slider).clamp(0.0, 1.0);
        if vol_alpha > 0.02 {
            ui.r.push_alpha(vol_alpha);
            let rect = Rect::new(ox + x_volume + height, oy + (height - 28.0) / 2.0, volume_slider - 16.0, 28.0);
            let value = if inp.muted { 0.0 } else { inp.volume / inp.volume_max };
            let s = ui.slider(idn(VOLUME), rect, SliderOpts { value, ticks: &[], step: 0.02, big_step: 0.1, enabled: true });
            if let Some(v) = s.moved {
                acts.push(DockAction::SetVolume(v * inp.volume_max));
                self.poke_volume(volume_compact, now);
            }
            if s.dragging || ui.has_focus(idn(VOLUME)) {
                self.poke_volume(volume_compact, now);
            }
            ui.r.pop();
        }
        ui.r.pop_clip();

        // ---- seek: elapsed, bar, total --------------------------------------------------------------------
        let sg = groups[2];
        let seek_id = idn(SEEK);
        let dragging_seek = ui.st.active == seek_id;
        let drag_v = ui.st.widget(seek_id).drag;
        let elapsed = crate::ui::format_time(if dragging_seek { drag_v as f64 * inp.duration } else { inp.position });
        let total = crate::ui::format_time(inp.duration);
        let label_h = ui.r.line_height(12.0);
        if !compact_seek {
            ui.r.text(&elapsed, sg.x + pad + 8.0, oy + (height - label_h) / 2.0, 12.0, Weight::Medium, tc);
            let tw = ui.r.text_width(&total, 12.0, Weight::Medium);
            ui.r.text(&total, sg.x + sg.w - pad - 8.0 - tw, oy + (height - label_h) / 2.0, 12.0, Weight::Medium, tc);
        }
        let sx = sg.x + pad + 8.0 + if compact_seek { 0.0 } else { 46.0 };
        let sw = sg.w - 2.0 * (pad + 8.0) - if compact_seek { 0.0 } else { 92.0 };
        let seek_rect = Rect::new(sx, oy + (height - 28.0) / 2.0, sw.max(10.0), 28.0);
        let d = inp.duration;
        let s = ui.slider(
            seek_id,
            seek_rect,
            SliderOpts {
                value: if d > 0.0 { (inp.position / d) as f32 } else { 0.0 },
                ticks: &inp.chapters,
                step: if d > 0.0 { (5.0 / d) as f32 } else { 0.02 },
                big_step: if d > 0.0 { (30.0 / d) as f32 } else { 0.1 },
                enabled: d > 0.0,
            },
        );
        if let (Some(v), true) = (s.moved, d > 0.0) {
            // keyframe seeks while dragging, exact on release
            if s.committed.is_none() {
                acts.push(DockAction::Seek(v as f64 * d, false));
            }
        }
        if let (Some(v), true) = (s.committed, d > 0.0) {
            acts.push(DockAction::Seek(v as f64 * d, true));
        }
        self.busy = s.dragging || self.volume_open;

        // ---- speed: − 1.0× + --------------------------------------------------------------------------------
        let spx = ox + x_speed + pad + speed_nudge;
        let minus_r = Rect::new(spx, oy + pad, 32.0, 40.0);
        let num_r = Rect::new(spx + 32.0, oy + pad, 58.0, 40.0);
        let plus_r = Rect::new(spx + 90.0, oy + pad, 32.0, 40.0);
        if ui.text_button(idn(SLOWER), minus_r, "−", 17.0, BtnStyle { label: "Slower", ..BtnStyle::default() }).clicked {
            self.press(ui);
            acts.push(DockAction::StepSpeed(-1));
        }
        let speed_label = format!("Playback speed {}. Click to reset, hold for presets", format_speed(inp.speed));
        let now_text = format_speed(inp.speed);
        let prev_text = self.prev_speed.clone();
        let dir = self.roll_dir;
        let accent = ui.th.accent;
        let normal = ui.th.cv.text_primary.0;
        let num = ui.button(idn(SPEED), num_r, BtnStyle { label: &speed_label, long_press: true, ..BtnStyle::default() }, |ui, r, _| {
            ui.r.push_clip(r);
            let col = if (inp.speed - 1.0).abs() > 1e-6 { accent } else { normal };
            let h = ui.r.line_height(14.0);
            let (pw, nw) = (ui.r.text_width(&prev_text, 14.0, Weight::DemiBold), ui.r.text_width(&now_text, 14.0, Weight::DemiBold));
            let c = r.center();
            ui.r.push_alpha(1.0 - roll);
            ui.r.text(&prev_text, c.0 - pw / 2.0, c.1 - h / 2.0 - dir * 8.0 * roll, 14.0, Weight::DemiBold, col);
            ui.r.pop();
            ui.r.push_alpha(roll);
            ui.r.text(&now_text, c.0 - nw / 2.0, c.1 - h / 2.0 + dir * 8.0 * (1.0 - roll), 14.0, Weight::DemiBold, col);
            ui.r.pop();
            ui.r.pop_clip();
        });
        if num.clicked {
            self.press(ui);
            acts.push(DockAction::ResetSpeed);
        }
        if num.long_pressed || num.right_clicked {
            acts.push(DockAction::SpeedPresets);
        }
        if ui.text_button(idn(FASTER), plus_r, "+", 17.0, BtnStyle { label: "Faster", ..BtnStyle::default() }).clicked {
            self.press(ui);
            acts.push(DockAction::StepSpeed(1));
        }

        // ---- tools: audio, subtitles, settings, open, fullscreen (folded into "…" when narrow) -------------
        let tg = groups[4];
        ui.r.push_clip(tg);
        let row_a = self.tools_opacity.to(if self.tools_collapsed { 0.0 } else { 1.0 }, ui.th.fade(160.0), Ease::OutCubic, dt, &mut ui.animating);
        if row_a > 0.01 {
            ui.r.push_alpha(row_a);
            let on = !self.tools_collapsed;
            let by = oy + pad + 2.0;
            let bx = |i: f32| tg.x + pad + 36.0 * i;
            let st = |label| BtnStyle { label, enabled: on, ..BtnStyle::default() };
            if ui.icon_button(idn(AUDIO), Rect::new(bx(0.0), by, 36.0, 36.0), "audio", 18.0, BtnStyle { label: "Audio track", enabled: on && inp.has_audio_tracks, ..BtnStyle::default() }).clicked {
                self.press(ui);
                acts.push(DockAction::Popover("audio"));
            }
            if ui.icon_button(idn(SUBTITLES), Rect::new(bx(1.0), by, 36.0, 36.0), "subtitles", 18.0, BtnStyle { active: inp.subtitle_active, ..st("Subtitles") }).clicked {
                self.press(ui);
                acts.push(DockAction::Popover("subtitles"));
            }
            if ui.icon_button(idn(SETTINGS), Rect::new(bx(2.0), by, 36.0, 36.0), "settings", 18.0, st("Settings")).clicked {
                self.press(ui);
                acts.push(DockAction::Popover("settings"));
            }
            if ui.icon_button(idn(OPEN), Rect::new(bx(3.0), by, 36.0, 36.0), "open", 18.0, st("Open")).clicked {
                self.press(ui);
                acts.push(DockAction::Open);
            }
            let (icon, label) = if inp.fullscreen { ("exitFullscreen", "Exit full screen") } else { ("fullscreen", "Full screen") };
            if ui.icon_button(idn(FULLSCREEN), Rect::new(bx(4.0), by, 36.0, 36.0), icon, 18.0, st(label)).clicked {
                self.press(ui);
                acts.push(DockAction::Fullscreen);
            }
            ui.r.pop();
        }
        if row_a < 0.99 {
            ui.r.push_alpha(1.0 - row_a);
            let on = self.tools_collapsed;
            if ui.icon_button(idn(MORE), Rect::new(tg.x + pad, oy + pad, 40.0, 40.0), "more", 18.0, BtnStyle { label: "More", enabled: on, ..BtnStyle::default() }).clicked {
                self.press(ui);
                acts.push(DockAction::Popover("tools"));
            }
            ui.r.pop();
        }
        ui.r.pop_clip();

        // ---- time bubble over the seek bar (follows the pointer with a light spring lag) --------------------
        let target_x = seek_rect.x + (if dragging_seek { drag_v } else { s.hover_value }) * seek_rect.w;
        let bx = self.bubble_x.to(target_x, dt, rm, &mut ui.animating);
        let show = (s.hovered || s.dragging) && d > 0.0;
        let bs = self.bubble_show.to(if show { 1.0 } else { 0.0 }, dt, rm, &mut ui.animating);
        if bs > 0.02 {
            let text = crate::ui::format_time((if dragging_seek { drag_v } else { s.hover_value }) as f64 * d);
            let tw = ui.r.text_width(&text, 12.0, Weight::DemiBold);
            let (bw, bh) = (tw + 20.0, 28.0);
            let by = oy - bh - 10.0 + (1.0 - bs) * 6.0;
            let rect = Rect::new(bx - bw / 2.0, by, bw, bh);
            ui.r.push(rect.center(), 0.85 + 0.15 * bs, (0.0, 0.0), bs);
            ui.glass(GlassSpec { role: Role::Pill, shapes: vec![(rect, bh / 2.0, 0.0)], dim: 0.0, shimmer: -1.0, overlap: 0.0 });
            let lh = ui.r.line_height(12.0);
            ui.r.text(&text, rect.x + 10.0, rect.y + (bh - lh) / 2.0, 12.0, Weight::DemiBold, ui.th.c.text_primary.0);
            ui.r.pop();
        }
        ui.r.pop();
        acts
    }
}

impl Default for DockState {
    fn default() -> Self {
        Self::new()
    }
}
