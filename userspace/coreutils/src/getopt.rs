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

//! # The walk over argv
//!
//! [`Program::parse`] is the loop `getopt_long` itself is: it yields one
//! [`Opt`] per option or operand, in the order they were typed, and produces
//! the five sentences above when it cannot. Everything a utility does with the
//! result stays in the utility, because that part genuinely differs.
//!
//! It exists because of the four spellings an option's *value* can take, all of
//! which GNU accepts and all of which a hand-written parser gets wrong in a
//! different way:
//!
//! ```text
//! -r FILE      --reference FILE
//! -rFILE       --reference=FILE
//! ```
//!
//! and because of the cases around them that are easy to miss: a bundle whose
//! last letter takes a value (`touch -cr ref f`), a value-taking option at the
//! very end of argv (two different sentences, short and long), an *empty* value
//! (`--time=`, which must reach `argmatch` rather than count as absent), and an
//! option the utility does not implement, which must still consume its value —
//! or the `2001-01-01` in `touch -d 2001-01-01 f` is left behind to be created
//! as a file.
//!
//! # Why it yields items one at a time instead of returning a list
//!
//! Because `--help` has to win over a bad option that follows it, and lose to
//! one that precedes it. Measured: `readlink --help --bogus` prints the help and
//! exits 0, while `readlink --bogus --help` is an error. `getopt_long` is called
//! in a loop and the caller acts on each answer before asking for the next, so
//! ordering falls out; a parser that validated the whole of argv before handing
//! anything back would turn the first of those into an error.

use std::ffi::OsString;

// Both, and the difference is load-bearing: `quote` is gnulib's locale-aware
// style (curly under UTF-8) and belongs only to `bad_argument`; `quote_glibc`
// is glibc's straight-marked one and belongs to every other diagnostic here.
use crate::quote::{os_bytes, os_from_bytes, quote, quote_glibc};

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
///
/// **Straight marks, not [`quote`]'s curly ones.** Every diagnostic in this
/// file except [`bad_argument`]'s is glibc's rather than gnulib's, and glibc
/// spells the quotes into its format strings instead of asking the locale, so
/// these stay `'--key'` under a UTF-8 locale where an argmatch message has
/// become `‘quiet’`. Measured: `sort --key` and `sort --sort=zzz` disagree in
/// exactly this way. See `quote::quote_glibc`.
fn named(name: &[u8]) -> String {
    quote_glibc(name)
}

/// Render a resolved long option, which is a table entry and so always plain
/// ASCII. It is quoted anyway rather than relying on that staying true.
///
/// Straight marks, for the same reason as [`named`].
fn named_long(resolved: &str) -> String {
    quote_glibc(format!("--{resolved}").as_bytes())
}

