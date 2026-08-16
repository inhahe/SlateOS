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
//! * **The reordering features** — `rphf`, `half`, `pref`, `abvs` and the rest
//!   of the Indic and Universal Shaping Engine sets. Unlike the Arabic four,
//!   these need the cluster *rearranged* before the features are chosen, which
//!   is a shaper this crate does not have yet. See
//!   `TD-FONT-HAS-NO-JOINING-OR-REORDERING-SHAPER`.
//! * **Syriac's `fin2`, `fin3` and `med2`**, the alaph forms, and Arabic's
//!   `mset` and `stch`. See [`joining`](crate::joining).

use alloc::vec::Vec;

use crate::context::{MAX_NESTING, Matched, Nested, chain_match, context_match, read_records};
use crate::indic::Char;
use crate::joining::Form;
use crate::lang::Lang;
use crate::otl::{
    ByScript, Lookup, MAX_SUBTABLES, coverage_index, lookup_at, lookup_list,
};
use crate::script::ScriptTags;
use crate::sfnt::{Span, u16_at};
use crate::skip::{CLASS_BASE, CLASS_MARK, Definitions, Skipper};
use crate::would::would_apply;

/// The features read, in the order whose positions become the mask bits.
///
/// The unconditional ones come first so that "every feature a glyph always
/// gets" is one contiguous run of bits ([`ALWAYS`]); the four positional ones
/// follow, one bit each.
///
/// The positioning-sounding tags at the end of the unconditional run are here
/// for the same reason the substitution-sounding ones are in
/// [`gpos`](crate::gpos)'s list: HarfBuzz builds one feature map and compiles
/// it against both tables, so nothing stops a face filing a substitution under
/// `mark` or a `PairPos` under `calt`. The two lists are the same set,
/// deliberately.
/// Visible to the crate so that `the_survey_matches_the_shapers_feature_list`
/// in [`otl`](crate::otl) can pin it against the tool that measures faces
/// against it.
pub(crate) const FEATURES: &[&[u8; 4]] = &[
    // Unconditional: every glyph is eligible for all of these.
    b"ccmp", b"locl", b"liga", b"rlig", b"clig", b"calt", b"rclt", b"abvm", b"blwm", b"curs",
    b"dist", b"kern", b"mark", b"mkmk",
    // Positional: a glyph is eligible for at most one, and only when the
    // cursive joining pass says so.
    b"isol", b"init", b"medi", b"fina",
    // Indic: a glyph is eligible for one of these only when the Indic shaper
    // has laid its syllable out and said so. The order is HarfBuzz's
    // `indic_features[]`, which is also the order they are *applied* in — the
    // eleven basic ones one stage each, then the rest together — so
    // [`indic_shape`](crate::indic_shape) can name a stage by its bit.
    //
    // `init` is not repeated: the Indic shaper's is the same tag as the
    // cursive one, gated the same way (per glyph, set by the shaper), and no
    // run is both Indic and cursive. HarfBuzz likewise builds one feature map
    // per plan and lets whichever shaper is running own the tag.
    b"nukt", b"akhn", b"rphf", b"rkrf", b"pref", b"blwf", b"abvf", b"half", b"pstf", b"vatu",
    b"cjct", b"pres", b"abvs", b"blws", b"psts", b"haln",
    // Hangul: a glyph is eligible for one of these only when
    // [`hangul`](crate::hangul) has decided the syllable's spelling and said
    // which slot the jamo occupies. Appended rather than inserted, so that no
    // bit constant above moves.
    b"ljmo", b"vjmo", b"tjmo",
];

/// The feature mask every glyph carries: bits for the fourteen unconditional
/// entries of [`FEATURES`], and none of the four positional ones.
///
/// Written out rather than computed from `FEATURES`, so that no shift or
/// subtraction appears in a path the arithmetic lints police. The two are
/// kept in step by `the_masks_match_the_feature_list`, which fails if an
/// entry is ever inserted or reordered.
const ALWAYS: u64 = 0b0011_1111_1111_1111;
/// The bit for `isol`, the fifteenth entry of [`FEATURES`].
const ISOL: u64 = 0b0100_0000_0000_0000;
/// The bit for `init`, the sixteenth.
const INIT: u64 = 0b1000_0000_0000_0000;
/// The bit for `medi`, the seventeenth.
const MEDI: u64 = 0b1_0000_0000_0000_0000;
/// The bit for `fina`, the eighteenth.
const FINA: u64 = 0b10_0000_0000_0000_0000;

/// The bit for `calt`, the sixth entry of [`FEATURES`].
///
/// Named because the Hangul pass has to *clear* it, which is the one place a
/// constructor takes a bit away rather than adding one.
const CALT: u64 = 0b10_0000;

/// The bit for `ljmo`, the thirty-fifth entry of [`FEATURES`].
const LJMO: u64 = 1 << 34;
/// The bit for `vjmo`, the thirty-sixth.
const VJMO: u64 = 1 << 35;
/// The bit for `tjmo`, the thirty-seventh.
const TJMO: u64 = 1 << 36;

/// Every feature at once: the one stage a caller with no staging plan runs.
///
/// It is all sixty-four bits rather than the bits of [`FEATURES`] because a
/// stage is intersected with each lookup's own mask, and a bit no feature
/// occupies is a bit no lookup carries — so the extra ones select nothing and
/// the constant needs no maintenance when the list grows.
pub(crate) const ALL_FEATURES: u64 = u64::MAX;

/// The mask bit of the feature tagged `tag`, or `0` for a tag this crate never
/// asks a face for.
///
/// Zero is a usable answer rather than an error: a bit no feature occupies is a
/// bit no lookup carries, so intersecting with it selects nothing — which is
/// exactly what "this crate does not read that feature" should mean to a
/// caller. It is the right answer for the wrong reason, and it is why every tag
/// [`indic_shape`](crate::indic_shape) names must also be listed in
/// [`FEATURES`]; `the_indic_features_all_have_bits` is the test that says so.
pub(crate) fn feature_bit(tag: &[u8; 4]) -> u64 {
    FEATURES
        .iter()
        .position(|want| *want == tag)
        .and_then(|i| u32::try_from(i).ok())
        .and_then(|i| 1u64.checked_shl(i))
        .unwrap_or(0)
}

/// The mask selecting every feature in `tags` at once.
///
/// A tag this crate never asks a face for contributes nothing, for the reason
/// [`feature_bit`] gives.
pub(crate) fn feature_bits(tags: &[&[u8; 4]]) -> u64 {
    tags.iter().fold(0, |mask, tag| mask | feature_bit(tag))
}

/// The mask for a glyph whose cursive form is `form`.
///
/// `None` — a space, a mark, a Latin letter, anything in a face with no
/// cursive script at all — gets the unconditional features only.
fn form_mask(form: Option<Form>) -> u64 {
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
pub(crate) const LOOKUP_SINGLE: u16 = 1;
/// `GSUB` lookup type for multiple substitution: one glyph becomes several.
pub(crate) const LOOKUP_MULTIPLE: u16 = 2;
/// `GSUB` lookup type for alternate substitution: one glyph, several
/// candidates, an index chooses.
pub(crate) const LOOKUP_ALTERNATE: u16 = 3;
/// `GSUB` lookup type for ligature substitution: several glyphs for one.
pub(crate) const LOOKUP_LIGATURE: u16 = 4;
/// `GSUB` lookup type for contextual substitution: a rule that fires only
/// where a given sequence of glyphs stands, and then invokes other lookups.
pub(crate) const LOOKUP_CONTEXT: u16 = 5;
/// `GSUB` lookup type for chaining contextual substitution: like
/// [`LOOKUP_CONTEXT`], but the context extends either side of what is
/// substituted.
pub(crate) const LOOKUP_CHAIN_CONTEXT: u16 = 6;
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
    pub(crate) mask: u64,
    /// Where this glyph's character attaches, if it is a combining mark —
    /// [`fallback::attach_class`](crate::fallback::attach_class) of the
    /// character `cmap` looked it up from, and `0` for anything that is not a
    /// mark.
    ///
    /// It rides along here rather than being recovered later from
    /// [`cluster`](Self::cluster), because a cluster is a byte offset shared
    /// by a base and every mark on it and so cannot tell them apart; and
    /// rather than from the glyph id, because the glyph id is precisely what
    /// substitution is free to change. Substitution has no opinion about
    /// combining classes, so every lookup carries this through untouched — and
    /// a ligature keeps its first component's, which is the base's, which is
    /// zero.
    ///
    /// Left at `0` unless the face needs the fallback at all, since deriving
    /// it costs a table lookup per character.
    pub(crate) klass: u8,
    /// Whether this glyph's character is a non-spacing combining mark —
    /// general category `Mn`.
    ///
    /// Separate from [`klass`](Self::klass) because it answers a different
    /// question and is true in cases the class is not. `klass` says *where*
    /// the fallback should put a mark and is left at `0` for the scripts whose
    /// marks the fallback declines to place; this says only *that* it is a
    /// mark, which is what decides its advance is zero — and a mark takes no
    /// room whether or not anything is willing to place it. It is also true
    /// for the marks Unicode leaves at combining class 0 because they never
    /// need reordering, U+0E35 THAI SARA II among them.
    ///
    /// Carried through substitution exactly as `klass` is: no lookup has an
    /// opinion about it, and a ligature keeps its first component's, which is
    /// the base's, which is false.
    ///
    /// Left `false` unless the face has no `GPOS`, since a face that has one
    /// has a `GDEF` to be asked about the glyph instead — a better answer,
    /// because it is about the glyph substitution actually produced rather
    /// than about the character it started as.
    pub(crate) mark: bool,
    /// Where this glyph sits inside a ligature, once one has swallowed it or
    /// the glyphs around it. Written by ligature substitution and read by
    /// `GPOS`'s mark-to-ligature attachment, which is the only thing that
    /// needs to know *which* half of an `ﻻ` a vowel sign belongs over.
    pub(crate) lig: Lig,
    /// Which syllable this glyph belongs to, for the features that may not
    /// match across a syllable boundary.
    ///
    /// Only the Indic shaper writes it; everything else leaves it at `0`,
    /// where it means "one syllable, the whole run" and constrains nothing.
    /// The value is a small serial rather than an index, because all that is
    /// ever asked of it is whether two glyphs share one — so it need only
    /// differ between *neighbours*, and a byte is enough for that however long
    /// the run is.
    ///
    /// Carried through substitution untouched, exactly as
    /// [`klass`](Self::klass) is: a ligature keeps its first component's, and
    /// the pieces of a decomposition all keep the whole's. Both are right,
    /// because a lookup cannot join glyphs from two syllables in the first
    /// place.
    pub(crate) syllable: u8,
    /// Whether this glyph's character *continues* a word — so that the glyph
    /// after it does not begin one.
    ///
    /// HarfBuzz asks the preceding character's Unicode general category and
    /// counts a word as continuing through `Cf`, `Cn`, `Co`, `Cs`, every letter
    /// and every mark. The question is asked of the *character*, so like
    /// [`indic`](Self::indic) it has to be answered while there is still a
    /// character to ask, and carried here.
    ///
    /// Read by exactly one thing: the Indic shaper's `init` feature, which is
    /// for a left matra that begins a word. So it is only computed on runs an
    /// Indic shaper will see, and left `false` — "everything begins a word" —
    /// everywhere else, where nothing reads it.
    pub(crate) word: bool,
    /// What the Indic shaper thinks this glyph is and where it belongs.
    ///
    /// Set from the *character* `cmap` looked the glyph up from, because that
    /// is the only place the answer exists: an Indic category is a property of
    /// the code point, and by the time a lookup has run there may be no code
    /// point left to ask. Then edited in place by the shaper, which is the
    /// whole of reordering — [`Position`](crate::indic::Position) starts as the
    /// script's default for the character and ends as where in the laid-out
    /// syllable it goes.
    ///
    /// Carried through substitution untouched, like [`klass`](Self::klass), and
    /// for the same reason: no lookup has an opinion about it. That a ligature
    /// keeps its first component's is what makes a stacked conjunct inherit the
    /// position of the consonant it was built from.
    ///
    /// Left at its default — not Indic, never reordered — for every run no
    /// Indic shaper touches, which is nearly all of them.
    pub(crate) indic: Char,
}

