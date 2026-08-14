//! `GSUB`: the glyph the font asks for, in place of the one `cmap` gave.
//!
//! # Why this is not a flourish
//!
//! In most serif faces the `f` ends in a hood that overhangs to the right, and
//! the `i` has a dot. Set next to each other at their nominal advances the
//! hood and the dot overlap, or nearly do, and the pair reads as a smudge. The
//! designer's answer is a single `fi` glyph with the collision resolved by
//! hand, plus an instruction in the font to use it. `ffi`, `ffl`, `fl` and
//! `ff` are the same problem. Ignoring the instruction does not render plain
//! text — it renders text the designer specifically marked as broken.
//!
//! That is why these features are *on by default* in every text engine, unlike
//! `dlig` (discretionary ligatures — `ct`, `st`, the decorative ones), which
//! is off by default and stays off here.
//!
//! # How it is applied
//!
//! A `GSUB` table is a *list of lookups*, and the list is applied in order:
//! the whole of the first lookup runs across the whole run before any of the
//! second does, so what the first substitutes is what the second sees. That
//! ordering is the whole mechanism by which `ccmp` — which normalises a run
//! into the glyphs the rest of the table expects — reliably runs before the
//! ligature lookups that depend on it.
//!
//! Within one lookup the subtables are tried in the order the font lists them
//! and the first that matches wins; a glyph one subtable has already
//! substituted is not offered to the next. Positions are walked left to right,
//! once per lookup.
//!
//! # What is read
//!
//! Features, all of them on by default in every engine:
//!
//! * **`ccmp`** — glyph composition and decomposition. Normalises a run so the
//!   later lookups, and mark attachment, see what they expect.
//! * **`locl`** — localized forms. Under the DefaultLangSys — the only
//!   LangSys this crate reads — this is the face's *default* localization,
//!   which is what every shaper applies when the caller names no language.
//!   Leaving it out was visible: Sans Serif Collection maps `space` through
//!   its Latin `locl`, so every space in every Latin string came out as the
//!   wrong glyph. It is safe to read only because features are now chosen per
//!   script; under the old script-blind walk it would have handed a Latin run
//!   some other writing system's letterforms.
//! * **`liga`** — standard ligatures. The `fi` family.
//! * **`rlig`** — *required* ligatures. For Latin this is nearly empty; for
//!   Arabic, `lam-alef` is not optional, and a face that has it will look
//!   wrong without it. Reading it costs nothing beyond a second tag.
//! * **`clig`, `calt`, `rclt`** — contextual ligatures, contextual alternates
//!   and required contextual alternates: the features lookup types 5 and 6
//!   exist for. All three are default-on in HarfBuzz too.
//!
//! And the four *positional* features, which are on by default but not
//! unconditional — see "Positional forms" below:
//!
//! * **`isol`, `init`, `medi`, `fina`** — the cursive forms of a letter
//!   standing alone, starting a word, inside one, and ending one.
//!
//! Lookup types:
//!
//! * **1, `SingleSubst`** — one glyph for one glyph, in both its formats: a
//!   delta applied to every covered glyph, or an explicit list.
//! * **2, `MultipleSubst`** — one glyph becomes several, each carrying the
//!   cluster of the character behind it. This is how `ccmp` decomposes a
//!   precomposed letter into a base and a mark so that GPOS can then attach
//!   the mark; without it, a face that ships only the decomposed forms draws
//!   the missing-glyph box for text that is perfectly well spelled.
//! * **3, `AlternateSubst`** — one glyph, a set of candidates, and an index
//!   into the set that comes from the value the caller gave the feature. Every
//!   feature read here is on-or-off, and "on" is the value 1, which selects the
//!   first alternate. Microsoft Uighur writes its positional substitutions this
//!   way, so skipping the type meant leaving every Uighur word unjoined.
//! * **4, `LigatureSubst`** — several glyphs for one.
//!
//! # Positional forms, and why a feature tag is not enough
//!
//! Arabic is written joined, and a letter's glyph depends on its neighbours.
//! A face ships the four forms and reaches them through `isol`, `init`, `medi`
//! and `fina` — but those lookups are ordinary type-1 substitutions whose
//! coverage is, typically, *every* Arabic letter in the face. Applying `fina`
//! the way `liga` is applied would rewrite a whole word into final forms.
//!
//! So each glyph carries a mask of the features it is eligible for, and each
//! lookup carries the mask of the features that reached it (see
//! [`otl`](crate::otl)). A lookup is offered a position only when the two
//! intersect. The unconditional features set their bit on every glyph, so they
//! behave exactly as before; the positional four set theirs on the one glyph
//! [`joining`](crate::joining) says takes that form.
//!
//! The mask is checked at the position a lookup is *applied* to, and not on
//! the glyphs a ligature or a context goes on to match. HarfBuzz checks both.
//! The difference can only show up in a face whose positional feature reaches
//! a multi-glyph lookup — real ones use type 1 — and closing it means
//! threading the mask through every matcher. Noted rather than guessed at.
//!
//! # What is deliberately not implemented
//!
//! * **Alternates past the first.** Type 3 is applied, but only ever with
//!   index 1, because there is no per-run feature list for a caller to say
//!   "the third swash" with. That is the right answer for every feature read
//!   here, all of which are on-or-off; it would be the wrong one for `aalt`,
//!   which is not read.
//! * **Lookup flags.** `IgnoreMarks`, `IgnoreLigatures`, `IgnoreBaseGlyphs`
//!   and the mark-filtering sets are not honoured, so a lookup that means to
//!   step over a combining mark is instead stopped by it. Tracked as
//!   `TD-FONT-IGNORES-GSUB-LOOKUP-FLAGS`.
//! * **`dlig`, `hlig`, `swsh`** and the other opt-in features, which are off
//!   by default by design and have no way to be turned on yet — there is no
//!   per-run feature list to turn them on *with*.
//! * **Language selection.** Only each script's DefaultLangSys is read, so a
//!   `locl` override registered under `SRB ` or `TRK ` is not reached. See
//!   [`otl`](crate::otl) and `TD-FONT-IGNORES-LANGSYS-OVERRIDES`.
//! * **The reordering features** — `rphf`, `half`, `pref`, `abvs` and the rest
//!   of the Indic and Universal Shaping Engine sets. Unlike the Arabic four,
//!   these need the cluster *rearranged* before the features are chosen, which
//!   is a shaper this crate does not have yet. See
//!   `TD-FONT-HAS-NO-JOINING-OR-REORDERING-SHAPER`.
//! * **Syriac's `fin2`, `fin3` and `med2`**, the alaph forms, and Arabic's
//!   `mset` and `stch`. See [`joining`](crate::joining).

use alloc::vec::Vec;

use crate::joining::Form;
use crate::otl::{
    ByScript, Lookup, MAX_SUBTABLES, coverage_index, glyph_class, lookup_at, lookup_list,
};
use crate::script::ScriptTags;
use crate::sfnt::{Span, u16_at};
use crate::skip::{Definitions, Skipper};

/// The features read, in the order whose positions become the mask bits.
///
/// The unconditional ones come first so that "every feature a glyph always
/// gets" is one contiguous run of bits ([`ALWAYS`]); the four positional ones
/// follow, one bit each.
const FEATURES: &[&[u8; 4]] = &[
    // Unconditional: every glyph is eligible for all of these.
    b"ccmp", b"locl", b"liga", b"rlig", b"clig", b"calt", b"rclt",
    // Positional: a glyph is eligible for at most one, and only when the
    // cursive joining pass says so.
    b"isol", b"init", b"medi", b"fina",
];

/// The feature mask every glyph carries: bits for the seven unconditional
/// entries of [`FEATURES`], and none of the four positional ones.
///
/// Written out rather than computed from `FEATURES`, so that no shift or
/// subtraction appears in a path the arithmetic lints police. The two are
/// kept in step by `the_masks_match_the_feature_list`, which fails if an
/// entry is ever inserted or reordered.
const ALWAYS: u32 = 0b0111_1111;
/// The bit for `isol`, the eighth entry of [`FEATURES`].
const ISOL: u32 = 0b1000_0000;
/// The bit for `init`, the ninth.
const INIT: u32 = 0b1_0000_0000;
/// The bit for `medi`, the tenth.
const MEDI: u32 = 0b10_0000_0000;
/// The bit for `fina`, the eleventh.
const FINA: u32 = 0b100_0000_0000;

/// The mask for a glyph whose cursive form is `form`.
///
/// `None` — a space, a mark, a Latin letter, anything in a face with no
/// cursive script at all — gets the unconditional features only.
fn form_mask(form: Option<Form>) -> u32 {
    ALWAYS
        | match form {
            None => 0,
            Some(Form::Isolated) => ISOL,
            Some(Form::Initial) => INIT,
            Some(Form::Medial) => MEDI,
            Some(Form::Final) => FINA,
        }
}

/// `GSUB` lookup type for single substitution: one glyph for one glyph.
const LOOKUP_SINGLE: u16 = 1;
/// `GSUB` lookup type for multiple substitution: one glyph becomes several.
const LOOKUP_MULTIPLE: u16 = 2;
/// `GSUB` lookup type for alternate substitution: one glyph, several
/// candidates, an index chooses.
const LOOKUP_ALTERNATE: u16 = 3;
/// `GSUB` lookup type for ligature substitution: several glyphs for one.
const LOOKUP_LIGATURE: u16 = 4;
/// `GSUB` lookup type for contextual substitution: a rule that fires only
/// where a given sequence of glyphs stands, and then invokes other lookups.
const LOOKUP_CONTEXT: u16 = 5;
/// `GSUB` lookup type for chaining contextual substitution: like
/// [`LOOKUP_CONTEXT`], but the context extends either side of what is
/// substituted.
const LOOKUP_CHAIN_CONTEXT: u16 = 6;
/// `GSUB` lookup type for an extension, which wraps a subtable of another type
/// at a 32-bit offset. `GPOS` numbers its own extension 9; the two tables
/// number their lookup types independently.
const LOOKUP_EXTENSION: u16 = 7;

/// The lookup types a nested invocation may reach, which is every type this
/// module applies — a contextual lookup may invoke another contextual one.
const NESTABLE: &[u16] = &[
    LOOKUP_SINGLE,
    LOOKUP_MULTIPLE,
    LOOKUP_ALTERNATE,
    LOOKUP_LIGATURE,
    LOOKUP_CONTEXT,
    LOOKUP_CHAIN_CONTEXT,
];

/// A ceiling on how many glyphs one ligature may swallow.
///
/// The largest in real use is four (`ffi`, `ffl`, and Arabic's four-component
/// forms). The cap is what stops a corrupt `componentCount` from making every
/// position in a line scan to the end of it.
const MAX_COMPONENTS: usize = 16;

/// A ceiling on how many glyphs one glyph may decompose into.
///
/// The inverse of [`MAX_COMPONENTS`] and set to match it: real decompositions
/// are two or three glyphs (a base and its marks), and a font claiming to turn
/// one glyph into thousands is not a font a run should be resized for. The cap
/// bounds the growth of the buffer, which is otherwise the one place
/// substitution can allocate without limit.
const MAX_SEQUENCE: usize = 16;

/// A ceiling on how long a context — backtrack, input or lookahead — may be.
///
/// Every glyph of a context is compared at every position of the run, so an
/// unbounded one makes matching quadratic in the run length. Real contexts are
/// two or three glyphs; the longest in common use are the Indic reordering
/// rules, still well inside this.
const MAX_CONTEXT: usize = 16;

/// A ceiling on how many lookups one context match may invoke.
const MAX_NESTED: usize = 16;

/// A ceiling on how many rules one rule set may hold.
///
/// A rule set is already keyed by the first glyph or its class, so a real one
/// holds a handful; the cap is what stops a corrupt `seqRuleCount` from making
/// every position in a line scan tens of thousands of rules.
const MAX_RULES: usize = 256;

/// How deep a contextual lookup's invocations may nest.
///
/// A contextual lookup may invoke another contextual lookup, and nothing in
/// the format forbids that from being a cycle — lookup 3 invoking lookup 3.
/// Matching HarfBuzz's limit, which real fonts stay far inside.
const MAX_NESTING: usize = 6;

/// A glyph on its way through substitution.
///
/// Carries its cluster along with its id because substitution is what makes
/// the two diverge: a ligature swallows several glyphs and keeps the first
/// one's cluster, so the run that comes out no longer has one entry per
/// character and only this pass knows which entries merged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubGlyph {
    /// The glyph id, as `cmap` first gave it and each lookup since may have
    /// replaced it.
    pub gid: u16,
    /// Byte offset in the source string of the first character behind this
    /// glyph. A ligature keeps its first component's, which is the only place
    /// a caret can honestly be drawn: the joined glyph has no interior
    /// boundary to point at.
    pub cluster: usize,
    /// Which features this glyph is eligible for, one bit per entry of
    /// `FEATURES`. Crate-private because it is this module's own bookkeeping:
    /// a caller has no way to know which bit is which, and setting it wrongly
    /// would silently disable ligatures. [`skip`](crate::skip) reads it because
    /// it is the module that decides, for every matcher, whether a position is
    /// eligible.
    pub(crate) mask: u32,
}

impl SubGlyph {
    /// A glyph eligible for the unconditional features and no others — which
    /// is every glyph outside a cursive script.
    #[must_use]
    pub fn new(gid: u16, cluster: usize) -> Self {
        Self {
            gid,
            cluster,
            mask: ALWAYS,
        }
    }

    /// The same, but eligible for the positional feature that `form` names.
    ///
    /// `None` is identical to [`new`](Self::new): a character that takes no
    /// cursive form is not eligible for any of the four.
    #[must_use]
    pub(crate) fn cursive(gid: u16, cluster: usize, form: Option<Form>) -> Self {
        Self {
            gid,
            cluster,
            mask: form_mask(form),
        }
    }

    /// A glyph eligible for exactly `mask`.
    ///
    /// For tests of the eligibility gate itself, which need masks that do not
    /// correspond to any real form: the two production constructors between
    /// them can only make a glyph eligible for every unconditional feature,
    /// which is precisely the case that cannot show the gate working.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn masked(gid: u16, cluster: usize, mask: u32) -> Self {
        Self { gid, cluster, mask }
    }
}

/// The substitutions of one face, as the lookups to run and in what order.
///
/// Offsets rather than decoded tables, for the same reason as
/// [`Kerning`](crate::kern): the tables are already indexed for lookup, and a
/// face that is drawn with touches a handful of the entries in them.
#[derive(Clone, Debug)]
pub(crate) struct Substitutions {
    /// The lookups reachable from the default-on features, keyed by the script
    /// that reaches them and in the order the font's LookupList puts them,
    /// which is the order they apply in.
    lookups: ByScript,
    /// Where the font's LookupList begins. Kept because a contextual lookup
    /// names the lookups it invokes by index into it, and those may be lookups
    /// no feature reaches — which is exactly how a font hides a helper.
    lookup_list: usize,
    /// The `GDEF` class definitions a lookup flag consults to decide which
    /// glyphs it is allowed to see. Kept here rather than looked up per lookup
    /// because they are a property of the face, not of the rule.
    defs: Definitions,
}

