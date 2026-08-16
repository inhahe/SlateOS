//! The Indic shaping plan: what the script and the face say about a syllable.
//!
//! [`indic`](crate::indic) says what each *character* is. This module says what
//! the *script* and the *face* are, which is the other half of what reordering
//! needs, and it is where the two facts live that nothing else can supply:
//!
//! * **Per-script behaviour.** The nine Indic scripts do not reorder alike.
//!   Bengali draws the reph after the below-base forms and Oriya draws it right
//!   after the base; Telugu only forms a reph when the writer asks with a ZWJ;
//!   Kannada and Telugu apply the below-base feature to post-base consonants
//!   only. [`Config`] is that table, transcribed from HarfBuzz's
//!   `indic_configs[]`.
//! * **Per-face behaviour.** Where a consonant is *drawn* — under the base,
//!   after it, or as a base of its own — is not a property of the character.
//!   It is a property of the typeface, and the only way to learn it is to ask
//!   the face whether it has a below-base form for this consonant, then a
//!   post-base one, and so on. [`Plan::consonant_position`] asks, through
//!   [`Substitutions::would_substitute`].
//!
//! # Old spec and new spec
//!
//! OpenType renumbered the Indic script tags once: Devanagari features may be
//! filed under `deva` or under `dev2`, and the choice is not cosmetic. Under
//! the old tag the shaping engine classified reph and half-forms itself and the
//! font merely supplied the glyphs, in the order Uniscribe fed them —
//! consonant then virama. Under the new tag the font classifies, and the order
//! is virama then consonant.
//!
//! So [`Plan::old_spec`] is read off the tag the *face* registered, not off the
//! run: a face that files Devanagari under `deva` is asking for the older
//! behaviour whatever the text is. That is what
//! [`ByScript::chosen_script`](crate::otl::ByScript::chosen_script) exists to
//! answer.
//!
//! # Zero context
//!
//! Asking a face "would this feature substitute these glyphs" is a question
//! about glyphs that are in no run, so a chaining rule's backtrack and
//! lookahead can never be satisfied. `zero_context` decides what to do about
//! that: reject such a rule outright, or ignore its context and judge it on its
//! input alone. HarfBuzz rejects for the new spec and accepts for the old — and
//! for Malayalam under either, because testing showed Windows does. The comment
//! there reads "DON'T TOUCH OTHERWISE", and this is a transcription of it
//! rather than a derivation.

use alloc::vec::Vec;

use crate::bidi::{self, Class};
use crate::gsub::{ALL_FEATURES, Staging, SubGlyph, Substitutions, feature_bit, feature_bits};
use crate::indic::{Category, Position, Syllable, syllables};
use crate::lang::Lang;
use crate::script::ScriptTags;
use crate::syllabic;

/// One of the nine scripts this shaper shapes.
///
/// Nine and not more: the other Brahmi-derived scripts — Sinhala, Tibetan,
/// Javanese, Balinese — are shaped by the Universal Shaping Engine, and Khmer
/// and Myanmar have shapers of their own, because their reordering is not this
/// reordering. See [`indic`](crate::indic) for why their characters are not
/// even in the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Script {
    /// Devanagari — Hindi, Marathi, Nepali, Sanskrit.
    Devanagari,
    /// Bengali — Bengali, Assamese.
    Bengali,
    /// Gurmukhi — Punjabi.
    Gurmukhi,
    /// Gujarati.
    Gujarati,
    /// Oriya — Odia.
    Oriya,
    /// Tamil.
    Tamil,
    /// Telugu.
    Telugu,
    /// Kannada.
    Kannada,
    /// Malayalam.
    Malayalam,
}

impl Script {
    /// The Indic script `tags` names, or `None` for a run this shaper does not
    /// shape.
    ///
    /// Matched on the *preferred* tag because that is the one spelling per
    /// script that [`ScriptTags::of`] produces from a character, and the nine
    /// are distinct: the older spellings are not (`beng` and `bng2` share no
    /// prefix), so matching on either would be matching on two tables.
    #[must_use]
    pub(crate) fn of(tags: ScriptTags) -> Option<Self> {
        Some(match &tags.preferred {
            b"dev2" => Self::Devanagari,
            b"bng2" => Self::Bengali,
            b"gur2" => Self::Gurmukhi,
            b"gjr2" => Self::Gujarati,
            b"ory2" => Self::Oriya,
            b"tml2" => Self::Tamil,
            b"tel2" => Self::Telugu,
            b"knd2" => Self::Kannada,
            b"mlm2" => Self::Malayalam,
            _ => return None,
        })
    }

    /// The Indic script a run of `tags` is in, or `None` for a run this shaper
    /// does not shape — including one with no script at all.
    ///
    /// The `Option`-taking form of [`of`](Self::of), because that is the shape
    /// every caller has: a run's script is optional, since a run of digits and
    /// punctuation belongs to none.
    #[must_use]
    pub(crate) fn shaping(tags: Option<ScriptTags>) -> Option<Self> {
        tags.and_then(Self::of)
    }

    /// This script's configuration.
    #[must_use]
    fn config(self) -> Config {
        // Transcribed from HarfBuzz's `indic_configs[]`. The default row there
        // — reph before post, implicit, pre-and-post, and no virama — is not
        // reproduced: it exists for scripts routed to the Indic shaper that
        // are not in the table, and [`Script`] has no such variant.
        let (virama, reph_pos, reph_mode, blwf_mode) = match self {
            Self::Devanagari => (
                '\u{94d}',
                Position::BeforePost,
                RephMode::Implicit,
                BlwfMode::PreAndPost,
            ),
            Self::Bengali => (
                '\u{9cd}',
                Position::AfterSub,
                RephMode::Implicit,
                BlwfMode::PreAndPost,
            ),
            Self::Gurmukhi => (
                '\u{a4d}',
                Position::BeforeSub,
                RephMode::Implicit,
                BlwfMode::PreAndPost,
            ),
            Self::Gujarati => (
                '\u{acd}',
                Position::BeforePost,
                RephMode::Implicit,
                BlwfMode::PreAndPost,
            ),
            Self::Oriya => (
                '\u{b4d}',
                Position::AfterMain,
                RephMode::Implicit,
                BlwfMode::PreAndPost,
            ),
            Self::Tamil => (
                '\u{bcd}',
                Position::AfterPost,
                RephMode::Implicit,
                BlwfMode::PreAndPost,
            ),
            Self::Telugu => (
                '\u{c4d}',
                Position::AfterPost,
                RephMode::Explicit,
                BlwfMode::PostOnly,
            ),
            Self::Kannada => (
                '\u{ccd}',
                Position::AfterPost,
                RephMode::Implicit,
                BlwfMode::PostOnly,
            ),
            Self::Malayalam => (
                '\u{d4d}',
                Position::AfterMain,
                RephMode::LogRepha,
                BlwfMode::PreAndPost,
            ),
        };
        Config {
            virama,
            reph_pos,
            reph_mode,
            blwf_mode,
        }
    }
}

/// How a reph is recognised in the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RephMode {
    /// Out of an initial RA + virama. The ordinary case.
    Implicit,
    /// Out of an initial RA + virama + ZWJ only — the writer has to ask.
    /// Telugu, where a bare RA + virama is drawn as a stacked conjunct.
    Explicit,
    /// The reph is a character of its own, already in logical position, and
    /// only needs moving. Malayalam's U+0D4E.
    LogRepha,
}

/// Which consonants the below-base feature is offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BlwfMode {
    /// Both those before the base and those after it.
    PreAndPost,
    /// Only those after the base. Telugu and Kannada, where a pre-base
    /// consonant that took a below-base form would be drawn under the wrong
    /// letter.
    PostOnly,
}

/// What one script's reordering does differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Config {
    /// The script's virama — the character that kills a consonant's inherent
    /// vowel. Needed as a *glyph*, to ask the face about consonant forms:
    /// every such question is about a consonant next to a virama.
    virama: char,
    /// Where the reph ends up. One of [`Position::AfterMain`],
    /// [`Position::BeforeSub`], [`Position::AfterSub`],
    /// [`Position::BeforePost`] or [`Position::AfterPost`] — HarfBuzz's
    /// `reph_position_t` is literally a subset of its position enum, and
    /// keeping it as one is what lets final reordering compare it against a
    /// glyph's own position.
    reph_pos: Position,
    /// How a reph is recognised.
    reph_mode: RephMode,
    /// Which consonants get the below-base feature.
    blwf_mode: BlwfMode,
}

/// The face, as the Indic shaper interrogates it.
///
/// Bundled because every question is the same three arguments — the font's
/// bytes, its substitutions, and the script tags to file the question under —
/// and threading them through the reordering by hand would put them in every
/// signature it has.
#[derive(Clone, Copy)]
pub(crate) struct Probe<'a> {
    /// The font's bytes.
    data: &'a [u8],
    /// Its `GSUB`, or `None` in a face that has none — or has one this crate
    /// can apply nothing from, which comes to the same thing.
    ///
    /// Optional because reordering is not conditional on the font having
    /// substitutions: a left matra is written before its consonant and drawn
    /// before it, and moving it there is this shaper's job whether or not
    /// anything else happens to the syllable. A face with no `GSUB` simply
    /// answers "no" to every question the plan asks it, which is what the
    /// forwarding methods below return.
    subs: Option<&'a Substitutions>,
    /// The script to look features up under.
    tags: Option<ScriptTags>,
    /// The language to look them up under inside that script, or `None` for
    /// the script's default language system. See [`lang`](crate::lang).
    lang: Option<Lang>,
    /// Which script tag the face's `GSUB` ScriptList was chosen under, or
    /// `None` when it names none of the ones this run would accept.
    ///
    /// Passed in rather than derived from [`subs`](Self::subs), because it is a
    /// question about the ScriptList and `subs` records only the scripts that
    /// reached a lookup this crate can apply. See
    /// [`Face::gsub_chosen_script`](crate::sfnt::Face::gsub_chosen_script).
    chosen: Option<[u8; 4]>,
}

impl<'a> Probe<'a> {
    /// A probe over `subs`, asking about a run of `tags` in `lang` in a face
    /// whose `GSUB` was chosen under `chosen`.
    #[must_use]
    pub(crate) fn new(
        data: &'a [u8],
        subs: Option<&'a Substitutions>,
        tags: Option<ScriptTags>,
        lang: Option<Lang>,
        chosen: Option<[u8; 4]>,
    ) -> Self {
        Self {
            data,
            subs,
            tags,
            lang,
            chosen,
        }
    }
}

/// Everything the reordering needs beyond the characters themselves.
pub(crate) struct Plan<'a> {
    /// The face to ask.
    probe: Probe<'a>,
    /// Which script this is.
    pub(crate) script: Script,
    /// What that script does differently.
    config: Config,
    /// Whether the face asked for the pre-revision behaviour. See the module
    /// documentation.
    pub(crate) old_spec: bool,
    /// Whether a chaining rule with any context at all is rejected outright
    /// when the face is asked a hypothetical. See the module documentation.
    zero_context: bool,
    /// The glyph for [`Config::virama`], or `None` in a face that has none —
    /// in which case nothing can be asked about consonant forms, since every
    /// such question is about a consonant beside a virama.
    virama: Option<u16>,
    /// The seven features the reordering hands out per glyph, as the bits that
    /// select them — or `0` for a feature this face does not have.
    ///
    /// HarfBuzz's `mask_array`, kept for the same reason it keeps one: the
    /// answer is a property of the face and the script, so asking once per plan
    /// rather than once per syllable turns a scan of the face's lookups into a
    /// field read. That a missing feature reads `0` is load-bearing rather than
    /// incidental — "does this face form a reph at all?" is asked as
    /// `masks.rphf != 0`, and a zero mask also sets no bit on any glyph, so the
    /// one value is both the question and the right answer to it.
    masks: Masks,
}

/// The bits that select the seven per-glyph Indic features on one face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Masks {
    /// Reph form, from an initial RA + virama.
    rphf: u64,
    /// Pre-base form, the one that moves to the head of the syllable.
    pref: u64,
    /// Below-base form.
    blwf: u64,
    /// Above-base form.
    abvf: u64,
    /// Post-base form.
    pstf: u64,
    /// Half form: a consonant that lost its vowel and is drawn narrow.
    half: u64,
    /// Word-initial form, for a syllable a left matra begins.
    init: u64,
}

impl<'a> Plan<'a> {
    /// The plan for a run of `script` through `probe`.
    ///
    /// `glyph` maps a character to a glyph — the face's `cmap` — and is passed
    /// rather than taken from the probe because the probe carries `GSUB` only,
    /// and because it is called exactly once, for the virama.
    #[must_use]
    pub(crate) fn new(
        script: Script,
        probe: Probe<'a>,
        glyph: impl FnOnce(char) -> Option<u16>,
    ) -> Self {
        let config = script.config();
        // The *face's* tag decides, not the run's: see the module docs. The
        // new spelling is the one ending in `2`, so everything else — `deva`,
        // and the `DFLT`/`dflt`/`latn` a face with no Indic script at all
        // falls back to — is the old one. That last case is not a shrug: it
        // still decides how the reordering itself runs, and HarfBuzz reaches
        // the same answer by the same route, its script selection reporting
        // `DFLT` rather than nothing when it finds no script it asked for.
        let old_spec = probe.chosen.is_none_or(|tag| tag.get(3) != Some(&b'2'));
        let mask = |tag: &[u8; 4]| {
            probe
                .subs
                .map_or(0, |subs| subs.feature_mask(probe.tags, probe.lang, tag))
        };
        Self {
            probe,
            script,
            config,
            old_spec,
            zero_context: !old_spec && script != Script::Malayalam,
            virama: glyph(config.virama),
            masks: Masks {
                rphf: mask(b"rphf"),
                pref: mask(b"pref"),
                blwf: mask(b"blwf"),
                abvf: mask(b"abvf"),
                pstf: mask(b"pstf"),
                half: mask(b"half"),
                init: mask(b"init"),
            },
        }
    }

