//! How things move: the one motion every transition on the desktop follows.
//!
//! `roadmap-detailed.md` → *Tier 3 — Animation Tuning* asks a theme to set how
//! long the desktop's transitions take (`animation-duration-ms`), the curve
//! they follow (`animation-easing`: ease-out, spring or linear) and whether
//! there are any at all (`enable-animations`). The user has a say as well:
//! the animation speed in `appearance.yaml` -- Off, Fast, Normal or Slow. This
//! module is the two read together, as one value, a [`Motion`], which a
//! [`Palette`] carries to everything that draws ([`Palette::motion`]) as it
//! carries the widget style -- so a program that animates needs no new
//! argument to follow it, and every application is handed it with its
//! colours. `design-decisions.md` §1446 has the reasoning.
//!
//! # Stated against a standard
//!
//! Each transition keeps the length it was designed at -- the overview's
//! fade, the taskbar's slide, a notification sliding in -- stated against a
//! standard transition of [`Motion::STANDARD_MS`]. A motion scales them all
//! together ([`Motion::duration_ms`]): a theme whose standard is 300 ms makes
//! every one half again as long, and a user who picks Slow makes them longer
//! still. So one setting speeds up or slows down the whole desktop, and a
//! theme cannot make the taskbar's slide outlast the overview's fade where
//! they were designed the other way round.
//!
//! # Arriving and leaving
//!
//! A curve is read one way for something coming into view or into place
//! ([`Motion::arriving`]) and another for something going away
//! ([`Motion::leaving`]), because the two want different shapes: an arrival
//! that slows into place looks placed, and a departure that slows at the end
//! looks reluctant. Under [`Curve::EaseOut`] -- the built-in theme's --
//! arrivals decelerate into place and departures accelerate away. Under
//! [`Curve::Spring`] arrivals overshoot a little and settle, and departures
//! are ease-out's: nothing is gained by overshooting "gone". Under
//! [`Curve::Linear`] both are a constant speed.
//!
//! **A spring's arrival passes its destination** on the way: `arriving`
//! returns up to about 1.09 before it settles on exactly 1. That is the
//! curve, not an error, and it is the caller's to use or to clamp -- a toast
//! floating on the desktop can overshoot; a panel anchored to the screen's
//! edge clamps at 1, or it would open a gap between itself and the edge; an
//! opacity clamps, as there is nothing past opaque.
//!
//! # What a motion does not choose
//!
//! - **Whether a busy indicator turns.** A spinner says the machine is
//!   working. It is state, not a transition, and a user who turned the
//!   desktop's animations off has not asked to stop being told.
//! - **Where anything ends up.** A motion times the way there; the
//!   destination is the same under every one, and a still motion goes
//!   straight to it.
//! - **How long something stays.** A notification's time on screen, a
//!   tooltip's delay and a hide's delay are waits, not motion, and are the
//!   user's settings of their own.
//!
//! [`Palette`]: crate::palette::Palette
//! [`Palette::motion`]: crate::palette::Palette::motion

/// The shape of a transition. See the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Curve {
    /// Arrivals decelerate into place, departures accelerate away. The
    /// built-in theme's.
    EaseOut,
    /// A constant speed, both ways.
    Linear,
    /// Arrivals overshoot a little and settle, as on a spring; departures
    /// are [`EaseOut`](Self::EaseOut)'s.
    Spring,
}

impl Curve {
    /// Every curve, in the order a list offers them.
    pub const ALL: [Self; 3] = [Self::EaseOut, Self::Linear, Self::Spring];

    /// The name a theme writes it by: `ease-out`, `linear` or `spring`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::EaseOut => "ease-out",
            Self::Linear => "linear",
            Self::Spring => "spring",
        }
    }

    /// The curve a theme's name names, if any.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|curve| curve.name() == name)
    }
}

/// How the desktop's transitions move: how long the standard one takes, and
/// along which [`Curve`] -- or that nothing moves at all
/// ([`STILL`](Self::STILL)). See the module docs.
///
/// Whole milliseconds, for the reason the widget style keeps whole pixels: a
/// [`Palette`](crate::palette::Palette) carries it and is compared for
/// equality, so it must be `Eq`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Motion {
    /// How long the standard transition takes; zero is still.
    standard_ms: u16,
    /// The curve every transition follows.
    curve: Curve,
}

