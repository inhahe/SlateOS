//! `cmp` — compare two files byte by byte.
//!
//! ```text
//! Usage: cmp [OPTION]... FILE1 [FILE2 [SKIP1 [SKIP2]]]
//!   -b, --print-bytes          print differing bytes
//!   -i, --ignore-initial=SKIP[:SKIP2]  skip leading bytes
//!   -l, --verbose              output byte numbers and differing byte values
//!   -n, --bytes=LIMIT          compare at most LIMIT bytes
//!   -s, --quiet, --silent      suppress all normal output
//! ```
//!
//! Exit status is **0** if the inputs are the same, **1** if they differ, and
//! **2** if something went wrong — which is why every diagnostic here has to be
//! careful about which of the three it is claiming.
//!
//! # Why this is a port rather than a re-derivation
//!
//! `cmp` is not a coreutils tool; it belongs to **diffutils**, and its output
//! is one of the more precisely specified things in POSIX. Every string below
//! was measured against GNU diffutils 3.10 before it was written down, and the
//! four format strings are the ones in the shipped binary:
//!
//! ```text
//! %s %s differ: byte %s, line %s
//! %s %s differ: byte %s, line %s is %3o %s %3o %s
//! cmp: EOF on %s after byte %s, line %s
//! cmp: EOF on %s after byte %s, in line %s
//! ```
//!
//! The distinction in the last two is easy to miss and is not decoration: a
//! file that ends *at* a line boundary reports `line N`, one that ends
//! mid-line reports `in line N+1`. `-l` reports neither, because upstream does
//! not track lines in that mode at all.
//!
//! A file name is a byte string — `design.txt` allows every byte but `/` and
//! NUL — so argv is read with [`std::env::args_os`] and carried as `[u8]`
//! throughout. The version this replaces collected `Vec<String>` and therefore
//! *panicked* on a name it was perfectly legal to be handed.
//!
//! # Four deliberate differences from GNU
//!
//! 1. **File names in output are quoted when they need it.** GNU prints them
//!    raw, so a file named with an embedded newline forges a whole extra line
//!    of `cmp`'s output — measured, `cmp 'sp ace' $'nl\nname'` prints two
//!    lines that both look like real ones. `quotef` leaves an ordinary name
//!    completely untouched, so `cmp a b` is byte-identical to GNU's; only a
//!    name that could lie about the output changes. See `design-decisions.md`
//!    §373, and §371 for the same call in `stat`.
//! 2. **A rejected option value is escaped inside its quotes.** The marks stay
//!    straight — `invalid --ignore-initial value '%s'` spells them literally in
//!    diffutils' own format string, so unlike a `quote()` message they do not
//!    follow the locale — but the bytes between them go through `quote_glibc`
//!    rather than being interpolated raw, so a control byte cannot move the
//!    cursor. Every value that is not an attack renders identically.
//! 3. **No `Report bugs to:` footer** in `--help`, matching every other
//!    utility here.
//! 4. **Under `-l`, the `EOF on …` note always comes after the rows it
//!    summarises.** GNU's order is whatever its stdio buffering happens to
//!    produce: the rows go to stdout and the note to stderr, and stdout is
//!    flushed only at exit. On a terminal stdout is line-buffered and the rows
//!    win; into a pipe it is block-buffered and the note jumps ahead of them —
//!    measured, `cmp -l x1 x2 2>&1 | cat` prints the note first. We flush
//!    stdout before writing the note, so the order is the terminal one in both
//!    cases. A summary printed before what it summarises is not a behaviour
//!    worth reproducing, and nothing can depend on it: it is not stable in GNU
//!    either.
//!
//! # `byte` or `char`
//!
//! POSIX mandates `differ: char N` in the POSIX locale; GNU says `byte`
//! everywhere else, on the grounds that "char" is a lie in a multibyte locale.
//! Which one appears is decided by gnulib's `hard_locale (LC_MESSAGES)`, i.e.
//! by whether `LC_ALL`/`LC_MESSAGES`/`LANG` name anything other than `C` or
//! `POSIX` — the same test `ls` already applies to `LC_TIME`. With `-b` it is
//! always `byte`, because upstream's combined format string has no `char`
//! spelling.
//!
//! # Every way of naming a skip raises it; none of them lowers it
//!
//! `-i`, `--ignore-initial`, and the positional `SKIP1`/`SKIP2` operands all
//! reach upstream's one `specify_ignore_initial`, which ends
//! `if (ignore_initial[f] < val) ignore_initial[f] = val;`. So `cmp -i 5 a b
//! 0 0` still skips 5, and `cmp -i 5:6 a b 7 2` skips 7 and 6 — each slot
//! independently keeping whichever of the two numbers naming it was larger.
//! The one asymmetry: a lone `-i N` copies itself into the second slot (again
//! only when that raises it), while a lone positional `SKIP1` says nothing
//! about the second file at all.
//!
//! # How this is tested
//!
//! The body below is `#[cfg(unix)]`, so on the Windows build host it does not
//! exist and `cargo test` can reach only the parser and the formatters. The
//! end-to-end coverage is `scripts/cmp-diff.sh`, which builds this file for
//! Linux inside WSL and compares it against GNU diffutils case by case — the
//! same answer `du`, `find` and `ls` already use. It found three defects that
//! thirty-three green unit tests had not. See `design-decisions.md` §365 and
//! §374; anything changed below should be re-measured there, not merely
//! unit-tested here.

// Off unix the whole of `imp` disappears and every platform-neutral item below
// it loses its only non-test caller. The items are still built and still
// tested; only the entry point is missing.
#![cfg_attr(not(unix), allow(dead_code))]

#[cfg(not(unix))]
use coreutils::diag;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{quote_glibc, quotef};
use std::ffi::OsString;
use std::io::{self, Read, Write};

const CMP: Program = Program::new("cmp", 2);

/// The inputs are the same. GNU's `EXIT_SUCCESS`.
const EXIT_SAME: u8 = 0;
/// The inputs differ. GNU's `EXIT_FAILURE`.
const EXIT_DIFFER: u8 = 1;
/// Something went wrong. GNU's `EXIT_TROUBLE`.
const EXIT_TROUBLE: u8 = 2;

/// Upstream's cap on `--bytes` and `--ignore-initial`: both land in an `off_t`.
const COUNT_MAX: u64 = i64::MAX as u64;

// ---------------------------------------------------------------- counts ---