    /// Where the reph ends up.
    #[must_use]
    pub(crate) fn reph_pos(&self) -> Position {
        self.config.reph_pos
    }

    /// How a reph is recognised.
    #[must_use]
    pub(crate) fn reph_mode(&self) -> RephMode {
        self.config.reph_mode
    }

    /// Which consonants get the below-base feature.
    #[must_use]
    pub(crate) fn blwf_mode(&self) -> BlwfMode {
        self.config.blwf_mode
    }

    /// Would the feature `tag` substitute `glyphs`, on this run's script and
    /// language?
    #[must_use]
    pub(crate) fn would(&self, tag: &[u8; 4], glyphs: &[u16]) -> bool {
        self.probe.subs.is_some_and(|subs| {
            subs.would_substitute(
                self.probe.data,
                self.probe.tags,
                self.probe.lang,
                tag,
                glyphs,
                self.zero_context,
            )
        })
    }

    /// Where this face draws `consonant` when it stands beside a virama.
    ///
    /// The question the base-finding walk is really asking is "is this
    /// consonant drawn as a form hanging off some other letter, or is it a
    /// letter in its own right", and only the face can answer: the same
    /// consonant is a below-base form in one typeface and a post-base form in
    /// the next. So the face is asked, feature by feature, whether it has such
    /// a form — and the first feature that says yes names the position.
    ///
    /// [`Position::BaseC`] when none does, and when the face has no virama
    /// glyph to ask about: a consonant with no dependent form is a base.
    ///
    /// Both glyph orders are tried for each feature, which is not
    /// over-generosity but a transcription. The new spec puts the virama
    /// first and the old spec puts the consonant first, and some fonts — Free
    /// Sans is HarfBuzz's example — copied their old-spec lookups into the
    /// new-spec table unchanged. Uniscribe honours those lookups anyway, so
    /// text set in those fonts is only laid out correctly by a shaper that
    /// also does.
    #[must_use]
    pub(crate) fn consonant_position(&self, consonant: u16) -> Position {
        let Some(virama) = self.virama else {
            return Position::BaseC;
        };
        // The two orders are the two windows of this: `[virama, consonant]`
        // and `[consonant, virama]`.
        let both = [virama, consonant, virama];
        let either = |tag: &[u8; 4]| {
            both.get(..2).is_some_and(|pair| self.would(tag, pair))
                || both.get(1..).is_some_and(|pair| self.would(tag, pair))
        };
        // Order is HarfBuzz's and is load-bearing: a font may register the
        // same consonant under both `blwf` and `pstf`, and the first asked
        // wins. `vatu` is asked alongside `blwf` because the Vattu variants it
        // forms are below-base forms by another name.
        if either(b"blwf") || either(b"vatu") {
            Position::BelowC
        } else if either(b"pstf") || either(b"pref") {
            Position::PostC
        } else {
            Position::BaseC
        }
    }

    /// Re-ask the face about every consonant the table called a base.
    ///
    /// Runs over the whole run before any syllable is laid out, because the
    /// base-finding walk reads the answers and reads them across the syllable
    /// it is in. The table's [`Position::BaseC`] means only "this is a
    /// consonant"; what it is *in this font* — a below-base form, a post-base
    /// form, or a letter in its own right — is what
    /// [`consonant_position`](Self::consonant_position) settles here.
    pub(crate) fn update_consonant_positions(&self, glyphs: &mut [SubGlyph]) {
        // Every question is about a consonant beside a virama, so with no
        // virama every answer would be the base the position already is.
        if self.virama.is_none() {
            return;
        }
        for g in glyphs {
            if g.indic.position == Position::BaseC {
                g.indic.position = self.consonant_position(g.gid);
            }
        }
    }
}

/// The category a glyph still counts as.
///
/// [`Category::Other`] once it has ligated, because it is no longer the
/// character it was categorised from: `ka` and `ka + virama + ssa` are one
/// glyph after `cjct`, and calling that a consonant would have the base search
/// stop on a conjunct. HarfBuzz's `is_one_of` opens with the same rule.
///
/// The shaper reads the raw [`SubGlyph::indic`] field directly where HarfBuzz
/// does, which is most of the places a *halant* is looked for: those run before
/// anything has ligated, or want the halant back precisely because a ligature
/// swallowed it.
fn category(g: &SubGlyph) -> Category {
    if g.lig.ligated() {
        Category::Other
    } else {
        g.indic.category
    }
}

/// Whether this glyph may be a syllable's base, or stand where one would.
fn is_consonant(g: &SubGlyph) -> bool {
    category(g).is_base_candidate()
}

/// Whether this glyph is a ZWJ or a ZWNJ.
fn is_joiner(g: &SubGlyph) -> bool {
    matches!(category(g), Category::Joiner | Category::NonJoiner)
}

/// Give every glyph in `glyphs` the earliest cluster any of them has.
///
/// HarfBuzz's `merge_clusters`, and the reason reordering needs it: a cluster
/// is a byte offset into the source, and reordering moves glyphs past each
/// other, so after it the offsets no longer ascend. A caret placed by one of
/// them would jump backwards inside a word. Merging says what is actually true
/// of a reordered syllable — that it has no interior boundary a caret can
/// honestly point at — by giving the whole of it one offset.
fn merge(glyphs: &mut [SubGlyph]) {
    syllabic::merge_clusters(glyphs);
}

/// The longest syllable this lays out cluster-by-cluster rather than wholesale.
///
/// Past it the permutation no longer fits the byte that records it, and every
/// glyph after the base is merged into one cluster instead. HarfBuzz draws the
/// line in the same place and for the same reason. A syllable this long is not
/// a word in any Indic script; it is a pathological string, and a coarser caret
/// is the right thing to give up on it.
const MAX_TRACKED: usize = 127;

/// Lay out one syllable: HarfBuzz's `initial_reordering_syllable_indic`.
///
/// `glyphs` is exactly the syllable — the caller has already cut the run at the
/// boundaries [`indic::syllables`](crate::indic::syllables) found — and comes
/// out the same length, since initial reordering only permutes and annotates.
/// `order` is scratch the caller owns so that laying out a word does not
/// allocate once per syllable; its contents on entry are ignored.
///
/// A symbol cluster and a non-Indic one are left exactly as they are. Both of
/// the others are laid out as consonant syllables: an independent vowel and a
/// dotted circle are treated as consonants throughout, which is what lets one
/// piece of code serve all four.
pub(crate) fn initial_reordering_syllable(
    plan: &Plan,
    kind: Syllable,
    glyphs: &mut [SubGlyph],
    order: &mut Vec<u8>,
) {
    match kind {
        Syllable::Consonant | Syllable::Vowel | Syllable::Standalone | Syllable::Broken => {}
        Syllable::Symbol | Syllable::NonIndic => return,
    }
    let end = glyphs.len();
    if end == 0 {
        return;
    }

    // Ra,H,ZWJ must behave like Ra,ZWJ,H, for compatibility with how Kannada
    // was written before the joiner's meaning there was settled.
    // https://github.com/harfbuzz/harfbuzz/issues/435
    if plan.script == Script::Kannada
        && end >= 3
        && glyphs.first().map(category) == Some(Category::Ra)
        && glyphs.get(1).map(category) == Some(Category::Halant)
        && glyphs.get(2).map(category) == Some(Category::Joiner)
    {
        merge(glyphs.get_mut(1..3).unwrap_or_default());
        glyphs.swap(1, 2);
    }

    let (mut base, has_reph) = find_base(plan, glyphs);
    assign_positions(plan, glyphs, base, has_reph);
    base = sort_syllable(plan, glyphs, order);
    set_masks(plan, glyphs, base);
}

/// Step 1: which glyph is the base consonant, and does the syllable open with
/// a reph?
///
/// The rule, from the Devanagari spec: start at the end of the syllable and
/// move backwards until a consonant is found that has neither a below-base nor
/// a post-base form — post-base forms have to follow below-base ones, so a
/// post-base form is only disqualifying once one has been seen — or until the
/// first consonant is reached. That one is the base.
///
/// A pre-base-reordering RA needs no case of its own: this crate marks those
/// [`Position::PostC`], so the walk already steps over them.
fn find_base(plan: &Plan, glyphs: &[SubGlyph]) -> (usize, bool) {
    let end = glyphs.len();
    let mut base = end;
    let mut has_reph = false;
    // Where the backwards walk stops. It moves past an opening reph, since a
    // RA that became one is no longer a candidate for base.
    let mut limit = 0usize;

    let explicit = plan.reph_mode() == RephMode::Explicit;
    let opens_reph = plan.masks.rphf != 0
        && end >= 3
        && match plan.reph_mode() {
            // An implicit reph is RA,H and is inhibited by either joiner
            // after it: a ZWJ asks for the letter, a ZWNJ for the half form.
            RephMode::Implicit => glyphs.get(2).is_none_or(|g| !is_joiner(g)),
            // An explicit one is spelled with the ZWJ.
            RephMode::Explicit => glyphs.get(2).map(category) == Some(Category::Joiner),
            RephMode::LogRepha => false,
        };
    if opens_reph {
        let probe = [
            glyphs.first().map_or(0, |g| g.gid),
            glyphs.get(1).map_or(0, |g| g.gid),
            // The third glyph is part of the question only when the ZWJ is,
            // and HarfBuzz passes a glyph id of zero rather than a glyph when
            // it is not. Zero is `.notdef`, which no `rphf` covers.
            if explicit {
                glyphs.get(2).map_or(0, |g| g.gid)
            } else {
                0
            },
        ];
        let formed = probe.get(..2).is_some_and(|p| plan.would(b"rphf", p))
            || (explicit && plan.would(b"rphf", &probe));
        if formed {
            limit = 2;
            while limit < end && glyphs.get(limit).is_some_and(is_joiner) {
                limit = limit.saturating_add(1);
            }
            base = 0;
            has_reph = true;
        }
    } else if plan.reph_mode() == RephMode::LogRepha
        && glyphs.first().map(category) == Some(Category::Repha)
    {
        // Malayalam encodes the reph as its own character, so there is nothing
        // to ask the font — it is already a reph, and only needs moving.
        limit = 1;
        while limit < end && glyphs.get(limit).is_some_and(is_joiner) {
            limit = limit.saturating_add(1);
        }
        base = 0;
        has_reph = true;
    }

    let mut i = end;
    let mut seen_below = false;
    loop {
        i = i.saturating_sub(1);
        let Some(g) = glyphs.get(i) else { break };
        if is_consonant(g) {
            if g.indic.position != Position::BelowC
                && (g.indic.position != Position::PostC || seen_below)
            {
                base = i;
                break;
            }
            if g.indic.position == Position::BelowC {
                seen_below = true;
            }
            base = i;
        } else if i > 0
            && g.indic.category == Category::Joiner
            && glyphs.get(i.wrapping_sub(1)).map(|p| p.indic.category) == Some(Category::Halant)
        {
            // A ZWJ *after* a halant stops the search and asks for an explicit
            // half form. A ZWJ *before* one asks for a subjoined form instead,
            // so the search goes on — which is what makes the Bengali sequence
            // Ra,H,Ya subjoin the Ya into a Ya-Phalaa.
            break;
        }
        if i <= limit {
            break;
        }
    }

    // Only for an unforced reph: Ra,H,ZWJ asked for one explicitly and keeps
    // it. Otherwise a syllable with no other consonant has nothing for the
    // reph to sit over, so the RA is the base after all.
    if has_reph && base == 0 && limit <= 2 {
        has_reph = false;
    }
    (base, has_reph)
}