impl Default for Motion {
    fn default() -> Self {
        Self::STANDARD
    }
}

impl Motion {
    /// The length every transition's own is stated against: the built-in
    /// theme's standard transition.
    pub const STANDARD_MS: u16 = 200;

    /// The shortest standard transition a theme may ask for. Shorter reads as
    /// a jump that stutters, and a theme that wants no motion says so
    /// (`enabled: false`) rather than asking for a very fast one.
    pub const MIN_MS: u16 = 50;

    /// The longest standard transition a theme may ask for. Past a second the
    /// desktop is in the user's way: every panel keeps them waiting to use
    /// it. (A user may still slow it further with their own speed setting.)
    pub const MAX_MS: u16 = 1000;

    /// The built-in theme's motion: 200 ms, ease-out.
    pub const STANDARD: Self = Self {
        standard_ms: Self::STANDARD_MS,
        curve: Curve::EaseOut,
    };

    /// Nothing moves: every transition is over before it starts.
    pub const STILL: Self = Self {
        standard_ms: 0,
        curve: Curve::EaseOut,
    };

    /// A motion whose standard transition takes `standard_ms` along `curve`;
    /// zero milliseconds is [`STILL`](Self::STILL).
    ///
    /// Not held to [`MIN_MS`](Self::MIN_MS)..=[`MAX_MS`](Self::MAX_MS): those
    /// bound what a *theme* may ask, and the theme's reader holds it to them;
    /// a user's speed setting may then take it past either.
    #[must_use]
    pub const fn new(standard_ms: u16, curve: Curve) -> Self {
        if standard_ms == 0 {
            Self::STILL
        } else {
            Self { standard_ms, curve }
        }
    }

    /// Whether nothing moves.
    #[must_use]
    pub const fn is_still(self) -> bool {
        self.standard_ms == 0
    }

    /// How long the standard transition takes; zero when still.
    #[must_use]
    pub const fn standard_ms(self) -> u16 {
        self.standard_ms
    }

    /// The curve every transition follows.
    #[must_use]
    pub const fn curve(self) -> Curve {
        self.curve
    }

