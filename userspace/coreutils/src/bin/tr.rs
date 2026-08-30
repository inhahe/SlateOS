//! `tr` — translate, squeeze, and/or delete bytes.
//!
//! ```text
//! tr [-cCdst] [--] SET1 [SET2]
//! ```
//!
//! | option | effect |
//! |---|---|
//! | `-c`, `-C`, `--complement` | use the complement of SET1 |
//! | `-d`, `--delete` | delete the bytes in SET1 rather than translating |
//! | `-s`, `--squeeze-repeats` | collapse each run of a byte in the *last* SET |
//! | `-t`, `--truncate-set1` | cut SET1 down to SET2's length before translating |
//!
//! ## What this used to be
//!
//! `tr [-d] SET1 [SET2]`, with ranges and nothing else. Everything below was
//! missing, and each absence turns a working one-liner into either an error or,
//! worse, a plausible wrong answer:
//!
//! * **`-c`, `-s` and `-t` did not exist** and were not refused either — the
//!   option loop kept anything that was not exactly `-d` as a *set*, so
//!   `tr -s ' ' ' '` squeezed nothing, silently treated `-s` as SET1 and `' '`
//!   as SET2, and translated `-`, `s` and space into spaces. `tr -cd '[:print:]'`
//!   — the standard "strip control characters" line — deleted printable text
//!   instead of keeping it.
//! * **No escape sequences.** `tr -d '\n'` deleted backslashes and the letter
//!   `n`. `tr '\t' ' '` did nothing to tabs and rewrote every `t`.
//! * **No character classes.** `tr '[:upper:]' '[:lower:]'` mapped the eleven
//!   bytes of the literal text `[:upper:]` onto the eleven of `[:lower:]`, so
//!   `[` became `[`, `u` became `l`, and the letters it was asked about were
//!   untouched.
//! * **No `[c*n]` repeat and no `[=c=]` equivalence class**, both likewise read
//!   as literal punctuation.
//! * **No `--` and no long options**, so `tr -- -a -b` translated `-`.
//! * **A reversed range was accepted and silently reversed.** `expand_set`
//!   wrote `e-a` as `edcba`; GNU refuses it, because a reversed range is nearly
//!   always a typo for the forward one and quietly guessing costs the caller
//!   the diagnosis.
//! * **The whole of stdin was read into memory** before a byte came out, so
//!   `tr` could not be used in a pipeline with an endless producer — `tail -f
//!   log | tr -d '\r'` printed nothing, ever.
//! * **Every failure exited 0.** Both writes were `let _ = …`, so a full disk
//!   was reported as success.
//!
//! ## The SET grammar
//!
//! A SET is a byte string in which most bytes stand for themselves. The rest
//! is parsed exactly as upstream's `build_spec_list` does — which matters
//! because the constructs are recognised by *shape*, and a shape that does not
//! quite match is not an error but a run of ordinary bytes:
//!
//! | written | means | when the shape does not match |
//! |---|---|---|
//! | `\n`, `\t`, `\\`, … | the control byte | `\z` is `z`; a trailing `\` is a `\` and a warning |
//! | `\NNN` | 1–3 octal digits | `\400` is `\040` then `0`, and a warning |
//! | `A-Z` | the range, ascending | `Z-A` is an error, never a descending range |
//! | `[:alpha:]` | the class | `[:alpha:` is `[`, `:`, `a`, … |
//! | `[=a=]` | everything equal to `a` | `[=a` is `[`, `=`, `a` |
//! | `[a*3]`, `[a*]` | three `a`s; as many as SET1 is long | `[ab]` is four bytes |
//!
//! The literal-fallback column is not a nicety: `tr -d '[]'` deletes brackets,
//! and a parser that treated `[` as always-special would refuse it.
//!
//! ## Which SET each option reads
//!
//! Three different jobs read three different sets, and `-c` complements only
//! one of them:
//!
//! | command | delete | squeeze | translate |
//! |---|---|---|---|
//! | `tr S1 S2` | — | — | S1 → S2 |
//! | `tr -d S1` | S1 | — | — |
//! | `tr -s S1` | — | S1 | — |
//! | `tr -s S1 S2` | — | **S2** | S1 → S2 |
//! | `tr -d -s S1 S2` | S1 | **S2** | — |
//!
//! `-c` applies to SET1 only, in every row. So `tr -cs '[:alnum:]' '\n'`
//! reads: replace every non-alphanumeric byte with a newline, then squeeze
//! runs of newlines — and the squeeze set is the newline, not the complement.
//!
//! ## Padding, and why `-t` exists
//!
//! When translating, SET2 is padded to SET1's length by repeating its **last**
//! byte, so `tr 'a-z' 'x'` maps every lowercase letter to `x`. `-t` asks for
//! the other reading: cut SET1 down to SET2's length instead, so only the
//! first byte is touched. Upstream refuses an empty SET2 without `-t`
//! (`when not truncating set1, string2 must be non-empty`) because there is no
//! last byte to pad with.
//!
//! Where two entries of SET1 name the same byte, the **last** wins:
//! `tr '[a*3]b' 'XYZW'` maps `a` to `Z`, not to `X`.
//!
//! ## `[:upper:]` and `[:lower:]` must line up
//!
//! In SET2, while translating, the only classes allowed at all are `upper` and
//! `lower` — `tr 'a-c' '[:digit:]'` is refused rather than expanded, because
//! a class's order is only defined for those two. And an allowed one must sit
//! at the **same offset into the expanded set** as an `upper`/`lower` class in
//! SET1: `tr 'a-b[:lower:]' 'cd[:upper:]'` is fine (both start at offset 2)
//! while `tr 'a[:lower:]' 'bc[:upper:]'` is `misaligned [:upper:] and/or
//! [:lower:] construct`. Measured, not assumed — the check counts expanded
//! bytes, not list entries, which is why the range/pair pair above agrees.
//!
//! ## Checked against GNU
//!
//! `scripts/tr-diff.sh` runs this and glibc's `tr` over the same command lines
//! and compares stdout, stderr and the exit status byte for byte.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program};
use coreutils::quote::quote;
use coreutils::stdfd;
use std::env;
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::process::ExitCode;

const TR: Program = Program::new("tr", 1);

/// `\a`, which Rust has no escape for.
const BEL: u8 = 0x07;
/// `\b`.
const BACKSPACE: u8 = 0x08;
/// `\f`.
const FORM_FEED: u8 = 0x0C;
/// `\v`.
const VERTICAL_TAB: u8 = 0x0B;

/// The long options, **in GNU's declaration order** — which is observable,
/// because `getopt_long` lists an ambiguous prefix's candidates in it.
/// Measured with `tr --=x`.
const LONG_OPTIONS: &[(&str, Long)] = &[
    ("complement", Long::Complement),
    ("delete", Long::Delete),
    ("squeeze-repeats", Long::Squeeze),
    ("truncate-set1", Long::Truncate),
    ("help", Long::Help),
    ("version", Long::Version),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Long {
    Complement,
    Delete,
    Squeeze,
    Truncate,
    Help,
    Version,
}

/// The twelve POSIX character classes, expanded for the C locale.
///
/// Byte sets rather than `is*` calls: this is a byte-oriented utility on a
/// byte-oriented system, and the membership was **measured** against glibc
/// under both `C` and `C.UTF-8` (`tr -d -c '[:CLASS:]'` over bytes 1..255).
/// The two locales agreed on every class and no byte above 0x7F belonged to
/// any of them, which is what makes a fixed table honest rather than a
/// simplification.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Alnum,
    Alpha,
    Blank,
    Cntrl,
    Digit,
    Graph,
    Lower,
    Print,
    Punct,
    Space,
    Upper,
    Xdigit,
}

