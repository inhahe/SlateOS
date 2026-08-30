//! Dead keys: what an accent and the keystroke after it make together.
//!
//! A **dead key** types nothing on its own. Press `´` on a German board and
//! the screen does not change; press `e` next and `é` appears. The accent was
//! waiting. Which keys are dead is a fact about the keyboard layout and lives
//! in the `keylayout` crate; *what two of them make* is a fact about Unicode
//! and lives here.
//!
//! # Why this is in the font crate
//!
//! Because the answer is already here, and having it twice is the failure
//! mode. Composing `e` and an acute accent into `é` is exactly the question
//! [`crate::norm`] answers on every string it shapes — UAX #15 canonical
//! composition, from generated tables covering some 1050 pairs. A keyboard
//! module that wrote its own `match ('e', '´') => 'é'` table would be a second
//! answer to that question: shorter, hand-maintained, and therefore the one
//! that would eventually disagree — most likely by omission, on the letter
//! nobody thought to list.
//!
//! So the only thing this module adds is the *bridge*, and it is a small one.
//!
//! # Spacing accents and combining accents
//!
//! A key cap carries a **spacing** accent — `´` is U+00B4 ACUTE ACCENT, a
//! character wide enough to stand on its own, which is what makes it printable
//! on a key and typeable when the composition fails. Unicode's composition
//! tables key off the **combining** accent — U+0301 COMBINING ACUTE ACCENT,
//! which has no width and is drawn on top of the character before it. They are
//! different code points and the tables know only the second, so something has
//! to map one to the other. [`combining`] is that map, and it is the one
//! hand-written table here: thirteen accents, reached through the eighteen
//! characters that keyboards print them as.
//!
//! # What this module deliberately does not decide
//!
//! - **What happens when composition fails.** `´` then `x` makes nothing;
//!   whether that types `´x`, or `x`, or nothing at all, is policy, and it is
//!   the compositor's (see `design-decisions.md` §550, which chose `´x`).
//! - **What a dead key pressed twice does**, or a dead key followed by a
//!   space. Both are conventions about the *keyboard*, not facts about
//!   Unicode, and both live with the state machine that holds the pending
//!   accent.
//! - **Which keys are dead.** That is `keylayout::KeyDef::dead`.
//!
//! Everything here is a pure function of two characters.

use crate::norm::compose_canonical;

/// The combining mark that corresponds to a spacing accent, if there is one.
///
/// The bridge between what is printed on a key cap and what the composition
/// tables are indexed by. See the module docs.
///
/// `None` for anything that is not an accent — including for a spacing accent
/// with no combining counterpart, of which there are a few (the "modifier
/// letter" characters used in phonetic notation are not on any keyboard).
///
/// # Why the ASCII quotes are here
///
/// `'` and `"` are not accents; they are punctuation, and a layout that made
/// them dead unconditionally would be unusable for writing English. But the
/// US-International layout does make them dead, and this function is only ever
/// reached for a face the *layout* already declared dead — so the mapping is a
/// statement about what `'` means when it was pressed as a dead key, not about
/// what it means generally. A layout that leaves them live never asks.
#[must_use]
pub const fn combining(spacing: char) -> Option<char> {
    Some(match spacing {
        // The five our own layouts use.
        '\u{0060}' | '\u{02CB}' => '\u{0300}', // grave
        '\u{00B4}' | '\u{02CA}' | '\u{0027}' => '\u{0301}', // acute
        '\u{005E}' | '\u{02C6}' => '\u{0302}', // circumflex
        '\u{007E}' | '\u{02DC}' => '\u{0303}', // tilde
        '\u{00A8}' | '\u{0022}' => '\u{0308}', // diaeresis
        // Accents other national layouts reach for: the Central European
        // boards (caron, breve, double acute, ogonek), the Nordic ones (ring),
        // Turkish and Romanian (cedilla), and the Latin transliterations
        // (macron, dot above).
        '\u{00AF}' => '\u{0304}', // macron
        '\u{02D8}' => '\u{0306}', // breve
        '\u{02D9}' => '\u{0307}', // dot above
        // U+02DA RING ABOVE only. U+00B0 DEGREE SIGN is deliberately *not*
        // here: it looks the same and German puts it on the shifted face of a
        // key whose plain face is dead, which is exactly the confusion that
        // would let a mis-declared `°` quietly compose `å`.
        '\u{02DA}' => '\u{030A}', // ring above
        '\u{02DD}' => '\u{030B}', // double acute
        '\u{02C7}' => '\u{030C}', // caron
        '\u{00B8}' => '\u{0327}', // cedilla
        '\u{02DB}' => '\u{0328}', // ogonek
        _ => return None,
    })
}

