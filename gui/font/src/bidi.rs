//! The Unicode Bidirectional Algorithm, UAX #9.
//!
//! Arabic and Hebrew are written right to left, but they are *stored* left to
//! right — in the order the characters were typed, which is the order they are
//! read in. Something has to turn one into the other, and it cannot simply be
//! "reverse the run if the script is RTL": a line of Arabic containing an
//! English brand name, a phone number and a pair of parentheses has spans
//! running in both directions, nested, and the rules deciding where each span
//! begins and ends are neither local nor obvious. Rule N0 alone — which
//! direction a bracket pair takes — depends on what is *inside* the brackets
//! and, failing that, on what came before them.
//!
//! So this is the whole algorithm rather than a shortcut. A shortcut is worse
//! than nothing here: reversing every RTL run gets the simple cases right and
//! mixed text subtly wrong, and a subtly wrong answer is one nobody notices
//! until a user reports that a phone number in an Arabic sentence has its
//! digits backwards.
//!
//! # What it produces
//!
//! An *embedding level* per character: an integer where even means
//! left-to-right and odd means right-to-left, and larger means more deeply
//! nested. [`Paragraph::reorder`] turns levels into the visual order the
//! glyphs are drawn in, by rule L2. [`mirror`] answers rule L4, which is why
//! `(` is drawn as `)` inside an Arabic phrase: the character means "the
//! bracket that opens", and in a right-to-left run the bracket that opens is
//! the one that looks like `)`.
//!
//! # Where it sits
//!
//! Above the shaper and below layout. The shaper keeps returning glyphs in
//! logical order — a ligature's components must stay adjacent and in order for
//! `GSUB` to match them, and reversing before shaping would break every
//! contextual lookup. Layout then walks the reordered indices. `script::runs`
//! splits on level as well as script, so a run handed to `GSUB` is uniform in
//! both.
//!
//! # Structure
//!
//! The rules are numbered by UAX #9 and the code keeps those numbers, because
//! the numbers are how the spec is navigated and how a conformance failure is
//! reported. The stages, in order:
//!
//! * **P2–P3** — the paragraph's own direction, from its first strong
//!   character.
//! * **X1–X8** — explicit embeddings and isolates: the `RLE`/`LRE`/`RLO`/`LRO`
//!   /`PDF` stack and the newer `RLI`/`LRI`/`FSI`/`PDI` isolates, which differ
//!   in that the text inside an isolate is invisible to the text outside it.
//! * **X9** — remove the formatting characters, which have no glyphs.
//! * **X10** — carve the text into *isolating run sequences*: the units the
//!   remaining rules run over. A sequence spans the isolate boundaries so that
//!   an isolate's surroundings read as continuous text.
//! * **W1–W7** — weak types: numbers, separators and combining marks take
//!   their direction from their neighbours.
//! * **N0–N2** — neutrals: brackets first, then whitespace and punctuation.
//! * **I1–I2** — turn resolved types into level increments.
//! * **L1–L2** — reset trailing whitespace to the paragraph level, then
//!   reorder.
//!
//! # What it is checked against
//!
//! Unicode ships a conformance suite, `BidiCharacterTest.txt`: ninety-odd
//! thousand strings with their expected paragraph level, per-character levels
//! and visual order. `gui/font/tools/bidi_conformance.py` runs the whole of it
//! and `tests/bidi_conformance.rs` runs a checked-in sample, so a rule cannot
//! be quietly wrong.

use alloc::vec;
use alloc::vec::Vec;

use crate::bidi_tables::{BIDI_CLASS_RANGES, BRACKETS, MIRRORED};

/// An embedding level: even is left-to-right, odd is right-to-left.
pub type Level = u8;

/// The deepest embedding UAX #9 allows (BD2).
///
/// Beyond this an embedding or isolate initiator is an *overflow*: it is
/// counted so that its matching terminator can be counted off against it, but
/// it changes no level. The cap exists so that a hostile string of a million
/// `RLE`s cannot make the stack grow without bound.
const MAX_DEPTH: Level = 125;

/// The most bracket pairs rule N0 will track (BD16).
///
/// The spec's own limit, and for the same reason as [`MAX_DEPTH`]: the stack
/// is bounded so the rule terminates on any input. Text that exceeds it stops
/// having its brackets paired, which is the behaviour BD16 specifies —
/// not an error.
const MAX_BRACKET_PAIRS: usize = 63;

/// A character's `Bidi_Class`.
///
/// The variant order is the one [`bidi_tables`](crate::bidi_tables) is
/// generated against, so the two must change together.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    /// Left-to-right: Latin, Greek, Cyrillic, and the default almost
    /// everywhere else.
    L,
    /// Right-to-left: Hebrew and the other RTL scripts that are not Arabic.
    R,
    /// Arabic letter. Distinct from `R` because it changes how the digits
    /// after it are read (W2).
    Al,
    /// European number: `0`–`9`.
    En,
    /// European separator: `+`, `-`.
    Es,
    /// European terminator: `$`, `%`, `°`.
    Et,
    /// Arabic-Indic number.
    An,
    /// Common separator: `,`, `.`, `:`.
    Cs,
    /// Non-spacing mark. Takes the direction of what it sits on (W1).
    Nsm,
    /// Boundary neutral: formatting characters with no width. Removed by X9.
    Bn,
    /// Paragraph separator.
    B,
    /// Segment separator: tab.
    S,
    /// Whitespace.
    Ws,
    /// Other neutral: most punctuation and symbols.
    On,
    /// `U+202A` LEFT-TO-RIGHT EMBEDDING.
    Lre,
    /// `U+202D` LEFT-TO-RIGHT OVERRIDE.
    Lro,
    /// `U+202B` RIGHT-TO-LEFT EMBEDDING.
    Rle,
    /// `U+202E` RIGHT-TO-LEFT OVERRIDE.
    Rlo,
    /// `U+202C` POP DIRECTIONAL FORMATTING.
    Pdf,
    /// `U+2066` LEFT-TO-RIGHT ISOLATE.
    Lri,
    /// `U+2067` RIGHT-TO-LEFT ISOLATE.
    Rli,
    /// `U+2068` FIRST STRONG ISOLATE.
    Fsi,
    /// `U+2069` POP DIRECTIONAL ISOLATE.
    Pdi,
}

impl Class {
    /// Is this one of the three isolate initiators? (BD8)
    #[must_use]
    const fn is_isolate_initiator(self) -> bool {
        matches!(self, Self::Lri | Self::Rli | Self::Fsi)
    }

