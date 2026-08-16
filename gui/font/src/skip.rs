//! Which glyphs a lookup is allowed to see.
//!
//! Every OpenType lookup carries a `lookupFlag`, and the flag's whole purpose
//! is to make a lookup *not* see certain glyphs. The one that matters in
//! practice is `IgnoreMarks`. A face that writes an Arabic ligature says "these
//! two letters", not "these two letters and whatever vowel marks happen to
//! stand between them" — the marks are supposed to be invisible to the match.
//! A shaper that walks the run with `i + 1` will look at the fatha, find it is
//! not the second letter, and silently decline to form a ligature the face
//! plainly asked for. The text still renders; it just renders wrong, on
//! exactly the vowelled Arabic where the face was trying hardest.
//!
//! # Why one iterator rather than a check in each matcher
//!
//! "Skip this glyph" has to be honoured by *every* matcher — the single
//! substitution's position, the ligature's component walk, and the backtrack,
//! input and lookahead walks of the two contextual types. Honouring it in some
//! of them and not others is worse than honouring it nowhere: the output stops
//! being explicable, because whether a rule fired depends on which of six
//! code paths happened to reach it. So the flag is not consulted by the
//! matchers at all. They step through a [`Skipper`], and stepping is the only
//! way any of them moves through the run.
//!
//! This is the same shape as HarfBuzz's `skipping_iterator_t`, and for the
//! same reason.
//!
//! # Skipping is not the same as not matching
//!
//! The iterator answers two different questions and must not conflate them.
//!
//! * A glyph the *flag* excludes is **skipped**: the walk steps over it and
//!   carries on, and the match can still succeed. That is what lets a ligature
//!   form across a mark.
//! * A glyph that is not eligible for the lookup's *features* — the per-glyph
//!   feature mask of `gsub` — is **not a match**: the walk stops and the rule
//!   fails. Skipping there would let a `fina` ligature form across a letter
//!   that is not in final form, by pretending the letter is not there.
//!
//! [`Skipper::next`] and [`Skipper::prev`] therefore return `None` for both
//! "ran off the run" and "found an ineligible glyph", and never step over the
//! latter.
//!
//! # And there is a third answer, for the characters that are never drawn
//!
//! A soft hyphen, a joiner, a variation selector — the [`Ignorable`] set — is
//! neither of the two above. It is **stepped over unless it is exactly the
//! glyph the rule wanted at that spot**. HarfBuzz calls this `SKIP_MAYBE` and
//! the distinction is not decoration:
//!
//! * `f ZWJ i` has to reach the `fi` ligature, because a joiner is a request
//!   for a ligature and not an obstacle to one. So the ligature's component
//!   walk steps over the joiner.
//! * A face that ships a rule naming U+200D — several Indic and emoji faces
//!   do — has to be able to match it. So the same walk, offered a rule whose
//!   next component *is* the joiner, matches it instead of stepping.
//!
//! One rule covers both: try the criterion first, step over only if it says
//! no. That is why the walks take the caller's criterion rather than handing
//! back a position for the caller to test — a caller that tested afterwards
//! could not tell the walk to carry on.
//!
//! Which characters are steppable depends on where the lookup is; see
//! [`Joiners`].
//!
//! # The mask applies to the input only
//!
//! A chaining context matches three sequences: backtrack, input, lookahead.
//! The feature mask gates the input, because the input is what the rule is
//! *about* — it is the span the nested lookups will rewrite. Backtrack and
//! lookahead are context: they describe the neighbourhood the rule needs, and
//! a neighbour is a neighbour whether or not it happens to be eligible for the
//! feature that reached this lookup. [`Skipper::context`] is the same skipper
//! with the mask opened to everything, and it is what the two outer walks use.
//! HarfBuzz splits its iterators the same way (`iter_input` against
//! `iter_context`) and this is not a coincidence — getting it wrong makes a
//! chaining `fina` rule fail whenever its lookahead is a medial letter, which
//! is most of the time.
//!
//! # Where the flag comes from on the positioning side
//!
//! `GPOS` goes through this module too, but not always with the lookup's own
//! flag. Types 1, 2 and 3 use it as written; the two mark types deliberately
//! substitute one of their own, because "which glyph does this mark hang off"
//! is not a question the font gets to answer with a flag. See
//! [`Positioning::one`](crate::gpos::Positioning) for which and why.
//!
//! `RightToLeft` (bit 0) is read by cursive attachment alone, which is the
//! only thing it means anything to: it selects which of the two joined glyphs
//! moves. It is not a general statement about the run's direction and must not
//! be treated as one.

use crate::gsub::SubGlyph;
use crate::norm::Ignorable;
use crate::otl::{coverage_index, glyph_class};
use crate::sfnt::Span;

