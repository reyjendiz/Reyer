//! User settings, stored as JSON in `%APPDATA%\Reyer\settings.json`.
use indexmap::IndexMap;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const MAX_RESUME_ENTRIES: usize = 300;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    /// Last NORMAL (windowed) rect in physical pixels: x, y, w, h.
    pub window_rect: Option<[i32; 4]>,
    pub volume: f32,
    pub muted: bool,
    /// auto | regular | clear
    pub glass_style: String,
    /// unified (one pill) | grouped (separate capsules)
    pub dock_style: String,
    pub background_extension: bool,
    /// 0 = clearest glass .. 1 = most frosted; the same for the dock, menus and toasts.
    pub glass_opacity: f32,
    pub accent: String,
    /// auto | on | off (auto follows Windows)
    pub reduce_transparency: String,
    pub increase_contrast: String,
    pub reduce_motion: String,
    pub hwdec: bool,
    /// Bitstream Dolby/DTS to an AV receiver over HDMI/S/PDIF.
    pub audio_passthrough: bool,
    pub resume_playback: bool,
    /// Play the next file of the folder when one ends.
    pub autoplay_next: bool,
    pub resume_positions: IndexMap<String, f64>,
    pub last_folder: String,
    pub generate_artwork: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            window_rect: None,
            volume: 80.0,
            muted: false,
            glass_style: "auto".into(),
            dock_style: "unified".into(),
            background_extension: true,
            glass_opacity: 0.2,
            accent: "#FF6A3D".into(),
            reduce_transparency: "auto".into(),
            increase_contrast: "auto".into(),
            reduce_motion: "auto".into(),
            hwdec: true,
            audio_passthrough: false,
            resume_playback: true,
            autoplay_next: true,
            resume_positions: IndexMap::new(),
            last_folder: String::new(),
            generate_artwork: false,
        }
    }
}

fn field<T: DeserializeOwned>(raw: &serde_json::Map<String, Value>, key: &str, default: T) -> T {
    // a hand-edited file with a wrong type falls back to the default instead of breaking startup
    raw.get(key).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(default)
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        let raw = match std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
            Some(Value::Object(map)) => map,
            _ => return Settings::default(),
        };
        let d = Settings::default();
        Settings {
            window_rect: field(&raw, "window_rect", d.window_rect),
            volume: field(&raw, "volume", d.volume),
            muted: field(&raw, "muted", d.muted),
            glass_style: field(&raw, "glass_style", d.glass_style),
            dock_style: field(&raw, "dock_style", d.dock_style),
            background_extension: field(&raw, "background_extension", d.background_extension),
            glass_opacity: field::<f32>(&raw, "glass_opacity", d.glass_opacity).clamp(0.0, 1.0),
            accent: field(&raw, "accent", d.accent),
            reduce_transparency: field(&raw, "reduce_transparency", d.reduce_transparency),
            increase_contrast: field(&raw, "increase_contrast", d.increase_contrast),
            reduce_motion: field(&raw, "reduce_motion", d.reduce_motion),
            hwdec: field(&raw, "hwdec", d.hwdec),
            audio_passthrough: field(&raw, "audio_passthrough", d.audio_passthrough),
            resume_playback: field(&raw, "resume_playback", d.resume_playback),
            autoplay_next: field(&raw, "autoplay_next", d.autoplay_next),
            resume_positions: field(&raw, "resume_positions", d.resume_positions),
            last_folder: field(&raw, "last_folder", d.last_folder),
            generate_artwork: field(&raw, "generate_artwork", d.generate_artwork),
        }
    }

    pub fn save(&mut self, path: &Path) -> std::io::Result<()> {
        if self.resume_positions.len() > MAX_RESUME_ENTRIES {
            let skip = self.resume_positions.len() - MAX_RESUME_ENTRIES;
            self.resume_positions = self.resume_positions.drain(..).skip(skip).collect();
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, path) // atomic: a crash mid-write never leaves a broken settings file
    }
}

/// `%APPDATA%\Reyer`, created on demand.
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from));
    let dir = base.unwrap_or_else(|| PathBuf::from(".")).join("Reyer");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("reyer-test-{}-{}", std::process::id(), rand_id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn rand_id() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn round_trip() {
        let dir = tmp();
        let path = dir.join("settings.json");
        let mut s = Settings { window_rect: Some([10, 20, 1100, 650]), volume: 55.0, glass_style: "clear".into(), muted: true, ..Settings::default() };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
    }

    #[test]
    fn missing_or_corrupt_file_gives_defaults() {
        let dir = tmp();
        assert_eq!(Settings::load(&dir.join("nope.json")), Settings::default());
        std::fs::write(dir.join("bad.json"), "{ not json").unwrap();
        assert_eq!(Settings::load(&dir.join("bad.json")), Settings::default());
    }

    #[test]
    fn wrong_types_fall_back_per_field() {
        let dir = tmp();
        let path = dir.join("s.json");
        std::fs::write(&path, r#"{"volume": "loud", "window_rect": [1, 2, 3], "muted": true, "glass_style": 5}"#).unwrap();
        let s = Settings::load(&path);
        assert_eq!(s.volume, Settings::default().volume);
        assert_eq!(s.window_rect, None);
        assert!(s.muted);
        assert_eq!(s.glass_style, "auto");
    }

    #[test]
    fn int_volume_is_accepted() {
        let dir = tmp();
        let path = dir.join("s.json");
        std::fs::write(&path, r#"{"volume": 40}"#).unwrap();
        assert_eq!(Settings::load(&path).volume, 40.0);
    }

    #[test]
    fn resume_positions_are_trimmed_to_the_most_recent() {
        let dir = tmp();
        let path = dir.join("s.json");
        let mut s = Settings::default();
        for i in 0..MAX_RESUME_ENTRIES + 10 {
            s.resume_positions.insert(format!("f{i}"), i as f64);
        }
        s.save(&path).unwrap();
        let kept = Settings::load(&path).resume_positions;
        assert_eq!(kept.len(), MAX_RESUME_ENTRIES);
        assert!(kept.contains_key(&format!("f{}", MAX_RESUME_ENTRIES + 9)) && !kept.contains_key("f0"));
    }

    #[test]
    fn save_leaves_no_temp_file() {
        let dir = tmp();
        Settings::default().save(&dir.join("settings.json")).unwrap();
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["settings.json"]);
    }
}