/// gnulib's `xstrtoumax (s, &end, 0, &val, "0kKMGTPEZY")`, which is how both
/// `--bytes` and `--ignore-initial` read their argument.
///
/// Returns the value and whatever followed it, or `None` if no number could be
/// read at all or the arithmetic overflowed. The `0` in upstream's suffix list
/// is not a suffix: it is gnulib's flag for "call `strtoumax` with base 0", and
/// it is why `-n 0x10` and `-n 010` are accepted as hex and octal.
///
/// The caller decides whether a non-empty tail is an error, because upstream
/// deliberately allows one: the first half of `-i SKIP1:SKIP2` stops at the
/// colon and hands the rest back.
fn scan_count(text: &[u8]) -> Option<(u64, &[u8])> {
    let mut at = 0usize;
    while text.get(at).is_some_and(u8::is_ascii_whitespace) {
        at = at.saturating_add(1);
    }
    // `strtoumax` would accept a leading `-` and wrap; gnulib's unsigned
    // wrapper refuses it, which is why `cmp -n -1` is a usage error and not a
    // silent 18446744073709551615.
    if text.get(at) == Some(&b'+') {
        at = at.saturating_add(1);
    }

    let (radix, skip) = match (text.get(at), text.get(at.saturating_add(1))) {
        (Some(b'0'), Some(b'x' | b'X'))
            if text
                .get(at.saturating_add(2))
                .is_some_and(u8::is_ascii_hexdigit) =>
        {
            (16u64, 2usize)
        }
        (Some(b'0'), _) => (8, 0),
        _ => (10, 0),
    };
    at = at.saturating_add(skip);

    let start = at;
    let mut value: u64 = 0;
    while let Some(digit) = text.get(at).and_then(|c| (*c as char).to_digit(32)) {
        let digit = u64::from(digit);
        if digit >= radix {
            break;
        }
        value = value.checked_mul(radix)?.checked_add(digit)?;
        at = at.saturating_add(1);
    }
    if at == start {
        return None;
    }

    // gnulib's `bkm_scale_by_power`, restricted to the suffixes `cmp` declares.
    // A trailing `B` (or the obsolescent `D`) switches the base from 1024 to
    // 1000, which is the whole difference between `1K` and `1kB`.
    let power = match text.get(at) {
        Some(b'k' | b'K') => 1u32,
        Some(b'M') => 2,
        Some(b'G') => 3,
        Some(b'T') => 4,
        Some(b'P') => 5,
        Some(b'E') => 6,
        Some(b'Z') => 7,
        Some(b'Y') => 8,
        _ => 0,
    };
    if power != 0 {
        at = at.saturating_add(1);
        let base = if matches!(text.get(at), Some(b'B' | b'D')) {
            at = at.saturating_add(1);
            1000u64
        } else {
            1024
        };
        value = value.checked_mul(base.checked_pow(power)?)?;
    }

    Some((value, text.get(at..).unwrap_or(&[])))
}

/// Upstream's `specify_ignore_initial`, which is also how `--bytes` is read.
///
/// `delimiter` is the one byte the value is allowed to stop before; upstream
/// passes `':'` for the first half of `-i SKIP1:SKIP2` and NUL — that is,
/// `None` here — everywhere else. On failure the *whole* remaining text is
/// named, not the offending byte, which is why `cmp -i 1:2:3` complains about
/// `2:3`.
///
/// `quote_glibc`, not `quote`: upstream's format string is
/// `invalid --%s value '%s'`, with the apostrophes written literally, so the
/// marks stay straight in every locale. We escape where upstream interpolates
/// raw bytes; see `xnum::strtol_fatal` for the same call made for the same
/// reason.
fn take_count<'a>(
    text: &'a [u8],
    option: &str,
    delimiter: Option<u8>,
) -> Result<(u64, &'a [u8]), getopt::Error> {
    let bad = || CMP.usage_referring(format!("invalid --{option} value {}", quote_glibc(text)));
    let (value, rest) = scan_count(text).ok_or_else(bad)?;
    let stopped_well = match rest.first() {
        None => true,
        Some(c) => Some(*c) == delimiter,
    };
    if !stopped_well || value > COUNT_MAX {
        return Err(bad());
    }
    Ok((value, rest))
}

// -------------------------------------------------------------- settings ---

/// What `--help` / `--version` / a real run resolve to.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Run(Box<Settings>),
}

/// A parsed command line.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Settings {
    /// `-b`: show each differing byte as an octal value and a character.
    print_bytes: bool,
    /// `-l`: report every difference rather than stopping at the first.
    verbose: bool,
    /// `-s`: exit status only.
    quiet: bool,
    /// `-i` / the `SKIP1 SKIP2` operands: bytes to drop from each input.
    skip: [u64; 2],
    /// `-n`: how many bytes to compare at most.
    limit: u64,
    /// The two operands, as typed. The second defaults to `-`.
    names: [Vec<u8>; 2],
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            print_bytes: false,
            verbose: false,
            quiet: false,
            skip: [0, 0],
            // Upstream starts `bytes` at the largest `off_t` so that repeated
            // `-n` can simply take the smaller of the two.
            limit: COUNT_MAX,
            names: [Vec::new(), b"-".to_vec()],
        }
    }
}

const SHORT_OPTIONS: &str = "bci:ln:sv";

/// Upstream's `longopts`, in its order. `--print-chars` has been an alias of
/// `--print-bytes` since diffutils 2.7.3 and is still accepted.
///
/// `--version` comes *before* `--help`, which is the opposite of every
/// coreutils bin in this tree: coreutils spells the tail of its table with
/// `GETOPT_HELP_OPTION_DECL` then `GETOPT_VERSION_OPTION_DECL`, while diffutils
/// writes both entries out by hand in the other order. The order is not
/// cosmetic — glibc reports `pfound`, the first entry an ambiguous prefix
/// matched, so a table holding the same names in a different order names a
/// different option in its diagnostics. Measured with `cmp --=x`, whose empty
/// prefix matches everything and so prints the whole table in declaration
/// order; `scripts/getopt-ambiguity-check.py` compares the two as sequences and
/// caught this one.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("print-bytes", Takes::Nothing),
    ("print-chars", Takes::Nothing),
    ("ignore-initial", Takes::Required),
    ("verbose", Takes::Nothing),
    ("bytes", Takes::Required),
    ("silent", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("version", Takes::Nothing),
    ("help", Takes::Nothing),
];

/// Every way of naming a skip *raises* it; none of them lowers it.
///
/// Upstream funnels `-i`, `--ignore-initial` and the positional `SKIP1`/`SKIP2`
/// operands through one function, `specify_ignore_initial`, whose last line is
/// `if (ignore_initial[f] < val) ignore_initial[f] = val;` — a maximum, not an
/// assignment. So `cmp -i 5 a b 0 0` still skips 5 (measured), and `cmp -i 5:6
/// a b 7 2` skips 7 and 6: each slot independently keeps whichever of the two
/// numbers naming it was larger. Getting this wrong is silent — the run still
/// succeeds, it just compares the wrong bytes and reports offsets against the
/// wrong origin.
fn raise_skip(set: &mut Settings, slot: usize, value: u64) {
    if let Some(current) = set.skip.get_mut(slot)
        && *current < value
    {
        *current = value;
    }
}

