//! `GPOS`: the positioning half of OpenType Layout, applied as one pass.
//!
//! # Why one pass, and not one pass per thing being adjusted
//!
//! `GPOS` has eight lookup types and they are not independent. A cursive
//! attachment moves a letter onto the previous letter's exit stroke; a mark
//! attachment then hangs a vowel sign off the letter *where it ended up*; a
//! single adjustment may nudge either of them afterwards. The table says which
//! order that happens in — LookupList order, the same rule `GSUB` follows — and
//! a reader that applies kerning in one place and mark anchors in another has
//! no way to honour it. It also has no way to honour a `lookupFlag`, because
//! the flag belongs to the lookup rather than to the subtable, and a pass that
//! picks subtables out of the table by type has already thrown the lookup away.
//!
//! So: every lookup a run's features reach is applied in order, over the whole
//! run, through the same [`Skipper`] that `GSUB` uses.
//!
//! # The model
//!
//! One [`Adjust`] per glyph, in font units, holding what the glyph's advance
//! became and how far its image is displaced from the pen. That is HarfBuzz's
//! `hb_glyph_position_t`, and it is deliberately the same: this crate's output
//! is checked glyph-for-glyph against HarfBuzz's, so where the spec leaves room
//! the answer has to be the one HarfBuzz picked.
//!
//! Attachments are not resolved as they are made. A mark attached to a base
//! records *which* glyph it hangs off, as a relative index, and the offset it
//! wants relative to that glyph's origin. Only when every lookup has run does
//! [`propagate`] walk those chains and turn them into offsets from each glyph's
//! own pen — because until then the base may still move, and a mark stacked on
//! a mark stacked on a base needs the whole chain settled from the root down.
//!
//! # What is not here
//!
//! Types 5 (mark-to-ligature), 7 (contextual) and 8 (chained contextual). See
//! `TD-GPOS-HAS-NO-CONTEXTUAL-OR-MARK-TO-LIGATURE-POSITIONING`. Device tables
//! are read past: they tune a value for one specific ppem, and the value
//! without them is the designer's intent at every size.

use alloc::vec::Vec;

use crate::gsub::SubGlyph;
use crate::mark::{attachment, lig_attachment};
use crate::otl::{
    ByScript, Lookup, binary_search, coverage_index, glyph_class, value_size,
};
use crate::script::ScriptTags;
use crate::sfnt::{Span, i16_at, u16_at};
use crate::skip::{CLASS_MARK, Definitions, IGNORE_FLAGS, IGNORE_MARKS, Skipper};

/// Single adjustment: move one glyph and change its advance.
const SINGLE_POS: u16 = 1;
/// Pair adjustment: kerning, and anything else expressed about two glyphs.
const PAIR_POS: u16 = 2;
/// Cursive attachment: join one glyph's exit stroke to the next one's entry.
const CURSIVE_POS: u16 = 3;
/// Mark-to-base attachment.
const MARK_BASE_POS: u16 = 4;
/// Mark-to-ligature attachment: like mark-to-base, but the glyph below offers
/// one set of attachment points per component it swallowed.
const MARK_LIG_POS: u16 = 5;
/// Mark-to-mark attachment.
const MARK_MARK_POS: u16 = 6;
/// Extension: a subtable of another type behind a 32-bit offset, so that a
/// large table can exceed the 16-bit offsets used everywhere else. `GSUB`
/// numbers its own extension differently, which is why the shared walk takes
/// it as a parameter.
const EXTENSION_POS: u16 = 9;

/// The lookup types this module can apply, in the order [`ByScript`] wants
/// them: as a set, not a sequence — application order comes from the
/// LookupList, not from here.
const KINDS: [u16; 6] = [
    SINGLE_POS,
    PAIR_POS,
    CURSIVE_POS,
    MARK_BASE_POS,
    MARK_LIG_POS,
    MARK_MARK_POS,
];

/// The positioning features applied to every run.
///
/// HarfBuzz's unconditional `GPOS` set, minus the vertical ones this crate has
/// no layout for. All of them are on for every run: unlike `GSUB`, where the
/// Arabic positional features must reach only the glyphs the shaper marked
/// eligible, a positioning feature is gated by its own glyph coverage — a
/// face's `abvm` simply does not cover glyphs that have nothing above them.
const FEATURES: [&[u8; 4]; 7] = [
    b"abvm", b"blwm", b"curs", b"dist", b"kern", b"mark", b"mkmk",
];

/// `lookupFlag` bit 0. On a cursive lookup it says which end of the join is
/// anchored — not, despite the name, which way the text runs.
const RIGHT_TO_LEFT: u16 = 0x0001;

/// How deep an attachment chain may be followed before it is treated as a
/// cycle. A real chain is a mark on a mark on a base, so three; a font that
/// nests further is either doing something exotic or is hostile, and either way
/// stopping is better than looping.
const MAX_CHAIN: usize = 64;

/// Value-record flags, in the order the fields appear in the record.
const X_PLACEMENT: u16 = 0x0001;
const Y_PLACEMENT: u16 = 0x0002;
const X_ADVANCE: u16 = 0x0004;
const Y_ADVANCE: u16 = 0x0008;

/// One `ValueRecord`, with the device-table corrections dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Value {
    pub(crate) x_placement: i16,
    pub(crate) y_placement: i16,
    pub(crate) x_advance: i16,
    pub(crate) y_advance: i16,
}

