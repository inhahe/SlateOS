//! tail — output the last part of files.
//!
//! The fifth of the 85 utilities moved onto the shared [`coreutils::getopt`]
//! (see `known-issues.md` → `TD-COREUTILS-LONG-OPTIONS-DO-NOT-ABBREVIATE`), and
//! by some distance the largest: the parser this replaces knew `-n` and nothing
//! else, and the thing it did not have at all was `-f`. It also read input as
//! UTF-8 `String` lines, so a file that is not UTF-8 was truncated at the first
//! bad byte and `\r\n` came back out as `\n`; and it substituted 10 silently
//! for any count it could not parse, so `tail -n 5O file` (letter O) printed
//! ten lines rather than saying so.
//!
//! # Two option syntaxes, and the obsolete one is stricter than `head`'s
//!
//! `tail -3 file` is the pre-POSIX form, parsed by hand before `getopt_long`
//! ever runs. `head` recognises its own version of it whenever it is the first
//! argument; `tail` additionally requires that **the whole command line have
//! one of three shapes**, because the form is ambiguous with the modern one and
//! upstream will not guess:
//!
//! | Shape | Example |
//! |---|---|
//! | the option word alone | `tail -3` |
//! | the option word and one non-option | `tail -3 file`, `tail -3 -` |
//! | the option word, `--`, and at most one more | `tail -3 -- file` |
//!
//! Anything else — `tail -3 a b`, `tail -3 -q f`, `tail -q -3 f` — is refused,
//! and the digit that then reaches getopt produces `option used in invalid
//! context -- 3`. (`head` words the same situation `invalid trailing option --
//! 3`; the two utilities do not share the sentence.)
//!
//! Within the obsolete word the letters are **not** the modern flags. `b`, `c`
//! and `l` choose the unit, `f` means follow, and `b` is also a ×512
//! multiplier — but one applied in two different places depending on whether
//! digits were given, which is why `tail -b` is 5120 *bytes* while `tail -2b`
//! is 1024. Upstream scales its *default* by 512 in the first case, and in the
//! second hands the digits to `xstrtoumax` with `"b"` as the suffix list.
//!
//! # `--help` differs from GNU's on purpose
//!
//! GNU's help mentions `inotify` twice — that `--max-unchanged-stats` is
//! "rarely useful" with it, and that `--pid` is checked "at least once every N
//! seconds" with it. Both sentences are false here: this implementation always
//! polls, so `--max-unchanged-stats` is always in play and `--pid` is checked
//! exactly once per iteration. The clauses are dropped rather than copied.

use coreutils::errmsg::strerror;
use coreutils::filekind;
use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::{os_bytes, quote, quoteaf, quotef};
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::{File, Metadata};
use std::io::{self, ErrorKind, IsTerminal, Read, Seek, SeekFrom, Write};
use std::process::ExitCode;

/// Measured: `tail --zzz; echo $?` is 1.
const TAIL: Program = Program::new("tail", 1);

/// The count with no `-n`/`-c`, and the number the help text quotes.
const DEFAULT_NUMBER: u64 = 10;

/// `--max-unchanged-stats`'s default, named in the help text.
const DEFAULT_MAX_UNCHANGED: u64 = 5;

/// `--sleep-interval`'s default, in seconds. Also named in the help text.
const DEFAULT_SLEEP: f64 = 1.0;

/// The largest value `--pid` accepts: glibc's `PID_T_MAX`, `pid_t` being `int`.
const PID_MAX: u64 = i32::MAX as u64;

/// The long options in **GNU's declaration order**, which is observable: it is
/// the order `getopt_long` lists candidates in when an abbreviation is
/// ambiguous. Measured with `tail --=x`, an empty prefix that matches every
/// entry and so prints the whole table.
///
/// Two entries are worth stopping on. `follow` takes an **optional** argument,
/// the only one here that does — so `--follow` and `--follow=name` are both
/// legal but `--follow name` is not, `name` being an operand. And
/// `-disable-inotify` and `-presume-input-pipe` are not typos: upstream hides
/// an option by giving it a name that begins with a dash, so the spelling a
/// user must type carries three of them.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("bytes", Takes::Required),
    ("follow", Takes::Optional),
    ("lines", Takes::Required),
    ("max-unchanged-stats", Takes::Required),
    ("-disable-inotify", Takes::Nothing),
    ("pid", Takes::Required),
    ("-presume-input-pipe", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("retry", Takes::Nothing),
    ("silent", Takes::Nothing),
    ("sleep-interval", Takes::Required),
    ("verbose", Takes::Nothing),
    ("zero-terminated", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `--follow`'s argument, and the table `argmatch` resolves abbreviations of it
/// against — `--follow=d` is `descriptor`.
const FOLLOW_MODES: &[(&str, Follow)] =
    &[("descriptor", Follow::Descriptor), ("name", Follow::Name)];

/// Whether the count is in lines or in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Lines,
    Bytes,
}

impl Unit {
    /// The half of the diagnostic that names the unit —
    /// `invalid number of lines` against `invalid number of bytes`.
    fn invalid_number(self) -> &'static str {
        match self {
            Self::Lines => "invalid number of lines",
            Self::Bytes => "invalid number of bytes",
        }
    }
}

/// What `-f` follows: the file that was opened, or whatever the name refers to
/// from moment to moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Follow {
    /// The default. A renamed file is still followed; a new file created under
    /// the old name is not.
    Descriptor,
    /// `--follow=name`, and half of `-F`. Survives log rotation.
    Name,
}

/// When to print a `==> name <==` banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Headers {
    Never,
    /// The default: only when there is more than one operand.
    Multiple,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Options {
    unit: Unit,
    n_units: u64,
    /// `-n +5`: count from the start and skip, rather than count from the end.
    /// Note that this is **sticky** upstream — the flag is set by a `+` and
    /// never cleared by a later `-n` without one, so `tail -n +2 -n 2` skips
    /// rather than printing two lines. Faithfully reproduced.
    from_start: bool,
    /// `-f`: do not stop at end of file.
    forever: bool,
    follow: Follow,
    /// `--retry`: keep trying to open a file that is not there yet.
    retry: bool,
    /// `--pid`: stop once this process has.
    pid: Option<u64>,
    sleep_interval: f64,
    max_unchanged: u64,
    headers: Headers,
    /// `-z` makes this NUL. It is what a "line" ends with everywhere below,
    /// which is why it is carried rather than hard-coded.
    line_end: u8,
    /// `---presume-input-pipe`: take the streaming path even for input that
    /// could be seeked. Unlike in `head`, where the option does nothing, this
    /// one is load-bearing — the two paths are separate code, and this is how
    /// the slower one gets exercised against a seekable file.
    presume_input_pipe: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            unit: Unit::Lines,
            n_units: DEFAULT_NUMBER,
            from_start: false,
            forever: false,
            follow: Follow::Descriptor,
            retry: false,
            pid: None,
            sleep_interval: DEFAULT_SLEEP,
            max_unchanged: DEFAULT_MAX_UNCHANGED,
            headers: Headers::Multiple,
            line_end: b'\n',
            presume_input_pipe: false,
        }
    }
}

/// What the command line asked for.
#[derive(Debug, PartialEq)]
enum Request {
    Help,
    Version,
    Run(Options, Vec<OsString>),
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("tail (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(options, files)) => {
            warn_about_unused(&options);
            run(&options, &files)
        }
        Err(e) => {
            eprintln!("tail: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    format!(
        "\
Usage: tail [OPTION]... [FILE]...
Print the last {DEFAULT_NUMBER} lines of each FILE to standard output.
With more than one FILE, precede each with a header giving the file name.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -c, --bytes=[+]NUM       output the last NUM bytes; or use -c +NUM to
                             output starting with byte NUM of each file
  -f, --follow[={{name|descriptor}}]
                           output appended data as the file grows;
                             an absent option argument means 'descriptor'
  -F                       same as --follow=name --retry
  -n, --lines=[+]NUM       output the last NUM lines, instead of the last \
{DEFAULT_NUMBER};
                             or use -n +NUM to skip NUM-1 lines at the start
      --max-unchanged-stats=N
                           with --follow=name, reopen a FILE which has not
                             changed size after N (default {DEFAULT_MAX_UNCHANGED}) iterations
                             to see if it has been unlinked or renamed
                             (this is the usual case of rotated log files)
      --pid=PID            with -f, terminate after process ID, PID dies
  -q, --quiet, --silent    never output headers giving file names
      --retry              keep trying to open a file if it is inaccessible
  -s, --sleep-interval=N   with -f, sleep for approximately N seconds
                             (default {DEFAULT_SLEEP:.1}) between iterations
  -v, --verbose            always output headers giving file names
  -z, --zero-terminated    line delimiter is NUL, not newline
      --help        display this help and exit
      --version     output version information and exit

NUM may have a multiplier suffix:
b 512, kB 1000, K 1024, MB 1000*1000, M 1024*1024,
GB 1000*1000*1000, G 1024*1024*1024, and so on for T, P, E, Z, Y, R, Q.
Binary prefixes can be used, too: KiB=K, MiB=M, and so on.

With --follow (-f), tail defaults to following the file descriptor, which
means that even if a tail'ed file is renamed, tail will continue to track
its end.  This default behavior is not desirable when you really want to
track the actual name of the file, not the file descriptor (e.g., log
rotation).  Use --follow=name in that case.  That causes tail to track the
named file in a way that accommodates renaming, removal and creation.
"
    )
}

// ---------------------------------------------------------------- parsing ---

/// Parse argv: the obsolete `-NUM`/`+NUM` form first, if the command line has
/// one of the three shapes that form is allowed to take, and then
/// `getopt_long`.
///
/// # Errors
///
/// Any getopt diagnostic, plus `tail`'s own: a count, PID, iteration limit or
/// sleep interval that is not a number, and a digit reaching getopt (which
/// means an obsolete form that was not in one of the three shapes).
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut options = Options::default();
    let mut files: Vec<OsString> = Vec::new();
    let mut only_operands = false;
    let mut i = 0usize;

    if parse_obsolete(args, &mut options)? {
        i = 1;
    }

    while let Some(arg) = args.get(i) {
        i = i.saturating_add(1);
        if only_operands {
            files.push(arg.clone());
            continue;
        }
        let bytes = arg_bytes(arg);

        if bytes == b"--" {
            only_operands = true;
        } else if bytes == b"-" || bytes.first() != Some(&b'-') {
            // A lone `-` is standard input, which is an operand, not an option.
            files.push(arg.clone());
        } else if bytes.starts_with(b"--") {
            if let Some(request) = long_option(&bytes, args, &mut i, &mut options)? {
                return Ok(request);
            }
        } else {
            short_options(&bytes, args, &mut i, &mut options)?;
        }
    }

