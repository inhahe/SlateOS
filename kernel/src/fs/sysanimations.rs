//! System Animations — UI animation configuration.
//!
//! Controls system-wide animation settings including window transitions,
//! menu animations, scroll smoothness, and motion reduction for
//! accessibility.
//!
//! ## Architecture
//!
//! ```text
//! Window operation
//!   → sysanimations::get_animation(type) → duration/curve
//!
//! Configuration
//!   → sysanimations::set_enabled(enabled)
//!   → sysanimations::set_speed(multiplier)
//!   → sysanimations::set_reduce_motion(enabled)
//!
//! Integration:
//!   → a11y (reduce motion accessibility)
//!   → theme (animation matches theme)
//!   → winsnap (snap animations)
//!   → startmenu (menu animations)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Animation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationType {
    WindowOpen,
    WindowClose,
    WindowMinimize,
    WindowMaximize,
    WindowSnap,
    MenuOpen,
    MenuClose,
    TooltipFade,
    ScrollSmooth,
    TaskbarSlide,
    NotificationSlide,
    DesktopSwitch,
}

impl AnimationType {
    pub fn label(self) -> &'static str {
        match self {
            Self::WindowOpen => "Window Open",
            Self::WindowClose => "Window Close",
            Self::WindowMinimize => "Window Minimize",
            Self::WindowMaximize => "Window Maximize",
            Self::WindowSnap => "Window Snap",
            Self::MenuOpen => "Menu Open",
            Self::MenuClose => "Menu Close",
            Self::TooltipFade => "Tooltip Fade",
            Self::ScrollSmooth => "Smooth Scroll",
            Self::TaskbarSlide => "Taskbar Slide",
            Self::NotificationSlide => "Notification Slide",
            Self::DesktopSwitch => "Desktop Switch",
        }
    }
}

/// Easing curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EasingCurve {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    Spring,
    Bounce,
}

impl EasingCurve {
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::EaseIn => "Ease In",
            Self::EaseOut => "Ease Out",
            Self::EaseInOut => "Ease In-Out",
            Self::Spring => "Spring",
            Self::Bounce => "Bounce",
        }
    }
}

/// Animation configuration for a specific type.
#[derive(Debug, Clone)]
pub struct AnimationConfig {
    pub animation_type: AnimationType,
    /// Base duration in milliseconds.
    pub duration_ms: u32,
    pub easing: EasingCurve,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    animations: Vec<AnimationConfig>,
    global_enabled: bool,
    /// Speed multiplier in percent (50 = half speed, 200 = double).
    speed_percent: u32,
    reduce_motion: bool,
    total_changes: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

fn default_animations() -> Vec<AnimationConfig> {
    alloc::vec![
        AnimationConfig { animation_type: AnimationType::WindowOpen, duration_ms: 200, easing: EasingCurve::EaseOut, enabled: true },
        AnimationConfig { animation_type: AnimationType::WindowClose, duration_ms: 150, easing: EasingCurve::EaseIn, enabled: true },
        AnimationConfig { animation_type: AnimationType::WindowMinimize, duration_ms: 250, easing: EasingCurve::EaseInOut, enabled: true },
        AnimationConfig { animation_type: AnimationType::WindowMaximize, duration_ms: 200, easing: EasingCurve::EaseOut, enabled: true },
        AnimationConfig { animation_type: AnimationType::WindowSnap, duration_ms: 150, easing: EasingCurve::EaseOut, enabled: true },
        AnimationConfig { animation_type: AnimationType::MenuOpen, duration_ms: 100, easing: EasingCurve::EaseOut, enabled: true },
        AnimationConfig { animation_type: AnimationType::MenuClose, duration_ms: 80, easing: EasingCurve::EaseIn, enabled: true },
        AnimationConfig { animation_type: AnimationType::TooltipFade, duration_ms: 150, easing: EasingCurve::Linear, enabled: true },
        AnimationConfig { animation_type: AnimationType::ScrollSmooth, duration_ms: 120, easing: EasingCurve::EaseOut, enabled: true },
        AnimationConfig { animation_type: AnimationType::TaskbarSlide, duration_ms: 200, easing: EasingCurve::EaseInOut, enabled: true },
        AnimationConfig { animation_type: AnimationType::NotificationSlide, duration_ms: 300, easing: EasingCurve::Spring, enabled: true },
        AnimationConfig { animation_type: AnimationType::DesktopSwitch, duration_ms: 250, easing: EasingCurve::EaseInOut, enabled: true },
    ]
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        animations: default_animations(),
        global_enabled: true,
        speed_percent: 100,
        reduce_motion: false,
        total_changes: 0,
        ops: 0,
    });
}

/// Enable/disable all animations.
pub fn set_global_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.global_enabled = enabled;
        state.total_changes += 1;
        Ok(())
    })
}

