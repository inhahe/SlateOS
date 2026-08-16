//! Placing a combining mark on a face that cannot say where it goes.
//!
//! [`GPOS` mark attachment](crate::mark) is the right answer to "where does
//! this accent sit?", because the font's designer wrote it down. Thousands of
//! shipping faces never wrote it down: they carry no `GPOS` table at all,
//! having been built for an era when a `é` was one character with one glyph
//! and a bare U+0301 was somebody else's problem. Drawn by that font, `c` +
//! U+0327 + U+0301 puts the cedilla and the acute at the pen — which is the
//! *left edge* of the following cell — so the two accents overprint each
//! other in the gap after the letter. On a sweep of 556 installed faces
//! against HarfBuzz, that single failure accounted for roughly 489 of the 559
//! runs this crate placed differently.
//!
//! This module is the answer of last resort: measure the base, measure the
//! mark, and centre one on the other. It is deliberately a *reimplementation
//! of HarfBuzz's* fallback (`hb-ot-shape-fallback.cc`) rather than an
//! independently invented one, down to the truncating integer division and
//! the `upem/16` gap. Two reasons. The output is checked against HarfBuzz —
//! an "improvement" here would read as a regression in the sweep and hide
//! real divergence in the noise. And the numbers themselves are not arbitrary
//! taste: they are what a decade of complaints about specific fonts settled
//! on, and this crate has no evidence to overrule them with.
//!
//! # When it runs
//!
//! Two conditions, both necessary.
//!
//! The face must have **no `GPOS` table whatsoever** — see
//! [`Face::has_positioning`](crate::sfnt::Face::has_positioning) for why that
//! is the line, and not "no `mark` feature".
//!
//! And the run's script must be one this crate can place marks for at all:
//! see [`positions_marks`]. Devanagari, Khmer, Thai and the rest of the
//! complex scripts are excluded, because in those scripts a mark's place in
//! the cluster is decided by a reordering pass this crate does not have, and
//! centring a virama on the consonant it follows is worse than leaving it
//! where it fell.
//!
//! # What it does
//!
//! Every mark's advance is zeroed, and its offset is computed from two ink
//! boxes and its canonical combining class:
//!
//! * **Horizontally** the mark is centred on the base — or left-aligned,
//!   right-aligned, or hung off the base's right edge, according to the class.
//!   The base's box is replaced by `0 .. advance` first, so that a zero-ink
//!   base (a space) still centres its mark sensibly.
//! * **Vertically** the mark clears the base's top or bottom by `upem/16`,
//!   and each further mark of the same class clears the one before it, so a
//!   stack of two accents does not collapse into one.
//!
//! The combining class is first *recategorized*: Unicode's fixed-position
//! classes for Hebrew (10–26), Arabic (27–36), Thai, Lao and Tibetan encode a
//! canonical ordering, not a place on the glyph, so they have to be mapped
//! onto the "above/below/left/right" classes before the geometry means
//! anything. [`attach_class`] is that map.

use crate::norm;
use crate::script::ScriptTags;

/// Combining classes that name a position rather than an ordering.
///
/// Unicode's own values, as `UAX #44` assigns them. Named here because the
/// geometry below switches on all twelve and bare numbers would make it
/// unreadable.
mod class {
    pub(super) const ATTACHED_BELOW_LEFT: u8 = 200;
    pub(super) const ATTACHED_BELOW: u8 = 202;
    pub(super) const ATTACHED_ABOVE: u8 = 214;
    pub(super) const ATTACHED_ABOVE_RIGHT: u8 = 216;
    pub(super) const BELOW_LEFT: u8 = 218;
    pub(super) const BELOW: u8 = 220;
    pub(super) const BELOW_RIGHT: u8 = 222;
    pub(super) const ABOVE_LEFT: u8 = 228;
    pub(super) const ABOVE: u8 = 230;
    pub(super) const ABOVE_RIGHT: u8 = 232;
    pub(super) const DOUBLE_BELOW: u8 = 233;
    pub(super) const DOUBLE_ABOVE: u8 = 234;
}

use class::{
    ABOVE, ABOVE_LEFT, ABOVE_RIGHT, ATTACHED_ABOVE, ATTACHED_ABOVE_RIGHT, ATTACHED_BELOW,
    ATTACHED_BELOW_LEFT, BELOW, BELOW_LEFT, BELOW_RIGHT, DOUBLE_ABOVE, DOUBLE_BELOW,
};

