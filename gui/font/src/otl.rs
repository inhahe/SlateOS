//! The tables `GSUB` and `GPOS` have in common.
//!
//! OpenType Layout is two tables with one shape. Both begin with the same
//! header, both reach their real content through the same three lists
//! (script → feature → lookup), and both express "which glyphs does this
//! apply to?" with the same coverage and class-definition tables. Only the
//! *subtables* at the end of that walk differ: `GSUB`'s replace glyphs,
//! `GPOS`'s move them.
//!
//! So the walk lives here once. The alternative — a copy in each of
//! [`kern`](crate::kern) and [`gsub`](crate::gsub) — is how a bounds check
//! ends up in one copy and not the other, and a font that crashes the
//! substitution path but not the positioning path is a bug nobody would
//! think to look for.
//!
//! Every function here is total: a malformed or truncated table yields
//! `None`, never a panic and never a read past the end of `data`. Offsets
//! are absolute byte positions into the whole font file, since that is what
//! the callers cache.
//!
//! # Script selection
//!
//! The walk starts at the ScriptList, not the FeatureList. That matters
//! because a tag is not unique: a face supporting both Arabic and Latin has
//! two features called `liga`, filed under different scripts and meaning
//! entirely different things. Walking the FeatureList and taking every record
//! with a wanted tag — which this module used to do — applies both to
//! everything, so an Arabic rule can rewrite a Latin word. `ebrima.ttf` on the
//! development host has such a rule.
//!
//! [`select`] resolves a script tag to a LangSys, following OpenType's
//! fallback chain: the requested script, then its older spelling for the Indic
//! scripts OpenType renumbered, then `DFLT`, then a script literally named
//! `dflt`. A face that registers its features under none of those has nothing
//! to offer the run, and is shaped without features rather than with all of
//! them. The chain stops at the first tag the face *registers*, even if that
//! script then names no features at all — script selection and feature
//! selection are separate steps, and only the first falls back.
//!
//! # Feature masks
//!
//! Every lookup comes back paired with a mask saying which of the requested
//! feature tags reached it. Most callers can ignore it: a feature like `liga`
//! is unconditional, so the lookups it reaches simply run. Arabic's positional
//! features are not — `fina` is an ordinary substitution covering every letter
//! in the face, and applying it the way `liga` is applied would turn a whole
//! word into final forms. The mask is how [`gsub`](crate::gsub) restricts such
//! a lookup to the glyphs [`joining`](crate::joining) marked eligible.
//!
//! # What is not here
//!
//! **Language selection.** Only the DefaultLangSys of the chosen script is
//! used; the LangSysRecords, which hold the per-language overrides (Turkish
//! dotless i, Serbian Cyrillic italics), are skipped. That needs a language
//! to come down from the caller, which nothing above this crate tracks yet.
//! Tracked as `TD-FONT-IGNORES-LANGSYS-OVERRIDES`.
//!
//! **Required features.** A LangSys may name one feature as required, outside
//! its feature index list. Nothing in the tag sets this crate asks for is ever
//! registered that way in practice, but skipping it is a real gap.
//!
//! **Script selection for `GPOS`.** Only `GSUB` selects per run, through
//! [`ByScript`]. Kerning and mark attachment take the union over every script,
//! through [`feature_subtables`], because they are consulted a glyph pair at a
//! time from a public API with no run behind it. Tracked as
//! `TD-GPOS-APPLIES-EVERY-SCRIPTS-FEATURES`.

use alloc::vec::Vec;

use crate::script::ScriptTags;
use crate::sfnt::{u16_at, u32_at};

/// A lookup, named by number, and the mask of the feature tags that reached
/// it.
///
/// The number is a LookupList index before the lookups are decoded and a
/// position in [`ByScript::lookups`] afterwards. The two are never mixed: the
/// translation happens exactly once, at the end of [`ByScript::parse`].
type Masked = (u16, u32);

/// One script's tag and the lookups it selects, each with its mask.
type Selection = ([u8; 4], Vec<Masked>);

/// A resolved script: where its DefaultLangSys is, and where the FeatureList
/// that its indices point into begins.
///
/// `lang_sys` is `None` when the script table was found but names no default
/// language system. That is not the same as not finding the script at all: the
/// script *was* chosen, and having chosen it the run gets the features it
/// names, which is none. See [`select`].
#[derive(Clone, Copy, Debug)]
struct LangSys {
    lang_sys: Option<usize>,
    feature_list: usize,
}

