//! `--debug`: what upstream's `sort` says about the keys before it sorts, and
//! the underlines it draws beneath every line it writes.
//!
//! Both are transcriptions of `sort.c` -- `key_warnings` for the first, and
//! `debug_line`, `debug_key` and `mark_key` for the second -- and both were
//! measured against GNU coreutils 9.4 under `LC_ALL=C.UTF-8`, the locale every
//! harness of the family runs it in (and SlateOS's, design-decisions §351).
//!
//! An underline marks the bytes a key compares, in columns: it starts as many
//! columns in as the line before the key occupies, and is as many underscores
//! wide as the key is columns wide. A key that compares nothing -- an empty
//! field, a `-n` key with no number in it -- is marked `^ no match for key` at
//! the column it would have started at. Columns are `mbswidth`'s, with each
//! tab counted as one, since a tab is drawn as `>`.

use crate::keydef::KeySpec;
use crate::order::{self, Ignore, is_blank};
use coreutils::mbswidth::mbswidth;
use coreutils::quote::quote;

/// A key's letters, as upstream's `struct keyfield` keeps them: one flag each,
/// whichever of them decided the comparison.
#[derive(Clone, Copy, Default)]
#[allow(clippy::struct_excessive_bools)] // One flag per option letter, as upstream keeps them.
struct Flags {
    skipsblanks: bool,
    skipeblanks: bool,
    ignore: Option<Ignore>,
    translate: bool,
    general: bool,
    human: bool,
    month: bool,
    numeric: bool,
    random: bool,
    reverse: bool,
    version: bool,
}

impl Flags {
    fn of(key: &KeySpec) -> Self {
        Flags {
            skipsblanks: key.skip_start_blanks,
            skipeblanks: key.skip_end_blanks,
            ignore: key.ignore,
            translate: key.fold,
            general: key.named.general,
            human: key.named.human,
            month: key.named.month,
            numeric: key.named.numeric,
            random: key.named.random,
            reverse: key.reverse,
            version: key.named.version,
        }
    }

    /// Upstream's `key_numeric`.
    fn numeric_any(self) -> bool {
        self.numeric || self.general || self.human
    }

    /// `! default_key_compare`: whether these letters order anything. `r`
    /// alone does not.
    fn orders(self) -> bool {
        self.ignore.is_some()
            || self.translate
            || self.skipsblanks
            || self.skipeblanks
            || self.numeric_any()
            || self.month
            || self.version
            || self.random
    }

    /// Upstream's `key_to_opts`: the letters, in its order.
    fn letters(self) -> String {
        [
            (self.skipsblanks || self.skipeblanks, 'b'),
            (self.ignore == Some(Ignore::NonDictionary), 'd'),
            (self.translate, 'f'),
            (self.general, 'g'),
            (self.human, 'h'),
            (self.ignore == Some(Ignore::NonPrinting), 'i'),
            (self.month, 'M'),
            (self.numeric, 'n'),
            (self.random, 'R'),
            (self.reverse, 'r'),
            (self.version, 'V'),
        ]
        .iter()
        .filter(|(set, _)| *set)
        .map(|&(_, letter)| letter)
        .collect()
    }
}

/// What `--debug` needs to know of the command line.
pub struct Settings<'a> {
    /// The keys compared, in order: the `-k` keys after inheritance, or the
    /// one the global options became.
    pub keys: &'a [KeySpec],
    /// The global options, `gkey`.
    pub global: &'a KeySpec,
    /// Whether `keys` is the global options' key alone -- upstream's
    /// `gkey_only`.
    pub gkey_only: bool,
    pub tab: Option<u8>,
    pub stable: bool,
    pub unique: bool,
}

