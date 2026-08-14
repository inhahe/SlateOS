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

// Nothing outside this module builds a plan yet: `scaled` still shapes Indic
// runs as if they were Latin. `expect` rather than `allow` on purpose — the
// moment the shaper is wired in this goes unfulfilled and the compiler asks
// for the line back.
#![expect(
    dead_code,
    reason = "the shaper that builds this plan is not wired in yet"
)]

use crate::gsub::Substitutions;
use crate::indic::Position;
use crate::script::ScriptTags;

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
    /// Its `GSUB`.
    subs: &'a Substitutions,
    /// The script to look features up under.
    tags: Option<ScriptTags>,
}

impl<'a> Probe<'a> {
    /// A probe over `subs`, asking about a run of `tags`.
    #[must_use]
    pub(crate) fn new(data: &'a [u8], subs: &'a Substitutions, tags: Option<ScriptTags>) -> Self {
        Self { data, subs, tags }
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
        let old_spec = probe
            .subs
            .chosen_script(probe.tags)
            .is_none_or(|tag| tag.get(3) != Some(&b'2'));
        Self {
            probe,
            script,
            config,
            old_spec,
            zero_context: !old_spec && script != Script::Malayalam,
            virama: glyph(config.virama),
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

    /// Would the feature `tag` substitute `glyphs`, on this run's script?
    #[must_use]
    pub(crate) fn would(&self, tag: &[u8; 4], glyphs: &[u16]) -> bool {
        self.probe.subs.would_substitute(
            self.probe.data,
            self.probe.tags,
            tag,
            glyphs,
            self.zero_context,
        )
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
    use crate::fixture::{gsub_from_scripts, ligature, ligature_set, ligature_subst, script_list, span};
    use crate::gsub::LOOKUP_LIGATURE;
    use alloc::vec::Vec;

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
        let subtable = ligature_subst(
            &pair[..1],
            &[ligature_set(&[ligature(JOINED, &pair[1..])])],
        );
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

    /// A plan over `data`, for `script`, with `VIRAMA` as the virama glyph.
    fn plan<'a>(data: &'a [u8], subs: &'a Substitutions, script: Script) -> Plan<'a> {
        Plan::new(script, Probe::new(data, subs, Some(tags(script))), |_| {
            Some(VIRAMA)
        })
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
        let plan = Plan::new(
            Script::Devanagari,
            Probe::new(&data, &subs, Some(tags(Script::Devanagari))),
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
}