/// Which option a spelling *is*, for the ambiguity test in
/// [`Program::resolve_long_aliased`]. Our stand-in for GNU's `struct option`
/// `val`, which is what `getopt_long` actually compares.
///
/// A spelling that is nobody's alias is its own identity, so a table with no
/// aliases behaves exactly as it did before this existed. The map is not
/// followed transitively — GNU has no chain of aliases, and a one-step lookup
/// cannot loop.
///
/// The result is only ever *compared*, never printed. What gets printed is the
/// table entry itself, which is why the resolved name stays the spelling the
/// table declared rather than the alias's target; see
/// [`Program::resolve_long_aliased`].
fn identity<'a>(name: &'a str, aliases: &'a [(&'a str, &'a str)]) -> &'a str {
    aliases
        .iter()
        .find(|(spelling, _)| *spelling == name)
        .map_or(name, |&(_, target)| target)
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

    /// Print `name: <error>` to stderr — the whole of what a utility that stops
    /// at the first command-line error does with one.
    ///
    /// Every bin here spells that out as `eprintln!("tee: {e}")`, which is fine
    /// when the name is a literal. It stops being fine in a module shared by
    /// several programs, where the name is data: the literal becomes
    /// `"{}: {e}"`, and `scripts/host-errmsg.py` — which exempts the shape
    /// `<name>: {e}` precisely because it is always a [`Error`] and never an
    /// `io::Error` — cannot tell that from a real site printing the host's
    /// error text with one word of context. Naming the operation removes the
    /// literal instead of widening the gate to admit a shape it cannot check.
    ///
    /// The status is deliberately not returned. It is [`Error::status`], the
    /// caller is what exits, and a function that both printed and produced an
    /// exit code would be used by callers that only wanted one of the two.
    pub fn report(self, e: &Error) {
        eprintln!("{}: {}", self.name, e.message());
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
        self.resolve_long_aliased(typed, whole, table, &[])
    }

    /// [`resolve_long`](Self::resolve_long), for a table in which two spellings
    /// name **one** option.
    ///
    /// GNU's `struct option` carries a `val` — the integer the option resolves
    /// to — and `getopt_long` judges ambiguity by *that*, not by the name. Our
    /// table is keyed by name and so has no `val`; for the handful of tables
    /// where a second spelling exists, `aliases` supplies what `val` would have
    /// told us. Each entry is `(spelling, the option that spelling *is*)`.
    ///
    /// Without it, a deprecated alias makes its own option unabbreviatable.
    /// `rmdir`'s table holds both `--path` and `--parents`, which are the same
    /// option, and GNU accepts `rmdir --p`:
    ///
    /// ```text
    /// $ rmdir --p a/b        # measured: succeeds, removes b then a
    /// $ cp --p x y
    /// cp: option '--p' is ambiguous; possibilities: '--parents' '--preserve'
    /// ```
    ///
    /// The `cp` line is the one that pins the rule, and it is worth reading
    /// twice: `--p` is a prefix of `--parents`, `--path` **and** `--preserve`,
    /// yet only two are listed. `--path` is dropped because it is the same
    /// option as `--parents`, and `--preserve` is listed because it is not — so
    /// this is not "aliases are hidden", it is "an alias is not a second
    /// candidate".
    ///
    /// **The resolved name is the first *table* match, not the alias's target.**
    /// glibc returns `pfound`, the earliest entry that matched, and names it in
    /// any later diagnostic — measured, and the two utilities disagree because
    /// their tables are ordered differently:
    ///
    /// ```text
    /// $ rmdir --pa=1        # table order: … '--path' '--parents' …
    /// rmdir: option '--path' doesn't allow an argument
    /// $ cp --pa=1           # table order: … '--parents' '--path' …
    /// cp: option '--parents' doesn't allow an argument
    /// ```
    ///
    /// So a caller with an alias in its table must handle **both** spellings —
    /// `"path" | "parents" => …` — rather than expecting one canonical name.
    /// That is deliberate: canonicalising here would be one line, and would
    /// silently make the message above name an option the user did not type.
    ///
    /// # Errors
    ///
    /// The typed name matching no option, or more than one *distinct* option.
    pub fn resolve_long_aliased<'t, T: Copy>(
        self,
        typed: &str,
        whole: &[u8],
        table: &'t [(&'t str, T)],
        aliases: &[(&str, &str)],
    ) -> Result<(&'t str, T), Error> {
        if let Some(hit) = table.iter().find(|(n, _)| *n == typed) {
            return Ok(*hit);
        }
        let matches: Vec<_> = table.iter().filter(|(n, _)| n.starts_with(typed)).collect();
        let Some(first) = matches.first() else {
            return Err(self.unrecognized_option(whole));
        };
        // glibc compares every later match against `pfound` — the first one —
        // and never against each other, so a table `[A, B, B']` where `B'`
        // aliases `B` lists all three. Mirroring that exactly matters: the list
        // is user-visible output.
        let first_id = identity(first.0, aliases);
        let ambiguous: Vec<_> = matches
            .iter()
            .filter(|(n, _)| identity(n, aliases) != first_id)
            .collect();
        if ambiguous.is_empty() {
            return Ok(**first);
        }
        let mut list: Vec<String> = vec![named_long(first.0)];
        list.extend(ambiguous.iter().map(|(n, _)| named_long(n)));
        // Back into table order: `first` is the earliest match by construction,
        // and `ambiguous` preserves the table's order among the rest, so
        // prepending `first` is already sorted. Stated rather than assumed
        // because the order is asserted on in tests.
        Err(sentence(
            self,
            &format!(
                "option {} is ambiguous; possibilities: {}",
                named(whole),
                list.join(" ")
            ),
        ))
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

    /// Walk argv the way `getopt_long` does. See the module docs.
    ///
    /// `shorts` is GNU's own `getopt_long` string for the utility, copied
    /// verbatim — `"acd:fhmr:t:"` for `touch`. One colon after a letter means
    /// the option requires a value, two mean it takes one optionally, none mean
    /// it takes none. Copy it rather than deriving it: it must list options the
    /// utility does **not** implement, or their values are left behind as
    /// operands (see the module docs). A leading `+` is honoured and means
    /// "stop at the first operand", as in `nice` and `env`; the other two
    /// leading markers glibc understands, `-` and `:`, have no coreutils caller
    /// and are not implemented — a table using one would see it read as an
    /// option letter.
    ///
    /// `longs` is the same table [`resolve_long`](Self::resolve_long) takes,
    /// **in GNU's declaration order**, which is observable.
    ///
    /// The result is an iterator of `Result`, so a caller handles each item as
    /// it arrives:
    ///
    /// ```
    /// use coreutils::getopt::{Opt, Program, Takes};
    /// use std::ffi::OsString;
    ///
    /// const TOUCH: Program = Program::new("touch", 1);
    /// const LONGS: &[(&str, Takes)] = &[("reference", Takes::Required)];
    ///
    /// let argv: Vec<OsString> = ["-cr", "ref", "f"].iter().map(OsString::from).collect();
    /// let got: Vec<Opt<'_>> = TOUCH
    ///     .parse(&argv, "acd:fhmr:t:", LONGS)
    ///     .collect::<Result<_, _>>()
    ///     .unwrap();
    /// assert_eq!(got[0], Opt::Short(b'c', None));
    /// assert_eq!(got[1], Opt::Short(b'r', Some(OsString::from("ref"))));
    /// assert_eq!(got[2], Opt::Operand(&argv[2]));
    /// ```
    ///
    /// # Errors
    ///
    /// Yielded rather than returned: an unknown short option, a long name
    /// resolving to none or to several, a value given to an option that takes
    /// none, or a required value missing. The iterator ends after its first
    /// error — glibc's own loop carries on, which only `nl` makes visible, and
    /// no converted utility does.
    #[must_use]
    pub fn parse<'a>(
        self,
        argv: &'a [OsString],
        shorts: &'a str,
        longs: &'a [(&'a str, Takes)],
    ) -> Parser<'a> {
        self.parse_aliased(argv, shorts, longs, &[])
    }

    /// [`parse`](Self::parse), for a table in which two spellings name one
    /// option. See [`resolve_long_aliased`](Self::resolve_long_aliased) for why
    /// that needs saying and what goes wrong without it.
    #[must_use]
    pub fn parse_aliased<'a>(
        self,
        argv: &'a [OsString],
        shorts: &'a str,
        longs: &'a [(&'a str, Takes)],
        aliases: &'a [(&'a str, &'a str)],
    ) -> Parser<'a> {
        let (stop_at_operand, shorts) = match shorts.strip_prefix('+') {
            Some(rest) => (true, rest),
            None => (false, shorts),
        };
        Parser {
            program: self,
            argv,
            at: 0,
            shorts,
            longs,
            aliases,
            word: 0,
            cluster: Vec::new(),
            only_operands: false,
            stop_at_operand,
            done: false,
        }
    }
}