/// Find the LangSys that governs a run of `script`, following OpenType's
/// fallback chain.
///
/// `script` is `None` for a run with no script of its own — all digits and
/// punctuation — which goes straight to the default entries.
///
/// The chain stops at the first script tag the ScriptList *has*, whether or not
/// that script table turns out to offer anything. Selecting the script and
/// selecting its language system are two separate steps in OpenType, and only
/// the first one falls back: a face that registers `latn` with no DefaultLangSys
/// — `NotoSansLisu-Bold.ttf` on the development host does, filing its features
/// under `MOL ` and `ROM ` alone — is saying its Latin text gets no features,
/// not that its Latin text should be shaped as `DFLT`. Reading on to `DFLT`
/// there would attach marks the font deliberately declines to attach.
fn select(data: &[u8], base: usize, script: Option<ScriptTags>) -> Option<LangSys> {
    let script_list = base.checked_add(usize::from(u16_at(data, base.checked_add(4)?)?))?;
    let feature_list = base.checked_add(usize::from(u16_at(data, base.checked_add(6)?)?))?;

    // Order matters and is the whole of the policy: the run's own script
    // first, then the older OpenType spelling of it, then the two ways a font
    // says "these features are for everything". `DFLT` is the registered
    // default script tag; `dflt` is a common misspelling that real fonts ship,
    // and every shaper accepts it.
    for want in fallback_chain(script) {
        let Some(script_table) = find_script(data, script_list, &want) else {
            continue;
        };
        // A script table may exist and still have no default language system.
        // The run has been placed under this script all the same, and gets the
        // no features that its default language system names.
        let off = u16_at(data, script_table)?;
        return Some(LangSys {
            lang_sys: (off != 0)
                .then(|| script_table.checked_add(usize::from(off)))
                .flatten(),
            feature_list,
        });
    }
    None
}

/// The script tags to try for a run of `script`, best first.
///
/// Order is the whole of the policy: the run's own script first, then the
/// older OpenType spelling of it for the Indic scripts OpenType renumbered,
/// then the two ways a font says "these features are for everything". `DFLT`
/// is the registered default script tag; `dflt` is a misspelling that real
/// fonts ship, and every shaper accepts it.
///
/// A run with no script of its own — all digits and punctuation — starts at
/// `DFLT`, which is also what HarfBuzz does with a `Common` buffer.
fn fallback_chain(script: Option<ScriptTags>) -> impl Iterator<Item = [u8; 4]> {
    let (preferred, fallback) = script.map(|s| (s.preferred, s.fallback)).unzip();
    [preferred, fallback, Some(*b"DFLT"), Some(*b"dflt")]
        .into_iter()
        .flatten()
}

/// Where the ScriptTable for `want` begins, if the ScriptList has one.
fn find_script(data: &[u8], script_list: usize, want: &[u8; 4]) -> Option<usize> {
    let count = u16_at(data, script_list)?;
    for i in 0..usize::from(count) {
        let rec = script_list.checked_add(2)?.checked_add(i.checked_mul(6)?)?;
        if data.get(rec..rec.checked_add(4)?)? != want.as_slice() {
            continue;
        }
        return script_list.checked_add(usize::from(u16_at(data, rec.checked_add(4)?)?));
    }
    None
}

/// A limit on how many subtables are followed in total, so that a corrupt or
/// hostile font cannot make shaping arbitrarily slow: every glyph of a run is
/// offered to every subtable, so the cost of a shape is the run length times
/// this number.
///
/// The value is measured, not guessed, and the measurement is the whole point
/// — an earlier 64 here was set from a per-lookup glance ("real fonts use
/// single digits") and silently truncated 61 of the 365 installed faces that
/// have a `GSUB`, so Amiri and FiraCode lost their Latin ligatures entirely
/// while the tests stayed green. Counting what our own enabled features
/// actually reach across every face installed on the development host:
///
/// | measure | worst face | count |
/// |---|---|---|
/// | subtables in one lookup | SansSerifCollection | 675 |
/// | lookups reached | SansSerifCollection | 256 |
/// | subtables in total | SansSerifCollection | 1874 |
/// | (runner-up total) | JetBrains Mono | 768 |
///
/// 8192 is a little over four times the worst real face, which leaves room for
/// the CJK and Indic faces this host does not have without leaving the cost of
/// a hostile font unbounded. A face that does exceed it is shaped with the
/// lookups found so far rather than rejected: a slightly wrong ligature is a
/// better failure than a blank page.
pub(crate) const MAX_SUBTABLES: usize = 8192;

/// `lookupFlag` bit 4, the one bit this module has to understand: it is what
/// decides whether a `markFilteringSet` field follows the subtable offsets, so
/// the rest of the lookup header cannot be read without it.
const USE_MARK_FILTERING_SET: u16 = 0x0010;