    Ok(Request::Run(options, files))
}

/// The obsolete `-NUM[bcl][f]` / `+NUM[bcl][f]` word, returning whether it was
/// there and consumed.
///
/// The shape test comes first and is upstream's verbatim, argument counts and
/// all: the form is only recognised on a command line that could not be
/// anything else. Note that it is a test on the *whole* command line, so
/// whether `-2` is a count or an error depends on what follows it two
/// arguments later.
///
/// # Errors
///
/// Only one: digits that overflow, or whose `b` suffix overflows. A letter that
/// does not belong is not an error here — it makes the word not an obsolete
/// option, and it is then getopt's problem.
fn parse_obsolete(args: &[OsString], options: &mut Options) -> Result<bool, getopt::Error> {
    let Some(first) = args.first() else {
        return Ok(false);
    };
    if !obsolete_shape(args) {
        return Ok(false);
    }
    let whole = arg_bytes(first);
    let mut at = 1usize;
    let from_start = match whole.first() {
        Some(b'+') => true,
        Some(b'-') => {
            // Upstream: `if (!obsolete_usage && !p[p[0] == 'c']) return false;`
            // — under a modern `_POSIX2_VERSION`, a bare `-` is standard input
            // and a bare `-c` is an option needing an argument, so both must
            // fall through to getopt. Everything else starting with a dash is a
            // candidate, including `-f` and `-b`, which have no digits at all.
            let body = whole.get(1..).unwrap_or_default();
            let probe = usize::from(body.first() == Some(&b'c'));
            if body.get(probe).is_none() {
                return Ok(false);
            }
            false
        }
        _ => return Ok(false),
    };

    let digits_from = at;
    while whole.get(at).is_some_and(u8::is_ascii_digit) {
        at = at.saturating_add(1);
    }
    let has_digits = at > digits_from;

    // The unit letter, which is also where the ×512 multiplier lives.
    let mut default_count = DEFAULT_NUMBER;
    let mut unit = Unit::Lines;
    match whole.get(at) {
        Some(b'b') => {
            default_count = default_count.saturating_mul(512);
            unit = Unit::Bytes;
            at = at.saturating_add(1);
        }
        Some(b'c') => {
            unit = Unit::Bytes;
            at = at.saturating_add(1);
        }
        Some(b'l') => at = at.saturating_add(1),
        _ => {}
    }

    let forever = whole.get(at) == Some(&b'f');
    if forever {
        at = at.saturating_add(1);
    }
    if at != whole.len() {
        // Trailing junk: not this form after all. `tail -2x f` ends up here and
        // is then refused by getopt for the digit, not for the `x`.
        return Ok(false);
    }

    options.n_units = if has_digits {
        // The digits *and everything after them* go to `xstrtoumax` with `b` as
        // the only suffix, which is how `-2b` becomes 1024 rather than 2. A
        // suffix character it does not know is masked off and ignored — that is
        // the `l` in `-2lf` — but an overflow is not.
        obsolete_number(whole.get(digits_from..).unwrap_or_default(), &whole)?
    } else {
        default_count
    };
    options.unit = unit;
    options.from_start = from_start;
    options.forever = forever;
    Ok(true)
}

/// Upstream's three-shape test, which decides whether the obsolete form is
/// looked for at all.
///
/// `args` here is `argv[1..]`, so upstream's `argc` is one more than its
/// length.
fn obsolete_shape(args: &[OsString]) -> bool {
    match args.len() {
        1 => true,
        // `! (argv[2][0] == '-' && argv[2][1])`: a second word may be an
        // operand or a lone `-`, but not an option.
        2 => args.get(1).is_some_and(|a| {
            let b = arg_bytes(a);
            !(b.first() == Some(&b'-') && b.len() > 1)
        }),
        3 => args.get(1).is_some_and(|a| arg_bytes(a) == b"--"),
        _ => false,
    }
}

/// `xstrtoumax(digits, nullptr, 10, &n, "b")` with `LONGINT_INVALID_SUFFIX_CHAR`
/// masked off — the obsolete form's number, which is a different parser from
/// the modern `-n`'s.
///
/// The differences that matter: only `b` is a suffix (and it means 512), and a
/// character that is not one is ignored rather than rejected. What is *not*
/// ignored is an overflow, which is reported against the whole original word.
fn obsolete_number(text: &[u8], whole: &[u8]) -> Result<u64, getopt::Error> {
    let mut value: u64 = 0;
    let mut at = 0usize;
    let mut overflowed = false;
    while let Some(d) = text.get(at).filter(|c| c.is_ascii_digit()) {
        at = at.saturating_add(1);
        let digit = u64::from(d.wrapping_sub(b'0'));
        match value.checked_mul(10).and_then(|v| v.checked_add(digit)) {
            Some(v) => value = v,
            None => overflowed = true,
        }
    }
    if overflowed {
        value = u64::MAX;
    }
    if text.get(at) == Some(&b'b') {
        match value.checked_mul(512) {
            Some(v) => value = v,
            None => overflowed = true,
        }
    }
    if overflowed {
        // Upstream passes `errno` — ERANGE, set by `strtoumax` — to `error`, so
        // this one sentence ends in `strerror(ERANGE)` rather than in the fixed
        // wording `xdectoumax` uses, and it quotes the *whole* word including
        // its leading sign.
        return Err(TAIL.usage(format!(
            "invalid number: {}: Numerical result out of range",
            quote(whole)
        )));
    }
    Ok(value)
}

/// One `-abc` cluster.
fn short_options(
    bytes: &[u8],
    args: &[OsString],
    i: &mut usize,
    options: &mut Options,
) -> Result<(), getopt::Error> {
    let body = bytes.get(1..).unwrap_or_default();
    let mut at = 0usize;
    // Bytes, not `char`s: `-é` is two bytes, and iterating `char`s would report
    // `invalid option -- 'é'`, an option nobody typed.
    while let Some(&c) = body.get(at) {
        at = at.saturating_add(1);
        match c {
            b'q' => options.headers = Headers::Never,
            b'v' => options.headers = Headers::Always,
            b'z' => options.line_end = 0,
            b'f' => options.forever = true,
            b'F' => {
                options.forever = true;
                options.follow = Follow::Name;
                options.retry = true;
            }
            b'c' | b'n' | b's' => {
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
                            .ok_or_else(|| TAIL.short_missing_argument(c))?
                            .clone();
                        *i = i.saturating_add(1);
                        arg_bytes(&next)
                    }
                };
                if c == b's' {
                    options.sleep_interval = parse_seconds(&value)?;
                } else {
                    let unit = if c == b'c' { Unit::Bytes } else { Unit::Lines };
                    set_count(unit, &value, options)?;
                }
            }
            // A digit only reaches here when the obsolete form was not in one
            // of its three shapes — the form itself is handled before this loop
            // ever runs. Note the wording: `head` says `invalid trailing
            // option` for the same situation, and neither sentence carries the
            // `Try 'tail --help'` referral.
            b'0'..=b'9' => {
                return Err(TAIL.usage(format!(
                    "option used in invalid context -- {}",
                    char::from(c)
                )));
            }
            _ => return Err(TAIL.invalid_option(c)),
        }
    }
    Ok(())
}

/// One `--name` argument. Returns `Some` when the option ends parsing —
/// `--help` and `--version` — and `None` when it only set something.
fn long_option(
    bytes: &[u8],
    args: &[OsString],
    i: &mut usize,
    options: &mut Options,
) -> Result<Option<Request>, getopt::Error> {
    let body = bytes.get(2..).unwrap_or_default();
    // `--name=value`: split before resolving, so the name is what gets matched
    // and the whole argument is what gets echoed back when it resolves to
    // nothing.
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(at) => (
            body.get(..at).unwrap_or_default(),
            body.get(at.saturating_add(1)..),
        ),
        None => (body, None),
    };
    // Every option is ASCII, so a name that is not UTF-8 matches none of them;
    // it takes the unrecognised path rather than erroring differently.
    let typed = std::str::from_utf8(typed).map_err(|_| TAIL.unrecognized_option(bytes))?;
    let (name, takes) = TAIL.resolve_long(typed, bytes, LONG_OPTIONS)?;

    if takes == Takes::Nothing && inline.is_some() {
        return Err(TAIL.long_unwanted_argument(name));
    }
    let value: Option<Vec<u8>> = match (takes, inline) {
        (_, Some(v)) => Some(v.to_vec()),
        (Takes::Required, None) => {
            let next = args
                .get(*i)
                .ok_or_else(|| TAIL.long_missing_argument(name))?
                .clone();
            *i = i.saturating_add(1);
            Some(arg_bytes(&next))
        }
        // `Takes::Optional` never consumes the next argument: `--follow name`
        // is follow-by-descriptor of a file called `name`.
        (_, None) => None,
    };

    match name {
        "bytes" => set_count(Unit::Bytes, &value.unwrap_or_default(), options)?,
        "lines" => set_count(Unit::Lines, &value.unwrap_or_default(), options)?,
        "follow" => {
            options.forever = true;
            options.follow = match value {
                None => Follow::Descriptor,
                Some(v) => TAIL.argmatch(&v, "--follow", FOLLOW_MODES)?,
            };
        }
        "max-unchanged-stats" => {
            options.max_unchanged = parse_uint(
                &value.unwrap_or_default(),
                u64::MAX,
                "invalid maximum number of unchanged stats between opens",
            )?;
        }
        "pid" => {
            options.pid = Some(parse_uint(
                &value.unwrap_or_default(),
                PID_MAX,
                "invalid PID",
            )?);
        }
        "sleep-interval" => options.sleep_interval = parse_seconds(&value.unwrap_or_default())?,
        "retry" => options.retry = true,
        "quiet" | "silent" => options.headers = Headers::Never,
        "verbose" => options.headers = Headers::Always,
        "zero-terminated" => options.line_end = 0,
        // Upstream's two escape hatches for testing its own fast paths. We have
        // no inotify at all, so the first is already true and disabling it is a
        // no-op; the second forces the streaming reader over the seeking one,
        // which here is a real choice and is honoured.
        "-disable-inotify" => {}
        "-presume-input-pipe" => options.presume_input_pipe = true,
        "help" => return Ok(Some(Request::Help)),
        "version" => return Ok(Some(Request::Version)),
        // `resolve_long` returns only names from the table, all of which are
        // above.
        _ => {}
    }
    Ok(None)
}

