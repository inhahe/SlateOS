//! The pending accent: the one piece of memory between two key events.
//!
//! Everything else in [`keymap`](crate::keymap) is a pure function of one
//! keystroke. A dead key is not: pressing `´` on a German board changes
//! nothing on screen, and the `é` appears only when `e` follows. Something has
//! to remember the accent in between, and this is it — a single `Option<char>`
//! and the rules for what the *next* keystroke does to it.
//!
//! The two facts it needs are already answered elsewhere, and deliberately not
//! re-answered here:
//!
//! * **Which keys are dead** is a property of the layout, and lives in
//!   [`keylayout`] — per *face*, not per character, because French AZERTY
//!   carries `^` twice and one of them is an ordinary circumflex.
//! * **What an accent and a letter make** is a fact about Unicode, and lives
//!   in [`osfont::deadkey`], which answers it out of the same generated UAX #15
//!   tables the text shaper uses rather than a second hand-written table.
//!
//! The compositor is the only process that links both, which is exactly why
//! `design-decisions.md` §456 put scancode translation here: one answer for the
//! whole desktop, so two applications cannot disagree about what a key typed.
//!
//! # The rules, and which of them are conventions rather than facts
//!
//! Read in order, for a key **press** that the compositor itself translated:
//!
//! | The keystroke | What happens to the pending accent |
//! |---|---|
//! | a modifier key, or a Ctrl/Alt chord | nothing — it keeps waiting |
//! | another dead key | the waiting one is typed; the new one waits |
//! | space | the waiting one is typed, alone; the space is not |
//! | a key that composes with it | the composed character is typed |
//! | any other key that types something | both are typed, accent first |
//! | a key that types nothing at all | the accent is discarded |
//!
//! Rows three through five are the ones that are *decided* rather than
//! derived, and `design-decisions.md` §551 records why each went the way it
//! did. In brief:
//!
//! * **Both are typed** when composition fails — `´` then `x` gives `´x` —
//!   because a text field that silently eats a keystroke is the one failure it
//!   must not have. That half is §550, decided before any of this was written.
//! * **Space types the accent alone.** Universal on Windows, macOS and X11, and
//!   the only way to type a bare `´` on a board where that key is dead.
//! * **A dead key is never a base character.** Checked before composition, and
//!   that order matters: Unicode really does compose `¨` with an acute into
//!   `΅` GREEK DIALYTIKA TONOS, and a Spanish typist pressing their two dead
//!   keys in a row wants neither Greek nor a surprise.
//! * **A key that types nothing cancels.** Backspace is the case that decides
//!   it: a user who pressed `´` by mistake reaches for Backspace, and on every
//!   real keyboard that is what un-arms it. Nothing visible is lost, because a
//!   pending accent was never drawn — which is what makes this different from
//!   the failed-composition case, where the user can see the keystroke coming.
//!
//! # When the compositor is not the one translating
//!
//! [`Compositor::handle_key`](crate::Compositor::handle_key) takes an optional
//! character from the input source, and skips this machine entirely when it
//! gets one: a source that hands over a finished character has already run
//! whatever composition its own layout implies, and running ours on top would
//! compose twice.
//!
//! No production backend takes that branch today, and the reason is worth
//! knowing before someone "fixes" it. The Windows host backend does read the
//! host's layout, but it reports `character: None` on the key press and
//! delivers the host's character separately as
//! [`InputEvent::TextInput`](crate::InputEvent::TextInput) — which
//! `handle_text_input` discards. That is deliberate and is
//! `design-decisions.md` §456: the compositor names the key and produces the
//! text, so that a US developer running a German build types German. The
//! consequence for dead keys is the useful one — the machine runs on the host
//! backend too, so the feature is testable without a bare-metal boot.
//!
//! The skip therefore exists for the source that does not exist yet: a remote
//! or virtual-machine input channel carrying text its far end already composed.

use keylayout::{Layout, Level};
use osfont::deadkey;

