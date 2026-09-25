//! Which part of a line a key is, and how that part is compared.
//!
//! A `-k` spec names a start and an end position, each a field number and an
//! optional character offset inside it, each able to carry its own ordering
//! letters: `-k2.3n,4.1b`. This module parses those specs and cuts the slice
//! they name out of a line.
//!
//! ## The two field models
//!
//! With no `-t`, a field is a run of blanks followed by a run of non-blanks,
//! and **the blanks belong to the field that follows them**. That is why
//! `-k2,2` of `a  b` is `"  b"` and not `"b"`, and why `b` (skip leading
//! blanks) exists as a per-position flag at all.
//!
//! With `-t C`, fields are what lies *between* separators and the separator
//! belongs to no field. ` foo bar` under `-t' '` therefore has three fields —
//! an empty one, `foo`, and `bar` — and `-k1,1` selects the empty one.
//!
//! Both models are GNU's `begfield`/`limfield` transcribed, including the
//! parts that look like accidents: an end position with no character offset
//! means "through the end of that field" (implemented by advancing one field
//! further), and an end that lands before its start is an empty key rather
//! than an error, so `-k2,1` ties every line.

use std::cmp::Ordering;

use crate::order::{self, Ignore, is_blank};

/// How a key's bytes are turned into an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    /// Bytes, after `-d`/`-i`/`-f`.
    #[default]
    Default,
    /// `-n`: exact decimal.
    Numeric,
    /// `-g`: through a `double`.
    General,
    /// `-h`: `2K` below `1M`.
    Human,
    /// `-M`: month names.
    Month,
    /// `-V`: version numbers.
    Version,
}

/// Every ordering a key named -- GNU's separate `numeric`, `general_numeric`,
/// `human_numeric`, `month` and `version` flags.
///
/// [`KeySpec::kind`] holds the one the comparison uses. This holds all of them,
/// because naming two is an error that is only *reported* once the whole
/// command line has been read (see [`check_compatibility`]), and by then the
/// first one alone cannot say what the second was.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // One flag per option letter, as upstream keeps them.
pub struct Named {
    pub numeric: bool,
    pub general: bool,
    pub human: bool,
    pub month: bool,
    pub version: bool,
}

/// One key: where it starts, where it ends, and how it is compared.
///
/// Field and character numbers are 0-origin here; the command line's are
/// 1-origin and are converted as they are parsed.
#[derive(Debug, Clone, Default)]
pub struct KeySpec {
    /// Start field, or `None` for the start of the line.
    pub sword: Option<usize>,
    /// Characters into the start field to skip.
    pub schar: usize,
    /// End field, or `None` for the end of the line.
    pub eword: Option<usize>,
    /// Characters into the end field to keep. Zero means the whole field.
    pub echar: usize,
    /// `b` on the start position: leading blanks are not part of the key.
    pub skip_start_blanks: bool,
    /// `b` on the end position.
    pub skip_end_blanks: bool,
    pub kind: Kind,
    /// Every ordering named, for [`check_compatibility`].
    pub named: Named,
    pub ignore: Option<Ignore>,
    pub fold: bool,
    pub reverse: bool,
}

impl KeySpec {
    /// The whole line, compared as bytes.
    pub fn whole_line() -> Self {
        Self::default()
    }

    /// Whether this key names an ordering of its own, and so inherits nothing.
    ///
    /// A key that names none takes the global one, which is what makes
    /// `sort -n -k2,2` sort field 2 numerically even though the `n` is not on
    /// the key. Inheritance is all-or-nothing: `-k2,2n` names *one* option and
    /// therefore takes none of the others, which is why `sort -r -k2,2n` does
    /// not reverse.
    pub fn has_ordering(&self) -> bool {
        self.reverse || self.makes_a_key()
    }

    /// Whether the *global* options amount to a key, when no `-k` was given.
    ///
    /// This is [`Self::has_ordering`] without `reverse`, and the difference is
    /// not an oversight — it is two different tests in GNU. `sort -r` on its
    /// own has no key at all: the reversal is applied to the last-resort
    /// whole-line comparison. But `-k2,2r` *is* a key that names an ordering,
    /// so it inherits nothing from the global options.
    pub fn makes_a_key(&self) -> bool {
        self.ignore.is_some()
            || self.fold
            || self.skip_start_blanks
            || self.skip_end_blanks
            || self.kind != Kind::Default
    }