/// Apply a `-n`/`-c` value, splitting off the leading sign.
///
/// `+` means "from the start", and is left on the string for the number parser
/// (which accepts it, as `strtoumax` does). `-` means the default, from the
/// end, and is taken *off* — which is why `tail -n -x` reports `'x'` while
/// `tail -n +x` reports `'+x'`.
///
/// Nothing here ever clears `from_start`. That is upstream's, and it is
/// observable: `tail -n +2 -n 2` skips the first line rather than printing the
/// last two.
fn set_count(unit: Unit, value: &[u8], options: &mut Options) -> Result<(), getopt::Error> {
    let text = match value.first() {
        Some(b'+') => {
            options.from_start = true;
            value
        }
        Some(b'-') => value.get(1..).unwrap_or_default(),
        _ => value,
    };
    options.unit = unit;
    options.n_units = parse_count(text, unit)?;
    Ok(())
}

/// The multiplier suffixes `tail` accepts, and the power of the base each
/// stands for.
///
/// This is upstream's `"bkKmMGTPEZYRQ0"` less the `0`, which is not a suffix at
/// all: it is gnulib's flag asking for the second-suffix base switch that
/// [`parse_count`] implements below.
const SUFFIXES: &[(u8, u32)] = &[
    (b'b', 0), // 512 exactly, handled below rather than as a power
    (b'k', 1),
    (b'K', 1),
    (b'm', 2),
    (b'M', 2),
    (b'G', 3),
    (b'T', 4),
    (b'P', 5),
    (b'E', 6),
    (b'Z', 7),
    (b'Y', 8),
    (b'R', 9),
    (b'Q', 10),
];

/// gnulib's `xdectoumax` as `tail` calls it for `-n`/`-c`: a decimal count with
/// an optional multiplier suffix. Identical to `head`'s, which is no
/// coincidence — both call the same function with the same suffix list.
///
/// The rules that are not guessable, all measured against glibc:
///
/// - **Leading whitespace and a leading `+` are accepted** (`strtoumax` skips
///   them), but trailing whitespace is not.
/// - **A bare suffix means one of it.** `tail -n K` is 1024 lines, because when
///   `strtoumax` consumes nothing gnulib substitutes 1 — but only if the very
///   first byte is itself a valid suffix, so `tail -n " K"` is still an error.
/// - **A second suffix changes the base.** `B` or `D` after the first make it a
///   power of 1000; `iB` keeps 1024.
/// - **A bad suffix outranks an overflow**, so the suffix must be validated
///   before the magnitude is reported.
///
/// # Errors
///
/// A number that does not parse, or one that overflows `u64`.
fn parse_count(text: &[u8], unit: Unit) -> Result<u64, getopt::Error> {
    // `quote`, not `quoteaf`: gnulib's `xdectoumax` echoes the offending text
    // with `quote()`, whose escaping is C's rather than the shell's.
    let invalid = || TAIL.usage(format!("{}: {}", unit.invalid_number(), quote(text)));
    let overflow = || {
        TAIL.usage(format!(
            "{}: {}: Value too large for defined data type",
            unit.invalid_number(),
            quote(text)
        ))
    };

    let (mut value, mut at, mut overflowed) = scan_uint(text, true).ok_or_else(invalid)?;

    // The suffix, if any.
    if let Some(&first) = text.get(at) {
        let Some(power) = suffix_power(first) else {
            return Err(invalid());
        };
        at = at.saturating_add(1);
        let base = match (text.get(at), text.get(at.saturating_add(1))) {
            // `iB` — explicitly binary, and the only use of a lone `i`.
            (Some(b'i'), Some(b'B')) => {
                at = at.saturating_add(2);
                1024u64
            }
            // `B` and the obsolescent `D` — decimal.
            (Some(b'B' | b'D'), _) => {
                at = at.saturating_add(1);
                1000u64
            }
            _ => 1024u64,
        };
        if at != text.len() {
            return Err(invalid());
        }
        let factor = if first == b'b' {
            // `b` is 512 flat and takes no second suffix — it is not in the
            // base-switching group at all.
            512u64
        } else {
            base.checked_pow(power).ok_or_else(overflow)?
        };
        match value.checked_mul(factor) {
            Some(v) => value = v,
            None => overflowed = true,
        }
    } else if at != text.len() {
        return Err(invalid());
    }
    if overflowed {
        return Err(overflow());
    }
    Ok(value)
}

/// `xdectoumax` with an **empty** suffix list and a ceiling — how `--pid` and
/// `--max-unchanged-stats` are parsed.
///
/// No suffix is accepted at all (`--pid=5k` is an error), and a value above
/// `max` reports the overflow wording rather than the invalid one, which is how
/// `--pid=99999999999999999999` and `--pid=x` come out differently.
///
/// # Errors
///
/// A number that does not parse, or one above `max`.
fn parse_uint(text: &[u8], max: u64, what: &str) -> Result<u64, getopt::Error> {
    let invalid = || TAIL.usage(format!("{what}: {}", quote(text)));
    let overflow = || {
        TAIL.usage(format!(
            "{what}: {}: Value too large for defined data type",
            quote(text)
        ))
    };

    // `false`: with no valid suffixes there is no bare-suffix fallback, so a
    // string with no digits in it is simply invalid.
    let (value, at, overflowed) = scan_uint(text, false).ok_or_else(invalid)?;
    if at != text.len() {
        return Err(invalid());
    }
    if overflowed || value > max {
        return Err(overflow());
    }
    Ok(value)
}

/// The `strtoumax` half of both parsers: optional whitespace, optional `+`,
/// then digits.
///
/// Returns the value, how far it got, and whether it overflowed — or `None`
/// when nothing at all was consumed and no fallback applies. `bare_suffix_ok`
/// is gnulib's rule that an argument which is *entirely* a suffix means one of
/// them; it looks at `text[0]`, before the whitespace was skipped, which is why
/// `" K"` does not qualify.
fn scan_uint(text: &[u8], bare_suffix_ok: bool) -> Option<(u64, usize, bool)> {
    let mut at = 0usize;
    while text.get(at).is_some_and(u8::is_ascii_whitespace) {
        at = at.saturating_add(1);
    }
    if text.get(at) == Some(&b'+') {
        at = at.saturating_add(1);
    }
    let digits_from = at;
    let mut value: u64 = 0;
    let mut overflowed = false;
    while let Some(d) = text.get(at).filter(|c| c.is_ascii_digit()) {
        at = at.saturating_add(1);
        let digit = u64::from(d.wrapping_sub(b'0'));
        match value.checked_mul(10).and_then(|v| v.checked_add(digit)) {
            Some(v) => value = v,
            // `strtoumax` keeps consuming digits after ERANGE and returns
            // UINTMAX_MAX; whatever follows still has to be valid.
            None => overflowed = true,
        }
    }
    if at == digits_from {
        if !(bare_suffix_ok && text.first().is_some_and(|c| suffix_power(*c).is_some())) {
            return None;
        }
        at = 0;
        value = 1;
    }
    if overflowed {
        value = u64::MAX;
    }
    Some((value, at, overflowed))
}

fn suffix_power(c: u8) -> Option<u32> {
    SUFFIXES.iter().find(|(s, _)| *s == c).map(|(_, p)| *p)
}

/// `-s`'s value: gnulib's `xstrtod` with `strtod`, plus upstream's `0 <= s`.
///
/// `strtod`, not Rust's `f64::from_str`: leading whitespace is skipped, and a
/// hexadecimal float is a number. `nan` parses and is then rejected by the
/// range test, every comparison against it being false — which is upstream's
/// behaviour rather than a special case anyone wrote.
///
/// # Errors
///
/// Anything `strtod` would not consume entirely, and anything negative or NaN.
fn parse_seconds(text: &[u8]) -> Result<f64, getopt::Error> {
    let bad = || TAIL.usage(format!("invalid number of seconds: {}", quote(text)));
    let trimmed = {
        let mut at = 0usize;
        while text.get(at).is_some_and(u8::is_ascii_whitespace) {
            at = at.saturating_add(1);
        }
        text.get(at..).unwrap_or_default()
    };
    let body = std::str::from_utf8(trimmed).map_err(|_| bad())?;
    let value = match parse_hex_float(body) {
        Some(v) => v,
        None => body.parse::<f64>().map_err(|_| bad())?,
    };
    if value >= 0.0 { Ok(value) } else { Err(bad()) }
}

/// C99 hexadecimal floating point — `0x1.8p3` is 12 — which `strtod` accepts
/// and Rust's parser does not.
///
/// Returns `None` when the text is not in that form, leaving the decimal parser
/// to have its say; the two are disjoint because only this one starts `0x`.
fn parse_hex_float(text: &str) -> Option<f64> {
    let (sign, rest) = match text.as_bytes().first() {
        Some(b'-') => (-1.0f64, text.get(1..)?),
        Some(b'+') => (1.0f64, text.get(1..)?),
        _ => (1.0f64, text),
    };
    let digits = rest
        .strip_prefix("0x")
        .or_else(|| rest.strip_prefix("0X"))?;
    let (mantissa, exponent) = match digits.find(['p', 'P']) {
        Some(at) => (digits.get(..at)?, Some(digits.get(at.saturating_add(1)..)?)),
        None => (digits, None),
    };
    let (whole, fraction) = match mantissa.find('.') {
        Some(at) => (
            mantissa.get(..at)?,
            mantissa.get(at.saturating_add(1)..)?,
        ),
        None => (mantissa, ""),
    };
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    let mut value = 0.0f64;
    for c in whole.chars() {
        value = value * 16.0 + f64::from(c.to_digit(16)?);
    }
    let mut scale = 1.0f64 / 16.0;
    for c in fraction.chars() {
        value += f64::from(c.to_digit(16)?) * scale;
        scale /= 16.0;
    }
    // The exponent is a *decimal* power of two, and is mandatory in C99 but
    // optional in glibc's `strtod`, which is what is being matched.
    if let Some(text) = exponent {
        let exponent: i32 = text.parse().ok()?;
        value *= 2.0f64.powi(exponent);
    }
    Some(sign * value)
}

/// The three warnings upstream prints after parsing, when an option was given
/// that the rest of the command line makes pointless.
///
/// They go to standard error, they stop nothing, and they do not change the
/// exit status. They are also *not* printed for `--help`/`--version`, which
/// return before this runs — hence the call site in `main` rather than at the
/// end of [`parse_args`].
fn warn_about_unused(options: &Options) {
    if options.retry {
        if !options.forever {
            eprintln!("tail: warning: --retry ignored; --retry is useful only when following");
        } else if options.follow == Follow::Descriptor {
            eprintln!("tail: warning: --retry only effective for the initial open");
        }
    }
    if options.pid.is_some() && !options.forever {
        eprintln!("tail: warning: PID ignored; --pid=PID is useful only when following");
    }
}

// --------------------------------------------------------------- printing ---

/// How much is read at a time, and the block size the backwards scan walks the
/// file in. Only a performance choice — every routine below is correct for any
/// chunking, including a pipe that dribbles one byte at a time.
const CHUNK: usize = 64 * 1024;