    /// Is this a character rule X9 removes?
    ///
    /// The embedding and override formatting characters, and the boundary
    /// neutrals. None of them has a glyph, and every later rule would have to
    /// special-case them, so they are taken out of the sequence entirely.
    #[must_use]
    const fn is_removed_by_x9(self) -> bool {
        matches!(
            self,
            Self::Rle | Self::Lre | Self::Rlo | Self::Lro | Self::Pdf | Self::Bn
        )
    }

    /// Is this an `NI` — a neutral or isolate formatting character? (BD11)
    ///
    /// The set rules N0–N2 resolve. Isolate initiators and `PDI` are in it
    /// because, from outside, an isolate is a single neutral object.
    #[must_use]
    const fn is_neutral_or_isolate(self) -> bool {
        matches!(self, Self::B | Self::S | Self::Ws | Self::On)
            || self.is_isolate_initiator()
            || matches!(self, Self::Pdi)
    }

    /// Is this one of the strong types P2 looks for?
    #[must_use]
    const fn is_strong(self) -> bool {
        matches!(self, Self::L | Self::R | Self::Al)
    }
}

/// The direction to resolve a paragraph in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Base {
    /// Rule P2–P3: take it from the paragraph's first strong character. What a
    /// text editor or a chat window wants, since the user's language is a
    /// property of what they typed.
    #[default]
    Auto,
    /// Force left-to-right. What a UI wants for a label whose surrounding
    /// layout is left-to-right regardless of the text in it.
    Ltr,
    /// Force right-to-left.
    Rtl,
}

/// One resolved paragraph: a level per character, plus the paragraph's own.
#[derive(Clone, Debug)]
pub struct Paragraph {
    level: Level,
    levels: Vec<Level>,
    /// The classes as they arrived, before any rule rewrote them. L1 needs
    /// these rather than the resolved ones — the whitespace it resets is
    /// whitespace as typed, not whatever N2 turned it into.
    original: Vec<Class>,
    /// The indices rule X9 kept, in logical order. `reorder` walks these, and
    /// only these: see there for why an `LRE` left in the sequence would
    /// reorder the text around it wrongly.
    retained: Vec<usize>,
}

impl Paragraph {
    /// The paragraph's own embedding level: 0 for left-to-right, 1 for right.
    #[must_use]
    pub const fn level(&self) -> Level {
        self.level
    }

    /// Does this paragraph read right to left?
    #[must_use]
    pub const fn is_rtl(&self) -> bool {
        self.level % 2 == 1
    }

    /// The resolved embedding level of each character, in logical order.
    #[must_use]
    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    /// Is any character at a level that differs from the paragraph's?
    ///
    /// The question a caller asks to skip the reordering work entirely, which
    /// is the answer for essentially all English text.
    #[must_use]
    pub fn is_uniform(&self) -> bool {
        self.levels.iter().all(|&l| l == self.level)
    }

    /// The character indices in the order they are drawn, left to right (L2).
    ///
    /// Rule L2, in full: from the highest level down to the lowest odd level,
    /// reverse every contiguous run of characters at or above that level. The
    /// nesting falls out of doing it repeatedly — an English phrase inside an
    /// Arabic sentence is reversed once as part of the Arabic and once on its
    /// own, which puts it back the right way round.
    ///
    /// The characters rule X9 removed are **not** in the result. They have no
    /// glyphs, so a caller has nothing to draw for them; and they must not be
    /// in the sequence L2 walks, because an `LRE` keeps the level in force
    /// before it and would split the run around it in two — which reverses the
    /// two halves separately and puts the text in the wrong order. L2 runs
    /// after X9 for exactly that reason.
    #[must_use]
    pub fn reorder(&self) -> Vec<usize> {
        let levels: Vec<Level> = self
            .retained
            .iter()
            .map(|&i| self.levels.get(i).copied().unwrap_or(self.level))
            .collect();
        visual_order(&levels)
            .into_iter()
            .map(|at| self.retained.get(at).copied().unwrap_or(at))
            .collect()
    }

    /// A level per character for a *renderer*, with the characters rule X9
    /// removed given a neighbour's level instead of their own.
    ///
    /// [`levels`](Self::levels) reports what the algorithm resolved, where a
    /// removed character keeps the level in force *before* it. That is right
    /// for the algorithm and wrong for a glyph stream that still contains the
    /// character: an `LRE` between two Arabic letters would sit at level 0
    /// between two level-1 neighbours, splitting one level run into two, and
    /// L2 would then reverse the two halves separately and put the word in the
    /// wrong order. [`reorder`](Self::reorder) avoids that by dropping the
    /// removed characters entirely — but a shaper cannot always drop them,
    /// because `ZWJ` is one of them and Arabic joining is decided by it.
    ///
    /// So here they ride along with their neighbours: each takes the level of
    /// the character before it that survived X9, or the paragraph's own level
    /// when there is none. They are invisible either way; all that matters is
    /// that they never divide a run.
    #[must_use]
    pub fn render_levels(&self) -> Vec<Level> {
        let mut out = self.levels.clone();
        if self.retained.len() == self.levels.len() {
            return out;
        }
        let mut prev = self.level;
        for i in 0..out.len() {
            if self.original.get(i).is_some_and(|c| c.is_removed_by_x9()) {
                if let Some(slot) = out.get_mut(i) {
                    *slot = prev;
                }
            } else {
                prev = out.get(i).copied().unwrap_or(prev);
            }
        }
        out
    }
}

/// Rule L2 over a level per item: the order those items are drawn in.
///
/// From the highest level down to the lowest odd level, reverse every
/// contiguous stretch at or above that level. The nesting falls out of doing
/// it repeatedly — an English phrase inside an Arabic sentence is reversed
/// once as part of the Arabic and once on its own, which puts it back the
/// right way round.
///
/// Taking a level array rather than a [`Paragraph`] is what lets a shaper
/// apply L2 to *glyphs*: a ligature is one glyph for several characters and a
/// decomposition is several glyphs for one, so by the time there is something
/// to draw the run no longer has one item per character. What it does still
/// have is a level per item.
#[must_use]
pub fn visual_order(levels: &[Level]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..levels.len()).collect();
    let Some(&highest) = levels.iter().max() else {
        return order;
    };
    // The lowest odd level, which is where the reversing stops. Levels below
    // it are all even and all left-to-right, so reversing them would be wrong.
    // No odd level at all means nothing moves, which is the answer for every
    // left-to-right string.
    let Some(&lowest_odd) = levels.iter().filter(|l| !l.is_multiple_of(2)).min() else {
        return order;
    };

    let mut level = highest;
    while level >= lowest_odd && level > 0 {
        let mut i = 0;
        while i < levels.len() {
            if levels.get(i).is_none_or(|&l| l < level) {
                i = i.saturating_add(1);
                continue;
            }
            let start = i;
            while levels.get(i).is_some_and(|&l| l >= level) {
                i = i.saturating_add(1);
            }
            if let Some(slice) = order.get_mut(start..i) {
                slice.reverse();
            }
        }
        level = level.saturating_sub(1);
    }
    order
}