/// The OpenType script tags whose marks this fallback must not touch,
/// sorted so [`positions_marks`] can binary-search it.
///
/// These are the scripts HarfBuzz hands to one of its four complex shapers —
/// Indic, Khmer, Myanmar, USE — or to the Thai shaper, every one of which
/// sets `fallback_position = false` (`hb-ot-shaper-*.cc`). The list is the
/// script set of `hb_ot_shaper_categorize` in `hb-ot-shaper.hh`, mapped
/// through this crate's own tag table.
///
/// The reason those shapers refuse the fallback is the same reason this crate
/// must: in a Brahmic cluster the marks are not a stack of accents sitting on
/// one base. A virama is a *spacing* glyph that suppresses a vowel, a matra
/// may be reordered to before the consonant it logically follows, and which
/// glyph a mark belongs to is not "the last non-mark to my left" but the
/// output of a reordering pass. Measuring a box and centring on it produces a
/// confident wrong answer; leaving the mark at its natural advance produces
/// an obviously unshaped one, which is both more honest and, in practice,
/// closer to legible.
///
/// Matched on the *preferred* tag only, which is half the question: the script
/// picks a complex shaper and the *face* may then call it off. See
/// [`shaped_as_default`].
static COMPLEX_SCRIPTS: [[u8; 4]; 101] = [
    *b"adlm", *b"ahom", *b"bali", *b"batk", *b"berf", *b"bhks", *b"bng2", *b"brah", *b"bugi",
    *b"buhd", *b"cakm", *b"cham", *b"chrs", *b"cpmn", *b"dev2", *b"diak", *b"dogr", *b"dupl",
    *b"egyp", *b"elym", *b"gara", *b"gjr2", *b"gong", *b"gonm", *b"gran", *b"gukh", *b"gur2",
    *b"hano", *b"hmng", *b"hmnp", *b"java", *b"kali", *b"kawi", *b"khar", *b"khmr", *b"khoj",
    *b"kits", *b"knd2", *b"krai", *b"kthi", *b"lana", *b"lao ", *b"lepc", *b"limb", *b"mahj",
    *b"maka", *b"mand", *b"mani", *b"marc", *b"medf", *b"mlm2", *b"modi", *b"mong", *b"mtei",
    *b"mult", *b"mym2", *b"nagm", *b"nand", *b"newa", *b"nko ", *b"onao", *b"ory2", *b"ougr",
    *b"phag", *b"phlp", *b"plrd", *b"rjng", *b"rohg", *b"saur", *b"shrd", *b"sidd", *b"sidt",
    *b"sind", *b"sinh", *b"sogd", *b"sogo", *b"soyo", *b"sund", *b"sunu", *b"sylo", *b"tagb",
    *b"takr", *b"tale", *b"tavt", *b"tayo", *b"tel2", *b"tfng", *b"tglg", *b"thai", *b"tibt",
    *b"tirh", *b"tml2", *b"tnsa", *b"todr", *b"tols", *b"toto", *b"tutg", *b"vith", *b"wcho",
    *b"yezi", *b"zanb",
];

/// The three complex scripts a face cannot call off, sorted.
///
/// Every other entry of [`COMPLEX_SCRIPTS`] reaches its shaper through an arm
/// of `hb_ot_shaper_categorize` that first checks what the font declares; Thai,
/// Lao and Khmer reach theirs unconditionally. There is no stated reason for
/// the asymmetry beyond history — the Thai shaper predates the check and the
/// Khmer one was split out of the Indic shaper after it — but it is observable,
/// so it is transcribed rather than tidied.
static ALWAYS_COMPLEX: [[u8; 4]; 3] = [*b"khmr", *b"lao ", *b"thai"];

/// Whether a run of `tags` is shaped by the *default* shaper despite its script
/// asking for a complex one, because of what the face declares.
///
/// `gsub` is the script tag the face's `GSUB` features were actually taken
/// from — [`Substitutions::chosen_script`](crate::gsub::Substitutions::chosen_script) —
/// which is the run's own tag if the face registers it, and otherwise whatever
/// the fallback chain reached. HarfBuzz asks the same question of the same
/// value, in `hb_ot_shaper_categorize`:
///
/// ```c
/// /* If the designer designed the font for the 'DFLT' script,
///  * (or we ended up arbitrarily pick 'latn'), use the default shaper.
///  * Otherwise, use the specific shaper. */
/// if (gsub_script == HB_TAG ('D','F','L','T') ||
///     gsub_script == HB_TAG ('l','a','t','n'))
///   return &_hb_ot_shaper_default;
/// ```
///
/// with a third tag, `mymr`, in the Myanmar arm: that is the tag from before
/// the Myanmar shaping spec existed, so a face using it is asking for the
/// pre-spec behaviour, which is no shaping at all.
///
/// The point is that a complex shaper is a contract with the font. Devanagari
/// reordering only produces something legible if the face has half forms, reph
/// forms and a `pref` feature to move a matra into; a face that files every
/// feature it has under `latn` has none of them and has said so. Shaping its
/// Devanagari as if it did means running a reordering nothing implements and
/// withholding the mark handling that would at least stack the vowel signs on
/// the consonant.
///
/// Note what is *not* here: a face with no `GSUB` at all, or one whose
/// `GSUB` names neither the run's script nor any default, gives `None` and
/// keeps its complex shaper. HarfBuzz reaches the same answer by a different
/// road — `hb_ot_layout_table_select_script` leaves `chosen_script` at
/// `HB_TAG_NONE`, which equals neither `DFLT` nor `latn`. It matters, because
/// that is exactly the face [`NO_ZERO_WIDTH_MARKS`] was measured against.
///
/// One divergence, and it cannot arise here: HarfBuzz sends an Indic run to its
/// USE shaper when the chosen tag's last byte is `'3'`. No such tag is in this
/// crate's fallback chain, so no face can be chosen under one.
pub(crate) fn shaped_as_default(tags: Option<ScriptTags>, gsub: Option<[u8; 4]>) -> bool {
    // A run with no script, or with a simple one, is already on the default
    // shaper; there is nothing for the face to call off.
    let Some(tags) = tags else { return false };
    if COMPLEX_SCRIPTS.binary_search(&tags.preferred).is_err()
        || ALWAYS_COMPLEX.binary_search(&tags.preferred).is_ok()
    {
        return false;
    }
    gsub.is_some_and(|tag| {
        tag == *b"DFLT" || tag == *b"latn" || (tag == *b"mymr" && tags.preferred == *b"mym2")
    })
}