/// Why a name is not currently being read, in the three grades upstream's
/// `f->errnum` distinguishes.
///
/// It is compared against the previous iteration's value to decide whether a
/// diagnostic would be a repeat, so `Io` carries the kind rather than the
/// message: two failures of the same kind are the same failure as far as the
/// follow loop is concerned, which is the granularity `errno` has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trouble {
    /// `errnum == 0` — the file is open and well.
    None,
    /// `errnum == -1` — something wrong that is not an `errno`, which here
    /// means a file whose type cannot be followed.
    NotAnErrno,
    Io(io::ErrorKind),
}

/// One operand, plus everything the follow loop has to remember between
/// iterations.
struct Watched {
    /// The operand as typed, which is what gets reopened.
    name: OsString,
    /// What diagnostics and banners call it: `standard input` for `-`.
    label: Vec<u8>,
    is_stdin: bool,
    /// `None` once the file has been closed or could never be opened.
    file: Option<File>,
    trouble: Trouble,
    /// Set when there is no point looking at this name again.
    ignore: bool,
    /// Whether the last look found something that could be followed. Only used
    /// to decide whether a *change* is worth reporting.
    tailable: bool,
    /// How much of the file has been printed, and what its last look was, so
    /// that growth, truncation and replacement can be told apart.
    size: u64,
    modified: Option<std::time::SystemTime>,
    regular: bool,
    id: Option<FileId>,
    /// Consecutive iterations in which nothing about the file changed. Only
    /// `--follow=name` uses it, to decide when to look at the *name* again.
    unchanged: u64,
}

impl Watched {
    fn new(name: &OsString) -> Self {
        let bytes = arg_bytes(name);
        let is_stdin = bytes == b"-";
        Watched {
            name: name.clone(),
            label: if is_stdin {
                b"standard input".to_vec()
            } else {
                bytes
            },
            is_stdin,
            file: None,
            trouble: Trouble::None,
            ignore: false,
            tailable: true,
            size: 0,
            modified: None,
            regular: false,
            id: None,
            unchanged: 0,
        }
    }
}

/// Run over every operand, returning the exit status.
fn run(options: &Options, files: &[OsString]) -> ExitCode {
    let mut options = *options;
    // "To start printing with item N_UNITS from the start of the file, skip
    // N_UNITS - 1 items." `+0` and `+1` therefore mean the same thing, which is
    // upstream's stated concession to Unix compatibility.
    if options.from_start {
        options.n_units = options.n_units.saturating_sub(1);
    }

    let default = [OsString::from("-")];
    let operands: &[OsString] = if files.is_empty() { &default } else { files };
    let has_stdin = operands.iter().any(|f| arg_bytes(f) == b"-");

    // A name is what `--follow=name` follows, and standard input has none.
    if has_stdin && options.follow == Follow::Name {
        eprintln!("tail: cannot follow {} by name", quoteaf(b"-"));
        return ExitCode::from(1);
    }
    if options.forever && has_stdin {
        // Upstream's condition, which is subtler than it looks: the warning is
        // for the case where the loop would poll `fstat` on a terminal and
        // never see it change. When there is exactly one file, no `--pid` and
        // no `--follow=name`, the read itself blocks and does the waiting, so
        // there is nothing to warn about.
        let blocking = options.pid.is_none()
            && options.follow == Follow::Descriptor
            && operands.len() == 1
            && !stdin_is_regular();
        if !blocking && io::stdin().is_terminal() {
            eprintln!("tail: warning: following standard input indefinitely is ineffective");
        }
    }

    // Nothing will ever be printed, so nothing is opened and no banner appears
    // — `tail -v -n0 file` prints nothing at all, not even the header.
    if options.n_units == 0 && !options.forever && !options.from_start {
        return ExitCode::SUCCESS;
    }

    let print_headers = match options.headers {
        Headers::Never => false,
        Headers::Always => true,
        Headers::Multiple => operands.len() > 1,
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut watched: Vec<Watched> = operands.iter().map(Watched::new).collect();
    let mut ok = true;
    let mut first_header = true;

    for w in &mut watched {
        ok &= start_file(w, &mut out, &options, print_headers, &mut first_header);
    }

    if options.forever {
        follow(&mut watched, &mut out, &options, print_headers, &mut first_header);
    }

    if out.flush().is_err() {
        return ExitCode::from(1);
    }
    if ok { ExitCode::SUCCESS } else { ExitCode::from(1) }
}

/// Open one operand and print the part of it that was asked for — upstream's
/// `tail_file`.
///
/// Returns whether it succeeded. A failure is not fatal: the remaining operands
/// are still processed, and with `-f` the name may still be worth watching.
fn start_file(
    w: &mut Watched,
    out: &mut impl Write,
    options: &Options,
    print_headers: bool,
    first_header: &mut bool,
) -> bool {
    let opened = if w.is_stdin {
        Ok(stdin_as_file())
    } else {
        File::open(&w.name)
    };
    let mut file = match opened {
        Ok(f) => f,
        Err(e) => {
            // `--retry` is what makes an unopenable name worth keeping.
            w.tailable = !options.retry;
            if options.forever {
                w.trouble = Trouble::Io(e.kind());
                w.ignore = !options.retry;
            }
            eprintln!(
                "tail: cannot open {} for reading: {}",
                quoteaf(&w.label),
                strerror(&e)
            );
            return false;
        }
    };

    if print_headers {
        write_header(out, &w.label, first_header);
    }
    let mut ok = match emit(&mut file, out, options) {
        Ok(()) => true,
        Err(e) if e.kind() == ErrorKind::BrokenPipe => {
            // Nothing downstream is listening. Upstream notices this through
            // `check_output_alive`; either way there is nothing to report.
            w.file = None;
            return true;
        }
        Err(e) => {
            eprintln!("tail: error reading {}: {}", quoteaf(&w.label), strerror(&e));
            false
        }
    };

    if options.forever {
        match file.metadata() {
            Ok(m) => {
                if tailable(&m) {
                    w.trouble = if ok { Trouble::None } else { Trouble::NotAnErrno };
                    remember(w, &m, &mut file);
                    w.file = Some(file);
                    return ok;
                }
                ok = false;
                w.trouble = Trouble::NotAnErrno;
                w.tailable = false;
                w.ignore = !options.retry;
                eprintln!(
                    "tail: {}: cannot follow end of this type of file{}",
                    quotef(&w.label),
                    if w.ignore {
                        "; giving up on this name"
                    } else {
                        ""
                    }
                );
            }
            Err(e) => {
                ok = false;
                w.trouble = Trouble::Io(e.kind());
                eprintln!("tail: error reading {}: {}", quoteaf(&w.label), strerror(&e));
            }
        }
    }
    w.file = None;
    ok
}

/// Note the file's identity and length, so that the next look can tell growth
/// from truncation from replacement.
fn remember(w: &mut Watched, m: &Metadata, file: &mut File) {
    // Not `m.is_file()`: the follow loop compares sizes only for a regular
    // file, and on the host build a pipe would claim to be one and report its
    // buffer contents as a length. See `filekind`.
    w.regular = filekind::is_regular(file);
    w.modified = m.modified().ok();
    w.id = file_id(m);
    // Where reading actually stopped, which is not the same as the length: the
    // file may have grown between the last read and this `stat`, and those
    // bytes must not be skipped.
    w.size = file.stream_position().unwrap_or(m.len());
}

/// `==> name <==`, with a blank line before every one but the first.
///
/// The flag counts banners *printed*, not files seen: a file that fails to open
/// never gets one, so the next file that succeeds is still the first and gets
/// no leading blank line.
fn write_header(out: &mut impl Write, label: &[u8], first: &mut bool) {
    let sep: &[u8] = if *first { b"" } else { b"\n" };
    let _ = out.write_all(sep);
    let _ = out.write_all(b"==> ");
    let _ = out.write_all(label);
    let _ = out.write_all(b" <==\n");
    *first = false;
}

/// Print the requested part of `file`, leaving the read position at the end of
/// what was printed so that `-f` can carry on from there.
fn emit(file: &mut File, out: &mut impl Write, options: &Options) -> io::Result<()> {
    // Whether the seeking paths are available. A pipe is not seekable, and
    // `---presume-input-pipe` pretends nothing is.
    //
    // `filekind::is_seekable`, not `metadata().is_file()`: on the harness's
    // Windows build an MSYS pipe answers yes to the latter, reports the number
    // of bytes buffered in it as a length, and accepts a seek that moves
    // nothing — which sent `printf 'a\nb\nc\n' | tail -n3` down the backwards
    // block scan, where it read the pipe dry looking for a fourth line and then
    // printed nothing at all. See `filekind`'s module documentation.
    let seekable = !options.presume_input_pipe && filekind::is_seekable(file);

    match (options.unit, options.from_start, seekable) {
        (Unit::Bytes, true, true) => {
            let at = file.stream_position()?;
            // Seeking past the end is legal and leaves nothing to print, which
            // is what skipping more bytes than the file holds should do.
            file.seek(SeekFrom::Start(at.saturating_add(options.n_units)))?;
            dump(file, out)
        }
        (Unit::Bytes, true, false) => skip_bytes(file, out, options.n_units),
        (Unit::Lines, true, _) => skip_lines(file, out, options.n_units, options.line_end),
        (Unit::Bytes, false, true) => last_bytes_seek(file, out, options.n_units),
        (Unit::Bytes, false, false) => last_bytes_stream(file, out, options.n_units),
        (Unit::Lines, false, true) => last_lines_seek(file, out, options.n_units, options.line_end),
        (Unit::Lines, false, false) => {
            last_lines_stream(file, out, options.n_units, options.line_end)
        }
    }
}

/// Everything from the current position to end of file.
fn dump(source: &mut impl Read, out: &mut impl Write) -> io::Result<()> {
    io::copy(source, out).map(|_| ())
}

/// Skip `n` bytes of a stream that cannot be seeked.
fn skip_bytes(source: &mut impl Read, out: &mut impl Write, n: u64) -> io::Result<()> {
    let mut left = n;
    let mut buf = vec![0u8; CHUNK];
    while left > 0 {
        let want = usize::try_from(left.min(CHUNK as u64)).unwrap_or(CHUNK);
        let got = source.read(buf.get_mut(..want).unwrap_or_default())?;
        if got == 0 {
            // End of input before the skip finished: there is nothing to print
            // and that is not an error.
            return Ok(());
        }
        left = left.saturating_sub(got as u64);
    }
    dump(source, out)
}

/// Everything from the `n`th line terminator onwards — `tail -n +N`, after the
/// `N - 1` adjustment.
///
/// Works on any input, seekable or not: unlike counting from the end, counting
/// from the start needs no lookahead.
fn skip_lines(source: &mut impl Read, out: &mut impl Write, n: u64, line_end: u8) -> io::Result<()> {
    let mut left = n;
    let mut buf = vec![0u8; CHUNK];
    while left > 0 {
        let got = source.read(&mut buf)?;
        if got == 0 {
            return Ok(());
        }
        let chunk = buf.get(..got).unwrap_or_default();
        let mut at = 0usize;
        while left > 0 {
            match chunk.get(at..).and_then(|t| t.iter().position(|&b| b == line_end)) {
                Some(rel) => {
                    at = at.saturating_add(rel).saturating_add(1);
                    left = left.saturating_sub(1);
                }
                None => break,
            }
        }
        if left == 0 {
            out.write_all(chunk.get(at..).unwrap_or_default())?;
        }
    }
    dump(source, out)
}

/// The last `n` bytes of a seekable file.
fn last_bytes_seek(file: &mut File, out: &mut impl Write, n: u64) -> io::Result<()> {
    let start = file.stream_position()?;
    let end = file.seek(SeekFrom::End(0))?;
    let from = end.saturating_sub(n).max(start);
    file.seek(SeekFrom::Start(from))?;
    dump(file, out)
}

/// The last `n` bytes of a stream that cannot be seeked.
///
/// Held bytes never exceed `n`: a byte is dropped as soon as `n` later ones
/// exist to displace it, so this costs the size of the answer and not the size
/// of the input.
fn last_bytes_stream(source: &mut impl Read, out: &mut impl Write, n: u64) -> io::Result<()> {
    let Ok(keep) = usize::try_from(n) else {
        // More bytes than this machine can address: the answer is the whole
        // input, whatever its length.
        return dump(source, out);
    };
    if keep == 0 {
        return drain(source);
    }
    let mut held: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let got = source.read(&mut buf)?;
        if got == 0 {
            return out.write_all(&held);
        }
        held.extend_from_slice(buf.get(..got).unwrap_or_default());
        if let Some(drop) = held.len().checked_sub(keep) {
            held.drain(..drop);
        }
    }
}

