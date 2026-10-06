//! Character names: every emoji, in CLDR's order with its name, group and
//! keywords, and the names of the characters a picker offers besides emoji --
//! symbols, maths, arrows, currency, Latin, Greek and Cyrillic letters -- with
//! a search over both.
//!
//! # Why this crate exists
//!
//! `roadmap-detailed.md` asks for a Unicode selection dialog -- "a picker
//! with category tabs (Smileys & Emotion, People, Animals, Symbols, Math,
//! Arrows, Currency, Latin/Greek/Cyrillic, etc.) and a live search box (match
//! by character name, keyword, or codepoint, e.g. 'shrug', 'U+00E9', 'arrow
//! right')" -- for a shortcut that types a character, for the tray's emoji
//! entry and for programs. A picker needs the characters' names and
//! keywords, and nothing on the image has them. They are generated into this
//! crate from the Unicode Consortium's own files (`gen.py`: emoji-test.txt,
//! UnicodeData.txt and CLDR's English annotations), so a picker opens at once
//! and offline, and only a program that offers one links them.
//!
//! # Skin tones
//!
//! An emoji that comes in skin tones is listed once, untoned; [`Found::in_tone`]
//! gives its variant in one of the five [`SkinTone`]s. A picker offers each
//! emoji once and draws it in the tone its user chose, rather than every hand
//! six times. A sequence of two people in two different tones is not offered:
//! one tone choice cannot name it.
//!
//! # Searching
//!
//! [`search`] takes words, a character itself, or a code point (`U+00E9`,
//! `u+1f600`). A character matches where every word of the query starts a
//! word of its name or of its keywords, in any order and any case: "arrow
//! right" finds RIGHTWARDS ARROW, "shrug" the person shrugging, "money" the
//! dollar sign by its keyword. What it finds comes best first -- a name that
//! is the query, then names that match, then what matched only by a keyword --
//! and within each, emoji in CLDR's order before characters by code point.

mod emoji_table;
mod names_table;

pub use emoji_table::VERSION;

use emoji_table::NO_TONES;

/// A skin tone an emoji can be drawn in: the five of the Fitzpatrick scale
/// Unicode's modifiers stand for, light to dark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SkinTone {
    /// U+1F3FB, Fitzpatrick types 1 and 2.
    Light,
    /// U+1F3FC, type 3.
    MediumLight,
    /// U+1F3FD, type 4.
    Medium,
    /// U+1F3FE, type 5.
    MediumDark,
    /// U+1F3FF, type 6.
    Dark,
}

impl SkinTone {
    /// Every tone, light to dark.
    pub const ALL: [Self; 5] = [
        Self::Light,
        Self::MediumLight,
        Self::Medium,
        Self::MediumDark,
        Self::Dark,
    ];

    /// Its name as CLDR words it: "medium-light".
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::MediumLight => "medium-light",
            Self::Medium => "medium",
            Self::MediumDark => "medium-dark",
            Self::Dark => "dark",
        }
    }

    /// The modifier character that sets it, U+1F3FB to U+1F3FF -- which an
    /// emoji face draws alone as a swatch of the tone.
    #[must_use]
    pub const fn modifier(self) -> char {
        match self {
            Self::Light => '\u{1F3FB}',
            Self::MediumLight => '\u{1F3FC}',
            Self::Medium => '\u{1F3FD}',
            Self::MediumDark => '\u{1F3FE}',
            Self::Dark => '\u{1F3FF}',
        }
    }

    /// Its place in [`ALL`](Self::ALL), and in each row of the toned table.
    const fn index(self) -> usize {
        match self {
            Self::Light => 0,
            Self::MediumLight => 1,
            Self::Medium => 2,
            Self::MediumDark => 3,
            Self::Dark => 4,
        }
    }
}

/// One emoji, untoned, with what it is filed under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Emoji {
    /// What it is drawn from: one character, or a sequence joined as one
    /// (a family, a flag).
    pub text: &'static str,
    /// Its name, as CLDR gives it: "grinning face".
    pub name: &'static str,
    /// Its group's index in [`groups`].
    pub group: usize,
    /// Its subgroup's name: "face-smiling".
    pub subgroup: &'static str,
    /// Its row of skin-toned variants, or [`NO_TONES`].
    tones: u16,
}

impl Emoji {
    /// It as something a picker offers.
    #[must_use]
    pub fn found(self) -> Found {
        Found {
            text: self.text,
            name: self.name,
            tones: self.tones,
        }
    }
}

