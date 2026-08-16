//! The Hangul shaper: which spelling of a syllable this face can actually draw.
//!
//! # Why Hangul needs a shaper at all
//!
//! Korean is written in syllable blocks, each built from a leading consonant
//! (**L**), a vowel (**V**) and an optional trailing consonant (**T**).
//! Unicode encodes the same block two ways, and both are in use:
//!
//! * **Precomposed** — 11,172 characters in U+AC00..U+D7A3, one per block.
//!   `각` is a single character.
//! * **Conjoining jamo** — the parts, encoded separately in U+1100..U+11FF and
//!   the U+A960/U+D7B0 extension blocks. The same `각` is `ᄀ` `ᅡ` `ᆨ`, three
//!   characters.
//!
//! They are canonically equivalent, so normalization would settle the question
//! — except that a *font* is not obliged to cover both. A Korean text font
//! typically ships the 11,172 precomposed syllables and no jamo at all. A font
//! for Old Korean ships jamo and lets `ljmo`/`vjmo`/`tjmo` pick the positional
//! variant of each, because the Old Korean blocks it must draw have no
//! precomposed character to be composed *to*. And most of the fonts on any
//! given machine cover neither.
//!
//! So the choice of spelling is a question about the face, not about the text,
//! which is exactly why it cannot live in [`norm`](crate::norm) and why
//! HarfBuzz gives Hangul a shaper with
//! `HB_OT_SHAPE_NORMALIZATION_MODE_NONE`. This module is that shaper's
//! `preprocess_text_hangul`, and it runs *instead of* normalizing Hangul rather
//! than after it.
//!
//! # What it decides
//!
//! Per syllable, in this order:
//!
//! 1. If the whole thing can be precomposed **and the face has that glyph**,
//!    do that.
//! 2. Otherwise, if it is precomposed and the face lacks it, fully decompose —
//!    but only if the face has every jamo, since three missing-glyph boxes are
//!    worse than one.
//! 3. Otherwise leave it exactly as it was written, and tag the jamo with
//!    `ljmo`/`vjmo`/`tjmo` so a jamo-shaping face can pick their forms.
//!
//! The asymmetry in the last two is deliberate and is HarfBuzz's: text typed
//! as `<L,V,T>` in a face that can draw none of it stays three characters,
//! where the same text typed as `<LVT>` stays one. That is *not* a defensible
//! reading of canonical equivalence — it renders two spellings of one string
//! differently — but it is what every other engine does, and the alternative
//! costs more than it buys: composing first would turn three typed characters
//! into a single caret stop and a single missing-glyph box, and the user who
//! typed three jamo would have no way to tell which of them the font was
//! missing.
//!
//! # Tone marks
//!
//! Middle Korean marked pitch with U+302E and U+302F, written *after* the
//! syllable and drawn to its *left*. So a tone mark that follows a syllable
//! this pass recognized is moved in front of it — unless the face draws it
//! zero-width, which is a font saying "I overstrike, leave me where I am". A
//! tone mark with no syllable in front of it gets a dotted circle to sit
//! against, on the same reasoning as
//! [`indic_shape`](crate::indic_shape)'s.
//!
//! # Where this runs
//!
//! [`preprocess`] is called from [`ScaledFont::shape_lang`](crate::scaled)
//! after normalization, on the pieces, before the run is split by script. It
//! runs *after* the sort and the composition because everything it moves is a
//! jamo or a tone mark, neither of which normalization touches — unlike the
//! Thai pass in [`thai`](crate::thai), which has to run before the sort
//! because the mark it produces would be sorted to the wrong side.

use alloc::vec::Vec;

use crate::norm::Piece;

/// The Hangul composition constants, from UAX #15 §3.12. Repeated here rather
/// than shared with [`norm`](crate::norm) because the two modules want them for
/// opposite reasons — that one to normalize Hangul, this one to decide that it
/// must not be normalized — and a shared constant would suggest a shared
/// policy.
const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
/// One *below* the first trailing jamo: `T_BASE + 0` is the "no trailing
/// consonant" filler and is not a character.
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;