/// The last `n` lines of a seekable file — upstream's `file_lines`, which walks
/// the file backwards a block at a time rather than reading it forwards.
///
/// The one rule that is not obvious: **a final line with no terminator counts
/// as one of the `n`**, which is why the last block is examined for that before
/// the scan begins. `printf 'a\nb' | tail -n1` is `b`, not `a\nb`.
fn last_lines_seek(file: &mut File, out: &mut impl Write, n: u64, line_end: u8) -> io::Result<()> {
    let start = file.stream_position()?;
    let end = file.seek(SeekFrom::End(0))?;
    if end <= start {
        file.seek(SeekFrom::Start(start))?;
        return Ok(());
    }

    let mut want = n;
    let mut pos = end;
    let mut buf = vec![0u8; CHUNK];
    let mut last_block = true;
    while pos > start {
        // The first block read is the *ragged* one, so that every subsequent
        // seek is block-aligned.
        let span = match pos.saturating_sub(start) % (CHUNK as u64) {
            0 => CHUNK as u64,
            rest => rest,
        };
        pos = pos.saturating_sub(span);
        let span = usize::try_from(span).unwrap_or(CHUNK);
        file.seek(SeekFrom::Start(pos))?;
        let block = buf.get_mut(..span).unwrap_or_default();
        file.read_exact(block)?;

        if last_block {
            last_block = false;
            if block.last().is_some_and(|&b| b != line_end) {
                want = want.saturating_sub(1);
            }
        }

        let mut scan = span;
        while let Some(at) = block
            .get(..scan)
            .and_then(|t| t.iter().rposition(|&b| b == line_end))
        {
            scan = at;
            if want == 0 {
                // Everything after this terminator, then the rest of the file —
                // the read position is already at the end of this block.
                out.write_all(block.get(at.saturating_add(1)..).unwrap_or_default())?;
                return dump(file, out);
            }
            want = want.saturating_sub(1);
        }
    }
    // Fewer lines in the file than were asked for: all of it.
    file.seek(SeekFrom::Start(start))?;
    dump(file, out)
}

/// The last `n` lines of a stream that cannot be seeked.
///
/// Holds at most `n` complete lines plus the one being read, which is the least
/// that can answer the question — the last line cannot be known until the
/// input ends.
fn last_lines_stream(
    source: &mut impl Read,
    out: &mut impl Write,
    n: u64,
    line_end: u8,
) -> io::Result<()> {
    let Ok(keep) = usize::try_from(n) else {
        return dump(source, out);
    };
    if keep == 0 {
        return drain(source);
    }
    let mut lines: VecDeque<Vec<u8>> = VecDeque::new();
    let mut partial: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let got = source.read(&mut buf)?;
        if got == 0 {
            break;
        }
        let mut chunk = buf.get(..got).unwrap_or_default();
        while let Some(at) = chunk.iter().position(|&b| b == line_end) {
            partial.extend_from_slice(chunk.get(..=at).unwrap_or_default());
            lines.push_back(std::mem::take(&mut partial));
            if lines.len() > keep {
                lines.pop_front();
            }
            chunk = chunk.get(at.saturating_add(1)..).unwrap_or_default();
        }
        partial.extend_from_slice(chunk);
    }
    // The unterminated remainder is a line as well.
    if !partial.is_empty() {
        lines.push_back(partial);
        if lines.len() > keep {
            lines.pop_front();
        }
    }
    for line in &lines {
        out.write_all(line)?;
    }
    Ok(())
}

/// Consume the input without printing any of it, so that a pipe's writer is not
/// left blocked on a reader that never reads.
fn drain(source: &mut impl Read) -> io::Result<()> {
    let mut buf = vec![0u8; CHUNK];
    while source.read(&mut buf)? != 0 {}
    Ok(())
}

// --------------------------------------------------------------- following ---

/// Poll the watched files until there is nothing left to watch — upstream's
/// `tail_forever`.
///
/// This is the polling loop and only the polling loop. Upstream has a second,
/// `inotify`-driven implementation that it prefers where the kernel offers it;
/// we have no such interface, so the `---disable-inotify` switch is already the
/// permanent state of affairs. That is not merely a missing optimisation — it
/// changes what the help text can honestly say, which is why `--help` here is
/// two clauses shorter than GNU's.
fn follow(
    watched: &mut [Watched],
    out: &mut impl Write,
    options: &Options,
    print_headers: bool,
    first_header: &mut bool,
) {
    // Which file the last banner named. A new banner is only needed when the
    // output moves to a different one.
    let mut last = watched.len().saturating_sub(1);
    let mut writer_is_dead = false;

    loop {
        let mut any_input = false;
        for i in 0..watched.len() {
            let Some(w) = watched.get_mut(i) else { continue };
            if w.ignore {
                continue;
            }
            if w.file.is_none() {
                recheck(w, options);
                continue;
            }
            any_input |= poll_one(w, out, options, print_headers, first_header, i, &mut last);
        }

        if !any_live(watched, options) {
            eprintln!("tail: no files remaining");
            return;
        }
        if !any_input && out.flush().is_err() {
            return;
        }
        if !any_input {
            if writer_is_dead {
                return;
            }
            // Once the writer is known dead, go round once more before
            // stopping: it may have written something between the last read and
            // its death.
            writer_is_dead = options.pid.is_some_and(|p| !process_alive(p));
            if !writer_is_dead {
                sleep(options.sleep_interval);
            }
        }
    }
}

/// One file, one iteration. `true` when something was read.
fn poll_one(
    w: &mut Watched,
    out: &mut impl Write,
    options: &Options,
    print_headers: bool,
    first_header: &mut bool,
    index: usize,
    last: &mut usize,
) -> bool {
    let stats = match w.file.as_ref().map(File::metadata) {
        Some(Ok(m)) => m,
        Some(Err(e)) => {
            w.trouble = Trouble::Io(e.kind());
            eprintln!("tail: {}: {}", quotef(&w.label), strerror(&e));
            w.file = None;
            return false;
        }
        None => return false,
    };

    // Asked once, of the handle rather than of the metadata: on the host build
    // `Metadata::is_file` calls a pipe a regular file and gives it the length of
    // whatever is buffered in it, which would make every poll of a pipe look
    // like a file that keeps changing size. See `filekind`.
    let regular = w.file.as_ref().is_some_and(filekind::is_regular);

    let unchanged = w.regular == regular
        && (!regular || w.size == stats.len())
        && w.modified == stats.modified().ok();
    let mut read_unchanged = false;
    if unchanged {
        w.unchanged = w.unchanged.saturating_add(1);
        if options.max_unchanged < w.unchanged && options.follow == Follow::Name {
            // The file has not moved for a while; the *name* may have. This is
            // the log-rotation case, and the only thing `--max-unchanged-stats`
            // controls.
            recheck(w, options);
            w.unchanged = 0;
            return false;
        }
        if regular || *last != index {
            return false;
        }
        // A pipe or terminal whose `mtime` never moves: reading is the only way
        // to find out whether anything arrived.
        read_unchanged = true;
    }

    w.modified = stats.modified().ok();
    w.regular = regular;
    if !read_unchanged {
        w.unchanged = 0;
    }

    if regular && stats.len() < w.size {
        eprintln!("tail: {}: file truncated", quotef(&w.label));
        if let Some(f) = w.file.as_mut() {
            let _ = f.seek(SeekFrom::Start(0));
        }
        w.size = 0;
    }

    if !read_unchanged && index != *last {
        if print_headers {
            write_header(out, &w.label, first_header);
        }
        *last = index;
    }

    let Some(file) = w.file.as_mut() else {
        return false;
    };
    let before = w.size;
    match io::copy(file, out) {
        Ok(read) => {
            w.size = before.saturating_add(read);
            if read_unchanged && read != 0 {
                w.unchanged = 0;
            }
            read != 0
        }
        Err(e) => {
            eprintln!("tail: error reading {}: {}", quoteaf(&w.label), strerror(&e));
            false
        }
    }
}