/// Where a glyph sits inside a ligature: HarfBuzz's `lig_props`, unpacked.
///
/// A ligature is one glyph standing for several characters, and a mark that
/// was typed against one of those characters has to be placed against the
/// *component* it belonged to rather than against the joined glyph's single
/// origin. Nothing in the glyph run records that by itself — after `ﻟ`+`ﺎ`
/// ligate, the run is one glyph and a mark, and the mark's cluster is shared
/// with the base. So substitution writes it down as it goes: the ligature
/// records how many components it swallowed, and every glyph that stood
/// between two of them records which component it follows.
///
/// The fields are HarfBuzz's byte with the bit-packing undone. Keeping the
/// packing would buy nothing here — this rides in a `SubGlyph`, not in a
/// buffer the caller allocates per character — and would hide the one
/// genuinely subtle part, which is that `comps` and `comp` are mutually
/// exclusive (HarfBuzz's `IS_LIG_BASE` bit selects between them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Lig {
    /// Which ligature this glyph belongs to, or `0` for none.
    ///
    /// Handed out per run and wrapping after seven, which is HarfBuzz's three
    /// bits. Two live ligatures eight apart in one run cannot both still have
    /// marks pending against them, so the reuse is not observable.
    pub(crate) id: u8,
    /// On the ligature glyph itself: how many components it swallowed. `0` on
    /// everything else, which is what tells the two cases apart.
    comps: u8,
    /// On a glyph that stood *between* the components: which one it follows,
    /// counted from one. Meaningless, and read as `0`, when `comps` is set.
    comp: u8,
    /// Whether a ligature substitution produced this glyph.
    ///
    /// Separate from `comps` on purpose, even though a ligature sets both.
    /// This is HarfBuzz's *glyph property* `LIGATED`, `comps` is its *ligature
    /// property* `IS_LIG_BASE`, and the two come apart: decomposing a ligature
    /// leaves the ligature properties alone — the pieces are still inside the
    /// components they were inside — while the glyph properties gain
    /// `MULTIPLIED`. The Indic shaper reads exactly that combination, and
    /// clears both bits again without disturbing the component numbering.
    ligated: bool,
    /// Whether this glyph is one piece of a multiple substitution's output.
    ///
    /// It changes what the glyph is worth to a later ligature: the pieces of
    /// a decomposition all belong to the component the *first* piece does, so
    /// the pieces after the first contribute none of their own. Mark-to-base
    /// attaches a mark only to the first piece for the same reason, and the
    /// two have to agree or a mark lands a component out.
    multiplied: bool,
}

/// The widest component count and index HarfBuzz's four bits can hold. Kept
/// because the wrap is observable: a ligature of more than fifteen components
/// puts its marks on a component the arithmetic wrapped to, and matching that
/// is the point of transcribing the algorithm rather than inventing one.
const LIG_FIELD_MAX: u8 = 0x0F;

impl Lig {
    /// The glyph a ligature substitution produced.
    ///
    /// Clearing `multiplied` is HarfBuzz's, and it has a note explaining why:
    /// Uniscribe only cares about the *last* transformation, so a glyph that
    /// ligated, decomposed and ligated again is forgiven the decomposition.
    fn ligature(id: u8, components: u8) -> Self {
        Self {
            id,
            comps: (components & LIG_FIELD_MAX).max(1),
            comp: 0,
            ligated: true,
            multiplied: false,
        }
    }

    /// The same glyph, renumbered as one that stood between the components of
    /// ligature `id`, following component `comp`.
    ///
    /// Only the ligature properties change: whether this glyph ligated or was
    /// decomposed is a fact about its history that renumbering it into a new
    /// ligature does not alter.
    fn mark(self, id: u8, comp: u8) -> Self {
        Self {
            id,
            comps: 0,
            comp: comp & LIG_FIELD_MAX,
            ..self
        }
    }

    /// The same glyph as the `n`-th piece of a multiple substitution's output,
    /// counted from zero — so the first piece is `0`, meaning "part of no
    /// component", which is what keeps a decomposition that nothing later
    /// ligates looking exactly like the glyph it replaced.
    fn piece(self, n: u8) -> Self {
        Self {
            multiplied: true,
            ..self.mark(0, n)
        }
    }

    /// Build one directly, for tests in other modules that need a run already
    /// inside a ligature without running substitution to put it there.
    #[cfg(test)]
    pub(crate) fn at(id: u8, components: u8, comp: u8) -> Self {
        Self {
            id,
            comps: components,
            comp,
            ligated: components > 0,
            multiplied: false,
        }
    }

    /// The same, as a multiple substitution leaves it after breaking the
    /// ligature apart again.
    #[cfg(test)]
    pub(crate) fn split(self) -> Self {
        Self {
            multiplied: true,
            ..self
        }
    }

    /// Whether a ligature substitution produced this glyph.
    ///
    /// The Indic shaper asks because a glyph that ligated is no longer the
    /// character it was categorised from — `ka` and `ka+virama+ssa` are one
    /// glyph now, and calling it a consonant would have the base search stop on
    /// a conjunct. HarfBuzz answers the same question the same way and in the
    /// same place: its `is_one_of` opens with "if it ligated, all bets are off".
    pub(crate) fn ligated(self) -> bool {
        self.ligated
    }

    /// Whether a multiple substitution split this glyph out of another.
    pub(crate) fn multiplied(self) -> bool {
        self.multiplied
    }

    /// Whether this glyph ligated and was *not* later broken apart again.
    ///
    /// The Indic shaper's test for "did the font really form this?" — a reph
    /// only moves if the `Ra,Halant` behind it actually joined, and a pre-base
    /// Ra only moves if `pref` actually produced something. A glyph that
    /// ligated and then decomposed produced nothing that survives, so it does
    /// not count.
    pub(crate) fn ligated_and_didnt_multiply(self) -> bool {
        self.ligated && !self.multiplied
    }

    /// Forget that this glyph ligated or was decomposed, keeping which
    /// ligature it belongs to and which component of it.
    ///
    /// HarfBuzz's `_hb_glyph_info_clear_ligated_and_multiplied`, used in one
    /// place: an Indic virama that ligated into a conjunct and was then split
    /// back out has lost the category the shaper needs, and restoring the
    /// category is only half the repair — the glyph also has to stop looking
    /// like a ligature, or every `is_halant` test still refuses it.
    pub(crate) fn clear_ligated_and_multiplied(&mut self) {
        self.ligated = false;
        self.multiplied = false;
    }

    /// Which component of its ligature this glyph follows, or `0` for "none" —
    /// which is also the answer for the ligature glyph itself, since a
    /// ligature does not sit inside itself.
    pub(crate) fn comp(self) -> u8 {
        if self.comps > 0 { 0 } else { self.comp }
    }

    /// How many components this glyph is worth when a further ligature
    /// swallows it. One for anything that is not already a ligature.
    fn components(self) -> u8 {
        if self.comps > 0 { self.comps } else { 1 }
    }

