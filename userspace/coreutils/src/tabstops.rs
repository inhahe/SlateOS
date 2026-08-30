//! The tab-stop list that `expand` and `unexpand` share.
//!
//! This is a port of `coreutils-9.4/src/expand-common.c`, and it is in the
//! library for the same reason it is one file upstream: `expand -t` and
//! `unexpand -t` are documented as the same option, and the grammar they accept
//! is intricate enough that two hand-written parsers would certainly disagree
//! somewhere. They already do elsewhere in this tree, which is what
//! [`crate::getopt`] exists to stop.
//!
//! # The grammar
//!
//! A tab-stop argument is a list of column numbers separated by commas or
//! blanks — `-t 1,3,5`, `-t '1 3 5'`, and `-t1 -t3 -t5` all mean the same
//! thing, because every `-t` **appends** to one list rather than replacing it.
//! Empty entries are skipped, so `-t 1,,2` is `-t 1,2`.
//!
//! The last entry may carry a prefix, and the two prefixes mean different
//! things once the explicit stops run out:
//!
//! | argument | stops at | then |
//! |---|---|---|
//! | `-t 2,4` | 2, 4 | every column: one space per tab |
//! | `-t 2,/4` | 2 | multiples of 4 — 4, 8, 12 … |
//! | `-t 2,+4` | 2 | 4 past the *last stop* — 6, 10, 14 … |
//!
//! # Two rules that look like bugs and are not
//!
//! **A zero-valued prefix is not an error, it is *no prefix*.** `expand -t /0`
//! and `expand -t +0` both behave as plain `-t 8`, because the C stores the
//! size in a `uintmax_t` whose zero doubles as "unset", and the validator only
//! ever inspects the explicit list. The same goes for a prefix with no number
//! at all (`-t /`, `-t +`): nothing is stored, so nothing happens.
//!
//! **A prefix with no explicit stops becomes the plain tab size.** `-t /4` and
//! `-t +4` are both exactly `-t 4`, because `finalize` falls back to
//! `extend ? extend : increment ? increment : 8` when the list is empty.
//!
//! # Errors
//!
//! Every diagnostic here is `error (0, 0, …)` upstream followed by an eventual
//! `exit (EXIT_FAILURE)` — so they carry **no** `Try '… --help'` referral, and
//! several can be printed before the exit. [`TabStops::parse`] therefore
//! returns a *list* of messages rather than one: `-t 1/2/3` prints two.
//! [`TabStops::finalize`]'s three are `error (EXIT_FAILURE, …)` and stop at the
//! first, so it returns a single message.

use crate::quote::quote;

/// A parsed `-t`/`--tabs` list, accumulated across every occurrence of the
/// option and then [`finalize`](TabStops::finalize)d once parsing is over.
///
/// The default — no `-t` at all — is a stop every 8 columns, but that is
/// established by `finalize`, not by [`Default`]: an unfinalized `TabStops` has
/// no stops at all and [`next_stop`](TabStops::next_stop) would report every
/// tab as past the end.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TabStops {
    /// The explicit stops, in the order given. Ascending and nonzero after
    /// `finalize`; unchecked before it.
    list: Vec<u64>,
    /// The `/` size: after the list, stop at every multiple of this. 0 = unset,
    /// which is why `-t /0` is indistinguishable from no `/` at all.
    extend: u64,
    /// The `+` size: after the list, stop every this-many columns *past the
    /// last explicit stop*. 0 = unset, as above.
    increment: u64,
    /// One stop every `size` columns, ignoring `list`. 0 = use `list`.
    /// Set by `finalize`, which collapses the three single-stop cases into it.
    size: u64,
    /// The widest gap between consecutive stops, which `unexpand` needs in
    /// order to size its pending-blank buffer.
    max_column_width: u64,
}

impl TabStops {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The widest gap between two consecutive stops — `unexpand`'s buffer size.
    ///
    /// Only meaningful after [`finalize`](TabStops::finalize), which is what
    /// sets it for the single-size cases.
    #[must_use]
    pub fn max_column_width(&self) -> u64 {
        self.max_column_width
    }