/// Read the command line.
///
/// `last_word` is what upstream names in `missing operand after '%s'`: it is
/// `argv[optind - 1]`, and since that error can only happen when every word was
/// consumed as an option, it is the last word of the whole argv — `cmp` itself
/// when there were no arguments at all.
fn parse_args(argv: &[OsString], last_word: &[u8]) -> Result<Request, getopt::Error> {
    let mut set = Settings::default();
    let mut operands: Vec<Vec<u8>> = Vec::new();

    for item in CMP.parse(argv, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Short(b'b' | b'c', _) | Opt::Long("print-bytes" | "print-chars", _) => {
                set.print_bytes = true;
            }
            Opt::Short(b'l', _) | Opt::Long("verbose", _) => set.verbose = true,
            Opt::Short(b's', _) | Opt::Long("silent" | "quiet", _) => set.quiet = true,
            Opt::Short(b'i', value) | Opt::Long("ignore-initial", value) => {
                let raw = value.unwrap_or_default();
                let text = coreutils::quote::os_bytes(&raw).into_owned();
                let (first, rest) = take_count(&text, "ignore-initial", Some(b':'))?;
                raise_skip(&mut set, 0, first);
                match rest.split_first() {
                    Some((b':', tail)) => {
                        let (second, _) = take_count(tail, "ignore-initial", None)?;
                        raise_skip(&mut set, 1, second);
                    }
                    // One value means "skip this much of both", which upstream
                    // gets by copying the first slot into the second — again
                    // only when that raises it, so an earlier `-i 1:9` survives
                    // a later `-i 5` with the 9 intact. Upstream's `else if`.
                    _ => raise_skip(&mut set, 1, first),
                }
            }
            Opt::Short(b'n', value) | Opt::Long("bytes", value) => {
                let raw = value.unwrap_or_default();
                let text = coreutils::quote::os_bytes(&raw).into_owned();
                let (limit, _) = take_count(&text, "bytes", None)?;
                set.limit = set.limit.min(limit);
            }
            Opt::Short(b'v', _) | Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Operand(word) => operands.push(coreutils::quote::os_bytes(word).into_owned()),
            // Unreachable: every letter of `SHORT_OPTIONS` and every row of
            // `LONG_OPTIONS` is handled above.
            Opt::Short(..) | Opt::Long(..) => {}
        }
    }

    // Checked before the operands are looked at: `cmp -l -s` with no files at
    // all reports the conflict, not a missing operand.
    if set.verbose && set.quiet {
        return Err(CMP.usage_referring("options -l and -s are incompatible".to_string()));
    }

    let mut operands = operands.into_iter();
    let Some(first) = operands.next() else {
        // Straight marks again: `missing operand after '%s'` and
        // `extra operand '%s'` both spell the apostrophes literally upstream.
        return Err(
            CMP.usage_referring(format!("missing operand after {}", quote_glibc(last_word)))
        );
    };
    *set.names.get_mut(0).unwrap_or(&mut Vec::new()) = first;
    if let Some(second) = operands.next() {
        *set.names.get_mut(1).unwrap_or(&mut Vec::new()) = second;
    }
    // The third and fourth operands are SKIP1 and SKIP2. They go through the
    // same maximum as `-i` — and unlike `-i`, a lone SKIP1 says nothing about
    // the second file, so `cmp a b 5` skips 5 bytes of `a` and none of `b`.
    for slot in 0..2usize {
        if let Some(word) = operands.next() {
            let (value, _) = take_count(&word, "ignore-initial", None)?;
            raise_skip(&mut set, slot, value);
        }
    }
    if let Some(extra) = operands.next() {
        return Err(CMP.usage_referring(format!("extra operand {}", quote_glibc(&extra))));
    }

    Ok(Request::Run(Box::new(set)))
}

/// Render a parse error the way diffutils does, which is *not* the way GNU
/// coreutils does — and the difference is a whole `cmp: ` on the second line.
///
/// Coreutils' `usage()` writes the referral with a bare `fprintf`, so `ls
/// --bogus` prints an unprefixed `Try 'ls --help' …`. Diffutils' `try_help`
/// instead makes a second `error (…)` call, and `error` always prefixes the
/// program name, so `cmp -Z` prints `cmp: Try 'cmp --help' …`. Measured on both.
/// [`getopt::Error`]'s own `Display` follows the coreutils shape, which every
/// other utility here wants, so `cmp` assembles its own.
fn diagnostic(error: &getopt::Error) -> String {
    let mut text = format!("cmp: {}", error.sentence);
    if error.referral.is_some() {
        text.push_str("\ncmp: Try 'cmp --help' for more information.");
    }
    text
}

fn help_text() -> String {
    "\
Usage: cmp [OPTION]... FILE1 [FILE2 [SKIP1 [SKIP2]]]
Compare two files byte by byte.

The optional SKIP1 and SKIP2 specify the number of bytes to skip
at the beginning of each file (zero by default).

Mandatory arguments to long options are mandatory for short options too.
  -b, --print-bytes          print differing bytes
  -i, --ignore-initial=SKIP         skip first SKIP bytes of both inputs
  -i, --ignore-initial=SKIP1:SKIP2  skip first SKIP1 bytes of FILE1 and
                                      first SKIP2 bytes of FILE2
  -l, --verbose              output byte numbers and differing byte values
  -n, --bytes=LIMIT          compare at most LIMIT bytes
  -s, --quiet, --silent      suppress all normal output
      --help                 display this help and exit
  -v, --version              output version information and exit

SKIP values may be followed by the following multiplicative suffixes:
kB 1000, K 1024, MB 1,000,000, M 1,048,576,
GB 1,000,000,000, G 1,073,741,824, and so on for T, P, E, Z, Y.

If a FILE is '-' or missing, read standard input.
Exit status is 0 if inputs are the same, 1 if different, 2 if trouble.
"
    .to_string()
}

// ------------------------------------------------------------- rendering ---

/// gnulib's `hard_locale (LC_MESSAGES)`: false for exactly `C` and `POSIX`,
/// and the three variables are consulted in the order the C library does.
fn hard_locale_messages() -> bool {
    let var = |key: &str| {
        std::env::var_os(key)
            .map(|v| coreutils::quote::os_bytes(&v).into_owned())
            .filter(|v| !v.is_empty())
    };
    !matches!(
        var("LC_ALL")
            .or_else(|| var("LC_MESSAGES"))
            .or_else(|| var("LANG"))
            .unwrap_or_default()
            .as_slice(),
        b"" | b"C" | b"POSIX"
    )
}

/// Upstream's `sprintc`: a byte as `cat -v` would show it.
///
/// The high bit becomes an `M-` prefix, a control byte becomes `^` plus the
/// letter 64 above it, and DEL becomes `^?`. Everything else is itself — a
/// space really does print as a space, which is why the `-bl` column is padded.
fn sprintc(byte: u8) -> String {
    let mut out = String::with_capacity(4);
    if byte >= 0x80 {
        out.push_str("M-");
    }
    let low = byte & 0x7f;
    if low < 0x20 {
        out.push('^');
        out.push(char::from(low.saturating_add(0x40)));
    } else if low == 0x7f {
        out.push_str("^?");
    } else {
        out.push(char::from(low));
    }
    out
}

/// The width of the byte-offset column in `-l` mode.
///
/// Upstream takes the number of digits in the *smallest* number of bytes that
/// could possibly be compared: the `--bytes` limit, lowered by each input that
/// is a regular file and so has a known length. Two pipes therefore give 19,
/// the width of the largest `off_t`, which looks odd but is what GNU prints.
fn offset_width(limit: u64, remaining: [Option<u64>; 2]) -> usize {
    let mut smallest = limit;
    for size in remaining.iter().flatten() {
        smallest = smallest.min(*size);
    }
    let mut width = 1usize;
    let mut left = smallest / 10;
    while left != 0 {
        width = width.saturating_add(1);
        left /= 10;
    }
    width
}

/// One `-l` row: the byte number, then each byte in octal, and with `-b` each
/// byte's character too.
fn verbose_row(width: usize, number: u64, a: u8, b: u8, print_bytes: bool) -> String {
    if print_bytes {
        format!(
            "{number:>width$} {a:3o} {:<4} {b:3o} {}",
            sprintc(a),
            sprintc(b)
        )
    } else {
        format!("{number:>width$} {a:3o} {b:3o}")
    }
}

/// The line a default-mode run prints on stdout when it finds a difference.
///
/// `pair` is `Some` under `-b`, and its presence also forces the `byte`
/// spelling: upstream's combined format string has no `char` variant.
fn differ_line(
    names: [&[u8]; 2],
    byte: u64,
    line: u64,
    pair: Option<(u8, u8)>,
    hard_locale: bool,
) -> String {
    let unit = if hard_locale || pair.is_some() {
        "byte"
    } else {
        "char"
    };
    let mut out = format!(
        "{} {} differ: {unit} {byte}, line {line}",
        quotef(names.first().copied().unwrap_or(b"")),
        quotef(names.get(1).copied().unwrap_or(b"")),
    );
    if let Some((a, b)) = pair {
        out.push_str(&format!(" is {a:3o} {} {b:3o} {}", sprintc(a), sprintc(b)));
    }
    out
}

