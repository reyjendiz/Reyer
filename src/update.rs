//! Updates: the latest GitHub release is checked in the background at startup. Installing downloads its
//! setup.exe and runs it silently; the installer closes Reyer, replaces the files and starts it again.
use crate::shell::UserEvent;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use winit::event_loop::EventLoopProxy;

const LATEST: &str = "https://api.github.com/repos/reyjendiz/Reyer/releases/latest";

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Idle,
    Available { version: String, url: String },
    Downloading { version: String },
    /// The installer is running; Reyer should quit so its files can be replaced.
    Launched,
    Failed(String),
}

pub struct Updater {
    state: Arc<Mutex<State>>,
    proxy: EventLoopProxy<UserEvent>,
}

impl Updater {
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> Updater {
        Updater { state: Arc::new(Mutex::new(State::Idle)), proxy }
    }

    pub fn state(&self) -> State {
        self.state.lock().map(|s| s.clone()).unwrap_or(State::Idle)
    }

    fn spawn(&self, job: impl FnOnce() -> State + Send + 'static) {
        let (state, proxy) = (self.state.clone(), self.proxy.clone());
        std::thread::spawn(move || {
            let next = job();
            if let Ok(mut s) = state.lock() {
                *s = next;
            }
            let _ = proxy.send_event(UserEvent::Wake);
        });
    }

    /// Silent on any network error: no update shown is the same as no update.
    pub fn check(&self) {
        self.spawn(|| latest().unwrap_or(State::Idle));
    }

    pub fn install(&self) {
        let State::Available { version, url } = self.state() else { return };
        if let Ok(mut s) = self.state.lock() {
            *s = State::Downloading { version: version.clone() };
        }
        self.spawn(move || match download_and_run(&url, &version) {
            Ok(()) => State::Launched,
            Err(e) => State::Failed(e),
        });
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).user_agent(concat!("Reyer/", env!("CARGO_PKG_VERSION"))).build()
}

fn latest() -> Option<State> {
    let release: Value = agent().get(LATEST).timeout(Duration::from_secs(20)).call().ok()?.into_json().ok()?;
    let tag = release["tag_name"].as_str()?;
    if !newer(tag, env!("CARGO_PKG_VERSION")) {
        return None;
    }
    let url = release["assets"].as_array()?.iter().find_map(|a| {
        let name = a["name"].as_str()?;
        name.ends_with("-setup.exe").then(|| a["browser_download_url"].as_str().map(str::to_string)).flatten()
    })?;
    Some(State::Available { version: tag.trim_start_matches('v').to_string(), url })
}

fn download_and_run(url: &str, version: &str) -> Result<(), String> {
    let path: PathBuf = std::env::temp_dir().join(format!("Reyer-{version}-setup.exe"));
    let resp = agent().get(url).timeout(Duration::from_secs(600)).call().map_err(|e| e.to_string())?;
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    std::io::copy(&mut resp.into_reader(), &mut file).map_err(|e| e.to_string())?;
    drop(file);
    // the installer keeps the previous install folder and mode; it asks for admin rights itself if needed
    std::process::Command::new(&path)
        .args(["/SILENT", "/SP-", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// `tag` ("v2.1.0") is newer than `current` ("2.0.0"), comparing numbers part by part.
pub fn newer(tag: &str, current: &str) -> bool {
    let parse = |s: &str| s.trim_start_matches('v').split('.').map(|p| p.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    parse(tag) > parse(current)
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn versions_compare_as_numbers() {
        assert!(newer("v2.1.0", "2.0.0"));
        assert!(newer("v2.10.0", "2.9.3"));
        assert!(!newer("v2.0.0", "2.0.0"));
        assert!(!newer("v1.9.9", "2.0.0"));
    }
}
