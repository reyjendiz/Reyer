//! Media file knowledge: extensions, DRM detection, tags and embedded cover art.
use lofty::file::TaggedFileExt;
use lofty::picture::PictureType;
use lofty::probe::Probe;
use lofty::tag::Accessor;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const VIDEO_EXTS: &[&str] = &[
    "mov", "qt", "m4v", "mp4", "mkv", "avi", "wmv", "webm", "flv", "ts", "m2ts", "mts", "vob", "3gp", "3g2", "mpg", "mpeg",
    "m2v", "ogv", "asf", "rm", "rmvb", "divx", "f4v", "mxf",
];
pub const AUDIO_EXTS: &[&str] = &[
    "m4a", "m4b", "m4r", "caf", "aif", "aiff", "aifc", "mp3", "flac", "wav", "aac", "ogg", "oga", "opus", "wma", "ape", "wv",
    "ac3", "eac3", "dts", "mka", "alac", "mp2", "tta", "dsf", "dff",
];
const DRM_EXTS: &[&str] = &["m4p"];
const DRM_MARKERS: [&[u8]; 3] = [b"drms", b"drmi", b"p608"];

fn ext(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn is_media(path: &Path) -> bool {
    let e = ext(path);
    [VIDEO_EXTS, AUDIO_EXTS, DRM_EXTS].iter().any(|list| list.contains(&e.as_str()))
}

/// File dialog filter strings, e.g. `["mov", "mp4"]`.
pub fn video_filter() -> Vec<&'static str> {
    VIDEO_EXTS.to_vec()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// FairPlay files carry `drms`/`drmi` sample entries. The moov box may sit at either end of the file.
pub fn looks_protected(path: &Path) -> bool {
    if DRM_EXTS.contains(&ext(path).as_str()) {
        return true;
    }
    let scan = || -> std::io::Result<bool> {
        let mut f = File::open(path)?;
        let len = f.metadata()?.len();
        let mut head = vec![0u8; 1 << 20];
        let n = f.read(&mut head)?;
        head.truncate(n);
        f.seek(SeekFrom::Start(len.saturating_sub(1 << 20)))?;
        let mut tail = Vec::new();
        f.read_to_end(&mut tail)?;
        Ok(DRM_MARKERS.iter().any(|m| contains(&head, m) || contains(&tail, m)))
    };
    scan().unwrap_or(false)
}

#[derive(Default, Clone, Debug)]
pub struct Tags {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover: Option<Vec<u8>>,
}

/// Title, artist, album and the embedded cover image (MP4/M4A, FLAC, MP3/AIFF, OGG/Opus, WMA), if any.
/// A broken or unusual container yields empty tags, never a crash.
pub fn read_tags(path: &Path) -> Tags {
    let Ok(probe) = Probe::open(path) else { return Tags::default() };
    let Ok(file) = probe.read() else { return Tags::default() };
    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else { return Tags::default() };
    let pictures = tag.pictures();
    let cover = pictures
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())
        .map(|p| p.data().to_vec());
    Tags {
        title: tag.title().map(|s| s.into_owned()).unwrap_or_default(),
        artist: tag.artist().map(|s| s.into_owned()).unwrap_or_default(),
        album: tag.album().map(|s| s.into_owned()).unwrap_or_default(),
        cover,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("reyer-media-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn media_extensions_are_case_insensitive() {
        for name in ["a.MOV", "b.m4a", "c.CAF", "d.aiff", "e.mkv", "f.flac", "g.m4p"] {
            assert!(is_media(Path::new(name)), "{name}");
        }
        assert!(!is_media(Path::new("notes.txt")));
        assert!(!is_media(Path::new("noext")));
    }

    #[test]
    fn drm_detection() {
        assert!(looks_protected(Path::new("song.m4p")));
        let mut protected = vec![0u8; 100];
        protected.extend_from_slice(b"stsd....drms");
        protected.extend_from_slice(&[0u8; 100]);
        assert!(looks_protected(&scratch("protected.m4a", &protected)));
        assert!(!looks_protected(&scratch("clean.m4a", &[0u8; 1000])));
    }

    #[test]
    fn tags_of_a_non_media_file_are_empty() {
        let t = read_tags(&scratch("x.mp3", b"not really audio"));
        assert!(t.cover.is_none() && t.title.is_empty());
    }
}