/// Which of the three jamo features a glyph is eligible for.
///
/// A face that draws jamo rather than precomposed syllables needs to know which
/// slot each one occupies, because the same consonant is drawn differently as a
/// leading and as a trailing jamo. The three features say which.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Jamo {
    /// `ljmo` — the leading consonant.
    Leading,
    /// `vjmo` — the vowel.
    Vowel,
    /// `tjmo` — the trailing consonant.
    Trailing,
}

/// A leading jamo, including the Extended-A block.
///
/// Wider than the range that *composes*: U+1100..U+115F holds the conjoining
/// leading jamo of which only the first 19 have precomposed syllables, and
/// U+A960..U+A97C the Extended-A ones, which have none at all. Both are
/// leading jamo for the purpose of picking a form.
#[must_use]
fn is_l(ch: char) -> bool {
    let cp = ch as u32;
    (0x1100..=0x115F).contains(&cp) || (0xA960..=0xA97C).contains(&cp)
}

/// A vowel jamo, including the Extended-B block.
#[must_use]
fn is_v(ch: char) -> bool {
    let cp = ch as u32;
    (0x1160..=0x11A7).contains(&cp) || (0xD7B0..=0xD7C6).contains(&cp)
}

/// A trailing jamo, including the Extended-B block.
#[must_use]
fn is_t(ch: char) -> bool {
    let cp = ch as u32;
    (0x11A8..=0x11FF).contains(&cp) || (0xD7CB..=0xD7FB).contains(&cp)
}

/// Any conjoining jamo, which is what `calt` is kept away from.
///
/// Uniscribe does not apply `calt` to Hangul, and Noto Sans CJK and Source Han
/// Sans file *all* of their jamo lookups under `calt` — so applying it would
/// rewrite jamo that this pass has already decided to leave alone. HarfBuzz
/// clears the bit per glyph rather than switching the feature off for the run,
/// so that Latin sharing the run still gets its contextual alternates.
#[must_use]
pub(crate) fn is_jamo(ch: char) -> bool {
    is_l(ch) || is_v(ch) || is_t(ch)
}

/// A leading jamo that has precomposed syllables, U+1100..U+1112.
#[must_use]
fn is_combining_l(ch: char) -> bool {
    (ch as u32).wrapping_sub(L_BASE) < L_COUNT
}

/// A vowel jamo that has precomposed syllables, U+1161..U+1175.
#[must_use]
fn is_combining_v(ch: char) -> bool {
    (ch as u32).wrapping_sub(V_BASE) < V_COUNT
}

/// A trailing jamo that has precomposed syllables, U+11A8..U+11C2.
///
/// `T_BASE` itself is excluded: it is the filler that stands for "no trailing
/// consonant" and is not a character anyone types.
#[must_use]
fn is_combining_t(ch: char) -> bool {
    let t = (ch as u32).wrapping_sub(T_BASE);
    t > 0 && t < T_COUNT
}

/// A precomposed syllable, U+AC00..U+D7A3.
#[must_use]
fn is_syllable(ch: char) -> bool {
    (ch as u32).wrapping_sub(S_BASE) < S_COUNT
}

/// A Middle Korean tone mark, U+302E or U+302F.
#[must_use]
fn is_tone(ch: char) -> bool {
    matches!(ch, '\u{302E}' | '\u{302F}')
}

