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
//! # The private-use fallback
//!
//! The rest of <https://linux.thai.net/~thep/th-otf/shaping.html> is about
//! where a tone mark goes when the consonant under it is the wrong shape.
//! Thai stacks up to two marks over a consonant, and three things spoil the
//! default stacking:
//!
//! * **Tall consonants** — `ป`, `ฝ`, `ฟ` carry an ascender the mark would
//!   collide with, so a mark above them shifts *left* of it.
//! * **Descenders** — `ญ` and `ฐ` have a tail that a below-base vowel would
//!   land on, so the tail is removed and a bare form drawn instead.
//! * **A mark already there** — a tone mark over a vowel that is itself above
//!   the consonant has to shift *down* to sit on the vowel rather than float.
//!
//! An OpenType Thai font expresses all of that in `GSUB` and `GPOS`. A font
//! from before OpenType expresses it by shipping the shifted forms as extra
//! glyphs in the private use area — one set at U+F700 for Windows, another at
//! U+F880 for the Mac — and leaving the engine to pick them. [`pua_shape`] is
//! that pass: two small state machines, one tracking what is stacked above the
//! consonant and one what is below, that between them name the substitution.
//!
//! It runs **only on a face whose `GSUB` does not register `thai`**, because a
//! face that does has described its own shaping and is entitled to be believed.
//! HarfBuzz gates it the same way, on `!plan->map.found_script[0]`, and applies
//! it to Thai only — Lao has no private-use convention.
//!
//! What it replaces is the *glyph*, not the character: a shifted mai ek is
//! still a mai ek, and the shaper downstream still has to know it is a mark to
//! give it no advance. Rewriting the character would lose that, because a
//! private-use codepoint's general category is `Co`.

use alloc::vec::Vec;
use core::ops::Range;

use crate::norm::Piece;
use crate::script::ScriptTags;

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

/// Whether a run of `script` is one the private-use fallback applies to.
///
/// Thai and not Lao. The private-use conventions were Windows' and Apple's
/// answers to Thai specifically; no equivalent was ever defined for Lao, and
/// HarfBuzz tests `props.script == HB_SCRIPT_THAI` for the same reason.
#[must_use]
pub(crate) fn legacy_run(script: Option<ScriptTags>) -> bool {
    script.is_some_and(|s| s.preferred == *b"thai")
}

/// What kind of consonant a character is, which is what decides how much room
/// there is above it and what is in the way below.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Consonant {
    /// Normal: no ascender, no descender.
    Normal,
    /// Ascending — `ป` PO PLA, `ฝ` FO FA, `ฟ` FO FAN. A mark above one of
    /// these has to move left of the ascender.
    Ascending,
    /// Removable descender — `ญ` YO YING, `ฐ` THO THAN. The tail comes off to
    /// make room for a below-base vowel.
    Removable,
    /// Fixed descender — `ฎ` DO CHADA, `ฏ` TO PATAK. The tail stays and the
    /// vowel moves instead.
    Descending,
    /// Not a Thai consonant at all, including a vowel or a space. Treated as
    /// the most crowded case, so nothing is shifted onto it.
    None,
}

/// Which of the three positions a mark occupies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MarkType {
    /// Above-base vowel, and the nikhahit.
    Above,
    /// Below-base vowel.
    Below,
    /// Tone mark, or thanthakhat. Drawn above whatever is already there.
    Tone,
}

/// What to draw instead, when the default form will not do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    /// Leave it alone.
    Nop,
    /// Shift the mark down — there is nothing above the consonant after all.
    Down,
    /// Shift the mark left — the consonant has an ascender in the way.
    Left,
    /// Both.
    DownLeft,
    /// Remove the consonant's descender. The only action that rewrites the
    /// *base* rather than the mark.
    Descender,
}

/// How full the space above the consonant is.
///
/// The names are HarfBuzz's `T0`–`T3`, and the order is how much is stacked:
/// nothing, an ascender, an ascender and a mark, full.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Above {
    Empty,
    Ascender,
    AscenderAndMark,
    Full,
}