/// Whether a run of this script may have its marks placed by measurement.
///
/// `None` — a run with no script of its own, which is what an entirely
/// scriptless string like `"123"` or a lone combining mark produces — is
/// allowed: that is HarfBuzz's default shaper, and its fallback is on. So is a
/// complex script the face called off, for the same reason: `simple` is
/// [`shaped_as_default`], and the shaper it names is the one with
/// `fallback_position = true`.
///
/// See [`COMPLEX_SCRIPTS`] for what is excluded and why.
pub(crate) fn positions_marks(tags: Option<ScriptTags>, simple: bool) -> bool {
    simple || tags.is_none_or(|tags| COMPLEX_SCRIPTS.binary_search(&tags.preferred).is_err())
}

/// The scripts whose marks keep their advance even so.
///
/// A much shorter list than [`COMPLEX_SCRIPTS`], and a different question.
/// Declining to *place* a mark says the geometry cannot be guessed; it does
/// not say the mark takes room. HarfBuzz keeps the two apart — every shaper
/// carries a `fallback_position` flag and a separate `zero_width_marks` one —
/// and only the Indic and Khmer shapers set the second to `NONE`. Its Thai,
/// Myanmar and USE shapers all decline the placement and zero the advance
/// anyway.
///
/// Measured rather than transcribed, since the shaper a script reaches
/// depends on what the *font* declares and the interesting case here is a
/// font that declares nothing. Shaping "consonant + `Mn` mark" with HarfBuzz
/// against a face with no `GSUB`, `GPOS` or `GDEF` at all: Devanagari,
/// Bengali, Gurmukhi, Gujarati, Oriya, Tamil, Telugu, Kannada, Malayalam and
/// Khmer keep the advance; Sinhala, Myanmar, Tibetan, Mongolian, Cham,
/// Balinese, Thai, Lao and Hebrew zero it.
///
/// "Declares nothing" is load-bearing: the same ten in a face that files its
/// features under `DFLT` or `latn` *do* have their advances zeroed, because
/// that face has called the shaper off. See [`shaped_as_default`].
/// `hang` is here for a different reason than the other ten, and not from the
/// same measurement: HarfBuzz's Hangul shaper sets `zero_width_marks` to
/// `NONE` outright. It is deliberately **not** in [`COMPLEX_SCRIPTS`], because
/// that shaper's `fallback_position` is `true` — Hangul declines the zeroing
/// and accepts the placement, which is the opposite pairing to the Indic ten.
static NO_ZERO_WIDTH_MARKS: [[u8; 4]; 11] = [
    *b"bng2", *b"dev2", *b"gjr2", *b"gur2", *b"hang", *b"khmr", *b"knd2", *b"mlm2", *b"ory2",
    *b"tel2", *b"tml2",
];

/// When a run's combining marks lose their advance, relative to `GPOS`.
///
/// Three answers and not two, because HarfBuzz's `zero_width_marks` is three
/// values — and the third is not a refinement, it changes the width a real
/// face reports. See [`zeroes_mark_advances`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Zeroing {
    /// Never: the mark keeps whatever `hmtx` and `GPOS` between them say.
    Never,
    /// Before the positioning lookups run, so a lookup that charges an advance
    /// charges it onto nothing and the mark ends up with the width the lookup
    /// asked for.
    BeforeGpos,
    /// After they have run, so whatever they charged is thrown away.
    AfterGpos,
}

/// When a run of this script has its marks' advances zeroed.
///
/// Separate from [`positions_marks`], and not [`Zeroing::Never`] for nearly
/// everything: a combining mark takes no room whether or not anything is
/// willing to work out where to draw it. See [`NO_ZERO_WIDTH_MARKS`] for the
/// eleven that differ, and [`shaped_as_default`] for `simple`, which puts ten
/// of those eleven back.
///
/// [`Zeroing::BeforeGpos`] is Myanmar's alone here, and it is transcribed
/// rather than measured: of HarfBuzz's nine shapers only Myanmar and USE set
/// `HB_OT_SHAPE_ZERO_WIDTH_MARKS_BY_GDEF_EARLY`, and USE is not written yet.
/// The distinction is invisible in a face whose marks are all zero-width in
/// `hmtx` and decisive in one whose are not: `mmrtext.ttf` classes U+103C
/// (medial ra, the hook drawn under and around the consonant) as a `GDEF`
/// mark but gives it a 440-unit advance, and its `dist` feature charges that
/// 440 back on. Zeroing afterwards discards the `dist` adjustment and every
/// glyph after the hook slides 440 units left; zeroing first keeps it, which
/// is what HarfBuzz prints.
pub(crate) fn zeroes_mark_advances(tags: Option<ScriptTags>, simple: bool) -> Zeroing {
    if simple {
        return Zeroing::AfterGpos;
    }
    match tags {
        Some(tags) if NO_ZERO_WIDTH_MARKS.binary_search(&tags.preferred).is_ok() => Zeroing::Never,
        Some(tags) if tags.preferred == *b"mym2" => Zeroing::BeforeGpos,
        _ => Zeroing::AfterGpos,
    }
}

