//! cut — print selected parts of lines.
//!
//! The sixth of the 85 utilities moved onto the shared [`coreutils::getopt`]
//! (see `known-issues.md` → `TD-COREUTILS-LONG-OPTIONS-DO-NOT-ABBREVIATE`).
//! The parser this replaces knew `-c`, `-d` and `-f`; it had no `-s`, no `-z`,
//! no `-n`, no `--complement` and no `--output-delimiter`, and it read input as
//! UTF-8 `String` lines, so a file that is not UTF-8 stopped at the first bad
//! byte and `\r\n` came back out as `\n`.
//!
//! It was also wrong about the one thing `cut` is for. `parse_ranges` expanded
//! every range into a `Vec` of individual indices, so `cut -f2-` — the
//! commonest form there is — parsed as the single field 2, and
//! `cut -f1-1000000` allocated a million entries to select four fields.
//!
//! # The selection is a list of ranges, and its *shape* is observable
//!
//! Upstream sorts the ranges by their start and merges any that overlap, but
//! **does not merge ones that merely touch**. That looks like an internal
//! detail and is not, because `--output-delimiter` emits its string at the
//! start of each range:
//!
//! ```text
//! $ printf 'abcdefgh\n' | cut -b1-2,3-4 --output-delimiter=.
//! ab.cd
//! $ printf 'abcdefgh\n' | cut -b1-2,2-4 --output-delimiter=.
//! abcd
//! ```
//!
//! Both name bytes 1 to 4. The first is two ranges because 3 does not overlap
//! 1-2; the second is one because 2 does. So the merge rule is user-visible
//! output and is reproduced exactly rather than tidied into a set of indices.
//!
//! # `-c` selects bytes, not characters
//!
//! GNU 9.4 implements `-c` by falling through to `-b`: the two are the same
//! code path, and `cut -c1` on a UTF-8 é returns half of it. The name is
//! aspirational upstream and the multibyte version is a long-standing wishlist
//! item. Since these utilities are certified byte-for-byte against GNU, `-c` is
//! bytes here too — a `cut` that quietly did the better thing would disagree
//! with every script written against the real one, and the two would give
//! different answers about the same file. The only trace of the distinction
//! that survives is the diagnostic wording, which does say "byte/character".
//!
//! # Every rule here was measured
//!
//! Against glibc's `cut` (GNU coreutils 9.4) through WSL, not against the
//! `cut` on this host's `PATH`, which is MSYS2's. `scripts/cut-diff.sh` is the
//! executable form of every claim in this file.

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::{os_bytes, quote, quotef};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
use std::process::ExitCode;

/// Measured: `cut --zzz-bogus; echo $?` is 1.
const CUT: Program = Program::new("cut", 1);

/// The long options in **GNU's declaration order**, which is observable: it is
/// the order `getopt_long` lists candidates in when an abbreviation is
/// ambiguous. Measured with `cut --=x`, an empty prefix that matches every
/// entry and so prints the whole table.
///
/// Note there is no long name for `-n`. Upstream lists `n` in the short-option
/// string and does nothing with it, so `cut --n` is *unrecognised* while
/// `cut -n` is accepted and ignored.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("bytes", Takes::Required),
    ("characters", Takes::Required),
    ("fields", Takes::Required),
    ("delimiter", Takes::Required),
    ("only-delimited", Takes::Nothing),
    ("output-delimiter", Takes::Required),
    ("complement", Takes::Nothing),
    ("zero-terminated", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// One selected span, 1-based and inclusive at both ends.
///
/// `hi == u64::MAX` is how an open range (`3-`) is written, and a pair of
/// `u64::MAX` is the sentinel that terminates the list — both are upstream's
/// representation, kept because the arithmetic around them (`hi + 1` in the
/// complement, `idx > hi` in the scan) is what the observable output falls out
/// of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Range {
    lo: u64,
    hi: u64,
}

/// Which of the two scanners runs. `-b` and `-c` both mean [`Mode::Bytes`];
/// see the module documentation for why `-c` is not characters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Bytes,
    Fields,
}

/// A command line that parsed, reduced to what the scanners actually need.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Options {
    mode: Mode,
    /// Sorted, merged, and terminated by the `u64::MAX` sentinel.
    ranges: Vec<Range>,
    /// The input field delimiter. `-d ''` means NUL, which is why this is a
    /// byte and not an `Option`.
    delim: u8,
    output_delim: Vec<u8>,
    /// Whether `--output-delimiter` was given. In byte mode this decides
    /// whether ranges are separated *at all*, so it is not derivable from
    /// `output_delim` — the default value is a one-byte string too.
    output_delim_given: bool,
    line_delim: u8,
    suppress_non_delimited: bool,
}

/// What a parsed command line asks for.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Request {
    Run(Options, Vec<OsString>),
    Help,
    Version,
}

