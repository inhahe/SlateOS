//! `uniq` — collapse adjacent matching lines.
//!
//! The seventh utility moved onto [`coreutils::getopt`]. What it replaced
//! accepted `-c`, `-d` and `-u`, answered anything else with
//! `uniq: unknown option: -x`, and had no long options at all. That is a
//! smaller fraction of `uniq` than the option count suggests, because the
//! comparison itself was not configurable: `-f`, `-s` and `-w` decide *which
//! bytes of a line are compared*, and without them `uniq` can only collapse
//! lines that are equal in full.
//!
//! ## Two argv forms that predate options
//!
//! `uniq` is old enough that its original interface was not options at all, and
//! both spellings still work — they are not aliases for `-f`/`-s`, they are
//! parsed in places `-f` and `-s` are not:
//!
//! | Typed | Means | Where it is parsed |
//! |---|---|---|
//! | `uniq -3` | skip 3 fields | a short option, one digit at a time |
//! | `uniq +3` | skip 3 chars | an **operand**, checked before it becomes a file name |
//!
//! The digits **accumulate across separate arguments**, because each is its own
//! short option and each shifts the running value by a decimal place:
//! `uniq -1 -2` skips *twelve* fields, not one and then two. `-f` resets that
//! accumulator, so `uniq -f3 -1` skips one field rather than thirty-one.
//!
//! The `+3` form is an operand that is intercepted, which is why it is
//! disqualified by three separate things: `--` (after which nothing is
//! intercepted), `POSIXLY_CORRECT` with an operand already seen (after which
//! nothing is parsed at all), and `_POSIX2_VERSION` naming a standard between
//! 200112 and 200808, in which the form was withdrawn. Under any of those,
//! `+3` is the name of a file.
//!
//! ## The second operand is an output file, and it is truncated
//!
//! `uniq INPUT OUTPUT` is not `uniq FILE FILE` — there is no multi-file mode,
//! and the second name is opened for **writing**. `uniq a b` destroys `b`. This
//! is worth stating loudly because every neighbour in this directory takes a
//! list of inputs, so the shape a reader arrives with is the wrong one, and the
//! failure is silent and lossy. It also constrains the differential harness,
//! which must never pass a fixture as the second operand.
//!
//! ## Options may not follow operands, sometimes
//!
//! `uniq`'s optstring begins with `-`, which puts glibc's `getopt_long` in
//! `RETURN_IN_ORDER` mode: operands come back interleaved with options instead
//! of being permuted to the end. That is what makes the `+3` interception
//! possible. It also *disables* getopt's own `POSIXLY_CORRECT` handling, so
//! `uniq` reads the variable itself — and stops parsing options after the first
//! operand when it is set. `POSIXLY_CORRECT=1 uniq in.txt -c` therefore writes
//! to a file named `-c`.
//!
//! ## Reference
//!
//! Against glibc's `uniq` (GNU coreutils 9.4) through WSL, not against the
//! `uniq` on this host's `PATH`, which is MSYS2's and whose getopt words every
//! diagnostic differently. `scripts/uniq-diff.sh` is the executable form of
//! every claim in this file.

#![deny(clippy::all)]
#![warn(clippy::pedantic)]

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::{os_bytes, quote, quoteaf_os, quotef_os};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, ErrorKind, Read, Write};
use std::process::ExitCode;

/// `uniq` exits 1 on a bad command line — measured, not assumed; the utilities
/// that exit 2 are the ones that have already spent 1 on a real answer, and
/// `uniq` has not.
const UNIQ: Program = Program::new("uniq", 1);

/// Every long option `uniq` accepts, **in GNU's declaration order**, which is
/// observable: `getopt_long` lists an ambiguous prefix's candidates in it.
///
/// ```text
/// $ uniq --c
/// uniq: option '--c' is ambiguous; possibilities: '--count' '--check-chars'
/// ```
///
/// Alphabetically `--check-chars` would come first. It does not. The order was
/// measured with `uniq --=x`, whose empty prefix matches everything.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("count", Takes::Nothing),
    ("repeated", Takes::Nothing),
    ("all-repeated", Takes::Optional),
    ("group", Takes::Optional),
    ("ignore-case", Takes::Nothing),
    ("unique", Takes::Nothing),
    ("skip-fields", Takes::Required),
    ("skip-chars", Takes::Required),
    ("check-chars", Takes::Required),
    ("zero-terminated", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// Whether and where `--all-repeated` puts a blank line around each run of
/// duplicates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Delimit {
    /// `-D`, and `--all-repeated[=none]`.
    None,
    /// A delimiter before every group.
    Prepend,
    /// A delimiter between groups, so none before the first.
    Separate,
}

/// `--all-repeated`'s words. The order is GNU's array order, which the
/// `Valid arguments are:` list is printed in.
const DELIMIT_METHODS: &[(&str, Delimit)] = &[
    ("none", Delimit::None),
    ("prepend", Delimit::Prepend),
    ("separate", Delimit::Separate),
];

/// Whether and where `--group` puts a blank line around *every* group, repeated
/// or not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Grouping {
    /// `--group` was not given. Note this is not one of the words below —
    /// there is no way to ask for it explicitly.
    None,
    Prepend,
    Append,
    Separate,
    Both,
}

/// `--group`'s words, in GNU's array order. `none` is deliberately absent:
/// `Grouping::None` means the option was not given at all, and admitting a word
/// for it would make `--group=none` a way to reach a state that also fails
/// `uniq`'s mutual-exclusion check for a different reason.
const GROUPING_METHODS: &[(&str, Grouping)] = &[
    ("prepend", Grouping::Prepend),
    ("append", Grouping::Append),
    ("separate", Grouping::Separate),
    ("both", Grouping::Both),
];

/// Which lines of a group get printed.
///
/// Upstream keeps these as three separate file-scope booleans under one
/// comment; they are one decision made in three parts, and no single option
/// sets exactly one of them — `-d` clears `unique`, `-D` clears it *and* sets
/// `later_repeated`, `-u` clears `first_repeated`. Grouping them is what lets
/// [`writeline`] take the decision as a unit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Selection {
    /// Whether a group of exactly one line is printed. `-d` and `-D` clear it.
    unique: bool,
    /// Whether the *first* line of a repeated group is printed. `-u` clears it.
    first_repeated: bool,
    /// Whether the second and later lines of a repeated group are printed.
    /// Only `-D`/`--all-repeated` sets it.
    later_repeated: bool,
}

impl Default for Selection {
    fn default() -> Self {
        Selection {
            unique: true,
            first_repeated: true,
            later_repeated: false,
        }
    }
}

/// Everything a parsed command line decided, after the cross-checks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Options {
    /// Fields to skip before comparing. `u64::MAX` is upstream's `SIZE_MAX`,
    /// which both `-f` overflow and digit-accumulation overflow clamp to.
    skip_fields: u64,
    /// Characters to skip *after* skipping fields — the order is fixed and is
    /// the reason `-s` cannot be expressed as an `-f` variant.
    skip_chars: u64,
    /// Characters to compare, `u64::MAX` meaning "all of them". Note `-w 0`
    /// compares nothing, so every line matches every other.
    check_chars: u64,
    /// `-c`: prefix each line with how many times it occurred.
    count: bool,
    select: Selection,
    ignore_case: bool,
    delimit: Delimit,
    grouping: Grouping,
    /// `\n`, or NUL under `-z`. Note this does **not** change what counts as a
    /// field separator: a literal newline separates fields even under `-z`.
    delimiter: u8,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            skip_fields: 0,
            skip_chars: 0,
            check_chars: u64::MAX,
            count: false,
            select: Selection::default(),
            ignore_case: false,
            delimit: Delimit::None,
            grouping: Grouping::None,
            delimiter: b'\n',
        }
    }
}

/// What a parsed command line asks for. The two operands are separate fields
/// rather than a `Vec` because they mean different things and there are never
/// three: the first is read, the second is *written*.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Request {
    Run(Options, OsString, OsString),
    Help,
    Version,
}