    /// Append one stop that was assembled outside this module.
    ///
    /// [`parse`](TabStops::parse) is the way `-t` gets here. This is the way
    /// `unexpand`'s obsolete digit form does, which is a genuinely different
    /// mechanism rather than the same one spelled differently: `expand` gives
    /// its digits *optional arguments* and hands the whole cluster to `parse`,
    /// while `unexpand` gives them **no** argument and accumulates them one
    /// digit at a time across the entire command line, with `,` flushing the
    /// number. So `expand -1 -2` is two stops and `unexpand -1 -2` is one stop
    /// at twelve, and only `unexpand` needs this entry point.
    pub fn add(&mut self, value: u64) {
        self.add_stop(value);
    }

    /// Append one stop, tracking the widest gap.
    fn add_stop(&mut self, value: u64) {
        let previous = self.list.last().copied().unwrap_or(0);
        // The C is `prev <= tabval ? tabval - prev : 0`; the list is not yet
        // known to ascend at this point, so the guard is not decoration.
        let width = value.saturating_sub(previous);
        self.list.push(value);
        if self.max_column_width < width {
            self.max_column_width = width;
        }
        // Upstream also has `if (SIZE_MAX < column_width) error (…, "tabs are
        // too far apart")` here. It is dead code wherever `uintmax_t` and
        // `size_t` are both 64 bits, which is every target we build for, and it
        // is left out rather than ported as an unreachable branch — unlike
        // `uniq`'s dead branches, this one cannot change any real command
        // line's output.
    }

    /// `/N`: one message if a second `/` value is given.
    fn set_extend(&mut self, value: u64) -> Result<(), String> {
        // Note the assignment happens either way, exactly as upstream — the
        // error does not abandon the value, it only records that the command
        // line will fail.
        let repeated = self.extend != 0;
        self.extend = value;
        if repeated {
            return Err("'/' specifier only allowed with the last value".to_string());
        }
        Ok(())
    }

    /// `+N`: one message if a second `+` value is given.
    fn set_increment(&mut self, value: u64) -> Result<(), String> {
        let repeated = self.increment != 0;
        self.increment = value;
        if repeated {
            return Err("'+' specifier only allowed with the last value".to_string());
        }
        Ok(())
    }

