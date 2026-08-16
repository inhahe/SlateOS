//! What language a run is in, in the form a font can answer.
//!
//! A font can hold rules that apply to one language and not another written in
//! the same script. Turkish spells the lowercase of `I` as a dotless `ı`, so a
//! Turkish `fi` must not ligate the way an English one does. Serbian Cyrillic
//! draws italic `б`, `г`, `д`, `п` and `т` with entirely different shapes from
//! Russian's. Romanian wants `ș` and `ț` with a comma below rather than the
//! cedilla the Unicode chart shows. Each of these is one font, one script, and
//! two answers, and OpenType expresses it by letting a script table hold named
//! *language systems* beside its default one.
//!
//! This module is the lookup that turns what the caller knows — a BCP 47 tag
//! like `tr`, `sr-Cyrl`, `ro-MD` — into the four-byte tag the font filed those
//! rules under. [`otl::select`](crate::otl) does the rest.
//!
//! # The mapping is a registry
//!
//! Nothing derives `TRK ` from `tr`, or `DEU ` from `de`, or `IRI ` from `ga`.
//! It is a table of about eleven hundred entries maintained by Microsoft, and
//! the only defensible way to have it is to take it from the same place the
//! shaper this crate measures itself against takes it. So
//! [`lang_tables`](crate::lang_tables) is generated from HarfBuzz's
//! `hb-ot-tag-table.hh` by `tools/gen_lang_tables.py`, and the resolution order
//! below is HarfBuzz's `hb_ot_tags_from_language` step for step.
//!
//! # What a language does *not* do
//!
//! It never changes which script a run is shaped as, and it never moves the
//! script fallback chain along. A face that registers `latn` and no `TRK `
//! under it shapes Turkish with `latn`'s default rules — it has said its Latin
//! text does not vary by language, and that is an answer. See
//! [`otl`](crate::otl)'s "Language selection".
//!
//! Nor is it a hint about the text's content: the shaper does not read it to
//! decide word boundaries, casing or anything else. Its entire effect is which
//! LangSysRecord a face's feature walk starts from.
//!
//! # Not knowing is normal, and is the default
//!
//! [`ScaledFont::shape`](crate::scaled::ScaledFont::shape) names no language,
//! and that is the right thing when the caller genuinely does not know one: it
//! selects each script's default language system, which is what the font's
//! author intended for text they were not told about. A *wrong* language is
//! worse than none — shaping English as Turkish suppresses `fi` — so a caller
//! that is guessing should not guess.

use crate::lang_tables::{BLOCKED_3, COMPLEX, LANGUAGES_2, LANGUAGES_3, Rule};

/// An OpenType language system tag: four bytes, space-padded, as a font spells
/// it in a LangSysRecord.
///
/// Constructed from a BCP 47 language tag with [`Lang::new`], which is the only
/// way in from outside this crate — the point of the type is that a
/// `[u8; 4]` that reached [`otl`](crate::otl) came through the registry rather
/// than from a caller inventing one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lang([u8; 4]);

impl Lang {
    /// The OpenType language system a BCP 47 tag selects, or `None` for a tag
    /// no font could have filed anything under.
    ///
    /// Case and the underscore spelling do not matter: `tr`, `TR`, `tr-TR` and
    /// `tr_TR` are one answer. Neither does anything after the parts that
    /// count — `en-GB-oxendict` resolves as `en` does, since no font registers
    /// a language system for a dictionary convention.
    ///
    /// The order is HarfBuzz's, and each step is there because the one before
    /// it cannot answer:
    ///
    /// 1. **The whole tag**, against the rules that need more than a first
    ///    subtag. `ro-MD` is Moldavian and not Romanian, `zh-Hant` is
    ///    Traditional and not Simplified, and anything marked `-fonipa` is the
    ///    IPA rather than the language being transcribed.
    /// 2. **The primary subtag**, in the two- and three-letter registries.
    ///    An extended language subtag replaces it first, so `zh-yue` resolves
    ///    as Cantonese rather than as Chinese.
    /// 3. **Uppercased**, for a three-letter subtag in neither registry. An
    ///    unregistered ISO 639-3 code and its OpenType tag are overwhelmingly
    ///    the same letters in a different case, so this is right far more often
    ///    than `None` would be — except for the codes where the uppercased form
    ///    is *already another language's* tag, which HarfBuzz lists and which
    ///    are skipped.
    ///
    /// # Examples
    ///
    /// ```
    /// use osfont::lang::Lang;
    /// assert_eq!(Lang::new("tr"), Lang::new("tr-TR"));
    /// assert_ne!(Lang::new("ro"), Lang::new("ro-MD"));
    /// assert_eq!(Lang::new(""), None);
    /// ```
    #[must_use]
    pub fn new(bcp47: &str) -> Option<Self> {
        let s = bcp47.as_bytes();
        if s.is_empty() {
            return None;
        }
        if let Some(tag) = complex(s) {
            return Some(Self(tag));
        }
        let (at, len) = primary(s);
        let sub = s.get(at..at.checked_add(len)?)?;
        match len {
            2 => search(LANGUAGES_2, sub).map(Self),
            3 => match search(LANGUAGES_3, sub) {
                Some(tag) => Some(Self(tag)),
                // Not registered under this spelling. Assume ISO 639-3 and
                // uppercase it, unless that would name a different language.
                None if find(BLOCKED_3, &padded(sub)).is_none() => uppercase(sub).map(Self),
                None => None,
            },
            _ => None,
        }
    }

