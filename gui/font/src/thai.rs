//! The Thai/Lao shaper: taking SARA AM apart, and putting its halves where
//! they are read.
//!
//! # What SARA AM is
//!
//! Thai U+0E33 SARA AM (`ำ`) and its Lao twin U+0EB3 are single characters that
//! are drawn as two marks in two places: a small circle **above** the
//! consonant — the same shape as U+0E4D NIKHAHIT — and a spacing vowel
//! **after** it, the same shape as U+0E32 SARA AA. Unicode gives SARA AM no
//! canonical decomposition, so [`norm`](crate::norm) leaves it whole and a
//! font is asked for one glyph covering both halves. Nearly no font has one.
//!
//! Every engine therefore splits it, and having split it must decide where the
//! circle goes. It cannot simply stay where SARA AM was, because the character
//! is typed *after* the tone mark it is drawn *under*:
//!
//! ```text
//!   typed    <0E14 DO DEK, 0E4B MAI CHATTAWA, 0E33 SARA AM>
//!   drawn    <0E14 DO DEK, 0E4D NIKHAHIT, 0E4B MAI CHATTAWA, 0E32 SARA AA>
//! ```
//!
//! — the nikhahit sits on the consonant and the tone rides above *it*, so the
//! circle has to move back over every above-base mark between it and the base.
//! This is not in the Microsoft OpenType Thai specification; it is what
//! Uniscribe does, what HarfBuzz's `preprocess_text_thai` copies from it, and
//! what readers expect. This module is that pass.
//!
//! The move is legitimate **only** for a nikhahit that came from a SARA AM. A
//! nikhahit the text actually typed after a tone mark stays put: `<0E14, 0E4B,
//! 0E4D>` is probably not what its author meant, but it says "nikhahit above
//! chattawa" and this pass is not entitled to rewrite it into something else.
//! That is why the reordering lives here rather than in a table of combining
//! classes — a class cannot tell the two spellings apart, because after the
//! split they are the same characters in the same order.
//!
//! # Why it runs before the marks are sorted
//!
//! [`norm::pieces`](crate::norm::pieces) calls this between decomposition and
//! canonical ordering, which is where HarfBuzz calls it too (its `preprocess_text`
//! hook runs ahead of `hb_ot_shape_normalize`). The order is load-bearing and
//! not a detail: sorting first would let a below-base vowel overtake the tone
//! mark, so the backwards scan would meet the tone rather than the vowel and
//! stop one place further back. `<0E14, 0E4B, 0E38, 0E33>` comes out
//! `<0E14, 0E38, 0E4B, 0E4D, 0E32>` when this runs first and
//! `<0E14, 0E38, 0E4D, 0E4B, 0E32>` when it runs second — the same glyphs, the
//! circle on the wrong side of the tone.
//!
//! Running before normalization is safe here in a way it would not be for most
//! scripts: no Thai or Lao character has a canonical decomposition, and nothing
//! outside those blocks decomposes *into* one, so decomposition can neither
//! create nor destroy a SARA AM. The two orders differ only in the sort, which
//! is exactly the difference above.
//!
//! # Clusters
//!
//! Both halves start with SARA AM's own cluster, and then the run from the
//! nikhahit's final position through SARA AA is merged to the lowest cluster in
//! it — including the character in front, when there is one. Two reasons, and
//! they agree: clusters have to stay non-decreasing, which moving a piece
//! backwards would otherwise break; and the nikhahit is now drawn on the
//! preceding consonant, so by this crate's own rule — a mark joins the cluster
//! of what it attaches to — that is the cluster it belongs in. The cost is that
//! a caret cannot land between the consonant and the vowel it carries, which is
//! what HarfBuzz's default cluster level does too.
//!
//! # Not done here
//!
//! The other rules on <https://linux.thai.net/~thep/th-otf/shaping.html> pick
//! private-use glyphs for tone marks that have to shift down or left over a
//! tall or descending consonant. Those apply only to a face with no Thai
//! `GSUB`, which is the shaping the font declined to describe; see
//! `known-issues.md`, `TD-FONT-HAS-NO-UNIVERSAL-SHAPING-ENGINE`.