/// One lookup: what it does, and where the subtables that do it live.
///
/// Kept as a unit rather than flattened into one list of subtables because a
/// lookup is the unit of *application*: the whole of lookup 1 runs across the
/// whole buffer before any of lookup 2 does, so a substitution the first makes
/// is visible to the second. Flattening loses that boundary, and with it the
/// only thing that makes `ccmp` reliably run before `liga`.
///
/// Callers that genuinely have one lookup type and no ordering to preserve —
/// pair kerning, mark attachment — use [`feature_subtables`] instead and get
/// the flat list.
#[derive(Clone, Debug)]
pub(crate) struct Lookup {
    /// The lookup type, in the numbering of the table it came from, and with
    /// any extension redirect already resolved: a caller never sees the
    /// extension type itself, only what it wrapped.
    pub(crate) kind: u16,
    /// The `lookupFlag`: which glyphs this lookup is not allowed to see, plus
    /// a mark-attachment class in the high byte. Interpreted by
    /// [`Skipper`](crate::skip::Skipper), not here — this is the byte range's
    /// only job.
    pub(crate) flag: u16,
    /// The `markFilteringSet` index, which follows the subtable offsets and is
    /// present only when the flag's `UseMarkFilteringSet` bit is set. Zero
    /// otherwise, which is harmless because nothing reads it unless the bit is.
    pub(crate) filter: u16,
    /// Absolute byte offsets of this lookup's subtables, in the order the font
    /// lists them — which is the order they are tried in, first match winning.
    pub(crate) subtables: Vec<usize>,
}

/// Offsets of the subtables reachable from the features tagged `tags`, under
/// *any* script the font registers.
///
/// `want` is the lookup type whose subtables the caller can read, and
/// `extension` is the lookup type that table uses for its 32-bit-offset
/// redirect (9 in `GPOS`, 7 in `GSUB`) — the two tables number their lookup
/// types independently, so neither can be hardcoded here.
///
/// The union over scripts is deliberate and is what separates this from
/// [`ByScript`]. It exists for the `GPOS` callers — pair kerning and mark
/// attachment — which have no run to ask about: they are consulted one glyph
/// pair at a time, from a public API that has no script to pass. That is a
/// real, if narrow, gap: see `TD-GPOS-APPLIES-EVERY-SCRIPTS-FEATURES`. It is
/// narrow because a positioning subtable is gated on glyph coverage, so a
/// face's Arabic `kern` simply does not cover Latin glyphs; substitution has
/// no such protection, which is why `GSUB` uses [`ByScript`] instead.
///
/// Returns `None` rather than an empty vector when nothing is found, because
/// every caller treats "this table has nothing for me" as a reason to fall
/// back rather than as a result.
pub(crate) fn feature_subtables(
    data: &[u8],
    base: usize,
    tags: &[&[u8; 4]],
    want: u16,
    extension: u16,
) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    for lookup in feature_lookups(data, base, tags, want, extension)? {
        out.extend_from_slice(&lookup.subtables);
    }
    (!out.is_empty()).then_some(out)
}

/// The same walk as [`feature_subtables`], keeping the lookups whole.
///
/// The flat list is the right shape for a caller that only wants coverage —
/// mark attachment asks "does any subtable have an anchor for this pair" and
/// the answer does not depend on which lookup the subtable came from. It is
/// the wrong shape for kerning, because a `kern` lookup's `lookupFlag` says
/// which glyphs the pair may be read *across*, and flattening throws the flag
/// away along with the lookup it belonged to.
pub(crate) fn feature_lookups(
    data: &[u8],
    base: usize,
    tags: &[&[u8; 4]],
    want: u16,
    extension: u16,
) -> Option<Vec<Lookup>> {
    let out = ByScript::parse(data, base, tags, &[want], extension)?
        .all()
        .to_vec();
    (!out.is_empty()).then_some(out)
}