/// The syllable `l`, `v` and `t` compose to, when all three can compose.
///
/// `t` is `None` for an LV block. Returns `None` when any part is outside the
/// composing ranges, which is the Old Korean case: those blocks have no
/// precomposed character and must be drawn from their jamo.
#[must_use]
fn compose(l: char, v: char, t: Option<char>) -> Option<char> {
    if !is_combining_l(l) || !is_combining_v(v) {
        return None;
    }
    if let Some(t) = t
        && !is_combining_t(t)
    {
        return None;
    }
    let li = (l as u32).checked_sub(L_BASE)?;
    let vi = (v as u32).checked_sub(V_BASE)?;
    let ti = t.map_or(0, |t| (t as u32).wrapping_sub(T_BASE));
    char::from_u32(
        S_BASE
            .checked_add(li.checked_mul(N_COUNT)?)?
            .checked_add(vi.checked_mul(T_COUNT)?)?
            .checked_add(ti)?,
    )
}

/// The three jamo a precomposed syllable is made of.
///
/// The trailing one is `None` for an LV block. Returns `None` for a character
/// that is not a precomposed syllable.
#[must_use]
fn parts(s: char) -> Option<(char, char, Option<char>)> {
    let si = (s as u32).checked_sub(S_BASE)?;
    if si >= S_COUNT {
        return None;
    }
    let l = char::from_u32(L_BASE.checked_add(si / N_COUNT)?)?;
    let v = char::from_u32(V_BASE.checked_add(si % N_COUNT / T_COUNT)?)?;
    let ti = si % T_COUNT;
    let t = if ti == 0 {
        None
    } else {
        Some(char::from_u32(T_BASE.checked_add(ti)?)?)
    };
    Some((l, v, t))
}

/// Whether `text` has anything this pass would look at.
///
/// One scan of three ranges, and the reason the pass costs nothing for the text
/// that is not Korean — which is nearly all of it.
#[must_use]
pub(crate) fn present(pieces: &[Piece]) -> bool {
    pieces
        .iter()
        .any(|&(ch, _)| is_jamo(ch) || is_syllable(ch) || is_tone(ch))
}

