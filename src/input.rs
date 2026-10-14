//! Input collected by the window shell between frames and consumed by the UI.
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum K {
    Space,
    Enter,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Tab,
    PageUp,
    PageDown,
    Home,
    End,
    Backspace,
    Menu,
    Char(char),
}

#[derive(Clone, Copy, Debug)]
pub struct KeyPress {
    pub key: K,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub repeat: bool,
}

#[derive(Default)]
pub struct Input {
    /// Pointer position in logical px, window coordinates.
    pub mouse: (f32, f32),
    pub mouse_in: bool,
    pub moved: bool,
    pub down: bool,
    pub pressed: bool,
    pub released: bool,
    pub double_click: bool,
    pub right_pressed: bool,
    pub right_released: bool,
    /// Wheel notches this frame (positive = up).
    pub wheel: f32,
    pub keys: Vec<KeyPress>,
    pub files: Vec<String>,
    /// A file is being dragged over the window.
    pub hovering_files: bool,
    last_click: Option<(Instant, (f32, f32))>,
}

impl Input {
    pub fn press(&mut self) {
        self.pressed = true;
        self.down = true;
        let now = Instant::now();
        if let Some((t, p)) = self.last_click {
            if now.duration_since(t) < Duration::from_millis(400) && (p.0 - self.mouse.0).abs() + (p.1 - self.mouse.1).abs() < 6.0 {
                self.double_click = true;
                self.last_click = None;
                return;
            }
        }
        self.last_click = Some((now, self.mouse));
    }

    pub fn release(&mut self) {
        self.released = true;
        self.down = false;
    }

    /// Clears everything that only lasts one frame.
    pub fn end_frame(&mut self) {
        self.pressed = false;
        self.released = false;
        self.double_click = false;
        self.right_pressed = false;
        self.right_released = false;
        self.moved = false;
        self.wheel = 0.0;
        self.keys.clear();
        self.files.clear();
    }

    pub fn key(&self, k: K) -> Option<KeyPress> {
        self.keys.iter().copied().find(|p| p.key == k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_quick_presses_make_a_double_click() {
        let mut i = Input::default();
        i.press();
        assert!(!i.double_click);
        i.end_frame();
        i.press();
        assert!(i.double_click);
    }

    #[test]
    fn a_far_second_press_is_a_new_click() {
        let mut i = Input::default();
        i.press();
        i.end_frame();
        i.mouse = (100.0, 100.0);
        i.press();
        assert!(!i.double_click);
    }
}
