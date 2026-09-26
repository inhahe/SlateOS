//! Which face draws which stretch of a line: face fallback.
//!
//! No one face has every character. The UI face has the Latin alphabet and
//! not the Arabic one; a text face has neither the emoji nor the
//! Devanagari; the emoji face has only emoji. A line of text is therefore
//! drawn from several faces, and this module decides which: it walks the
//! line a **grapheme cluster** at a time and gives each cluster to the first
//! face, in the font's preference order, that has a glyph for every character
//! in it. [`SystemFont`](crate::system::SystemFont) then shapes each face's
//! stretch with that face and joins the stretches into one run.
//!
//! # Why a cluster and not a character
//!
//! Because a cluster is the unit a reader sees, and splitting one across two
//! faces breaks it visibly:
//!
//! * `e` + U+0301 is one letter. A face with the `e` and without the combining
//!   acute must not put the accent in another face, beside the letter, at that
//!   face's metrics; the whole cluster goes to a face that has both, or stays
//!   with the letter.
//! * An emoji **sequence** is one picture made of several characters: a flag
//!   is two regional indicators, a skin tone is an emoji and a modifier, a
//!   family or a profession is several emoji joined by ZERO WIDTH JOINERs. Only
//!   the emoji face has the picture for the sequence; split, it falls apart
//!   into its parts, or worse, into boxes.
//!
//! The clusters are UAX #29's extended grapheme clusters, for the rules that
//! bear on this: marks, joiners, variation selectors, emoji modifiers and
//! tags extend the cluster before them (GB9, GB9a); a pictograph after a ZWJ
//! stays with the one before (GB11); regional indicators pair (GB12, GB13);
//! Hangul jamo stay with their syllable (GB6-GB8); CR LF is one (GB3).
//!
//! # Text and emoji
//!
//! Some characters are in text faces *and* the emoji face -- `☺`, `❤`,
//! `☀`, the digits a keycap is built on. Which face draws them is the reader's
//! question, answered by Unicode: U+FE0F after one asks for its emoji form,
//! U+FE0E for its text form, and without either the character's own
//! `Emoji_Presentation` says. A cluster that asks for emoji tries the colour
//! faces first; any other tries the text faces first; either way the rest
//! follow, so a character only one face has is still drawn.
//!
//! # Default ignorables
//!
//! The joiners and selectors *instruct* the shaper and are never drawn, so a
//! face need not have a glyph for them to draw the cluster -- most do not. They
//! are not asked about.

use alloc::vec::Vec;
use core::ops::Range;

use crate::emoji_tables::{EMOJI_PRESENTATION, EXTENDED_PICTOGRAPHIC};
use crate::norm;

/// The faces a line may be drawn from, as fallback sees them: by index,
/// 0 being the font's own face and the rest its fallbacks in preference
/// order.
pub(crate) trait Faces {
    /// How many faces there are.
    fn count(&self) -> usize;
    /// Whether face `face` has a glyph for `ch`.
    fn has(&self, face: usize, ch: char) -> bool;
    /// Whether face `face` draws in colour -- an emoji face.
    fn is_colour(&self, face: usize) -> bool;
}

/// One stretch of a line, and the face that draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stretch {
    /// Byte range in the line.
    pub(crate) range: Range<usize>,
    /// Index of the face, as [`Faces`] numbers them.
    pub(crate) face: usize,
}

/// `text` cut into stretches that each go to one face, covering it in order.
///
/// Adjacent clusters that go to the same face are one stretch, so a line the
/// first face can draw whole is one stretch, and costs one look at each
/// character.
pub(crate) fn stretches(text: &str, faces: &impl Faces) -> Vec<Stretch> {
    let mut out: Vec<Stretch> = Vec::new();
    for range in clusters(text) {
        let Some(cluster) = text.get(range.clone()) else {
            continue;
        };
        let face = face_for(cluster, faces);
        match out.last_mut() {
            Some(last) if last.face == face && last.range.end == range.start => {
                last.range.end = range.end;
            }
            _ => out.push(Stretch { range, face }),
        }
    }
    out
}