use alloc::vec::Vec;

use crate::norm::Piece;

/// Whether `ch` is SARA AM — Thai U+0E33 or Lao U+0EB3.
///
/// Lao's Thai-derived characters sit exactly 0x80 above their Thai
/// counterparts, so clearing that bit answers for both blocks at once. No
/// character outside them can reach `0x0E33` that way: only `0x0E33` and
/// `0x0EB3` differ in bit 7 alone.
fn is_sara_am(ch: char) -> bool {
    (ch as u32) & !0x0080 == 0x0E33
}

/// The nikhahit that SARA AM's upper half is drawn as — U+0E4D, or U+0ECD for
/// Lao.
fn nikhahit_from(ch: char) -> Option<char> {
    char::from_u32((ch as u32).checked_sub(0x0E33)?.checked_add(0x0E4D)?)
}

/// The sara aa that SARA AM's lower half is drawn as — U+0E32, or U+0EB2 for
/// Lao. One below SARA AM in both blocks.
fn sara_aa_from(ch: char) -> Option<char> {
    char::from_u32((ch as u32).checked_sub(1)?)
}

/// Whether `ch` is a mark the nikhahit has to be moved in front of.
///
/// The set is Uniscribe's, and is *not* "every above-base mark": it is
/// `<0E31, 0E34..0E37, 0E3B, 0E47..0E4E>` for Thai and the same plus 0x80 for
/// Lao. Sara u and sara uu (U+0E38, U+0E39) are absent because they are drawn
/// below, and so is phinthu (U+0E3A); the nikhahit passes over what is stacked
/// above the consonant and stops at anything that is not.
fn is_above_base_mark(ch: char) -> bool {
    let cp = (ch as u32) & !0x0080;
    matches!(cp, 0x0E31 | 0x0E34..=0x0E37 | 0x0E3B | 0x0E47..=0x0E4E)
}

/// Whether `text` has anything for [`preprocess`] to do.
///
/// Asked of the string rather than the pieces so that the whole pass can be
/// skipped without allocating, which is the case for every string that is not
/// Thai or Lao. SARA AM is not a combining mark and has no decomposition, so a
/// string containing nothing else is one that
/// [`norm::needs_work`](crate::norm::needs_work) correctly calls already
/// normalized — this is a second, separate reason to do the work, not a
/// refinement of that one.
#[must_use]
pub(crate) fn present(text: &str) -> bool {
    text.chars().any(is_sara_am)
}

/// Replace every SARA AM with the two marks it is drawn as, moving the upper
/// one back over the marks it is drawn beneath.
///
/// A no-op, and free, for pieces with no SARA AM in them.
pub(crate) fn preprocess(pieces: &mut Vec<Piece>) {
    if !pieces.iter().any(|&(ch, _)| is_sara_am(ch)) {
        return;
    }
    let mut out: Vec<Piece> = Vec::with_capacity(pieces.len().saturating_add(2));
    for &(ch, cluster) in pieces.iter() {
        if !is_sara_am(ch) {
            out.push((ch, cluster));
            continue;
        }
        // Both arms are arithmetic on a character that is SARA AM, so neither
        // can fail; a `let else` rather than an `expect` because a shaper has
        // no business panicking on text it was handed.
        let (Some(nikhahit), Some(sara_aa)) = (nikhahit_from(ch), sara_aa_from(ch)) else {
            out.push((ch, cluster));
            continue;
        };
        out.push((nikhahit, cluster));
        out.push((sara_aa, cluster));
        let end = out.len();
        // Where the nikhahit is now: one before SARA AA, which is last.
        let Some(mut start) = end.checked_sub(2) else {
            continue;
        };
        while let Some(prev) = start.checked_sub(1)
            && out.get(prev).is_some_and(|&(m, _)| is_above_base_mark(m))
        {
            start = prev;
        }
        if start.saturating_add(2) < end {
            // Rotate the nikhahit — the last element of `[start, end - 1)` —
            // down to `start`, sliding the marks it passed over up by one. A
            // rotate rather than a remove-and-insert so the marks keep their
            // order among themselves; they are stacked, and their order is
            // which one is nearer the consonant.
            if let Some(last) = end.checked_sub(1)
                && let Some(run) = out.get_mut(start..last)
            {
                run.rotate_right(1);
            }
            merge_clusters(&mut out, start, end);
        }
        if let Some(prev) = start.checked_sub(1) {
            merge_clusters(&mut out, prev, end);
        }
    }
    *pieces = out;
}