    /// Take the global ordering options, for a key that named none.
    pub fn inherit(&mut self, global: &KeySpec) {
        self.ignore = global.ignore;
        self.fold = global.fold;
        self.skip_start_blanks = global.skip_start_blanks;
        self.skip_end_blanks = global.skip_end_blanks;
        self.kind = global.kind;
        self.named = global.named;
        self.reverse = global.reverse;
    }

    /// Name an ordering on this key: `-n`, `--month-sort`, `--sort=version`
    /// and the letters inside a `-k` spec all arrive here.
    ///
    /// The first one named is the one compared by; every one is recorded, so a
    /// second can be refused later with both in the message.
    pub fn name(&mut self, kind: Kind) {
        if self.kind == Kind::Default {
            self.kind = kind;
        }
        match kind {
            Kind::Default => {}
            Kind::Numeric => self.named.numeric = true,
            Kind::General => self.named.general = true,
            Kind::Human => self.named.human = true,
            Kind::Month => self.named.month = true,
            Kind::Version => self.named.version = true,
        }
    }

    /// The slice of `line` this key selects.
    ///
    /// The end is computed first because a whole-line start skips blanks only
    /// as far as the end — a key whose end lands inside the leading blanks is
    /// empty, not inverted.
    pub fn extract<'a>(&self, line: &'a [u8], tab: Option<u8>) -> &'a [u8] {
        let end = match self.eword {
            Some(_) => self.limfield(line, tab),
            None => line.len(),
        };
        let start = match self.sword {
            Some(_) => self.begfield(line, tab),
            None => {
                let mut ptr = 0usize;
                if self.skip_start_blanks {
                    while ptr < end && line.get(ptr).copied().is_some_and(is_blank) {
                        ptr = ptr.saturating_add(1);
                    }
                }
                ptr
            }
        };
        // An end before its start is an empty key, not a panic.
        line.get(start..end.max(start)).unwrap_or_default()
    }

    /// Where the key starts: past `sword` fields, past blanks if `b`, plus
    /// `schar` characters.
    fn begfield(&self, line: &[u8], tab: Option<u8>) -> usize {
        let lim = line.len();
        let mut ptr = 0usize;
        let mut sword = self.sword.unwrap_or(0);
        while ptr < lim && sword > 0 {
            sword = sword.saturating_sub(1);
            ptr = skip_one_field(line, ptr, lim, tab, true);
        }
        if self.skip_start_blanks {
            while ptr < lim && line.get(ptr).copied().is_some_and(is_blank) {
                ptr = ptr.saturating_add(1);
            }
        }
        lim.min(ptr.saturating_add(self.schar))
    }

    /// Where the key ends.
    fn limfield(&self, line: &[u8], tab: Option<u8>) -> usize {
        let lim = line.len();
        let mut ptr = 0usize;
        let echar = self.echar;
        // No character offset means "through the end of this field", which is
        // the same place as "the start of the next one".
        let mut eword = self.eword.unwrap_or(0);
        if echar == 0 {
            eword = eword.saturating_add(1);
        }
        while ptr < lim && eword > 0 {
            eword = eword.saturating_sub(1);
            // With an explicit separator, the last field walked stops *at* its
            // separator rather than past it — otherwise `-k1,1` of `a::b` under
            // `-t:` would reach into field 2.
            let consume_separator = eword > 0 || echar > 0;
            ptr = skip_one_field(line, ptr, lim, tab, consume_separator);
        }
        // Both of these are inside "there is a character offset to apply".
        // Without one the key ends where the field does, and `b` on the end
        // position has nothing to do — `-k1,1b` of ` a b` is ` a`, not ` a `.
        // That is not an oversight in GNU: skipping the blanks would push the
        // end *into* the next field's leading run.
        if echar != 0 {
            if self.skip_end_blanks {
                while ptr < lim && line.get(ptr).copied().is_some_and(is_blank) {
                    ptr = ptr.saturating_add(1);
                }
            }
            ptr = lim.min(ptr.saturating_add(echar));
        }
        ptr
    }

    /// Compare two lines under this key alone.
    pub fn compare(&self, a: &[u8], b: &[u8], tab: Option<u8>) -> Ordering {
        let ka = self.extract(a, tab);
        let kb = self.extract(b, tab);
        let diff = match self.kind {
            Kind::Default => order::default_order(ka, kb, self.ignore, self.fold),
            Kind::Numeric => order::numeric(ka, kb),
            Kind::General => order::general(ka, kb),
            Kind::Human => order::human(ka, kb),
            Kind::Month => order::month(ka, kb),
            Kind::Version => order::version(ka, kb),
        };
        if self.reverse { diff.reverse() } else { diff }
    }
}

