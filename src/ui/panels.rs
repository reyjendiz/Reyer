//! Popover contents: the settings panel and the file-information panel. Both scroll when they
//! don't fit; every control is keyboard reachable (Tab, arrows, Space/Enter).
use crate::i18n::tr;
use super::{id, id_n, Interact, Ui};
use crate::gfx::{Rect, Weight};
use crate::input::K;
use crate::player::Section;
use crate::settings::Settings;
use crate::tokens::{parse_color, tokens};

const ACCENTS: [&str; 6] = ["#FF6A3D", "#FF375F", "#BF5AF2", "#0A84FF", "#30D158", "#FFD60A"];

#[derive(Debug, Clone, PartialEq)]
pub enum PanelAction {
    /// A boolean setting that the player must apply (key, value).
    Applied(&'static str, bool),
    /// A setting that the theme or dock reads changed.
    Changed,
    InfoRequested,
    UpdateRequested,
    Copy(String),
}

#[derive(Default)]
pub struct PanelState {
    scroll: f32,
    content_h: f32,
}

impl PanelState {
    pub fn new() -> PanelState {
        PanelState::default()
    }

    pub fn reset(&mut self) {
        self.scroll = 0.0;
    }

    /// Wheel scrolling plus clamping; `view` is the visible height.
    fn scroll(&mut self, ui: &mut Ui, area: Rect) {
        let max = (self.content_h - area.h).max(0.0);
        if ui.pointer_in(area) && ui.input.wheel != 0.0 {
            self.scroll -= ui.input.wheel * 48.0;
        }
        self.scroll = self.scroll.clamp(0.0, max);
    }

