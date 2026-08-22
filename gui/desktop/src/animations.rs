//! Animation and transition system for the desktop shell.
//!
//! Provides smooth animations for window operations, desktop transitions,
//! and UI element state changes. All animations respect the `reduced_motion`
//! accessibility setting.
//!
//! Durations are **wall-clock milliseconds**, and every animation is advanced
//! by the elapsed time reported by the frame clock
//! ([`oswindow::EventLoop`]'s `Event::Tick { elapsed_ms }`), not by counting
//! frames.
//!
//! This module used to count ticks — "one tick = one frame", a fixed number of
//! ticks per animation — which made every duration a function of how often the
//! loop happened to wake. That is defensible only when the loop wakes at a
//! fixed rate, and ours deliberately does not: it parks until the next deadline
//! and a deadline can be late (see `design-decisions.md` §521). Under a
//! frame-counted scheme a busy moment does not drop frames, it *slows the
//! animation down* — the symptom is a menu that opens sluggishly exactly when
//! the machine is loaded, and no measurement anywhere would show it. Counting
//! milliseconds instead means a late frame is a bigger step, which is what
//! every consumer of `elapsed_ms` in the tree already assumes.
//!
//! It is also what the settings design asks for: the Tier 3 theme knob is
//! `animation-duration-ms`, which is not expressible in frames.

use guitk::color::Color;
use guitk::render::RenderCommand;
use guitk::style::CornerRadii;

// ============================================================================
// Easing functions
// ============================================================================

/// Easing function type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Easing {
    /// Linear interpolation (no easing).
    Linear,
    /// Slow start, fast end.
    EaseIn,
    /// Fast start, slow end.
    EaseOut,
    /// Slow start and end.
    EaseInOut,
    /// Bounce at the end.
    Bounce,
    /// Overshoot then settle.
    Elastic,
    /// Accelerate from zero.
    QuadraticIn,
    /// Decelerate to zero.
    QuadraticOut,
    /// Accelerate then decelerate.
    CubicInOut,
}

impl Easing {
    /// Apply the easing function to a normalized progress value (0.0 to 1.0).
    pub fn apply(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t,
            Self::EaseOut => t * (2.0 - t),
            Self::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    -1.0 + (4.0 - 2.0 * t) * t
                }
            }
            Self::Bounce => {
                // Simplified bounce: three bounces.
                let t2 = if t < 0.3636 {
                    7.5625 * t * t
                } else if t < 0.7273 {
                    let t2 = t - 0.5455;
                    7.5625 * t2 * t2 + 0.75
                } else if t < 0.9091 {
                    let t2 = t - 0.8182;
                    7.5625 * t2 * t2 + 0.9375
                } else {
                    let t2 = t - 0.9545;
                    7.5625 * t2 * t2 + 0.984375
                };
                t2.clamp(0.0, 1.0)
            }
            Self::Elastic => {
                if t <= 0.0 || t >= 1.0 {
                    return t;
                }
                // Simplified elastic using sine approximation.
                let p = 0.3;
                let s = p / 4.0;
                let t1 = t - 1.0;
                let pow = 2.0_f32.powf(10.0 * t1);
                // Use a crude sine approximation to avoid std dependency issues.
                let angle = (t1 - s) / p * core::f32::consts::TAU;
                let sine = sine_approx(angle);
                (1.0 - pow * sine).clamp(0.0, 1.2)
            }
            Self::QuadraticIn => t * t,
            Self::QuadraticOut => -t * (t - 2.0),
            Self::CubicInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let f = 2.0 * t - 2.0;
                    0.5 * f * f * f + 1.0
                }
            }
        }
    }
}

/// Bhaskara I sine approximation (avoids pulling in libm for no_std compat).
fn sine_approx(x: f32) -> f32 {
    // Normalize to [0, 2*PI).
    let pi = core::f32::consts::PI;
    let two_pi = core::f32::consts::TAU;
    let mut x = x % two_pi;
    if x < 0.0 {
        x += two_pi;
    }

    let sign = if x > pi { -1.0 } else { 1.0 };
    let x = if x > pi { x - pi } else { x };

    // Bhaskara I: sin(x) ≈ 16x(π-x) / (5π² - 4x(π-x))
    let num = 16.0 * x * (pi - x);
    let den = 5.0 * pi * pi - 4.0 * x * (pi - x);
    if den.abs() < 0.0001 {
        return 0.0;
    }
    sign * num / den
}

// ============================================================================
// Animation primitives
// ============================================================================

/// A single property animation.
#[derive(Debug, Clone)]
pub struct Animation {
    /// Start value.
    pub from: f32,
    /// End value.
    pub to: f32,
    /// Duration in milliseconds. Never zero — see [`Animation::new`].
    pub duration_ms: u32,
    /// Milliseconds elapsed within the current pass.
    pub elapsed_ms: u32,
    /// Easing function.
    pub easing: Easing,
    /// Whether the animation is running.
    pub active: bool,
    /// Whether to auto-reverse (ping-pong).
    pub auto_reverse: bool,
    /// Whether to loop.
    pub looping: bool,
    /// Direction (false = forward, true = reversing).
    reversing: bool,
}