/// `lookupFlag` bit 1: do not see glyphs `GDEF` calls simple bases.
const IGNORE_BASE_GLYPHS: u16 = 0x0002;
/// `lookupFlag` bit 2: do not see glyphs `GDEF` calls ligatures.
const IGNORE_LIGATURES: u16 = 0x0004;
/// `lookupFlag` bit 3: do not see glyphs `GDEF` calls marks.
///
/// Public because `GPOS` mark-to-base attachment searches for its base with
/// this flag *instead of* the lookup's own: a mark-to-base lookup that did not
/// ignore marks would attach the second accent of a stack to the first and call
/// it a base.
pub(crate) const IGNORE_MARKS: u16 = 0x0008;
/// The three bits that hide a whole `GDEF` class from a lookup.
///
/// Public for the same reason: mark-to-*mark* attachment keeps the lookup's
/// flag except for these, so that a mark-attachment class or a filtering set
/// still selects which marks may stack while nothing is stepped over.
pub(crate) const IGNORE_FLAGS: u16 = IGNORE_BASE_GLYPHS | IGNORE_LIGATURES | IGNORE_MARKS;
/// `lookupFlag` bit 4: see only the marks in the named `MarkGlyphSet`.
const USE_MARK_FILTERING_SET: u16 = 0x0010;
/// `lookupFlag` high byte: see only the marks of this attachment class.
const MARK_ATTACHMENT_TYPE: u16 = 0xFF00;

/// `GDEF` `GlyphClassDef` class 1: a base glyph, standing on its own.
pub(crate) const CLASS_BASE: u16 = 1;
/// `GDEF` `GlyphClassDef` class 2: a glyph that is itself a ligature.
const CLASS_LIGATURE: u16 = 2;
/// `GDEF` `GlyphClassDef` class 3: a combining mark.
pub(crate) const CLASS_MARK: u16 = 3;

/// The three `GDEF` tables a lookup flag can consult, found once per face.
///
/// Held as offsets rather than decoded maps because a flag that excludes
/// nothing — which is the overwhelming majority — never reads them at all, and
/// decoding a `ClassDef` for a face whose lookups all have flag zero would be
/// pure waste.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Definitions {
    /// `GlyphClassDef`: base, ligature or mark.
    classes: Option<usize>,
    /// `MarkAttachClassDef`: which attachment class a mark belongs to. A
    /// separate classification from `GlyphClassDef` and not derivable from it.
    attach: Option<usize>,
    /// `MarkGlyphSetsDef`: an array of coverage tables, indexed by the
    /// `markFilteringSet` field of the lookup. Present only from `GDEF` 1.2.
    sets: Option<usize>,
}

impl Definitions {
    /// Read the class-definition offsets out of a face's `GDEF`.
    ///
    /// Every field is optional and a missing one is not an error — a face with
    /// no `GDEF` at all is normal, and so is one that defines glyph classes but
    /// no mark sets. The version gates only `MarkGlyphSetsDef`: the earlier
    /// fields sit at fixed places in every version, so they need no
    /// interpretation.
    #[must_use]
    pub(crate) fn parse(data: &[u8], gdef: Option<Span>) -> Self {
        let Some(gdef) = gdef else {
            return Self::default();
        };
        let base = gdef.off;
        let at = |field: usize| -> Option<usize> {
            let off = base.checked_add(field).and_then(|o| u16_at(data, o))?;
            // Zero means absent everywhere in OpenType; following it would land
            // on the table header and read a version as a class format.
            (off != 0).then_some(())?;
            base.checked_add(usize::from(off))
        };
        // GDEF 1.2 added MarkGlyphSetsDef at offset 12. Reading it from a 1.0
        // table reads whatever follows the header, which is data.
        let minor = base.checked_add(2).and_then(|o| u16_at(data, o));
        Self {
            classes: at(4),
            attach: at(10),
            sets: minor.filter(|&m| m >= 2).and_then(|_| at(12)),
        }
    }

    /// What `GDEF` calls this glyph — base, ligature, mark — or `0` for a face
    /// that classifies nothing, which is the answer for a face with no `GDEF`
    /// and for a glyph the table simply does not mention.
    ///
    /// Public because `GPOS` mark-to-mark attachment has to check that what it
    /// found below the mark really is a mark, which is a question about the
    /// class rather than about any lookup's flag.
    #[must_use]
    pub(crate) fn class(self, data: &[u8], glyph: u16) -> u16 {
        self.classes
            .and_then(|table| glyph_class(data, table, glyph))
            .unwrap_or(0)
    }
}

/// Which of the never-drawn characters a lookup may step over.
///
/// Three of the five [`Ignorable`] kinds are conditional, and the conditions
/// are a transcription of HarfBuzz's `hb_ot_apply_context_t::matcher_t::init`:
///
/// | | `GSUB`, input | `GSUB`, context | `GPOS` |
/// |---|---|---|---|
/// | ZWNJ | only if `auto_zwnj` is off — never | only if `auto_zwnj` | always |
/// | ZWJ | if `auto_zwj` | always | always |
/// | hidden (`CGJ`, Mongolian FVS, tags) | never | never | always |
///
/// The asymmetry between the two joiners in `GSUB` input is the whole point of
/// having them: ZWJ asks for a ligature, so a walk looking for one steps over
/// it; ZWNJ forbids one, so the same walk must stop dead on it. Reading the
/// table the other way round makes `f ZWNJ i` ligate, which is precisely what
/// the writer typed the character to prevent.
///
/// `auto_zwnj` and `auto_zwj` are off for the features whose *own subject* is
/// the joiners — the Indic-family basic features, where a joiner selects
/// between a conjunct and a half-form rather than decorating a ligature. A
/// lookup reached by both a manual feature and an automatic one is manual, as
/// in HarfBuzz, where merging two entries for one lookup `&&`s the two flags.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Joiners {
    /// Positioning rather than substitution. Positioning steps over
    /// everything: nothing attaches to a character that is not drawn.
    positioning: bool,
    /// Whether ZWNJ is the font's business rather than the shaper's.
    auto_zwnj: bool,
    /// Whether ZWJ is the font's business rather than the shaper's.
    auto_zwj: bool,
}

