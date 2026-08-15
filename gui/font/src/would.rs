//! "Would this feature substitute these glyphs?" — asking the font a question
//! instead of telling it to do something.
//!
//! Every other path in this crate hands a lookup a run and lets it rewrite it.
//! The Indic shaper needs the opposite: before it can decide anything it has
//! to know what the *font* thinks, about glyph sequences that are not in the
//! run and never will be.
//!
//! Two decisions depend on it, and neither can be made from Unicode alone:
//!
//! * **Where the base consonant is.** A syllable-initial `Ra` followed by a
//!   virama becomes a reph — the little hook over the syllable — but only if
//!   the font has a glyph for one. The shaper asks whether `rphf` would
//!   substitute `Ra, Halant`; if the answer is no, that `Ra` is an ordinary
//!   consonant and may be the base.
//!
//! * **Where a consonant sits relative to the base.** Devanagari's `ra` below
//!   a `k` is drawn under it, its `ya` after it, and which consonants go where
//!   is a property of the typeface, not of the script. The shaper asks whether
//!   `blwf` or `vatu` would substitute the pair — below-base — then `pstf` or
//!   `pref` — post-base — and calls it a base-position consonant only if
//!   nothing claims it.
//!
//! Each pair is probed in *both* orders, `{virama, consonant}` and
//! `{consonant, virama}`, because fonts written to OpenType's first Indic
//! specification used one and fonts written to the second use the other, and
//! plenty of the latter were made by copying the former's lookups into the new
//! tables. That is the caller's business; this module just answers about the
//! sequence it is given.
//!
//! # Why this is not [`gsub::apply`](crate::gsub)
//!
//! It looks like it should be: build a two-glyph run, apply the feature, see
//! whether it changed. It is not, for two reasons that are both about the
//! question being hypothetical.
//!
//! The glyphs being asked about are not in any run, so there is nothing on
//! either side of them. A chaining rule whose real context is "after a
//! consonant" would match nothing when probed, and the font would be reported
//! as having no below-base form for a consonant it draws below the base every
//! time. HarfBuzz's answer, which this copies, is that a chaining rule with
//! any backtrack or lookahead simply does not answer the question — it is
//! skipped, not failed — and the *input* alone decides.
//!
//! And the whole point is to ask *without* rewriting: the shaper is deciding
//! how to lay the syllable out, which happens before any feature is applied.
//!
//! # `zero_context`
//!
//! The rule just described — skip a chaining rule that has any context — is
//! what `zero_context` turns on, and HarfBuzz turns it *off* for the old Indic
//! specification and for Malayalam. Those fonts write their below-base and
//! post-base forms as chaining rules with real context, and holding them to
//! the strict rule would find nothing at all in them. With it off, the
//! backtrack and lookahead are not matched *or* rejected: they are ignored,
//! and the input alone decides.
//!
//! # What this deliberately does not do
//!
//! A matched contextual rule names other lookups to run. This never follows
//! them: the question is whether the feature *reaches* this sequence, not what
//! the eventual glyph would be. HarfBuzz does the same, and for the same
//! reason — the answer is used to place a consonant, not to draw one.

use crate::context::MAX_RULES;
use crate::gsub::{
    LOOKUP_ALTERNATE, LOOKUP_CHAIN_CONTEXT, LOOKUP_CONTEXT, LOOKUP_LIGATURE, LOOKUP_MULTIPLE,
    LOOKUP_SINGLE,
};
use crate::otl::{Lookup, coverage_index, glyph_class};
use crate::sfnt::u16_at;

/// Would any subtable of `lookup` substitute exactly the sequence `glyphs`?
///
/// `glyphs` is the whole hypothetical run: a rule matching a prefix of it does
/// not count, and neither does one needing more than it. That exactness is
/// what makes the answer mean "this font has a below-base form for this pair"
/// rather than "something in this font touches this glyph".
pub(crate) fn would_apply(
    data: &[u8],
    lookup: &Lookup,
    glyphs: &[u16],
    zero_context: bool,
) -> bool {
    if glyphs.is_empty() {
        return false;
    }
    lookup
        .subtables
        .iter()
        .any(|&sub| subtable(data, lookup.kind, sub, glyphs, zero_context).unwrap_or(false))
}

/// One subtable's answer. `None` is a truncated or malformed table, which the
/// caller reads as "no" — the same as a sound table that does not match.
fn subtable(
    data: &[u8],
    kind: u16,
    sub: usize,
    glyphs: &[u16],
    zero_context: bool,
) -> Option<bool> {
    match kind {
        // All three replace one glyph, so all three answer only about a
        // one-glyph sequence, and all three keep their coverage in the same
        // place in every format they have.
        LOOKUP_SINGLE | LOOKUP_MULTIPLE | LOOKUP_ALTERNATE => {
            if glyphs.len() != 1 {
                return Some(false);
            }
            let coverage = offset(data, sub, sub.checked_add(2)?)?;
            Some(coverage_index(data, coverage, *glyphs.first()?).is_some())
        }
        LOOKUP_LIGATURE => ligature(data, sub, glyphs),
        LOOKUP_CONTEXT => context(data, sub, glyphs),
        LOOKUP_CHAIN_CONTEXT => chain(data, sub, glyphs, zero_context),
        _ => Some(false),
    }
}