/// The option state as the loop builds it, plus the two things that only the
/// loop needs.
#[derive(Default)]
struct Draft {
    options: Options,
    /// Whether any of `-c`, `-d`, `-D`, `-u` was seen. Not derivable from
    /// `options` afterwards: `-d` and `-D` overlap in what they set, and `-c`
    /// with `--group` must be refused as a *conflict* rather than silently
    /// taking one of them.
    output_option_used: bool,
    /// Whether the last thing to set `skip_fields` was `-f` rather than a
    /// digit. A digit arriving after `-f` restarts the accumulator from zero,
    /// so `uniq -f3 -1` skips one field and not thirty-one. Upstream spells
    /// this as a three-state enum, but its `SFO_NONE` and `SFO_OBSOLETE` are
    /// indistinguishable at the one place the value is read.
    skip_fields_from_f: bool,
}

/// The two environment variables `uniq` reads, resolved once so that the parser
/// is a pure function of its inputs and can be tested without touching the
/// process environment.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Env {
    /// `POSIXLY_CORRECT` set to anything at all, `""` included — upstream tests
    /// `getenv (…) != nullptr`, not the value.
    posixly_correct: bool,
    /// `_POSIX2_VERSION` naming a standard in which `uniq +N` was withdrawn.
    strict_posix2: bool,
}

impl Env {
    fn from_process() -> Self {
        Env {
            posixly_correct: std::env::var_os("POSIXLY_CORRECT").is_some(),
            strict_posix2: strict_posix2(std::env::var_os("_POSIX2_VERSION").as_deref()),
        }
    }
}

/// gnulib's `posix2_version()` combined with `uniq`'s own `strict_posix2()`:
/// true when `_POSIX2_VERSION` names a standard in the half-open range
/// \[200112, 200809), the window in which `uniq +N` was not permitted.
///
/// The parse is `strtol`'s, which is looser than it looks and was measured
/// rather than recalled: leading whitespace is skipped and a sign is allowed,
/// but **any** trailing byte makes the whole variable fall back to the default
/// (200809, i.e. not strict). So `_POSIX2_VERSION=' 200112'` is strict and
/// `_POSIX2_VERSION='200112x'` is not.
fn strict_posix2(value: Option<&OsStr>) -> bool {
    const DEFAULT: i64 = 200_809;
    let version = value
        .map(os_bytes)
        .filter(|bytes| !bytes.is_empty())
        .and_then(|bytes| strtol(&bytes))
        .unwrap_or(DEFAULT);
    (200_112..200_809).contains(&version)
}