/// The lookups a table offers, resolved once per script the font registers.
///
/// Selection is per *run* — a mixed Arabic-and-Latin string is two runs and
/// picks different features for each — but decoding is per *face*, and the two
/// have to be reconciled somewhere. Doing the feature walk at shaping time is
/// the obvious answer and is too slow: on the worst installed face it means
/// re-reading 1874 subtable offsets for every string drawn, on a path that
/// runs once per label per frame.
///
/// So every script the font registers is resolved up front. The lookups
/// themselves are decoded *once* and shared: scripts overwhelmingly select
/// overlapping sets of them, and a face that registers ten scripts would
/// otherwise hold ten copies of nearly the same list. What varies per script
/// is only which of them apply, and that is two bytes each.
#[derive(Clone, Debug)]
pub(crate) struct ByScript {
    /// Every lookup any script reaches, in LookupList order — which is the
    /// order they must be applied in.
    lookups: Vec<Lookup>,
    /// Script tag to the positions in `lookups` it selects, each with the
    /// mask of the feature tags that reached it; sorted by tag so a run can
    /// find its own with a binary search. Positions are ascending, so applying
    /// them in order is applying them in LookupList order.
    ///
    /// The mask is per *script* rather than stored on the [`Lookup`] because
    /// one lookup may be reached by different features under different
    /// scripts — a face may use the same substitution for Latin's `liga` and
    /// Arabic's `fina`. Folding the two together would let the Latin run's
    /// unconditional `liga` bit switch on a lookup that, for the Arabic run,
    /// must only reach final-form glyphs.
    scripts: Vec<Selection>,
}

impl ByScript {
    /// Resolve `tags` for every script `base` registers.
    ///
    /// Each lookup comes back paired with a mask: bit *i* is set when the
    /// feature tagged `tags[i]` reached it. Callers that apply features
    /// unconditionally can ignore it; the one that cannot is `GSUB`, where the
    /// Arabic positional features are ordinary substitutions covering every
    /// letter and are correct only for the glyphs the shaper marked eligible.
    /// Tags past the 32nd get no bit, since nothing here asks for that many.
    ///
    /// Returns `None` when no script selects any lookup of a wanted type,
    /// which is not an error: a monospace face has no ligatures by design.
    pub(crate) fn parse(
        data: &[u8],
        base: usize,
        tags: &[&[u8; 4]],
        want: &[u16],
        extension: u16,
    ) -> Option<Self> {
        let lookup_list = lookup_list(data, base)?;
        let lookup_count = u16_at(data, lookup_list)?;

        // Which lookups each script selects, and the union of all of them.
        let mut selected: Vec<Selection> = Vec::new();
        let mut union: Vec<u16> = Vec::new();
        for tag in script_tags(data, base)? {
            // A script is asked for under its own tag exactly: the fallback
            // chain belongs to the *run*, in `for_script`, and applying it
            // here would file `DFLT`'s lookups under every script's name.
            let Some((_, indices)) = lookup_indices(data, base, tags, Some(ScriptTags::exactly(tag))) else {
                continue;
            };
            union.extend(
                indices
                    .iter()
                    .map(|&(i, _)| i)
                    .filter(|&i| i < lookup_count),
            );
            selected.push((tag, indices));
        }
        union.sort_unstable();
        union.dedup();

        // Decode each lookup exactly once, in LookupList order. Scripts
        // overwhelmingly select overlapping sets, and a face registering ten
        // scripts would otherwise hold ten copies of nearly the same list.
        // Decoding in order is what makes a script's positions ascending, and
        // therefore what makes "apply them in order" the correct thing to do.
        let mut budget = MAX_SUBTABLES;
        let mut lookups: Vec<Lookup> = Vec::new();
        // LookupList index to its position in `lookups`, ascending in both.
        let mut at: Vec<(u16, u16)> = Vec::new();
        for idx in union {
            if budget == 0 {
                break;
            }
            let Some(off) = lookup_list
                .checked_add(2)
                .and_then(|o| o.checked_add(usize::from(idx).checked_mul(2)?))
                .and_then(|a| u16_at(data, a))
                .and_then(|o| lookup_list.checked_add(usize::from(o)))
            else {
                continue;
            };
            // Not of a type this caller can apply — most of a font's lookups,
            // for most callers.
            let Some(lookup) = read_lookup(data, off, want, extension, &mut budget) else {
                continue;
            };
            let Ok(pos) = u16::try_from(lookups.len()) else {
                break;
            };
            lookups.push(lookup);
            at.push((idx, pos));
        }

        let mut scripts: Vec<Selection> = Vec::new();
        for (tag, indices) in selected {
            // `indices` is ascending and deduplicated, and `at` ascends in
            // both columns, so the result is ascending and unique too.
            let positions: Vec<Masked> = indices
                .iter()
                .filter_map(|&(idx, mask)| {
                    let k = at.binary_search_by_key(&idx, |&(i, _)| i).ok()?;
                    at.get(k).map(|&(_, pos)| (pos, mask))
                })
                .collect();
            // Kept even when empty. An empty entry is what stops `for_script`'s
            // fallback chain at a script the font registers and then gives
            // nothing: having been chosen, the script's own answer — none —
            // is the run's answer, and reading on to `DFLT` would apply rules
            // the font filed under another script entirely.
            scripts.push((tag, positions));
        }
        if lookups.is_empty() {
            return None;
        }
        scripts.sort_unstable_by_key(|&(tag, _)| tag);
        Some(Self { lookups, scripts })
    }