/// Set global speed multiplier (50-400%).
pub fn set_speed(percent: u32) -> KernelResult<()> {
    with_state(|state| {
        state.speed_percent = percent.clamp(10, 400);
        state.total_changes += 1;
        Ok(())
    })
}

/// Enable reduce-motion mode (disables most animations, instant transitions).
pub fn set_reduce_motion(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.reduce_motion = enabled;
        state.total_changes += 1;
        Ok(())
    })
}

/// Set duration for a specific animation.
pub fn set_duration(anim_type: AnimationType, duration_ms: u32) -> KernelResult<()> {
    with_state(|state| {
        let anim = state.animations.iter_mut().find(|a| a.animation_type == anim_type)
            .ok_or(KernelError::NotFound)?;
        anim.duration_ms = duration_ms.clamp(0, 5000);
        state.total_changes += 1;
        Ok(())
    })
}

/// Set easing curve for a specific animation.
pub fn set_easing(anim_type: AnimationType, easing: EasingCurve) -> KernelResult<()> {
    with_state(|state| {
        let anim = state.animations.iter_mut().find(|a| a.animation_type == anim_type)
            .ok_or(KernelError::NotFound)?;
        anim.easing = easing;
        state.total_changes += 1;
        Ok(())
    })
}

/// Enable/disable a specific animation.
pub fn set_animation_enabled(anim_type: AnimationType, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let anim = state.animations.iter_mut().find(|a| a.animation_type == anim_type)
            .ok_or(KernelError::NotFound)?;
        anim.enabled = enabled;
        state.total_changes += 1;
        Ok(())
    })
}

/// Get effective duration for an animation (applying speed multiplier and reduce-motion).
pub fn effective_duration(anim_type: AnimationType) -> u32 {
    let guard = STATE.lock();
    let state = match guard.as_ref() {
        Some(s) => s,
        None => return 0,
    };
    if !state.global_enabled || state.reduce_motion { return 0; }
    state.animations.iter().find(|a| a.animation_type == anim_type)
        .map_or(0, |a| {
            if !a.enabled { 0 }
            else { a.duration_ms * state.speed_percent / 100 }
        })
}

/// List all animation configs.
pub fn list_animations() -> Vec<AnimationConfig> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.animations.clone())
}

/// Get global state: (enabled, speed_percent, reduce_motion).
pub fn global_state() -> (bool, u32, bool) {
    STATE.lock().as_ref().map_or((true, 100, false), |s| {
        (s.global_enabled, s.speed_percent, s.reduce_motion)
    })
}

/// Statistics: (animation_count, enabled_count, total_changes, ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let enabled = s.animations.iter().filter(|a| a.enabled).count();
            (s.animations.len(), enabled, s.total_changes, s.ops)
        }
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("sysanimations::self_test() — running tests...");
    init_defaults();

    // 1: Default 12 animations.
    let anims = list_animations();
    assert_eq!(anims.len(), 12);
    assert!(anims.iter().all(|a| a.enabled));
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Effective duration.
    let dur = effective_duration(AnimationType::WindowOpen);
    assert_eq!(dur, 200);
    crate::serial_println!("  [2/8] effective duration: OK");

    // 3: Speed multiplier.
    set_speed(200).expect("speed");
    let dur = effective_duration(AnimationType::WindowOpen);
    assert_eq!(dur, 400); // 200 * 200%
    set_speed(100).expect("reset");
    crate::serial_println!("  [3/8] speed multiplier: OK");

    // 4: Reduce motion.
    set_reduce_motion(true).expect("reduce");
    let dur = effective_duration(AnimationType::WindowOpen);
    assert_eq!(dur, 0);
    set_reduce_motion(false).expect("unreduced");
    crate::serial_println!("  [4/8] reduce motion: OK");

    // 5: Custom duration.
    set_duration(AnimationType::MenuOpen, 50).expect("dur");
    let dur = effective_duration(AnimationType::MenuOpen);
    assert_eq!(dur, 50);
    crate::serial_println!("  [5/8] custom duration: OK");

    // 6: Custom easing.
    set_easing(AnimationType::WindowSnap, EasingCurve::Spring).expect("ease");
    let anims = list_animations();
    let snap = anims.iter().find(|a| a.animation_type == AnimationType::WindowSnap).unwrap();
    assert_eq!(snap.easing, EasingCurve::Spring);
    crate::serial_println!("  [6/8] custom easing: OK");

    // 7: Disable specific animation.
    set_animation_enabled(AnimationType::TooltipFade, false).expect("dis");
    let dur = effective_duration(AnimationType::TooltipFade);
    assert_eq!(dur, 0);
    crate::serial_println!("  [7/8] disable specific: OK");

    // 8: Stats.
    let (count, enabled, changes, ops) = stats();
    assert_eq!(count, 12);
    assert_eq!(enabled, 11); // one disabled
    assert!(changes >= 5);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("sysanimations::self_test() — all 8 tests passed");
}