impl Value {
    /// Read the record at `at`, whose fields are the ones `format` names.
    ///
    /// The fields are stored in flag order with the absent ones simply missing,
    /// so a field's offset within the record is the size of a record holding
    /// only the flags *below* it — which is what `bit - 1` masks out.
    fn read(data: &[u8], at: usize, format: u16) -> Self {
        let field = |bit: u16| -> i16 {
            if format & bit == 0 {
                return 0;
            }
            let skip = value_size(format & bit.wrapping_sub(1));
            at.checked_add(skip)
                .and_then(|o| i16_at(data, o))
                .unwrap_or(0)
        };
        Self {
            x_placement: field(X_PLACEMENT),
            y_placement: field(Y_PLACEMENT),
            x_advance: field(X_ADVANCE),
            y_advance: field(Y_ADVANCE),
        }
    }
}

/// What a glyph is hanging off, if anything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Attach {
    /// Free-standing: its offset is already relative to its own pen.
    #[default]
    Free,
    /// A mark placed against the glyph its chain points at.
    Mark,
    /// A letter joined to the glyph its chain points at.
    Cursive,
}

/// Where one glyph ended up, in font units.
///
/// The advance is absolute rather than a correction, because cursive
/// attachment *assigns* an advance rather than adding to one: a joined letter's
/// width is the distance to its exit stroke, whatever `hmtx` said.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Adjust {
    /// Horizontal displacement of the glyph's image from the pen.
    pub(crate) x_offset: i32,
    /// Vertical displacement, positive upwards.
    pub(crate) y_offset: i32,
    /// The advance to charge for this glyph.
    pub(crate) x_advance: i32,
    /// The vertical advance, which horizontal layout ignores but which the
    /// value records still carry.
    pub(crate) y_advance: i32,
    /// How much of `x_advance` came from a *pair*, and so belongs to the gap
    /// between this glyph and the next rather than to the glyph itself.
    ///
    /// Split out because reordering a right-to-left run moves the gap to the
    /// other glyph of the pair while leaving each glyph's own width alone —
    /// see `recharge_kerns` in [`scaled`](crate::scaled).
    pub(crate) kern: i32,
    /// Relative index of the glyph this one hangs off; `0` for none.
    chain: i32,
    /// What kind of attachment `chain` describes.
    kind: Attach,
}

impl Adjust {
    /// A glyph at its nominal advance and no displacement.
    ///
    /// What a glyph no lookup touched ends up with, and so also the seed the
    /// pass starts from and the filler a caller uses for the glyphs — tabs,
    /// and anything outside a positioned run — the pass never sees.
    pub(crate) fn plain(advance: i32) -> Self {
        Self {
            x_advance: advance,
            ..Self::default()
        }
    }

    /// Apply a value record to this glyph.
    fn add(&mut self, v: Value) {
        self.x_offset = self.x_offset.saturating_add(i32::from(v.x_placement));
        self.y_offset = self.y_offset.saturating_add(i32::from(v.y_placement));
        self.x_advance = self.x_advance.saturating_add(i32::from(v.x_advance));
        self.y_advance = self.y_advance.saturating_add(i32::from(v.y_advance));
    }
}

/// One stretch of glyphs to position together, and what is known about it.
pub(crate) struct Run<'a> {
    /// The glyphs, in logical order — which is the order `GPOS` is defined
    /// over, whichever way the run reads.
    pub(crate) glyphs: &'a [SubGlyph],
    /// Each glyph's `hmtx` advance, in font units.
    pub(crate) advances: &'a [i32],
    /// Whether each glyph is one whose advance must be zeroed because it is a
    /// combining mark. Decided by the caller, which knows the face; a mark's
    /// width is dropped after the lookups have run and before the attachment
    /// chains are resolved, so that a mark between a base and another mark
    /// contributes nothing to the distance between them.
    pub(crate) marks: &'a [bool],
    /// Whether the run reads right to left.
    pub(crate) rtl: bool,
    /// The run's script, which selects the lookups.
    pub(crate) script: Option<ScriptTags>,
}

/// A face's `GPOS`, resolved once per script.
#[derive(Clone, Debug)]
pub(crate) struct Positioning {
    lookups: ByScript,
    defs: Definitions,
}

impl Positioning {
    /// Resolve the positioning lookups of a face that has a `GPOS` table.
    ///
    /// `None` when no script reaches a lookup of a type this module applies,
    /// which is a normal answer: a face whose whole `GPOS` is contextual has
    /// nothing here to run.
    pub(crate) fn parse(data: &[u8], gpos: Span, gdef: Option<Span>) -> Option<Self> {
        let lookups = ByScript::parse(data, gpos.off, &FEATURES, &KINDS, EXTENSION_POS)?;
        Some(Self {
            lookups,
            defs: Definitions::parse(data, gdef),
        })
    }

    /// Position one run, returning one adjustment per glyph.
    pub(crate) fn apply(&self, data: &[u8], run: &Run<'_>) -> Vec<Adjust> {
        let mut out: Vec<Adjust> = (0..run.glyphs.len())
            .map(|i| Adjust::plain(run.advances.get(i).copied().unwrap_or(0)))
            .collect();
        for (lookup, _) in self.lookups.for_script(run.script) {
            self.run_lookup(data, lookup, run, &mut out);
        }
        // Marks lose their width here, between the lookups and the chains, for
        // the same reason and at the same point HarfBuzz does it: an anchored
        // mark's offset is measured back from its base across the advances in
        // between, and a mark that still had a width would push every mark
        // after it off the letter.
        for (adjust, &mark) in out.iter_mut().zip(run.marks.iter()) {
            if mark {
                adjust.x_advance = 0;
                adjust.y_advance = 0;
                adjust.kern = 0;
            }
        }
        propagate(&mut out, run.rtl);
        out
    }