impl Joiners {
    /// `GPOS`, and the legacy `kern` table, which HarfBuzz walks with a `GPOS`
    /// matcher: every never-drawn character is stepped over.
    pub(crate) const POSITIONING: Self = Self {
        positioning: true,
        auto_zwnj: true,
        auto_zwj: true,
    };

    /// `GSUB`, with the joiners left to the font or read by the feature.
    ///
    /// There is no `SUBSTITUTION` constant beside `POSITIONING` on purpose:
    /// every substitution lookup has to answer the `manual` question, and a
    /// constant that answered it silently is one a caller would reach for
    /// without knowing it had been asked.
    ///
    /// `manual` is the answer for the whole set of features that reached the
    /// lookup, not for one of them: see the type's doc.
    #[must_use]
    pub(crate) const fn substitution(manual: bool) -> Self {
        Self {
            positioning: false,
            auto_zwnj: !manual,
            auto_zwj: !manual,
        }
    }
}

/// One lookup's view of the run: which positions it may look at, and which of
/// those its features make it eligible to act on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Skipper<'a> {
    data: &'a [u8],
    defs: Definitions,
    /// The lookup's `lookupFlag`.
    flag: u16,
    /// The `markFilteringSet` index, read only when the flag asks for it.
    filter: u16,
    /// The set of feature bits that reached this lookup. A glyph whose own
    /// mask does not intersect it is not a match.
    mask: u64,
    /// Which never-drawn characters this lookup may step over.
    joiners: Joiners,
    /// Whether this is the context half of a chaining rule, which is one of
    /// the two things [`Skipper::context`] changes — the other being the mask.
    context: bool,
}

/// The placeholder criterion for a walk that has none: the caller wants the
/// next position the lookup would consider, whatever glyph turns out to be
/// there.
///
/// Spelled out as a constant because "no closure" still needs a type, and
/// `impl Trait` in argument position cannot be turbofished at a call site that
/// passes `None`.
const ANY: Option<fn(usize) -> bool> = None;

impl<'a> Skipper<'a> {
    /// The skipper for a lookup's *input*: flag-aware and feature-gated.
    #[must_use]
    pub(crate) fn new(
        data: &'a [u8],
        defs: Definitions,
        flag: u16,
        filter: u16,
        mask: u64,
        joiners: Joiners,
    ) -> Self {
        Self {
            data,
            defs,
            flag,
            filter,
            mask,
            joiners,
            context: false,
        }
    }

    /// The same skipper for a lookup's *context* — backtrack and lookahead —
    /// which skips what the flag skips but is not gated by the features.
    ///
    /// See the module doc: a neighbour is a neighbour regardless of which
    /// feature reached the rule that is asking about it. And a joiner is not a
    /// neighbour at all: matching context, both joiners are stepped over
    /// (ZWNJ only where the feature left them to the font), because a rule
    /// that says "when a `t` follows" is describing the letters around the
    /// input and not the invisible characters between them.
    #[must_use]
    pub(crate) fn context(self) -> Self {
        Self {
            mask: u64::MAX,
            context: true,
            ..self
        }
    }

    /// Does this lookup have anything to skip at all?
    ///
    /// The answer is no for almost every lookup in almost every face, and it
    /// is worth asking because a `false` lets a caller keep a plain `i + 1`
    /// walk instead of stepping.
    #[must_use]
    pub(crate) fn is_trivial(self) -> bool {
        self.flag
            & (IGNORE_BASE_GLYPHS
                | IGNORE_LIGATURES
                | IGNORE_MARKS
                | USE_MARK_FILTERING_SET
                | MARK_ATTACHMENT_TYPE)
            == 0
    }

    /// Is this glyph one the flag hides from the lookup?
    ///
    /// A face with no `GDEF` classifies nothing, so `glyph_class` answers
    /// `None` and nothing is skipped — which is the right answer: a flag that
    /// says "ignore marks" over a face that never said which glyphs are marks
    /// has named an empty set.
    ///
    /// Public because `GPOS` asks the question about a bare glyph id rather
    /// than about a position in a run: kerning is consulted a pair at a time,
    /// and what it needs to know is whether the glyphs standing between the
    /// pair are ones this lookup would have stepped over.
    #[must_use]
    pub(crate) fn skips(self, glyph: u16) -> bool {
        if self.is_trivial() {
            return false;
        }
        match self.defs.class(self.data, glyph) {
            CLASS_BASE => self.flag & IGNORE_BASE_GLYPHS != 0,
            CLASS_LIGATURE => self.flag & IGNORE_LIGATURES != 0,
            CLASS_MARK => self.skips_mark(glyph),
            _ => false,
        }
    }

    /// Is this *mark* hidden — by the blanket bit, by a filtering set, or by
    /// an attachment class?
    ///
    /// The three are mutually exclusive in the order the spec gives them: a
    /// filtering set, when present, replaces the attachment-class test rather
    /// than adding to it.
    #[must_use]
    fn skips_mark(self, glyph: u16) -> bool {
        if self.flag & IGNORE_MARKS != 0 {
            return true;
        }
        if self.flag & USE_MARK_FILTERING_SET != 0 {
            // A named set the face did not supply excludes everything, which
            // is the conservative reading: the lookup asked to see only the
            // marks in that set, and there are none.
            return !self.in_filter_set(glyph);
        }
        let want = self.flag & MARK_ATTACHMENT_TYPE;
        if want == 0 {
            return false;
        }
        let Some(table) = self.defs.attach else {
            return false;
        };
        let got = glyph_class(self.data, table, glyph).unwrap_or(0);
        // The class is stored in the high byte of the flag and as a plain
        // class number in the table, so one of them has to be shifted.
        got.checked_shl(8).unwrap_or(0) != want
    }