/// The emoji groups, in order: "Smileys & Emotion", "People & Body" ...
#[must_use]
pub fn groups() -> &'static [&'static str] {
    &emoji_table::GROUPS
}

/// The emoji table's row `row` as an [`Emoji`].
fn emoji_at(row: &'static (&'static str, &'static str, u8, &'static str, u16)) -> Emoji {
    let &(text, name, sg, _, tones) = row;
    let (group, subgroup) = emoji_table::SUBGROUPS
        .get(usize::from(sg))
        .map_or((0, ""), |&(g, s)| (usize::from(g), s));
    Emoji {
        text,
        name,
        group,
        subgroup,
        tones,
    }
}

/// Every emoji once, untoned, in CLDR's order -- the order a keyboard's
/// palette shows them in.
pub fn emoji() -> impl Iterator<Item = Emoji> {
    emoji_table::EMOJI.iter().map(emoji_at)
}

/// The emoji in group `group`, in order.
pub fn emoji_in(group: usize) -> impl Iterator<Item = Emoji> {
    emoji().filter(move |e| e.group == group)
}

/// The categories of characters besides emoji, in order: "Symbols",
/// "Math", "Arrows", "Currency", "Latin", "Greek", "Cyrillic".
pub fn categories() -> impl Iterator<Item = &'static str> {
    names_table::CATEGORIES.iter().map(|(name, _)| *name)
}

/// What a picker offers: an emoji -- in a skin tone or none -- or a named
/// character, with its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    /// The text to insert: one character, or an emoji's sequence.
    pub text: &'static str,
    /// Its name.
    pub name: &'static str,
    /// Its row of skin-toned variants, or [`NO_TONES`].
    tones: u16,
}

impl Found {
    /// A named character: one that comes in no tones.
    const fn plain(text: &'static str, name: &'static str) -> Self {
        Self {
            text,
            name,
            tones: NO_TONES,
        }
    }

    /// Whether it comes in skin tones.
    #[must_use]
    pub fn has_tones(&self) -> bool {
        self.tones != NO_TONES
    }

    /// It in skin tone `tone` -- its text and name the variant's -- or
    /// itself, untouched, if it comes in none.
    #[must_use]
    pub fn in_tone(self, tone: SkinTone) -> Self {
        emoji_table::TONED
            .get(usize::from(self.tones))
            .and_then(|row| row.get(tone.index()))
            .map_or(self, |&(text, name)| Self {
                text,
                name,
                tones: self.tones,
            })
    }
}

/// The named characters in the category called `category`, by code point.
pub fn characters_in(category: &str) -> impl Iterator<Item = Found> {
    let ranges: &'static [(u32, u32)] = names_table::CATEGORIES
        .iter()
        .find(|(name, _)| *name == category)
        .map_or(&[], |(_, r)| r);
    names_table::NAMES
        .iter()
        .filter(move |(cp, ..)| ranges.iter().any(|(lo, hi)| cp >= lo && cp <= hi))
        .map(|&(_, text, name, _)| Found::plain(text, name))
}

/// `text` with any emoji presentation selector (U+FE0F) taken out: what an
/// emoji typed with or without one has in common.
fn bare(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().filter(|&c| c != '\u{fe0f}')
}

/// What `text` is -- one character or an emoji's sequence, in a skin tone
/// or none, with its presentation selector or without: an emoji, else a
/// named character -- if it is anything this crate knows.
#[must_use]
pub fn lookup(text: &str) -> Option<Found> {
    let same = |t: &str| bare(t).eq(bare(text));
    if let Some(&(t, name, _, _, tones)) = emoji_table::EMOJI.iter().find(|row| same(row.0)) {
        return Some(Found {
            text: t,
            name,
            tones,
        });
    }
    for (row, variants) in (0u16..).zip(emoji_table::TONED.iter()) {
        if let Some(&(t, name)) = variants.iter().find(|&&(t, _)| same(t)) {
            return Some(Found {
                text: t,
                name,
                tones: row,
            });
        }
    }
    let mut chars = text.chars();
    let c = chars.next().filter(|_| chars.next().is_none())?;
    names_table::NAMES
        .binary_search_by_key(&u32::from(c), |&(cp, ..)| cp)
        .ok()
        .and_then(|i| names_table::NAMES.get(i))
        .map(|&(_, text, name, _)| Found::plain(text, name))
}