    /// Apply one lookup across the whole run.
    ///
    /// The walk is HarfBuzz's: try the subtables in order at the current
    /// position, first match winning, and let the match say where to resume —
    /// a pair that consumed its second glyph resumes past it, everything else
    /// resumes at the next glyph.
    fn run_lookup(&self, data: &[u8], lookup: &Lookup, run: &Run<'_>, out: &mut [Adjust]) {
        let skip = Skipper::new(data, self.defs, lookup.flag, lookup.filter, u32::MAX);
        let mut i = 0usize;
        while i < run.glyphs.len() {
            let next = if skip.considers(run.glyphs, i) {
                lookup
                    .subtables
                    .iter()
                    .copied()
                    .find_map(|sub| self.one(data, lookup, sub, run, i, out))
            } else {
                None
            };
            // A subtable that reports a resume position at or before where it
            // started would spin here forever, which a malformed font could
            // otherwise arrange.
            i = next
                .filter(|&n| n > i)
                .unwrap_or_else(|| i.saturating_add(1));
        }
    }

    /// Try one subtable at one position, reporting where to resume.
    fn one(
        &self,
        data: &[u8],
        lookup: &Lookup,
        sub: usize,
        run: &Run<'_>,
        i: usize,
        out: &mut [Adjust],
    ) -> Option<usize> {
        match lookup.kind {
            SINGLE_POS => single(data, sub, run.glyphs, i, out),
            PAIR_POS => {
                let skip = Skipper::new(data, self.defs, lookup.flag, lookup.filter, u32::MAX);
                pair(data, sub, skip, run.glyphs, i, out)
            }
            CURSIVE_POS => {
                let skip = Skipper::new(data, self.defs, lookup.flag, lookup.filter, u32::MAX);
                cursive(data, sub, skip, lookup.flag, run, i, out)
            }
            MARK_BASE_POS => {
                // The base is found with a fixed `IgnoreMarks`, not with the
                // lookup's own flag: a mark-to-base lookup that did not ignore
                // marks would attach the second accent of a stack to the first
                // and call it a base.
                let skip = Skipper::new(data, self.defs, IGNORE_MARKS, 0, u32::MAX);
                let j = skip.prev(run.glyphs, i)?;
                attach(data, sub, run.glyphs, i, j, out)
            }
            MARK_LIG_POS => {
                // Same search as mark-to-base, for the same reason.
                let skip = Skipper::new(data, self.defs, IGNORE_MARKS, 0, u32::MAX);
                let j = skip.prev(run.glyphs, i)?;
                attach_to_lig(data, sub, run.glyphs, i, j, out)
            }
            MARK_MARK_POS => {
                // Here the lookup's flag is kept except for the three "ignore"
                // bits, so that a mark-attachment class or a filtering set
                // still selects which marks may stack — but nothing is stepped
                // over, because the mark below is the one immediately below.
                let skip = Skipper::new(
                    data,
                    self.defs,
                    lookup.flag & !IGNORE_FLAGS,
                    lookup.filter,
                    u32::MAX,
                );
                let j = skip.prev(run.glyphs, i)?;
                let below = run.glyphs.get(j)?.gid;
                if self.defs.class(data, below) != CLASS_MARK {
                    return None;
                }
                attach(data, sub, run.glyphs, i, j, out)
            }
            _ => None,
        }
    }
}

/// Type 1: move one glyph, and change its advance.
fn single(
    data: &[u8],
    sub: usize,
    glyphs: &[SubGlyph],
    i: usize,
    out: &mut [Adjust],
) -> Option<usize> {
    let gid = glyphs.get(i)?.gid;
    let coverage = sub.checked_add(usize::from(u16_at(data, sub.checked_add(2)?)?))?;
    let index = coverage_index(data, coverage, gid)?;
    let format = u16_at(data, sub.checked_add(4)?)?;
    let at = match u16_at(data, sub)? {
        // One record shared by every covered glyph.
        1 => sub.checked_add(6)?,
        // One record per covered glyph, in coverage order.
        2 => {
            if index >= u16_at(data, sub.checked_add(6)?)? {
                return None;
            }
            sub.checked_add(8)?
                .checked_add(usize::from(index).checked_mul(value_size(format))?)?
        }
        _ => return None,
    };
    out.get_mut(i)?.add(Value::read(data, at, format));
    i.checked_add(1)
}

/// Type 2: adjust a glyph and the next one the lookup can see.
fn pair(
    data: &[u8],
    sub: usize,
    skip: Skipper<'_>,
    glyphs: &[SubGlyph],
    i: usize,
    out: &mut [Adjust],
) -> Option<usize> {
    let left = glyphs.get(i)?.gid;
    let j = skip.next(glyphs, i)?;
    let right = glyphs.get(j)?.gid;
    let (first, second, has_second) = pair_values(data, sub, left, right)?;
    if let Some(adjust) = out.get_mut(i) {
        adjust.add(first);
        adjust.kern = adjust.kern.saturating_add(i32::from(first.x_advance));
    }
    if has_second {
        out.get_mut(j)?.add(second);
        // The second glyph has been positioned, so the next pair starts after
        // it. A subtable that carries no second record has not touched it, and
        // it may still be the first half of the following pair.
        return j.checked_add(1);
    }
    Some(j)
}