/// Advance past one field, leaving `ptr` at the start of the next.
///
/// `consume_separator` is only consulted in the `-t` model: without a
/// separator character a field boundary is a run of blanks that belongs to the
/// following field, so there is nothing to consume.
fn skip_one_field(line: &[u8], from: usize, lim: usize, tab: Option<u8>, consume: bool) -> usize {
    let mut ptr = from;
    match tab {
        Some(t) => {
            while ptr < lim && line.get(ptr) != Some(&t) {
                ptr = ptr.saturating_add(1);
            }
            if ptr < lim && consume {
                ptr = ptr.saturating_add(1);
            }
        }
        None => {
            while ptr < lim && line.get(ptr).copied().is_some_and(is_blank) {
                ptr = ptr.saturating_add(1);
            }
            while ptr < lim && line.get(ptr).copied().is_some_and(|c| !is_blank(c)) {
                ptr = ptr.saturating_add(1);
            }
        }
    }
    ptr
}

// ── parsing a spec ──────────────────────────────────────────────────────────

/// Parse a `-k` argument.
///
/// The diagnostics are GNU's, word for word, because a script that greps for
/// one is grepping for that text.
pub fn parse_key(spec: &[u8]) -> Result<KeySpec, String> {
    let bad = |msg: &str| -> String {
        format!(
            "{msg}: invalid field specification {}",
            coreutils::quote::quote(spec)
        )
    };
    let mut key = KeySpec::default();
    let mut rest = spec;

    let (field, after) = parse_count(rest, "invalid number at field start")?;
    if field == 0 {
        return Err(bad("field number is zero"));
    }
    key.sword = Some(field.saturating_sub(1));
    rest = after;

    if rest.first() == Some(&b'.') {
        let (ch, after) = parse_count(
            rest.get(1..).unwrap_or_default(),
            "invalid number after '.'",
        )?;
        if ch == 0 {
            return Err(bad("character offset is zero"));
        }
        key.schar = ch.saturating_sub(1);
        rest = after;
    }

    rest = set_ordering(rest, &mut key, Blanks::Start);

    if rest.first() == Some(&b',') {
        let (field, after) = parse_count(
            rest.get(1..).unwrap_or_default(),
            "invalid number after ','",
        )?;
        if field == 0 {
            return Err(bad("field number is zero"));
        }
        key.eword = Some(field.saturating_sub(1));
        rest = after;
        if rest.first() == Some(&b'.') {
            let (ch, after) = parse_count(
                rest.get(1..).unwrap_or_default(),
                "invalid number after '.'",
            )?;
            key.echar = ch;
            rest = after;
        }
        rest = set_ordering(rest, &mut key, Blanks::End);
    }

    if !rest.is_empty() {
        return Err(bad("stray character in field spec"));
    }
    Ok(key)
}

/// Parse an obsolete `+POS` start, as in `sort +1 -2`.
///
/// Returns `None` when the argument is not a position at all, in which case it
/// is a file name. The numbers here are already 0-origin — that is the whole
/// difference between `+1` and `-k2` — and `+0` with no character offset means
/// the start of the line rather than the start of field 1.
pub fn parse_obsolete_start(spec: &[u8]) -> Option<KeySpec> {
    let mut key = KeySpec::default();
    let mut rest = spec.strip_prefix(b"+")?;
    let (field, after) = parse_count(rest, "").ok()?;
    key.sword = Some(field);
    rest = after;
    if rest.first() == Some(&b'.') {
        let (ch, after) = parse_count(rest.get(1..).unwrap_or_default(), "").ok()?;
        key.schar = ch;
        rest = after;
    }
    if key.sword == Some(0) && key.schar == 0 {
        key.sword = None;
    }
    let rest = set_ordering(rest, &mut key, Blanks::Start);
    if rest.is_empty() { Some(key) } else { None }
}