/// `strtol` with base 10 over a whole byte string, or `None` if anything is
/// left over. Saturates rather than wrapping, matching `strtol`'s `ERANGE`
/// behaviour of returning `LONG_MAX`/`LONG_MIN` with the tail consumed.
fn strtol(bytes: &[u8]) -> Option<i64> {
    let mut i = 0usize;
    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
        i = i.saturating_add(1);
    }
    let negative = match bytes.get(i) {
        Some(b'-') => {
            i = i.saturating_add(1);
            true
        }
        Some(b'+') => {
            i = i.saturating_add(1);
            false
        }
        _ => false,
    };
    let digits = bytes.get(i..)?;
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut value: i64 = 0;
    for &d in digits {
        value = value
            .saturating_mul(10)
            .saturating_add(i64::from(d.wrapping_sub(b'0')));
    }
    Some(if negative {
        value.saturating_neg()
    } else {
        value
    })
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args, Env::from_process()) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("uniq (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(options, input, output)) => run(&options, &input, &output),
        Err(e) => {
            // Only the first line carries the `uniq: ` prefix, and the referral
            // — when there is one — is already part of the message.
            eprintln!("uniq: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

/// GNU's `--help`, byte for byte, minus the trailing block of URLs that names
/// the GNU project's own bug addresses.
///
/// The ragged column alignment is upstream's, not a transcription slip:
/// `-z, --zero-terminated` and the `--help`/`--version` pair are indented
/// differently from everything around them because they come from shared
/// macros rather than from `uniq`'s own string.
fn help_text() -> String {
    "\
Usage: uniq [OPTION]... [INPUT [OUTPUT]]
Filter adjacent matching lines from INPUT (or standard input),
writing to OUTPUT (or standard output).

With no options, matching lines are merged to the first occurrence.

Mandatory arguments to long options are mandatory for short options too.
  -c, --count           prefix lines by the number of occurrences
  -d, --repeated        only print duplicate lines, one for each group
  -D                    print all duplicate lines
      --all-repeated[=METHOD]  like -D, but allow separating groups
                                 with an empty line;
                                 METHOD={none(default),prepend,separate}
  -f, --skip-fields=N   avoid comparing the first N fields
      --group[=METHOD]  show all items, separating groups with an empty line;
                          METHOD={separate(default),prepend,append,both}
  -i, --ignore-case     ignore differences in case when comparing
  -s, --skip-chars=N    avoid comparing the first N characters
  -u, --unique          only print unique lines
  -z, --zero-terminated     line delimiter is NUL, not newline
  -w, --check-chars=N   compare no more than N characters in lines
      --help        display this help and exit
      --version     output version information and exit

A field is a run of blanks (usually spaces and/or TABs), then non-blank
characters.  Fields are skipped before chars.

Note: 'uniq' does not detect repeated lines unless they are adjacent.
You may want to sort the input first, or use 'sort -u' without 'uniq'.
"
    .to_string()
}

/// Parse the whole command line.
///
/// The loop shape is upstream's rather than this directory's usual one, because
/// `uniq`'s optstring begins with `-`. Three things follow, and all three are
/// observable:
///
/// * an operand does not end option parsing (`uniq f.txt -c` counts), but
/// * under `POSIXLY_CORRECT` the *first* operand ends it completely — every
///   later argument is an operand whatever it looks like, and
/// * an operand is inspected before it is accepted, which is where `+N` is
///   intercepted. After `--`, and under the `POSIXLY_CORRECT` rule above, it is
///   not inspected, so `+N` is a file name there.
///
/// # Errors
///
/// Any getopt diagnostic, `argmatch`'s for a bad `--group`/`--all-repeated`
/// word, `uniq`'s three number diagnostics, `extra operand`, and the two
/// reachable cross-checks in [`finish`].
fn parse_args(args: &[OsString], env: Env) -> Result<Request, getopt::Error> {
    let mut draft = Draft::default();
    let mut files: Vec<OsString> = Vec::new();
    let mut only_operands = false;
    let mut i = 0usize;

    while let Some(arg) = args.get(i) {
        i = i.saturating_add(1);
        // Upstream's `optc == -1 || (posixly_correct && nfiles != 0)`: both
        // take the argument as a file *without* the `+N` inspection below.
        if only_operands || (env.posixly_correct && !files.is_empty()) {
            push_file(&mut files, arg)?;
            continue;
        }
        let bytes = arg_bytes(arg);

        if bytes == b"--" {
            only_operands = true;
        } else if bytes == b"-" || bytes.first() != Some(&b'-') {
            // A lone `-` is standard input, which is an operand. It reaches the
            // `+N` inspection too, and fails it on the first byte.
            take_operand(arg, &bytes, env, &mut draft, &mut files)?;
        } else if bytes.starts_with(b"--") {
            if let Some(request) = long_option(&bytes, args, &mut i, &mut draft)? {
                return Ok(request);
            }
        } else {
            short_options(&bytes, args, &mut i, &mut draft)?;
        }
    }

    let options = finish(&draft)?;
    let input = files
        .first()
        .cloned()
        .unwrap_or_else(|| OsString::from("-"));
    let output = files.get(1).cloned().unwrap_or_else(|| OsString::from("-"));
    Ok(Request::Run(options, input, output))
}

/// An operand, which might be the obsolete `+N` skip-chars form instead.
///
/// The three disqualifiers are checked in upstream's order, though none has a
/// side effect so the order is not observable: it must start with `+`, the
/// standard in force must not be one that withdrew the form, and the rest must
/// parse as an exact number. **Overflow disqualifies it too** — the parse must
/// return an exact value, so `+99999999999999999999999` is a file name where
/// `-s 99999999999999999999999` is a clamped skip count.
fn take_operand(
    arg: &OsString,
    bytes: &[u8],
    env: Env,
    draft: &mut Draft,
    files: &mut Vec<OsString>,
) -> Result<(), getopt::Error> {
    if bytes.first() == Some(&b'+')
        && !env.strict_posix2
        && let Some(Number::Exact(n)) = xstrtoumax(bytes)
    {
        draft.options.skip_chars = n;
        return Ok(());
    }
    push_file(files, arg)
}

/// Accept an operand, or refuse a third one.
///
/// `extra operand` is one of the diagnostics that *does* carry the
/// `Try '… --help'` referral, because upstream reports it with `error (0, …)`
/// and then calls `usage (EXIT_FAILURE)`. Its name goes through `quote()` —
/// C-style escaping — where the file names in I/O errors go through `quotef()`.
fn push_file(files: &mut Vec<OsString>, arg: &OsString) -> Result<(), getopt::Error> {
    if files.len() >= 2 {
        return Err(UNIQ.usage_referring(format!("extra operand {}", quote(&arg_bytes(arg)))));
    }
    files.push(arg.clone());
    Ok(())
}

/// One `--name` or `--name=value`, returning a [`Request`] for the two options
/// that end parsing and `None` when it only set something.
fn long_option(
    bytes: &[u8],
    args: &[OsString],
    i: &mut usize,
    draft: &mut Draft,
) -> Result<Option<Request>, getopt::Error> {
    let body = bytes.get(2..).unwrap_or_default();
    // Split before resolving, so the name is what gets matched and the whole
    // argument is what gets echoed back when it resolves to nothing.
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(at) => (
            body.get(..at).unwrap_or_default(),
            body.get(at.saturating_add(1)..),
        ),
        None => (body, None),
    };
    // Every option is ASCII, so a name that is not UTF-8 matches none of them;
    // it takes the unrecognised path rather than erroring differently.
    let typed = std::str::from_utf8(typed).map_err(|_| UNIQ.unrecognized_option(bytes))?;
    let (name, takes) = UNIQ.resolve_long(typed, bytes, LONG_OPTIONS)?;

    if takes == Takes::Nothing && inline.is_some() {
        return Err(UNIQ.long_unwanted_argument(name));
    }
    // An *optional* value is taken only from `=value`, never from the next
    // word: `uniq --group separate` groups by the default method and reads a
    // file called `separate`.
    let value: Option<Vec<u8>> = match (takes, inline) {
        (_, Some(v)) => Some(v.to_vec()),
        (Takes::Required, None) => {
            let next = args
                .get(*i)
                .ok_or_else(|| UNIQ.long_missing_argument(name))?
                .clone();
            *i = i.saturating_add(1);
            Some(arg_bytes(&next))
        }
        (_, None) => None,
    };

    match name {
        "count" => {
            draft.options.count = true;
            draft.output_option_used = true;
        }
        "repeated" => {
            draft.options.select.unique = false;
            draft.output_option_used = true;
        }
        "all-repeated" => set_all_repeated(value.as_deref(), draft)?,
        "group" => {
            draft.options.grouping = match value.as_deref() {
                None => Grouping::Separate,
                Some(word) => UNIQ.argmatch(word, "--group", GROUPING_METHODS)?,
            };
        }
        "ignore-case" => draft.options.ignore_case = true,
        "unique" => {
            draft.options.select.first_repeated = false;
            draft.output_option_used = true;
        }
        "skip-fields" => {
            draft.options.skip_fields = size_opt(
                &value.unwrap_or_default(),
                "invalid number of fields to skip",
            )?;
            draft.skip_fields_from_f = true;
        }
        "skip-chars" => {
            draft.options.skip_chars = size_opt(
                &value.unwrap_or_default(),
                "invalid number of bytes to skip",
            )?;
        }
        "check-chars" => {
            draft.options.check_chars = size_opt(
                &value.unwrap_or_default(),
                "invalid number of bytes to compare",
            )?;
        }
        "zero-terminated" => draft.options.delimiter = 0,
        "help" => return Ok(Some(Request::Help)),
        "version" => return Ok(Some(Request::Version)),
        // `resolve_long` returns only names from the table, all of which are
        // above.
        _ => {}
    }
    Ok(None)
}

/// `-D` and `--all-repeated[=METHOD]`, which share a case upstream.
///
/// Note that a *later* `-D` resets the method to `none`, because the short form
/// takes no argument and upstream's `optarg == nullptr` branch assigns rather
/// than leaves alone: `uniq --all-repeated=separate -D` separates nothing.
fn set_all_repeated(value: Option<&[u8]>, draft: &mut Draft) -> Result<(), getopt::Error> {
    draft.options.select.unique = false;
    draft.options.select.later_repeated = true;
    draft.options.delimit = match value {
        None => Delimit::None,
        Some(word) => UNIQ.argmatch(word, "--all-repeated", DELIMIT_METHODS)?,
    };
    draft.output_option_used = true;
    Ok(())
}

/// One `-abc` cluster, digits included.
fn short_options(
    bytes: &[u8],
    args: &[OsString],
    i: &mut usize,
    draft: &mut Draft,
) -> Result<(), getopt::Error> {
    let body = bytes.get(1..).unwrap_or_default();
    let mut at = 0usize;
    // Bytes, not `char`s: `-é` is two bytes, and iterating `char`s would report
    // `invalid option -- 'é'`, an option nobody typed.
    while let Some(&c) = body.get(at) {
        at = at.saturating_add(1);
        match c {
            b'0'..=b'9' => accumulate_digit(c, draft),
            b'c' => {
                draft.options.count = true;
                draft.output_option_used = true;
            }
            b'd' => {
                draft.options.select.unique = false;
                draft.output_option_used = true;
            }
            // Short `-D` never takes an argument — `D` carries no colon in the
            // optstring — so a cluster like `-Dc` is two options, not `-D c`.
            b'D' => set_all_repeated(None, draft)?,
            b'i' => draft.options.ignore_case = true,
            b'u' => {
                draft.options.select.first_repeated = false;
                draft.output_option_used = true;
            }
            b'z' => draft.options.delimiter = 0,
            b'f' | b's' | b'w' => {
                // The value is the rest of the cluster if there is one, else
                // the next argument.
                let value: Vec<u8> = match body.get(at..) {
                    Some(rest) if !rest.is_empty() => {
                        at = body.len();
                        rest.to_vec()
                    }
                    _ => {
                        let next = args
                            .get(*i)
                            .ok_or_else(|| UNIQ.short_missing_argument(c))?
                            .clone();
                        *i = i.saturating_add(1);
                        arg_bytes(&next)
                    }
                };
                match c {
                    b'f' => {
                        draft.options.skip_fields =
                            size_opt(&value, "invalid number of fields to skip")?;
                        draft.skip_fields_from_f = true;
                    }
                    b's' => {
                        draft.options.skip_chars =
                            size_opt(&value, "invalid number of bytes to skip")?;
                    }
                    _ => {
                        draft.options.check_chars =
                            size_opt(&value, "invalid number of bytes to compare")?;
                    }
                }
            }
            _ => return Err(UNIQ.invalid_option(c)),
        }
    }
    Ok(())
}

/// One digit of the obsolete `-N` skip-fields form.
///
/// Each digit shifts the running value by a decimal place, and the digits need
/// not be adjacent — they are separate short options, so `uniq -1 -2` and
/// `uniq -12` both skip twelve fields. An intervening `-f` restarts the
/// accumulator; anything else does not.
fn accumulate_digit(c: u8, draft: &mut Draft) {
    if draft.skip_fields_from_f {
        draft.options.skip_fields = 0;
        draft.skip_fields_from_f = false;
    }
    draft.options.skip_fields = draft
        .options
        .skip_fields
        .checked_mul(10)
        .and_then(|v| v.checked_add(u64::from(c.wrapping_sub(b'0'))))
        // Upstream's `DECIMAL_DIGIT_ACCUMULATE` reports overflow and `uniq`
        // answers it with `SIZE_MAX` rather than a diagnostic — the same
        // saturation `-f` gives an over-large argument.
        .unwrap_or(u64::MAX);
}

/// A number too large to hold, kept apart from an exact one because the two
/// argv forms disagree about it: `-s 10^30` saturates and runs, `+10^30` is a
/// file name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Number {
    Exact(u64),
    Overflowed,
}

/// gnulib's `xstrtoumax (s, nullptr, 10, &v, "")` — base ten, no size suffixes.
///
/// The grammar is `strtoumax`'s and is looser at the front than at the back:
/// leading whitespace and a leading `+` are accepted, a leading `-` is refused
/// outright by `xstrtoumax`'s own unsigned check, and **any** trailing byte —
/// including trailing whitespace — makes the whole thing invalid. That is why
/// `uniq -f 0x10` is rejected: base ten consumes the `0` and stops at the `x`.
fn xstrtoumax(bytes: &[u8]) -> Option<Number> {
    let mut i = 0usize;
    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
        i = i.saturating_add(1);
    }
    // `xstrtoumax` looks for this itself, before calling `strtoumax`, because
    // `strtoumax` would happily negate into a huge unsigned value.
    if bytes.get(i) == Some(&b'-') {
        return None;
    }
    if bytes.get(i) == Some(&b'+') {
        i = i.saturating_add(1);
    }
    let digits = bytes.get(i..)?;
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut value: u64 = 0;
    let mut overflowed = false;
    for &d in digits {
        match value
            .checked_mul(10)
            .and_then(|v| v.checked_add(u64::from(d.wrapping_sub(b'0'))))
        {
            Some(v) => value = v,
            None => overflowed = true,
        }
    }
    Some(if overflowed {
        Number::Overflowed
    } else {
        Number::Exact(value)
    })
}