    /// Is `glyph` in the `MarkGlyphSet` this lookup named?
    #[must_use]
    fn in_filter_set(self, glyph: u16) -> bool {
        let Some(sets) = self.defs.sets else {
            return false;
        };
        let found = (|| {
            // MarkGlyphSetsDef: format, count, then one 32-bit coverage offset
            // per set — 32-bit because a face with many sets can push the last
            // of them past 64 KiB.
            let count = u16_at(self.data, sets.checked_add(2)?)?;
            if self.filter >= count {
                return None;
            }
            let at = sets
                .checked_add(4)?
                .checked_add(usize::from(self.filter).checked_mul(4)?)?;
            let off = usize::try_from(u32_at(self.data, at)?).ok()?;
            let coverage = sets.checked_add(off)?;
            coverage_index(self.data, coverage, glyph)
        })();
        found.is_some()
    }

    /// May a walk step over this glyph on its way past, if the rule turns out
    /// not to want it here?
    ///
    /// HarfBuzz's `matcher_t::may_skip` returning `SKIP_MAYBE`, and the
    /// conditions are [`Joiners`]'s table. Note what the question is *not*: it
    /// is not "is this glyph drawn", which is [`Ignorable::erased`] and is
    /// settled later, and it is not "does the flag hide it", which is
    /// [`Skipper::skips`] and is unconditional.
    #[must_use]
    fn steps_over(self, glyph: &SubGlyph) -> bool {
        let j = self.joiners;
        match glyph.ignorable {
            Ignorable::No => false,
            Ignorable::Plain => true,
            Ignorable::Zwnj => j.positioning || (self.context && j.auto_zwnj),
            Ignorable::Zwj => self.context || j.auto_zwj,
            Ignorable::Hidden => j.positioning,
        }
    }

    /// The first position from `from` — forward, or backward when `back` — at
    /// which this lookup matches.
    ///
    /// The one walk every other one is built from, and a transcription of
    /// HarfBuzz's `skipping_iterator_t::match`. Three outcomes per position:
    ///
    /// * the flag hides it — step over it, unconditionally;
    /// * `wants` says yes, or there is no `wants` and the glyph is not one a
    ///   criterion-less walk steps past — this is the answer;
    /// * otherwise it is a match failure, which ends the walk unless the glyph
    ///   is one the lookup may step over.
    ///
    /// The last line is the whole `SKIP_MAYBE` rule. `wants` is consulted
    /// *before* steppability, not after, which is what lets a rule that names
    /// U+200D match one instead of walking past it.
    fn scan<F: FnMut(usize) -> bool>(
        self,
        glyphs: &[SubGlyph],
        from: usize,
        back: bool,
        mut wants: Option<F>,
    ) -> Option<usize> {
        let mut i = from;
        loop {
            let glyph = glyphs.get(i)?;
            if !self.skips(glyph.gid) {
                let eligible = glyph.mask & self.mask != 0;
                let over = self.steps_over(glyph);
                let matched = match wants {
                    Some(ref mut f) => eligible && f(i),
                    // With no criterion, HarfBuzz's `may_match` answers
                    // `MATCH_MAYBE`, which pairs with `SKIP_MAYBE` to give a
                    // skip: a caller asking only for "the next glyph you would
                    // consider" does not want a soft hyphen.
                    None => eligible && !over,
                };
                if matched {
                    return Some(i);
                }
                if !over {
                    return None;
                }
            }
            i = if back {
                i.checked_sub(1)?
            } else {
                i.checked_add(1)?
            };
        }
    }

    /// Is this exact position one the lookup may act on — neither hidden by
    /// the flag nor ineligible for the features?
    ///
    /// Distinct from the walks in that it does not scan, and in that it does
    /// not ask about the never-drawn characters: HarfBuzz's outer `apply_forward`
    /// tests the mask and the glyph property and nothing else, so a lookup is
    /// still *offered* a position holding a joiner. It will not usually match
    /// there — the joiner is not in its coverage — but a face that covers one
    /// is entitled to fire.
    ///
    /// The outer pass asks this once per position in the run, which makes it
    /// the hottest question in the crate.
    #[must_use]
    pub(crate) fn considers(self, glyphs: &[SubGlyph], pos: usize) -> bool {
        glyphs
            .get(pos)
            .is_some_and(|g| g.mask & self.mask != 0 && !self.skips(g.gid))
    }

    /// The first position strictly after `from` that this lookup considers.
    ///
    /// `None` when the run ends first, or when the first glyph it does not
    /// step over is not eligible — the two are the same answer to every
    /// caller, which is that the match fails here.
    #[must_use]
    pub(crate) fn next(self, glyphs: &[SubGlyph], from: usize) -> Option<usize> {
        self.scan(glyphs, from.checked_add(1)?, false, ANY)
    }