    /// The tag as a font spells it.
    #[must_use]
    pub fn tag(self) -> [u8; 4] {
        self.0
    }

    /// A tag read out of a font's LangSysRecord.
    ///
    /// Not public, and deliberately: outside this crate a `Lang` means "the
    /// registry says this", and a font is allowed to register a tag the
    /// registry has never heard of. [`ByScript::parse`](crate::otl::ByScript)
    /// needs to key its selections by whatever the face actually wrote, which
    /// is why it may build one this way.
    pub(crate) fn from_tag(tag: [u8; 4]) -> Self {
        Self(tag)
    }
}

/// The rules that need more than a first subtag, tried in HarfBuzz's order.
///
/// First match wins, so the order in [`COMPLEX`] is part of the data and not a
/// detail of how it was generated.
fn complex(s: &[u8]) -> Option<[u8; 4]> {
    // HarfBuzz's own guards: nothing here can match a tag with no `-`, and the
    // variant rules are skipped outright for tags too short to hold one. They
    // are reproduced rather than dropped because they are observable — `x-geok`
    // is six bytes and HarfBuzz does *not* read `-geok` out of it.
    let dash = s.iter().position(|&b| norm(b) == b'-')?;
    let long_enough = s.len() >= 7 && s.len().checked_sub(dash).is_some_and(|n| n >= 5);
    for &(rule, tag) in COMPLEX {
        let matched = match rule {
            Rule::Variant(sub) => long_enough && has_subtag(s, sub),
            Rule::Exact(key) => equals(s, key),
            Rule::Prefix(key) => prefix_of(s, key).is_some_and(|n| boundary_at(s, n)),
            Rule::PrefixVariant(key, sub) => prefix_of(s, key).is_some() && has_subtag(s, sub),
        };
        if matched {
            return Some(tag);
        }
    }
    None
}

/// Where the subtag that names the language starts, and how long it is.
///
/// Normally the first component, but an *extended language subtag* replaces it:
/// `zh-yue` is Cantonese, not Chinese written somehow. The test for one is
/// HarfBuzz's — a second component of exactly three characters, the first of
/// them a letter, in a tag of at least six bytes — and it is a test on shape
/// rather than on the registry, so `en-abc` is read the same way.
fn primary(s: &[u8]) -> (usize, usize) {
    let mut at = 0;
    if s.len() >= 6
        && let Some(dash) = s.iter().position(|&b| norm(b) == b'-')
    {
        let rest = dash.saturating_add(1);
        let extlang = s
            .get(rest..)
            .map_or(0, |t| t.iter().position(|&b| norm(b) == b'-').unwrap_or(t.len()));
        if extlang == 3 && s.get(rest).is_some_and(u8::is_ascii_alphabetic) {
            at = rest;
        }
    }
    let len = s
        .get(at..)
        .map_or(0, |t| t.iter().position(|&b| norm(b) == b'-').unwrap_or(t.len()));
    (at, len)
}

