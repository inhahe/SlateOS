//! Contextual rule matching, shared by `GSUB` and `GPOS`.
//!
//! `SequenceContext` and `ChainedSequenceContext` are one pair of table
//! formats that both layout tables embed: `GSUB` numbers them types 5 and 6,
//! `GPOS` numbers them 7 and 8, and byte for byte they are identical. A rule
//! says "when these glyphs stand here, with these before and these after, run
//! lookup *n* at input glyph *k*" — and nothing in that sentence mentions
//! whether lookup *n* substitutes or positions.
//!
//! So the *matching* is shared and lives here, and the *applying* does not.
//! What a matched rule does differs sharply between the two tables:
//! substitution may grow or shrink the run under a later record's feet, so
//! [`gsub`](crate::gsub) has to track where each matched glyph moved to;
//! positioning never changes the run's length, so [`gpos`](crate::gpos) can
//! run the records against the positions the match reported and be done. That
//! difference is the whole reason `apply_nested` is not in this module.
//!
//! # What a caller supplies
//!
//! [`context_match`] and [`chain_match`] take the run as `&[SubGlyph]` and a
//! [`Skipper`], and return a [`Matched`] describing the hit: which absolute
//! positions the input landed on, how far the match reaches, and where the
//! `SequenceLookupRecord` array is. The caller reads that array with
//! [`read_records`] and does whatever its table does with it.
//!
//! The `rules` buffer both matchers take is a caller-owned scratch `Vec`
//! rather than a local, so that a lookup which matches at many positions
//! allocates once. It must not be shared with a nested invocation — see
//! `gsub::apply_context`.

use alloc::vec::Vec;

use crate::gsub::SubGlyph;
use crate::otl::{coverage_index, glyph_class};
use crate::sfnt::u16_at;
use crate::skip::Skipper;

/// A ceiling on how long a context — backtrack, input or lookahead — may be.
///
/// Every glyph of a context is compared at every position of the run, so an
/// unbounded one makes matching quadratic in the run length. Real contexts are
/// two or three glyphs; the longest in common use are the Indic reordering
/// rules, still well inside this.
pub(crate) const MAX_CONTEXT: usize = 16;

/// A ceiling on how many lookups one context match may invoke.
const MAX_NESTED: usize = 16;

/// A ceiling on how many rules one rule set may hold.
///
/// A rule set is already keyed by the first glyph or its class, so a real one
/// holds a handful; the cap is what stops a corrupt `seqRuleCount` from making
/// every position in a line scan tens of thousands of rules.
pub(crate) const MAX_RULES: usize = 256;

/// How deep a contextual lookup's invocations may nest.
///
/// A contextual lookup may invoke another contextual lookup, and nothing in
/// the format forbids that from being a cycle — lookup 3 invoking lookup 3.
/// Matching HarfBuzz's limit, which real fonts stay far inside.
pub(crate) const MAX_NESTING: usize = 6;

/// A `SequenceLookupRecord`: run lookup `lookup` at input position `at`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Nested {
    /// Which glyph *of the matched input* to run the lookup at — not a
    /// position in the run. The two differ once an earlier record has changed
    /// the run's length.
    pub(crate) at: u16,
    /// Index into the font's LookupList.
    pub(crate) lookup: u16,
}

/// What a contextual subtable matched.
///
/// `at` is what makes this a struct rather than a pair of counts: once the
/// lookup's flag can hide a glyph, the matched input is no longer the run of
/// positions `i..i + input`, and the nested lookups have to be told where the
/// glyphs they name actually are.
pub(crate) struct Matched {
    /// Absolute positions of the matched input glyphs, in order.
    pub(crate) at: [usize; MAX_CONTEXT],
    /// How many entries of `at` are real.
    pub(crate) input: usize,
    /// One past the last matched input position: the span the match covers is
    /// `i..end`, which includes anything the flag skipped inside it.
    pub(crate) end: usize,
    /// Where the `SequenceLookupRecord` array is.
    pub(crate) records: usize,
    /// How many records it holds.
    pub(crate) count: usize,
}