/// Can `text` be laid out without resolving it at all?
///
/// True when nothing in it can produce a right-to-left run: no strong
/// right-to-left character, and no explicit directional formatting. Every
/// character is then at an even level, [`visual_order`] is the identity and
/// rule L4 mirrors nothing — so a shaper can skip the entire algorithm, which
/// is the answer for English and for every other left-to-right script.
///
/// The code-point comparison is not a micro-optimization but the whole of the
/// check's cost in the common case: the lowest right-to-left character in
/// Unicode is U+0590, so a string of Latin, Greek, Cyrillic, Han or Devanagari
/// is rejected by one comparison per character and never reaches the class
/// table at all.
///
/// Note what is *not* here: `EN` and `AN` do raise a character's level (rule
/// I1 takes a digit in left-to-right text to level 2), but only ever to an
/// even one, so digits neither reverse nor mirror and do not disqualify the
/// fast path.
#[must_use]
pub fn is_trivially_ltr(text: &str) -> bool {
    /// U+0590, the first code point in the Hebrew block, below which no
    /// character is `R`, `AL`, or an explicit directional formatting control.
    const LOWEST_RTL: u32 = 0x590;
    !text.chars().any(|ch| {
        ch as u32 >= LOWEST_RTL
            && matches!(
                class(ch),
                Class::R
                    | Class::Al
                    | Class::Rle
                    | Class::Rlo
                    | Class::Rli
                    | Class::Fsi
                    // These four cannot make a level odd by themselves, but
                    // they are removed by rule X9 and so must not be drawn.
                    // Sending them down the slow path is what removes them.
                    | Class::Lre
                    | Class::Lro
                    | Class::Lri
                    | Class::Pdf
            )
    })
}

/// `Bidi_Class` of one character.
#[must_use]
pub fn class(ch: char) -> Class {
    let cp = ch as u32;
    let found = BIDI_CLASS_RANGES.binary_search_by(|&(lo, hi, _)| {
        if hi < cp {
            core::cmp::Ordering::Less
        } else if lo > cp {
            core::cmp::Ordering::Greater
        } else {
            core::cmp::Ordering::Equal
        }
    });
    match found.ok().and_then(|i| BIDI_CLASS_RANGES.get(i)) {
        Some(&(_, _, kind)) => kind,
        // Absent from the table means `L`: see `gen_bidi_tables.py` for why
        // the most common value is the one left out.
        None => Class::L,
    }
}

/// The mirrored form of `ch`, if it has one (rule L4).
///
/// `(` in a right-to-left run is drawn as `)`, because the character encodes
/// "the bracket that opens" and the side a bracket opens on depends on which
/// way the text runs. Callers apply this only to characters at an odd level.
#[must_use]
pub fn mirror(ch: char) -> Option<char> {
    let cp = ch as u32;
    let i = MIRRORED.binary_search_by_key(&cp, |&(c, _)| c).ok()?;
    let &(_, m) = MIRRORED.get(i)?;
    char::from_u32(m)
}

/// The bracket entry for `ch`: its canonical pair, and whether it opens.
fn bracket(ch: char) -> Option<(u32, bool)> {
    let cp = ch as u32;
    let i = BRACKETS.binary_search_by_key(&cp, |&(c, _, _)| c).ok()?;
    let &(_, pair, opening) = BRACKETS.get(i)?;
    Some((pair, opening))
}

/// `ch` folded to the canonical spelling its bracket pairs are recorded under.
///
/// A closing bracket has to compare equal to whatever the opening bracket
/// named as its pair, and the table stores that name canonically — so the
/// closing bracket must be folded the same way. Only two characters in Unicode
/// need this (`U+2329` and `U+232A`, the angle brackets with singleton
/// decompositions), and both are handled by their own table rows: an entry's
/// `pair` is already folded, so folding a *closing* bracket means asking the
/// table for the pair of its pair. Simpler: a closing bracket's own canonical
/// form is the `pair` its counterpart names, and that is what this returns.
fn canonical_bracket(ch: char) -> u32 {
    // `BRACKETS` records, for each bracket, the *canonical* code point of its
    // counterpart. The counterpart of the counterpart is the canonical form of
    // this character, and going round the loop that way avoids carrying a
    // decomposition table into this module for two code points.
    let Some((pair, _)) = bracket(ch) else {
        return ch as u32;
    };
    let Some(back) = char::from_u32(pair).and_then(bracket) else {
        return ch as u32;
    };
    back.0
}

/// Rules P2 and P3: the direction of `classes`, from its first strong
/// character.
///
/// Characters inside an isolate are skipped — that is the whole point of an
/// isolate, and it is why this cannot be a plain `find`. `None` means no
/// strong character was found, which P3 resolves as left-to-right.
fn first_strong(classes: &[Class]) -> Option<Level> {
    let mut i = 0;
    while let Some(&kind) = classes.get(i) {
        if kind.is_isolate_initiator() {
            i = match matching_pdi(classes, i) {
                Some(at) => at.saturating_add(1),
                None => return None,
            };
            continue;
        }
        match kind {
            Class::L => return Some(0),
            Class::R | Class::Al => return Some(1),
            _ => i = i.saturating_add(1),
        }
    }
    None
}

/// BD9: the index of the `PDI` that matches the isolate initiator at `at`.
///
/// Nested isolates are counted off against each other, so the match is the
/// first `PDI` that is not consumed by an inner isolate. `None` means the
/// initiator is unmatched — legal, and treated by X6a and the sequence rules
/// as running to the end of the paragraph.
fn matching_pdi(classes: &[Class], at: usize) -> Option<usize> {
    let mut depth = 1u32;
    let mut i = at.saturating_add(1);
    while let Some(&kind) = classes.get(i) {
        if kind.is_isolate_initiator() {
            depth = depth.saturating_add(1);
        } else if kind == Class::Pdi {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(i);
            }
        }
        i = i.saturating_add(1);
    }
    None
}