/// A `LigatureSubst`: the sequence must be exactly one ligature's components.
fn ligature(data: &[u8], sub: usize, glyphs: &[u16]) -> Option<bool> {
    if u16_at(data, sub)? != 1 {
        return Some(false);
    }
    let coverage = offset(data, sub, sub.checked_add(2)?)?;
    let index = coverage_index(data, coverage, *glyphs.first()?)?;
    let count = u16_at(data, sub.checked_add(4)?)?;
    let Some(set) = set_at(data, sub, sub.checked_add(6)?, count, index) else {
        return Some(false);
    };
    any_of(data, set, |lig| {
        // `componentCount` counts the first glyph, which the coverage has
        // already accounted for and which is therefore not in the array.
        let components = usize::from(u16_at(data, lig.checked_add(2)?)?);
        if components != glyphs.len() {
            return Some(false);
        }
        let at = lig.checked_add(4)?;
        for k in 1..components {
            let want = u16_at(data, at.checked_add(k.checked_sub(1)?.checked_mul(2)?)?)?;
            if *glyphs.get(k)? != want {
                return Some(false);
            }
        }
        Some(true)
    })
}

/// A `SequenceContext` in any of its three formats.
fn context(data: &[u8], sub: usize, glyphs: &[u16]) -> Option<bool> {
    let first = *glyphs.first()?;
    match u16_at(data, sub)? {
        1 => {
            let coverage = offset(data, sub, sub.checked_add(2)?)?;
            let index = coverage_index(data, coverage, first)?;
            let count = u16_at(data, sub.checked_add(4)?)?;
            let Some(set) = set_at(data, sub, sub.checked_add(6)?, count, index) else {
                return Some(false);
            };
            any_of(data, set, |rule| seq_rule(data, rule, By::Glyph, glyphs))
        }
        // No coverage gate, deliberately: HarfBuzz's `would_apply` for this
        // format goes straight to the ClassDef, and a font whose coverage and
        // ClassDef disagree must be read the way every other engine reads it.
        2 => {
            let classes = offset(data, sub, sub.checked_add(4)?)?;
            let class = glyph_class(data, classes, first)?;
            let count = u16_at(data, sub.checked_add(6)?)?;
            let Some(set) = set_at(data, sub, sub.checked_add(8)?, count, class) else {
                return Some(false);
            };
            any_of(data, set, |rule| seq_rule(data, rule, By::Class(classes), glyphs))
        }
        3 => {
            let count = usize::from(u16_at(data, sub.checked_add(2)?)?);
            if count != glyphs.len() {
                return Some(false);
            }
            covered_tail(data, sub, sub.checked_add(6)?, glyphs)
        }
        _ => Some(false),
    }
}

/// A `ChainedSequenceContext` in any of its three formats.
fn chain(data: &[u8], sub: usize, glyphs: &[u16], zero_context: bool) -> Option<bool> {
    let first = *glyphs.first()?;
    match u16_at(data, sub)? {
        1 => {
            let coverage = offset(data, sub, sub.checked_add(2)?)?;
            let index = coverage_index(data, coverage, first)?;
            let count = u16_at(data, sub.checked_add(4)?)?;
            let Some(set) = set_at(data, sub, sub.checked_add(6)?, count, index) else {
                return Some(false);
            };
            any_of(data, set, |rule| {
                chain_rule(data, rule, By::Glyph, glyphs, zero_context)
            })
        }
        2 => {
            // Three ClassDefs, one per part; the input's is what classifies
            // the first glyph, since that is the part being asked about.
            let classes = offset(data, sub, sub.checked_add(6)?)?;
            let class = glyph_class(data, classes, first)?;
            let count = u16_at(data, sub.checked_add(10)?)?;
            let Some(set) = set_at(data, sub, sub.checked_add(12)?, count, class) else {
                return Some(false);
            };
            any_of(data, set, |rule| {
                chain_rule(data, rule, By::Class(classes), glyphs, zero_context)
            })
        }
        3 => {
            let at = sub.checked_add(2)?;
            let back = usize::from(u16_at(data, at)?);
            let at = at.checked_add(2)?.checked_add(back.checked_mul(2)?)?;
            let count = usize::from(u16_at(data, at)?);
            if count != glyphs.len() {
                return Some(false);
            }
            let input = at.checked_add(2)?;
            if zero_context {
                let ahead = input.checked_add(count.checked_mul(2)?)?;
                if back != 0 || u16_at(data, ahead)? != 0 {
                    return Some(false);
                }
            }
            covered_tail(data, sub, input, glyphs)
        }
        _ => Some(false),
    }
}

