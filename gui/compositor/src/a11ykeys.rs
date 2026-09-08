//! Sticky, filter and mouse keys: the live state behind the accessibility
//! settings.
//!
//! The compositor is the only place these can work, because it is the only
//! place every keystroke passes through. `inputsettings` owns what the user
//! *chose*; this module owns what is *currently true* — which modifier is
//! stuck down, when each key was last accepted, how far a held arrow has
//! accelerated. That split is the one `inputsettings`' own module doc draws,
//! and it is why the state machines are here rather than beside the settings.
//!
//! # Why this exists at all
//!
//! All three features were already written, in `desktop::a11y`, complete with
//! thorough tests — and constructed by nothing but those tests. Turning on
//! Sticky Keys did nothing whatsoever. See `known-issues.md`
//! `TD-C-STICKY-FILTER-AND-MOUSE-KEYS-ARE-BUILT-TESTED-AND-CONNECTED-TO-NOTHING`.
//! The logic here is that logic, moved to where the key events are and driven
//! from the shared config rather than from fields nobody set.
//!
//! # What is deliberately not decided here
//!
//! *When* a slow key is delivered. [`FilterKeys::on_press`] answers "should
//! this keystroke count", given the time it was held; it does not choose
//! whether the compositor learns that on release or from a timer. That is a
//! scheduling question with a visible consequence for typing, and it belongs
//! to the caller.

use inputsettings::{AccessibilityKeysConfig, FilterKeysConfig, MouseKeysConfig, StickyKeysConfig};

/// Which modifier a sticky-keys event is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickyModifier {
    Ctrl,
    Alt,
    Shift,
    Super,
}

impl StickyModifier {
    /// Every modifier, for iterating without naming them four times.
    pub const ALL: [Self; 4] = [Self::Ctrl, Self::Alt, Self::Shift, Self::Super];
}

/// State of one sticky modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum StickyState {
    /// Not held.
    #[default]
    Off,
    /// Applies to the next key, then clears.
    Sticky,
    /// Stays until pressed again.
    Locked,
}

/// The four modifiers' sticky state.
#[derive(Debug, Clone, Default)]
pub struct StickyKeys {
    config: StickyKeysConfig,
    ctrl: StickyState,
    alt: StickyState,
    shift: StickyState,
    super_key: StickyState,
}