/// What is below the consonant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Below {
    /// Nothing in the way.
    Clear,
    /// A descender that may be removed to make room.
    Removable,
    /// A descender that stays, or something already under there.
    Occupied,
}

/// The Thai consonants that shape differently, and everything else.
///
/// U+0E2C LO CHULA is an ascender too, but HarfBuzz leaves it out — the
/// comment in `hb-ot-shaper-thai.cc` has it commented out rather than absent,
/// so this is a deliberate match with Uniscribe rather than an oversight, and
/// this crate copies it to stay comparable against the oracle.
fn consonant_type(ch: char) -> Consonant {
    match ch as u32 {
        0x0E1B | 0x0E1D | 0x0E1F => Consonant::Ascending,
        0x0E0D | 0x0E10 => Consonant::Removable,
        0x0E0E | 0x0E0F => Consonant::Descending,
        0x0E01..=0x0E2E => Consonant::Normal,
        _ => Consonant::None,
    }
}

/// Which position a mark takes, or `None` when the character is not a mark
/// this fallback knows about — which resets both machines.
fn mark_type(ch: char) -> Option<MarkType> {
    match ch as u32 {
        0x0E31 | 0x0E34..=0x0E37 | 0x0E47 | 0x0E4D | 0x0E4E => Some(MarkType::Above),
        0x0E38..=0x0E3A => Some(MarkType::Below),
        0x0E48..=0x0E4C => Some(MarkType::Tone),
        _ => None,
    }
}

/// Where the above-base machine starts, given the consonant it starts on.
///
/// An ascending consonant already fills the first level; anything that is not
/// a consonant is treated as full, so a stray mark is never shifted onto it.
fn above_start(consonant: Consonant) -> Above {
    match consonant {
        Consonant::Ascending => Above::Ascender,
        Consonant::None => Above::Full,
        _ => Above::Empty,
    }
}

/// Where the below-base machine starts.
fn below_start(consonant: Consonant) -> Below {
    match consonant {
        Consonant::Removable => Below::Removable,
        Consonant::Descending | Consonant::None => Below::Occupied,
        _ => Below::Clear,
    }
}

/// One step of the above-base machine: what to do, and what the space above
/// the consonant looks like afterwards.
///
/// A tone mark landing on an empty space drops onto the consonant
/// ([`Action::Down`]); landing over an ascender it drops *and* moves left; and
/// once a vowel is already up there it only moves left, because the vowel is
/// holding it at the right height. After anything at all the space is full and
/// nothing more is shifted.
fn above_step(state: Above, mark: MarkType) -> (Action, Above) {
    match (state, mark) {
        (Above::Empty, MarkType::Above) => (Action::Nop, Above::Full),
        (Above::Empty, MarkType::Below) => (Action::Nop, Above::Empty),
        (Above::Empty, MarkType::Tone) => (Action::Down, Above::Full),
        (Above::Ascender, MarkType::Above) => (Action::Left, Above::AscenderAndMark),
        (Above::Ascender, MarkType::Below) => (Action::Nop, Above::Ascender),
        (Above::Ascender, MarkType::Tone) => (Action::DownLeft, Above::AscenderAndMark),
        (Above::AscenderAndMark, MarkType::Above) => (Action::Nop, Above::Full),
        (Above::AscenderAndMark, MarkType::Below) => (Action::Nop, Above::AscenderAndMark),
        (Above::AscenderAndMark, MarkType::Tone) => (Action::Left, Above::Full),
        (Above::Full, _) => (Action::Nop, Above::Full),
    }
}

/// One step of the below-base machine.
///
/// Only a below-base vowel does anything: it takes a removable descender off
/// the consonant, or — if the descender is one that stays — shifts itself
/// down out of the way. Either way the space below is occupied afterwards.
fn below_step(state: Below, mark: MarkType) -> (Action, Below) {
    match (state, mark) {
        (Below::Clear, MarkType::Below) => (Action::Nop, Below::Occupied),
        (Below::Removable, MarkType::Below) => (Action::Descender, Below::Occupied),
        (Below::Occupied, MarkType::Below) => (Action::Down, Below::Occupied),
        (state, _) => (Action::Nop, state),
    }
}