impl Matched {
    /// An empty match, for a walk to fill in.
    fn blank() -> Self {
        Self {
            at: [0; MAX_CONTEXT],
            input: 0,
            end: 0,
            records: 0,
            count: 0,
        }
    }

    /// The positions the match landed on, as a slice.
    pub(crate) fn positions(&self) -> &[usize] {
        self.at.get(..self.input).unwrap_or_default()
    }
}

/// The `SequenceLookupRecord` array at `at`.
pub(crate) fn read_records(data: &[u8], at: usize, count: usize, out: &mut Vec<Nested>) {
    out.clear();
    for i in 0..count.min(MAX_NESTED) {
        let Some(rec) = i.checked_mul(4).and_then(|d| at.checked_add(d)) else {
            return;
        };
        // A record that cannot be read ends the list rather than discarding
        // the ones already found: a truncated table should lose what it
        // truncated, not what came before it.
        let (Some(at), Some(lookup)) = (
            u16_at(data, rec),
            rec.checked_add(2).and_then(|o| u16_at(data, o)),
        ) else {
            return;
        };
        out.push(Nested { at, lookup });
    }
}

/// How a context names the glyphs it matches.
///
/// Format 1 and format 2 of both contextual types are the same rule layout
/// read one way or the other — ids, or the classes a ClassDef sorts glyphs
/// into — so they share one walk rather than two nearly-identical ones.
#[derive(Clone, Copy)]
enum By {
    /// Entries are glyph ids.
    Glyph,
    /// Entries are classes, assigned by the ClassDef at this offset.
    Class(usize),
}

/// Does the glyph at `pos` answer to `want` under `by`?
fn answers(data: &[u8], by: By, want: u16, glyphs: &[SubGlyph], pos: usize) -> Option<bool> {
    let gid = glyphs.get(pos)?.gid;
    Some(match by {
        By::Glyph => gid == want,
        By::Class(table) => glyph_class(data, table, gid)? == want,
    })
}

/// Match `count` entries read from `at` against the glyphs the lookup
/// considers running forward from `from`, and report the position just past
/// the last one.
///
/// `record` is handed each matched entry's index and the absolute position it
/// landed on. Callers matching a *context* discard it; callers matching the
/// *input* keep it, because that is what the nested lookups will be run at.
fn forward(
    data: &[u8],
    at: usize,
    count: usize,
    by: By,
    glyphs: &[SubGlyph],
    from: usize,
    skip: Skipper<'_>,
    mut record: impl FnMut(usize, usize),
) -> Option<usize> {
    skip.walk_forward(glyphs, from, count, |k, pos| {
        let want = u16_at(data, at.checked_add(k.checked_mul(2)?)?)?;
        answers(data, by, want, glyphs, pos)?.then_some(())?;
        record(k, pos);
        Some(())
    })
}

/// Match `count` entries read from `at` against the glyphs running *backward*
/// from just before `from`.
///
/// A backtrack is stored closest-glyph-first, which is the reverse of reading
/// order: entry 0 is the glyph immediately before the input, not the leftmost
/// of the context.
fn backward(
    data: &[u8],
    at: usize,
    count: usize,
    by: By,
    glyphs: &[SubGlyph],
    from: usize,
    skip: Skipper<'_>,
) -> Option<()> {
    skip.walk_backward(glyphs, from, count, |k, pos| {
        let want = u16_at(data, at.checked_add(k.checked_mul(2)?)?)?;
        answers(data, by, want, glyphs, pos)?.then_some(())
    })
}

/// Match `count` coverage offsets read from `at`, each measured from `sub`,
/// against the glyphs the lookup considers running forward from `from`.
fn forward_covered(
    data: &[u8],
    sub: usize,
    at: usize,
    count: usize,
    glyphs: &[SubGlyph],
    from: usize,
    skip: Skipper<'_>,
    mut record: impl FnMut(usize, usize),
) -> Option<usize> {
    skip.walk_forward(glyphs, from, count, |k, pos| {
        let cov = sub_offset(data, sub, at.checked_add(k.checked_mul(2)?)?)?;
        coverage_index(data, cov, glyphs.get(pos)?.gid)?;
        record(k, pos);
        Some(())
    })
}