/// The line a run prints on stderr when one input ends before the other.
///
/// Three shapes, and the choice between the last two is the one that is easy to
/// get wrong: a file whose last compared byte was a newline ended *at* a line
/// boundary and reports `line N`, where N counts the newlines; one that ended
/// mid-line reports `in line N+1`. `-l` reports no line at all, because
/// upstream does not count them in that mode.
fn eof_message(
    name: &[u8],
    byte: u64,
    newlines: u64,
    at_line_start: bool,
    verbose: bool,
) -> String {
    let name = quotef(name);
    if byte == 0 {
        format!("cmp: EOF on {name} which is empty")
    } else if verbose {
        format!("cmp: EOF on {name} after byte {byte}")
    } else if at_line_start {
        format!("cmp: EOF on {name} after byte {byte}, line {newlines}")
    } else {
        format!(
            "cmp: EOF on {name} after byte {byte}, in line {}",
            newlines.saturating_add(1)
        )
    }
}

// --------------------------------------------------------------- compare ---

/// What the byte-by-byte walk found.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Outcome {
    /// Every compared byte matched and neither input ran out first.
    Same,
    /// `-l` found at least one difference; both inputs then ended together.
    Differed,
    /// The first difference, which is all default mode looks for.
    Diff { byte: u64, line: u64, a: u8, b: u8 },
    /// One input ended first.
    Eof {
        /// `0` or `1`: which input ended.
        which: usize,
        /// How many bytes were compared before it did.
        byte: u64,
        /// How many of those bytes were newlines.
        newlines: u64,
        /// Whether the last of them was.
        at_line_start: bool,
    },
}

/// A failure that ends the run with status 2, remembering whose it was.
enum Trouble {
    /// Reading input 0 or 1 failed.
    Input(usize, io::Error),
    /// Writing the report failed.
    Output(io::Error),
}

/// Big enough that the syscall cost disappears, small enough to stay off the
/// stack: both buffers are heap-allocated once per run.
const BUFFER: usize = 64 * 1024;

/// The comparison itself, with no knowledge of where its inputs came from.
struct Compare {
    verbose: bool,
    print_bytes: bool,
    limit: u64,
    offset_width: usize,
}