/// The script tag a run of `tags` insists the face's `GPOS` name before it will
/// accept any of it, if there is one.
///
/// Almost always `None`: a run takes whatever the face's `GPOS` offers it,
/// through `DFLT` if the face registers nothing closer. Hebrew is the
/// exception, and as far as HarfBuzz is concerned the *only* exception — it is
/// the one shaper that sets a `gpos_tag`, and
///
/// ```c
/// bool disable_gpos = plan.shaper->gpos_tag &&
///                     plan.shaper->gpos_tag != plan.map.chosen_script[1];
/// ```
///
/// turns the whole positioning table off for a Hebrew run in a face that files
/// its `GPOS` under anything else. The reason is that a face's `DFLT` (or
/// `latn`) positioning is written for the script the face is *for*: applying
/// Latin's accent anchors to Hebrew points is worse than not positioning them,
/// because it silently produces plausible-looking wrong geometry instead of
/// falling through to the fallback that measures the glyphs.
///
/// Confirmed by measurement, not read off the source: shaping one string per
/// script through HarfBuzz against Consolas — whose `GPOS` names `cyrl`, `grek`
/// and `latn`, and no `DFLT` — the fallback runs for Hebrew and for nothing
/// else, Arabic, Devanagari, Myanmar, Khmer, Mongolian, Syriac, Thaana,
/// Ethiopic and Thai all keeping their unzeroed advances. See
/// `TD-FONT-GATES-THE-MARK-FALLBACK-ON-THE-FACE-NOT-THE-RUN`.
pub(crate) fn demands_own_gpos_script(tags: Option<ScriptTags>) -> Option<[u8; 4]> {
    let tags = tags?;
    (tags.preferred == *b"hebr").then_some(*b"hebr")
}

/// A glyph's ink box, in the shape the placement arithmetic wants it.
///
/// Not [`BBox`](crate::sfnt::BBox), which is four edges. This is an origin
/// plus two signed extents, because that is what the arithmetic adds and
/// subtracts: `y_bearing` is the **top** of the ink and `height` runs
/// **downwards** and is therefore negative, so `y_bearing + height` is the
/// bottom. That convention is FreeType's and HarfBuzz's, and keeping it means
/// the placement rules below can be read against theirs line for line instead
/// of being mentally sign-flipped.
///
/// Font units, as integers, for the same reason: HarfBuzz does this
/// arithmetic in font units with truncating integer division, and doing it in
/// floats would round differently in the last unit and make an exact
/// comparison impossible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Extents {
    /// Left edge of the ink.
    pub(crate) x_bearing: i32,
    /// Top edge of the ink.
    pub(crate) y_bearing: i32,
    /// Width, rightwards, so non-negative.
    pub(crate) width: i32,
    /// Height, *downwards*, so non-positive.
    pub(crate) height: i32,
}

impl Extents {
    /// The box around `x_min .. x_max` by `y_min .. y_max`.
    pub(crate) fn new(x_min: i32, y_min: i32, x_max: i32, y_max: i32) -> Self {
        Self {
            x_bearing: x_min,
            y_bearing: y_max,
            width: x_max.saturating_sub(x_min),
            height: y_min.saturating_sub(y_max),
        }
    }
}

/// Where the class map recategorizes `ch` to, for the purpose of placing it.
///
/// Zero means "not a combining mark", which is what the caller uses to tell a
/// base from a mark.
///
/// Two groups of input are handled:
///
/// * Classes **200 and up** already name a position (`ABOVE`, `BELOW_RIGHT`,
///   …) and pass through untouched. That is nearly every mark in Latin,
///   Greek, Cyrillic and Vietnamese.
/// * Classes **10–36** are Unicode's *fixed-position* classes for Hebrew,
///   Arabic and Syriac. Their numeric value encodes the order the marks must
///   be sorted into, not where they are drawn, so each is mapped onto the
///   position class its script actually wants — Arabic fatha above, kasra
///   below, Hebrew sheva below, shin dot above-right, and so on.
///
/// Unicode's other fixed-position classes — 103 and 107 (Thai), 118 and 122
/// (Lao), 129–132 (Tibetan) — are deliberately absent, along with the Thai and
/// Lao vowel signs that carry class 0 despite being drawn above or below. Not
/// because they have no answer but because nothing can ask the question: the
/// one caller asks only about a run whose script passed
/// [`positions_marks`], and `thai`, `lao ` and `tibt` are all in
/// [`COMPLEX_SCRIPTS`]. Arms for them would be claims no test could check and
/// no sweep could measure. HarfBuzz declines those scripts' fallback placement
/// too, so their absence changes no output.
///
/// The mapping is HarfBuzz's `recategorize_combining_class`, transposed from
/// its "modified" combining classes back onto Unicode's. HarfBuzz permutes
/// classes 10–26 and 27–35 before this point so that Hebrew and Arabic marks
/// *sort* into display order; the permutation is injective and this function
/// does not sort, so matching on the unpermuted value selects exactly the same
/// characters. What this crate does not do is the reordering itself — see
/// `known-issues.md`.
pub(crate) fn attach_class(ch: char) -> u8 {
    let klass = norm::combining_class(ch);
    if klass >= 200 {
        return klass;
    }
    match klass {
        // Hebrew points. Everything under the letter except the dots that
        // distinguish shin from sin, the holam that sits to the upper left,
        // the varika above, and rafe which touches the letter's top.
        10..=18 | 20 | 22 => BELOW,
        23 => ATTACHED_ABOVE,
        24 => ABOVE_RIGHT,
        19 | 25 => ABOVE_LEFT,
        26 => ABOVE,
        // Dagesh (21) is *inside* the letter, which no position class
        // describes; left alone it centres horizontally and does not move
        // vertically, which is as close as this scheme gets.
        //
        // Arabic and Syriac vowels: everything above but kasra and kasratan.
        27 | 28 | 30 | 31 | 33..=36 => ABOVE,
        29 | 32 => BELOW,
        other => other,
    }
}