/// Steps 2 and 3: say where each glyph of the syllable goes.
///
/// The matra decomposition and the nukta/halant reordering the spec calls for
/// here have already happened — normalisation does both — so what is left is
/// to write the positions down and, for a face shaped by the old rules, to move
/// the halant the engine rather than the font is expected to place.
fn assign_positions(plan: &Plan, glyphs: &mut [SubGlyph], base: usize, has_reph: bool) {
    let end = glyphs.len();
    for g in glyphs.get_mut(..base).unwrap_or_default() {
        g.indic.position = g.indic.position.min(Position::PreC);
    }
    if let Some(g) = glyphs.get_mut(base) {
        g.indic.position = Position::BaseC;
    }
    if has_reph && let Some(g) = glyphs.first_mut() {
        g.indic.position = Position::RaToBecomeReph;
    }

    if plan.old_spec {
        move_old_spec_halant(plan, glyphs, base);
    }

    // Attach the marks that have no position of their own to whatever came
    // before them, so that they travel with it through the sort.
    let mut last_pos = Position::Start;
    for i in 0..end {
        let Some(&g) = glyphs.get(i) else { break };
        let travels = matches!(
            g.indic.category,
            Category::Joiner
                | Category::NonJoiner
                | Category::Nukta
                | Category::Cantillation
                | Category::ConsonantMedial
                | Category::Halant
        );
        if travels {
            let mut pos = last_pos;
            if g.indic.category == Category::Halant && pos == Position::PreM {
                // Uniscribe does not move a halant with a left matra, so
                // neither does this. TEST: U+092B,U+093F,U+094D.
                for j in (0..i).rev() {
                    let before = glyphs.get(j).map_or(Position::Start, |p| p.indic.position);
                    if before != Position::PreM {
                        pos = before;
                        break;
                    }
                }
            }
            if let Some(g) = glyphs.get_mut(i) {
                g.indic.position = pos;
            }
        } else if g.indic.position != Position::Smvd {
            // A syllable modifier followed by an always-post matra belongs
            // where the matra does, not where its own table entry says.
            if g.indic.category == Category::MatraPost
                && glyphs
                    .get(i.wrapping_sub(1))
                    .is_some_and(|p| p.indic.category == Category::SyllableModifier)
                && let Some(p) = glyphs.get_mut(i.wrapping_sub(1))
            {
                p.indic.position = g.indic.position;
            }
            last_pos = g.indic.position;
        }
    }

    // A post-base consonant owns everything between it and the last consonant
    // or matra, so that a below-base form takes its own halant down with it.
    let mut last = base;
    for i in base.saturating_add(1)..end {
        let Some(&g) = glyphs.get(i) else { break };
        if is_consonant(&g) {
            let pos = g.indic.position;
            for owned in glyphs
                .get_mut(last.saturating_add(1)..i)
                .unwrap_or_default()
            {
                if owned.indic.position < Position::Smvd {
                    owned.indic.position = pos;
                }
            }
            last = i;
        } else if matches!(g.indic.category, Category::Matra | Category::MatraPost) {
            last = i;
        }
    }
}

/// Move the first post-base halant to after the last consonant, for a face
/// shaped by the pre-revision rules.
///
/// Reports suggest Uniscribe does this in Kannada only when there is not
/// already a halant after the last consonant, and unconditionally elsewhere —
/// Malayalam, Bengali and Devanagari are all known to reorder regardless — so
/// Kannada is the one script this holds back on. Test cases, each with the font
/// that showed it: U+0C9A,U+0CCD,U+0C9A,U+0CCD with Lohit Kannada;
/// U+0D38,U+0D4D,U+0D31,U+0D4D,U+0D31,U+0D4D with Lohit Malayalam;
/// U+0998,U+09CD,U+09AF,U+09CD with Vrinda; U+091F,U+094D,U+0930,U+094D with
/// Chandas.
fn move_old_spec_halant(plan: &Plan, glyphs: &mut [SubGlyph], base: usize) {
    let end = glyphs.len();
    let disallow_double_halants = plan.script == Script::Kannada;
    for i in base.saturating_add(1)..end {
        if glyphs.get(i).map(|g| g.indic.category) != Some(Category::Halant) {
            continue;
        }
        let mut j = end.saturating_sub(1);
        while j > i {
            let stop = glyphs.get(j).is_some_and(|g| {
                is_consonant(g) || (disallow_double_halants && g.indic.category == Category::Halant)
            });
            if stop {
                break;
            }
            j = j.saturating_sub(1);
        }
        if j > i && glyphs.get(j).map(|g| g.indic.category) != Some(Category::Halant) {
            // Rotating is the move: everything from `i+1` through `j` shifts
            // down one and the halant lands on `j`.
            if let Some(run) = glyphs.get_mut(i..=j) {
                run.rotate_left(1);
            }
        }
        break;
    }
}

/// Sort the syllable into its laid-out order and report where the base landed.
///
/// The sort is by [`Position`] and is stable, which between them *are* the
/// reordering: the variants are declared in the order they are drawn in, and
/// stability is what keeps two glyphs the table gave the same position to in
/// the order they were typed.
fn sort_syllable(plan: &Plan, glyphs: &mut [SubGlyph], order: &mut Vec<u8>) -> usize {
    let end = glyphs.len();
    order.clear();
    order.extend((0..end).map(|i| u8::try_from(i).unwrap_or(u8::MAX)));
    // Insertion sort: stable, in place, and it carries `order` along so that
    // the permutation is still known afterwards — which is what the cluster
    // merging below needs and what a call to `sort_by` would throw away. A
    // syllable is a handful of glyphs, so the quadratic term never bites.
    for i in 1..end {
        let mut j = i;
        while j > 0
            && glyphs.get(j.wrapping_sub(1)).map(|g| g.indic.position)
                > glyphs.get(j).map(|g| g.indic.position)
        {
            glyphs.swap(j.wrapping_sub(1), j);
            order.swap(j.wrapping_sub(1), j);
            j = j.wrapping_sub(1);
        }
    }

    // Find the base again — the sort moved it — and, on the way, the run of
    // left matras that now sits at the head.
    let mut base = end;
    let mut first_left = end;
    let mut last_left = end;
    for (i, g) in glyphs.iter().enumerate() {
        if g.indic.position == Position::BaseC {
            base = i;
            break;
        } else if g.indic.position == Position::PreM {
            if first_left == end {
                first_left = i;
            }
            last_left = i;
        }
    }
    // Several left matras come out in the order they were typed, but they are
    // drawn leftwards from the base, so the sequence reads backwards.
    // Reversing it and then reversing each matra's own marks back is what puts
    // them right. https://github.com/harfbuzz/harfbuzz/issues/3863
    if first_left < last_left {
        reverse(glyphs, order, first_left, last_left);
        let mut i = first_left;
        for j in first_left..=last_left {
            if glyphs
                .get(j)
                .is_some_and(|g| matches!(g.indic.category, Category::Matra | Category::MatraPost))
            {
                reverse(glyphs, order, i, j);
                i = j.saturating_add(1);
            }
        }
    }

    merge_reordered(plan, glyphs, order, base);
    base
}

/// Reverse `glyphs[from..=to]`, keeping `order` in step.
fn reverse(glyphs: &mut [SubGlyph], order: &mut [u8], from: usize, to: usize) {
    if let Some(run) = glyphs.get_mut(from..=to) {
        run.reverse();
    }
    if let Some(run) = order.get_mut(from..=to) {
        run.reverse();
    }
}

/// Merge the clusters of everything the sort actually moved, from the base on.
///
/// Only from the base on. Things before it move again in final reordering —
/// a left matra is brought back towards the base — so merging them now would
/// join a cluster to one it is about to leave. Final reordering merges up to
/// the base for the same reason, and the two interlock.
/// https://github.com/harfbuzz/harfbuzz/issues/2272
fn merge_reordered(plan: &Plan, glyphs: &mut [SubGlyph], order: &mut [u8], base: usize) {
    let end = glyphs.len();
    // In old-spec mode halants were moved around above, so nothing after the
    // base can be trusted to have stayed put; and past `MAX_TRACKED` the
    // permutation no longer fits `order`. Either way, merge it all.
    if plan.old_spec || end > MAX_TRACKED {
        merge(glyphs.get_mut(base..).unwrap_or_default());
        return;
    }
    // Otherwise merge each cycle of the permutation: a glyph that ended up
    // where another began is a glyph that moved past it, and the span between
    // them is what can no longer be pointed into.
    const DONE: u8 = u8::MAX;
    for i in base..end {
        if order.get(i).copied() == Some(DONE) {
            continue;
        }
        let (mut lo, mut hi) = (i, i);
        let mut j = order.get(i).map_or(i, |&o| usize::from(o));
        while j != i {
            lo = lo.min(j);
            hi = hi.max(j);
            let next = order.get(j).map_or(i, |&o| usize::from(o));
            if let Some(slot) = order.get_mut(j) {
                *slot = DONE;
            }
            j = next;
        }
        merge(
            glyphs
                .get_mut(base.max(lo)..=hi.max(base.max(lo)))
                .unwrap_or_default(),
        );
    }
}

/// Say which of the per-glyph features each glyph of the laid-out syllable is
/// eligible for.
///
/// This is the point of everything above: a half form is asked for by setting
/// `half` on the glyphs before the base and nowhere else, so the font's own
/// `half` lookups match exactly where the layout says a half form belongs.
fn set_masks(plan: &Plan, glyphs: &mut [SubGlyph], base: usize) {
    let end = glyphs.len();
    for g in glyphs.iter_mut() {
        if g.indic.position != Position::RaToBecomeReph {
            break;
        }
        g.mask |= plan.masks.rphf;
    }

    // Before the base: half forms, and — under the new rules, in the scripts
    // that allow it — below-base forms too, since a vattu may sit under a half
    // form as well as under the base.
    let mut pre = plan.masks.half;
    if !plan.old_spec && plan.blwf_mode() == BlwfMode::PreAndPost {
        pre |= plan.masks.blwf;
    }
    for g in glyphs.get_mut(..base).unwrap_or_default() {
        g.mask |= pre;
    }
    // After it: the three dependent forms.
    let post = plan.masks.blwf | plan.masks.abvf | plan.masks.pstf;
    for g in glyphs.get_mut(base.saturating_add(1)..).unwrap_or_default() {
        g.mask |= post;
    }

    if plan.old_spec && plan.script == Script::Devanagari {
        // The old spec: "The feature 'below-base form' is applied to consonants
        // having below-base forms and following the base consonant. The
        // exception is vattu, which may appear below half forms as well as
        // below the base glyph. The feature 'below-base form' will be applied
        // to all such occurrences of Ra as well."
        //
        // TEST: U+0924,U+094D,U+0930,U+094D,U+0915 with Sanskrit 2003.
        //
        // Ra,Halant,ZWJ is how the eyelash form is asked for, though, so that
        // sequence is left alone. TEST: the same with a U+200D before the
        // U+0915.
        for i in 0..base.saturating_sub(1) {
            let eyelash = glyphs.get(i).map(|g| g.indic.category) == Some(Category::Ra)
                && glyphs.get(i.saturating_add(1)).map(|g| g.indic.category)
                    == Some(Category::Halant)
                && (i.saturating_add(2) == base
                    || glyphs.get(i.saturating_add(2)).map(|g| g.indic.category)
                        != Some(Category::Joiner));
            if eyelash {
                for g in glyphs.get_mut(i..=i.saturating_add(1)).unwrap_or_default() {
                    g.mask |= plan.masks.blwf;
                }
            }
        }
    }

    // A Halant,Ra after the base is the pre-base-reordering sequence, and only
    // the font can say whether this one is: the pair is offered to `pref` and
    // marked if it would take it. The first such pair wins.
    if plan.masks.pref != 0 {
        for i in base.saturating_add(1)..end.saturating_sub(1) {
            let pair = [
                glyphs.get(i).map_or(0, |g| g.gid),
                glyphs.get(i.saturating_add(1)).map_or(0, |g| g.gid),
            ];
            if plan.would(b"pref", &pair) {
                for g in glyphs.get_mut(i..=i.saturating_add(1)).unwrap_or_default() {
                    g.mask |= plan.masks.pref;
                }
                break;
            }
        }
    }

    // Both joiners disable `cjct` simply by being there — the feature does not
    // skip them, so their presence breaks the sequence it would have matched.
    // A ZWNJ additionally disables `half`, back to the consonant it follows.
    for i in 1..end {
        if !glyphs.get(i).is_some_and(is_joiner) {
            continue;
        }
        let non_joiner = glyphs.get(i).map(category) == Some(Category::NonJoiner);
        let mut j = i;
        loop {
            j = j.wrapping_sub(1);
            if non_joiner && let Some(g) = glyphs.get_mut(j) {
                g.mask &= !plan.masks.half;
            }
            if j == 0 || glyphs.get(j).is_some_and(is_consonant) {
                break;
            }
        }
    }
}

/// Whether this glyph is still a virama.
fn is_halant(g: &SubGlyph) -> bool {
    category(g) == Category::Halant
}

/// The position of `glyphs[i]`, or [`Position::End`] past the end.
fn pos_at(glyphs: &[SubGlyph], i: usize) -> Position {
    glyphs.get(i).map_or(Position::End, |g| g.indic.position)
}

/// The category `glyphs[i]` still counts as, or [`Category::Other`] past the
/// end — which is what a glyph that has ligated counts as too, so the two
/// answers need not be told apart.
fn cat_at(glyphs: &[SubGlyph], i: usize) -> Category {
    glyphs.get(i).map_or(Category::Other, category)
}

/// Give `glyphs[from..to]` one cluster. HarfBuzz's `merge_clusters`, whose
/// second argument is likewise one past the last glyph merged.
fn merge_range(glyphs: &mut [SubGlyph], from: usize, to: usize) {
    merge(glyphs.get_mut(from..to).unwrap_or_default());
}

