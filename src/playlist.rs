//! Folder playlist in natural sort order.
use crate::media::is_media;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// "previous" restarts the track past this point (seconds).
pub const RESTART_THRESHOLD: f64 = 4.0;

/// Natural order: digit runs compare as numbers, text compares case-insensitively.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (na, nb) = (take(&mut a), take(&mut b));
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn sorted(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort_by(|a, b| natural_cmp(&name_of(a), &name_of(b)));
    paths
}

pub fn media_in(folder: &Path, recursive: bool) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else { return vec![] };
    let entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    let mut files = sorted(entries.iter().filter(|p| p.is_file() && is_media(p)).cloned().collect());
    if recursive {
        for sub in sorted(entries.iter().filter(|p| p.is_dir()).cloned().collect()) {
            files.extend(media_in(&sub, true));
        }
    }
    files
}

#[derive(Default, Clone, Debug)]
pub struct Playlist {
    pub items: Vec<String>,
    pub index: Option<usize>,
}

impl Playlist {
    pub fn new(items: Vec<String>, index: usize) -> Self {
        Playlist { index: (index < items.len()).then_some(index), items }
    }

    /// One file queues its folder's media (starting at it); several files or folders queue exactly those.
    pub fn for_paths(paths: &[String]) -> Playlist {
        if let [only] = paths {
            let target = PathBuf::from(only);
            if target.is_file() {
                let mut siblings = target.parent().map(|p| media_in(p, false)).unwrap_or_default();
                if !siblings.contains(&target) {
                    // unknown extensions are still played, just not queued from siblings
                    siblings.push(target.clone());
                    siblings = sorted(siblings);
                }
                let index = siblings.iter().position(|p| *p == target).unwrap_or(0);
                return Playlist::new(siblings.iter().map(|p| p.to_string_lossy().into_owned()).collect(), index);
            }
        }
        let mut items = Vec::new();
        for raw in paths {
            let p = Path::new(raw);
            if p.is_dir() {
                items.extend(media_in(p, true).iter().map(|x| x.to_string_lossy().into_owned()));
            } else if p.is_file() || raw.contains("://") {
                items.push(raw.clone());
            }
        }
        Playlist::new(items, 0)
    }

    pub fn current(&self) -> Option<&str> {
        self.index.and_then(|i| self.items.get(i)).map(String::as_str)
    }

    pub fn next(&mut self) -> Option<&str> {
        match self.index {
            Some(i) if i + 1 < self.items.len() => {
                self.index = Some(i + 1);
                self.current()
            }
            _ => None,
        }
    }

    /// `None` means: restart the current track (past 4 s, or already at the first item).
    pub fn previous(&mut self, position: f64) -> Option<&str> {
        match self.index {
            Some(i) if position <= RESTART_THRESHOLD && i > 0 => {
                self.index = Some(i - 1);
                self.current()
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("reyer-pl-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(dir: &Path, names: &[&str]) {
        for n in names {
            std::fs::write(dir.join(n), b"").unwrap();
        }
    }

    fn names(pl: &Playlist) -> Vec<String> {
        pl.items.iter().map(|p| name_of(Path::new(p))).collect()
    }

    #[test]
    fn natural_sort_puts_track2_before_track10() {
        let mut v = vec!["track10.mp3", "Track2.mp3", "track1.mp3", "track2b.mp3", "track2a.mp3"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["track1.mp3", "Track2.mp3", "track2a.mp3", "track2b.mp3", "track10.mp3"]);
    }

    #[test]
    fn opening_one_file_queues_its_folder_from_that_file() {
        let d = folder("one");
        touch(&d, &["ep10.mkv", "ep2.mkv", "ep1.mkv", "notes.txt", "cover.jpg"]);
        let pl = Playlist::for_paths(&[d.join("ep2.mkv").to_string_lossy().into_owned()]);
        assert_eq!(names(&pl), ["ep1.mkv", "ep2.mkv", "ep10.mkv"]);
        assert_eq!(name_of(Path::new(pl.current().unwrap())), "ep2.mkv");
    }

    #[test]
    fn unknown_extension_is_still_played() {
        let d = folder("unknown");
        touch(&d, &["a.mp4", "weird.xyz"]);
        let pl = Playlist::for_paths(&[d.join("weird.xyz").to_string_lossy().into_owned()]);
        assert_eq!(name_of(Path::new(pl.current().unwrap())), "weird.xyz");
        assert_eq!(pl.items.len(), 2);
    }

    #[test]
    fn apple_formats_are_queued() {
        let d = folder("apple");
        let all = ["a.mov", "b.m4a", "c.caf", "d.aiff", "e.m4b", "f.qt", "g.aifc", "h.m4r", "i.m4v"];
        touch(&d, &all);
        assert_eq!(Playlist::for_paths(&[d.join("a.mov").to_string_lossy().into_owned()]).items.len(), all.len());
    }

    #[test]
    fn folders_are_expanded_recursively_in_natural_order() {
        let d = folder("rec");
        for sub in ["CD 10", "CD 2"] {
            std::fs::create_dir_all(d.join(sub)).unwrap();
        }
        touch(&d.join("CD 10"), &["1.flac"]);
        touch(&d.join("CD 2"), &["1.flac", "2.flac"]);
        let pl = Playlist::for_paths(&[d.to_string_lossy().into_owned()]);
        let parents: Vec<_> = pl.items.iter().map(|p| name_of(Path::new(p).parent().unwrap())).collect();
        assert_eq!(parents, ["CD 2", "CD 2", "CD 10"]);
    }

    #[test]
    fn next_and_end_of_list() {
        let mut pl = Playlist::new(vec!["a".into(), "b".into()], 0);
        assert_eq!(pl.next(), Some("b"));
        assert_eq!(pl.next(), None);
        assert_eq!(pl.current(), Some("b"));
    }

    #[test]
    fn previous_restarts_after_four_seconds() {
        let mut pl = Playlist::new(vec!["a".into(), "b".into()], 1);
        assert_eq!(pl.previous(4.5), None);
        assert_eq!(pl.current(), Some("b"));
        assert_eq!(pl.previous(3.0), Some("a"));
        assert_eq!(pl.previous(0.0), None); // first item: restart
    }

    #[test]
    fn urls_are_accepted() {
        let pl = Playlist::for_paths(&["https://example.com/stream.m3u8".into()]);
        assert_eq!(pl.current(), Some("https://example.com/stream.m3u8"));
    }
}
