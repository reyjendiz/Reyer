//! Where the window opens. Rects are (x, y, width, height) in physical pixels.
pub type Rect = [i32; 4];

pub const DEFAULT_SIZE: (i32, i32) = (960, 540);
pub const MIN_SIZE: (i32, i32) = (520, 300);

fn overlap(a: Rect, b: Rect) -> i64 {
    let w = (a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0]);
    let h = (a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1]);
    w.max(0) as i64 * h.max(0) as i64
}

pub fn centered(size: (i32, i32), area: Rect) -> Rect {
    let (w, h) = (size.0.min(area[2]), size.1.min(area[3]));
    [area[0] + (area[2] - w) / 2, area[1] + (area[3] - h) / 2, w, h]
}

/// The last windowed rect, kept on a visible work area. First launch: default size, centered.
pub fn restore_rect(saved: Option<Rect>, work_areas: &[Rect], primary: Rect) -> Rect {
    let Some([x, y, w, h]) = saved else {
        return centered(DEFAULT_SIZE, primary);
    };
    let (w, h) = (w.max(MIN_SIZE.0), h.max(MIN_SIZE.1));
    let rect = [x, y, w, h];
    let area = work_areas.iter().copied().max_by_key(|a| overlap(rect, *a)).unwrap_or(primary);
    if overlap(rect, area) == 0 {
        return centered((w, h), primary); // its monitor is gone
    }
    let (w, h) = (w.min(area[2]), h.min(area[3]));
    let x = x.clamp(area[0], area[0] + area[2] - w);
    let y = y.clamp(area[1], area[1] + area[3] - h);
    [x, y, w, h]
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: Rect = [0, 0, 1920, 1040];
    const SECOND: Rect = [1920, 0, 2560, 1400];

    #[test]
    fn first_launch_is_960x540_centered() {
        assert_eq!(restore_rect(None, &[PRIMARY], PRIMARY), [(1920 - 960) / 2, (1040 - 540) / 2, 960, 540]);
    }

    #[test]
    fn saved_rect_comes_back_identical() {
        let saved = [300, 200, 1100, 650];
        assert_eq!(restore_rect(Some(saved), &[PRIMARY, SECOND], PRIMARY), saved);
    }

    #[test]
    fn rect_on_second_monitor_stays_there() {
        let saved = [2100, 100, 1280, 720];
        assert_eq!(restore_rect(Some(saved), &[PRIMARY, SECOND], PRIMARY), saved);
    }

    #[test]
    fn monitor_removed_recenters_on_primary() {
        assert_eq!(restore_rect(Some([2100, 100, 1280, 720]), &[PRIMARY], PRIMARY), [(1920 - 1280) / 2, (1040 - 720) / 2, 1280, 720]);
    }

    #[test]
    fn partly_offscreen_is_pulled_into_the_work_area() {
        assert_eq!(restore_rect(Some([1700, 900, 800, 400]), &[PRIMARY], PRIMARY), [1120, 640, 800, 400]);
    }

    #[test]
    fn too_big_is_shrunk_and_too_small_grown() {
        assert_eq!(restore_rect(Some([0, 0, 5000, 3000]), &[PRIMARY], PRIMARY), PRIMARY);
        let r = restore_rect(Some([10, 10, 100, 50]), &[PRIMARY], PRIMARY);
        assert_eq!((r[2], r[3]), MIN_SIZE);
    }
}