/// Put one laid-out syllable in its final order: HarfBuzz's
/// `final_reordering_syllable_indic`.
///
/// Runs after the eleven basic features, and that is the whole reason it
/// exists. Initial reordering had to guess — it put a left matra at the head of
/// the syllable and a reph at the head too, because it could not yet know what
/// the font would do. Now it can see: the half forms either formed or did not,
/// the `Ra,Halant` either ligated into a reph or did not, `pref` either
/// produced a pre-base form or did not. This walks the results back and moves
/// each of those three things to where the outcome says it belongs.
///
/// `word_start` says whether the syllable begins a word — whether there is
/// anything before it that is a letter, a mark or a format character. Only the
/// `init` feature reads it, and the caller has to answer because the run no
/// longer remembers: HarfBuzz asks the *preceding character's* Unicode general
/// category, which is a fact about the text and not about any glyph here.
///
/// Unlike [`initial_reordering_syllable`] this takes no syllable kind. HarfBuzz
/// runs it over every syllable including the non-Indic ones, and it is a no-op
/// on them for free: every glyph in one sits at [`Position::End`] with no
/// feature mask, so the base lands on the first glyph, nothing is before it,
/// and each of the three moves is skipped for want of anything to move.
pub(crate) fn final_reordering_syllable(plan: &Plan, glyphs: &mut [SubGlyph], word_start: bool) {
    let end = glyphs.len();
    if end == 0 {
        return;
    }

    recover_viramas(plan, glyphs);

    // Whether a pre-base form is still in play. The base search clears it on
    // finding a `pref` candidate the font declined, since after that there is
    // no pre-base form to go looking for.
    let mut try_pref = plan.masks.pref != 0;
    let mut base = find_base_again(plan, glyphs, &mut try_pref);
    reorder_matras(plan, glyphs, &mut base);
    reorder_reph(plan, glyphs, &mut base);
    reorder_pre_base(plan, glyphs, &mut base, try_pref);

    // A left matra that starts a word asks for the word-initial form. Where it
    // does not start one HarfBuzz marks the join unsafe to break instead, which
    // needs a facility we do not have; the mask is simply not set.
    if word_start && pos_at(glyphs, 0) == Position::PreM {
        if let Some(g) = glyphs.first_mut() {
            g.mask |= plan.masks.init;
        }
    }
}

/// Give back the category of a virama that a ligature swallowed and a later
/// decomposition spat out again.
///
/// Everything below leans on finding halants, and by now a great deal of
/// ligating has happened; a glyph that went into a conjunct and came back out
/// of a `ccmp` no longer looks like anything. When the glyph is *the* virama
/// glyph and its history is exactly "ligated, then split", the intent is not in
/// doubt, so the category is restored — and the ligature record cleared with
/// it, because every test for a halant refuses a glyph that ligated.
fn recover_viramas(plan: &Plan, glyphs: &mut [SubGlyph]) {
    let Some(virama) = plan.virama else {
        return;
    };
    for g in glyphs {
        if g.gid == virama && g.lig.ligated() && g.lig.multiplied() {
            g.indic.category = Category::Halant;
            g.lig.clear_ligated_and_multiplied();
        }
    }
}

/// Find the base again, now that substitution has had its say.
///
/// The sort in initial reordering moved it, and worse, the font may have
/// disagreed with where it was: a consonant marked for `pref` that the font
/// declined to give a pre-base form is not a pre-base consonant at all, and the
/// base is around there instead. Returns the index, and clears `try_pref` when
/// it finds that declined candidate — after which there is no pre-base form
/// left to reorder.
fn find_base_again(plan: &Plan, glyphs: &mut [SubGlyph], try_pref: &mut bool) -> usize {
    let end = glyphs.len();
    let mut base = end;
    'find: for b in 0..end {
        if pos_at(glyphs, b) < Position::BaseC {
            continue;
        }
        base = b;

        if *try_pref && base.saturating_add(1) < end {
            for i in base.saturating_add(1)..end {
                if glyphs.get(i).is_none_or(|g| g.mask & plan.masks.pref == 0) {
                    continue;
                }
                // Only the first candidate is judged, whichever way it goes.
                if !glyphs
                    .get(i)
                    .is_some_and(|g| g.lig.ligated_and_didnt_multiply())
                {
                    base = i;
                    while base < end && glyphs.get(base).is_some_and(is_halant) {
                        base = base.saturating_add(1);
                    }
                    if let Some(g) = glyphs.get_mut(base) {
                        g.indic.position = Position::BaseC;
                    }
                    *try_pref = false;
                }
                break;
            }
            if base == end {
                break 'find;
            }
        }

        // Malayalam: step over below-base forms the font never formed — but not
        // over post-base ones, which are drawn where they are whether or not a
        // dedicated glyph exists.
        if plan.script == Script::Malayalam {
            let mut i = base.saturating_add(1);
            while i < end {
                while i < end && glyphs.get(i).is_some_and(is_joiner) {
                    i = i.saturating_add(1);
                }
                if i == end || !glyphs.get(i).is_some_and(is_halant) {
                    break;
                }
                i = i.saturating_add(1);
                while i < end && glyphs.get(i).is_some_and(is_joiner) {
                    i = i.saturating_add(1);
                }
                if glyphs.get(i).is_some_and(is_consonant) && pos_at(glyphs, i) == Position::BelowC
                {
                    base = i;
                    if let Some(g) = glyphs.get_mut(base) {
                        g.indic.position = Position::BaseC;
                    }
                }
                i = i.saturating_add(1);
            }
        }

        // The first glyph positioned at or after the base is not necessarily
        // the base: if it is positioned strictly after one, the base is the
        // glyph before it.
        if base > 0 && pos_at(glyphs, base) > Position::BaseC {
            base = base.saturating_sub(1);
        }
        break 'find;
    }

    // No glyph claims to be at or after the base. A trailing ZWJ is not it.
    if base == end && end > 0 && cat_at(glyphs, end.saturating_sub(1)) == Category::Joiner {
        base = base.saturating_sub(1);
    }
    // Neither a nukta nor a halant can be a base; back up over any.
    if base < end {
        while base > 0 && matches!(cat_at(glyphs, base), Category::Nukta | Category::Halant) {
            base = base.saturating_sub(1);
        }
    }
    base
}

/// Bring the left matras back towards the base.
///
/// Initial reordering parked them at the head of the syllable because it could
/// not know how wide what follows would turn out to be. The spec puts them
/// "after the last standalone halant glyph, after the initial matra position
/// and before the main consonant" — which is to say, after whatever half forms
/// the font actually made, since a half form ends in a halant that ligated away
/// and a consonant that did *not* get one still has its halant sitting there.
///
/// The joiner rule is Uniscribe's rather than the spec's, and the spec is
/// simply wrong about it: a ZWJ after the halant means the matra does **not**
/// move there and the search continues; a ZWNJ means it does, which the
/// syllable machine has already arranged by ending the syllable at the ZWNJ.
/// TEST: `U+091F,U+094D,U+200C,U+092F,U+093F` moves,
/// `U+091F,U+094D,U+200D,U+092F,U+093F` does not.
/// <https://github.com/harfbuzz/harfbuzz/issues/1070>
fn reorder_matras(plan: &Plan, glyphs: &mut [SubGlyph], base: &mut usize) {
    let end = glyphs.len();
    // Otherwise there can be no pre-base matra to move.
    if end < 2 || *base == 0 {
        return;
    }

    // Having lost the base, position before the last thing instead.
    let mut new_pos = if *base == end {
        base.saturating_sub(2)
    } else {
        base.saturating_sub(1)
    };

    // Malayalam and Tamil have neither half forms nor explicit virama forms —
    // what their `half` makes is a chillu or a ligated virama, and the matra
    // goes after those, which is where it already is.
    if !matches!(plan.script, Script::Malayalam | Script::Tamil) {
        loop {
            while new_pos > 0
                && !matches!(
                    cat_at(glyphs, new_pos),
                    Category::Matra | Category::MatraPost | Category::Halant
                )
            {
                new_pos = new_pos.saturating_sub(1);
            }
            // No halant found: done. Otherwise proceed only if the halant is
            // not part of the matra itself.
            if glyphs.get(new_pos).is_some_and(is_halant)
                && pos_at(glyphs, new_pos) != Position::PreM
            {
                let after = new_pos.saturating_add(1);
                if after < end
                    && glyphs.get(after).map(|g| g.indic.category) == Some(Category::Joiner)
                    && new_pos > 0
                {
                    new_pos = new_pos.saturating_sub(1);
                    continue;
                }
            } else {
                new_pos = 0;
            }
            break;
        }
    }

    if new_pos > 0 && pos_at(glyphs, new_pos) != Position::PreM {
        // Now go see whether there are actually any matras to move.
        let mut i = new_pos;
        while i > 0 {
            let old_pos = i.saturating_sub(1);
            if pos_at(glyphs, old_pos) == Position::PreM {
                if old_pos < *base && *base <= new_pos {
                    *base = base.saturating_sub(1);
                }
                if let Some(run) = glyphs.get_mut(old_pos..=new_pos) {
                    run.rotate_left(1);
                }
                // Deliberately *after* the move: the span that can no longer be
                // pointed into is the one the matra crossed, which is only
                // known once it has crossed it.
                merge_range(glyphs, new_pos, end.min(base.saturating_add(1)));
                new_pos = new_pos.saturating_sub(1);
            }
            i = old_pos;
        }
    } else {
        // Nothing moved, but a matra still at the head shares a cluster with
        // everything up to the base, which initial reordering left it outside.
        for i in 0..*base {
            if pos_at(glyphs, i) == Position::PreM {
                merge_range(glyphs, i, end.min(base.saturating_add(1)));
                break;
            }
        }
    }
}

/// Move the reph to where the script says a reph goes.
///
/// A reph starts at the head of the syllable and stays there through the basic
/// features so that `rphf` can match it; where it *ends up* — after the main
/// consonant, before or after the below-base forms, after the post-base forms —
/// is the script's business, and is only settled now.
///
/// The guard is an exclusive-or, and it reads oddly until you see it as two
/// cases. A reph written out as `Ra,Halant` only moves if those two actually
/// ligated into one. A reph written as a single character (a Repha) only moves
/// if it did *not* ligate — if it did, the font is doing the reordering itself
/// and moving the glyph would undo the font's work.
fn reorder_reph(plan: &Plan, glyphs: &mut [SubGlyph], base: &mut usize) {
    let end = glyphs.len();
    let ligated = glyphs
        .first()
        .is_some_and(|g| g.lig.ligated_and_didnt_multiply());
    let repha = glyphs.first().map(|g| g.indic.category) == Some(Category::Repha);
    if end < 2 || pos_at(glyphs, 0) != Position::RaToBecomeReph || repha == ligated {
        return;
    }

    let new_reph_pos = reph_target(plan, glyphs, *base);
    merge_range(glyphs, 0, new_reph_pos.saturating_add(1));
    if let Some(run) = glyphs.get_mut(..=new_reph_pos) {
        run.rotate_left(1);
    }
    if *base > 0 && *base <= new_reph_pos {
        *base = base.saturating_sub(1);
    }
}

/// Where the reph goes: the six numbered steps of the spec, in order.
fn reph_target(plan: &Plan, glyphs: &[SubGlyph], base: usize) -> usize {
    let end = glyphs.len();
    let reph_pos = plan.reph_pos();

    // Steps 2 and 5 are the same search, and step 5 says so by copying it:
    // after the first explicit halant between the first post-reph consonant and
    // the last main consonant, or after a joiner following that halant. In an
    // old-spec font nothing is ever found here, because there the shaping
    // engine fixed the classifications and no halant survives in that span.
    let after_halant = || {
        let mut p = 1;
        while p < base && !glyphs.get(p).is_some_and(is_halant) {
            p = p.saturating_add(1);
        }
        if p < base && glyphs.get(p).is_some_and(is_halant) {
            let after = p.saturating_add(1);
            if after < base && glyphs.get(after).is_some_and(is_joiner) {
                p = after;
            }
            Some(p)
        } else {
            None
        }
    };

    // Step 1: after the post-base forms means straight to step 5.
    if reph_pos != Position::AfterPost {
        // Step 2.
        if let Some(p) = after_halant() {
            return p;
        }
        // Step 3: after the main consonant is after everything that ligated
        // with it — that is, everything still positioned no later than "after
        // main".
        if reph_pos == Position::AfterMain {
            let mut p = base;
            while p.saturating_add(1) < end
                && pos_at(glyphs, p.saturating_add(1)) <= Position::AfterMain
            {
                p = p.saturating_add(1);
            }
            if p < end {
                return p;
            }
        }
        // Step 4: before the post-base consonant forms. Our reading of a step
        // the spec states very badly.
        if reph_pos == Position::AfterSub {
            let mut p = base;
            while p.saturating_add(1) < end
                && !matches!(
                    pos_at(glyphs, p.saturating_add(1)),
                    Position::PostC | Position::AfterPost | Position::Smvd
                )
            {
                p = p.saturating_add(1);
            }
            if p < end {
                return p;
            }
        }
    }

    // Step 5, copied from step 2.
    // See https://github.com/harfbuzz/harfbuzz/issues/2298#issuecomment-615318654
    if let Some(p) = after_halant() {
        return p;
    }

    // Step 6: otherwise the end of the syllable, before its modifiers.
    let mut p = end.saturating_sub(1);
    while p > 0 && pos_at(glyphs, p) == Position::Smvd {
        p = p.saturating_sub(1);
    }
    // Landing after a Matra,Halant sequence would put the reph out of the
    // matra's reach, so step back before that halant. After a plain
    // Consonant,Halant it should not — and Uniscribe does not.
    // TEST: U+0930,U+094D,U+0915,U+094B,U+094D
    if glyphs.get(p).is_some_and(is_halant) {
        let mut i = base.saturating_add(1);
        while i < p {
            if matches!(
                glyphs.get(i).map(|g| g.indic.category),
                Some(Category::Matra | Category::MatraPost)
            ) {
                p = p.saturating_sub(1);
            }
            i = i.saturating_add(1);
        }
    }
    p
}