impl Class {
    fn from_name(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"alnum" => Self::Alnum,
            b"alpha" => Self::Alpha,
            b"blank" => Self::Blank,
            b"cntrl" => Self::Cntrl,
            b"digit" => Self::Digit,
            b"graph" => Self::Graph,
            b"lower" => Self::Lower,
            b"print" => Self::Print,
            b"punct" => Self::Punct,
            b"space" => Self::Space,
            b"upper" => Self::Upper,
            b"xdigit" => Self::Xdigit,
            _ => return None,
        })
    }

    /// The class's bytes, in ascending order — the order upstream's
    /// `card_of_char_class` walks, and the only one `[:lower:]`/`[:upper:]`
    /// pairing is defined for.
    fn bytes(self) -> Vec<u8> {
        let member = |b: u8| match self {
            Self::Alnum => b.is_ascii_alphanumeric(),
            Self::Alpha => b.is_ascii_alphabetic(),
            Self::Blank => b == b' ' || b == b'\t',
            Self::Cntrl => b < 0x20 || b == 0x7F,
            Self::Digit => b.is_ascii_digit(),
            Self::Graph => (0x21..=0x7E).contains(&b),
            Self::Lower => b.is_ascii_lowercase(),
            Self::Print => (0x20..=0x7E).contains(&b),
            Self::Punct => b.is_ascii_punctuation(),
            Self::Space => b == b' ' || (0x09..=0x0D).contains(&b),
            Self::Upper => b.is_ascii_uppercase(),
            Self::Xdigit => b.is_ascii_hexdigit(),
        };
        (0u8..=255).filter(|&b| member(b)).collect()
    }
}

/// How many copies `[c*n]` asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Repeat {
    /// `[c*]` or `[c*0]`: as many as it takes to reach SET1's length.
    Fill,
    Times(usize),
}

/// One entry of a parsed SET, before expansion.
///
/// The list is kept rather than expanded on the spot because three separate
/// rules need to see the *structure*: `[c*]` needs the other set's length,
/// `-t` truncates after expansion, and the `[:upper:]`/`[:lower:]` alignment
/// check needs to know which stretch of the expansion came from a class.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Spec {
    Char(u8),
    Range(u8, u8),
    Class(Class),
    /// `[=c=]`. In the C locale a byte is equivalent only to itself, so this
    /// expands to one byte — but it is kept distinct because SET2 refuses it
    /// while translating and a plain byte there is fine.
    Equiv(u8),
    Repeat(u8, Repeat),
}

impl Spec {
    /// How many bytes this contributes, or `None` for an unresolved `[c*]`.
    fn len(self) -> Option<usize> {
        Some(match self {
            Self::Char(_) | Self::Equiv(_) => 1,
            Self::Range(lo, hi) => usize::from(hi.saturating_sub(lo)).saturating_add(1),
            Self::Class(c) => c.bytes().len(),
            Self::Repeat(_, Repeat::Times(n)) => n,
            Self::Repeat(_, Repeat::Fill) => return None,
        })
    }
}

/// Which of the two operands a diagnostic is about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Which {
    One,
    Two,
}

/// A command line that cannot be run.
///
/// Two variants because upstream refuses one in two different places. A getopt
/// or operand-count complaint ends in `Try 'tr --help' for more information.`;
/// everything the SET parser rejects is an `error (EXIT_FAILURE, …)` on the
/// spot, with no referral.
#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    Getopt(getopt::Error),
    /// A sentence printed as `tr: <text>`, with no referral.
    Plain(String),
    /// `missing operand` / `extra operand` and their explanatory second line,
    /// which *does* carry the referral.
    Operands {
        first: String,
        second: Option<String>,
    },
}

impl Refusal {
    fn report(&self) -> ExitCode {
        match self {
            Self::Getopt(e) => {
                diag!("tr: {}", e.message());
                ExitCode::from(u8::try_from(e.status).unwrap_or(1))
            }
            Self::Plain(text) => {
                diag!("tr: {text}");
                ExitCode::FAILURE
            }
            Self::Operands { first, second } => {
                diag!("tr: {first}");
                if let Some(second) = second {
                    diag!("{second}");
                }
                diag!("Try 'tr --help' for more information.");
                ExitCode::FAILURE
            }
        }
    }
}

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    Run(Job),
    Help,
    Version,
}

/// The options, before the SETs are looked at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Flags {
    complement: bool,
    delete: bool,
    squeeze: bool,
    truncate: bool,
}

/// The work to do, with every set already resolved to bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Job {
    /// The byte-to-byte map, or `None` when not translating.
    table: Option<Box<[u8; 256]>>,
    /// Bytes to drop, or `None` when not deleting.
    delete: Option<Box<[bool; 256]>>,
    /// Bytes whose runs collapse, or `None` when not squeezing.
    squeeze: Option<Box<[bool; 256]>>,
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(refusal) => return refusal.report(),
    };

    let job = match request {
        Request::Help => {
            print!("{}", help_text());
            return ExitCode::SUCCESS;
        }
        Request::Version => {
            println!("tr (SlateOS coreutils) 0.1.0");
            return ExitCode::SUCCESS;
        }
        Request::Run(job) => job,
    };

    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    match stream(&job, &mut input, &mut out) {
        Ok(()) => {}
        Err(trouble) => return trouble.report(),
    }
    // Buffered output has to reach the OS before success can be claimed; a
    // flush that fails here is a truncated file reported as a complete one.
    if let Err(e) = out.flush() {
        return Trouble::Write(e).report();
    }
    ExitCode::SUCCESS
}

/// GNU's `--help`, byte for byte, minus the trailing block of URLs that names
/// the GNU project's own bug addresses and an `info` manual this does not ship.
///
/// Upstream says "ARRAY" where the synopsis and every diagnostic say "STRING";
/// that inconsistency is GNU's and is reproduced rather than tidied, because a
/// script that greps this text greps the text that exists.
fn help_text() -> String {
    "\
Usage: tr [OPTION]... STRING1 [STRING2]
Translate, squeeze, and/or delete characters from standard input,
writing to standard output.  STRING1 and STRING2 specify arrays of
characters ARRAY1 and ARRAY2 that control the action.

  -c, -C, --complement    use the complement of ARRAY1
  -d, --delete            delete characters in ARRAY1, do not translate
  -s, --squeeze-repeats   replace each sequence of a repeated character
                            that is listed in the last specified ARRAY,
                            with a single occurrence of that character
  -t, --truncate-set1     first truncate ARRAY1 to length of ARRAY2
      --help        display this help and exit
      --version     output version information and exit

ARRAYs are specified as strings of characters.  Most represent themselves.
Interpreted sequences are:

  \\NNN            character with octal value NNN (1 to 3 octal digits)
  \\\\              backslash
  \\a              audible BEL
  \\b              backspace
  \\f              form feed
  \\n              new line
  \\r              return
  \\t              horizontal tab
  \\v              vertical tab
  CHAR1-CHAR2     all characters from CHAR1 to CHAR2 in ascending order
  [CHAR*]         in ARRAY2, copies of CHAR until length of ARRAY1
  [CHAR*REPEAT]   REPEAT copies of CHAR, REPEAT octal if starting with 0
  [:alnum:]       all letters and digits
  [:alpha:]       all letters
  [:blank:]       all horizontal whitespace
  [:cntrl:]       all control characters
  [:digit:]       all digits
  [:graph:]       all printable characters, not including space
  [:lower:]       all lower case letters
  [:print:]       all printable characters, including space
  [:punct:]       all punctuation characters
  [:space:]       all horizontal or vertical whitespace
  [:upper:]       all upper case letters
  [:xdigit:]      all hexadecimal digits
  [=CHAR=]        all characters which are equivalent to CHAR

Translation occurs if -d is not given and both STRING1 and STRING2 appear.
-t is only significant when translating.  ARRAY2 is extended to length of
ARRAY1 by repeating its last character as necessary.  Excess characters
of ARRAY2 are ignored.  Character classes expand in unspecified order;
while translating, [:lower:] and [:upper:] may be used in pairs to
specify case conversion.  Squeezing occurs after translation or deletion.
"
    .to_string()
}

/// A failure that ends the run.
#[derive(Debug)]
enum Trouble {
    Read(io::Error),
    Write(io::Error),
}

impl Trouble {
    fn report(&self) -> ExitCode {
        match self {
            Self::Read(e) => diag!("tr: read error: {}", strerror(e)),
            Self::Write(e) => diag!("tr: write error: {}", strerror(e)),
        }
        ExitCode::FAILURE
    }
}

// --------------------------------------------------------------------- filter

/// The squeeze state that has to survive a buffer boundary.
///
/// A run of repeated bytes does not respect the size of whatever chunk the
/// reader happened to hand over, so the last byte emitted is carried across.
/// `None` means nothing has been emitted yet, which is distinct from "the last
/// byte was 0".
#[derive(Debug, Default)]
struct Squeezer {
    last: Option<u8>,
}