    // ---- settings ------------------------------------------------------------------------------------
    /// `update`: the available version and whether it is downloading already.
    pub fn settings(&mut self, ui: &mut Ui, body: Rect, s: &mut Settings, has_media: bool, update: Option<(&str, bool)>) -> Vec<PanelAction> {
        let t = tokens();
        let mut acts = Vec::new();
        let area = body.inset(t.spacing.l);
        self.scroll(ui, area);
        ui.r.push_clip(area);
        let (x, w) = (area.x, area.w);
        let mut y = area.y - self.scroll;
        let top = y;
        let text = ui.th.c.text_primary.0;

        // header with the File Info button
        let lh = ui.r.line_height(15.0);
        ui.r.text(tr("Settings"), x, y + (30.0 - lh) / 2.0, 15.0, Weight::DemiBold, text);
        let label = tr("File Info");
        let bw = ui.r.text_width(label, 13.0, Weight::DemiBold) + 24.0;
        let btn = Rect::new(x + w - bw, y + 1.0, bw, 28.0);
        let r = ui.interact(id("settings/info"), btn, Interact { enabled: has_media, ..Interact::default() });
        if r.hover > 0.0 {
            ui.r.rrect(btn, 14.0, if ui.th.light { [0.0, 0.0, 0.0, 0.08 * r.hover] } else { [1.0, 1.0, 1.0, 0.12 * r.hover] });
        }
        let bh = ui.r.line_height(13.0);
        ui.r.push_alpha(if has_media { 1.0 } else { 0.45 });
        ui.r.text(label, btn.x + 12.0, btn.y + (btn.h - bh) / 2.0, 13.0, Weight::DemiBold, ui.th.accent);
        ui.r.pop();
        ui.focus_ring(id("settings/info"), btn, 14.0, -2.0);
        if r.clicked {
            acts.push(PanelAction::InfoRequested);
        }
        y += 32.0;

        let mut changed = false;
        macro_rules! seg {
            ($key:literal, $label:literal, $field:expr, $opts:expr) => {{
                let opts: &[(&str, &str)] = $opts;
                let rect = Rect::new(x, y, w, 34.0);
                if segmented(ui, $key, $label, rect, &mut $field, opts) {
                    changed = true;
                }
                y += 36.0;
            }};
        }
        let auto: &[(&str, &str)] = &[("auto", "Auto"), ("on", "On"), ("off", "Off")];
        seg!("language", "Language", s.language, &[("auto", "Auto"), ("en", "English"), ("ru", "Русский"), ("uk", "Українська")]);
        seg!("glass_style", "Glass Style", s.glass_style, &[("auto", "Auto"), ("regular", "Regular"), ("clear", "Clear")]);
        seg!("dock_style", "Dock Shape", s.dock_style, &[("unified", "Single"), ("grouped", "Grouped")]);
        seg!("reduce_transparency", "Reduce Transparency", s.reduce_transparency, auto);
        seg!("increase_contrast", "Increase Contrast", s.increase_contrast, auto);
        seg!("reduce_motion", "Reduce Motion", s.reduce_motion, auto);

        let toggle = |ui: &mut Ui, key: &'static str, label: &str, hint: &str, value: &mut bool, y: &mut f32, acts: &mut Vec<PanelAction>| {
            let tw = w - 44.0 - t.spacing.m;
            let hint_lines = if hint.is_empty() { vec![] } else { ui.r.wrap(tr(hint), 11.0, Weight::Regular, tw, false) };
            let label_lines = ui.r.wrap(tr(label), 13.0, Weight::Medium, tw, false);
            let (l13, l11) = (ui.r.line_height(13.0), ui.r.line_height(11.0));
            let texts_h = l13 * label_lines.len() as f32 + l11 * hint_lines.len() as f32;
            let h = (texts_h + 10.0).max(34.0);
            let rect = Rect::new(x, *y, w, h);
            let r = ui.interact(id(key), rect, Interact { press_scale: 1.0, ..Interact::default() });
            if r.clicked {
                *value = !*value;
                acts.push(PanelAction::Applied(key, *value));
                acts.push(PanelAction::Changed);
            }
            if r.hover > 0.0 {
                let base = if ui.th.light { [0.0, 0.0, 0.0, 0.05 * r.hover] } else { [1.0, 1.0, 1.0, 0.07 * r.hover] };
                ui.r.rrect(rect.inset(-4.0), 10.0, base);
            }
            let mut ty = rect.y + (h - texts_h) / 2.0;
            for line in &label_lines {
                ui.r.text(line, x, ty, 13.0, Weight::Medium, text);
                ty += l13;
            }
            for line in &hint_lines {
                ui.r.text(line, x, ty, 11.0, Weight::Regular, ui.th.c.text_secondary.0);
                ty += l11;
            }
            let sw = Rect::new(x + w - 44.0, rect.y + (h - 26.0) / 2.0, 44.0, 26.0);
            let k = ui.st.widget(id_n(key, 1)).knob.to(if *value { 1.0 } else { 0.0 }, ui.dt, ui.th.reduce_motion, &mut ui.animating);
            let off = if ui.th.light { [0.0, 0.0, 0.0, 0.14] } else { [1.0, 1.0, 1.0, 0.18] };
            ui.r.rrect(sw, 13.0, crate::gfx::geom::mix_color(off, ui.th.accent, k));
            ui.r.circle((sw.x + 13.0 + k * 18.0, sw.y + 13.0), 11.0, [1.0, 1.0, 1.0, 1.0]);
            ui.focus_ring(id(key), sw, 13.0, -2.0);
            *y += h + 2.0;
        };
        // glass transparency slider
        {
            let lh = ui.r.line_height(13.0);
            ui.r.text(tr("Glass Transparency"), x, y + (40.0 - lh) / 2.0, 13.0, Weight::Medium, text);
            let rect = Rect::new(x + w * 0.5, y + 10.0, w * 0.5 - 6.0, 20.0);
            // slider left = clearest: the stored value is "how frosted", shown as "how transparent"
            let r = ui.slider(id("settings/glass_opacity"), rect, crate::ui::SliderOpts { value: 1.0 - s.glass_opacity, ticks: &[], step: 0.05, big_step: 0.2, enabled: true });
            if let Some(v) = r.moved {
                s.glass_opacity = 1.0 - v;
            }
            if r.committed.is_some() {
                acts.push(PanelAction::Changed);
            }
            y += 42.0;
        }
        toggle(ui, "background_extension", "Background Extension", "Fill letterbox bars with the video's edge light", &mut s.background_extension, &mut y, &mut acts);
        toggle(ui, "hwdec", "Hardware Decoding", "", &mut s.hwdec, &mut y, &mut acts);
        toggle(
            ui,
            "audio_passthrough",
            "Surround Passthrough",
            "Sends Dolby (incl. Atmos) and DTS (incl. DTS:X) untouched to an AV receiver or soundbar over HDMI. Only with such a device; volume is then set on the receiver.",
            &mut s.audio_passthrough,
            &mut y,
            &mut acts,
        );
        toggle(ui, "resume_playback", "Resume Where I Left Off", "", &mut s.resume_playback, &mut y, &mut acts);
        toggle(ui, "loop_file", "Loop", "Repeats the current video or track", &mut s.loop_file, &mut y, &mut acts);
        toggle(ui, "autoplay_next", "Play Next File in Folder", "Continues with the next file when one ends", &mut s.autoplay_next, &mut y, &mut acts);
        toggle(ui, "check_updates", "Check for Updates", "Looks for a new version on GitHub at startup", &mut s.check_updates, &mut y, &mut acts);
        toggle(ui, "generate_artwork", "Generate Artwork for Tracks Without Cover", "Uses the local ComfyUI studio if it's installed", &mut s.generate_artwork, &mut y, &mut acts);

