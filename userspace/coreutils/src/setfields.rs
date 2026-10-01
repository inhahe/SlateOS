//! gnulib's `set_fields` (`src/set-fields.c`): a `cut`-style LIST of fields
//! or positions, as sorted, merged ranges.
//!
//! Upstream compiles this file into `cut` and `numfmt`, which is why `numfmt
//! --field=2-4` accepts exactly what `cut -f 2-4` accepts and refuses it in the
//! same words. It was `cut.rs`'s private function until `numfmt` arrived as the
//! second caller; the three option bits upstream passes are [`Flags`].
//!
//! # The grammar
//!
//! Items `N`, `N-`, `-M` and `N-M`, separated by commas **or by blanks** --
//! `cut -f '1 3'` is `cut -f 1,3`, which nothing documents and the code makes
//! plain. Indices are 1-based; 0, a decreasing range, two dashes in one item, a
//! non-digit and a number too large for the type are errors. A lone `-` is an
//! error too, unless [`Flags::allow_dash`] says it means every field (`numfmt
//! --field=-`).
//!
//! Every error is upstream's `FATAL_ERROR`: the message, then the *calling*
//! program's `Try '… --help'` line, status 1 -- which is why the program is a
//! parameter.

use crate::getopt::{self, Program};
use crate::quote::quote;

/// One selected span, 1-based and inclusive at both ends.
///
/// `hi == u64::MAX` is how an open range (`3-`) is written, and a pair of
/// `u64::MAX` is the sentinel that terminates the list — both are upstream's
/// representation, kept because the arithmetic around them (`hi + 1` in the
/// complement, `idx > hi` in `cut`'s scan) is what the observable output falls
/// out of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Range {
    pub lo: u64,
    pub hi: u64,
}

/// Upstream's `SETFLD_*` option bits.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Flags {
    /// `SETFLD_ALLOW_DASH`: a lone `-` means every field, as `1-`.
    pub allow_dash: bool,
    /// `SETFLD_COMPLEMENT`: select everything the list does not.
    pub complement: bool,
    /// `SETFLD_ERRMSG_USE_POS`: say "byte/character position" rather than
    /// "field" -- `cut -b` and `-c`.
    pub errmsg_use_pos: bool,
}