    /// The same, as a ligature about to swallow this glyph should count it.
    ///
    /// Zero for every piece of a decomposition after the first, which is what
    /// makes them all land in one component of the new ligature.
    fn components_in_ligation(self) -> u8 {
        if self.multiplied && self.comp() > 0 {
            0
        } else {
            self.components()
        }
    }
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
            klass: 0,
            mark: false,
            lig: Lig::default(),
            indic: Char::DEFAULT,
            syllable: 0,
            word: false,
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
            klass: 0,
            mark: false,
            lig: Lig::default(),
            indic: Char::DEFAULT,
            syllable: 0,
            word: false,
        }
    }

    /// The same, but for a conjoining Hangul jamo.
    ///
    /// Two departures from [`new`](Self::new), both of them HarfBuzz's
    /// `setup_masks_hangul`:
    ///
    /// - the one feature naming this jamo's slot is added, because the same
    ///   consonant is drawn differently leading and trailing and only the
    ///   feature says which;
    /// - `calt` is **removed**, and this is not an optimization. Noto Sans CJK
    ///   and Source Han Sans file all of their jamo lookups under `calt`, so a
    ///   jamo left eligible for it would be rewritten a second time by the
    ///   lookups this pass has already applied. Clearing the bit per glyph
    ///   rather than switching `calt` off for the run is what lets Latin
    ///   sharing the run keep its contextual alternates.
    ///
    /// `None` means a jamo with no slot — the tone marks and the filler — which
    /// still gets `calt` cleared, since it is still Hangul the face's `calt`
    /// lookups might match.
    #[must_use]
    pub(crate) fn jamo(gid: u16, cluster: usize, slot: Option<crate::hangul::Jamo>) -> Self {
        let bit = match slot {
            Some(crate::hangul::Jamo::Leading) => LJMO,
            Some(crate::hangul::Jamo::Vowel) => VJMO,
            Some(crate::hangul::Jamo::Trailing) => TJMO,
            None => 0,
        };
        Self {
            gid,
            cluster,
            mask: (ALWAYS & !CALT) | bit,
            klass: 0,
            mark: false,
            lig: Lig::default(),
            indic: Char::DEFAULT,
            syllable: 0,
            word: false,
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
    pub(crate) fn masked(gid: u16, cluster: usize, mask: u64) -> Self {
        Self {
            gid,
            cluster,
            mask,
            klass: 0,
            mark: false,
            lig: Lig::default(),
            indic: Char::DEFAULT,
            syllable: 0,
            word: false,
        }
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

    /// Apply every lookup this face offers a run of `script` in `lang` to
    /// `glyphs`, in order, rewriting it in place.
    ///
    /// `glyphs` is one substitution run and the lookups may join anything in
    /// it, so a caller that does not want a ligature to form across some
    /// boundary of its own — a tab, a style change, a bidi run edge — passes
    /// the pieces separately rather than the whole line. A script boundary is
    /// such a boundary, and the reason `script` is a parameter rather than a
    /// property of the face: applying Arabic's `liga` to a Latin word is how a
    /// face that supports both silently corrupts one of them.
    ///
    /// `lang` picks the language system inside that script — `None`, and any
    /// language the script does not register, take its default one. See
    /// [`lang`](crate::lang).
    pub(crate) fn apply(
        &self,
        data: &[u8],
        script: Option<ScriptTags>,
        lang: Option<Lang>,
        glyphs: &mut Vec<SubGlyph>,
    ) {
        self.apply_stages(data, script, lang, &[ALL_FEATURES], 0, glyphs, |_, _| {});
    }

    /// Apply the lookups in several passes, one per entry of `stages`.
    ///
    /// A stage is a set of feature bits; a lookup runs in a stage when some
    /// feature that reached it is in that stage's set, and it sees only the
    /// glyphs eligible for the features in the intersection. [`apply`](Self::apply)
    /// is the one-stage case, and for it this is exactly the old single pass:
    /// every lookup, once, in LookupList order.
    ///
    /// Staging exists for the Indic shaper. Its eleven basic features have to
    /// be applied *one at a time*, each as its own complete pass over the run,
    /// because a later one is written to match glyphs an earlier one built —
    /// `rphf` makes the reph that `abvs` then positions, `half` makes the
    /// half-form that `cjct` then stacks. Run them together, in whatever order
    /// the LookupList happens to list them, and the second half of that pair
    /// looks at the run before the first half rewrote it and does nothing.
    ///
    /// `between` is called after each stage with the stage's index, for the
    /// reordering that has to happen at a particular point in the sequence
    /// rather than before or after all of it.
    ///
    /// `per_syllable` is the set of features that may not match across a
    /// syllable boundary: a lookup some feature in it reaches is offered each
    /// maximal run of glyphs sharing a [`SubGlyph::syllable`] separately, so no
    /// rule inside it can see beyond one. Indic features are all declared that
    /// way — a ligature spanning two syllables is never what the font meant,
    /// whatever its coverage says — but the general ones the Indic shaper runs
    /// alongside them in its last stage are not.
    ///
    /// A *set* and not a flag because that is the granularity the confinement
    /// really has: one lookup can be reached by both a per-syllable feature and
    /// an unconstrained one, and HarfBuzz resolves the clash by confining it —
    /// its `per_syllable |= ` when it merges two entries for one lookup, which
    /// is what the intersection below reproduces.
    pub(crate) fn apply_stages(
        &self,
        data: &[u8],
        script: Option<ScriptTags>,
        lang: Option<Lang>,
        stages: &[u64],
        per_syllable: u64,
        glyphs: &mut Vec<SubGlyph>,
        mut between: impl FnMut(usize, &mut Vec<SubGlyph>),
    ) {
        let mut ctx = Ctx {
            lookup_list: self.lookup_list,
            depth: MAX_NESTING,
            scratch: Vec::new(),
            defs: self.defs,
            mask: ALWAYS,
            serial: 0,
        };
        // Hoisted out of both loops for the same reason `Ctx::scratch` is: a
        // per-syllable stage would otherwise allocate once per syllable per
        // lookup, and Devanagari text is nothing but syllables.
        let mut piece: Vec<SubGlyph> = Vec::new();
        for (i, &stage) in stages.iter().enumerate() {
            for (lookup, mask) in self.lookups.for_script(script, lang) {
                let mask = mask & stage;
                if mask == 0 {
                    continue;
                }
                ctx.mask = mask;
                if mask & per_syllable != 0 {
                    apply_per_syllable(data, lookup, glyphs, &mut ctx, &mut piece);
                } else {
                    apply_lookup(data, lookup, glyphs, &mut ctx);
                }
            }
            between(i, glyphs);
        }
    }

    /// The bit that selects `tag`'s lookups on a run of `script` in `lang`, or
    /// `0` if this face reaches none.
    ///
    /// HarfBuzz's `get_1_mask`, and the difference from
    /// [`feature_bit`] is the whole point of having it: `feature_bit` says
    /// which bit the tag *would* use, this says whether the face gives it
    /// anything to select. The Indic shaper reads it as a yes-or-no — "does
    /// this font form a reph at all?" — and then, when the answer is yes, as
    /// the bit to set on the glyphs that should get one.
    pub(crate) fn feature_mask(
        &self,
        script: Option<ScriptTags>,
        lang: Option<Lang>,
        tag: &[u8; 4],
    ) -> u64 {
        let bit = feature_bit(tag);
        if bit != 0 && self.lookups.for_script(script, lang).any(|(_, m)| m & bit != 0) {
            bit
        } else {
            0
        }
    }

    /// Would the feature tagged `tag`, on a run of `script` in `lang`,
    /// substitute exactly the glyph sequence `glyphs`?
    ///
    /// This is a question, not an instruction: nothing is rewritten, and
    /// `glyphs` need not be — and for its callers never is — part of any run.
    /// The Indic shaper asks it before it lays a syllable out, because where
    /// the base consonant goes and where the others sit relative to it are
    /// facts about the *typeface*: whether this font draws this consonant
    /// under the base is something only this font can say. See
    /// [`would`](crate::would) for what each lookup type counts as an answer.
    ///
    /// `false` for a tag not in [`FEATURES`], since a feature this crate never
    /// asks a face for has no bit and so reaches no lookup. That is the right
    /// answer for the wrong reason, and it is why every tag the shaper probes
    /// must also be listed there.
    pub(crate) fn would_substitute(
        &self,
        data: &[u8],
        script: Option<ScriptTags>,
        lang: Option<Lang>,
        tag: &[u8; 4],
        glyphs: &[u16],
        zero_context: bool,
    ) -> bool {
        let bit = feature_bit(tag);
        if bit == 0 {
            return false;
        }
        self.lookups
            .for_script(script, lang)
            .filter(|&(_, mask)| mask & bit != 0)
            .any(|(lookup, _)| would_apply(data, lookup, glyphs, zero_context))
    }
}

/// Run one lookup over each syllable of `glyphs` in turn, never across a
/// boundary.
///
/// Each syllable is copied out, rewritten alone, and spliced back, which is
/// what makes the boundary real rather than advisory: a lookup handed only one
/// syllable cannot match beyond it however it is written, and neither can the
/// backtrack or lookahead of a chaining rule inside it. Restricting the
/// *starting* position instead would not do — a two-glyph ligature starting on
/// the syllable's last glyph would still swallow the next syllable's first.
///
/// The splice may change the syllable's length, so the walk tracks where the
/// next one now begins rather than trusting the original indices.
fn apply_per_syllable(
    data: &[u8],
    lookup: &Lookup,
    glyphs: &mut Vec<SubGlyph>,
    ctx: &mut Ctx,
    piece: &mut Vec<SubGlyph>,
) {
    let mut at = 0usize;
    while at < glyphs.len() {
        let Some(first) = glyphs.get(at) else { break };
        let syllable = first.syllable;
        let end = glyphs
            .iter()
            .enumerate()
            .skip(at)
            .find(|&(_, g)| g.syllable != syllable)
            .map_or(glyphs.len(), |(j, _)| j);
        piece.clear();
        piece.extend_from_slice(glyphs.get(at..end).unwrap_or_default());
        apply_lookup(data, lookup, piece, ctx);
        let len = piece.len();
        glyphs.splice(at..end, piece.iter().copied());
        // The floor of one is only reachable if a syllable rewrote itself to
        // nothing, which no lookup type can do; it is here so the walk
        // terminates without relying on that.
        at = at.saturating_add(len.max(1));
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
    mask: u64,
    /// How many ligature ids have been handed out so far in this run, for
    /// [`next_lig_id`](Ctx::next_lig_id).
    serial: u8,
}

impl Ctx {
    /// The next ligature id: cycles through 1..=7 and never returns `0`,
    /// because `0` is what [`Lig::id`] uses for "belongs to no ligature".
    ///
    /// HarfBuzz's `_hb_allocate_lig_id`, which is three bits of a per-buffer
    /// serial. The range is small on purpose: an id only has to tell apart
    /// the ligatures whose marks are still waiting to be placed, and a run
    /// with eight of those live at once does not occur.
    fn next_lig_id(&mut self) -> u8 {
        self.serial = self.serial.wrapping_add(1);
        if self.serial & 0x07 == 0 {
            self.serial = self.serial.wrapping_add(1);
        }
        self.serial & 0x07
    }
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
        LOOKUP_LIGATURE => apply_ligature(data, subs, glyphs, i, skip, ctx),
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
    // A one-glyph sequence is a replacement, not a decomposition: the run does
    // not grow, nothing was split, and HarfBuzz special-cases it so the glyph
    // is not marked as multiplied. Stamping it would tell a later ligature
    // that this glyph is worth no components of its own.
    let pieces = grown > 1;
    // Every glyph of the sequence inherits the source's cluster *and* its
    // feature mask: they all came from the one character, so they are all
    // eligible for exactly what it was. A `ccmp` that splits a letter into a
    // base and a mark must not leave the base ineligible for `fina`.
    //
    // The ligature bookkeeping is the exception, and it splits in two. The
    // component *numbering* is left alone for a glyph that already belongs to
    // a ligature — its pieces are still inside the component it was inside,
    // and overwriting it would strand them; a glyph that belongs to none gets
    // one piece number each, so a ligature swallowing the pieces later can
    // tell they were one thing. The record that a decomposition happened at
    // all is stamped either way, because that is a fact about every piece.
    glyphs.splice(
        i..=i,
        ctx.scratch.iter().enumerate().map(|(n, &gid)| SubGlyph {
            gid,
            lig: if pieces {
                if glyph.lig.id == 0 {
                    glyph.lig.piece(u8::try_from(n).unwrap_or(u8::MAX))
                } else {
                    Lig {
                        multiplied: true,
                        ..glyph.lig
                    }
                }
            } else {
                glyph.lig
            },
            ..glyph
        }),
    );
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
/// keeps its vowels after its letters ligate. Each of them is stamped with the
/// [`Lig`] of the component it followed, which is the only record that will
/// exist of where in the joined glyph it belongs: `GPOS`'s mark-to-ligature
/// attachment reads it back to choose an anchor.
fn apply_ligature(
    data: &[u8],
    subtables: &[usize],
    glyphs: &mut Vec<SubGlyph>,
    i: usize,
    skip: Skipper<'_>,
    ctx: &mut Ctx,
) -> Option<usize> {
    let mut at = [0usize; MAX_COMPONENTS];
    let (gid, count, total) = subtables
        .iter()
        .find_map(|&sub| ligature_at(data, sub, glyphs, i, skip, &mut at))?;
    let end = at.get(count.checked_sub(1)?).copied()?;
    // Before the removal, while the recorded positions still mean something.
    stamp_components(data, glyphs, &at, count, total, ctx);
    if let Some(first) = glyphs.get_mut(i) {
        // The cluster stays as it was: it is the first component's, and the
        // components that follow are being swallowed, not moved.
        first.gid = gid;
    }
    // Removed from the back so that the earlier indices stay valid. Component
    // zero is the glyph just rewritten and stays.
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

/// Record, on every glyph the match touched, where it sits in the ligature
/// about to be formed.
///
/// HarfBuzz's `ligate_input` without the buffer mechanics. Three cases, and
/// the distinctions between them are all HarfBuzz's, each with a font behind
/// it:
///
/// * **A base and some marks joining.** Treated as a base, not a ligature, so
///   that further marks can still attach to it. It is given no id.
/// * **Only marks joining.** A *mark ligature*: two vowel signs becoming one
///   glyph. It keeps whatever id it already had, so that it can still attach
///   to the base ligature its components were attached to — otherwise
///   `LAM,LAM,SHADDA,FATHA,HEH` loses the shadda-fatha's place on the
///   lam-lam-heh the moment the two marks join.
/// * **Anything else** is a real ligature: a fresh id, and every glyph
///   standing between two of its components is stamped with the component it
///   follows.
///
/// The last case has a tail. A component may itself be a ligature with marks
/// already assigned to *its* components, and those marks may stand after the
/// whole match — so the walk continues past the last component for as long as
/// the glyphs still belong to it, renumbering them into the new ligature.
fn stamp_components(
    data: &[u8],
    glyphs: &mut [SubGlyph],
    at: &[usize; MAX_COMPONENTS],
    count: usize,
    total: u8,
    ctx: &mut Ctx,
) {
    let defs = ctx.defs;
    let class_of = |glyphs: &[SubGlyph], pos: usize| {
        glyphs.get(pos).map_or(0, |g| defs.class(data, g.gid))
    };
    let Some(&first) = at.first() else { return };
    let rest_all_marks =
        (1..count).all(|k| at.get(k).is_some_and(|&p| class_of(glyphs, p) == CLASS_MARK));
    let first_class = class_of(glyphs, first);
    let mark_ligature = rest_all_marks && first_class == CLASS_MARK;
    let ligature = !(rest_all_marks && matches!(first_class, CLASS_BASE | CLASS_MARK));

    let id = if ligature { ctx.next_lig_id() } else { 0 };
    let Some(head) = glyphs.get_mut(first) else {
        return;
    };
    let mut last_id = head.lig.id;
    let mut last_components = head.lig.components();
    let mut so_far = last_components;
    if ligature {
        head.lig = Lig::ligature(id, total);
    }

    let mut from = first.saturating_add(1);
    for k in 1..count {
        let Some(&pos) = at.get(k) else { break };
        if ligature {
            for p in from..pos {
                renumber(glyphs, p, id, so_far, last_components);
            }
        }
        let Some(component) = glyphs.get(pos) else {
            break;
        };
        last_id = component.lig.id;
        last_components = component.lig.components_in_ligation();
        so_far = so_far.saturating_add(last_components);
        from = pos.saturating_add(1);
    }

    if mark_ligature || last_id == 0 {
        return;
    }
    for p in from..glyphs.len() {
        let Some(glyph) = glyphs.get(p) else { break };
        if glyph.lig.id != last_id || glyph.lig.comp() == 0 {
            break;
        }
        renumber(glyphs, p, id, so_far, last_components);
    }
}

/// Move one mark from the component it followed in its old ligature to the
/// component that same position has become in the new one.
///
/// The arithmetic is HarfBuzz's. `so_far - last` is how many components of the
/// new ligature were complete before the one this mark belongs to began; the
/// `min` is what keeps a mark that pointed past the end of its old ligature —
/// which a malformed font can arrange — inside the component it names.
fn renumber(glyphs: &mut [SubGlyph], at: usize, id: u8, so_far: u8, last: u8) {
    let Some(glyph) = glyphs.get_mut(at) else {
        return;
    };
    let this = match glyph.lig.comp() {
        // A glyph that belonged to no component of anything counts as a whole
        // one, so that it lands after everything the last component covered.
        0 => last,
        n => n,
    };
    let comp = so_far
        .saturating_sub(last)
        .saturating_add(this.min(last));
    glyph.lig = glyph.lig.mark(id, comp);
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
) -> Option<(u16, usize, u8)> {
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
///
/// Reports the ligature glyph, how many components matched, and how many
/// components the result is worth — which is not the same number, because a
/// component may itself be a ligature that already swallowed several.
///
/// Matching a glyph id is not on its own enough. A component that already
/// belongs to some *other* ligature must not be swallowed by this one: if
/// `LAM,LAM,HEH` has already joined, the `SHADDA,FATHA` it left adjacent
/// belong to different components of that ligature and joining them would
/// silently move one of them. See [`ligation_allowed`].
fn ligature_matches(
    data: &[u8],
    lig: usize,
    glyphs: &[SubGlyph],
    i: usize,
    skip: Skipper<'_>,
    at: &mut [usize; MAX_COMPONENTS],
) -> Option<(u16, usize, u8)> {
    let glyph = u16_at(data, lig)?;
    let components = usize::from(u16_at(data, lig.checked_add(2)?)?);
    if components < 2 || components > MAX_COMPONENTS {
        return None;
    }
    *at.get_mut(0)? = i;
    let head = glyphs.get(i)?.lig;
    let mut total = head.components();
    let mut ligbase: Option<bool> = None;
    let mut pos = i;
    for k in 1..components {
        let want = u16_at(
            data,
            lig.checked_add(4)?
                .checked_add(k.checked_sub(1)?.checked_mul(2)?)?,
        )?;
        pos = skip.next(glyphs, pos)?;
        let component = glyphs.get(pos)?;
        if component.gid != want {
            return None;
        }
        if !ligation_allowed(glyphs, i, head, component.lig, skip, &mut ligbase) {
            return None;
        }
        total = total.saturating_add(component.lig.components_in_ligation());
        *at.get_mut(k)? = pos;
    }
    Some((glyph, components, total))
}

/// May this component join the ligature the one at `first` is starting?
///
/// Two rules, both HarfBuzz's `match_input`:
///
/// * If the first component was itself part of an earlier ligature, every
///   later component must be part of the *same* component of it — otherwise
///   the marks of two different components are being joined, which moves one
///   of them. The exception, and the reason `ligbase` exists, is that the
///   earlier ligature's base glyph may be one this lookup's flag hides: a
///   lookup that cannot see the base cannot be said to be crossing it, so the
///   join is allowed.
/// * If the first component was *not* part of an earlier ligature, no later
///   component may be part of one either — unless it is part of the first
///   component itself.
///
/// `ligbase` caches the first rule's expensive half across the components of
/// one match: it is a backward scan, and the answer cannot change within a
/// match because nothing before `first` moves.
fn ligation_allowed(
    glyphs: &[SubGlyph],
    first: usize,
    head: Lig,
    component: Lig,
    skip: Skipper<'_>,
    ligbase: &mut Option<bool>,
) -> bool {
    if head.id != 0 && head.comp() != 0 {
        if head.id == component.id && head.comp() == component.comp() {
            return true;
        }
        return *ligbase.get_or_insert_with(|| base_is_hidden(glyphs, first, head.id, skip));
    }
    component.id == 0 || component.comp() == 0 || component.id == head.id
}

/// Is the base glyph of ligature `id` — the glyph its marks hang off, found by
/// walking back through the glyphs that belong to it — one this lookup's flag
/// hides?
fn base_is_hidden(glyphs: &[SubGlyph], from: usize, id: u8, skip: Skipper<'_>) -> bool {
    let mut j = from;
    while let Some(k) = j.checked_sub(1) {
        let Some(glyph) = glyphs.get(k) else { return false };
        if glyph.lig.id != id {
            return false;
        }
        if glyph.lig.comp() == 0 {
            return skip.skips(glyph.gid);
        }
        j = k;
    }
    false
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
    let mut positions: Vec<Option<usize>> = hit.positions().iter().map(|&p| Some(p)).collect();

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
    use crate::context::MAX_CONTEXT;
    use crate::fixture::*;

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
        subs.apply(data, None, None, &mut glyphs);
        glyphs.iter().map(|g| g.gid).collect()
    }

    /// The clusters `gids` come out with, one source character per glyph in.
    fn clusters(data: &[u8], subs: &Substitutions, gids: &[u16]) -> Vec<usize> {
        let mut glyphs: Vec<SubGlyph> = gids
            .iter()
            .enumerate()
            .map(|(i, &gid)| SubGlyph::new(gid, i))
            .collect();
        subs.apply(data, None, None, &mut glyphs);
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

    /// Run the lookups over `gids`, each glyph tagged with the syllable given
    /// alongside it, confined so that no rule may look across a boundary.
    fn per_syllable(data: &[u8], subs: &Substitutions, gids: &[(u16, u8)]) -> Vec<u16> {
        let mut glyphs: Vec<SubGlyph> = gids
            .iter()
            .enumerate()
            .map(|(i, &(gid, syllable))| SubGlyph {
                syllable,
                ..SubGlyph::new(gid, i)
            })
            .collect();
        subs.apply_stages(
            data,
            None,
            None,
            &[ALL_FEATURES],
            ALL_FEATURES,
            &mut glyphs,
            |_, _| {},
        );
        glyphs.iter().map(|g| g.gid).collect()
    }

    #[test]
    fn a_ligature_does_not_form_across_a_syllable_boundary() {
        let (data, subs) = fi_font();
        // The same two glyphs, the same lookup: one syllable ligates, two do
        // not. Nothing about the font differs between the two calls.
        assert_eq!(per_syllable(&data, &subs, &[(10, 0), (11, 0)]), [20]);
        assert_eq!(per_syllable(&data, &subs, &[(10, 0), (11, 1)]), [10, 11]);
    }

    #[test]
    fn every_syllable_is_shaped_and_not_just_the_first() {
        let (data, subs) = fi_font();
        assert_eq!(
            per_syllable(&data, &subs, &[(10, 0), (11, 0), (10, 1), (11, 1)]),
            [20, 20]
        );
    }

    #[test]
    fn a_syllable_that_shrinks_does_not_lose_the_next_one() {
        let (data, subs) = fi_font();
        // The first syllable's three glyphs become one, so the second no
        // longer starts where it did. Splicing by the rewritten length rather
        // than the original is what keeps `l` and the `fi` after it.
        assert_eq!(
            per_syllable(
                &data,
                &subs,
                &[(10, 0), (10, 0), (11, 0), (12, 1), (10, 2), (11, 2)]
            ),
            [21, 12, 20]
        );
    }

    #[test]
    fn a_syllable_of_one_glyph_is_still_offered_to_the_lookup() {
        let (data, subs) = fi_font();
        // Every glyph its own syllable: nothing can ligate, and the walk must
        // still reach the end rather than stalling on a syllable that came
        // back the length it went in.
        assert_eq!(
            per_syllable(&data, &subs, &[(10, 0), (11, 1), (10, 2), (11, 3)]),
            [10, 11, 10, 11]
        );
    }

    #[test]
    fn a_stage_naming_no_feature_rewrites_nothing() {
        let (data, subs) = fi_font();
        let mut glyphs: Vec<SubGlyph> = [10u16, 11]
            .iter()
            .enumerate()
            .map(|(i, &gid)| SubGlyph::new(gid, i))
            .collect();
        subs.apply_stages(&data, None, None, &[0], 0, &mut glyphs, |_, _| {});
        assert_eq!(glyphs.iter().map(|g| g.gid).collect::<Vec<_>>(), [10, 11]);
    }

    #[test]
    fn the_callback_runs_once_after_each_stage() {
        let (data, subs) = fi_font();
        let mut glyphs: Vec<SubGlyph> = [10u16, 11]
            .iter()
            .enumerate()
            .map(|(i, &gid)| SubGlyph::new(gid, i))
            .collect();
        let mut seen: Vec<(usize, usize)> = Vec::new();
        // Stage 0 selects nothing, so the ligature is still two glyphs when
        // the first callback runs and one by the second: the callback sees the
        // run as that stage left it, which is the whole point of it.
        subs.apply_stages(
            &data,
            None,
            None,
            &[0, ALL_FEATURES],
            0,
            &mut glyphs,
            |i, glyphs| seen.push((i, glyphs.len())),
        );
        assert_eq!(seen, [(0, 2), (1, 1)]);
    }

    /// Would `tag` substitute `gids`, under the strict rule that a chaining
    /// rule with any context does not answer?
    fn would(data: &[u8], subs: &Substitutions, tag: &[u8; 4], gids: &[u16]) -> bool {
        subs.would_substitute(data, None, None, tag, gids, true)
    }

    #[test]
    fn a_ligature_feature_answers_about_the_pair_it_would_form() {
        let (data, subs) = fi_font();
        assert!(would(&data, &subs, b"liga", &[10, 11]));
        assert!(would(&data, &subs, b"liga", &[10, 10]));
        assert!(would(&data, &subs, b"liga", &[10, 10, 11]));
        // A pair the font has no ligature for.
        assert!(!would(&data, &subs, b"liga", &[10, 12]));
        // The wrong way round: coverage is on the *first* glyph.
        assert!(!would(&data, &subs, b"liga", &[11, 10]));
    }

    #[test]
    fn a_sequence_the_rule_only_starts_is_not_an_answer() {
        let (data, subs) = fi_font();
        // The question is whether the feature substitutes *exactly* this
        // sequence. `f` alone begins three ligatures and is none of them, and
        // `f f i l` is one with a stray glyph after it — a shaper deciding
        // where a consonant sits must not read either as a yes.
        assert!(!would(&data, &subs, b"liga", &[10]));
        assert!(!would(&data, &subs, b"liga", &[10, 10, 11, 12]));
        assert!(!would(&data, &subs, b"liga", &[]));
    }

    #[test]
    fn a_tag_this_crate_never_asks_a_face_for_reaches_nothing() {
        let (data, subs) = fi_font();
        // Not in `FEATURES`, so it has no bit and no lookup carries it. Every
        // tag the Indic shaper probes has to be added there first.
        assert!(!would(&data, &subs, b"rphf", &[10, 11]));
    }

    #[test]
    fn a_single_substitution_answers_about_one_glyph_and_no_more() {
        let sub = single_delta(&[10, 11], 5);
        let data = gsub_table(b"liga", LOOKUP_SINGLE, &sub);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert!(would(&data, &subs, b"liga", &[10]));
        assert!(would(&data, &subs, b"liga", &[11]));
        assert!(!would(&data, &subs, b"liga", &[12]));
        // One glyph in, one glyph out: a two-glyph question is not one this
        // lookup type can ever answer yes to.
        assert!(!would(&data, &subs, b"liga", &[10, 11]));
    }

    #[test]
    fn a_decomposition_answers_like_a_replacement() {
        let sub = multiple(&[10], &[&[10, 11, 12]]);
        let data = gsub_table(b"liga", LOOKUP_MULTIPLE, &sub);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        // What it turns into is not the question; whether it touches the glyph
        // is. HarfBuzz answers the same way, and the callers only ever ask
        // about the input.
        assert!(would(&data, &subs, b"liga", &[10]));
        assert!(!would(&data, &subs, b"liga", &[11]));
    }

    #[test]
    fn a_context_with_no_context_answers_about_its_input() {
        let sub = context3(&[&[10], &[11]], &[(0, 0)]);
        let data = gsub_table(b"liga", LOOKUP_CONTEXT, &sub);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert!(would(&data, &subs, b"liga", &[10, 11]));
        assert!(!would(&data, &subs, b"liga", &[10, 12]));
        assert!(!would(&data, &subs, b"liga", &[10]));
    }

    #[test]
    fn a_chaining_rule_that_needs_a_neighbour_does_not_answer() {
        // `10 11` becomes something, but only after a `9`. Probed on its own
        // there is no `9` and never will be, so under the strict rule the
        // font is reported as saying nothing about the pair.
        let sub = chain_context3(&[&[9]], &[&[10], &[11]], &[], &[(0, 0)]);
        let data = gsub_table(b"liga", LOOKUP_CHAIN_CONTEXT, &sub);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert!(!subs.would_substitute(&data, None, None, b"liga", &[10, 11], true));
        // With the strict rule off — which is what the old Indic
        // specification and Malayalam need — the context is ignored rather
        // than failed, and the input alone decides.
        assert!(subs.would_substitute(&data, None, None, b"liga", &[10, 11], false));
        // Ignored, not matched: the input still has to be right.
        assert!(!subs.would_substitute(&data, None, None, b"liga", &[10, 12], false));
    }

    #[test]
    fn a_chaining_rule_with_no_neighbours_answers_either_way() {
        let sub = chain_context3(&[], &[&[10], &[11]], &[], &[(0, 0)]);
        let data = gsub_table(b"liga", LOOKUP_CHAIN_CONTEXT, &sub);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert!(subs.would_substitute(&data, None, None, b"liga", &[10, 11], true));
        assert!(subs.would_substitute(&data, None, None, b"liga", &[10, 11], false));
        assert!(!subs.would_substitute(&data, None, None, b"liga", &[10, 11, 12], true));
    }

    #[test]
    fn a_chaining_rule_set_answers_the_same_as_a_bare_one() {
        // Format 1: the same rule reached through a coverage-keyed rule set.
        let set = rule_set_of(&[chained(&[], &[11], &[], &[(0, 0)])]);
        let sub = context1(&[10], &[set]);
        let data = gsub_table(b"liga", LOOKUP_CHAIN_CONTEXT, &sub);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert!(would(&data, &subs, b"liga", &[10, 11]));
        assert!(!would(&data, &subs, b"liga", &[10, 12]));
        // The same rule with a lookahead stops answering.
        let set = rule_set_of(&[chained(&[], &[11], &[12], &[(0, 0)])]);
        let sub = context1(&[10], &[set]);
        let data = gsub_table(b"liga", LOOKUP_CHAIN_CONTEXT, &sub);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert!(!would(&data, &subs, b"liga", &[10, 11]));
    }

    #[test]
    fn a_feature_the_script_does_not_name_answers_no() {
        // Two scripts, each naming its own feature; the lookup is only
        // `latn`'s. A run of Arabic must not be told the font would ligate.
        let data = gsub_scripts(
            &[(b"arab", b"init"), (b"latn", b"liga")],
            LOOKUP_LIGATURE,
            &[&fi_subtable()],
        );
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        let latn = ScriptTags::exactly(*b"latn");
        let arab = ScriptTags::exactly(*b"arab");
        assert!(subs.would_substitute(&data, Some(latn), None, b"liga", &[10, 11], true));
        assert!(!subs.would_substitute(&data, Some(arab), None, b"liga", &[10, 11], true));
    }

    const IGNORE_MARKS: u16 = 0x0008;
    const USE_MARK_FILTERING_SET: u16 = 0x0010;

    /// `f`=10 plus `i`=11 becomes `fi`=20, and nothing else. The smallest
    /// lookup that can tell a skipped glyph from a matched one.
    fn fi_subtable() -> Vec<u8> {
        ligature_subst(&[10], &[ligature_set(&[ligature(20, &[11])])])
    }

    /// Run every lookup over `gids` and report where each glyph ended up
    /// inside a ligature: `(glyph, ligature id, component)`.
    fn ligature_props(data: &[u8], subs: &Substitutions, gids: &[u16]) -> Vec<(u16, u8, u8)> {
        let mut glyphs: Vec<SubGlyph> = gids
            .iter()
            .enumerate()
            .map(|(i, &gid)| SubGlyph::new(gid, i))
            .collect();
        subs.apply(data, None, None, &mut glyphs);
        glyphs
            .iter()
            .map(|g| (g.gid, g.lig.id, g.lig.comp()))
            .collect()
    }

    /// The same, reporting instead how many components each glyph is worth —
    /// which is what a further ligature counts it as.
    fn lig_components(data: &[u8], subs: &Substitutions, gids: &[u16]) -> Vec<u8> {
        let mut glyphs: Vec<SubGlyph> = gids
            .iter()
            .enumerate()
            .map(|(i, &gid)| SubGlyph::new(gid, i))
            .collect();
        subs.apply(data, None, None, &mut glyphs);
        glyphs.iter().map(|g| g.lig.components()).collect()
    }

    #[test]
    fn a_skipped_mark_records_the_component_it_followed() {
        // The bookkeeping `GPOS`'s mark-to-ligature attachment reads back. The
        // mark stood between the two components, so it belongs after the
        // first — and once the ligature has formed there is nothing else left
        // in the run that could say so.
        let gsub = gsub_flagged(b"liga", LOOKUP_LIGATURE, IGNORE_MARKS, 0, &fi_subtable());
        let (data, subs) = with_gdef(&gsub, &gdef(&class_def(90, &[3]), &[]));
        assert_eq!(
            ligature_props(&data, &subs, &[10, 90, 11]),
            [(20, 1, 0), (90, 1, 1)]
        );
    }

    #[test]
    fn each_mark_is_numbered_for_the_component_it_stood_after() {
        // Three components with a mark between each pair: the numbering has to
        // count components, not glyphs, or the second mark lands on the first
        // half of the ligature.
        let sub = ligature_subst(&[10], &[ligature_set(&[ligature(21, &[11, 12])])]);
        let gsub = gsub_flagged(b"liga", LOOKUP_LIGATURE, IGNORE_MARKS, 0, &sub);
        let (data, subs) = with_gdef(&gsub, &gdef(&class_def(90, &[3]), &[]));
        assert_eq!(
            ligature_props(&data, &subs, &[10, 90, 11, 90, 12]),
            [(21, 1, 0), (90, 1, 1), (90, 1, 2)]
        );
    }

    #[test]
    fn a_base_and_its_marks_joining_is_still_a_base() {
        // A ligature whose components are a base and nothing but marks is not
        // given an id, because it is not really a ligature: it is the base
        // with its marks drawn in, and further marks must still be able to
        // attach to it as a base. Giving it an id would send them looking for
        // a component instead.
        let sub = ligature_subst(&[90], &[ligature_set(&[ligature(92, &[91])])]);
        let gsub = gsub_flagged(b"liga", LOOKUP_LIGATURE, 0, 0, &sub);
        let (data, subs) = with_gdef(&gsub, &gdef(&class_def(90, &[1, 3]), &[]));
        assert_eq!(ligature_props(&data, &subs, &[90, 91]), [(92, 0, 0)]);
    }

    #[test]
    fn two_marks_joining_keep_the_ligature_they_were_attached_to() {
        // HarfBuzz's mark-ligature case. A shadda and a fatha over a lam-lam
        // ligature join into one glyph; if that join allocated a fresh id, the
        // joined mark would no longer belong to any component of the ligature
        // beneath it and would fall back to the last one. So a ligature of
        // nothing but marks keeps its first component's id.
        let joined = ligature_subst(&[90], &[ligature_set(&[ligature(94, &[93])])]);
        let marks = ligature_subst(&[91], &[ligature_set(&[ligature(95, &[92])])]);
        let gsub = gsub_lookups_flagged(
            &[
                (b"liga", LOOKUP_LIGATURE, joined),
                (b"rlig", LOOKUP_LIGATURE, marks),
            ],
            &[IGNORE_MARKS, 0],
        );
        let (data, subs) = with_gdef(&gsub, &gdef(&class_def(90, &[1, 3, 3, 1, 2, 3]), &[]));
        assert_eq!(
            ligature_props(&data, &subs, &[90, 91, 92, 93]),
            [(94, 1, 0), (95, 1, 1)]
        );
    }

    #[test]
    fn the_pieces_of_a_decomposition_count_as_one_component() {
        // `ccmp` splits 10 into 30 and 31, and a ligature then swallows both
        // along with an 11. The result stands for two characters, not three:
        // the pieces after the first are worth no component of their own,
        // because a mark that attaches to the decomposed character attaches to
        // its first piece. Counting them separately would number every later
        // mark one component too high.
        let sub = ligature_subst(&[30], &[ligature_set(&[ligature(40, &[31, 11])])]);
        let data = gsub_lookups(&[
            (b"ccmp", LOOKUP_MULTIPLE, multiple(&[10], &[&[30, 31]])),
            (b"liga", LOOKUP_LIGATURE, sub),
        ]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 11]), [40]);
        assert_eq!(lig_components(&data, &subs, &[10, 11]), [2]);
    }

    #[test]
    fn a_one_glyph_sequence_is_a_replacement_not_a_decomposition() {
        // The pieces of a decomposition are numbered so a later ligature can
        // tell they were one thing. A sequence of length one split nothing, so
        // numbering it would tell that ligature the glyph is worth no
        // component — and a two-component ligature would come out claiming
        // one.
        let sub = ligature_subst(&[30], &[ligature_set(&[ligature(40, &[11])])]);
        let data = gsub_lookups(&[
            (b"ccmp", LOOKUP_MULTIPLE, multiple(&[10], &[&[30]])),
            (b"liga", LOOKUP_LIGATURE, sub),
        ]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(lig_components(&data, &subs, &[10, 11]), [2]);
    }

    #[test]
    fn a_mark_already_inside_a_ligature_does_not_ligate_with_a_stranger() {
        // Once a mark belongs to component 1 of a ligature, a lookup may only
        // join it to glyphs that belong to the same component. Otherwise a
        // `rlig` could pull one mark out of the ligature it was placed on and
        // join it to a mark that was never there, stranding both.
        let joined = ligature_subst(&[90], &[ligature_set(&[ligature(94, &[93])])]);
        // 91 is inside the ligature; 96 stands after the whole thing.
        let marks = ligature_subst(&[91], &[ligature_set(&[ligature(95, &[96])])]);
        let gsub = gsub_lookups_flagged(
            &[
                (b"liga", LOOKUP_LIGATURE, joined),
                (b"rlig", LOOKUP_LIGATURE, marks),
            ],
            &[IGNORE_MARKS, 0],
        );
        let (data, subs) = with_gdef(&gsub, &gdef(&class_def(90, &[1, 3, 3, 1, 2, 3, 3]), &[]));
        assert_eq!(subst(&data, &subs, &[90, 91, 93, 96]), [94, 91, 96]);
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
        subs.apply(data, None, None, &mut glyphs);
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
            1u64 << i
        };
        assert_eq!(bit(b"isol"), ISOL);
        assert_eq!(bit(b"init"), INIT);
        assert_eq!(bit(b"medi"), MEDI);
        assert_eq!(bit(b"fina"), FINA);
        assert_eq!(bit(b"calt"), CALT);
        assert_eq!(bit(b"ljmo"), LJMO);
        assert_eq!(bit(b"vjmo"), VJMO);
        assert_eq!(bit(b"tjmo"), TJMO);
        // `calt` is one of the unconditional features, which is what makes
        // clearing it in `SubGlyph::jamo` meaningful: a bit that was not in
        // `ALWAYS` would already be clear and the removal a no-op.
        assert_eq!(ALWAYS & CALT, CALT);
        assert!(FEATURES.len() < u64::BITS as usize, "a feature past the 64th gets no bit");
        // `ALWAYS` is a prefix of the list: every feature before the first
        // positional one and none after it. Being a *prefix* is the property
        // that lets it be a literal — an unconditional feature added after a
        // positional one would make it discontiguous and this fail.
        let positional = ISOL | INIT | MEDI | FINA;
        assert_eq!(ALWAYS.trailing_ones(), ALWAYS.count_ones());
        assert_eq!(ALWAYS | positional, (1u64 << 18) - 1);
        assert_eq!(ALWAYS & positional, 0);
    }

    /// Every tag the Indic shaper names has a bit, since a tag missing from
    /// [`FEATURES`] would silently answer "this face has no such feature" —
    /// the right answer for the wrong reason, and invisible.
    #[test]
    fn the_indic_features_all_have_bits() {
        for tag in [
            b"nukt", b"akhn", b"rphf", b"rkrf", b"pref", b"blwf", b"abvf", b"half", b"pstf",
            b"vatu", b"cjct", b"init", b"pres", b"abvs", b"blws", b"psts", b"haln", b"locl",
            b"ccmp",
        ] {
            assert_ne!(feature_bit(tag), 0, "{:?}", core::str::from_utf8(tag));
        }
        assert_eq!(feature_bit(b"zzzz"), 0);
        // Distinct bits, or two features would turn each other on.
        let mut seen = 0u64;
        for tag in FEATURES {
            let bit = feature_bit(tag);
            assert_ne!(bit, 0, "{:?}", core::str::from_utf8(*tag));
            assert_eq!(seen & bit, 0, "{:?} repeats a bit", core::str::from_utf8(*tag));
            seen |= bit;
        }
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

    /// One language system, as a font writes it: the index of the feature it
    /// *requires* — `NO_REQUIRED` for none — and the features its index list
    /// names.
    type Sys<'a> = (u16, &'a [u16]);

    /// `requiredFeatureIndex`'s "there is no required feature" value.
    const NO_REQUIRED: u16 = 0xFFFF;

    /// One script: its tag, its DefaultLangSys if it has one, and the language
    /// systems it names beside the default.
    type Script<'a> = (&'a [u8; 4], Option<Sys<'a>>, &'a [(&'a [u8; 4], Sys<'a>)]);

    /// A ScriptList spelling out each script's DefaultLangSys *and* the named
    /// language systems beside it.
    ///
    /// [`fixture::script_list`](crate::fixture::script_list) can express
    /// neither a script without a default nor a script with a named language,
    /// and those two shapes are what the tests below are about. Every offset
    /// inside is relative to the ScriptList's own start, as the format
    /// requires.
    fn languages(scripts: &[Script<'_>]) -> Vec<u8> {
        fn body((required, features): Sys<'_>) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&be16(0)); // lookupOrder, always zero
            out.extend_from_slice(&be16(required));
            out.extend_from_slice(&be16(u16::try_from(features.len()).unwrap()));
            for f in features {
                out.extend_from_slice(&be16(*f));
            }
            out
        }
        let mut records = Vec::new();
        let mut tables = Vec::new();
        // The Script tables follow the ScriptRecords.
        let mut at = 2 + scripts.len() * 6;
        for (tag, default, langs) in scripts {
            records.extend_from_slice(*tag);
            records.extend_from_slice(&be16(u16::try_from(at).unwrap()));

            // One Script table: a header of the default's offset, the count
            // and the LangSysRecords, then every LangSys the script owns —
            // the default first, when it has one.
            let header = 4 + langs.len() * 6;
            let mut bodies = Vec::new();
            let mut lang_records = Vec::new();
            let mut off = header;
            let default_off = match default {
                Some(sys) => {
                    let block = body(*sys);
                    off += block.len();
                    bodies.extend_from_slice(&block);
                    u16::try_from(header).unwrap()
                }
                // Zero, not an offset: a script may have no default at all,
                // and that is not the same as one naming no features.
                None => 0,
            };
            for (lang, sys) in *langs {
                lang_records.extend_from_slice(*lang);
                lang_records.extend_from_slice(&be16(u16::try_from(off).unwrap()));
                let block = body(*sys);
                off += block.len();
                bodies.extend_from_slice(&block);
            }
            tables.extend_from_slice(&be16(default_off));
            tables.extend_from_slice(&be16(u16::try_from(langs.len()).unwrap()));
            tables.extend_from_slice(&lang_records);
            tables.extend_from_slice(&bodies);
            at += off;
        }
        let mut out = Vec::new();
        out.extend_from_slice(&be16(u16::try_from(scripts.len()).unwrap()));
        out.extend_from_slice(&records);
        out.extend_from_slice(&tables);
        out
    }

    /// Shape glyph 10 alone under `script` and `lang`, and report what it
    /// became.
    fn one(data: &[u8], subs: &Substitutions, script: Option<ScriptTags>, lang: &str) -> u16 {
        let lang = (!lang.is_empty())
            .then(|| Lang::new(lang).expect("the tests name real languages"));
        let mut glyphs = vec![SubGlyph::new(10, 0)];
        subs.apply(data, script, lang, &mut glyphs);
        glyphs.first().map_or(0, |g| g.gid)
    }

    /// A `locl` registered under `TRK ` and nowhere else — the Turkish dotless
    /// `i`, and the commonest per-language rule there is: 140 of the 996
    /// language overrides on the development host are this shape.
    ///
    /// Reaching it from a run that never asked for Turkish would hand its
    /// reader a spelling they did not ask for; *not* reaching it from a run
    /// that did ask is what `TD-FONT-IGNORES-LANGSYS-OVERRIDES` was.
    #[test]
    fn a_feature_only_a_language_system_reaches_applies_only_to_that_language() {
        let scripts = languages(&[(b"latn", None, &[(b"TRK ", (NO_REQUIRED, &[0]))])]);
        let sub = single_delta(&[10], 5);
        let data = gsub_from_scripts(&scripts, &[b"locl"], LOOKUP_SINGLE, &[&sub]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();

        assert_eq!(one(&data, &subs, LATIN, "tr"), 15);
        assert_eq!(one(&data, &subs, LATIN, "tr-TR"), 15);
        // This script has no default language system at all, so every other
        // run reaches nothing — including one that names no language, which is
        // what a caller who does not know says.
        for lang in ["", "en", "de", "az"] {
            assert_eq!(one(&data, &subs, LATIN, lang), 10, "{lang:?}");
        }
    }

    /// A LangSysRecord *replaces* its script's feature list rather than adding
    /// to it, which is why a language can take a feature away as well as add
    /// one.
    #[test]
    fn a_language_system_replaces_its_scripts_features_rather_than_adding_to_them() {
        let scripts = languages(&[(
            b"latn",
            Some((NO_REQUIRED, &[0])),
            &[(b"TRK ", (NO_REQUIRED, &[]))],
        )]);
        let sub = single_delta(&[10], 5);
        let data = gsub_from_scripts(&scripts, &[b"locl"], LOOKUP_SINGLE, &[&sub]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();

        assert_eq!(one(&data, &subs, LATIN, ""), 15);
        assert_eq!(one(&data, &subs, LATIN, "en"), 15);
        assert_eq!(one(&data, &subs, LATIN, "tr"), 10);
    }

    /// A language system names one feature *outside* its index list, and it is
    /// as binding as the ones inside: `requiredFeatureIndex` is how a font says
    /// "this one is not optional".
    #[test]
    fn a_required_feature_is_applied_like_any_other() {
        // Neither language lists a feature; the Turkish one requires it.
        let scripts = languages(&[(
            b"latn",
            Some((NO_REQUIRED, &[])),
            &[(b"TRK ", (0, &[]))],
        )]);
        let sub = single_delta(&[10], 5);
        let data = gsub_from_scripts(&scripts, &[b"locl"], LOOKUP_SINGLE, &[&sub]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(one(&data, &subs, LATIN, "tr"), 15);
        assert_eq!(one(&data, &subs, LATIN, ""), 10);

        // And the same field on a DefaultLangSys, which every run reads.
        let scripts = languages(&[(b"latn", Some((0, &[])), &[])]);
        let data = gsub_from_scripts(&scripts, &[b"locl"], LOOKUP_SINGLE, &[&sub]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(one(&data, &subs, LATIN, ""), 15);
    }

    /// Only the *script* falls back. A run whose script is registered but whose
    /// language is not takes that script's default rules — not the same
    /// language's rules under some other script.
    ///
    /// HarfBuzz's order, in `hb_ot_layout_table_select_script` followed by
    /// `hb_ot_layout_script_select_language`: the script is chosen first and
    /// the language is looked up only inside it.
    #[test]
    fn language_selection_does_not_fall_back_to_another_script() {
        // `DFLT` has the Turkish rule; `latn` is registered and says nothing.
        let scripts = languages(&[
            (
                b"DFLT",
                Some((NO_REQUIRED, &[])),
                &[(b"TRK ", (NO_REQUIRED, &[0]))],
            ),
            (b"latn", Some((NO_REQUIRED, &[])), &[]),
        ]);
        let sub = single_delta(&[10], 5);
        let data = gsub_from_scripts(&scripts, &[b"locl"], LOOKUP_SINGLE, &[&sub]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();

        // A Latin run stops the chain at `latn`, and `latn` has no Turkish.
        assert_eq!(one(&data, &subs, LATIN, "tr"), 10);
        // A run with no script of its own starts at `DFLT`, where it is.
        assert_eq!(one(&data, &subs, None, "tr"), 15);
        assert_eq!(one(&data, &subs, None, ""), 10);
    }

    /// A BCP 47 tag names *several* OpenType tags, tried in order, and a face
    /// that registers only a later one still answers.
    ///
    /// This is the bug the HarfBuzz sweep caught: `ro-MD` is `MOL ` and then
    /// `ROM `, and 66 of the development host's 556 faces file Romanian's
    /// comma-below `locl` under `ROM ` and register no `MOL ` at all. Stopping
    /// at the first candidate lost the feature on every one of them.
    #[test]
    fn a_language_reaches_a_face_that_registers_only_its_second_candidate() {
        let scripts = languages(&[(
            b"latn",
            Some((NO_REQUIRED, &[])),
            &[(b"ROM ", (NO_REQUIRED, &[0]))],
        )]);
        let sub = single_delta(&[10], 5);
        let data = gsub_from_scripts(&scripts, &[b"locl"], LOOKUP_SINGLE, &[&sub]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();

        assert_eq!(one(&data, &subs, LATIN, "ro"), 15);
        assert_eq!(one(&data, &subs, LATIN, "ro-MD"), 15);
        assert_eq!(one(&data, &subs, LATIN, ""), 10);
    }

    /// When a face registers more than one of a language's candidates, the
    /// *first* answers — even when what it selects is exactly the script's
    /// default.
    ///
    /// That last clause is the whole test. A named language whose features
    /// match its script's default is stored nowhere, because the default entry
    /// already holds the answer; if "which candidate wins" were decided by
    /// which one is stored, a `MOL ` that says nothing would silently hand
    /// Moldavian over to `ROM `'s overrides. The face registered `MOL ` and
    /// said it has no rules of its own, and that is an answer.
    #[test]
    fn the_first_candidate_a_face_registers_wins_even_when_it_selects_nothing() {
        let scripts = languages(&[(
            b"latn",
            Some((NO_REQUIRED, &[])),
            &[
                (b"MOL ", (NO_REQUIRED, &[])),
                (b"ROM ", (NO_REQUIRED, &[0])),
            ],
        )]);
        let sub = single_delta(&[10], 5);
        let data = gsub_from_scripts(&scripts, &[b"locl"], LOOKUP_SINGLE, &[&sub]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();

        // Romanian is `ROM ` alone, and reaches the rule.
        assert_eq!(one(&data, &subs, LATIN, "ro"), 15);
        // Moldavian stops at `MOL `, which this face registers and leaves bare.
        assert_eq!(one(&data, &subs, LATIN, "ro-MD"), 10);
        assert_eq!(one(&data, &subs, LATIN, ""), 10);
    }

    /// And the other way round, so the test above cannot pass by ignoring the
    /// language list altogether.
    #[test]
    fn the_first_candidate_a_face_registers_wins_when_it_selects_something() {
        let scripts = languages(&[(
            b"latn",
            Some((NO_REQUIRED, &[])),
            &[
                (b"MOL ", (NO_REQUIRED, &[0])),
                (b"ROM ", (NO_REQUIRED, &[])),
            ],
        )]);
        let sub = single_delta(&[10], 5);
        let data = gsub_from_scripts(&scripts, &[b"locl"], LOOKUP_SINGLE, &[&sub]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();

        assert_eq!(one(&data, &subs, LATIN, "ro-MD"), 15);
        assert_eq!(one(&data, &subs, LATIN, "ro"), 10);
        assert_eq!(one(&data, &subs, LATIN, ""), 10);
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
        subs.apply(&data, LATIN, None, &mut latin_run);
        assert_eq!(latin_run.first().map(|g| g.gid), Some(60));

        let mut arabic_run = vec![SubGlyph::new(10, 0)];
        subs.apply(&data, ARABIC, None, &mut arabic_run);
        assert_eq!(arabic_run.first().map(|g| g.gid), Some(50));
    }

    /// A face that registers neither the run's script, nor a default, nor
    /// `latn` has nothing to say about it, and saying nothing is the correct
    /// answer — the alternative is applying some other writing system's rules.
    ///
    /// `latn` is the chain's last resort, and HarfBuzz's: old fonts file
    /// everything under it whatever they are really for, so a face that
    /// registers it answers for every run that gets that far. `Gabriola.ttf`
    /// needs exactly that — its `GPOS` registers `cyrl`, `grek` and `latn` and
    /// no default at all, so `123 456` reaches its `kern` feature only here.
    #[test]
    fn a_script_the_face_does_not_register_falls_back_to_latin_and_then_nothing() {
        let hebrew = Some(ScriptTags {
            preferred: *b"hebr",
            fallback: *b"hebr",
        });

        // This face registers `latn`, so the chain ends there rather than
        // empty-handed.
        let (data, subs) = two_script_font();
        let mut glyphs = vec![SubGlyph::new(10, 0)];
        subs.apply(&data, hebrew, None, &mut glyphs);
        assert_eq!(glyphs.first().map(|g| g.gid), Some(60));
        // A run with no script of its own asks for `DFLT` first, which this
        // face does not register either, and lands in the same place.
        let mut none = vec![SubGlyph::new(10, 0)];
        subs.apply(&data, None, None, &mut none);
        assert_eq!(none.first().map(|g| g.gid), Some(60));

        // A face with no `latn` and no default really does say nothing.
        let data = gsub_scripts(&[(b"arab", b"liga")], LOOKUP_SINGLE, &[&single_list(
            &[10],
            &[50],
        )]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        let mut glyphs = vec![SubGlyph::new(10, 0)];
        subs.apply(&data, hebrew, None, &mut glyphs);
        assert_eq!(glyphs.first().map(|g| g.gid), Some(10));
        let mut none = vec![SubGlyph::new(10, 0)];
        subs.apply(&data, None, None, &mut none);
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
            subs.apply(&data, script, None, &mut glyphs);
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
        subs.apply(&out, LATIN, None, &mut glyphs);
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
    fn decomposing_a_ligature_records_that_it_was_broken_apart() {
        // The two halves of the bookkeeping come apart here, which is the
        // whole reason they are two fields. The component numbering survives —
        // the pieces are still inside the component the ligature was inside,
        // and renumbering them would strand any marks pointing at it — but
        // both pieces now also say a decomposition happened, and that is what
        // stops the Indic shaper treating either of them as a formed ligature.
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10], &[&[30, 31]]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        let mut glyphs = alloc::vec![SubGlyph {
            lig: Lig::at(3, 2, 0),
            ..SubGlyph::new(10, 0)
        }];
        subs.apply(&data, None, None, &mut glyphs);
        assert_eq!(glyphs.len(), 2);
        for g in &glyphs {
            assert_eq!(g.lig.id, 3);
            assert!(g.lig.ligated());
            assert!(g.lig.multiplied());
            assert!(!g.lig.ligated_and_didnt_multiply());
        }
    }

    #[test]
    fn replacing_a_ligature_with_one_glyph_is_not_a_decomposition() {
        // A one-glyph Sequence is a replacement: nothing was split, so nothing
        // should look as though it had been.
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10], &[&[30]]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        let mut glyphs = alloc::vec![SubGlyph {
            lig: Lig::at(3, 2, 0),
            ..SubGlyph::new(10, 0)
        }];
        subs.apply(&data, None, None, &mut glyphs);
        assert_eq!(glyphs.len(), 1);
        assert!(!glyphs[0].lig.multiplied());
        assert!(glyphs[0].lig.ligated_and_didnt_multiply());
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