/// The face that draws `cluster`.
///
/// The first face, in the order the cluster's presentation asks for, that
/// has every character in it; failing that the first that has its first
/// character, so a letter is still drawn if its accent can be drawn nowhere;
/// and failing that face 0, whose missing-glyph box is the honest answer.
fn face_for(cluster: &str, faces: &impl Faces) -> usize {
    let Some(base) = cluster.chars().next() else {
        return 0;
    };
    let emoji = wants_emoji(cluster);
    let order = |pass: usize| {
        (0..faces.count()).filter(move |&face| faces.is_colour(face) == (emoji == (pass == 0)))
    };
    let preferred = order(0).chain(order(1));
    let mut first_with_base = None;
    for face in preferred {
        if cluster
            .chars()
            .all(|ch| is_default_ignorable(ch) || faces.has(face, ch))
        {
            return face;
        }
        if first_with_base.is_none() && faces.has(face, base) {
            first_with_base = Some(face);
        }
    }
    first_with_base.unwrap_or(0)
}

/// Whether `cluster` asks to be drawn as emoji.
///
/// U+FE0F says yes and U+FE0E says no, whatever the character is; without
/// either, a character whose `Emoji_Presentation` is `Yes`, an emoji modifier
/// sequence, a ZWJ sequence, a flag or a keycap does.
fn wants_emoji(cluster: &str) -> bool {
    let mut chars = cluster.chars();
    let Some(base) = chars.next() else {
        return false;
    };
    if cluster.contains('\u{FE0E}') {
        return false;
    }
    cluster.contains('\u{FE0F}')
        || cluster.contains('\u{20E3}')
        || cluster.contains('\u{200D}') && is_pictographic(base)
        || is_regional_indicator(base)
        || chars.any(is_emoji_modifier)
        || emoji_presentation(base)
}

/// The grapheme clusters of `text`, as byte ranges, in order.
pub(crate) fn clusters(text: &str) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut prev: Option<char> = None;
    // Regional indicators seen so far in the current cluster: they pair, so
    // an odd count is one waiting for its partner.
    let mut regional = 0usize;
    // Whether the cluster so far ends pictograph, extenders, ZWJ -- the one
    // shape a following pictograph joins (GB11).
    let mut pictograph_zwj = false;
    let mut pictograph_run = false;
    for (at, ch) in text.char_indices() {
        let joins = prev.is_some_and(|p| continues(p, ch, regional, pictograph_zwj));
        if joins {
            if let Some(last) = out.last_mut() {
                last.end = at.saturating_add(ch.len_utf8());
            }
        } else {
            out.push(at..at.saturating_add(ch.len_utf8()));
            regional = 0;
            pictograph_run = false;
        }
        if is_regional_indicator(ch) {
            regional = regional.saturating_add(1);
        }
        // GB11's state: a pictograph starts a run, extenders carry it, a ZWJ
        // after it arms it, anything else ends it.
        if is_pictographic(ch) {
            pictograph_run = true;
            pictograph_zwj = false;
        } else if ch == '\u{200D}' {
            pictograph_zwj = pictograph_run;
        } else if !is_extend(ch) {
            pictograph_run = false;
            pictograph_zwj = false;
        }
        prev = Some(ch);
    }
    out
}

/// Whether `ch` continues the cluster `prev` is the last character of.
fn continues(prev: char, ch: char, regional: usize, pictograph_zwj: bool) -> bool {
    // GB3: CR LF is one cluster. GB4/GB5: any other control breaks.
    if prev == '\r' && ch == '\n' {
        return true;
    }
    if is_control(prev) || is_control(ch) {
        return false;
    }
    // GB6-GB8: Hangul syllable sequences.
    if hangul_continues(prev, ch) {
        return true;
    }
    // GB9, GB9a: extenders, ZWJ and spacing marks join what precedes them.
    if is_extend(ch) || ch == '\u{200D}' || norm::is_any_mark(ch) {
        return true;
    }
    // GB11: a pictograph after pictograph (Extend)* ZWJ.
    if prev == '\u{200D}' && pictograph_zwj && is_pictographic(ch) {
        return true;
    }
    // GB12/GB13: regional indicators pair.
    if is_regional_indicator(prev) && is_regional_indicator(ch) && !regional.is_multiple_of(2) {
        return true;
    }
    false
}