/// The private-use forms, as `(character, Windows, Mac)`.
///
/// Two conventions because two vendors invented one each, and a font may carry
/// either. Windows' is tried first, which is HarfBuzz's order and the one that
/// matches the fonts that are actually installed.
const SHIFTED_DOWN: &[(u32, u32, u32)] = &[
    (0x0E48, 0xF70A, 0xF88B), // MAI EK
    (0x0E49, 0xF70B, 0xF88E), // MAI THO
    (0x0E4A, 0xF70C, 0xF891), // MAI TRI
    (0x0E4B, 0xF70D, 0xF894), // MAI CHATTAWA
    (0x0E4C, 0xF70E, 0xF897), // THANTHAKHAT
    (0x0E38, 0xF718, 0xF89B), // SARA U
    (0x0E39, 0xF719, 0xF89C), // SARA UU
    (0x0E3A, 0xF71A, 0xF89D), // PHINTHU
];

/// Shifted down and left. Tone marks only: a vowel never needs both.
const SHIFTED_DOWN_LEFT: &[(u32, u32, u32)] = &[
    (0x0E48, 0xF705, 0xF88C), // MAI EK
    (0x0E49, 0xF706, 0xF88F), // MAI THO
    (0x0E4A, 0xF707, 0xF892), // MAI TRI
    (0x0E4B, 0xF708, 0xF895), // MAI CHATTAWA
    (0x0E4C, 0xF709, 0xF898), // THANTHAKHAT
];

/// Shifted left, to clear an ascender.
const SHIFTED_LEFT: &[(u32, u32, u32)] = &[
    (0x0E48, 0xF713, 0xF88A), // MAI EK
    (0x0E49, 0xF714, 0xF88D), // MAI THO
    (0x0E4A, 0xF715, 0xF890), // MAI TRI
    (0x0E4B, 0xF716, 0xF893), // MAI CHATTAWA
    (0x0E4C, 0xF717, 0xF896), // THANTHAKHAT
    (0x0E31, 0xF710, 0xF884), // MAI HAN-AKAT
    (0x0E34, 0xF701, 0xF885), // SARA I
    (0x0E35, 0xF702, 0xF886), // SARA II
    (0x0E36, 0xF703, 0xF887), // SARA UE
    (0x0E37, 0xF704, 0xF888), // SARA UEE
    (0x0E47, 0xF712, 0xF889), // MAITAIKHU
    (0x0E4D, 0xF711, 0xF899), // NIKHAHIT
];

/// The consonants with their descender taken off.
const NO_DESCENDER: &[(u32, u32, u32)] = &[
    (0x0E0D, 0xF70F, 0xF89A), // YO YING
    (0x0E10, 0xF700, 0xF89E), // THO THAN
];

/// The private-use character `ch` should be drawn as under `action`, if this
/// face has one.
///
/// `None` for a face that has neither vendor's form, which is every font that
/// was designed for OpenType — and the reason this pass is silently harmless
/// on a face it does not apply to.
fn shifted(ch: char, action: Action, has_glyph: &impl Fn(char) -> bool) -> Option<char> {
    let table = match action {
        Action::Nop => return None,
        Action::Down => SHIFTED_DOWN,
        Action::DownLeft => SHIFTED_DOWN_LEFT,
        Action::Left => SHIFTED_LEFT,
        Action::Descender => NO_DESCENDER,
    };
    let &(_, windows, mac) = table.iter().find(|&&(u, _, _)| u == ch as u32)?;
    [windows, mac]
        .into_iter()
        .filter_map(char::from_u32)
        .find(|&form| has_glyph(form))
}

