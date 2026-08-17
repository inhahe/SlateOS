//! Scancode → key-name translation, and modifier-state tracking.
//!
//! A keyboard reports *which switch closed*, not *which letter it means*: the
//! same physical key is `A` on a US layout and `Q` on a French one. Something
//! has to bridge that, and `design-decisions.md` §456 puts it here, in the
//! compositor, rather than in each client — so that one system keymap governs
//! every application and a layout change takes effect everywhere at once
//! instead of app by app as each notices. Clients receive a
//! [`Key`](guitk::event::Key); the raw scancode is forwarded alongside for the
//! few that want physical positions (games binding "the key left of S",
//! remapping utilities) rather than letters.
//!
//! ## Scancode convention
//!
//! Codes are **scan code set 1**, which is what the PS/2 path produces after
//! the i8042's set-2→set-1 translation (`kernel/src/keyboard.rs`) and what the
//! USB HID path is translated into (`HID_TO_SCANCODE`). Extended keys — the
//! ones the hardware prefixes with `0xE0` — are represented with that prefix in
//! the high byte: `0xE04B` is Left arrow, while a bare `0x4B` is keypad 4. They
//! must stay distinguishable, because they are different keys with the same low
//! byte, and the arrows are precisely the keys a text editor cares about most.
//!
//! ## What this is not
//!
//! This is a US-QWERTY table, and it is the *only* table. That is a known
//! limitation rather than a design position: the structure — one central map
//! consulted per key event — is what a real layout system needs, but selecting
//! between layouts, and the dead-key and compose sequences a non-English layout
//! requires, are not built. See `known-issues.md` →
//! `TD-ONLY-ONE-KEYBOARD-LAYOUT`.

use guitk::event::{Key, Modifiers};

/// Translate a scan-code-set-1 code to a key name.
///
/// Returns [`Key::Unknown`] carrying the raw code rather than dropping it: a
/// key this table does not know about should still reach the client as *a* key
/// press, so that a client which does understand it (a remapper, a game reading
/// physical positions) is not blocked by a gap in the compositor's table.
#[must_use]
pub const fn key_for_scancode(scancode: u32) -> Key {
    match scancode {
        // --- Row 1: escape and the number row ---
        0x01 => Key::Escape,
        0x02 => Key::Num1,
        0x03 => Key::Num2,
        0x04 => Key::Num3,
        0x05 => Key::Num4,
        0x06 => Key::Num5,
        0x07 => Key::Num6,
        0x08 => Key::Num7,
        0x09 => Key::Num8,
        0x0A => Key::Num9,
        0x0B => Key::Num0,
        0x0C => Key::Minus,
        0x0D => Key::Equals,
        0x0E => Key::Backspace,

        // --- Row 2 ---
        0x0F => Key::Tab,
        0x10 => Key::Q,
        0x11 => Key::W,
        0x12 => Key::E,
        0x13 => Key::R,
        0x14 => Key::T,
        0x15 => Key::Y,
        0x16 => Key::U,
        0x17 => Key::I,
        0x18 => Key::O,
        0x19 => Key::P,
        0x1A => Key::LeftBracket,
        0x1B => Key::RightBracket,
        0x1C => Key::Enter,

        // --- Row 3 ---
        0x1D => Key::LeftCtrl,
        0x1E => Key::A,
        0x1F => Key::S,
        0x20 => Key::D,
        0x21 => Key::F,
        0x22 => Key::G,
        0x23 => Key::H,
        0x24 => Key::J,
        0x25 => Key::K,
        0x26 => Key::L,
        0x27 => Key::Semicolon,
        0x28 => Key::Apostrophe,
        0x29 => Key::Grave,

        // --- Row 4 ---
        0x2A => Key::LeftShift,
        0x2B => Key::Backslash,
        0x2C => Key::Z,
        0x2D => Key::X,
        0x2E => Key::C,
        0x2F => Key::V,
        0x30 => Key::B,
        0x31 => Key::N,
        0x32 => Key::M,
        0x33 => Key::Comma,
        0x34 => Key::Period,
        0x35 => Key::Slash,
        0x36 => Key::RightShift,

        // --- Row 5 ---
        0x38 => Key::LeftAlt,
        0x39 => Key::Space,
        0x3A => Key::CapsLock,

        // --- Function keys. F11/F12 sit at 0x57/0x58, well away from F1-F10,
        //     because they were added to the PC keyboard years later. ---
        0x3B => Key::F1,
        0x3C => Key::F2,
        0x3D => Key::F3,
        0x3E => Key::F4,
        0x3F => Key::F5,
        0x40 => Key::F6,
        0x41 => Key::F7,
        0x42 => Key::F8,
        0x43 => Key::F9,
        0x44 => Key::F10,
        0x57 => Key::F11,
        0x58 => Key::F12,

        0x45 => Key::NumLock,
        0x46 => Key::ScrollLock,

        // --- Extended (0xE0-prefixed) keys. The navigation cluster and the
        //     right-hand modifiers, all of which share a low byte with a keypad
        //     key and so must keep the prefix. ---
        //
        // The keypad duplicates (0x47-0x53 unprefixed) are deliberately absent:
        // `Key` has no `Numpad7` to map them to, so they arrive as
        // `Key::Unknown` with their code intact rather than being silently
        // conflated with the navigation keys that share their numbers. Merging
        // them would make Home and keypad-7 indistinguishable to every client.
        0xE048 => Key::Up,
        0xE050 => Key::Down,
        0xE04B => Key::Left,
        0xE04D => Key::Right,
        0xE047 => Key::Home,
        0xE04F => Key::End,
        0xE049 => Key::PageUp,
        0xE051 => Key::PageDown,
        0xE052 => Key::Insert,
        0xE053 => Key::Delete,
        0xE01C => Key::Enter, // keypad Enter — the same key name, correctly
        0xE01D => Key::RightCtrl,
        0xE038 => Key::RightAlt,
        0xE05B => Key::LeftSuper,
        0xE05C => Key::RightSuper,
        0xE037 => Key::PrintScreen,

        // Pause is the one genuinely irregular key: the hardware sends a
        // six-byte sequence beginning 0xE1, which the driver collapses to this.
        0xE11D => Key::Pause,

        other => Key::Unknown(other),
    }
}