        // accent swatches
        let lh = ui.r.line_height(13.0);
        ui.r.text(tr("Accent"), x, y + (40.0 - lh) / 2.0, 13.0, Weight::Medium, text);
        let gap = t.spacing.s;
        let mut sx = x + w - (ACCENTS.len() as f32 * 22.0 + (ACCENTS.len() as f32 - 1.0) * gap);
        for (i, hex) in ACCENTS.iter().enumerate() {
            let rect = Rect::new(sx, y + 9.0, 22.0, 22.0);
            let wid = id_n("settings/accent", i as u64);
            let r = ui.interact(wid, rect, Interact { press_scale: 1.0, ..Interact::default() });
            if r.clicked {
                s.accent = hex.to_string();
                changed = true;
            }
            ui.r.circle(rect.center(), 11.0, parse_color(hex));
            if s.accent.eq_ignore_ascii_case(hex) {
                ui.r.rrect_stroke(rect.inset(-1.0), 12.0, 2.0, text);
            }
            ui.focus_ring(wid, rect, 11.0, -3.0);
            sx += 22.0 + gap;
        }
        y += 40.0;

        if let Some((version, busy)) = update {
            let label = if busy { tr("Downloading update…").to_string() } else { crate::i18n::tr1("Update to {}", version) };
            let btn = Rect::new(x, y + 4.0, w, 34.0);
            let r = ui.interact(id("settings/update"), btn, Interact { enabled: !busy, ..Interact::default() });
            let mut bg = ui.th.accent;
            bg[3] = if busy { 0.45 } else { 0.85 + 0.15 * r.hover };
            ui.r.rrect(btn, 17.0, bg);
            let (tw, bh) = (ui.r.text_width(&label, 13.0, Weight::DemiBold), ui.r.line_height(13.0));
            ui.r.text(&label, btn.x + (btn.w - tw) / 2.0, btn.y + (btn.h - bh) / 2.0, 13.0, Weight::DemiBold, [1.0, 1.0, 1.0, 1.0]);
            ui.focus_ring(id("settings/update"), btn, 17.0, -2.0);
            if r.clicked {
                acts.push(PanelAction::UpdateRequested);
            }
            y += 46.0;
        }
        ui.r.pop_clip();
        self.content_h = y - top;
        if changed {
            acts.push(PanelAction::Changed);
        }
        acts
    }

    // ---- file information ----------------------------------------------------------------------------
    /// Clicking a value copies it (the original used selectable text).
    pub fn info(&mut self, ui: &mut Ui, body: Rect, sections: &[Section]) -> Vec<PanelAction> {
        let t = tokens();
        let mut acts = Vec::new();
        let area = body.inset(t.spacing.l);
        self.scroll(ui, area);
        if ui.take_key(K::Down).is_some() {
            self.scroll += 48.0;
        }
        if ui.take_key(K::Up).is_some() {
            self.scroll -= 48.0;
        }
        self.scroll = self.scroll.clamp(0.0, (self.content_h - area.h).max(0.0));
        ui.r.push_clip(area);
        let (x, w) = (area.x, area.w);
        let mut y = area.y - self.scroll;
        let top = y;
        let (primary, secondary) = (ui.th.c.text_primary.0, ui.th.c.text_secondary.0);
        let lh = ui.r.line_height(15.0);
        ui.r.text(tr("File Information"), x, y + (30.0 - lh) / 2.0, 15.0, Weight::DemiBold, primary);
        y += 30.0 + t.spacing.xs;
        if sections.is_empty() {
            ui.r.text(tr("Nothing is playing."), x, y, 13.0, Weight::Regular, secondary);
            y += ui.r.line_height(13.0);
        }
        let label_w = 136.0;
        let value_w = w - label_w - t.spacing.m;
        let (l12, l13) = (ui.r.line_height(12.0), ui.r.line_height(13.0));
        for (si, sec) in sections.iter().enumerate() {
            y += t.spacing.s;
            ui.r.text(tr(sec.title), x, y, 12.0, Weight::DemiBold, secondary);
            y += l12 + 2.0 + 2.0;
            for (ri, (label, value)) in sec.rows.iter().enumerate() {
                let lines = ui.r.wrap(value, 13.0, Weight::Medium, value_w, true);
                let h = l13 * lines.len().max(1) as f32;
                let rect = Rect::new(x + label_w + t.spacing.m, y, value_w, h);
                let r = ui.interact(id_n("info/row", (si * 100 + ri) as u64), rect, Interact { focusable: false, keys: false, press_scale: 1.0, ..Interact::default() });
                if r.hovered {
                    ui.r.rrect(rect.inset(-2.0), 4.0, if ui.th.light { [0.0, 0.0, 0.0, 0.06] } else { [1.0, 1.0, 1.0, 0.08] });
                }
                if r.clicked {
                    acts.push(PanelAction::Copy(value.clone()));
                }
                let l = ui.r.elide(tr(label), 13.0, Weight::Regular, label_w);
                ui.r.text(&l, x, y, 13.0, Weight::Regular, secondary);
                for (i, line) in lines.iter().enumerate() {
                    ui.r.text(line, rect.x, y + i as f32 * l13, 13.0, Weight::Medium, primary);
                }
                y += h + 2.0;
            }
        }
        ui.r.pop_clip();
        self.content_h = y - top;
        acts
    }
}