/// Look at the *name* again and report what became of it — upstream's
/// `recheck`, less the branches that only fire when `inotify` is in use.
///
/// Two of upstream's diagnostics are unreachable here and are absent rather
/// than dead: `has been replaced with an untailable symbolic link` and `has
/// been replaced with an untailable remote file` are both guarded by
/// `! disable_inotify`, and inotify is permanently disabled in this
/// implementation.
fn recheck(w: &mut Watched, options: &Options) {
    let was_tailable = w.tailable;
    let previous = w.trouble;

    let opened = if w.is_stdin {
        Ok(stdin_as_file())
    } else {
        File::open(&w.name)
    };
    w.tailable = !(options.retry && opened.is_err());

    let (mut file, stats) = match opened.and_then(|f| f.metadata().map(|m| (f, m))) {
        Ok(pair) => pair,
        Err(e) => {
            w.trouble = Trouble::Io(e.kind());
            if w.tailable {
                // A different failure from last time is news; the same one
                // again is not.
                if previous != w.trouble {
                    eprintln!("tail: {}: {}", quotef(&w.label), strerror(&e));
                }
            } else if was_tailable {
                eprintln!(
                    "tail: {} has become inaccessible: {}",
                    quoteaf(&w.label),
                    strerror(&e)
                );
            }
            w.file = None;
            return;
        }
    };

    if !tailable(&stats) {
        w.trouble = Trouble::NotAnErrno;
        w.tailable = false;
        w.ignore = !(options.retry && options.follow == Follow::Name);
        if was_tailable || previous != Trouble::NotAnErrno {
            eprintln!(
                "tail: {} has been replaced with an untailable file{}",
                quoteaf(&w.label),
                if w.ignore {
                    "; giving up on this name"
                } else {
                    ""
                }
            );
        }
        w.file = None;
        return;
    }

    let id = file_id(&stats);
    let fresh = if previous != Trouble::None && previous != Trouble::Io(ErrorKind::NotFound) {
        eprintln!("tail: {} has become accessible", quoteaf(&w.label));
        true
    } else if w.file.is_none() {
        // A name that was missing and is here again is a new file even when the
        // identity matches, because identities get reused.
        eprintln!(
            "tail: {} has appeared;  following new file",
            quoteaf(&w.label)
        );
        true
    } else if w.id != id {
        eprintln!(
            "tail: {} has been replaced;  following new file",
            quoteaf(&w.label)
        );
        true
    } else {
        false
    };
    w.trouble = Trouble::None;

    if fresh {
        // A new file is read from its start, not from where the old one
        // stopped.
        w.size = 0;
        w.regular = filekind::is_regular(&file);
        w.modified = stats.modified().ok();
        w.id = id;
        w.file = Some(file);
    } else {
        // Nothing changed; keep the handle already open and drop the new one.
        remember(w, &stats, &mut file);
    }
}

/// Whether anything is still worth watching.
///
/// `--retry --follow=name` is always worth watching: the whole point of the
/// combination is to wait for a file that does not exist yet.
fn any_live(watched: &[Watched], options: &Options) -> bool {
    if options.retry && options.follow == Follow::Name {
        return true;
    }
    watched.iter().any(|w| w.file.is_some())
}

/// Whether a file of this type can be followed at all: upstream's
/// `IS_TAILABLE_FILE_TYPE`, which is everything except a directory and a block
/// device.
#[cfg(unix)]
fn tailable(m: &Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    let t = m.file_type();
    m.is_file() || t.is_fifo() || t.is_socket() || t.is_char_device()
}

#[cfg(not(unix))]
fn tailable(m: &Metadata) -> bool {
    // The host build. There is no file-type detail here beyond "directory or
    // not", and a directory cannot be opened in the first place — see the
    // module doc.
    !m.is_dir()
}

/// A file's identity, for telling a rotated log from the same log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileId {
    volume: u64,
    file: u64,
}

#[cfg(unix)]
fn file_id(m: &Metadata) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt;
    Some(FileId {
        volume: m.dev(),
        file: m.ino(),
    })
}

#[cfg(not(unix))]
fn file_id(m: &Metadata) -> Option<FileId> {
    use std::os::windows::fs::MetadataExt;
    // The real answer is the volume serial number and the file index, which
    // `std` only exposes behind an unstable feature. Creation time is the
    // stand-in: it does not identify a file, but it does change when one name
    // comes to refer to a different file, which is the only question asked of
    // it here. This is the host test build; SlateOS presents as `unix`.
    Some(FileId {
        volume: 0,
        file: m.creation_time(),
    })
}

/// Wait between iterations. A non-finite or absurd interval sleeps for as long
/// as the clock allows rather than failing, which is what `-s inf` asks for.
fn sleep(seconds: f64) {
    let duration = std::time::Duration::try_from_secs_f64(seconds)
        .unwrap_or(std::time::Duration::from_secs(u64::MAX / 2));
    std::thread::sleep(duration);
}

/// Whether the process `--pid` names is still running.
///
/// "Running" includes "running but not ours to signal": a process owned by
/// somebody else answers `EPERM`, which proves it exists.
#[cfg(unix)]
fn process_alive(pid: u64) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 performs the existence and permission checks and
    // delivers nothing. The call takes two integers and touches no memory of
    // ours.
    if unsafe { kill(pid, 0) } == 0 {
        return true;
    }
    io::Error::last_os_error().kind() == ErrorKind::PermissionDenied
}