/// Parse LIST into sorted, merged ranges, ending with the `u64::MAX` sentinel.
///
/// # Errors
///
/// A zero index, a decreasing range, two dashes in one item, a lone `-`
/// without [`Flags::allow_dash`], a non-digit, a number that does not fit in a
/// `u64`, or an empty list -- each as `program`'s usage error.
pub fn set_fields(
    program: Program,
    spec: &[u8],
    flags: Flags,
) -> Result<Vec<Range>, getopt::Error> {
    let use_pos = flags.errmsg_use_pos;
    let mut ranges: Vec<Range> = Vec::new();
    let mut initial: u64 = 1;
    let mut value: u64 = 0;
    let mut lhs_specified = false;
    let mut rhs_specified = false;
    let mut dash_found = false;
    let mut in_digits = false;
    let mut num_start = 0usize;
    let mut at = 0usize;

    // `--field=-` is `--field=1-`: the dash is consumed as though `1` came
    // before it.
    if flags.allow_dash && spec == b"-" {
        value = 1;
        lhs_specified = true;
        dash_found = true;
        at = 1;
    }

    loop {
        // Past the end stands for C's terminating NUL, which the loop treats as
        // a separator that also ends it.
        let c = spec.get(at).copied();
        match c {
            Some(b'-') => {
                in_digits = false;
                if dash_found {
                    return Err(program.usage_referring(
                        if use_pos {
                            "invalid byte or character range"
                        } else {
                            "invalid field range"
                        }
                        .to_string(),
                    ));
                }
                dash_found = true;
                at = at.saturating_add(1);
                if lhs_specified && value == 0 {
                    return Err(program.usage_referring(numbered_from_1(use_pos)));
                }
                initial = if lhs_specified { value } else { 1 };
                value = 0;
            }
            None | Some(b',' | b' ' | b'\t') => {
                in_digits = false;
                if dash_found {
                    dash_found = false;
                    if !lhs_specified && !rhs_specified {
                        // A lone dash inside a list: every field, when that is
                        // allowed.
                        if flags.allow_dash {
                            initial = 1;
                        } else {
                            return Err(program
                                .usage_referring("invalid range with no endpoint: -".to_string()));
                        }
                    }
                    if rhs_specified {
                        if value < initial {
                            return Err(
                                program.usage_referring("invalid decreasing range".to_string())
                            );
                        }
                        ranges.push(Range {
                            lo: initial,
                            hi: value,
                        });
                    } else {
                        // `n-`: from here to the end of the line.
                        ranges.push(Range {
                            lo: initial,
                            hi: u64::MAX,
                        });
                    }
                    value = 0;
                } else {
                    if value == 0 {
                        return Err(program.usage_referring(numbered_from_1(use_pos)));
                    }
                    ranges.push(Range {
                        lo: value,
                        hi: value,
                    });
                    value = 0;
                }
                if c.is_none() {
                    break;
                }
                at = at.saturating_add(1);
                lhs_specified = false;
                rhs_specified = false;
            }
            Some(d) if d.is_ascii_digit() => {
                if !in_digits {
                    num_start = at;
                }
                in_digits = true;
                if dash_found {
                    rhs_specified = true;
                } else {
                    lhs_specified = true;
                }
                // `u64::MAX` is not merely an overflow guard: it is the value
                // that means "to end of line", so a list may not name it.
                let digit = u64::from(d.wrapping_sub(b'0'));
                match value.checked_mul(10).and_then(|v| v.checked_add(digit)) {
                    Some(v) if v != u64::MAX => value = v,
                    _ => {
                        // Only the *first* offending number is reported, and
                        // the whole of it — upstream re-scans from where the
                        // digit run began, so `cut -c 99999999999999999999,22`
                        // names the long number and stops.
                        let run = spec
                            .get(num_start..)
                            .unwrap_or_default()
                            .iter()
                            .position(|b| !b.is_ascii_digit())
                            .map_or_else(
                                || spec.get(num_start..).unwrap_or_default(),
                                |len| {
                                    spec.get(num_start..num_start.saturating_add(len))
                                        .unwrap_or_default()
                                },
                            );
                        return Err(program.usage_referring(format!(
                            "{} {} is too large",
                            if use_pos {
                                "byte/character offset"
                            } else {
                                "field number"
                            },
                            quote(run)
                        )));
                    }
                }
                at = at.saturating_add(1);
            }
            Some(_) => {
                // The rest of the string, not just the offending byte — GNU
                // echoes from here to the end.
                return Err(program.usage_referring(format!(
                    "{} {}",
                    if use_pos {
                        "invalid byte/character position"
                    } else {
                        "invalid field value"
                    },
                    quote(spec.get(at..).unwrap_or_default())
                )));
            }
        }
    }

    if ranges.is_empty() {
        return Err(program.usage_referring(
            if use_pos {
                "missing list of byte/character positions"
            } else {
                "missing list of fields"
            }
            .to_string(),
        ));
    }

    ranges.sort_by_key(|r| r.lo);
    merge_ranges(&mut ranges);
    if flags.complement {
        ranges = complement_ranges(&ranges);
    }
    // The sentinel the scanners walk into and never past.
    ranges.push(Range {
        lo: u64::MAX,
        hi: u64::MAX,
    });
    Ok(ranges)
}

/// The wording shared by the two "0 is not an index" diagnostics.
fn numbered_from_1(use_pos: bool) -> String {
    if use_pos {
        "byte/character positions are numbered from 1"
    } else {
        "fields are numbered from 1"
    }
    .to_string()
}

/// Fold ranges that **overlap** into one, leaving ones that merely touch
/// alone.
///
/// `2-5,3-4` becomes `2-5`; `1-2,3-4` stays two ranges. The distinction is
/// visible through `cut --output-delimiter`, which separates ranges.
fn merge_ranges(ranges: &mut Vec<Range>) {
    let mut i = 0usize;
    while i < ranges.len() {
        loop {
            let j = i.saturating_add(1);
            let (Some(&next), Some(&here)) = (ranges.get(j), ranges.get(i)) else {
                break;
            };
            if next.lo > here.hi {
                break;
            }
            if let Some(slot) = ranges.get_mut(i) {
                slot.hi = here.hi.max(next.hi);
            }
            ranges.remove(j);
        }
        i = i.saturating_add(1);
    }
}