/// Rewrite `pieces` into the spelling of each Hangul syllable this face can
/// draw, and record which jamo feature each surviving jamo is eligible for.
///
/// `jamo` comes back the same length as `pieces` and is left **empty** when the
/// text has no Hangul in it, which is the signal to the caller that no glyph
/// needs a jamo mask.
///
/// `has_glyph` is the face's `cmap`. `zero_width` is the narrower question
/// "does the face draw this character with no advance", asked only about tone
/// marks: a zero-advance tone mark is a font declaring that it overstrikes, and
/// moving it would draw it over the wrong letter.
///
/// This is HarfBuzz's `preprocess_text_hangul`, which walks the run once with a
/// cursor and a record of the extent of the most recently recognized syllable —
/// `start..end` below, in *output* positions, which is what lets a tone mark
/// several characters later know where the block it belongs to begins.
pub(crate) fn preprocess(
    pieces: &mut Vec<Piece>,
    jamo: &mut Vec<Option<Jamo>>,
    has_glyph: impl Fn(char) -> bool,
    zero_width: impl Fn(char) -> bool,
) {
    jamo.clear();
    if !present(pieces) {
        return;
    }
    let mut out: Vec<Piece> = Vec::with_capacity(pieces.len().saturating_add(2));
    // The extent of the most recently recognized syllable, in `out` positions.
    // Valid only while `start < end`, which is how "there is no syllable for
    // this tone mark to attach to" is spelled.
    let (mut start, mut end) = (0usize, 0usize);
    let mut i = 0usize;
    while let Some(&(u, cluster)) = pieces.get(i) {
        if is_tone(u) {
            i = i.saturating_add(1);
            if start < end && end == out.len() {
                out.push((u, cluster));
                jamo.push(None);
                if !zero_width(u) {
                    // Move the tone in front of the block it marks, and give
                    // the whole thing one cluster: a caret must not be able to
                    // land between a syllable and the pitch it is read with.
                    rotate_tone(&mut out, jamo, start, end);
                }
            } else {
                // Nothing to mark. Give it a ring to sit against, on the same
                // side the tone would have gone.
                let circle = '\u{25CC}';
                if has_glyph(circle) {
                    if zero_width(u) {
                        out.push((circle, cluster));
                        out.push((u, cluster));
                    } else {
                        out.push((u, cluster));
                        out.push((circle, cluster));
                    }
                    jamo.push(None);
                    jamo.push(None);
                } else {
                    out.push((u, cluster));
                    jamo.push(None);
                }
            }
            start = out.len();
            end = start;
            continue;
        }

        // Where a syllable starting here would begin in the output. Only read
        // if something below sets `end` past it.
        start = out.len();

        if is_l(u)
            && let Some(&(v, _)) = pieces.get(i.saturating_add(1))
            && is_v(v)
        {
            // `<L,V>` or `<L,V,T>`.
            let t = pieces
                .get(i.saturating_add(2))
                .map(|&(t, _)| t)
                .filter(|&t| is_t(t));
            let len = if t.is_some() { 3 } else { 2 };
            if let Some(s) = compose(u, v, t)
                && has_glyph(s)
            {
                out.push((s, cluster));
                jamo.push(None);
                i = i.saturating_add(len);
                end = start.saturating_add(1);
                continue;
            }
            // Either an Old Korean block with no precomposed character, or a
            // face that does not have the one there is. Draw the jamo, and say
            // which slot each occupies so a jamo-shaping face can form them.
            out.push((u, cluster));
            jamo.push(Some(Jamo::Leading));
            out.push((v, cluster));
            jamo.push(Some(Jamo::Vowel));
            if let Some(t) = t {
                out.push((t, cluster));
                jamo.push(Some(Jamo::Trailing));
            }
            i = i.saturating_add(len);
            end = start.saturating_add(len);
            continue;
        }

        if is_syllable(u) {
            let drawable = has_glyph(u);
            let Some((l, v, t)) = parts(u) else {
                // Unreachable: `is_syllable` is `parts`' own precondition.
                out.push((u, cluster));
                jamo.push(None);
                i = i.saturating_add(1);
                continue;
            };
            // `<LV,T>`: a precomposed LV block followed by a loose trailing
            // jamo. Try to absorb the T, which is the only way this run can
            // reach the LVT glyph the face may well have.
            let loose_t = if t.is_none() {
                pieces
                    .get(i.saturating_add(1))
                    .map(|&(t, _)| t)
                    .filter(|&t| is_t(t))
            } else {
                None
            };
            if let Some(loose) = loose_t
                && is_combining_t(loose)
                && let Some(s) = compose(l, v, Some(loose))
                && has_glyph(s)
            {
                out.push((s, cluster));
                jamo.push(None);
                i = i.saturating_add(2);
                end = start.saturating_add(1);
                continue;
            }
            // Decompose when the face cannot draw the block, or when a loose
            // trailing jamo follows that could not be absorbed — in the second
            // case the block *is* drawable, but drawing it beside a stray T
            // would show the vowel twice, so the whole thing goes to jamo.
            if !drawable || loose_t.is_some() {
                let parts_drawable = has_glyph(l) && has_glyph(v) && t.is_none_or(&has_glyph);
                if parts_drawable {
                    out.push((l, cluster));
                    jamo.push(Some(Jamo::Leading));
                    out.push((v, cluster));
                    jamo.push(Some(Jamo::Vowel));
                    let mut len = 2usize;
                    if let Some(t) = t {
                        out.push((t, cluster));
                        jamo.push(Some(Jamo::Trailing));
                        len = 3;
                    }
                    i = i.saturating_add(1);
                    // The loose T that forced the split joins the syllable it
                    // was written against rather than being left to start one
                    // of its own.
                    if drawable
                        && loose_t.is_some()
                        && let Some(&(loose, loose_cluster)) = pieces.get(i)
                    {
                        out.push((loose, loose_cluster));
                        jamo.push(Some(Jamo::Trailing));
                        i = i.saturating_add(1);
                        len = len.saturating_add(1);
                    }
                    end = start.saturating_add(len);
                    continue;
                }
            }
            out.push((u, cluster));
            jamo.push(None);
            i = i.saturating_add(1);
            if drawable {
                end = start.saturating_add(1);
            }
            continue;
        }

        // Not Hangul, or a lone jamo that starts nothing. `end` is left at or
        // below `start`, so a tone mark after it finds no block to move in
        // front of.
        out.push((u, cluster));
        jamo.push(None);
        i = i.saturating_add(1);
    }
    *pieces = out;
}