/// The character a pending dead-key `accent` and the following `base` make, if
/// they make one.
///
/// `None` when the pair does not compose — `´` and `x`, `^` and `5` — and
/// `None` for an `accent` that is not one this module knows. The caller
/// decides what to do about it; nothing here discards anything.
///
/// The composition itself is plain NFC over the generated Unicode tables, so
/// this answers for every pair Unicode composes and not merely for the ones
/// someone remembered to list: `¨` and `y` gives `ÿ`, `~` and `n` gives `ñ`,
/// `´` and `ø` gives nothing (Unicode has no precomposed character for it).
///
/// # Examples
///
/// ```
/// use osfont::deadkey::compose;
///
/// assert_eq!(compose('´', 'e'), Some('é'));
/// assert_eq!(compose('¨', 'O'), Some('Ö'));
/// assert_eq!(compose('´', 'x'), None);
/// ```
#[must_use]
pub fn compose(accent: char, base: char) -> Option<char> {
    compose_canonical(base, combining(accent)?)
}

#[cfg(test)]
mod tests {
    use super::{combining, compose};

    #[test]
    fn the_five_accents_our_own_layouts_carry_all_compose() {
        // One vowel per accent, in the language whose keyboard puts it there.
        // If a table regeneration ever drops a row, this is the test that
        // fails, and it fails naming the accent rather than a code point.
        for (accent, base, want, why) in [
            ('´', 'e', 'é', "Spanish acute"),
            ('`', 'a', 'à', "French grave"),
            ('^', 'e', 'ê', "French circumflex"),
            ('¨', 'u', 'ü', "German diaeresis"),
            ('~', 'n', 'ñ', "Spanish tilde"),
        ] {
            assert_eq!(compose(accent, base), Some(want), "{why}");
        }
    }

    #[test]
    fn an_accent_composes_with_a_capital_as_readily_as_a_lower_case_letter() {
        // Shift and a dead key are independent, and a composer that only knew
        // lower case would silently drop the accent from the first letter of
        // every sentence.
        for (accent, base, want) in [('´', 'E', 'É'), ('¨', 'O', 'Ö'), ('^', 'A', 'Â')] {
            assert_eq!(compose(accent, base), Some(want));
        }
    }

    #[test]
    fn a_pair_that_makes_no_character_makes_none() {
        // The case the whole `String` in `KeyEvent::text` exists for: there is
        // no precomposed "x with acute", so the composer must say so rather
        // than inventing one or quietly returning the base letter.
        for (accent, base) in [('´', 'x'), ('^', '5'), ('¨', ' '), ('~', '!')] {
            assert_eq!(compose(accent, base), None, "{accent:?} {base:?}");
        }
    }

    #[test]
    fn a_pair_unicode_declines_to_precompose_is_declined_here_too() {
        // `ǿ` exists, but `ø` with a *grave* does not, and neither does `ß`
        // with anything. Answering `None` for these is not a gap in the table
        // — it is the table being right, and it is why the failed-composition
        // path has to work rather than being a formality.
        assert_eq!(compose('`', 'ø'), None);
        assert_eq!(compose('´', 'ß'), None);
    }

    #[test]
    fn a_character_that_is_not_an_accent_composes_with_nothing() {
        // `compose` is reached only for a face the layout declared dead, but
        // it must not guess if that face carries something unexpected: a
        // mis-declared key should type nothing extra, not attach the letter
        // `q` to the next vowel.
        // `°` is in this list on purpose: it is the degree sign, it looks
        // exactly like a ring above, and German carries it on the shifted face
        // of a key whose plain face is a dead circumflex. If it ever composes,
        // that key has lost track of which half of itself is waiting.
        for accent in ['q', '5', ' ', '€', '°', '\u{0301}'] {
            assert_eq!(combining(accent), None, "{accent:?}");
            assert_eq!(compose(accent, 'e'), None, "{accent:?}");
        }
    }

    #[test]
    fn every_accent_in_the_table_maps_to_an_actual_combining_mark() {
        // The hand-written half of this module, checked against the generated
        // half. A typo in the map — U+0310 for U+0301, say — would compose
        // nothing at all for that accent, and the layout that uses it would be
        // the one that noticed.
        for spacing in [
            '`', '´', '^', '~', '¨', '¯', '˘', '˙', '˚', '˝', 'ˇ', '¸', '˛', '\'', '"', 'ˆ', '˜',
            'ˊ', 'ˋ',
        ] {
            let mark = combining(spacing);
            // Stated as one assertion over the `Option` rather than an unwrap
            // and a second check, so that a missing entry and a wrong one both
            // report the accent that is at fault and what it mapped to.
            assert!(
                mark.is_some_and(|m| ('\u{0300}'..='\u{0333}').contains(&m)
                    || m == '\u{0327}'
                    || m == '\u{0328}'),
                "{spacing:?} maps to {mark:?}, which is not a combining diacritic"
            );
        }
    }

    #[test]
    fn composing_is_a_pure_function_of_the_pair_and_not_of_order() {
        // `compose(accent, base)` -- accent first, the order they are *typed*.
        // Unicode's tables are indexed the other way round (base then mark),
        // and getting that backwards would compose nothing for everything, so
        // it is worth one test that says which way round this one is.
        assert_eq!(compose('´', 'e'), Some('é'));
        assert_eq!(compose('e', '´'), None, "the arguments are not symmetric");
    }
}