/// Auto / On / Off style choice with a sliding knob. Returns true when the value changed.
fn segmented(ui: &mut Ui, key: &'static str, label: &str, rect: Rect, value: &mut String, options: &[(&str, &str)]) -> bool {
    let wid = id(key);
    let label = tr(label);
    let seg_w = options.iter().map(|o| ui.r.text_width(tr(o.1), 12.0, Weight::DemiBold) + 18.0).fold(58.0, f32::max);
    let track = Rect::new(rect.right() - seg_w * options.len() as f32, rect.y + 3.0, seg_w * options.len() as f32, 28.0);
    let r = ui.interact(wid, track, Interact { press_scale: 1.0, keys: false, ..Interact::default() });
    let index = options.iter().position(|o| o.0 == value).unwrap_or(0);
    let mut new = None;
    if r.clicked {
        let m = ui.mouse();
        let i = (((m.0 - track.x).max(0.0) / seg_w) as usize).min(options.len() - 1);
        new = Some(i);
    }
    if r.focused {
        if ui.take_key(K::Left).is_some() {
            new = Some(index.saturating_sub(1));
            ui.set_nav(true);
        } else if ui.take_key(K::Right).is_some() {
            new = Some((index + 1).min(options.len() - 1));
            ui.set_nav(true);
        }
    }
    let mut changed = false;
    if let Some(i) = new {
        if i != index {
            *value = options[i].0.to_string();
            changed = true;
        }
    }
    let index = options.iter().position(|o| o.0 == value).unwrap_or(0);
    let lh = ui.r.line_height(13.0);
    ui.r.text(label, rect.x, rect.y + (rect.h - lh) / 2.0, 13.0, Weight::Medium, ui.th.c.text_primary.0);
    ui.r.rrect(track, 14.0, if ui.th.light { [0.0, 0.0, 0.0, 0.07] } else { [1.0, 1.0, 1.0, 0.10] });
    let knob = ui.st.widget(id_n(key, 7)).knob.to(index as f32 * seg_w, ui.dt, ui.th.reduce_motion, &mut ui.animating);
    ui.r.rrect(Rect::new(track.x + knob + 2.0, track.y + 2.0, seg_w - 4.0, 24.0), 12.0, if ui.th.light { [1.0, 1.0, 1.0, 1.0] } else { [1.0, 1.0, 1.0, 0.22] });
    let l12 = ui.r.line_height(12.0);
    for (i, o) in options.iter().enumerate() {
        let tw = ui.r.text_width(tr(o.1), 12.0, Weight::DemiBold);
        let c = if i == index { ui.th.c.text_primary.0 } else { ui.th.c.text_secondary.0 };
        ui.r.text(tr(o.1), track.x + i as f32 * seg_w + (seg_w - tw) / 2.0, track.y + (track.h - l12) / 2.0, 12.0, Weight::DemiBold, c);
    }
    ui.focus_ring(wid, track, 14.0, -2.0);
    changed
}