impl Substitutions {
    /// Find this face's substitutions.
    ///
    /// Returns `None` when the face has no `GSUB`, or has one with no
    /// default-on feature reaching a lookup type this can apply — which is not
    /// an error. Monospace faces in particular have no ligatures by design: a
    /// ligature would break the grid.
    pub(crate) fn parse(data: &[u8], gsub: Option<Span>, gdef: Option<Span>) -> Option<Self> {
        let base = gsub?.off;
        let lookups = ByScript::parse(data, base, FEATURES, NESTABLE, LOOKUP_EXTENSION)?;
        Some(Self {
            lookups,
            lookup_list: lookup_list(data, base)?,
            defs: Definitions::parse(data, gdef),
        })
    }

    /// Apply every lookup this face offers a run of `script` to `glyphs`, in
    /// order, rewriting it in place.
    ///
    /// `glyphs` is one substitution run and the lookups may join anything in
    /// it, so a caller that does not want a ligature to form across some
    /// boundary of its own — a tab, a style change, a bidi run edge — passes
    /// the pieces separately rather than the whole line. A script boundary is
    /// such a boundary, and the reason `script` is a parameter rather than a
    /// property of the face: applying Arabic's `liga` to a Latin word is how a
    /// face that supports both silently corrupts one of them.
    pub(crate) fn apply(
        &self,
        data: &[u8],
        script: Option<ScriptTags>,
        glyphs: &mut Vec<SubGlyph>,
    ) {
        let mut ctx = Ctx {
            lookup_list: self.lookup_list,
            depth: MAX_NESTING,
            scratch: Vec::new(),
            defs: self.defs,
            mask: ALWAYS,
        };
        for (lookup, mask) in self.lookups.for_script(script) {
            ctx.mask = mask;
            apply_lookup(data, lookup, glyphs, &mut ctx);
        }
    }
}

/// What a lookup needs beyond the run it is rewriting.
///
/// Exists because of types 5 and 6: a contextual lookup invokes *other*
/// lookups by index, so applying one needs the LookupList to resolve them in
/// and a bound on how far the invocations may nest.
struct Ctx {
    /// Where the font's LookupList begins, for resolving a nested lookup's
    /// index.
    lookup_list: usize,
    /// How many more levels of nested invocation are allowed. A contextual
    /// lookup may invoke another contextual lookup; nothing in the format
    /// stops that from being a cycle.
    depth: usize,
    /// A buffer for a multiple substitution's replacement sequence, hoisted to
    /// the pass so that a run of ordinary text — where nothing matches — does
    /// not allocate once per position.
    scratch: Vec<u16>,
    /// The face's `GDEF` class definitions, which a lookup's flag consults to
    /// decide which glyphs it may see.
    defs: Definitions,
    /// The features that reached the lookup currently being applied.
    ///
    /// It does *not* change on the way into a nested lookup: a contextual rule
    /// reached by `fina` is still a `fina` rule when it invokes a helper, and
    /// the helper's own coverage is what decides whether it fires. The flag,
    /// by contrast, is the nested lookup's own — which is why a skipper is
    /// built per invocation rather than passed down.
    mask: u32,
}

/// Run one lookup across the whole run, left to right.
///
/// Each lookup type is a rule about *one position*; this is what turns a rule
/// into a pass. After a match, walking resumes past what the match produced
/// rather than at the next glyph, so a lookup is never offered its own output.
/// That is what stops a font whose output is also its input — a ligature of
/// itself, a glyph that decomposes to itself — from looping, and it lives here
/// rather than in each type so that no type can forget it.
///
/// A position the lookup does not consider — because its flag hides the glyph,
/// or because the glyph is not eligible for the features that reached the
/// lookup — is stepped over. For every feature but the cursive four the
/// eligibility half is no restriction at all, since their bits are set on
/// every glyph, so ordinary text pays a comparison rather than a branch.
fn apply_lookup(data: &[u8], lookup: &Lookup, glyphs: &mut Vec<SubGlyph>, ctx: &mut Ctx) {
    let skip = skipper(lookup, data, ctx);
    let mut i = 0usize;
    while i < glyphs.len() {
        if !skip.considers(glyphs, i) {
            i = i.saturating_add(1);
            continue;
        }
        // A match that somehow produced nothing would leave `i` where it was;
        // the floor of one is what makes the walk terminate regardless.
        let step = apply_at(data, lookup, glyphs, i, ctx).unwrap_or(0).max(1);
        i = i.saturating_add(step);
    }
}

/// The view of the run that `lookup` is entitled to, under the features
/// currently being applied.
fn skipper<'a>(lookup: &Lookup, data: &'a [u8], ctx: &Ctx) -> Skipper<'a> {
    Skipper::new(data, ctx.defs, lookup.flag, lookup.filter, ctx.mask)
}

/// Apply one lookup at exactly one position, if it matches there.
///
/// Returns how many glyphs now stand where `glyphs[i]` stood: one for a single
/// substitution, the sequence length for a multiple one, one for a ligature
/// that swallowed several, and the rewritten length of the matched span for a
/// contextual one. `None` means nothing matched and the run is untouched.
///
/// This is also the entry point a contextual lookup calls back into, which is
/// why it is "at a position" rather than "across the run": a nested lookup
/// applies where the context matched and nowhere else.
fn apply_at(
    data: &[u8],
    lookup: &Lookup,
    glyphs: &mut Vec<SubGlyph>,
    i: usize,
    ctx: &mut Ctx,
) -> Option<usize> {
    let subs = &lookup.subtables;
    // Built from *this* lookup's flag, not the caller's: a contextual lookup
    // that invokes a helper does not lend the helper its own view of the run.
    let skip = skipper(lookup, data, ctx);
    match lookup.kind {
        LOOKUP_SINGLE => apply_single(data, subs, glyphs, i),
        LOOKUP_MULTIPLE => apply_multiple(data, subs, glyphs, i, ctx),
        LOOKUP_ALTERNATE => apply_alternate(data, subs, glyphs, i),
        LOOKUP_LIGATURE => apply_ligature(data, subs, glyphs, i, skip),
        LOOKUP_CONTEXT => apply_context(data, subs, glyphs, i, skip, ctx),
        LOOKUP_CHAIN_CONTEXT => apply_chain_context(data, subs, glyphs, i, skip, ctx),
        // `feature_lookups` and `lookup_at` are both asked for these types
        // only, so there is nothing else to reach here; ignoring anything that
        // does is what keeps adding a type to those lists from being able to
        // silently corrupt a run.
        _ => None,
    }
}

/// Apply a `SingleSubst` lookup at one position.
///
/// The position is independent of every other — nothing here can look at a
/// neighbour — so the run's length and clusters come out unchanged.
fn apply_single(data: &[u8], subtables: &[usize], glyphs: &mut [SubGlyph], i: usize) -> Option<usize> {
    let glyph = glyphs.get_mut(i)?;
    // First subtable that covers the glyph wins, and the result is not offered
    // to the rest: within one lookup a glyph is substituted once.
    let gid = subtables
        .iter()
        .find_map(|&sub| single_at(data, sub, glyph.gid))?;
    glyph.gid = gid;
    Some(1)
}

/// The glyph one `SingleSubst` subtable puts in place of `glyph`.
fn single_at(data: &[u8], sub: usize, glyph: u16) -> Option<u16> {
    // Both formats put the coverage offset in the same place, and neither
    // substitutes a glyph it does not cover.
    let coverage = sub.checked_add(usize::from(u16_at(data, sub.checked_add(2)?)?))?;
    let index = coverage_index(data, coverage, glyph)?;
    match u16_at(data, sub)? {
        // Format 1: one delta shared by every covered glyph, for the common
        // case of a block of related forms laid out in the same order as the
        // originals. The spec's arithmetic is modulo 65536, so this wraps
        // rather than saturating or refusing.
        1 => Some(glyph.wrapping_add(u16_at(data, sub.checked_add(4)?)?)),
        // Format 2: an explicit replacement per covered glyph, in coverage
        // order.
        2 => {
            let count = u16_at(data, sub.checked_add(4)?)?;
            if index >= count {
                return None;
            }
            let at = sub
                .checked_add(6)?
                .checked_add(usize::from(index).checked_mul(2)?)?;
            u16_at(data, at)
        }
        _ => None,
    }
}

/// Apply a `MultipleSubst` lookup at one position.
///
/// The run grows: the glyph becomes a sequence, and every glyph of that
/// sequence carries the cluster of the one it replaced, because they all came
/// from the same character. That is what makes `ShapedGlyph::cluster` a
/// many-to-many mapping, and why the queries on
/// [`ShapedRun`](crate::shape::ShapedRun) work in whole clusters.
fn apply_multiple(
    data: &[u8],
    subtables: &[usize],
    glyphs: &mut Vec<SubGlyph>,
    i: usize,
    ctx: &mut Ctx,
) -> Option<usize> {
    let glyph = *glyphs.get(i)?;
    subtables
        .iter()
        .find_map(|&sub| sequence_at(data, sub, glyph.gid, &mut ctx.scratch))?;
    // `sequence_at` owns the buffer: it clears on entry and only returns
    // `Some` after pushing at least one glyph, so a match here is never empty
    // and never carries a failed subtable's partial read. Checking emptiness
    // again would be dead code that hides the guard inside.
    let grown = ctx.scratch.len();
    // Every glyph of the sequence inherits the source's cluster *and* its
    // feature mask: they all came from the one character, so they are all
    // eligible for exactly what it was. A `ccmp` that splits a letter into a
    // base and a mark must not leave the base ineligible for `fina`.
    glyphs.splice(i..=i, ctx.scratch.iter().map(|&gid| SubGlyph { gid, ..glyph }));
    Some(grown)
}

/// Apply an `AlternateSubst` lookup at one position.
///
/// The subtable offers a *set* of glyphs for each covered one and leaves the
/// choice to the caller — the format exists for `aalt`, "access all
/// alternates", where an application shows the user a menu of swashes. Nothing
/// here has a menu, so why apply it at all?
///
/// Because the choice is only open for a feature the caller gave a *value*.
/// An on-by-default feature has the value 1, and 1 selects the first
/// alternate: OpenType numbers them from one, so an alternate substitution
/// reached from a boolean feature is a single substitution with extra steps.
/// HarfBuzz arrives at the same answer by the same route — it packs the
/// feature's value into the glyph mask and indexes with it, which for a
/// boolean feature is always 1.
///
/// And it matters: Microsoft Uighur writes its `init`, `medi` and `fina` as
/// type 3. Skipping the type left every Uighur word in isolated forms while
/// every other Arabic face on the host shaped correctly.
///
/// The position is independent of every other, so the run's length and
/// clusters come out unchanged.
fn apply_alternate(
    data: &[u8],
    subtables: &[usize],
    glyphs: &mut [SubGlyph],
    i: usize,
) -> Option<usize> {
    let glyph = glyphs.get_mut(i)?;
    let gid = subtables
        .iter()
        .find_map(|&sub| alternate_at(data, sub, glyph.gid))?;
    glyph.gid = gid;
    Some(1)
}

/// The first alternate one `AlternateSubst` subtable offers for `glyph`.
fn alternate_at(data: &[u8], sub: usize, glyph: u16) -> Option<u16> {
    // Only one format is defined; a subtable claiming another is one this
    // cannot read rather than one to guess at.
    if u16_at(data, sub)? != 1 {
        return None;
    }
    let coverage = sub.checked_add(usize::from(u16_at(data, sub.checked_add(2)?)?))?;
    let index = coverage_index(data, coverage, glyph)?;
    let count = u16_at(data, sub.checked_add(4)?)?;
    if index >= count {
        return None;
    }
    let at = sub
        .checked_add(6)?
        .checked_add(usize::from(index).checked_mul(2)?)?;
    let set = sub.checked_add(usize::from(u16_at(data, at)?))?;
    // An empty set has no first alternate, which is a subtable with nothing to
    // say rather than a reason to substitute glyph zero.
    if u16_at(data, set)? == 0 {
        return None;
    }
    u16_at(data, set.checked_add(2)?)
}

/// The sequence one `MultipleSubst` subtable puts in place of `glyph`, written
/// into `out`.
///
/// `out` is cleared here rather than by the caller, because the caller tries
/// the subtables of a lookup in turn: a subtable that reads half a sequence and
/// then finds the table truncated must not leave those glyphs in front of the
/// next subtable's answer. Clearing on entry makes `out` mean "what the
/// subtable that returned `Some` matched", nothing more. Returning the glyphs
/// through a buffer rather than a fresh `Vec` is what keeps a run of ordinary
/// text — where nothing matches — from allocating once per position.
fn sequence_at(data: &[u8], sub: usize, glyph: u16, out: &mut Vec<u16>) -> Option<()> {
    out.clear();
    // Only one format is defined, and a subtable claiming another is one this
    // cannot read rather than one to guess at.
    if u16_at(data, sub)? != 1 {
        return None;
    }
    let coverage = sub.checked_add(usize::from(u16_at(data, sub.checked_add(2)?)?))?;
    let index = coverage_index(data, coverage, glyph)?;
    let count = u16_at(data, sub.checked_add(4)?)?;
    if index >= count {
        return None;
    }
    let at = sub
        .checked_add(6)?
        .checked_add(usize::from(index).checked_mul(2)?)?;
    let sequence = sub.checked_add(usize::from(u16_at(data, at)?))?;
    let glyph_count = usize::from(u16_at(data, sequence)?);
    // A sequence of length zero would delete the glyph. The spec forbids it,
    // and some shapers honour it anyway for compatibility — but a deleted
    // glyph takes its cluster with it, and a character that no query can name
    // a position for is worse than a character drawn as it arrived. Refusing
    // here rather than in the caller is what lets a *later* subtable of the
    // same lookup still have its say.
    if glyph_count == 0 || glyph_count > MAX_SEQUENCE {
        return None;
    }
    for i in 0..glyph_count {
        let at = sequence
            .checked_add(2)?
            .checked_add(i.checked_mul(2)?)?;
        out.push(u16_at(data, at)?);
    }
    Some(())
}

