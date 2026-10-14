//! Optional: abstract covers for tracks without embedded art, made by a local ComfyUI install.
//! Off by default. Never blocks playback, cancellable, skipped while a video plays, frees the GPU.
//!
//! Jobs go through the Studio UI server (:8189), which proxies the ComfyUI API (:8188) and starts
//! ComfyUI on demand. Whatever this module starts, it stops again, so the GPU is freed.
use crate::settings::data_dir;
use crate::shell::UserEvent;
use serde_json::{json, Value};
use std::net::{SocketAddr, TcpStream};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::event_loop::EventLoopProxy;

const UI_PORT: u16 = 8189;
const COMFY_PORT: u16 = 8188;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const STYLE: &str = "frosted liquid glass, soft refraction, subtle specular highlights, deep dark background, Apple-style minimal, soft studio light";
const PALETTES: [&str; 8] = [
    "amber and deep plum",
    "teal and midnight blue",
    "rose and violet",
    "emerald and graphite",
    "coral and indigo",
    "gold and charcoal",
    "cyan and magenta",
    "sage and warm sand",
];

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (!(crc & 1)).wrapping_add(1));
        }
    }
    !crc
}

fn root() -> PathBuf {
    std::env::var_os("REYER_COMFYUI").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\ComfyUI"))
}

pub fn available() -> bool {
    root().join("ui").join("server.py").is_file() && root().join("python_embeded").join("python.exe").is_file()
}

/// A stable file name for a track key (FNV-1a, so the cache survives restarts and updates).
pub fn cache_path(key: &str) -> PathBuf {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.as_bytes() {
        h = (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    let dir = data_dir().join("art-cache");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{h:016x}.jpg"))
}

pub fn cached(key: &str) -> Option<PathBuf> {
    Some(cache_path(key)).filter(|p| p.is_file())
}

/// Words of the track only steer palette and mood; the image itself never contains text.
pub fn cover_prompt(title: &str, artist: &str, album: &str) -> String {
    let palette = PALETTES[crc32(format!("{artist}{album}{title}").as_bytes()) as usize % PALETTES.len()];
    let mood = [title, album].iter().filter(|w| !w.is_empty()).map(|w| w.to_lowercase()).collect::<Vec<_>>().join(", ");
    format!(
        "abstract album cover art, flowing translucent glass forms and soft light, mood evoking {mood}, palette of {palette}, {STYLE}, square composition, no text, no letters, no typography"
    )
}

fn port_open(port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(500)).is_ok()
}

fn zimage_workflow(prompt: &str, width: u32, height: u32, seed: u64, prefix: &str) -> Value {
    json!({
        "1": {"class_type": "UNETLoader", "inputs": {"unet_name": "zImageUltimateNSFW_v20.safetensors", "weight_dtype": "default"}},
        "2": {"class_type": "CLIPLoader", "inputs": {"clip_name": "qwen_3_4b_fp8_mixed.safetensors", "type": "lumina2", "device": "default"}},
        "3": {"class_type": "VAELoader", "inputs": {"vae_name": "ae.safetensors"}},
        "4": {"class_type": "ModelSamplingAuraFlow", "inputs": {"model": ["1", 0], "shift": 3}},
        "5": {"class_type": "CLIPTextEncode", "inputs": {"clip": ["2", 0], "text": prompt}},
        "6": {"class_type": "ConditioningZeroOut", "inputs": {"conditioning": ["5", 0]}},
        "7": {"class_type": "EmptySD3LatentImage", "inputs": {"width": width, "height": height, "batch_size": 1}},
        "8": {"class_type": "KSampler", "inputs": {"model": ["4", 0], "positive": ["5", 0], "negative": ["6", 0], "latent_image": ["7", 0],
              "seed": seed, "steps": 8, "cfg": 1, "sampler_name": "res_multistep", "scheduler": "simple", "denoise": 1}},
        "9": {"class_type": "VAEDecode", "inputs": {"samples": ["8", 0], "vae": ["3", 0]}},
        "10": {"class_type": "SaveImage", "inputs": {"images": ["9", 0], "filename_prefix": prefix}},
    })
}

/// Starts the Studio server if needed; on drop stops what it started.
struct Studio {
    server: Option<Child>,
    comfy_was_running: bool,
}

impl Studio {
    fn start(cancel: &AtomicBool) -> Result<Studio, String> {
        let mut s = Studio { server: None, comfy_was_running: port_open(COMFY_PORT) };
        if !port_open(UI_PORT) {
            let child = Command::new(root().join("python_embeded").join("python.exe"))
                .arg(root().join("ui").join("server.py"))
                .current_dir(root())
                .creation_flags(CREATE_NO_WINDOW)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
            s.server = Some(child);
            let deadline = Instant::now() + Duration::from_secs(30);
            while !port_open(UI_PORT) {
                if Instant::now() > deadline || cancel.load(Ordering::Relaxed) {
                    return Err("the ComfyUI Studio server did not start".into());
                }
                std::thread::sleep(Duration::from_millis(300));
            }
        }
        Ok(s)
    }

    fn run(&self, workflow: Value, cancel: &AtomicBool) -> Result<PathBuf, String> {
        let base = format!("http://127.0.0.1:{UI_PORT}");
        let reply: Value = ureq::post(&format!("{base}/prompt"))
            .timeout(Duration::from_secs(600))
            .send_json(json!({ "prompt": workflow }))
            .map_err(|e| e.to_string())?
            .into_json()
            .map_err(|e| e.to_string())?;
        let id = reply["prompt_id"].as_str().ok_or("no prompt id")?.to_string();
        let start = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            std::thread::sleep(Duration::from_secs(1));
            let history: Value = ureq::get(&format!("{base}/history/{id}")).timeout(Duration::from_secs(30)).call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
            let Some(item) = history.get(&id) else {
                if start.elapsed() > Duration::from_secs(900) {
                    return Err("ComfyUI job timed out".into());
                }
                continue;
            };
            if item["status"]["status_str"] == "error" {
                return Err("ComfyUI error".into());
            }
            let image = item["outputs"].as_object().into_iter().flat_map(|o| o.values()).flat_map(|o| o["images"].as_array().cloned().unwrap_or_default()).next();
            if let Some(im) = image {
                let out = root().join("ComfyUI").join("output").join(im["subfolder"].as_str().unwrap_or("")).join(im["filename"].as_str().unwrap_or(""));
                return Ok(out);
            }
        }
    }
}