/// Pick private-use forms for the marks in `range` that the default forms
/// would place wrongly.
///
/// `out` is one slot per piece, and this writes the replacement *glyph*
/// character into the slots that need one — never the piece itself, because
/// the character still has to answer questions about what kind of mark it is.
/// Slots outside `range`, and those needing no replacement, are left alone.
pub(crate) fn pua_shape(
    pieces: &[Piece],
    range: Range<usize>,
    out: &mut [Option<char>],
    has_glyph: impl Fn(char) -> bool,
) {
    let mut above = above_start(Consonant::None);
    let mut below = below_start(Consonant::None);
    // The consonant the marks are landing on, which [`Action::Descender`]
    // rewrites instead of the mark. Starts at the run's first piece, matching
    // HarfBuzz — a mark with no consonant before it can only reach `Descender`
    // from a state no such run is in, so what it names is unreachable rather
    // than wrong.
    let mut base = range.start;
    for i in range {
        let Some(&(ch, _)) = pieces.get(i) else {
            break;
        };
        let Some(mark) = mark_type(ch) else {
            let consonant = consonant_type(ch);
            above = above_start(consonant);
            below = below_start(consonant);
            base = i;
            continue;
        };
        let (above_action, above_next) = above_step(above, mark);
        let (below_action, below_next) = below_step(below, mark);
        above = above_next;
        below = below_next;
        // At most one machine ever fires: the above one only acts on marks it
        // has room for, the below one only on below-base vowels, and no state
        // pairs an action with an action.
        let action = if above_action == Action::Nop {
            below_action
        } else {
            above_action
        };
        let (at, target) = if action == Action::Descender {
            (base, pieces.get(base).map(|&(c, _)| c))
        } else {
            (i, Some(ch))
        };
        if let Some(target) = target
            && let Some(form) = shifted(target, action, &has_glyph)
            && let Some(slot) = out.get_mut(at)
        {
            *slot = Some(form);
        }
    }
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

    /// The private-use pass over `text`, on a face carrying every form of
    /// `vendor` — `0xF700` for the Windows convention, `0xF880` for the Mac's.
    fn pua(text: &str, vendor: u32) -> Vec<Option<u32>> {
        let mut pieces: Vec<Piece> = text.char_indices().map(|(at, ch)| (ch, at)).collect();
        preprocess(&mut pieces);
        let mut out = vec![None; pieces.len()];
        pua_shape(&pieces, 0..pieces.len(), &mut out, |ch| {
            (ch as u32) & 0xFF80 == vendor
        });
        out.iter()
            .map(|slot| slot.map(|ch: char| ch as u32))
            .collect()
    }

    const WIN: u32 = 0xF700;
    const MAC: u32 = 0xF880;

    #[test]
    fn a_tone_mark_on_a_plain_consonant_shifts_down() {
        // Nothing is above KO KAI, so the mark drops onto it rather than
        // floating at the height a vowel would have held it.
        assert_eq!(pua("\u{0E01}\u{0E48}", WIN), vec![None, Some(0xF70A)]);
    }

    #[test]
    fn a_tone_mark_on_an_ascending_consonant_shifts_down_and_left() {
        // PO PLA's ascender is where the mark would have gone.
        assert_eq!(pua("\u{0E1B}\u{0E48}", WIN), vec![None, Some(0xF705)]);
    }

    #[test]
    fn a_vowel_on_an_ascending_consonant_shifts_left() {
        assert_eq!(pua("\u{0E1B}\u{0E34}", WIN), vec![None, Some(0xF701)]);
    }

    #[test]
    fn a_tone_mark_over_a_shifted_vowel_shifts_left_only() {
        // The vowel is already holding the tone at the right height, so it
        // only has to clear the ascender.
        assert_eq!(
            pua("\u{0E1B}\u{0E34}\u{0E48}", WIN),
            vec![None, Some(0xF701), Some(0xF713)]
        );
    }

    #[test]
    fn a_tone_mark_over_an_unshifted_vowel_is_left_alone() {
        // KO KAI has no ascender, so the vowel sits where it always does and
        // the tone sits on the vowel. Both default forms are right.
        assert_eq!(pua("\u{0E01}\u{0E34}\u{0E48}", WIN), vec![None, None, None]);
    }

    #[test]
    fn a_below_base_vowel_takes_off_a_removable_descender() {
        // YO YING's tail comes off; the vowel itself is unchanged, and it is
        // the *base* that is rewritten.
        assert_eq!(pua("\u{0E0D}\u{0E38}", WIN), vec![Some(0xF70F), None]);
    }

    #[test]
    fn a_below_base_vowel_moves_down_past_a_descender_that_stays() {
        // DO CHADA's tail is part of the letter, so the vowel gives way.
        assert_eq!(pua("\u{0E0E}\u{0E38}", WIN), vec![None, Some(0xF718)]);
    }

    #[test]
    fn a_below_base_vowel_on_a_plain_consonant_is_left_alone() {
        assert_eq!(pua("\u{0E01}\u{0E38}", WIN), vec![None, None]);
    }

    #[test]
    fn the_mac_forms_are_taken_when_the_windows_ones_are_missing() {
        assert_eq!(pua("\u{0E01}\u{0E48}", MAC), vec![None, Some(0xF88B)]);
    }

    #[test]
    fn a_face_with_neither_vendors_forms_is_left_entirely_alone() {
        // Which is every font designed for OpenType, and the reason this pass
        // is safe to run whenever the `GSUB` script is missing.
        assert_eq!(pua("\u{0E1B}\u{0E34}\u{0E48}", 0), vec![None, None, None]);
    }

    #[test]
    fn the_nikhahit_from_a_sara_am_is_shifted_like_any_other_vowel() {
        // The two passes in sequence: SARA AM splits, and the circle it left
        // above PO PLA then has to clear PO PLA's ascender.
        assert_eq!(
            pua("\u{0E1B}\u{0E33}", WIN),
            vec![None, Some(0xF711), None]
        );
    }

    #[test]
    fn a_consonant_resets_both_machines() {
        // Two syllables, and the second is shaped as if the first were not
        // there.
        assert_eq!(
            pua("\u{0E1B}\u{0E48}\u{0E01}\u{0E48}", WIN),
            vec![None, Some(0xF705), None, Some(0xF70A)]
        );
    }

    #[test]
    fn a_mark_with_no_consonant_before_it_is_left_alone() {
        // The machines start in their most crowded states, so a stray mark is
        // never shifted onto something that is not there.
        assert_eq!(pua("\u{0E48}", WIN), vec![None]);
        assert_eq!(pua(" \u{0E48}", WIN), vec![None, None]);
    }

    #[test]
    fn only_thai_runs_reach_the_private_use_pass() {
        assert!(legacy_run(Some(ScriptTags {
            preferred: *b"thai",
            fallback: *b"thai",
        })));
        assert!(!legacy_run(Some(ScriptTags {
            preferred: *b"lao ",
            fallback: *b"lao ",
        })));
        assert!(!legacy_run(None));
    }

    #[test]
    fn no_private_use_form_stands_for_two_different_things() {
        // A character may well be in three tables — a tone mark has a down, a
        // left and a down-left form — but a private-use codepoint stands for
        // exactly one of them, in one vendor's convention. Two entries sharing
        // one would silently draw the wrong shift, and the tables are 60 hand-
        // transcribed hex numbers.
        let tables = [SHIFTED_DOWN, SHIFTED_DOWN_LEFT, SHIFTED_LEFT, NO_DESCENDER];
        let mut forms: Vec<u32> = Vec::new();
        for table in tables {
            let mut characters: Vec<u32> = table.iter().map(|&(u, _, _)| u).collect();
            let before = characters.len();
            characters.sort_unstable();
            characters.dedup();
            assert_eq!(before, characters.len(), "a table names a character twice");
            for &(_, windows, mac) in table {
                assert!(
                    (0xF700..0xF800).contains(&windows),
                    "{windows:04X} is not in the Windows range"
                );
                assert!(
                    (0xF880..0xF8A0).contains(&mac),
                    "{mac:04X} is not in the Mac range"
                );
                forms.push(windows);
                forms.push(mac);
            }
        }
        let before = forms.len();
        forms.sort_unstable();
        forms.dedup();
        assert_eq!(before, forms.len(), "two entries share a private-use form");
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