    /// This motion at the user's speed: `factor` times as long -- 0.75 for
    /// Fast, 1.5 for Slow. Zero, a negative factor or one that is not a
    /// number is still: it is Off's, and a hand-edited file's NaN must not
    /// become a motion that never ends.
    ///
    /// A motion that moves still moves however fast it is asked to: the
    /// result is at least a millisecond, never zero, which would read as
    /// still.
    #[must_use]
    pub fn at_speed(self, factor: f32) -> Self {
        if self.is_still() || !factor.is_finite() || factor <= 0.0 {
            return Self::STILL;
        }
        let scaled = (f32::from(self.standard_ms) * factor).round();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to 1..=u16::MAX first, so it fits and is positive"
        )]
        let standard_ms = scaled.clamp(1.0, f32::from(u16::MAX)) as u16;
        Self {
            standard_ms,
            ..self
        }
    }

    /// How long a transition designed to take `stated_ms` against the
    /// standard takes under this motion; zero when still.
    ///
    /// Rounded to the nearest millisecond, and never zero for a transition
    /// that has a length and a motion that moves -- a zero would read as
    /// "still" to a caller, which is not what a very short motion meant.
    #[must_use]
    pub fn duration_ms(self, stated_ms: u32) -> u32 {
        if self.is_still() || stated_ms == 0 {
            return 0;
        }
        // Half the standard added before dividing by it: to the nearest.
        const HALF_STANDARD: u64 = Motion::STANDARD_MS as u64 / 2;
        let scaled = u64::from(stated_ms)
            .saturating_mul(u64::from(self.standard_ms))
            .saturating_add(HALF_STANDARD)
            .checked_div(u64::from(Self::STANDARD_MS))
            // The standard is a nonzero constant; this is never taken.
            .unwrap_or(u64::MAX);
        u32::try_from(scaled).unwrap_or(u32::MAX).max(1)
    }

    /// Where something coming into view or into place is, `t` of the way
    /// through its transition: 0 at the start, 1 at the end.
    ///
    /// Under [`Curve::Spring`] it passes 1 on the way -- see the module
    /// docs. `t` is held to 0..=1; one that is not a number is the end, and
    /// under a still motion everything is at its end.
    #[must_use]
    pub fn arriving(self, t: f32) -> f32 {
        let Some(t) = progress(self, t) else {
            return 1.0;
        };
        match self.curve {
            // Cubic: quick to arrive, gentle into place.
            Curve::EaseOut => {
                let rest = 1.0 - t;
                1.0 - rest * rest * rest
            }
            Curve::Linear => t,
            Curve::Spring => spring(t),
        }
    }

    /// Where something going away is, `t` of the way through its transition:
    /// 0 where it started, 1 gone. Never outside 0..=1, under any curve.
    ///
    /// `t` is held to 0..=1; one that is not a number is the end, and under a
    /// still motion everything is at its end.
    #[must_use]
    pub fn leaving(self, t: f32) -> f32 {
        let Some(t) = progress(self, t) else {
            return 1.0;
        };
        match self.curve {
            // Quadratic: away without lingering. A cubic start is so slow
            // that a closing panel seems to hesitate.
            Curve::EaseOut | Curve::Spring => t * t,
            Curve::Linear => t,
        }
    }

    /// When, in an arrival, something first reaches `place` -- the inverse
    /// of [`arriving`](Self::arriving).
    ///
    /// For turning a departure around part-way without a jump: a panel half
    /// gone that is called back must start its arrival from where it is
    /// *drawn*, and under an asymmetric curve that is not where the
    /// departure's clock says -- half-way through leaving is a quarter gone
    /// under ease-out, and half-way through arriving is seven-eighths there.
    /// `place` is held to 0..=1. A spring reaches 1 early, on its way past,
    /// and the first time is the answer. Under a still motion, or for a place
    /// that is not a number, the end: 1.
    #[must_use]
    pub fn when_arriving_at(self, place: f32) -> f32 {
        let Some(place) = progress(self, place) else {
            return 1.0;
        };
        match self.curve {
            Curve::EaseOut => 1.0 - (1.0 - place).cbrt(),
            Curve::Linear => place,
            Curve::Spring => first_time(spring, place, SPRING_FIRST_AT_ONE),
        }
    }

    /// When, in a departure, something reaches `place` -- the inverse of
    /// [`leaving`](Self::leaving), for turning an arrival around part-way;
    /// see [`when_arriving_at`](Self::when_arriving_at).
    #[must_use]
    pub fn when_leaving_at(self, place: f32) -> f32 {
        let Some(place) = progress(self, place) else {
            return 1.0;
        };
        match self.curve {
            Curve::EaseOut | Curve::Spring => place.sqrt(),
            Curve::Linear => place,
        }
    }
}

/// When [`spring`] first reaches 1: where cos(2.5 pi t) first reaches
/// zero. It rises all the way there, which is what lets [`first_time`] find
/// any place below it by halving.
const SPRING_FIRST_AT_ONE: f32 = 0.2;

/// The first `t` in 0..=`end` at which `curve`, rising over that span from 0
/// to at least 1, reaches `place` -- by halving the span, to well under a
/// millisecond of any transition.
fn first_time(curve: fn(f32) -> f32, place: f32, end: f32) -> f32 {
    let (mut low, mut high) = (0.0_f32, end);
    for _ in 0..32 {
        let mid = f32::midpoint(low, high);
        if curve(mid) < place {
            low = mid;
        } else {
            high = mid;
        }
    }
    high
}

/// `t` held to 0..=1, or none where the transition is simply at its end:
/// the motion is still, or `t` is not a number.
fn progress(motion: Motion, t: f32) -> Option<f32> {
    if motion.is_still() || t.is_nan() {
        None
    } else {
        Some(t.clamp(0.0, 1.0))
    }
}