/// Parse an obsolete `-POS` end onto an already-parsed `+POS` start.
///
/// `-B` alone ends at the end of field `B`, so it becomes end field `B-1` with
/// "whole field"; `-B.y` ends `y` characters into field `B+1`, which is the
/// asymmetry POSIX inherited and GNU kept.
pub fn parse_obsolete_end(spec: &[u8], key: &mut KeySpec) -> Result<(), String> {
    let rest = spec.strip_prefix(b"-").unwrap_or(spec);
    let (field, mut rest) = parse_count(rest, "invalid number after '-'")?;
    let mut echar = 0usize;
    if rest.first() == Some(&b'.') {
        let (ch, after) = parse_count(
            rest.get(1..).unwrap_or_default(),
            "invalid number after '.'",
        )?;
        echar = ch;
        rest = after;
    }
    let eword = if echar == 0 && field > 0 {
        field.saturating_sub(1)
    } else {
        field
    };
    key.eword = Some(eword);
    key.echar = echar;
    let rest = set_ordering(rest, key, Blanks::End);
    if !rest.is_empty() {
        return Err(format!(
            "stray character in field spec: invalid field specification {}",
            coreutils::quote::quote(spec)
        ));
    }
    Ok(())
}

/// Which position the `b` flag applies to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Blanks {
    Start,
    End,
    /// The global `-b`, which applies to both.
    Both,
}

/// Read the ordering letters at the front of `s`, returning what is left.
///
/// Unlike a normal option parser this stops at the first letter it does not
/// know rather than failing — the caller decides whether the leftover is a
/// `,`, the end of the spec, or an error.
pub fn set_ordering<'a>(s: &'a [u8], key: &mut KeySpec, blanks: Blanks) -> &'a [u8] {
    let mut rest = s;
    while let Some(&c) = rest.first() {
        match c {
            b'b' => {
                if blanks != Blanks::End {
                    key.skip_start_blanks = true;
                }
                if blanks != Blanks::Start {
                    key.skip_end_blanks = true;
                }
            }
            b'd' => key.ignore = Some(Ignore::NonDictionary),
            b'i' => key.ignore = Some(Ignore::NonPrinting),
            b'f' => key.fold = true,
            b'g' => key.name(Kind::General),
            b'h' => key.name(Kind::Human),
            b'M' => key.name(Kind::Month),
            b'n' => key.name(Kind::Numeric),
            b'V' => key.name(Kind::Version),
            b'r' => key.reverse = true,
            _ => break,
        }
        rest = rest.get(1..).unwrap_or_default();
    }
    rest
}

/// GNU's `check_ordering_compatibility`: the first key that combines orderings
/// which cannot be combined, reported with every option it carries.
///
/// It runs once, after the whole command line is read and the global options
/// have been inherited -- not as each `-k` is parsed -- because that is where
/// upstream runs it, and the difference is visible. `sort -nM` is refused
/// through the whole-line key the global options become; `sort -nM -k1` through
/// a key that inherited both; `sort -nM -k1,1r` and `sort -nM -k1d` not at all,
/// since those keys name an ordering of their own and inherit nothing. And
/// `sort -k1nM -k0` reports the zero field, which parsing reaches first.
///
/// Numeric, general-numeric, human and month count one each. Version and the
/// ignore options (`-d`, `-i`) share a single slot, which is why `-k1Vd` is
/// accepted and `-k1nd` is not; upstream counts `-R` there too, which is
/// refused as unimplemented before this runs. The message lists the key's
/// options in upstream's order with `b` and `r` left out -- `-dMn`, `-fin` --
/// so `f` is printed although it is never what makes a key incompatible.
///
/// # Errors
///
/// `options '-…' are incompatible`, for the first such key.
pub fn check_compatibility(keys: &[KeySpec]) -> Result<(), String> {
    for key in keys {
        let n = key.named;
        let slots = [
            n.numeric,
            n.general,
            n.human,
            n.month,
            n.version || key.ignore.is_some(),
        ];
        if slots.iter().filter(|&&set| set).count() <= 1 {
            continue;
        }
        let letters = [
            (key.ignore == Some(Ignore::NonDictionary), 'd'),
            (key.fold, 'f'),
            (n.general, 'g'),
            (n.human, 'h'),
            (key.ignore == Some(Ignore::NonPrinting), 'i'),
            (n.month, 'M'),
            (n.numeric, 'n'),
            (n.version, 'V'),
        ];
        let opts: String = letters
            .iter()
            .filter(|(set, _)| *set)
            .map(|&(_, c)| c)
            .collect();
        return Err(format!("options '-{opts}' are incompatible"));
    }
    Ok(())
}