/// A `SequenceRule` or `ClassSequenceRule`: a count, then the input from its
/// *second* glyph on — the first having been matched by the coverage or the
/// ClassDef already.
fn seq_rule(data: &[u8], rule: usize, by: By, glyphs: &[u16]) -> Option<bool> {
    let count = usize::from(u16_at(data, rule)?);
    if count != glyphs.len() {
        return Some(false);
    }
    tail(data, rule.checked_add(4)?, by, glyphs)
}

/// A `ChainedSequenceRule` or `ChainedClassSequenceRule`.
///
/// The backtrack and lookahead are read only to find the input between them —
/// and, under `zero_context`, to refuse a rule that has either. They are never
/// *matched*, because there is nothing outside the probed sequence to match
/// them against.
fn chain_rule(
    data: &[u8],
    rule: usize,
    by: By,
    glyphs: &[u16],
    zero_context: bool,
) -> Option<bool> {
    let back = usize::from(u16_at(data, rule)?);
    let at = rule.checked_add(2)?.checked_add(back.checked_mul(2)?)?;
    let count = usize::from(u16_at(data, at)?);
    if count != glyphs.len() {
        return Some(false);
    }
    let input = at.checked_add(2)?;
    if zero_context {
        // `count` is at least one, `glyphs` being non-empty, so the input
        // array holds `count - 1` entries and the lookahead count follows it.
        let ahead = input.checked_add(count.checked_sub(1)?.checked_mul(2)?)?;
        if back != 0 || u16_at(data, ahead)? != 0 {
            return Some(false);
        }
    }
    tail(data, input, by, glyphs)
}

/// How a rule names the glyphs it matches: by id, or by the class a ClassDef
/// puts them in.
#[derive(Clone, Copy)]
enum By {
    /// Entries are glyph ids.
    Glyph,
    /// Entries are classes, assigned by the ClassDef at this offset.
    Class(usize),
}

/// Match `glyphs[1..]` against the entries at `at`, which are `glyphs.len() -
/// 1` of them.
fn tail(data: &[u8], at: usize, by: By, glyphs: &[u16]) -> Option<bool> {
    for k in 1..glyphs.len() {
        let want = u16_at(data, at.checked_add(k.checked_sub(1)?.checked_mul(2)?)?)?;
        let gid = *glyphs.get(k)?;
        let ok = match by {
            By::Glyph => gid == want,
            By::Class(table) => glyph_class(data, table, gid)? == want,
        };
        if !ok {
            return Some(false);
        }
    }
    Some(true)
}

/// The same for the format-3 tables, whose entries are coverage offsets and
/// whose array is indexed by the glyph's own position — so entry 0 is the
/// first glyph's, and is skipped for the same reason the arrays above start at
/// the second.
fn covered_tail(data: &[u8], sub: usize, at: usize, glyphs: &[u16]) -> Option<bool> {
    for k in 1..glyphs.len() {
        let coverage = offset(data, sub, at.checked_add(k.checked_mul(2)?)?)?;
        if coverage_index(data, coverage, *glyphs.get(k)?).is_none() {
            return Some(false);
        }
    }
    Some(true)
}

/// Try `f` on each entry of the offset array at `set`, stopping at the first
/// that answers yes.
///
/// Both a `LigatureSet` and a rule set have this shape — a count, then offsets
/// from their own start — and both are tried in font order.
fn any_of(data: &[u8], set: usize, mut f: impl FnMut(usize) -> Option<bool>) -> Option<bool> {
    let count = u16_at(data, set)?;
    for i in 0..usize::from(count).min(MAX_RULES) {
        let Some(entry) = set
            .checked_add(2)
            .and_then(|o| i.checked_mul(2).and_then(|d| o.checked_add(d)))
            .and_then(|at| u16_at(data, at))
            .and_then(|o| set.checked_add(usize::from(o)))
        else {
            continue;
        };
        // One unreadable entry does not condemn the set: the rest of it may
        // still be sound, and a rule that cannot be read is a rule that does
        // not match.
        if f(entry).unwrap_or(false) {
            return Some(true);
        }
    }
    Some(false)
}

/// The entry at `index` of an array of `count` offsets starting at `at`, each
/// measured from `sub`.
fn set_at(data: &[u8], sub: usize, at: usize, count: u16, index: u16) -> Option<usize> {
    if index >= count {
        return None;
    }
    offset(data, sub, at.checked_add(usize::from(index).checked_mul(2)?)?)
}

/// Follow an offset stored at `field` and measured from `sub`, refusing a null
/// one — zero means "absent" everywhere in OpenType, and following it lands
/// back on the subtable's own header.
fn offset(data: &[u8], sub: usize, field: usize) -> Option<usize> {
    let off = u16_at(data, field)?;
    if off == 0 {
        return None;
    }
    sub.checked_add(usize::from(off))
}