/// Apply a `LigatureSubst` lookup at one position.
///
/// The matched components collapse to one glyph, which keeps the first one's
/// cluster: the joined glyph has no interior boundary a caret could point at,
/// so the characters it swallowed all answer with the offset of the first.
///
/// Glyphs the lookup's flag hid — marks, typically — stand *between* the
/// components and are not part of the match. They are left where they are and
/// simply close up behind the removed components, so a vowelled Arabic word
/// keeps its vowels after its letters ligate. That is a simplification of what
/// HarfBuzz does, which is to reposition each skipped mark against the
/// component it belonged to; ours leaves them in run order, which is right
/// whenever the marks sit at the end of the ligature's own span and is what
/// almost every face produces.
fn apply_ligature(
    data: &[u8],
    subtables: &[usize],
    glyphs: &mut Vec<SubGlyph>,
    i: usize,
    skip: Skipper<'_>,
) -> Option<usize> {
    let mut at = [0usize; MAX_COMPONENTS];
    let (gid, count) = subtables
        .iter()
        .find_map(|&sub| ligature_at(data, sub, glyphs, i, skip, &mut at))?;
    if let Some(first) = glyphs.get_mut(i) {
        // The cluster stays as it was: it is the first component's, and the
        // components that follow are being swallowed, not moved.
        first.gid = gid;
    }
    // Removed from the back so that the earlier indices stay valid. Component
    // zero is the glyph just rewritten and stays.
    let end = at.get(count.checked_sub(1)?).copied()?;
    for k in (1..count).rev() {
        let Some(&pos) = at.get(k) else { continue };
        if pos < glyphs.len() {
            glyphs.remove(pos);
        }
    }
    // How many glyphs now stand where the match stood: the span it covered,
    // less the components taken out of it. Anything the flag skipped is still
    // in there, which is why this is not simply one.
    let span = end.checked_sub(i)?.checked_add(1)?;
    Some(span.saturating_sub(count.saturating_sub(1)).max(1))
}

/// Look for a ligature starting at `glyphs[i]` in one `LigatureSubst`
/// subtable, recording each matched component's position in `at`.
fn ligature_at(
    data: &[u8],
    sub: usize,
    glyphs: &[SubGlyph],
    i: usize,
    skip: Skipper<'_>,
    at: &mut [usize; MAX_COMPONENTS],
) -> Option<(u16, usize)> {
    if u16_at(data, sub)? != 1 {
        return None;
    }
    let first = glyphs.get(i)?.gid;
    let coverage = sub.checked_add(usize::from(u16_at(data, sub.checked_add(2)?)?))?;
    let index = coverage_index(data, coverage, first)?;

    let set_count = u16_at(data, sub.checked_add(4)?)?;
    if index >= set_count {
        return None;
    }
    let off = sub
        .checked_add(6)?
        .checked_add(usize::from(index).checked_mul(2)?)?;
    let set = sub.checked_add(usize::from(u16_at(data, off)?))?;

    // The set is ordered by the font, longest first by convention, and the
    // first match wins — which is what makes `ffi` beat `ff` in one pass.
    let count = u16_at(data, set)?;
    for k in 0..usize::from(count) {
        let off = set.checked_add(2)?.checked_add(k.checked_mul(2)?)?;
        let Some(lig) = u16_at(data, off).and_then(|o| set.checked_add(usize::from(o))) else {
            continue;
        };
        if let Some(hit) = ligature_matches(data, lig, glyphs, i, skip, at) {
            return Some(hit);
        }
    }
    None
}

/// Test one `Ligature` record against the run from `i`, stepping over whatever
/// the lookup's flag hides.
///
/// The record lists its components from the *second* onwards: the first is
/// the one the coverage table already matched, so storing it again would be
/// storing it twice.
fn ligature_matches(
    data: &[u8],
    lig: usize,
    glyphs: &[SubGlyph],
    i: usize,
    skip: Skipper<'_>,
    at: &mut [usize; MAX_COMPONENTS],
) -> Option<(u16, usize)> {
    let glyph = u16_at(data, lig)?;
    let components = usize::from(u16_at(data, lig.checked_add(2)?)?);
    if components < 2 || components > MAX_COMPONENTS {
        return None;
    }
    *at.get_mut(0)? = i;
    let mut pos = i;
    for k in 1..components {
        let want = u16_at(
            data,
            lig.checked_add(4)?
                .checked_add(k.checked_sub(1)?.checked_mul(2)?)?,
        )?;
        pos = skip.next(glyphs, pos)?;
        if glyphs.get(pos)?.gid != want {
            return None;
        }
        *at.get_mut(k)? = pos;
    }
    Some((glyph, components))
}

/// A `SequenceLookupRecord`: run lookup `lookup` at input position `at`.
#[derive(Clone, Copy, Debug)]
struct Nested {
    /// Which glyph *of the matched input* to run the lookup at — not a
    /// position in the run. The two differ once an earlier record has changed
    /// the run's length.
    at: u16,
    /// Index into the font's LookupList.
    lookup: u16,
}

/// What a contextual subtable matched.
///
/// `at` is what makes this a struct rather than a pair of counts: once the
/// lookup's flag can hide a glyph, the matched input is no longer the run of
/// positions `i..i + input`, and the nested lookups have to be told where the
/// glyphs they name actually are.
struct Matched {
    /// Absolute positions of the matched input glyphs, in order.
    at: [usize; MAX_CONTEXT],
    /// How many entries of `at` are real.
    input: usize,
    /// One past the last matched input position: the span the match covers is
    /// `i..end`, which includes anything the flag skipped inside it.
    end: usize,
    /// Where the `SequenceLookupRecord` array is.
    records: usize,
    /// How many records it holds.
    count: usize,
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
}

/// Apply a `SequenceContext` (type 5) lookup at one position.
fn apply_context(
    data: &[u8],
    subtables: &[usize],
    glyphs: &mut Vec<SubGlyph>,
    i: usize,
    skip: Skipper<'_>,
    ctx: &mut Ctx,
) -> Option<usize> {
    // These two buffers are locals rather than fields of `Ctx` on purpose: a
    // nested lookup may be another contextual one, which would be using the
    // same buffers a level up. Reentrancy is worth an allocation on the rare
    // position where a coverage matched.
    let mut rules = Vec::new();
    let mut records = Vec::new();
    for &sub in subtables {
        // First subtable that matches wins, as everywhere else in a lookup.
        let Some(hit) = context_match(data, sub, glyphs, i, skip, &mut rules) else {
            continue;
        };
        read_records(data, hit.records, hit.count, &mut records);
        return Some(apply_nested(data, &records, glyphs, i, &hit, ctx));
    }
    None
}

/// Apply a `ChainedSequenceContext` (type 6) lookup at one position.
fn apply_chain_context(
    data: &[u8],
    subtables: &[usize],
    glyphs: &mut Vec<SubGlyph>,
    i: usize,
    skip: Skipper<'_>,
    ctx: &mut Ctx,
) -> Option<usize> {
    let mut rules = Vec::new();
    let mut records = Vec::new();
    for &sub in subtables {
        let Some(hit) = chain_match(data, sub, glyphs, i, skip, &mut rules) else {
            continue;
        };
        read_records(data, hit.records, hit.count, &mut records);
        return Some(apply_nested(data, &records, glyphs, i, &hit, ctx));
    }
    None
}

/// Run the lookups a context match calls for, and report how many glyphs the
/// matched input span occupies afterwards.
///
/// The bookkeeping is the whole of the difficulty. A record names a glyph *of
/// the input as it was matched*, but the lookup it invokes may grow or shrink
/// the run — so by the time a later record runs, the glyph it names has moved,
/// or a ligature has swallowed it and it is not there at all. Tracking where
/// each matched glyph now stands, and marking the ones that stopped existing,
/// is what keeps a record from landing on a glyph the context never matched.
fn apply_nested(
    data: &[u8],
    records: &[Nested],
    glyphs: &mut Vec<SubGlyph>,
    start: usize,
    hit: &Matched,
    ctx: &mut Ctx,
) -> usize {
    // The span the match covers, which is not the number of glyphs it matched:
    // anything the lookup's flag skipped stands inside it and still occupies a
    // position the caller must step over.
    let mut span = hit.end.saturating_sub(start);
    // Out of depth: the context still counts as matched, so the caller steps
    // over it, but nothing is invoked. Silently applying at depth zero is what
    // would let a lookup that invokes itself run forever.
    if ctx.depth == 0 {
        return span.max(1);
    }
    let mut positions: Vec<Option<usize>> = hit
        .at
        .get(..hit.input)
        .unwrap_or_default()
        .iter()
        .map(|&p| Some(p))
        .collect();

    for rec in records {
        let Some(Some(at)) = positions.get(usize::from(rec.at)).copied() else {
            continue;
        };
        let before = glyphs.len();
        let mut budget = MAX_SUBTABLES;
        let resolved = lookup_at(
            data,
            ctx.lookup_list,
            rec.lookup,
            NESTABLE,
            LOOKUP_EXTENSION,
            &mut budget,
        );
        ctx.depth = ctx.depth.saturating_sub(1);
        let applied = resolved.and_then(|lookup| apply_at(data, &lookup, glyphs, at, ctx));
        ctx.depth = ctx.depth.saturating_add(1);
        if applied.is_none() {
            continue;
        }
        let after = glyphs.len();
        // Everything is tracked by absolute position rather than by index into
        // the record list, because with skipping the two stopped agreeing: a
        // ligature at position `at` swallows the glyphs that *follow* it in the
        // run, which may include ones the match stepped over and never named.
        if after >= before {
            let grew = after.saturating_sub(before);
            span = span.saturating_add(grew);
            for p in &mut positions {
                *p = p.map(|v| if v > at { v.saturating_add(grew) } else { v });
            }
        } else {
            let shrank = before.saturating_sub(after);
            span = span.saturating_sub(shrank);
            // The glyphs the shrink swallowed are gone. A later record naming
            // one of them is naming something that no longer exists, so mark
            // it absent rather than let the position slide onto a glyph the
            // context never matched.
            let gone = at.saturating_add(shrank);
            for p in &mut positions {
                let Some(v) = *p else { continue };
                if v <= at {
                    continue;
                }
                *p = if v <= gone {
                    None
                } else {
                    Some(v.saturating_sub(shrank))
                };
            }
        }
    }
    span.max(1)
}