    /// Every lookup any script reaches, in the order they must be applied.
    ///
    /// For the callers that have no run to ask about — see
    /// [`feature_subtables`] and [`feature_lookups`], which are the only two.
    pub(crate) fn all(&self) -> &[Lookup] {
        &self.lookups
    }

    /// The lookups that apply to a run of `script`, in the order they run,
    /// each with the mask of the feature tags that reached it.
    ///
    /// Follows the same fallback chain as [`select`]: the run's script, its
    /// older OpenType spelling, then the two default tags — and stops at the
    /// first tag the face registers, whether or not that script selected
    /// anything. An empty iterator means this face has nothing for the run,
    /// which is a normal answer — and the right one, since the alternative is
    /// applying another script's rules to it.
    pub(crate) fn for_script(
        &self,
        script: Option<ScriptTags>,
    ) -> impl Iterator<Item = (&Lookup, u32)> {
        let positions = fallback_chain(script)
            .find_map(|want| {
                self.scripts
                    .binary_search_by_key(&want, |&(tag, _)| tag)
                    .ok()
                    .and_then(|i| self.scripts.get(i))
                    .map(|(_, positions)| positions.as_slice())
            })
            .unwrap_or(&[]);
        positions
            .iter()
            .filter_map(|&(at, mask)| Some((self.lookups.get(usize::from(at))?, mask)))
    }
}

/// Every script tag the table's ScriptList registers, in file order.
pub(crate) fn script_tags(data: &[u8], base: usize) -> Option<Vec<[u8; 4]>> {
    let script_list = base.checked_add(usize::from(u16_at(data, base.checked_add(4)?)?))?;
    let count = u16_at(data, script_list)?;
    let mut out = Vec::with_capacity(usize::from(count));
    for i in 0..usize::from(count) {
        let rec = script_list.checked_add(2)?.checked_add(i.checked_mul(6)?)?;
        let Some(tag) = data
            .get(rec..rec.checked_add(4)?)
            .and_then(|s| <[u8; 4]>::try_from(s).ok())
        else {
            continue;
        };
        out.push(tag);
    }
    (!out.is_empty()).then_some(out)
}

/// Where a `GSUB`/`GPOS` table's LookupList begins.
///
/// Contextual lookups name the lookups they invoke by *index into this list*,
/// and the index may be any lookup in the font — including one no feature
/// reaches, which is the usual way a font hides a helper lookup. So a caller
/// that applies them needs the list itself, not just the lookups a feature
/// walk found.
pub(crate) fn lookup_list(data: &[u8], base: usize) -> Option<usize> {
    base.checked_add(usize::from(u16_at(data, base.checked_add(8)?)?))
}

/// One lookup of a LookupList by index, of a type in `want`, extensions
/// unwrapped.
///
/// This is the lookup-by-index that types 5 and 6 need. It re-reads the lookup
/// on every invocation rather than caching it: the alternative is decoding the
/// whole LookupList up front, most of which a run never reaches, and the read
/// is a header and a handful of offsets.
pub(crate) fn lookup_at(
    data: &[u8],
    lookup_list: usize,
    index: u16,
    want: &[u16],
    extension: u16,
    budget: &mut usize,
) -> Option<Lookup> {
    if index >= u16_at(data, lookup_list)? {
        return None;
    }
    let at = lookup_list
        .checked_add(2)?
        .checked_add(usize::from(index).checked_mul(2)?)?;
    let lookup = lookup_list.checked_add(usize::from(u16_at(data, at)?))?;
    read_lookup(data, lookup, want, extension, budget)
}