/// One thing found on the command line, in the order it was typed.
///
/// The asymmetry between the two payloads is deliberate. An operand is
/// **borrowed**, because it is a word of argv unchanged and a file name must
/// not be copied through anything that could reinterpret it. A value is
/// **owned**, because it is often only part of a word — the `FILE` of `-rFILE`
/// — and so has to be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opt<'a> {
    /// A short option, and its value if [`Takes`] says it has one.
    Short(u8, Option<OsString>),
    /// A long option, named as the **table** spells it rather than as it was
    /// typed, and its value. With an alias in the table that name may be either
    /// spelling; see [`Program::resolve_long_aliased`].
    Long(&'a str, Option<OsString>),
    /// A non-option argument, exactly as it was typed.
    Operand(&'a OsString),
}

/// The walk [`Program::parse`] returns. See the module docs.
pub struct Parser<'a> {
    program: Program,
    argv: &'a [OsString],
    /// The next word of argv to read. Advanced by the outer loop and *also* by
    /// an option that consumes the following word as its value, which is why
    /// this is an index rather than a `for` loop over `argv`.
    at: usize,
    shorts: &'a str,
    longs: &'a [(&'a str, Takes)],
    aliases: &'a [(&'a str, &'a str)],
    /// Where in `argv` the word being read now begins. See
    /// [`Parser::current_word`]; it is not derivable from `at`, which a bundle
    /// leaves pointing past the word it is still yielding from.
    word: usize,
    /// What is left of a bundle like `-am`: one argv word yields several items,
    /// so the remainder outlives the `next` call that started it.
    cluster: Vec<u8>,
    only_operands: bool,
    stop_at_operand: bool,
    done: bool,
}

impl<'a> Parser<'a> {
    /// The whole argv word the item just yielded came out of.
    ///
    /// Only one caller needs this, and it needs it for a rule that cannot be
    /// stated any other way. GNU `chmod` builds its mode string out of
    /// `argv[optind - 1]` — the *entire word* that contained a mode letter, not
    /// the letter and its value — so `chmod -Rw d` produces the mode `-Rw` and
    /// is rejected, where reconstructing `-` + `w` from the parsed option would
    /// produce `-w` and silently recurse. Measured: GNU answers
    /// `chmod: invalid mode: ‘-Rw’`.
    ///
    /// Using it requires binding the [`Parser`] rather than consuming it in a
    /// `for` loop, since the answer is only meaningful between one `next` and
    /// the next.
    #[must_use]
    pub fn current_word(&self) -> Option<&'a OsString> {
        self.argv.get(self.word)
    }

    /// The next word of argv, consumed as some option's value.
    fn next_word(&mut self) -> Option<OsString> {
        let word = self.argv.get(self.at)?.clone();
        self.at = self.at.saturating_add(1);
        Some(word)
    }

    /// Take one option off the front of the bundle in [`Parser::cluster`].
    fn take_short(&mut self) -> Result<Opt<'a>, Error> {
        let bundle = std::mem::take(&mut self.cluster);
        // Bytes, not `char`s. `-é` is two bytes in UTF-8, and iterating `char`s
        // would answer `invalid option -- 'é'` — an option nobody typed, and one
        // nobody could, since an option is a single byte. It also would not
        // survive an argument that is not UTF-8 at all.
        let Some((&flag, tail)) = bundle.split_first() else {
            // Unreachable: `next` only calls this with a non-empty bundle.
            return Err(self.program.invalid_option(b'-'));
        };
        let Some(takes) = short_takes(self.shorts, flag) else {
            return Err(self.program.invalid_option(flag));
        };
        if takes == Takes::Nothing {
            self.cluster = tail.to_vec();
            return Ok(Opt::Short(flag, None));
        }
        // A value-taking option ends the bundle either way: it eats the rest of
        // the word, or, if there is no rest, the word after it.
        let tail = tail.to_vec();
        let value = match (takes, tail.is_empty()) {
            // `-c` with no attached text is `-c` with no value. An optional
            // value is never the *next* word — that is the whole difference
            // between `Optional` and `Required`.
            (Takes::Optional, true) => None,
            (_, false) => Some(os_from_bytes(&tail)),
            (_, true) => Some(
                self.next_word()
                    .ok_or_else(|| self.program.short_missing_argument(flag))?,
            ),
        };
        Ok(Opt::Short(flag, value))
    }

    /// Handle one `--name[=value]` word.
    fn take_long(&mut self, body: &[u8], whole: &[u8]) -> Result<Opt<'a>, Error> {
        // Split before resolving: the name is what gets matched, and the
        // argument *as typed* — `=VALUE` included — is what gets echoed back if
        // it resolves to nothing.
        let (typed, inline) = match body.iter().position(|&c| c == b'=') {
            Some(at) => (
                body.get(..at).unwrap_or_default(),
                Some(body.get(at.saturating_add(1)..).unwrap_or_default()),
            ),
            None => (body, None),
        };
        // Every option name is ASCII, so a name that is not UTF-8 can match
        // none of them. It takes the unrecognised path — reported as the bytes
        // typed — rather than failing in some third way.
        let typed =
            std::str::from_utf8(typed).map_err(|_| self.program.unrecognized_option(whole))?;
        let (name, takes) =
            self.program
                .resolve_long_aliased(typed, whole, self.longs, self.aliases)?;

        if inline.is_some() && takes == Takes::Nothing {
            return Err(self.program.long_unwanted_argument(name));
        }
        let value = match takes {
            Takes::Nothing => None,
            // As for the short form: `--color x` leaves `x` an operand.
            Takes::Optional => inline.map(os_from_bytes),
            Takes::Required => Some(match inline {
                // `--time=` is an *empty* value, not a missing one, and must
                // reach the caller so `argmatch` can list the valid words.
                Some(text) => os_from_bytes(text),
                None => self
                    .next_word()
                    .ok_or_else(|| self.program.long_missing_argument(name))?,
            }),
        };
        Ok(Opt::Long(name, value))
    }
}