/// Read `input` to EOF, transform it, and write the result.
///
/// Streaming rather than slurping: `tr` is a filter, and a filter that waits
/// for EOF cannot sit in a pipeline behind an endless producer.
fn stream(job: &Job, input: &mut impl Read, out: &mut impl Write) -> Result<(), Trouble> {
    let mut buf = [0u8; 64 * 1024];
    let mut staging: Vec<u8> = Vec::with_capacity(buf.len());
    let mut squeezer = Squeezer::default();
    loop {
        let got = match input.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Trouble::Read(e)),
        };
        staging.clear();
        let chunk = buf.get(..got).unwrap_or_default();
        transform(job, chunk, &mut squeezer, &mut staging);
        out.write_all(&staging).map_err(Trouble::Write)?;
    }
}

/// Apply delete, then translate, then squeeze — upstream's order, and the one
/// the option table's "last specified set" wording only makes sense in.
fn transform(job: &Job, chunk: &[u8], squeezer: &mut Squeezer, out: &mut Vec<u8>) {
    for &byte in chunk {
        if let Some(drop) = job.delete.as_ref()
            && *drop.get(usize::from(byte)).unwrap_or(&false)
        {
            continue;
        }
        let byte = match job.table.as_ref() {
            Some(table) => *table.get(usize::from(byte)).unwrap_or(&byte),
            None => byte,
        };
        if let Some(squeeze) = job.squeeze.as_ref() {
            let repeatable = *squeeze.get(usize::from(byte)).unwrap_or(&false);
            if repeatable && squeezer.last == Some(byte) {
                continue;
            }
            squeezer.last = Some(byte);
        }
        out.push(byte);
    }
}

// ---------------------------------------------------------------- the SET set

/// Parse one SET string into its spec list.
///
/// `which` only selects the wording of the two diagnostics that name a side.
#[allow(clippy::too_many_lines)] // One `match` over the grammar; splitting it
// would scatter the literal-fallback rule.
fn parse_set(s: &[u8], which: Which) -> Result<Vec<Spec>, Refusal> {
    let mut specs: Vec<Spec> = Vec::new();
    // The decoded bytes, each flagged with whether an escape produced it: an
    // escaped `-` is not a range, and an escaped `*` does not open a repeat.
    // Upstream carries the same flag array (`struct E_string`).
    let (decoded, escaped) = unescape(s);

    let mut at = 0usize;
    while at < decoded.len() {
        let byte = *decoded.get(at).unwrap_or(&0);
        let is_escaped = *escaped.get(at).unwrap_or(&false);

        if byte == b'['
            && !is_escaped
            && let Some((spec, next)) = bracketed(&decoded, &escaped, at, which)?
        {
            specs.push(spec);
            at = next;
            continue;
        }

        // A range is `X-Y` where the `-` is not itself escaped and something
        // follows it. `a-` at the end of a set is three ordinary bytes.
        let dash = at.saturating_add(1);
        let end = at.saturating_add(2);
        if decoded.get(dash) == Some(&b'-')
            && !*escaped.get(dash).unwrap_or(&false)
            && end < decoded.len()
        {
            let hi = *decoded.get(end).unwrap_or(&0);
            if byte > hi {
                return Err(Refusal::Plain(format!(
                    "range-endpoints of '{}-{}' are in reverse collating sequence order",
                    printable_char(byte),
                    printable_char(hi)
                )));
            }
            specs.push(Spec::Range(byte, hi));
            at = end.saturating_add(1);
            continue;
        }

        specs.push(Spec::Char(byte));
        at = at.saturating_add(1);
    }
    Ok(specs)
}

/// Try to read a bracketed construct starting at `open`.
///
/// `Ok(None)` means the shape did not match, so the `[` is an ordinary byte —
/// the fallback that lets `tr -d '[]'` mean what it says.
fn bracketed(
    decoded: &[u8],
    escaped: &[bool],
    open: usize,
    which: Which,
) -> Result<Option<(Spec, usize)>, Refusal> {
    let after = open.saturating_add(1);
    match decoded.get(after) {
        Some(b':') if !*escaped.get(after).unwrap_or(&false) => {
            let Some(close) = find_closing(decoded, escaped, after.saturating_add(1), b':') else {
                return Ok(None);
            };
            let name = decoded
                .get(after.saturating_add(1)..close)
                .unwrap_or_default();
            if name.is_empty() {
                // `'[::]'` is spelled out with ASCII apostrophes rather than
                // run through `quote`, because upstream baked it into the
                // translatable string — `tr.c` says
                // `_("missing character class name '[::]'")` — instead of
                // quoting a runtime value. That text is therefore *not* locale
                // sensitive in GNU, and stays straight even where §351's marks
                // are curly.
                //
                // This is a per-message choice upstream made, not a rule about
                // constants: GNU's `test` puts its constant `]` through
                // `quote` and does come out curly, which is why `test.rs` is
                // right to keep doing so. Measurement is the only oracle here.
                // Measured against GNU 9.4 under `LC_ALL=C.UTF-8`, and pinned
                // by `tr-diff.sh` and the unit test below.
                return Err(Refusal::Plain(
                    "missing character class name '[::]'".to_string(),
                ));
            }
            let Some(class) = Class::from_name(name) else {
                // Both renderers, in this order: upstream passes the name
                // through `make_printable_str` and then hands *that text* to
                // `quote`, so an unprintable byte comes out as `\\377` — the
                // backslash the first pass wrote, escaped by the second.
                return Err(Refusal::Plain(format!(
                    "invalid character class {}",
                    quote(printable_str(name).as_bytes())
                )));
            };
            Ok(Some((Spec::Class(class), close.saturating_add(2))))
        }
        Some(b'=') if !*escaped.get(after).unwrap_or(&false) => {
            let Some(close) = find_closing(decoded, escaped, after.saturating_add(1), b'=') else {
                return Ok(None);
            };
            let body = decoded
                .get(after.saturating_add(1)..close)
                .unwrap_or_default();
            match body {
                // Straight apostrophes, for the same reason as `'[::]'` above:
                // upstream's string is `_("missing equivalence class
                // character '[==]'")`, quotes included.
                [] => Err(Refusal::Plain(
                    "missing equivalence class character '[==]'".to_string(),
                )),
                [one] => Ok(Some((Spec::Equiv(*one), close.saturating_add(2)))),
                // This one is `make_printable_str` *alone* — no `quote`, so an
                // apostrophe operand reports as a bare `''`.
                many => Err(Refusal::Plain(format!(
                    "{}: equivalence class operand must be a single character",
                    printable_str(many)
                ))),
            }
        }
        Some(_) => {
            // `[c*n]`: a byte, an unescaped `*`, then everything up to the
            // first unescaped `]`. Measured, rather than assumed to be digits:
            // `tr 'ab' '[b*x]'` is `invalid repeat count 'x'`, not four
            // literal bytes, so the count is scanned as a *field* and then
            // validated — while `[ab]` and `[a*` are literal because the shape
            // itself never matched.
            let star = open.saturating_add(2);
            if decoded.get(star) != Some(&b'*') || *escaped.get(star).unwrap_or(&false) {
                return Ok(None);
            }
            // The scan stops at the first `]` — but it *aborts* at the first
            // escaped byte of any kind, which is not the same rule and is
            // observable: `[x*a\b]` is six literal bytes, because the escaped
            // backspace ends the search before any `]` is reached. Upstream's
            // loop is `for (i = start + 2; i < len && !es->escaped[i]; i++)`.
            let from = star.saturating_add(1);
            let mut found = None;
            for i in from..decoded.len() {
                if *escaped.get(i).unwrap_or(&false) {
                    break;
                }
                if decoded.get(i) == Some(&b']') {
                    found = Some(i);
                    break;
                }
            }
            let Some(close) = found else {
                return Ok(None);
            };
            let digits = decoded.get(from..close).unwrap_or_default();
            let count = repeat_count(digits)?;
            if which == Which::One && count == Repeat::Fill {
                return Err(Refusal::Plain(
                    "the [c*] repeat construct may not appear in string1".to_string(),
                ));
            }
            let byte = *decoded.get(after).unwrap_or(&0);
            Ok(Some((Spec::Repeat(byte, count), close.saturating_add(1))))
        }
        None => Ok(None),
    }
}