/// UAX #29's `Extend`, the part of it that is not a combining mark (which
/// [`continues`] asks [`norm`] about): variation selectors, emoji modifiers
/// and tag characters, which all attach to the character before them.
fn is_extend(ch: char) -> bool {
    matches!(
        ch,
        '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}' | '\u{E0020}'..='\u{E007F}'
    ) || is_emoji_modifier(ch)
        || ch == '\u{200C}'
        || norm::is_any_mark(ch)
}

fn is_control(ch: char) -> bool {
    matches!(ch, '\r' | '\n') || (ch.is_control() && ch != '\u{200D}')
}

/// The joiners and selectors that are never drawn, so need no glyph.
fn is_default_ignorable(ch: char) -> bool {
    matches!(
        ch,
        '\u{200B}'..='\u{200F}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{E0100}'..='\u{E01EF}'
            | '\u{2060}'..='\u{2064}'
            | '\u{FEFF}'
    )
}

fn is_regional_indicator(ch: char) -> bool {
    matches!(ch, '\u{1F1E6}'..='\u{1F1FF}')
}

fn is_emoji_modifier(ch: char) -> bool {
    matches!(ch, '\u{1F3FB}'..='\u{1F3FF}')
}

fn emoji_presentation(ch: char) -> bool {
    in_ranges(&EMOJI_PRESENTATION, ch)
}

fn is_pictographic(ch: char) -> bool {
    in_ranges(&EXTENDED_PICTOGRAPHIC, ch)
}