impl<'a> Iterator for Parser<'a> {
    type Item = Result<Opt<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let outcome = self.step();
        if matches!(outcome, Some(Err(_)) | None) {
            self.done = true;
        }
        outcome
    }
}

impl<'a> Parser<'a> {
    /// One item, with no regard for whether an earlier one failed.
    fn step(&mut self) -> Option<Result<Opt<'a>, Error>> {
        if !self.cluster.is_empty() {
            return Some(self.take_short());
        }
        // A copy of the slice reference, so what comes out of it borrows argv
        // rather than `self` — an operand is handed back unchanged.
        let argv: &'a [OsString] = self.argv;
        let arg = argv.get(self.at)?;
        self.word = self.at;
        self.at = self.at.saturating_add(1);

        if self.only_operands {
            return Some(Ok(Opt::Operand(arg)));
        }
        let bytes = os_bytes(arg.as_os_str());

        if *bytes == *b"--" {
            self.only_operands = true;
            return self.step();
        }
        // A lone `-` is an operand. Every utility that gives it a meaning —
        // standard input, standard output — does so when it reads the operand,
        // not when it parses it.
        if *bytes == *b"-" || bytes.first() != Some(&b'-') {
            if self.stop_at_operand {
                self.only_operands = true;
            }
            return Some(Ok(Opt::Operand(arg)));
        }
        Some(match bytes.strip_prefix(b"--") {
            Some(body) => self.take_long(body, &bytes),
            None => {
                self.cluster = bytes.get(1..).unwrap_or_default().to_vec();
                self.take_short()
            }
        })
    }
}

/// What one letter of a `getopt_long` string says about its value.
///
/// `None` for a letter the string does not list, which is
/// `invalid option -- 'x'`.
fn short_takes(shorts: &str, flag: u8) -> Option<Takes> {
    if flag == b':' {
        // In this string a colon is punctuation, never an option; glibc gives
        // `-:` no way to be declared and coreutils has no such option.
        return None;
    }
    let bytes = shorts.as_bytes();
    let at = bytes.iter().position(|&c| c == flag)?;
    let mut colons = 0usize;
    let mut i = at.saturating_add(1);
    while bytes.get(i) == Some(&b':') {
        colons = colons.saturating_add(1);
        i = i.saturating_add(1);
    }
    Some(match colons {
        0 => Takes::Nothing,
        1 => Takes::Required,
        _ => Takes::Optional,
    })
}