/// The name of `c`, among the characters this knows: an emoji's CLDR name,
/// else its Unicode name.
#[must_use]
pub fn name_of(c: char) -> Option<&'static str> {
    let mut buf = [0u8; 4];
    lookup(c.encode_utf8(&mut buf)).map(|found| found.name)
}

/// The name of `text` -- one character, or an emoji's sequence in a skin
/// tone or none -- among those this knows.
#[must_use]
pub fn name_of_text(text: &str) -> Option<&'static str> {
    lookup(text).map(|found| found.name)
}

/// The words of `text`: its runs of letters and digits. A query and a name
/// are cut the same way, so "o'clock" typed finds "one o’clock" whatever
/// apostrophe either has.
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
}

/// Whether `word` starts with `prefix`, ignoring case; `prefix` is lower
/// case already.
fn starts_with(word: &str, prefix: &str) -> bool {
    let mut lower = word.chars().flat_map(char::to_lowercase);
    prefix.chars().all(|p| lower.next() == Some(p))
}

/// Whether `word` is `query_word`, ignoring case; `query_word` is lower
/// case already.
fn same_word(word: &str, query_word: &str) -> bool {
    word.chars().flat_map(char::to_lowercase).eq(query_word.chars())
}

/// How well an entry named `name`, with `keywords`, matches the query's
/// lower-case `query` words: 0 for a name that is the query, 1 for a name
/// every word of the query starts a word of, 2 for a match that needed a
/// keyword -- and `None` for no match.
fn rank(name: &str, keywords: &str, query: &[String]) -> Option<u8> {
    let in_name = |q: &String| words(name).any(|w| starts_with(w, q));
    if query.iter().all(in_name) {
        let whole = words(name).count() == query.len()
            && words(name).zip(query).all(|(w, q)| same_word(w, q));
        return Some(if whole { 0 } else { 1 });
    }
    let in_either = |q: &String| in_name(q) || words(keywords).any(|w| starts_with(w, q));
    query.iter().all(in_either).then_some(2)
}

/// The character a query names as a code point, `U+XXXX` or `u+xxxx`: any
/// but a control character, whether this crate knows its name or not.
#[must_use]
pub fn code_point(query: &str) -> Option<char> {
    let hex = query
        .trim()
        .strip_prefix("U+")
        .or_else(|| query.trim().strip_prefix("u+"))?;
    if hex.is_empty() || hex.len() > 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16)
        .ok()
        .and_then(char::from_u32)
        .filter(|c| !c.is_control())
}

/// Everything `query` finds, best first (see the module's "Searching"): a
/// code point (`U+00E9`) or a character typed finds that one; words find by
/// the starts of names' and keywords' words, in any order. An empty query
/// finds nothing, and so does a code point this crate has no name for --
/// [`code_point`] still reads it.
#[must_use]
pub fn search(query: &str) -> Vec<Found> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    if let Some(c) = code_point(query) {
        let mut buf = [0u8; 4];
        return lookup(c.encode_utf8(&mut buf)).into_iter().collect();
    }
    if let Some(found) = lookup(query) {
        return vec![found];
    }
    let query: Vec<String> = words(query).map(str::to_lowercase).collect();
    if query.is_empty() {
        return Vec::new();
    }
    let mut ranked: Vec<(u8, Found)> = emoji_table::EMOJI
        .iter()
        .filter_map(|&(text, name, _, keywords, tones)| {
            let found = Found { text, name, tones };
            rank(name, keywords, &query).map(|r| (r, found))
        })
        .collect();
    // A character that is an emoji too -- the smiling face -- is found once,
    // as the emoji.
    let emoji_found: std::collections::HashSet<String> = ranked
        .iter()
        .map(|(_, f)| bare(f.text).collect())
        .collect();
    ranked.extend(
        names_table::NAMES
            .iter()
            .filter(|(_, text, ..)| !emoji_found.contains(*text))
            .filter_map(|&(_, text, name, keywords)| {
                rank(name, keywords, &query).map(|r| (r, Found::plain(text, name)))
            }),
    );
    // Stable: within a rank, emoji in CLDR's order and then characters by
    // code point, as they were gathered.
    ranked.sort_by_key(|&(r, _)| r);
    ranked.into_iter().map(|(_, found)| found).collect()
}

#[cfg(test)]
mod tests;