    /// The first position strictly after `from` at which `wants` is satisfied,
    /// stepping over the never-drawn characters it is not satisfied by.
    ///
    /// The difference from [`next`](Self::next) plus a test afterwards is the
    /// stepping: `f ZWJ i` finds the `i` here and finds the joiner there. It
    /// is what a ligature's component walk needs, and it is the reason the
    /// criterion has to go *into* the walk.
    #[must_use]
    pub(crate) fn next_matching(
        self,
        glyphs: &[SubGlyph],
        from: usize,
        wants: impl FnMut(usize) -> bool,
    ) -> Option<usize> {
        self.scan(glyphs, from.checked_add(1)?, false, Some(wants))
    }

    /// The last position strictly before `from` that this lookup considers.
    #[must_use]
    pub(crate) fn prev(self, glyphs: &[SubGlyph], from: usize) -> Option<usize> {
        self.scan(glyphs, from.checked_sub(1)?, true, ANY)
    }

    /// Walk `count` considered positions forward from `from` inclusive,
    /// calling `each` with the index of every one, and report the position
    /// just past the last.
    ///
    /// This is the shape every forward matcher wants: it needs to test `count`
    /// entries against `count` considered glyphs and then know where the match
    /// ended, which is not `from + count` once anything was skipped.
    ///
    /// `each` is the walk's criterion as well as its output: a `None` from it
    /// says the entry did not match *here*, which ends the walk on an ordinary
    /// glyph and steps past a never-drawn one. It must therefore record
    /// nothing before it has decided to return `Some` — every caller already
    /// works that way, and one that did not would record a position the match
    /// then walked past.
    pub(crate) fn walk_forward(
        self,
        glyphs: &[SubGlyph],
        from: usize,
        count: usize,
        mut each: impl FnMut(usize, usize) -> Option<()>,
    ) -> Option<usize> {
        let mut at = from;
        for k in 0..count {
            let pos = self.scan(glyphs, at, false, Some(|pos| each(k, pos).is_some()))?;
            at = pos.checked_add(1)?;
        }
        Some(at)
    }

    /// Walk `count` considered positions *backward* from just before `from`,
    /// calling `each` with the index of every one.
    ///
    /// A backtrack sequence is stored closest-glyph-first, so entry `k` is the
    /// `k`-th considered glyph before the input — not the `k`-th from the left.
    pub(crate) fn walk_backward(
        self,
        glyphs: &[SubGlyph],
        from: usize,
        count: usize,
        mut each: impl FnMut(usize, usize) -> Option<()>,
    ) -> Option<()> {
        let mut at = from;
        for k in 0..count {
            let pos = self.scan(
                glyphs,
                at.checked_sub(1)?,
                true,
                Some(|pos| each(k, pos).is_some()),
            )?;
            at = pos;
        }
        Some(())
    }
}

/// A big-endian `u16` at `at`, or `None` if it does not fit.
fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    let bytes: [u8; 2] = data.get(at..end)?.try_into().ok()?;
    Some(u16::from_be_bytes(bytes))
}

