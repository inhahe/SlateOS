//! Version comparison for `.pc` dependency constraints.
//!
//! `.pc` files carry free-form `Version:` strings and `Requires:` clauses of
//! the shape `foo >= 1.2.3`.  Comparing two of those strings is not string
//! comparison and not numeric comparison — it is the `rpmvercmp` algorithm,
//! which both pkg-config and pkgconf use, so a package built against the
//! reference tools resolves identically here.
//!
//! The algorithm walks both strings in lockstep, splitting each into runs of
//! digits and runs of letters and comparing run against run:
//!
//! * non-alphanumeric characters are separators and carry no meaning, so
//!   `1.2.3`, `1-2-3` and `1_2_3` all compare equal;
//! * a numeric run always outranks an alphabetic one at the same position, so
//!   `1.10` > `1.a`;
//! * numeric runs compare by value (leading zeros stripped), so `1.10` > `1.9`
//!   even though `"10" < "9"` as text — the single most important reason a
//!   plain string compare cannot be used here;
//! * a `~` sorts *before* everything including the end of the string, so
//!   `1.0~rc1` < `1.0`.  This is the pkgconf/rpm behaviour; pkg-config 0.29
//!   itself lacks it and would sort `1.0~rc1` > `1.0`, which is wrong for
//!   every distribution that ships pre-release versions.

use core::cmp::Ordering;

/// A comparison operator from a `Requires:`/`Conflicts:` clause or from a
/// `--atleast-version`-style command-line check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Lt,
    Le,
    Eq,
    Ne,
    Ge,
    Gt,
}

impl CmpOp {
    /// Parse an operator token.  Returns `None` for anything else, which is
    /// how the `Requires:` scanner tells an operator from a package name.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "<" => Some(Self::Lt),
            "<=" => Some(Self::Le),
            "=" | "==" => Some(Self::Eq),
            "!=" => Some(Self::Ne),
            ">=" => Some(Self::Ge),
            ">" => Some(Self::Gt),
            _ => None,
        }
    }

    /// The canonical spelling, used when echoing a constraint back to the user
    /// (`--print-requires`, error messages).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Eq => "=",
            Self::Ne => "!=",
            Self::Ge => ">=",
            Self::Gt => ">",
        }
    }

    /// Does `ord` (the result of `compare(have, want)`) satisfy this operator?
    #[must_use]
    pub fn satisfied_by(self, ord: Ordering) -> bool {
        match self {
            Self::Lt => ord == Ordering::Less,
            Self::Le => ord != Ordering::Greater,
            Self::Eq => ord == Ordering::Equal,
            Self::Ne => ord != Ordering::Equal,
            Self::Ge => ord != Ordering::Less,
            Self::Gt => ord == Ordering::Greater,
        }
    }
}

/// Strip leading ASCII zeros from a numeric run.  May return an empty slice
/// (`"000"` -> `""`); that is deliberate and matches the reference algorithm,
/// where `"000"` and `"0"` both reduce to zero-length and so compare equal.
fn strip_leading_zeros(mut s: &[u8]) -> &[u8] {
    while let Some((&b'0', rest)) = s.split_first() {
        s = rest;
    }
    s
}