fn in_ranges(ranges: &[(u32, u32)], ch: char) -> bool {
    let cp = u32::from(ch);
    ranges
        .binary_search_by(|&(lo, hi)| {
            if hi < cp {
                core::cmp::Ordering::Less
            } else if lo > cp {
                core::cmp::Ordering::Greater
            } else {
                core::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// GB6-GB8: a leading jamo takes any jamo or syllable after it; a vowel or
/// an LV syllable takes a vowel or a trailing jamo; a trailing jamo or an
/// LVT syllable takes a trailing jamo.
fn hangul_continues(prev: char, ch: char) -> bool {
    use Jamo::{L, Lv, Lvt, T, V};
    matches!(
        (jamo(prev), jamo(ch)),
        (Some(L), Some(L | V | Lv | Lvt)) | (Some(Lv | V), Some(V | T)) | (Some(Lvt | T), Some(T))
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Jamo {
    L,
    V,
    T,
    Lv,
    Lvt,
}

fn jamo(ch: char) -> Option<Jamo> {
    let cp = u32::from(ch);
    match cp {
        0x1100..=0x115F | 0xA960..=0xA97C => Some(Jamo::L),
        0x1160..=0x11A7 | 0xD7B0..=0xD7C6 => Some(Jamo::V),
        0x11A8..=0x11FF | 0xD7CB..=0xD7FB => Some(Jamo::T),
        0xAC00..=0xD7A3 => Some(if cp.wrapping_sub(0xAC00).is_multiple_of(28) {
            Jamo::Lv
        } else {
            Jamo::Lvt
        }),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use alloc::string::String;
    use alloc::vec;

    /// Faces as sets of characters, the last one or more a colour face.
    struct Sets(Vec<(&'static str, bool)>);

    impl Faces for Sets {
        fn count(&self) -> usize {
            self.0.len()
        }
        fn has(&self, face: usize, ch: char) -> bool {
            self.0.get(face).is_some_and(|(set, _)| set.contains(ch))
        }
        fn is_colour(&self, face: usize) -> bool {
            self.0.get(face).is_some_and(|&(_, colour)| colour)
        }
    }

    fn pieces(text: &str) -> Vec<String> {
        clusters(text)
            .into_iter()
            .map(|r| String::from(&text[r]))
            .collect()
    }

    #[test]
    fn clusters_keep_marks_emoji_sequences_flags_and_jamo_together() {
        assert_eq!(pieces("ae\u{301}b"), ["a", "e\u{301}", "b"]);
        // A family: man ZWJ woman ZWJ girl, one picture.
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(pieces(&alloc::format!("x{family}y")), ["x", family, "y"]);
        // A skin tone and a text-or-emoji selector stay with their emoji.
        assert_eq!(
            pieces("\u{1F44D}\u{1F3FD}\u{2764}\u{FE0F}"),
            ["\u{1F44D}\u{1F3FD}", "\u{2764}\u{FE0F}"]
        );
        // Three regional indicators: a flag, then one waiting for a partner.
        assert_eq!(
            pieces("\u{1F1EF}\u{1F1F5}\u{1F1FA}"),
            ["\u{1F1EF}\u{1F1F5}", "\u{1F1FA}"]
        );
        // A keycap: digit, selector, enclosing keycap.
        assert_eq!(pieces("1\u{FE0F}\u{20E3}!"), ["1\u{FE0F}\u{20E3}", "!"]);
        // Conjoining jamo spell one syllable; a precomposed one is its own.
        assert_eq!(
            pieces("\u{1100}\u{1161}\u{11A8}\u{AC00}"),
            ["\u{1100}\u{1161}\u{11A8}", "\u{AC00}"]
        );
        // A ZWJ between letters joins nothing but itself.
        assert_eq!(pieces("a\u{200D}b"), ["a\u{200D}", "b"]);
        assert_eq!(pieces("\r\n\n"), ["\r\n", "\n"]);
        assert_eq!(pieces(""), Vec::<String>::new());
    }

    #[test]
    fn each_cluster_goes_to_the_first_face_that_has_all_of_it() {
        let faces = Sets(vec![
            ("ab e", false),
            ("abce\u{301}", false),
            ("\u{1F600}", true),
        ]);
        let got = stretches("ab e\u{301}c \u{1F600}", &faces);
        assert_eq!(
            got,
            vec![
                Stretch {
                    range: 0..3,
                    face: 0
                },
                // The accented e and the c, from the face that has both.
                Stretch {
                    range: 3..7,
                    face: 1
                },
                Stretch {
                    range: 7..8,
                    face: 0
                },
                Stretch {
                    range: 8..12,
                    face: 2
                },
            ]
        );
    }

    #[test]
    fn a_letter_with_an_accent_no_face_has_stays_where_the_letter_is() {
        let faces = Sets(vec![("x", false), ("e", false)]);
        assert_eq!(
            stretches("e\u{301}", &faces),
            vec![Stretch {
                range: 0..3,
                face: 1
            }]
        );
        // And a character no face has is the first face's missing glyph.
        assert_eq!(
            stretches("\u{4E00}", &faces),
            vec![Stretch {
                range: 0..3,
                face: 0
            }]
        );
    }

    #[test]
    fn presentation_decides_between_a_text_face_and_the_emoji_face() {
        // Both faces have the heart.
        let faces = Sets(vec![("\u{2764}a", false), ("\u{2764}\u{1F600}", true)]);
        // Without a selector U+2764 is text by default: the text face.
        assert_eq!(stretches("\u{2764}", &faces)[0].face, 0);
        // With U+FE0F it asks for emoji: the colour face.
        assert_eq!(stretches("\u{2764}\u{FE0F}", &faces)[0].face, 1);
        // U+1F600 is emoji by default, and only the colour face has it.
        assert_eq!(stretches("\u{1F600}", &faces)[0].face, 1);
        // A default-emoji character a text face also has still goes to emoji,
        // unless U+FE0E asks for the text form.
        let both = Sets(vec![("\u{231A}", false), ("\u{231A}", true)]);
        assert_eq!(stretches("\u{231A}", &both)[0].face, 1);
        assert_eq!(stretches("\u{231A}\u{FE0E}", &both)[0].face, 0);
    }

    #[test]
    fn the_emoji_tables_are_sorted_and_answer_as_unicode_does() {
        for table in [&EMOJI_PRESENTATION[..], &EXTENDED_PICTOGRAPHIC[..]] {
            for pair in table.windows(2) {
                assert!(pair[0].1 < pair[1].0, "{pair:?}");
            }
        }
        assert!(emoji_presentation('\u{1F600}'));
        assert!(!emoji_presentation('\u{2764}'));
        assert!(is_pictographic('\u{2764}'));
        assert!(!is_pictographic('a'));
    }
}