/// Both value records of one `PairPos` subtable, and whether the second exists.
///
/// Public to the crate because the pair-at-a-time kerning interface reads the
/// same subtables — see [`kern`](crate::kern). It has no run to walk and so
/// cannot use the pass, but it must not decode the subtables differently.
pub(crate) fn pair_values(
    data: &[u8],
    sub: usize,
    left: u16,
    right: u16,
) -> Option<(Value, Value, bool)> {
    let format = u16_at(data, sub)?;
    let coverage = sub.checked_add(usize::from(u16_at(data, sub.checked_add(2)?)?))?;
    let index = coverage_index(data, coverage, left)?;
    let value1 = u16_at(data, sub.checked_add(4)?)?;
    let value2 = u16_at(data, sub.checked_add(6)?)?;
    let at = match format {
        1 => pair_record_1(data, sub, index, right, value1, value2)?,
        2 => pair_record_2(data, sub, left, right, value1, value2)?,
        _ => return None,
    };
    Some((
        Value::read(data, at, value1),
        Value::read(data, at.checked_add(value_size(value1))?, value2),
        value2 != 0,
    ))
}

/// Format 1: one explicit list of second glyphs per covered first glyph.
///
/// Returns the offset of the pair's value records, which begin just past the
/// second glyph id.
fn pair_record_1(
    data: &[u8],
    sub: usize,
    index: u16,
    right: u16,
    value1: u16,
    value2: u16,
) -> Option<usize> {
    if index >= u16_at(data, sub.checked_add(8)?)? {
        return None;
    }
    let at = sub
        .checked_add(10)?
        .checked_add(usize::from(index).checked_mul(2)?)?;
    let set = sub.checked_add(usize::from(u16_at(data, at)?))?;
    let pairs = u16_at(data, set)?;
    let stride = 2usize
        .checked_add(value_size(value1))?
        .checked_add(value_size(value2))?;
    let records = set.checked_add(2)?;

    // The second glyphs are sorted, which is what makes a font with thousands
    // of pairs per glyph affordable to look up.
    let found = binary_search(usize::from(pairs), |i| {
        let rec = records.checked_add(i.checked_mul(stride)?)?;
        Some(u16_at(data, rec)?.cmp(&right))
    })?;
    records
        .checked_add(found.checked_mul(stride)?)?
        .checked_add(2)
}

/// Format 2: a grid indexed by two glyph classes, which is how a font expresses
/// "every capital before every round lowercase" without listing the product of
/// the two sets.
fn pair_record_2(
    data: &[u8],
    sub: usize,
    left: u16,
    right: u16,
    value1: u16,
    value2: u16,
) -> Option<usize> {
    let class_def1 = sub.checked_add(usize::from(u16_at(data, sub.checked_add(8)?)?))?;
    let class_def2 = sub.checked_add(usize::from(u16_at(data, sub.checked_add(10)?)?))?;
    let class1_count = u16_at(data, sub.checked_add(12)?)?;
    let class2_count = u16_at(data, sub.checked_add(14)?)?;
    let c1 = glyph_class(data, class_def1, left)?;
    let c2 = glyph_class(data, class_def2, right)?;
    if c1 >= class1_count || c2 >= class2_count {
        return None;
    }
    let stride = value_size(value1).checked_add(value_size(value2))?;
    let cell = usize::from(c1)
        .checked_mul(usize::from(class2_count))?
        .checked_add(usize::from(c2))?
        .checked_mul(stride)?;
    sub.checked_add(16)?.checked_add(cell)
}

