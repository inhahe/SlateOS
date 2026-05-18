//! Animation and transition system for the desktop shell.
//!
//! Provides smooth animations for window operations, desktop transitions,
//! and UI element state changes. All animations respect the `reduced_motion`
//! accessibility setting.
//!
//! Uses a tick-based system (one tick = one frame, typically ~6.94ms at 144Hz
//! or ~16.67ms at 60Hz). Animations complete in a fixed number of ticks
//! regardless of frame rate (actual wall time varies with refresh rate).

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
                let angle = (t1 - s) / p * 6.2832; // 2*PI
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
    let pi = 3.14159265;
    let two_pi = 2.0 * pi;
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
    /// Duration in ticks.
    pub duration_ticks: u32,
    /// Current tick.
    pub current_tick: u32,
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
    /// Create a new animation.
    pub fn new(from: f32, to: f32, duration_ticks: u32, easing: Easing) -> Self {
        Self {
            from,
            to,
            duration_ticks: duration_ticks.max(1),
            current_tick: 0,
            easing,
            active: true,
            auto_reverse: false,
            looping: false,
            reversing: false,
        }
    }

    /// Create a looping animation.
    pub fn looping(from: f32, to: f32, duration_ticks: u32, easing: Easing) -> Self {
        let mut anim = Self::new(from, to, duration_ticks, easing);
        anim.looping = true;
        anim.auto_reverse = true;
        anim
    }

    /// Advance by one tick. Returns the current interpolated value.
    pub fn tick(&mut self) -> f32 {
        if !self.active {
            return if self.reversing { self.from } else { self.to };
        }

        self.current_tick = self.current_tick.saturating_add(1);

        if self.current_tick >= self.duration_ticks {
            if self.auto_reverse && !self.reversing {
                self.reversing = true;
                self.current_tick = 0;
            } else if self.looping {
                self.reversing = false;
                self.current_tick = 0;
            } else {
                self.active = false;
                return if self.reversing { self.from } else { self.to };
            }
        }

        let progress = self.current_tick as f32 / self.duration_ticks as f32;
        let eased = self.easing.apply(progress);

        if self.reversing {
            self.to + (self.from - self.to) * eased
        } else {
            self.from + (self.to - self.from) * eased
        }
    }

    /// Get current value without advancing.
    pub fn value(&self) -> f32 {
        if !self.active {
            return if self.reversing { self.from } else { self.to };
        }
        let progress = self.current_tick as f32 / self.duration_ticks as f32;
        let eased = self.easing.apply(progress);
        if self.reversing {
            self.to + (self.from - self.to) * eased
        } else {
            self.from + (self.to - self.from) * eased
        }
    }

    /// Whether the animation has completed.
    pub fn is_done(&self) -> bool {
        !self.active
    }

    /// Reset the animation to the beginning.
    pub fn reset(&mut self) {
        self.current_tick = 0;
        self.active = true;
        self.reversing = false;
    }

    /// Normalized progress (0.0 to 1.0).
    pub fn progress(&self) -> f32 {
        if self.duration_ticks == 0 {
            return 1.0;
        }
        (self.current_tick as f32 / self.duration_ticks as f32).clamp(0.0, 1.0)
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
    pub fn new(from: Color, to: Color, duration_ticks: u32, easing: Easing) -> Self {
        Self {
            from,
            to,
            anim: Animation::new(0.0, 1.0, duration_ticks, easing),
        }
    }

    /// Advance and return current color.
    pub fn tick(&mut self) -> Color {
        let t = self.anim.tick();
        lerp_color(self.from, self.to, t)
    }

    pub fn value(&self) -> Color {
        lerp_color(self.from, self.to, self.anim.value())
    }

    pub fn is_done(&self) -> bool {
        self.anim.is_done()
    }

    pub fn reset(&mut self) {
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
    pub fn open(window_id: u64, x: f32, y: f32, w: f32, h: f32, ticks: u32) -> Self {
        let cx = x + w / 2.0;
        let cy = y + h / 2.0;
        Self {
            window_id,
            transition: WindowTransition::Open,
            x: Animation::new(cx - w * 0.4, x, ticks, Easing::EaseOut),
            y: Animation::new(cy - h * 0.4, y, ticks, Easing::EaseOut),
            width: Animation::new(w * 0.8, w, ticks, Easing::EaseOut),
            height: Animation::new(h * 0.8, h, ticks, Easing::EaseOut),
            opacity: Animation::new(0.0, 1.0, ticks, Easing::EaseOut),
        }
    }

    /// Create a window close animation.
    pub fn close(window_id: u64, x: f32, y: f32, w: f32, h: f32, ticks: u32) -> Self {
        let cx = x + w / 2.0;
        let cy = y + h / 2.0;
        Self {
            window_id,
            transition: WindowTransition::Close,
            x: Animation::new(x, cx - w * 0.4, ticks, Easing::EaseIn),
            y: Animation::new(y, cy - h * 0.4, ticks, Easing::EaseIn),
            width: Animation::new(w, w * 0.8, ticks, Easing::EaseIn),
            height: Animation::new(h, h * 0.8, ticks, Easing::EaseIn),
            opacity: Animation::new(1.0, 0.0, ticks, Easing::EaseIn),
        }
    }

    /// Create a minimize animation (shrink toward a taskbar position).
    pub fn minimize(
        window_id: u64,
        x: f32, y: f32, w: f32, h: f32,
        taskbar_x: f32, taskbar_y: f32,
        ticks: u32,
    ) -> Self {
        Self {
            window_id,
            transition: WindowTransition::Minimize,
            x: Animation::new(x, taskbar_x, ticks, Easing::EaseInOut),
            y: Animation::new(y, taskbar_y, ticks, Easing::EaseInOut),
            width: Animation::new(w, 48.0, ticks, Easing::EaseInOut),
            height: Animation::new(h, 48.0, ticks, Easing::EaseInOut),
            opacity: Animation::new(1.0, 0.0, ticks, Easing::EaseIn),
        }
    }

    /// Create a snap animation (move+resize to target zone).
    pub fn snap(
        window_id: u64,
        from_x: f32, from_y: f32, from_w: f32, from_h: f32,
        to_x: f32, to_y: f32, to_w: f32, to_h: f32,
        ticks: u32,
    ) -> Self {
        Self {
            window_id,
            transition: WindowTransition::Snap,
            x: Animation::new(from_x, to_x, ticks, Easing::EaseOut),
            y: Animation::new(from_y, to_y, ticks, Easing::EaseOut),
            width: Animation::new(from_w, to_w, ticks, Easing::EaseOut),
            height: Animation::new(from_h, to_h, ticks, Easing::EaseOut),
            opacity: Animation::new(1.0, 1.0, 1, Easing::Linear), // No opacity change.
        }
    }

    /// Advance all sub-animations by one tick. Returns current state.
    pub fn tick(&mut self) -> AnimatedRect {
        AnimatedRect {
            x: self.x.tick(),
            y: self.y.tick(),
            width: self.width.tick(),
            height: self.height.tick(),
            opacity: self.opacity.tick(),
        }
    }

    /// Whether all sub-animations have completed.
    pub fn is_done(&self) -> bool {
        self.x.is_done() && self.y.is_done() && self.width.is_done()
            && self.height.is_done() && self.opacity.is_done()
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
    pub fn new(direction: f32, screen_width: f32, duration_ticks: u32) -> Self {
        Self {
            direction,
            anim: Animation::new(0.0, 1.0, duration_ticks, Easing::EaseInOut),
            screen_width,
            active: true,
        }
    }

    /// Advance and return current slide offset.
    pub fn tick(&mut self) -> f32 {
        let progress = self.anim.tick();
        if self.anim.is_done() {
            self.active = false;
        }
        self.direction * progress * self.screen_width
    }

    pub fn is_done(&self) -> bool {
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
    /// Default animation duration in ticks.
    pub default_duration: u32,
}

impl AnimationManager {
    /// Create a new animation manager.
    pub fn new() -> Self {
        Self {
            window_anims: Vec::new(),
            desktop_transition: None,
            reduced_motion: false,
            default_duration: 12, // ~200ms at 60Hz, ~83ms at 144Hz.
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
            self.default_duration,
        ));
    }

    /// Advance all animations by one tick.
    /// Returns list of (window_id, AnimatedRect) for each active window anim.
    pub fn tick(&mut self) -> Vec<(u64, AnimatedRect)> {
        let mut results = Vec::with_capacity(self.window_anims.len());

        for anim in &mut self.window_anims {
            let rect = anim.tick();
            results.push((anim.window_id, rect));
        }

        // Clean up completed animations.
        self.window_anims.retain(|a| !a.is_done());

        // Tick desktop transition.
        if let Some(ref mut dt) = self.desktop_transition {
            dt.tick();
            if dt.is_done() {
                self.desktop_transition = None;
            }
        }

        results
    }

    /// Get the current desktop slide offset (0.0 if no transition).
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
    pub fn has_active(&self) -> bool {
        !self.window_anims.is_empty() || self.desktop_transition.is_some()
    }

    /// Get animation for a specific window.
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
    pub fn active_count(&self) -> usize {
        self.window_anims.len()
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
    pub fn fade_in(duration_ticks: u32) -> Self {
        Self {
            anim: Animation::new(1.0, 0.0, duration_ticks, Easing::EaseOut),
            color: Color::from_hex(0x000000),
        }
    }

    /// Create a fade-out to black.
    pub fn fade_out(duration_ticks: u32) -> Self {
        Self {
            anim: Animation::new(0.0, 1.0, duration_ticks, Easing::EaseIn),
            color: Color::from_hex(0x000000),
        }
    }

    /// Advance and render.
    pub fn tick_render(&mut self, screen_w: f32, screen_h: f32) -> Option<RenderCommand> {
        let alpha = self.anim.tick();
        if alpha <= 0.001 {
            return None;
        }
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

    pub fn is_done(&self) -> bool {
        self.anim.is_done()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
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
    fn test_animation_basic() {
        let mut anim = Animation::new(0.0, 100.0, 10, Easing::Linear);
        assert!(!anim.is_done());

        // Run through all ticks.
        let mut last_val = 0.0;
        for _ in 0..10 {
            last_val = anim.tick();
        }
        assert!((last_val - 100.0).abs() < 0.1);
        assert!(anim.is_done());
    }

    #[test]
    fn test_animation_progress() {
        let anim = Animation::new(0.0, 100.0, 10, Easing::Linear);
        assert!((anim.progress()).abs() < f32::EPSILON);
    }

    #[test]
    fn test_animation_value_without_tick() {
        let anim = Animation::new(50.0, 150.0, 10, Easing::Linear);
        assert!((anim.value() - 50.0).abs() < f32::EPSILON); // At tick 0, should be start value.
    }

    #[test]
    fn test_animation_reset() {
        let mut anim = Animation::new(0.0, 100.0, 5, Easing::Linear);
        for _ in 0..5 {
            anim.tick();
        }
        assert!(anim.is_done());
        anim.reset();
        assert!(!anim.is_done());
        assert!((anim.progress()).abs() < f32::EPSILON);
    }

    #[test]
    fn test_animation_auto_reverse() {
        let mut anim = Animation::new(0.0, 100.0, 5, Easing::Linear);
        anim.auto_reverse = true;

        // Forward phase.
        for _ in 0..5 {
            anim.tick();
        }
        assert!(!anim.is_done()); // Should now be reversing.

        // Reverse phase.
        let mut val = 0.0;
        for _ in 0..5 {
            val = anim.tick();
        }
        assert!(anim.is_done());
        assert!(val < 10.0); // Should be back near start.
    }

    #[test]
    fn test_animation_looping() {
        let mut anim = Animation::looping(0.0, 100.0, 5, Easing::Linear);
        // Run through two cycles.
        for _ in 0..20 {
            anim.tick();
        }
        assert!(!anim.is_done()); // Should never be done if looping.
    }

    // -- Color Animation --

    #[test]
    fn test_color_animation() {
        let mut ca = ColorAnimation::new(
            Color::rgba(0, 0, 0, 255),
            Color::rgba(255, 255, 255, 255),
            10,
            Easing::Linear,
        );
        let mut last = Color::rgba(0, 0, 0, 255);
        for _ in 0..10 {
            last = ca.tick();
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
    fn test_window_open_animation() {
        let mut wa = WindowAnimation::open(1, 100.0, 100.0, 400.0, 300.0, 10);
        assert!(!wa.is_done());
        for _ in 0..10 {
            wa.tick();
        }
        assert!(wa.is_done());
    }

    #[test]
    fn test_window_close_animation() {
        let mut wa = WindowAnimation::close(2, 100.0, 100.0, 400.0, 300.0, 10);
        for _ in 0..10 {
            wa.tick();
        }
        assert!(wa.is_done());
    }

    #[test]
    fn test_window_minimize_animation() {
        let mut wa = WindowAnimation::minimize(3, 100.0, 100.0, 400.0, 300.0, 500.0, 900.0, 8);
        let mut rect = AnimatedRect { x: 0.0, y: 0.0, width: 0.0, height: 0.0, opacity: 0.0 };
        for _ in 0..8 {
            rect = wa.tick();
        }
        assert!(wa.is_done());
        assert!((rect.width - 48.0).abs() < 1.0);
    }

    #[test]
    fn test_window_snap_animation() {
        let mut wa = WindowAnimation::snap(
            4,
            100.0, 100.0, 400.0, 300.0,
            0.0, 0.0, 960.0, 1080.0,
            10,
        );
        for _ in 0..10 {
            wa.tick();
        }
        assert!(wa.is_done());
    }

    // -- Desktop Transition --

    #[test]
    fn test_desktop_transition() {
        let mut dt = DesktopTransition::new(-1.0, 1920.0, 10);
        assert!(!dt.is_done());
        let mut last_offset = 0.0;
        for _ in 0..10 {
            last_offset = dt.tick();
        }
        assert!(dt.is_done());
        assert!((last_offset - (-1920.0)).abs() < 1.0);
    }

    // -- Animation Manager --

    #[test]
    fn test_animation_manager_basic() {
        let mut mgr = AnimationManager::new();
        assert!(!mgr.has_active());

        mgr.animate_window(WindowAnimation::open(1, 0.0, 0.0, 100.0, 100.0, 5));
        assert!(mgr.has_active());
        assert_eq!(mgr.active_count(), 1);

        for _ in 0..5 {
            mgr.tick();
        }
        assert!(!mgr.has_active());
    }

    #[test]
    fn test_animation_manager_reduced_motion() {
        let mut mgr = AnimationManager::new();
        mgr.reduced_motion = true;

        mgr.animate_window(WindowAnimation::open(1, 0.0, 0.0, 100.0, 100.0, 10));
        assert!(!mgr.has_active()); // Should not have added anything.
    }

    #[test]
    fn test_animation_manager_cancel_window() {
        let mut mgr = AnimationManager::new();
        mgr.animate_window(WindowAnimation::open(1, 0.0, 0.0, 100.0, 100.0, 10));
        mgr.animate_window(WindowAnimation::open(2, 0.0, 0.0, 100.0, 100.0, 10));
        assert_eq!(mgr.active_count(), 2);

        mgr.cancel_window(1);
        assert_eq!(mgr.active_count(), 1);
    }

    #[test]
    fn test_animation_manager_cancel_all() {
        let mut mgr = AnimationManager::new();
        mgr.animate_window(WindowAnimation::open(1, 0.0, 0.0, 100.0, 100.0, 10));
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
        mgr.tick();
        assert!(mgr.desktop_offset() > 0.0);
    }

    #[test]
    fn test_animation_manager_window_query() {
        let mut mgr = AnimationManager::new();
        assert!(mgr.window_animation(1).is_none());

        mgr.animate_window(WindowAnimation::open(1, 50.0, 50.0, 200.0, 150.0, 10));
        assert!(mgr.window_animation(1).is_some());
        assert!(mgr.window_animation(2).is_none());
    }

    // -- Fade Overlay --

    #[test]
    fn test_fade_in() {
        let mut fo = FadeOverlay::fade_in(10);
        let cmd = fo.tick_render(1920.0, 1080.0);
        assert!(cmd.is_some()); // Should render overlay at start.
    }

    #[test]
    fn test_fade_out() {
        let mut fo = FadeOverlay::fade_out(10);
        for _ in 0..10 {
            fo.tick_render(1920.0, 1080.0);
        }
        assert!(fo.is_done());
    }

    #[test]
    fn test_fade_in_transparent_at_end() {
        let mut fo = FadeOverlay::fade_in(5);
        let mut last_cmd = None;
        for _ in 0..5 {
            last_cmd = fo.tick_render(800.0, 600.0);
        }
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
        let v = sine_approx(3.14159265 / 2.0);
        assert!((v - 1.0).abs() < 0.02);
    }

    #[test]
    fn test_sine_approx_pi() {
        let v = sine_approx(3.14159265);
        assert!(v.abs() < 0.02);
    }

    // -- Replace existing animation for same window --

    #[test]
    fn test_animation_replaces_existing() {
        let mut mgr = AnimationManager::new();
        mgr.animate_window(WindowAnimation::open(1, 0.0, 0.0, 100.0, 100.0, 10));
        mgr.animate_window(WindowAnimation::close(1, 0.0, 0.0, 100.0, 100.0, 10));
        assert_eq!(mgr.active_count(), 1);
    }
}
