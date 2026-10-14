//! Volume curve: the UI percent is linear in amplitude, 100% is unity gain and the top third of the
//! range (up to 150%) is a +6 dB boost for quiet files. mpv's own `volume` is cubic, hence the cube root.
pub const MAX_PERCENT: f32 = 150.0;

pub fn amplitude(percent: f32) -> f32 {
    let p = percent.clamp(0.0, MAX_PERCENT);
    if p <= 100.0 {
        p / 100.0
    } else {
        1.0 + (p - 100.0) / (MAX_PERCENT - 100.0)
    }
}

pub fn to_mpv(percent: f32) -> f32 {
    100.0 * amplitude(percent).cbrt()
}

pub fn from_mpv(volume: f32) -> f32 {
    let a = (volume.max(0.0) / 100.0).powi(3);
    if a <= 1.0 {
        a * 100.0
    } else {
        100.0 + (a - 1.0) * (MAX_PERCENT - 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unity_gain_at_one_hundred() {
        assert!((amplitude(100.0) - 1.0).abs() < 1e-6);
        assert!((to_mpv(100.0) - 100.0).abs() < 1e-3);
    }

    #[test]
    fn the_curve_is_linear_below_unity() {
        assert!((amplitude(50.0) - 0.5).abs() < 1e-6);
        assert!((amplitude(25.0) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn boost_tops_out_at_plus_six_db() {
        let db = 20.0 * amplitude(150.0).log10();
        assert!((db - 6.02).abs() < 0.05);
        assert_eq!(amplitude(500.0), amplitude(150.0));
    }

    #[test]
    fn round_trip() {
        for p in [0.0, 1.0, 33.0, 80.0, 100.0, 125.0, 150.0] {
            assert!((from_mpv(to_mpv(p)) - p).abs() < 0.01, "{p}");
        }
    }

    #[test]
    fn half_is_minus_six_db_not_minus_eighteen() {
        let db = 20.0 * amplitude(50.0).log10();
        assert!((db - -6.02).abs() < 0.01);
        // mpv's own 50 would be -18 dB
        assert!((to_mpv(50.0) - 79.37).abs() < 0.01);
    }

    #[test]
    fn round_trip_wide() {
        for p in [0.0, 5.0, 37.5, 50.0, 99.0, 100.0, 120.0, MAX_PERCENT] {
            assert!((from_mpv(to_mpv(p)) - p).abs() < 0.01, "{p}");
        }
    }

    #[test]
    fn clamped() {
        assert_eq!(to_mpv(-10.0), 0.0);
        assert_eq!(to_mpv(1000.0), to_mpv(MAX_PERCENT));
    }
}