impl Animation {
    /// Create a new animation lasting `duration_ms` milliseconds.
    ///
    /// A zero duration is raised to 1 ms rather than rejected: the caller is
    /// usually a settings value or an arithmetic result, and an animation that
    /// finishes on its first frame is what "no duration" should mean. Zero
    /// itself cannot be stored because it is the one value that makes
    /// `progress` undefined.
    #[must_use]
    pub fn new(from: f32, to: f32, duration_ms: u32, easing: Easing) -> Self {
        Self {
            from,
            to,
            duration_ms: duration_ms.max(1),
            elapsed_ms: 0,
            easing,
            active: true,
            auto_reverse: false,
            looping: false,
            reversing: false,
        }
    }

    /// Create a looping animation.
    #[must_use]
    pub fn looping(from: f32, to: f32, duration_ms: u32, easing: Easing) -> Self {
        let mut anim = Self::new(from, to, duration_ms, easing);
        anim.looping = true;
        anim.auto_reverse = true;
        anim
    }

    /// Advance by `dt_ms` milliseconds of wall time. Returns the interpolated
    /// value at the new position.
    ///
    /// `dt_ms` is whatever the frame clock measured, so it is neither constant
    /// nor small: a frame that arrived late steps further, which is the whole
    /// point of measuring rather than counting. A step long enough to cross the
    /// end of a looping pass carries its remainder into the next one instead of
    /// restarting from zero, so a loop cannot lose time by being interrupted.
    pub fn tick(&mut self, dt_ms: u32) -> f32 {
        if !self.active {
            return self.value();
        }

        self.elapsed_ms = self.elapsed_ms.saturating_add(dt_ms);

        // A single step can span several passes if the loop was blocked for a
        // long time. `duration_ms` is never zero, so this terminates; the
        // `saturating_sub` is belt and braces rather than a real case.
        while self.elapsed_ms >= self.duration_ms {
            if self.auto_reverse && !self.reversing {
                self.reversing = true;
            } else if self.looping {
                self.reversing = false;
            } else {
                self.active = false;
                self.elapsed_ms = self.duration_ms;
                return self.value();
            }
            self.elapsed_ms = self.elapsed_ms.saturating_sub(self.duration_ms);
        }

        self.value()
    }

    /// Get current value without advancing.
    #[must_use]
    pub fn value(&self) -> f32 {
        if !self.active {
            return if self.reversing { self.from } else { self.to };
        }
        let eased = self.easing.apply(self.progress());
        if self.reversing {
            self.to + (self.from - self.to) * eased
        } else {
            self.from + (self.to - self.from) * eased
        }
    }

    /// Whether the animation has completed.
    #[must_use]
    pub const fn is_done(&self) -> bool {
        !self.active
    }

    /// Reset the animation to the beginning.
    pub const fn reset(&mut self) {
        self.elapsed_ms = 0;
        self.active = true;
        self.reversing = false;
    }

    /// Normalized progress (0.0 to 1.0) through the current pass.
    #[must_use]
    pub fn progress(&self) -> f32 {
        if self.duration_ms == 0 {
            return 1.0;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a duration large enough to lose f32 precision is 97 days"
        )]
        let p = self.elapsed_ms as f32 / self.duration_ms as f32;
        p.clamp(0.0, 1.0)
    }
}

// ============================================================================
// Color animation
// ============================================================================

/// Animate between two colors.
#[derive(Debug, Clone)]
pub struct ColorAnimation {
    pub from: Color,
    pub to: Color,
    pub anim: Animation,
}

impl ColorAnimation {
    /// Create a colour animation lasting `duration_ms` milliseconds.
    #[must_use]
    pub fn new(from: Color, to: Color, duration_ms: u32, easing: Easing) -> Self {
        Self {
            from,
            to,
            anim: Animation::new(0.0, 1.0, duration_ms, easing),
        }
    }

    /// Advance by `dt_ms` milliseconds and return the current colour.
    pub fn tick(&mut self, dt_ms: u32) -> Color {
        let t = self.anim.tick(dt_ms);
        lerp_color(self.from, self.to, t)
    }

    /// The current colour, without advancing.
    #[must_use]
    pub fn value(&self) -> Color {
        lerp_color(self.from, self.to, self.anim.value())
    }

    /// Whether the animation has completed.
    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.anim.is_done()
    }

    /// Restart from the beginning.
    pub const fn reset(&mut self) {
        self.anim.reset();
    }
}

/// Linearly interpolate between two colors.
fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    Color::rgba(
        (a.r as f32 * inv + b.r as f32 * t) as u8,
        (a.g as f32 * inv + b.g as f32 * t) as u8,
        (a.b as f32 * inv + b.b as f32 * t) as u8,
        (a.a as f32 * inv + b.a as f32 * t) as u8,
    )
}