/// One entry of the directional status stack (X1).
#[derive(Clone, Copy)]
struct Status {
    level: Level,
    /// `Some(L)` or `Some(R)` while an override is in force, which rewrites
    /// every character's class rather than merely nesting it.
    overridden: Option<Class>,
    /// Whether this entry was pushed by an isolate initiator, which decides
    /// what a `PDI` pops (X6a) and what a `PDF` may not (X7).
    isolate: bool,
}

/// Resolve one paragraph of `text` into embedding levels.
///
/// `text` is one paragraph: rule P1's split on paragraph separators is the
/// caller's, because a caller laying out a document already knows where its
/// paragraphs are and one laying out a single label has exactly one.
#[must_use]
pub fn resolve(text: &[char], base: Base) -> Paragraph {
    let original: Vec<Class> = text.iter().copied().map(class).collect();

    // P2, P3.
    let para = match base {
        Base::Ltr => 0,
        Base::Rtl => 1,
        Base::Auto => first_strong(&original).unwrap_or(0),
    };

    // X1–X8. `classes` is rewritten in place by the overrides.
    let mut classes = original.clone();
    let mut levels = vec![para; text.len()];
    explicit(&original, &mut classes, &mut levels, para);

    // X9: the formatting characters have no glyphs and no part in the rules
    // that follow. They keep whatever level X1–X8 gave them, which is what a
    // caller that still has to place a cursor between them wants.
    let retained: Vec<usize> = (0..text.len())
        .filter(|&i| original.get(i).is_some_and(|c| !c.is_removed_by_x9()))
        .collect();

    // X10, then W, N and I per sequence.
    for seq in isolating_run_sequences(&original, &levels, &retained, para) {
        resolve_sequence(text, &original, &mut classes, &mut levels, &seq);
    }

    // L1, over the whole text as a single line — which is what the callers
    // here have, and what the conformance suite measures.
    reset_whitespace(&original, &mut levels, para);

    Paragraph { level: para, levels, original, retained }
}

/// Rules X1 through X8: the embedding and isolate stack.
fn explicit(original: &[Class], classes: &mut [Class], levels: &mut [Level], para: Level) {
    let mut stack: Vec<Status> = vec![Status {
        level: para,
        overridden: None,
        isolate: false,
    }];
    // X1's three counters. `overflow_isolate` outranks `overflow_embedding`:
    // while an isolate has overflowed, an embedding inside it cannot even be
    // counted, because the isolate it belongs to does not exist.
    let mut overflow_isolate = 0u32;
    let mut overflow_embedding = 0u32;
    let mut valid_isolate = 0u32;

    for i in 0..original.len() {
        let Some(&kind) = original.get(i) else { break };
        let Some(&top) = stack.last() else { break };

        match kind {
            // X2–X5: the embeddings and overrides. Each asks for the next
            // level of its own parity above the current one.
            Class::Rle | Class::Lre | Class::Rlo | Class::Lro => {
                // The formatting character itself takes the level in force
                // *before* it, since X9 is about to remove it anyway and a
                // caller keeping it around wants it grouped with what precedes.
                if let Some(l) = levels.get_mut(i) {
                    *l = top.level;
                }
                let rtl = matches!(kind, Class::Rle | Class::Rlo);
                let next = next_level(top.level, rtl);
                let overridden = match kind {
                    Class::Rlo => Some(Class::R),
                    Class::Lro => Some(Class::L),
                    _ => None,
                };
                if next <= MAX_DEPTH && overflow_isolate == 0 && overflow_embedding == 0 {
                    stack.push(Status { level: next, overridden, isolate: false });
                } else if overflow_isolate == 0 {
                    overflow_embedding = overflow_embedding.saturating_add(1);
                }
            }

            // X5a–X5c: the isolates. Unlike an embedding, the initiator is a
            // character in its own right — it gets a level and takes part in
            // the neutral rules — so it is assigned *before* the push.
            Class::Rli | Class::Lri | Class::Fsi => {
                if let Some(l) = levels.get_mut(i) {
                    *l = top.level;
                }
                if let (Some(o), Some(c)) = (top.overridden, classes.get_mut(i)) {
                    *c = o;
                }
                // X5c: an FSI is whichever isolate its own contents call for.
                let rtl = match kind {
                    Class::Rli => true,
                    Class::Lri => false,
                    _ => {
                        let end = matching_pdi(original, i).unwrap_or(original.len());
                        let inner = original.get(i.saturating_add(1)..end).unwrap_or(&[]);
                        first_strong(inner).unwrap_or(0) == 1
                    }
                };
                let next = next_level(top.level, rtl);
                if next <= MAX_DEPTH && overflow_isolate == 0 && overflow_embedding == 0 {
                    valid_isolate = valid_isolate.saturating_add(1);
                    stack.push(Status { level: next, overridden: None, isolate: true });
                } else {
                    overflow_isolate = overflow_isolate.saturating_add(1);
                }
            }

            // X6a: a PDI closes the innermost *valid* isolate, discarding any
            // embeddings opened inside it — an isolate is a hard boundary, so
            // an unbalanced RLE within one cannot leak out past it.
            Class::Pdi => {
                if overflow_isolate > 0 {
                    overflow_isolate = overflow_isolate.saturating_sub(1);
                } else if valid_isolate > 0 {
                    overflow_embedding = 0;
                    while stack.last().is_some_and(|s| !s.isolate) {
                        stack.pop();
                    }
                    stack.pop();
                    valid_isolate = valid_isolate.saturating_sub(1);
                }
                // The PDI takes the level in force *after* the pop, so that it
                // pairs with its initiator rather than with the isolated text.
                let Some(&now) = stack.last() else { break };
                if let Some(l) = levels.get_mut(i) {
                    *l = now.level;
                }
                if let (Some(o), Some(c)) = (now.overridden, classes.get_mut(i)) {
                    *c = o;
                }
            }

            // X7.
            Class::Pdf => {
                if let Some(l) = levels.get_mut(i) {
                    *l = top.level;
                }
                if overflow_isolate > 0 {
                    // Nothing: the embedding this would close was never opened.
                } else if overflow_embedding > 0 {
                    overflow_embedding = overflow_embedding.saturating_sub(1);
                } else if !top.isolate && stack.len() >= 2 {
                    stack.pop();
                }
            }

            // X8: a paragraph separator resets to the paragraph level. It can
            // only be the last character of a paragraph, by P1.
            Class::B => {
                if let Some(l) = levels.get_mut(i) {
                    *l = para;
                }
            }

            // X6: everything else.
            _ => {
                if let Some(l) = levels.get_mut(i) {
                    *l = top.level;
                }
                if let (Some(o), Some(c)) = (top.overridden, classes.get_mut(i)) {
                    *c = o;
                }
            }
        }
    }
}