/// Type 3: join one glyph's exit stroke to the next one's entry.
///
/// Two separate effects, which is why this is longer than the others. Along the
/// writing direction the join *sets* the advances, so that the pen lands
/// exactly on the entry point. Across it the two glyphs are attached, and which
/// of them moves is what the `RightToLeft` lookup flag decides: it names the
/// end of the join that stays on the baseline, so with the flag clear the
/// *second* glyph is the anchor and the first swings up to meet it.
fn cursive(
    data: &[u8],
    sub: usize,
    skip: Skipper<'_>,
    flag: u16,
    run: &Run<'_>,
    i: usize,
    out: &mut [Adjust],
) -> Option<usize> {
    if u16_at(data, sub)? != 1 {
        return None;
    }
    let coverage = sub.checked_add(usize::from(u16_at(data, sub.checked_add(2)?)?))?;
    let count = u16_at(data, sub.checked_add(4)?)?;
    let record = |glyph: u16| -> Option<usize> {
        let index = coverage_index(data, coverage, glyph)?;
        if index >= count {
            return None;
        }
        sub.checked_add(6)?
            .checked_add(usize::from(index).checked_mul(4)?)
    };

    let this = record(run.glyphs.get(i)?.gid)?;
    let exit = crate::mark::anchor(data, sub, u16_at(data, this.checked_add(2)?)?)?;
    let j = skip.next(run.glyphs, i)?;
    let next = record(run.glyphs.get(j)?.gid)?;
    let entry = crate::mark::anchor(data, sub, u16_at(data, next)?)?;

    let (exit_x, exit_y) = (i32::from(exit.0), i32::from(exit.1));
    let (entry_x, entry_y) = (i32::from(entry.0), i32::from(entry.1));

    // Along the writing direction: the trailing glyph's advance is cut back to
    // its exit point and the leading glyph's origin is pulled to its entry, so
    // that the two strokes meet however wide `hmtx` thought the glyphs were.
    if run.rtl {
        let d = exit_x.saturating_add(out.get(i)?.x_offset);
        if let Some(a) = out.get_mut(i) {
            a.x_advance = a.x_advance.saturating_sub(d);
            a.x_offset = a.x_offset.saturating_sub(d);
        }
        if let Some(a) = out.get_mut(j) {
            a.x_advance = entry_x.saturating_add(a.x_offset);
        }
    } else {
        if let Some(a) = out.get_mut(i) {
            a.x_advance = exit_x.saturating_add(a.x_offset);
        }
        let d = entry_x.saturating_add(out.get(j)?.x_offset);
        if let Some(a) = out.get_mut(j) {
            a.x_advance = a.x_advance.saturating_sub(d);
            a.x_offset = a.x_offset.saturating_sub(d);
        }
    }

    // Across it: one of the pair hangs off the other.
    let (child, parent, y_offset) = if flag & RIGHT_TO_LEFT == 0 {
        (j, i, exit_y.saturating_sub(entry_y))
    } else {
        (i, j, entry_y.saturating_sub(exit_y))
    };
    // The child may already be the anchor of an earlier join. Turning the old
    // chain around rather than dropping it keeps everything that hung off it
    // hanging off the new parent, which is what makes a long cursive word one
    // connected tree instead of several.
    reverse_chain(out, child, parent);
    if let Some(a) = out.get_mut(child) {
        a.kind = Attach::Cursive;
        a.chain = delta(parent, child);
        a.y_offset = y_offset;
    }
    // If the parent was hanging off the child, the two would now point at each
    // other and neither would ever reach a root.
    let back = out.get(child).map_or(0, |a| a.chain);
    if let Some(a) = out.get_mut(parent)
        && a.chain == back.saturating_neg()
        && back != 0
    {
        a.chain = 0;
    }
    i.checked_add(1)
}

/// Types 4 and 6: hang the mark at `i` off the glyph at `j`.
///
/// The two subtable shapes are identical — see
/// [`attachment`](crate::mark::attachment) — so only the choice of `j` differs,
/// and that is made by the caller.
fn attach(
    data: &[u8],
    sub: usize,
    glyphs: &[SubGlyph],
    i: usize,
    j: usize,
    out: &mut [Adjust],
) -> Option<usize> {
    let (dx, dy) = attachment(data, sub, glyphs.get(j)?.gid, glyphs.get(i)?.gid)?;
    let adjust = out.get_mut(i)?;
    // Assignment, not accumulation: an anchor pair states where the mark goes,
    // and a second lookup that also has an anchor for it is overriding the
    // first rather than adding to it.
    adjust.x_offset = i32::from(dx);
    adjust.y_offset = i32::from(dy);
    adjust.kind = Attach::Mark;
    adjust.chain = delta(j, i);
    i.checked_add(1)
}

/// Type 5: hang the mark at `i` off the *component* of the ligature at `j`
/// that it belongs to.
///
/// Which component that is was decided during substitution, not here: the
/// ligature and the mark each carry a [`Lig`](crate::gsub::Lig), and the mark
/// belongs to a component of *this* ligature only when the two ids agree. When
/// they do not — the mark came from somewhere else, or nothing ligated and the
/// font is simply using type 5 where type 4 would have done — the component is
/// left unknown and [`lig_attachment`] falls back to the last one.
fn attach_to_lig(
    data: &[u8],
    sub: usize,
    glyphs: &[SubGlyph],
    i: usize,
    j: usize,
    out: &mut [Adjust],
) -> Option<usize> {
    let mark = *glyphs.get(i)?;
    let lig = *glyphs.get(j)?;
    let component = if lig.lig.id != 0 && lig.lig.id == mark.lig.id {
        mark.lig.comp()
    } else {
        0
    };
    let (dx, dy) = lig_attachment(data, sub, lig.gid, mark.gid, component)?;
    let adjust = out.get_mut(i)?;
    adjust.x_offset = i32::from(dx);
    adjust.y_offset = i32::from(dy);
    adjust.kind = Attach::Mark;
    adjust.chain = delta(j, i);
    i.checked_add(1)
}

/// `to - from` as the relative index an attachment chain stores.
fn delta(to: usize, from: usize) -> i32 {
    let to = i32::try_from(to).unwrap_or(i32::MAX);
    let from = i32::try_from(from).unwrap_or(i32::MAX);
    to.saturating_sub(from)
}

/// `at + chain`, or `None` if that is not a position.
fn step(at: usize, chain: i32) -> Option<usize> {
    let at = i32::try_from(at).ok()?;
    usize::try_from(at.checked_add(chain)?).ok()
}

/// Turn the cursive chain hanging off `from` around, so that it hangs off the
/// other way, stopping if it reaches `stop`.
///
/// HarfBuzz recurses here; this walks the chain into a list and unwinds it,
/// because the depth is the length of a cursive word and a kernel-side
/// rasterizer should not put that on the stack.
fn reverse_chain(out: &mut [Adjust], from: usize, stop: usize) {
    let mut path: Vec<(usize, usize, i32, Attach)> = Vec::new();
    let mut at = from;
    while path.len() < MAX_CHAIN {
        let Some(a) = out.get_mut(at) else { break };
        if a.chain == 0 || a.kind != Attach::Cursive {
            break;
        }
        let (chain, kind) = (a.chain, a.kind);
        let Some(next) = step(at, chain) else { break };
        a.chain = 0;
        if next == stop {
            break;
        }
        path.push((at, next, chain, kind));
        at = next;
    }
    for &(at, next, chain, kind) in path.iter().rev() {
        let offset = out.get(at).map_or(0, |a| a.y_offset);
        if let Some(a) = out.get_mut(next) {
            a.y_offset = offset.saturating_neg();
            a.chain = chain.saturating_neg();
            a.kind = kind;
        }
    }
}