/// Displace `mark` onto `base`, and grow `base` to cover it.
///
/// Returns the mark's offset from the *base glyph's origin* in font units,
/// `y` upwards — the same thing [`Face::mark_on_base`] returns for a face that
/// can answer, so that the caller subtracts the pen travel identically in both
/// cases.
///
/// `base` is `&mut` because the second mark of a stack must clear the first:
/// each call extends the box upwards or downwards by what it just placed, so
/// passing the same box through a run of marks stacks them. The caller resets
/// it to the base glyph's own box whenever the combining class changes, since
/// marks above and marks below grow the box in opposite directions and must
/// not see each other's growth.
///
/// `gap` is the clearance between one mark and the next, `upem/16`. `rtl` only
/// affects the two *double* classes (U+035C–U+0362 and friends), which are
/// drawn straddling the join between two glyphs and so hang off whichever edge
/// of the base the next glyph is on.
///
/// [`Face::mark_on_base`]: crate::sfnt::Face::mark_on_base
pub(crate) fn place(base: &mut Extents, mark: &Extents, klass: u8, gap: i32, rtl: bool) -> (i32, i32) {
    // Horizontal. Note that every arm subtracts the mark's own left bearing:
    // the offset moves the mark's *origin*, and what has to land in the right
    // place is its ink.
    let x = match klass {
        DOUBLE_BELOW | DOUBLE_ABOVE => {
            // Half of it belongs to the next glyph, so it straddles the edge
            // between them: centred on the base's trailing edge.
            let edge = if rtl {
                base.x_bearing
            } else {
                base.x_bearing.saturating_add(base.width)
            };
            edge.saturating_sub(mark.width / 2).saturating_sub(mark.x_bearing)
        }
        ATTACHED_BELOW_LEFT | BELOW_LEFT | ABOVE_LEFT => {
            base.x_bearing.saturating_sub(mark.x_bearing)
        }
        ATTACHED_ABOVE_RIGHT | BELOW_RIGHT | ABOVE_RIGHT => base
            .x_bearing
            .saturating_add(base.width)
            .saturating_sub(mark.width)
            .saturating_sub(mark.x_bearing),
        // Centre, which is where an unrecognised class goes too — a mark
        // whose position nobody stated is least wrong in the middle.
        _ => base
            .x_bearing
            .saturating_add((base.width.saturating_sub(mark.width)) / 2)
            .saturating_sub(mark.x_bearing),
    };

    // Vertical. `y_bearing` is the top and `height` is negative, so "grow
    // downwards" is `height -= n` and "grow upwards" is `y_bearing += n`.
    let mut y = 0;
    match klass {
        DOUBLE_BELOW | BELOW_LEFT | BELOW | BELOW_RIGHT | ATTACHED_BELOW_LEFT | ATTACHED_BELOW => {
            if !matches!(klass, ATTACHED_BELOW_LEFT | ATTACHED_BELOW) {
                // An *attached* mark touches the letter; every other kind
                // clears it.
                base.height = base.height.saturating_sub(gap);
            }
            y = base
                .y_bearing
                .saturating_add(base.height)
                .saturating_sub(mark.y_bearing);
            if (gap > 0) == (y > 0) {
                // The mark's own ink already reaches below the base's bottom,
                // so moving it down would open a hole. Leave it where it is
                // and record how far past the base it goes, so the next mark
                // still clears it.
                base.height = base.height.saturating_sub(y);
                y = 0;
            }
            base.height = base.height.saturating_add(mark.height);
        }
        DOUBLE_ABOVE | ABOVE_LEFT | ABOVE | ABOVE_RIGHT | ATTACHED_ABOVE | ATTACHED_ABOVE_RIGHT => {
            if !matches!(klass, ATTACHED_ABOVE | ATTACHED_ABOVE_RIGHT) {
                base.y_bearing = base.y_bearing.saturating_add(gap);
                base.height = base.height.saturating_sub(gap);
            }
            y = base
                .y_bearing
                .saturating_sub(mark.y_bearing.saturating_add(mark.height));
            if (gap > 0) != (y > 0) {
                // The mark hangs so far below its own origin that placing it
                // by its bottom edge would drop it onto the letter. Split the
                // difference rather than either overprinting or floating.
                let correction = y.saturating_neg() / 2;
                base.y_bearing = base.y_bearing.saturating_add(correction);
                base.height = base.height.saturating_sub(correction);
                y = y.saturating_add(correction);
            }
            base.y_bearing = base.y_bearing.saturating_sub(mark.height);
            base.height = base.height.saturating_add(mark.height);
        }
        // LEFT, RIGHT, and the classes that only order marks rather than
        // place them: centred horizontally above, and not moved vertically.
        _ => {}
    }
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate table is binary-searched, so it has to be sorted, and a typo
    /// in a four-byte literal is otherwise invisible.
    #[test]
    fn the_excluded_scripts_are_sorted() {
        assert!(COMPLEX_SCRIPTS.is_sorted(), "COMPLEX_SCRIPTS is out of order");
    }

    /// The scripts whose marks are a stack of accents on one base get the
    /// fallback; the ones whose marks are decided by a reordering pass this
    /// crate does not have do not.
    #[test]
    fn only_the_simple_scripts_get_their_marks_placed() {
        for tag in [*b"latn", *b"grek", *b"cyrl", *b"hebr", *b"arab", *b"syrc", *b"hang"] {
            assert!(
                positions_marks(Some(ScriptTags::exactly(tag)), false),
                "{:?} should be placed",
                core::str::from_utf8(&tag)
            );
        }
        // Indic (both spellings of the *preferred* tag are the `2` ones),
        // Khmer, Myanmar, Thai and a USE script.
        for tag in [*b"dev2", *b"bng2", *b"khmr", *b"mym2", *b"thai", *b"lao ", *b"tibt"] {
            assert!(
                !positions_marks(Some(ScriptTags::exactly(tag)), false),
                "{:?} should be left alone",
                core::str::from_utf8(&tag)
            );
        }
    }

    /// Text with no script of its own — `"123"`, or a combining mark with
    /// nothing before it — is HarfBuzz's default shaper, whose fallback is on.
    #[test]
    fn scriptless_text_still_gets_the_fallback() {
        assert!(positions_marks(None, false));
    }

    #[test]
    fn the_scripts_that_keep_their_mark_advances_are_sorted() {
        assert!(
            NO_ZERO_WIDTH_MARKS.is_sorted(),
            "NO_ZERO_WIDTH_MARKS is out of order"
        );
    }

    /// The two questions are not the same question, and this is the case that
    /// proves it: Thai and Myanmar decline the *placement* but still want the
    /// advance taken away, while Devanagari and Khmer decline both. Reading
    /// one answer off the other zeroes ten scripts' marks that should keep
    /// their width, or charges every Thai vowel a full one.
    #[test]
    fn declining_to_place_a_mark_is_not_declining_to_zero_it() {
        for tag in [*b"thai", *b"mym2", *b"tibt", *b"latn", *b"hebr", *b"arab"] {
            assert_ne!(
                zeroes_mark_advances(Some(ScriptTags::exactly(tag)), false),
                Zeroing::Never,
                "{:?} should be zeroed",
                core::str::from_utf8(&tag)
            );
        }
        for tag in [*b"dev2", *b"bng2", *b"khmr", *b"tml2", *b"ory2"] {
            assert_eq!(
                zeroes_mark_advances(Some(ScriptTags::exactly(tag)), false),
                Zeroing::Never,
                "{:?} should keep its advances",
                core::str::from_utf8(&tag)
            );
        }
        assert_eq!(zeroes_mark_advances(None, false), Zeroing::AfterGpos);
    }

    /// Myanmar is the one script here that zeroes *first*, and the difference
    /// is worth a test of its own because nothing else in the crate would
    /// notice if it silently became `AfterGpos` — every face whose marks are
    /// already zero-width in `hmtx` gives the same answer either way. See
    /// [`zeroes_mark_advances`] for the face that does not.
    #[test]
    fn myanmar_zeroes_its_marks_before_positioning_and_thai_after() {
        let mym2 = Some(ScriptTags::exactly(*b"mym2"));
        assert_eq!(zeroes_mark_advances(mym2, false), Zeroing::BeforeGpos);
        // A face that files its features under `DFLT` has called the Myanmar
        // shaper off, and the default shaper zeroes last like every other.
        assert_eq!(zeroes_mark_advances(mym2, true), Zeroing::AfterGpos);
        let thai = Some(ScriptTags::exactly(*b"thai"));
        assert_eq!(zeroes_mark_advances(thai, false), Zeroing::AfterGpos);
    }

    #[test]
    fn the_scripts_a_face_cannot_call_off_are_sorted() {
        assert!(ALWAYS_COMPLEX.is_sorted(), "ALWAYS_COMPLEX is out of order");
    }

    /// A face that files every feature it has under `DFLT` or `latn` has said
    /// it does no complex shaping, and that is the answer whatever the run's
    /// characters are.
    #[test]
    fn a_face_declaring_no_indic_script_calls_the_indic_shaper_off() {
        for tag in [*b"dev2", *b"bng2", *b"tml2", *b"mym2", *b"tibt", *b"java"] {
            for gsub in [*b"DFLT", *b"latn"] {
                assert!(
                    shaped_as_default(Some(ScriptTags::exactly(tag)), Some(gsub)),
                    "{:?} under {:?} should be shaped by the default shaper",
                    core::str::from_utf8(&tag),
                    core::str::from_utf8(&gsub)
                );
            }
        }
    }

    /// `mymr` is the tag from before the Myanmar shaping spec existed, so a
    /// face using it is asking for the behaviour that predates the shaper —
    /// and only a Myanmar run may read it that way.
    #[test]
    fn only_myanmar_reads_mymr_as_calling_its_shaper_off() {
        assert!(shaped_as_default(
            Some(ScriptTags::exactly(*b"mym2")),
            Some(*b"mymr")
        ));
        assert!(!shaped_as_default(
            Some(ScriptTags::exactly(*b"dev2")),
            Some(*b"mymr")
        ));
    }

    /// Thai, Lao and Khmer reach their shapers through an arm of HarfBuzz's
    /// categorizer that never looks at the font, so no face can call them off.
    #[test]
    fn three_scripts_keep_their_shaper_whatever_the_face_says() {
        for tag in [*b"thai", *b"lao ", *b"khmr"] {
            for gsub in [*b"DFLT", *b"latn"] {
                assert!(
                    !shaped_as_default(Some(ScriptTags::exactly(tag)), Some(gsub)),
                    "{:?} keeps its shaper",
                    core::str::from_utf8(&tag)
                );
            }
        }
    }

    /// A face that names *nothing* in the run's fallback chain — one with no
    /// `GSUB` at all, most often — has not said "no complex shaping"; it has
    /// said nothing. HarfBuzz reads its `HB_TAG_NONE` the same way, and it is
    /// the case [`NO_ZERO_WIDTH_MARKS`] was measured against.
    #[test]
    fn a_face_that_names_no_script_calls_nothing_off() {
        for tag in [*b"dev2", *b"mym2", *b"tibt"] {
            assert!(
                !shaped_as_default(Some(ScriptTags::exactly(tag)), None),
                "{:?} keeps its shaper in a face that declares nothing",
                core::str::from_utf8(&tag)
            );
        }
    }

    /// There is no complex shaper to call off for a simple script, or for a
    /// run with no script at all — both are already on the default shaper, and
    /// answering `true` would be a claim that something changed.
    #[test]
    fn a_simple_script_is_not_called_off() {
        for tag in [*b"latn", *b"cyrl", *b"arab", *b"hebr", *b"hang"] {
            assert!(
                !shaped_as_default(Some(ScriptTags::exactly(tag)), Some(*b"DFLT")),
                "{:?} has no complex shaper to lose",
                core::str::from_utf8(&tag)
            );
        }
        assert!(!shaped_as_default(None, Some(*b"DFLT")));
    }

    /// The point of the whole exercise: a Devanagari run in a face that
    /// declares only `latn` gets the default shaper's mark handling, which is
    /// both halves — placed by measurement *and* zero-width — where the Indic
    /// shaper would have withheld both. `Hack` on `हिन्दी` is the face and the
    /// string that found this.
    #[test]
    fn calling_the_shaper_off_restores_both_halves_of_the_mark_handling() {
        let deva = Some(ScriptTags::exactly(*b"dev2"));
        assert!(!positions_marks(deva, false));
        assert_eq!(zeroes_mark_advances(deva, false), Zeroing::Never);
        assert!(positions_marks(deva, true));
        assert_eq!(zeroes_mark_advances(deva, true), Zeroing::AfterGpos);
    }

    /// Hebrew is the whole of the list, and the negative half is what makes it
    /// a claim: if this ever answered `Some` for another script, that script's
    /// runs would refuse the `GPOS` of every face that files its features
    /// under `DFLT` — which is most faces — and be positioned by measurement
    /// instead of by the anchors the designer drew.
    #[test]
    fn only_hebrew_demands_a_gpos_registered_under_its_own_name() {
        assert_eq!(
            demands_own_gpos_script(Some(ScriptTags::exactly(*b"hebr"))),
            Some(*b"hebr")
        );
        for tag in [
            *b"latn", *b"arab", *b"cyrl", *b"grek", *b"thai", *b"dev2", *b"deva", *b"khmr",
            *b"mym2", *b"tibt", *b"hani", *b"DFLT",
        ] {
            assert_eq!(
                demands_own_gpos_script(Some(ScriptTags::exactly(tag))),
                None,
                "{:?} must take whatever GPOS the face offers",
                core::str::from_utf8(&tag)
            );
        }
        // A scriptless run — digits, punctuation, a lone mark — has no name to
        // demand and so demands nothing.
        assert_eq!(demands_own_gpos_script(None), None);
    }

    /// AGENCYB.TTF's `a`, whose numbers the placement rules below were
    /// checked against HarfBuzz with.
    fn agency_a() -> Extents {
        // Ink 78..733 by -4..1010, but the base's box is always replaced by
        // `0 .. advance` horizontally, and its advance is 815.
        let mut e = Extents::new(78, -4, 733, 1010);
        e.x_bearing = 0;
        e.width = 815;
        e
    }

    /// The same face's missing-glyph box, which is what its combining marks
    /// come out as: 128..896 by 0..1633.
    fn agency_notdef() -> Extents {
        Extents::new(128, 0, 896, 1633)
    }

    #[test]
    fn a_positional_class_passes_through() {
        assert_eq!(attach_class('\u{0301}'), ABOVE);
        assert_eq!(attach_class('\u{0323}'), BELOW);
        assert_eq!(attach_class('\u{0328}'), ATTACHED_BELOW);
    }

    #[test]
    fn an_ordinary_letter_is_not_a_mark() {
        assert_eq!(attach_class('a'), 0);
        assert_eq!(attach_class('\u{05D0}'), 0);
        assert_eq!(attach_class('\u{0627}'), 0);
    }

    #[test]
    fn arabic_vowels_are_sorted_above_and_below() {
        // fatha, damma, shadda, sukun, superscript alef.
        for ch in ['\u{064E}', '\u{064F}', '\u{0651}', '\u{0652}', '\u{0670}'] {
            assert_eq!(attach_class(ch), ABOVE, "{ch:?}");
        }
        // kasra and kasratan are the two that hang below.
        for ch in ['\u{0650}', '\u{064D}'] {
            assert_eq!(attach_class(ch), BELOW, "{ch:?}");
        }
    }

    #[test]
    fn hebrew_points_are_mostly_below() {
        // sheva, hiriq, qamats, meteg.
        for ch in ['\u{05B0}', '\u{05B4}', '\u{05B8}', '\u{05BD}'] {
            assert_eq!(attach_class(ch), BELOW, "{ch:?}");
        }
        // The shin dot goes upper right, the sin dot upper left, and holam
        // joins the sin dot.
        assert_eq!(attach_class('\u{05C1}'), ABOVE_RIGHT);
        assert_eq!(attach_class('\u{05C2}'), ABOVE_LEFT);
        assert_eq!(attach_class('\u{05B9}'), ABOVE_LEFT);
        // Rafe touches the top of the letter.
        assert_eq!(attach_class('\u{05BF}'), ATTACHED_ABOVE);
    }

    /// `attach_class` has no arm for Thai, Lao or Tibetan, and this is the
    /// assertion that keeps that honest: it is allowed to have none only
    /// because those scripts never reach it. Whoever takes one of them out of
    /// [`COMPLEX_SCRIPTS`] will fail here rather than silently start placing
    /// its marks by a class map that was never written for them — U+0E34 SARA
    /// I would arrive as class 0 and be taken for a base.
    #[test]
    fn the_scripts_the_class_map_omits_are_scripts_it_is_never_asked_about() {
        for tag in [*b"thai", *b"lao ", *b"tibt"] {
            assert!(
                !positions_marks(Some(ScriptTags::exactly(tag)), false),
                "{:?} reaches attach_class, which has no classes for it",
                core::str::from_utf8(&tag)
            );
        }
        assert_eq!(attach_class('\u{0E34}'), 0);
    }

    #[test]
    fn a_mark_below_clears_the_letters_bottom() {
        // HarfBuzz on AGENCYB.TTF, `a` + U+0323: offset (-920, -1765) once
        // the pen travel of 815 is taken off the horizontal.
        let mut base = agency_a();
        let (x, y) = place(&mut base, &agency_notdef(), BELOW, 2048 / 16, false);
        assert_eq!((x - 815, y), (-920, -1765));
    }

    #[test]
    fn a_mark_above_clears_the_letters_top() {
        // `a` + U+030C on the same face: (-920, 1138).
        let mut base = agency_a();
        let (x, y) = place(&mut base, &agency_notdef(), ABOVE, 2048 / 16, false);
        assert_eq!((x - 815, y), (-920, 1138));
    }

    #[test]
    fn a_second_mark_stacks_clear_of_the_first() {
        let gap = 2048 / 16;
        let mut base = agency_a();
        // Two below: -1765 then -3526.
        let (_, first) = place(&mut base, &agency_notdef(), BELOW, gap, false);
        let (_, second) = place(&mut base, &agency_notdef(), BELOW, gap, false);
        assert_eq!((first, second), (-1765, -3526));

        let mut base = agency_a();
        // Two above: 1138 then 2899.
        let (_, first) = place(&mut base, &agency_notdef(), ABOVE, gap, false);
        let (_, second) = place(&mut base, &agency_notdef(), ABOVE, gap, false);
        assert_eq!((first, second), (1138, 2899));
    }

    #[test]
    fn an_attached_mark_gets_no_clearance() {
        // `o` + U+0328 on AGENCYB: ink 82..748 by 0..1010, advance 831, and
        // HarfBuzz puts the ogonek at y = -1633 — the base's bottom with no
        // gap added, unlike the -1761 a BELOW mark would get.
        let mut base = Extents::new(82, 0, 748, 1010);
        base.x_bearing = 0;
        base.width = 831;
        let (_, y) = place(&mut base, &agency_notdef(), ATTACHED_BELOW, 2048 / 16, false);
        assert_eq!(y, -1633);
    }

    #[test]
    fn a_capital_is_measured_by_its_own_box() {
        // `A` + U+0323 on AGENCYB: ink 37..887 by 0..1567, advance 924, and
        // HarfBuzz reports (-974, -1761).
        let mut base = Extents::new(37, 0, 887, 1567);
        base.x_bearing = 0;
        base.width = 924;
        let (x, y) = place(&mut base, &agency_notdef(), BELOW, 2048 / 16, false);
        assert_eq!((x - 924, y), (-974, -1761));
    }

    #[test]
    fn alignment_follows_the_class() {
        let gap = 2048 / 16;
        let mark = agency_notdef();
        let centred = place(&mut agency_a(), &mark, ABOVE, gap, false).0;
        let left = place(&mut agency_a(), &mark, ABOVE_LEFT, gap, false).0;
        let right = place(&mut agency_a(), &mark, ABOVE_RIGHT, gap, false).0;
        // Base 0..815, mark ink 128..896 (width 768).
        assert_eq!(left, -128);
        assert_eq!(centred, (815 - 768) / 2 - 128);
        assert_eq!(right, 815 - 768 - 128);
        assert!(left < centred && centred < right);
    }

    #[test]
    fn a_double_mark_hangs_off_the_edge_the_next_glyph_is_on() {
        let gap = 2048 / 16;
        let mark = agency_notdef();
        let ltr = place(&mut agency_a(), &mark, DOUBLE_ABOVE, gap, false).0;
        let rtl = place(&mut agency_a(), &mark, DOUBLE_ABOVE, gap, true).0;
        assert_eq!(ltr, 815 - 768 / 2 - 128);
        assert_eq!(rtl, -768 / 2 - 128);
    }

    #[test]
    fn a_class_that_only_orders_does_not_move_the_mark_vertically() {
        // Dagesh (class 21) is inside the letter; nothing describes that, so
        // it centres and stays on the baseline.
        assert_eq!(attach_class('\u{05BC}'), 21);
        let mut base = agency_a();
        let before = base;
        let (x, y) = place(&mut base, &agency_notdef(), 21, 2048 / 16, false);
        assert_eq!(y, 0);
        assert_eq!(x, (815 - 768) / 2 - 128);
        // And it leaves no footprint for a following mark to clear.
        assert_eq!(base, before);
    }
}