/// The backward counterpart of [`forward_covered`].
fn backward_covered(
    data: &[u8],
    sub: usize,
    at: usize,
    count: usize,
    glyphs: &[SubGlyph],
    from: usize,
    skip: Skipper<'_>,
) -> Option<()> {
    skip.walk_backward(glyphs, from, count, |k, pos| {
        let cov = sub_offset(data, sub, at.checked_add(k.checked_mul(2)?)?)?;
        coverage_index(data, cov, glyphs.get(pos)?.gid)?;
        Some(())
    })
}

/// Follow an offset stored at `field` and measured from `sub`, refusing a null
/// one.
///
/// Zero means "absent" everywhere in OpenType, and following it lands back on
/// the subtable's own header — where a format number would be read as a
/// coverage format and answer nonsense.
fn sub_offset(data: &[u8], sub: usize, field: usize) -> Option<usize> {
    let off = u16_at(data, field)?;
    if off == 0 {
        return None;
    }
    sub.checked_add(usize::from(off))
}

/// The rule set at `index` of an array of `count` offsets starting at `at`.
///
/// A null offset means this glyph, or this class, has no rules — which format
/// 2 tables are mostly made of, since a ClassDef sorts every glyph in the font
/// into some class and only a few classes start a context.
fn rule_set(data: &[u8], sub: usize, at: usize, count: u16, index: u16) -> Option<usize> {
    if index >= count {
        return None;
    }
    sub_offset(data, sub, at.checked_add(usize::from(index).checked_mul(2)?)?)
}

/// The rules of a rule set, in the font's order — which is the order they are
/// tried in, first match winning.
fn read_rules(data: &[u8], set: usize, out: &mut Vec<usize>) {
    out.clear();
    let Some(count) = u16_at(data, set) else {
        return;
    };
    for i in 0..usize::from(count).min(MAX_RULES) {
        let Some(at) = i
            .checked_mul(2)
            .and_then(|d| set.checked_add(2).and_then(|s| s.checked_add(d)))
        else {
            return;
        };
        let Some(rule) = u16_at(data, at).and_then(|o| set.checked_add(usize::from(o))) else {
            return;
        };
        out.push(rule);
    }
}

/// Match a `SequenceRule` or `ClassSequenceRule` starting at `i`.
///
/// Returns the input length and where its lookup records are. The rule stores
/// its input from the *second* glyph onwards: the first is the one the
/// subtable's coverage — or, for format 2, its class — has already matched.
fn seq_rule(
    data: &[u8],
    rule: usize,
    by: By,
    glyphs: &[SubGlyph],
    i: usize,
    skip: Skipper<'_>,
) -> Option<Matched> {
    let count = usize::from(u16_at(data, rule)?);
    let records = usize::from(u16_at(data, rule.checked_add(2)?)?);
    if count == 0 || count > MAX_CONTEXT {
        return None;
    }
    let mut hit = Matched::blank();
    *hit.at.get_mut(0)? = i;
    let rest = count.checked_sub(1)?;
    let at = rule.checked_add(4)?;
    let mut fill = [0usize; MAX_CONTEXT];
    let end = forward(
        data,
        at,
        rest,
        by,
        glyphs,
        i.checked_add(1)?,
        skip,
        |k, pos| {
            if let Some(slot) = fill.get_mut(k) {
                *slot = pos;
            }
        },
    )?;
    hit.at
        .get_mut(1..count)?
        .copy_from_slice(fill.get(..rest)?);
    hit.input = count;
    hit.end = end;
    hit.records = at.checked_add(rest.checked_mul(2)?)?;
    hit.count = records;
    Some(hit)
}