impl StickyKeys {
    /// A fresh state machine for the given settings.
    #[must_use]
    pub fn new(config: StickyKeysConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    /// Adopt changed settings.
    ///
    /// Turning the feature *off* clears the held modifiers as well. Leaving
    /// them set would strand a Shift that the user can no longer see or
    /// release: the indicator is gone and the key that would clear it is no
    /// longer being intercepted.
    pub fn set_config(&mut self, config: StickyKeysConfig) {
        let was = self.config.enabled;
        self.config = config;
        if was && !config.enabled {
            self.reset();
        }
    }

    /// The settings in force.
    #[must_use]
    pub fn config(&self) -> StickyKeysConfig {
        self.config
    }

    /// A modifier key was pressed. Answers whether anything changed.
    ///
    /// Off → Sticky → Locked → Off when double-press locking is on, and
    /// Off → Sticky → Off when it is not.
    pub fn on_modifier_press(&mut self, modifier: StickyModifier) -> bool {
        if !self.config.enabled {
            return false;
        }
        let lock = self.config.lock_on_double_press;
        let state = self.state_mut(modifier);
        *state = match *state {
            StickyState::Off => StickyState::Sticky,
            StickyState::Sticky if lock => StickyState::Locked,
            StickyState::Sticky | StickyState::Locked => StickyState::Off,
        };
        true
    }

    /// A non-modifier key was pressed. Answers which modifiers apply to it.
    ///
    /// Order is ctrl, alt, shift, super. Sticky modifiers clear afterwards;
    /// locked ones stay.
    pub fn on_key_press(&mut self) -> StickyModifiers {
        if !self.config.enabled {
            return StickyModifiers::default();
        }
        let active = StickyModifiers {
            ctrl: self.ctrl != StickyState::Off,
            alt: self.alt != StickyState::Off,
            shift: self.shift != StickyState::Off,
            super_key: self.super_key != StickyState::Off,
        };
        for m in StickyModifier::ALL {
            let state = self.state_mut(m);
            if *state == StickyState::Sticky {
                *state = StickyState::Off;
            }
        }
        active
    }

    /// Two keys were held together, which is the gesture that turns the mode
    /// off. Answers whether it did.
    ///
    /// Someone able to hold a chord evidently did not need sticky keys, so
    /// this is the escape hatch from having switched it on by accident —
    /// without a trip to Settings, which is hard to reach if the modifiers
    /// are misbehaving. Standard on Windows and macOS both.
    pub fn on_chord_held(&mut self) -> bool {
        if !self.config.enabled || !self.config.release_on_two_keys {
            return false;
        }
        self.config.enabled = false;
        self.reset();
        true
    }

    /// Whether a modifier is stuck or locked.
    #[must_use]
    pub fn is_active(&self, modifier: StickyModifier) -> bool {
        self.state(modifier) != StickyState::Off
    }

    /// Whether a modifier is locked rather than merely sticky.
    #[must_use]
    pub fn is_locked(&self, modifier: StickyModifier) -> bool {
        self.state(modifier) == StickyState::Locked
    }

    /// Whether anything is stuck down — what the on-screen indicator asks.
    #[must_use]
    pub fn any_active(&self) -> bool {
        StickyModifier::ALL.iter().any(|&m| self.is_active(m))
    }

    /// Clear every modifier.
    pub fn reset(&mut self) {
        self.ctrl = StickyState::Off;
        self.alt = StickyState::Off;
        self.shift = StickyState::Off;
        self.super_key = StickyState::Off;
    }

    fn state(&self, m: StickyModifier) -> StickyState {
        match m {
            StickyModifier::Ctrl => self.ctrl,
            StickyModifier::Alt => self.alt,
            StickyModifier::Shift => self.shift,
            StickyModifier::Super => self.super_key,
        }
    }

    fn state_mut(&mut self, m: StickyModifier) -> &mut StickyState {
        match m {
            StickyModifier::Ctrl => &mut self.ctrl,
            StickyModifier::Alt => &mut self.alt,
            StickyModifier::Shift => &mut self.shift,
            StickyModifier::Super => &mut self.super_key,
        }
    }
}

/// Which modifiers a sticky-keys press contributed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StickyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}

impl StickyModifiers {
    /// Whether any modifier is contributed — lets the caller skip the merge.
    #[must_use]
    pub fn any(self) -> bool {
        self.ctrl || self.alt || self.shift || self.super_key
    }
}

/// Why a keystroke was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    /// Held for less than the slow-keys threshold.
    TooBrief,
    /// The same key was accepted less than the bounce window ago.
    TooSoon,
}

/// Slow keys and bounce keys.
///
/// Tracks the last accepted time per key code in a small bounded list. Bounded
/// because the table is keyed by whatever the hardware sends and an unbounded
/// map here would be a memory leak driven by input.
#[derive(Debug, Clone, Default)]
pub struct FilterKeys {
    config: FilterKeysConfig,
    last_accepted: Vec<(u16, u64)>,
}

impl FilterKeys {
    /// How many distinct key codes the bounce table remembers.
    ///
    /// A keyboard has on the order of a hundred keys and bounce windows are
    /// measured in milliseconds, so the oldest entry evicted here is one that
    /// could not still be inside its window on any real keyboard.
    pub const MAX_TRACKED: usize = 64;

    /// A fresh state machine for the given settings.
    #[must_use]
    pub fn new(config: FilterKeysConfig) -> Self {
        Self {
            config,
            last_accepted: Vec::new(),
        }
    }

    /// Adopt changed settings, forgetting the bounce history if switched off.
    pub fn set_config(&mut self, config: FilterKeysConfig) {
        let was = self.config.enabled;
        self.config = config;
        if was && !config.enabled {
            self.last_accepted.clear();
        }
    }

    /// The settings in force.
    #[must_use]
    pub fn config(&self) -> FilterKeysConfig {
        self.config
    }