/// Whether a scancode names a key whose *state* is a modifier.
///
/// Used to keep [`ModifierState`] in step; kept next to the table it reads so
/// the two cannot drift.
const fn modifier_of(scancode: u32) -> Option<ModifierBit> {
    match scancode {
        0x2A | 0x36 => Some(ModifierBit::Shift),
        0x1D | 0xE01D => Some(ModifierBit::Ctrl),
        0x38 | 0xE038 => Some(ModifierBit::Alt),
        0xE05B | 0xE05C => Some(ModifierBit::Super),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModifierBit {
    Shift,
    Ctrl,
    Alt,
    Super,
}

/// Which modifier keys are currently held.
///
/// Tracked per physical side rather than as four booleans, because releasing
/// one Shift while the other is still down must not clear the modifier —
/// the naive single-flag version drops the shift halfway through a
/// two-handed capital, which is a bug the user experiences as random
/// lowercase letters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModifierState {
    left_shift: bool,
    right_shift: bool,
    left_ctrl: bool,
    right_ctrl: bool,
    left_alt: bool,
    right_alt: bool,
    left_super: bool,
    right_super: bool,
    /// Caps Lock is a *latch*, not a held key: it toggles on press and is
    /// unaffected by release.
    caps_lock: bool,
}

impl ModifierState {
    /// A fresh state with nothing held.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            left_shift: false,
            right_shift: false,
            left_ctrl: false,
            right_ctrl: false,
            left_alt: false,
            right_alt: false,
            left_super: false,
            right_super: false,
            caps_lock: false,
        }
    }

    /// Fold a key event into the state. Call this for *every* key event, before
    /// reading [`Self::modifiers`], so that the modifiers reported with a
    /// chord include the modifier that was pressed to form it.
    pub const fn update(&mut self, scancode: u32, pressed: bool) {
        if scancode == 0x3A {
            // Toggle on the press edge only. Toggling on release too would
            // return it to where it started, so Caps Lock would do nothing —
            // which is exactly the bug that makes a latch look like a held key.
            if pressed {
                self.caps_lock = !self.caps_lock;
            }
            return;
        }
        match scancode {
            0x2A => self.left_shift = pressed,
            0x36 => self.right_shift = pressed,
            0x1D => self.left_ctrl = pressed,
            0xE01D => self.right_ctrl = pressed,
            0x38 => self.left_alt = pressed,
            0xE038 => self.right_alt = pressed,
            0xE05B => self.left_super = pressed,
            0xE05C => self.right_super = pressed,
            _ => {}
        }
    }

    /// The state as clients see it — sides collapsed, because a widget asking
    /// "was Ctrl held?" does not care which one.
    #[must_use]
    pub const fn modifiers(self) -> Modifiers {
        Modifiers {
            shift: self.left_shift || self.right_shift,
            ctrl: self.left_ctrl || self.right_ctrl,
            alt: self.left_alt || self.right_alt,
            super_key: self.left_super || self.right_super,
        }
    }

    /// Whether Caps Lock is latched on.
    #[must_use]
    pub const fn caps_lock(self) -> bool {
        self.caps_lock
    }

    /// Whether letters should currently come out capitalised.
    ///
    /// Caps Lock and Shift *cancel* rather than combine: with the latch on,
    /// holding Shift gives lowercase. Every mainstream keyboard behaves this
    /// way, and it is an exclusive-or rather than an or.
    #[must_use]
    pub const fn upper_case(self) -> bool {
        (self.left_shift || self.right_shift) != self.caps_lock
    }

    /// Release everything held.
    ///
    /// Called when the compositor loses the input device or the session is
    /// switched away: without it, a modifier held at the moment focus left
    /// stays held forever, and every subsequent keystroke arrives as a chord —
    /// the classic "stuck Ctrl" that makes a desktop appear to have crashed.
    pub const fn release_all(&mut self) {
        let caps = self.caps_lock;
        *self = Self::new();
        // The latch survives: it is a setting, not a key that is down.
        self.caps_lock = caps;
    }

    /// Whether this scancode is a modifier key at all.
    #[must_use]
    pub const fn is_modifier(scancode: u32) -> bool {
        modifier_of(scancode).is_some() || scancode == 0x3A
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use super::*;

    /// The `0xE0` prefix, in the high byte of an extended key's code. The table
    /// above spells its codes out literally — `0xE04B` reads as one key, where
    /// `EXTENDED | 0x4B` cannot even be written as a match pattern — so this
    /// lives here, where the sweep over every code needs it.
    const EXTENDED: u32 = 0xE000;

    #[test]
    fn the_home_row_maps_to_its_letters() {
        // Anchored against kernel/src/keyboard.rs's set-1 table, which is the
        // authority for what these codes mean on the way in.
        assert_eq!(key_for_scancode(0x1E), Key::A);
        assert_eq!(key_for_scancode(0x1F), Key::S);
        assert_eq!(key_for_scancode(0x20), Key::D);
        assert_eq!(key_for_scancode(0x21), Key::F);
        assert_eq!(key_for_scancode(0x26), Key::L);
    }

    #[test]
    fn every_letter_and_digit_is_mapped_exactly_once() {
        // A table where two codes both produce `K` types a doubled letter and
        // loses another one entirely; counting catches that where spot checks
        // do not.
        let letters = [
            Key::A,
            Key::B,
            Key::C,
            Key::D,
            Key::E,
            Key::F,
            Key::G,
            Key::H,
            Key::I,
            Key::J,
            Key::K,
            Key::L,
            Key::M,
            Key::N,
            Key::O,
            Key::P,
            Key::Q,
            Key::R,
            Key::S,
            Key::T,
            Key::U,
            Key::V,
            Key::W,
            Key::X,
            Key::Y,
            Key::Z,
            Key::Num0,
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ];
        for expected in letters {
            let hits: Vec<u32> = (0..0x100u32)
                .filter(|&c| key_for_scancode(c) == expected)
                .collect();
            assert_eq!(hits.len(), 1, "{expected:?} matched codes {hits:?}");
        }
    }

    #[test]
    fn the_arrow_keys_need_their_extended_prefix() {
        // The whole reason the prefix is kept. Without it, Left arrow and
        // keypad 4 are the same code, and a text editor's most-used key
        // becomes ambiguous.
        assert_eq!(key_for_scancode(0xE04B), Key::Left);
        assert_eq!(key_for_scancode(0xE04D), Key::Right);
        assert_eq!(key_for_scancode(0xE048), Key::Up);
        assert_eq!(key_for_scancode(0xE050), Key::Down);
        assert_eq!(key_for_scancode(0x4B), Key::Unknown(0x4B));
    }

    #[test]
    fn the_navigation_cluster_is_not_conflated_with_the_keypad() {
        for (extended, bare) in [
            (0xE047u32, 0x47u32),
            (0xE04F, 0x4F),
            (0xE049, 0x49),
            (0xE051, 0x51),
            (0xE052, 0x52),
            (0xE053, 0x53),
        ] {
            assert_ne!(
                key_for_scancode(extended),
                key_for_scancode(bare),
                "code {extended:#x} and {bare:#x} must stay distinct"
            );
            assert_eq!(key_for_scancode(bare), Key::Unknown(bare));
        }
    }

    #[test]
    fn an_unmapped_code_keeps_its_value() {
        assert_eq!(key_for_scancode(0x7F), Key::Unknown(0x7F));
        assert_eq!(key_for_scancode(0xFFFF), Key::Unknown(0xFFFF));
    }

    #[test]
    fn both_sides_of_a_modifier_produce_the_same_flag() {
        for (left, right) in [(0x2Au32, 0x36u32), (0x1D, 0xE01D), (0x38, 0xE038)] {
            let mut a = ModifierState::new();
            a.update(left, true);
            let mut b = ModifierState::new();
            b.update(right, true);
            assert_eq!(a.modifiers(), b.modifiers(), "{left:#x} vs {right:#x}");
        }
    }

    #[test]
    fn releasing_one_shift_does_not_clear_the_other() {
        // The bug this design exists to prevent: a two-handed capital where the
        // user lets go of the first Shift a moment early.
        let mut m = ModifierState::new();
        m.update(0x2A, true); // left shift down
        m.update(0x36, true); // right shift down
        m.update(0x2A, false); // left shift up
        assert!(
            m.modifiers().shift,
            "shift must survive while one side is held"
        );
        m.update(0x36, false);
        assert!(!m.modifiers().shift);
    }

    #[test]
    fn caps_lock_toggles_on_press_and_ignores_release() {
        let mut m = ModifierState::new();
        assert!(!m.caps_lock());
        m.update(0x3A, true);
        assert!(m.caps_lock());
        m.update(0x3A, false); // release must not toggle back
        assert!(m.caps_lock());
        m.update(0x3A, true);
        assert!(!m.caps_lock());
    }

    #[test]
    fn caps_lock_and_shift_cancel_rather_than_combine() {
        let mut m = ModifierState::new();
        m.update(0x3A, true); // caps on
        assert!(m.upper_case());
        m.update(0x2A, true); // shift down as well
        assert!(!m.upper_case(), "shift with caps on gives lowercase");
    }

    #[test]
    fn releasing_everything_clears_held_keys_but_keeps_the_latch() {
        let mut m = ModifierState::new();
        m.update(0x1D, true);
        m.update(0x2A, true);
        m.update(0x3A, true);
        m.release_all();
        assert_eq!(m.modifiers(), Modifiers::NONE, "no key may stay stuck down");
        assert!(m.caps_lock(), "the latch is a setting, not a held key");
    }

    #[test]
    fn a_modifier_is_recognised_as_one() {
        for code in [
            0x2Au32, 0x36, 0x1D, 0xE01D, 0x38, 0xE038, 0xE05B, 0xE05C, 0x3A,
        ] {
            assert!(ModifierState::is_modifier(code), "{code:#x}");
        }
        for code in [0x1Eu32, 0xE04B, 0x39] {
            assert!(!ModifierState::is_modifier(code), "{code:#x}");
        }
    }

    #[test]
    fn the_modifier_table_and_the_update_arms_agree() {
        // `modifier_of` and `update` are two lists of the same codes; if one
        // gains a key the other must too, or a modifier is reported as held by
        // one and not the other.
        for code in 0..0x100u32 {
            for prefixed in [code, EXTENDED | code] {
                let claimed = modifier_of(prefixed).is_some();
                let mut m = ModifierState::new();
                m.update(prefixed, true);
                let observed = m.modifiers() != Modifiers::NONE;
                assert_eq!(claimed, observed, "code {prefixed:#x}");
            }
        }
    }

    #[test]
    fn no_scancode_ever_panics() {
        // The compositor takes these from a driver; a table that panics on an
        // unexpected code takes the whole desktop down with it.
        for code in 0..0x1_0000u32 {
            let _ = key_for_scancode(code);
            let mut m = ModifierState::new();
            m.update(code, true);
            m.update(code, false);
        }
    }
}