    /// Parse one `-t`/`--tabs` argument and append its stops to the list.
    ///
    /// # Errors
    ///
    /// Every diagnostic the argument produced, in the order upstream prints
    /// them. A non-empty list means the command line must fail with status 1 —
    /// but only *after* the messages are printed, and with no referral.
    ///
    /// Some of these are recoverable enough that parsing continues and a second
    /// message can follow (`-t 1/2/3` reports both slashes), while an invalid
    /// character abandons the rest of the argument. The distinction is
    /// upstream's `break` versus its bare `ok = false`, and it is observable.
    pub fn parse(&mut self, stops: &[u8]) -> Result<(), Vec<String>> {
        let mut errors: Vec<String> = Vec::new();
        let mut have_value = false;
        let mut value: u64 = 0;
        let mut extend_next = false;
        let mut increment_next = false;
        let mut number_start = 0usize;
        let mut at = 0usize;

        while let Some(&c) = stops.get(at) {
            match c {
                // `isblank` in the C locale is these two and nothing else.
                b',' | b' ' | b'\t' => {
                    if have_value {
                        let flushed = if extend_next {
                            self.set_extend(value)
                        } else if increment_next {
                            self.set_increment(value)
                        } else {
                            self.add_stop(value);
                            Ok(())
                        };
                        if let Err(message) = flushed {
                            errors.push(message);
                            // Upstream breaks here but not at end-of-string,
                            // which is only observable when something follows.
                            return Err(errors);
                        }
                    }
                    have_value = false;
                }
                b'/' | b'+' => {
                    if have_value {
                        let which = if c == b'/' { '/' } else { '+' };
                        // The rest of the argument is quoted, not the one
                        // character: `-t 1/2/3` names '/2/3' and then '/3'.
                        errors.push(format!(
                            "'{which}' specifier not at start of number: {}",
                            quote(stops.get(at..).unwrap_or_default())
                        ));
                        // No `break`: the digits that follow keep accumulating
                        // into the *same* value, because `have_value` is left
                        // set. That is why `-t 1/2/3` reports twice rather than
                        // stopping at the first.
                    }
                    extend_next = c == b'/';
                    increment_next = c == b'+';
                }
                b'0'..=b'9' => {
                    if !have_value {
                        value = 0;
                        have_value = true;
                        number_start = at;
                    }
                    let digit = u64::from(c.saturating_sub(b'0'));
                    match value
                        .checked_mul(10)
                        .and_then(|scaled| scaled.checked_add(digit))
                    {
                        Some(next) => value = next,
                        None => {
                            // The whole digit run is named, including the part
                            // that had already been accumulated…
                            let end = stops
                                .iter()
                                .enumerate()
                                .skip(number_start)
                                .find(|(_, b)| !b.is_ascii_digit())
                                .map_or(stops.len(), |(i, _)| i);
                            errors.push(format!(
                                "tab stop is too large {}",
                                quote(stops.get(number_start..end).unwrap_or_default())
                            ));
                            // …and the rest of it is skipped, so one oversized
                            // number produces exactly one message. `have_value`
                            // stays set and `value` keeps its pre-overflow
                            // garbage; that value can still reach `add_stop`,
                            // which does not matter because the command line is
                            // already doomed.
                            at = end;
                            continue;
                        }
                    }
                }
                _ => {
                    errors.push(format!(
                        "tab size contains invalid character(s): {}",
                        quote(stops.get(at..).unwrap_or_default())
                    ));
                    return Err(errors);
                }
            }
            at = at.saturating_add(1);
        }

        if errors.is_empty() && have_value {
            let flushed = if extend_next {
                self.set_extend(value)
            } else if increment_next {
                self.set_increment(value)
            } else {
                self.add_stop(value);
                Ok(())
            };
            if let Err(message) = flushed {
                errors.push(message);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validate the accumulated stops and settle the default.
    ///
    /// Call this once, after the whole command line has been parsed — the
    /// checks are cross-argument, so `-t 4 -t 2` can only be caught here.
    ///
    /// # Errors
    ///
    /// A zero stop, a stop that does not exceed its predecessor, or `/` and `+`
    /// used together. Upstream stops at the first of these, so there is only
    /// ever one message.
    pub fn finalize(&mut self) -> Result<(), String> {
        let mut previous = 0u64;
        for &stop in &self.list {
            if stop == 0 {
                return Err("tab size cannot be 0".to_string());
            }
            if stop <= previous {
                return Err("tab sizes must be ascending".to_string());
            }
            previous = stop;
        }
        if self.increment != 0 && self.extend != 0 {
            return Err("'/' specifier is mutually exclusive with '+'".to_string());
        }

        if self.list.is_empty() {
            // A lone `/N` or `+N` is just `-N`: with no explicit stop to be
            // relative to, both prefixes degrade to a plain size. And with
            // nothing at all it is 8, which is the only place that default
            // lives.
            self.size = match (self.extend, self.increment) {
                (0, 0) => 8,
                (0, increment) => increment,
                (extend, _) => extend,
            };
            self.max_column_width = self.size;
        } else if self.list.len() == 1 && self.extend == 0 && self.increment == 0 {
            // One stop with no prefix does not mean "a single stop at column
            // N and one space per tab thereafter" — it means every N columns.
            self.size = self.list.first().copied().unwrap_or(0);
        } else {
            self.size = 0;
        }
        Ok(())
    }

    /// The column of the next tab stop at or after `column`, or `None` when the
    /// stops are exhausted and a tab is worth a single space.
    ///
    /// `index` is the caller's position in the explicit list and is advanced in
    /// place; a caller that rewinds `column` (which `expand` does on a
    /// backspace) must rewind `index` too.
    #[must_use]
    pub fn next_stop(&self, column: u64, index: &mut usize) -> Option<u64> {
        if self.size != 0 {
            return Some(column.wrapping_add(self.size.wrapping_sub(column % self.size)));
        }
        while let Some(&stop) = self.list.get(*index) {
            if column < stop {
                return Some(stop);
            }
            *index = index.saturating_add(1);
        }
        if self.extend != 0 {
            return Some(column.wrapping_add(self.extend.wrapping_sub(column % self.extend)));
        }
        if self.increment != 0 {
            let last = self.list.last().copied().unwrap_or(0);
            // `column >= last` holds whenever the loop above ran off the end,
            // and a backspace rewinds `index` in step with `column`, so this
            // subtraction cannot wrap in practice. It is written wrapping
            // anyway, because upstream's `uintmax_t` arithmetic would wrap
            // rather than trap and a divergence here would be silent.
            let past = column.wrapping_sub(last);
            return Some(column.wrapping_add(self.increment.wrapping_sub(past % self.increment)));
        }
        None
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Parse one argument, finalize, and return the first eight stops a line
    /// would hit — which is the only thing about a `TabStops` that is
    /// observable from a command line.
    fn stops(arg: &str) -> Vec<u64> {
        let mut t = TabStops::new();
        t.parse(arg.as_bytes()).expect("parse");
        t.finalize().expect("finalize");
        let mut index = 0usize;
        let mut column = 0u64;
        let mut out = Vec::new();
        for _ in 0..8 {
            match t.next_stop(column, &mut index) {
                Some(next) => {
                    out.push(next);
                    column = next;
                }
                None => {
                    out.push(0);
                    column = column.saturating_add(1);
                }
            }
        }
        out
    }

    fn errors(arg: &str) -> Vec<String> {
        let mut t = TabStops::new();
        match t.parse(arg.as_bytes()) {
            Err(e) => e,
            Ok(()) => match t.finalize() {
                Err(e) => vec![e],
                Ok(()) => panic!("expected a diagnostic from {arg:?}"),
            },
        }
    }

    #[test]
    fn no_argument_at_all_is_every_eight_columns() {
        let mut t = TabStops::new();
        t.finalize().unwrap();
        let mut index = 0usize;
        assert_eq!(t.next_stop(0, &mut index), Some(8));
        assert_eq!(t.next_stop(7, &mut index), Some(8));
        assert_eq!(t.next_stop(8, &mut index), Some(16));
    }

    #[test]
    fn one_stop_means_every_n_columns_not_one_stop() {
        assert_eq!(stops("4"), vec![4, 8, 12, 16, 20, 24, 28, 32]);
    }

    #[test]
    fn several_stops_run_out_and_then_a_tab_is_one_space() {
        // 0 stands for "no stop left", which the caller renders as one space.
        assert_eq!(stops("2,4"), vec![2, 4, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn a_slash_prefix_continues_in_multiples_of_itself() {
        // From column 2 the next multiple of 4 is 4, then 8, 12 …
        assert_eq!(stops("2,/4"), vec![2, 4, 8, 12, 16, 20, 24, 28]);
        // The multiples are of the *first* column, so an odd last stop is not
        // where the run continues from.
        assert_eq!(stops("3,/4"), vec![3, 4, 8, 12, 16, 20, 24, 28]);
    }

    #[test]
    fn a_plus_prefix_continues_relative_to_the_last_stop() {
        // From the last stop 2, every 4: 6, 10, 14 …
        assert_eq!(stops("2,+4"), vec![2, 6, 10, 14, 18, 22, 26, 30]);
        // Which differs from `/4` exactly when the last stop is not a multiple.
        assert_eq!(stops("3,+4"), vec![3, 7, 11, 15, 19, 23, 27, 31]);
    }

    #[test]
    fn a_prefix_with_no_explicit_stop_is_just_a_size() {
        assert_eq!(stops("/4"), stops("4"));
        assert_eq!(stops("+4"), stops("4"));
    }

    /// The zero that means "unset", which makes three commands that look like
    /// errors into three spellings of the default.
    #[test]
    fn a_zero_prefix_is_no_prefix_and_a_bare_prefix_is_nothing() {
        assert_eq!(stops("/0"), stops(""));
        assert_eq!(stops("+0"), stops(""));
        assert_eq!(stops("/"), stops(""));
        assert_eq!(stops("+"), stops(""));
        assert_eq!(stops(""), vec![8, 16, 24, 32, 40, 48, 56, 64]);
    }

    #[test]
    fn separators_are_commas_and_blanks_and_empty_entries_vanish() {
        assert_eq!(stops("1,3,5"), stops("1 3 5"));
        assert_eq!(stops("1\t3\t5"), stops("1,3,5"));
        assert_eq!(stops("1,,3"), stops("1,3"));
        assert_eq!(stops(",1,3,"), stops("1,3"));
    }

    #[test]
    fn every_occurrence_appends_rather_than_replacing() {
        let mut t = TabStops::new();
        t.parse(b"1").unwrap();
        t.parse(b"3").unwrap();
        t.finalize().unwrap();
        let mut index = 0usize;
        assert_eq!(t.next_stop(0, &mut index), Some(1));
        assert_eq!(t.next_stop(1, &mut index), Some(3));
        // Two stops, so past the end is one space per tab — not "every 3".
        assert_eq!(t.next_stop(3, &mut index), None);
    }

    #[test]
    fn the_four_parse_diagnostics() {
        assert_eq!(
            errors("1/2"),
            vec!["'/' specifier not at start of number: ‘/2’"]
        );
        assert_eq!(
            errors("1x2"),
            vec!["tab size contains invalid character(s): ‘x2’"]
        );
        assert_eq!(
            errors("4,5x"),
            vec!["tab size contains invalid character(s): ‘x’"]
        );
        assert_eq!(
            errors("99999999999999999999"),
            vec!["tab stop is too large ‘99999999999999999999’"]
        );
    }

    /// A misplaced specifier does not stop the parse, so a second one reports
    /// again — but an invalid character does, so nothing after it is examined.
    #[test]
    fn some_diagnostics_stop_the_parse_and_some_do_not() {
        assert_eq!(
            errors("1/2/3"),
            vec![
                "'/' specifier not at start of number: ‘/2/3’",
                "'/' specifier not at start of number: ‘/3’",
            ]
        );
        assert_eq!(
            errors("1+2+3"),
            vec![
                "'+' specifier not at start of number: ‘+2+3’",
                "'+' specifier not at start of number: ‘+3’",
            ]
        );
        // One message per oversized number, because the rest of the digit run
        // is skipped rather than re-entered.
        assert_eq!(
            errors("99999999999999999999,99999999999999999999"),
            vec![
                "tab stop is too large ‘99999999999999999999’",
                "tab stop is too large ‘99999999999999999999’",
            ]
        );
        // The invalid character wins over anything that would follow it.
        assert_eq!(
            errors("1x/2"),
            vec!["tab size contains invalid character(s): ‘x/2’"]
        );
    }

    #[test]
    fn a_repeated_specifier_is_refused_once() {
        assert_eq!(
            errors("/2,/3"),
            vec!["'/' specifier only allowed with the last value"]
        );
        assert_eq!(
            errors("+2,+3"),
            vec!["'+' specifier only allowed with the last value"]
        );
        // Across two arguments, which is the same list.
        let mut t = TabStops::new();
        t.parse(b"/2").unwrap();
        assert_eq!(
            t.parse(b"/3"),
            Err(vec![
                "'/' specifier only allowed with the last value".to_string()
            ])
        );
    }

    #[test]
    fn the_three_finalize_diagnostics() {
        assert_eq!(errors("0"), vec!["tab size cannot be 0"]);
        assert_eq!(errors("0,4"), vec!["tab size cannot be 0"]);
        assert_eq!(errors("4,2"), vec!["tab sizes must be ascending"]);
        // Equal is not ascending.
        assert_eq!(errors("4,4"), vec!["tab sizes must be ascending"]);
        assert_eq!(
            errors("2,/4,+6"),
            vec!["'/' specifier is mutually exclusive with '+'"]
        );
    }

    #[test]
    fn ascending_is_checked_across_arguments_not_within_one() {
        let mut t = TabStops::new();
        t.parse(b"4").unwrap();
        t.parse(b"2").unwrap();
        assert_eq!(t.finalize(), Err("tab sizes must be ascending".to_string()));
    }

    #[test]
    fn the_widest_gap_is_tracked_for_unexpands_buffer() {
        let mut t = TabStops::new();
        t.parse(b"1,10,12").unwrap();
        t.finalize().unwrap();
        assert_eq!(t.max_column_width(), 9);
        // A single size is its own width.
        let mut t = TabStops::new();
        t.parse(b"5").unwrap();
        t.finalize().unwrap();
        assert_eq!(t.max_column_width(), 5);
        // …including the default, which is only established by `finalize`.
        let mut t = TabStops::new();
        t.finalize().unwrap();
        assert_eq!(t.max_column_width(), 8);
    }

    #[test]
    fn a_tab_stop_argument_need_not_be_utf8() {
        // The message quotes the raw bytes, which is what `quote` is for.
        let mut t = TabStops::new();
        assert_eq!(
            t.parse(b"1\xff"),
            Err(vec![
                "tab size contains invalid character(s): ‘\\377’".to_string()
            ])
        );
    }
}