/// The option state as the loop builds it, before the cross-checks that can
/// only run once every argument has been seen.
#[derive(Default)]
struct Draft {
    spec: Option<Vec<u8>>,
    byte_mode: bool,
    delim: u8,
    delim_specified: bool,
    output_delim: Option<Vec<u8>>,
    complement: bool,
    suppress_non_delimited: bool,
    line_delim: Option<u8>,
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("cut (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(options, files)) => run(&options, &files),
        Err(e) => {
            // The referral, when there is one, is part of the message, and only
            // the first line carries the `cut: ` prefix — which is what GNU
            // prints, including for the two-line `-s` diagnostic.
            eprintln!("cut: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

/// GNU's `--help`, byte for byte, minus the trailing block of URLs that names
/// the GNU project's own bug addresses.
fn help_text() -> String {
    "\
Usage: cut OPTION... [FILE]...
Print selected parts of lines from each FILE to standard output.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -b, --bytes=LIST        select only these bytes
  -c, --characters=LIST   select only these characters
  -d, --delimiter=DELIM   use DELIM instead of TAB for field delimiter
  -f, --fields=LIST       select only these fields;  also print any line
                            that contains no delimiter character, unless
                            the -s option is specified
  -n                      (ignored)
      --complement        complement the set of selected bytes, characters
                            or fields
  -s, --only-delimited    do not print lines not containing delimiters
      --output-delimiter=STRING  use STRING as the output delimiter
                            the default is to use the input delimiter
  -z, --zero-terminated   line delimiter is NUL, not newline
      --help        display this help and exit
      --version     output version information and exit

Use one, and only one of -b, -c or -f.  Each LIST is made up of one
range, or many ranges separated by commas.  Selected input is written
in the same order that it is read, and is written exactly once.
Each range is one of:

  N     N'th byte, character or field, counted from 1
  N-    from N'th byte, character or field, to end of line
  N-M   from N'th to M'th (included) byte, character or field
  -M    from first to M'th (included) byte, character or field
"
    .to_string()
}

/// Parse the whole command line.
///
/// Options may follow operands — `cut f -f1` works — because glibc's
/// `getopt_long` permutes argv unless the caller asks it not to, and `cut` does
/// not ask.
///
/// # Errors
///
/// Any getopt diagnostic, plus `cut`'s own: the four cross-checks in
/// [`finish`] and everything [`set_fields`] can refuse.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut draft = Draft::default();
    let mut files: Vec<OsString> = Vec::new();
    let mut only_operands = false;
    let mut i = 0usize;

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
            if let Some(request) = long_option(&bytes, args, &mut i, &mut draft)? {
                return Ok(request);
            }
        } else {
            short_options(&bytes, args, &mut i, &mut draft)?;
        }
    }

    Ok(Request::Run(finish(draft)?, files))
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
    let typed = std::str::from_utf8(typed).map_err(|_| CUT.unrecognized_option(bytes))?;
    let (name, takes) = CUT.resolve_long(typed, bytes, LONG_OPTIONS)?;

    if takes == Takes::Nothing && inline.is_some() {
        return Err(CUT.long_unwanted_argument(name));
    }
    let value: Option<Vec<u8>> = match (takes, inline) {
        (_, Some(v)) => Some(v.to_vec()),
        (Takes::Required, None) => {
            let next = args
                .get(*i)
                .ok_or_else(|| CUT.long_missing_argument(name))?
                .clone();
            *i = i.saturating_add(1);
            Some(arg_bytes(&next))
        }
        (_, None) => None,
    };
    let value = value.unwrap_or_default();

    match name {
        "bytes" | "characters" => set_spec(value, true, draft)?,
        "fields" => set_spec(value, false, draft)?,
        "delimiter" => set_delimiter(&value, draft)?,
        "only-delimited" => draft.suppress_non_delimited = true,
        "output-delimiter" => draft.output_delim = Some(output_delimiter(&value)),
        "complement" => draft.complement = true,
        "zero-terminated" => draft.line_delim = Some(0),
        "help" => return Ok(Some(Request::Help)),
        "version" => return Ok(Some(Request::Version)),
        // `resolve_long` returns only names from the table, all of which are
        // above.
        _ => {}
    }
    Ok(None)
}

/// One `-abc` cluster. `cut` has no option that ends parsing, so unlike
/// `head`'s this returns nothing.
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
            // Accepted and ignored. Upstream's `case 'n': break;` — it once
            // meant "do not split multibyte characters" and never did anything.
            b'n' => {}
            b's' => draft.suppress_non_delimited = true,
            b'z' => draft.line_delim = Some(0),
            b'b' | b'c' | b'f' | b'd' => {
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
                            .ok_or_else(|| CUT.short_missing_argument(c))?
                            .clone();
                        *i = i.saturating_add(1);
                        arg_bytes(&next)
                    }
                };
                if c == b'd' {
                    set_delimiter(&value, draft)?;
                } else {
                    set_spec(value, c != b'f', draft)?;
                }
            }
            _ => return Err(CUT.invalid_option(c)),
        }
    }
    Ok(())
}

/// Record a `-b`/`-c`/`-f` list.
///
/// The "only one list" check fires on the *second* list whatever the two were,
/// so `cut -f1 -f2` is refused exactly like `cut -b1 -f1`. Note also that
/// `byte_mode` is sticky in upstream's sense: `-b` and `-c` set it and `-f`
/// never clears it, but since a second list is an error anyway that can only
/// matter in the error case.
fn set_spec(value: Vec<u8>, byte_mode: bool, draft: &mut Draft) -> Result<(), getopt::Error> {
    if draft.spec.is_some() {
        return Err(CUT.usage_referring("only one list may be specified".to_string()));
    }
    draft.byte_mode = byte_mode;
    draft.spec = Some(value);
    Ok(())
}

/// Record `-d`. The delimiter is one byte; `-d ''` means the NUL byte, which is
/// upstream's documented reading of an empty argument.
fn set_delimiter(value: &[u8], draft: &mut Draft) -> Result<(), getopt::Error> {
    if value.len() > 1 {
        return Err(CUT.usage_referring("the delimiter must be a single character".to_string()));
    }
    draft.delim = value.first().copied().unwrap_or(0);
    draft.delim_specified = true;
    Ok(())
}