/// Upstream's `make_printable_char`: a byte that prints in the C locale stands
/// for itself, anything else becomes three octal digits.
///
/// **Not** the shared `quote` module, and not interchangeable with it. `quote`
/// escapes `'` and `\` because in its output they are punctuation; this does
/// not, because the message it feeds has already supplied the quotes. Measured:
/// a reversed range from `` ` `` to `'` reports ``range-endpoints of '`-''``,
/// with the apostrophe bare. It also, unlike [`printable_str`], never uses a
/// named escape — `tr '\012-\011' x` says `'\012-\011'`, not `'\n-\t'`.
fn printable_char(b: u8) -> String {
    if (0x20..0x7f).contains(&b) {
        return (b as char).to_string();
    }
    format!("\\{:03o}", b)
}

/// Upstream's `make_printable_str`: like [`printable_char`] per byte, except
/// that the seven bytes C gives a name to get the name.
///
/// Measured against `[=…=]` with an operand of `\007\010\013\014\015\177`,
/// which reports `\a\b\v\f\r\177` — named where C names them, octal where it
/// does not.
fn printable_str(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        let named = match b {
            BEL => 'a',
            BACKSPACE => 'b',
            b'\t' => 't',
            b'\n' => 'n',
            VERTICAL_TAB => 'v',
            FORM_FEED => 'f',
            b'\r' => 'r',
            _ => {
                out.push_str(&printable_char(b));
                continue;
            }
        };
        out.push('\\');
        out.push(named);
    }
    out
}

/// Find the `X]` that closes a `[X…X]` construct, where `X` is `:` or `=`.
fn find_closing(decoded: &[u8], escaped: &[bool], from: usize, marker: u8) -> Option<usize> {
    (from..decoded.len().saturating_sub(1)).find(|&i| {
        decoded.get(i) == Some(&marker)
            && decoded.get(i.saturating_add(1)) == Some(&b']')
            && !*escaped.get(i).unwrap_or(&false)
    })
}

/// `[c*n]`'s `n`: empty or zero means "fill", a leading `0` means octal.
///
/// The grammar is `strtoumax`'s, not "a string of digits", because upstream
/// hands the field straight to `xstrtoumax` — and that costs two surprises
/// which are measured, not deduced:
///
/// * **Leading whitespace and a leading `+` are accepted**, since `strtoumax`
///   skips the one and consumes the other. `[x* 1]` and `[x*+1]` are both a
///   count of one. `[x*+ 1]` is not: the sign must come after the space, never
///   before it. A `-` is refused outright by gnulib before `strtoumax` sees it.
/// * **The base is chosen from the field's raw first byte**, which need not be
///   the first *digit*. `[x*010]` is octal 8; `[x* 010]` is decimal 10, because
///   the first byte is a space. `[x*08]` is an error and `[x* 08]` is eight.
fn repeat_count(digits: &[u8]) -> Result<Repeat, Refusal> {
    if digits.is_empty() {
        return Ok(Repeat::Fill);
    }
    // Upstream: `xstrtoumax (digit_str, nullptr, *digit_str == '0' ? 8 : 10, …)`.
    let radix = if digits.first() == Some(&b'0') { 8 } else { 10 };

    // `strtoumax`'s own prefix: whitespace, then at most one sign.
    let body = digits
        .iter()
        .position(|b| !matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
        .map_or(&[][..], |i| digits.get(i..).unwrap_or_default());
    let body = match body.first() {
        Some(b'+') => body.get(1..).unwrap_or_default(),
        _ => body,
    };

    let value = std::str::from_utf8(body)
        .ok()
        .filter(|t| !t.is_empty() && t.bytes().all(|b| (b'0'..b'0' + radix).contains(&b)))
        .and_then(|t| usize::from_str_radix(t, u32::from(radix)).ok());
    match value {
        Some(0) => Ok(Repeat::Fill),
        Some(n) => Ok(Repeat::Times(n)),
        None => Err(Refusal::Plain(format!(
            "invalid repeat count {} in [c*n] construct",
            quote(printable_str(digits).as_bytes())
        ))),
    }
}

/// Decode the backslash escapes, remembering which bytes an escape produced.
///
/// The flag is what makes `\-` a hyphen rather than a range and `\*` a star
/// rather than a repeat, so it has to come out of the same pass.
fn unescape(s: &[u8]) -> (Vec<u8>, Vec<bool>) {
    let mut out = Vec::with_capacity(s.len());
    let mut flags = Vec::with_capacity(s.len());
    let mut at = 0usize;
    while at < s.len() {
        let byte = *s.get(at).unwrap_or(&0);
        if byte != b'\\' {
            out.push(byte);
            flags.push(false);
            at = at.saturating_add(1);
            continue;
        }
        let Some(&next) = s.get(at.saturating_add(1)) else {
            // Upstream warns and keeps the backslash. It is a warning and not
            // an error because a lone `\` is what a shell hands over when the
            // user quoted one layer too few, and refusing would break scripts
            // that have "worked" for decades.
            diag!("tr: warning: an unescaped backslash at end of string is not portable");
            out.push(b'\\');
            flags.push(false);
            at = at.saturating_add(1);
            continue;
        };
        if next.is_ascii_digit() && next < b'8' {
            let (byte, used) = octal(s, at.saturating_add(1));
            out.push(byte);
            flags.push(true);
            at = at.saturating_add(1).saturating_add(used);
            continue;
        }
        let decoded = match next {
            b'a' => BEL,
            b'b' => BACKSPACE,
            b'f' => FORM_FEED,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => VERTICAL_TAB,
            // `\\` and everything else: the byte itself. Upstream says nothing
            // about `\z`, so neither does this — it is `z`, quietly.
            other => other,
        };
        out.push(decoded);
        flags.push(true);
        at = at.saturating_add(2);
    }
    (out, flags)
}

/// Read up to three octal digits at `from`, returning the byte and the count
/// of digits consumed.
///
/// Three digits can name a value above 255. Upstream does not truncate and
/// does not refuse: it backs off to two digits, leaves the third as a literal
/// digit, and says so — `\400` is a space followed by `0`. Guessing either
/// other way would silently change what a script deletes.
fn octal(s: &[u8], from: usize) -> (u8, usize) {
    let mut value: u32 = 0;
    let mut used = 0usize;
    while used < 3 {
        let Some(&d) = s.get(from.saturating_add(used)) else {
            break;
        };
        if !d.is_ascii_digit() || d >= b'8' {
            break;
        }
        value = value.saturating_mul(8).saturating_add(u32::from(d - b'0'));
        used = used.saturating_add(1);
    }
    if value > 255 && used == 3 {
        let third = s.get(from.saturating_add(2)).copied().unwrap_or(b'0');
        let two = value / 8;
        diag!(
            "tr: warning: the ambiguous octal escape \\{}{}{} is being\n\tinterpreted as the 2-byte sequence \\0{:o}, {}",
            char::from(s.get(from).copied().unwrap_or(b'0')),
            char::from(s.get(from.saturating_add(1)).copied().unwrap_or(b'0')),
            char::from(third),
            two,
            char::from(third),
        );
        return (u8::try_from(two).unwrap_or(0), 2);
    }
    (u8::try_from(value).unwrap_or(0), used)
}

// ------------------------------------------------------------------ expansion

/// Expand a spec list to bytes, resolving `[c*]` against `fill_to`.
fn expand(specs: &[Spec], fill_to: Option<usize>) -> Vec<u8> {
    // A `[c*]` fills whatever is left over once every other spec is counted.
    let fixed: usize = specs
        .iter()
        .map(|s| s.len().unwrap_or(0))
        .fold(0usize, usize::saturating_add);
    let fill = fill_to.map_or(0, |want| want.saturating_sub(fixed));

    // SET2 is never consulted past SET1's length — `--help` says "Excess
    // characters of ARRAY2 are ignored" — and upstream reaches it through a
    // generator that simply stops, so a count it never gets to costs nothing.
    // Materialising past the cap is what made `tr a '[x*4294967296]'` a 4 GiB
    // allocation and a 50-second wait here while GNU returned in 0.3s. SET1 has
    // no cap because there is nothing to bound it by; GNU grinds through a huge
    // SET1 too, and matching that is the point.
    let cap = fill_to.unwrap_or(usize::MAX);

    let mut out: Vec<u8> = Vec::new();
    for spec in specs {
        let room = cap.saturating_sub(out.len());
        if room == 0 {
            break;
        }
        match *spec {
            Spec::Char(b) | Spec::Equiv(b) => out.push(b),
            Spec::Range(lo, hi) => out.extend((lo..=hi).take(room)),
            Spec::Class(c) => out.extend(c.bytes().into_iter().take(room)),
            Spec::Repeat(b, Repeat::Times(n)) => out.extend(std::iter::repeat_n(b, n.min(room))),
            Spec::Repeat(b, Repeat::Fill) => out.extend(std::iter::repeat_n(b, fill.min(room))),
        }
    }
    out
}

/// The complement of a byte set, ascending — upstream's only defined order for
/// one, and the order `-c` with a translation depends on.
fn complement_of(set: &[u8]) -> Vec<u8> {
    let mut present = [false; 256];
    for &b in set {
        if let Some(slot) = present.get_mut(usize::from(b)) {
            *slot = true;
        }
    }
    (0u8..=255)
        .filter(|&b| !*present.get(usize::from(b)).unwrap_or(&false))
        .collect()
}

fn membership(set: &[u8]) -> Box<[bool; 256]> {
    let mut present = Box::new([false; 256]);
    for &b in set {
        if let Some(slot) = present.get_mut(usize::from(b)) {
            *slot = true;
        }
    }
    present
}

// ----------------------------------------------------------------- validation

/// Where each `[:upper:]`/`[:lower:]` sits in a set's *expanded* bytes.
///
/// Offsets, not list positions: `tr 'a-b[:lower:]' 'cd[:upper:]'` is accepted
/// because both classes begin at expanded offset 2, though one set reaches
/// there in one spec and the other in two.
fn case_class_offsets(specs: &[Spec]) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut at = 0usize;
    for spec in specs {
        if matches!(*spec, Spec::Class(Class::Upper | Class::Lower)) {
            offsets.push(at);
        }
        at = at.saturating_add(spec.len().unwrap_or(0));
    }
    offsets
}