/// The diagnostics `--debug` writes before anything is sorted, each without
/// its `sort: ` prefix: which ordering is in force, then upstream's
/// `key_warnings` -- keys that mean less than they appear to, and options
/// that change nothing.
pub fn notes(s: &Settings<'_>) -> Vec<String> {
    let mut notes = vec![format!(
        "text ordering performed using {} sorting rules",
        quote(b"C.UTF-8")
    )];
    let mut unused = Flags::of(s.global);
    let (mut basic, mut general) = (false, false);
    let (mut basic_span, mut general_span) = (false, false);
    for (number, key) in (1usize..).zip(s.keys) {
        let f = Flags::of(key);
        if f.numeric_any() {
            if f.general {
                general = true;
            } else {
                basic = true;
            }
        }
        if key.traditional {
            notes.push(obsolescent(key));
        }
        let zero_width = matches!((key.sword, key.eword), (Some(s), Some(e)) if e < s);
        if zero_width {
            notes.push(format!("key {number} has zero width and will be ignored"));
        }
        let implicit_skip = f.numeric_any() || f.month;
        let line_offset = key.eword == Some(0) && key.echar != 0;
        // Upstream's `(!skipsblanks && !implicit_skip) || (!skipsblanks &&
        // schar) || (!skipeblanks && echar)`, with its first two terms
        // joined.
        if !zero_width
            && !s.gkey_only
            && s.tab.is_none()
            && !line_offset
            && ((!f.skipsblanks && (!implicit_skip || key.schar != 0))
                || (!f.skipeblanks && key.echar != 0))
        {
            notes.push(format!(
                "leading blanks are significant in key {number}; consider also specifying 'b'"
            ));
        }
        if !s.gkey_only && f.numeric_any() {
            // Upstream's arithmetic: a missing start or end is `SIZE_MAX`,
            // which the `+ 1` wraps to zero.
            let start = key.sword.map_or(1, |w| w.wrapping_add(1).max(1));
            let end = key.eword.map_or(0, |w| w.wrapping_add(1));
            if end == 0 || start < end {
                notes.push(format!("key {number} is numeric and spans multiple fields"));
                if f.general {
                    general_span = true;
                } else {
                    basic_span = true;
                }
            }
        }
        // The global options a key took, or named for itself, are not unused.
        if unused.ignore.is_some() && unused.ignore == f.ignore {
            unused.ignore = None;
        }
        unused.translate &= !f.translate;
        unused.skipsblanks &= !f.skipsblanks;
        unused.skipeblanks &= !f.skipeblanks;
        unused.month &= !f.month;
        unused.numeric &= !f.numeric;
        unused.general &= !f.general;
        unused.human &= !f.human;
        unused.random &= !f.random;
        unused.version &= !f.version;
        unused.reverse &= !f.reverse;
    }
    // `C.UTF-8` has no thousands separator, so only a separator that is the
    // decimal point, a minus or a plus can be mistaken for part of a number.
    let mut warned = false;
    if basic_span || general_span {
        match s.tab {
            Some(b'.') => {
                notes.push(format!(
                    "field separator {} is treated as a decimal point in numbers",
                    quote(b".")
                ));
                warned = true;
            }
            Some(b'-') => notes.push(format!(
                "field separator {} is treated as a minus sign in numbers",
                quote(b"-")
            )),
            Some(b'+') if general_span => notes.push(format!(
                "field separator {} is treated as a plus sign in numbers",
                quote(b"+")
            )),
            _ => {}
        }
    }
    if (basic || general) && !warned {
        notes.push(format!(
            "{}numbers use {} as a decimal point in this locale",
            if s.tab == Some(b'.') { "" } else { "note " },
            quote(b".")
        ));
    }
    let keyed = !s.keys.is_empty();
    let quiet_last_resort = s.stable || s.unique;
    if unused.orders() || (unused.reverse && quiet_last_resort && keyed) {
        let mut listed = unused;
        if !quiet_last_resort {
            listed.reverse = false;
        }
        let letters = listed.letters();
        notes.push(if letters.chars().count() == 1 {
            format!("option '-{letters}' is ignored")
        } else {
            format!("options '-{letters}' are ignored")
        });
    }
    if unused.reverse && !quiet_last_resort && keyed {
        notes.push("option '-r' only applies to last-resort comparison".to_string());
    }
    notes
}