/// Match a `ChainedSequenceRule` or `ChainedClassSequenceRule` starting at `i`.
///
/// The three parts have three different `By`s because format 2 gives backtrack,
/// input and lookahead their own ClassDefs: a glyph may be class 3 as a
/// lookahead and class 1 as an input, which is how a font expresses "any vowel
/// follows" without listing them.
fn chain_rule(
    data: &[u8],
    rule: usize,
    by: (By, By, By),
    glyphs: &[SubGlyph],
    i: usize,
    skip: Skipper<'_>,
) -> Option<Matched> {
    let (back_by, in_by, ahead_by) = by;
    // Backtrack and lookahead describe the neighbourhood, not the thing being
    // rewritten, so they skip what the flag skips but are not gated by the
    // features. See the `skip` module doc.
    let context = skip.context();

    let back = usize::from(u16_at(data, rule)?);
    if back > MAX_CONTEXT {
        return None;
    }
    let at = rule.checked_add(2)?;
    backward(data, at, back, back_by, glyphs, i, context)?;

    let at = at.checked_add(back.checked_mul(2)?)?;
    let count = usize::from(u16_at(data, at)?);
    if count == 0 || count > MAX_CONTEXT {
        return None;
    }
    let mut hit = Matched::blank();
    *hit.at.get_mut(0)? = i;
    let rest = count.checked_sub(1)?;
    let at = at.checked_add(2)?;
    let mut fill = [0usize; MAX_CONTEXT];
    let end = forward(
        data,
        at,
        rest,
        in_by,
        glyphs,
        i.checked_add(1)?,
        skip,
        |k, pos| {
            if let Some(slot) = fill.get_mut(k) {
                *slot = pos;
            }
        },
    )?;
    hit.at
        .get_mut(1..count)?
        .copy_from_slice(fill.get(..rest)?);

    let at = at.checked_add(rest.checked_mul(2)?)?;
    let ahead = usize::from(u16_at(data, at)?);
    if ahead > MAX_CONTEXT {
        return None;
    }
    let at = at.checked_add(2)?;
    forward(data, at, ahead, ahead_by, glyphs, end, context, |_, _| {})?;

    let at = at.checked_add(ahead.checked_mul(2)?)?;
    hit.input = count;
    hit.end = end;
    hit.records = at.checked_add(2)?;
    hit.count = usize::from(u16_at(data, at)?);
    Some(hit)
}

/// Match one `SequenceContext` subtable at `i`, in any of its three formats.
///
/// Returns the matched input length, where its lookup records are, and how
/// many there are.
pub(crate) fn context_match(
    data: &[u8],
    sub: usize,
    glyphs: &[SubGlyph],
    i: usize,
    skip: Skipper<'_>,
    rules: &mut Vec<usize>,
) -> Option<Matched> {
    let gid = glyphs.get(i)?.gid;
    match u16_at(data, sub)? {
        // Format 1: rules keyed by the first glyph, listed by id. The most
        // direct form and the least compact, so fonts use it for the handful
        // of contexts that do not generalise.
        1 => {
            let coverage = sub_offset(data, sub, sub.checked_add(2)?)?;
            let index = coverage_index(data, coverage, gid)?;
            let count = u16_at(data, sub.checked_add(4)?)?;
            let set = rule_set(data, sub, sub.checked_add(6)?, count, index)?;
            read_rules(data, set, rules);
            rules
                .iter()
                .find_map(|&rule| seq_rule(data, rule, By::Glyph, glyphs, i, skip))
        }
        // Format 2: rules keyed by the first glyph's *class*, so one rule
        // covers every glyph that behaves alike. The coverage is only a gate —
        // it says which glyphs are worth computing a class for.
        2 => {
            let coverage = sub_offset(data, sub, sub.checked_add(2)?)?;
            coverage_index(data, coverage, gid)?;
            let classdef = sub_offset(data, sub, sub.checked_add(4)?)?;
            let class = glyph_class(data, classdef, gid)?;
            let count = u16_at(data, sub.checked_add(6)?)?;
            let set = rule_set(data, sub, sub.checked_add(8)?, count, class)?;
            read_rules(data, set, rules);
            rules
                .iter()
                .find_map(|&rule| seq_rule(data, rule, By::Class(classdef), glyphs, i, skip))
        }
        // Format 3: a single context, each position given its own coverage
        // table. No rule sets, so nothing is keyed by the first glyph.
        3 => {
            let count = usize::from(u16_at(data, sub.checked_add(2)?)?);
            let records = usize::from(u16_at(data, sub.checked_add(4)?)?);
            if count == 0 || count > MAX_CONTEXT {
                return None;
            }
            let at = sub.checked_add(6)?;
            let mut hit = Matched::blank();
            let mut fill = [0usize; MAX_CONTEXT];
            hit.end = forward_covered(data, sub, at, count, glyphs, i, skip, |k, pos| {
                if let Some(slot) = fill.get_mut(k) {
                    *slot = pos;
                }
            })?;
            hit.at.get_mut(..count)?.copy_from_slice(fill.get(..count)?);
            hit.input = count;
            hit.records = at.checked_add(count.checked_mul(2)?)?;
            hit.count = records;
            Some(hit)
        }
        _ => None,
    }
}