/// The least level greater than `level` with the requested parity.
const fn next_level(level: Level, rtl: bool) -> Level {
    // Saturating rather than wrapping: at `Level::MAX` the caller's `<=
    // MAX_DEPTH` test rejects the result anyway, and saturating keeps this
    // free of any overflow at all.
    if rtl {
        // The next odd level.
        level.saturating_add(1) | 1
    } else {
        // The next even level.
        level.saturating_add(2) & !1
    }
}

/// One isolating run sequence (BD13), with its boundary types.
struct Sequence {
    /// The character indices in it, in logical order. Never empty.
    indices: Vec<usize>,
    /// The direction of what precedes the sequence, and of what follows it
    /// (X10). Rules W and N treat these as if they were characters just off
    /// each end, which is how a sequence at the start of a paragraph knows
    /// what its neutrals should resolve to.
    sos: Class,
    eos: Class,
}

/// Rule X10: carve the retained characters into isolating run sequences.
///
/// A *level run* is a maximal stretch at one level. A *sequence* is one or
/// more level runs chained across isolate boundaries: if a run ends with an
/// isolate initiator that has a matching `PDI`, the run beginning with that
/// `PDI` continues the same sequence. That is what makes `a <RLI> ... <PDI> b`
/// read as one context for `a` and `b` while the isolated text between them
/// resolves entirely on its own.
fn isolating_run_sequences(
    original: &[Class],
    levels: &[Level],
    retained: &[usize],
    para: Level,
) -> Vec<Sequence> {
    // The level runs, as spans of positions within `retained`.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut at = 0;
    while at < retained.len() {
        let level = retained.get(at).and_then(|&i| levels.get(i)).copied();
        let start = at;
        while at < retained.len()
            && retained.get(at).and_then(|&i| levels.get(i)).copied() == level
        {
            at = at.saturating_add(1);
        }
        runs.push((start, at));
    }

    // Which run starts at which character, so a matched PDI can be followed to
    // the run it begins.
    let run_starting_at = |ch: usize| -> Option<usize> {
        runs.iter()
            .position(|&(s, _)| retained.get(s).copied() == Some(ch))
    };

    let mut out = Vec::new();
    let mut used = vec![false; runs.len()];
    for r in 0..runs.len() {
        if used.get(r).copied().unwrap_or(true) {
            continue;
        }
        // A run whose first character is a PDI matching an isolate initiator
        // is a continuation, not a start; it will be picked up by the sequence
        // that reaches it.
        let first = runs.get(r).and_then(|&(s, _)| retained.get(s)).copied();
        if first.is_some_and(|i| {
            original.get(i) == Some(&Class::Pdi) && matched_initiator(original, i)
        }) {
            continue;
        }

        let mut indices: Vec<usize> = Vec::new();
        let mut current = r;
        loop {
            if let Some(flag) = used.get_mut(current) {
                *flag = true;
            }
            let Some(&(s, e)) = runs.get(current) else { break };
            indices.extend(retained.get(s..e).unwrap_or(&[]).iter().copied());
            // Chain on, if this run ends with a matched isolate initiator.
            let Some(&last) = indices.last() else { break };
            let Some(&kind) = original.get(last) else { break };
            if !kind.is_isolate_initiator() {
                break;
            }
            let Some(pdi) = matching_pdi(original, last) else { break };
            let Some(next) = run_starting_at(pdi) else { break };
            if used.get(next).copied().unwrap_or(true) {
                break;
            }
            current = next;
        }

        if let Some(seq) = boundaries(original, levels, retained, para, indices) {
            out.push(seq);
        }
    }

    // Sequences come out in level-run order, which is logical order of their
    // first character — the order N0's bracket pairs and the conformance
    // suite both expect.
    out
}

/// Does the `PDI` at `at` match some isolate initiator before it? (BD9)
fn matched_initiator(original: &[Class], at: usize) -> bool {
    let mut depth = 1u32;
    let mut i = at;
    while i > 0 {
        i = i.saturating_sub(1);
        let Some(&kind) = original.get(i) else { break };
        if kind == Class::Pdi {
            depth = depth.saturating_add(1);
        } else if kind.is_isolate_initiator() {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                // Only a match if *this* initiator's own forward scan lands
                // back here: an initiator inside an overflowing nest does not
                // pair with a PDI outside it.
                return matching_pdi(original, i) == Some(at);
            }
        }
    }
    false
}

/// The `sos` and `eos` of a sequence (X10), given the characters in it.
fn boundaries(
    original: &[Class],
    levels: &[Level],
    retained: &[usize],
    para: Level,
    indices: Vec<usize>,
) -> Option<Sequence> {
    let &first = indices.first()?;
    let &last = indices.last()?;
    let level = levels.get(first).copied()?;

    // The retained character before the sequence starts, and the one after it
    // ends. Absent means the paragraph boundary, whose level is `para`.
    let before = retained
        .iter()
        .rev()
        .find(|&&i| i < first)
        .and_then(|&i| levels.get(i))
        .copied()
        .unwrap_or(para);
    // An unmatched isolate initiator at the end runs to the end of the
    // paragraph, so what follows it is the paragraph boundary whatever the
    // next character's level happens to be.
    let trailing_isolate = original
        .get(last)
        .is_some_and(|c| c.is_isolate_initiator() && matching_pdi(original, last).is_none());
    let after = if trailing_isolate {
        para
    } else {
        retained
            .iter()
            .find(|&&i| i > last)
            .and_then(|&i| levels.get(i))
            .copied()
            .unwrap_or(para)
    };

    Some(Sequence {
        indices,
        sos: direction(level.max(before)),
        eos: direction(levels.get(last).copied().unwrap_or(level).max(after)),
    })
}

/// The strong type a level stands for: odd is right-to-left.
const fn direction(level: Level) -> Class {
    if level % 2 == 1 { Class::R } else { Class::L }
}