/// Move a pre-base-reordering consonant — a Ra the font gave a pre-base form —
/// to the front of the cluster it belongs to.
///
/// Only if it ligated. A font may ask for the `pref` feature generally and then
/// block it in some contexts, and a Ra whose pre-base form was blocked is an
/// ordinary consonant that must stay where it is. The target is found the same
/// way as for a pre-base matra, and failing that it goes immediately before the
/// main consonant.
fn reorder_pre_base(plan: &Plan, glyphs: &mut [SubGlyph], base: &mut usize, try_pref: bool) {
    let end = glyphs.len();
    if !try_pref || base.saturating_add(1) >= end {
        return;
    }
    for old_pos in base.saturating_add(1)..end {
        if glyphs
            .get(old_pos)
            .is_none_or(|g| g.mask & plan.masks.pref == 0)
        {
            continue;
        }
        if glyphs
            .get(old_pos)
            .is_some_and(|g| g.lig.ligated_and_didnt_multiply())
        {
            let mut new_pos = *base;
            if !matches!(plan.script, Script::Malayalam | Script::Tamil) {
                while new_pos > 0
                    && !matches!(
                        cat_at(glyphs, new_pos.saturating_sub(1)),
                        Category::Matra | Category::MatraPost | Category::Halant
                    )
                {
                    new_pos = new_pos.saturating_sub(1);
                }
            }
            if new_pos > 0
                && glyphs.get(new_pos.saturating_sub(1)).is_some_and(is_halant)
                && new_pos < end
                && glyphs.get(new_pos).is_some_and(is_joiner)
            {
                new_pos = new_pos.saturating_add(1);
            }

            merge_range(glyphs, new_pos, old_pos.saturating_add(1));
            if let Some(run) = glyphs.get_mut(new_pos..=old_pos) {
                run.rotate_right(1);
            }
            if new_pos <= *base && *base < old_pos {
                *base = base.saturating_add(1);
            }
        }
        // The first `pref` glyph is the only candidate, whichever way it went.
        break;
    }
}

/// The two features that run before any reordering.
///
/// `locl` because a face may spell a character differently for one language,
/// and every question asked below is about the glyph it ends up as. `ccmp` is
/// not in the Indic specification at all; HarfBuzz applies it here anyway, on
/// the grounds that a face that uses it uses it at the start.
const BEFORE: [&[u8; 4]; 2] = [b"locl", b"ccmp"];

/// The eleven basic features, applied one at a time between the two
/// reorderings, in this order.
///
/// One stage each rather than one pass over all eleven, because a later one is
/// written to match glyphs an earlier one built: `rphf` makes the reph, and
/// `abvs` — in the last stage — positions it. Running them together would have
/// the second look at the run before the first rewrote it.
const BASIC: [&[u8; 4]; 11] = [
    b"nukt", b"akhn", b"rphf", b"rkrf", b"pref", b"blwf", b"abvf", b"half", b"pstf", b"vatu",
    b"cjct",
];

/// The six features that run after final reordering, together.
///
/// Together and not one at a time because they do not feed each other, and
/// because fonts in the field intermix their lookups: HarfBuzz's comment names
/// the default Bengali font on Windows, whose `init`, `pres`, `abvs` and `blws`
/// lookups are interleaved in the LookupList and only come out right applied in
/// LookupList order.
const AFTER: [&[u8; 4]; 6] = [b"init", b"pres", b"abvs", b"blws", b"psts", b"haln"];

/// The Indic features every glyph in the run is eligible for.
///
/// The rest — `rphf`, `pref`, `blwf`, `abvf`, `half`, `pstf`, `init` — are
/// handed out per glyph by the reordering, which is the whole mechanism by
/// which a font is told *which* consonant to draw as a half form. These seven
/// are absent here for that reason and not by oversight; HarfBuzz's flag is
/// `F_GLOBAL` and the split is exactly the same one.
const GLOBAL: [&[u8; 4]; 10] = [
    b"nukt", b"akhn", b"rkrf", b"vatu", b"cjct", b"pres", b"abvs", b"blws", b"psts", b"haln",
];

/// The features that read ZWJ and ZWNJ themselves: HarfBuzz's
/// `F_MANUAL_JOINERS`, which every entry of its `indic_features` carries.
///
/// That is [`BASIC`] and [`AFTER`] — **not** [`BEFORE`], whose `locl` and
/// `ccmp` are not in `indic_features` at all. `collect_features_indic` enables
/// those two with `F_PER_SYLLABLE` alone, so they keep the automatic joiner
/// skipping every ordinary feature has, and a `ccmp` ligature may still form
/// across a ZWJ.
///
/// For the rest a joiner is the *subject* of the rule: `KA VIRAMA ZWJ SSA`
/// asks for a half form where `KA VIRAMA SSA` asks for a conjunct, and
/// `KA VIRAMA ZWNJ SSA` asks for neither. A lookup that stepped over the
/// joiner would form exactly the shape it was typed to prevent.
fn manual_joiners() -> u64 {
    feature_bits(&BASIC) | feature_bits(&AFTER)
}

/// Does `ch` *continue* a word — so that whatever follows it does not begin
/// one?
///
/// HarfBuzz answers with the Unicode general category, counting a word as
/// continuing through `Cf`, `Cn`, `Co`, `Cs`, every letter and every mark, and
/// breaking on numbers, punctuation, symbols, separators and controls. This
/// crate has no general-category table and answers with the `Bidi_Class`
/// instead, which it does have and which draws very nearly the same line:
/// letters are `L`, `R` or `Al`, marks are `Nsm`, and the format characters
/// that matter here — the two joiners — are `Bn`. Digits (`En`, `An`), spaces
/// (`Ws`), punctuation and symbols (`On`, `Cs`, `Es`, `Et`) break a word under
/// both readings.
///
/// The two disagree on the explicit bidi controls, which HarfBuzz continues a
/// word through and this breaks on, and on the C0 controls, the other way
/// about. Read by exactly one thing — whether a left matra takes the `init`
/// form — so a disagreement costs one glyph variant on text that puts a bidi
/// override in the middle of a Devanagari word.
#[must_use]
pub(crate) fn continues_word(ch: char) -> bool {
    matches!(
        bidi::class(ch),
        Class::L | Class::R | Class::Al | Class::Nsm | Class::Bn
    )
}

/// Shape one run of Indic text: HarfBuzz's Indic shaper, end to end.
///
/// `glyphs` is one script run, one glyph per character, as `cmap` produced it
/// and with [`SubGlyph::indic`] already set from the character. It comes back
/// reordered and substituted, and may be a different length: a broken cluster
/// gains a dotted circle and a conjunct loses glyphs to a ligature.
///
/// `subs` may be `None`. Reordering is not conditional on the font having
/// substitutions — a left matra is written before its consonant and drawn
/// before it whatever the font does — so a face with no `GSUB` still gets the
/// two reorderings, with every question it is asked answered "no".
///
/// `glyph` maps a character to a glyph: the face's `cmap`. It is asked about
/// exactly two characters, the virama and the dotted circle.
pub(crate) fn shape(
    data: &[u8],
    subs: Option<&Substitutions>,
    tags: Option<ScriptTags>,
    lang: Option<Lang>,
    chosen: Option<[u8; 4]>,
    script: Script,
    glyphs: &mut Vec<SubGlyph>,
    glyph: impl Fn(char) -> Option<u16>,
) {
    if glyphs.is_empty() {
        return;
    }
    let plan = Plan::new(script, Probe::new(data, subs, tags, lang, chosen), &glyph);
    let global = feature_bits(&GLOBAL);
    for g in glyphs.iter_mut() {
        g.mask |= global;
    }
    setup_syllables(glyphs);

    let stages = stages();
    // Everything the shaper itself asked for is confined to one syllable; the
    // ordinary features sharing the last stage with `AFTER` are not.
    let per_syllable = feature_bits(&BEFORE) | feature_bits(&BASIC) | feature_bits(&AFTER);

    let dotted = glyph('\u{25CC}');
    let mut order: Vec<u8> = Vec::new();
    let Some(subs) = subs else {
        // No lookups to run, so no stages either — but both reorderings still
        // happen, back to back, which is what they would do with every stage
        // between them substituting nothing.
        initial_reordering(&plan, glyphs, dotted, &mut order);
        final_reordering(&plan, glyphs);
        return;
    };
    let staging = Staging {
        stages: &stages,
        per_syllable,
        manual_joiners: manual_joiners(),
    };
    subs.apply_stages(data, tags, lang, &staging, glyphs, |stage, glyphs| {
        if stage == 0 {
            initial_reordering(&plan, glyphs, dotted, &mut order);
        } else if stage == BASIC.len() {
            final_reordering(&plan, glyphs);
        }
    });
}

/// The thirteen passes the lookups are applied in, as sets of feature bits.
///
/// The two openers together, the eleven basic ones one each, then everything
/// else at once. The eleven are one stage each because a later one is written
/// to match glyphs an earlier one built — `rphf` makes the reph that `abvs`
/// then positions, `half` makes the half-form that `cjct` then stacks.
///
/// "Everything else" is every feature this crate knows rather than just the six
/// of [`AFTER`], because the ordinary features — `rlig`, `calt`, `clig`,
/// `rclt`, and the positioning ones a face may have filed under `GSUB` — belong
/// to that last stage too: HarfBuzz adds them after the shaper's own and they
/// land in whatever stage is open, which is this one. `liga` is the exception,
/// switched off for Indic outright, because a standard-ligature lookup written
/// for Latin has no business joining two Devanagari letters.
///
/// The twelve earlier stages are masked *out* of that last one, and have to be:
/// **a feature belongs to exactly one stage.** HarfBuzz gets that from its map
/// builder, which merges the two entries a tag can pick up — `locl` and `ccmp`
/// are named both by this shaper and by the common features every run gets —
/// and keeps the *lower* stage. Leave them in both and every lookup they reach
/// runs a second time. On a real face that is usually invisible, because the
/// second pass looks at glyphs the first already rewrote and matches nothing,
/// which is why 556 host faces never showed it; it took a face whose features
/// announce themselves — `tools/gen_khmer_probe.py`, built for the sibling
/// shaper, which has the same shape and had the same bug — to make it visible.
///
/// A lookup reached by *two* features in different stages still runs twice, and
/// should: the masks differ, and that is HarfBuzz's behaviour too. What must not
/// happen is one feature running in two stages.
fn stages() -> [u64; 13] {
    let mut stages = [0u64; 13];
    if let Some(first) = stages.first_mut() {
        *first = feature_bits(&BEFORE);
    }
    for (slot, tag) in stages.iter_mut().skip(1).zip(BASIC) {
        *slot = feature_bit(tag);
    }
    let staged = feature_bits(&BEFORE) | feature_bits(&BASIC);
    if let Some(last) = stages.last_mut() {
        *last = ALL_FEATURES & !staged & !feature_bit(b"liga");
    }
    stages
}

/// Cut the run into syllables and stamp each glyph with the one it is in.
///
/// The ranges are deliberately *not* kept: `locl` and `ccmp` run before the
/// first reordering and are free to ligate, after which every index past the
/// join is wrong. Stamping each glyph instead makes the boundaries survive
/// anything a lookup does, since a ligature keeps its first component's stamp
/// and a decomposition gives every piece the whole's.
fn setup_syllables(glyphs: &mut [SubGlyph]) {
    let mut cats: Vec<Category> = Vec::with_capacity(glyphs.len());
    cats.extend(glyphs.iter().map(|g| g.indic.category));
    let mut ranges: Vec<(usize, usize, Syllable)> = Vec::new();
    syllables(&cats, &mut ranges);
    syllabic::stamp(glyphs, &ranges, Syllable::code);
}

/// Call `f` on each syllable of `glyphs` in turn, with the kind its stamp
/// records.
fn for_each_syllable(glyphs: &mut [SubGlyph], mut f: impl FnMut(Syllable, bool, &mut [SubGlyph])) {
    syllabic::for_each(glyphs, |stamp, word_start, syllable| {
        f(Syllable::from_code(stamp), word_start, syllable);
    });
}

/// Everything that happens between `ccmp` and the first basic feature.
fn initial_reordering(
    plan: &Plan,
    glyphs: &mut Vec<SubGlyph>,
    dotted: Option<u16>,
    order: &mut Vec<u8>,
) {
    plan.update_consonant_positions(glyphs);
    insert_dotted_circles(glyphs, dotted);
    for_each_syllable(glyphs, |kind, _, syllable| {
        initial_reordering_syllable(plan, kind, syllable, order);
    });
}

/// Everything that happens between the last basic feature and the rest.
fn final_reordering(plan: &Plan, glyphs: &mut [SubGlyph]) {
    for_each_syllable(glyphs, |_, word_start, syllable| {
        final_reordering_syllable(plan, syllable, word_start);
    });
}