/// Read a decimal count, returning it and the rest of the spec.
///
/// A count too large for a `usize` is clamped rather than rejected: it names a
/// field past the end of every line, which is exactly what the clamped value
/// names too.
fn parse_count<'a>(s: &'a [u8], msgid: &str) -> Result<(usize, &'a [u8]), String> {
    let end = s
        .iter()
        .position(|c| !c.is_ascii_digit())
        .unwrap_or(s.len());
    if end == 0 {
        return Err(format!(
            "{msgid}: invalid count at start of {}",
            coreutils::quote::quote(s)
        ));
    }
    let mut value = 0usize;
    for &c in s.get(..end).unwrap_or_default() {
        value = value
            .saturating_mul(10)
            .saturating_add(usize::from(c.wrapping_sub(b'0')));
    }
    Ok((value, s.get(end..).unwrap_or_default()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn cut(spec: &str, line: &str) -> String {
        let key = parse_key(spec.as_bytes()).unwrap();
        String::from_utf8(key.extract(line.as_bytes(), None).to_vec()).unwrap()
    }

    fn cut_t(spec: &str, line: &str, tab: u8) -> String {
        let key = parse_key(spec.as_bytes()).unwrap();
        String::from_utf8(key.extract(line.as_bytes(), Some(tab)).to_vec()).unwrap()
    }

    #[test]
    fn a_field_owns_the_blanks_in_front_of_it() {
        // Measured against GNU sort 8.32 with --debug.
        assert_eq!(cut("1,1", " a b"), " a");
        assert_eq!(cut("2,2", " a b"), " b");
        assert_eq!(cut("2", "c  d"), "  d");
        assert_eq!(cut("1,1", "c  d"), "c");
    }

    #[test]
    fn b_drops_them_again() {
        assert_eq!(cut("1,1b", " a b"), " a");
        assert_eq!(cut("2b,2", " a b"), "b");
        assert_eq!(cut("1b,1", " a b"), "a");
    }

    #[test]
    fn a_character_offset_counts_from_the_field_start() {
        assert_eq!(cut("1.2", " a b"), "a b");
        assert_eq!(cut("1.2,2.1", " a b"), "a ");
        assert_eq!(cut("1", " a b"), " a b");
    }

    #[test]
    fn a_separator_belongs_to_no_field() {
        assert_eq!(cut_t("1,1", " foo bar", b' '), "");
        assert_eq!(cut_t("2,2", " foo bar", b' '), "foo");
        assert_eq!(cut_t("3,3", " foo bar", b' '), "bar");
        assert_eq!(cut_t("2,2", "a::b", b':'), "");
        assert_eq!(cut_t("1,1", "a::b", b':'), "a");
        assert_eq!(cut_t("3,3", "a::b", b':'), "b");
        // A field past the end of the line is empty, not an error.
        assert_eq!(cut_t("2,2", " foo bar", b':'), "");
    }

    #[test]
    fn an_end_before_its_start_is_empty() {
        assert_eq!(cut("2,1", "a b c"), "");
    }

    #[test]
    fn bad_specs_are_refused_with_gnus_words() {
        let err = |s: &str| parse_key(s.as_bytes()).unwrap_err();
        assert_eq!(
            err("0"),
            "field number is zero: invalid field specification ‘0’"
        );
        assert_eq!(
            err("1.0"),
            "character offset is zero: invalid field specification ‘1.0’"
        );
        assert_eq!(
            err("x"),
            "invalid number at field start: invalid count at start of ‘x’"
        );
        assert_eq!(
            err("1x"),
            "stray character in field spec: invalid field specification ‘1x’"
        );
        assert_eq!(
            err("1,0"),
            "field number is zero: invalid field specification ‘1,0’"
        );
        assert_eq!(
            err("1,x"),
            "invalid number after ',': invalid count at start of ‘x’"
        );
        assert_eq!(
            err("1."),
            "invalid number after '.': invalid count at start of ‘’"
        );
    }

    /// Upstream's canonical letter order, and its counting: version and the
    /// ignore options share one slot, `f` is listed but never counted.
    #[test]
    fn incompatible_orderings_are_reported_in_upstream_order() {
        let check = |specs: &[&str]| {
            let keys: Vec<KeySpec> = specs
                .iter()
                .map(|s| parse_key(s.as_bytes()).unwrap())
                .collect();
            check_compatibility(&keys)
        };
        let refused = |specs: &[&str]| check(specs).unwrap_err();
        assert_eq!(refused(&["1n,2M"]), "options '-Mn' are incompatible");
        assert_eq!(refused(&["1nd"]), "options '-dn' are incompatible");
        assert_eq!(refused(&["1nMf"]), "options '-fMn' are incompatible");
        assert_eq!(refused(&["1fin"]), "options '-fin' are incompatible");
        assert_eq!(refused(&["1bnr,1M"]), "options '-Mn' are incompatible");
        assert_eq!(refused(&["1nV"]), "options '-nV' are incompatible");
        // The first offending key in command-line order is the one named.
        assert_eq!(
            refused(&["1", "2gi", "3nM"]),
            "options '-gi' are incompatible"
        );
        assert!(check(&["1Vd"]).is_ok());
        assert!(check(&["1nf"]).is_ok());
        assert!(check(&["1,1n", "2,2M"]).is_ok());
    }

    #[test]
    fn ordering_letters_attach_to_the_position_they_follow() {
        let key = parse_key(b"2n,3r").unwrap();
        assert_eq!(key.kind, Kind::Numeric);
        assert!(key.reverse);
        assert_eq!(key.sword, Some(1));
        assert_eq!(key.eword, Some(2));
        assert_eq!(key.echar, 0);

        let key = parse_key(b"1b,2b").unwrap();
        assert!(key.skip_start_blanks && key.skip_end_blanks);
        let key = parse_key(b"1,2b").unwrap();
        assert!(!key.skip_start_blanks && key.skip_end_blanks);
    }

    #[test]
    fn obsolete_positions_are_zero_origin() {
        // `+1 -2` is `-k2,2`.
        let mut key = parse_obsolete_start(b"+1").unwrap();
        parse_obsolete_end(b"-2", &mut key).unwrap();
        assert_eq!(key.sword, Some(1));
        assert_eq!(key.eword, Some(1));
        assert_eq!(key.echar, 0);
        assert_eq!(
            key.extract(b"a 1", None),
            parse_key(b"2,2").unwrap().extract(b"a 1", None)
        );

        // `+0` alone is the whole line.
        let key = parse_obsolete_start(b"+0").unwrap();
        assert_eq!(key.sword, None);
        assert_eq!(key.extract(b" a b", None), b" a b");

        // Not a position at all.
        assert!(parse_obsolete_start(b"+file").is_none());
        assert!(parse_obsolete_start(b"file").is_none());
    }

    #[test]
    fn a_key_with_no_ordering_inherits_the_global_one() {
        let mut key = parse_key(b"2,2").unwrap();
        assert!(!key.has_ordering());
        let mut global = KeySpec::whole_line();
        global.kind = Kind::Numeric;
        key.inherit(&global);
        assert_eq!(key.kind, Kind::Numeric);

        // A key that names one keeps it.
        let key = parse_key(b"2,2V").unwrap();
        assert!(key.has_ordering());
    }
}