// ============================================================================
// Window animation types
// ============================================================================

/// Window transition animation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowTransition {
    /// Window opening (fade in + scale up from center).
    Open,
    /// Window closing (fade out + scale down to center).
    Close,
    /// Window minimizing (shrink to taskbar position).
    Minimize,
    /// Window restoring from minimized (expand from taskbar).
    Restore,
    /// Window maximizing (expand to fill screen).
    Maximize,
    /// Window snapping to a zone (move + resize).
    Snap,
    /// Window moving to another desktop.
    DesktopSwitch,
}

/// State for an in-progress window animation.
#[derive(Debug, Clone)]
pub struct WindowAnimation {
    /// Window ID being animated.
    pub window_id: u64,
    /// Type of transition.
    pub transition: WindowTransition,
    /// X position animation.
    pub x: Animation,
    /// Y position animation.
    pub y: Animation,
    /// Width animation.
    pub width: Animation,
    /// Height animation.
    pub height: Animation,
    /// Opacity animation (0.0 = invisible, 1.0 = fully visible).
    pub opacity: Animation,
}

impl WindowAnimation {
    /// Create a window open animation.
    pub fn open(window_id: u64, x: f32, y: f32, w: f32, h: f32, duration_ms: u32) -> Self {
        let cx = x + w / 2.0;
        let cy = y + h / 2.0;
        Self {
            window_id,
            transition: WindowTransition::Open,
            x: Animation::new(cx - w * 0.4, x, duration_ms, Easing::EaseOut),
            y: Animation::new(cy - h * 0.4, y, duration_ms, Easing::EaseOut),
            width: Animation::new(w * 0.8, w, duration_ms, Easing::EaseOut),
            height: Animation::new(h * 0.8, h, duration_ms, Easing::EaseOut),
            opacity: Animation::new(0.0, 1.0, duration_ms, Easing::EaseOut),
        }
    }

    /// Create a window close animation.
    pub fn close(window_id: u64, x: f32, y: f32, w: f32, h: f32, duration_ms: u32) -> Self {
        let cx = x + w / 2.0;
        let cy = y + h / 2.0;
        Self {
            window_id,
            transition: WindowTransition::Close,
            x: Animation::new(x, cx - w * 0.4, duration_ms, Easing::EaseIn),
            y: Animation::new(y, cy - h * 0.4, duration_ms, Easing::EaseIn),
            width: Animation::new(w, w * 0.8, duration_ms, Easing::EaseIn),
            height: Animation::new(h, h * 0.8, duration_ms, Easing::EaseIn),
            opacity: Animation::new(1.0, 0.0, duration_ms, Easing::EaseIn),
        }
    }

    /// Create a minimize animation (shrink toward a taskbar position).
    pub fn minimize(
        window_id: u64,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        taskbar_x: f32,
        taskbar_y: f32,
        duration_ms: u32,
    ) -> Self {
        Self {
            window_id,
            transition: WindowTransition::Minimize,
            x: Animation::new(x, taskbar_x, duration_ms, Easing::EaseInOut),
            y: Animation::new(y, taskbar_y, duration_ms, Easing::EaseInOut),
            width: Animation::new(w, 48.0, duration_ms, Easing::EaseInOut),
            height: Animation::new(h, 48.0, duration_ms, Easing::EaseInOut),
            opacity: Animation::new(1.0, 0.0, duration_ms, Easing::EaseIn),
        }
    }

    /// Create a snap animation (move+resize to target zone).
    pub fn snap(
        window_id: u64,
        from_x: f32,
        from_y: f32,
        from_w: f32,
        from_h: f32,
        to_x: f32,
        to_y: f32,
        to_w: f32,
        to_h: f32,
        duration_ms: u32,
    ) -> Self {
        Self {
            window_id,
            transition: WindowTransition::Snap,
            x: Animation::new(from_x, to_x, duration_ms, Easing::EaseOut),
            y: Animation::new(from_y, to_y, duration_ms, Easing::EaseOut),
            width: Animation::new(from_w, to_w, duration_ms, Easing::EaseOut),
            height: Animation::new(from_h, to_h, duration_ms, Easing::EaseOut),
            opacity: Animation::new(1.0, 1.0, 1, Easing::Linear), // No opacity change.
        }
    }

    /// Advance every sub-animation by `dt_ms` milliseconds. Returns the state
    /// after the step.
    pub fn tick(&mut self, dt_ms: u32) -> AnimatedRect {
        AnimatedRect {
            x: self.x.tick(dt_ms),
            y: self.y.tick(dt_ms),
            width: self.width.tick(dt_ms),
            height: self.height.tick(dt_ms),
            opacity: self.opacity.tick(dt_ms),
        }
    }