    /// Should this keystroke count?
    ///
    /// `held_ms` is how long the key was down, `now_ms` a monotonic clock.
    /// An accepted key records its time, so the caller must not call this
    /// speculatively — asking twice about one keystroke starts its bounce
    /// window from the second call.
    ///
    /// # Errors
    ///
    /// [`Rejected::TooBrief`] if it did not meet the slow-keys threshold,
    /// [`Rejected::TooSoon`] if the same key is still inside its bounce
    /// window.
    pub fn on_press(&mut self, key_code: u16, held_ms: u32, now_ms: u64) -> Result<(), Rejected> {
        if !self.config.enabled {
            return Ok(());
        }
        if held_ms < self.config.slow_keys_ms {
            return Err(Rejected::TooBrief);
        }
        if let Some(&(_, last)) = self.last_accepted.iter().find(|(k, _)| *k == key_code)
            && now_ms.saturating_sub(last) < u64::from(self.config.bounce_keys_ms)
        {
            return Err(Rejected::TooSoon);
        }
        self.record(key_code, now_ms);
        Ok(())
    }

    /// The slow-keys threshold, for a caller that must schedule around it.
    #[must_use]
    pub fn slow_keys_ms(&self) -> u32 {
        if self.config.enabled {
            self.config.slow_keys_ms
        } else {
            0
        }
    }

    fn record(&mut self, key_code: u16, now_ms: u64) {
        if let Some(entry) = self.last_accepted.iter_mut().find(|(k, _)| *k == key_code) {
            entry.1 = now_ms;
            return;
        }
        if self.last_accepted.len() >= Self::MAX_TRACKED {
            self.last_accepted.remove(0);
        }
        self.last_accepted.push((key_code, now_ms));
    }

    /// Forget every key's last-accepted time.
    pub fn reset(&mut self) {
        self.last_accepted.clear();
    }
}

/// What a mouse-keys keystroke asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseKeyAction {
    MoveUpLeft,
    MoveUp,
    MoveUpRight,
    MoveLeft,
    MoveRight,
    MoveDownLeft,
    MoveDown,
    MoveDownRight,
    Click,
    DoubleClick,
    RightClick,
}

impl MouseKeyAction {
    /// The direction this action moves, as unit steps, or `None` for a click.
    #[must_use]
    pub fn direction(self) -> Option<(f32, f32)> {
        let d = match self {
            Self::MoveUpLeft => (-1.0, -1.0),
            Self::MoveUp => (0.0, -1.0),
            Self::MoveUpRight => (1.0, -1.0),
            Self::MoveLeft => (-1.0, 0.0),
            Self::MoveRight => (1.0, 0.0),
            Self::MoveDownLeft => (-1.0, 1.0),
            Self::MoveDown => (0.0, 1.0),
            Self::MoveDownRight => (1.0, 1.0),
            Self::Click | Self::DoubleClick | Self::RightClick => return None,
        };
        Some(d)
    }

    /// The numeric-keypad key this action is bound to, by scancode.
    ///
    /// The standard AccessX layout: 7/8/9 and 1/2/3 are the diagonals and
    /// verticals, 4/6 horizontal, 5 clicks, 0 right-clicks and `+`
    /// double-clicks.
    #[must_use]
    pub fn from_keypad_scancode(scancode: u32) -> Option<Self> {
        Some(match scancode {
            0x47 => Self::MoveUpLeft,
            0x48 => Self::MoveUp,
            0x49 => Self::MoveUpRight,
            0x4B => Self::MoveLeft,
            0x4C => Self::Click,
            0x4D => Self::MoveRight,
            0x4F => Self::MoveDownLeft,
            0x50 => Self::MoveDown,
            0x51 => Self::MoveDownRight,
            0x52 => Self::RightClick,
            0x4E => Self::DoubleClick,
            _ => return None,
        })
    }
}

/// Pointer motion driven from the keypad.
#[derive(Debug, Clone, Default)]
pub struct MouseKeys {
    config: MouseKeysConfig,
    current_speed: f32,
}

impl MouseKeys {
    /// A fresh state machine for the given settings.
    #[must_use]
    pub fn new(config: MouseKeysConfig) -> Self {
        Self {
            config,
            current_speed: 0.0,
        }
    }

    /// Adopt changed settings, dropping any accumulated acceleration.
    pub fn set_config(&mut self, config: MouseKeysConfig) {
        self.config = config;
        self.current_speed = 0.0;
    }

    /// The settings in force.
    #[must_use]
    pub fn config(&self) -> MouseKeysConfig {
        self.config
    }