/// Turn every attachment chain into an offset from the glyph's own pen.
///
/// A mark records where it wants to be relative to its base's *origin*, but it
/// is drawn at its own pen — which is however far the line has moved since. A
/// cursive child records a displacement relative to its parent, which may
/// itself be displaced. Both are resolved from the root of the chain outwards,
/// which is why this walks up to the root before it applies anything.
fn propagate(out: &mut [Adjust], rtl: bool) {
    for start in 0..out.len() {
        // Every chain visited is cleared as it is walked, so a glyph reached
        // as part of an earlier chain costs one test here and nothing more.
        let mut path: Vec<(usize, usize, Attach)> = Vec::new();
        let mut at = start;
        while path.len() < MAX_CHAIN {
            let Some(a) = out.get_mut(at) else { break };
            if a.chain == 0 {
                break;
            }
            let (chain, kind) = (a.chain, a.kind);
            let Some(next) = step(at, chain) else { break };
            a.chain = 0;
            path.push((at, next, kind));
            at = next;
        }
        for &(i, j, kind) in path.iter().rev() {
            resolve(out, i, j, kind, rtl);
        }
    }
}

/// Add the parent's settled position into the child's.
fn resolve(out: &mut [Adjust], i: usize, j: usize, kind: Attach, rtl: bool) {
    let parent = out.get(j).copied().unwrap_or_default();
    match kind {
        // A cursive join only ever moved the child across the writing
        // direction, so only that axis is inherited.
        Attach::Cursive => {
            if let Some(a) = out.get_mut(i) {
                a.y_offset = a.y_offset.saturating_add(parent.y_offset);
            }
        }
        Attach::Mark => {
            // The pen has travelled from the base to here, and the mark's
            // offset was measured from the base, so the travel comes back off.
            // Which glyphs are travelled over depends on the direction: in a
            // right-to-left run the pen reaches the mark *before* the base, so
            // the travel is added rather than subtracted and the base's own
            // advance is part of it.
            let (mut dx, mut dy) = (0i32, 0i32);
            let span = if rtl {
                j.saturating_add(1)..i.saturating_add(1)
            } else {
                j..i
            };
            for k in span {
                let Some(a) = out.get(k) else { break };
                dx = dx.saturating_add(a.x_advance);
                dy = dy.saturating_add(a.y_advance);
            }
            if let Some(a) = out.get_mut(i) {
                a.x_offset = a.x_offset.saturating_add(parent.x_offset);
                a.y_offset = a.y_offset.saturating_add(parent.y_offset);
                if rtl {
                    a.x_offset = a.x_offset.saturating_add(dx);
                    a.y_offset = a.y_offset.saturating_add(dy);
                } else {
                    a.x_offset = a.x_offset.saturating_sub(dx);
                    a.y_offset = a.y_offset.saturating_sub(dy);
                }
            }
        }
        Attach::Free => {}
    }
}

#[cfg(test)]
// A test that indexes past the end of its own fixture *should* panic — that is
// the failure being reported, not a defect to guard against.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]
mod tests {
    use super::*;

    fn be16(v: u16) -> [u8; 2] {
        v.to_be_bytes()
    }

    fn glyphs(ids: &[u16]) -> Vec<SubGlyph> {
        ids.iter()
            .enumerate()
            .map(|(i, &g)| SubGlyph::new(g, i))
            .collect()
    }