/// Compare two version strings with the `rpmvercmp` algorithm.
///
/// This is a total order on the strings it is given, but note that it is *not*
/// antisymmetric with respect to `==`: `compare("1.2.3", "1-2-3")` is `Equal`
/// even though the strings differ, because separators are not significant.
#[must_use]
#[allow(clippy::missing_panics_doc)] // the only indexing is bounds-guarded below
pub fn compare(a: &str, b: &str) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }

    let one = a.as_bytes();
    let two = b.as_bytes();
    let mut i = 0usize;
    let mut j = 0usize;

    loop {
        // Separators carry no meaning; skip them on both sides independently.
        // `~` is not a separator — it is a segment of its own, handled next.
        while i < one.len() && !one[i].is_ascii_alphanumeric() && one[i] != b'~' {
            i += 1;
        }
        while j < two.len() && !two[j].is_ascii_alphanumeric() && two[j] != b'~' {
            j += 1;
        }

        // `~` sorts before everything, including the end of the string, so a
        // side that has one here loses outright unless the other does too.
        let tilde_one = i < one.len() && one[i] == b'~';
        let tilde_two = j < two.len() && two[j] == b'~';
        if tilde_one || tilde_two {
            if !tilde_one {
                return Ordering::Greater;
            }
            if !tilde_two {
                return Ordering::Less;
            }
            i += 1;
            j += 1;
            continue;
        }

        // One side ran out: the tail comparison below decides.
        if i >= one.len() || j >= two.len() {
            break;
        }

        // The character class is chosen from `one`; if `two` starts a run of
        // the *other* class its run comes out empty, and numeric beats alpha.
        let numeric = one[i].is_ascii_digit();
        let start_one = i;
        let start_two = j;
        if numeric {
            while i < one.len() && one[i].is_ascii_digit() {
                i += 1;
            }
            while j < two.len() && two[j].is_ascii_digit() {
                j += 1;
            }
        } else {
            while i < one.len() && one[i].is_ascii_alphabetic() {
                i += 1;
            }
            while j < two.len() && two[j].is_ascii_alphabetic() {
                j += 1;
            }
        }

        if j == start_two {
            // `two` had no run of this class at all — i.e. the two strings
            // diverge in character class here.  Digits win over letters.
            return if numeric {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        }

        let seg_one = one.get(start_one..i).unwrap_or_default();
        let seg_two = two.get(start_two..j).unwrap_or_default();

        let ord = if numeric {
            // Compare as numbers without parsing: after stripping leading
            // zeros the longer digit run is the larger value, and equal-length
            // runs compare lexicographically.  This avoids an integer overflow
            // on absurdly long version components.
            let s1 = strip_leading_zeros(seg_one);
            let s2 = strip_leading_zeros(seg_two);
            match s1.len().cmp(&s2.len()) {
                Ordering::Equal => s1.cmp(s2),
                other => other,
            }
        } else {
            seg_one.cmp(seg_two)
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }

    // Everything compared equal up to the point one side ran out; the side
    // with characters left over is the newer one.
    match (i >= one.len(), j >= two.len()) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        // Both spent — equal.  `(false, false)` cannot occur: the loop above
        // only exits when at least one side is exhausted.  It shares this arm
        // rather than getting an `unreachable!()`, because a version compare
        // that panics would take down every build system that shells out to
        // us, and `Equal` is the answer that case would want anyway.
        (true, true) | (false, false) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::{CmpOp, compare};
    use core::cmp::Ordering;

    #[test]
    fn identical_strings_are_equal() {
        assert_eq!(compare("1.2.3", "1.2.3"), Ordering::Equal);
        assert_eq!(compare("", ""), Ordering::Equal);
    }

    #[test]
    fn numeric_segments_compare_by_value_not_text() {
        // The whole reason a string compare is not good enough.
        assert_eq!(compare("1.10", "1.9"), Ordering::Greater);
        assert_eq!(compare("1.9", "1.10"), Ordering::Less);
        assert_eq!(compare("1.0.100", "1.0.99"), Ordering::Greater);
    }

    #[test]
    fn leading_zeros_are_not_significant() {
        assert_eq!(compare("1.007", "1.7"), Ordering::Equal);
        assert_eq!(compare("000", "0"), Ordering::Equal);
        assert_eq!(compare("1.08", "1.9"), Ordering::Less);
    }

    #[test]
    fn separators_are_not_significant() {
        assert_eq!(compare("1.2.3", "1-2-3"), Ordering::Equal);
        assert_eq!(compare("1.2.3", "1_2:3"), Ordering::Equal);
        assert_eq!(compare("1..2", "1.2"), Ordering::Equal);
    }

    #[test]
    fn longer_version_with_extra_segment_is_newer() {
        assert_eq!(compare("1.2.3", "1.2"), Ordering::Greater);
        assert_eq!(compare("1.2", "1.2.3"), Ordering::Less);
    }

    #[test]
    fn digits_outrank_letters_at_the_same_position() {
        assert_eq!(compare("1.10", "1.a"), Ordering::Greater);
        assert_eq!(compare("1.a", "1.10"), Ordering::Less);
    }

    #[test]
    fn alphabetic_segments_compare_lexicographically() {
        assert_eq!(compare("1.0a", "1.0b"), Ordering::Less);
        assert_eq!(compare("2.13.2", "2.13.1"), Ordering::Greater);
    }

    #[test]
    fn tilde_sorts_before_everything_including_end_of_string() {
        assert_eq!(compare("1.0~rc1", "1.0"), Ordering::Less);
        assert_eq!(compare("1.0", "1.0~rc1"), Ordering::Greater);
        assert_eq!(compare("1.0~rc1", "1.0~rc2"), Ordering::Less);
        assert_eq!(compare("1.0~~", "1.0~"), Ordering::Less);
    }

    #[test]
    fn real_world_pc_versions() {
        assert_eq!(compare("3.2.0", "3.0.0"), Ordering::Greater);
        assert_eq!(compare("1.6.40", "1.6.9"), Ordering::Greater);
        assert_eq!(compare("2.1.0", "2.1"), Ordering::Greater);
        assert_eq!(compare("0.29.2", "0.29"), Ordering::Greater);
        assert_eq!(compare("1.3.1", "1.3.1"), Ordering::Equal);
    }

    #[test]
    fn operators_round_trip() {
        for (text, op) in [
            ("<", CmpOp::Lt),
            ("<=", CmpOp::Le),
            ("=", CmpOp::Eq),
            ("!=", CmpOp::Ne),
            (">=", CmpOp::Ge),
            (">", CmpOp::Gt),
        ] {
            assert_eq!(CmpOp::parse(text), Some(op));
            assert_eq!(op.as_str(), text);
        }
        // `==` is accepted as an alias but canonicalises to `=`.
        assert_eq!(CmpOp::parse("=="), Some(CmpOp::Eq));
        assert_eq!(CmpOp::parse("~>"), None);
        assert_eq!(CmpOp::parse("zlib"), None);
    }

    #[test]
    fn operator_satisfaction() {
        assert!(CmpOp::Ge.satisfied_by(Ordering::Equal));
        assert!(CmpOp::Ge.satisfied_by(Ordering::Greater));
        assert!(!CmpOp::Ge.satisfied_by(Ordering::Less));
        assert!(CmpOp::Ne.satisfied_by(Ordering::Less));
        assert!(!CmpOp::Ne.satisfied_by(Ordering::Equal));
        assert!(CmpOp::Lt.satisfied_by(Ordering::Less));
        assert!(!CmpOp::Lt.satisfied_by(Ordering::Equal));
    }
}