use crate::keymap;

/// The accent waiting for the keystroke that will complete it.
///
/// Held by the compositor next to [`ModifierState`](crate::ModifierState), and
/// for the same reason: it is a state spanning two events, arbitrarily far
/// apart, and one answer for the whole system is the point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DeadKeys {
    /// The *spacing* accent as the key cap carries it — `´` U+00B4, not the
    /// combining U+0301. Storing what was typed rather than what it will
    /// become is what lets the failed-composition path type it verbatim.
    pending: Option<char>,
}

impl DeadKeys {
    /// Nothing waiting.
    pub(crate) const fn new() -> Self {
        Self { pending: None }
    }

    /// The accent currently waiting, if any.
    ///
    /// For tests and for a future indicator: a desktop that shows the user
    /// their pending accent — as several do — reads it from here rather than
    /// keeping a second copy that could disagree.
    #[cfg(test)]
    pub(crate) const fn pending(self) -> Option<char> {
        self.pending
    }

    /// Forget any pending accent.
    ///
    /// Call when the keyboard stops belonging to the focused window — a
    /// session switch, a VT change, focus moving to another window. An accent
    /// armed for one window must not complete itself in the next one, which
    /// would put a letter the user never typed into a document they had
    /// already left.
    pub(crate) const fn cancel(&mut self) {
        self.pending = None;
    }

