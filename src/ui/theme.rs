//! Design tokens resolved against the user's settings and the Windows preferences.
use crate::gfx::Rgba;
use crate::settings::Settings;
use crate::tokens::{parse_color, tokens, Palette};
use crate::win::SystemPrefs;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// The transport dock (clear glass over video).
    Dock,
    /// Window buttons (clear glass over video).
    Cluster,
    /// A small pill such as the time bubble.
    Pill,
    /// Menus, toasts, popovers, tooltips.
    Regular,
    /// The content-layer standard material (blur and vibrancy, no lensing).
    Material,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variant {
    Regular,
    Clear,
    Material,
}

pub struct Theme {
    pub reduce_transparency: bool,
    pub increase_contrast: bool,
    pub reduce_motion: bool,
    pub light: bool,
    /// Regular glass and the content layer follow the Windows theme.
    pub c: &'static Palette,
    /// Clear glass over video is always light-on-dark; the dimming rule keeps it legible over bright frames.
    pub cv: &'static Palette,
    pub accent: Rgba,
    glass_style: String,
    /// Settings > Glass Transparency: 0 clear .. 1 frosted, applied to every glass element alike.
    pub glass_opacity: f32,
    /// The specular light shifts subtly with the pointer.
    pub light_dir: [f32; 2],
}

fn pick(mode: &str, system: bool) -> bool {
    match mode {
        "on" => true,
        "off" => false,
        _ => system,
    }
}

/// Like Qt's `lighter(130)`: scales HSV value, then desaturates what overflows.
pub fn lighter(c: Rgba, factor: f32) -> Rgba {
    let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    if mx <= 0.0 {
        let v = (factor - 1.0).clamp(0.0, 1.0);
        return [v, v, v, c[3]];
    }
    let (v, s) = (mx, (mx - mn) / mx);
    let v2 = v * factor;
    let (v2, s2) = if v2 > 1.0 { (1.0, (s - (v2 - 1.0)).max(0.0)) } else { (v2, s) };
    // rebuild RGB keeping hue: scale the channel spread with the new saturation
    let mn2 = v2 * (1.0 - s2);
    let k = if mx - mn > 1e-6 { (v2 - mn2) / (mx - mn) } else { 0.0 };
    [mn2 + (c[0] - mn) * k, mn2 + (c[1] - mn) * k, mn2 + (c[2] - mn) * k, c[3]]
}

impl Theme {
    pub fn new(settings: &Settings, sys: &SystemPrefs, light_dir: [f32; 2]) -> Theme {
        let t = tokens();
        let increase_contrast = pick(&settings.increase_contrast, sys.increase_contrast);
        let light = sys.light_theme && !increase_contrast;
        let accent = parse_color(&settings.accent);
        Theme {
            reduce_transparency: pick(&settings.reduce_transparency, sys.reduce_transparency),
            increase_contrast,
            reduce_motion: pick(&settings.reduce_motion, sys.reduce_motion),
            light,
            c: if increase_contrast { &t.colors.increased_contrast } else if light { &t.colors.light } else { &t.colors.dark },
            cv: if increase_contrast { &t.colors.increased_contrast } else { &t.colors.dark },
            accent: if increase_contrast { lighter(accent, 1.3) } else { accent },
            glass_style: settings.glass_style.clone(),
            glass_opacity: settings.glass_opacity,
            light_dir,
        }
    }

    /// Duration of a fade, shortened (not removed) when motion is reduced.
    pub fn fade(&self, ms: f32) -> f32 {
        if self.reduce_motion {
            tokens().timings.reduced_motion_fade
        } else {
            ms
        }
    }

    /// Auto: clear glass only for the chrome floating over video; everything text-heavy is regular.
    pub fn variant_for(&self, role: Role) -> Variant {
        match role {
            Role::Material => Variant::Material,
            Role::Dock | Role::Cluster => match self.glass_style.as_str() {
                "regular" => Variant::Regular,
                _ => Variant::Clear,
            },
            Role::Pill => match self.glass_style.as_str() {
                "clear" => Variant::Clear,
                _ => Variant::Regular,
            },
            Role::Regular => Variant::Regular,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(style: &str) -> Theme {
        let s = Settings { glass_style: style.into(), ..Settings::default() };
        Theme::new(&s, &SystemPrefs::default(), [-0.55, -0.83])
    }

    #[test]
    fn auto_uses_clear_glass_only_for_chrome() {
        let t = theme("auto");
        assert_eq!(t.variant_for(Role::Dock), Variant::Clear);
        assert_eq!(t.variant_for(Role::Cluster), Variant::Clear);
        assert_eq!(t.variant_for(Role::Pill), Variant::Regular);
        assert_eq!(t.variant_for(Role::Regular), Variant::Regular);
        assert_eq!(t.variant_for(Role::Material), Variant::Material);
    }

    #[test]
    fn forced_styles() {
        assert_eq!(theme("regular").variant_for(Role::Dock), Variant::Regular);
        assert_eq!(theme("clear").variant_for(Role::Pill), Variant::Clear);
        assert_eq!(theme("clear").variant_for(Role::Regular), Variant::Regular);
    }

    #[test]
    fn overrides_beat_the_system() {
        let s = Settings { reduce_motion: "off".into(), reduce_transparency: "on".into(), ..Settings::default() };
        let sys = SystemPrefs { reduce_motion: true, ..SystemPrefs::default() };
        let t = Theme::new(&s, &sys, [0.0, -1.0]);
        assert!(!t.reduce_motion && t.reduce_transparency);
    }

    #[test]
    fn lighter_brightens_without_overflow() {
        let c = lighter([1.0, 0.416, 0.239, 1.0], 1.3);
        assert!(c.iter().all(|v| (0.0..=1.0001).contains(v)));
        assert!(c[1] > 0.416 && c[2] > 0.239);
    }
}