    /// Whether all sub-animations have completed.
    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.x.is_done()
            && self.y.is_done()
            && self.width.is_done()
            && self.height.is_done()
            && self.opacity.is_done()
    }
}

/// The current state of an animated rectangle.
#[derive(Debug, Clone, Copy)]
pub struct AnimatedRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub opacity: f32,
}

// ============================================================================
// Desktop transition
// ============================================================================

/// Virtual desktop switch animation.
#[derive(Debug, Clone)]
pub struct DesktopTransition {
    /// Direction: negative = sliding left, positive = sliding right.
    pub direction: f32,
    /// Animation progress.
    pub anim: Animation,
    /// Screen width (for calculating slide distance).
    pub screen_width: f32,
    /// Whether the transition is active.
    pub active: bool,
}

impl DesktopTransition {
    /// Create a desktop switch animation.
    /// `direction`: -1.0 for left, 1.0 for right.
    pub fn new(direction: f32, screen_width: f32, duration_ms: u32) -> Self {
        Self {
            direction,
            anim: Animation::new(0.0, 1.0, duration_ms, Easing::EaseInOut),
            screen_width,
            active: true,
        }
    }

    /// Advance by `dt_ms` milliseconds and return the current slide offset.
    pub fn tick(&mut self, dt_ms: u32) -> f32 {
        let progress = self.anim.tick(dt_ms);
        if self.anim.is_done() {
            self.active = false;
        }
        self.direction * progress * self.screen_width
    }

    /// Whether the slide has finished.
    #[must_use]
    pub const fn is_done(&self) -> bool {
        !self.active
    }
}

// ============================================================================
// Animation manager
// ============================================================================

/// Manages all active animations.
pub struct AnimationManager {
    /// Active window animations.
    window_anims: Vec<WindowAnimation>,
    /// Active desktop transition.
    desktop_transition: Option<DesktopTransition>,
    /// Whether animations are disabled (for accessibility).
    pub reduced_motion: bool,
    /// Default animation duration in milliseconds. This is the value the Tier 3
    /// `animation-duration-ms` theme setting will set.
    pub default_duration_ms: u32,
}

/// Default animation length. Long enough to read as a transition rather than a
/// jump, short enough not to be in the way of the next thing the user does.
pub const DEFAULT_DURATION_MS: u32 = 200;

impl AnimationManager {
    /// Create a new animation manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            window_anims: Vec::new(),
            desktop_transition: None,
            reduced_motion: false,
            default_duration_ms: DEFAULT_DURATION_MS,
        }
    }

    /// Start a window animation.
    pub fn animate_window(&mut self, anim: WindowAnimation) {
        if self.reduced_motion {
            return; // Skip animations when reduced motion is on.
        }
        // Remove any existing animation for this window.
        self.window_anims.retain(|a| a.window_id != anim.window_id);
        self.window_anims.push(anim);
    }

    /// Start a desktop transition.
    pub fn animate_desktop_switch(&mut self, direction: f32, screen_width: f32) {
        if self.reduced_motion {
            return;
        }
        self.desktop_transition = Some(DesktopTransition::new(
            direction,
            screen_width,
            self.default_duration_ms,
        ));
    }

    /// Advance every animation by `dt_ms` milliseconds of wall time.
    ///
    /// Returns `(window_id, AnimatedRect)` for each window animation that was
    /// stepped, **including the one that finished on this step** — the final
    /// frame is the one that puts the window at its destination, so dropping it
    /// would leave every animation ending one frame short of where it was going.
    pub fn tick(&mut self, dt_ms: u32) -> Vec<(u64, AnimatedRect)> {
        let mut results = Vec::with_capacity(self.window_anims.len());

        for anim in &mut self.window_anims {
            let rect = anim.tick(dt_ms);
            results.push((anim.window_id, rect));
        }

        // Clean up completed animations.
        self.window_anims.retain(|a| !a.is_done());

        // Tick desktop transition.
        if let Some(dt) = self.desktop_transition.as_mut() {
            dt.tick(dt_ms);
            if dt.is_done() {
                self.desktop_transition = None;
            }
        }

        results
    }

    /// Get the current desktop slide offset (0.0 if no transition).
    #[must_use]
    pub fn desktop_offset(&self) -> f32 {
        self.desktop_transition
            .as_ref()
            .map(|dt| {
                let progress = dt.anim.value();
                dt.direction * progress * dt.screen_width
            })
            .unwrap_or(0.0)
    }

    /// Whether any animations are active.
    ///
    /// This is what the shell asks after each tick to decide whether to arm the
    /// next frame, so it is the single condition that keeps an idle desktop
    /// idle: false here means no wake-up is registered and the loop parks with
    /// no bound at all.
    #[must_use]
    pub fn has_active(&self) -> bool {
        !self.window_anims.is_empty() || self.desktop_transition.is_some()
    }

    /// Get animation for a specific window.
    #[must_use]
    pub fn window_animation(&self, window_id: u64) -> Option<AnimatedRect> {
        self.window_anims
            .iter()
            .find(|a| a.window_id == window_id)
            .map(|a| AnimatedRect {
                x: a.x.value(),
                y: a.y.value(),
                width: a.width.value(),
                height: a.height.value(),
                opacity: a.opacity.value(),
            })
    }

    /// Cancel all animations for a window.
    pub fn cancel_window(&mut self, window_id: u64) {
        self.window_anims.retain(|a| a.window_id != window_id);
    }

    /// Cancel all animations.
    pub fn cancel_all(&mut self) {
        self.window_anims.clear();
        self.desktop_transition = None;
    }

    /// Number of active window animations.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.window_anims.len()
    }
}