    /// Coverage format 1 over a sorted glyph list.
    fn coverage1(ids: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(u16::try_from(ids.len()).unwrap()));
        for g in ids {
            out.extend_from_slice(&be16(*g));
        }
        out
    }

    #[test]
    fn a_value_record_finds_its_fields_whichever_are_present() {
        // XPlacement and XAdvance only: two fields, XAdvance second.
        let data = [0x00, 0x0A, 0xFF, 0xF6];
        let v = Value::read(&data, 0, X_PLACEMENT | X_ADVANCE);
        assert_eq!(v.x_placement, 10);
        assert_eq!(v.x_advance, -10);
        assert_eq!(v.y_placement, 0);
        assert_eq!(v.y_advance, 0);
        // The same bytes read as YAdvance alone put the first field there.
        let v = Value::read(&data, 0, Y_ADVANCE);
        assert_eq!(v.y_advance, 10);
        assert_eq!(v.x_advance, 0);
    }

    /// A `SinglePosFormat1` subtable over `ids`, applying `value`.
    fn single_pos1(ids: &[u16], value: Value) -> Vec<u8> {
        let cov = coverage1(ids);
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(10)); // coverage, past the 6-byte header + 4-byte record
        out.extend_from_slice(&be16(X_PLACEMENT | X_ADVANCE));
        out.extend_from_slice(&value.x_placement.to_be_bytes());
        out.extend_from_slice(&value.x_advance.to_be_bytes());
        out.extend_from_slice(&cov);
        out
    }

    #[test]
    fn a_single_adjustment_moves_the_glyph_and_its_advance() {
        let data = single_pos1(
            &[7],
            Value {
                x_placement: 30,
                x_advance: -40,
                ..Value::default()
            },
        );
        let run = glyphs(&[7, 8]);
        let mut out = alloc::vec![Adjust::plain(500), Adjust::plain(500)];
        assert_eq!(single(&data, 0, &run, 0, &mut out), Some(1));
        assert_eq!(out[0].x_offset, 30);
        assert_eq!(out[0].x_advance, 460);
        // The uncovered glyph is not touched, and reports no match.
        assert_eq!(single(&data, 0, &run, 1, &mut out), None);
        assert_eq!(out[1], Adjust::plain(500));
    }

    #[test]
    fn a_single_adjustment_of_format_two_indexes_by_coverage() {
        let cov = coverage1(&[4, 9]);
        let mut data = Vec::new();
        data.extend_from_slice(&be16(2));
        data.extend_from_slice(&be16(12)); // coverage, past header + two 2-byte records
        data.extend_from_slice(&be16(X_ADVANCE));
        data.extend_from_slice(&be16(2));
        data.extend_from_slice(&(-11i16).to_be_bytes());
        data.extend_from_slice(&(-22i16).to_be_bytes());
        data.extend_from_slice(&cov);

        let run = glyphs(&[4, 9]);
        let mut out = alloc::vec![Adjust::plain(100), Adjust::plain(100)];
        single(&data, 0, &run, 0, &mut out);
        single(&data, 0, &run, 1, &mut out);
        assert_eq!(out[0].x_advance, 89);
        assert_eq!(out[1].x_advance, 78);
    }

    /// A `PairPosFormat1` subtable with one first glyph and one pair, whose
    /// second record is present only if `second` is.
    fn pair_pos1(left: u16, right: u16, first: i16, second: Option<i16>) -> Vec<u8> {
        let value2 = if second.is_some() { X_ADVANCE } else { 0 };
        let mut set = Vec::new();
        set.extend_from_slice(&be16(1));
        set.extend_from_slice(&be16(right));
        set.extend_from_slice(&first.to_be_bytes());
        if let Some(v) = second {
            set.extend_from_slice(&v.to_be_bytes());
        }
        let cov = coverage1(&[left]);
        // header 10 | pairSetOffset 2 | coverage | pairSet
        let cov_at = 12usize;
        let set_at = cov_at + cov.len();
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(u16::try_from(cov_at).unwrap()));
        out.extend_from_slice(&be16(X_ADVANCE));
        out.extend_from_slice(&be16(value2));
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(u16::try_from(set_at).unwrap()));
        out.extend_from_slice(&cov);
        out.extend_from_slice(&set);
        out
    }

    #[test]
    fn a_pair_charges_the_first_glyph_and_records_it_as_a_kern() {
        let data = pair_pos1(1, 2, -60, None);
        let run = glyphs(&[1, 2, 1]);
        let mut out = alloc::vec![Adjust::plain(500); 3];
        let defs = Definitions::default();
        let skip = Skipper::new(&data, defs, 0, 0, u32::MAX);
        // No second record, so the second glyph may still open the next pair.
        assert_eq!(pair(&data, 0, skip, &run, 0, &mut out), Some(1));
        assert_eq!(out[0].x_advance, 440);
        assert_eq!(out[0].kern, -60);
        assert_eq!(out[1], Adjust::plain(500));
    }

    #[test]
    fn a_pair_with_a_second_record_consumes_both_glyphs() {
        let data = pair_pos1(1, 2, -60, Some(-10));
        let run = glyphs(&[1, 2]);
        let mut out = alloc::vec![Adjust::plain(500); 2];
        let skip = Skipper::new(&data, Definitions::default(), 0, 0, u32::MAX);
        assert_eq!(pair(&data, 0, skip, &run, 0, &mut out), Some(2));
        assert_eq!(out[0].x_advance, 440);
        assert_eq!(out[1].x_advance, 490);
        // Only the first half is a gap between the pair; the second is the
        // second glyph's own width and must not be moved by reordering.
        assert_eq!(out[1].kern, 0);
    }

    use crate::gsub::Lig;

    /// A MarkLigPos subtable over one mark class: `mark`'s own anchor sits at
    /// the origin, and the ligature `lig` offers `(xs[n], 0)` over component
    /// `n + 1`.
    fn mark_lig_pos(lig: u16, mark: u16, xs: &[i16]) -> Vec<u8> {
        fn anchor1(x: i16) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&be16(1));
            out.extend_from_slice(&x.to_be_bytes());
            out.extend_from_slice(&be16(0));
            out
        }

        let mark_cov = coverage1(&[mark]);
        let lig_cov = coverage1(&[lig]);

        // count, then one (class, anchor offset) record, then the anchor.
        let mut mark_array = Vec::new();
        mark_array.extend_from_slice(&be16(1));
        mark_array.extend_from_slice(&be16(0)); // class 0
        mark_array.extend_from_slice(&be16(6)); // anchor, past count + record
        mark_array.extend_from_slice(&anchor1(0));

        // LigatureAttach: componentCount, one anchor offset per component,
        // then the anchors — all offsets taken from the LigatureAttach.
        let mut attach = Vec::new();
        attach.extend_from_slice(&be16(u16::try_from(xs.len()).unwrap()));
        let mut anchors = Vec::new();
        let anchor_base = 2 + xs.len() * 2;
        for x in xs {
            attach.extend_from_slice(&be16(u16::try_from(anchor_base + anchors.len()).unwrap()));
            anchors.extend_from_slice(&anchor1(*x));
        }
        attach.extend_from_slice(&anchors);

        // LigatureArray: a count and one offset to that table.
        let mut lig_array = Vec::new();
        lig_array.extend_from_slice(&be16(1));
        lig_array.extend_from_slice(&be16(4));
        lig_array.extend_from_slice(&attach);

        let mark_cov_at = 12;
        let lig_cov_at = mark_cov_at + mark_cov.len();
        let mark_array_at = lig_cov_at + lig_cov.len();
        let lig_array_at = mark_array_at + mark_array.len();

        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(u16::try_from(mark_cov_at).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(lig_cov_at).unwrap()));
        out.extend_from_slice(&be16(1)); // one mark class
        out.extend_from_slice(&be16(u16::try_from(mark_array_at).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(lig_array_at).unwrap()));
        out.extend_from_slice(&mark_cov);
        out.extend_from_slice(&lig_cov);
        out.extend_from_slice(&mark_array);
        out.extend_from_slice(&lig_array);
        out
    }

    /// Where `attach_to_lig` puts the mark at 1 on the ligature at 0.
    fn on_component(data: &[u8], run: &[SubGlyph]) -> Option<i32> {
        let mut out = alloc::vec![Adjust::plain(0); run.len()];
        attach_to_lig(data, 0, run, 1, 0, &mut out)?;
        Some(out.get(1)?.x_offset)
    }

    #[test]
    fn a_mark_lands_on_the_component_substitution_gave_it() {
        let data = mark_lig_pos(100, 200, &[200, 800]);
        let mut run = glyphs(&[100, 200]);
        run[0].lig = Lig::at(1, 2, 0);
        run[1].lig = Lig::at(1, 0, 1);
        assert_eq!(on_component(&data, &run), Some(200));
        run[1].lig = Lig::at(1, 0, 2);
        assert_eq!(on_component(&data, &run), Some(800));
    }

    #[test]
    fn a_mark_from_another_ligature_falls_back_to_the_last_component() {
        // The ids disagree, so the mark was never inside *this* ligature and
        // its component number means nothing here. Using it anyway would place
        // the mark by a number that belongs to a different glyph.
        let data = mark_lig_pos(100, 200, &[200, 800]);
        let mut run = glyphs(&[100, 200]);
        run[0].lig = Lig::at(1, 2, 0);
        run[1].lig = Lig::at(2, 0, 1);
        assert_eq!(on_component(&data, &run), Some(800));
    }

    #[test]
    fn a_type_five_lookup_on_a_run_that_never_ligated_still_attaches() {
        // Some faces write mark-to-base as type 5 with a single-component
        // ligature array. Nothing here has an id, so the fallback is the only
        // path — and it has to work, or those faces lose every mark.
        let data = mark_lig_pos(100, 200, &[350]);
        let run = glyphs(&[100, 200]);
        assert_eq!(on_component(&data, &run), Some(350));
    }

    #[test]
    fn a_mark_offset_takes_back_the_pen_travel_from_its_base() {
        // Base at 0 with a 500-unit advance, mark at 1 wanting to sit 100
        // units right of the base's origin.
        let mut out = alloc::vec![Adjust::plain(500), Adjust::plain(0)];
        out[1].x_offset = 100;
        out[1].y_offset = 250;
        out[1].kind = Attach::Mark;
        out[1].chain = -1;
        propagate(&mut out, false);
        assert_eq!(out[1].x_offset, -400);
        assert_eq!(out[1].y_offset, 250);
    }

    #[test]
    fn a_stacked_mark_inherits_the_one_below_it() {
        // base, mark on base, mark on mark.
        let mut out = alloc::vec![Adjust::plain(500), Adjust::plain(0), Adjust::plain(0)];
        out[1].x_offset = 100;
        out[1].y_offset = 250;
        out[1].kind = Attach::Mark;
        out[1].chain = -1;
        out[2].x_offset = 0;
        out[2].y_offset = 120;
        out[2].kind = Attach::Mark;
        out[2].chain = -1;
        propagate(&mut out, false);
        // The lower mark ends 400 left of its own pen...
        assert_eq!(out[1].x_offset, -400);
        // ...and the upper one lands on top of it, its own pen having moved no
        // further because a mark carries no advance.
        assert_eq!(out[2].x_offset, -400);
        assert_eq!(out[2].y_offset, 370);
    }

    #[test]
    fn a_right_to_left_mark_adds_the_travel_instead_of_taking_it_off() {
        // The pen reaches the mark first, so the base's advance is ahead of it.
        let mut out = alloc::vec![Adjust::plain(500), Adjust::plain(0)];
        out[1].x_offset = 100;
        out[1].kind = Attach::Mark;
        out[1].chain = -1;
        propagate(&mut out, true);
        assert_eq!(out[1].x_offset, 100);
    }

    #[test]
    fn a_chain_that_points_at_itself_does_not_loop() {
        let mut out = alloc::vec![Adjust::plain(0); 2];
        out[0].kind = Attach::Mark;
        out[0].chain = 1;
        out[1].kind = Attach::Mark;
        out[1].chain = -1;
        propagate(&mut out, false);
        // Both chains cleared; the test is that this returned at all.
        assert_eq!(out[0].chain, 0);
        assert_eq!(out[1].chain, 0);
    }
}