/// Give every piece in `[start, end)` the lowest cluster any of them has.
///
/// The cluster of a run of glyphs that came from more than one character is
/// the first character's, which is the offset a caret landing anywhere in the
/// run should report.
fn merge_clusters(pieces: &mut [Piece], start: usize, end: usize) {
    let Some(run) = pieces.get_mut(start..end) else {
        return;
    };
    let Some(first) = run.iter().map(|&(_, cluster)| cluster).min() else {
        return;
    };
    for piece in run {
        piece.1 = first;
    }
}

#[cfg(test)]
#[expect(clippy::indexing_slicing, reason = "test code")]
mod tests {
    use super::*;
    use alloc::string::String;
    use alloc::vec;

    /// Run the pass over `text`, with one cluster per character as
    /// [`norm`](crate::norm) would assign it — a mark takes the cluster of the
    /// character before it.
    fn run(text: &str) -> Vec<Piece> {
        let mut pieces: Vec<Piece> = Vec::new();
        let mut cluster = 0usize;
        for (at, ch) in text.char_indices() {
            if crate::norm::combining_class(ch) == 0 || pieces.is_empty() {
                cluster = at;
            }
            pieces.push((ch, cluster));
        }
        preprocess(&mut pieces);
        pieces
    }

    fn chars(pieces: &[Piece]) -> String {
        pieces.iter().map(|&(ch, _)| ch).collect()
    }

    #[test]
    fn sara_am_becomes_a_nikhahit_and_a_sara_aa() {
        assert_eq!(chars(&run("\u{0E01}\u{0E33}")), "\u{0E01}\u{0E4D}\u{0E32}");
    }

    #[test]
    fn lao_sara_am_becomes_the_lao_pair() {
        assert_eq!(chars(&run("\u{0EA5}\u{0EB3}")), "\u{0EA5}\u{0ECD}\u{0EB2}");
    }

    #[test]
    fn the_nikhahit_moves_in_front_of_an_above_base_mark() {
        // The example from HarfBuzz and from the Thai shaping notes.
        assert_eq!(
            chars(&run("\u{0E14}\u{0E4B}\u{0E33}")),
            "\u{0E14}\u{0E4D}\u{0E4B}\u{0E32}"
        );
    }

    #[test]
    fn the_lao_nikhahit_moves_over_the_lao_marks() {
        assert_eq!(
            chars(&run("\u{0EA5}\u{0EC8}\u{0EB3}")),
            "\u{0EA5}\u{0ECD}\u{0EC8}\u{0EB2}"
        );
    }

    #[test]
    fn the_nikhahit_moves_over_a_whole_stack_and_the_stack_keeps_its_order() {
        // MAITAIKHU then MAI EK, both above-base: the circle goes under both,
        // and the two keep the order they were typed in.
        assert_eq!(
            chars(&run("\u{0E01}\u{0E47}\u{0E48}\u{0E33}")),
            "\u{0E01}\u{0E4D}\u{0E47}\u{0E48}\u{0E32}"
        );
    }

    #[test]
    fn the_nikhahit_stops_at_a_below_base_vowel() {
        // SARA U is drawn below, so it is not something the circle is stacked
        // on top of and the circle does not pass it.
        assert_eq!(
            chars(&run("\u{0E01}\u{0E38}\u{0E33}")),
            "\u{0E01}\u{0E38}\u{0E4D}\u{0E32}"
        );
    }

    #[test]
    fn the_nikhahit_stops_at_the_consonant() {
        assert_eq!(chars(&run("\u{0E01}\u{0E33}")), "\u{0E01}\u{0E4D}\u{0E32}");
    }

