//! Small geometry helpers shared by the renderer and the UI. Units are logical pixels unless noted.

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const ZERO: Rect = Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };

    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(self.x + d, self.y + d, (self.w - 2.0 * d).max(0.0), (self.h - 2.0 * d).max(0.0))
    }

    pub fn offset(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    pub fn lerp(&self, other: Rect, k: f32) -> Rect {
        Rect::new(lerp(self.x, other.x, k), lerp(self.y, other.y, k), lerp(self.w, other.w, k), lerp(self.h, other.h, k))
    }

    pub fn union(&self, o: Rect) -> Rect {
        let (x0, y0) = (self.x.min(o.x), self.y.min(o.y));
        Rect::new(x0, y0, self.right().max(o.right()) - x0, self.bottom().max(o.bottom()) - y0)
    }
}

pub fn lerp(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}

pub fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Straight-alpha color with its alpha scaled.
pub fn with_alpha(c: [f32; 4], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], c[3] * a]
}

pub fn mix_color(a: [f32; 4], b: [f32; 4], k: f32) -> [f32; 4] {
    [lerp(a[0], b[0], k), lerp(a[1], b[1], k), lerp(a[2], b[2], k), lerp(a[3], b[3], k)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_basics() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert!(r.contains(10.0, 20.0) && !r.contains(40.0, 20.0));
        assert_eq!(r.center(), (25.0, 40.0));
        assert_eq!(r.inset(5.0), Rect::new(15.0, 25.0, 20.0, 30.0));
        assert_eq!(r.lerp(Rect::new(0.0, 0.0, 0.0, 0.0), 0.5), Rect::new(5.0, 10.0, 15.0, 20.0));
    }
}