impl Default for AnimationManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Fade overlay helper
// ============================================================================

/// A full-screen fade overlay (for desktop transitions, lock screen, etc.).
pub struct FadeOverlay {
    pub anim: Animation,
    pub color: Color,
}

impl FadeOverlay {
    /// Create a fade-in from black.
    #[must_use]
    pub fn fade_in(duration_ms: u32) -> Self {
        Self {
            anim: Animation::new(1.0, 0.0, duration_ms, Easing::EaseOut),
            color: Color::from_hex(0x000000),
        }
    }

    /// Create a fade-out to black.
    #[must_use]
    pub fn fade_out(duration_ms: u32) -> Self {
        Self {
            anim: Animation::new(0.0, 1.0, duration_ms, Easing::EaseIn),
            color: Color::from_hex(0x000000),
        }
    }

    /// Advance by `dt_ms` milliseconds and render the overlay at its new
    /// opacity, or `None` once it is transparent enough to skip drawing.
    pub fn tick_render(
        &mut self,
        dt_ms: u32,
        screen_w: f32,
        screen_h: f32,
    ) -> Option<RenderCommand> {
        let alpha = self.anim.tick(dt_ms);
        if alpha <= 0.001 {
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to 0..=255 on the line above the cast"
        )]
        let a = (alpha * 255.0).clamp(0.0, 255.0) as u8;
        Some(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: screen_w,
            height: screen_h,
            color: Color::rgba(self.color.r, self.color.g, self.color.b, a),
            corner_radii: CornerRadii::ZERO,
        })
    }

    /// Whether the fade has finished.
    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.anim.is_done()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    // -- Easing --

    #[test]
    fn test_easing_linear() {
        assert!((Easing::Linear.apply(0.0)).abs() < f32::EPSILON);
        assert!((Easing::Linear.apply(0.5) - 0.5).abs() < f32::EPSILON);
        assert!((Easing::Linear.apply(1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_easing_ease_in() {
        let mid = Easing::EaseIn.apply(0.5);
        assert!(mid < 0.5); // Should be slower at start.
        assert!((Easing::EaseIn.apply(1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_easing_ease_out() {
        let mid = Easing::EaseOut.apply(0.5);
        assert!(mid > 0.5); // Should be faster at start.
        assert!((Easing::EaseOut.apply(1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_easing_ease_in_out() {
        let mid = Easing::EaseInOut.apply(0.5);
        assert!((mid - 0.5).abs() < 0.01); // Midpoint should be close to 0.5.
        assert!((Easing::EaseInOut.apply(0.0)).abs() < f32::EPSILON);
        assert!((Easing::EaseInOut.apply(1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_easing_clamp() {
        // Values outside [0,1] should be clamped.
        assert!((Easing::Linear.apply(-0.5)).abs() < f32::EPSILON);
        assert!((Easing::Linear.apply(1.5) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_easing_bounce_endpoints() {
        assert!((Easing::Bounce.apply(0.0)).abs() < 0.01);
        assert!((Easing::Bounce.apply(1.0) - 1.0).abs() < 0.02);
    }

    #[test]
    fn test_easing_quadratic_in() {
        let v = Easing::QuadraticIn.apply(0.5);
        assert!((v - 0.25).abs() < f32::EPSILON); // 0.5^2 = 0.25
    }

    #[test]
    fn test_easing_quadratic_out() {
        assert!((Easing::QuadraticOut.apply(1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_easing_cubic_in_out_endpoints() {
        assert!((Easing::CubicInOut.apply(0.0)).abs() < f32::EPSILON);
        assert!((Easing::CubicInOut.apply(1.0) - 1.0).abs() < f32::EPSILON);
    }

    // -- Animation --

    #[test]
    fn an_animation_lasts_the_number_of_milliseconds_it_was_given() {
        // The property the whole rewrite exists for: the duration is wall time,
        // not a frame count. Two loops that deliver wildly different numbers of
        // frames must finish at the same instant, not after the same number of
        // calls.
        let mut fast = Animation::new(0.0, 100.0, 200, Easing::Linear);
        let mut slow = Animation::new(0.0, 100.0, 200, Easing::Linear);

        for _ in 0..25 {
            fast.tick(8); // ~125 Hz
        }
        for _ in 0..4 {
            slow.tick(50); // ~20 Hz, a struggling machine
        }

        assert!(fast.is_done(), "200 ms of 8 ms frames did not finish");
        assert!(slow.is_done(), "200 ms of 50 ms frames did not finish");
        assert!((fast.value() - 100.0).abs() < 0.01);
        assert!((slow.value() - 100.0).abs() < 0.01);
    }

    #[test]
    fn a_late_frame_advances_further_than_an_early_one() {
        // The other half of the same property, stated so it fails against an
        // implementation that ignores `dt_ms` and steps a fixed amount.
        let mut early = Animation::new(0.0, 100.0, 1000, Easing::Linear);
        let mut late = Animation::new(0.0, 100.0, 1000, Easing::Linear);
        early.tick(10);
        late.tick(100);
        // Linear over 1000 ms: 1.0 against 10.0. Asserting the ratio rather
        // than a threshold is what makes this fail against a fixed step, which
        // would move both by the same amount whatever `dt_ms` said.
        assert!(
            late.value() > early.value() * 5.0,
            "a 100 ms frame moved {} where a 10 ms frame moved {}",
            late.value(),
            early.value()
        );
    }

    #[test]
    fn an_animation_does_not_finish_before_its_time_however_many_frames_arrive() {
        let mut anim = Animation::new(0.0, 100.0, 500, Easing::Linear);
        for _ in 0..100 {
            anim.tick(1);
        }
        assert!(
            !anim.is_done(),
            "100 frames finished a 500 ms animation after 100 ms"
        );
    }

    #[test]
    fn an_animation_starts_at_its_start_value_and_ends_at_its_end_value() {
        let mut anim = Animation::new(50.0, 150.0, 100, Easing::Linear);
        assert!((anim.value() - 50.0).abs() < f32::EPSILON);
        assert!(anim.progress().abs() < f32::EPSILON);
        anim.tick(100);
        assert!(anim.is_done());
        assert!((anim.value() - 150.0).abs() < 0.01);
    }

    #[test]
    fn a_step_past_the_end_lands_on_the_end_rather_than_overshooting() {
        // A frame can be arbitrarily late — the loop may have been blocked for
        // a second. A window must not be flung past where it was going.
        let mut anim = Animation::new(0.0, 100.0, 100, Easing::Linear);
        let v = anim.tick(5_000);
        assert!(anim.is_done());
        assert!((v - 100.0).abs() < 0.01, "overshot to {v}");
        assert!((anim.progress() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ticking_a_finished_animation_leaves_it_where_it_ended() {
        let mut anim = Animation::new(0.0, 100.0, 100, Easing::Linear);
        anim.tick(100);
        let settled = anim.value();
        for _ in 0..10 {
            anim.tick(100);
        }
        assert!((anim.value() - settled).abs() < f32::EPSILON);
    }

    #[test]
    fn resetting_puts_an_animation_back_at_the_beginning() {
        let mut anim = Animation::new(0.0, 100.0, 50, Easing::Linear);
        anim.tick(50);
        assert!(anim.is_done());
        anim.reset();
        assert!(!anim.is_done());
        assert!(anim.progress().abs() < f32::EPSILON);
        assert!((anim.value()).abs() < f32::EPSILON);
    }

    #[test]
    fn an_auto_reversing_animation_returns_to_its_start_and_then_stops() {
        let mut anim = Animation::new(0.0, 100.0, 50, Easing::Linear);
        anim.auto_reverse = true;

        anim.tick(50);
        assert!(!anim.is_done(), "should be reversing, not finished");

        let val = anim.tick(50);
        assert!(anim.is_done());
        assert!(val < 10.0, "came back to {val}, not near the start");
    }

    #[test]
    fn a_looping_animation_never_finishes() {
        let mut anim = Animation::looping(0.0, 100.0, 50, Easing::Linear);
        for _ in 0..20 {
            anim.tick(50);
        }
        assert!(!anim.is_done());
    }

    #[test]
    fn a_loop_carries_the_remainder_of_a_long_frame_into_the_next_pass() {
        // A frame that arrives 1.5 passes late must leave the loop half a pass
        // in, not back at zero. Restarting from zero silently discards time,
        // which is how a looping caret drifts out of step with itself.
        let mut anim = Animation::looping(0.0, 100.0, 100, Easing::Linear);
        anim.tick(150);
        assert!((anim.progress() - 0.5).abs() < 0.01, "{}", anim.progress());
    }

    // -- Color Animation --

    #[test]
    fn a_colour_animation_arrives_at_its_destination_colour() {
        let mut ca = ColorAnimation::new(
            Color::rgba(0, 0, 0, 255),
            Color::rgba(255, 255, 255, 255),
            100,
            Easing::Linear,
        );
        let mut last = Color::rgba(0, 0, 0, 255);
        for _ in 0..10 {
            last = ca.tick(10);
        }
        assert!(ca.is_done());
        assert_eq!(last.r, 255);
        assert_eq!(last.g, 255);
    }

    #[test]
    fn test_color_lerp() {
        let a = Color::rgba(0, 0, 0, 255);
        let b = Color::rgba(100, 200, 50, 255);
        let mid = lerp_color(a, b, 0.5);
        assert_eq!(mid.r, 50);
        assert_eq!(mid.g, 100);
        assert_eq!(mid.b, 25);
    }

    #[test]
    fn test_color_lerp_endpoints() {
        let a = Color::rgba(10, 20, 30, 40);
        let b = Color::rgba(100, 200, 150, 250);
        let start = lerp_color(a, b, 0.0);
        assert_eq!(start, a);
        let end = lerp_color(a, b, 1.0);
        assert_eq!(end, b);
    }

    // -- Window Animation --

    #[test]
    fn a_window_open_animation_finishes_after_its_duration() {
        let mut wa = WindowAnimation::open(1, 100.0, 100.0, 400.0, 300.0, 100);
        assert!(!wa.is_done());
        wa.tick(50);
        assert!(!wa.is_done(), "finished at half its duration");
        wa.tick(50);
        assert!(wa.is_done());
    }

    #[test]
    fn a_window_open_animation_ends_at_the_geometry_it_was_given() {
        // The last frame is the one that puts the window where it belongs. If
        // the final step were dropped the window would settle a few pixels
        // short of its own position, for ever.
        let mut wa = WindowAnimation::open(1, 100.0, 120.0, 400.0, 300.0, 100);
        let rect = wa.tick(100);
        assert!((rect.x - 100.0).abs() < 0.01, "x={}", rect.x);
        assert!((rect.y - 120.0).abs() < 0.01, "y={}", rect.y);
        assert!((rect.width - 400.0).abs() < 0.01, "w={}", rect.width);
        assert!((rect.height - 300.0).abs() < 0.01, "h={}", rect.height);
        assert!((rect.opacity - 1.0).abs() < 0.01, "a={}", rect.opacity);
    }

    #[test]
    fn a_window_close_animation_finishes_after_its_duration() {
        let mut wa = WindowAnimation::close(2, 100.0, 100.0, 400.0, 300.0, 100);
        wa.tick(100);
        assert!(wa.is_done());
    }

    #[test]
    fn a_minimize_animation_shrinks_to_the_taskbar() {
        let mut wa = WindowAnimation::minimize(3, 100.0, 100.0, 400.0, 300.0, 500.0, 900.0, 80);
        let rect = wa.tick(80);
        assert!(wa.is_done());
        assert!((rect.width - 48.0).abs() < 1.0);
        assert!((rect.x - 500.0).abs() < 1.0);
        assert!((rect.y - 900.0).abs() < 1.0);
    }

    #[test]
    fn a_snap_animation_finishes_at_the_zone_it_was_aimed_at() {
        let mut wa =
            WindowAnimation::snap(4, 100.0, 100.0, 400.0, 300.0, 0.0, 0.0, 960.0, 1080.0, 100);
        let rect = wa.tick(100);
        assert!(wa.is_done());
        assert!((rect.width - 960.0).abs() < 0.01);
        assert!((rect.height - 1080.0).abs() < 0.01);
        assert!(
            (rect.opacity - 1.0).abs() < 0.01,
            "a snap must not fade the window: {}",
            rect.opacity
        );
    }

    // -- Desktop Transition --

    #[test]
    fn a_desktop_slide_covers_the_whole_screen_width() {
        let mut dt = DesktopTransition::new(-1.0, 1920.0, 100);
        assert!(!dt.is_done());
        let last_offset = dt.tick(100);
        assert!(dt.is_done());
        assert!((last_offset - (-1920.0)).abs() < 1.0);
    }

    // -- Animation Manager --

    #[test]
    fn the_manager_reports_no_activity_once_every_animation_has_finished() {
        // This is what the shell's re-arm decision reads, so it is the test
        // that keeps an idle desktop idle.
        let mut mgr = AnimationManager::new();
        assert!(!mgr.has_active());

        mgr.animate_window(WindowAnimation::open(1, 0.0, 0.0, 100.0, 100.0, 50));
        assert!(mgr.has_active());
        assert_eq!(mgr.active_count(), 1);

        mgr.tick(25);
        assert!(mgr.has_active(), "gave up half way through");
        mgr.tick(25);
        assert!(!mgr.has_active());
    }

    #[test]
    fn the_final_frame_of_an_animation_is_still_reported() {
        // The step that finishes an animation is the step that puts the window
        // at its destination. Retiring it before reporting would leave every
        // window one frame short of where it was going.
        let mut mgr = AnimationManager::new();
        mgr.animate_window(WindowAnimation::open(7, 10.0, 20.0, 100.0, 80.0, 50));
        let stepped = mgr.tick(50);
        assert_eq!(stepped.len(), 1, "the last frame was swallowed");
        assert_eq!(stepped[0].0, 7);
        assert!((stepped[0].1.x - 10.0).abs() < 0.01);
        assert!(!mgr.has_active());
    }

    #[test]
    fn reduced_motion_refuses_the_animation_rather_than_playing_it_fast() {
        let mut mgr = AnimationManager::new();
        mgr.reduced_motion = true;

        mgr.animate_window(WindowAnimation::open(1, 0.0, 0.0, 100.0, 100.0, 100));
        assert!(!mgr.has_active());
    }

    #[test]
    fn test_animation_manager_cancel_window() {
        let mut mgr = AnimationManager::new();
        mgr.animate_window(WindowAnimation::open(1, 0.0, 0.0, 100.0, 100.0, 100));
        mgr.animate_window(WindowAnimation::open(2, 0.0, 0.0, 100.0, 100.0, 100));
        assert_eq!(mgr.active_count(), 2);

        mgr.cancel_window(1);
        assert_eq!(mgr.active_count(), 1);
    }

    #[test]
    fn test_animation_manager_cancel_all() {
        let mut mgr = AnimationManager::new();
        mgr.animate_window(WindowAnimation::open(1, 0.0, 0.0, 100.0, 100.0, 100));
        mgr.animate_desktop_switch(-1.0, 1920.0);
        assert!(mgr.has_active());

        mgr.cancel_all();
        assert!(!mgr.has_active());
    }

    #[test]
    fn test_animation_manager_desktop_offset() {
        let mut mgr = AnimationManager::new();
        assert!((mgr.desktop_offset()).abs() < f32::EPSILON);

        mgr.animate_desktop_switch(1.0, 1920.0);
        mgr.tick(16);
        assert!(mgr.desktop_offset() > 0.0);
    }

    #[test]
    fn test_animation_manager_window_query() {
        let mut mgr = AnimationManager::new();
        assert!(mgr.window_animation(1).is_none());

        mgr.animate_window(WindowAnimation::open(1, 50.0, 50.0, 200.0, 150.0, 100));
        assert!(mgr.window_animation(1).is_some());
        assert!(mgr.window_animation(2).is_none());
    }

    #[test]
    fn the_default_duration_is_expressed_in_milliseconds() {
        // Guards the unit itself. A frame count that happens to be plausible as
        // a duration is exactly the confusion this rewrite removes: 12 was a
        // sensible number of frames and is a nonsensical number of
        // milliseconds, and nothing but this assertion would notice.
        let mgr = AnimationManager::new();
        assert_eq!(mgr.default_duration_ms, DEFAULT_DURATION_MS);
        assert!(
            mgr.default_duration_ms >= 60,
            "{} ms is too short to be a duration; it looks like a frame count",
            mgr.default_duration_ms
        );

        let mut mgr = AnimationManager::new();
        mgr.animate_desktop_switch(1.0, 1920.0);
        mgr.tick(mgr.default_duration_ms.saturating_sub(1));
        assert!(mgr.has_active(), "the switch was over before its duration");
        mgr.tick(1);
        assert!(!mgr.has_active(), "the switch outlasted its duration");
    }

    // -- Fade Overlay --

    #[test]
    fn test_fade_in() {
        let mut fo = FadeOverlay::fade_in(100);
        let cmd = fo.tick_render(10, 1920.0, 1080.0);
        assert!(cmd.is_some()); // Should render overlay at start.
    }

    #[test]
    fn test_fade_out() {
        let mut fo = FadeOverlay::fade_out(100);
        fo.tick_render(100, 1920.0, 1080.0);
        assert!(fo.is_done());
    }

    #[test]
    fn test_fade_in_transparent_at_end() {
        let mut fo = FadeOverlay::fade_in(50);
        let last_cmd = fo.tick_render(50, 800.0, 600.0);
        // At the end of fade-in, the overlay should be nearly transparent.
        assert!(last_cmd.is_none() || fo.is_done());
    }

    // -- Sine approximation --

    #[test]
    fn test_sine_approx_zero() {
        assert!(sine_approx(0.0).abs() < 0.01);
    }

    #[test]
    fn test_sine_approx_pi_half() {
        let v = sine_approx(core::f32::consts::FRAC_PI_2);
        assert!((v - 1.0).abs() < 0.02);
    }

    #[test]
    fn test_sine_approx_pi() {
        let v = sine_approx(core::f32::consts::PI);
        assert!(v.abs() < 0.02);
    }

    // -- Replace existing animation for same window --

    #[test]
    fn test_animation_replaces_existing() {
        let mut mgr = AnimationManager::new();
        mgr.animate_window(WindowAnimation::open(1, 0.0, 0.0, 100.0, 100.0, 100));
        mgr.animate_window(WindowAnimation::close(1, 0.0, 0.0, 100.0, 100.0, 100));
        assert_eq!(mgr.active_count(), 1);
    }
}
