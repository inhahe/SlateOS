//! `getopt_long`, in the words glibc actually uses.
//!
//! Every GNU utility's option errors come from one place — glibc's
//! `getopt_long` and gnulib's `argmatch` — so every utility says the same thing
//! when you mistype an option. Ours did not, because each of the 85 binaries
//! parses argv by hand, and a hand-written parser reproduces the sentences its
//! author remembered. This module is the one copy.
//!
//! # These sentences were measured, and recall was wrong four times
//!
//! Not from memory, and not from the `sort` on this host's `PATH` either. That
//! `sort` is MSYS2's, and MSYS2 is a Cygwin derivative: it links `msys-2.0.dll`
//! rather than glibc, and **its getopt is not glibc's**. The two disagree on
//! every message here — `unknown option -- x` against `invalid option -- 'x'` —
//! so a differential harness pointed at it certifies wording no GNU/Linux
//! system prints. See `known-issues.md`
//! → `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`. The reference is
//! glibc, reached through WSL.
//!
//! # The five sentences
//!
//! A short option and a long one get *different* sentences, which is the detail
//! a hand-written parser always misses:
//!
//! | Command | glibc |
//! |---|---|
//! | `sort -x` | `invalid option -- 'x'` |
//! | `sort -k` | `option requires an argument -- 'k'` |
//! | `sort --fo` | `unrecognized option '--fo'` |
//! | `sort --k` | `option '--key' requires an argument` |
//! | `sort --s` | `option '--s' is ambiguous; possibilities: '--sort' '--stable'` |
//! | `sort --stab=x` | `option '--stable' doesn't allow an argument` |
//!
//! Note which name each carries. The two that resolved nothing echo what was
//! **typed**, `=VALUE` and all — `--fo=bar` is reported as `'--fo=bar'`. The two
//! that resolved something name the **resolution** — `--k` is reported as
//! `'--key'`, `--stab=x` as `'--stable'`.
//!
//! # Why the names are quoted
//!
//! Every name here goes through [`crate::quote`], which is where this parts
//! company with glibc on purpose. glibc writes the name between two literal `'`
//! and escapes nothing between them. A path may hold every byte but `/` and
//! NUL, so a file called
//!
//! ```text
//! --fo⏎sort: /etc/shadow: Permission denied
//! ```
//!
//! picked up by `sort *`, makes glibc print a second line that `sort` never
//! wrote. For every option a person would actually type the two are
//! byte-identical — `'--fo'` is `'--fo'` — and they differ only where glibc
//! would emit a raw control byte into a diagnostic. It is the same argument,
//! and the same fix, as the sweep that put every *file* name in a diagnostic
//! through `quote`.

use crate::quote::quote;

/// What a long option does with a value, matching `getopt_long`'s three cases.
///
/// The distinction between `Required` and `Optional` is observable, not
/// bookkeeping: an optional value must be written `--check=quiet`, because
/// `--check quiet` leaves `quiet` an operand. That is why GNU answers
/// `sort --check quiet` with `open failed: quiet` rather than with a check.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Takes {
    Nothing,
    Required,
    Optional,
}

