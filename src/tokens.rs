//! Design tokens: every color, spacing, radius, glass, spring and timing value lives in
//! `assets/design-tokens.json`; this parses it once into typed structs.
use serde::Deserialize;
use std::sync::OnceLock;

pub type Rgba = [f32; 4];

/// `#RRGGBB` or `#RRGGBBAA` (CSS order) to straight RGBA floats.
pub fn parse_color(css: &str) -> Rgba {
    let h = css.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f32 / 255.0;
    [byte(0), byte(2), byte(4), if h.len() >= 8 { byte(6) } else { 1.0 }]
}

#[derive(Deserialize, Clone, Copy)]
#[serde(from = "String")]
pub struct Color(pub Rgba);
impl From<String> for Color {
    fn from(s: String) -> Self {
        Color(parse_color(&s))
    }
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Palette {
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_tertiary: Color,
    pub glass_tint: Color,
    pub glass_tint_regular: Color,
    pub frost: Color,
    pub rim: Color,
    pub border: Color,
    pub shadow: Color,
    pub track: Color,
    pub track_fill: Color,
    pub focus_ring: Color,
    pub danger: Color,
    pub content_background: Color,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Colors {
    pub dark: Palette,
    pub light: Palette,
    pub increased_contrast: Palette,
}

#[derive(Deserialize)]
pub struct Spacing {
    pub xs: f32,
    pub s: f32,
    pub m: f32,
    pub l: f32,
    pub xl: f32,
}

#[derive(Deserialize)]
pub struct Radii {
    pub window: f32,
    pub popover: f32,
    pub toast: f32,
    pub control: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sizes {
    pub dock_height: f32,
    pub dock_max_width: f32,
    pub dock_bottom_margin: f32,
    pub control: f32,
    pub primary_control: f32,
    pub chrome_height: f32,
    pub window_min: [f32; 2],
    pub window_default: [f32; 2],
}

#[derive(Deserialize, Clone, Copy)]
pub struct GlassVariant {
    pub blur: f32,
    pub saturation: f32,
    pub lift: f32,
    pub refraction: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Glass {
    pub regular: GlassVariant,
    pub clear: GlassVariant,
    pub dimming: f32,
    pub dim_on_luminance: f32,
    pub dim_off_luminance: f32,
    pub luminance_sample_ms: u64,
    pub rim_width: f32,
    pub rim_high: f32,
    pub rim_low: f32,
    pub shadow_opacity: f32,
    pub smooth_union: f32,
    pub overlap_opacity_boost: f32,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SpringParams {
    pub response: f32,
    pub damping_ratio: f32,
}

#[derive(Deserialize)]
pub struct Springs {
    pub smooth: SpringParams,
    pub snappy: SpringParams,
    pub bouncy: SpringParams,
    pub follow: SpringParams,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timings {
    pub fade: f32,
    pub fade_fast: f32,
    pub fade_slow: f32,
    pub reduced_motion_fade: f32,
    pub auto_hide: f32,
    pub click_delay: f32,
    pub toast_hold: f32,
    pub volume_collapse: f32,
    pub geometry_save_debounce: f32,
    pub track_crossfade: f32,
    pub first_frame_fade: f32,
}

#[derive(Deserialize)]
pub struct Tokens {
    pub accent: Color,
    pub colors: Colors,
    pub spacing: Spacing,
    pub radii: Radii,
    pub sizes: Sizes,
    pub glass: Glass,
    pub springs: Springs,
    pub timings: Timings,
}

pub fn tokens() -> &'static Tokens {
    static T: OnceLock<Tokens> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(include_str!("../assets/design-tokens.json")).expect("design-tokens.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_parse_css_order() {
        assert_eq!(parse_color("#FF000080"), [1.0, 0.0, 0.0, 128.0 / 255.0]);
        assert_eq!(parse_color("#0B0B0E")[3], 1.0);
    }

    #[test]
    fn the_token_file_loads() {
        let t = tokens();
        assert_eq!(t.sizes.dock_height, 52.0);
        assert_eq!(t.springs.snappy, SpringParams { response: 0.35, damping_ratio: 0.85 });
    }
}
