//! The list of keys a game raises with F1, answered alike in every game.
//!
//! Every program in this tree raises its list of keys with F1 -- the
//! toolkit's card, `guitk::shortcut::render_card` -- and seven games drew
//! none, so their keys could be found only by pressing them: the history's
//! among them, Ctrl+Y and Alt+Z, which the operator's answer to C-Q24
//! (`design-decisions.md` §1416) added. A game keeps whether its list is up,
//! draws the card over everything while it is, and asks [`raises`] and
//! [`closes`] about each key.
//!
//! While a game's list is up it is the window's: a move made under the card
//! would be one the player cannot see. The game takes no other key then, and
//! a click anywhere puts the card away.

use guitk::event::{Key, KeyEvent};

/// What a game's card says puts it away.
pub const CLOSES: &str = "F1, ? or Esc closes this";

/// Whether `key` raises the list: F1, or `?` -- the slash key with Shift, as
/// `guitk::shortcut::keystrokes` spells it, or whichever key a layout puts
/// it on (German has it on the key right of 0). Never a release, and never
/// with Ctrl, Alt or the Windows key held: those are other programs' keys,
/// and the desktop's.
#[must_use]
pub fn raises(key: &KeyEvent) -> bool {
    let m = key.modifiers;
    if !key.pressed || m.ctrl || m.alt || m.super_key {
        return false;
    }
    key.key == Key::F1 || (key.key == Key::Slash && m.shift) || key.single_char() == Some('?')
}

/// Whether `key`, pressed while the list is up, puts it away: what raised
/// it, Escape, or Enter.
#[must_use]
pub fn closes(key: &KeyEvent) -> bool {
    raises(key) || (key.pressed && matches!(key.key, Key::Escape | Key::Enter))
}

#[cfg(test)]
mod tests {
    use super::*;
    use guitk::event::Modifiers;

    fn stroke(key: Key, modifiers: Modifiers, text: &str) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: text.to_owned(),
        }
    }

    #[test]
    fn f1_and_a_question_mark_raise_the_list() {
        assert!(raises(&stroke(Key::F1, Modifiers::NONE, "")));
        // As a key list spells it: the slash key with Shift, no text.
        assert!(raises(&stroke(Key::Slash, Modifiers::shift(), "")));
        // As a German keyboard types it: Shift and the key right of 0,
        // which the compositor names by its place on a US board.
        assert!(raises(&stroke(Key::Minus, Modifiers::shift(), "?")));
        // The slash key alone is `/`.
        assert!(!raises(&stroke(Key::Slash, Modifiers::NONE, "/")));
        assert!(!raises(&stroke(Key::H, Modifiers::NONE, "h")));
    }

    #[test]
    fn nothing_held_with_ctrl_alt_or_the_windows_key_raises_it() {
        let altgr = Modifiers {
            ctrl: true,
            alt: true,
            ..Modifiers::NONE
        };
        for held in [
            Modifiers::ctrl(),
            Modifiers::alt(),
            Modifiers::super_key(),
            altgr,
        ] {
            assert!(!raises(&stroke(Key::F1, held, "")), "{held:?}+F1");
            assert!(!raises(&stroke(Key::Minus, held, "?")), "{held:?}+?");
        }
        let mut release = stroke(Key::F1, Modifiers::NONE, "");
        release.pressed = false;
        assert!(!raises(&release));
    }

    #[test]
    fn escape_enter_and_what_raised_it_put_it_away() {
        for key in [Key::Escape, Key::Enter, Key::F1] {
            assert!(closes(&stroke(key, Modifiers::NONE, "")), "{key:?}");
        }
        assert!(closes(&stroke(Key::Slash, Modifiers::shift(), "")));
        assert!(!closes(&stroke(Key::Space, Modifiers::NONE, " ")));
        assert!(!closes(&stroke(Key::F1, Modifiers::alt(), "")));
        let mut release = stroke(Key::Escape, Modifiers::NONE, "");
        release.pressed = false;
        assert!(!closes(&release));
    }
}