/// A big-endian `u32` at `at`, or `None` if it does not fit.
fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    let bytes: [u8; 4] = data.get(at..end)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "a test that builds a malformed table should fail loudly, not \
              silently produce a different table"
)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    fn be16(v: u16) -> [u8; 2] {
        v.to_be_bytes()
    }

    /// A ClassDef format 2: ranges of (start, end, class).
    fn class_def(ranges: &[(u16, u16, u16)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be16(2));
        out.extend_from_slice(&be16(u16::try_from(ranges.len()).unwrap()));
        for &(lo, hi, class) in ranges {
            out.extend_from_slice(&be16(lo));
            out.extend_from_slice(&be16(hi));
            out.extend_from_slice(&be16(class));
        }
        out
    }

    /// A coverage table, format 1.
    fn coverage(glyphs: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(u16::try_from(glyphs.len()).unwrap()));
        for &g in glyphs {
            out.extend_from_slice(&be16(g));
        }
        out
    }

    /// A `GDEF` whose `GlyphClassDef` calls 10..=19 marks and 1..=9 bases.
    ///
    /// The header is the fixed 1.2 shape: major, minor, then the four 16-bit
    /// offsets, then `MarkGlyphSetsDef`.
    fn gdef(minor: u16, mark_set: Option<&[u16]>, attach: Option<&[(u16, u16, u16)]>) -> Vec<u8> {
        let header = 14usize;
        let classes = class_def(&[(1, 9, CLASS_BASE), (10, 19, CLASS_MARK)]);
        let attach_def = attach.map(class_def);
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(minor));
        out.extend_from_slice(&be16(u16::try_from(header).unwrap())); // GlyphClassDef
        out.extend_from_slice(&be16(0)); // AttachList
        out.extend_from_slice(&be16(0)); // LigCaretList
        let attach_at = header + classes.len();
        out.extend_from_slice(&be16(match attach_def {
            Some(_) => u16::try_from(attach_at).unwrap(),
            None => 0,
        }));
        let sets_at = attach_at + attach_def.as_ref().map_or(0, Vec::len);
        out.extend_from_slice(&be16(match mark_set {
            Some(_) => u16::try_from(sets_at).unwrap(),
            None => 0,
        }));
        assert_eq!(out.len(), header);
        out.extend_from_slice(&classes);
        if let Some(a) = &attach_def {
            out.extend_from_slice(a);
        }
        if let Some(set) = mark_set {
            // MarkGlyphSetsDef: format, count, one 32-bit offset per set.
            out.extend_from_slice(&be16(1));
            out.extend_from_slice(&be16(1));
            out.extend_from_slice(&8u32.to_be_bytes());
            out.extend_from_slice(&coverage(set));
        }
        out
    }

    fn span(len: usize) -> Span {
        Span { off: 0, len }
    }

    fn run(gids: &[u16]) -> Vec<SubGlyph> {
        gids.iter()
            .enumerate()
            .map(|(i, &g)| SubGlyph::new(g, i))
            .collect()
    }

    #[test]
    fn a_lookup_with_no_flag_skips_nothing() {
        let data = gdef(2, None, None);
        let defs = Definitions::parse(&data, Some(span(data.len())));
        let skip = Skipper::new(&data, defs, 0, 0, u64::MAX, Joiners::substitution(false));
        assert!(skip.is_trivial());
        let glyphs = run(&[1, 10, 2]);
        // The mark at 1 is visible: nothing asked for it to be hidden.
        assert_eq!(skip.next(&glyphs, 0), Some(1));
    }

    #[test]
    fn ignore_marks_steps_over_a_mark_and_finds_the_letter_past_it() {
        let data = gdef(2, None, None);
        let defs = Definitions::parse(&data, Some(span(data.len())));
        let skip = Skipper::new(
            &data,
            defs,
            IGNORE_MARKS,
            0,
            u64::MAX,
            Joiners::substitution(false),
        );
        assert!(!skip.is_trivial());
        // beh, fatha, beh — which is what a vowelled Arabic ligature has to
        // match across, and the whole reason this module exists.
        let glyphs = run(&[1, 10, 2]);
        assert_eq!(skip.next(&glyphs, 0), Some(2));
        assert_eq!(skip.prev(&glyphs, 2), Some(0));
    }

    #[test]
    fn ignore_bases_and_ignore_ligatures_read_their_own_classes() {
        // Reclassify 5 as a ligature so both bits can be told apart.
        let data = {
            let classes = class_def(&[
                (1, 4, CLASS_BASE),
                (5, 5, CLASS_LIGATURE),
                (10, 19, CLASS_MARK),
            ]);
            let mut out = Vec::new();
            out.extend_from_slice(&be16(1));
            out.extend_from_slice(&be16(0));
            out.extend_from_slice(&be16(12));
            out.extend_from_slice(&be16(0));
            out.extend_from_slice(&be16(0));
            out.extend_from_slice(&be16(0));
            assert_eq!(out.len(), 12);
            out.extend_from_slice(&classes);
            out
        };
        let defs = Definitions::parse(&data, Some(span(data.len())));
        let glyphs = run(&[1, 5, 10]);

        let bases = Skipper::new(
            &data,
            defs,
            IGNORE_BASE_GLYPHS,
            0,
            u64::MAX,
            Joiners::substitution(false),
        );
        assert!(!bases.considers(&glyphs, 0));
        assert!(bases.considers(&glyphs, 1));
        let ligs = Skipper::new(
            &data,
            defs,
            IGNORE_LIGATURES,
            0,
            u64::MAX,
            Joiners::substitution(false),
        );
        assert!(ligs.considers(&glyphs, 0));
        assert_eq!(ligs.next(&glyphs, 0), Some(2));
    }

    #[test]
    fn a_face_with_no_gdef_hides_nothing_however_the_flag_reads() {
        let defs = Definitions::parse(&[], None);
        let skip = Skipper::new(
            &[],
            defs,
            IGNORE_MARKS,
            0,
            u64::MAX,
            Joiners::substitution(false),
        );
        let glyphs = run(&[1, 10, 2]);
        // A flag that says "ignore marks" over a face that never said which
        // glyphs are marks has named the empty set, not every glyph.
        assert_eq!(skip.next(&glyphs, 0), Some(1));
    }

    #[test]
    fn a_mark_filtering_set_admits_only_the_marks_it_lists() {
        let data = gdef(2, Some(&[11]), None);
        let defs = Definitions::parse(&data, Some(span(data.len())));
        let skip = Skipper::new(
            &data,
            defs,
            USE_MARK_FILTERING_SET,
            0,
            u64::MAX,
            Joiners::substitution(false),
        );
        // 10 is a mark and not in the set, so it is hidden; 11 is in it.
        let hidden = run(&[1, 10, 2]);
        assert_eq!(skip.next(&hidden, 0), Some(2));
        let shown = run(&[1, 11, 2]);
        assert_eq!(skip.next(&shown, 0), Some(1));
    }

    #[test]
    fn a_mark_filtering_set_is_not_read_from_a_gdef_too_old_to_have_one() {
        // The offset is written where 1.2 keeps it, but the table says 1.0, so
        // reading it would be reading whatever follows the header as an offset.
        let data = gdef(0, Some(&[11]), None);
        let defs = Definitions::parse(&data, Some(span(data.len())));
        assert!(defs.sets.is_none());
    }

    #[test]
    fn a_mark_attachment_class_admits_only_its_own_marks() {
        // Marks 10..=14 are attachment class 1, 15..=19 are class 2.
        let data = gdef(2, None, Some(&[(10, 14, 1), (15, 19, 2)]));
        let defs = Definitions::parse(&data, Some(span(data.len())));
        // The class travels in the high byte of the flag.
        let skip = Skipper::new(
            &data,
            defs,
            1 << 8,
            0,
            u64::MAX,
            Joiners::substitution(false),
        );
        let glyphs = run(&[1, 15, 11, 2]);
        // 15 is class 2 and hidden; 11 is class 1 and visible.
        assert_eq!(skip.next(&glyphs, 0), Some(2));
    }

    /// A run whose glyphs carry the masks given, so the feature gate can be
    /// exercised independently of what any real face would produce.
    fn masked(entries: &[(u16, u64)]) -> Vec<SubGlyph> {
        entries
            .iter()
            .enumerate()
            .map(|(i, &(gid, mask))| SubGlyph::masked(gid, i, mask))
            .collect()
    }

    #[test]
    fn an_ineligible_glyph_stops_the_walk_rather_than_being_stepped_over() {
        let data = gdef(2, None, None);
        let defs = Definitions::parse(&data, Some(span(data.len())));
        // A lookup reached by feature bit 1 alone.
        let skip = Skipper::new(
            &data,
            defs,
            IGNORE_MARKS,
            0,
            0b10,
            Joiners::substitution(false),
        );
        // Glyph 2 is not eligible for that feature; glyph 10 is a mark the
        // flag hides. The walk must stop at 2 rather than hop over it to 3 —
        // hopping would form a ligature across a letter the feature does not
        // apply to, which is the whole distinction between skipping and not
        // matching.
        let glyphs = masked(&[(1, 0b10), (10, 0b10), (2, 0b01), (3, 0b10)]);
        assert_eq!(skip.next(&glyphs, 0), None);
    }

    #[test]
    fn the_context_skipper_skips_the_same_glyphs_but_gates_on_nothing() {
        let data = gdef(2, None, None);
        let defs = Definitions::parse(&data, Some(span(data.len())));
        let skip = Skipper::new(
            &data,
            defs,
            IGNORE_MARKS,
            0,
            0b10,
            Joiners::substitution(false),
        );
        let glyphs = masked(&[(1, 0b10), (10, 0b10), (2, 0b01)]);
        assert_eq!(skip.next(&glyphs, 0), None);
        // Same flag, same mark skipped, but the feature gate is open: a
        // neighbour is a neighbour whatever feature reached the rule.
        assert_eq!(skip.context().next(&glyphs, 0), Some(2));
    }

    #[test]
    fn a_forward_walk_reports_where_the_match_ended_not_where_it_would_have() {
        let data = gdef(2, None, None);
        let defs = Definitions::parse(&data, Some(span(data.len())));
        let skip = Skipper::new(
            &data,
            defs,
            IGNORE_MARKS,
            0,
            u64::MAX,
            Joiners::substitution(false),
        );
        let glyphs = run(&[1, 10, 2, 10, 3]);
        let mut seen = Vec::new();
        let end = skip.walk_forward(&glyphs, 0, 3, |_, pos| {
            seen.push(pos);
            Some(())
        });
        assert_eq!(seen, vec![0, 2, 4]);
        // Three matched glyphs spanning five positions: a caller that assumed
        // `from + count` would resume in the middle of its own match.
        assert_eq!(end, Some(5));
    }

    #[test]
    fn a_backward_walk_counts_outward_from_the_input() {
        let data = gdef(2, None, None);
        let defs = Definitions::parse(&data, Some(span(data.len())));
        let skip = Skipper::new(
            &data,
            defs,
            IGNORE_MARKS,
            0,
            u64::MAX,
            Joiners::substitution(false),
        );
        let glyphs = run(&[3, 10, 2, 10, 1]);
        let mut seen = Vec::new();
        let ok = skip.walk_backward(&glyphs, 4, 2, |_, pos| {
            seen.push(pos);
            Some(())
        });
        assert!(ok.is_some());
        // Entry 0 is the nearest glyph, not the leftmost.
        assert_eq!(seen, vec![2, 0]);
    }

    /// A run whose glyphs carry the ignorable kinds given, so the three-valued
    /// answer can be exercised without a face that actually maps a joiner.
    fn kinds(entries: &[(u16, Ignorable)]) -> Vec<SubGlyph> {
        entries
            .iter()
            .enumerate()
            .map(|(i, &(gid, kind))| {
                let mut glyph = SubGlyph::new(gid, i);
                glyph.ignorable = kind;
                glyph
            })
            .collect()
    }

    /// A `Skipper` over a `GDEF` that hides nothing, so only the ignorable
    /// kinds decide the walk.
    fn plain(data: &[u8], joiners: Joiners) -> Skipper<'_> {
        let defs = Definitions::parse(data, Some(span(data.len())));
        Skipper::new(data, defs, 0, 0, u64::MAX, joiners)
    }

    #[test]
    fn a_substitution_input_steps_over_zwj_but_stops_at_zwnj() {
        let data = gdef(2, None, None);
        let skip = plain(&data, Joiners::substitution(false));
        // `f ZWJ i` is asking for the ligature; `f ZWNJ i` is asking for it not
        // to form. A matcher that treats the two alike gets one of them wrong
        // whichever way it leans.
        let joined = kinds(&[(1, Ignorable::No), (2, Ignorable::Zwj), (3, Ignorable::No)]);
        assert_eq!(
            skip.next_matching(&joined, 0, |p| joined.get(p).is_some_and(|g| g.gid == 3)),
            Some(2)
        );
        let broken = kinds(&[(1, Ignorable::No), (2, Ignorable::Zwnj), (3, Ignorable::No)]);
        assert_eq!(
            skip.next_matching(&broken, 0, |p| broken.get(p).is_some_and(|g| g.gid == 3)),
            None
        );
        // Both are *considered* by a walk that asks for nothing in particular:
        // a rule with no criterion at this position takes whatever glyph is
        // there, and the ZWJ is skipped only because the criterion it failed
        // was allowed to carry on past it. The ZWNJ, having no reason to be
        // stepped over, is simply the next glyph.
        assert_eq!(skip.next(&joined, 0), Some(2));
        assert_eq!(skip.next(&broken, 0), Some(1));
    }

    #[test]
    fn a_soft_hyphen_is_stepped_over_by_every_kind_of_lookup() {
        let data = gdef(2, None, None);
        // A discretionary hyphen sits between the two halves of a word and is
        // drawn only at a line break. Nothing about it is a letter boundary.
        let glyphs = kinds(&[
            (1, Ignorable::No),
            (2, Ignorable::Plain),
            (3, Ignorable::No),
        ]);
        for joiners in [Joiners::substitution(false), Joiners::POSITIONING] {
            let skip = plain(&data, joiners);
            assert_eq!(skip.next(&glyphs, 0), Some(2));
            assert_eq!(skip.context().next(&glyphs, 0), Some(2));
        }
    }

    #[test]
    fn a_context_match_steps_over_both_joiners_where_an_input_match_would_not() {
        let data = gdef(2, None, None);
        // A rule's *surroundings* are joiner-blind even when its input is not:
        // the joiner is there to speak to the ligature, not to the context.
        for kind in [Ignorable::Zwnj, Ignorable::Zwj] {
            let glyphs = kinds(&[(1, Ignorable::No), (2, kind), (3, Ignorable::No)]);
            assert_eq!(
                plain(&data, Joiners::substitution(false))
                    .context()
                    .next(&glyphs, 0),
                Some(2)
            );
        }
    }

    #[test]
    fn a_hidden_character_is_stepped_over_when_positioning_and_not_before() {
        let data = gdef(2, None, None);
        // CGJ blocks a substitution deliberately — that is its whole job — but
        // it must not push a mark off the base it is anchored to.
        let glyphs = kinds(&[
            (1, Ignorable::No),
            (2, Ignorable::Hidden),
            (3, Ignorable::No),
        ]);
        let wants_3 = |p: usize| glyphs.get(p).is_some_and(|g| g.gid == 3);
        let subs = plain(&data, Joiners::substitution(false));
        assert_eq!(subs.next_matching(&glyphs, 0, wants_3), None);
        assert_eq!(subs.context().next_matching(&glyphs, 0, wants_3), None);
        let pos = plain(&data, Joiners::POSITIONING);
        assert_eq!(pos.next_matching(&glyphs, 0, wants_3), Some(2));
        assert_eq!(pos.next(&glyphs, 0), Some(2));
    }

    #[test]
    fn a_feature_that_handles_the_joiners_itself_is_not_second_guessed() {
        let data = gdef(2, None, None);
        // The Indic and Khmer features are `F_MANUAL_JOINERS`: their rules name
        // the joiners, so stepping over one would delete the distinction the
        // rule was written to read.
        let manual = Joiners::substitution(true);
        for kind in [Ignorable::Zwnj, Ignorable::Zwj] {
            let glyphs = kinds(&[(1, Ignorable::No), (2, kind), (3, Ignorable::No)]);
            let wants_3 = |p: usize| glyphs.get(p).is_some_and(|g| g.gid == 3);
            assert_eq!(
                plain(&data, manual).next_matching(&glyphs, 0, wants_3),
                None
            );
            // The same run under the automatic rule: the ZWJ is stepped over,
            // so the difference the flag makes is visible in one assertion.
            let auto =
                plain(&data, Joiners::substitution(false)).next_matching(&glyphs, 0, wants_3);
            assert_eq!(auto, (kind == Ignorable::Zwj).then_some(2));
        }
    }

    #[test]
    fn a_rule_that_names_an_ignorable_matches_it_instead_of_stepping_over_it() {
        let data = gdef(2, None, None);
        let skip = plain(&data, Joiners::substitution(false));
        let glyphs = kinds(&[
            (1, Ignorable::No),
            (2, Ignorable::Plain),
            (3, Ignorable::No),
        ]);
        // Stepping over is what happens when the rule had nothing to say about
        // the glyph. Ask for the glyph by name and it is there — which is why
        // the criterion has to travel into the walk rather than be applied to
        // whatever the walk happened to return.
        assert_eq!(
            skip.next_matching(&glyphs, 0, |p| glyphs.get(p).is_some_and(|g| g.gid == 2)),
            Some(1)
        );
        assert_eq!(
            skip.next_matching(&glyphs, 0, |p| glyphs.get(p).is_some_and(|g| g.gid == 3)),
            Some(2)
        );
        // A glyph that is not ignorable still stops the walk when it is not the
        // one asked for: `SKIP_NO` and `MATCH_NO` together are a failure.
        assert_eq!(
            skip.next_matching(&glyphs, 1, |p| glyphs.get(p).is_some_and(|g| g.gid == 9)),
            None
        );
    }

    #[test]
    fn a_walk_that_runs_out_of_run_fails_rather_than_matching_short() {
        let data = gdef(2, None, None);
        let defs = Definitions::parse(&data, Some(span(data.len())));
        let skip = Skipper::new(&data, defs, 0, 0, u64::MAX, Joiners::substitution(false));
        let glyphs = run(&[1, 2]);
        assert_eq!(skip.walk_forward(&glyphs, 0, 3, |_, _| Some(())), None);
        assert!(skip.walk_backward(&glyphs, 1, 2, |_, _| Some(())).is_none());
    }
}