/// Rules W1–W7, N0–N2 and I1–I2 over one isolating run sequence.
fn resolve_sequence(
    text: &[char],
    original: &[Class],
    classes: &mut [Class],
    levels: &mut [Level],
    seq: &Sequence,
) {
    // Working copies, so every rule is a plain pass over a flat slice rather
    // than a walk through an index indirection. The cost is two copies per
    // sequence; the benefit is that each rule reads like the spec.
    let mut cls: Vec<Class> = seq
        .indices
        .iter()
        .map(|&i| classes.get(i).copied().unwrap_or(Class::On))
        .collect();
    let orig: Vec<Class> = seq
        .indices
        .iter()
        .map(|&i| original.get(i).copied().unwrap_or(Class::On))
        .collect();
    let level = seq
        .indices
        .first()
        .and_then(|&i| levels.get(i))
        .copied()
        .unwrap_or(0);

    weak(&mut cls, seq.sos);
    neutral(text, seq, &mut cls, &orig, level);
    implicit(&seq.indices, &cls, levels, level);
}

/// Rules W1 through W7: the weak types.
fn weak(cls: &mut [Class], sos: Class) {
    // W1: a non-spacing mark takes the type of what it sits on. After an
    // isolate initiator or a PDI it becomes ON instead, because what it would
    // otherwise copy is a boundary rather than a character.
    let mut prev = sos;
    for c in cls.iter_mut() {
        if *c == Class::Nsm {
            *c = if prev.is_isolate_initiator() || prev == Class::Pdi {
                Class::On
            } else {
                prev
            };
        }
        prev = *c;
    }

    // W2: a European number after an Arabic letter is an Arabic number. The
    // digits in an Arabic sentence are read right to left with it even when
    // they are written with Latin digits.
    let mut strong = sos;
    for c in cls.iter_mut() {
        if c.is_strong() {
            strong = *c;
        } else if *c == Class::En && strong == Class::Al {
            *c = Class::An;
        }
    }

    // W3: with W2 done, the distinction between an Arabic letter and any other
    // right-to-left one has served its purpose.
    for c in cls.iter_mut() {
        if *c == Class::Al {
            *c = Class::R;
        }
    }

    // W4: a single separator between two numbers of the same kind joins them —
    // the `.` in `1.5`, the `,` in `1,234`.
    for i in 1..cls.len().saturating_sub(1) {
        let (Some(prev), Some(here), Some(next)) = (
            cls.get(i.saturating_sub(1)).copied(),
            cls.get(i).copied(),
            cls.get(i.saturating_add(1)).copied(),
        ) else {
            continue;
        };
        let joined = match here {
            Class::Es if prev == Class::En && next == Class::En => Some(Class::En),
            Class::Cs if prev == Class::En && next == Class::En => Some(Class::En),
            Class::Cs if prev == Class::An && next == Class::An => Some(Class::An),
            _ => None,
        };
        if let (Some(j), Some(slot)) = (joined, cls.get_mut(i)) {
            *slot = j;
        }
    }

    // W5: a run of European terminators touching a European number joins it —
    // the `%` in `50%`, the `$` in `$50`.
    let mut i = 0;
    while i < cls.len() {
        if cls.get(i) != Some(&Class::Et) {
            i = i.saturating_add(1);
            continue;
        }
        let start = i;
        while cls.get(i) == Some(&Class::Et) {
            i = i.saturating_add(1);
        }
        let touches = start.checked_sub(1).and_then(|p| cls.get(p)) == Some(&Class::En)
            || cls.get(i) == Some(&Class::En);
        if touches {
            for c in cls.get_mut(start..i).unwrap_or(&mut []) {
                *c = Class::En;
            }
        }
    }

    // W6: whatever separators and terminators are left were not part of a
    // number, so they are ordinary punctuation.
    for c in cls.iter_mut() {
        if matches!(*c, Class::Et | Class::Es | Class::Cs) {
            *c = Class::On;
        }
    }

    // W7: a European number in left-to-right context is simply left-to-right.
    let mut strong = sos;
    for c in cls.iter_mut() {
        if matches!(*c, Class::L | Class::R) {
            strong = *c;
        } else if *c == Class::En && strong == Class::L {
            *c = Class::L;
        }
    }
}

/// Rules N0, N1 and N2: the neutrals.
fn neutral(text: &[char], seq: &Sequence, cls: &mut [Class], orig: &[Class], level: Level) {
    let e = direction(level);

    // N0: bracket pairs, before the general neutral rules, so that a bracket
    // takes the direction of what it encloses rather than of what surrounds it.
    brackets(text, seq, cls, orig, e);

    // N1: a stretch of neutrals between two strong types of the same direction
    // takes that direction. Numbers count as right-to-left here: `1` in Hebrew
    // text is surrounded by right-to-left context even though the digits
    // themselves read left to right.
    let strength = |c: Class| match c {
        Class::L => Some(Class::L),
        Class::R | Class::En | Class::An => Some(Class::R),
        _ => None,
    };
    let mut i = 0;
    while i < cls.len() {
        if !cls.get(i).copied().is_some_and(Class::is_neutral_or_isolate) {
            i = i.saturating_add(1);
            continue;
        }
        let start = i;
        while cls
            .get(i)
            .copied()
            .is_some_and(Class::is_neutral_or_isolate)
        {
            i = i.saturating_add(1);
        }
        let before = start
            .checked_sub(1)
            .and_then(|p| cls.get(p))
            .copied()
            .and_then(strength)
            .unwrap_or(seq.sos);
        let after = cls
            .get(i)
            .copied()
            .and_then(strength)
            .unwrap_or(seq.eos);
        // N2 is the else branch: neutrals with strong types of *different*
        // directions on either side take the embedding direction.
        let to = if before == after { before } else { e };
        for c in cls.get_mut(start..i).unwrap_or(&mut []) {
            *c = to;
        }
    }
}