    #[test]
    fn a_leading_sara_am_has_nothing_to_move_over() {
        assert_eq!(chars(&run("\u{0E33}")), "\u{0E4D}\u{0E32}");
    }

    #[test]
    fn a_typed_nikhahit_is_not_moved() {
        // The whole reason this is a shaper and not a combining class: after
        // the split, `<0E14, 0E4D, 0E4B>` and this string are the same
        // characters, and only one of them may be reordered.
        let text = "\u{0E14}\u{0E4B}\u{0E4D}";
        assert_eq!(chars(&run(text)), text);
    }

    #[test]
    fn two_sara_ams_are_both_handled() {
        assert_eq!(
            chars(&run("\u{0E01}\u{0E33}\u{0E01}\u{0E4B}\u{0E33}")),
            "\u{0E01}\u{0E4D}\u{0E32}\u{0E01}\u{0E4D}\u{0E4B}\u{0E32}"
        );
    }

    #[test]
    fn text_with_no_sara_am_is_untouched() {
        let mut pieces = vec![('a', 0), ('\u{0E01}', 1), ('\u{0E4B}', 1)];
        let before = pieces.clone();
        preprocess(&mut pieces);
        assert_eq!(pieces, before);
    }

    #[test]
    fn present_is_asked_of_the_string() {
        assert!(present("\u{0E01}\u{0E33}"));
        assert!(present("\u{0EA5}\u{0EB3}"));
        assert!(!present("\u{0E01}\u{0E32}"));
        assert!(!present("hello"));
    }

    #[test]
    fn clusters_stay_non_decreasing_when_the_nikhahit_moves() {
        let pieces = run("\u{0E14}\u{0E4B}\u{0E33}");
        assert!(
            pieces.windows(2).all(|w| w[0].1 <= w[1].1),
            "clusters went backwards: {pieces:?}"
        );
    }

    #[test]
    fn both_halves_join_the_cluster_of_the_consonant_they_are_drawn_on() {
        // DO DEK at 0, MAI CHATTAWA at 3 (charged to DO DEK), SARA AM at 6.
        // The circle is drawn on DO DEK, so the whole thing is one cluster.
        assert_eq!(
            run("\u{0E14}\u{0E4B}\u{0E33}"),
            vec![
                ('\u{0E14}', 0),
                ('\u{0E4D}', 0),
                ('\u{0E4B}', 0),
                ('\u{0E32}', 0),
            ]
        );
    }

    #[test]
    fn a_leading_sara_am_keeps_its_own_cluster() {
        // Nothing in front to merge with, so the two halves stay where the
        // character was.
        assert_eq!(run("\u{0E33}"), vec![('\u{0E4D}', 0), ('\u{0E32}', 0)]);
    }

    #[test]
    fn every_thai_mark_uniscribe_reorders_has_a_lao_counterpart_at_the_same_offset() {
        // The claim `is_above_base_mark` rests on: clearing bit 7 answers for
        // both blocks, so the two sets must be translates of one another.
        for cp in 0x0E00u32..=0x0E7F {
            let Some(thai) = char::from_u32(cp) else {
                continue;
            };
            let Some(lao) = char::from_u32(cp | 0x0080) else {
                continue;
            };
            assert_eq!(
                is_above_base_mark(thai),
                is_above_base_mark(lao),
                "{cp:04X} and {:04X} disagree",
                cp | 0x0080
            );
        }
    }

    #[test]
    fn nothing_outside_the_two_blocks_is_mistaken_for_thai() {
        for cp in 0u32..0x11000 {
            let Some(ch) = char::from_u32(cp) else {
                continue;
            };
            if is_sara_am(ch) {
                assert!(
                    cp == 0x0E33 || cp == 0x0EB3,
                    "{cp:04X} was taken for SARA AM"
                );
            }
            if is_above_base_mark(ch) {
                assert!(
                    (0x0E00..0x0F00).contains(&cp),
                    "{cp:04X} was taken for a Thai or Lao mark"
                );
            }
        }
    }
}