/// A command line that cannot be run, and the status to exit with.
///
/// The status is part of the return value rather than a note in a doc comment
/// because it is **not** constant, and it varies along two axes that no
/// reimplementer would guess. Both were measured, across 28 utilities:
///
/// 1. **A bad option exits with the utility's own usage status**, which is 1
///    for almost everything but **2 for `ls`, `sort` and `grep`** — the ones
///    that have already spent 1 on a real answer (`sort -c` found the input
///    unsorted, `grep` matched nothing). That is [`Program`]'s second field,
///    and it has no default precisely because the minority is not derivable.
/// 2. **A bad *argument to* an option is 1 for everybody**, `ls` and `sort`
///    included, because those go through gnulib's `argmatch`, which dies with
///    the generic `EXIT_FAILURE` rather than the caller's usage status. So
///    `ls --zzz` is 2 while `ls --sort=zzz` is 1, in the same program, which
///    reads like an upstream oversight and is reproduced rather than tidied.
///
/// A caller therefore never supplies a status: it states its usage status once
/// when it builds its [`Program`], and this module applies rule 2 on top.
///
/// # The sentence and the referral are two things, not one string
///
/// Upstream they are produced by two different pieces of code. `getopt_long`
/// prints only the sentence — `nl: invalid option -- 'Z'` — and returns `'?'`;
/// the `Try 'nl --help' for more information.` line comes later, from the
/// caller's own `usage (EXIT_FAILURE)`. Most utilities call `usage` on the spot,
/// so the two always appear together and look like one message. `nl` is the
/// counter-example that makes the distinction visible: its option loop sets an
/// `ok` flag and keeps going, so `nl -Z -bX` prints *two* sentences and *one*
/// referral, in encounter order.
///
/// So [`sentence`](Error::sentence) holds the diagnostic's own text and
/// [`referral`](Error::referral) names the program when the referral follows it.
/// [`Error::message`] joins them, which is what a utility that stops at the
/// first error prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// The diagnostic's own text, with no referral and no `program: ` prefix.
    ///
    /// May be several lines — `argmatch`'s carries its list of valid arguments —
    /// so a caller must not assume it fits one `eprintln!` line's worth of
    /// framing.
    pub sentence: String,
    /// The program name, when this diagnostic is followed by `Try '… --help'`.
    ///
    /// Which shape a message takes is measured per message, not chosen: it is
    /// whether upstream called `error (EXIT_FAILURE, …)` or `error (0, …)` plus
    /// `usage (EXIT_FAILURE)`.
    pub referral: Option<&'static str>,
    pub status: i32,
}

impl Error {
    /// The full text to print: the sentence, then the referral if there is one.
    #[must_use]
    pub fn message(&self) -> String {
        match self.referral {
            Some(program) => format!("{}{}", self.sentence, try_help(program)),
            None => self.sentence.clone(),
        }
    }
}

/// The message as printed; the status is acted on, not shown.
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.sentence)?;
        match self.referral {
            Some(program) => f.write_str(&try_help(program)),
            None => Ok(()),
        }
    }
}

/// The referral every one of these diagnostics ends with.
fn try_help(program: &str) -> String {
    format!("\nTry '{program} --help' for more information.")
}

/// A getopt diagnostic: the sentence, the referral, and the caller's usage
/// status.
fn sentence(program: Program, body: &str) -> Error {
    Error {
        sentence: body.to_string(),
        referral: Some(program.name),
        status: program.usage_status,
    }
}

/// Render an option name — a byte string — as glibc would if glibc escaped.
fn named(name: &[u8]) -> String {
    quote(name)
}

/// Render a resolved long option, which is a table entry and so always plain
/// ASCII. It is quoted anyway rather than relying on that staying true.
fn named_long(resolved: &str) -> String {
    quote(format!("--{resolved}").as_bytes())
}

/// A utility's name and usage status, bound once so that every diagnostic it
/// produces carries the right ones.
///
/// It is a type rather than two parameters on each call because these are going
/// into 85 binaries, most of which will be converted by copying an already
/// converted neighbour — and parameters make `cat` printing `sort:`, or
/// exiting 2 where GNU's `cat` exits 1, a silent one-token slip that `cat`'s
/// own tests need not catch. Bound once at the top of a `main.rs`, neither can
/// drift:
///
/// ```
/// use coreutils::getopt::Program;
/// const CAT: Program = Program::new("cat", 1);
/// let e = CAT.invalid_option(b'x');
/// assert_eq!(e.sentence, "invalid option -- 'x'");
/// assert_eq!(e.message(), "invalid option -- 'x'\nTry 'cat --help' for more information.");
/// assert_eq!(e.status, 1);
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Program {
    name: &'static str,
    usage_status: i32,
}

impl Program {
    /// Name the utility and state what it exits with on a bad command line.
    ///
    /// `usage_status` has no default because the value that would be wrong is
    /// not the rare one. Measured across 28 utilities it is **1** for almost
    /// everything — `cat`, `head`, `wc`, `cp`, `tr`, `cut`, `sed`, `date` — and
    /// **2** for `ls`, `sort` and `grep`, which have already given 1 a meaning
    /// (`sort -c` found the input unsorted; `grep` matched nothing). Measure a
    /// new utility with `<util> --zzz-bogus; echo $?` rather than assuming.
    #[must_use]
    pub const fn new(name: &'static str, usage_status: i32) -> Self {
        Program { name, usage_status }
    }

