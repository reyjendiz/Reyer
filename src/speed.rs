//! Playback speed steps for the dock's `[ − ] 1.0× [ + ]` group.
pub const MIN_SPEED: f32 = 0.25;
pub const MAX_SPEED: f32 = 4.0;
const STEP: f32 = 0.25;
pub const PRESETS: [f32; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0];

/// Next grid value (multiples of 0.25) in `direction` (+1 / -1), clamped to the range.
pub fn step_speed(current: f32, direction: i32) -> f32 {
    let grid = current / STEP;
    let next = if direction > 0 { (grid + 1e-6).floor() + 1.0 } else { (grid - 1e-6).ceil() - 1.0 };
    (next * STEP).clamp(MIN_SPEED, MAX_SPEED)
}

/// `1.0×`, `1.25×`, `0.5×`.
pub fn format_speed(speed: f32) -> String {
    let mut t = format!("{speed:.2}");
    if t.ends_with('0') {
        t.pop();
    }
    format!("{t}×")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_on_the_grid() {
        assert_eq!(step_speed(1.0, 1), 1.25);
        assert_eq!(step_speed(1.0, -1), 0.75);
        assert_eq!(step_speed(1.1, 1), 1.25);
        assert_eq!(step_speed(1.1, -1), 1.0);
        assert_eq!(step_speed(3.0, 1), 3.25);
    }

    #[test]
    fn clamps_to_the_range() {
        assert_eq!(step_speed(4.0, 1), 4.0);
        assert_eq!(step_speed(0.25, -1), 0.25);
    }

    #[test]
    fn rapid_presses_accumulate() {
        let mut s = 1.0;
        for _ in 0..3 {
            s = step_speed(s, 1);
        }
        assert_eq!(s, 1.75);
    }

    #[test]
    fn formats() {
        assert_eq!(format_speed(1.0), "1.0×");
        assert_eq!(format_speed(1.25), "1.25×");
        assert_eq!(format_speed(0.5), "0.5×");
    }
}