    /// How far the pointer moves for this action, accelerating while held.
    ///
    /// Returns `(0.0, 0.0)` when the feature is off or the action is a click,
    /// so a caller can apply the result unconditionally.
    pub fn move_delta(&mut self, action: MouseKeyAction) -> (f32, f32) {
        let Some((dx, dy)) = action.direction() else {
            return (0.0, 0.0);
        };
        if !self.config.enabled {
            return (0.0, 0.0);
        }
        // First step is `speed`: `0 * acceleration` is 0, and the `max` lifts
        // it. Thereafter it grows by the factor until it reaches the ceiling.
        self.current_speed = (self.current_speed * self.config.acceleration)
            .max(self.config.speed)
            .min(self.config.max_speed);
        (dx * self.current_speed, dy * self.current_speed)
    }

    /// Whether the keypad should be read as pointer control right now.
    #[must_use]
    pub fn intercepts_keypad(&self) -> bool {
        self.config.enabled && self.config.use_numpad
    }

    /// Drop the accumulated speed — call when no direction key is held.
    pub fn release(&mut self) {
        self.current_speed = 0.0;
    }
}

/// The three features together, as the compositor holds them.
#[derive(Debug, Clone, Default)]
pub struct AccessibilityKeys {
    pub sticky: StickyKeys,
    pub filter: FilterKeys,
    pub mouse: MouseKeys,
}

impl AccessibilityKeys {
    /// Build all three from one settings block.
    #[must_use]
    pub fn new(config: AccessibilityKeysConfig) -> Self {
        Self {
            sticky: StickyKeys::new(config.sticky),
            filter: FilterKeys::new(config.filter),
            mouse: MouseKeys::new(config.mouse),
        }
    }

    /// Adopt a changed settings block.
    pub fn set_config(&mut self, config: AccessibilityKeysConfig) {
        self.sticky.set_config(config.sticky);
        self.filter.set_config(config.filter);
        self.mouse.set_config(config.mouse);
    }