/// `sub` padded to four bytes with spaces, which is how the tables key them.
///
/// A subtag longer than four bytes cannot be a key and is truncated to one
/// that is not either: the callers only ever pass two or three bytes, and the
/// tables hold nothing of any other length, so a mismatch is the correct
/// outcome rather than a case to handle.
fn padded(sub: &[u8]) -> [u8; 4] {
    let mut out = [b' '; 4];
    for (dst, &src) in out.iter_mut().zip(sub.iter()) {
        *dst = norm(src);
    }
    out
}

/// The tag `sub` selects in a sorted subtag-to-tag table.
fn search(table: &[([u8; 4], [u8; 4])], sub: &[u8]) -> Option<[u8; 4]> {
    let key = padded(sub);
    table
        .binary_search_by_key(&key, |&(k, _)| k)
        .ok()
        .and_then(|i| table.get(i))
        .map(|&(_, tag)| tag)
}

/// Where `key` sits in a sorted list of tags, if it is there.
fn find(table: &[[u8; 4]], key: &[u8; 4]) -> Option<usize> {
    table.binary_search(key).ok()
}

/// A three-letter subtag as its own uppercase, space-padded — the fallback for
/// an ISO 639-3 code the registry does not list.
///
/// `None` for anything not three ASCII letters. HarfBuzz would uppercase the
/// bytes whatever they are, by clearing one bit, which turns a digit into a
/// control character and produces a tag no font can hold; refusing is the same
/// answer reached without writing rubbish into a tag.
fn uppercase(sub: &[u8]) -> Option<[u8; 4]> {
    let mut out = [b' '; 4];
    if sub.len() != 3 {
        return None;
    }
    for (dst, &src) in out.iter_mut().zip(sub.iter()) {
        if !src.is_ascii_alphabetic() {
            return None;
        }
        *dst = src.to_ascii_uppercase();
    }
    Some(out)
}

/// BCP 47 is case-insensitive, and the underscore spelling is what POSIX locale
/// names use — `tr_TR` must resolve as `tr-TR` does.
///
/// Applied at every comparison rather than by rewriting the string first, so
/// that this stays allocation-free and imposes no length limit: a normalizing
/// pass into a fixed buffer would have to decide what to do with a tag longer
/// than the buffer, and every answer to that is wrong.
fn norm(b: u8) -> u8 {
    if b == b'_' {
        b'-'
    } else {
        b.to_ascii_lowercase()
    }
}

/// Whether the whole tag is `key`.
fn equals(s: &[u8], key: &str) -> bool {
    s.len() == key.len() && prefix_of(s, key).is_some()
}

/// How long `key` is, if the tag starts with it. `None` if it does not.
fn prefix_of(s: &[u8], key: &str) -> Option<usize> {
    let k = key.as_bytes();
    let head = s.get(..k.len())?;
    head.iter()
        .zip(k.iter())
        .all(|(&a, &b)| norm(a) == b)
        .then_some(k.len())
}

/// Whether position `n` ends a subtag: the end of the tag, or a separator.
fn boundary_at(s: &[u8], n: usize) -> bool {
    s.get(n).is_none_or(|&b| norm(b) == b'-')
}