/// `uniq`'s `size_opt`: a count, saturating rather than refusing when too
/// large.
///
/// The diagnostic is the odd one out in this directory twice over. It carries
/// **no** `Try '… --help'` referral, because upstream reports it with
/// `error (EXIT_FAILURE, …)` and exits on the spot; and the offending argument
/// is **not quoted at all** — the format string is a bare `"%s: %s"`, where
/// every neighbouring message runs the argument through `quote()`.
///
/// ```text
/// $ uniq -f x
/// uniq: x: invalid number of fields to skip
/// ```
fn size_opt(bytes: &[u8], what: &str) -> Result<u64, getopt::Error> {
    match xstrtoumax(bytes) {
        Some(Number::Exact(v)) => Ok(v),
        Some(Number::Overflowed) => Ok(u64::MAX),
        None => Err(UNIQ.usage(format!(
            "{}: {what}",
            String::from_utf8_lossy(bytes).into_owned()
        ))),
    }
}

/// The cross-checks that can only run once every argument has been seen.
///
/// The second of upstream's three is unreachable and is kept here for the same
/// reason it is kept there — so that a later edit which makes it reachable
/// finds it already written. `-c` sets `output_option_used`, so the first check
/// fires first for every command line that would have reached the second.
///
/// # Errors
///
/// A `--group` combined with an output-selecting option, or `-c` with `-D`.
fn finish(draft: &Draft) -> Result<Options, getopt::Error> {
    let options = draft.options;
    if options.grouping != Grouping::None && draft.output_option_used {
        return Err(
            UNIQ.usage_referring("--group is mutually exclusive with -c/-d/-D/-u".to_string())
        );
    }
    if options.grouping != Grouping::None && options.count {
        return Err(
            UNIQ.usage_referring("grouping and printing repeat counts is meaningless".to_string())
        );
    }
    if options.count && options.select.later_repeated {
        return Err(UNIQ.usage_referring(
            "printing all duplicated lines and repeat counts is meaningless".to_string(),
        ));
    }
    Ok(options)
}

/// What separates one field from the next: `isblank() || '\n'`, which under the
/// C locale is a space, a tab, or a newline.
///
/// The newline is not redundant even though lines are split on it, because
/// under `-z` they are not: a NUL-delimited record may contain newlines, and
/// each one still ends a field.
fn field_sep(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\n'
}

/// Where the compared part of a line begins.
///
/// A line here always ends with the delimiter — `Reader` appends one at an
/// unterminated end of file — so the searchable extent is one byte short of the
/// buffer. Fields are skipped first and characters after, in that order, which
/// is why `-s` counts from wherever `-f` stopped rather than from the start of
/// the line.
fn find_field(line: &[u8], options: &Options) -> usize {
    let size = line.len().saturating_sub(1);
    let mut i = 0usize;
    let mut count = 0u64;
    while count < options.skip_fields && i < size {
        // A field is the run of separators before it plus the run of
        // non-separators that follows, so a leading blank does not make an
        // empty first field.
        while i < size && line.get(i).copied().is_some_and(field_sep) {
            i = i.saturating_add(1);
        }
        while i < size && line.get(i).copied().is_some_and(|c| !field_sep(c)) {
            i = i.saturating_add(1);
        }
        count = count.saturating_add(1);
    }
    let remaining = size.saturating_sub(i);
    i.saturating_add(clamp_usize(options.skip_chars).min(remaining))
}

/// A `u64` count as an index, saturating — upstream's counts are `size_t` and
/// this is the one place the two widths could disagree.
fn clamp_usize(v: u64) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

/// The compared extent of a line: from `at` up to but not including the
/// delimiter.
fn field_of(line: &[u8], at: usize) -> &[u8] {
    line.get(at..line.len().saturating_sub(1))
        .unwrap_or_default()
}

/// Whether two already-trimmed fields differ, under `-w` and `-i`.
///
/// Truncation to `check_chars` happens before the length comparison, which is
/// what makes `-w N` mean "compare at most N characters" rather than "compare
/// the first N and then the rest": `uniq -w1` collapses `ab` and `ax`.
fn different(a: &[u8], b: &[u8], options: &Options) -> bool {
    let cap = clamp_usize(options.check_chars);
    let a = a.get(..cap.min(a.len())).unwrap_or_default();
    let b = b.get(..cap.min(b.len())).unwrap_or_default();
    if a.len() != b.len() {
        return true;
    }
    if options.ignore_case {
        // gnulib's `memcasecmp` folds with `toupper`. Folding the other way
        // agrees on every ASCII byte, and ASCII is all the C locale has —
        // bytes above 127 are left alone either way, so a UTF-8 `é` is not
        // folded onto `É` here any more than it is there.
        !a.eq_ignore_ascii_case(b)
    } else {
        a != b
    }
}

/// gnulib's `readlinebuffer_delim`, which differs from `BufRead::lines` in the
/// two ways that matter here.
///
/// It **keeps the delimiter** in the buffer, and it **appends one** to a final
/// line that had none, so that every line is uniform and the comparison extent
/// is always `len - 1`. That is also why `uniq` echoes a missing final newline
/// back as a present one.
///
/// A read error is recorded rather than returned, because upstream discovers it
/// through `ferror` after the loop and reports it as `error reading FILE`
/// naming the operand — which the loop does not know.
struct Reader<R: BufRead> {
    src: R,
    error: Option<io::Error>,
}

impl<R: BufRead> Reader<R> {
    fn new(src: R) -> Self {
        Reader { src, error: None }
    }