/// Match one `ChainedSequenceContext` subtable at `i`, in any of its three
/// formats.
pub(crate) fn chain_match(
    data: &[u8],
    sub: usize,
    glyphs: &[SubGlyph],
    i: usize,
    skip: Skipper<'_>,
    rules: &mut Vec<usize>,
) -> Option<Matched> {
    let gid = glyphs.get(i)?.gid;
    match u16_at(data, sub)? {
        1 => {
            let coverage = sub_offset(data, sub, sub.checked_add(2)?)?;
            let index = coverage_index(data, coverage, gid)?;
            let count = u16_at(data, sub.checked_add(4)?)?;
            let set = rule_set(data, sub, sub.checked_add(6)?, count, index)?;
            read_rules(data, set, rules);
            let by = (By::Glyph, By::Glyph, By::Glyph);
            rules
                .iter()
                .find_map(|&rule| chain_rule(data, rule, by, glyphs, i, skip))
        }
        2 => {
            let coverage = sub_offset(data, sub, sub.checked_add(2)?)?;
            coverage_index(data, coverage, gid)?;
            let back = sub_offset(data, sub, sub.checked_add(4)?)?;
            let input = sub_offset(data, sub, sub.checked_add(6)?)?;
            let ahead = sub_offset(data, sub, sub.checked_add(8)?)?;
            let class = glyph_class(data, input, gid)?;
            let count = u16_at(data, sub.checked_add(10)?)?;
            let set = rule_set(data, sub, sub.checked_add(12)?, count, class)?;
            read_rules(data, set, rules);
            let by = (By::Class(back), By::Class(input), By::Class(ahead));
            rules
                .iter()
                .find_map(|&rule| chain_rule(data, rule, by, glyphs, i, skip))
        }
        // Format 3 has no coverage gate of its own: the first input coverage
        // is the gate.
        3 => {
            let context = skip.context();
            let at = sub.checked_add(2)?;
            let back = usize::from(u16_at(data, at)?);
            if back > MAX_CONTEXT {
                return None;
            }
            let at = at.checked_add(2)?;
            backward_covered(data, sub, at, back, glyphs, i, context)?;

            let at = at.checked_add(back.checked_mul(2)?)?;
            let count = usize::from(u16_at(data, at)?);
            if count == 0 || count > MAX_CONTEXT {
                return None;
            }
            let at = at.checked_add(2)?;
            let mut fill = [0usize; MAX_CONTEXT];
            let end = forward_covered(data, sub, at, count, glyphs, i, skip, |k, pos| {
                if let Some(slot) = fill.get_mut(k) {
                    *slot = pos;
                }
            })?;

            let at = at.checked_add(count.checked_mul(2)?)?;
            let ahead = usize::from(u16_at(data, at)?);
            if ahead > MAX_CONTEXT {
                return None;
            }
            let at = at.checked_add(2)?;
            forward_covered(data, sub, at, ahead, glyphs, end, context, |_, _| {})?;

            let at = at.checked_add(ahead.checked_mul(2)?)?;
            let mut hit = Matched::blank();
            hit.at.get_mut(..count)?.copy_from_slice(fill.get(..count)?);
            hit.input = count;
            hit.end = end;
            hit.records = at.checked_add(2)?;
            hit.count = usize::from(u16_at(data, at)?);
            Some(hit)
        }
        _ => None,
    }
}