/// Whether `sub` — which always begins with `-` — appears as a whole subtag.
///
/// HarfBuzz's `subtag_matches`: the match must not be a prefix of a longer
/// subtag, so `-syr` does not match `sr-syre`, but it may be followed by
/// another `-` and more components. Searched from the front because the needle
/// starts with a separator, which cannot occur before the tag's first one.
fn has_subtag(s: &[u8], sub: &str) -> bool {
    let n = sub.len();
    let Some(last) = s.len().checked_sub(n) else {
        return false;
    };
    (0..=last).any(|i| {
        s.get(i..)
            .and_then(|t| prefix_of(t, sub))
            .is_some_and(|_| s.get(i.saturating_add(n)).is_none_or(|b| !b.is_ascii_alphanumeric()))
    })
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

    fn tag(s: &str) -> Option<[u8; 4]> {
        Lang::new(s).map(Lang::tag)
    }

    #[test]
    fn the_two_letter_registry_answers() {
        assert_eq!(tag("tr"), Some(*b"TRK "));
        assert_eq!(tag("sr"), Some(*b"SRB "));
        assert_eq!(tag("de"), Some(*b"DEU "));
        assert_eq!(tag("ro"), Some(*b"ROM "));
        assert_eq!(tag("ca"), Some(*b"CAT "));
        assert_eq!(tag("he"), Some(*b"IWR "));
    }

    #[test]
    fn case_and_separator_do_not_matter() {
        assert_eq!(tag("TR"), Some(*b"TRK "));
        assert_eq!(tag("tr-TR"), Some(*b"TRK "));
        assert_eq!(tag("tr_TR"), Some(*b"TRK "));
        assert_eq!(tag("Tr-Latn-TR"), Some(*b"TRK "));
    }

    #[test]
    fn a_region_can_change_the_language() {
        // The whole point of consulting the complex rules first: Moldavian is
        // Romanian plus a region, and fonts file the two separately.
        assert_eq!(tag("ro"), Some(*b"ROM "));
        assert_eq!(tag("ro-MD"), Some(*b"MOL "));
        assert_eq!(tag("ro-RO"), Some(*b"ROM "));
    }

    #[test]
    fn a_script_can_change_the_language() {
        assert_eq!(tag("zh-Hans"), Some(*b"ZHS "));
        assert_eq!(tag("zh-Hant"), Some(*b"ZHT "));
        assert_eq!(tag("zh-Hant-HK"), Some(*b"ZHH "));
    }

    #[test]
    fn a_variant_can_replace_the_language_entirely() {
        // `-fonipa` says the text is a phonetic transcription, and a font's IPA
        // rules have nothing to do with the language being transcribed.
        assert_eq!(tag("en-fonipa"), Some(*b"IPPH"));
        assert_eq!(tag("und-fonipa"), Some(*b"IPPH"));
        assert_eq!(tag("en-fonnapa"), Some(*b"APPH"));
        assert_eq!(tag("el-polyton"), Some(*b"PGR "));
    }

    #[test]
    fn a_variant_must_be_a_whole_subtag() {
        // `subtag_matches` refuses a prefix of a longer subtag, which is what
        // stops `-syrn` matching inside `-syrnx`.
        assert_ne!(tag("syr-syrnx"), Some(*b"SYRN"));
        assert_eq!(tag("syr-syrn"), Some(*b"SYRN"));
    }

    #[test]
    fn an_extended_language_subtag_replaces_the_primary_one() {
        assert_eq!(tag("zh-yue"), tag("yue"));
        assert_ne!(tag("zh-yue"), tag("zh"));
    }

    #[test]
    fn an_unregistered_three_letter_code_is_uppercased() {
        // Not in HarfBuzz's tables, so the ISO 639-3 fallback applies. If the
        // registry ever gains it this test is the thing that notices.
        assert!(LANGUAGES_3.binary_search_by_key(b"zzz ", |&(k, _)| k).is_err());
        assert_eq!(tag("zzz"), Some(*b"ZZZ "));
    }

    #[test]
    fn a_blocked_three_letter_code_is_not_uppercased() {
        // `afk` is a language of Papua New Guinea; `AFK ` is Afrikaans. Guessing
        // here would shape one as the other.
        assert!(BLOCKED_3.binary_search(b"afk ").is_ok());
        assert_eq!(tag("afk"), None);
    }

    #[test]
    fn nothing_that_cannot_be_a_language_resolves() {
        assert_eq!(tag(""), None);
        assert_eq!(tag("-"), None);
        assert_eq!(tag("x"), None);
        assert_eq!(tag("toolongtobealanguage"), None);
        assert_eq!(tag("12"), None);
        assert_eq!(tag("123"), None);
    }

    #[test]
    fn every_generated_tag_is_four_printable_bytes() {
        // A table entry that is not is one no font can register, and would sit
        // in the selection map matching nothing forever.
        let all = LANGUAGES_2
            .iter()
            .chain(LANGUAGES_3.iter())
            .map(|&(_, t)| t)
            .chain(COMPLEX.iter().map(|&(_, t)| t));
        for tag in all {
            assert!(
                tag.iter().all(|&b| (0x20..0x7F).contains(&b)),
                "tag {tag:?} is not printable ASCII"
            );
        }
    }

    #[test]
    fn the_tables_are_sorted_for_binary_search() {
        assert!(LANGUAGES_2.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(LANGUAGES_3.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(BLOCKED_3.windows(2).all(|w| w[0] < w[1]));
    }
}