/// Which lookups the features tagged `tags` use, each with the mask of the
/// tags that reached it, and where the LookupList is.
///
/// `None` only when `script` resolves to no script table at all. A script that
/// resolves and then names none of `tags` comes back with an empty list, which
/// is a different answer and has to stay different: it is what tells
/// [`ByScript::parse`] to record the script as offering nothing rather than to
/// leave it out and let the run fall through to `DFLT`.
///
/// Ascending and deduplicated by index, because lookups apply in LookupList
/// order regardless of the order a feature happens to list them in. Two
/// features may share a lookup, and when they do the masks are combined: the
/// lookup then applies wherever *either* feature would have applied it, which
/// is what HarfBuzz does with the same collision.
fn lookup_indices(
    data: &[u8],
    base: usize,
    tags: &[&[u8; 4]],
    script: Option<ScriptTags>,
) -> Option<(usize, Vec<Masked>)> {
    let lookup_list = lookup_list(data, base)?;
    let LangSys {
        lang_sys,
        feature_list,
    } = select(data, base, script)?;

    // The LangSys names features by index into the FeatureList, and *only*
    // those features apply to this run. This indirection is the whole point of
    // the ScriptList: the FeatureList is the font's whole inventory, and a
    // language system picks its subset out of it. No LangSys at all means the
    // script names no features, which is an answer and not a failure.
    let feature_count = lang_sys
        .and_then(|at| at.checked_add(4))
        .and_then(|at| u16_at(data, at))
        .unwrap_or(0);
    let mut indices = Vec::new();
    for i in 0..usize::from(feature_count) {
        let Some(lang_sys) = lang_sys else { break };
        let at = lang_sys.checked_add(6)?.checked_add(i.checked_mul(2)?)?;
        let Some(feature_index) = u16_at(data, at) else {
            continue;
        };
        let Some(rec) = feature_list
            .checked_add(2)
            .and_then(|o| o.checked_add(usize::from(feature_index).checked_mul(6)?))
        else {
            continue;
        };
        // An index past the end of the FeatureList is corruption, not a
        // reason to stop: the rest of the list may still be sound.
        let Some(tag) = rec.checked_add(4).and_then(|e| data.get(rec..e)) else {
            continue;
        };
        let Some(which) = tags.iter().position(|want| want.as_slice() == tag) else {
            continue;
        };
        // Bit per tag, in the order the caller listed them. A caller asking
        // for more than 32 gets no bit for the rest, which would silently
        // disable them — so it must not; nothing here comes close.
        let mask = u32::try_from(which).ok().and_then(|b| 1u32.checked_shl(b));
        let Some(mask) = mask else { continue };
        let Some(feature) = u16_at(data, rec.checked_add(4)?)
            .and_then(|o| feature_list.checked_add(usize::from(o)))
        else {
            continue;
        };
        let count = u16_at(data, feature.checked_add(2)?)?;
        for j in 0..usize::from(count) {
            let at = feature.checked_add(4)?.checked_add(j.checked_mul(2)?)?;
            if let Some(idx) = u16_at(data, at) {
                indices.push((idx, mask));
            }
        }
    }
    // Sort by index, then fold the duplicates together by OR-ing their masks.
    indices.sort_unstable_by_key(|&(idx, _)| idx);
    let mut folded: Vec<Masked> = Vec::with_capacity(indices.len());
    for (idx, mask) in indices {
        match folded.last_mut() {
            Some((last, into)) if *last == idx => *into |= mask,
            _ => folded.push((idx, mask)),
        }
    }
    // Empty is a result, not a failure: a script the face registers and then
    // gives no wanted feature has answered the question. `None` here is
    // reserved for "there is no such script", which is what lets the caller
    // stop the fallback chain at the right place.
    Some((lookup_list, folded))
}

/// One lookup, if it is of a type in `want`, with extensions unwrapped.
///
/// `budget` is how many more subtables may be followed in total, decremented
/// as they are taken — a corrupt or hostile font must not be able to make
/// lookup quadratic by declaring thousands of them.
fn read_lookup(
    data: &[u8],
    lookup: usize,
    want: &[u16],
    extension: u16,
    budget: &mut usize,
) -> Option<Lookup> {
    let kind = u16_at(data, lookup)?;
    let flag = u16_at(data, lookup.checked_add(2)?)?;
    let count = u16_at(data, lookup.checked_add(4)?)?;
    let extended = kind == extension;
    if !extended && !want.contains(&kind) {
        return None;
    }
    // For an extension lookup the real type is not known until a subtable has
    // been unwrapped. The spec requires every subtable of one lookup to have
    // the same type, so the first one seen settles it and a later disagreement
    // is a malformed font, not a second type to honour.
    let mut effective = if extended { None } else { Some(kind) };
    let mut subtables = Vec::new();
    for i in 0..usize::from(count) {
        if *budget == 0 {
            break;
        }
        let Some(sub) = lookup
            .checked_add(6)
            .and_then(|o| i.checked_mul(2).and_then(|d| o.checked_add(d)))
            .and_then(|at| u16_at(data, at))
            .and_then(|o| lookup.checked_add(usize::from(o)))
        else {
            continue;
        };
        if !extended {
            subtables.push(sub);
            *budget = budget.saturating_sub(1);
            continue;
        }
        // An extension subtable is a three-field redirect: format, the type it
        // wraps, then a 32-bit offset from the extension's own start.
        let Some(wrapped) = sub.checked_add(2).and_then(|o| u16_at(data, o)) else {
            continue;
        };
        if !want.contains(&wrapped) || effective.is_some_and(|k| k != wrapped) {
            continue;
        }
        if let Some(target) = sub
            .checked_add(4)
            .and_then(|o| u32_at(data, o))
            .and_then(|o| usize::try_from(o).ok())
            .and_then(|o| sub.checked_add(o))
        {
            effective = Some(wrapped);
            subtables.push(target);
            *budget = budget.saturating_sub(1);
        }
    }
    let kind = effective?;
    // `markFilteringSet` follows the subtable-offset array, so its position
    // depends on the *declared* count rather than on how many offsets were
    // actually read — a truncated table must not move the field.
    let filter = if flag & USE_MARK_FILTERING_SET == 0 {
        0
    } else {
        lookup
            .checked_add(6)
            .and_then(|o| usize::from(count).checked_mul(2).and_then(|d| o.checked_add(d)))
            .and_then(|at| u16_at(data, at))
            .unwrap_or(0)
    };
    (!subtables.is_empty()).then_some(Lookup {
        kind,
        flag,
        filter,
        subtables,
    })
}