    /// The text a key press types, given what is waiting.
    ///
    /// `laid_out` is the character the layout produces for this scancode and
    /// level, which the caller has already resolved — passed in rather than
    /// looked up again so that the character this decides about and the
    /// character the caller reports are the same one by construction.
    ///
    /// `command_chord` is true when Ctrl or Alt is held *as a modifier*, after
    /// the AltGr fold. It is not the same as "Alt is down": on a German board
    /// AltGr spends itself selecting a character and the keystroke is text, not
    /// a command.
    pub(crate) fn press(
        &mut self,
        layout: &Layout,
        scancode: u32,
        level: Level,
        laid_out: Option<char>,
        command_chord: bool,
    ) -> String {
        // A modifier key is half of a chord, not a keystroke of its own, and a
        // command chord is not text entry at all. Neither may disturb the
        // accent: Shift after a dead key is how `É` is typed, and a user who
        // saves with Ctrl+S mid-word should find their accent still waiting
        // afterwards rather than attached to the next letter or gone.
        if command_chord || keymap::ModifierState::is_modifier(scancode) {
            return laid_out.map_or_else(String::new, String::from);
        }

        let dead = layout.is_dead(scancode, level);

        let Some(pending) = self.pending else {
            // Nothing waiting. A dead key arms; everything else is ordinary.
            // `laid_out` is checked rather than assumed: a face declared dead
            // with no character on it would otherwise arm the machine with
            // nothing, and swallow the next keystroke forever.
            if dead && let Some(accent) = laid_out {
                self.pending = Some(accent);
                return String::new();
            }
            return laid_out.map_or_else(String::new, String::from);
        };

        let Some(typed) = laid_out else {
            // Enter, F5, an arrow, Escape — and Backspace, which is the one
            // that decides this. See the module docs.
            self.pending = None;
            return String::new();
        };

        self.pending = None;

        if dead {
            // Another accent. Before the composition attempt on purpose: `¨`
            // and an acute really do make `΅` under Unicode, and that is not
            // what a Spanish typist pressing both of their dead keys means.
            //
            // Note what this gives for the same key twice: the first is typed,
            // the second waits. Press it n times and n-1 accents appear, which
            // is the X11 result and needs no rule of its own.
            self.pending = Some(typed);
            return pending.to_string();
        }

        if typed == ' ' {
            // The escape hatch every keyboard has, and the reason the
            // no-character case above can afford to discard: there is always a
            // way to type the bare accent deliberately.
            return pending.to_string();
        }

        if let Some(composed) = deadkey::compose(pending, typed) {
            return composed.to_string();
        }

        // `design-decisions.md` §550: type both rather than discard. This is
        // the case `KeyEvent::text` was widened from `Option<char>` to a
        // `String` for -- it is the only one that types two characters.
        // Eight, because a `char` is at most four bytes in UTF-8 and there are
        // exactly two of them. Written as the constant rather than as a sum of
        // the two lengths so that there is no arithmetic to overflow-check on
        // a path that runs once per keystroke.
        let mut both = String::with_capacity(8);
        both.push(pending);
        both.push(typed);
        both
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        // `sc` spells scancodes as `u16` because a layout only ever names keys
        // in the alphanumeric block; everything downstream of the driver uses
        // `u32`, because an extended key does not fit in a byte and the `0xE0`
        // prefix has to go somewhere. The widening is what it looks like.
        clippy::cast_lossless
    )]

    use keylayout::{Layout, Level, by_id, default_layout, sc};

    use super::DeadKeys;
    use crate::keymap;

    /// The German board, whose accent key is dead on both of its faces.
    fn german() -> &'static Layout {
        by_id("de-qwertz").expect("de-qwertz is a builtin layout")
    }

    /// German `´` plain, `` ` `` shifted — both dead.
    const ACCENT: u32 = sc::EQUALS as u32;
    /// German `^` plain (dead), `°` shifted (live — a degree sign combines
    /// with nothing).
    const CIRCUMFLEX: u32 = sc::GRAVE as u32;
    const E: u32 = sc::E as u32;
    const A: u32 = sc::A as u32;
    const S: u32 = sc::S as u32;
    const X: u32 = sc::X as u32;
    /// The space bar. Not in `sc`, and that is the point: a [`Layout`] is the
    /// alphanumeric block, and space is not part of what a layout rearranges.
    const SPACE: u32 = 0x39;
    /// Left Shift.
    const LEFT_SHIFT: u32 = 0x2A;

    const PLAIN: Level = Level::PLAIN;
    const SHIFT: Level = Level::shift();

    /// Type a scancode at a level, the way the compositor would.
    ///
    /// Goes through [`keymap::key_for_layout`] rather than `layout.key(..)`
    /// directly, because that is the call
    /// [`Compositor::handle_key`](crate::Compositor::handle_key) makes and the
    /// two do not agree: a [`Layout`] is the alphanumeric block, so it has
    /// nothing to say about the space bar, and it is `key_for_layout` that
    /// supplies the `' '` the dead-then-space rule below depends on. Asking
    /// the layout here would test a machine the compositor does not run.
    fn press(state: &mut DeadKeys, layout: &Layout, scancode: u32, level: Level) -> String {
        let (_, laid_out) = keymap::key_for_layout(layout, scancode, level);
        state.press(layout, scancode, level, laid_out, false)
    }

    #[test]
    fn a_dead_key_types_nothing_and_the_letter_after_it_types_the_accented_form() {
        // The whole feature in four lines. German `´` is `ACCENT` plain.
        let mut state = DeadKeys::new();
        let de = german();
        assert_eq!(press(&mut state, de, ACCENT, PLAIN), "");
        assert_eq!(state.pending(), Some('\u{00B4}'), "the accent is waiting");
        assert_eq!(press(&mut state, de, E, PLAIN), "é");
        assert_eq!(state.pending(), None, "and is spent");
    }

    #[test]
    fn shift_between_the_accent_and_the_letter_capitalises_rather_than_cancelling() {
        // The case that forces modifier keys to be excluded from the machine.
        // Shift produces no character, so the "types nothing cancels" rule
        // would eat the accent and the user would get `E` -- and would have no
        // way at all to type a capital with an accent.
        let mut state = DeadKeys::new();
        let de = german();
        assert_eq!(press(&mut state, de, ACCENT, PLAIN), "");
        assert_eq!(
            press(&mut state, de, LEFT_SHIFT, SHIFT),
            "",
            "left shift down"
        );
        assert_eq!(state.pending(), Some('\u{00B4}'), "still waiting");
        assert_eq!(press(&mut state, de, E, SHIFT), "É");
    }

    #[test]
    fn a_composition_that_fails_types_both_characters_rather_than_dropping_one() {
        // `design-decisions.md` §550. There is no "x with acute" in Unicode,
        // and the alternative -- typing `x` alone, as X11 does -- loses a
        // keystroke the user watched themselves make.
        let mut state = DeadKeys::new();
        let de = german();
        assert_eq!(press(&mut state, de, ACCENT, PLAIN), "");
        assert_eq!(press(&mut state, de, X, PLAIN), "´x");
        assert_eq!(state.pending(), None);
    }

    #[test]
    fn a_dead_key_then_space_types_the_bare_accent_and_no_space() {
        // The escape hatch. Without it there is no way to type `´` at all on a
        // board where that key is dead, which would make the feature a
        // regression for anyone who writes about accents rather than with them.
        let mut state = DeadKeys::new();
        let de = german();
        assert_eq!(press(&mut state, de, ACCENT, PLAIN), "");
        assert_eq!(press(&mut state, de, SPACE, PLAIN), "´");
        assert_eq!(state.pending(), None);
    }

    #[test]
    fn a_dead_key_pressed_twice_types_one_accent_and_leaves_one_waiting() {
        // Falls out of "another dead key flushes and re-arms" rather than being
        // a rule; asserted because the behaviour is user-visible and the
        // no-special-case property is the reason it is allowed to be this.
        let mut state = DeadKeys::new();
        let de = german();
        assert_eq!(press(&mut state, de, ACCENT, PLAIN), "");
        assert_eq!(press(&mut state, de, ACCENT, PLAIN), "´");
        assert_eq!(state.pending(), Some('\u{00B4}'), "the second one waits");
        // ... and the escape hatch still gets the second one out.
        assert_eq!(press(&mut state, de, SPACE, PLAIN), "´");
    }

    #[test]
    fn two_different_dead_keys_do_not_compose_with_each_other() {
        // Unicode says `¨` and an acute make `΅` GREEK DIALYTIKA TONOS, and it
        // is right; it is just not what a typist pressing two accent keys
        // means. This is why deadness is checked before composition, and the
        // test names the character so that reordering those two checks fails
        // here with the Greek in the message rather than somewhere obscure.
        let mut state = DeadKeys::new();
        let de = german();
        // German: `´` on EQUALS plain, `` ` `` on EQUALS shifted. Use the
        // circumflex key (GRAVE, dead) as the second accent.
        assert_eq!(press(&mut state, de, ACCENT, PLAIN), "");
        assert_eq!(press(&mut state, de, CIRCUMFLEX, PLAIN), "´");
        assert_eq!(state.pending(), Some('^'), "the circumflex now waits");
        assert_eq!(press(&mut state, de, A, PLAIN), "â");
    }

    #[test]
    fn a_key_that_types_nothing_cancels_the_pending_accent() {
        // Backspace is the one that decides this: a user who armed an accent
        // by mistake reaches for it, and expects the arming undone rather than
        // an accent to delete that was never shown to them.
        for (scancode, what) in [
            (0x0E, "backspace"),
            (0x1C, "enter"),
            (0x01, "escape"),
            (0x3F, "F5"),
            (0xE04B, "left arrow"),
        ] {
            let mut state = DeadKeys::new();
            let de = german();
            assert_eq!(press(&mut state, de, ACCENT, PLAIN), "");
            assert_eq!(press(&mut state, de, scancode, PLAIN), "", "{what}");
            assert_eq!(state.pending(), None, "{what} left an accent armed");
            // And the next letter is then its plain self, not an accented one.
            assert_eq!(press(&mut state, de, E, PLAIN), "e", "{what}");
        }
    }

    #[test]
    fn a_command_chord_leaves_the_accent_waiting() {
        // Ctrl+S in the middle of a word. Neither composing with `s` nor
        // discarding is right: the user saved, and their accent is still owed
        // a vowel.
        let mut state = DeadKeys::new();
        let de = german();
        assert_eq!(press(&mut state, de, ACCENT, PLAIN), "");
        let (_, laid_out) = keymap::key_for_layout(de, S, PLAIN);
        assert_eq!(state.press(de, S, PLAIN, laid_out, true), "s");
        assert_eq!(state.pending(), Some('\u{00B4}'));
        assert_eq!(press(&mut state, de, E, PLAIN), "é");
    }

    #[test]
    fn a_layout_with_no_dead_keys_is_completely_unaffected() {
        // US QWERTY has none, and the machine must be invisible there --
        // including for the characters that are accents *somewhere*. A US user
        // typing `` ` `` then `a` in a shell means two characters and gets two.
        let mut state = DeadKeys::new();
        let us = default_layout();
        assert_eq!(press(&mut state, us, CIRCUMFLEX, PLAIN), "`");
        assert_eq!(state.pending(), None);
        assert_eq!(press(&mut state, us, A, PLAIN), "a");
    }

    #[test]
    fn cancelling_disarms_a_pending_accent() {
        // Losing the keyboard, or focus moving to another window. The accent
        // belonged to the window that is no longer listening.
        let mut state = DeadKeys::new();
        let de = german();
        assert_eq!(press(&mut state, de, ACCENT, PLAIN), "");
        state.cancel();
        assert_eq!(state.pending(), None);
        assert_eq!(press(&mut state, de, E, PLAIN), "e");
    }

    #[test]
    fn every_dead_face_in_every_builtin_layout_composes_with_something() {
        // The cross-crate check, and the compositor is the only crate that can
        // make it: `keylayout` declares deadness and does not know what an
        // accent composes into, `osfont` composes and does not know which keys
        // are dead, and neither depends on the other.
        //
        // What it catches is the silent failure: a layout declaring a face dead
        // whose character `osfont::deadkey::combining` has no entry for. That
        // key would swallow every keystroke after it and type nothing, for
        // every user of that layout, and nothing else in the tree would notice.
        //
        // The sweep goes through `KeyDef::is_dead`, which is the same
        // `face()` call production makes, so it also covers the fallback
        // case: AltGr+Shift on a key with no fourth-level character resolves
        // to the AltGr face, and its accent had better be one we know too.
        let levels = [
            Level::PLAIN,
            Level::shift(),
            Level::alt_gr(),
            Level {
                shift: true,
                caps: false,
                alt_gr: true,
            },
        ];
        let mut checked = 0_usize;
        for layout in keylayout::builtins() {
            for key in layout.keys() {
                for level in levels {
                    if !key.is_dead(level) {
                        continue;
                    }
                    // `is_dead` is false for a level with no character on it,
                    // so this cannot be `None` — but saying so out loud beats
                    // an `unwrap` that would report only a line number if the
                    // two ever drifted apart.
                    let Some(accent) = key.character(level) else {
                        panic!(
                            "{}: scancode {:#04X} is dead at {level:?} with no character",
                            layout.id, key.scancode
                        );
                    };
                    assert!(
                        osfont::deadkey::combining(accent).is_some(),
                        "{}: scancode {:#04X} is dead and carries {accent:?}, which \
                         osfont::deadkey::combining does not know -- that key would \
                         swallow every keystroke after it",
                        layout.id,
                        key.scancode
                    );
                    checked += 1;
                }
            }
        }
        // German declares three dead faces, French four and Spanish four; the
        // two French AltGr ones are reached twice, once directly and once
        // through the AltGr+Shift fallback. A layout that lost its `dead`
        // block would still pass every assertion above by checking nothing,
        // which is the failure this number exists to catch.
        assert_eq!(
            checked, 13,
            "expected 13 dead faces across the builtin layouts, found {checked}; \
             a layout gained or lost dead-key declarations"
        );
    }
}