/// Give every broken cluster a dotted circle: [`syllabic::insert_dotted_circles`]
/// with the Indic grammar's answers filled in.
///
/// The circle goes *after* a repha, not before it: a repha is drawn above the
/// letter that follows, and the letter that follows is the circle.
fn insert_dotted_circles(glyphs: &mut Vec<SubGlyph>, dotted: Option<u16>) {
    syllabic::insert_dotted_circles(
        glyphs,
        dotted,
        Category::DottedCircle,
        |stamp| Syllable::from_code(stamp) == Syllable::Broken,
        |g| g.indic.category == Category::Repha,
    );
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
    use crate::fixture::{
        gsub_from_scripts, ligature, ligature_set, ligature_subst, script_list, span,
    };
    use crate::gsub::{LOOKUP_LIGATURE, Lig};
    use crate::indic::Char;
    use alloc::vec::Vec;

    /// No feature may appear in two stages. A lookup a feature reaches is
    /// applied once per stage that names it, so a tag left in both an early
    /// stage and the catch-all last one runs its lookups twice — which on a
    /// real face is silent, because the second pass sees glyphs the first
    /// already rewrote and matches nothing, and on a face whose features
    /// announce themselves doubles every marker. This is the assertion that
    /// keeps `stages`'s `& !staged` from being deleted as redundant.
    #[test]
    fn no_feature_is_applied_in_two_stages() {
        let stages = stages();
        let mut seen = 0u64;
        for (i, &stage) in stages.iter().enumerate() {
            assert_eq!(
                stage & seen,
                0,
                "stage {i} repeats a feature of an earlier one"
            );
            seen |= stage;
        }
        // And the last stage really is the catch-all, minus what ran already
        // and minus the one feature Indic switches off.
        assert_eq!(seen, ALL_FEATURES & !feature_bit(b"liga"));
    }

    /// Each of the eleven basic features gets a stage to itself, in order,
    /// after the one the two openers share: a later one is written to match
    /// glyphs an earlier one built, so merging any two would make the second
    /// look at the run before the first rewrote it.
    #[test]
    fn the_basic_features_get_one_stage_each_in_order() {
        let stages = stages();
        assert_eq!(stages.len(), BASIC.len() + 2);
        assert_eq!(stages[0], feature_bits(&BEFORE));
        for (i, tag) in BASIC.iter().enumerate() {
            assert_eq!(stages[i + 1], feature_bit(tag), "{:?} is not alone", tag);
        }
    }

    /// Every script, so a table change cannot be tested against only the one
    /// script the test author had in mind.
    const ALL: [Script; 9] = [
        Script::Devanagari,
        Script::Bengali,
        Script::Gurmukhi,
        Script::Gujarati,
        Script::Oriya,
        Script::Tamil,
        Script::Telugu,
        Script::Kannada,
        Script::Malayalam,
    ];

    #[test]
    fn every_script_is_named_by_its_new_spec_tag() {
        let tags = [
            (b"dev2", Script::Devanagari),
            (b"bng2", Script::Bengali),
            (b"gur2", Script::Gurmukhi),
            (b"gjr2", Script::Gujarati),
            (b"ory2", Script::Oriya),
            (b"tml2", Script::Tamil),
            (b"tel2", Script::Telugu),
            (b"knd2", Script::Kannada),
            (b"mlm2", Script::Malayalam),
        ];
        for (tag, want) in tags {
            assert_eq!(Script::of(ScriptTags::exactly(*tag)), Some(want));
        }
        // And nothing else is: a script this shaper cannot shape must not be
        // routed to it, or its text is reordered by another script's rules.
        for tag in [b"latn", b"arab", b"sinh", b"khmr", b"mym2", b"DFLT"] {
            assert_eq!(Script::of(ScriptTags::exactly(*tag)), None);
        }
    }

    /// The tag a character produces has to be the tag [`Script::of`] matches,
    /// or the shaper is never reached for text that needs it.
    #[test]
    fn the_scripts_own_letters_name_it() {
        let letters = [
            ('\u{939}', Script::Devanagari),
            ('\u{9ac}', Script::Bengali),
            ('\u{a30}', Script::Gurmukhi),
            ('\u{ab0}', Script::Gujarati),
            ('\u{b30}', Script::Oriya),
            ('\u{bb0}', Script::Tamil),
            ('\u{c30}', Script::Telugu),
            ('\u{cb0}', Script::Kannada),
            ('\u{d30}', Script::Malayalam),
        ];
        for (ch, want) in letters {
            let tags = ScriptTags::of(ch).expect("an Indic letter has a script");
            assert_eq!(Script::of(tags), Some(want), "{ch:?}");
        }
    }

    /// Each script's virama must be a virama — the character
    /// [`indic`](crate::indic) files as [`Category::Halant`] — or every
    /// question asked of the face is about the wrong pair of glyphs.
    #[test]
    fn every_scripts_virama_is_one() {
        use crate::indic::{Category, Char};
        for script in ALL {
            let virama = script.config().virama;
            assert_eq!(Char::of(virama).category, Category::Halant, "{script:?}");
            // And it belongs to that script, not to the one above it in the
            // table — the failure a copy-paste in the table would produce.
            let tags = ScriptTags::of(virama).expect("a virama has a script");
            assert_eq!(Script::of(tags), Some(script));
        }
    }

    /// The reph position is a position the sort can act on. Anything outside
    /// the five HarfBuzz allows would put the reph somewhere final reordering
    /// never looks.
    #[test]
    fn every_reph_position_is_one_of_the_five() {
        for script in ALL {
            let pos = script.config().reph_pos;
            assert!(
                matches!(
                    pos,
                    Position::AfterMain
                        | Position::BeforeSub
                        | Position::AfterSub
                        | Position::BeforePost
                        | Position::AfterPost
                ),
                "{script:?} puts its reph at {pos:?}"
            );
        }
    }

    /// The two scripts that differ from the common case, spelled out, so that
    /// a table edit that flattens them fails here rather than in a sweep.
    #[test]
    fn the_scripts_that_differ_still_do() {
        assert_eq!(Script::Telugu.config().reph_mode, RephMode::Explicit);
        assert_eq!(Script::Malayalam.config().reph_mode, RephMode::LogRepha);
        assert_eq!(Script::Telugu.config().blwf_mode, BlwfMode::PostOnly);
        assert_eq!(Script::Kannada.config().blwf_mode, BlwfMode::PostOnly);
        for script in ALL {
            if !matches!(script, Script::Telugu | Script::Malayalam) {
                assert_eq!(script.config().reph_mode, RephMode::Implicit, "{script:?}");
            }
            if !matches!(script, Script::Telugu | Script::Kannada) {
                assert_eq!(
                    script.config().blwf_mode,
                    BlwfMode::PreAndPost,
                    "{script:?}"
                );
            }
        }
    }

    /// A `GSUB` registering every tag in `tags` under `script`, all naming one
    /// ligature lookup that joins `pair` into a glyph.
    ///
    /// A ligature rather than a single substitution because that is what the
    /// question is: a below-base form is one glyph standing for a consonant
    /// *and* its virama, and a lookup that matched only one of the two would
    /// answer yes to a probe that should be about the pair.
    fn face(script: &[u8; 4], tags: &[&[u8; 4]], pair: &[u16; 2]) -> Vec<u8> {
        let indices: Vec<u16> = (0..u16::try_from(tags.len()).unwrap()).collect();
        let subtable = ligature_subst(&pair[..1], &[ligature_set(&[ligature(JOINED, &pair[1..])])]);
        gsub_from_scripts(
            &script_list(&[(script, &indices)]),
            tags,
            LOOKUP_LIGATURE,
            &[&subtable],
        )
    }

    /// What the test faces' one ligature produces.
    const JOINED: u16 = 50;
    /// The virama of Devanagari, as this crate's test faces number glyphs.
    const VIRAMA: u16 = 1;
    /// A consonant to ask about.
    const CONSONANT: u16 = 2;

    /// Which tag the test face in `data` is chosen under for a run of `tags`.
    ///
    /// The same walk [`Face`](crate::sfnt::Face) does, over the same
    /// ScriptList, rather than a literal repeated from whatever the face was
    /// built with — so a test that registers `deva` and asks about `dev2` gets
    /// the answer the shipping code would give it and not the one the test
    /// author expected.
    fn chosen(data: &[u8], tags: Option<ScriptTags>) -> Option<[u8; 4]> {
        let mut names = crate::otl::script_tags(data, 0).unwrap_or_default();
        names.sort_unstable();
        crate::otl::chosen_from(&names, tags)
    }

    /// A plan over `data`, for `script`, with `VIRAMA` as the virama glyph.
    fn plan<'a>(data: &'a [u8], subs: &'a Substitutions, script: Script) -> Plan<'a> {
        let run = Some(tags(script));
        Plan::new(
            script,
            Probe::new(data, Some(subs), run, None, chosen(data, run)),
            |_| Some(VIRAMA),
        )
    }

    /// Both of `script`'s spellings, as a run of its text would carry them.
    ///
    /// Taken from the script's own virama rather than written out, because a
    /// plan built on [`ScriptTags::exactly`] could never reach an old-spec
    /// face: the whole question is which of the two spellings the face
    /// registers, and `exactly` offers it only one.
    fn tags(script: Script) -> ScriptTags {
        ScriptTags::of(script.config().virama).expect("a virama has a script")
    }

    #[test]
    fn a_consonant_no_feature_claims_is_a_base() {
        let data = face(b"dev2", &[b"blwf"], &[VIRAMA, 99]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        let plan = plan(&data, &subs, Script::Devanagari);
        assert_eq!(plan.consonant_position(CONSONANT), Position::BaseC);
    }

    #[test]
    fn a_consonant_the_below_form_feature_claims_is_drawn_below() {
        for tag in [b"blwf", b"vatu"] {
            let data = face(b"dev2", &[tag], &[VIRAMA, CONSONANT]);
            let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
            let plan = plan(&data, &subs, Script::Devanagari);
            assert_eq!(
                plan.consonant_position(CONSONANT),
                Position::BelowC,
                "{:?}",
                core::str::from_utf8(tag)
            );
        }
    }

    #[test]
    fn a_consonant_the_post_form_feature_claims_is_drawn_after() {
        for tag in [b"pstf", b"pref"] {
            let data = face(b"dev2", &[tag], &[VIRAMA, CONSONANT]);
            let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
            let plan = plan(&data, &subs, Script::Devanagari);
            assert_eq!(
                plan.consonant_position(CONSONANT),
                Position::PostC,
                "{:?}",
                core::str::from_utf8(tag)
            );
        }
    }

    /// Below beats after when a face claims the consonant under both, which is
    /// the reason the questions are asked in a fixed order rather than
    /// whichever the font lists first.
    #[test]
    fn the_below_form_is_asked_about_first() {
        let data = face(b"dev2", &[b"pstf", b"blwf"], &[VIRAMA, CONSONANT]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        let plan = plan(&data, &subs, Script::Devanagari);
        assert_eq!(plan.consonant_position(CONSONANT), Position::BelowC);
    }

    /// The old-spec glyph order — consonant then virama — is honoured too, in
    /// a new-spec face. Fonts really do ship it, and Uniscribe really does
    /// honour it; a shaper that only tried one order would find no below-base
    /// form and put the consonant on the base.
    #[test]
    fn either_glyph_order_answers() {
        let data = face(b"dev2", &[b"blwf"], &[CONSONANT, VIRAMA]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        let plan = plan(&data, &subs, Script::Devanagari);
        assert_eq!(plan.consonant_position(CONSONANT), Position::BelowC);
    }

    /// A face with no virama glyph can be asked nothing, and every consonant
    /// is a base — which is the reading that at least draws the letters.
    #[test]
    fn a_face_without_a_virama_calls_every_consonant_a_base() {
        let data = face(b"dev2", &[b"blwf"], &[VIRAMA, CONSONANT]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        let run = Some(tags(Script::Devanagari));
        let plan = Plan::new(
            Script::Devanagari,
            Probe::new(&data, Some(&subs), run, None, chosen(&data, run)),
            |_| None,
        );
        assert_eq!(plan.consonant_position(CONSONANT), Position::BaseC);
    }

    /// The tag the *face* registers decides the spec, not the tag the run
    /// asked for: both runs below ask for `dev2`, and the face that answers
    /// with `deva` is the one that gets old-spec treatment.
    #[test]
    fn the_face_decides_which_spec_is_in_force() {
        let new = face(b"dev2", &[b"blwf"], &[VIRAMA, CONSONANT]);
        let subs = Substitutions::parse(&new, Some(span(0, new.len())), None).unwrap();
        assert!(!plan(&new, &subs, Script::Devanagari).old_spec);

        let old = face(b"deva", &[b"blwf"], &[VIRAMA, CONSONANT]);
        let subs = Substitutions::parse(&old, Some(span(0, old.len())), None).unwrap();
        assert!(plan(&old, &subs, Script::Devanagari).old_spec);
    }

    /// Zero context is off for the old spec, and off for Malayalam under
    /// either spec. Transcribed rather than derived — see the module docs.
    #[test]
    fn zero_context_follows_the_spec_except_for_malayalam() {
        let new = face(b"dev2", &[b"blwf"], &[VIRAMA, CONSONANT]);
        let subs = Substitutions::parse(&new, Some(span(0, new.len())), None).unwrap();
        assert!(plan(&new, &subs, Script::Devanagari).zero_context);

        let old = face(b"deva", &[b"blwf"], &[VIRAMA, CONSONANT]);
        let subs = Substitutions::parse(&old, Some(span(0, old.len())), None).unwrap();
        assert!(!plan(&old, &subs, Script::Devanagari).zero_context);

        let mlym = face(b"mlm2", &[b"blwf"], &[VIRAMA, CONSONANT]);
        let subs = Substitutions::parse(&mlym, Some(span(0, mlym.len())), None).unwrap();
        assert!(!plan(&mlym, &subs, Script::Malayalam).zero_context);
    }

    // ---- Reordering ----------------------------------------------------

    /// A glyph run for `text`, one glyph per character and all distinct, so
    /// that an assertion about the order the glyphs came out in is an
    /// assertion about which *character* went where.
    fn run(text: &str) -> Vec<SubGlyph> {
        text.char_indices()
            .enumerate()
            .map(|(i, (at, ch))| SubGlyph {
                indic: Char::of(ch),
                ..SubGlyph::new(u16::try_from(FIRST + i).unwrap(), at)
            })
            .collect()
    }

    /// The glyph id [`run`] gives a run's first character. Past [`JOINED`] so
    /// that nothing collides with the glyph the test faces' ligature makes.
    const FIRST: usize = 100;

    /// A feature to register when a test wants a face with none of the ones
    /// the shaper reads.
    ///
    /// A `GSUB` that registers nothing at all does not parse — there is no
    /// table to point at — so "a font with no `rphf`" has to be a font with
    /// *something else*. `liga` is the something else: the Indic shaper turns
    /// it off rather than asking about it, so it can never be mistaken for one
    /// of the features under test.
    const INERT: &[u8; 4] = b"liga";

    /// Which characters of the original run, by index, `glyphs` now are.
    fn order_of(glyphs: &[SubGlyph]) -> Vec<usize> {
        glyphs
            .iter()
            .map(|g| usize::from(g.gid).saturating_sub(FIRST))
            .collect()
    }

    /// Build the face `tags` describes and hand a plan over it to `f`.
    ///
    /// The face's one lookup joins the run's first two glyphs, so a probe
    /// about the head of the syllable — which is what `rphf` asks — has
    /// something to say yes to.
    fn with_face(script: Script, tag: &[u8; 4], tags: &[&[u8; 4]], f: impl FnOnce(&Plan)) {
        let first = u16::try_from(FIRST).unwrap();
        let data = face(
            tag,
            if tags.is_empty() { &[INERT] } else { tags },
            &[first, first + 1],
        );
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        let run = Some(ScriptTags::exactly(*tag));
        let plan = Plan::new(
            script,
            Probe::new(&data, Some(&subs), run, None, chosen(&data, run)),
            |_| Some(VIRAMA),
        );
        f(&plan);
    }

    /// Lay `glyphs` out in place under a face registering `tags`: the first
    /// half only, which is what everything above this line is about.
    fn with_plan(script: Script, tag: &[u8; 4], tags: &[&[u8; 4]], glyphs: &mut [SubGlyph]) {
        with_face(script, tag, tags, |plan| {
            plan.update_consonant_positions(glyphs);
            initial_reordering_syllable(plan, Syllable::Consonant, glyphs, &mut Vec::new());
        });
    }

    /// Both halves back to back, with nothing between them — which is what a
    /// face whose lookups change nothing produces, and so lets a test say what
    /// reordering alone does to a syllable.
    fn both_halves(script: Script, tag: &[u8; 4], tags: &[&[u8; 4]], glyphs: &mut [SubGlyph]) {
        with_face(script, tag, tags, |plan| {
            plan.update_consonant_positions(glyphs);
            initial_reordering_syllable(plan, Syllable::Consonant, glyphs, &mut Vec::new());
            final_reordering_syllable(plan, glyphs, true);
        });
    }

    /// The reason this module exists: the `i` of Devanagari is typed after the
    /// consonant it modifies and drawn before it.
    #[test]
    fn a_left_matra_is_drawn_before_its_consonant() {
        // U+0939 HA, U+093F vowel sign I.
        let mut glyphs = run("\u{939}\u{93f}");
        with_plan(Script::Devanagari, b"dev2", &[], &mut glyphs);
        assert_eq!(order_of(&glyphs), [1, 0]);
    }

    /// And the matra drawn to the right is not moved, so that the rule is
    /// about the character rather than about matras.
    #[test]
    fn a_right_matra_stays_where_it_was_typed() {
        // U+0926 DA, U+0940 vowel sign II.
        let mut glyphs = run("\u{926}\u{940}");
        with_plan(Script::Devanagari, b"dev2", &[], &mut glyphs);
        assert_eq!(order_of(&glyphs), [0, 1]);
    }

    /// The base is the last consonant that is a letter in its own right.
    /// Everything before it is a pre-base form, and the halant that killed a
    /// consonant's vowel travels with the consonant.
    #[test]
    fn the_base_is_the_last_consonant_with_no_dependent_form() {
        // U+0928 NA, U+094D virama, U+0926 DA — a conjunct whose base is DA.
        let mut glyphs = run("\u{928}\u{94d}\u{926}");
        with_plan(Script::Devanagari, b"dev2", &[b"half"], &mut glyphs);
        assert_eq!(order_of(&glyphs), [0, 1, 2]);
        assert_eq!(glyphs[2].indic.position, Position::BaseC);
        assert_eq!(glyphs[0].indic.position, Position::PreC);
        assert_eq!(glyphs[1].indic.position, Position::PreC);
    }

    /// A half form is asked for by mask, and only before the base.
    #[test]
    fn only_the_glyphs_before_the_base_are_offered_a_half_form() {
        let mut glyphs = run("\u{928}\u{94d}\u{926}");
        with_plan(Script::Devanagari, b"dev2", &[b"half"], &mut glyphs);
        let half = feature_bit(b"half");
        assert_ne!(glyphs[0].mask & half, 0);
        assert_ne!(glyphs[1].mask & half, 0);
        assert_eq!(glyphs[2].mask & half, 0);
    }

    /// A ZWNJ after the virama asks for the letter rather than the half form,
    /// and cancels the mask that would have made one.
    #[test]
    fn a_non_joiner_cancels_the_half_form() {
        // U+0928 NA, U+094D virama, U+200C ZWNJ, U+0926 DA.
        let mut glyphs = run("\u{928}\u{94d}\u{200c}\u{926}");
        with_plan(Script::Devanagari, b"dev2", &[b"half"], &mut glyphs);
        let half = feature_bit(b"half");
        // The cancellation walks back from the ZWNJ to the consonant before
        // it, so the NA and its virama lose the bit. The ZWNJ itself keeps it,
        // exactly as HarfBuzz leaves it — the walk starts one glyph back — and
        // it costs nothing, since no font's `half` covers a joiner.
        assert_eq!(glyphs[0].mask & half, 0);
        assert_eq!(glyphs[1].mask & half, 0);
        // And the base never had it.
        assert_eq!(glyphs[3].mask & half, 0);
        // Without the ZWNJ, those two glyphs would have asked for a half form.
        let mut joined = run("\u{928}\u{94d}\u{926}");
        with_plan(Script::Devanagari, b"dev2", &[b"half"], &mut joined);
        assert_ne!(joined[0].mask & half, 0);
    }

    /// A face that has no `half` feature hands out no `half` mask, so no font
    /// is ever asked for a form it does not have.
    #[test]
    fn a_face_without_half_forms_hands_out_no_half_mask() {
        let mut glyphs = run("\u{928}\u{94d}\u{926}");
        with_plan(Script::Devanagari, b"dev2", &[b"blwf"], &mut glyphs);
        let half = feature_bit(b"half");
        for g in &glyphs {
            assert_eq!(g.mask & half, 0);
        }
    }

    /// A syllable opening RA + virama, in a font that forms a reph, sends the
    /// RA to the head as a reph-to-be and takes the next consonant as base.
    #[test]
    fn an_opening_ra_and_virama_become_a_reph() {
        // U+0930 RA, U+094D virama, U+0915 KA.
        let mut glyphs = run("\u{930}\u{94d}\u{915}");
        with_plan(Script::Devanagari, b"dev2", &[b"rphf"], &mut glyphs);
        assert_eq!(glyphs[0].indic.position, Position::RaToBecomeReph);
        assert_eq!(glyphs[2].indic.position, Position::BaseC);
        let rphf = feature_bit(b"rphf");
        assert_ne!(glyphs[0].mask & rphf, 0);
        assert_eq!(glyphs[2].mask & rphf, 0);
    }

    /// But only if the font says it forms one. The same text in a font with no
    /// `rphf` leaves the RA an ordinary pre-base consonant.
    #[test]
    fn a_font_that_forms_no_reph_gets_none() {
        let mut glyphs = run("\u{930}\u{94d}\u{915}");
        with_plan(Script::Devanagari, b"dev2", &[b"half"], &mut glyphs);
        assert_ne!(glyphs[0].indic.position, Position::RaToBecomeReph);
        assert_eq!(glyphs[2].indic.position, Position::BaseC);
    }

    /// And only if there is another consonant for it to sit over: RA + virama
    /// alone is a RA, not a reph with nothing under it.
    #[test]
    fn a_reph_needs_a_consonant_to_sit_over() {
        let mut glyphs = run("\u{930}\u{94d}");
        with_plan(Script::Devanagari, b"dev2", &[b"rphf"], &mut glyphs);
        assert_eq!(glyphs[0].indic.position, Position::BaseC);
    }

    /// Under the pre-revision rules a halant may have been moved, so nothing
    /// after the base can be trusted to have stayed put and the whole of it
    /// becomes one cluster: a caret can only honestly point at the head of a
    /// stretch that is no longer in typing order.
    #[test]
    fn the_old_rules_merge_everything_after_the_base() {
        // U+0928 NA, U+094D virama, U+0926 DA, U+0940 vowel sign II. `deva`
        // rather than `dev2` is what puts the face under the old rules.
        let mut glyphs = run("\u{928}\u{94d}\u{926}\u{940}");
        with_plan(Script::Devanagari, b"deva", &[b"half"], &mut glyphs);
        assert_eq!(glyphs[2].cluster, glyphs[3].cluster);
        // But not what is before it: those move again in final reordering, so
        // merging them now would join a cluster to one it is about to leave.
        assert_ne!(glyphs[0].cluster, glyphs[2].cluster);
    }

    /// A left matra crossing its base is *not* merged here, though it plainly
    /// scrambled the offsets.
    ///
    /// That is deliberate rather than an oversight, and it is why this is a
    /// test: a left matra is moved a second time in final reordering, brought
    /// back towards the base, so a merge at this point would fuse it to a
    /// cluster it is about to leave. Final reordering merges up to the base
    /// once the matra has landed, and the two halves interlock.
    /// <https://github.com/harfbuzz/harfbuzz/issues/2272>
    #[test]
    fn a_left_matra_keeps_its_own_cluster_until_final_reordering() {
        let mut glyphs = run("\u{939}\u{93f}");
        with_plan(Script::Devanagari, b"dev2", &[], &mut glyphs);
        assert_eq!(order_of(&glyphs), [1, 0]);
        assert_ne!(glyphs[0].cluster, glyphs[1].cluster);
    }

    /// A symbol cluster and a non-Indic one are not syllables, and are left
    /// exactly as they were.
    #[test]
    fn the_clusters_that_are_not_syllables_are_left_alone() {
        let data = face(b"dev2", &[INERT], &[100, 101]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len())), None).unwrap();
        let plan = plan(&data, &subs, Script::Devanagari);
        for kind in [Syllable::Symbol, Syllable::NonIndic] {
            let before = run("\u{939}\u{93f}");
            let mut after = before.clone();
            initial_reordering_syllable(&plan, kind, &mut after, &mut Vec::new());
            assert_eq!(before, after, "{kind:?}");
        }
    }

    /// An empty syllable cannot happen — the scanner never emits one — but the
    /// layout must not panic if one arrives anyway.
    #[test]
    fn an_empty_syllable_is_laid_out_without_complaint() {
        let mut glyphs: Vec<SubGlyph> = Vec::new();
        with_plan(Script::Devanagari, b"dev2", &[], &mut glyphs);
        assert!(glyphs.is_empty());
        both_halves(Script::Devanagari, b"dev2", &[], &mut glyphs);
        assert!(glyphs.is_empty());
    }

    // ---- Final reordering ----------------------------------------------

    /// The other half of the cluster interlock: a left matra with nowhere
    /// nearer to go stays where initial reordering put it, and *now* joins the
    /// cluster it crossed. This is the string the HarfBuzz sweep disagreed on.
    #[test]
    fn a_left_matra_with_nowhere_to_go_joins_its_base() {
        // U+0939 HA, U+093F vowel sign I.
        let mut glyphs = run("\u{939}\u{93f}");
        both_halves(Script::Devanagari, b"dev2", &[], &mut glyphs);
        assert_eq!(order_of(&glyphs), [1, 0]);
        assert_eq!(glyphs[0].cluster, glyphs[1].cluster);
    }

    /// A left matra is brought back to just after the last standalone halant,
    /// because that halant is where the half form the font made ends.
    #[test]
    fn a_left_matra_lands_after_the_last_standalone_halant() {
        // U+0915 KA, U+094D virama, U+0937 SSA, U+093F vowel sign I.
        let mut glyphs = run("\u{915}\u{94d}\u{937}\u{93f}");
        both_halves(Script::Devanagari, b"dev2", &[b"half"], &mut glyphs);
        // KA, virama, matra, SSA — the matra crossed back over the base and
        // the half form, and stopped at the halant.
        assert_eq!(order_of(&glyphs), [0, 1, 3, 2]);
        // And the span it crossed is one cluster, merged after the move.
        assert_eq!(glyphs[2].cluster, glyphs[3].cluster);
    }

    /// Unless a ZWJ follows that halant, which asks for the full letter and so
    /// leaves the matra out at the head of the syllable. Uniscribe's rule, not
    /// the spec's — the spec says the opposite.
    /// <https://github.com/harfbuzz/harfbuzz/issues/1070>
    #[test]
    fn a_joiner_after_the_halant_keeps_the_matra_at_the_head() {
        // U+091F TTA, U+094D virama, U+200D ZWJ, U+092F YA, U+093F vowel sign I.
        let mut glyphs = run("\u{91f}\u{94d}\u{200d}\u{92f}\u{93f}");
        both_halves(Script::Devanagari, b"dev2", &[b"half"], &mut glyphs);
        assert_eq!(order_of(&glyphs), [4, 0, 1, 2, 3]);
        // Nothing moved, so the whole syllable is one cluster instead.
        assert!(glyphs.iter().all(|g| g.cluster == glyphs[0].cluster));
    }

    /// A reph that the font really formed is moved out of the head of the
    /// syllable to where the script puts one.
    #[test]
    fn a_reph_that_formed_moves_off_the_head_of_the_syllable() {
        // The run as `rphf` leaves it: the RA and its virama are one glyph.
        let mut glyphs = run("\u{930}\u{915}");
        glyphs[0].indic.position = Position::RaToBecomeReph;
        glyphs[0].lig = Lig::at(1, 2, 0);
        glyphs[1].indic.position = Position::BaseC;
        with_face(Script::Devanagari, b"dev2", &[b"rphf"], |plan| {
            final_reordering_syllable(plan, &mut glyphs, true);
        });
        assert_eq!(order_of(&glyphs), [1, 0]);
        assert_eq!(glyphs[0].cluster, glyphs[1].cluster);
    }

    /// And one that did not stays put: the `Ra,Halant` that never ligated is
    /// still two letters, and moving them would be moving the wrong thing.
    #[test]
    fn a_reph_that_did_not_form_stays_where_it_is() {
        let mut glyphs = run("\u{930}\u{915}");
        glyphs[0].indic.position = Position::RaToBecomeReph;
        glyphs[1].indic.position = Position::BaseC;
        with_face(Script::Devanagari, b"dev2", &[b"rphf"], |plan| {
            final_reordering_syllable(plan, &mut glyphs, true);
        });
        assert_eq!(order_of(&glyphs), [0, 1]);
    }

    /// A pre-base form the font really made is moved in front of the base.
    #[test]
    fn a_pre_base_form_moves_in_front_of_the_base() {
        // The run as `pref` leaves it: the virama and the RA are one glyph,
        // carrying the mask that asked for it and the record that it ligated.
        let mut glyphs = run("\u{915}\u{930}");
        glyphs[0].indic.position = Position::BaseC;
        glyphs[1].indic.position = Position::PostC;
        glyphs[1].mask |= feature_bit(b"pref");
        glyphs[1].lig = Lig::at(1, 2, 0);
        with_face(Script::Devanagari, b"dev2", &[b"pref"], |plan| {
            final_reordering_syllable(plan, &mut glyphs, true);
        });
        assert_eq!(order_of(&glyphs), [1, 0]);
        assert_eq!(glyphs[0].cluster, glyphs[1].cluster);
    }

    /// A font may ask for `pref` generally and block it in context. A Ra whose
    /// pre-base form was blocked is an ordinary consonant: it does not move,
    /// and it is the base.
    #[test]
    fn a_pre_base_candidate_the_font_declined_is_the_base() {
        let mut glyphs = run("\u{915}\u{930}");
        glyphs[0].indic.position = Position::BaseC;
        glyphs[1].indic.position = Position::PostC;
        glyphs[1].mask |= feature_bit(b"pref");
        with_face(Script::Devanagari, b"dev2", &[b"pref"], |plan| {
            final_reordering_syllable(plan, &mut glyphs, true);
        });
        assert_eq!(order_of(&glyphs), [0, 1]);
        assert_eq!(glyphs[1].indic.position, Position::BaseC);
    }

    /// A left matra that begins a word asks for the word-initial form, and one
    /// that does not begin a word does not.
    #[test]
    fn only_a_word_initial_left_matra_asks_for_the_initial_form() {
        let init = feature_bit(b"init");
        for word_start in [true, false] {
            let mut glyphs = run("\u{939}\u{93f}");
            with_face(Script::Devanagari, b"dev2", &[b"init"], |plan| {
                plan.update_consonant_positions(&mut glyphs);
                initial_reordering_syllable(
                    plan,
                    Syllable::Consonant,
                    &mut glyphs,
                    &mut Vec::new(),
                );
                final_reordering_syllable(plan, &mut glyphs, word_start);
            });
            assert_eq!(glyphs[0].mask & init != 0, word_start, "{word_start}");
        }
    }

    /// A virama that a conjunct swallowed and a later decomposition gave back
    /// is a virama again — otherwise every halant test below refuses it and
    /// the matra has nowhere to land.
    #[test]
    fn a_virama_a_ligature_swallowed_and_gave_back_is_one_again() {
        let mut glyphs = run("\u{915}\u{94d}\u{937}");
        // As substitution would leave it: the category gone with the ligature,
        // the glyph itself back from a decomposition.
        glyphs[1].gid = VIRAMA;
        glyphs[1].indic.category = Category::Other;
        glyphs[1].lig = Lig::at(1, 2, 0).split();
        with_face(Script::Devanagari, b"dev2", &[b"half"], |plan| {
            final_reordering_syllable(plan, &mut glyphs, true);
        });
        assert_eq!(glyphs[1].indic.category, Category::Halant);
        assert!(!glyphs[1].lig.ligated());
        assert!(!glyphs[1].lig.multiplied());
    }

    // ---- The driver ----------------------------------------------------

    /// The stamps a run carries after [`setup_syllables`], one per glyph.
    fn stamps(glyphs: &[SubGlyph]) -> Vec<u8> {
        glyphs.iter().map(|g| g.syllable).collect()
    }

    /// Two syllables written end to end are stamped apart, and every glyph of
    /// one carries the same byte — which is the whole of what the boundaries
    /// are, since the ranges themselves are thrown away.
    #[test]
    fn each_syllable_is_stamped_and_the_next_one_differently() {
        // HA + vowel sign I, then HA again: two consonant syllables.
        let mut glyphs = run("\u{939}\u{93f}\u{939}");
        setup_syllables(&mut glyphs);
        let s = stamps(&glyphs);
        assert_eq!(s[0], s[1], "one syllable, one stamp");
        assert_ne!(s[1], s[2], "the next syllable is stamped apart");
        for stamp in s {
            assert_ne!(stamp, 0, "no glyph a syllable covers keeps zero");
            assert_eq!(Syllable::from_code(stamp), Syllable::Consonant);
        }
    }

    /// The kind rides in the low nibble, so a cluster that is not a syllable
    /// says so on every one of its glyphs.
    #[test]
    fn the_stamp_records_which_kind_of_cluster_it_is() {
        // A lone vowel sign: a matra with nothing to attach to.
        let mut glyphs = run("\u{93f}");
        setup_syllables(&mut glyphs);
        assert_eq!(Syllable::from_code(glyphs[0].syllable), Syllable::Broken);

        // An independent vowel heads a syllable of its own kind.
        let mut glyphs = run("\u{905}");
        setup_syllables(&mut glyphs);
        assert_eq!(Syllable::from_code(glyphs[0].syllable), Syllable::Vowel);
    }

    /// The serial is four bits and never takes zero, so it has fifteen values
    /// and a long enough run must reuse them. Reuse is harmless as long as it
    /// is never *adjacent* — which is what the wrap does, and what the walk in
    /// [`for_each_syllable`] relies on to find a boundary at all.
    #[test]
    fn a_run_longer_than_the_serial_still_separates_its_syllables() {
        let mut glyphs = run(&"\u{939}".repeat(20));
        setup_syllables(&mut glyphs);
        let s = stamps(&glyphs);
        assert_eq!(s.len(), 20, "one syllable per consonant");
        for pair in s.windows(2) {
            assert_ne!(pair[0], pair[1], "neighbours must not share a stamp");
        }
        assert!(s.contains(&s[16]), "and the serial does come back around");
        assert_eq!(s[0], s[15], "fifteen values, then the sixteenth repeats");
    }

    /// The walk hands each stamped run over once, whole and in order.
    #[test]
    fn every_syllable_is_visited_once() {
        let mut glyphs = run("\u{939}\u{93f}\u{939}\u{905}");
        setup_syllables(&mut glyphs);
        let mut seen: Vec<(Syllable, Vec<usize>)> = Vec::new();
        for_each_syllable(&mut glyphs, |kind, _, syllable| {
            seen.push((kind, order_of(syllable)));
        });
        assert_eq!(
            seen,
            [
                (Syllable::Consonant, vec![0, 1]),
                (Syllable::Consonant, vec![2]),
                (Syllable::Vowel, vec![3]),
            ]
        );
    }

    /// A syllable begins a word when nothing that continues one precedes it —
    /// which is what decides whether its left matra may ask for the
    /// word-initial form.
    #[test]
    fn only_a_syllable_no_letter_precedes_begins_a_word() {
        for (continues, expected) in [(true, [true, false]), (false, [true, true])] {
            let mut glyphs = run("\u{939}\u{939}");
            setup_syllables(&mut glyphs);
            glyphs[0].word = continues;
            let mut starts: Vec<bool> = Vec::new();
            for_each_syllable(&mut glyphs, |_, word_start, _| starts.push(word_start));
            assert_eq!(starts, expected, "preceded by a letter: {continues}");
        }
    }

    /// The glyph id the dotted-circle tests hand the shaper for U+25CC. Past
    /// everything [`run`] produces, so its arrival is unambiguous.
    const CIRCLE: u16 = 900;

    /// A broken cluster gains a circle to hang its marks on, in front of the
    /// mark and carrying the cluster's own stamp — so the circle is inside the
    /// syllable rather than a syllable of its own.
    #[test]
    fn a_broken_cluster_gains_a_dotted_circle() {
        let mut glyphs = run("\u{93f}");
        setup_syllables(&mut glyphs);
        let stamp = glyphs[0].syllable;
        insert_dotted_circles(&mut glyphs, Some(CIRCLE));
        assert_eq!(glyphs.len(), 2);
        assert_eq!(glyphs[0].gid, CIRCLE);
        assert_eq!(glyphs[0].indic.category, Category::DottedCircle);
        assert_eq!(glyphs[0].syllable, stamp, "the circle joins the cluster");
        assert_eq!(glyphs[0].cluster, glyphs[1].cluster);
    }

    /// One circle per broken cluster, not one per mark, and the well-formed
    /// syllable beside it gains nothing.
    #[test]
    fn a_well_formed_syllable_gains_nothing_and_a_broken_one_gains_one() {
        // HA + I, a space to close the syllable, then two stray vowel signs.
        let mut glyphs = run("\u{939}\u{93f} \u{93f}\u{93f}");
        setup_syllables(&mut glyphs);
        assert_eq!(Syllable::from_code(glyphs[3].syllable), Syllable::Broken);
        insert_dotted_circles(&mut glyphs, Some(CIRCLE));
        assert_eq!(
            glyphs.iter().filter(|g| g.gid == CIRCLE).count(),
            1,
            "one circle for the whole broken cluster"
        );
        assert_eq!(glyphs[3].gid, CIRCLE, "and it goes in front of the marks");
    }

    /// A repha is drawn above the letter that follows it, so the circle it
    /// needs goes after it rather than before.
    #[test]
    fn the_circle_goes_after_a_repha_not_before_it() {
        let mut glyphs = run("\u{93f}\u{93f}");
        setup_syllables(&mut glyphs);
        glyphs[0].indic.category = Category::Repha;
        insert_dotted_circles(&mut glyphs, Some(CIRCLE));
        assert_eq!(glyphs.len(), 3);
        assert_eq!(order_of(&glyphs)[0], 0, "the repha stays at the head");
        assert_eq!(glyphs[1].gid, CIRCLE);
    }

    /// Nothing to draw, nothing drawn: a face with no U+25CC is left alone
    /// rather than given a notdef box.
    #[test]
    fn a_face_with_no_dotted_circle_gains_nothing() {
        let mut glyphs = run("\u{93f}");
        setup_syllables(&mut glyphs);
        insert_dotted_circles(&mut glyphs, None);
        assert_eq!(glyphs.len(), 1);
    }

    /// End to end through [`shape`] with no `GSUB` at all: both reorderings
    /// still run, because a left matra is drawn before its consonant whatever
    /// the font does.
    #[test]
    fn a_face_with_no_gsub_is_still_reordered() {
        let mut glyphs = run("\u{939}\u{93f}");
        let tags = Some(ScriptTags::exactly(*b"dev2"));
        shape(
            &[],
            None,
            tags,
            None,
            None,
            Script::Devanagari,
            &mut glyphs,
            |ch| (ch == '\u{25CC}').then_some(CIRCLE),
        );
        assert_eq!(order_of(&glyphs), [1, 0]);
    }

    /// And an empty run is not a special case anywhere: it comes back empty.
    #[test]
    fn an_empty_run_is_shaped_into_nothing() {
        let mut glyphs: Vec<SubGlyph> = Vec::new();
        let tags = Some(ScriptTags::exactly(*b"dev2"));
        shape(
            &[],
            None,
            tags,
            None,
            None,
            Script::Devanagari,
            &mut glyphs,
            |_| None,
        );
        assert!(glyphs.is_empty());
    }
}