/// The two rules SET2 obeys only while translating.
fn validate_set2_classes(set1: &[Spec], set2: &[Spec]) -> Result<(), Refusal> {
    if set2
        .iter()
        .any(|s| matches!(*s, Spec::Class(c) if c != Class::Upper && c != Class::Lower))
    {
        return Err(Refusal::Plain(
            "when translating, the only character classes that may appear in\nstring2 are 'upper' and 'lower'"
                .to_string(),
        ));
    }
    if set2.iter().any(|s| matches!(*s, Spec::Equiv(_))) {
        return Err(Refusal::Plain(
            "[=c=] expressions may not appear in string2 when translating".to_string(),
        ));
    }
    let ours = case_class_offsets(set2);
    let theirs = case_class_offsets(set1);
    if ours.iter().any(|at| !theirs.contains(at)) {
        return Err(Refusal::Plain(
            "misaligned [:upper:] and/or [:lower:] construct".to_string(),
        ));
    }
    Ok(())
}

// -------------------------------------------------------------- the whole job

/// Turn the flags and the operands into the three tables the filter needs.
fn build_job(flags: Flags, operands: &[Vec<u8>]) -> Result<Job, Refusal> {
    let translating = !flags.delete && operands.len() == 2;

    let specs1 = parse_set(
        operands.first().map(Vec::as_slice).unwrap_or_default(),
        Which::One,
    )?;
    let specs2 = match operands.get(1) {
        Some(s) => Some(parse_set(s, Which::Two)?),
        None => None,
    };

    if translating && let Some(specs2) = specs2.as_deref() {
        validate_set2_classes(&specs1, specs2)?;
    }

    let mut set1 = expand(&specs1, None);
    if flags.complement {
        set1 = complement_of(&set1);
    }

    // `[c*]` in SET2 fills up to SET1's length, so SET1 has to be final first.
    let set2 = specs2.as_deref().map(|s| expand(s, Some(set1.len())));

    if flags.truncate
        && let Some(set2) = set2.as_deref()
    {
        set1.truncate(set2.len());
    }

    let table = if translating {
        let set2 = set2.clone().unwrap_or_default();
        // An empty SET2 is refused only when there is something to translate.
        // `tr '' ''` and `tr '' x` are both accepted and pass input through,
        // and the length compared against is SET1's *after* complementing —
        // `tr -c '' ''` is refused, because the complement of nothing is every
        // byte there is.
        if set2.is_empty() && !set1.is_empty() && !flags.truncate {
            return Err(Refusal::Plain(
                "when not truncating set1, string2 must be non-empty".to_string(),
            ));
        }
        Some(translate_table(&set1, &set2))
    } else {
        None
    };

    let delete = flags.delete.then(|| membership(&set1));

    // "The last specified set": SET2 when there is one, SET1 otherwise — and
    // SET1 there is the complemented one, which is why `tr -cs '[:alnum:]'`
    // squeezes runs of punctuation rather than runs of letters.
    let squeeze = flags.squeeze.then(|| match set2.as_deref() {
        Some(set2) => membership(set2),
        None => membership(&set1),
    });

    Ok(Job {
        table,
        delete,
        squeeze,
    })
}

/// SET1 → SET2, with SET2 padded by its last byte and later entries winning.
fn translate_table(set1: &[u8], set2: &[u8]) -> Box<[u8; 256]> {
    let mut table = Box::new([0u8; 256]);
    for (i, slot) in table.iter_mut().enumerate() {
        *slot = u8::try_from(i).unwrap_or(0);
    }
    let last = set2.last().copied();
    for (i, &from) in set1.iter().enumerate() {
        let Some(to) = set2.get(i).copied().or(last) else {
            continue;
        };
        if let Some(slot) = table.get_mut(usize::from(from)) {
            *slot = to;
        }
    }
    table
}

// -------------------------------------------------------------- the arguments

fn parse_args(args: &[OsString]) -> Result<Request, Refusal> {
    let mut flags = Flags::default();
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let mut only_operands = false;
    let mut at = 0usize;

    while let Some(arg) = args.get(at) {
        at = at.saturating_add(1);
        let bytes = arg_bytes(arg);
        if only_operands {
            operands.push(bytes);
            continue;
        }

        if bytes == b"--" {
            only_operands = true;
        } else if bytes == b"-" || bytes.first() != Some(&b'-') {
            // A lone `-` is not standard input here: `tr` has no file
            // operands, so it is a one-byte SET naming the hyphen.
            operands.push(bytes);
        } else if let Some(body) = bytes.strip_prefix(b"--") {
            if let Some(request) = long_option(body, &bytes, &mut flags)? {
                return Ok(request);
            }
        } else {
            short_options(&bytes, &mut flags)?;
        }
    }

    let job = check_operands(flags, &operands)?;
    Ok(Request::Run(job))
}

/// The operand-count rules, whose second lines explain *why* the count is
/// wrong rather than repeating that it is.
fn check_operands(flags: Flags, operands: &[Vec<u8>]) -> Result<Job, Refusal> {
    let nth = |i: usize| quote(operands.get(i).map(Vec::as_slice).unwrap_or_default());

    if operands.is_empty() {
        return Err(Refusal::Operands {
            first: "missing operand".to_string(),
            second: None,
        });
    }

    // Deleting *without* squeezing is the one mode that takes a single set, so
    // it is the one mode whose first-too-many operand is the second and not the
    // third. Measured: `tr -d a b c` names 'b', while `tr -s a b c` names 'c'.
    if flags.delete && !flags.squeeze {
        if operands.len() > 1 {
            return Err(Refusal::Operands {
                first: format!("extra operand {}", nth(1)),
                // "Only one string may be given" answers a two-operand mistake
                // and would not explain a four-operand one, so upstream attaches
                // it only at exactly two. Measured: `tr -d a b c` omits it.
                second: (operands.len() == 2).then(|| {
                    "Only one string may be given when deleting without squeezing repeats."
                        .to_string()
                }),
            });
        }
    } else if operands.len() > 2 {
        return Err(Refusal::Operands {
            first: format!("extra operand {}", nth(2)),
            second: None,
        });
    } else if operands.len() == 1 {
        // A single set is enough for squeezing alone. Everything still in this
        // branch — translating, or deleting *and* squeezing — needs two.
        if flags.delete || !flags.squeeze {
            let why = if flags.delete {
                "Two strings must be given when both deleting and squeezing repeats."
            } else {
                "Two strings must be given when translating."
            };
            return Err(Refusal::Operands {
                first: format!("missing operand after {}", nth(0)),
                second: Some(why.to_string()),
            });
        }
    }

    build_job(flags, operands)
}