/// `--complement`: everything the given ranges do not cover.
///
/// Touching ranges are skipped rather than producing an empty gap, which is why
/// this cannot be written as "invert each boundary". Applied *after* merging,
/// so the input is sorted and non-overlapping.
fn complement_ranges(ranges: &[Range]) -> Vec<Range> {
    let mut out: Vec<Range> = Vec::new();
    let (Some(&first), Some(&last)) = (ranges.first(), ranges.last()) else {
        return out;
    };
    if first.lo > 1 {
        out.push(Range {
            lo: 1,
            hi: first.lo.saturating_sub(1),
        });
    }
    for (prev, here) in ranges.iter().zip(ranges.iter().skip(1)) {
        if prev.hi.saturating_add(1) == here.lo {
            continue;
        }
        out.push(Range {
            lo: prev.hi.saturating_add(1),
            hi: here.lo.saturating_sub(1),
        });
    }
    if last.hi < u64::MAX {
        out.push(Range {
            lo: last.hi.saturating_add(1),
            hi: u64::MAX,
        });
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    const TEST: Program = Program::new("test", 1);

    fn spans(spec: &str, flags: Flags) -> Vec<(u64, u64)> {
        set_fields(TEST, spec.as_bytes(), flags)
            .unwrap()
            .iter()
            .map(|r| (r.lo, r.hi))
            .collect()
    }

    const MAX: u64 = u64::MAX;

    #[test]
    fn items_sort_merge_and_end_in_the_sentinel() {
        let f = Flags::default();
        assert_eq!(spans("3,1", f), vec![(1, 1), (3, 3), (MAX, MAX)]);
        assert_eq!(spans("2-5,3-4", f), vec![(2, 5), (MAX, MAX)]);
        assert_eq!(spans("1-2,3-4", f), vec![(1, 2), (3, 4), (MAX, MAX)]);
        assert_eq!(spans("-2,5-", f), vec![(1, 2), (5, MAX), (MAX, MAX)]);
        assert_eq!(spans("1 3", f), vec![(1, 1), (3, 3), (MAX, MAX)]);
    }

    #[test]
    fn a_lone_dash_is_every_field_only_when_allowed() {
        let allow = Flags {
            allow_dash: true,
            ..Flags::default()
        };
        assert_eq!(spans("-", allow), vec![(1, MAX), (MAX, MAX)]);
        assert_eq!(spans("2,-", allow), vec![(1, MAX), (MAX, MAX)]);
        let e = set_fields(TEST, b"-", Flags::default()).unwrap_err();
        assert_eq!(
            e.message(),
            "invalid range with no endpoint: -\nTry 'test --help' for more information."
        );
    }

    #[test]
    fn complement() {
        let c = Flags {
            complement: true,
            ..Flags::default()
        };
        assert_eq!(spans("2-3", c), vec![(1, 1), (4, MAX), (MAX, MAX)]);
    }

    #[test]
    fn refusals_name_the_calling_program_and_the_kind_of_list() {
        let pos = Flags {
            errmsg_use_pos: true,
            ..Flags::default()
        };
        let e = set_fields(TEST, b"0", pos).unwrap_err();
        assert!(
            e.message()
                .starts_with("byte/character positions are numbered from 1\nTry 'test")
        );
        let e = set_fields(TEST, b"3-1", Flags::default()).unwrap_err();
        assert!(e.message().starts_with("invalid decreasing range"));
        let e = set_fields(TEST, b"1x", Flags::default()).unwrap_err();
        assert!(e.message().starts_with("invalid field value ‘x’"));
        let e = set_fields(TEST, b"", Flags::default()).unwrap_err();
        assert!(e.message().starts_with("fields are numbered from 1"));
    }
}