/// `argmatch`'s two diagnostics, which differ only in that first word.
///
/// **The only pair here that gnulib writes rather than glibc**, and so the only
/// pair whose quotes follow the locale. Measured under `LC_ALL=C.UTF-8`:
///
/// ```text
/// $ sort --check=
/// sort: ambiguous argument ‘’ for ‘--check’
/// Valid arguments are:
///   - ‘quiet’, ‘silent’
///   - ‘diagnose-first’
/// Try 'sort --help' for more information.
/// ```
///
/// Note the last line: the referral comes from coreutils' own `usage()`, which
/// spells its quotes literally, so it stays straight in the middle of a message
/// whose other four lines have gone curly. That is GNU's output, not an
/// inconsistency of ours.
///
/// The "Valid arguments are" list is generated from the same table the match
/// used rather than written out beside it, because the two must agree and a
/// hand-written copy is what drifts. Runs of words sharing a value are joined
/// onto one line, which is both gnulib's rendering and the reason it is worth
/// generating: `‘quiet’, ‘silent’` on one line *is* the statement that they
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
                // `quote`, not `named`: this is the one diagnostic in this file
                // that gnulib writes rather than glibc, so it is the one that
                // follows the locale into curly marks. Measured under
                // `LC_ALL=C.UTF-8`: `sort: invalid argument ‘zzz’ for ‘--sort’`
                // against `sort: option '--key' requires an argument`.
                "{what} argument {} for {}\nValid arguments are:\n{}",
                quote(given),
                quote(option.as_bytes()),
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

    /// The sentence alone, having first checked that the referral is there.
    ///
    /// The referral is `sort`'s for every test that uses `SORT`, which is most
    /// of them; the alias tests build their own `Program` to reproduce a
    /// measured `cp` or `rmdir` table, so the expected name is a parameter with
    /// a default rather than the hard-coded `"sort"` it began as.
    fn without_referral(err: &Error) -> String {
        without_referral_of("sort", err)
    }

    fn without_referral_of(program: &str, err: &Error) -> String {
        assert_eq!(
            err.referral,
            Some(program),
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

    /// `rmdir`'s table, whose `--path` and `--parents` are one option under two
    /// spellings — GNU's `struct option` gives them the same `val`. Measured
    /// with `rmdir --=x`.
    const RMDIR_LONGS: &[(&str, Takes)] = &[
        ("ignore-fail-on-non-empty", Takes::Nothing),
        ("path", Takes::Nothing),
        ("parents", Takes::Nothing),
        ("verbose", Takes::Nothing),
        ("help", Takes::Nothing),
        ("version", Takes::Nothing),
    ];
    const RMDIR_ALIASES: &[(&str, &str)] = &[("path", "parents")];
    const RMDIR: Program = Program::new("rmdir", 1);

    /// The bug this pair of functions was added for: a deprecated alias made
    /// its own option unabbreviatable. Measured — `rmdir --p a/b` succeeds.
    #[test]
    fn an_alias_is_not_a_second_candidate() {
        let (name, _) = RMDIR
            .resolve_long_aliased("p", b"--p", RMDIR_LONGS, RMDIR_ALIASES)
            .unwrap();
        // `pfound`: the *first* table entry that matched, which for `rmdir` is
        // the deprecated spelling. Measured: `rmdir --pa=1` answers
        // `option '--path' doesn't allow an argument`.
        assert_eq!(name, "path");
        // Without the alias map the same table is ambiguous, which is exactly
        // what `rmdir` did before and what GNU does not do.
        assert!(RMDIR.resolve_long("p", b"--p", RMDIR_LONGS).is_err());
    }

    /// An exact match still wins outright and is returned as itself, alias or
    /// not: `rmdir --path` is `--path`, not `--parents`.
    #[test]
    fn an_exact_alias_resolves_to_the_spelling_typed() {
        let (name, _) = RMDIR
            .resolve_long_aliased("path", b"--path", RMDIR_LONGS, RMDIR_ALIASES)
            .unwrap();
        assert_eq!(name, "path");
    }

    /// The rule is "an alias is not a second candidate", **not** "aliases are
    /// hidden". `cp`'s `--p` matches `--parents`, `--path` and `--preserve`,
    /// and GNU lists two of the three. Measured:
    ///
    /// ```text
    /// cp: option '--p' is ambiguous; possibilities: '--parents' '--preserve'
    /// ```
    ///
    /// This is the assertion that fails if someone implements the alias rule by
    /// dropping aliases from the table, or from the message, instead.
    #[test]
    fn a_real_ambiguity_survives_an_alias_in_the_same_prefix() {
        const CP_LONGS: &[(&str, Takes)] = &[
            ("parents", Takes::Nothing),
            ("path", Takes::Nothing),
            ("preserve", Takes::Optional),
        ];
        const CP_ALIASES: &[(&str, &str)] = &[("path", "parents")];
        let cp = Program::new("cp", 1);
        let err = cp
            .resolve_long_aliased("p", b"--p", CP_LONGS, CP_ALIASES)
            .unwrap_err();
        assert_eq!(
            without_referral_of("cp", &err),
            "option '--p' is ambiguous; possibilities: '--parents' '--preserve'"
        );
    }

    /// glibc compares each later match against `pfound` and never against the
    /// others, so when the alias pair is *not* first both of its spellings are
    /// listed. Mirroring that exactly matters because the list is output.
    #[test]
    fn an_alias_pair_that_is_not_first_is_listed_in_full() {
        const T: &[(&str, Takes)] = &[
            ("pear", Takes::Nothing),
            ("plum", Takes::Nothing),
            ("prune", Takes::Nothing),
        ];
        const A: &[(&str, &str)] = &[("prune", "plum")];
        let p = Program::new("t", 1);
        let err = p.resolve_long_aliased("p", b"--p", T, A).unwrap_err();
        assert_eq!(
            without_referral_of("t", &err),
            "option '--p' is ambiguous; possibilities: '--pear' '--plum' '--prune'"
        );
    }

    /// A table with no aliases must behave exactly as it did before the alias
    /// rule existed — every already-converted bin depends on that.
    #[test]
    fn an_empty_alias_map_changes_nothing() {
        for typed in ["k", "r", "random-sort", "fo", "stable"] {
            let whole = format!("--{typed}");
            let plain = SORT.resolve_long(typed, whole.as_bytes(), SORT_LONGS);
            let aliased = SORT.resolve_long_aliased(typed, whole.as_bytes(), SORT_LONGS, &[]);
            match (plain, aliased) {
                (Ok(a), Ok(b)) => assert_eq!(a.0, b.0, "{typed}"),
                (Err(a), Err(b)) => assert_eq!(a.sentence, b.sentence, "{typed}"),
                (a, b) => panic!("{typed}: disagreed: {a:?} vs {b:?}"),
            }
        }
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
            "ambiguous argument ‘’ for ‘--check’\nValid arguments are:\n  \
             - ‘quiet’, ‘silent’\n  - ‘diagnose-first’"
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
            without_referral(&err).starts_with("invalid argument ‘bogus’ for ‘--check’"),
            "{err}"
        );
        // A multi-byte character cannot prefix any of these ASCII words, so it
        // takes the no-match path rather than erroring differently — and it
        // reaches the message *as itself*, because `quote()` escapes what does
        // not decode, not what is not ASCII. Measured: GNU 9.4 under
        // `LC_ALL=C.UTF-8` prints `‘é’` here too (`tests/quotearg-gnu.txt`).
        let err = SORT
            .argmatch("é".as_bytes(), "--check", CHECK_WORDS)
            .unwrap_err();
        assert!(
            without_referral(&err).starts_with("invalid argument ‘é’ for ‘--check’"),
            "{err}"
        );
        // A byte that decodes to nothing still escapes, which is the half of
        // the rule the character above no longer covers.
        let err = SORT.argmatch(b"\xff", "--check", CHECK_WORDS).unwrap_err();
        assert!(
            without_referral(&err).starts_with(r"invalid argument ‘\377’ for ‘--check’"),
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
                .contains("  - ‘quiet’, ‘silent’\n  - ‘diagnose-first’"),
            "{err:?}"
        );
        // All-distinct values put every word on its own line.
        let distinct: &[(&str, u8)] = &[("month", 0), ("numeric", 1)];
        let err = SORT.argmatch(b"x", "--sort", distinct).unwrap_err();
        assert!(
            err.sentence.contains("  - ‘month’\n  - ‘numeric’"),
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

    // ---------------------------------------------------------------------
    // The walk over argv.
    //
    // These began as `touch`'s own parser tests and moved here with the code,
    // because they are the specification of the four value spellings and of
    // the cases around them. `touch` is kept as the subject rather than being
    // generalised away: its table is a measured GNU one, `"acd:fhmr:t:"` with
    // the seven long names below, and a table invented for a test would be
    // free to be wrong in a way GNU's is not.
    // ---------------------------------------------------------------------

    const TOUCH: Program = Program::new("touch", 1);

    /// GNU's whole table for `touch`, in declaration order, including the
    /// options this project does not implement — see [`Program::parse`] on why
    /// the unimplemented ones must be here.
    const TOUCH_LONGS: &[(&str, Takes)] = &[
        ("time", Takes::Required),
        ("no-create", Takes::Nothing),
        ("date", Takes::Required),
        ("reference", Takes::Required),
        ("no-dereference", Takes::Nothing),
        ("help", Takes::Nothing),
        ("version", Takes::Nothing),
    ];

    const TOUCH_SHORTS: &str = "acd:fhmr:t:";

    fn argv(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    /// Every item, or the first error. The common shape: a caller that walks to
    /// the end without acting on anything before it.
    fn walk(args: &[OsString]) -> Result<Vec<Opt<'_>>, Error> {
        TOUCH.parse(args, TOUCH_SHORTS, TOUCH_LONGS).collect()
    }

    fn short(flag: u8) -> Opt<'static> {
        Opt::Short(flag, None)
    }

    fn short_with<'a>(flag: u8, value: &str) -> Opt<'a> {
        Opt::Short(flag, Some(OsString::from(value)))
    }

    fn long_with(name: &'static str, value: &str) -> Opt<'static> {
        Opt::Long(name, Some(OsString::from(value)))
    }

    #[test]
    fn reference_takes_its_file_in_all_four_spellings() {
        for words in [
            ["-r", "ref", "f"].as_slice(),
            ["-rref", "f"].as_slice(),
            ["--reference", "ref", "f"].as_slice(),
            ["--reference=ref", "f"].as_slice(),
        ] {
            let args = argv(words);
            let got = walk(&args).unwrap();
            let value = match got.first() {
                Some(Opt::Short(b'r', Some(v)) | Opt::Long("reference", Some(v))) => v.clone(),
                other => panic!("{words:?} parsed as {other:?}"),
            };
            assert_eq!(value, OsString::from("ref"), "{words:?}");
            assert_eq!(got.get(1), Some(&Opt::Operand(&args[words.len() - 1])));
            assert_eq!(got.len(), 2, "{words:?}");
        }
    }

    #[test]
    fn a_bundle_ending_in_an_argument_option_still_bundles() {
        // Bundling continues up to the value-taking letter, which then eats the
        // *next* word rather than ending the bundle empty-handed.
        let args = argv(&["-cr", "ref", "f"]);
        let got = walk(&args).unwrap();
        assert_eq!(
            got,
            vec![short(b'c'), short_with(b'r', "ref"), Opt::Operand(&args[2]),]
        );
        // And the same letter with text glued to it takes that text, so the
        // bundle ends there too.
        let args = argv(&["-crref", "f"]);
        assert_eq!(
            walk(&args).unwrap(),
            vec![short(b'c'), short_with(b'r', "ref"), Opt::Operand(&args[1]),]
        );
    }

    #[test]
    fn an_option_that_wants_a_value_and_has_none() {
        // Two different sentences, which is why the driver has two error
        // constructors rather than one.
        let args = argv(&["-r"]);
        assert_eq!(
            without_referral_of("touch", &walk(&args).unwrap_err()),
            "option requires an argument -- 'r'"
        );
        let args = argv(&["--reference"]);
        assert_eq!(
            without_referral_of("touch", &walk(&args).unwrap_err()),
            "option '--reference' requires an argument"
        );
        // The value is the next word even when that word looks like an option.
        // glibc does not second-guess this, and neither do we.
        let args = argv(&["-r", "-c"]);
        assert_eq!(walk(&args).unwrap(), vec![short_with(b'r', "-c")]);
    }

    #[test]
    fn a_value_on_an_option_that_takes_none() {
        let args = argv(&["--no-create=x"]);
        assert_eq!(
            without_referral_of("touch", &walk(&args).unwrap_err()),
            "option '--no-create' doesn't allow an argument"
        );
        // The sentence names the *table's* spelling, not the abbreviation typed.
        let args = argv(&["--no-c=x"]);
        assert_eq!(
            without_referral_of("touch", &walk(&args).unwrap_err()),
            "option '--no-create' doesn't allow an argument"
        );
    }

    #[test]
    fn an_empty_value_is_a_value_and_not_a_missing_one() {
        // `--time=` must arrive as `Some("")` so the caller's `argmatch` runs
        // and lists the valid words. Were it `None`, the caller would instead
        // report a missing argument, which GNU does not.
        let args = argv(&["--time=", "f"]);
        assert_eq!(
            walk(&args).unwrap(),
            vec![long_with("time", ""), Opt::Operand(&args[1])]
        );
        // The short form has no way to spell an empty value: `-t` with nothing
        // glued to it takes the next word, which here is the file.
        let args = argv(&["-t", "", "f"]);
        assert_eq!(
            walk(&args).unwrap(),
            vec![short_with(b't', ""), Opt::Operand(&args[2])]
        );
    }

    #[test]
    fn a_refused_option_still_swallows_its_value() {
        // `-d` is declared `d:` even though this project does not implement it.
        // If it were left out of the string, the parse would answer `invalid
        // option -- 'd'` and then hand `2001-01-01` back as a file to create.
        let args = argv(&["-d", "2001-01-01", "f"]);
        assert_eq!(
            walk(&args).unwrap(),
            vec![short_with(b'd', "2001-01-01"), Opt::Operand(&args[2])]
        );
        let args = argv(&["--date", "2001-01-01", "f"]);
        assert_eq!(
            walk(&args).unwrap(),
            vec![long_with("date", "2001-01-01"), Opt::Operand(&args[2])]
        );
    }

    #[test]
    fn the_time_option_takes_its_word_either_way() {
        let args = argv(&["--time=mtime", "f"]);
        assert_eq!(got_first(&args), long_with("time", "mtime"));
        let args = argv(&["--time", "mtime", "f"]);
        assert_eq!(got_first(&args), long_with("time", "mtime"));
    }

    fn got_first(args: &[OsString]) -> Opt<'_> {
        walk(args).unwrap().first().cloned().expect("one option")
    }

    #[test]
    fn double_dash_ends_options_and_bare_dash_is_an_operand() {
        // After `--`, a word that looks like an option is a file name.
        let args = argv(&["--", "-c", "--reference"]);
        assert_eq!(
            walk(&args).unwrap(),
            vec![Opt::Operand(&args[1]), Opt::Operand(&args[2])]
        );
        // A lone `-` is an operand wherever it appears, and does not end
        // options for what follows it.
        let args = argv(&["-", "-c"]);
        assert_eq!(
            walk(&args).unwrap(),
            vec![Opt::Operand(&args[0]), short(b'c')]
        );
        // A second `--` is a file called `--`.
        let args = argv(&["--", "--"]);
        assert_eq!(walk(&args).unwrap(), vec![Opt::Operand(&args[1])]);
    }

    #[test]
    fn options_may_follow_operands() {
        // GNU permutes: `touch f -c` sets the flag. Our order differs from
        // glibc's — it moves the operands to the end, we leave them where they
        // were typed — but every caller collects the two into separate places,
        // so only the relative order *within* each matters, and that is kept.
        let args = argv(&["f", "-c", "g"]);
        assert_eq!(
            walk(&args).unwrap(),
            vec![Opt::Operand(&args[0]), short(b'c'), Opt::Operand(&args[2]),]
        );
    }

    #[test]
    fn a_leading_plus_stops_at_the_first_operand() {
        // `nice`'s shape: everything after the command name belongs to the
        // command, however much it looks like an option of ours.
        const NICE: Program = Program::new("nice", 125);
        let args = argv(&["-n", "5", "cmd", "-c", "--reference"]);
        let got: Vec<Opt<'_>> = NICE
            .parse(&args, "+n:", &[("adjustment", Takes::Required)])
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            got,
            vec![
                short_with(b'n', "5"),
                Opt::Operand(&args[2]),
                Opt::Operand(&args[3]),
                Opt::Operand(&args[4]),
            ]
        );
    }

    #[test]
    fn an_unknown_short_option_names_the_byte_typed() {
        let args = argv(&["-z"]);
        assert_eq!(
            without_referral_of("touch", &walk(&args).unwrap_err()),
            "invalid option -- 'z'"
        );
        // A colon is punctuation in the shorts string, never an option.
        let args = argv(&["-:"]);
        assert_eq!(
            without_referral_of("touch", &walk(&args).unwrap_err()),
            "invalid option -- ':'"
        );
    }

    /// An `OsString` no `String` can hold, built the way the running host can.
    ///
    /// The two halves are not the same fixture and cannot be: a Unix `OsStr` is
    /// bytes, so its unrepresentable case is a byte that is not UTF-8, while a
    /// Windows one is UTF-16, so its case is an unpaired surrogate (a code unit
    /// in `0xD800..=0xDFFF` with no partner). Testing only the Unix shape would
    /// leave the whole of this file's non-text handling unexercised on the
    /// development host, which is exactly the blind spot that let the original
    /// `env::args()` panic survive.
    #[cfg(unix)]
    fn untranslatable(prefix: &str) -> OsString {
        use std::os::unix::ffi::OsStringExt;
        let mut bytes = prefix.as_bytes().to_vec();
        bytes.push(0xFF);
        OsString::from_vec(bytes)
    }

    #[cfg(windows)]
    fn untranslatable(prefix: &str) -> OsString {
        use std::os::windows::ffi::OsStringExt;
        let mut units: Vec<u16> = prefix.encode_utf16().collect();
        units.push(0xD800);
        OsString::from_wide(&units)
    }

    #[test]
    fn a_non_utf8_argument_is_unrecognised_not_a_panic() {
        // Every long name is ASCII, so an argument that is not text matches
        // none of them and takes the unrecognised path — rather than failing in
        // some third way, or panicking as `env::args()` once did.
        let args = vec![untranslatable("--")];
        let sentence = without_referral_of("touch", &walk(&args).unwrap_err());
        assert!(sentence.starts_with("unrecognized option "), "{sentence:?}");
        // Whatever it holds, the diagnostic is printable and cannot forge a
        // second line — the property the escaping exists for.
        assert!(
            sentence.chars().all(|c| c != '\n' && c != '\r'),
            "{sentence:?}"
        );
    }

    /// The bytes of a name are the OS's, and nothing here may reinterpret them.
    ///
    /// Unix-only: on Windows `os_bytes` is documented-lossy, so a round trip
    /// through it is not exact there and the assertion would be testing the
    /// host rather than the driver.
    #[test]
    #[cfg(unix)]
    fn a_name_that_is_not_text_survives_as_an_operand_and_as_a_value() {
        let name = untranslatable("f");
        // An operand is handed back *borrowed*, so it is the same word of argv
        // and cannot have been rebuilt on the way.
        let args = vec![name.clone()];
        assert_eq!(walk(&args).unwrap(), vec![Opt::Operand(&args[0])]);
        assert_eq!(args[0], name);
        // A value has to be built — it is often only part of a word — but is
        // built from bytes.
        let args = vec![OsString::from("-r"), name.clone()];
        assert_eq!(walk(&args).unwrap(), vec![Opt::Short(b'r', Some(name))]);
        // Glued to its option, and after an `=`, both of which take the
        // in-word path rather than the next-word one.
        let glued = untranslatable("-r");
        let args = vec![glued];
        assert_eq!(
            walk(&args).unwrap(),
            vec![Opt::Short(b'r', Some(untranslatable("")))]
        );
        let args = vec![untranslatable("--reference=")];
        assert_eq!(
            walk(&args).unwrap(),
            vec![Opt::Long("reference", Some(untranslatable("")))]
        );
    }

    #[test]
    fn a_short_option_that_is_not_one_byte_is_still_one_byte() {
        // `-é` is two bytes in UTF-8. Iterating characters would answer
        // `invalid option -- 'é'`, an option nobody could declare; glibc
        // reports the first byte, and then the second.
        let args = argv(&["-é"]);
        assert_eq!(
            without_referral_of("touch", &walk(&args).unwrap_err()),
            "invalid option -- '\\303'"
        );
    }

    #[test]
    fn the_walk_stops_at_its_first_error() {
        // glibc's own loop carries on after a bad option, but no converted
        // utility makes that visible: every one of them exits on the first
        // diagnostic. Stopping is therefore the honest contract, and it keeps
        // a caller from having to remember to stop.
        let args = argv(&["-c", "-z", "-q"]);
        let mut walk = TOUCH.parse(&args, TOUCH_SHORTS, TOUCH_LONGS);
        assert_eq!(walk.next(), Some(Ok(short(b'c'))));
        assert!(matches!(walk.next(), Some(Err(_))));
        assert_eq!(walk.next(), None, "nothing follows the error");
    }

    #[test]
    fn help_wins_over_a_bad_option_after_it_and_loses_to_one_before() {
        // The reason the walk yields one item at a time. Measured:
        // `readlink --help --bogus` prints the help and exits 0, while
        // `readlink --bogus --help` is an error.
        let args = argv(&["--help", "--bogus"]);
        let mut walk = TOUCH.parse(&args, TOUCH_SHORTS, TOUCH_LONGS);
        assert_eq!(walk.next(), Some(Ok(Opt::Long("help", None))));
        // A caller acts on that and never asks for the next item. Had the
        // driver validated all of argv first, it could not.
        let args = argv(&["--bogus", "--help"]);
        let mut walk = TOUCH.parse(&args, TOUCH_SHORTS, TOUCH_LONGS);
        assert!(matches!(walk.next(), Some(Err(_))));
    }

    #[test]
    fn a_long_name_may_be_abbreviated_and_reports_the_full_one() {
        // The resolved name is the table's, so a caller matches on one spelling
        // however the user abbreviated it.
        let args = argv(&["--refe=ref"]);
        assert_eq!(walk(&args).unwrap(), vec![long_with("reference", "ref")]);
        // Ambiguity lists the candidates in declaration order.
        let args = argv(&["--no"]);
        assert_eq!(
            without_referral_of("touch", &walk(&args).unwrap_err()),
            "option '--no' is ambiguous; possibilities: '--no-create' '--no-dereference'"
        );
    }

    #[test]
    fn an_optional_value_is_never_the_next_word() {
        // The whole difference between `Optional` and `Required`: `--check x`
        // leaves `x` an operand, where `--key x` would take it.
        let args = argv(&["--check", "f"]);
        let got: Vec<Opt<'_>> = SORT
            .parse(&args, "c::k:", SORT_LONGS)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(got, vec![Opt::Long("check", None), Opt::Operand(&args[1])]);
        // Glued, it is a value — in both spellings.
        let args = argv(&["--check=diagnose-first"]);
        let got: Vec<Opt<'_>> = SORT
            .parse(&args, "c::k:", SORT_LONGS)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            got,
            vec![Opt::Long("check", Some(OsString::from("diagnose-first")))]
        );
        let args = argv(&["-c", "f"]);
        let got: Vec<Opt<'_>> = SORT
            .parse(&args, "c::k:", SORT_LONGS)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(got, vec![short(b'c'), Opt::Operand(&args[1])]);
        let args = argv(&["-cq"]);
        let got: Vec<Opt<'_>> = SORT
            .parse(&args, "c::k:", SORT_LONGS)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(got, vec![short_with(b'c', "q")]);
    }

    #[test]
    fn an_alias_pair_is_one_option_and_keeps_the_spelling_declared() {
        // `rmdir`'s measured pair: `--path` and `--parents` are one option, so
        // `--p` is not ambiguous between them. Measured — `rmdir --p a/b`
        // succeeds. Through the plain `parse` it would be an error.
        let args = argv(&["--p", "a/b"]);
        let got: Vec<Opt<'_>> = RMDIR
            .parse_aliased(&args, "pv", RMDIR_LONGS, RMDIR_ALIASES)
            .collect::<Result<_, _>>()
            .unwrap();
        // The name is the *first table row* that matched, which for `rmdir` is
        // the deprecated spelling — so a caller must handle both.
        assert_eq!(got, vec![Opt::Long("path", None), Opt::Operand(&args[1])]);
        let args = argv(&["--parents"]);
        let got: Vec<Opt<'_>> = RMDIR
            .parse_aliased(&args, "pv", RMDIR_LONGS, RMDIR_ALIASES)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(got, vec![Opt::Long("parents", None)]);
        assert!(
            RMDIR
                .parse(&argv(&["--p"]), "pv", RMDIR_LONGS)
                .next()
                .is_some_and(|item| item.is_err()),
            "without the alias map the same table is ambiguous"
        );
    }

    #[test]
    fn the_shorts_string_is_read_as_getopt_reads_it() {
        assert_eq!(short_takes("acd:fhmr:t:", b'a'), Some(Takes::Nothing));
        assert_eq!(short_takes("acd:fhmr:t:", b'd'), Some(Takes::Required));
        assert_eq!(short_takes("c::k:", b'c'), Some(Takes::Optional));
        assert_eq!(short_takes("c::k:", b'k'), Some(Takes::Required));
        // A letter the string does not list, and the colon itself.
        assert_eq!(short_takes("acd:fhmr:t:", b'z'), None);
        assert_eq!(short_takes("acd:fhmr:t:", b':'), None);
        // A trailing letter has no colon after it and takes nothing.
        assert_eq!(short_takes("ab", b'b'), Some(Takes::Nothing));
    }

    #[test]
    fn an_empty_command_line_yields_nothing() {
        let args: Vec<OsString> = Vec::new();
        assert!(walk(&args).unwrap().is_empty());
        // And a `--` with nothing after it is not an operand.
        let args = argv(&["--"]);
        assert!(walk(&args).unwrap().is_empty());
    }
}