    /// Whether any feature is on.
    ///
    /// The key handler's fast path: false means the accessibility step can be
    /// skipped entirely, which is what keeps this free for everyone who never
    /// turns it on.
    #[must_use]
    pub fn any_enabled(&self) -> bool {
        self.sticky.config().enabled || self.filter.config().enabled || self.mouse.config().enabled
    }
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it. The defensive lints keep panics out of code that runs
    // on a user's machine, which this is not.
    // `float_cmp`: the speeds under test are exact small values produced by
    // multiplication and `min`, not accumulated error, so an exact comparison
    // is the assertion that means something.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::panic,
        clippy::float_cmp
    )]

    use super::*;

    fn sticky_on() -> StickyKeys {
        StickyKeys::new(StickyKeysConfig {
            enabled: true,
            ..StickyKeysConfig::default()
        })
    }

    #[test]
    fn a_disabled_sticky_keys_changes_nothing() {
        let mut s = StickyKeys::default();
        assert!(!s.on_modifier_press(StickyModifier::Shift));
        assert!(!s.is_active(StickyModifier::Shift));
        assert!(!s.on_key_press().any());
    }

    /// The whole point: press Shift, release it, press A, and A is shifted.
    #[test]
    fn a_released_modifier_still_applies_to_the_next_key() {
        let mut s = sticky_on();
        assert!(s.on_modifier_press(StickyModifier::Shift));
        assert!(s.is_active(StickyModifier::Shift));

        let mods = s.on_key_press();
        assert!(mods.shift, "the next key was not shifted");
        assert!(
            !s.is_active(StickyModifier::Shift),
            "a sticky modifier must clear after the key it applied to"
        );
    }

    #[test]
    fn a_second_press_locks_and_a_third_clears() {
        let mut s = sticky_on();
        s.on_modifier_press(StickyModifier::Ctrl);
        s.on_modifier_press(StickyModifier::Ctrl);
        assert!(s.is_locked(StickyModifier::Ctrl));

        // A locked modifier survives the keys it applies to.
        assert!(s.on_key_press().ctrl);
        assert!(s.is_locked(StickyModifier::Ctrl), "lock did not hold");

        s.on_modifier_press(StickyModifier::Ctrl);
        assert!(!s.is_active(StickyModifier::Ctrl));
    }

    #[test]
    fn without_double_press_locking_a_second_press_clears() {
        let mut s = StickyKeys::new(StickyKeysConfig {
            enabled: true,
            lock_on_double_press: false,
            ..StickyKeysConfig::default()
        });
        s.on_modifier_press(StickyModifier::Alt);
        s.on_modifier_press(StickyModifier::Alt);
        assert!(!s.is_active(StickyModifier::Alt));
    }

    #[test]
    fn modifiers_are_independent() {
        let mut s = sticky_on();
        s.on_modifier_press(StickyModifier::Shift);
        s.on_modifier_press(StickyModifier::Ctrl);
        let mods = s.on_key_press();
        assert!(mods.shift && mods.ctrl);
        assert!(!mods.alt && !mods.super_key);
    }

    /// Holding a real chord turns the feature off — the escape hatch for
    /// someone who switched it on by accident.
    #[test]
    fn holding_two_keys_at_once_switches_the_mode_off() {
        let mut s = sticky_on();
        s.on_modifier_press(StickyModifier::Shift);
        assert!(s.on_chord_held());
        assert!(!s.config().enabled);
        assert!(
            !s.any_active(),
            "switching off must not strand a modifier the user can no longer clear"
        );
    }

    #[test]
    fn the_escape_hatch_can_be_turned_off() {
        let mut s = StickyKeys::new(StickyKeysConfig {
            enabled: true,
            release_on_two_keys: false,
            ..StickyKeysConfig::default()
        });
        assert!(!s.on_chord_held());
        assert!(s.config().enabled);
    }

    /// Switching the feature off must not leave a modifier stuck down: the
    /// indicator is gone and the key that would clear it is no longer being
    /// intercepted.
    #[test]
    fn disabling_sticky_keys_clears_what_was_held() {
        let mut s = sticky_on();
        s.on_modifier_press(StickyModifier::Super);
        assert!(s.any_active());
        s.set_config(StickyKeysConfig::default());
        assert!(!s.any_active());
    }

    // -- Filter keys ----------------------------------------------------------

    fn filter(slow: u32, bounce: u32) -> FilterKeys {
        FilterKeys::new(FilterKeysConfig {
            enabled: true,
            slow_keys_ms: slow,
            bounce_keys_ms: bounce,
            ..FilterKeysConfig::default()
        })
    }

    #[test]
    fn a_disabled_filter_accepts_everything() {
        let mut f = FilterKeys::default();
        assert_eq!(f.on_press(30, 0, 0), Ok(()));
        assert_eq!(f.on_press(30, 0, 0), Ok(()), "and does not bounce either");
    }

    #[test]
    fn a_key_held_too_briefly_is_refused() {
        let mut f = filter(300, 0);
        assert_eq!(f.on_press(30, 299, 1_000), Err(Rejected::TooBrief));
        assert_eq!(f.on_press(30, 300, 1_000), Ok(()), "the threshold is met");
    }

    #[test]
    fn the_same_key_again_too_soon_is_refused() {
        let mut f = filter(0, 200);
        assert_eq!(f.on_press(30, 0, 1_000), Ok(()));
        assert_eq!(f.on_press(30, 0, 1_150), Err(Rejected::TooSoon));
        assert_eq!(f.on_press(30, 0, 1_200), Ok(()), "the window has passed");
    }

    /// Bounce is per key: a different key is not blocked by this one.
    #[test]
    fn a_different_key_is_not_bounced() {
        let mut f = filter(0, 500);
        assert_eq!(f.on_press(30, 0, 1_000), Ok(()));
        assert_eq!(f.on_press(31, 0, 1_001), Ok(()));
    }

    /// A refused key must not start a bounce window, or one tremor would
    /// silence the key for the whole window afterwards.
    #[test]
    fn a_refused_key_does_not_start_a_bounce_window() {
        let mut f = filter(300, 500);
        assert_eq!(f.on_press(30, 10, 1_000), Err(Rejected::TooBrief));
        assert_eq!(
            f.on_press(30, 300, 1_010),
            Ok(()),
            "a rejection blocked the deliberate press that followed it"
        );
    }

    #[test]
    fn the_bounce_table_is_bounded() {
        let mut f = filter(0, 10_000);
        for k in 0..(FilterKeys::MAX_TRACKED as u16 * 2) {
            assert_eq!(f.on_press(k, 0, 1_000), Ok(()));
        }
        assert!(f.last_accepted.len() <= FilterKeys::MAX_TRACKED);
    }

    #[test]
    fn a_clock_that_goes_backwards_does_not_panic() {
        let mut f = filter(0, 200);
        assert_eq!(f.on_press(30, 0, 5_000), Ok(()));
        // Earlier than the recorded time: saturating_sub gives 0, which is
        // inside any window, so it is refused rather than crashing.
        assert_eq!(f.on_press(30, 0, 1_000), Err(Rejected::TooSoon));
    }

    // -- Mouse keys -----------------------------------------------------------

    fn mouse_on() -> MouseKeys {
        MouseKeys::new(MouseKeysConfig {
            enabled: true,
            speed: 10.0,
            acceleration: 2.0,
            max_speed: 40.0,
            use_numpad: true,
        })
    }

    #[test]
    fn a_disabled_mouse_keys_does_not_move_the_pointer() {
        let mut m = MouseKeys::default();
        assert_eq!(m.move_delta(MouseKeyAction::MoveRight), (0.0, 0.0));
        assert!(!m.intercepts_keypad());
    }

    #[test]
    fn the_first_step_is_the_configured_speed() {
        let mut m = mouse_on();
        assert_eq!(m.move_delta(MouseKeyAction::MoveRight), (10.0, 0.0));
    }

    #[test]
    fn holding_a_direction_accelerates_up_to_the_ceiling() {
        let mut m = mouse_on();
        let first = m.move_delta(MouseKeyAction::MoveDown).1;
        let second = m.move_delta(MouseKeyAction::MoveDown).1;
        assert!(second > first, "a held key did not accelerate");
        for _ in 0..20 {
            m.move_delta(MouseKeyAction::MoveDown);
        }
        assert_eq!(
            m.move_delta(MouseKeyAction::MoveDown).1,
            40.0,
            "acceleration ran past the ceiling"
        );
    }

    #[test]
    fn releasing_the_key_drops_back_to_the_starting_speed() {
        let mut m = mouse_on();
        for _ in 0..10 {
            m.move_delta(MouseKeyAction::MoveRight);
        }
        m.release();
        assert_eq!(m.move_delta(MouseKeyAction::MoveRight), (10.0, 0.0));
    }

    #[test]
    fn a_click_action_does_not_move_the_pointer() {
        let mut m = mouse_on();
        for action in [
            MouseKeyAction::Click,
            MouseKeyAction::DoubleClick,
            MouseKeyAction::RightClick,
        ] {
            assert_eq!(m.move_delta(action), (0.0, 0.0), "{action:?} moved");
        }
    }

    #[test]
    fn the_diagonals_move_on_both_axes() {
        let mut m = mouse_on();
        assert_eq!(m.move_delta(MouseKeyAction::MoveUpLeft), (-10.0, -10.0));
    }

    /// The keypad map is the standard AccessX one, and 5 clicks rather than
    /// moving.
    #[test]
    fn the_keypad_maps_to_the_standard_actions() {
        assert_eq!(
            MouseKeyAction::from_keypad_scancode(0x48),
            Some(MouseKeyAction::MoveUp)
        );
        assert_eq!(
            MouseKeyAction::from_keypad_scancode(0x4C),
            Some(MouseKeyAction::Click)
        );
        assert_eq!(
            MouseKeyAction::from_keypad_scancode(0x1E),
            None,
            "a letter key must not be read as pointer control"
        );
    }

    /// Every keypad scancode maps to a distinct action, so no two keys do the
    /// same thing and none is unreachable.
    #[test]
    fn the_eleven_keypad_bindings_are_all_distinct() {
        let mut seen = Vec::new();
        for sc in 0x00..=0xFFu32 {
            if let Some(a) = MouseKeyAction::from_keypad_scancode(sc) {
                assert!(!seen.contains(&a), "{a:?} is bound to two scancodes");
                seen.push(a);
            }
        }
        assert_eq!(seen.len(), 11, "expected the eleven AccessX bindings");
    }

    // -- The three together ---------------------------------------------------

    #[test]
    fn any_enabled_gates_the_whole_step() {
        let mut keys = AccessibilityKeys::default();
        assert!(!keys.any_enabled());
        keys.set_config(AccessibilityKeysConfig {
            sticky: StickyKeysConfig {
                enabled: true,
                ..StickyKeysConfig::default()
            },
            ..AccessibilityKeysConfig::default()
        });
        assert!(keys.any_enabled());
    }
}