impl Drop for Studio {
    fn drop(&mut self) {
        if let Some(child) = self.server.take() {
            let _ = Command::new("taskkill").args(["/T", "/F", "/PID", &child.id().to_string()]).creation_flags(CREATE_NO_WINDOW).output();
            if !self.comfy_was_running {
                // the server launched ComfyUI as its own process: free the GPU
                let script = format!("Get-NetTCPConnection -LocalPort {COMFY_PORT} -State Listen -ErrorAction SilentlyContinue | ForEach-Object {{ Stop-Process -Id $_.OwningProcess -Force }}");
                let _ = Command::new("powershell").args(["-NoProfile", "-Command", &script]).creation_flags(CREATE_NO_WINDOW).output();
            }
        }
    }
}

pub struct ArtGenerator {
    cancel: Arc<AtomicBool>,
    tx: Sender<(String, PathBuf)>,
    rx: Receiver<(String, PathBuf)>,
    proxy: EventLoopProxy<UserEvent>,
}

impl ArtGenerator {
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> ArtGenerator {
        let (tx, rx) = channel();
        ArtGenerator { cancel: Arc::new(AtomicBool::new(false)), tx, rx, proxy }
    }

    pub fn request(&mut self, key: &str, title: &str, artist: &str, album: &str) {
        if !available() || cached(key).is_some() {
            return;
        }
        self.cancel();
        self.cancel = Arc::new(AtomicBool::new(false));
        let (cancel, tx, proxy) = (self.cancel.clone(), self.tx.clone(), self.proxy.clone());
        let (key, prompt) = (key.to_string(), cover_prompt(title, artist, album));
        std::thread::spawn(move || {
            // cancelled, ComfyUI busy or broken: the generated fallback art stays
            let _ = (|| -> Result<(), String> {
                let studio = Studio::start(&cancel)?;
                let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) % (1 << 31);
                let png = studio.run(zimage_workflow(&prompt, 1024, 1024, seed, "reyer_cover"), &cancel)?;
                drop(studio);
                if cancel.load(Ordering::Relaxed) {
                    return Ok(());
                }
                let img = image::open(&png).map_err(|e| e.to_string())?.to_rgb8();
                let out = cache_path(&key);
                let mut file = std::fs::File::create(&out).map_err(|e| e.to_string())?;
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 92).encode_image(&img).map_err(|e| e.to_string())?;
                let _ = tx.send((key, out));
                let _ = proxy.send_event(UserEvent::Wake);
                Ok(())
            })();
        });
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn poll(&self) -> Option<(String, PathBuf)> {
        self.rx.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn prompts_never_ask_for_text_and_pick_a_stable_palette() {
        let a = cover_prompt("Song", "Artist", "Album");
        assert!(a.contains("no text"));
        assert_eq!(a, cover_prompt("Song", "Artist", "Album"));
        assert!(a.contains("song, album"));
    }

    #[test]
    fn cache_names_are_stable() {
        assert_eq!(cache_path("a|b|c"), cache_path("a|b|c"));
        assert_ne!(cache_path("a|b|c"), cache_path("a|b|d"));
    }
}