/// Rule N0 and BD16: bracket pairs.
fn brackets(text: &[char], seq: &Sequence, cls: &mut [Class], orig: &[Class], e: Class) {
    let pairs = bracket_pairs(text, seq, cls);
    let o = if e == Class::L { Class::R } else { Class::L };

    for (open, close) in pairs {
        // The strong types enclosed by the pair. Numbers count as
        // right-to-left, as everywhere in the neutral rules.
        let mut found_e = false;
        let mut found_o = false;
        for c in cls.get(open.saturating_add(1)..close).unwrap_or(&[]) {
            let s = match *c {
                Class::L => Class::L,
                Class::R | Class::En | Class::An => Class::R,
                _ => continue,
            };
            if s == e {
                found_e = true;
            } else {
                found_o = true;
            }
        }

        let to = if found_e {
            // N0 b: the embedding direction is inside, so the brackets take it.
            Some(e)
        } else if found_o {
            // N0 c: only the opposite direction is inside. Whether the
            // brackets follow it depends on the context *before* them — `he
            // said "<HE> (foo)"` versus a bracketed aside in a run that was
            // already going the other way.
            let mut context = seq.sos;
            for c in cls.get(..open).unwrap_or(&[]).iter().rev() {
                match *c {
                    Class::L => {
                        context = Class::L;
                        break;
                    }
                    Class::R | Class::En | Class::An => {
                        context = Class::R;
                        break;
                    }
                    _ => {}
                }
            }
            Some(if context == o { o } else { e })
        } else {
            // N0 d: nothing strong inside, so the pair is left to N1 and N2.
            None
        };

        let Some(to) = to else { continue };
        for at in [open, close] {
            if let Some(c) = cls.get_mut(at) {
                *c = to;
            }
            // "Any number of characters that had original bidirectional
            // character type NSM prior to the application of W1 that
            // immediately follow a paired bracket which changed to L or R
            // under N0 should change to match the type of the paired bracket."
            let mut j = at.saturating_add(1);
            while orig.get(j) == Some(&Class::Nsm) {
                if let Some(c) = cls.get_mut(j) {
                    *c = to;
                }
                j = j.saturating_add(1);
            }
        }
    }
}

/// BD16: the bracket pairs in a sequence, sorted by opening position.
fn bracket_pairs(text: &[char], seq: &Sequence, cls: &[Class]) -> Vec<(usize, usize)> {
    // `(what would close this, where it opened)`.
    let mut stack: Vec<(u32, usize)> = Vec::new();
    let mut pairs: Vec<(usize, usize)> = Vec::new();

    for (at, &ch_at) in seq.indices.iter().enumerate() {
        // Only a character still neutral after the weak rules can be a
        // bracket: an override has turned the others into plain strong types.
        if cls.get(at) != Some(&Class::On) {
            continue;
        }
        let Some(&ch) = text.get(ch_at) else { continue };
        let Some((pair, opening)) = bracket(ch) else {
            continue;
        };
        if opening {
            if stack.len() >= MAX_BRACKET_PAIRS {
                // BD16 stops entirely rather than dropping one bracket, so
                // that the pairs it did find are not paired across the gap.
                break;
            }
            stack.push((pair, at));
        } else {
            let want = canonical_bracket(ch);
            if let Some(found) = stack.iter().rposition(|&(p, _)| p == want) {
                if let Some(&(_, open)) = stack.get(found) {
                    pairs.push((open, at));
                }
                stack.truncate(found);
            }
        }
    }

    pairs.sort_unstable();
    pairs
}

/// Rules I1 and I2: resolved types become level increments.
fn implicit(indices: &[usize], cls: &[Class], levels: &mut [Level], level: Level) {
    let even = level.is_multiple_of(2);
    for (at, &kind) in cls.iter().enumerate() {
        let bump = if even {
            // I1: in a left-to-right context, right-to-left text goes one
            // level deeper and numbers two — numbers read left to right but
            // are *placed* as a right-to-left unit.
            match kind {
                Class::R => 1,
                Class::An | Class::En => 2,
                _ => 0,
            }
        } else {
            // I2.
            match kind {
                Class::L | Class::En | Class::An => 1,
                _ => 0,
            }
        };
        if let (Some(&i), true) = (indices.get(at), bump > 0)
            && let Some(l) = levels.get_mut(i)
        {
            *l = l.saturating_add(bump);
        }
    }
}

/// Rule L1: reset separators and trailing whitespace to the paragraph level.
///
/// The rule reads the *original* classes, not the resolved ones: the trailing
/// space at the end of a right-to-left line is still a space even after N2
/// gave it a direction, and it belongs at the paragraph's own level so the
/// cursor sits where the reader expects.
fn reset_whitespace(original: &[Class], levels: &mut [Level], para: Level) {
    // Walking backwards makes "a run of whitespace before a separator or the
    // end of the line" a single condition: reset while the characters are
    // resettable, and re-arm at every separator.
    let mut resetting = true;
    for i in (0..levels.len()).rev() {
        let Some(&kind) = original.get(i) else { continue };
        match kind {
            Class::B | Class::S => {
                if let Some(l) = levels.get_mut(i) {
                    *l = para;
                }
                resetting = true;
            }
            Class::Ws | Class::Lri | Class::Rli | Class::Fsi | Class::Pdi => {
                if resetting && let Some(l) = levels.get_mut(i) {
                    *l = para;
                }
            }
            // X9 removed these, so they are transparent to L1: a space
            // followed by an LRE followed by the end of the line is still
            // trailing whitespace.
            c if c.is_removed_by_x9() => {
                if resetting && let Some(l) = levels.get_mut(i) {
                    *l = para;
                }
            }
            _ => resetting = false,
        }
    }
}

impl Paragraph {
    /// The original `Bidi_Class` of each character, in logical order.
    ///
    /// Exposed for the conformance harness and for a caller that wants to know
    /// whether a character was whitespace without consulting the table again.
    #[must_use]
    pub fn classes(&self) -> &[Class] {
        &self.original
    }
}