/// `--output-delimiter=STRING`, with upstream's reading of the empty string.
///
/// `--output-delimiter=` sets the length to 1 while leaving the pointer at the
/// empty C string, so the byte that gets written is that string's terminator: a
/// single NUL. Measured, not inferred — `cut -d: -f1,3 --output-delimiter=`
/// prints `a\0c`.
fn output_delimiter(value: &[u8]) -> Vec<u8> {
    if value.is_empty() {
        vec![0]
    } else {
        value.to_vec()
    }
}

/// The checks that can only run once the whole command line has been seen, in
/// upstream's order — which is observable whenever two of them apply at once.
///
/// # Errors
///
/// No list; `-d` or `-s` in byte mode; anything [`set_fields`] refuses.
fn finish(draft: Draft) -> Result<Options, getopt::Error> {
    let Some(spec) = draft.spec else {
        return Err(CUT.usage_referring(
            "you must specify a list of bytes, characters, or fields".to_string(),
        ));
    };

    if draft.byte_mode {
        if draft.delim_specified {
            return Err(CUT.usage_referring(
                "an input delimiter may be specified only when operating on fields".to_string(),
            ));
        }
        if draft.suppress_non_delimited {
            // Two lines, and the second begins with a literal tab. Only the
            // first carries the `cut: ` prefix.
            return Err(CUT.usage_referring(
                "suppressing non-delimited lines makes sense\n\tonly when operating on fields"
                    .to_string(),
            ));
        }
    }

    let ranges = set_fields(&spec, draft.byte_mode, draft.complement)?;
    let delim = if draft.delim_specified {
        draft.delim
    } else {
        b'\t'
    };

    Ok(Options {
        mode: if draft.byte_mode {
            Mode::Bytes
        } else {
            Mode::Fields
        },
        ranges,
        delim,
        output_delim_given: draft.output_delim.is_some(),
        output_delim: draft.output_delim.unwrap_or_else(|| vec![delim]),
        line_delim: draft.line_delim.unwrap_or(b'\n'),
        suppress_non_delimited: draft.suppress_non_delimited,
    })
}