/// `obsolescent key '+A -B' used; consider '-k A+1,B' instead`, rebuilt from
/// the key as upstream rebuilds it -- from its numbers, so a character offset
/// is not shown, `+01` reads `+1`, and an end offset bumps the suggested end.
fn obsolescent(key: &KeySpec) -> String {
    let start = key.sword.unwrap_or(0);
    let mut old = format!("+{start}");
    let mut new = format!("-k {}", start.wrapping_add(1));
    if let Some(end) = key.eword {
        old.push_str(&format!(" -{}", end.wrapping_add(1)));
        new.push_str(&format!(
            ",{}",
            end.wrapping_add(1)
                .wrapping_add(usize::from(key.echar == usize::MAX))
        ));
    }
    format!(
        "obsolescent key {} used; consider {} instead",
        quote(old.as_bytes()),
        quote(new.as_bytes())
    )
}

/// One line as `--debug` writes it, terminator excluded from `line`: the
/// line with each tab drawn as `>` and a newline for its terminator --
/// whichever it was, so `-z`'s NUL too -- then an underline for each key,
/// then one for the whole line, the last-resort comparison, unless `-s` or
/// `-u` turned that off. With no key there is only the whole line's.
pub fn annotate(
    out: &mut Vec<u8>,
    line: &[u8],
    keys: &[KeySpec],
    tab: Option<u8>,
    last_resort: bool,
) {
    out.extend(line.iter().map(|&c| if c == b'\t' { b'>' } else { c }));
    out.push(b'\n');
    if keys.is_empty() {
        underline(out, line, None, tab);
        return;
    }
    for key in keys {
        underline(out, line, Some(key), tab);
    }
    if last_resort {
        underline(out, line, None, tab);
    }
}

/// Upstream's `debug_key`: the underline of one key, or of the whole line.
///
/// Where the key starts and ends is `begfield` and `limfield`, unclamped.
/// A numeric or month key, or a line-start key that skips blanks, is then
/// tightened: its blanks skipped, and its end brought in to where its number
/// or month name ends -- and to nothing at all, `^ no match for key`, when it
/// has none.
fn underline(out: &mut Vec<u8>, line: &[u8], key: Option<&KeySpec>, tab: Option<u8>) {
    let mut beg = 0usize;
    let mut lim = line.len();
    if let Some(key) = key {
        if key.sword.is_some() {
            beg = key.begfield(line, tab);
        }
        if key.eword.is_some() {
            lim = key.limfield(line, tab);
        }
        let f = Flags::of(key);
        if (f.skipsblanks && key.sword.is_none()) || f.month || f.numeric_any() {
            // Upstream ends the key with a NUL at `lim` before it scans, so
            // the scans stop there -- unless the key starts past it, where
            // they run on to the line's own end.
            let stop = if beg <= lim { lim } else { line.len() };
            while beg < stop && line.get(beg).copied().is_some_and(is_blank) {
                beg = beg.saturating_add(1);
            }
            if beg <= lim {
                let text = line.get(beg..lim).unwrap_or_default();
                let len = if f.month {
                    order::month_end(text)
                } else if f.general {
                    order::general_end(text)
                } else if f.numeric || f.human {
                    order::number_end(text, f.human).unwrap_or(0)
                } else {
                    text.len()
                };
                lim = beg.saturating_add(len);
            }
        }
    }
    let offset = columns(line.get(..beg).unwrap_or_default());
    let width = if beg < lim {
        columns(line.get(beg..lim).unwrap_or_default())
    } else {
        0
    };
    out.extend(std::iter::repeat_n(b' ', offset));
    if width == 0 {
        out.extend_from_slice(b"^ no match for key\n");
    } else {
        out.extend(std::iter::repeat_n(b'_', width));
        out.push(b'\n');
    }
}