#[cfg(windows)]
fn process_alive(pid: u64) -> bool {
    use core::ffi::c_void;
    /// `PROCESS_QUERY_LIMITED_INFORMATION` — the least that can answer this,
    /// and the most that is granted for a process of another user.
    const QUERY_LIMITED: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn GetExitCodeProcess(handle: *mut c_void, code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    let Ok(pid) = u32::try_from(pid) else {
        return false;
    };
    // SAFETY: `OpenProcess` returns null on failure and is checked for it. The
    // handle is closed on every path out.
    unsafe {
        let handle = OpenProcess(QUERY_LIMITED, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code: u32 = 0;
        let got = GetExitCodeProcess(handle, &raw mut code);
        CloseHandle(handle);
        got != 0 && code == STILL_ACTIVE
    }
}

#[cfg(not(any(unix, windows)))]
fn process_alive(_pid: u64) -> bool {
    // No way to ask, so never stop early on this account.
    true
}

/// Standard input as a [`File`], so that the seeking paths and `metadata` work
/// on it exactly as they do on an opened file.
///
/// The returned handle does **not** own the descriptor — dropping it must not
/// close standard input, which the rest of the program and the caller's shell
/// still have opinions about.
fn stdin_as_file() -> File {
    #[cfg(unix)]
    {
        use std::os::fd::{AsRawFd, FromRawFd};
        // SAFETY: descriptor 0 is open for the life of the process, and the
        // `File` is leaked rather than dropped, so it is never closed here.
        let borrowed = unsafe { File::from_raw_fd(io::stdin().as_raw_fd()) };
        clone_or_leak(borrowed)
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        // SAFETY: as above, for the equivalent handle.
        let borrowed = unsafe { File::from_raw_handle(io::stdin().as_raw_handle()) };
        clone_or_leak(borrowed)
    }
}

/// Turn a borrowed handle into an owned one by duplicating it, falling back to
/// leaking the borrow if the duplicate fails.
///
/// Either way the original descriptor survives: `try_clone` makes a second one,
/// and `ManuallyDrop::into_inner` of a leaked handle is never reached because
/// the leak is what is returned.
fn clone_or_leak(borrowed: File) -> File {
    let borrowed = std::mem::ManuallyDrop::new(borrowed);
    borrowed.try_clone().unwrap_or_else(|_| {
        // SAFETY: the handle inside is still open and is never dropped, because
        // `borrowed` is a `ManuallyDrop` that goes out of scope without running
        // a destructor. The copy handed out aliases it for the rest of the
        // process, which is exactly what a borrowed standard input is.
        unsafe { std::ptr::read(&raw const *borrowed) }
    })
}

/// Whether standard input is a regular file, which decides whether a plain
/// `read` on it would block.
fn stdin_is_regular() -> bool {
    filekind::is_regular(&stdin_as_file())
}

// -------------------------------------------------------------- byte paths ---

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

    fn parse(items: &[&str]) -> Options {
        match parse_args(&args(items)) {
            Ok(Request::Run(o, _)) => o,
            other => panic!("expected a run request, got {other:?}"),
        }
    }

    fn operands(items: &[&str]) -> Vec<String> {
        match parse_args(&args(items)) {
            Ok(Request::Run(_, files)) => files
                .iter()
                .map(|f| f.to_string_lossy().into_owned())
                .collect(),
            other => panic!("expected a run request, got {other:?}"),
        }
    }

    fn fail(items: &[&str]) -> getopt::Error {
        parse_args(&args(items)).unwrap_err()
    }

    /// The message without the referral every getopt diagnostic ends with.
    fn body(e: &getopt::Error) -> String {
        e.message
            .strip_suffix("\nTry 'tail --help' for more information.")
            .unwrap_or(&e.message)
            .to_string()
    }

    /// Run the streaming half of [`emit`] — the paths that take any `Read`, and
    /// so the ones that can be exercised without a file on disk. This is exactly
    /// what `---presume-input-pipe` selects, which is why it is set here.
    fn stream(input: &[u8], items: &[&str]) -> Vec<u8> {
        let mut options = parse(items);
        options.presume_input_pipe = true;
        let mut source = input;
        let mut out: Vec<u8> = Vec::new();
        let n = options.n_units;
        match (options.unit, options.from_start) {
            // The `N - 1` that `run` applies before calling `emit`; repeated
            // here because this helper stands in for `run`.
            (Unit::Bytes, true) => skip_bytes(&mut source, &mut out, n.saturating_sub(1)),
            (Unit::Lines, true) => {
                skip_lines(&mut source, &mut out, n.saturating_sub(1), options.line_end)
            }
            (Unit::Bytes, false) => last_bytes_stream(&mut source, &mut out, n),
            (Unit::Lines, false) => last_lines_stream(&mut source, &mut out, n, options.line_end),
        }
        .unwrap();
        out
    }

    /// The same request against a real file, so the seeking paths run. Both
    /// halves must agree — that they are separate code is the reason to check.
    fn seeking(input: &[u8], items: &[&str]) -> Vec<u8> {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "slateos-tail-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, input).unwrap();
        let options = parse(items);
        let mut file = File::open(&path).unwrap();
        let mut out: Vec<u8> = Vec::new();
        let n = options.n_units;
        let result = match (options.unit, options.from_start) {
            (Unit::Bytes, true) => {
                file.seek(SeekFrom::Start(n.saturating_sub(1))).unwrap();
                dump(&mut file, &mut out)
            }
            (Unit::Lines, true) => {
                skip_lines(&mut file, &mut out, n.saturating_sub(1), options.line_end)
            }
            (Unit::Bytes, false) => last_bytes_seek(&mut file, &mut out, n),
            (Unit::Lines, false) => last_lines_seek(&mut file, &mut out, n, options.line_end),
        };
        drop(file);
        let _ = std::fs::remove_file(&path);
        result.unwrap();
        out
    }

    /// Both halves at once: they are required to be indistinguishable.
    fn both(input: &[u8], items: &[&str]) -> Vec<u8> {
        let streamed = stream(input, items);
        let sought = seeking(input, items);
        assert_eq!(
            String::from_utf8_lossy(&streamed),
            String::from_utf8_lossy(&sought),
            "the seeking and streaming paths disagree for {items:?}"
        );
        streamed
    }

    // ------------------------------------------------------------ options ---

    #[test]
    fn the_default_is_ten_lines_from_the_end() {
        let o = parse(&[]);
        assert_eq!(o, Options::default());
        assert_eq!(o.unit, Unit::Lines);
        assert_eq!(o.n_units, 10);
        assert!(!o.from_start);
        assert!(!o.forever);
        assert_eq!(o.follow, Follow::Descriptor);
        assert_eq!(o.headers, Headers::Multiple);
        assert_eq!(o.line_end, b'\n');
        assert_eq!(o.sleep_interval, 1.0);
        assert_eq!(o.max_unchanged, 5);
    }

    #[test]
    fn long_options_abbreviate_the_way_getopt_long_does() {
        assert_eq!(parse(&["--by", "3"]).unit, Unit::Bytes);
        assert_eq!(parse(&["--li=3"]).n_units, 3);
        assert_eq!(parse(&["--verb"]).headers, Headers::Always);
        assert_eq!(parse(&["--zero"]).line_end, 0);
        assert_eq!(parse(&["--sl=2"]).sleep_interval, 2.0);
        // `--q` and `--s` are ambiguous here in a way they are not in `head`:
        // `--s` prefixes both `--silent` and `--sleep-interval`, which differ.
        assert_eq!(parse(&["--q"]).headers, Headers::Never);
        assert_eq!(
            body(&fail(&["--s"])),
            "option '--s' is ambiguous; possibilities: '--silent' '--sleep-interval'"
        );
        // `--r` is likewise `--retry` alone, since nothing else starts with r.
        assert!(parse(&["--r", "-f"]).retry);
    }

    /// The two hidden options really do take three dashes.
    #[test]
    fn the_hidden_options_are_spelled_with_three_dashes() {
        assert!(parse(&["---presume-input-pipe"]).presume_input_pipe);
        assert!(parse(&["---p"]).presume_input_pipe);
        assert_eq!(parse(&["---d"]), Options::default());
        // Two dashes is a different name, and matches nothing.
        assert_eq!(
            body(&fail(&["--presume-input-pipe"])),
            "unrecognized option '--presume-input-pipe'"
        );
    }

    /// Measured with `tail --=x`: an empty prefix matches every option, so the
    /// list is the whole table in GNU's declaration order.
    #[test]
    fn the_ambiguity_list_is_in_gnus_declaration_order() {
        assert_eq!(
            body(&fail(&["--=x"])),
            "option '--=x' is ambiguous; possibilities: '--bytes' '--follow' \
             '--lines' '--max-unchanged-stats' '---disable-inotify' '--pid' \
             '---presume-input-pipe' '--quiet' '--retry' '--silent' \
             '--sleep-interval' '--verbose' '--zero-terminated' '--help' \
             '--version'"
        );
    }

    #[test]
    fn every_getopt_sentence_matches_glibc() {
        assert_eq!(body(&fail(&["-x"])), "invalid option -- 'x'");
        assert_eq!(body(&fail(&["-c"])), "option requires an argument -- 'c'");
        // Not `--fol`, which is a unique prefix of `--follow` and resolves.
        assert_eq!(body(&fail(&["--nosuch"])), "unrecognized option '--nosuch'");
        assert_eq!(
            body(&fail(&["--lines"])),
            "option '--lines' requires an argument"
        );
        assert_eq!(
            body(&fail(&["--verbose=1"])),
            "option '--verbose' doesn't allow an argument"
        );
        for a in [["-x"], ["--lines"]] {
            assert_eq!(fail(&a).status, 1);
        }
    }

    /// `--follow`'s argument goes through `argmatch`, which abbreviates too and
    /// has its own two-line diagnostic.
    #[test]
    fn follow_takes_an_optional_abbreviated_keyword() {
        assert_eq!(parse(&["--follow"]).follow, Follow::Descriptor);
        assert!(parse(&["--follow"]).forever);
        assert_eq!(parse(&["--follow=n"]).follow, Follow::Name);
        assert_eq!(parse(&["--follow=d"]).follow, Follow::Descriptor);
        // An *optional* argument is never taken from the next word, so this is
        // follow-by-descriptor of a file called `name`.
        assert_eq!(parse(&["--follow", "name"]).follow, Follow::Descriptor);
        assert_eq!(operands(&["--follow", "name"]), vec!["name"]);
        assert_eq!(
            body(&fail(&["--follow=x"])),
            "invalid argument 'x' for '--follow'\n\
             Valid arguments are:\n  - 'descriptor'\n  - 'name'"
        );
    }

    #[test]
    fn dash_capital_f_is_follow_name_plus_retry() {
        let o = parse(&["-F"]);
        assert!(o.forever);
        assert!(o.retry);
        assert_eq!(o.follow, Follow::Name);
    }

    #[test]
    fn a_lone_dash_and_everything_after_dash_dash_are_operands() {
        assert_eq!(operands(&["-n1", "-", "f"]), vec!["-", "f"]);
        assert_eq!(operands(&["--", "-3", "-v"]), vec!["-3", "-v"]);
        // An option after an operand is still an option: glibc permutes.
        assert_eq!(parse(&["f", "-v"]).headers, Headers::Always);
    }

    #[test]
    fn a_short_cluster_takes_its_value_from_the_rest_or_the_next_argument() {
        assert_eq!(parse(&["-qn2"]).n_units, 2);
        assert_eq!(parse(&["-qn", "2"]).n_units, 2);
        assert_eq!(parse(&["-vz"]), Options {
            headers: Headers::Always,
            line_end: 0,
            ..Options::default()
        });
        // `-c2n`: the whole rest of the cluster is `c`'s argument, so the `n`
        // is part of the number and the number is bad.
        assert_eq!(body(&fail(&["-c2n"])), "invalid number of bytes: '2n'");
    }

    // ---------------------------------------------------- the obsolete form ---

    #[test]
    fn the_obsolete_form_is_only_recognised_in_three_shapes() {
        // One word.
        assert_eq!(parse(&["-3"]).n_units, 3);
        // Two, where the second is not an option.
        assert_eq!(parse(&["-3", "f"]).n_units, 3);
        assert_eq!(parse(&["-3", "-"]).n_units, 3);
        // Three, where the second is exactly `--`.
        assert_eq!(parse(&["-3", "--", "f"]).n_units, 3);
        // Anything else and the digit reaches getopt, which refuses it — note
        // that whether `-3` is a count depends on words that come *after* it.
        for a in [
            vec!["-3", "-q"],
            vec!["-3", "f", "g"],
            vec!["-3", "-", "-"],
            vec!["-3", "--", "f", "g"],
            vec!["-q", "-3"],
            vec!["f", "-3"],
        ] {
            let e = fail(&a);
            assert_eq!(body(&e), "option used in invalid context -- 3", "{a:?}");
            assert_eq!(e.status, 1);
        }
    }

    #[test]
    fn the_obsolete_form_counts_from_the_start_with_a_plus() {
        let o = parse(&["+3", "f"]);
        assert!(o.from_start);
        assert_eq!(o.n_units, 3);
        // A `+` word in any other position is an operand, not a count.
        assert_eq!(operands(&["-q", "+3"]), vec!["+3"]);
    }

    #[test]
    fn a_bare_dash_and_a_bare_dash_c_fall_through_to_getopt() {
        // Upstream's `!p[p[0] == 'c']` test: `-` is standard input …
        assert_eq!(operands(&["-"]), vec!["-"]);
        // … and `-c` is an option that wants an argument.
        assert_eq!(body(&fail(&["-c"])), "option requires an argument -- 'c'");
        // But `-f`, `-b` and `-l` have no digits and are still the obsolete
        // form, which is how `tail -b` means 5120 bytes.
        assert_eq!(parse(&["-f"]), Options {
            forever: true,
            ..Options::default()
        });
        assert_eq!(parse(&["-b"]), Options {
            unit: Unit::Bytes,
            n_units: 5120,
            ..Options::default()
        });
        assert_eq!(parse(&["-l"]), Options::default());
    }

    #[test]
    fn the_obsolete_letters_are_units_and_b_is_also_a_multiplier() {
        // With digits, `b` multiplies the digits …
        assert_eq!(parse(&["-2b"]), Options {
            unit: Unit::Bytes,
            n_units: 1024,
            ..Options::default()
        });
        // … while without them it multiplies the default, which is the same
        // rule applied in a different place and gives 10 × 512.
        assert_eq!(parse(&["-b"]).n_units, 5120);
        assert_eq!(parse(&["-2c"]), Options {
            unit: Unit::Bytes,
            n_units: 2,
            ..Options::default()
        });
        assert_eq!(parse(&["-2l"]).n_units, 2);
        assert_eq!(parse(&["-2l"]).unit, Unit::Lines);
        // A trailing `f` is `-f`, and only there.
        assert!(parse(&["-2f"]).forever);
        assert!(parse(&["-2lf"]).forever);
        assert!(parse(&["+2bf"]).forever);
        // `k` and `m` are *not* suffixes in this form — they are junk, which
        // makes the word not obsolete at all, so the digit reaches getopt.
        assert_eq!(
            body(&fail(&["-2k"])),
            "option used in invalid context -- 2"
        );
    }

    #[test]
    fn the_obsolete_number_reports_overflow_against_the_whole_word() {
        assert_eq!(
            body(&fail(&["-99999999999999999999999"])),
            "invalid number: '-99999999999999999999999': \
             Numerical result out of range"
        );
        // The ×512 can overflow on its own, with the same sentence.
        assert_eq!(
            body(&fail(&["-99999999999999999999b"])),
            "invalid number: '-99999999999999999999b': \
             Numerical result out of range"
        );
    }

    // ------------------------------------------------------------ numbers ---

    #[test]
    fn counts_take_the_multiplier_suffixes() {
        assert_eq!(parse(&["-n", "3"]).n_units, 3);
        assert_eq!(parse(&["-c", "1b"]).n_units, 512);
        assert_eq!(parse(&["-c", "2K"]).n_units, 2048);
        assert_eq!(parse(&["-c", "2k"]).n_units, 2048);
        assert_eq!(parse(&["-c", "2KB"]).n_units, 2000);
        assert_eq!(parse(&["-c", "2KiB"]).n_units, 2048);
        assert_eq!(parse(&["-c", "2M"]).n_units, 2 * 1024 * 1024);
        assert_eq!(parse(&["-c", "2MB"]).n_units, 2_000_000);
        // A bare suffix means one of it …
        assert_eq!(parse(&["-c", "K"]).n_units, 1024);
        // … but only when it is the very first byte.
        assert_eq!(body(&fail(&["-c", " K"])), "invalid number of bytes: ' K'");
        // Leading whitespace and a leading `+` are `strtoumax`'s to skip.
        assert_eq!(parse(&["-n", " 3"]).n_units, 3);
        assert_eq!(parse(&["-n", "+3"]).n_units, 3);
        assert!(parse(&["-n", "+3"]).from_start);
    }

    #[test]
    fn a_bad_count_is_quoted_the_way_gnulib_quotes_it() {
        // `quote()`, whose escaping is C's and not the shell's — the two agree
        // on everything without a quote or a backslash in it.
        assert_eq!(body(&fail(&["-n", "a'b"])), "invalid number of lines: 'a\\'b'");
        assert_eq!(body(&fail(&["-n", "a\\b"])), "invalid number of lines: 'a\\\\b'");
        assert_eq!(body(&fail(&["-c", "x"])), "invalid number of bytes: 'x'");
        // The sign is stripped for `-` and kept for `+`, so the same bad text
        // is echoed back two different ways.
        assert_eq!(body(&fail(&["-n", "-x"])), "invalid number of lines: 'x'");
        assert_eq!(body(&fail(&["-n", "+x"])), "invalid number of lines: '+x'");
        assert_eq!(
            body(&fail(&["-n", "99999999999999999999"])),
            "invalid number of lines: '99999999999999999999': \
             Value too large for defined data type"
        );
        // A bad suffix outranks an overflow.
        assert_eq!(
            body(&fail(&["-n", "99999999999999999999x"])),
            "invalid number of lines: '99999999999999999999x'"
        );
    }

    #[test]
    fn from_start_is_sticky_once_set() {
        // Upstream never clears the flag, so this skips one line rather than
        // printing the last two.
        let o = parse(&["-n", "+2", "-n", "2"]);
        assert!(o.from_start);
        assert_eq!(o.n_units, 2);
    }

    #[test]
    fn pid_and_max_unchanged_take_no_suffix_and_have_their_own_wording() {
        assert_eq!(parse(&["--pid=7", "-f"]).pid, Some(7));
        assert_eq!(parse(&["--max-unchanged-stats=3"]).max_unchanged, 3);
        assert_eq!(body(&fail(&["--pid=5k"])), "invalid PID: '5k'");
        assert_eq!(body(&fail(&["--pid=x"])), "invalid PID: 'x'");
        // Above INT_MAX is the overflow wording, not the invalid one.
        assert_eq!(
            body(&fail(&["--pid=2147483648"])),
            "invalid PID: '2147483648': Value too large for defined data type"
        );
        assert_eq!(parse(&["--pid=2147483647", "-f"]).pid, Some(PID_MAX));
        assert_eq!(
            body(&fail(&["--max-unchanged-stats=x"])),
            "invalid maximum number of unchanged stats between opens: 'x'"
        );
    }

    #[test]
    fn the_sleep_interval_is_a_float_and_strtod_parses_it() {
        assert_eq!(parse(&["-s", "0.5"]).sleep_interval, 0.5);
        assert_eq!(parse(&["-s", "1e1"]).sleep_interval, 10.0);
        assert_eq!(parse(&["-s", " 2"]).sleep_interval, 2.0);
        assert_eq!(parse(&["-s", "0"]).sleep_interval, 0.0);
        // A hexadecimal float is a number to `strtod`, and is not to Rust's
        // own parser — which is why there is a hand-written one.
        assert_eq!(parse(&["-s", "0x1.8p3"]).sleep_interval, 12.0);
        assert_eq!(parse(&["-s", "0x10"]).sleep_interval, 16.0);
        assert_eq!(body(&fail(&["-s", "x"])), "invalid number of seconds: 'x'");
        assert_eq!(body(&fail(&["-s", "-1"])), "invalid number of seconds: '-1'");
        // NaN parses and is then rejected by `0 <= s`, every comparison
        // against it being false.
        assert_eq!(body(&fail(&["-s", "nan"])), "invalid number of seconds: 'nan'");
        // Infinity is not rejected: it is not negative.
        assert!(parse(&["-s", "inf"]).sleep_interval.is_infinite());
    }

    // --------------------------------------------------------------- body ---

    const FIVE: &[u8] = b"1\n2\n3\n4\n5\n";

    #[test]
    fn the_last_n_lines_are_printed() {
        assert_eq!(both(FIVE, &["-n", "2"]), b"4\n5\n");
        assert_eq!(both(FIVE, &["-n", "5"]), FIVE);
        // More than there are is all of them, not an error.
        assert_eq!(both(FIVE, &["-n", "99"]), FIVE);
        assert_eq!(both(FIVE, &[]), FIVE);
        assert_eq!(both(FIVE, &["-n", "0"]), b"");
        assert_eq!(both(b"", &["-n", "2"]), b"");
    }

    #[test]
    fn an_unterminated_final_line_counts_as_a_line() {
        assert_eq!(both(b"1\n2\n3", &["-n", "1"]), b"3");
        assert_eq!(both(b"1\n2\n3", &["-n", "2"]), b"2\n3");
        // And so does a file that is one unterminated line.
        assert_eq!(both(b"only", &["-n", "1"]), b"only");
        assert_eq!(both(b"only", &["-n", "3"]), b"only");
    }

    #[test]
    fn a_trailing_terminator_does_not_open_an_empty_last_line() {
        // `1\n2\n` is two lines, so the last one is `2\n` and not an empty
        // string after it.
        assert_eq!(both(b"1\n2\n", &["-n", "1"]), b"2\n");
        // A file of nothing but terminators is that many empty lines.
        assert_eq!(both(b"\n\n\n", &["-n", "2"]), b"\n\n");
    }

    #[test]
    fn the_last_n_bytes_are_printed() {
        assert_eq!(both(FIVE, &["-c", "4"]), b"4\n5\n");
        assert_eq!(both(FIVE, &["-c", "0"]), b"");
        assert_eq!(both(FIVE, &["-c", "99"]), FIVE);
        assert_eq!(both(b"abc", &["-c", "1"]), b"c");
    }

    #[test]
    fn a_plus_count_skips_from_the_start() {
        assert_eq!(both(FIVE, &["-n", "+3"]), b"3\n4\n5\n");
        assert_eq!(both(FIVE, &["-n", "+1"]), FIVE);
        // Past the end is nothing, and not an error.
        assert_eq!(both(FIVE, &["-n", "+99"]), b"");
        assert_eq!(both(FIVE, &["-c", "+3"]), b"2\n3\n4\n5\n");
        assert_eq!(both(FIVE, &["-c", "+1"]), FIVE);
        assert_eq!(both(FIVE, &["-c", "+99"]), b"");
    }

    /// `-n +0` and `-n 0` are opposites, which is the one place the sign
    /// matters for a value that is otherwise the same number.
    #[test]
    fn plus_zero_is_the_whole_file_and_zero_is_none_of_it() {
        assert_eq!(both(FIVE, &["-n", "+0"]), FIVE);
        assert_eq!(both(FIVE, &["-n", "0"]), b"");
        assert_eq!(both(FIVE, &["-c", "+0"]), FIVE);
        assert_eq!(both(FIVE, &["-c", "0"]), b"");
    }

    #[test]
    fn zero_terminated_changes_what_a_line_is() {
        let input = b"a\0b\0c\0";
        assert_eq!(both(input, &["-z", "-n", "2"]), b"b\0c\0");
        // And newlines are then just bytes.
        assert_eq!(both(b"a\nb\n", &["-z", "-n", "1"]), b"a\nb\n");
    }

    /// The backwards scan reads the file in blocks, so a file bigger than one
    /// block is the case that exercises the loop rather than its first step.
    #[test]
    fn the_backwards_scan_crosses_block_boundaries() {
        let mut input: Vec<u8> = Vec::new();
        for i in 1..=40_000u32 {
            input.extend_from_slice(format!("{i}\n").as_bytes());
        }
        assert_eq!(both(&input, &["-n", "3"]), b"39998\n39999\n40000\n");
        assert_eq!(both(&input, &["-n", "1"]), b"40000\n");
        assert_eq!(both(&input, &["-c", "6"]), b"40000\n");
        assert_eq!(both(&input, &["-c", "7"]), b"\n40000\n");
        assert_eq!(both(&input, &["-n", "+39999"]), b"39999\n40000\n");
        // A line count larger than the file is still the whole file.
        assert_eq!(both(&input, &["-n", "100000"]).len(), input.len());
    }

    /// A line longer than one block is the other way the loop can be wrong: the
    /// terminator it is looking for is nowhere in the block it just read.
    #[test]
    fn a_line_longer_than_a_block_is_still_one_line() {
        let mut input: Vec<u8> = vec![b'x'; CHUNK * 3 + 7];
        input.push(b'\n');
        input.extend_from_slice(b"last\n");
        assert_eq!(both(&input, &["-n", "1"]), b"last\n");
        assert_eq!(both(&input, &["-n", "2"]).len(), input.len());
    }

    // ----------------------------------------------------------- warnings ---

    #[test]
    fn the_warned_about_combinations_are_recognised() {
        // `--retry` without following at all …
        let o = parse(&["--retry"]);
        assert!(o.retry && !o.forever);
        // … and with following by descriptor, which is a different warning.
        let o = parse(&["--retry", "-f"]);
        assert!(o.retry && o.forever && o.follow == Follow::Descriptor);
        // `-F` is neither, since it sets `--follow=name` itself.
        let o = parse(&["-F"]);
        assert!(o.retry && o.forever && o.follow == Follow::Name);
        // `--pid` without `-f`.
        assert_eq!(parse(&["--pid=1"]).pid, Some(1));
        assert!(!parse(&["--pid=1"]).forever);
    }

    // --------------------------------------------------------------- help ---

    #[test]
    fn help_and_version_end_parsing_and_outrank_a_bad_operand() {
        assert_eq!(parse_args(&args(&["--help"])), Ok(Request::Help));
        assert_eq!(parse_args(&args(&["--version"])), Ok(Request::Version));
        // Abbreviated, and after other options.
        assert_eq!(parse_args(&args(&["-q", "--hel"])), Ok(Request::Help));
        // But a diagnostic earlier on the line still wins, because parsing is
        // left to right.
        assert!(parse_args(&args(&["-x", "--help"])).is_err());
    }

    #[test]
    fn the_help_text_mentions_every_option_it_has() {
        let help = help_text();
        for name in [
            "--bytes", "--follow", "-F", "--lines", "--max-unchanged-stats", "--pid",
            "--quiet", "--silent", "--retry", "--sleep-interval", "--verbose",
            "--zero-terminated", "--help", "--version",
        ] {
            assert!(help.contains(name), "{name} is missing from --help");
        }
        // The hidden ones stay hidden.
        assert!(!help.contains("presume-input-pipe"));
        assert!(!help.contains("disable-inotify"));
    }
}