    /// The utility's *own* usage errors — `invalid field specification '0'`,
    /// `invalid --parallel argument 'x'` — rather than getopt's.
    ///
    /// They take the same status as a getopt error but, measured, carry **no**
    /// `Try '… --help'` referral. This exists so that a caller with
    /// hand-written usage messages still gets its status from one place instead
    /// of writing the number out again.
    ///
    /// Use [`Program::usage_referring`] for the ones that *do* refer; upstream
    /// has both shapes and the difference is visible output.
    #[must_use]
    pub fn usage(self, message: String) -> Error {
        Error {
            sentence: message,
            referral: None,
            status: self.usage_status,
        }
    }

    /// The utility's own usage errors that *do* refer the reader to `--help`.
    ///
    /// Which shape a given message takes is not a matter of taste upstream, it
    /// is which function was called, so it is measured per message rather than
    /// guessed. `sort -k0` calls `error (SORT_FAILURE, …)`, which prints one
    /// line and stops. `wc --files0-from=- FILE` calls `error (0, …)` and then
    /// `usage (EXIT_FAILURE)`, which adds the referral:
    ///
    /// ```text
    /// wc: extra operand 'w1'
    /// file operands cannot be combined with --files0-from
    /// Try 'wc --help' for more information.
    /// ```
    ///
    /// `message` may be several lines, as it is there — only the first carries
    /// the `wc: ` prefix, which is the caller's business because the caller is
    /// what prints it.
    #[must_use]
    pub fn usage_referring(self, message: String) -> Error {
        sentence(self, &message)
    }

    /// `sort -x` → `invalid option -- 'x'`.
    #[must_use]
    pub fn invalid_option(self, flag: u8) -> Error {
        sentence(self, &format!("invalid option -- {}", named(&[flag])))
    }

    /// `sort -k` → `option requires an argument -- 'k'`.
    ///
    /// Note this is *not* the same sentence as the long form below; glibc puts the
    /// name last for a short option and first for a long one.
    #[must_use]
    pub fn short_missing_argument(self, flag: u8) -> Error {
        sentence(
            self,
            &format!("option requires an argument -- {}", named(&[flag])),
        )
    }

    /// `sort --fo` → `unrecognized option '--fo'`.
    ///
    /// `whole` is the argument exactly as typed, `--` and any `=VALUE` included,
    /// because there is nothing resolved to name instead.
    #[must_use]
    pub fn unrecognized_option(self, whole: &[u8]) -> Error {
        sentence(self, &format!("unrecognized option {}", named(whole)))
    }

    /// `sort --k` → `option '--key' requires an argument`, naming the resolution.
    #[must_use]
    pub fn long_missing_argument(self, resolved: &str) -> Error {
        sentence(
            self,
            &format!("option {} requires an argument", named_long(resolved)),
        )
    }

    /// `sort --stab=x` → `option '--stable' doesn't allow an argument`.
    #[must_use]
    pub fn long_unwanted_argument(self, resolved: &str) -> Error {
        sentence(
            self,
            &format!("option {} doesn't allow an argument", named_long(resolved)),
        )
    }