/// Upstream's `debug_width`: the columns `bytes` occupies, a tab counted as
/// one -- it is drawn as `>`.
fn columns(bytes: &[u8]) -> usize {
    // `naive_bytecount` wants the `bytecount` crate, which the coreutils do
    // not take on for a count over one line.
    #[allow(clippy::naive_bytecount)]
    let tabs = bytes.iter().filter(|&&c| c == b'\t').count();
    mbswidth(bytes).saturating_add(tabs)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::keydef::parse_key;

    fn drawn(line: &str, keys: &[&str], last_resort: bool) -> String {
        let keys: Vec<KeySpec> = keys
            .iter()
            .map(|k| parse_key(k.as_bytes()).unwrap())
            .collect();
        let mut out = Vec::new();
        annotate(&mut out, line.as_bytes(), &keys, None, last_resort);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn a_line_is_underlined_key_by_key_then_whole() {
        // Measured: GNU's `sort --debug -k2,2` on ` e 2`.
        assert_eq!(drawn(" e 2", &["2,2"], true), " e 2\n  __\n____\n");
        // `-s` and `-u` turn the last resort off, and its underline with it.
        assert_eq!(drawn("a 10", &["2,2"], false), "a 10\n ___\n");
    }

    #[test]
    fn a_key_that_compares_nothing_says_so() {
        assert_eq!(
            drawn("b 2", &["3,3"], false),
            "b 2\n   ^ no match for key\n"
        );
        // `-n` on a line with no number at the key's start.
        assert_eq!(drawn(" e 2", &["1n"], false), " e 2\n ^ no match for key\n");
    }

    #[test]
    fn a_numeric_key_is_tightened_to_its_number() {
        assert_eq!(drawn("d  3", &["2,2n"], false), "d  3\n   _\n");
        assert_eq!(drawn("1.5K", &["1h"], false), "1.5K\n____\n");
        assert_eq!(drawn("-2", &["1h"], false), "-2\n__\n");
        assert_eq!(drawn("jan 1", &["1M"], false), "jan 1\n___\n");
        assert_eq!(drawn("0x10", &["1g"], false), "0x10\n____\n");
    }

    #[test]
    fn tabs_are_drawn_as_one_column_and_wide_characters_as_two() {
        assert_eq!(drawn("a\tb", &[], true), "a>b\n___\n");
        assert_eq!(drawn("日本 y", &["2,2"], false), "日本 y\n    __\n");
    }

    fn settings_notes(keys: &[&str], global: &str, tab: Option<u8>) -> Vec<String> {
        let keys: Vec<KeySpec> = keys
            .iter()
            .map(|k| parse_key(k.as_bytes()).unwrap())
            .collect();
        let mut global_key = KeySpec::whole_line();
        crate::keydef::set_ordering(
            global.as_bytes(),
            &mut global_key,
            crate::keydef::Blanks::Both,
        );
        notes(&Settings {
            keys: &keys,
            global: &global_key,
            gkey_only: false,
            tab,
            stable: false,
            unique: false,
        })
    }

    #[test]
    fn the_warnings_are_upstreams() {
        let n = settings_notes(&["2,2"], "", None);
        assert_eq!(
            n.get(1).map(String::as_str),
            Some("leading blanks are significant in key 1; consider also specifying 'b'")
        );
        let n = settings_notes(&["2,1"], "", None);
        assert_eq!(
            n.get(1).map(String::as_str),
            Some("key 1 has zero width and will be ignored")
        );
        let n = settings_notes(&["1n"], "", Some(b'.'));
        assert_eq!(
            n.get(1..),
            Some(
                &[
                    "key 1 is numeric and spans multiple fields".to_string(),
                    "field separator \u{2018}.\u{2019} is treated as a decimal point in numbers"
                        .to_string()
                ][..]
            )
        );
        let n = settings_notes(&["1,1n"], "d", None);
        assert_eq!(n.last().map(String::as_str), Some("option '-d' is ignored"));
    }
}
