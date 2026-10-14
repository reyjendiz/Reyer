//! Physical springs for every movement in the UI.
//!
//! The integrator is the exact closed-form solution of a damped harmonic oscillator (mass 1), so
//! twelve steps of 1/120 s equal one step of 0.1 s: animations are frame-rate independent and
//! carry their velocity through every interruption.
use crate::tokens::{tokens, SpringParams};

const EPSILON: f32 = 1e-3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preset {
    Smooth,
    Snappy,
    Bouncy,
    Follow,
}

pub fn params(p: Preset) -> SpringParams {
    let s = &tokens().springs;
    match p {
        Preset::Smooth => s.smooth,
        Preset::Snappy => s.snappy,
        Preset::Bouncy => s.bouncy,
        Preset::Follow => s.follow,
    }
}

/// Advance position `x` and velocity `v` towards `target` by `dt` seconds.
pub fn step(x: f64, v: f64, target: f64, p: SpringParams, dt: f64) -> (f64, f64) {
    let w = 2.0 * std::f64::consts::PI / p.response as f64;
    let z = p.damping_ratio as f64;
    let u0 = x - target;
    let (u, du) = if z < 1.0 {
        let wd = w * (1.0 - z * z).sqrt();
        let a = z * w;
        let b = (v + a * u0) / wd;
        let e = (-a * dt).exp();
        let (s, c) = (wd * dt).sin_cos();
        (e * (u0 * c + b * s), e * (v * c - (a * b + u0 * wd) * s))
    } else if z == 1.0 {
        let b = v + w * u0;
        let e = (-w * dt).exp();
        (e * (u0 + b * dt), e * (v - w * b * dt))
    } else {
        let r = (z * z - 1.0).sqrt();
        let (r1, r2) = (-w * (z - r), -w * (z + r));
        let c2 = (v - r1 * u0) / (r2 - r1);
        let c1 = u0 - c2;
        let (e1, e2) = ((r1 * dt).exp(), (r2 * dt).exp());
        (c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
    };
    (target + u, du)
}

/// Past a limit the value keeps following the pointer, but with growing resistance.
pub fn rubber_band(value: f32, low: f32, high: f32, dimension: f32) -> f32 {
    if (low..=high).contains(&value) {
        return value;
    }
    let limit = if value < low { low } else { high };
    let over = (value - limit).abs();
    let eased = (1.0 - 1.0 / (over * 0.55 / dimension.max(1.0) + 1.0)) * dimension;
    if value < low {
        limit - eased
    } else {
        limit + eased
    }
}

/// A value that follows `target` like a physical spring.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    pub preset: Preset,
    started: bool,
    moving: bool,
    /// Reduce Motion: a short, non-bouncing transition instead of the preset.
    quick: bool,
}

impl Spring {
    /// Starts at its first target without animating (declarative initial state).
    pub const fn new(preset: Preset) -> Self {
        Spring { value: 0.0, target: 0.0, velocity: 0.0, preset, started: false, moving: false, quick: false }
    }

    /// A spring that starts at `v` and animates from there to its first different target.
    pub const fn at(preset: Preset, v: f32) -> Self {
        Spring { value: v, target: v, velocity: 0.0, preset, started: true, moving: false, quick: false }
    }

    pub fn settled(&self) -> bool {
        !self.moving
    }

    /// Jump to `v` without animating (also clears the velocity).
    pub fn snap(&mut self, v: f32) {
        if !v.is_finite() {
            return;
        }
        self.value = v;
        self.target = v;
        self.velocity = 0.0;
        self.started = true;
        self.moving = false;
    }

    /// Kick the spring: a velocity that carries the value past its target and back.
    pub fn nudge(&mut self, velocity: f32) {
        if velocity.is_finite() {
            self.velocity = velocity;
            self.moving = true;
        }
    }

    pub fn set_target(&mut self, target: f32, reduce_motion: bool) {
        if !target.is_finite() {
            return; // a NaN from a not-yet-laid-out value would stick forever
        }
        if !self.started {
            self.snap(target);
            return;
        }
        // Reduce Motion keeps a brief critically damped change (like a short cross-fade), never a jump
        self.quick = reduce_motion;
        if target != self.target {
            self.target = target;
            self.moving = true;
        }
    }