/// One `--name` or `--name=value` argument.
fn long_option(body: &[u8], whole: &[u8], flags: &mut Flags) -> Result<Option<Request>, Refusal> {
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(at) => (
            body.get(..at).unwrap_or_default(),
            Some(body.get(at.saturating_add(1)..).unwrap_or_default()),
        ),
        None => (body, None),
    };
    let typed =
        std::str::from_utf8(typed).map_err(|_| Refusal::Getopt(TR.unrecognized_option(whole)))?;
    let (name, which) = TR
        .resolve_long(typed, whole, LONG_OPTIONS)
        .map_err(Refusal::Getopt)?;

    // Every one of `tr`'s long options is a flag, so any `=value` is refused.
    if inline.is_some() {
        return Err(Refusal::Getopt(TR.long_unwanted_argument(name)));
    }
    match which {
        Long::Complement => flags.complement = true,
        Long::Delete => flags.delete = true,
        Long::Squeeze => flags.squeeze = true,
        Long::Truncate => flags.truncate = true,
        Long::Help => return Ok(Some(Request::Help)),
        Long::Version => return Ok(Some(Request::Version)),
    }
    Ok(None)
}

/// One `-abc` cluster. Bytes, not `char`s, so `-é` reports the byte typed.
fn short_options(bytes: &[u8], flags: &mut Flags) -> Result<(), Refusal> {
    for &c in bytes.get(1..).unwrap_or_default() {
        match c {
            b'c' | b'C' => flags.complement = true,
            b'd' => flags.delete = true,
            b's' => flags.squeeze = true,
            b't' => flags.truncate = true,
            _ => return Err(Refusal::Getopt(TR.invalid_option(c))),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn arg_bytes(arg: &OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    arg.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn arg_bytes(arg: &OsString) -> Vec<u8> {
    arg.to_string_lossy().into_owned().into_bytes()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Run `tr` with the given command line over `input`.
    fn run(argv: &[&str], input: &[u8]) -> Vec<u8> {
        let args: Vec<OsString> = argv.iter().map(OsString::from).collect();
        let Request::Run(job) = parse_args(&args).expect("command line should parse") else {
            panic!("expected a run");
        };
        let mut out: Vec<u8> = Vec::new();
        let mut reader = io::Cursor::new(input.to_vec());
        stream(&job, &mut reader, &mut out).expect("in-memory streaming cannot fail");
        out
    }

    fn refuse(argv: &[&str]) -> Refusal {
        let args: Vec<OsString> = argv.iter().map(OsString::from).collect();
        parse_args(&args).expect_err("command line should be refused")
    }

    fn text(refusal: &Refusal) -> String {
        match refusal {
            Refusal::Plain(t) => t.clone(),
            Refusal::Getopt(e) => e.sentence.clone(),
            Refusal::Operands { first, second } => match second {
                Some(second) => format!("{first}\n{second}"),
                None => first.clone(),
            },
        }
    }

    // ------------------------------------------------------------ translating

    #[test]
    fn a_range_maps_onto_a_range() {
        assert_eq!(run(&["a-c", "x-z"], b"abcd"), b"xyzd");
    }

    #[test]
    fn set2_is_padded_with_its_last_byte() {
        assert_eq!(run(&["abc", "x"], b"abc"), b"xxx");
    }

    #[test]
    fn truncate_set1_maps_only_the_overlap() {
        assert_eq!(run(&["-t", "abc", "x"], b"abc"), b"xbc");
    }

    #[test]
    fn a_later_entry_of_set1_wins() {
        // `[a*3]b` is a,a,a,b against X,Y,Z,W — so `a` ends up at Z.
        assert_eq!(run(&["[a*3]b", "XYZW"], b"abc"), b"ZWc");
    }

    #[test]
    fn a_reversed_range_is_refused_rather_than_reversed() {
        // The old implementation expanded `c-a` to `cba`, which turns a typo
        // into a working-looking command.
        assert_eq!(
            text(&refuse(&["c-a", "b"])),
            "range-endpoints of 'c-a' are in reverse collating sequence order"
        );
    }

    #[test]
    fn the_two_renderers_of_untrusted_text_are_not_interchangeable() {
        // `make_printable_char`: octal, never named, and `'`/`\` left bare
        // because the message supplies the quotes itself.
        assert_eq!(
            text(&refuse(&["\\377-\\376", "x"])),
            "range-endpoints of '\\377-\\376' are in reverse collating sequence order"
        );
        assert_eq!(
            text(&refuse(&["\\012-\\011", "x"])),
            "range-endpoints of '\\012-\\011' are in reverse collating sequence order"
        );
        assert_eq!(
            text(&refuse(&["\\140-\\047", "x"])),
            "range-endpoints of '`-'' are in reverse collating sequence order"
        );
        // 0x7f is unprintable and 0x20 is not.
        assert_eq!(
            text(&refuse(&["\\177-\\176", "x"])),
            "range-endpoints of '\\177-~' are in reverse collating sequence order"
        );
        assert_eq!(
            text(&refuse(&["\\041-\\040", "x"])),
            "range-endpoints of '!- ' are in reverse collating sequence order"
        );

        // `make_printable_str` alone: named escapes, no quotes, `'` bare.
        assert_eq!(
            text(&refuse(&["-d", "[=\\012\\011=]"])),
            "\\n\\t: equivalence class operand must be a single character"
        );
        assert_eq!(
            text(&refuse(&["-d", "[=\\007\\010\\013\\014\\015\\177=]"])),
            "\\a\\b\\v\\f\\r\\177: equivalence class operand must be a single character"
        );
        assert_eq!(
            text(&refuse(&["-d", "[=''=]"])),
            "'': equivalence class operand must be a single character"
        );

        // `make_printable_str` *then* `quote`: the backslash the first pass
        // wrote is escaped by the second, so one unprintable byte reads
        // `\\377`. An apostrophe, by contrast, passes through untouched —
        // `quote` wraps its value in curly marks, which no straight `'` in
        // the value can be mistaken for, so there is nothing to escape. (Only
        // `quote_glibc`, whose marks *are* straight, has to escape one.)
        assert_eq!(
            text(&refuse(&["-d", "[:no\\377such:]"])),
            "invalid character class ‘no\\\\377such’"
        );
        assert_eq!(
            text(&refuse(&["-d", "[:a\\012b:]"])),
            "invalid character class ‘a\\\\nb’"
        );
        assert_eq!(
            text(&refuse(&["-d", "[:a'b:]"])),
            "invalid character class ‘a'b’"
        );
        assert_eq!(
            text(&refuse(&["a", "[x*a'b]"])),
            "invalid repeat count ‘a'b’ in [c*n] construct"
        );
    }

    #[test]
    fn the_repeat_count_is_strtoumax_and_not_a_string_of_digits() {
        // Leading whitespace and a leading `+` are `strtoumax`'s, so they are
        // `tr`'s. `a-f` is six bytes, and `y` marks where the run of `x` ends.
        assert_eq!(run(&["a-f", "[x*1]y"], b"abcdef"), b"xyyyyy");
        assert_eq!(run(&["a-f", "[x* 1]y"], b"abcdef"), b"xyyyyy");
        assert_eq!(run(&["a-f", "[x*+1]y"], b"abcdef"), b"xyyyyy");
        assert_eq!(run(&["a-f", "[x* +1]y"], b"abcdef"), b"xyyyyy");
        assert_eq!(run(&["a-f", "[x*\n2]y"], b"abcdef"), b"xxyyyy");
        // ...but the sign may not precede the space.
        assert_eq!(
            text(&refuse(&["a", "[x*+ 1]"])),
            "invalid repeat count ‘+ 1’ in [c*n] construct"
        );
        // The base comes from the raw first byte, which need not be a digit:
        // `010` is octal 8, ` 010` is decimal 10, and `08` is an error while
        // ` 08` is eight. `a-m` is thirteen bytes so the two are visibly apart.
        assert_eq!(
            run(&["a-m", "[x*010]y"], b"abcdefghijklm"),
            b"xxxxxxxxyyyyy"
        );
        assert_eq!(
            run(&["a-m", "[x* 010]y"], b"abcdefghijklm"),
            b"xxxxxxxxxxyyy"
        );
        assert_eq!(
            run(&["a-m", "[x*+010]y"], b"abcdefghijklm"),
            b"xxxxxxxxxxyyy"
        );
        assert_eq!(
            run(&["a-m", "[x* 08]y"], b"abcdefghijklm"),
            b"xxxxxxxxyyyyy"
        );
        assert_eq!(
            text(&refuse(&["a", "[x*08]"])),
            "invalid repeat count ‘08’ in [c*n] construct"
        );
        // A field of nothing but whitespace has no digits at all.
        assert_eq!(
            text(&refuse(&["a", "[x* ]"])),
            "invalid repeat count ‘ ’ in [c*n] construct"
        );
        // A negative count is refused before `strtoumax` could wrap it.
        assert_eq!(
            text(&refuse(&["a", "[x*-1]"])),
            "invalid repeat count ‘-1’ in [c*n] construct"
        );
    }

    #[test]
    fn an_escaped_byte_aborts_the_repeat_scan_rather_than_ending_it() {
        // The scan stops at the first `]`, but *aborts* at the first escaped
        // byte of any kind — so these are literal bytes, not repeat counts.
        // `[x*a\b]` in particular has no `]` the scan ever reaches.
        assert_eq!(run(&["a-f", "[x*a\\b]y"], b"abcdef"), b"[x*a\x08]");
        assert_eq!(run(&["a-f", "[x*1\\]y"], b"abcdef"), b"[x*1]y");
        assert_eq!(run(&["a-f", "[x*\\062]y"], b"abcdef"), b"[x*2]y");
        assert_eq!(run(&["a-f", "[x*\\]]y"], b"abcdef"), b"[x*]]y");
    }

    #[test]
    fn an_empty_set2_is_refused_only_when_there_is_something_to_translate() {
        // Two empty sets are a legal no-op, and so is an empty SET1 against a
        // non-empty SET2 — the check is on SET1, not on SET2 alone.
        assert_eq!(run(&["", ""], b"abc"), b"abc");
        assert_eq!(run(&["", "x"], b"abc"), b"abc");
        assert_eq!(run(&["-t", "a", ""], b"abc"), b"abc");
        assert_eq!(
            text(&refuse(&["a", ""])),
            "when not truncating set1, string2 must be non-empty"
        );
        // The length that counts is SET1's *after* complementing: the
        // complement of nothing is every byte there is.
        assert_eq!(
            text(&refuse(&["-c", "", ""])),
            "when not truncating set1, string2 must be non-empty"
        );
        assert_eq!(run(&["-ct", "a-z", ""], b"abc"), b"abc");
    }

    #[test]
    fn set2_is_not_materialised_past_set1() {
        // Upstream reaches SET2 through a generator that stops once SET1 is
        // exhausted, so a count it never gets to costs nothing. Expanding it
        // eagerly made this a 4 GiB allocation and a 50-second wait against
        // GNU's 0.3s — a difference no byte-for-byte comparison can see, which
        // is why it is pinned here as a test rather than in the harness.
        assert_eq!(run(&["a", "[x*4294967296]"], b"az"), b"xz");
        assert_eq!(run(&["a-b", "[x*99999999]"], b"abz"), b"xxz");
        // Capping must not disturb the answer where the cap does not bite.
        assert_eq!(run(&["a-c", "[x*2]y"], b"abc"), b"xxy");
        assert_eq!(run(&["a-c", "[x*9]"], b"abc"), b"xxx");
    }

    // ------------------------------------------------------------- complement

    #[test]
    fn complement_translates_everything_else() {
        assert_eq!(run(&["-c", "a-c", "XY"], b"abcd\n"), b"abcYY");
    }

    #[test]
    fn complement_with_truncate_keeps_only_set2s_worth() {
        // The complement is ascending from 0, so truncating to two leaves NUL
        // and \x01 — neither of which is in the input.
        assert_eq!(run(&["-ct", "a-c", "XY"], b"abcd\n"), b"abcd\n");
    }

    #[test]
    fn complement_delete_keeps_what_was_named() {
        assert_eq!(run(&["-cd", "a-z"], b"hello world\n"), b"helloworld");
    }

    // ---------------------------------------------------------------- squeeze

    #[test]
    fn squeeze_alone_uses_set1() {
        assert_eq!(run(&["-s", "a-c"], b"aabbcc\n"), b"abc\n");
    }

    #[test]
    fn squeeze_with_two_sets_uses_set2() {
        // `l` and `o` become `L` and `O`; the squeeze set is the *output*
        // alphabet, so the doubled `l` collapses after translation.
        assert_eq!(run(&["-s", "lo", "LO"], b"hello\n"), b"heLO\n");
        assert_eq!(run(&["-s", "ab", "xx"], b"aabb\n"), b"x\n");
    }

    #[test]
    fn delete_then_squeeze_reads_both_sets() {
        assert_eq!(run(&["-ds", "a", "b"], b"aabbcc\n"), b"bcc\n");
    }

    #[test]
    fn a_run_split_across_two_reads_still_squeezes() {
        // The filter works in 64 KiB chunks; a run that straddles the boundary
        // must not re-emit at the seam.
        let input = vec![b'a'; 200_000];
        assert_eq!(run(&["-s", "a"], &input), b"a");
    }

    // ------------------------------------------------------------ the grammar

    #[test]
    fn escapes_are_decoded() {
        assert_eq!(run(&["\\t", " "], b"a\tb\n"), b"a b\n");
        assert_eq!(run(&["\\141", "Z"], b"abc"), b"Zbc");
        assert_eq!(run(&["a", "\\n"], b"abc"), b"\nbc");
        // An unknown escape is the byte itself, quietly.
        assert_eq!(run(&["\\z", "Z"], b"abz"), b"abZ");
        // An escaped `-` is a hyphen, not a range.
        assert_eq!(run(&["A\\-B", "xyz"], b"A-B\n"), b"xyz\n");
    }

    #[test]
    fn an_octal_escape_too_large_backs_off_to_two_digits() {
        // `\400` is a space and then the literal digit `0`.
        let (bytes, escaped) = unescape(b"\\400");
        assert_eq!(bytes, b" 0");
        assert_eq!(escaped, vec![true, false]);
    }

    #[test]
    fn character_classes_expand_in_ascending_order() {
        assert_eq!(run(&["[:lower:]", "[:upper:]"], b"abc"), b"ABC");
        assert_eq!(run(&["[:upper:]", "[:lower:]"], b"ABC"), b"abc");
        assert_eq!(Class::Digit.bytes(), b"0123456789".to_vec());
        assert_eq!(Class::Blank.bytes(), b"\t ".to_vec());
        assert_eq!(Class::Space.bytes(), b"\t\n\x0b\x0c\r ".to_vec());
        assert_eq!(Class::Cntrl.bytes().len(), 33);
        assert_eq!(Class::Xdigit.bytes(), b"0123456789ABCDEFabcdef".to_vec());
    }

    #[test]
    fn a_bracket_that_opens_nothing_is_an_ordinary_byte() {
        // The rule that makes `tr -d '[]'` mean what it says.
        assert_eq!(run(&["[]", "XY"], b"a]b"), b"aYb");
        assert_eq!(run(&["[ab]", "Z"], b"[ab]"), b"ZZZZ");
        assert_eq!(run(&["[a*", "Z"], b"[a*"), b"ZZZ");
        assert_eq!(run(&["[:alpha:", "Z"], b"[:"), b"ZZ");
        assert_eq!(run(&["[=a", "Z"], b"[="), b"ZZ");
    }

    #[test]
    fn a_repeat_count_is_octal_when_it_starts_with_zero() {
        assert_eq!(
            expand(&parse_set(b"[a*012]", Which::Two).unwrap(), None).len(),
            10
        );
        assert_eq!(
            expand(&parse_set(b"[a*12]", Which::Two).unwrap(), None).len(),
            12
        );
    }

    #[test]
    fn a_fill_repeat_reaches_set1s_length() {
        assert_eq!(run(&["a-d", "[x*]"], b"abcd"), b"xxxx");
    }

    #[test]
    fn a_fill_repeat_is_refused_in_set1() {
        // There is nothing for it to fill *to*.
        assert_eq!(
            text(&refuse(&["[a*]", "Z"])),
            "the [c*] repeat construct may not appear in string1"
        );
        assert_eq!(
            text(&refuse(&["[a*0]", "Z"])),
            "the [c*] repeat construct may not appear in string1"
        );
    }

    #[test]
    fn a_repeat_count_that_is_not_a_number_is_reported() {
        assert_eq!(
            text(&refuse(&["ab", "[b*x]"])),
            "invalid repeat count ‘x’ in [c*n] construct"
        );
        assert_eq!(
            text(&refuse(&["ab", "[b*3x]"])),
            "invalid repeat count ‘3x’ in [c*n] construct"
        );
    }

    #[test]
    fn the_bracket_scan_stops_at_the_first_close() {
        // `[a*3]]` is three `a`s and then a literal `]`.
        assert_eq!(
            expand(&parse_set(b"[a*3]]", Which::Two).unwrap(), None),
            b"aaa]".to_vec()
        );
    }

    #[test]
    fn an_unknown_class_and_an_empty_one_are_told_apart() {
        // Three messages, two quoting conventions, and the split is upstream's
        // rather than ours. `foo` is runtime data — it came off the command line
        // — and GNU runs it through `quote`, so it picks up §351's curly marks.
        // The other two are spelled out with ASCII apostrophes inside GNU's own
        // translatable literal, so they stay straight in every locale. See the
        // comments at the two call sites; measurement is the only oracle here,
        // and both were measured against GNU 9.4 under `LC_ALL=C.UTF-8`.
        assert_eq!(
            text(&refuse(&["[:foo:]", "b"])),
            "invalid character class ‘foo’"
        );
        assert_eq!(
            text(&refuse(&["[::]", "b"])),
            "missing character class name '[::]'"
        );
        assert_eq!(
            text(&refuse(&["[==]", "b"])),
            "missing equivalence class character '[==]'"
        );
        assert_eq!(
            text(&refuse(&["[=ab=]", "b"])),
            "ab: equivalence class operand must be a single character"
        );
    }

    // ------------------------------------------------------------- validation

    #[test]
    fn set2s_case_classes_must_line_up_with_set1s() {
        // Offsets, not list positions: a two-byte range and two single bytes
        // both put the class at expanded offset 2.
        assert_eq!(run(&["a-b[:lower:]", "cd[:upper:]"], b"abc"), b"ABC");
        assert_eq!(run(&["x[:lower:]", "y[:upper:]"], b"abc"), b"ABC");
        assert_eq!(
            text(&refuse(&["a[:lower:]", "bc[:upper:]"])),
            "misaligned [:upper:] and/or [:lower:] construct"
        );
        assert_eq!(
            text(&refuse(&["a-z", "[:upper:]"])),
            "misaligned [:upper:] and/or [:lower:] construct"
        );
    }

    #[test]
    fn only_upper_and_lower_may_appear_in_set2() {
        assert_eq!(
            text(&refuse(&["a-c", "[:digit:]"])),
            "when translating, the only character classes that may appear in\nstring2 are 'upper' and 'lower'"
        );
        assert_eq!(
            text(&refuse(&["a", "[=b=]"])),
            "[=c=] expressions may not appear in string2 when translating"
        );
    }

    #[test]
    fn an_empty_set2_needs_truncation_to_mean_anything() {
        assert_eq!(
            text(&refuse(&["abc", ""])),
            "when not truncating set1, string2 must be non-empty"
        );
        // With `-t` it is legal and does nothing at all.
        assert_eq!(run(&["-t", "abc", ""], b"abc"), b"abc");
    }

    // ---------------------------------------------------------- the arguments

    #[test]
    fn the_operand_count_rules_explain_themselves() {
        assert_eq!(text(&refuse(&[])), "missing operand");
        assert_eq!(
            text(&refuse(&["abc"])),
            "missing operand after ‘abc’\nTwo strings must be given when translating."
        );
        assert_eq!(
            text(&refuse(&["-ds", "a"])),
            "missing operand after ‘a’\nTwo strings must be given when both deleting and squeezing repeats."
        );
        assert_eq!(
            text(&refuse(&["-d", "a", "b"])),
            "extra operand ‘b’\nOnly one string may be given when deleting without squeezing repeats."
        );
        assert_eq!(text(&refuse(&["a", "b", "c"])), "extra operand ‘c’");
        // The excess operand is the first one past what the *mode* allows, not
        // simply the third: deleting without squeezing allows one set, so `b`
        // is already one too many however many follow it. And the explanation
        // is dropped once more than one operand is excess, because "only one
        // string may be given" does not answer a four-operand mistake.
        assert_eq!(text(&refuse(&["-d", "a", "b", "c"])), "extra operand ‘b’");
        assert_eq!(
            text(&refuse(&["-d", "a", "b", "c", "d"])),
            "extra operand ‘b’"
        );
        assert_eq!(text(&refuse(&["-ds", "a", "b", "c"])), "extra operand ‘c’");
        assert_eq!(text(&refuse(&["-s", "a", "b", "c"])), "extra operand ‘c’");
        assert_eq!(text(&refuse(&["a", "b", "c", "d"])), "extra operand ‘c’");
        // Squeezing alone is content with a single set.
        assert_eq!(run(&["-s", "ab"], b"aabb"), b"ab");
    }

    #[test]
    fn options_may_be_clustered_abbreviated_or_ended() {
        assert_eq!(run(&["-cd", "a-z"], b"a1b"), b"ab");
        assert_eq!(run(&["--complement", "--delete", "a-z"], b"a1b"), b"ab");
        assert_eq!(run(&["--com", "--de", "a-z"], b"a1b"), b"ab");
        // `-C` is `-c`.
        assert_eq!(run(&["-Cd", "a-z"], b"a1b"), b"ab");
        // After `--`, an option-looking operand is a set.
        assert_eq!(run(&["--", "-a", "xy"], b"-a"), b"xy");
    }

    #[test]
    fn help_carries_gnus_body_and_not_a_summary_of_it() {
        // The body is GNU's verbatim, so a script that greps `--help` for a
        // construct it wants to use finds the same line it finds on Linux.
        // Everything after the class list is the part that answers "what does
        // [CHAR*] do", which a one-line synopsis silently drops.
        let help = help_text();
        assert!(help.starts_with("Usage: tr [OPTION]... STRING1 [STRING2]\n"));
        assert!(help.ends_with("Squeezing occurs after translation or deletion.\n"));
        for line in [
            "  \\NNN            character with octal value NNN (1 to 3 octal digits)\n",
            "  \\\\              backslash\n",
            "  [CHAR*]         in ARRAY2, copies of CHAR until length of ARRAY1\n",
            "  [CHAR*REPEAT]   REPEAT copies of CHAR, REPEAT octal if starting with 0\n",
            "  [=CHAR=]        all characters which are equivalent to CHAR\n",
        ] {
            assert!(help.contains(line), "missing from --help: {line:?}");
        }
        // Every class this implements is documented, and none it does not.
        for class in [
            "alnum", "alpha", "blank", "cntrl", "digit", "graph", "lower", "print", "punct",
            "space", "upper", "xdigit",
        ] {
            assert!(
                help.contains(&format!("  [:{class}:]")),
                "undocumented: {class}"
            );
        }
        // GNU's own referrals name a project this is not; they stay out.
        assert!(!help.contains("gnu.org"));
        assert!(!help.contains("info '(coreutils)"));
    }

    #[test]
    fn a_bad_option_is_worded_as_glibc_words_it() {
        assert_eq!(text(&refuse(&["-x", "a"])), "invalid option -- 'x'");
        assert_eq!(text(&refuse(&["--zz", "a"])), "unrecognized option '--zz'");
        assert_eq!(
            text(&refuse(&["--delete=x", "a"])),
            "option '--delete' doesn't allow an argument"
        );
    }

    #[test]
    fn a_lone_hyphen_is_a_set_and_not_standard_input() {
        // `tr` has no file operands, so `-` can only be the byte itself.
        assert_eq!(run(&["-", "_"], b"a-b"), b"a_b");
    }
}