    /// Fill `buf` with the next line, delimiter included, returning false at
    /// end of input or on the first error.
    fn read_line(&mut self, buf: &mut Vec<u8>, delimiter: u8) -> bool {
        buf.clear();
        match self.src.read_until(delimiter, buf) {
            Ok(0) => false,
            Ok(_) => {
                if buf.last() != Some(&delimiter) {
                    buf.push(delimiter);
                }
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }
}

/// Print one line, if the mode asks for it.
///
/// `linecount` is the number of *matches*, so a group of one has zero and the
/// printed count is one more. The three-way test is upstream's, and it is a
/// choice between the three flags rather than a disjunction of them: which one
/// applies depends on where in its group the line sits.
fn writeline<W: Write>(
    out: &mut W,
    line: &[u8],
    matched: bool,
    linecount: u64,
    options: &Options,
) -> io::Result<()> {
    let wanted = if linecount == 0 {
        options.select.unique
    } else if matched {
        options.select.later_repeated
    } else {
        options.select.first_repeated
    };
    if !wanted {
        return Ok(());
    }
    if options.count {
        // `%7ju ` — right-aligned in seven columns, then a space, and wider
        // counts push the line right rather than being truncated.
        write!(out, "{:>7} ", linecount.saturating_add(1))?;
    }
    out.write_all(line)
}

/// The path that can print a line the moment it is read.
///
/// It applies when every line of a group would be printed anyway — no `-c`,
/// no `-d`, no `-u`, no `-D` — so the only thing still unknown when a line
/// arrives is whether to precede it with a group separator, and that depends
/// on the line itself rather than on the next one. `--group` qualifies for the
/// same reason: it prints everything.
fn uniq_streaming<R: BufRead, W: Write>(
    reader: &mut Reader<R>,
    out: &mut W,
    options: &Options,
) -> io::Result<()> {
    let mut thisline: Vec<u8> = Vec::new();
    let mut prevline: Vec<u8> = Vec::new();
    let mut prev: Option<usize> = None;
    let mut first_group_printed = false;

    while reader.read_line(&mut thisline, options.delimiter) {
        let thisfield = find_field(&thisline, options);
        let new_group = match prev {
            None => true,
            Some(prevfield) => different(
                field_of(&thisline, thisfield),
                field_of(&prevline, prevfield),
                options,
            ),
        };

        if new_group
            && options.grouping != Grouping::None
            && (options.grouping == Grouping::Prepend
                || options.grouping == Grouping::Both
                || (first_group_printed
                    && (options.grouping == Grouping::Append
                        || options.grouping == Grouping::Separate)))
        {
            out.write_all(&[options.delimiter])?;
        }

        if new_group || options.grouping != Grouping::None {
            out.write_all(&thisline)?;
            std::mem::swap(&mut prevline, &mut thisline);
            prev = Some(thisfield);
            first_group_printed = true;
        }
    }

    if (options.grouping == Grouping::Both || options.grouping == Grouping::Append)
        && first_group_printed
    {
        out.write_all(&[options.delimiter])?;
    }
    Ok(())
}

/// The general path, which must hold a line back until it knows how long its
/// group is.
fn uniq_buffered<R: BufRead, W: Write>(
    reader: &mut Reader<R>,
    out: &mut W,
    options: &Options,
) -> io::Result<()> {
    let mut thisline: Vec<u8> = Vec::new();
    let mut prevline: Vec<u8> = Vec::new();

    if !reader.read_line(&mut prevline, options.delimiter) {
        return Ok(());
    }
    let mut prevfield = find_field(&prevline, options);
    let mut match_count: u64 = 0;
    let mut first_delimiter = true;

    loop {
        if !reader.read_line(&mut thisline, options.delimiter) {
            // A read error abandons the input without the final `writeline`,
            // because the caller is about to report it and exit non-zero.
            if reader.error.is_some() {
                return Ok(());
            }
            break;
        }
        let thisfield = find_field(&thisline, options);
        let matched = !different(
            field_of(&thisline, thisfield),
            field_of(&prevline, prevfield),
            options,
        );
        match_count = match_count.saturating_add(u64::from(matched));
        if match_count == u64::MAX {
            // Upstream guards a `too many repeated lines` error here with
            // `if (count_occurrences)` — the *enum constant*, whose value is
            // zero — so the error is unreachable and the counter simply stops.
            // Reproduced rather than tidied; either way it needs 2^64 equal
            // lines to observe.
            match_count = match_count.saturating_sub(1);
        }

        if options.delimit != Delimit::None {
            if matched {
                if match_count == 1
                    && (options.delimit == Delimit::Prepend
                        || (options.delimit == Delimit::Separate && !first_delimiter))
                {
                    out.write_all(&[options.delimiter])?;
                }
            } else if match_count != 0 {
                // A group has already been printed, so from here on
                // `--all-repeated=separate` has something to separate from.
                first_delimiter = false;
            }
        }

        if !matched || options.select.later_repeated {
            writeline(out, &prevline, matched, match_count, options)?;
            std::mem::swap(&mut prevline, &mut thisline);
            prevfield = thisfield;
            if !matched {
                match_count = 0;
            }
        }
    }

    writeline(out, &prevline, false, match_count, options)
}

/// Whether the fast path applies. Extracted so that a test can assert the
/// two paths agree rather than assuming they do.
fn can_stream(options: &Options) -> bool {
    options.select.unique && options.select.first_repeated && !options.count
}

fn uniq_stream<R: BufRead, W: Write>(
    reader: &mut Reader<R>,
    out: &mut W,
    options: &Options,
) -> io::Result<()> {
    if can_stream(options) {
        uniq_streaming(reader, out, options)
    } else {
        uniq_buffered(reader, out, options)
    }
}

fn run(options: &Options, input: &OsString, output: &OsString) -> ExitCode {
    // Input is opened first, so a bad input name leaves the output file
    // untouched rather than truncating it and then failing.
    let source: Box<dyn Read> = if input == OsStr::new("-") {
        Box::new(io::stdin())
    } else {
        match File::open(input) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("uniq: {}: {}", quotef_os(input), strerror(&e));
                return ExitCode::from(1);
            }
        }
    };
    let sink: Box<dyn Write> = if output == OsStr::new("-") {
        Box::new(io::stdout())
    } else {
        match File::create(output) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("uniq: {}: {}", quotef_os(output), strerror(&e));
                return ExitCode::from(1);
            }
        }
    };

    let mut reader = Reader::new(BufReader::with_capacity(64 * 1024, source));
    let mut out = BufWriter::with_capacity(64 * 1024, sink);

    if let Err(e) = uniq_stream(&mut reader, &mut out, options) {
        return write_failure(&e);
    }
    // Everything written so far is flushed before the read error is reported,
    // which is what GNU's `atexit (close_stdout)` does.
    if let Err(e) = out.flush() {
        return write_failure(&e);
    }
    if let Some(e) = reader.error.as_ref() {
        // `quoteaf` — always quoted — where the open failure above uses
        // `quotef`, which elides the quotes when the name needs none.
        eprintln!("uniq: error reading {}: {}", quoteaf_os(input), strerror(e));
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// A failed write. GNU dies of `SIGPIPE` when the reader goes away, printing
/// nothing; Rust masks that signal, so the same situation arrives as `EPIPE`
/// and has to be recognised and kept quiet. Any other write failure is
/// upstream's `write_error()`.
fn write_failure(e: &io::Error) -> ExitCode {
    if e.kind() == ErrorKind::BrokenPipe {
        return ExitCode::SUCCESS;
    }
    eprintln!("uniq: write error: {}", strerror(e));
    ExitCode::from(1)
}

/// An operand's bytes. Paths are bytes on the target and 16-bit units on the
/// host; this is the one place that difference is absorbed.
fn arg_bytes(a: &OsString) -> Vec<u8> {
    os_bytes(a.as_os_str()).into_owned()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// The options a command line parses to, panicking if it does not parse.
    fn parse(items: &[&str]) -> Options {
        match parse_args(&args(items), Env::default()) {
            Ok(Request::Run(o, _, _)) => o,
            other => panic!("expected a runnable request, got {other:?}"),
        }
    }

    /// The two operands a command line parses to.
    fn operands(items: &[&str]) -> (String, String) {
        match parse_args(&args(items), Env::default()) {
            Ok(Request::Run(_, i, o)) => (
                i.to_string_lossy().into_owned(),
                o.to_string_lossy().into_owned(),
            ),
            other => panic!("expected a runnable request, got {other:?}"),
        }
    }

    /// The error a command line fails with, panicking if it succeeds.
    fn fail(items: &[&str]) -> getopt::Error {
        match parse_args(&args(items), Env::default()) {
            Err(e) => e,
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    /// A diagnostic without its `Try '…'` referral, which most of these carry
    /// and none of them is about.
    fn body(e: &getopt::Error) -> String {
        e.message
            .split_once("\nTry '")
            .map_or_else(|| e.message.clone(), |(head, _)| head.to_string())
    }

    /// Run the utility over `input` and return exactly what it wrote.
    fn uniq(input: &[u8], items: &[&str]) -> Vec<u8> {
        let options = parse(items);
        let mut reader = Reader::new(io::BufReader::new(input));
        let mut out: Vec<u8> = Vec::new();
        uniq_stream(&mut reader, &mut out, &options).unwrap();
        out
    }

    /// The same, forced down the general path, so the two can be compared.
    fn uniq_slow(input: &[u8], items: &[&str]) -> Vec<u8> {
        let options = parse(items);
        let mut reader = Reader::new(io::BufReader::new(input));
        let mut out: Vec<u8> = Vec::new();
        uniq_buffered(&mut reader, &mut out, &options).unwrap();
        out
    }

    // ---------------- the option table ----------------

    #[test]
    fn long_options_abbreviate_the_way_getopt_long_does() {
        assert!(parse(&["--coun"]).count);
        assert!(parse(&["--ignore"]).ignore_case);
        assert_eq!(parse(&["--skip-f", "3"]).skip_fields, 3);
        // An exact name wins outright even where it is a prefix of nothing:
        // `--group` is unambiguous, `--g` is too.
        assert_eq!(parse(&["--g"]).grouping, Grouping::Separate);
    }

    #[test]
    fn the_ambiguous_list_is_in_gnus_declaration_order() {
        assert_eq!(
            body(&fail(&["--c"])),
            "option '--c' is ambiguous; possibilities: '--count' '--check-chars'"
        );
        assert_eq!(
            body(&fail(&["--s"])),
            "option '--s' is ambiguous; possibilities: '--skip-fields' '--skip-chars'"
        );
        // Alphabetically `--check-chars` precedes `--count`, and `--skip-chars`
        // precedes `--skip-fields`. Neither does here, which is what would
        // catch a table someone had tidied into alphabetical order.
        let all = fail(&["--=x"]).message;
        assert!(all.find("'--count'") < all.find("'--check-chars'"), "{all}");
        assert!(all.find("'--group'") < all.find("'--unique'"), "{all}");
    }

    #[test]
    fn the_five_getopt_sentences() {
        assert_eq!(body(&fail(&["-x"])), "invalid option -- 'x'");
        assert_eq!(body(&fail(&["-f"])), "option requires an argument -- 'f'");
        assert_eq!(body(&fail(&["--zz"])), "unrecognized option '--zz'");
        assert_eq!(
            body(&fail(&["--skip-f"])),
            "option '--skip-fields' requires an argument"
        );
        assert_eq!(
            body(&fail(&["--count=3"])),
            "option '--count' doesn't allow an argument"
        );
        // All of them are status 1: `uniq` has not spent 1 on anything else.
        assert_eq!(fail(&["-x"]).status, 1);
    }

    #[test]
    fn an_optional_argument_is_never_taken_from_the_next_word() {
        // `--group separate` groups by the default method and reads a file
        // called `separate` — it does not parse `separate` as the method.
        let (input, output) = operands(&["--group", "separate"]);
        assert_eq!((input.as_str(), output.as_str()), ("separate", "-"));
        assert_eq!(parse(&["--group", "separate"]).grouping, Grouping::Separate);
        assert_eq!(parse(&["--group=append"]).grouping, Grouping::Append);
        // Same for `--all-repeated`, whose default is a *different* word.
        assert_eq!(parse(&["--all-repeated"]).delimit, Delimit::None);
        assert_eq!(parse(&["--all-repeated=prepend"]).delimit, Delimit::Prepend);
    }

    #[test]
    fn an_option_argument_abbreviates_and_reports_the_table_it_matched() {
        assert_eq!(parse(&["--group=b"]).grouping, Grouping::Both);
        assert_eq!(parse(&["--all-repeated=s"]).delimit, Delimit::Separate);
        let e = fail(&["--group=zz"]);
        assert_eq!(
            body(&e),
            "invalid argument 'zz' for '--group'\nValid arguments are:\n  \
             - 'prepend'\n  - 'append'\n  - 'separate'\n  - 'both'"
        );
        // The empty word matches all four, which disagree.
        assert!(body(&fail(&["--group="])).starts_with("ambiguous argument '' for '--group'"));
        assert_eq!(fail(&["--group=zz"]).status, 1);
    }

    #[test]
    fn a_later_dash_d_resets_the_separator_the_long_form_asked_for() {
        assert_eq!(
            parse(&["--all-repeated=separate", "-D"]).delimit,
            Delimit::None
        );
        assert_eq!(
            parse(&["-D", "--all-repeated=separate"]).delimit,
            Delimit::Separate
        );
    }

    // ---------------- the obsolete argv forms ----------------

    #[test]
    fn digits_accumulate_across_separate_arguments() {
        assert_eq!(parse(&["-1", "-2"]).skip_fields, 12);
        assert_eq!(parse(&["-12"]).skip_fields, 12);
        assert_eq!(parse(&["-1", "-2", "-3"]).skip_fields, 123);
        // A non-digit option in between does not interrupt the accumulator.
        assert_eq!(parse(&["-1", "-i", "-2"]).skip_fields, 12);
    }

    #[test]
    fn a_skip_fields_option_restarts_the_digit_accumulator() {
        assert_eq!(parse(&["-f3", "-1"]).skip_fields, 1);
        assert_eq!(parse(&["--skip-fields=3", "-1"]).skip_fields, 1);
        // …but only once: the digits after it accumulate among themselves.
        assert_eq!(parse(&["-f3", "-1", "-2"]).skip_fields, 12);
        // And the last one wins the other way round.
        assert_eq!(parse(&["-1", "-f3"]).skip_fields, 3);
    }

    #[test]
    fn too_many_digits_saturate_rather_than_failing() {
        let many: Vec<&str> = vec!["-9"; 25];
        assert_eq!(parse(&many).skip_fields, u64::MAX);
    }

    #[test]
    fn a_plus_operand_is_a_skip_chars_count_unless_disqualified() {
        assert_eq!(parse(&["+3"]).skip_chars, 3);
        assert_eq!(operands(&["+3"]), ("-".to_string(), "-".to_string()));
        // It is checked wherever an operand is, including after other operands.
        assert_eq!(parse(&["f.txt", "+3"]).skip_chars, 3);

        // Disqualifier 1: `--` stops the inspection, so it is a file name.
        assert_eq!(operands(&["--", "+3"]), ("+3".to_string(), "-".to_string()));
        assert_eq!(parse(&["--", "+3"]).skip_chars, 0);

        // Disqualifier 2: a value too large to hold exactly.
        assert_eq!(
            operands(&["+99999999999999999999999"]),
            ("+99999999999999999999999".to_string(), "-".to_string())
        );
        // Where `-s` with the same digits saturates and runs.
        assert_eq!(
            parse(&["-s", "99999999999999999999999"]).skip_chars,
            u64::MAX
        );

        // Disqualifier 3: not a number at all.
        assert_eq!(operands(&["+x"]), ("+x".to_string(), "-".to_string()));
    }

    #[test]
    fn a_withdrawn_standard_makes_plus_n_a_file_name() {
        let strict = Env {
            posixly_correct: false,
            strict_posix2: true,
        };
        match parse_args(&args(&["+3"]), strict) {
            Ok(Request::Run(o, input, _)) => {
                assert_eq!(o.skip_chars, 0);
                assert_eq!(input, OsString::from("+3"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_posix2_version_window_is_half_open() {
        let strict = |v: &str| strict_posix2(Some(OsStr::new(v)));
        assert!(!strict("200111"));
        assert!(strict("200112"));
        assert!(strict("200808"));
        assert!(!strict("200809"));
        assert!(!strict("999999"));
        // `strtol` skips leading whitespace but refuses a trailing byte, in
        // which case the whole variable is ignored and the default applies.
        assert!(strict(" 200112"));
        assert!(!strict("200112x"));
        // Unset and empty both mean the default.
        assert!(!strict(""));
        assert!(!strict_posix2(None));
    }

    #[test]
    fn posixly_correct_ends_option_parsing_at_the_first_operand() {
        let posix = Env {
            posixly_correct: true,
            strict_posix2: false,
        };
        // `-c` after an operand becomes the *output file*, which is why this
        // rule is worth a test rather than a comment.
        match parse_args(&args(&["in.txt", "-c"]), posix) {
            Ok(Request::Run(o, input, output)) => {
                assert!(!o.count);
                assert_eq!(input, OsString::from("in.txt"));
                assert_eq!(output, OsString::from("-c"));
            }
            other => panic!("{other:?}"),
        }
        // Before the first operand it is still an option.
        match parse_args(&args(&["-c", "in.txt"]), posix) {
            Ok(Request::Run(o, input, output)) => {
                assert!(o.count);
                assert_eq!(input, OsString::from("in.txt"));
                assert_eq!(output, OsString::from("-"));
            }
            other => panic!("{other:?}"),
        }
        // `+3` after an operand is not inspected either.
        match parse_args(&args(&["in.txt", "+3"]), posix) {
            Ok(Request::Run(o, _, output)) => {
                assert_eq!(o.skip_chars, 0);
                assert_eq!(output, OsString::from("+3"));
            }
            other => panic!("{other:?}"),
        }
        // Without the variable, the same line is an option and a count.
        assert!(parse(&["in.txt", "-c"]).count);
    }

    // ---------------- operands ----------------

    #[test]
    fn the_second_operand_is_the_output_and_the_third_is_refused() {
        assert_eq!(operands(&["a"]), ("a".to_string(), "-".to_string()));
        assert_eq!(operands(&["a", "b"]), ("a".to_string(), "b".to_string()));
        assert_eq!(operands(&[]), ("-".to_string(), "-".to_string()));
        // A lone `-` is an operand, not an option.
        assert_eq!(operands(&["-", "b"]), ("-".to_string(), "b".to_string()));
        let e = fail(&["a", "b", "c"]);
        assert_eq!(body(&e), "extra operand 'c'");
        // This one *does* refer to --help: upstream reports it and then calls
        // usage(), where the number diagnostics exit on the spot.
        assert!(
            e.message
                .ends_with("Try 'uniq --help' for more information.")
        );
        assert_eq!(e.status, 1);
    }

    // ---------------- numbers ----------------

    #[test]
    fn the_three_number_diagnostics_are_unquoted_and_do_not_refer_to_help() {
        for (option, expected) in [
            ("-f", "x: invalid number of fields to skip"),
            ("-s", "x: invalid number of bytes to skip"),
            ("-w", "x: invalid number of bytes to compare"),
        ] {
            let e = fail(&[option, "x"]);
            assert_eq!(e.message, expected, "{option}");
            assert!(!e.message.contains("Try '"), "{option}: {e}");
            assert_eq!(e.status, 1);
        }
        // Not quoted at all — the format string is a bare `"%s: %s"`.
        assert_eq!(
            fail(&["-f", "a b"]).message,
            "a b: invalid number of fields to skip"
        );
    }

    #[test]
    fn the_number_grammar_is_strtoumaxs() {
        assert_eq!(xstrtoumax(b"5"), Some(Number::Exact(5)));
        assert_eq!(xstrtoumax(b" 5"), Some(Number::Exact(5)));
        assert_eq!(xstrtoumax(b"\t+5"), Some(Number::Exact(5)));
        assert_eq!(xstrtoumax(b"007"), Some(Number::Exact(7)));
        assert_eq!(
            xstrtoumax(b"18446744073709551616"),
            Some(Number::Overflowed)
        );
        // A leading `-` is refused by xstrtoumax before strtoumax could negate
        // it into a huge unsigned value.
        assert_eq!(xstrtoumax(b"-5"), None);
        // Any trailing byte invalidates the whole thing, trailing space too.
        assert_eq!(xstrtoumax(b"5x"), None);
        assert_eq!(xstrtoumax(b"5 "), None);
        // Base ten stops at the `x`, leaving trailing garbage.
        assert_eq!(xstrtoumax(b"0x10"), None);
        assert_eq!(xstrtoumax(b""), None);
        assert_eq!(xstrtoumax(b"+"), None);
        // Overflow saturates for -f/-s/-w …
        assert_eq!(parse(&["-f", "18446744073709551616"]).skip_fields, u64::MAX);
    }

    // ---------------- cross-checks ----------------

    #[test]
    fn group_is_refused_alongside_every_output_selecting_option() {
        for other in [
            vec!["-c"],
            vec!["-d"],
            vec!["-D"],
            vec!["-u"],
            vec!["--all-repeated=separate"],
            vec!["--repeated"],
        ] {
            let mut items = vec!["--group"];
            items.extend(other.iter().copied());
            let e = fail(&items);
            assert_eq!(
                body(&e),
                "--group is mutually exclusive with -c/-d/-D/-u",
                "{items:?}"
            );
            assert_eq!(e.status, 1);
        }
    }

    #[test]
    fn counting_every_duplicate_is_refused_and_the_checks_have_an_order() {
        assert_eq!(
            body(&fail(&["-c", "-D"])),
            "printing all duplicated lines and repeat counts is meaningless"
        );
        assert_eq!(
            body(&fail(&["-c", "--all-repeated=separate"])),
            "printing all duplicated lines and repeat counts is meaningless"
        );
        // With `--group` as well, the *first* check fires — which is why the
        // second (unreachable) one can never be observed: `-c` sets
        // output_option_used, so check one always gets there first.
        assert_eq!(
            body(&fail(&["-c", "-D", "--group"])),
            "--group is mutually exclusive with -c/-d/-D/-u"
        );
        // `-d -u` is not a conflict; it just prints nothing.
        let o = parse(&["-d", "-u"]);
        assert!(!o.select.unique && !o.select.first_repeated);
    }

    // ---------------- comparison ----------------

    #[test]
    fn merging_keeps_the_first_of_each_run() {
        assert_eq!(uniq(b"a\na\nb\na\n", &[]), b"a\nb\na\n");
        // Non-adjacent duplicates are not merged, which is the whole contract.
        assert_eq!(uniq(b"a\nb\na\n", &[]), b"a\nb\na\n");
        assert_eq!(uniq(b"", &[]), b"");
    }

    #[test]
    fn an_unterminated_last_line_comes_back_terminated() {
        assert_eq!(uniq(b"a\na", &[]), b"a\n");
        assert_eq!(uniq(b"a\nb", &[]), b"a\nb\n");
        assert_eq!(uniq(b"a", &["-c"]), b"      1 a\n");
    }

    #[test]
    fn the_count_is_right_aligned_in_seven_columns() {
        assert_eq!(uniq(b"a\n", &["-c"]), b"      1 a\n");
        assert_eq!(uniq(b"a\na\n", &["-c"]), b"      2 a\n");
        let many: Vec<u8> = b"x\n".repeat(1234);
        assert_eq!(uniq(&many, &["-c"]), b"   1234 x\n");
    }

    #[test]
    fn d_u_and_capital_d_select_different_parts_of_a_group() {
        let input = b"a\na\nb\nc\nc\nc\n";
        assert_eq!(uniq(input, &["-d"]), b"a\nc\n");
        assert_eq!(uniq(input, &["-u"]), b"b\n");
        assert_eq!(uniq(input, &["-D"]), b"a\na\nc\nc\nc\n");
        assert_eq!(uniq(input, &["-d", "-u"]), b"");
    }

    #[test]
    fn all_repeated_separators_go_between_groups_or_before_them() {
        let input = b"a\na\nb\nc\nc\nc\nd\n";
        assert_eq!(
            uniq(input, &["--all-repeated=separate"]),
            b"a\na\n\nc\nc\nc\n".as_slice()
        );
        assert_eq!(
            uniq(input, &["--all-repeated=prepend"]),
            b"\na\na\n\nc\nc\nc\n".as_slice()
        );
        assert_eq!(uniq(input, &["--all-repeated=none"]), b"a\na\nc\nc\nc\n");
    }

    #[test]
    fn group_prints_everything_and_differs_only_in_where_the_blanks_go() {
        let input = b"a\na\nb\n";
        assert_eq!(uniq(input, &["--group"]), b"a\na\n\nb\n");
        assert_eq!(uniq(input, &["--group=separate"]), b"a\na\n\nb\n");
        assert_eq!(uniq(input, &["--group=prepend"]), b"\na\na\n\nb\n");
        assert_eq!(uniq(input, &["--group=append"]), b"a\na\n\nb\n\n");
        assert_eq!(uniq(input, &["--group=both"]), b"\na\na\n\nb\n\n");
        // An empty input gets no separators at all, not even from `both`.
        assert_eq!(uniq(b"", &["--group=both"]), b"");
    }

    #[test]
    fn fields_are_skipped_before_chars() {
        // Two fields differing only in the first: `-f1` merges them.
        assert_eq!(uniq(b"x same\ny same\n", &["-f1"]), b"x same\n");
        assert_eq!(uniq(b"x same\ny same\n", &[]), b"x same\ny same\n");
        // A field is its leading blanks plus the run that follows, so a leading
        // blank does not count as an empty first field.
        assert_eq!(uniq(b"  x a\n  y a\n", &["-f1"]), b"  x a\n");
        // `-s` counts from where `-f` stopped, not from the start of the line —
        // and where it stops is *before* the blank that begins the next field,
        // so skipping one field and then two characters lands on `b`.
        assert_eq!(uniq(b"aa Xbc\naa Ybc\n", &["-f1", "-s2"]), b"aa Xbc\n");
        assert_eq!(
            uniq(b"aa Xbc\naa Ybc\n", &["-f1", "-s1"]),
            b"aa Xbc\naa Ybc\n"
        );
        assert_eq!(uniq(b"aa Xbc\naa Ybc\n", &["-s2"]), b"aa Xbc\naa Ybc\n");
        // Skipping past the end leaves nothing, so every line matches.
        assert_eq!(uniq(b"a\nbb\nccc\n", &["-f9"]), b"a\n");
        assert_eq!(uniq(b"a\nbb\nccc\n", &["-s9"]), b"a\n");
    }

    #[test]
    fn check_chars_truncates_before_the_length_comparison() {
        assert_eq!(uniq(b"ab\nax\n", &["-w1"]), b"ab\n");
        assert_eq!(uniq(b"ab\nax\n", &["-w2"]), b"ab\nax\n");
        // `-w0` compares nothing, so every line matches every other.
        assert_eq!(uniq(b"ab\ncd\n", &["-w0"]), b"ab\n");
        // Different lengths within the window still differ.
        assert_eq!(uniq(b"a\nab\n", &["-w9"]), b"a\nab\n");
    }

    #[test]
    fn ignore_case_folds_ascii_only() {
        assert_eq!(uniq(b"Ab\naB\n", &["-i"]), b"Ab\n");
        assert_eq!(uniq(b"Ab\naB\n", &[]), b"Ab\naB\n");
        // 0xC3 0xA9 is `é` in UTF-8; neither byte is an ASCII letter, so
        // folding leaves them alone rather than mangling the encoding.
        assert_eq!(
            uniq(b"\xc3\xa9\n\xc3\x89\n", &["-i"]),
            b"\xc3\xa9\n\xc3\x89\n"
        );
    }

    #[test]
    fn zero_terminated_changes_the_line_delimiter_but_not_the_field_separator() {
        assert_eq!(uniq(b"a\0a\0b\0", &["-z"]), b"a\0b\0");
        // A newline inside a NUL-delimited record still ends a field.
        assert_eq!(uniq(b"x\na\0y\na\0", &["-z", "-f1"]), b"x\na\0");
        // And an unterminated last record gets a NUL, not a newline.
        assert_eq!(uniq(b"a\0b", &["-z"]), b"a\0b\0");
    }

    #[test]
    fn bytes_that_are_not_text_survive_unchanged() {
        // A CR is data, not part of the line ending: these two lines differ.
        assert_eq!(uniq(b"a\r\na\n", &[]), b"a\r\na\n");
        // An invalid UTF-8 byte is compared and echoed as itself.
        assert_eq!(uniq(b"\xff\n\xff\n\xfe\n", &[]), b"\xff\n\xfe\n");
    }

    // ---------------- the two paths ----------------

    #[test]
    fn the_fast_path_is_taken_exactly_when_every_line_is_printed() {
        assert!(can_stream(&parse(&[])));
        assert!(can_stream(&parse(&["--group"])));
        assert!(can_stream(&parse(&["-i", "-f1", "-w3", "-z"])));
        assert!(!can_stream(&parse(&["-c"])));
        assert!(!can_stream(&parse(&["-d"])));
        assert!(!can_stream(&parse(&["-u"])));
        assert!(!can_stream(&parse(&["-D"])));
    }

    #[test]
    fn the_two_paths_agree_wherever_both_apply() {
        // The fast path is an optimisation, so anything it handles the general
        // path must handle identically. This is the assertion that would catch
        // a change to one that was not made to the other.
        for input in [
            b"a\na\nb\n".as_slice(),
            b"a\nb\na\n",
            b"x\n",
            b"",
            b"a\na\na\n",
            b"a",
        ] {
            for items in [
                vec![],
                vec!["-i"],
                vec!["-f1"],
                vec!["-w1"],
                vec!["-s1"],
                vec!["-z"],
            ] {
                assert_eq!(
                    uniq(input, &items),
                    uniq_slow(input, &items),
                    "{items:?} over {input:?}"
                );
            }
        }
    }

    // ---------------- help ----------------

    #[test]
    fn help_and_version_end_parsing_wherever_they_appear() {
        assert_eq!(
            parse_args(&args(&["-c", "--help", "--bogus"]), Env::default()),
            Ok(Request::Help)
        );
        assert_eq!(
            parse_args(&args(&["--version"]), Env::default()),
            Ok(Request::Version)
        );
        // But not after `--`, where they are file names.
        assert_eq!(
            operands(&["--", "--help"]),
            ("--help".to_string(), "-".to_string())
        );
    }

    #[test]
    fn the_help_text_is_gnus() {
        let text = help_text();
        assert!(text.starts_with("Usage: uniq [OPTION]... [INPUT [OUTPUT]]\n"));
        assert!(text.ends_with("use 'sort -u' without 'uniq'.\n"));
        // The ragged indentation of the shared-macro lines is upstream's.
        assert!(text.contains("  -z, --zero-terminated     line delimiter is NUL, not newline\n"));
        assert!(text.contains("      --help        display this help and exit\n"));
        // Every option in the table is documented, which is the check that
        // catches a table and a help text drifting apart.
        for (name, _) in LONG_OPTIONS {
            assert!(text.contains(&format!("--{name}")), "{name} undocumented");
        }
    }
}