/// Where `glyph` sits in a coverage table, or `None` if it is not covered.
pub(crate) fn coverage_index(data: &[u8], table: usize, glyph: u16) -> Option<u16> {
    match u16_at(data, table)? {
        1 => {
            let count = u16_at(data, table.checked_add(2)?)?;
            let first = table.checked_add(4)?;
            // A sorted list of glyph ids; the position *is* the index.
            let i = binary_search(usize::from(count), |i| {
                let at = first.checked_add(i.checked_mul(2)?)?;
                Some(u16_at(data, at)?.cmp(&glyph))
            })?;
            u16::try_from(i).ok()
        }
        2 => {
            let count = u16_at(data, table.checked_add(2)?)?;
            let first = table.checked_add(4)?;
            let at = range_containing(data, first, usize::from(count), glyph)?;
            let start = u16_at(data, at)?;
            let base = u16_at(data, at.checked_add(4)?)?;
            // The record says where its first glyph sits; the rest of the
            // range follows on consecutively.
            base.checked_add(glyph.checked_sub(start)?)
        }
        _ => None,
    }
}

/// The class `glyph` belongs to. Class 0 is "everything not listed", which is
/// a real class that a subtable may act on, so an unlisted glyph is `Some(0)`
/// rather than `None`.
pub(crate) fn glyph_class(data: &[u8], table: usize, glyph: u16) -> Option<u16> {
    match u16_at(data, table)? {
        1 => {
            let start = u16_at(data, table.checked_add(2)?)?;
            let count = u16_at(data, table.checked_add(4)?)?;
            let Some(i) = glyph.checked_sub(start).filter(|i| *i < count) else {
                return Some(0);
            };
            let at = table
                .checked_add(6)?
                .checked_add(usize::from(i).checked_mul(2)?)?;
            Some(u16_at(data, at).unwrap_or(0))
        }
        2 => {
            let count = u16_at(data, table.checked_add(2)?)?;
            let first = table.checked_add(4)?;
            let class = range_containing(data, first, usize::from(count), glyph)
                .and_then(|at| u16_at(data, at.checked_add(4)?));
            Some(class.unwrap_or(0))
        }
        _ => None,
    }
}

/// Find the record covering `glyph` in a sorted array of `count` six-byte
/// range records starting at `first`, and return its offset.
///
/// Coverage format 2 and class-definition format 2 have exactly this shape —
/// `start`, `end`, then one payload field — so they share the search. The
/// offset comes back rather than the payload because the two want different
/// fields out of the record, and computing both eagerly is what makes a
/// non-matching probe fail: `glyph - start` underflows on every range the
/// search steps over on its way to the right one.
pub(crate) fn range_containing(
    data: &[u8],
    first: usize,
    count: usize,
    glyph: u16,
) -> Option<usize> {
    binary_search(count, |i| {
        let at = first.checked_add(i.checked_mul(6)?)?;
        let start = u16_at(data, at)?;
        let end = u16_at(data, at.checked_add(2)?)?;
        Some(if end < glyph {
            core::cmp::Ordering::Less
        } else if start > glyph {
            core::cmp::Ordering::Greater
        } else {
            core::cmp::Ordering::Equal
        })
    })
    .and_then(|i| first.checked_add(i.checked_mul(6)?))
}