/// Read until `buf` is full or the input ends. A short return means EOF, which
/// is what lets the caller tell "this file is shorter" from "this read was
/// partial".
fn fill(input: &mut dyn Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut got = 0usize;
    while got < buf.len() {
        let Some(rest) = buf.get_mut(got..) else {
            break;
        };
        match input.read(rest) {
            Ok(0) => break,
            Ok(n) => got = got.saturating_add(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(got)
}

impl Compare {
    fn run(
        &self,
        a: &mut dyn Read,
        b: &mut dyn Read,
        out: &mut dyn Write,
    ) -> Result<Outcome, Trouble> {
        let mut buf_a = vec![0u8; BUFFER];
        let mut buf_b = vec![0u8; BUFFER];
        let mut compared: u64 = 0;
        let mut newlines: u64 = 0;
        let mut at_line_start = false;
        let mut differed = false;

        loop {
            let want = usize::try_from(self.limit.saturating_sub(compared))
                .unwrap_or(BUFFER)
                .min(BUFFER);
            if want == 0 {
                return Ok(if differed {
                    Outcome::Differed
                } else {
                    Outcome::Same
                });
            }

            let (Some(slot_a), Some(slot_b)) = (buf_a.get_mut(..want), buf_b.get_mut(..want))
            else {
                // Unreachable: `want <= BUFFER`, which is both buffers' length.
                return Ok(Outcome::Same);
            };
            let got_a = fill(a, slot_a).map_err(|e| Trouble::Input(0, e))?;
            let got_b = fill(b, slot_b).map_err(|e| Trouble::Input(1, e))?;
            let common = got_a.min(got_b);

            for i in 0..common {
                let (Some(&x), Some(&y)) = (buf_a.get(i), buf_b.get(i)) else {
                    break;
                };
                if x != y {
                    if self.verbose {
                        differed = true;
                        let row = verbose_row(
                            self.offset_width,
                            compared.saturating_add(i as u64).saturating_add(1),
                            x,
                            y,
                            self.print_bytes,
                        );
                        writeln!(out, "{row}").map_err(Trouble::Output)?;
                    } else {
                        return Ok(Outcome::Diff {
                            byte: compared.saturating_add(i as u64).saturating_add(1),
                            line: newlines.saturating_add(1),
                            a: x,
                            b: y,
                        });
                    }
                }
                // Counted from the first input. Where it matters — default
                // mode, which stops at the first difference — the two inputs
                // agree on every byte counted here.
                at_line_start = x == b'\n';
                if at_line_start {
                    newlines = newlines.saturating_add(1);
                }
            }
            compared = compared.saturating_add(common as u64);

            if got_a != got_b {
                return Ok(Outcome::Eof {
                    which: usize::from(got_a >= got_b),
                    byte: compared,
                    newlines,
                    at_line_start,
                });
            }
            if got_a < want {
                return Ok(if differed {
                    Outcome::Differed
                } else {
                    Outcome::Same
                });
            }
        }
    }
}

// ------------------------------------------------------------------ main ---

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    diag!("cmp: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(EXIT_TROUBLE)
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
#[cfg(unix)]
fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(imp::main(), 2)
}

#[cfg(unix)]
mod imp {
    use super::{
        Compare, EXIT_DIFFER, EXIT_SAME, EXIT_TROUBLE, Outcome, Request, Settings, Trouble,
        diagnostic, differ_line, eof_message, hard_locale_messages, help_text, offset_width,
        parse_args,
    };
    use coreutils::diag;
    use coreutils::errmsg::strerror;
    use coreutils::quote::{os_bytes, os_from_bytes, quotef};
    use std::fs::File;
    use std::io::{self, Read, Seek, SeekFrom, Write};
    use std::mem::ManuallyDrop;
    use std::os::fd::FromRawFd;
    use std::os::unix::fs::MetadataExt;
    use std::process::ExitCode;

    /// One opened input.
    struct Opened {
        /// The name as it was typed, for diagnostics.
        name: Vec<u8>,
        /// `-` is standard input, and the descriptor behind it belongs to the
        /// process rather than to us, so it must not be closed on drop.
        file: ManuallyDrop<File>,
        /// True when `file` owns its descriptor and should close it.
        owned: bool,
        /// `st_size` when this is a regular file. Only regular files constrain
        /// the `-l` column width, and only they can be skipped by seeking.
        size: Option<u64>,
        /// `(st_dev, st_ino)`, for the "these are the same file" shortcut.
        id: (u64, u64),
    }

    impl Drop for Opened {
        fn drop(&mut self) {
            if self.owned {
                // SAFETY: `owned` is set only for a `File` this process opened
                // and has not otherwise dropped, so this is its one close.
                unsafe { ManuallyDrop::drop(&mut self.file) };
            }
        }
    }

    /// Open one operand. `-` is descriptor 0, exactly as upstream does it —
    /// not a file named `-`.
    fn open(name: &[u8]) -> io::Result<Opened> {
        let (file, owned) = if name == b"-" {
            // SAFETY: descriptor 0 is standard input, which the runtime keeps
            // open for the life of the process. `owned` stays false, so the
            // `ManuallyDrop` is never dropped and the descriptor is not closed.
            (unsafe { ManuallyDrop::new(File::from_raw_fd(0)) }, false)
        } else {
            (ManuallyDrop::new(File::open(os_from_bytes(name))?), true)
        };
        let meta = file.metadata()?;
        Ok(Opened {
            name: name.to_vec(),
            size: meta.is_file().then(|| meta.size()),
            id: (meta.dev(), meta.ino()),
            file,
            owned,
        })
    }

    /// Drop `count` leading bytes. A regular file seeks; anything else — a
    /// pipe, a terminal — has to read and throw away, which is also the only
    /// correct thing to do to a stream that cannot be rewound.
    ///
    /// A seek that the kernel refuses falls through to the reading path rather
    /// than failing the run. `-i` accepts anything up to `OFF_T_MAX`, but Linux
    /// rejects an `lseek` past the filesystem's maximum file size outright, so
    /// `cmp -i 9223372036854775807 a b` — two tiny files, a skip that lands
    /// them both at EOF and therefore makes them equal — used to die with
    /// `Invalid argument` where GNU quietly exits 0. `lseek` leaves the offset
    /// untouched when it fails, so the fallback starts from the right place,
    /// and it costs at most one pass over a file we were told to skip anyway.
    fn skip(input: &mut Opened, count: u64) -> io::Result<()> {
        if count == 0 {
            return Ok(());
        }
        if input.size.is_some()
            && let Ok(offset) = i64::try_from(count)
            && input.file.seek(SeekFrom::Current(offset)).is_ok()
        {
            return Ok(());
        }
        let mut scratch = vec![0u8; 64 * 1024];
        let mut left = count;
        while left != 0 {
            let want = usize::try_from(left)
                .unwrap_or(scratch.len())
                .min(scratch.len());
            let Some(slot) = scratch.get_mut(..want) else {
                break;
            };
            match input.file.read(slot) {
                Ok(0) => break,
                Ok(n) => left = left.saturating_sub(n as u64),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Where the descriptor is now. `None` for a stream that cannot say, which
    /// is how two reads of the same pipe still compare equal.
    fn position(input: &mut Opened) -> Option<u64> {
        input.file.stream_position().ok()
    }

    pub fn main() -> ExitCode {
        let full: Vec<std::ffi::OsString> = std::env::args_os().collect();
        let last_word = full
            .last()
            .map(|w| os_bytes(w).into_owned())
            .unwrap_or_else(|| b"cmp".to_vec());
        let argv = full.get(1..).unwrap_or(&[]).to_vec();

        let settings = match parse_args(&argv, &last_word) {
            Ok(Request::Help) => {
                print!("{}", help_text());
                return ExitCode::from(EXIT_SAME);
            }
            Ok(Request::Version) => {
                println!("cmp (SlateOS coreutils) 0.1.0");
                return ExitCode::from(EXIT_SAME);
            }
            Ok(Request::Run(settings)) => *settings,
            Err(e) => {
                diag!("{}", diagnostic(&e));
                return ExitCode::from(u8::try_from(e.status).unwrap_or(EXIT_TROUBLE));
            }
        };

        match run(&settings) {
            Ok(code) => ExitCode::from(code),
            Err(code) => ExitCode::from(code),
        }
    }

    fn run(settings: &Settings) -> Result<u8, u8> {
        let mut inputs: Vec<Opened> = Vec::with_capacity(2);
        for slot in 0..2usize {
            let name = settings.names.get(slot).map_or(&b"-"[..], Vec::as_slice);
            match open(name) {
                Ok(opened) => inputs.push(opened),
                Err(e) => {
                    // `-s` is "suppress all normal output", and upstream takes
                    // that to cover the open failure too — *whatever* it was.
                    // Measured: `cmp -s noperm a` prints nothing and exits 2,
                    // exactly as `cmp -s nosuch a` does. The status is the
                    // whole answer, which is the point of the option.
                    if !settings.quiet {
                        diag!("cmp: {}: {}", quotef(name), strerror(&e));
                    }
                    return Err(EXIT_TROUBLE);
                }
            }
        }

        for slot in 0..2usize {
            let count = settings.skip.get(slot).copied().unwrap_or(0);
            let Some(input) = inputs.get_mut(slot) else {
                break;
            };
            if let Err(e) = skip(input, count) {
                diag!("cmp: {}: {}", quotef(&input.name), strerror(&e));
                return Err(EXIT_TROUBLE);
            }
        }

        // Two names for one file, read from the same place, cannot differ —
        // and answering without reading is the difference between `cmp a a` on
        // a 40 GiB image costing nothing and costing forty gigabytes of I/O.
        // A stream that cannot report a position answers `None` twice, which
        // is how upstream's `cmp - -` also exits 0.
        let same_file = match (inputs.first().map(|i| i.id), inputs.get(1).map(|i| i.id)) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        if same_file {
            // Borrowed one at a time: `inputs` is a `Vec`, and two simultaneous
            // `get_mut` calls into it would need a split that buys nothing here.
            let first = inputs.get_mut(0).and_then(position);
            let second = inputs.get_mut(1).and_then(position);
            if first == second {
                return Ok(EXIT_SAME);
            }
        }

        compare(settings, &mut inputs)
    }

    fn compare(settings: &Settings, inputs: &mut [Opened]) -> Result<u8, u8> {
        let remaining = [
            inputs.first().and_then(|i| {
                i.size
                    .map(|s| s.saturating_sub(settings.skip.first().copied().unwrap_or(0)))
            }),
            inputs.get(1).and_then(|i| {
                i.size
                    .map(|s| s.saturating_sub(settings.skip.get(1).copied().unwrap_or(0)))
            }),
        ];
        let engine = Compare {
            verbose: settings.verbose,
            print_bytes: settings.print_bytes,
            limit: settings.limit,
            offset_width: offset_width(settings.limit, remaining),
        };

        let [name_a, name_b] = [
            inputs
                .first()
                .map_or(&b"-"[..], |i| i.name.as_slice())
                .to_vec(),
            inputs
                .get(1)
                .map_or(&b"-"[..], |i| i.name.as_slice())
                .to_vec(),
        ];

        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());
        let (head, tail) = inputs.split_at_mut(1);
        let (Some(a), Some(b)) = (head.first_mut(), tail.first_mut()) else {
            return Err(EXIT_TROUBLE);
        };
        // Under `-s` the rows and the `differ:` line are suppressed, so the
        // engine writes into a sink that costs nothing.
        let mut sink = io::sink();
        let outcome = if settings.quiet {
            engine.run(
                &mut a.file as &mut File,
                &mut b.file as &mut File,
                &mut sink,
            )
        } else {
            engine.run(&mut a.file as &mut File, &mut b.file as &mut File, &mut out)
        };

        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(Trouble::Input(which, e)) => {
                let name = if which == 0 { &name_a } else { &name_b };
                let _ = out.flush();
                diag!("cmp: {}: {}", quotef(name), strerror(&e));
                return Err(EXIT_TROUBLE);
            }
            // The engine writes nothing but difference rows, so a failed write
            // is proof that a difference was found. A closed downstream reader
            // — `cmp -l a b | head -1` — is an ordinary end to a pipeline and
            // gets that answer; anything else is a genuine failure.
            Err(Trouble::Output(e)) if e.kind() == io::ErrorKind::BrokenPipe => {
                return Ok(EXIT_DIFFER);
            }
            Err(Trouble::Output(e)) => {
                diag!("cmp: write error: {}", strerror(&e));
                return Err(EXIT_TROUBLE);
            }
        };

        let status = match outcome {
            Outcome::Same => EXIT_SAME,
            Outcome::Differed => EXIT_DIFFER,
            Outcome::Diff { byte, line, a, b } => {
                if !settings.quiet {
                    let pair = settings.print_bytes.then_some((a, b));
                    let line = differ_line(
                        [name_a.as_slice(), name_b.as_slice()],
                        byte,
                        line,
                        pair,
                        hard_locale_messages(),
                    );
                    if let Err(e) = writeln!(out, "{line}")
                        && e.kind() != io::ErrorKind::BrokenPipe
                    {
                        diag!("cmp: write error: {}", strerror(&e));
                        return Err(EXIT_TROUBLE);
                    }
                }
                EXIT_DIFFER
            }
            Outcome::Eof {
                which,
                byte,
                newlines,
                at_line_start,
            } => {
                if !settings.quiet {
                    // The rows go to stdout and this goes to stderr, so the
                    // buffer has to be emptied first or `-l` reports the end of
                    // a file before the differences inside it.
                    let _ = out.flush();
                    let name = if which == 0 { &name_a } else { &name_b };
                    diag!(
                        "{}",
                        eof_message(name, byte, newlines, at_line_start, settings.verbose)
                    );
                }
                EXIT_DIFFER
            }
        };

        match out.flush() {
            Ok(()) => Ok(status),
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(status),
            Err(e) => {
                diag!("cmp: write error: {}", strerror(&e));
                Err(EXIT_TROUBLE)
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn settings(words: &[&str]) -> Settings {
        match parse_args(&args(words), b"cmp").unwrap() {
            Request::Run(set) => *set,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    fn error(words: &[&str]) -> String {
        parse_args(&args(words), b"cmp").unwrap_err().sentence
    }

    /// What the program actually writes to stderr for a parse error, referral
    /// line and all.
    fn printed(words: &[&str]) -> String {
        diagnostic(&parse_args(&args(words), b"cmp").unwrap_err())
    }

    /// Run the engine over two byte strings and return (outcome, stdout).
    fn run(a: &[u8], b: &[u8], set: &Settings, width: usize) -> (Outcome, String) {
        let engine = Compare {
            verbose: set.verbose,
            print_bytes: set.print_bytes,
            limit: set.limit,
            offset_width: width,
        };
        let mut out = Vec::new();
        let outcome = match engine.run(&mut &a[..], &mut &b[..], &mut out) {
            Ok(o) => o,
            Err(_) => panic!("in-memory readers cannot fail"),
        };
        (outcome, String::from_utf8(out).unwrap())
    }

    // ------------------------------------------------------ count parsing ---

    #[test]
    fn a_count_is_read_in_base_zero_with_gnus_suffixes() {
        let value = |s: &str| scan_count(s.as_bytes()).map(|(v, rest)| (v, rest.to_vec()));
        assert_eq!(value("0"), Some((0, vec![])));
        assert_eq!(value("12"), Some((12, vec![])));
        assert_eq!(value("+5"), Some((5, vec![])));
        assert_eq!(value(" 2"), Some((2, vec![])));
        assert_eq!(value("010"), Some((8, vec![])));
        assert_eq!(value("0x10"), Some((16, vec![])));
        assert_eq!(value("1k"), Some((1024, vec![])));
        assert_eq!(value("1K"), Some((1024, vec![])));
        assert_eq!(value("1kB"), Some((1000, vec![])));
        assert_eq!(value("1KB"), Some((1000, vec![])));
        assert_eq!(value("1kD"), Some((1000, vec![])));
        assert_eq!(value("1M"), Some((1024 * 1024, vec![])));
        assert_eq!(value("1MB"), Some((1_000_000, vec![])));
        assert_eq!(value("1E"), Some((1u64 << 60, vec![])));
        // A tail is handed back rather than refused; the caller decides.
        assert_eq!(value("1:2"), Some((1, b":2".to_vec())));
        assert_eq!(value("1x"), Some((1, b"x".to_vec())));
    }

    #[test]
    fn a_count_that_is_not_one_is_refused() {
        assert_eq!(scan_count(b""), None);
        assert_eq!(scan_count(b":2"), None);
        assert_eq!(scan_count(b"-1"), None);
        assert_eq!(scan_count(b"x"), None);
        // 1Z is 2^70: a valid suffix, an impossible value.
        assert_eq!(scan_count(b"1Z"), None);
        assert_eq!(scan_count(b"1Y"), None);
    }

    #[test]
    fn a_count_above_off_t_is_refused_even_though_it_fits_a_u64() {
        // Measured: GNU accepts i64::MAX and refuses the next one up.
        assert!(take_count(b"9223372036854775807", "bytes", None).is_ok());
        assert!(take_count(b"9223372036854775808", "bytes", None).is_err());
        assert!(take_count(b"18446744073709551615", "bytes", None).is_err());
    }

    #[test]
    fn a_count_names_the_whole_remaining_text_when_it_fails() {
        // Every one of these strings is GNU's, measured.
        assert!(error(&["-i", ":2", "a", "b"]).starts_with("invalid --ignore-initial value ':2'"));
        assert!(error(&["-i", "2:", "a", "b"]).starts_with("invalid --ignore-initial value ''"));
        assert!(
            error(&["-i", "1:2:3", "a", "b"]).starts_with("invalid --ignore-initial value '2:3'")
        );
        assert!(error(&["-n", "-1", "a", "b"]).starts_with("invalid --bytes value '-1'"));
        assert!(error(&["-i", "x", "a", "b"]).starts_with("invalid --ignore-initial value 'x'"));
    }

    // ---------------------------------------------------- argument parsing ---

    #[test]
    fn the_second_operand_defaults_to_standard_input() {
        let set = settings(&["a"]);
        assert_eq!(set.names, [b"a".to_vec(), b"-".to_vec()]);
    }

    #[test]
    fn options_cluster_and_take_attached_values() {
        let set = settings(&["-bl", "a", "b"]);
        assert!(set.print_bytes && set.verbose);
        assert_eq!(settings(&["-n5", "a", "b"]).limit, 5);
        assert_eq!(settings(&["-i2", "a", "b"]).skip, [2, 2]);
    }

    #[test]
    fn options_may_follow_the_operands() {
        assert!(settings(&["a", "b", "-l"]).verbose);
    }

    #[test]
    fn print_chars_is_still_accepted_as_print_bytes() {
        assert!(settings(&["-c", "a", "b"]).print_bytes);
        assert!(settings(&["--print-chars", "a", "b"]).print_bytes);
    }

    #[test]
    fn a_single_skip_raises_the_second_but_never_lowers_it() {
        // GNU's `else if (ignore_initial[1] < ignore_initial[0])`. Measured:
        // `cmp -i 1:9 -i 5 a b` reports EOF on b, which only happens if b is
        // still skipping 9.
        assert_eq!(settings(&["-i", "1:9", "-i", "5", "a", "b"]).skip, [5, 9]);
        assert_eq!(settings(&["-i", "5", "a", "b"]).skip, [5, 5]);
        assert_eq!(settings(&["-i", "1:2", "a", "b"]).skip, [1, 2]);
    }

    #[test]
    fn repeated_bytes_takes_the_smaller() {
        assert_eq!(settings(&["-n", "10", "-n", "3", "a", "b"]).limit, 3);
        assert_eq!(settings(&["-n", "3", "-n", "10", "a", "b"]).limit, 3);
    }

    #[test]
    fn the_skip_operands_raise_the_option_rather_than_replacing_it() {
        // Measured with `cmp -l`, whose first row names the two bytes actually
        // reached and so reads back both skips at once. Against
        // `a=ABCDEFGHIJ`, `b=abcdefghij`:
        //
        //   cmp -l -i 5 a b 0 0    ->  `1 106 146`, i.e. a[5]='F', b[5]='f'
        //   cmp -l -i 5:6 a b 7 2  ->  `1 110 147`, i.e. a[7]='H', b[6]='g'
        //
        // Under an override the first would have read `1 101 141` (a[0], b[0]).
        assert_eq!(settings(&["-i", "5", "a", "b", "0", "0"]).skip, [5, 5]);
        assert_eq!(settings(&["-i", "5:6", "a", "b", "7", "2"]).skip, [7, 6]);
        assert_eq!(settings(&["-i", "5", "a", "b", "10", "10"]).skip, [10, 10]);
        assert_eq!(settings(&["a", "b", "2", "3"]).skip, [2, 3]);
    }

    #[test]
    fn a_lone_skip_operand_says_nothing_about_the_second_file() {
        // Unlike `-i 5`, which copies itself into the second slot, a bare
        // `SKIP1` operand leaves the second file alone: upstream's operand path
        // has no `else if`. Measured: `cmp -l a b 3 4` starts `1 104 145`
        // (a[3], b[4]) and `cmp a b 5` differs at byte 1, which is only true
        // with b unskipped.
        assert_eq!(settings(&["a", "b", "5"]).skip, [5, 0]);
    }

    #[test]
    fn a_later_option_never_lowers_a_slot() {
        // `cmp -l -i 5 -i 1:9 a b` reports `1 106 152` — a[5], b[9] — so the 5
        // survived the later 1 and the 9 replaced the 5. Both directions of the
        // maximum in one case.
        assert_eq!(settings(&["-i", "5", "-i", "1:9", "a", "b"]).skip, [5, 9]);
    }

    #[test]
    fn a_third_operand_that_is_not_a_number_is_a_skip_that_did_not_parse() {
        // GNU: `cmp a b c d e` complains about `c`, not about `e`.
        assert!(
            error(&["a", "b", "c", "d", "e"]).starts_with("invalid --ignore-initial value 'c'")
        );
        assert!(error(&["a", "b", "1", "2", "3"]).starts_with("extra operand '3'"));
    }

    #[test]
    fn verbose_and_quiet_are_refused_before_the_operands_are_looked_at() {
        let e = parse_args(&args(&["-l", "-s"]), b"-s").unwrap_err();
        assert!(e.sentence.starts_with("options -l and -s are incompatible"));
        assert_eq!(e.status, 2);
        // Even with a file named, and even with a file that does not exist.
        assert!(
            error(&["-s", "-l", "nosuchfile"]).starts_with("options -l and -s are incompatible")
        );
    }

    #[test]
    fn a_missing_operand_names_the_last_word_of_argv() {
        // Both lines carry the `cmp: ` prefix, because diffutils reaches the
        // referral through a second `error ()` call rather than coreutils' bare
        // `fprintf`. Measured against cmp 3.10 — the only reason this does not
        // simply print `getopt::Error`'s own `Display`.
        assert_eq!(
            printed(&[]),
            "cmp: missing operand after 'cmp'\ncmp: Try 'cmp --help' for more information."
        );
        let e = parse_args(&args(&["-s"]), b"-s").unwrap_err();
        assert_eq!(
            diagnostic(&e),
            "cmp: missing operand after '-s'\ncmp: Try 'cmp --help' for more information."
        );
    }

    #[test]
    fn a_bad_option_exits_two_not_one() {
        // Straight marks, as everywhere in this program: `getopt_long`'s own
        // sentences quote with `'`, and diffutils writes `'` literally into
        // every format string of its own. Nothing in `cmp` reaches gnulib's
        // locale-aware `quote()`, so nothing here ever turns curly.
        let e = parse_args(&args(&["-Z", "a", "b"]), b"b").unwrap_err();
        assert!(e.sentence.starts_with("invalid option -- 'Z'"));
        assert_eq!(e.status, 2);
        let e = parse_args(&args(&["--bogus", "a", "b"]), b"b").unwrap_err();
        assert_eq!(e.status, 2);
        assert_eq!(
            diagnostic(&e),
            "cmp: unrecognized option '--bogus'\ncmp: Try 'cmp --help' for more information."
        );
    }

    #[test]
    fn help_and_version_win_over_everything() {
        assert_eq!(parse_args(&args(&["--help"]), b"cmp"), Ok(Request::Help));
        assert_eq!(parse_args(&args(&["-v"]), b"cmp"), Ok(Request::Version));
        assert_eq!(
            parse_args(&args(&["--version"]), b"cmp"),
            Ok(Request::Version)
        );
    }

    #[test]
    fn a_non_utf8_operand_survives() {
        let name = coreutils::quote::os_from_bytes(b"a\xffb");
        let argv = vec![name, OsString::from("b")];
        let Ok(Request::Run(set)) = parse_args(&argv, b"b") else {
            panic!("expected a run")
        };
        // On a Unix host the bytes come through untouched. The Windows build
        // host has no way to hold them, which is a property of the host and
        // not of `cmp`.
        #[cfg(unix)]
        assert_eq!(set.names[0], b"a\xffb");
        #[cfg(not(unix))]
        assert!(set.names[0].starts_with(b"a"));
    }

    #[test]
    fn help_mentions_every_option_it_accepts() {
        let help = help_text();
        for flag in [
            "-b",
            "--print-bytes",
            "-i",
            "--ignore-initial",
            "-l",
            "--verbose",
            "-n",
            "--bytes",
            "-s",
            "--quiet",
            "--silent",
            "--help",
            "-v",
            "--version",
        ] {
            assert!(help.contains(flag), "help does not mention {flag}");
        }
        assert!(!help.contains("Report bugs"));
    }

    // ------------------------------------------------------------ renderers ---

    #[test]
    fn a_byte_is_rendered_the_way_cat_v_would() {
        // Every one of these was measured out of `cmp -bl`.
        assert_eq!(sprintc(b'A'), "A");
        assert_eq!(sprintc(0x01), "^A");
        assert_eq!(sprintc(0x09), "^I");
        assert_eq!(sprintc(0x00), "^@");
        assert_eq!(sprintc(0x20), " ");
        assert_eq!(sprintc(0x7e), "~");
        assert_eq!(sprintc(0x7f), "^?");
        assert_eq!(sprintc(0x80), "M-^@");
        assert_eq!(sprintc(0xfe), "M-~");
        assert_eq!(sprintc(0xff), "M-^?");
    }

    #[test]
    fn the_offset_column_is_as_wide_as_the_shorter_input() {
        // Two 100-byte files: three digits. Measured.
        assert_eq!(offset_width(COUNT_MAX, [Some(100), Some(100)]), 3);
        // 100 against 20000: the *smaller* decides.
        assert_eq!(offset_width(COUNT_MAX, [Some(100), Some(20000)]), 3);
        assert_eq!(offset_width(COUNT_MAX, [Some(20000), Some(20000)]), 5);
        // `-n 5` lowers it further.
        assert_eq!(offset_width(5, [Some(100), Some(100)]), 1);
        // Eight-byte files: one digit.
        assert_eq!(offset_width(COUNT_MAX, [Some(8), Some(8)]), 1);
        // A pipe contributes no bound; two of them leave the whole off_t.
        assert_eq!(offset_width(COUNT_MAX, [None, Some(3)]), 1);
        assert_eq!(offset_width(COUNT_MAX, [None, None]), 19);
        assert_eq!(offset_width(0, [None, None]), 1);
    }

    #[test]
    fn a_verbose_row_is_the_number_then_two_octals() {
        assert_eq!(verbose_row(1, 6, 0o145, 0o130, false), "6 145 130");
        assert_eq!(verbose_row(3, 1, 0, 0o340, false), "  1   0 340");
        assert_eq!(verbose_row(5, 1, 0, 0o115, false), "    1   0 115");
    }

    #[test]
    fn print_bytes_adds_a_padded_character_column() {
        // Measured, row for row, from `cmp -bl` on eight-byte inputs.
        assert_eq!(verbose_row(1, 1, b'A', b'B', true), "1 101 A    102 B");
        assert_eq!(verbose_row(1, 2, 0x01, 0x02, true), "2   1 ^A     2 ^B");
        assert_eq!(verbose_row(1, 3, 0x7f, 0x7e, true), "3 177 ^?   176 ~");
        assert_eq!(verbose_row(1, 4, 0x80, 0x81, true), "4 200 M-^@ 201 M-^A");
        assert_eq!(verbose_row(1, 5, 0xff, 0xfe, true), "5 377 M-^? 376 M-~");
        assert_eq!(verbose_row(1, 6, b' ', b'\t', true), "6  40       11 ^I");
        assert_eq!(verbose_row(1, 7, b'\\', b'/', true), "7 134 \\     57 /");
    }

    #[test]
    fn the_differ_line_says_byte_in_a_real_locale_and_char_in_the_c_one() {
        let names = [&b"a"[..], &b"b"[..]];
        assert_eq!(
            differ_line(names, 6, 2, None, true),
            "a b differ: byte 6, line 2"
        );
        assert_eq!(
            differ_line(names, 6, 2, None, false),
            "a b differ: char 6, line 2"
        );
        // With -b it is `byte` either way: upstream's combined format string
        // has no `char` spelling.
        assert_eq!(
            differ_line(names, 6, 2, Some((0o145, 0o130)), false),
            "a b differ: byte 6, line 2 is 145 e 130 X"
        );
    }

    #[test]
    fn a_name_that_could_forge_a_line_is_quoted_and_an_ordinary_one_is_not() {
        // The whole reason this diverges from GNU, which prints both raw.
        assert_eq!(
            differ_line([&b"sp ace"[..], b"nl\nname"], 1, 1, None, true),
            "'sp ace' 'nl'$'\\n''name' differ: byte 1, line 1"
        );
        assert_eq!(
            differ_line([&b"-"[..], b"plain.txt"], 1, 1, None, true),
            "- plain.txt differ: byte 1, line 1"
        );
    }

    #[test]
    fn the_eof_message_distinguishes_ending_at_a_line_from_ending_inside_one() {
        // "abc\n": four bytes, one newline, ends at a boundary.
        assert_eq!(
            eof_message(b"c", 4, 1, true, false),
            "cmp: EOF on c after byte 4, line 1"
        );
        // "abc\nd": five bytes, one newline, ends mid-line -> "in line 2".
        assert_eq!(
            eof_message(b"n3", 5, 1, false, false),
            "cmp: EOF on n3 after byte 5, in line 2"
        );
        // "ab": no newline at all.
        assert_eq!(
            eof_message(b"s1", 2, 0, false, false),
            "cmp: EOF on s1 after byte 2, in line 1"
        );
        // -l counts no lines, so it reports none.
        assert_eq!(
            eof_message(b"n2", 4, 1, true, true),
            "cmp: EOF on n2 after byte 4"
        );
        // Nothing was read at all.
        assert_eq!(
            eof_message(b"e", 0, 0, false, false),
            "cmp: EOF on e which is empty"
        );
        assert_eq!(
            eof_message(b"e", 0, 0, false, true),
            "cmp: EOF on e which is empty"
        );
    }

    // -------------------------------------------------------------- engine ---

    #[test]
    fn identical_inputs_compare_equal() {
        let set = Settings::default();
        assert_eq!(run(b"abc\ndef\n", b"abc\ndef\n", &set, 1).0, Outcome::Same);
        assert_eq!(run(b"", b"", &set, 1).0, Outcome::Same);
    }

    #[test]
    fn the_first_difference_carries_its_byte_and_line() {
        let set = Settings::default();
        assert_eq!(
            run(b"abc\ndef\n", b"abc\ndXf\n", &set, 1).0,
            Outcome::Diff {
                byte: 6,
                line: 2,
                a: b'e',
                b: b'X'
            }
        );
        // The line number counts the newlines *before* the differing byte, so
        // a difference that is itself a newline stays on the current line.
        assert_eq!(
            run(b"a\nb", b"a b", &set, 1).0,
            Outcome::Diff {
                byte: 2,
                line: 1,
                a: b'\n',
                b: b' '
            }
        );
    }

    #[test]
    fn a_shorter_input_reports_which_one_ended_and_where() {
        let set = Settings::default();
        assert_eq!(
            run(b"abc\ndef\n", b"abc\n", &set, 1).0,
            Outcome::Eof {
                which: 1,
                byte: 4,
                newlines: 1,
                at_line_start: true,
            }
        );
        // The other way round names the other input, and the message still
        // names the shorter file rather than a positional one.
        assert_eq!(
            run(b"abc\n", b"abc\ndef\n", &set, 1).0,
            Outcome::Eof {
                which: 0,
                byte: 4,
                newlines: 1,
                at_line_start: true,
            }
        );
        assert_eq!(
            run(b"abc\ndef\ngh", b"ab", &set, 1).0,
            Outcome::Eof {
                which: 1,
                byte: 2,
                newlines: 0,
                at_line_start: false,
            }
        );
    }

    #[test]
    fn verbose_mode_reports_every_difference_and_then_the_short_input() {
        let set = Settings {
            verbose: true,
            ..Settings::default()
        };
        let (outcome, rows) = run(b"aXcdef", b"aYc", &set, 1);
        assert_eq!(rows, "2 130 131\n");
        assert_eq!(
            outcome,
            Outcome::Eof {
                which: 1,
                byte: 3,
                newlines: 0,
                at_line_start: false,
            }
        );
    }

    #[test]
    fn verbose_mode_over_equal_lengths_ends_in_differed() {
        let set = Settings {
            verbose: true,
            ..Settings::default()
        };
        let (outcome, rows) = run(b"aXc\ndef\nghi\n", b"aYc\ndZf\nghi\n", &set, 1);
        assert_eq!(rows, "2 130 131\n6 145 132\n");
        assert_eq!(outcome, Outcome::Differed);
    }

    #[test]
    fn a_limit_stops_the_comparison_and_can_hide_a_difference() {
        let limited = |n: u64| Settings {
            limit: n,
            ..Settings::default()
        };
        // "aXc…" vs "aYc…": the difference is at byte 2.
        assert_eq!(run(b"aXc", b"aYc", &limited(1), 1).0, Outcome::Same);
        assert!(matches!(
            run(b"aXc", b"aYc", &limited(2), 1).0,
            Outcome::Diff { byte: 2, .. }
        ));
        assert_eq!(run(b"abc", b"abc", &limited(0), 1).0, Outcome::Same);
        // A limit reached exactly at the shorter input's end is not an EOF.
        assert_eq!(
            run(b"abc\ndef\n", b"abc\n", &limited(4), 1).0,
            Outcome::Same
        );
        assert!(matches!(
            run(b"abc\ndef\n", b"abc\n", &limited(5), 1).0,
            Outcome::Eof { byte: 4, .. }
        ));
    }

    #[test]
    fn a_difference_beyond_the_buffer_is_still_found() {
        // Crossing the 64 KiB refill boundary is where an off-by-one in the
        // running counters would show up, and nowhere smaller.
        let mut a = vec![b'.'; BUFFER * 2 + 7];
        let mut b = a.clone();
        a[10] = b'\n';
        b[10] = b'\n';
        let at = BUFFER + 3;
        b[at] = b'!';
        let set = Settings::default();
        assert_eq!(
            run(&a, &b, &set, 1).0,
            Outcome::Diff {
                byte: at as u64 + 1,
                line: 2,
                a: b'.',
                b: b'!'
            }
        );
    }

    #[test]
    fn an_input_that_ends_exactly_on_the_buffer_boundary_reports_that_length() {
        let a = vec![b'x'; BUFFER * 2];
        let b = vec![b'x'; BUFFER];
        let set = Settings::default();
        assert_eq!(
            run(&a, &b, &set, 1).0,
            Outcome::Eof {
                which: 1,
                byte: BUFFER as u64,
                newlines: 0,
                at_line_start: false,
            }
        );
    }
}