    /// `getopt_long`'s name resolution: an exact match wins outright, otherwise the
    /// name must be a prefix of exactly one option.
    ///
    /// The exact-match rule is not redundant with the prefix rule — `--version` is
    /// a prefix of `--version-sort`, so without it every `sort --version` would be
    /// refused as ambiguous.
    ///
    /// **The table's order is load-bearing, and must be GNU's rather than
    /// alphabetical.** `getopt_long` lists an ambiguous prefix's candidates in the
    /// order they appear in the caller's `struct option[]`, so the array order is
    /// observable output:
    ///
    /// ```text
    /// $ sort --r
    /// sort: option '--r' is ambiguous; possibilities: '--random-sort' '--random-source' '--reverse'
    /// ```
    ///
    /// Measure it rather than recalling it — recall got `sort`'s wrong, where
    /// `--random-sort` precedes `--random-source`. The instrument is one command:
    /// an empty prefix matches every option, so `sort --=x` prints the whole table
    /// in declaration order.
    ///
    /// A table must also list options the utility does **not** implement, because
    /// the table is what decides whether an abbreviation is ambiguous. Drop
    /// `--debug` from `sort`'s and `--d` silently resolves to `--dictionary-order`,
    /// giving a user who meant `--debug` a dictionary sort instead of an error.
    ///
    /// # Errors
    ///
    /// The typed name matching no option, or more than one.
    pub fn resolve_long<'t, T: Copy>(
        self,
        typed: &str,
        whole: &[u8],
        table: &'t [(&'t str, T)],
    ) -> Result<(&'t str, T), Error> {
        if let Some(hit) = table.iter().find(|(n, _)| *n == typed) {
            return Ok(*hit);
        }
        let matches: Vec<_> = table.iter().filter(|(n, _)| n.starts_with(typed)).collect();
        match matches.as_slice() {
            [] => Err(self.unrecognized_option(whole)),
            [only] => Ok(**only),
            many => {
                let list: Vec<String> = many.iter().map(|(n, _)| named_long(n)).collect();
                Err(sentence(
                    self,
                    &format!(
                        "option {} is ambiguous; possibilities: {}",
                        named(whole),
                        list.join(" ")
                    ),
                ))
            }
        }
    }

    /// gnulib's `argmatch`: resolve an option's *argument* against its list of
    /// words.
    ///
    /// It is a prefix match, exactly like `resolve_long`'s treatment of option
    /// names — `sort --sort=hum` and `sort --check=q` both work, and a utility that
    /// only accepts the full spelling refuses valid commands.
    ///
    /// Ambiguity is judged by **value, not by spelling**. `--check=` matches
    /// `quiet`, `silent` and `diagnose-first` and is refused; a prefix matching only
    /// `quiet` and `silent` would be accepted, because both mean the same thing and
    /// there is nothing for the user to disambiguate. That is why the table pairs
    /// spellings with values rather than listing spellings alone — and why a value
    /// type that is `PartialEq` is required rather than convenient.
    ///
    /// The caller supplies the option's own name (`"--check"`) for the message.
    /// These carry status **1** where the getopt errors above carry 2; see
    /// [`Error`] for why, and why the caller does not get to choose.
    ///
    /// # Errors
    ///
    /// The given word matching no valid word, or several that disagree.
    pub fn argmatch<T: Copy + PartialEq>(
        self,
        given: &[u8],
        option: &str,
        table: &[(&str, T)],
    ) -> Result<T, Error> {
        let hits: Vec<&(&str, T)> = match std::str::from_utf8(given) {
            // A word that is not UTF-8 cannot be a prefix of any of these, which
            // are all ASCII; it takes the no-match path rather than being an error
            // of its own.
            Err(_) => Vec::new(),
            Ok(text) => table.iter().find(|(w, _)| *w == text).map_or_else(
                || table.iter().filter(|(w, _)| w.starts_with(text)).collect(),
                |exact| vec![exact],
            ),
        };
        let Some((_, first)) = hits.first() else {
            return Err(bad_argument(self, "invalid", given, option, table));
        };
        if hits.iter().any(|(_, v)| v != first) {
            return Err(bad_argument(self, "ambiguous", given, option, table));
        }
        Ok(*first)
    }
}