/// Move the last element of `start..=end` to `start`, and give the whole span
/// one cluster.
///
/// The tone mark was pushed at `end`; this is the `memmove` HarfBuzz does, plus
/// the cluster merge, which has to happen because the caret positions of a
/// block and its tone are not separable once the tone is drawn to the left of
/// the letter it follows in storage.
fn rotate_tone(out: &mut [Piece], jamo: &mut [Option<Jamo>], start: usize, end: usize) {
    let Some(span) = out.get_mut(start..=end) else {
        return;
    };
    span.rotate_right(1);
    let lowest = span.iter().map(|&(_, c)| c).min().unwrap_or(0);
    for piece in span.iter_mut() {
        piece.1 = lowest;
    }
    if let Some(span) = jamo.get_mut(start..=end) {
        span.rotate_right(1);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use alloc::string::String;

    use super::*;

    /// A piece list for `text`, one piece per character at its own offset.
    fn run(text: &str) -> Vec<Piece> {
        text.char_indices().map(|(at, ch)| (ch, at)).collect()
    }

    fn chars(pieces: &[Piece]) -> String {
        pieces.iter().map(|&(ch, _)| ch).collect()
    }

    fn clusters(pieces: &[Piece]) -> Vec<usize> {
        pieces.iter().map(|&(_, c)| c).collect()
    }

    /// Run the pass over `text` with a face that has exactly `covers`.
    fn shape(text: &str, covers: &str) -> (Vec<Piece>, Vec<Option<Jamo>>) {
        let mut pieces = run(text);
        let mut jamo = Vec::new();
        let has: Vec<char> = covers.chars().collect();
        preprocess(
            &mut pieces,
            &mut jamo,
            |ch| has.contains(&ch),
            |_| false,
        );
        (pieces, jamo)
    }

    /// The arithmetic both directions rest on: every precomposed syllable
    /// comes apart into jamo that compose back to it.
    #[test]
    fn every_syllable_is_its_own_jamo_composed() {
        for cp in S_BASE..S_BASE + S_COUNT {
            let s = char::from_u32(cp).unwrap();
            let (l, v, t) = parts(s).expect("a syllable must have parts");
            assert!(is_l(l) && is_v(v), "{s} decomposed to non-jamo");
            assert!(t.is_none_or(is_t), "{s} has a non-trailing tail");
            assert_eq!(compose(l, v, t), Some(s), "{s} did not compose back");
        }
    }

    /// And nothing outside the block claims to be one.
    #[test]
    fn only_the_syllable_block_decomposes() {
        assert!(parts('\u{ABFF}').is_none());
        assert!(parts('\u{D7A4}').is_none());
        assert!(!is_syllable('\u{ABFF}'));
        assert!(!is_syllable('\u{D7A4}'));
    }

    /// A face with the syllable gets the syllable, however the text was typed.
    #[test]
    fn jamo_compose_when_the_face_has_the_block() {
        // U+1100 U+1161 U+11A8 -> U+AC01.
        let (pieces, jamo) = shape("\u{1100}\u{1161}\u{11A8}", "\u{AC01}");
        assert_eq!(chars(&pieces), "\u{AC01}");
        assert_eq!(jamo, [None]);
        assert_eq!(clusters(&pieces), [0]);
    }

    /// An LV block with no trailing consonant composes from two jamo.
    #[test]
    fn two_jamo_compose_to_an_lv_block() {
        let (pieces, _) = shape("\u{1100}\u{1161}", "\u{AC00}");
        assert_eq!(chars(&pieces), "\u{AC00}");
    }

    /// The whole point: a face that has neither the block nor the jamo leaves
    /// the text as three characters, so three boxes are drawn for the three
    /// that were typed. This is 553 of the 556 faces on the host.
    #[test]
    fn jamo_a_face_can_draw_neither_way_stay_as_typed() {
        let (pieces, jamo) = shape("\u{1100}\u{1161}\u{11A8}", "");
        assert_eq!(chars(&pieces), "\u{1100}\u{1161}\u{11A8}");
        assert_eq!(jamo, [
            Some(Jamo::Leading),
            Some(Jamo::Vowel),
            Some(Jamo::Trailing)
        ]);
    }

    /// A face that draws jamo but has no precomposed syllable keeps them and
    /// tags them, which is how an Old Korean font is reached.
    #[test]
    fn a_jamo_face_keeps_the_jamo_and_names_their_slots() {
        let (pieces, jamo) = shape("\u{1100}\u{1161}\u{11A8}", "\u{1100}\u{1161}\u{11A8}");
        assert_eq!(chars(&pieces), "\u{1100}\u{1161}\u{11A8}");
        assert_eq!(jamo, [
            Some(Jamo::Leading),
            Some(Jamo::Vowel),
            Some(Jamo::Trailing)
        ]);
    }

    /// An Old Korean block has no precomposed character at all, so it stays
    /// decomposed even in a face that covers every syllable.
    #[test]
    fn an_old_korean_block_has_nothing_to_compose_to() {
        // U+A960 is an Extended-A leading jamo: outside the composing range.
        let (pieces, jamo) = shape("\u{A960}\u{1161}", "\u{A960}\u{1161}");
        assert_eq!(chars(&pieces), "\u{A960}\u{1161}");
        assert_eq!(jamo, [Some(Jamo::Leading), Some(Jamo::Vowel)]);
    }

    /// The other direction: a precomposed syllable the face cannot draw comes
    /// apart, when the face has the jamo.
    #[test]
    fn a_syllable_the_face_lacks_comes_apart_into_jamo_it_has() {
        let (pieces, jamo) = shape("\u{AC01}", "\u{1100}\u{1161}\u{11A8}");
        assert_eq!(chars(&pieces), "\u{1100}\u{1161}\u{11A8}");
        assert_eq!(jamo, [
            Some(Jamo::Leading),
            Some(Jamo::Vowel),
            Some(Jamo::Trailing)
        ]);
        // One character was typed, so there is one caret stop.
        assert_eq!(clusters(&pieces), [0, 0, 0]);
    }

    /// And stays whole when the face has neither, because three boxes are
    /// worse than one where one character was typed.
    #[test]
    fn a_syllable_the_face_lacks_stays_whole_without_jamo_to_show() {
        let (pieces, jamo) = shape("\u{AC01}", "");
        assert_eq!(chars(&pieces), "\u{AC01}");
        assert_eq!(jamo, [None]);
    }

    /// `<LV,T>`: a precomposed LV block and a loose trailing jamo compose to
    /// the LVT block when the face has it.
    #[test]
    fn a_loose_trailing_jamo_is_absorbed_into_the_block() {
        let (pieces, _) = shape("\u{AC00}\u{11A8}", "\u{AC00}\u{AC01}\u{11A8}");
        assert_eq!(chars(&pieces), "\u{AC01}");
        assert_eq!(clusters(&pieces), [0]);
    }

    /// And when it cannot be absorbed, the LV block comes apart so that the
    /// stray consonant is drawn beside jamo rather than beside a full block.
    #[test]
    fn an_unabsorbable_trailing_jamo_splits_the_block_it_follows() {
        let (pieces, jamo) = shape("\u{AC00}\u{11A8}", "\u{AC00}\u{1100}\u{1161}\u{11A8}");
        assert_eq!(chars(&pieces), "\u{1100}\u{1161}\u{11A8}");
        assert_eq!(jamo, [
            Some(Jamo::Leading),
            Some(Jamo::Vowel),
            Some(Jamo::Trailing)
        ]);
    }

    /// A tone mark is written after its syllable and drawn before it.
    #[test]
    fn a_tone_mark_moves_in_front_of_its_syllable() {
        let (pieces, _) = shape("\u{AC01}\u{302E}", "\u{AC01}\u{302E}");
        assert_eq!(chars(&pieces), "\u{302E}\u{AC01}");
        // One caret stop: the tone is not separable from the block now that it
        // is drawn on the other side of it.
        assert_eq!(clusters(&pieces), [0, 0]);
    }

    /// It moves in front of every jamo of a block that stayed decomposed, not
    /// just the last one.
    #[test]
    fn a_tone_mark_clears_the_whole_decomposed_block() {
        let (pieces, jamo) = shape(
            "\u{1100}\u{1161}\u{11A8}\u{302E}",
            "\u{1100}\u{1161}\u{11A8}\u{302E}",
        );
        assert_eq!(chars(&pieces), "\u{302E}\u{1100}\u{1161}\u{11A8}");
        assert_eq!(jamo, [
            None,
            Some(Jamo::Leading),
            Some(Jamo::Vowel),
            Some(Jamo::Trailing)
        ]);
    }

    /// A face that draws the tone mark zero-width has said it overstrikes, so
    /// it is left where it was written.
    #[test]
    fn a_zero_width_tone_mark_is_left_alone() {
        let mut pieces = run("\u{AC01}\u{302E}");
        let mut jamo = Vec::new();
        preprocess(&mut pieces, &mut jamo, |_| true, |ch| ch == '\u{302E}');
        assert_eq!(chars(&pieces), "\u{AC01}\u{302E}");
    }

    /// A tone mark with no syllable in front of it gets a ring to sit against.
    #[test]
    fn a_stray_tone_mark_gets_a_dotted_circle() {
        let (pieces, _) = shape("\u{302E}", "\u{302E}\u{25CC}");
        assert_eq!(chars(&pieces), "\u{302E}\u{25CC}");
    }

    /// And nothing at all in a face with no ring to give it.
    #[test]
    fn a_stray_tone_mark_in_a_face_with_no_circle_is_left_alone() {
        let (pieces, _) = shape("\u{302E}", "\u{302E}");
        assert_eq!(chars(&pieces), "\u{302E}");
    }

    /// Text with no Hangul in it is not touched and asks for no jamo masks.
    #[test]
    fn text_that_is_not_korean_is_left_exactly_alone() {
        let (pieces, jamo) = shape("hello", "helo");
        assert_eq!(chars(&pieces), "hello");
        assert!(jamo.is_empty(), "no glyph needs a jamo mask");
    }

    /// A lone leading jamo is not a syllable and starts nothing, so a tone
    /// mark after it has nothing to move in front of.
    #[test]
    fn a_lone_leading_jamo_is_not_a_block() {
        let (pieces, _) = shape("\u{1100}\u{302E}", "\u{1100}\u{302E}\u{25CC}");
        assert_eq!(chars(&pieces), "\u{1100}\u{302E}\u{25CC}");
    }

    /// Hangul beside text that is not Hangul leaves the neighbours alone.
    #[test]
    fn only_the_korean_part_of_a_run_is_rewritten() {
        let (pieces, jamo) = shape("a\u{1100}\u{1161}b", "ab\u{AC00}");
        assert_eq!(chars(&pieces), "a\u{AC00}b");
        assert_eq!(jamo, [None, None, None]);
        assert_eq!(clusters(&pieces), [0, 1, 7]);
    }
}