/// A damped spring's way to 1: `1 - e^(-7t) cos(2.5 pi t)`.
///
/// Chosen for three properties, each a test: it starts at 0; it passes 1
/// once, by under a tenth (8.7%, near t = 0.3), and comes back to within a
/// hair of it; and it is exactly 1 at t = 1, where cos(2.5 pi) is zero -- so
/// the transition ends where it was going, rather than a hair short of it,
/// with no jump. (The end is taken as exactly 1 rather than computed, where
/// f32's cosine is a hair off zero.) Damped at 7 rather than less because at
/// 6 the overshoot is 12%, which reads as a bounce rather than a settle.
fn spring(t: f32) -> f32 {
    if t >= 1.0 {
        return 1.0;
    }
    1.0 - (-7.0 * t).exp() * (2.5 * core::f32::consts::PI * t).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every motion that moves, one per curve.
    fn moving() -> [Motion; 3] {
        Curve::ALL.map(|curve| Motion::new(Motion::STANDARD_MS, curve))
    }

    /// Samples of 0..=1, ends included.
    fn samples() -> impl Iterator<Item = f32> {
        (0..=100u8).map(|i| f32::from(i) / 100.0)
    }

    /// **Every curve starts at its start and ends at its end**, both ways.
    #[test]
    fn every_curve_runs_from_zero_to_one() {
        for motion in moving() {
            assert!(motion.arriving(0.0).abs() < 1e-6, "{motion:?}");
            assert!(motion.leaving(0.0).abs() < 1e-6, "{motion:?}");
            assert!((motion.arriving(1.0) - 1.0).abs() < 1e-6, "{motion:?}");
            assert!((motion.leaving(1.0) - 1.0).abs() < 1e-6, "{motion:?}");
        }
    }

    /// **Ease-out arrives quickly and settles gently; it leaves slowly and
    /// goes quickly** -- and a linear motion is at its time.
    #[test]
    fn ease_out_decelerates_in_and_accelerates_away() {
        let ease = Motion::STANDARD;
        assert!((ease.arriving(0.5) - 0.875).abs() < 1e-6);
        assert!((ease.leaving(0.5) - 0.25).abs() < 1e-6);
        let linear = Motion::new(200, Curve::Linear);
        for t in samples() {
            assert!((linear.arriving(t) - t).abs() < 1e-6);
            assert!((linear.leaving(t) - t).abs() < 1e-6);
        }
    }

    /// **Only a spring's arrival passes its destination**, once, by under a
    /// tenth; every departure, and every other arrival, stays in 0..=1 and
    /// never goes back.
    #[test]
    fn only_a_springs_arrival_overshoots() {
        let spring = Motion::new(200, Curve::Spring);
        let peak = samples().map(|t| spring.arriving(t)).fold(0.0, f32::max);
        assert!(peak > 1.08 && peak < 1.09, "{peak}");
        assert!((spring.arriving(0.31) - peak).abs() < 0.001, "{peak}");
        // Back to within a hair of 1 well before the end.
        assert!((spring.arriving(0.9) - 1.0).abs() < 0.01);
        for motion in moving() {
            let mut last = 0.0;
            for t in samples() {
                let away = motion.leaving(t);
                assert!((0.0..=1.0).contains(&away), "{motion:?} {t}");
                assert!(away >= last, "{motion:?} {t}");
                last = away;
            }
            if motion.curve() == Curve::Spring {
                continue;
            }
            let mut last = 0.0;
            for t in samples() {
                let here = motion.arriving(t);
                assert!((0.0..=1.0).contains(&here), "{motion:?} {t}");
                assert!(here >= last, "{motion:?} {t}");
                last = here;
            }
        }
    }

    /// **Past either end, or not a number, a transition is at an end** -- and
    /// under a still motion every transition is already over.
    #[test]
    fn out_of_range_and_still_are_at_an_end() {
        for motion in moving() {
            assert!(motion.arriving(-1.0).abs() < 1e-6);
            assert!((motion.arriving(2.0) - 1.0).abs() < 1e-6);
            assert!((motion.leaving(2.0) - 1.0).abs() < 1e-6);
            assert!((motion.arriving(f32::NAN) - 1.0).abs() < 1e-6);
            assert!((motion.leaving(f32::NAN) - 1.0).abs() < 1e-6);
        }
        for t in [0.0, 0.3, 1.0] {
            assert!((Motion::STILL.arriving(t) - 1.0).abs() < 1e-6);
            assert!((Motion::STILL.leaving(t) - 1.0).abs() < 1e-6);
        }
    }

    /// **Where something is, read back as when it was there**: each inverse
    /// undoes its curve, so a transition turned around part-way starts from
    /// where it was drawn.
    #[test]
    fn the_inverses_undo_their_curves() {
        for motion in moving() {
            for place in samples() {
                let back = motion.arriving(motion.when_arriving_at(place));
                assert!((back - place).abs() < 1e-4, "{motion:?} {place} {back}");
                let back = motion.leaving(motion.when_leaving_at(place));
                assert!((back - place).abs() < 1e-4, "{motion:?} {place} {back}");
            }
        }
        // Half-way through leaving is a quarter gone under ease-out; called
        // back, the arrival starts three-quarters there.
        let ease = Motion::STANDARD;
        let there = 1.0 - ease.leaving(0.5);
        let t = ease.when_arriving_at(there);
        assert!((ease.arriving(t) - 0.75).abs() < 1e-5);
        // A spring's first time at 1 is on its way past, not its end.
        let spring = Motion::new(200, Curve::Spring);
        assert!((spring.when_arriving_at(1.0) - 0.2).abs() < 1e-3);
        // Still, or not a number: the end.
        assert!((Motion::STILL.when_arriving_at(0.5) - 1.0).abs() < 1e-6);
        assert!((Motion::STILL.when_leaving_at(0.5) - 1.0).abs() < 1e-6);
        assert!((ease.when_arriving_at(f32::NAN) - 1.0).abs() < 1e-6);
        assert!((ease.when_leaving_at(2.0) - 1.0).abs() < 1e-6);
        assert!(ease.when_leaving_at(-1.0).abs() < 1e-6);
    }

    /// **Every transition's length scales with the standard's**, rounded, and
    /// is nothing when still.
    #[test]
    fn durations_scale_with_the_standard() {
        assert_eq!(Motion::STANDARD.duration_ms(150), 150);
        assert_eq!(Motion::new(300, Curve::EaseOut).duration_ms(150), 225);
        assert_eq!(Motion::new(100, Curve::Linear).duration_ms(300), 150);
        // Rounded to the nearest: 3 * 250 / 200 is 3.75, and 1 * 50 / 200 a
        // quarter -- which is still a millisecond, not nothing.
        assert_eq!(Motion::new(250, Curve::Linear).duration_ms(3), 4);
        assert_eq!(Motion::new(250, Curve::Linear).duration_ms(1), 1);
        assert_eq!(Motion::new(50, Curve::Linear).duration_ms(3), 1);
        assert_eq!(Motion::new(50, Curve::Linear).duration_ms(1), 1);
        assert_eq!(Motion::STILL.duration_ms(150), 0);
        assert_eq!(Motion::STANDARD.duration_ms(0), 0);
        assert_eq!(
            Motion::new(u16::MAX, Curve::Linear).duration_ms(u32::MAX),
            u32::MAX
        );
    }

    /// **The user's speed scales the standard**; Off, and anything that is
    /// not a speed, is still; and a motion that moves is never scaled to
    /// still.
    #[test]
    fn a_speed_scales_the_standard() {
        assert_eq!(Motion::STANDARD.at_speed(0.75).standard_ms(), 150);
        assert_eq!(Motion::STANDARD.at_speed(1.5).standard_ms(), 300);
        assert_eq!(Motion::STANDARD.at_speed(1.0), Motion::STANDARD);
        let spring = Motion::new(1000, Curve::Spring).at_speed(1.5);
        assert_eq!(spring.standard_ms(), 1500);
        assert_eq!(spring.curve(), Curve::Spring);
        for off in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(Motion::STANDARD.at_speed(off).is_still(), "{off}");
        }
        assert!(Motion::STILL.at_speed(1.5).is_still());
        assert_eq!(Motion::STANDARD.at_speed(0.0001).standard_ms(), 1);
        assert_eq!(Motion::STANDARD.at_speed(1e9).standard_ms(), u16::MAX);
    }

    /// **Zero milliseconds is still**, whatever the curve; the default is
    /// the standard.
    #[test]
    fn zero_is_still() {
        assert_eq!(Motion::new(0, Curve::Spring), Motion::STILL);
        assert!(!Motion::new(1, Curve::Spring).is_still());
        assert_eq!(Motion::default(), Motion::STANDARD);
        assert_eq!(Motion::STANDARD.curve(), Curve::EaseOut);
    }

    /// **A curve's name reads back as the curve**, and nothing else does.
    #[test]
    fn a_curves_name_reads_back() {
        for curve in Curve::ALL {
            assert_eq!(Curve::from_name(curve.name()), Some(curve));
        }
        assert_eq!(Curve::from_name("Ease-Out"), None);
        assert_eq!(Curve::from_name("bounce"), None);
    }
}