/// What `tests/bidi.rs` cannot reach.
///
/// The conformance suite is the real test of the algorithm — ninety thousand
/// cases, every rule, no judgement required — so nothing here re-tests a rule
/// it already covers. What it does *not* cover is everything around the
/// algorithm: the property lookups, the `Base` the caller asks for as
/// distinct from the level the suite expects, and the questions
/// (`is_uniform`, `is_rtl`) a caller asks to decide whether to do any of this
/// work at all. Those are this module's subject.
#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{Base, Class, Level, class, mirror, resolve};

    /// The Hebrew word "שלום", which is right-to-left and strong.
    const HEBREW: [char; 4] = ['\u{5E9}', '\u{5DC}', '\u{5D5}', '\u{5DD}'];

    fn levels(text: &[char], base: Base) -> Vec<Level> {
        resolve(text, base).levels().to_vec()
    }

    #[test]
    fn the_class_table_answers_at_its_own_boundaries() {
        // Absent from the table is `L`, which is what makes the table small.
        assert_eq!(class('A'), Class::L);
        assert_eq!(class('\u{0}'), Class::Bn);
        // The first and last code points of a range, and the ones either side.
        assert_eq!(class('\u{590}'), Class::R, "unassigned, but in a R block");
        assert_eq!(class('\u{5D0}'), Class::R, "HEBREW LETTER ALEF");
        assert_eq!(class('\u{627}'), Class::Al, "ARABIC LETTER ALEF");
        assert_eq!(class('\u{660}'), Class::An, "ARABIC-INDIC DIGIT ZERO");
        assert_eq!(class('0'), Class::En);
        assert_eq!(class('+'), Class::Es);
        assert_eq!(class('$'), Class::Et);
        assert_eq!(class(','), Class::Cs);
        assert_eq!(class('\u{300}'), Class::Nsm);
        assert_eq!(class('\n'), Class::B);
        assert_eq!(class('\t'), Class::S);
        assert_eq!(class(' '), Class::Ws);
        assert_eq!(class('('), Class::On);
        assert_eq!(class('\u{202B}'), Class::Rle);
        assert_eq!(class('\u{2069}'), Class::Pdi);
        // The very top of the code space: a noncharacter, which the UCD gives
        // BN, and the private-use code point below it, which defaults to L.
        assert_eq!(class('\u{10FFFF}'), Class::Bn);
        assert_eq!(class('\u{10FFFD}'), Class::L);
    }

    #[test]
    fn mirroring_is_a_pairing_and_only_of_brackets() {
        assert_eq!(mirror('('), Some(')'));
        assert_eq!(mirror(')'), Some('('));
        assert_eq!(mirror('['), Some(']'));
        assert_eq!(mirror('<'), Some('>'));
        assert_eq!(mirror('\u{2329}'), Some('\u{232A}'));
        // A letter, a digit and a full stop read the same either way round.
        for ch in ['A', '\u{5D0}', '0', '.', ' '] {
            assert_eq!(mirror(ch), None, "{ch:?} should not mirror");
        }
        // Every mirroring is its own inverse, which is what lets rule L4 be a
        // single lookup rather than a direction-dependent pair of them.
        for &(cp, _) in &super::MIRRORED {
            let Some(ch) = char::from_u32(cp) else { continue };
            let m = mirror(ch).expect("a table entry must mirror");
            assert_eq!(mirror(m), Some(ch), "{ch:?} -> {m:?} does not come back");
        }
    }

    #[test]
    fn a_forced_base_overrules_the_text_it_is_given() {
        // Rule P2 would make this paragraph right-to-left; `Base::Ltr` says
        // otherwise, which is what a left-to-right UI wants for a label.
        assert!(resolve(&HEBREW, Base::Auto).is_rtl());
        assert_eq!(resolve(&HEBREW, Base::Ltr).level(), 0);
        assert_eq!(resolve(&HEBREW, Base::Rtl).level(), 1);

        let english: Vec<char> = "hello".chars().collect();
        assert!(!resolve(&english, Base::Auto).is_rtl());
        assert_eq!(resolve(&english, Base::Rtl).level(), 1);

        // Forcing the base does not force the *characters*: Hebrew is still
        // odd-levelled inside a left-to-right paragraph.
        assert_eq!(levels(&HEBREW, Base::Ltr), vec![1; 4]);
    }

    #[test]
    fn a_paragraph_with_no_strong_character_takes_the_default() {
        // P2 finds nothing strong, so P3 says level 0 — and `Base::Rtl` still
        // overrides that.
        let digits: Vec<char> = "123 .,".chars().collect();
        assert_eq!(resolve(&digits, Base::Auto).level(), 0);
        assert_eq!(resolve(&digits, Base::Rtl).level(), 1);
        assert_eq!(resolve(&[], Base::Auto).level(), 0);
        assert!(resolve(&[], Base::Auto).reorder().is_empty());
    }

    #[test]
    fn uniform_is_the_question_a_caller_asks_to_skip_the_work() {
        let english: Vec<char> = "hello, world".chars().collect();
        let para = resolve(&english, Base::Auto);
        assert!(para.is_uniform(), "plain English needs no reordering");
        assert_eq!(para.reorder(), (0..english.len()).collect::<Vec<_>>());

        // Hebrew alone is uniform too — every character sits at the
        // paragraph's own level 1 — but it still reorders.
        let para = resolve(&HEBREW, Base::Auto);
        assert!(para.is_uniform());
        assert_eq!(para.reorder(), vec![3, 2, 1, 0]);

        // A mixture is not.
        let mixed: Vec<char> = "a\u{5D0}".chars().collect();
        assert!(!resolve(&mixed, Base::Auto).is_uniform());
    }

    #[test]
    fn an_english_phrase_inside_hebrew_comes_back_the_right_way_round() {
        // The nesting L2 produces by reversing twice: the Hebrew reverses, and
        // the English inside it reverses back.
        let mut text: Vec<char> = HEBREW.to_vec();
        text.push(' ');
        text.extend("abc".chars());
        let para = resolve(&text, Base::Auto);
        assert!(para.is_rtl());
        assert_eq!(para.levels(), [1, 1, 1, 1, 1, 2, 2, 2]);
        // Drawn left to right: "abc", the space, then the Hebrew reversed.
        assert_eq!(para.reorder(), vec![5, 6, 7, 4, 3, 2, 1, 0]);
    }

    #[test]
    fn the_characters_rule_x9_removes_are_not_in_the_visual_order() {
        // U+202B RLE ... U+202C PDF around English inside an LTR paragraph.
        let mut text: Vec<char> = "a".chars().collect();
        text.push('\u{202B}');
        text.extend(HEBREW);
        text.push('\u{202C}');
        text.push('b');
        let para = resolve(&text, Base::Auto);
        let order = para.reorder();
        assert_eq!(order.len(), text.len().saturating_sub(2));
        assert!(!order.contains(&1), "the RLE has no glyph to draw");
        assert!(!order.contains(&6), "the PDF has no glyph to draw");
        assert_eq!(order, vec![0, 5, 4, 3, 2, 7]);
    }

    #[test]
    fn classes_are_the_ones_that_arrived_not_the_resolved_ones() {
        // W2 turns this EN into an AN, and L1 resets the trailing space — but
        // `classes` reports what was typed, which is what L1 itself needs.
        let text: Vec<char> = "\u{627}1 ".chars().collect();
        let para = resolve(&text, Base::Auto);
        assert_eq!(para.classes(), [Class::Al, Class::En, Class::Ws]);
        assert_eq!(
            para.levels().last(),
            Some(&para.level()),
            "L1 puts the trailing space back at the paragraph level"
        );
    }
}