    /// Advance by `dt`; returns the current value.
    pub fn tick(&mut self, dt: f32) -> f32 {
        if self.moving {
            let p = if self.quick { params(Preset::Follow) } else { params(self.preset) };
            let (x, v) = step(self.value as f64, self.velocity as f64, self.target as f64, p, dt as f64);
            let scale = self.target.abs().max(1.0);
            let near = (x as f32 - self.target).abs() < EPSILON * scale && (v as f32).abs() < EPSILON * 10.0 * scale;
            if near || !x.is_finite() || !v.is_finite() {
                self.value = self.target;
                self.velocity = 0.0;
                self.moving = false;
            } else {
                self.value = x as f32;
                self.velocity = v as f32;
            }
        }
        self.value
    }

    /// The usual per-frame call: aim at `target`, advance, report whether it is still moving.
    pub fn to(&mut self, target: f32, dt: f32, reduce_motion: bool, animating: &mut bool) -> f32 {
        self.set_target(target, reduce_motion);
        let v = self.tick(dt);
        if self.moving {
            *animating = true;
        }
        v
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ease {
    Linear,
    OutCubic,
    InOutSine,
}

impl Ease {
    fn apply(self, t: f32) -> f32 {
        match self {
            Ease::Linear => t,
            Ease::OutCubic => 1.0 - (1.0 - t).powi(3),
            Ease::InOutSine => 0.5 - 0.5 * (std::f32::consts::PI * t).cos(),
        }
    }
}

/// A timed transition (a "Behavior"): when the target changes it eases from the current value.
#[derive(Clone, Copy, Debug)]
pub struct Tween {
    pub value: f32,
    from: f32,
    to: f32,
    elapsed: f32,
    dur: f32,
    ease: Ease,
}

impl Tween {
    pub const fn new(v: f32) -> Tween {
        Tween { value: v, from: v, to: v, elapsed: 0.0, dur: 0.0, ease: Ease::OutCubic }
    }

    pub fn done(&self) -> bool {
        self.value == self.to
    }

    pub fn snap(&mut self, v: f32) {
        *self = Tween::new(v);
    }

    /// Eases towards `target` over `ms` (`ms` is already adjusted for reduced motion by the caller).
    pub fn to(&mut self, target: f32, ms: f32, ease: Ease, dt: f32, animating: &mut bool) -> f32 {
        if !target.is_finite() {
            return self.value;
        }
        if target != self.to {
            self.from = self.value;
            self.to = target;
            self.elapsed = 0.0;
            self.dur = ms / 1000.0;
            self.ease = ease;
        }
        if self.value != self.to {
            self.elapsed += dt;
            let t = if self.dur <= 0.0 { 1.0 } else { (self.elapsed / self.dur).clamp(0.0, 1.0) };
            self.value = if t >= 1.0 { self.to } else { self.from + (self.to - self.from) * self.ease.apply(t) };
            if self.value != self.to {
                *animating = true;
            }
        }
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tween_reaches_its_target_and_reports_motion() {
        let mut t = Tween::new(0.0);
        let mut anim = false;
        t.to(1.0, 200.0, Ease::OutCubic, 0.05, &mut anim);
        assert!(anim && t.value > 0.0 && t.value < 1.0);
        for _ in 0..10 {
            t.to(1.0, 200.0, Ease::OutCubic, 0.05, &mut anim);
        }
        assert_eq!(t.value, 1.0);
        anim = false;
        t.to(1.0, 200.0, Ease::OutCubic, 0.05, &mut anim);
        assert!(!anim);
    }

    const SNAPPY: SpringParams = SpringParams { response: 0.35, damping_ratio: 0.85 };
    const SMOOTH: SpringParams = SpringParams { response: 0.45, damping_ratio: 1.0 };
    const OVER: SpringParams = SpringParams { response: 0.4, damping_ratio: 1.4 };

    fn run(p: SpringParams, mut x: f64, mut v: f64, target: f64, dt: f64, steps: usize) -> (f64, f64) {
        for _ in 0..steps {
            (x, v) = step(x, v, target, p, dt);
        }
        (x, v)
    }

    #[test]
    fn presets_come_from_the_design_tokens() {
        assert_eq!(params(Preset::Smooth), SpringParams { response: 0.45, damping_ratio: 1.0 });
        assert_eq!(params(Preset::Snappy), SNAPPY);
        assert_eq!(params(Preset::Bouncy), SpringParams { response: 0.5, damping_ratio: 0.7 });
    }

    #[test]
    fn frame_rate_independent() {
        for p in [SNAPPY, SMOOTH, OVER] {
            let at60 = run(p, 0.0, 3.0, 1.0, 1.0 / 60.0, 30);
            let at120 = run(p, 0.0, 3.0, 1.0, 1.0 / 120.0, 60);
            let once = run(p, 0.0, 3.0, 1.0, 0.5, 1);
            assert!((at60.0 - at120.0).abs() < 1e-9 && (at60.0 - once.0).abs() < 1e-9);
            assert!((at60.1 - at120.1).abs() < 1e-9 && (at60.1 - once.1).abs() < 1e-9);
        }
    }

    #[test]
    fn critically_damped_never_overshoots() {
        let (mut x, mut v, mut peak) = (0.0, 0.0, 0.0f64);
        for _ in 0..200 {
            (x, v) = step(x, v, 1.0, SMOOTH, 1.0 / 120.0);
            peak = peak.max(x);
        }
        assert!(peak <= 1.0 + 1e-9);
    }

    #[test]
    fn underdamped_overshoots_then_settles() {
        let (mut x, mut v, mut peak) = (0.0, 0.0, 0.0f64);
        for _ in 0..240 {
            (x, v) = step(x, v, 1.0, SNAPPY, 1.0 / 120.0);
            peak = peak.max(x);
        }
        assert!(peak > 1.0);
        assert!((x - 1.0).abs() < 1e-3);
    }

    #[test]
    fn velocity_carries_through_an_interruption() {
        let (x, v) = run(SNAPPY, 0.0, 0.0, 1.0, 1.0 / 120.0, 12);
        assert!(v > 0.0);
        let (x2, _) = step(x, v, 0.0, SNAPPY, 1.0 / 120.0);
        assert!(x2 > x, "still moving forward right after the turn");
    }

    #[test]
    fn rubber_band_resists() {
        assert_eq!(rubber_band(50.0, 0.0, 100.0, 20.0), 50.0);
        let over = rubber_band(150.0, 0.0, 100.0, 20.0);
        assert!(over > 100.0 && over < 120.0);
        assert!(rubber_band(1000.0, 0.0, 100.0, 20.0) < 120.0);
        let under = rubber_band(-50.0, 0.0, 100.0, 20.0);
        assert!(under > -20.0 && under < 0.0);
    }

    #[test]
    fn ignores_non_finite_input_and_settles() {
        let mut s = Spring::new(Preset::Snappy);
        let mut anim = false;
        s.to(f32::NAN, 0.016, false, &mut anim);
        assert!(s.value.is_finite() && !anim);
        s.snap(0.0);
        s.to(1.0, 0.016, false, &mut anim);
        assert!(anim);
        for _ in 0..600 {
            s.to(1.0, 1.0 / 60.0, false, &mut anim);
        }
        assert_eq!(s.value, 1.0);
        assert!(s.settled());
    }

    #[test]
    fn first_target_does_not_animate() {
        let mut s = Spring::new(Preset::Smooth);
        let mut anim = false;
        assert_eq!(s.to(5.0, 0.016, false, &mut anim), 5.0);
        assert!(!anim);
    }

    #[test]
    fn reduce_motion_is_short_and_never_overshoots() {
        let mut s = Spring::at(Preset::Bouncy, 0.0);
        let mut anim = false;
        let first = s.to(1.0, 1.0 / 60.0, true, &mut anim);
        assert!(anim && first > 0.0 && first < 1.0, "still a transition, not a jump");
        let mut peak = first;
        for _ in 0..15 {
            peak = peak.max(s.to(1.0, 1.0 / 60.0, true, &mut anim));
        }
        assert!(peak <= 1.0 + 1e-6);
        assert!((s.value - 1.0).abs() < 0.01, "done within a quarter second");
    }
}
