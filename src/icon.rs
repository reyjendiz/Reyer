//! The window icon (the executable carries the full icon as a resource).
use winit::window::Icon;

pub fn window_icon() -> Option<Icon> {
    let img = image::load_from_memory(include_bytes!("../assets/icon/reyer_64.png")).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).ok()
}