/// gnulib's `set_fields` (`src/set-fields.c`): parse a LIST into sorted,
/// merged ranges.
///
/// The grammar is `N`, `N-`, `-M` or `N-M`, separated by commas **or by
/// blanks** — `cut -f '1 3'` is the same as `cut -f 1,3`, which nothing
/// documents and the code makes plain. Indices are 1-based and 0 is an error.
///
/// `use_pos` selects the byte-mode wording of five of the diagnostics; it is
/// upstream's `SETFLD_ERRMSG_USE_POS`, and it is the only surviving trace of
/// the `-b`/`-c` distinction.
///
/// # Errors
///
/// A zero index, a decreasing range, two dashes in one item, a lone `-`, a
/// non-digit, or a number that does not fit in a `u64`.
fn set_fields(spec: &[u8], use_pos: bool, complement: bool) -> Result<Vec<Range>, getopt::Error> {
    let mut ranges: Vec<Range> = Vec::new();
    let mut initial: u64 = 1;
    let mut value: u64 = 0;
    let mut lhs_specified = false;
    let mut rhs_specified = false;
    let mut dash_found = false;
    let mut in_digits = false;
    let mut num_start = 0usize;
    let mut at = 0usize;

    loop {
        // Past the end stands for C's terminating NUL, which the loop treats as
        // a separator that also ends it.
        let c = spec.get(at).copied();
        match c {
            Some(b'-') => {
                in_digits = false;
                if dash_found {
                    return Err(CUT.usage_referring(
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
                    return Err(CUT.usage_referring(numbered_from_1(use_pos)));
                }
                initial = if lhs_specified { value } else { 1 };
                value = 0;
            }
            None | Some(b',' | b' ' | b'\t') => {
                in_digits = false;
                if dash_found {
                    dash_found = false;
                    if !lhs_specified && !rhs_specified {
                        return Err(
                            CUT.usage_referring("invalid range with no endpoint: -".to_string())
                        );
                    }
                    if rhs_specified {
                        if value < initial {
                            return Err(CUT.usage_referring("invalid decreasing range".to_string()));
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
                        return Err(CUT.usage_referring(numbered_from_1(use_pos)));
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
                        return Err(CUT.usage_referring(format!(
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
                return Err(CUT.usage_referring(format!(
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
        return Err(CUT.usage_referring(
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
    if complement {
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
/// visible through `--output-delimiter`, which separates ranges — see the
/// module documentation.
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

/// Where the scan currently is in the range list: upstream's `current_rp`.
///
/// It only ever moves forwards, which is what makes the whole selection an O(1)
/// test per byte rather than a search.
struct Cursor<'r> {
    ranges: &'r [Range],
    at: usize,
}

impl<'r> Cursor<'r> {
    fn new(ranges: &'r [Range]) -> Self {
        Cursor { ranges, at: 0 }
    }

    fn reset(&mut self) {
        self.at = 0;
    }

    /// The range under the cursor. The sentinel guarantees there is one.
    fn current(&self) -> Range {
        self.ranges.get(self.at).copied().unwrap_or(Range {
            lo: u64::MAX,
            hi: u64::MAX,
        })
    }

    /// Advance the item index, and the cursor with it once the index leaves the
    /// range it was in.
    fn next_item(&mut self, idx: &mut u64) {
        *idx = idx.saturating_add(1);
        if *idx > self.current().hi {
            self.at = self.at.saturating_add(1);
        }
    }

    fn selected(&self, k: u64) -> bool {
        self.current().lo <= k
    }

    fn starts_range(&self, k: u64) -> bool {
        k == self.current().lo
    }
}

/// One input, read a byte at a time with a one-byte pushback — C's `getc` and
/// `ungetc`, which the two scanners are written in terms of.
struct Input<R: Read> {
    inner: R,
    buf: Vec<u8>,
    pos: usize,
    len: usize,
    done: bool,
    /// The first read failure, reported once the scan stops rather than at the
    /// point it happened: upstream checks `ferror` after the loop.
    error: Option<io::Error>,
}

impl<R: Read> Input<R> {
    fn new(inner: R) -> Self {
        Input {
            inner,
            buf: vec![0; 64 * 1024],
            pos: 0,
            len: 0,
            done: false,
            error: None,
        }
    }

    /// Refill when empty. Returns false at end of input or on error.
    fn fill(&mut self) -> bool {
        if self.pos < self.len {
            return true;
        }
        if self.done {
            return false;
        }
        loop {
            match self.inner.read(&mut self.buf) {
                Ok(0) => {
                    self.done = true;
                    return false;
                }
                Ok(n) => {
                    self.pos = 0;
                    self.len = n;
                    return true;
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => {
                    self.error = Some(e);
                    self.done = true;
                    return false;
                }
            }
        }
    }

    /// C's `getc`. `None` is EOF *or* a read error; the two are told apart by
    /// [`Input::error`] afterwards, exactly as `feof`/`ferror` do.
    fn get(&mut self) -> Option<u8> {
        if !self.fill() {
            return None;
        }
        let c = self.buf.get(self.pos).copied();
        self.pos = self.pos.saturating_add(1);
        c
    }

    /// C's `ungetc`, valid only immediately after a [`Input::get`] that
    /// returned a byte — which is the only way upstream uses it.
    fn unget(&mut self) {
        self.pos = self.pos.saturating_sub(1);
    }

    /// gnulib's `getndelim2` as `cut` calls it: read up to and including
    /// whichever of the two delimiters comes first, appending to `out`.
    ///
    /// Returns false when nothing at all was read, which is end of input.
    fn read_until_either(&mut self, a: u8, b: u8, out: &mut Vec<u8>) -> bool {
        out.clear();
        loop {
            let Some(c) = self.get() else {
                return !out.is_empty();
            };
            out.push(c);
            if c == a || c == b {
                return true;
            }
        }
    }
}

/// `cut -b`/`-c`: select bytes of each line.
fn cut_bytes<R: Read, W: Write>(
    input: &mut Input<R>,
    out: &mut W,
    options: &Options,
) -> io::Result<()> {
    let mut cursor = Cursor::new(&options.ranges);
    let mut byte_idx: u64 = 0;
    // Whether a range has already been printed on this line, which is what
    // decides where an output delimiter goes.
    let mut print_delimiter = false;

    loop {
        let Some(c) = input.get() else {
            // An unterminated final line still gets a terminator, provided
            // something was on it.
            if byte_idx > 0 {
                out.write_all(&[options.line_delim])?;
            }
            return Ok(());
        };
        if c == options.line_delim {
            out.write_all(&[c])?;
            byte_idx = 0;
            print_delimiter = false;
            cursor.reset();
            continue;
        }
        cursor.next_item(&mut byte_idx);
        if cursor.selected(byte_idx) {
            // Without `--output-delimiter` the selected bytes are simply
            // concatenated; the separator logic does not run at all, which is
            // why the default cannot be modelled as "the delimiter is empty".
            if options.output_delim_given {
                if print_delimiter && cursor.starts_range(byte_idx) {
                    out.write_all(&options.output_delim)?;
                }
                print_delimiter = true;
            }
            out.write_all(&[c])?;
        }
    }
}

/// `cut -f`: select delimiter-separated fields.
///
/// The shape is upstream's, including the part that looks avoidable: the first
/// field is sometimes read into a buffer before any of it is printed, because
/// whether a line is *delimited at all* is only knowable once the first field's
/// terminator is seen, and `-s` (and the absence of `-s`) both need that answer
/// before deciding whether to print anything.
#[allow(clippy::too_many_lines)]
fn cut_fields<R: Read, W: Write>(
    input: &mut Input<R>,
    out: &mut W,
    options: &Options,
) -> io::Result<()> {
    let mut cursor = Cursor::new(&options.ranges);
    let mut field_idx: u64 = 1;
    let mut found_any_selected_field = false;
    let mut c: Option<u8> = Some(0);
    let mut first_field: Vec<u8> = Vec::new();

    // An empty input produces no output at all, not even a line terminator.
    if input.get().is_none() {
        return Ok(());
    }
    input.unget();

    // Buffering the first field is unnecessary when the answer is the same
    // either way: if non-delimited lines are printed and field 1 is selected,
    // or if they are suppressed and it is not. A non-delimited line has
    // exactly one field. (Upstream writes this as
    // `suppress_non_delimited ^ !print_kth (1)`, which is this negated twice.)
    let buffer_first_field = options.suppress_non_delimited == cursor.selected(1);

    loop {
        if field_idx == 1 && buffer_first_field {
            if !input.read_until_either(options.delim, options.line_delim, &mut first_field) {
                return Ok(());
            }
            c = Some(0);

            let last = first_field.last().copied().unwrap_or(0);
            if last != options.delim {
                // The line holds no delimiter at all: print it whole, or not at
                // all, and start the next one.
                if !options.suppress_non_delimited {
                    out.write_all(&first_field)?;
                    if last != options.line_delim {
                        out.write_all(&[options.line_delim])?;
                    }
                    c = Some(options.line_delim);
                }
                continue;
            }

            if cursor.selected(1) {
                let body = first_field
                    .get(..first_field.len().saturating_sub(1))
                    .unwrap_or_default();
                out.write_all(body)?;
                // With `-d $'\n'` the final newline is a line terminator and
                // not a delimiter, so a field it ends must not be counted as
                // one that was found — otherwise a delimiter is printed after
                // the last line.
                if options.delim == options.line_delim {
                    if input.get().is_some() {
                        input.unget();
                        found_any_selected_field = true;
                    }
                } else {
                    found_any_selected_field = true;
                }
            }
            cursor.next_item(&mut field_idx);
        }

        let mut prev_c = c;

        if cursor.selected(field_idx) {
            if found_any_selected_field {
                out.write_all(&options.output_delim)?;
            }
            found_any_selected_field = true;
            loop {
                c = input.get();
                match c {
                    Some(ch) if ch != options.delim && ch != options.line_delim => {
                        out.write_all(&[ch])?;
                        prev_c = Some(ch);
                    }
                    _ => break,
                }
            }
        } else {
            loop {
                c = input.get();
                match c {
                    Some(ch) if ch != options.delim && ch != options.line_delim => {
                        prev_c = Some(ch);
                    }
                    _ => break,
                }
            }
        }

        // Same rule as above, on the other side: a trailing newline under
        // `-d $'\n'` ends the input rather than opening another field.
        if options.delim == options.line_delim && c == Some(options.delim) {
            if input.get().is_some() {
                input.unget();
            } else {
                c = None;
            }
        }

        if c == Some(options.delim) {
            cursor.next_item(&mut field_idx);
        } else {
            // Either the line ended or the input did.
            let line_was_printed =
                found_any_selected_field || !(options.suppress_non_delimited && field_idx == 1);
            // The terminator is not repeated when the line already ended in one
            // and this iteration consumed nothing — except under `-d $'\n'`,
            // where the delimiter *is* the terminator and every field ends a
            // line of its own.
            let needs_terminator = c == Some(options.line_delim)
                || prev_c != Some(options.line_delim)
                || options.delim == options.line_delim;
            if line_was_printed && needs_terminator {
                out.write_all(&[options.line_delim])?;
            }
            if c.is_none() {
                return Ok(());
            }
            field_idx = 1;
            cursor.reset();
            found_any_selected_field = false;
        }
    }
}

/// Whichever scanner the mode calls for, over one already-open input.
fn cut_stream<R: Read, W: Write>(
    input: &mut Input<R>,
    out: &mut W,
    options: &Options,
) -> io::Result<()> {
    match options.mode {
        Mode::Bytes => cut_bytes(input, out, options),
        Mode::Fields => cut_fields(input, out, options),
    }
}

/// One operand. `Ok(true)` means it was processed; `Ok(false)` that it failed
/// and has been reported; `Err` is a *write* failure, which is fatal for the
/// whole run rather than for this file.
fn cut_file<W: Write>(name: &OsString, options: &Options, out: &mut W) -> io::Result<bool> {
    let bytes = arg_bytes(name);
    let read_error = |e: &io::Error| {
        eprintln!("cut: {}: {}", quotef(&bytes), strerror(e));
    };

    if bytes == b"-" {
        let mut input = Input::new(io::stdin());
        cut_stream(&mut input, out, options)?;
        if let Some(e) = input.error.as_ref() {
            read_error(e);
            return Ok(false);
        }
        return Ok(true);
    }

    let file = match File::open(name) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cut: {}: {}", quotef(&bytes), strerror(&e));
            return Ok(false);
        }
    };
    let mut input = Input::new(file);
    cut_stream(&mut input, out, options)?;
    if let Some(e) = input.error.as_ref() {
        read_error(e);
        return Ok(false);
    }
    Ok(true)
}

fn run(options: &Options, files: &[OsString]) -> ExitCode {
    let stdout = io::stdout();
    let mut out = io::BufWriter::with_capacity(64 * 1024, stdout.lock());
    let default = [OsString::from("-")];
    let operands: &[OsString] = if files.is_empty() { &default } else { files };
    let mut ok = true;

    for name in operands {
        match cut_file(name, options, &mut out) {
            Ok(good) => ok &= good,
            Err(e) => return write_failure(&e, ok),
        }
    }

    if let Err(e) = out.flush() {
        return write_failure(&e, ok);
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// A failed write. GNU dies of `SIGPIPE` when the reader goes away, printing
/// nothing; Rust masks that signal, so the same situation arrives as `EPIPE`
/// and has to be recognised and kept quiet. Any other write failure is
/// upstream's `write_error()`.
fn write_failure(e: &io::Error, ok: bool) -> ExitCode {
    if e.kind() == ErrorKind::BrokenPipe {
        return ExitCode::from(u8::from(!ok));
    }
    eprintln!("cut: write error: {}", strerror(e));
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
        match parse_args(&args(items)) {
            Ok(Request::Run(o, _)) => o,
            other => panic!("expected a runnable request, got {other:?}"),
        }
    }

    /// The operands a command line parses to.
    fn operands(items: &[&str]) -> Vec<String> {
        match parse_args(&args(items)) {
            Ok(Request::Run(_, f)) => f.iter().map(|s| s.to_string_lossy().into_owned()).collect(),
            other => panic!("expected a runnable request, got {other:?}"),
        }
    }

    /// The error a command line fails with, panicking if it succeeds.
    fn fail(items: &[&str]) -> getopt::Error {
        match parse_args(&args(items)) {
            Err(e) => e,
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    /// A diagnostic's own sentence, which is what these assertions are about —
    /// the `Try '…'` referral every one of them carries is not.
    fn body(e: &getopt::Error) -> String {
        e.sentence.clone()
    }

    /// Run the utility over `input` and return exactly what it wrote.
    fn cut(input: &[u8], items: &[&str]) -> Vec<u8> {
        let options = parse(items);
        let mut source = Input::new(input);
        let mut out: Vec<u8> = Vec::new();
        cut_stream(&mut source, &mut out, &options).unwrap();
        out
    }

    /// The ranges a LIST selects, without the sentinel.
    fn ranges(items: &[&str]) -> Vec<(u64, u64)> {
        let mut rs = parse(items).ranges;
        rs.pop();
        rs.into_iter().map(|r| (r.lo, r.hi)).collect()
    }

    // ---------------- the option table ----------------

    #[test]
    fn long_options_abbreviate_the_way_getopt_long_does() {
        // A unique prefix resolves.
        assert_eq!(parse(&["--fie=2"]).mode, Mode::Fields);
        assert_eq!(parse(&["--by=2"]).mode, Mode::Bytes);
        assert!(parse(&["--only", "-f1"]).suppress_non_delimited);
        assert_eq!(parse(&["--z", "-f1"]).line_delim, 0);
        // `--c` is ambiguous between `--characters` and `--complement`, and the
        // candidates come out in the table's order, not alphabetically.
        assert_eq!(
            body(&fail(&["--c=1"])),
            "option '--c=1' is ambiguous; possibilities: '--characters' '--complement'"
        );
        // `--o` likewise, between `--only-delimited` and `--output-delimiter`.
        assert_eq!(
            body(&fail(&["--o"])),
            "option '--o' is ambiguous; possibilities: '--only-delimited' '--output-delimiter'"
        );
    }

    #[test]
    fn the_empty_prefix_prints_the_whole_table_in_declaration_order() {
        assert_eq!(
            body(&fail(&["--=x"])),
            "option '--=x' is ambiguous; possibilities: '--bytes' '--characters' \
             '--fields' '--delimiter' '--only-delimited' '--output-delimiter' \
             '--complement' '--zero-terminated' '--help' '--version'"
        );
    }

    #[test]
    fn n_is_a_short_option_only() {
        // Accepted and ignored…
        assert_eq!(parse(&["-n", "-f1"]).mode, Mode::Fields);
        // …but there is no long name for it, so `--n` is not an abbreviation of
        // anything.
        assert_eq!(body(&fail(&["--n"])), "unrecognized option '--n'");
    }

    #[test]
    fn the_five_getopt_sentences() {
        assert_eq!(body(&fail(&["-x"])), "invalid option -- 'x'");
        assert_eq!(body(&fail(&["-f"])), "option requires an argument -- 'f'");
        assert_eq!(body(&fail(&["--nosuch"])), "unrecognized option '--nosuch'");
        assert_eq!(
            body(&fail(&["--fields"])),
            "option '--fields' requires an argument"
        );
        assert_eq!(
            body(&fail(&["--complement=x", "-f1"])),
            "option '--complement' doesn't allow an argument"
        );
    }

    #[test]
    fn help_and_version_end_parsing() {
        assert_eq!(parse_args(&args(&["--help"])), Ok(Request::Help));
        assert_eq!(parse_args(&args(&["--vers"])), Ok(Request::Version));
        // Even with a command line that would otherwise be refused outright.
        assert_eq!(parse_args(&args(&["--help"])), Ok(Request::Help));
        assert_eq!(
            parse_args(&args(&["-n", "--version"])),
            Ok(Request::Version)
        );
    }

    // ---------------- operands ----------------

    #[test]
    fn options_may_follow_operands_and_a_lone_dash_is_one() {
        assert_eq!(operands(&["f", "-f1", "g"]), vec!["f", "g"]);
        assert_eq!(operands(&["-f1", "-"]), vec!["-"]);
        // After `--`, even something that looks like an option is a file.
        assert_eq!(operands(&["-f1", "--", "-x"]), vec!["-x"]);
    }

    // ---------------- the LIST grammar ----------------

    #[test]
    fn a_list_is_numbers_ranges_and_open_ends() {
        assert_eq!(ranges(&["-f", "2"]), vec![(2, 2)]);
        assert_eq!(ranges(&["-f", "2-4"]), vec![(2, 4)]);
        assert_eq!(ranges(&["-f", "3-"]), vec![(3, u64::MAX)]);
        assert_eq!(ranges(&["-f", "-3"]), vec![(1, 3)]);
        assert_eq!(ranges(&["-f", "1,3"]), vec![(1, 1), (3, 3)]);
    }

    #[test]
    fn blanks_separate_a_list_exactly_as_commas_do() {
        assert_eq!(ranges(&["-f", "1 3"]), ranges(&["-f", "1,3"]));
        assert_eq!(ranges(&["-f", "1\t3"]), ranges(&["-f", "1,3"]));
    }

    #[test]
    fn ranges_are_sorted_and_overlapping_ones_merged_but_not_touching_ones() {
        assert_eq!(ranges(&["-f", "3,1"]), vec![(1, 1), (3, 3)]);
        assert_eq!(ranges(&["-f", "2-5,3-4"]), vec![(2, 5)]);
        // Touching, not overlapping: two ranges, which `--output-delimiter`
        // can see.
        assert_eq!(ranges(&["-f", "1-2,3-4"]), vec![(1, 2), (3, 4)]);
        assert_eq!(ranges(&["-f", "1-,3"]), vec![(1, u64::MAX)]);
    }

    #[test]
    fn complement_inverts_the_gaps_and_leaves_no_empty_ones() {
        assert_eq!(
            ranges(&["-f", "2", "--complement"]),
            vec![(1, 1), (3, u64::MAX)]
        );
        assert_eq!(ranges(&["-f", "1", "--complement"]), vec![(2, u64::MAX)]);
        // Touching ranges leave no gap between them to invert.
        assert_eq!(
            ranges(&["-f", "1-2,3-4", "--complement"]),
            vec![(5, u64::MAX)]
        );
        // The complement of everything selects nothing at all.
        assert_eq!(ranges(&["-f", "1-", "--complement"]), Vec::new());
    }

    #[test]
    fn the_list_diagnostics_are_worded_by_mode() {
        assert_eq!(body(&fail(&["-f0"])), "fields are numbered from 1");
        assert_eq!(
            body(&fail(&["-c0"])),
            "byte/character positions are numbered from 1"
        );
        assert_eq!(body(&fail(&["-fx"])), "invalid field value ‘x’");
        assert_eq!(body(&fail(&["-cx"])), "invalid byte/character position ‘x’");
        assert_eq!(body(&fail(&["-f", "1-2-3"])), "invalid field range");
        assert_eq!(
            body(&fail(&["-c", "1-2-3"])),
            "invalid byte or character range"
        );
        assert_eq!(body(&fail(&["-f", "2-1"])), "invalid decreasing range");
        assert_eq!(
            body(&fail(&["-f", "-"])),
            "invalid range with no endpoint: -"
        );
        // An empty list is not "missing"; it is a zero index.
        assert_eq!(body(&fail(&["-f", ""])), "fields are numbered from 1");
    }

    #[test]
    fn a_number_too_large_names_the_whole_run_and_only_the_first() {
        assert_eq!(
            body(&fail(&["-f", "99999999999999999999999"])),
            "field number ‘99999999999999999999999’ is too large"
        );
        assert_eq!(
            body(&fail(&["-c", "99999999999999999999999,22"])),
            "byte/character offset ‘99999999999999999999999’ is too large"
        );
        // `u64::MAX` itself is refused: that value means "to end of line".
        assert_eq!(
            body(&fail(&["-f", "18446744073709551615"])),
            "field number ‘18446744073709551615’ is too large"
        );
        // One less is fine.
        assert_eq!(
            ranges(&["-f", "18446744073709551614"]),
            vec![(18_446_744_073_709_551_614, 18_446_744_073_709_551_614)]
        );
    }

    #[test]
    fn the_offending_text_is_echoed_with_gnulib_quote_not_shell_escaping() {
        // `quote()` escapes the way C does — a backslash inside becomes `\\`
        // — where shell-escaping would switch to double quotes. A single
        // quote needs no escape at all, because the marks around the value
        // are curly and a straight `'` cannot be mistaken for one of them.
        assert_eq!(body(&fail(&["-f", "a'b"])), "invalid field value ‘a'b’");
        assert_eq!(body(&fail(&["-f", "a\\b"])), "invalid field value ‘a\\\\b’");
    }

    // ---------------- the cross-checks ----------------

    #[test]
    fn a_list_is_required_and_only_one_of_them() {
        assert_eq!(
            body(&fail(&[])),
            "you must specify a list of bytes, characters, or fields"
        );
        assert_eq!(
            body(&fail(&["-f1", "-c1"])),
            "only one list may be specified"
        );
        // Two of the *same* option is the same error.
        assert_eq!(
            body(&fail(&["-f1", "-f2"])),
            "only one list may be specified"
        );
    }

    #[test]
    fn byte_mode_refuses_the_two_field_only_options() {
        assert_eq!(
            body(&fail(&["-b1", "-d,"])),
            "an input delimiter may be specified only when operating on fields"
        );
        assert_eq!(
            body(&fail(&["-b1", "-s"])),
            "suppressing non-delimited lines makes sense\n\tonly when operating on fields"
        );
        // The delimiter check comes first when both apply.
        assert_eq!(
            body(&fail(&["-b1", "-d,", "-s"])),
            "an input delimiter may be specified only when operating on fields"
        );
        // And both come before the list is even parsed, so a bad list behind
        // them is never reported.
        assert_eq!(
            body(&fail(&["-b0", "-d,"])),
            "an input delimiter may be specified only when operating on fields"
        );
    }

    #[test]
    fn the_delimiter_is_one_byte_and_an_empty_one_is_nul() {
        assert_eq!(parse(&["-f1", "-d", ":"]).delim, b':');
        assert_eq!(parse(&["-f1", "-d", ""]).delim, 0);
        assert_eq!(parse(&["-f1"]).delim, b'\t');
        assert_eq!(
            body(&fail(&["-f1", "-d", "ab"])),
            "the delimiter must be a single character"
        );
    }

    #[test]
    fn the_output_delimiter_defaults_to_the_input_one_and_empty_means_nul() {
        assert_eq!(parse(&["-f1", "-d:"]).output_delim, b":".to_vec());
        assert!(!parse(&["-f1", "-d:"]).output_delim_given);
        assert_eq!(
            parse(&["-f1", "--output-delimiter=XY"]).output_delim,
            b"XY".to_vec()
        );
        assert_eq!(parse(&["-f1", "--output-delimiter="]).output_delim, vec![0]);
        assert!(parse(&["-f1", "--output-delimiter="]).output_delim_given);
    }

    // ---------------- byte mode ----------------

    #[test]
    fn bytes_are_selected_in_input_order_and_written_once() {
        assert_eq!(cut(b"abcdefgh\nij\n", &["-b2,4"]), b"bd\nj\n");
        // A range beyond the end of the line contributes nothing.
        assert_eq!(cut(b"ab\n", &["-b1-100"]), b"ab\n");
        // The list's order does not change the output's.
        assert_eq!(cut(b"abcd\n", &["-b3,1"]), cut(b"abcd\n", &["-b1,3"]));
    }

    #[test]
    fn the_output_delimiter_separates_ranges_not_bytes() {
        assert_eq!(
            cut(b"abcdefgh\n", &["-b1-2,3-4", "--output-delimiter=."]),
            b"ab.cd\n"
        );
        // Merged into one range, so nothing separates anything.
        assert_eq!(
            cut(b"abcdefgh\n", &["-b1-2,2-4", "--output-delimiter=."]),
            b"abcd\n"
        );
        // Without the option there is no separator at all.
        assert_eq!(cut(b"abcdefgh\n", &["-b1-2,4-5"]), b"abde\n");
    }

    #[test]
    fn an_unterminated_last_line_gains_a_terminator() {
        assert_eq!(cut(b"abc", &["-b1-2"]), b"ab\n");
        // But an empty input produces nothing.
        assert_eq!(cut(b"", &["-b1-2"]), b"");
    }

    #[test]
    fn zero_terminated_changes_both_ends() {
        assert_eq!(cut(b"abc\0def\0", &["-b2", "-z"]), b"b\0e\0");
        // A newline is then an ordinary byte, and counts.
        assert_eq!(cut(b"a\nc\0", &["-b2", "-z"]), b"\n\0");
    }

    #[test]
    fn complement_in_byte_mode() {
        assert_eq!(
            cut(b"abcdefgh\nij\n", &["-b1-3", "--complement"]),
            b"defgh\n\n"
        );
        // Complementing everything leaves each line empty but still present.
        assert_eq!(cut(b"abc\n", &["-b1-", "--complement"]), b"\n");
    }

    // ---------------- field mode ----------------

    #[test]
    fn fields_are_split_on_the_delimiter() {
        let input = b"a:b:c\nd:e:f\n";
        assert_eq!(cut(input, &["-d:", "-f2"]), b"b\ne\n");
        assert_eq!(cut(input, &["-d:", "-f1,3"]), b"a:c\nd:f\n");
        assert_eq!(cut(input, &["-d:", "-f2-"]), b"b:c\ne:f\n");
        assert_eq!(cut(input, &["-d:", "-f-2"]), b"a:b\nd:e\n");
        // The default delimiter is a tab.
        assert_eq!(cut(b"a\tb\n", &["-f2"]), b"b\n");
    }

    #[test]
    fn a_line_with_no_delimiter_is_printed_whole_unless_s_is_given() {
        let input = b"a:b\nnodelim\nc:d\n";
        assert_eq!(cut(input, &["-d:", "-f2"]), b"b\nnodelim\nd\n");
        assert_eq!(cut(input, &["-d:", "-f2", "-s"]), b"b\nd\n");
        // A selected field that the line does not have is an empty line, and
        // `-s` does not suppress it — the line *is* delimited.
        assert_eq!(cut(b"a:b\n", &["-d:", "-f4", "-s"]), b"\n");
    }

    #[test]
    fn the_output_delimiter_replaces_the_input_one() {
        let input = b"a:b:c\n";
        assert_eq!(cut(input, &["-d:", "-f1,3"]), b"a:c\n");
        assert_eq!(
            cut(input, &["-d:", "-f1,3", "--output-delimiter=XX"]),
            b"aXXc\n"
        );
        // A non-delimited line passes through untouched, delimiter or no.
        assert_eq!(
            cut(b"plain\n", &["-d:", "-f1,3", "--output-delimiter=XX"]),
            b"plain\n"
        );
    }

    #[test]
    fn an_unterminated_last_field_line_gains_a_terminator() {
        assert_eq!(cut(b"a:b", &["-d:", "-f2"]), b"b\n");
        assert_eq!(cut(b"nodelim", &["-d:", "-f1"]), b"nodelim\n");
        assert_eq!(cut(b"", &["-d:", "-f1"]), b"");
    }

    #[test]
    fn a_newline_delimiter_does_not_double_as_a_line_terminator() {
        // Every line is one field, so `-f1` prints the first line only.
        assert_eq!(cut(b"a\nb\nc\n", &["-d\n", "-f1"]), b"a\n");
    }

    #[test]
    fn complement_in_field_mode() {
        assert_eq!(cut(b"a:b:c\n", &["-d:", "-f1", "--complement"]), b"b:c\n");
        // Nothing selected still prints the line's terminator.
        assert_eq!(cut(b"a:b:c\n", &["-d:", "-f1-3", "--complement"]), b"\n");
    }

    #[test]
    fn a_nul_delimiter_is_reachable_through_an_empty_argument() {
        assert_eq!(cut(b"a\0b\0c\n", &["-d", "", "-f2"]), b"b\n");
    }
}
