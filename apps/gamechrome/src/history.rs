//! The keys a game's history answers, read the same way in every game.
//!
//! The operator's answer to C-Q24 (`design-decisions.md` §1416) asks every
//! program that can undo for Ctrl+Z and Ctrl+Shift+Z, and for a redo tree:
//! undoing and then doing something new keeps what was undone, as a branch.
//! A game keeps its moves in the toolkit's `guitk::undo::UndoHistory`, which
//! is that tree, and reads its keys here:
//!
//! | Keys | Asks for |
//! |---|---|
//! | Ctrl+Z | [`HistoryKey::Undo`] -- the move before, on the branch the game is on |
//! | Ctrl+Y, Ctrl+Shift+Z | [`HistoryKey::Redo`] -- the move after, on that branch |
//! | Alt+Z | [`HistoryKey::Earlier`] -- the position reached before this one, on any branch |
//! | Alt+Shift+Z | [`HistoryKey::Later`] -- the position reached after this one, on any branch |
//!
//! Ctrl means Ctrl without Alt: AltGr arrives as Ctrl+Alt, and is how several
//! layouts type a letter -- Polish AltGr+Z is ż. Alt means Alt without Ctrl.
//! Nothing held with the Windows key is a game's: those keys are the
//! desktop's.
//!
//! A game's own undo key -- a bare `Z` or `U` in several -- stays the game's,
//! beside these.

use guitk::event::{Key, KeyEvent};

/// What a key asks of a game's history. See the [module docs](self).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryKey {
    /// Ctrl+Z: take back the last move.
    Undo,
    /// Ctrl+Y or Ctrl+Shift+Z: make again the move last taken back, on the
    /// branch the game is on.
    Redo,
    /// Alt+Z: the position reached just before this one, on whichever
    /// branch -- the way back to moves undone and then played over.
    Earlier,
    /// Alt+Shift+Z: the position reached just after this one.
    Later,
}

impl HistoryKey {
    /// The history key `key` is, if it is one: a press, not a release, of Z
    /// or Y held as the [module docs](self) say.
    #[must_use]
    pub fn of(key: &KeyEvent) -> Option<Self> {
        let m = key.modifiers;
        if !key.pressed || m.super_key {
            return None;
        }
        match (key.key, m.ctrl, m.alt, m.shift) {
            (Key::Z, true, false, false) => Some(Self::Undo),
            (Key::Z, true, false, true) | (Key::Y, true, false, false) => Some(Self::Redo),
            (Key::Z, false, true, false) => Some(Self::Earlier),
            (Key::Z, false, true, true) => Some(Self::Later),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guitk::event::Modifiers;

    fn with(key: Key, ctrl: bool, alt: bool, shift: bool, super_key: bool) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers {
                ctrl,
                alt,
                shift,
                super_key,
            },
            text: String::new(),
        }
    }

    #[test]
    fn the_four_are_read() {
        let of = |key, ctrl, alt, shift| HistoryKey::of(&with(key, ctrl, alt, shift, false));
        assert_eq!(of(Key::Z, true, false, false), Some(HistoryKey::Undo));
        assert_eq!(of(Key::Y, true, false, false), Some(HistoryKey::Redo));
        assert_eq!(of(Key::Z, true, false, true), Some(HistoryKey::Redo));
        assert_eq!(of(Key::Z, false, true, false), Some(HistoryKey::Earlier));
        assert_eq!(of(Key::Z, false, true, true), Some(HistoryKey::Later));
    }

    /// AltGr arrives as Ctrl+Alt and types a letter: it is none of them.
    #[test]
    fn altgr_is_neither_ctrl_nor_alt() {
        for (key, shift) in [(Key::Z, false), (Key::Z, true), (Key::Y, false)] {
            assert_eq!(
                HistoryKey::of(&with(key, true, true, shift, false)),
                None,
                "AltGr+{key:?} (shift {shift})"
            );
        }
    }

    /// A key held with the Windows key is the desktop's.
    #[test]
    fn nothing_with_the_windows_key() {
        for (ctrl, alt, shift) in [
            (true, false, false),
            (true, false, true),
            (false, true, false),
            (false, true, true),
        ] {
            assert_eq!(HistoryKey::of(&with(Key::Z, ctrl, alt, shift, true)), None);
        }
        assert_eq!(
            HistoryKey::of(&with(Key::Y, true, false, false, true)),
            None
        );
    }

    /// A bare Z is a game's own to bind, and a release is not a press.
    #[test]
    fn a_bare_key_or_a_release_is_not_one() {
        assert_eq!(
            HistoryKey::of(&with(Key::Z, false, false, false, false)),
            None
        );
        assert_eq!(
            HistoryKey::of(&with(Key::Z, false, false, true, false)),
            None
        );
        assert_eq!(
            HistoryKey::of(&with(Key::Y, false, false, false, false)),
            None
        );
        let mut released = with(Key::Z, true, false, false, false);
        released.pressed = false;
        assert_eq!(HistoryKey::of(&released), None);
    }

    /// Only Z and Y: Ctrl+Shift+Y and Alt+Y are not keys of the history.
    #[test]
    fn other_keys_are_not_one() {
        assert_eq!(
            HistoryKey::of(&with(Key::Y, true, false, true, false)),
            None
        );
        assert_eq!(
            HistoryKey::of(&with(Key::Y, false, true, false, false)),
            None
        );
        assert_eq!(
            HistoryKey::of(&with(Key::X, true, false, false, false)),
            None
        );
    }
}