/// `argmatch`'s two diagnostics, which differ only in that first word.
///
/// The "Valid arguments are" list is generated from the same table the match
/// used rather than written out beside it, because the two must agree and a
/// hand-written copy is what drifts. Runs of words sharing a value are joined
/// onto one line, which is both gnulib's rendering and the reason it is worth
/// generating: `'quiet', 'silent'` on one line *is* the statement that they
/// mean the same thing, and it is the same fact the matcher uses to decide that
/// a prefix hitting both is not ambiguous.
fn bad_argument<T: PartialEq>(
    program: Program,
    what: &str,
    given: &[u8],
    option: &str,
    table: &[(&str, T)],
) -> Error {
    let mut lines: Vec<String> = Vec::new();
    let mut prev: Option<&T> = None;
    for (word, value) in table {
        let spelled = quote(word.as_bytes());
        match (prev, lines.last_mut()) {
            (Some(p), Some(line)) if p == value => {
                line.push_str(", ");
                line.push_str(&spelled);
            }
            _ => lines.push(format!("  - {spelled}")),
        }
        prev = Some(value);
    }
    Error {
        // Always 1, overriding the caller's usage status rather than taking it
        // — measured: `ls --zzz` is 2 but `ls --sort=zzz` is 1. See [`Error`].
        status: 1,
        ..sentence(
            program,
            &format!(
                "{what} argument {} for {}\nValid arguments are:\n{}",
                named(given),
                named(option.as_bytes()),
                lines.join("\n")
            ),
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    const SORT: Program = Program::new("sort", 2);

    /// The literals below are glibc's, taken from `sort` under `LC_ALL=C`. They
    /// are here rather than only in a differential harness so that they are
    /// checked on a host with no reference `sort` at all — and because the
    /// harness on *this* host spent a long time comparing against MSYS2's
    /// non-glibc getopt and reporting agreement.
    const SORT_LONGS: &[(&str, Takes)] = &[
        ("ignore-leading-blanks", Takes::Nothing),
        ("check", Takes::Optional),
        ("random-sort", Takes::Nothing),
        ("random-source", Takes::Required),
        ("sort", Takes::Required),
        ("reverse", Takes::Nothing),
        ("stable", Takes::Nothing),
        ("key", Takes::Required),
    ];

    fn without_referral(err: &Error) -> String {
        assert_eq!(
            err.referral,
            Some("sort"),
            "every diagnostic here ends with the referral"
        );
        err.sentence.clone()
    }

    #[test]
    fn a_short_option_and_a_long_one_get_different_sentences() {
        assert_eq!(
            without_referral(&SORT.invalid_option(b'x')),
            "invalid option -- 'x'"
        );
        assert_eq!(
            without_referral(&SORT.short_missing_argument(b'k')),
            "option requires an argument -- 'k'"
        );
        // The long form puts the name first and spells it out in full.
        assert_eq!(
            without_referral(&SORT.long_missing_argument("key")),
            "option '--key' requires an argument"
        );
        assert_eq!(
            without_referral(&SORT.long_unwanted_argument("stable")),
            "option '--stable' doesn't allow an argument"
        );
        assert_eq!(
            without_referral(&SORT.unrecognized_option(b"--fo")),
            "unrecognized option '--fo'"
        );
    }

    #[test]
    fn an_exact_match_beats_a_prefix_it_is_also_a_prefix_of() {
        // `sort` is a prefix of nothing here, but `random-so` is ambiguous
        // while the full `random-sort` is not.
        let (name, takes) = SORT
            .resolve_long("random-sort", b"--random-sort", SORT_LONGS)
            .unwrap();
        assert_eq!((name, takes), ("random-sort", Takes::Nothing));
        assert!(
            SORT.resolve_long("random-so", b"--random-so", SORT_LONGS)
                .is_err()
        );
        // An unambiguous abbreviation resolves, and reports the option it
        // resolved to rather than what was typed.
        let (name, _) = SORT.resolve_long("k", b"--k", SORT_LONGS).unwrap();
        assert_eq!(name, "key");
    }

    #[test]
    fn the_ambiguous_list_is_in_the_tables_order_not_alphabetical() {
        let err = SORT.resolve_long("r", b"--r", SORT_LONGS).unwrap_err();
        assert_eq!(
            without_referral(&err),
            "option '--r' is ambiguous; possibilities: \
             '--random-sort' '--random-source' '--reverse'"
        );
        // Alphabetically `random-source` would come first. It does not, because
        // the order is GNU's table order, and this is what would catch a table
        // that had been "tidied" into alphabetical order.
        let m = &err.sentence;
        assert!(m.find("'--random-sort'") < m.find("'--random-source'"));
    }

    #[test]
    fn a_name_that_resolved_nothing_is_echoed_whole() {
        // `=VALUE` included: there is no resolved option to name instead.
        assert_eq!(
            without_referral(
                &SORT
                    .resolve_long("fo", b"--fo=bar", SORT_LONGS)
                    .unwrap_err()
            ),
            "unrecognized option '--fo=bar'"
        );
    }

    #[test]
    fn an_option_name_cannot_forge_a_second_diagnostic_line() {
        let forged = SORT.unrecognized_option(b"--fo\nsort: /etc/shadow: Permission denied");
        assert_eq!(
            without_referral(&forged),
            r#"unrecognized option '--fo\nsort: /etc/shadow: Permission denied'"#
        );
        // One line, which is the whole point: glibc's would be two.
        assert_eq!(without_referral(&forged).lines().count(), 1);
        // A byte is reported as the byte it was. Rendering it through `char`
        // would map 0xC3 to `Ã` and re-encode it as two bytes — an option
        // nobody typed.
        assert_eq!(
            without_referral(&SORT.invalid_option(0xC3)),
            r"invalid option -- '\303'"
        );
    }

    const CHECK_WORDS: &[(&str, u8)] = &[("quiet", 0), ("silent", 0), ("diagnose-first", 1)];

    #[test]
    fn an_option_argument_abbreviates_like_an_option_name() {
        assert_eq!(SORT.argmatch(b"q", "--check", CHECK_WORDS), Ok(0));
        assert_eq!(SORT.argmatch(b"d", "--check", CHECK_WORDS), Ok(1));
        assert_eq!(SORT.argmatch(b"quiet", "--check", CHECK_WORDS), Ok(0));
        assert_eq!(SORT.argmatch(b"s", "--check", CHECK_WORDS), Ok(0));
    }

    #[test]
    fn ambiguity_is_judged_by_value_and_not_by_spelling() {
        // The empty string matches all three words, which disagree.
        let err = SORT.argmatch(b"", "--check", CHECK_WORDS).unwrap_err();
        assert_eq!(
            without_referral(&err),
            "ambiguous argument '' for '--check'\nValid arguments are:\n  \
             - 'quiet', 'silent'\n  - 'diagnose-first'"
        );
        // A prefix matching only the two synonyms is *not* ambiguous: there is
        // nothing for the user to disambiguate. This is the case a
        // spelling-counting implementation gets wrong.
        let synonyms: &[(&str, u8)] = &[("quiet", 0), ("quiescent", 0), ("loud", 1)];
        assert_eq!(SORT.argmatch(b"qu", "--check", synonyms), Ok(0));
        assert!(SORT.argmatch(b"", "--check", synonyms).is_err());
    }

    #[test]
    fn an_invalid_argument_is_a_different_word_from_an_ambiguous_one() {
        let err = SORT.argmatch(b"bogus", "--check", CHECK_WORDS).unwrap_err();
        assert!(
            without_referral(&err).starts_with("invalid argument 'bogus' for '--check'"),
            "{err}"
        );
        // Not UTF-8: cannot prefix any of these ASCII words, so it takes the
        // no-match path rather than erroring differently.
        let err = SORT
            .argmatch(b"\xc3\xa9", "--check", CHECK_WORDS)
            .unwrap_err();
        assert!(
            without_referral(&err).starts_with(r"invalid argument '\303\251' for '--check'"),
            "{err}"
        );
    }

    #[test]
    fn the_valid_list_is_generated_from_the_table_that_did_the_matching() {
        // Three words, two values, so two lines — the grouping states which
        // spellings mean the same thing, and cannot drift from the matcher
        // because it is derived from the matcher's own table.
        let err = SORT.argmatch(b"bogus", "--check", CHECK_WORDS).unwrap_err();
        assert!(
            err.sentence
                .contains("  - 'quiet', 'silent'\n  - 'diagnose-first'"),
            "{err:?}"
        );
        // All-distinct values put every word on its own line.
        let distinct: &[(&str, u8)] = &[("month", 0), ("numeric", 1)];
        let err = SORT.argmatch(b"x", "--sort", distinct).unwrap_err();
        assert!(
            err.sentence.contains("  - 'month'\n  - 'numeric'"),
            "{err:?}"
        );
    }

    /// The two statuses, which are the reason [`Error`] carries one at all.
    ///
    /// A caller cannot get these wrong because it never supplies them — which
    /// is the point, since the split is an upstream quirk rather than anything
    /// a reimplementer would arrive at independently.
    #[test]
    fn a_bad_option_and_a_bad_option_argument_exit_differently() {
        assert_eq!(SORT.invalid_option(b'x').status, 2);
        assert_eq!(
            SORT.resolve_long("fo", b"--fo", SORT_LONGS)
                .unwrap_err()
                .status,
            2
        );
        assert_eq!(
            SORT.argmatch(b"bogus", "--check", CHECK_WORDS)
                .unwrap_err()
                .status,
            1
        );
        // A utility's own usage message takes its usage status, and no referral.
        let own = SORT.usage("field number is zero".to_string());
        assert_eq!(own.status, 2);
        assert_eq!(own.referral, None, "{own:?}");
        // The same three calls for a status-1 utility, which is most of them.
        // `argmatch` stays 1 for both: that is the whole oddity.
        const CAT: Program = Program::new("cat", 1);
        assert_eq!(CAT.invalid_option(b'x').status, 1);
        assert_eq!(
            CAT.argmatch(b"bogus", "--check", CHECK_WORDS)
                .unwrap_err()
                .status,
            1
        );
        assert_eq!(
            CAT.invalid_option(b'x').message(),
            "invalid option -- 'x'\nTry 'cat --help' for more information."
        );
    }
}