/// The `SequenceLookupRecord` array at `at`.
fn read_records(data: &[u8], at: usize, count: usize, out: &mut Vec<Nested>) {
    out.clear();
    for i in 0..count.min(MAX_NESTED) {
        let Some(rec) = i.checked_mul(4).and_then(|d| at.checked_add(d)) else {
            return;
        };
        // A record that cannot be read ends the list rather than discarding
        // the ones already found: a truncated table should lose what it
        // truncated, not what came before it.
        let (Some(at), Some(lookup)) = (u16_at(data, rec), rec.checked_add(2).and_then(|o| u16_at(data, o)))
        else {
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
fn context_match(
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
fn chain_match(
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

#[cfg(test)]
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

    fn span(off: usize, len: usize) -> Span {
        Span { off, len }
    }

    /// Coverage format 1 over a sorted glyph list.
    fn coverage1(glyphs: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(u16::try_from(glyphs.len()).unwrap()));
        for g in glyphs {
            out.extend_from_slice(&be16(*g));
        }
        out
    }

    /// One `Ligature` record: the result glyph, then the components after the
    /// first.
    fn ligature(result: u16, rest: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be16(result));
        out.extend_from_slice(&be16(u16::try_from(rest.len() + 1).unwrap()));
        for g in rest {
            out.extend_from_slice(&be16(*g));
        }
        out
    }

    /// A `LigatureSet`: its records in the order given, which is the order
    /// they are tried in.
    fn ligature_set(records: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be16(u16::try_from(records.len()).unwrap()));
        let mut at = 2 + records.len() * 2;
        for r in records {
            out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
            at += r.len();
        }
        for r in records {
            out.extend_from_slice(r);
        }
        out
    }

    /// A whole `LigatureSubstFormat1` subtable: one set per covered first
    /// glyph, in coverage order.
    fn ligature_subst(first_glyphs: &[u16], sets: &[Vec<u8>]) -> Vec<u8> {
        assert_eq!(first_glyphs.len(), sets.len());
        let coverage = coverage1(first_glyphs);
        let header = 6 + sets.len() * 2;
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(u16::try_from(header).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(sets.len()).unwrap()));
        let mut at = header + coverage.len();
        for s in sets {
            out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
            at += s.len();
        }
        out.extend_from_slice(&coverage);
        for s in sets {
            out.extend_from_slice(s);
        }
        out
    }

    /// A `GSUB` table with one feature tagged `tag`, one lookup of `kind`, and
    /// `subtable` as that lookup's only subtable.
    fn gsub_table(tag: &[u8; 4], kind: u16, subtable: &[u8]) -> Vec<u8> {
        gsub_subtables(tag, kind, &[subtable])
    }

    /// A `GSUB` table with one feature tagged `tag` and one lookup of `kind`
    /// holding every subtable in `subtables`, in the order given.
    ///
    /// Several subtables in one lookup is the case that separates "try the
    /// next subtable" from "give up on this glyph": the font's order is the
    /// order they are tried in, and the first one that matches wins.
    fn gsub_subtables(tag: &[u8; 4], kind: u16, subtables: &[&[u8]]) -> Vec<u8> {
        gsub_scripts(&[(b"DFLT", tag)], kind, subtables)
    }

    /// A ScriptList: one Script per entry, each with a DefaultLangSys naming
    /// the feature indices given and no language-specific systems.
    ///
    /// Every offset inside is relative to the ScriptList's own start, so a
    /// caller only has to know where it put the block and how long it came
    /// out — not what is inside it.
    fn script_list(scripts: &[(&[u8; 4], &[u16])]) -> Vec<u8> {
        let n = scripts.len();
        let mut out = Vec::new();
        out.extend_from_slice(&be16(u16::try_from(n).unwrap()));
        // The Script tables follow the records, each one a 4-byte Script
        // header plus its DefaultLangSys.
        let mut at = 2 + n * 6;
        for (tag, features) in scripts {
            out.extend_from_slice(*tag);
            out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
            at += 4 + 6 + features.len() * 2;
        }
        for (_, features) in scripts {
            out.extend_from_slice(&be16(4)); // defaultLangSys, from the Script
            out.extend_from_slice(&be16(0)); // langSysCount
            out.extend_from_slice(&be16(0)); // lookupOrder, always zero
            out.extend_from_slice(&be16(0xFFFF)); // no required feature
            out.extend_from_slice(&be16(u16::try_from(features.len()).unwrap()));
            for f in *features {
                out.extend_from_slice(&be16(*f));
            }
        }
        out
    }

    /// A `GSUB` table registering one feature per entry of `scripts`, each
    /// under its own script tag, and one lookup of `kind` that every one of
    /// them selects.
    ///
    /// Several scripts naming the same lookup is the arrangement that matters:
    /// it is what a real face supporting two writing systems looks like, and
    /// the reason the selection walk starts at the ScriptList rather than the
    /// FeatureList — a tag alone does not say which script a feature is for.
    fn gsub_scripts(scripts: &[(&[u8; 4], &[u8; 4])], kind: u16, subtables: &[&[u8]]) -> Vec<u8> {
        // header 10 | scriptList | featureList | lookupList | subtables
        //
        // Each script gets its own Script table with a DefaultLangSys naming
        // exactly one feature — its own, by position in `scripts`.
        let n = scripts.len();
        let indices: Vec<[u16; 1]> = (0..n).map(|i| [u16::try_from(i).unwrap()]).collect();
        let entries: Vec<(&[u8; 4], &[u16])> = scripts
            .iter()
            .zip(&indices)
            .map(|((script, _), idx)| (*script, idx.as_slice()))
            .collect();
        let tags: Vec<&[u8; 4]> = scripts.iter().map(|&(_, tag)| tag).collect();
        gsub_from_scripts(&script_list(&entries), &tags, kind, subtables)
    }

    /// A `GSUB` table over a caller-built ScriptList: one Feature per entry of
    /// `tags`, each naming the single lookup of `kind`.
    ///
    /// Split out from [`gsub_scripts`] so a test can hand in a ScriptList that
    /// [`script_list`] cannot express — a script whose features are reachable
    /// only through a language system, for one.
    fn gsub_from_scripts(
        script_block: &[u8],
        tags: &[&[u8; 4]],
        kind: u16,
        subtables: &[&[u8]],
    ) -> Vec<u8> {
        gsub_flagged_from_scripts(script_block, tags, kind, 0, 0, subtables)
    }

    /// A `GSUB` like [`gsub_table`], but with a `lookupFlag` — and, when the
    /// flag asks for one, a `markFilteringSet` index — on its single lookup.
    ///
    /// The flag is the whole point of a handful of tests below: every other
    /// builder here writes a zero flag, so without this one the skipping walk
    /// is only ever exercised in its do-nothing configuration.
    fn gsub_flagged(tag: &[u8; 4], kind: u16, flag: u16, filter: u16, subtable: &[u8]) -> Vec<u8> {
        gsub_flagged_from_scripts(
            &script_list(&[(b"DFLT", &[0])]),
            &[tag],
            kind,
            flag,
            filter,
            &[subtable],
        )
    }

    fn gsub_flagged_from_scripts(
        script_block: &[u8],
        tags: &[&[u8; 4]],
        kind: u16,
        flag: u16,
        filter: u16,
        subtables: &[&[u8]],
    ) -> Vec<u8> {
        let n = tags.len();
        let feature_list = 10 + script_block.len();
        // count(2) + one 6-byte FeatureRecord each, then the Features.
        let features_at = 2 + n * 6;
        let feature_len = 6usize; // params + count + one index
        let lookup_list = feature_list + features_at + n * feature_len;

        let mut out = Vec::new();
        out.extend_from_slice(&be16(1)); // major
        out.extend_from_slice(&be16(0)); // minor
        out.extend_from_slice(&be16(10)); // scriptList
        out.extend_from_slice(&be16(u16::try_from(feature_list).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(lookup_list).unwrap()));
        out.extend_from_slice(script_block);

        out.extend_from_slice(&be16(u16::try_from(n).unwrap())); // featureCount
        for (i, tag) in tags.iter().enumerate() {
            out.extend_from_slice(*tag);
            let at = features_at + i * feature_len;
            out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
        }
        for _ in 0..n {
            out.extend_from_slice(&be16(0)); // featureParams
            out.extend_from_slice(&be16(1)); // lookupIndexCount
            out.extend_from_slice(&be16(0)); // lookup 0
        }

        // LookupList: count(2) + one offset(2) = 4, then the Lookup.
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(4));
        out.extend_from_slice(&be16(kind));
        out.extend_from_slice(&be16(flag));
        out.extend_from_slice(&be16(u16::try_from(subtables.len()).unwrap()));
        let lookup = lookup_list + 4;
        // `markFilteringSet` sits between the offset array and whatever the
        // offsets point at, so its presence moves the subtables along by two.
        let set = usize::from(flag & 0x0010 != 0) * 2;
        // Offsets are measured from the start of the Lookup, and the first
        // subtable begins after the whole offset array.
        let mut at = out.len() + subtables.len() * 2 + set - lookup;
        for s in subtables {
            out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
            at += s.len();
        }
        if set != 0 {
            out.extend_from_slice(&be16(filter));
        }
        for s in subtables {
            out.extend_from_slice(s);
        }
        out
    }

    /// `SingleSubstFormat1`: one delta over a covered range.
    fn single_delta(glyphs: &[u16], delta: u16) -> Vec<u8> {
        let coverage = coverage1(glyphs);
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(6)); // coverage follows the header
        out.extend_from_slice(&be16(delta));
        out.extend_from_slice(&coverage);
        out
    }

    /// `SingleSubstFormat2`: an explicit replacement per covered glyph.
    fn single_list(glyphs: &[u16], to: &[u16]) -> Vec<u8> {
        assert_eq!(glyphs.len(), to.len());
        let coverage = coverage1(glyphs);
        let header = 6 + to.len() * 2;
        let mut out = Vec::new();
        out.extend_from_slice(&be16(2));
        out.extend_from_slice(&be16(u16::try_from(header).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(to.len()).unwrap()));
        for g in to {
            out.extend_from_slice(&be16(*g));
        }
        out.extend_from_slice(&coverage);
        out
    }

    /// `MultipleSubstFormat1`: one sequence per covered glyph.
    ///
    /// `sequences[n]` is what `glyphs[n]` decomposes into. A sequence may be
    /// empty, which is how the "zero glyphs deletes the glyph" case is built.
    fn multiple(glyphs: &[u16], sequences: &[&[u16]]) -> Vec<u8> {
        assert_eq!(glyphs.len(), sequences.len());
        let coverage = coverage1(glyphs);
        // header(6) + one offset per sequence, then the Sequence tables, then
        // the coverage.
        let header = 6 + sequences.len() * 2;
        let mut at = header;
        let mut offsets = Vec::new();
        for seq in sequences {
            offsets.push(at);
            at += 2 + seq.len() * 2;
        }

        let mut out = Vec::new();
        out.extend_from_slice(&be16(1)); // substFormat
        out.extend_from_slice(&be16(u16::try_from(at).unwrap())); // coverage
        out.extend_from_slice(&be16(u16::try_from(sequences.len()).unwrap()));
        for off in &offsets {
            out.extend_from_slice(&be16(u16::try_from(*off).unwrap()));
        }
        for seq in sequences {
            out.extend_from_slice(&be16(u16::try_from(seq.len()).unwrap()));
            for g in *seq {
                out.extend_from_slice(&be16(*g));
            }
        }
        out.extend_from_slice(&coverage);
        out
    }

    /// `AlternateSubstFormat1`: one set of candidates per covered glyph.
    ///
    /// Byte-for-byte the same layout as [`multiple`] — coverage, a count, an
    /// offset per covered glyph, and each target a count followed by glyph
    /// ids. Only the lookup type tells the two apart, which is exactly why
    /// `a_subtable_is_read_as_the_type_its_lookup_declares` exists.
    fn alternate(glyphs: &[u16], sets: &[&[u16]]) -> Vec<u8> {
        multiple(glyphs, sets)
    }

    /// A `GSUB` table with `features.len()` features and one lookup each, in
    /// the order given — which is both the feature order and the LookupList
    /// order, so a test can say which lookup runs first.
    ///
    /// Kept separate from [`gsub_table`] rather than replacing it: the
    /// single-feature builder is what nearly every test wants, and threading
    /// slices through it would obscure them all to serve two.
    fn gsub_lookups(features: &[(&[u8; 4], u16, Vec<u8>)]) -> Vec<u8> {
        gsub_lookups_flagged(features, &[])
    }

    /// As [`gsub_lookups`], with `flags[i]` as lookup `i`'s `lookupFlag`. A
    /// short `flags` leaves the rest of the lookups unflagged.
    fn gsub_lookups_flagged(features: &[(&[u8; 4], u16, Vec<u8>)], flags: &[u16]) -> Vec<u8> {
        let n = features.len();
        // One default script that selects every feature, which is what a
        // Latin-only face ships and what leaves the feature order — the thing
        // these tests are about — the only variable.
        let all: Vec<u16> = (0..n).map(|i| u16::try_from(i).unwrap()).collect();
        let script_block = script_list(&[(b"DFLT", &all)]);

        let feature_list = 10 + script_block.len();
        // count(2) + one 6-byte record each, then one 6-byte Feature each
        // (params, lookupIndexCount, one index).
        let features_at = feature_list + 2 + n * 6;
        let lookup_list = features_at + n * 6;
        // count(2) + one offset each, then one 6-byte Lookup header each
        // (type, flags, subTableCount) plus its single subtable offset.
        let lookups_at = lookup_list + 2 + n * 2;

        let mut out = Vec::new();
        out.extend_from_slice(&be16(1)); // major
        out.extend_from_slice(&be16(0)); // minor
        out.extend_from_slice(&be16(10)); // scriptList
        out.extend_from_slice(&be16(u16::try_from(feature_list).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(lookup_list).unwrap()));
        out.extend_from_slice(&script_block);

        out.extend_from_slice(&be16(u16::try_from(n).unwrap()));
        for (i, (tag, _, _)) in features.iter().enumerate() {
            out.extend_from_slice(*tag);
            let at = features_at + i * 6 - feature_list;
            out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
        }
        for i in 0..n {
            out.extend_from_slice(&be16(0)); // featureParams
            out.extend_from_slice(&be16(1)); // lookupIndexCount
            out.extend_from_slice(&be16(u16::try_from(i).unwrap()));
        }

        out.extend_from_slice(&be16(u16::try_from(n).unwrap()));
        let mut at = lookups_at;
        for _ in 0..n {
            out.extend_from_slice(&be16(u16::try_from(at - lookup_list).unwrap()));
            at += 8;
        }
        // Every Lookup header is the same size, so the subtables sit in a block
        // after all of them and each offset is computed from its own lookup.
        let mut sub_at = lookups_at + n * 8;
        for (i, (_, kind, subtable)) in features.iter().enumerate() {
            let lookup = lookups_at + i * 8;
            out.extend_from_slice(&be16(*kind));
            out.extend_from_slice(&be16(flags.get(i).copied().unwrap_or(0)));
            out.extend_from_slice(&be16(1)); // subTableCount
            out.extend_from_slice(&be16(u16::try_from(sub_at - lookup).unwrap()));
            sub_at += subtable.len();
        }
        for (_, _, subtable) in features {
            out.extend_from_slice(subtable);
        }
        out
    }

    /// `ClassDefFormat1`: a class per glyph over a contiguous range.
    fn class_def(start: u16, classes: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(start));
        out.extend_from_slice(&be16(u16::try_from(classes.len()).unwrap()));
        for c in classes {
            out.extend_from_slice(&be16(*c));
        }
        out
    }

    /// A `MarkGlyphSetsDef`: one coverage per set, reached through a *32-bit*
    /// offset — the one place in either layout table where an offset is not a
    /// `u16`.
    fn mark_glyph_sets(sets: &[&[u16]]) -> Vec<u8> {
        let covers: Vec<Vec<u8>> = sets.iter().map(|s| coverage1(s)).collect();
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1)); // format
        out.extend_from_slice(&be16(u16::try_from(sets.len()).unwrap()));
        let mut at = 4 + sets.len() * 4;
        for c in &covers {
            out.extend_from_slice(&u32::try_from(at).unwrap().to_be_bytes());
            at += c.len();
        }
        for c in &covers {
            out.extend_from_slice(c);
        }
        out
    }

    /// A `GDEF` whose `GlyphClassDef` is `classes`, plus the mark glyph sets in
    /// `sets`.
    ///
    /// Declares itself 1.0 when there are no sets and 1.2 when there are,
    /// because `markGlyphSetsDef` is a field 1.0 does not have: a reader that
    /// took the offset from a 1.0 table would be reading whatever follows the
    /// header.
    fn gdef(classes: &[u8], sets: &[&[u16]]) -> Vec<u8> {
        let header = if sets.is_empty() { 12 } else { 14 };
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1)); // major
        out.extend_from_slice(&be16(if sets.is_empty() { 0 } else { 2 }));
        out.extend_from_slice(&be16(u16::try_from(header).unwrap())); // glyphClassDef
        out.extend_from_slice(&be16(0)); // attachList
        out.extend_from_slice(&be16(0)); // ligCaretList
        out.extend_from_slice(&be16(0)); // markAttachClassDef
        if !sets.is_empty() {
            let at = header + classes.len();
            out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
        }
        out.extend_from_slice(classes);
        if !sets.is_empty() {
            out.extend_from_slice(&mark_glyph_sets(sets));
        }
        out
    }

    /// Parse a `GSUB` and a `GDEF` the way a face presents them: two spans into
    /// one file, not two files.
    fn with_gdef(gsub: &[u8], gdef: &[u8]) -> (Vec<u8>, Substitutions) {
        let mut data = gsub.to_vec();
        let at = data.len();
        data.extend_from_slice(gdef);
        let subs = Substitutions::parse(&data, Some(span(0, gsub.len())), Some(span(at, gdef.len())))
            .expect("a flagged lookup must still parse");
        (data, subs)
    }

    /// The `SequenceLookupRecord` array: `(input position, lookup index)`.
    fn records(recs: &[(u16, u16)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (at, lookup) in recs {
            out.extend_from_slice(&be16(*at));
            out.extend_from_slice(&be16(*lookup));
        }
        out
    }

    /// One `SequenceRule`/`ClassSequenceRule`. `rest` is the input from the
    /// *second* entry on, the first being the one coverage already matched.
    fn rule(rest: &[u16], recs: &[(u16, u16)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be16(u16::try_from(rest.len() + 1).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(recs.len()).unwrap()));
        for g in rest {
            out.extend_from_slice(&be16(*g));
        }
        out.extend_from_slice(&records(recs));
        out
    }

    /// One `ChainedSequenceRule`/`ChainedClassSequenceRule`.
    fn chained(back: &[u16], rest: &[u16], ahead: &[u16], recs: &[(u16, u16)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be16(u16::try_from(back.len()).unwrap()));
        for g in back {
            out.extend_from_slice(&be16(*g));
        }
        out.extend_from_slice(&be16(u16::try_from(rest.len() + 1).unwrap()));
        for g in rest {
            out.extend_from_slice(&be16(*g));
        }
        out.extend_from_slice(&be16(u16::try_from(ahead.len()).unwrap()));
        for g in ahead {
            out.extend_from_slice(&be16(*g));
        }
        out.extend_from_slice(&be16(u16::try_from(recs.len()).unwrap()));
        out.extend_from_slice(&records(recs));
        out
    }

    /// A rule set: count, one offset per rule, then the rules.
    fn rule_set_of(rules: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be16(u16::try_from(rules.len()).unwrap()));
        let mut at = 2 + rules.len() * 2;
        for r in rules {
            out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
            at += r.len();
        }
        for r in rules {
            out.extend_from_slice(r);
        }
        out
    }

    /// A table of offsets from the subtable start, laid out as
    /// `header | coverage-ish blocks | sets`. Shared by the format-1 and
    /// format-2 builders, which differ only in how long the header is and
    /// what sits between it and the sets.
    fn sets_after(header: usize, blocks: &[&[u8]], sets: &[Vec<u8>]) -> (Vec<u16>, Vec<u16>) {
        let mut at = header;
        let mut block_at = Vec::new();
        for b in blocks {
            block_at.push(u16::try_from(at).unwrap());
            at += b.len();
        }
        let mut set_at = Vec::new();
        for s in sets {
            set_at.push(u16::try_from(at).unwrap());
            at += s.len();
        }
        (block_at, set_at)
    }

    /// `SequenceContextFormat1`, and — the layout being identical —
    /// `ChainedSequenceContextFormat1` too.
    fn context1(glyphs: &[u16], sets: &[Vec<u8>]) -> Vec<u8> {
        assert_eq!(glyphs.len(), sets.len());
        let coverage = coverage1(glyphs);
        let (blocks, offs) = sets_after(6 + sets.len() * 2, &[&coverage], sets);
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(blocks[0]));
        out.extend_from_slice(&be16(u16::try_from(sets.len()).unwrap()));
        for o in &offs {
            out.extend_from_slice(&be16(*o));
        }
        out.extend_from_slice(&coverage);
        for s in sets {
            out.extend_from_slice(s);
        }
        out
    }

    /// `SequenceContextFormat2`: rule sets keyed by the first glyph's class.
    fn context2(glyphs: &[u16], classes: &[u8], sets: &[Vec<u8>]) -> Vec<u8> {
        let coverage = coverage1(glyphs);
        let (blocks, offs) = sets_after(8 + sets.len() * 2, &[&coverage, classes], sets);
        let mut out = Vec::new();
        out.extend_from_slice(&be16(2));
        out.extend_from_slice(&be16(blocks[0]));
        out.extend_from_slice(&be16(blocks[1]));
        out.extend_from_slice(&be16(u16::try_from(sets.len()).unwrap()));
        for o in &offs {
            out.extend_from_slice(&be16(*o));
        }
        out.extend_from_slice(&coverage);
        out.extend_from_slice(classes);
        for s in sets {
            out.extend_from_slice(s);
        }
        out
    }

    /// `ChainedSequenceContextFormat2`: three ClassDefs, one per part.
    fn chain_context2(
        glyphs: &[u16],
        back: &[u8],
        input: &[u8],
        ahead: &[u8],
        sets: &[Vec<u8>],
    ) -> Vec<u8> {
        let coverage = coverage1(glyphs);
        let (blocks, offs) = sets_after(
            12 + sets.len() * 2,
            &[&coverage, back, input, ahead],
            sets,
        );
        let mut out = Vec::new();
        out.extend_from_slice(&be16(2));
        for b in &blocks {
            out.extend_from_slice(&be16(*b));
        }
        out.extend_from_slice(&be16(u16::try_from(sets.len()).unwrap()));
        for o in &offs {
            out.extend_from_slice(&be16(*o));
        }
        out.extend_from_slice(&coverage);
        out.extend_from_slice(back);
        out.extend_from_slice(input);
        out.extend_from_slice(ahead);
        for s in sets {
            out.extend_from_slice(s);
        }
        out
    }

    /// `SequenceContextFormat3`: one coverage table per input position.
    fn context3(covers: &[&[u16]], recs: &[(u16, u16)]) -> Vec<u8> {
        let tables: Vec<Vec<u8>> = covers.iter().map(|c| coverage1(c)).collect();
        let mut at = 6 + covers.len() * 2 + recs.len() * 4;
        let mut out = Vec::new();
        out.extend_from_slice(&be16(3));
        out.extend_from_slice(&be16(u16::try_from(covers.len()).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(recs.len()).unwrap()));
        for t in &tables {
            out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
            at += t.len();
        }
        out.extend_from_slice(&records(recs));
        for t in &tables {
            out.extend_from_slice(t);
        }
        out
    }

    /// `ChainedSequenceContextFormat3`: the format nearly every real chaining
    /// context in a Latin face uses.
    fn chain_context3(
        back: &[&[u16]],
        input: &[&[u16]],
        ahead: &[&[u16]],
        recs: &[(u16, u16)],
    ) -> Vec<u8> {
        let all: Vec<Vec<u8>> = back
            .iter()
            .chain(input.iter())
            .chain(ahead.iter())
            .map(|c| coverage1(c))
            .collect();
        let header = 2 + 2 + back.len() * 2 + 2 + input.len() * 2 + 2 + ahead.len() * 2 + 2;
        let mut at = header + recs.len() * 4;
        let mut offs = Vec::new();
        for t in &all {
            offs.push(u16::try_from(at).unwrap());
            at += t.len();
        }
        let mut out = Vec::new();
        out.extend_from_slice(&be16(3));
        let mut next = offs.iter();
        for (n, part) in [back.len(), input.len(), ahead.len()].into_iter().enumerate() {
            let _ = n;
            out.extend_from_slice(&be16(u16::try_from(part).unwrap()));
            for _ in 0..part {
                out.extend_from_slice(&be16(*next.next().unwrap()));
            }
        }
        out.extend_from_slice(&be16(u16::try_from(recs.len()).unwrap()));
        out.extend_from_slice(&records(recs));
        for t in &all {
            out.extend_from_slice(t);
        }
        out
    }

    /// Run every lookup over `gids` and report what comes out.
    fn subst(data: &[u8], subs: &Substitutions, gids: &[u16]) -> Vec<u16> {
        let mut glyphs: Vec<SubGlyph> = gids
            .iter()
            .enumerate()
            .map(|(i, &gid)| SubGlyph::new(gid, i))
            .collect();
        subs.apply(data, None, &mut glyphs);
        glyphs.iter().map(|g| g.gid).collect()
    }

    /// The clusters `gids` come out with, one source character per glyph in.
    fn clusters(data: &[u8], subs: &Substitutions, gids: &[u16]) -> Vec<usize> {
        let mut glyphs: Vec<SubGlyph> = gids
            .iter()
            .enumerate()
            .map(|(i, &gid)| SubGlyph::new(gid, i))
            .collect();
        subs.apply(data, None, &mut glyphs);
        glyphs.iter().map(|g| g.cluster).collect()
    }

    /// `f`=10, `i`=11, `l`=12, `fi`=20, `ffi`=21, `ff`=22.
    fn fi_font() -> (Vec<u8>, Substitutions) {
        let set_f = ligature_set(&[
            ligature(21, &[10, 11]), // ffi — longest first
            ligature(22, &[10]),     // ff
            ligature(20, &[11]),     // fi
        ]);
        let sub = ligature_subst(&[10], &[set_f]);
        let data = gsub_table(b"liga", LOOKUP_LIGATURE, &sub);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).expect("liga must parse");
        (data, subs)
    }

    #[test]
    fn a_pair_becomes_one_glyph() {
        let (data, subs) = fi_font();
        assert_eq!(subst(&data, &subs, &[10, 11]), [20]);
    }

    #[test]
    fn the_longest_ligature_wins() {
        let (data, subs) = fi_font();
        // f f i must become `ffi`, not `ff` followed by a stray `i`.
        assert_eq!(subst(&data, &subs, &[10, 10, 11]), [21]);
        // f f alone is still `ff`.
        assert_eq!(subst(&data, &subs, &[10, 10]), [22]);
    }

    #[test]
    fn what_follows_the_ligature_is_kept() {
        let (data, subs) = fi_font();
        assert_eq!(subst(&data, &subs, &[10, 11, 12, 99]), [20, 12, 99]);
    }

    #[test]
    fn a_second_ligature_forms_after_the_first() {
        // The lookup runs across the whole run, not just its start: `fifi` is
        // two ligatures, and a pass that stopped at the first would leave the
        // second pair unjoined.
        let (data, subs) = fi_font();
        assert_eq!(subst(&data, &subs, &[10, 11, 10, 11]), [20, 20]);
    }

    #[test]
    fn a_ligature_keeps_its_first_components_cluster() {
        // A caret can be put before or after `fi` but not inside it, which is
        // only true if the joined glyph reports where the `f` began.
        let (data, subs) = fi_font();
        assert_eq!(clusters(&data, &subs, &[99, 10, 11, 99]), [0, 1, 3]);
    }

    #[test]
    fn a_glyph_outside_the_coverage_never_matches() {
        let (data, subs) = fi_font();
        // `i` starts nothing.
        assert_eq!(subst(&data, &subs, &[11, 10]), [11, 10]);
        // `f` followed by something with no ligature.
        assert_eq!(subst(&data, &subs, &[10, 99]), [10, 99]);
    }

    #[test]
    fn one_glyph_cannot_ligate() {
        let (data, subs) = fi_font();
        assert_eq!(subst(&data, &subs, &[10]), [10]);
        assert!(subst(&data, &subs, &[]).is_empty());
    }

    const IGNORE_MARKS: u16 = 0x0008;
    const USE_MARK_FILTERING_SET: u16 = 0x0010;

    /// `f`=10 plus `i`=11 becomes `fi`=20, and nothing else. The smallest
    /// lookup that can tell a skipped glyph from a matched one.
    fn fi_subtable() -> Vec<u8> {
        ligature_subst(&[10], &[ligature_set(&[ligature(20, &[11])])])
    }

    #[test]
    fn a_mark_between_the_components_defeats_an_unflagged_ligature() {
        // The baseline the flagged tests below are measured against: with no
        // flag the mark is an ordinary glyph, and `f` mark `i` is simply not
        // the pair the face described. Without this test a passing
        // `IgnoreMarks` test could just as well be a lookup that ignores its
        // input entirely.
        let gsub = gsub_flagged(b"liga", LOOKUP_LIGATURE, 0, 0, &fi_subtable());
        let (data, subs) = with_gdef(&gsub, &gdef(&class_def(90, &[3]), &[]));
        assert_eq!(subst(&data, &subs, &[10, 90, 11]), [10, 90, 11]);
    }

    #[test]
    fn ignore_marks_forms_the_ligature_across_the_mark() {
        // A face writing `fi` says "an f and an i", not "an f and an i with no
        // vowel mark between them". The mark is skipped, not consumed, so it
        // survives into the output — dropping it would delete a diacritic the
        // user typed.
        let gsub = gsub_flagged(b"liga", LOOKUP_LIGATURE, IGNORE_MARKS, 0, &fi_subtable());
        let (data, subs) = with_gdef(&gsub, &gdef(&class_def(90, &[3]), &[]));
        assert_eq!(subst(&data, &subs, &[10, 90, 11]), [20, 90]);
    }

    #[test]
    fn ignore_marks_still_stops_at_a_glyph_that_is_not_a_mark() {
        // Skipping is not the same as not matching: the flag lets the walk
        // step over a mark, and over nothing else. A `12` between the
        // components must end the match, or every lookup with this flag would
        // reach across whole words.
        let gsub = gsub_flagged(b"liga", LOOKUP_LIGATURE, IGNORE_MARKS, 0, &fi_subtable());
        let (data, subs) = with_gdef(&gsub, &gdef(&class_def(90, &[3]), &[]));
        assert_eq!(subst(&data, &subs, &[10, 12, 11]), [10, 12, 11]);
    }

    #[test]
    fn a_flag_with_no_gdef_behind_it_hides_nothing() {
        // "Ignore marks" is a statement about glyph classes, and a face with
        // no `GDEF` has never said which glyphs are marks. Guessing — reading
        // the flag as licence to skip whatever looks mark-like — would form
        // ligatures the face did not ask for.
        let gsub = gsub_flagged(b"liga", LOOKUP_LIGATURE, IGNORE_MARKS, 0, &fi_subtable());
        let subs = Substitutions::parse(&gsub, Some(span(0, gsub.len())), None).unwrap();
        assert_eq!(subst(&gsub, &subs, &[10, 90, 11]), [10, 90, 11]);
    }

    #[test]
    fn a_mark_filtering_set_hides_every_mark_it_does_not_name() {
        // `markFilteringSet` is the one lookup field that is not at a fixed
        // offset — it follows the subtable-offset array, so its position
        // depends on how many subtables the lookup declared. Reading it two
        // bytes early would give a set index of whatever the first subtable
        // offset happens to be.
        //
        // The sense of the flag is the opposite of `IgnoreMarks`: the lookup
        // asked to *see* the marks in its set, so those stop the match and
        // every other mark is skipped.
        let gsub = gsub_flagged(
            b"liga",
            LOOKUP_LIGATURE,
            USE_MARK_FILTERING_SET,
            0,
            &fi_subtable(),
        );
        let (data, subs) = with_gdef(&gsub, &gdef(&class_def(90, &[3, 3]), &[&[90]]));
        assert_eq!(subst(&data, &subs, &[10, 90, 11]), [10, 90, 11]);
        assert_eq!(subst(&data, &subs, &[10, 91, 11]), [20, 91]);
    }

    #[test]
    fn a_chaining_rule_skips_marks_in_its_backtrack_too() {
        // The flag governs the whole walk, context included: a neighbour is a
        // neighbour whether the rule reached it as input or as backtrack. A
        // shaper that honoured the flag only for the input would fail every
        // chaining rule whose context happens to sit across a diacritic.
        let chain = chain_context3(&[&[12]], &[&[10]], &[], &[(0, 1)]);
        let lookups: Vec<(&[u8; 4], u16, Vec<u8>)> = alloc::vec![
            (b"calt", LOOKUP_CHAIN_CONTEXT, chain),
            // `dlig` is off by default, so the helper can only run by being
            // invoked — which is what makes this a test of the chaining rule.
            (b"dlig", LOOKUP_SINGLE, single_list(&[10], &[30])),
        ];
        let flagged = gsub_lookups_flagged(&lookups, &[IGNORE_MARKS]);
        let plain = gsub_lookups(&lookups);
        let classes = gdef(&class_def(90, &[3]), &[]);

        let (data, subs) = with_gdef(&flagged, &classes);
        assert_eq!(subst(&data, &subs, &[12, 90, 10]), [12, 90, 30]);
        // …and with the flag off the same mark ends the backtrack.
        let (data, subs) = with_gdef(&plain, &classes);
        assert_eq!(subst(&data, &subs, &[12, 90, 10]), [12, 90, 10]);
    }

    #[test]
    fn a_required_ligature_is_read_too() {
        // `rlig` is a different tag reaching the same machinery. Arabic needs
        // it; a face that has it is wrong without it.
        let set = ligature_set(&[ligature(30, &[11])]);
        let sub = ligature_subst(&[10], &[set]);
        let data = gsub_table(b"rlig", LOOKUP_LIGATURE, &sub);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 11]), [30]);
    }

    #[test]
    fn a_feature_we_do_not_ask_for_is_left_alone() {
        // `dlig` is off by default: reading it would turn on decorative
        // ligatures nobody asked for.
        let set = ligature_set(&[ligature(30, &[11])]);
        let sub = ligature_subst(&[10], &[set]);
        let data = gsub_table(b"dlig", LOOKUP_LIGATURE, &sub);
        assert!(Substitutions::parse(&data, Some(span(0, data.len())), None).is_none());
    }

    #[test]
    fn the_default_localization_is_applied() {
        // `locl` under the DefaultLangSys is the face's *default* localized
        // form — the one a shaper applies when the caller names no language,
        // which is this crate's only mode. Sans Serif Collection maps `space`
        // through its Latin `locl`, so skipping this feature turned every
        // space in every Latin string into the wrong glyph.
        let data = gsub_table(b"locl", LOOKUP_SINGLE, &single_delta(&[10], 5));
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), &[15]);
    }

    /// Run every lookup over `gids`, each glyph eligible for the cursive form
    /// beside it, and report what comes out.
    fn subst_cursive(
        data: &[u8],
        subs: &Substitutions,
        run: &[(u16, Option<Form>)],
    ) -> Vec<u16> {
        let mut glyphs: Vec<SubGlyph> = run
            .iter()
            .enumerate()
            .map(|(i, &(gid, form))| SubGlyph::cursive(gid, i, form))
            .collect();
        subs.apply(data, None, &mut glyphs);
        glyphs.iter().map(|g| g.gid).collect()
    }

    /// The whole point of the mask. A real face's `fina` covers every letter
    /// it has, so a `fina` applied the way `liga` is applied would rewrite an
    /// entire word into final forms.
    #[test]
    fn a_positional_feature_reaches_only_the_glyph_that_takes_that_form() {
        // One `fina` lookup covering all three glyphs of the run.
        let data = gsub_table(b"fina", LOOKUP_SINGLE, &single_delta(&[10, 11, 12], 5));
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(
            subst_cursive(
                &data,
                &subs,
                &[
                    (10, Some(Form::Initial)),
                    (11, Some(Form::Medial)),
                    (12, Some(Form::Final)),
                ]
            ),
            // Only the last one is eligible, though all three are covered.
            &[10, 11, 17]
        );
    }

    /// The mirror image: an unconditional feature is unaffected by the forms,
    /// which is what keeps every Latin string shaping exactly as before.
    #[test]
    fn an_unconditional_feature_reaches_every_glyph_whatever_its_form() {
        let data = gsub_table(b"liga", LOOKUP_SINGLE, &single_delta(&[10, 11, 12], 5));
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(
            subst_cursive(
                &data,
                &subs,
                &[
                    (10, Some(Form::Initial)),
                    (11, None),
                    (12, Some(Form::Final)),
                ]
            ),
            &[15, 16, 17]
        );
    }

    /// The mask constants are written out rather than derived, so this is what
    /// stops a feature inserted into the middle of [`FEATURES`] from silently
    /// aiming `fina`'s bit at `rclt`.
    #[test]
    fn the_masks_match_the_feature_list() {
        let bit = |tag: &[u8; 4]| {
            let i = FEATURES
                .iter()
                .position(|f| *f == tag)
                .unwrap_or_else(|| panic!("{tag:?} is not in FEATURES"));
            1u32 << i
        };
        assert_eq!(bit(b"isol"), ISOL);
        assert_eq!(bit(b"init"), INIT);
        assert_eq!(bit(b"medi"), MEDI);
        assert_eq!(bit(b"fina"), FINA);
        // `ALWAYS` is exactly the features that are not positional.
        let positional = ISOL | INIT | MEDI | FINA;
        let all = (1u32 << FEATURES.len()) - 1;
        assert_eq!(ALWAYS, all & !positional);
        // And they really are a prefix: no unconditional feature may sit
        // after a positional one, or `ALWAYS` would not be contiguous.
        assert_eq!(ALWAYS.count_ones() + positional.count_ones(), all.count_ones());
        assert_eq!(ALWAYS.trailing_ones(), ALWAYS.count_ones());
    }

    /// A glyph eligible for no positional form at all — every glyph outside a
    /// cursive script — must not be reached by one.
    #[test]
    fn a_glyph_with_no_form_is_reached_by_no_positional_feature() {
        for tag in [b"isol", b"init", b"medi", b"fina"] {
            let data = gsub_table(tag, LOOKUP_SINGLE, &single_delta(&[10], 5));
            let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
            assert_eq!(subst(&data, &subs, &[10]), &[10], "{:?}", *tag);
        }
    }

    #[test]
    fn a_feature_only_a_language_system_reaches_is_not_applied() {
        // The other half of the same question. A `locl` registered under
        // `TRK ` is Turkish, not the default, and reaching it would hand a
        // reader who never asked for Turkish its dotless `i`. The gap this
        // pins down is deliberate and tracked as
        // `TD-FONT-IGNORES-LANGSYS-OVERRIDES`: only DefaultLangSys is read,
        // so a face whose *only* route to a feature is a language system
        // contributes nothing at all.
        let mut scripts = Vec::new();
        scripts.extend_from_slice(&be16(1)); // scriptCount
        scripts.extend_from_slice(b"latn");
        scripts.extend_from_slice(&be16(8)); // the Script table follows
        scripts.extend_from_slice(&be16(0)); // no DefaultLangSys
        scripts.extend_from_slice(&be16(1)); // langSysCount
        scripts.extend_from_slice(b"TRK ");
        scripts.extend_from_slice(&be16(10)); // LangSys, from the Script
        scripts.extend_from_slice(&be16(0)); // lookupOrder, always zero
        scripts.extend_from_slice(&be16(0xFFFF)); // no required feature
        scripts.extend_from_slice(&be16(1)); // featureIndexCount
        scripts.extend_from_slice(&be16(0)); // feature 0

        let sub = single_delta(&[10], 5);
        let data = gsub_from_scripts(&scripts, &[b"locl"], LOOKUP_SINGLE, &[&sub]);
        assert!(Substitutions::parse(&data, Some(span(0, data.len())), None).is_none());
    }

    #[test]
    fn no_gsub_means_no_substitutions() {
        assert!(Substitutions::parse(&[], None, None).is_none());
    }

    #[test]
    fn an_extension_lookup_is_followed() {
        let set = ligature_set(&[ligature(20, &[11])]);
        let inner = ligature_subst(&[10], &[set]);
        // ExtensionSubstFormat1: format, wrapped type, 32-bit offset.
        let mut ext = Vec::new();
        ext.extend_from_slice(&be16(1));
        ext.extend_from_slice(&be16(LOOKUP_LIGATURE));
        ext.extend_from_slice(&8u32.to_be_bytes());
        ext.extend_from_slice(&inner);
        let data = gsub_table(b"liga", LOOKUP_EXTENSION, &ext);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 11]), [20]);
    }

    #[test]
    fn a_lookup_type_we_cannot_apply_is_not_mistaken_for_one_we_can() {
        // Type 8, `ReverseChainSingleSubst`, opens with a format and a
        // coverage offset just as the single substitution above does, so a
        // walk that ignored the lookup type would happily read it and
        // substitute the wrong glyph.
        let data = gsub_table(b"liga", 8, &single_list(&[10], &[42]));
        assert!(Substitutions::parse(&data, Some(span(0, data.len())), None).is_none());
    }

    /// The first alternate is what an on-or-off feature selects: OpenType
    /// numbers alternates from one, and "on" is the value one.
    #[test]
    fn an_alternate_substitution_takes_the_first_candidate() {
        let data = gsub_table(
            b"liga",
            LOOKUP_ALTERNATE,
            &alternate(&[10], &[&[42, 43, 44]]),
        );
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), &[42]);
        // An uncovered glyph is left alone.
        assert_eq!(subst(&data, &subs, &[11]), &[11]);
    }

    /// An empty set has no first alternate. Reading one anyway would take the
    /// two bytes after the count — whatever happens to follow the table.
    #[test]
    fn an_empty_alternate_set_substitutes_nothing() {
        let data = gsub_table(b"liga", LOOKUP_ALTERNATE, &alternate(&[10], &[&[]]));
        let Some(subs) = Substitutions::parse(&data, Some(span(0, data.len())), None) else {
            return;
        };
        assert_eq!(subst(&data, &subs, &[10]), &[10]);
    }

    /// The two formats are byte-identical, so only the declared lookup type
    /// separates "one glyph becomes three" from "one glyph becomes the first
    /// of three".
    #[test]
    fn a_subtable_is_read_as_the_type_its_lookup_declares() {
        let bytes = alternate(&[10], &[&[42, 43, 44]]);
        let as_alt = gsub_table(b"liga", LOOKUP_ALTERNATE, &bytes);
        let as_mult = gsub_table(b"liga", LOOKUP_MULTIPLE, &bytes);
        let alt = Substitutions::parse(&as_alt, Some(span(0, as_alt.len())), None).unwrap();
        let mult = Substitutions::parse(&as_mult, Some(span(0, as_mult.len())), None).unwrap();
        assert_eq!(subst(&as_alt, &alt, &[10]), &[42]);
        assert_eq!(subst(&as_mult, &mult, &[10]), &[42, 43, 44]);
    }

    // ---- script selection ----

    /// A `GSUB` with two `liga` features, one Arabic and one Latin, each
    /// reaching a lookup of its own. This is the shape that made script
    /// selection necessary: `ebrima.ttf` on the development host really does
    /// have an Arabic rule that a script-blind walk applies to Latin text.
    fn two_script_font() -> (Vec<u8>, Substitutions) {
        // Feature 0 (`arab`) → lookup 0, turns 10 into 50.
        // Feature 1 (`latn`) → lookup 1, turns 10 into 60.
        let all = script_list(&[(b"arab", &[0]), (b"latn", &[1])]);
        let feature_list = 10 + all.len();
        let lookup_list = feature_list + 2 + 2 * 6 + 2 * 6;

        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(0));
        out.extend_from_slice(&be16(10));
        out.extend_from_slice(&be16(u16::try_from(feature_list).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(lookup_list).unwrap()));
        out.extend_from_slice(&all);

        out.extend_from_slice(&be16(2)); // featureCount
        for (i, tag) in [b"liga", b"liga"].into_iter().enumerate() {
            out.extend_from_slice(tag);
            out.extend_from_slice(&be16(u16::try_from(2 + 2 * 6 + i * 6).unwrap()));
        }
        for i in 0..2u16 {
            out.extend_from_slice(&be16(0)); // featureParams
            out.extend_from_slice(&be16(1)); // lookupIndexCount
            out.extend_from_slice(&be16(i));
        }

        // Two lookups, each with one single-substitution subtable.
        let subtables = [single_list(&[10], &[50]), single_list(&[10], &[60])];
        let lookups_at = lookup_list + 2 + 2 * 2;
        out.extend_from_slice(&be16(2));
        for i in 0..2 {
            let at = lookups_at + i * 8 - lookup_list;
            out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
        }
        let mut sub_at = lookups_at + 2 * 8;
        for (i, sub) in subtables.iter().enumerate() {
            out.extend_from_slice(&be16(LOOKUP_SINGLE));
            out.extend_from_slice(&be16(0));
            out.extend_from_slice(&be16(1));
            let lookup = lookups_at + i * 8;
            out.extend_from_slice(&be16(u16::try_from(sub_at - lookup).unwrap()));
            sub_at += sub.len();
        }
        for sub in &subtables {
            out.extend_from_slice(sub);
        }

        let subs = Substitutions::parse(&out, Some(span(0, out.len())), None).expect("two scripts");
        (out, subs)
    }

    const LATIN: Option<ScriptTags> = Some(ScriptTags {
        preferred: *b"latn",
        fallback: *b"latn",
    });

    const ARABIC: Option<ScriptTags> = Some(ScriptTags {
        preferred: *b"arab",
        fallback: *b"arab",
    });

    /// The whole point: two features with the same tag mean different things,
    /// and a run gets only the one filed under its own script.
    #[test]
    fn a_run_gets_only_its_own_scripts_features() {
        let (data, subs) = two_script_font();
        let mut latin_run = vec![SubGlyph::new(10, 0)];
        subs.apply(&data, LATIN, &mut latin_run);
        assert_eq!(latin_run.first().map(|g| g.gid), Some(60));

        let mut arabic_run = vec![SubGlyph::new(10, 0)];
        subs.apply(&data, ARABIC, &mut arabic_run);
        assert_eq!(arabic_run.first().map(|g| g.gid), Some(50));
    }

    /// A face that registers neither the run's script nor a default has
    /// nothing to say about it, and saying nothing is the correct answer —
    /// the alternative is applying some other writing system's rules.
    #[test]
    fn a_script_the_face_does_not_register_gets_nothing() {
        let (data, subs) = two_script_font();
        let hebrew = Some(ScriptTags {
            preferred: *b"hebr",
            fallback: *b"hebr",
        });
        let mut glyphs = vec![SubGlyph::new(10, 0)];
        subs.apply(&data, hebrew, &mut glyphs);
        assert_eq!(glyphs.first().map(|g| g.gid), Some(10));
        // And a run with no script of its own, which asks for `DFLT`, is in
        // the same position here: this face registers no default either.
        let mut none = vec![SubGlyph::new(10, 0)];
        subs.apply(&data, None, &mut none);
        assert_eq!(none.first().map(|g| g.gid), Some(10));
    }

    /// `DFLT` is what a run falls back to, and what a face that says nothing
    /// about scripts registers. Every other test in this file relies on it.
    #[test]
    fn an_unregistered_script_falls_back_to_the_default_one() {
        let data = gsub_table(b"liga", LOOKUP_SINGLE, &single_list(&[10], &[42]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        for script in [LATIN, ARABIC, None] {
            let mut glyphs = vec![SubGlyph::new(10, 0)];
            subs.apply(&data, script, &mut glyphs);
            assert_eq!(
                glyphs.first().map(|g| g.gid),
                Some(42),
                "{script:?} should have fallen back to DFLT"
            );
        }
    }

    /// A feature no language system reaches is inert, however invitingly it is
    /// tagged. A walk that started at the FeatureList would apply it anyway.
    #[test]
    fn a_feature_no_script_reaches_does_not_apply() {
        // One script, `latn`, whose DefaultLangSys names feature 1 — so
        // feature 0, also a `liga`, is registered and unreachable.
        let all = script_list(&[(b"latn", &[1])]);
        let feature_list = 10 + all.len();
        let lookup_list = feature_list + 2 + 2 * 6 + 2 * 6;
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(0));
        out.extend_from_slice(&be16(10));
        out.extend_from_slice(&be16(u16::try_from(feature_list).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(lookup_list).unwrap()));
        out.extend_from_slice(&all);
        out.extend_from_slice(&be16(2));
        for i in 0..2usize {
            out.extend_from_slice(b"liga");
            out.extend_from_slice(&be16(u16::try_from(2 + 2 * 6 + i * 6).unwrap()));
        }
        for i in 0..2u16 {
            out.extend_from_slice(&be16(0));
            out.extend_from_slice(&be16(1));
            out.extend_from_slice(&be16(i));
        }
        let subtables = [single_list(&[10], &[50]), single_list(&[11], &[60])];
        let lookups_at = lookup_list + 2 + 2 * 2;
        out.extend_from_slice(&be16(2));
        for i in 0..2 {
            out.extend_from_slice(&be16(u16::try_from(lookups_at + i * 8 - lookup_list).unwrap()));
        }
        let mut sub_at = lookups_at + 2 * 8;
        for (i, sub) in subtables.iter().enumerate() {
            out.extend_from_slice(&be16(LOOKUP_SINGLE));
            out.extend_from_slice(&be16(0));
            out.extend_from_slice(&be16(1));
            out.extend_from_slice(&be16(u16::try_from(sub_at - (lookups_at + i * 8)).unwrap()));
            sub_at += sub.len();
        }
        for sub in &subtables {
            out.extend_from_slice(sub);
        }

        let subs = Substitutions::parse(&out, Some(span(0, out.len())), None).unwrap();
        let mut glyphs = vec![
            SubGlyph::new(10, 0),
            SubGlyph::new(11, 1),
        ];
        subs.apply(&out, LATIN, &mut glyphs);
        assert_eq!(
            glyphs.iter().map(|g| g.gid).collect::<Vec<_>>(),
            [10, 60],
            "the unreachable feature 0 was applied"
        );
    }

    // ---- single substitution ----

    #[test]
    fn a_single_substitution_replaces_by_delta() {
        let data = gsub_lookups(&[(b"ccmp", LOOKUP_SINGLE, single_delta(&[10, 11], 90))]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 11, 12]), [100, 101, 12]);
    }

    #[test]
    fn a_single_substitution_replaces_by_list() {
        let data = gsub_lookups(&[(b"ccmp", LOOKUP_SINGLE, single_list(&[10, 12], &[70, 80]))]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        // 11 is between the two covered glyphs and must be left alone: the
        // replacements are indexed by coverage order, not by glyph id.
        assert_eq!(subst(&data, &subs, &[10, 11, 12]), [70, 11, 80]);
    }

    #[test]
    fn a_delta_that_runs_past_the_last_glyph_wraps() {
        // The spec's arithmetic is modulo 65536. Saturating instead would
        // quietly substitute the face's last glyph for a whole covered range.
        let data = gsub_lookups(&[(b"ccmp", LOOKUP_SINGLE, single_delta(&[u16::MAX], 1))]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[u16::MAX]), [0]);
    }

    #[test]
    fn a_single_substitution_does_not_change_the_run_or_its_clusters() {
        let data = gsub_lookups(&[(b"ccmp", LOOKUP_SINGLE, single_delta(&[10], 90))]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(clusters(&data, &subs, &[10, 11, 10]), [0, 1, 2]);
    }

    // ---- lookup ordering ----

    #[test]
    fn an_earlier_lookup_feeds_a_later_one() {
        // This is the whole reason lookups are kept as units. `ccmp` turns 10
        // into 11, and only then does the ligature lookup — which covers 11,
        // not 10 — have anything to join. One flat pass over both subtables
        // would find neither.
        let set = ligature_set(&[ligature(20, &[12])]);
        let data = gsub_lookups(&[
            (b"ccmp", LOOKUP_SINGLE, single_list(&[10], &[11])),
            (b"liga", LOOKUP_LIGATURE, ligature_subst(&[11], &[set])),
        ]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 12]), [20]);
    }

    #[test]
    fn a_later_lookup_does_not_feed_an_earlier_one() {
        // The mirror of the test above, and the reason the order is the
        // font's: the same two lookups listed the other way round must *not*
        // ligate, because the ligature lookup runs before the glyph it needs
        // exists. A pass that looped until nothing changed would wrongly
        // ligate here, and would not terminate on a font whose lookups feed
        // each other in a cycle.
        let set = ligature_set(&[ligature(20, &[12])]);
        let data = gsub_lookups(&[
            (b"liga", LOOKUP_LIGATURE, ligature_subst(&[11], &[set])),
            (b"ccmp", LOOKUP_SINGLE, single_list(&[10], &[11])),
        ]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 12]), [11, 12]);
    }

    #[test]
    fn a_substitution_is_not_offered_to_the_lookup_that_made_it() {
        // A font whose output is also its input must not loop or cascade
        // inside one lookup: 10 becomes 11 once, not 12.
        let data = gsub_lookups(&[(
            b"ccmp",
            LOOKUP_SINGLE,
            single_list(&[10, 11], &[11, 12]),
        )]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [11]);
    }

    /// A truncated table must come back empty, not panic and not read past
    /// the end. Fonts arrive from the filesystem and are not trusted.
    #[test]
    fn a_truncated_table_is_survivable() {
        let (data, subs) = fi_font();
        for cut in 0..data.len() {
            let short = &data[..cut];
            // Parsing what is left must not panic...
            let _ = Substitutions::parse(short, Some(span(0, short.len())), None);
            // ...and neither must applying the lookups to it.
            let _ = subst(short, &subs, &[10, 11, 12]);
        }
    }

    /// The same, for a table with two lookups of different types.
    #[test]
    fn a_truncated_multi_lookup_table_is_survivable() {
        let set = ligature_set(&[ligature(20, &[12])]);
        let data = gsub_lookups(&[
            (b"ccmp", LOOKUP_SINGLE, single_delta(&[10], 1)),
            (b"liga", LOOKUP_LIGATURE, ligature_subst(&[11], &[set])),
        ]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        for cut in 0..data.len() {
            let short = &data[..cut];
            let _ = Substitutions::parse(short, Some(span(0, short.len())), None);
            let _ = subst(short, &subs, &[10, 11, 12]);
        }
    }

    #[test]
    fn a_ligature_claiming_more_components_than_exist_is_refused() {
        // componentCount is a u16 the font supplies; a corrupt one must not
        // make the matcher walk off the end of the run.
        let mut lig = Vec::new();
        lig.extend_from_slice(&be16(20));
        lig.extend_from_slice(&be16(u16::MAX));
        let set = ligature_set(&[lig]);
        let sub = ligature_subst(&[10], &[set]);
        let data = gsub_table(b"liga", LOOKUP_LIGATURE, &sub);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 11, 12]), [10, 11, 12]);
    }

    #[test]
    fn a_multiple_substitution_decomposes_one_glyph_into_several() {
        // 10 is a precomposed letter the face draws as a base plus a mark.
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10], &[&[30, 31]]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [30, 31]);
        assert_eq!(subst(&data, &subs, &[9, 10, 11]), [9, 30, 31, 11]);
        // Uncovered glyphs are untouched.
        assert_eq!(subst(&data, &subs, &[9, 11]), [9, 11]);
    }

    #[test]
    fn a_decomposed_glyph_gives_every_piece_its_own_characters_cluster() {
        // The clusters are what the layout queries key on: both new glyphs
        // came from the same character, so both must name its byte offset.
        // Anything else and a caret can be drawn between them.
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[11], &[&[30, 31, 32]]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 11, 12]), [10, 30, 31, 32, 12]);
        assert_eq!(clusters(&data, &subs, &[10, 11, 12]), [0, 1, 1, 1, 2]);
    }

    #[test]
    fn every_glyph_of_a_run_is_decomposed_not_just_the_first() {
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10, 12], &[&[30, 31], &[40, 41]]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 11, 12]), [30, 31, 11, 40, 41]);
        assert_eq!(clusters(&data, &subs, &[10, 11, 12]), [0, 0, 1, 2, 2]);
    }

    #[test]
    fn a_decomposition_is_not_offered_back_to_the_lookup_that_made_it() {
        // 10 decomposes to 30 and 12 — and 12 is itself covered. Walking has
        // to resume *past* the whole insertion, so the 12 this lookup just
        // wrote is left alone; resuming one glyph on would decompose it again
        // and hand back three glyphs.
        let data = gsub_table(
            b"ccmp",
            LOOKUP_MULTIPLE,
            &multiple(&[10, 12], &[&[30, 12], &[40, 41]]),
        );
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [30, 12]);
        // A 12 that was in the run to begin with is still decomposed: the rule
        // is about what this lookup produced, not about the glyph id.
        assert_eq!(subst(&data, &subs, &[12]), [40, 41]);

        // And the degenerate case the same rule has to cover: a glyph that
        // decomposes to itself. Anything re-examining its own output would not
        // terminate here at all, which is why the rule is a rule and not a
        // tidiness preference.
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10], &[&[10, 30]]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [10, 30]);
    }

    #[test]
    fn a_decomposition_feeds_a_later_ligature() {
        // The ordering rule, in the direction `ccmp` exists for: 10 becomes
        // 30 and 31, and only then does a ligature covering 31+12 have
        // anything to join. Cluster 1 is swallowed by the ligature, so the
        // run ends 30(cluster 0), 40(cluster 0) — the ligature keeps its
        // first component's cluster, which is the decomposed character's.
        let set = ligature_set(&[ligature(40, &[12])]);
        let data = gsub_lookups(&[
            (b"ccmp", LOOKUP_MULTIPLE, multiple(&[10], &[&[30, 31]])),
            (b"liga", LOOKUP_LIGATURE, ligature_subst(&[31], &[set])),
        ]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 12]), [30, 40]);
        assert_eq!(clusters(&data, &subs, &[10, 12]), [0, 0]);
    }

    #[test]
    fn a_sequence_of_no_glyphs_leaves_the_glyph_alone() {
        // The spec forbids an empty Sequence; some shapers delete the glyph
        // anyway. Deleting takes the cluster with it, leaving a character no
        // caret position corresponds to, so this refuses instead.
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10], &[&[]]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 11]), [10, 11]);
        assert_eq!(clusters(&data, &subs, &[10, 11]), [0, 1]);
    }

    #[test]
    fn an_empty_sequence_lets_a_later_subtable_of_the_same_lookup_answer() {
        // Refusing an empty Sequence is not the same as giving up on the
        // glyph. The subtables of a lookup are tried in turn, so the refusal
        // has to be *this subtable's* — the next one still gets its say.
        let empty = multiple(&[10], &[&[]]);
        let real = multiple(&[10], &[&[30, 31]]);
        let data = gsub_subtables(b"ccmp", LOOKUP_MULTIPLE, &[&empty, &real]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [30, 31]);
        assert_eq!(clusters(&data, &subs, &[10]), [0, 0]);
    }

    #[test]
    fn a_subtable_that_reads_half_a_sequence_leaves_nothing_behind() {
        // The first subtable covers 10 and its Sequence claims two glyphs,
        // but the table ends between them: the read collects one glyph and
        // then fails. Those glyphs are not an answer, and must not turn up in
        // front of the one the second subtable does have.
        let real = multiple(&[10], &[&[30, 31]]);
        // A MultipleSubstFormat1 by hand, so its Sequence can be pointed at
        // the very end of the table — the only place a read can run off.
        let mut bad = Vec::new();
        bad.extend_from_slice(&be16(1)); // substFormat
        bad.extend_from_slice(&be16(8)); // coverage, after the offset array
        bad.extend_from_slice(&be16(1)); // sequenceCount
        bad.extend_from_slice(&be16(0)); // sequence offset, patched below
        bad.extend_from_slice(&coverage1(&[10]));

        let mut data = gsub_subtables(b"ccmp", LOOKUP_MULTIPLE, &[&bad, &real]);
        let sub = data.len() - real.len() - bad.len();
        let tail = data.len();
        data.extend_from_slice(&be16(2)); // glyphCount
        data.extend_from_slice(&be16(44)); // glyph 0 — and then nothing
        let offset = be16(u16::try_from(tail - sub).unwrap());
        data[sub + 6..sub + 8].copy_from_slice(&offset);

        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [30, 31]);
    }

    #[test]
    fn a_sequence_longer_than_the_cap_is_refused() {
        let long: Vec<u16> = (0..u16::try_from(MAX_SEQUENCE + 1).unwrap()).collect();
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10], &[&long]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [10]);

        // And exactly the cap is still allowed: the bound is on absurdity,
        // not on a font that happens to be at the limit.
        let at_cap: Vec<u16> = (0..u16::try_from(MAX_SEQUENCE).unwrap()).collect();
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10], &[&at_cap]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10]).len(), MAX_SEQUENCE);
    }

    #[test]
    fn a_multiple_substitution_of_an_unknown_format_is_refused() {
        let mut sub = multiple(&[10], &[&[30, 31]]);
        sub[0..2].copy_from_slice(&be16(2));
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &sub);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [10]);
    }

    #[test]
    fn a_truncated_multiple_substitution_is_survivable() {
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10, 12], &[&[30, 31], &[40]]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        for cut in 0..data.len() {
            let short = &data[..cut];
            let _ = Substitutions::parse(short, Some(span(0, short.len())), None);
            let _ = subst(short, &subs, &[10, 11, 12]);
        }
    }

    /// A contextual lookup and the helper it invokes.
    ///
    /// The helper is tagged `dlig`, which is *off* by default, so the only way
    /// it can reach a run is by being invoked — which is the arrangement real
    /// fonts use and the thing that makes these tests test the invocation
    /// rather than the helper.
    fn context_font(kind: u16, sub: Vec<u8>, helpers: &[(u16, Vec<u8>)]) -> (Vec<u8>, Substitutions) {
        let mut lookups: Vec<(&[u8; 4], u16, Vec<u8>)> = alloc::vec![(b"calt", kind, sub)];
        // Three off-by-default tags, so a helper is never reachable on its own.
        for (tag, (kind, sub)) in [b"dlig", b"hlig", b"swsh"].iter().zip(helpers) {
            lookups.push((tag, *kind, sub.clone()));
        }
        let data = gsub_lookups(&lookups);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        (data, subs)
    }

    /// Single substitution 10 -> 30, the usual helper below.
    fn helper_10_to_30() -> (u16, Vec<u8>) {
        (LOOKUP_SINGLE, single_list(&[10], &[30]))
    }

    #[test]
    fn a_context_substitutes_only_where_the_context_stands() {
        // 10 becomes 30, but only when an 11 follows it.
        let set = rule_set_of(&[rule(&[11], &[(0, 1)])]);
        let (data, subs) = context_font(
            LOOKUP_CONTEXT,
            context1(&[10], &[set]),
            &[helper_10_to_30()],
        );
        assert_eq!(subst(&data, &subs, &[10, 11]), [30, 11]);
        assert_eq!(subst(&data, &subs, &[10, 12]), [10, 12]);
        assert_eq!(subst(&data, &subs, &[11, 10, 11]), [11, 30, 11]);
        // The context is the whole of the input, so a 10 with nothing after it
        // is not one.
        assert_eq!(subst(&data, &subs, &[10]), [10]);
    }

    #[test]
    fn a_lookup_no_feature_reaches_is_still_invocable() {
        // The helper alone would turn every 10 into a 30. It is tagged `dlig`,
        // which is off by default, so it must do nothing until the context
        // calls for it — the arrangement by which a font keeps a rule private
        // to one context.
        let set = rule_set_of(&[rule(&[11], &[(0, 1)])]);
        let (data, subs) = context_font(
            LOOKUP_CONTEXT,
            context1(&[10], &[set]),
            &[helper_10_to_30()],
        );
        assert_eq!(subst(&data, &subs, &[10, 10, 10]), [10, 10, 10]);
        assert_eq!(subst(&data, &subs, &[10, 11, 10]), [30, 11, 10]);
    }

    #[test]
    fn a_rule_set_tries_its_rules_in_the_fonts_order() {
        // Longest first, as with ligatures: the font decides, and the first
        // rule that matches wins rather than the most specific one.
        let set = rule_set_of(&[
            rule(&[11, 12], &[(0, 1)]),
            rule(&[11], &[(0, 2)]),
        ]);
        let (data, subs) = context_font(
            LOOKUP_CONTEXT,
            context1(&[10], &[set]),
            &[helper_10_to_30(), (LOOKUP_SINGLE, single_list(&[10], &[40]))],
        );
        assert_eq!(subst(&data, &subs, &[10, 11, 12]), [30, 11, 12]);
        assert_eq!(subst(&data, &subs, &[10, 11, 13]), [40, 11, 13]);
    }

    #[test]
    fn a_class_based_context_matches_every_glyph_of_a_class() {
        // Glyphs 10 and 11 are class 1; 12 and 13 are class 2. One rule —
        // "class 1 then class 2" — covers all four pairings, which is the
        // whole reason format 2 exists.
        let classes = class_def(10, &[1, 1, 2, 2]);
        let sets = alloc::vec![
            rule_set_of(&[]),
            rule_set_of(&[rule(&[2], &[(0, 1)])]),
            rule_set_of(&[]),
        ];
        let (data, subs) = context_font(
            LOOKUP_CONTEXT,
            context2(&[10, 11, 12, 13], &classes, &sets),
            &[(LOOKUP_SINGLE, single_list(&[10, 11], &[30, 31]))],
        );
        assert_eq!(subst(&data, &subs, &[10, 12]), [30, 12]);
        assert_eq!(subst(&data, &subs, &[11, 13]), [31, 13]);
        // Two glyphs of the same class are not "class 1 then class 2".
        assert_eq!(subst(&data, &subs, &[10, 11]), [10, 11]);
        // And a glyph outside the coverage never reaches the class lookup at
        // all, even though the ClassDef would put it in class 0.
        assert_eq!(subst(&data, &subs, &[20, 12]), [20, 12]);
    }

    #[test]
    fn a_null_rule_set_offset_is_not_followed() {
        // Zero means absent, and a format-2 table is mostly zeroes: a ClassDef
        // sorts every glyph in the font into some class, and only a few
        // classes start a context. Following one would land back on the
        // subtable's own header and read its format as a rule count.
        let classes = class_def(10, &[1, 1]);
        // Both glyphs are class 1, and a format-2 rule names classes.
        let sets = alloc::vec![
            rule_set_of(&[]),
            rule_set_of(&[rule(&[1], &[(0, 1)])]),
        ];
        let mut sub = context2(&[10, 11], &classes, &sets);
        // The class-1 rule set is the second offset, at byte 10 of the header.
        assert_eq!(subst_with(&sub, &[10, 11]), [30, 11]);
        sub[10..12].copy_from_slice(&be16(0));
        assert_eq!(subst_with(&sub, &[10, 11]), [10, 11]);
    }

    /// Build a context font around `sub` with the usual 10 -> 30 helper and
    /// run it, for the tests that only care about whether the context fired.
    fn subst_with(sub: &[u8], gids: &[u16]) -> Vec<u16> {
        let (data, subs) = context_font(
            LOOKUP_CONTEXT,
            sub.to_vec(),
            &[helper_10_to_30()],
        );
        subst(&data, &subs, gids)
    }

    #[test]
    fn a_coverage_based_context_matches_a_fixed_sequence() {
        // Format 3 is one context, not a set of them: every position gets its
        // own coverage table and there is nothing keyed by the first glyph.
        let (data, subs) = context_font(
            LOOKUP_CONTEXT,
            context3(&[&[10, 11], &[20]], &[(0, 1)]),
            &[(LOOKUP_SINGLE, single_list(&[10, 11], &[30, 31]))],
        );
        assert_eq!(subst(&data, &subs, &[10, 20]), [30, 20]);
        assert_eq!(subst(&data, &subs, &[11, 20]), [31, 20]);
        assert_eq!(subst(&data, &subs, &[10, 21]), [10, 21]);
    }

    #[test]
    fn a_chaining_context_looks_behind_and_ahead() {
        // Only the input is substituted; the backtrack and lookahead are
        // conditions, and come out of the run exactly as they went in.
        let (data, subs) = context_font(
            LOOKUP_CHAIN_CONTEXT,
            chain_context3(&[&[5]], &[&[10]], &[&[20]], &[(0, 1)]),
            &[helper_10_to_30()],
        );
        assert_eq!(subst(&data, &subs, &[5, 10, 20]), [5, 30, 20]);
        assert_eq!(subst(&data, &subs, &[10, 20]), [10, 20]);
        assert_eq!(subst(&data, &subs, &[5, 10, 21]), [5, 10, 21]);
        assert_eq!(subst(&data, &subs, &[6, 10, 20]), [6, 10, 20]);
        // The backtrack may sit anywhere in the run, not just at its start.
        assert_eq!(subst(&data, &subs, &[9, 5, 10, 20]), [9, 5, 30, 20]);
    }

    #[test]
    fn a_backtrack_is_stored_closest_glyph_first() {
        // The one part of the format that reads backwards: entry 0 is the
        // glyph immediately before the input, not the leftmost of the context.
        // Reading it in text order would match the reversed run instead.
        let (data, subs) = context_font(
            LOOKUP_CHAIN_CONTEXT,
            chain_context3(&[&[6], &[5]], &[&[10]], &[], &[(0, 1)]),
            &[helper_10_to_30()],
        );
        assert_eq!(subst(&data, &subs, &[5, 6, 10]), [5, 6, 30]);
        assert_eq!(subst(&data, &subs, &[6, 5, 10]), [6, 5, 10]);
    }

    #[test]
    fn a_chaining_context_by_glyph_and_by_class() {
        // Format 1: rule sets keyed by the input's first glyph.
        let set = rule_set_of(&[chained(&[5], &[], &[20], &[(0, 1)])]);
        let (data, subs) = context_font(
            LOOKUP_CHAIN_CONTEXT,
            context1(&[10], &[set]),
            &[helper_10_to_30()],
        );
        assert_eq!(subst(&data, &subs, &[5, 10, 20]), [5, 30, 20]);
        assert_eq!(subst(&data, &subs, &[5, 10, 21]), [5, 10, 21]);

        // Format 2: three ClassDefs, one per part — and a glyph may sit in a
        // different class in each. Here 5 and 6 are backtrack class 1, 10 is
        // input class 1, and 20 and 21 are lookahead class 1.
        let back = class_def(5, &[1, 1]);
        let input = class_def(10, &[1]);
        let ahead = class_def(20, &[1, 1]);
        let sets = alloc::vec![
            rule_set_of(&[]),
            rule_set_of(&[chained(&[1], &[], &[1], &[(0, 1)])]),
        ];
        let (data, subs) = context_font(
            LOOKUP_CHAIN_CONTEXT,
            chain_context2(&[10], &back, &input, &ahead, &sets),
            &[helper_10_to_30()],
        );
        assert_eq!(subst(&data, &subs, &[5, 10, 20]), [5, 30, 20]);
        assert_eq!(subst(&data, &subs, &[6, 10, 21]), [6, 30, 21]);
        assert_eq!(subst(&data, &subs, &[7, 10, 20]), [7, 10, 20]);
        assert_eq!(subst(&data, &subs, &[5, 10, 22]), [5, 10, 22]);
    }

    #[test]
    fn a_nested_decomposition_moves_the_records_that_follow_it() {
        // Record 0 turns glyph 10 into two glyphs. Record 1 names input
        // position 1 — glyph 11 — which by then stands one place further
        // right. Naming the position as it was matched, rather than as it now
        // is, is the whole difficulty of running several records.
        let (data, subs) = context_font(
            LOOKUP_CONTEXT,
            context3(&[&[10], &[11]], &[(0, 1), (1, 2)]),
            &[
                (LOOKUP_MULTIPLE, multiple(&[10], &[&[30, 31]])),
                (LOOKUP_SINGLE, single_list(&[11], &[41])),
            ],
        );
        // Without the shift, record 1 would land on glyph 31 — which lookup 2
        // does not cover — and the 11 would come out unchanged.
        assert_eq!(subst(&data, &subs, &[10, 11]), [30, 31, 41]);
        assert_eq!(clusters(&data, &subs, &[10, 11]), [0, 0, 1]);
    }

    #[test]
    fn a_nested_ligature_moves_the_records_that_follow_it() {
        // The mirror case: record 0 joins two glyphs into one, so everything
        // to its right stands one place further left.
        let set = ligature_set(&[ligature(40, &[11])]);
        let (data, subs) = context_font(
            LOOKUP_CONTEXT,
            context3(&[&[10], &[11], &[12], &[13]], &[(0, 1), (2, 2)]),
            &[
                (LOOKUP_LIGATURE, ligature_subst(&[10], &[set])),
                (LOOKUP_SINGLE, single_list(&[12], &[50])),
            ],
        );
        assert_eq!(subst(&data, &subs, &[10, 11, 12, 13]), [40, 50, 13]);
    }

    #[test]
    fn a_record_naming_a_glyph_a_ligature_swallowed_is_dropped() {
        // Record 0 joins 10 and 11. Record 1 names input position 1 — the 11,
        // which no longer exists. Sliding the index instead would apply the
        // lookup to the 12, a glyph the context matched but this record never
        // named.
        let set = ligature_set(&[ligature(40, &[11])]);
        let (data, subs) = context_font(
            LOOKUP_CONTEXT,
            context3(&[&[10], &[11], &[12]], &[(0, 1), (1, 2)]),
            &[
                (LOOKUP_LIGATURE, ligature_subst(&[10], &[set])),
                (LOOKUP_SINGLE, single_list(&[11, 12], &[51, 52])),
            ],
        );
        assert_eq!(subst(&data, &subs, &[10, 11, 12]), [40, 12]);
    }

    #[test]
    fn a_context_that_invokes_itself_terminates() {
        // Nothing in the format forbids lookup 0 from naming lookup 0. The
        // depth cap is the only thing that ends it, and a run that never
        // returns is not a rendering fault a user can work around.
        let (data, subs) = context_font(LOOKUP_CONTEXT, context3(&[&[10]], &[(0, 0)]), &[]);
        assert_eq!(subst(&data, &subs, &[10, 10]), [10, 10]);

        // And two lookups that name each other, which the per-invocation
        // depth has to bound just the same.
        let data = gsub_lookups(&[
            (b"calt", LOOKUP_CONTEXT, context3(&[&[10]], &[(0, 1)])),
            (b"dlig", LOOKUP_CONTEXT, context3(&[&[10]], &[(0, 0)])),
        ]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [10]);
    }

    #[test]
    fn a_context_longer_than_the_cap_is_refused() {
        let covers: Vec<Vec<u16>> = (0..=MAX_CONTEXT)
            .map(|i| alloc::vec![u16::try_from(i).unwrap() + 10])
            .collect();
        let refs: Vec<&[u16]> = covers.iter().map(alloc::vec::Vec::as_slice).collect();
        let run: Vec<u16> = (0..u16::try_from(MAX_CONTEXT + 1).unwrap())
            .map(|i| i + 10)
            .collect();
        let (data, subs) = context_font(
            LOOKUP_CONTEXT,
            context3(&refs, &[(0, 1)]),
            &[helper_10_to_30()],
        );
        assert_eq!(subst(&data, &subs, &run), run);

        // Exactly the cap is still allowed: the bound is on absurdity, not on
        // a font sitting at the limit.
        let (data, subs) = context_font(
            LOOKUP_CONTEXT,
            context3(&refs[..MAX_CONTEXT], &[(0, 1)]),
            &[helper_10_to_30()],
        );
        assert_eq!(subst(&data, &subs, &run)[0], 30);
    }

    #[test]
    fn a_contextual_substitution_of_an_unknown_format_is_refused() {
        for kind in [LOOKUP_CONTEXT, LOOKUP_CHAIN_CONTEXT] {
            let mut sub = context3(&[&[10], &[11]], &[(0, 1)]);
            sub[0..2].copy_from_slice(&be16(4));
            let (data, subs) = context_font(kind, sub, &[helper_10_to_30()]);
            assert_eq!(subst(&data, &subs, &[10, 11]), [10, 11]);
        }
    }

    #[test]
    fn a_truncated_contextual_substitution_is_survivable() {
        let subtables = alloc::vec![
            (LOOKUP_CONTEXT, context1(&[10], &[rule_set_of(&[rule(&[11], &[(0, 1)])])])),
            (LOOKUP_CONTEXT, context2(&[10, 11], &class_def(10, &[1, 1]), &[
                rule_set_of(&[]),
                rule_set_of(&[rule(&[1], &[(0, 1)])]),
            ])),
            (LOOKUP_CONTEXT, context3(&[&[10], &[11]], &[(0, 1)])),
            (LOOKUP_CHAIN_CONTEXT, context1(&[10], &[rule_set_of(&[chained(&[5], &[], &[20], &[(0, 1)])])])),
            (LOOKUP_CHAIN_CONTEXT, chain_context2(&[10], &class_def(5, &[1]), &class_def(10, &[1]), &class_def(20, &[1]), &[
                rule_set_of(&[]),
                rule_set_of(&[chained(&[1], &[], &[1], &[(0, 1)])]),
            ])),
            (LOOKUP_CHAIN_CONTEXT, chain_context3(&[&[5]], &[&[10]], &[&[20]], &[(0, 1)])),
        ];
        for (kind, sub) in subtables {
            let (data, subs) = context_font(kind, sub, &[helper_10_to_30()]);
            for cut in 0..data.len() {
                let short = &data[..cut];
                let _ = Substitutions::parse(short, Some(span(0, short.len())), None);
                let _ = subst(short, &subs, &[5, 10, 11, 20]);
            }
        }
    }
}
