//! Wayland `wl_keyboard` — keyboard event type constants.
//!
//! The `wl_keyboard` interface handles key press/release events,
//! keymap distribution (via XKB), modifier state, and keyboard
//! focus enter/leave. The keymap is typically an XKB keymap sent
//! as a file descriptor from the compositor.

// ---------------------------------------------------------------------------
// Key state (wl_keyboard.key_state)
// ---------------------------------------------------------------------------

/// Key is released.
pub const WL_KEYBOARD_KEY_STATE_RELEASED: u32 = 0;
/// Key is pressed.
pub const WL_KEYBOARD_KEY_STATE_PRESSED: u32 = 1;

// ---------------------------------------------------------------------------
// Keymap format (wl_keyboard.keymap_format)
// ---------------------------------------------------------------------------

/// No keymap (compositor provides no layout info).
pub const WL_KEYBOARD_KEYMAP_FORMAT_NO_KEYMAP: u32 = 0;
/// XKB v1 keymap (standard for Wayland/X11).
pub const WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1: u32 = 1;

// ---------------------------------------------------------------------------
// Common XKB modifier indices
// ---------------------------------------------------------------------------

/// Shift modifier index.
pub const WL_KEYBOARD_MOD_SHIFT: u32 = 0;
/// Caps Lock modifier index.
pub const WL_KEYBOARD_MOD_CAPS: u32 = 1;
/// Control modifier index.
pub const WL_KEYBOARD_MOD_CTRL: u32 = 2;
/// Alt/Mod1 modifier index.
pub const WL_KEYBOARD_MOD_ALT: u32 = 3;
/// Num Lock modifier index.
pub const WL_KEYBOARD_MOD_NUM: u32 = 4;
/// Super/Logo modifier index.
pub const WL_KEYBOARD_MOD_LOGO: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_states_distinct() {
        assert_ne!(
            WL_KEYBOARD_KEY_STATE_RELEASED,
            WL_KEYBOARD_KEY_STATE_PRESSED
        );
    }

    #[test]
    fn test_keymap_formats_distinct() {
        assert_ne!(
            WL_KEYBOARD_KEYMAP_FORMAT_NO_KEYMAP,
            WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1
        );
    }

    #[test]
    fn test_modifier_indices_distinct() {
        let mods = [
            WL_KEYBOARD_MOD_SHIFT,
            WL_KEYBOARD_MOD_CAPS,
            WL_KEYBOARD_MOD_CTRL,
            WL_KEYBOARD_MOD_ALT,
            WL_KEYBOARD_MOD_NUM,
            WL_KEYBOARD_MOD_LOGO,
        ];
        for i in 0..mods.len() {
            for j in (i + 1)..mods.len() {
                assert_ne!(mods[i], mods[j]);
            }
        }
    }

    #[test]
    fn test_modifier_indices_sequential() {
        assert_eq!(WL_KEYBOARD_MOD_SHIFT, 0);
        assert_eq!(WL_KEYBOARD_MOD_CAPS, 1);
        assert_eq!(WL_KEYBOARD_MOD_CTRL, 2);
        assert_eq!(WL_KEYBOARD_MOD_ALT, 3);
        assert_eq!(WL_KEYBOARD_MOD_NUM, 4);
        assert_eq!(WL_KEYBOARD_MOD_LOGO, 5);
    }
}