/// Binary search over `count` records, returning the index of the one whose
/// `probe` reports [`Ordering::Equal`](core::cmp::Ordering::Equal).
///
/// Shared because every sorted array in these tables — coverage lists,
/// coverage ranges, class ranges, pair records, legacy kern pairs — is
/// searched the same way, and writing the loop five times is how an off-by-one
/// gets into one copy and not the others. A `probe` that fails (a truncated
/// table) ends the search rather than reading past the end.
pub(crate) fn binary_search(
    count: usize,
    probe: impl Fn(usize) -> Option<core::cmp::Ordering>,
) -> Option<usize> {
    let mut lo = 0usize;
    let mut hi = count;
    while lo < hi {
        let mid = lo.checked_add(hi.checked_sub(lo)?.checked_div(2)?)?;
        match probe(mid)? {
            core::cmp::Ordering::Less => lo = mid.checked_add(1)?,
            core::cmp::Ordering::Greater => hi = mid,
            core::cmp::Ordering::Equal => return Some(mid),
        }
    }
    None
}

/// Size in bytes of a value record with `format`: one 16-bit field per set bit.
pub(crate) fn value_size(format: u16) -> usize {
    // At most 16 bits are set, so the product is at most 32 and the cast
    // cannot lose anything on any target.
    (format.count_ones() as usize).saturating_mul(2)
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
    use alloc::vec;

    fn be16(v: u16) -> [u8; 2] {
        v.to_be_bytes()
    }

    /// A coverage format 1 table listing `glyphs`.
    fn coverage1(glyphs: &[u16]) -> Vec<u8> {
        let mut t = vec![];
        t.extend(be16(1));
        t.extend(be16(u16::try_from(glyphs.len()).unwrap()));
        for g in glyphs {
            t.extend(be16(*g));
        }
        t
    }

    #[test]
    fn coverage_format_1_reports_the_position_in_the_list() {
        let t = coverage1(&[10, 20, 30, 40]);
        assert_eq!(coverage_index(&t, 0, 10), Some(0));
        assert_eq!(coverage_index(&t, 0, 30), Some(2));
        assert_eq!(coverage_index(&t, 0, 40), Some(3));
        assert_eq!(coverage_index(&t, 0, 35), None);
        assert_eq!(coverage_index(&t, 0, 0), None);
        assert_eq!(coverage_index(&t, 0, 99), None);
    }

    #[test]
    fn coverage_format_2_counts_through_the_ranges() {
        // Two ranges: 10..=12 at coverage 0..=2, then 50..=51 at 3..=4.
        let mut t = vec![];
        t.extend(be16(2));
        t.extend(be16(2));
        t.extend(be16(10));
        t.extend(be16(12));
        t.extend(be16(0));
        t.extend(be16(50));
        t.extend(be16(51));
        t.extend(be16(3));
        assert_eq!(coverage_index(&t, 0, 10), Some(0));
        assert_eq!(coverage_index(&t, 0, 12), Some(2));
        assert_eq!(coverage_index(&t, 0, 50), Some(3));
        assert_eq!(coverage_index(&t, 0, 51), Some(4));
        assert_eq!(coverage_index(&t, 0, 13), None);
    }

    #[test]
    fn a_glyph_no_class_definition_mentions_is_in_class_zero() {
        // Format 1 covering 10..=12 with classes 1, 2, 1.
        let mut t = vec![];
        t.extend(be16(1));
        t.extend(be16(10));
        t.extend(be16(3));
        t.extend(be16(1));
        t.extend(be16(2));
        t.extend(be16(1));
        assert_eq!(glyph_class(&t, 0, 10), Some(1));
        assert_eq!(glyph_class(&t, 0, 11), Some(2));
        // Class 0 is a real class the grid can kern, so "not listed" must not
        // become "no answer" — that would silently drop every pair whose
        // second glyph is unclassed.
        assert_eq!(glyph_class(&t, 0, 9), Some(0));
        assert_eq!(glyph_class(&t, 0, 13), Some(0));
    }

    #[test]
    fn class_definition_format_2_reads_its_ranges() {
        let mut t = vec![];
        t.extend(be16(2));
        t.extend(be16(2));
        t.extend(be16(10));
        t.extend(be16(19));
        t.extend(be16(3));
        t.extend(be16(30));
        t.extend(be16(30));
        t.extend(be16(7));
        assert_eq!(glyph_class(&t, 0, 15), Some(3));
        assert_eq!(glyph_class(&t, 0, 30), Some(7));
        assert_eq!(glyph_class(&t, 0, 25), Some(0));
    }

    #[test]
    fn a_value_records_size_is_two_bytes_per_set_flag() {
        assert_eq!(value_size(0), 0);
        // XAdvance alone.
        assert_eq!(value_size(0x0004), 2);
        // XPlacement + YPlacement + XAdvance.
        assert_eq!(value_size(0x0007), 6);
        assert_eq!(value_size(0xFFFF), 32);
    }
}
