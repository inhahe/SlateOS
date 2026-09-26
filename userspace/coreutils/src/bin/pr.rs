//! `pr` — paginate or columnate files for printing.
//!
//! ```text
//! Usage: pr [OPTION]... [FILE]...
//! ```
//!
//! A port of GNU coreutils 9.4's `src/pr.c`. POSIX specifies `pr`, and its
//! output is a layout -- a five-line header of date, name and page number,
//! the text, and a five-line trailer, 66 lines to the page -- so the port is
//! judged by the byte: `scripts/pr-diff.sh` runs each case through both.
//!
//! # How the file is arranged
//!
//! Upstream is one file of file-scope globals and the functions that read and
//! write them, and almost all of its behaviour lives in how those functions
//! interact: whether a form feed ends a page or only a column, when a pending
//! separator is finally written, what a line number does to the width of the
//! column after it. So the globals are one struct here, [`Pr`], and the
//! functions its methods, **with upstream's names**: `print_page`,
//! `read_line`, `store_columns`, `char_to_clump` and the rest can be read side
//! by side with `pr.c`. That is worth more for this program than any
//! restructuring would be, because the thing to check is sameness.
//!
//! Upstream's `COLUMN *p` is an index into [`Pr::column_vector`], and its
//! `FILE *` an index into the inputs, since several columns share one stream
//! when a single file is printed in several columns. `getc` and `ungetc` are
//! reproduced with glibc's semantics, including the sticky end-of-file flag
//! that `ungetc` clears.
//!
//! # Where this differs from upstream on purpose
//!
//! - **Upstream's undefined behaviour is not reproduced.** A `FIXME` in
//!   `print_stored` notes a read of uninitialised memory, and many `int`
//!   computations can overflow for enormous option values. Positions here are
//!   `i64`, so those overflow nowhere, and the one read past what was stored
//!   yields nothing rather than whatever the heap held.
//! - **Widths are UTF-8 in every locale** ([`coreutils::mbswidth`]). Upstream
//!   centres its header by `mbswidth`, which under `LC_ALL=C` counts a byte as
//!   a column; the string layer here has no single-byte locale
//!   (`design-decisions.md` §356). ASCII headers are the same either way.
//! - **A failing `fclose` of an input is not reported.** Upstream checks it;
//!   closing a file that was only read cannot fail in any way this port can
//!   provoke, and Rust's `File` does not return the answer.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::locale::{Category, hard_locale};
use coreutils::mbswidth::mbswidth;
use coreutils::quote::{os_bytes, quote, quotef};
use coreutils::stdfd::{self, Stream};
use coreutils::xnum::{self, Status};
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

coreutils::guard_std_fds!();

const PR: Program = Program::new("pr", 1);

/// Upstream's `short_options`, verbatim.
///
/// The leading `-` is glibc's return-in-order mode: every operand comes back
/// through the option loop in argv order, which is how a `+FIRST_PAGE` operand
/// is seen at all, and `POSIXLY_CORRECT` is never consulted -- measured,
/// `POSIXLY_CORRECT=1 pr f -t` still applies the `-t`. [`Program::parse`] reads
/// the prefix the way glibc does; [`getopt::Parser::stopped`] supplies the one
/// part of the mode an iterator cannot carry.
const SHORT_OPTIONS: &str = "-0123456789D:FJN:S::TW:abcde::fh:i::l:mn::o:rs::tvw:";

/// Upstream's `long_options`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("pages", Takes::Required),
    ("columns", Takes::Required),
    ("across", Takes::Nothing),
    ("show-control-chars", Takes::Nothing),
    ("double-space", Takes::Nothing),
    ("date-format", Takes::Required),
    ("expand-tabs", Takes::Optional),
    ("form-feed", Takes::Nothing),
    ("header", Takes::Required),
    ("output-tabs", Takes::Optional),
    ("join-lines", Takes::Nothing),
    ("length", Takes::Required),
    ("merge", Takes::Nothing),
    ("number-lines", Takes::Optional),
    ("first-line-number", Takes::Required),
    ("indent", Takes::Required),
    ("no-file-warnings", Takes::Nothing),
    ("separator", Takes::Optional),
    ("sep-string", Takes::Optional),
    ("omit-header", Takes::Nothing),
    ("omit-pagination", Takes::Nothing),
    ("show-nonprinting", Takes::Nothing),
    ("width", Takes::Required),
    ("page-width", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// The short option each long one stands for, where there is one.
const LONG_TO_SHORT: &[(&str, u8)] = &[
    ("across", b'a'),
    ("show-control-chars", b'c'),
    ("double-space", b'd'),
    ("date-format", b'D'),
    ("expand-tabs", b'e'),
    ("form-feed", b'f'),
    ("header", b'h'),
    ("output-tabs", b'i'),
    ("join-lines", b'J'),
    ("length", b'l'),
    ("merge", b'm'),
    ("number-lines", b'n'),
    ("first-line-number", b'N'),
    ("indent", b'o'),
    ("no-file-warnings", b'r'),
    ("separator", b's'),
    ("sep-string", b'S'),
    ("omit-header", b't'),
    ("omit-pagination", b'T'),
    ("show-nonprinting", b'v'),
    ("width", b'w'),
    ("page-width", b'W'),
];

/// `ANYWHERE`: a column that may begin at any horizontal position.
const ANYWHERE: i64 = 0;
/// `lines_per_header` and `lines_per_footer`.
const LINES_PER_HEADER: i64 = 5;
const LINES_PER_FOOTER: i64 = 5;
/// The form feed.
const FF: u8 = 0x0c;
/// C's `INT_MAX`, the bound upstream's `int` options are held to.
const INT_MAX: i64 = 2_147_483_647;
const INT_MIN: i64 = -2_147_483_648;

fn help_text() -> &'static str {
    "\
Usage: pr [OPTION]... [FILE]...
Paginate or columnate FILE(s) for printing.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  +FIRST_PAGE[:LAST_PAGE], --pages=FIRST_PAGE[:LAST_PAGE]
                    begin [stop] printing with page FIRST_[LAST_]PAGE
  -COLUMN, --columns=COLUMN
                    output COLUMN columns and print columns down,
                    unless -a is used. Balance number of lines in the
                    columns on each page
  -a, --across      print columns across rather than down, used together
                    with -COLUMN
  -c, --show-control-chars
                    use hat notation (^G) and octal backslash notation
  -d, --double-space
                    double space the output
  -D, --date-format=FORMAT
                    use FORMAT for the header date
  -e[CHAR[WIDTH]], --expand-tabs[=CHAR[WIDTH]]
                    expand input CHARs (TABs) to tab WIDTH (8)
  -F, -f, --form-feed
                    use form feeds instead of newlines to separate pages
                    (by a 3-line page header with -F or a 5-line header
                    and trailer without -F)
  -h, --header=HEADER
                    use a centered HEADER instead of filename in page header,
                    -h \"\" prints a blank line, don't use -h\"\"
  -i[CHAR[WIDTH]], --output-tabs[=CHAR[WIDTH]]
                    replace spaces with CHARs (TABs) to tab WIDTH (8)
  -J, --join-lines  merge full lines, turns off -W line truncation, no column
                    alignment, --sep-string[=STRING] sets separators
  -l, --length=PAGE_LENGTH
                    set the page length to PAGE_LENGTH (66) lines
                    (default number of lines of text 56, and with -F 63).
                    implies -t if PAGE_LENGTH <= 10
  -m, --merge       print all files in parallel, one in each column,
                    truncate lines, but join lines of full length with -J
  -n[SEP[DIGITS]], --number-lines[=SEP[DIGITS]]
                    number lines, use DIGITS (5) digits, then SEP (TAB),
                    default counting starts with 1st line of input file
  -N, --first-line-number=NUMBER
                    start counting with NUMBER at 1st line of first
                    page printed (see +FIRST_PAGE)
  -o, --indent=MARGIN
                    offset each line with MARGIN (zero) spaces, do not
                    affect -w or -W, MARGIN will be added to PAGE_WIDTH
  -r, --no-file-warnings
                    omit warning when a file cannot be opened
  -s[CHAR], --separator[=CHAR]
                    separate columns by a single character, default for CHAR
                    is the <TAB> character without -w and 'no char' with -w.
                    -s[CHAR] turns off line truncation of all 3 column
                    options (-COLUMN|-a -COLUMN|-m) except -w is set
  -S[STRING], --sep-string[=STRING]
                    separate columns by STRING,
                    without -S: Default separator <TAB> with -J and <space>
                    otherwise (same as -S\" \"), no effect on column options
  -t, --omit-header  omit page headers and trailers;
                     implied if PAGE_LENGTH <= 10
  -T, --omit-pagination
                    omit page headers and trailers, eliminate any pagination
                    by form feeds set in input files
  -v, --show-nonprinting
                    use octal backslash notation
  -w, --width=PAGE_WIDTH
                    set page width to PAGE_WIDTH (72) characters for
                    multiple text-column output only, -s[char] turns off (72)
  -W, --page-width=PAGE_WIDTH
                    set page width to PAGE_WIDTH (72) characters always,
                    truncate lines, except -J option is set, no interference
                    with -S or -s
      --help        display this help and exit
      --version     output version information and exit
"
}

// ------------------------------------------------------------- options ---

/// Upstream's option globals, as the command line leaves them.
///
/// The engine goes on to change several of these -- `init_parameters` turns
/// on truncation and tabification for any multi-column layout, for one --
/// which is why [`Pr`] owns a copy rather than borrowing one.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "upstream's switches, one per option letter and each read on its own"
)]
struct Options {
    parallel_files: bool,
    explicit_columns: bool,
    extremities: bool,
    keep_ff: bool,
    use_form_feed: bool,
    print_across_flag: bool,
    storing_columns: bool,
    balance_columns: bool,
    lines_per_page: i64,
    chars_per_line: i64,
    truncate_lines: bool,
    join_lines: bool,
    untabify_input: bool,
    input_tab_char: u8,
    chars_per_input_tab: i64,
    tabify_output: bool,
    output_tab_char: u8,
    chars_per_output_tab: i64,
    chars_per_margin: i64,
    columns: i64,
    first_page_number: u64,
    last_page_number: u64,
    numbered_lines: bool,
    number_separator: u8,
    skip_count: bool,
    start_line_num: i64,
    chars_per_number: i64,
    use_esc_sequence: bool,
    use_cntrl_prefix: bool,
    double_space: bool,
    ignore_failed_opens: bool,
    use_col_separator: bool,
    col_sep_string: Vec<u8>,
    custom_header: Option<Vec<u8>>,
    /// `-D`, or `None` for the default, which depends on the environment and
    /// is chosen in `main`.
    date_format: Option<Vec<u8>>,
    files: Vec<OsString>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            parallel_files: false,
            explicit_columns: false,
            extremities: true,
            keep_ff: false,
            use_form_feed: false,
            print_across_flag: false,
            storing_columns: true,
            balance_columns: false,
            lines_per_page: 66,
            chars_per_line: 72,
            truncate_lines: false,
            join_lines: false,
            untabify_input: false,
            input_tab_char: b'\t',
            chars_per_input_tab: 8,
            tabify_output: false,
            output_tab_char: b'\t',
            chars_per_output_tab: 8,
            chars_per_margin: 0,
            columns: 1,
            first_page_number: 0,
            last_page_number: u64::MAX,
            numbered_lines: false,
            number_separator: b'\t',
            skip_count: true,
            start_line_num: 1,
            chars_per_number: 5,
            use_esc_sequence: false,
            use_cntrl_prefix: false,
            double_space: false,
            ignore_failed_opens: false,
            use_col_separator: false,
            col_sep_string: Vec::new(),
            custom_header: None,
            date_format: None,
            files: Vec::new(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Run(Box<Options>),
}

/// Upstream's `getoptnum`: `n_str` as an `int` no smaller than `min`.
///
/// # Errors
///
/// `xdectoimax`'s diagnostic, naming the quantity as `what` does.
fn getoptnum(n_str: &[u8], min: i64, what: &str) -> Result<i64, getopt::Error> {
    xnum::xdectoimax(n_str, min, INT_MAX, Some(b""), what).map_err(|message| PR.usage(message))
}

/// Upstream's `getoptarg`: the `CHAR[NUMBER]` argument of `-e`, `-i` and `-n`.
/// A first byte that is not a digit is the character; what follows it, if
/// anything, is the number, which must be positive and fit an `int`.
///
/// # Errors
///
/// An empty argument, or a number that is not one; both refer to `--help`.
fn getoptarg(
    arg: &[u8],
    switch_char: u8,
    character: &mut u8,
    number: &mut i64,
) -> Result<(), getopt::Error> {
    let switch = char::from(switch_char);
    let Some(&first) = arg.first() else {
        return Err(PR.usage_referring(format!("'-{switch}': Invalid argument: {}", quote(arg))));
    };
    let mut rest = arg;
    if !first.is_ascii_digit() {
        *character = first;
        rest = arg.get(1..).unwrap_or_default();
    }
    if rest.is_empty() {
        return Ok(());
    }
    let (value, status) = xnum::xstrtoimax(rest, Some(b""));
    let status = match status {
        Status::Ok if value <= 0 => Status::Invalid,
        Status::Ok if value > INT_MAX => Status::Overflow,
        other => other,
    };
    if status == Status::Ok {
        *number = value;
        return Ok(());
    }
    // `e & LONGINT_OVERFLOW ? EOVERFLOW : 0`: the overflow bit alone decides,
    // so a number both too large and followed by junk still carries the tail.
    let tail = match status {
        Status::Overflow | Status::InvalidSuffixWithOverflow => {
            ": Value too large for defined data type"
        }
        Status::Ok | Status::Invalid | Status::InvalidSuffix => "",
    };
    Err(PR.usage_referring(format!(
        "'-{switch}' extra characters or invalid number in the argument: {}{tail}",
        quote(rest)
    )))
}

/// Upstream's `first_last_page`: `FIRST[:LAST]`, from `+FIRST[:LAST]` or
/// `--pages`. `option` names which, as `xstrtol_fatal` prints it.
///
/// `Ok(false)` for text that is not a page range but not an error either --
/// a `+` operand like that is a file name, and `--pages` calls it an invalid
/// range.
///
/// # Errors
///
/// A number the conversion refused outright.
fn first_last_page(o: &mut Options, option: &str, pages: &[u8]) -> Result<bool, getopt::Error> {
    let fatal =
        |status: Status| PR.usage(xnum::strtol_fatal(status, option, pages).unwrap_or_default());
    let (first, status, end) = xnum::xstrtoumax_end(pages, 10, Some(b""));
    if status != Status::Ok && status != Status::InvalidSuffix {
        return Err(fatal(status));
    }
    if end == 0 || first == 0 {
        return Ok(false);
    }
    let mut last = u64::MAX;
    let mut at = end;
    if pages.get(at) == Some(&b':') {
        let p1 = at.saturating_add(1);
        let tail = pages.get(p1..).unwrap_or_default();
        let (value, status, len) = xnum::xstrtoumax_end(tail, 10, Some(b""));
        if status != Status::Ok {
            return Err(fatal(status));
        }
        if len == 0 || value < first {
            return Ok(false);
        }
        last = value;
        at = p1.saturating_add(len);
    }
    if at < pages.len() {
        return Ok(false);
    }
    o.first_page_number = first;
    o.last_page_number = last;
    Ok(true)
}

/// Upstream's `main`, up to where it starts printing: the option loop and the
/// translations of the old `-s` and `-w` into the new options.
///
/// # Errors
///
/// Any refused option or argument, with upstream's wording and status.
#[allow(
    clippy::too_many_lines,
    reason = "one arm per option, as upstream's switch; splitting it would hide the order they are read in"
)]
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut o = Options::default();
    let mut old_options = false;
    let mut old_w = false;
    let mut old_s = false;
    // `column_count_string` and `n_digits`: the digits of `-NNN` so far, which
    // a later digit option overwrites from the start once any other option
    // has come between.
    let mut column_count: Option<Vec<u8>> = None;
    let mut n_digits = 0usize;

    let mut parser = PR.parse(args, SHORT_OPTIONS, LONG_OPTIONS);
    while let Some(item) = parser.next() {
        let item = item?;
        let stopped = parser.stopped();
        let (letter, value) = match item {
            Opt::Short(c, v) => (c, v),
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Long("pages", v) => {
                n_digits = 0;
                let text = v.map(|v| os_bytes(&v).into_owned()).unwrap_or_default();
                if !first_last_page(&mut o, "--pages", &text)? {
                    return Err(PR.usage(format!("invalid page range {}", quote(&text))));
                }
                continue;
            }
            Opt::Long("columns", v) => {
                n_digits = 0;
                let text = v.map(|v| os_bytes(&v).into_owned()).unwrap_or_default();
                o.columns = getoptnum(&text, 1, "invalid number of columns")?;
                o.explicit_columns = true;
                // A `-NNN` before it is overridden, not combined.
                column_count = None;
                continue;
            }
            Opt::Long(name, v) => match LONG_TO_SHORT.iter().find(|(long, _)| *long == name) {
                Some(&(_, c)) => (c, v),
                None => {
                    return Err(PR.usage_referring(format!("option '--{name}' is unhandled")));
                }
            },
            Opt::Operand(word) => {
                n_digits = 0;
                // `case 1`: an operand met in order. Once getopt has stopped,
                // upstream collects the rest as file names untested.
                let bytes = os_bytes(word);
                let is_pages = !stopped
                    && o.first_page_number == 0
                    && bytes.first() == Some(&b'+')
                    && first_last_page(&mut o, "+", bytes.get(1..).unwrap_or_default())?;
                if !is_pages {
                    o.files.push(word.clone());
                }
                continue;
            }
        };

        if letter.is_ascii_digit() {
            let digits = column_count.get_or_insert_with(Vec::new);
            digits.truncate(n_digits);
            digits.push(letter);
            n_digits = n_digits.saturating_add(1);
            continue;
        }
        n_digits = 0;

        let value = value.map(|v| os_bytes(&v).into_owned());
        let arg = value.as_deref();
        match letter {
            b'a' => {
                o.print_across_flag = true;
                o.storing_columns = false;
            }
            b'b' => o.balance_columns = true,
            b'c' => o.use_cntrl_prefix = true,
            b'd' => o.double_space = true,
            b'D' => o.date_format = value.clone(),
            b'e' => {
                if let Some(arg) = arg {
                    getoptarg(arg, b'e', &mut o.input_tab_char, &mut o.chars_per_input_tab)?;
                }
                o.untabify_input = true;
            }
            b'f' | b'F' => o.use_form_feed = true,
            b'h' => o.custom_header = value.clone(),
            b'i' => {
                if let Some(arg) = arg {
                    getoptarg(
                        arg,
                        b'i',
                        &mut o.output_tab_char,
                        &mut o.chars_per_output_tab,
                    )?;
                }
                o.tabify_output = true;
            }
            b'J' => o.join_lines = true,
            b'l' => {
                o.lines_per_page = getoptnum(
                    arg.unwrap_or_default(),
                    1,
                    "'-l PAGE_LENGTH' invalid number of lines",
                )?;
            }
            b'm' => {
                o.parallel_files = true;
                o.storing_columns = false;
            }
            b'n' => {
                o.numbered_lines = true;
                if let Some(arg) = arg {
                    getoptarg(arg, b'n', &mut o.number_separator, &mut o.chars_per_number)?;
                }
            }
            b'N' => {
                o.skip_count = false;
                o.start_line_num = getoptnum(
                    arg.unwrap_or_default(),
                    INT_MIN,
                    "'-N NUMBER' invalid starting line number",
                )?;
            }
            b'o' => {
                o.chars_per_margin = getoptnum(
                    arg.unwrap_or_default(),
                    0,
                    "'-o MARGIN' invalid line offset",
                )?;
            }
            b'r' => o.ignore_failed_opens = true,
            b's' => {
                old_options = true;
                old_s = true;
                if !o.use_col_separator
                    && let Some(arg) = arg
                {
                    o.col_sep_string = arg.to_vec();
                }
            }
            b'S' => {
                old_s = false;
                // `-S` dominates `-s`: whatever `-s` set is reset.
                o.col_sep_string = Vec::new();
                o.use_col_separator = true;
                if let Some(arg) = arg {
                    o.col_sep_string = arg.to_vec();
                }
            }
            b't' => {
                o.extremities = false;
                o.keep_ff = true;
            }
            b'T' => {
                o.extremities = false;
                o.keep_ff = false;
            }
            b'v' => o.use_esc_sequence = true,
            b'w' => {
                old_options = true;
                old_w = true;
                let width = getoptnum(
                    arg.unwrap_or_default(),
                    1,
                    "'-w PAGE_WIDTH' invalid number of characters",
                )?;
                if !o.truncate_lines {
                    o.chars_per_line = width;
                }
            }
            b'W' => {
                old_w = false;
                o.truncate_lines = true;
                o.chars_per_line = getoptnum(
                    arg.unwrap_or_default(),
                    1,
                    "'-W PAGE_WIDTH' invalid number of characters",
                )?;
            }
            // Unreachable: every letter of the table is handled above.
            other => return Err(PR.invalid_option(other)),
        }
    }

    if let Some(digits) = column_count {
        o.columns = getoptnum(&digits, 1, "invalid number of columns")?;
        o.explicit_columns = true;
    }
    if o.first_page_number == 0 {
        o.first_page_number = 1;
    }
    if o.parallel_files && o.explicit_columns {
        return Err(
            PR.usage("cannot specify number of columns when printing in parallel".to_string())
        );
    }
    if o.parallel_files && o.print_across_flag {
        return Err(
            PR.usage("cannot specify both printing across and printing in parallel".to_string())
        );
    }

    // Translate the old `-s` and `-w` into the new options, for downward
    // compatibility with other UNIX `pr`s and with POSIX.
    if old_options {
        if old_w {
            if o.parallel_files || o.explicit_columns {
                // Activate -W.
                o.truncate_lines = true;
                if old_s {
                    // HP-UX and SunOS: -s means no separator; activate -S.
                    o.use_col_separator = true;
                }
            } else {
                // The old -w sets a width with columns only; activate -J.
                o.join_lines = true;
            }
        } else if !o.use_col_separator && old_s && (o.parallel_files || o.explicit_columns) {
            if o.truncate_lines {
                // With -W: HP-UX and SunOS, -s means no separator.
                o.use_col_separator = true;
            } else {
                // The old -s without -w or -W annuls column alignment and uses
                // fields: activate -J, and -S if a separator was given.
                o.join_lines = true;
                if !o.col_sep_string.is_empty() {
                    o.use_col_separator = true;
                }
            }
        }
    }
    Ok(Request::Run(Box::new(o)))
}

// --------------------------------------------------------------- input ---

/// Where an input's bytes come from.
enum Reader {
    /// Descriptor 0, read with [`stdfd::read`] so that a closed one is
    /// `EBADF` and not an empty file.
    Stdin,
    File(std::fs::File),
    /// Test input.
    #[cfg(test)]
    Bytes(io::Cursor<Vec<u8>>),
    /// A file already closed.
    Closed,
}

/// A `FILE *` opened for reading: a buffer, one byte of `ungetc`, and the
/// end-of-file and error indicators.
struct Input {
    reader: Reader,
    buf: Vec<u8>,
    start: usize,
    end: usize,
    pushed: Option<u8>,
    /// glibc's end-of-file indicator. Sticky, as C99 requires: once set, `getc`
    /// answers `EOF` without reading again until `ungetc` or `clearerr`.
    eof: bool,
    /// The error indicator, holding the failure itself as `errno` would.
    error: Option<io::Error>,
    /// The modification time `fstat` reported when it was opened.
    mtime: Option<(i64, u32)>,
}

const INPUT_BUFFER: usize = 64 * 1024;

impl Input {
    fn new(reader: Reader, mtime: Option<(i64, u32)>) -> Self {
        Input {
            reader,
            buf: vec![0; INPUT_BUFFER],
            start: 0,
            end: 0,
            pushed: None,
            eof: false,
            error: None,
            mtime,
        }
    }

    /// `getc`: the next byte, or `None` for `EOF` -- the end of the input or
    /// a failure to read it, told apart by the error indicator.
    fn getc(&mut self) -> Option<u8> {
        if let Some(c) = self.pushed.take() {
            return Some(c);
        }
        if let Some(&c) = self.buf.get(self.start).filter(|_| self.start < self.end) {
            self.start = self.start.saturating_add(1);
            return Some(c);
        }
        if self.eof {
            return None;
        }
        let got = match &mut self.reader {
            Reader::Stdin => stdfd::read(0, &mut self.buf),
            Reader::File(f) => loop {
                match f.read(&mut self.buf) {
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    other => break other,
                }
            },
            #[cfg(test)]
            Reader::Bytes(c) => c.read(&mut self.buf),
            Reader::Closed => Ok(0),
        };
        match got {
            Ok(0) => {
                self.eof = true;
                None
            }
            Ok(n) => {
                self.start = 1;
                self.end = n;
                self.buf.first().copied()
            }
            Err(e) => {
                self.error = Some(e);
                None
            }
        }
    }

    /// `ungetc`: push `c` back, clearing the end-of-file indicator. Pushing
    /// back `EOF` does nothing, as in C.
    fn ungetc(&mut self, c: Option<u8>) {
        if let Some(c) = c {
            self.pushed = Some(c);
            self.eof = false;
        }
    }
}

/// Which input a column reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Stdin,
    File(usize),
}

// -------------------------------------------------------------- engine ---

/// `COLUMN.status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileStatus {
    Open,
    /// Used with `-b`: a form feed was met while storing, changed to `OnHold`
    /// once the page's header is printed.
    FfFound,
    /// Hit a form feed.
    OnHold,
    Closed,
}

/// `COLUMN.print_func`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrintFunc {
    ReadLine,
    PrintStored,
}

/// `COLUMN.char_func`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharFunc {
    PrintChar,
    StoreChar,
}

/// Upstream's `COLUMN`: see the comment above its declaration in `pr.c`.
#[derive(Clone, Debug)]
struct Column {
    source: Source,
    /// The name diagnostics use: the operand, or `standard input`.
    name: Vec<u8>,
    status: FileStatus,
    print_func: PrintFunc,
    char_func: CharFunc,
    current_line: usize,
    lines_stored: i64,
    lines_to_print: i64,
    start_position: i64,
    numbered: bool,
    /// A full page was printed without a form feed, so a form feed first
    /// thing on the next line must not make an extra empty page.
    full_page_printed: bool,
}

impl Column {
    fn new(source: Source, name: Vec<u8>) -> Self {
        Column {
            source,
            name,
            status: FileStatus::Open,
            print_func: PrintFunc::ReadLine,
            char_func: CharFunc::PrintChar,
            current_line: 0,
            lines_stored: 0,
            lines_to_print: 0,
            start_position: 0,
            numbered: false,
            full_page_printed: false,
        }
    }
}

/// A failure that ends the run, as upstream's `error (EXIT_FAILURE, ...)`:
/// the message after `pr: `.
#[derive(Debug)]
struct Fatal(String);

/// `isprint` in the `C` and UTF-8 locales alike: a byte on its own is a
/// printable character only in ASCII's printable range.
fn isprint(c: u8) -> bool {
    (0x20..=0x7e).contains(&c)
}

/// `TAB_WIDTH (c, h)`: the spaces a tab of width `c` takes from position `h`.
/// C's `%` truncates toward zero, and so does Rust's.
fn tab_width(c: i64, h: i64) -> i64 {
    c.saturating_sub(h.checked_rem(c).unwrap_or(0))
}

/// `POS_AFTER_TAB (c, h)`.
fn pos_after_tab(c: i64, h: i64) -> i64 {
    h.saturating_add(tab_width(c, h))
}

/// A `SystemTime` as a `timespec`.
fn timespec(t: SystemTime) -> (i64, u32) {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => (
            i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
            d.subsec_nanos(),
        ),
        Err(before) => {
            let d = before.duration();
            let secs = i64::try_from(d.as_secs()).unwrap_or(i64::MAX);
            match d.subsec_nanos() {
                0 => (secs.saturating_neg(), 0),
                ns => (
                    secs.saturating_neg().saturating_sub(1),
                    1_000_000_000_u32.saturating_sub(ns),
                ),
            }
        }
    }
}

/// Upstream's file-scope state, and its functions as methods. See the module
/// docs for why the names and the shapes are upstream's.
struct Pr<W> {
    o: Options,
    date_format: Vec<u8>,
    zone: localtime::Zone,

    print_a_ff: bool,
    print_a_header: bool,
    have_read_stdin: bool,
    lines_per_body: i64,
    chars_per_column: i64,
    spaces_not_printed: i64,
    output_position: i64,
    input_position: i64,
    failed_opens: bool,
    files_ready_to_read: i64,
    page_number: u64,
    line_number: i64,
    line_count: i64,
    number_width: i64,
    total_files: i64,
    separators_not_printed: i64,
    padding_not_printed: i64,
    pad_vertically: bool,
    date_text: Vec<u8>,
    file_text: Vec<u8>,
    header_width_available: i64,
    clump_buff: Vec<u8>,
    last_line: bool,
    align_empty_cols: bool,
    empty_line: bool,
    ff_only: bool,
    /// The time a header uses when it has no file's to use: taken once, the
    /// first time it is needed, as upstream's `static struct timespec`.
    now: Option<(i64, u32)>,

    column_vector: Vec<Column>,
    buff: Vec<u8>,
    line_vector: Vec<usize>,
    end_vector: Vec<i64>,

    stdin: Input,
    files: Vec<Input>,
    /// Standard output, or a buffer under test.
    out: W,
}

// Positions and counts are C `int`s widened to `i64`: every operand is an
// option held to `INT_MAX` or a count of bytes and lines actually read, so no
// sum or product here approaches `i64`'s range, and none of upstream's `int`
// overflows can happen. The indices are column numbers below `columns` -- the
// length of `column_vector`, fixed once per file -- and stored-line numbers
// below the lengths `init_store_cols` gives `line_vector` and `end_vector`,
// both by construction, as in upstream; the one read that could pass the end
// (`print_stored`'s `FIXME`) uses `get`.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "see the comment above: C ints widened past their overflow, and indices bounded by construction"
)]
impl<W: Write> Pr<W> {
    fn new(o: Options, date_format: Vec<u8>, zone: localtime::Zone, out: W) -> Self {
        Pr {
            o,
            date_format,
            zone,
            print_a_ff: false,
            print_a_header: false,
            have_read_stdin: false,
            lines_per_body: 0,
            chars_per_column: 0,
            spaces_not_printed: 0,
            output_position: 0,
            input_position: 0,
            failed_opens: false,
            files_ready_to_read: 0,
            page_number: 0,
            line_number: 0,
            line_count: 1,
            number_width: 0,
            total_files: 0,
            separators_not_printed: 0,
            padding_not_printed: 0,
            pad_vertically: false,
            date_text: Vec::new(),
            file_text: Vec::new(),
            header_width_available: 0,
            clump_buff: Vec::new(),
            last_line: false,
            align_empty_cols: false,
            empty_line: false,
            ff_only: false,
            now: None,
            column_vector: Vec::new(),
            buff: Vec::new(),
            line_vector: Vec::new(),
            end_vector: Vec::new(),
            stdin: Input::new(Reader::Stdin, None),
            files: Vec::new(),
            out,
        }
    }

    /// `putchar`. Deliberately unread: a failed write is the `Stream`'s to
    /// remember and `close_stdout`'s to report, once.
    fn put(&mut self, c: u8) {
        let _ = self.out.write_all(&[c]);
    }

    /// `fputs`, for the header.
    fn put_bytes(&mut self, bytes: &[u8]) {
        let _ = self.out.write_all(bytes);
    }

    fn col_sep_length(&self) -> i64 {
        i64::try_from(self.o.col_sep_string.len()).unwrap_or(INT_MAX)
    }

    fn columns(&self) -> usize {
        self.column_vector.len()
    }

    fn input(&mut self, source: Source) -> &mut Input {
        match source {
            Source::Stdin => &mut self.stdin,
            Source::File(i) => &mut self.files[i],
        }
    }

    fn getc(&mut self, pi: usize) -> Option<u8> {
        let source = self.column_vector[pi].source;
        self.input(source).getc()
    }

    fn ungetc(&mut self, pi: usize, c: Option<u8>) {
        let source = self.column_vector[pi].source;
        self.input(source).ungetc(c);
    }

    /// The whole run: every file, or all of them at once under `-m`.
    fn run(&mut self) -> ExitCode {
        let files = std::mem::take(&mut self.o.files);
        let result = if files.is_empty() {
            self.print_files(&[])
        } else if self.o.parallel_files {
            self.print_files(&files)
        } else {
            files
                .iter()
                .try_for_each(|f| self.print_files(std::slice::from_ref(f)))
        };
        if let Err(Fatal(message)) = result {
            diag!("pr: {message}");
            return ExitCode::FAILURE;
        }
        // Upstream then checks `fclose (stdin)`, which cannot fail on a
        // descriptor every read of which has already succeeded.
        if self.failed_opens {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        }
    }

    /// `cols_ready_to_print`: the columns with an open file or stored lines.
    fn cols_ready_to_print(&self) -> usize {
        self.column_vector
            .iter()
            .filter(|q| {
                q.status == FileStatus::Open
                    || q.status == FileStatus::FfFound
                    || (self.o.storing_columns && q.lines_stored > 0 && q.lines_to_print > 0)
            })
            .count()
    }

    /// `init_parameters`: the formatting parameters for one file, or for all
    /// of them under `-m`.
    fn init_parameters(&mut self, number_of_files: usize) -> Result<(), Fatal> {
        let mut chars_used_by_number = 0;

        self.lines_per_body = self.o.lines_per_page - LINES_PER_HEADER - LINES_PER_FOOTER;
        if self.lines_per_body <= 0 {
            self.o.extremities = false;
            self.o.keep_ff = true;
        }
        if !self.o.extremities {
            self.lines_per_body = self.o.lines_per_page;
        }
        if self.o.double_space {
            self.lines_per_body = (self.lines_per_body / 2).max(1);
        }

        // Standard input cannot be printed in parallel.
        if number_of_files == 0 {
            self.o.parallel_files = false;
        }
        if self.o.parallel_files {
            self.o.columns = i64::try_from(number_of_files).unwrap_or(INT_MAX);
        }

        // One file, several columns down: -b is set too, which makes a form
        // feed in the input consistent with the balancing on the last page.
        if self.o.storing_columns {
            self.o.balance_columns = true;
        }

        // Tabification is assumed for multiple columns.
        if self.o.columns > 1 {
            if !self.o.use_col_separator {
                // The default separator.
                self.o.col_sep_string = if self.o.join_lines { b"\t" } else { b" " }.to_vec();
                self.o.use_col_separator = true;
            } else if !self.o.join_lines && self.o.col_sep_string == b"\t" {
                // A TAB separator is pointless with column alignment.
                self.o.col_sep_string = b" ".to_vec();
            }
            self.o.truncate_lines = true;
            if self.o.col_sep_string != b"\t" {
                self.o.untabify_input = true;
            }
            self.o.tabify_output = true;
        } else {
            self.o.storing_columns = false;
        }

        // -J dominates -w in any case.
        if self.o.join_lines {
            self.o.truncate_lines = false;
        }

        if self.o.numbered_lines {
            let chars_per_default_tab = 8;
            self.line_count = self.o.start_line_num;
            // The width without any margin, kept constant.
            self.number_width = if self.o.number_separator == b'\t' {
                self.o.chars_per_number + tab_width(chars_per_default_tab, self.o.chars_per_number)
            } else {
                self.o.chars_per_number + 1
            };
            // The number is part of the column width unless files are printed
            // in parallel.
            if self.o.parallel_files {
                chars_used_by_number = self.number_width;
            }
        }

        // `ckd_mul`, then `ckd_sub`, in `int`: an overflowing product is
        // `INT_MAX`, and an overflowing difference is zero.
        let mut sep_chars = (self.o.columns - 1) * self.col_sep_length();
        if !(INT_MIN..=INT_MAX).contains(&sep_chars) {
            sep_chars = INT_MAX;
        }
        let mut useful_chars = self.o.chars_per_line - chars_used_by_number - sep_chars;
        if !(INT_MIN..=INT_MAX).contains(&useful_chars) {
            useful_chars = 0;
        }
        self.chars_per_column = useful_chars / self.o.columns.max(1);
        if self.chars_per_column < 1 {
            return Err(Fatal("page width too narrow".to_string()));
        }
        Ok(())
    }

    /// `init_fps`: open the files and make a column for each column of
    /// output. `false` if nothing could be opened.
    fn init_fps(&mut self, av: &[OsString]) -> bool {
        self.total_files = 0;
        self.column_vector.clear();
        self.files.clear();

        if self.o.parallel_files {
            for name in av {
                if let Some(column) = self.open_file(name) {
                    self.column_vector.push(column);
                }
            }
            self.o.columns = i64::try_from(self.column_vector.len()).unwrap_or(INT_MAX);
            if self.column_vector.is_empty() {
                return false;
            }
            self.init_header(None, None);
        } else {
            let first = match av.first() {
                Some(name) => {
                    let Some(column) = self.open_file(name) else {
                        return false;
                    };
                    let bytes = os_bytes(name).into_owned();
                    match column.source {
                        // `STREQ (filename, "-")`: no descriptor to stat.
                        Source::Stdin => self.init_header(None, None),
                        Source::File(i) => {
                            let mtime = self.files[i].mtime;
                            self.init_header(Some(&bytes), mtime);
                        }
                    }
                    column
                }
                None => {
                    self.have_read_stdin = true;
                    self.total_files += 1;
                    self.init_header(None, None);
                    Column::new(Source::Stdin, b"standard input".to_vec())
                }
            };
            let columns = usize::try_from(self.o.columns).unwrap_or(1).max(1);
            let mut rest = first.clone();
            rest.status = FileStatus::Open;
            rest.full_page_printed = false;
            rest.lines_stored = 0;
            self.column_vector.push(first);
            for _ in 1..columns {
                self.column_vector.push(rest.clone());
            }
        }
        self.files_ready_to_read = self.total_files;
        true
    }

    /// `init_funcs`: how each column prints and stores, and where it starts.
    fn init_funcs(&mut self) {
        let sep = self.col_sep_length();
        let mut h = self.o.chars_per_margin;
        let mut h_next = if !self.o.truncate_lines {
            ANYWHERE
        } else if self.o.parallel_files && self.o.numbered_lines {
            // Numbering parallel files widens the first column to hold the
            // number.
            h + self.chars_per_column + self.number_width
        } else {
            h + self.chars_per_column
        };

        // The first column's start moves by the separator too, so that every
        // column's padding works alike.
        h += sep;

        let columns = self.columns();
        for i in 1..columns {
            let p = &mut self.column_vector[i - 1];
            if self.o.storing_columns {
                p.char_func = CharFunc::StoreChar;
                p.print_func = PrintFunc::PrintStored;
            } else {
                p.char_func = CharFunc::PrintChar;
                p.print_func = PrintFunc::ReadLine;
            }
            // Only the first column is numbered when files are in parallel.
            p.numbered = self.o.numbered_lines && (!self.o.parallel_files || i == 1);
            p.start_position = h;
            if self.o.truncate_lines {
                h = h_next + sep;
                h_next = h + self.chars_per_column;
            } else {
                h = ANYWHERE;
                h_next = ANYWHERE;
            }
        }

        // The rightmost column is stored only to balance the last page.
        let storing = self.o.storing_columns && self.o.balance_columns;
        let numbered = self.o.numbered_lines && (!self.o.parallel_files || columns == 1);
        if let Some(p) = self.column_vector.last_mut() {
            if storing {
                p.char_func = CharFunc::StoreChar;
                p.print_func = PrintFunc::PrintStored;
            } else {
                p.char_func = CharFunc::PrintChar;
                p.print_func = PrintFunc::ReadLine;
            }
            p.numbered = numbered;
            p.start_position = h;
        }
    }

    /// `open_file`: a column for `name`, or `None` after saying why not.
    fn open_file(&mut self, name: &OsString) -> Option<Column> {
        let bytes = os_bytes(name).into_owned();
        let column = if bytes == b"-" {
            self.have_read_stdin = true;
            Column::new(Source::Stdin, b"standard input".to_vec())
        } else {
            match std::fs::File::open(name) {
                Ok(f) => {
                    let mtime = f
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .map(timespec);
                    self.files.push(Input::new(Reader::File(f), mtime));
                    Column::new(Source::File(self.files.len() - 1), bytes)
                }
                Err(why) => {
                    self.failed_opens = true;
                    if !self.o.ignore_failed_opens {
                        diag!("pr: {}: {}", quotef(&bytes), strerror(&why));
                    }
                    return None;
                }
            }
        };
        self.total_files += 1;
        Some(column)
    }

    /// `close_file`: done with the file in column `pi`, and so, unless files
    /// are in parallel, with every column.
    fn close_file(&mut self, pi: usize) -> Result<(), Fatal> {
        if self.column_vector[pi].status == FileStatus::Closed {
            return Ok(());
        }
        let source = self.column_vector[pi].source;
        let input = self.input(source);
        let failure = input.error.take();
        match source {
            // `clearerr`: standard input stays open for a later `-`.
            Source::Stdin => input.eof = false,
            Source::File(_) => {
                input.reader = Reader::Closed;
                input.buf = Vec::new();
            }
        }
        if let Some(why) = failure {
            let name = quotef(&self.column_vector[pi].name);
            return Err(Fatal(format!("{name}: {}", strerror(&why))));
        }

        if self.o.parallel_files {
            let p = &mut self.column_vector[pi];
            p.status = FileStatus::Closed;
            p.lines_to_print = 0;
        } else {
            for q in &mut self.column_vector {
                q.status = FileStatus::Closed;
                if q.lines_stored == 0 {
                    q.lines_to_print = 0;
                }
            }
        }
        self.files_ready_to_read -= 1;
        Ok(())
    }

    /// `hold_file`: a form feed was hit, so the file waits for the next page.
    fn hold_file(&mut self, pi: usize) {
        if self.o.parallel_files {
            self.column_vector[pi].status = FileStatus::OnHold;
        } else {
            let status = if self.o.storing_columns {
                FileStatus::FfFound
            } else {
                FileStatus::OnHold
            };
            for q in &mut self.column_vector {
                q.status = status;
            }
        }
        self.column_vector[pi].lines_to_print = 0;
        self.files_ready_to_read -= 1;
    }

    /// `reset_status`: every file on hold is open again, at a page's end.
    fn reset_status(&mut self) {
        for p in &mut self.column_vector {
            if p.status == FileStatus::OnHold {
                p.status = FileStatus::Open;
                self.files_ready_to_read += 1;
            }
        }
        if self.o.storing_columns {
            self.files_ready_to_read = i64::from(
                self.column_vector
                    .first()
                    .is_some_and(|p| p.status != FileStatus::Closed),
            );
        }
    }

    /// `print_files`: print one file, or all of them in parallel.
    fn print_files(&mut self, av: &[OsString]) -> Result<(), Fatal> {
        self.init_parameters(av.len())?;
        if !self.init_fps(av) {
            return Ok(());
        }
        if self.o.storing_columns {
            self.init_store_cols()?;
        }
        if self.o.first_page_number > 1 {
            if !self.skip_to_page(self.o.first_page_number)? {
                return Ok(());
            }
            self.page_number = self.o.first_page_number;
        } else {
            self.page_number = 1;
        }
        self.init_funcs();
        self.line_number = self.line_count;
        while self.print_page()? {}
        Ok(())
    }

    /// `init_header`: the date and the name for the page headers. `filename`
    /// is `None` for standard input and for files in parallel, which have no
    /// one file's time to use.
    fn init_header(&mut self, filename: Option<&[u8]>, mtime: Option<(i64, u32)>) {
        let (sec, ns) = match (filename, mtime) {
            (Some(_), Some(t)) => t,
            _ => *self.now.get_or_insert_with(|| timespec(SystemTime::now())),
        };
        let tm = self.zone.local(sec, ns);
        // glibc's `localtime` fails when the year overflows `tm_year`, an
        // `int` counting from 1900, and upstream then prints the seconds.
        let fits = (INT_MIN + 1900..=INT_MAX + 1900).contains(&tm.year);
        self.date_text = if fits {
            localtime::strftime(&self.date_format, &tm)
        } else {
            format!("{sec}.{ns:09}").into_bytes()
        };
        self.file_text = match (&self.o.custom_header, filename) {
            (Some(header), _) => header.clone(),
            (None, Some(name)) => name.to_vec(),
            (None, None) => Vec::new(),
        };
        let width = |s: &[u8]| i64::try_from(mbswidth(s)).unwrap_or(INT_MAX);
        self.header_width_available =
            self.o.chars_per_line - width(&self.date_text) - width(&self.file_text);
    }

    /// `init_page`: how many lines each column may print on this page.
    fn init_page(&mut self) -> Result<(), Fatal> {
        if self.o.storing_columns {
            self.store_columns()?;
            let last = self.columns() - 1;
            for p in self.column_vector.iter_mut().take(last) {
                p.lines_to_print = p.lines_stored;
            }
            let lines_per_body = self.lines_per_body;
            let p = &mut self.column_vector[last];
            p.lines_to_print = if self.o.balance_columns {
                p.lines_stored
            } else if p.status == FileStatus::Open {
                // Not balancing, so the rightmost column is read straight from
                // the file.
                lines_per_body
            } else {
                0
            };
        } else {
            let lines_per_body = self.lines_per_body;
            for p in &mut self.column_vector {
                p.lines_to_print = if p.status == FileStatus::Open {
                    lines_per_body
                } else {
                    0
                };
            }
        }
        Ok(())
    }

    /// `align_column`: pad to and separate an empty column of parallel files.
    fn align_column(&mut self, pi: usize) {
        self.padding_not_printed = self.column_vector[pi].start_position;
        let sep = self.col_sep_length();
        if sep < self.padding_not_printed {
            self.pad_across_to(self.padding_not_printed - sep);
            self.padding_not_printed = ANYWHERE;
        }
        if self.o.use_col_separator {
            self.print_sep_string();
        }
        if self.column_vector[pi].numbered {
            self.add_line_number(pi);
        }
    }

    /// `print_page`: one page. `false` when there is nothing more to print.
    fn print_page(&mut self) -> Result<bool, Fatal> {
        self.init_page()?;
        if self.cols_ready_to_print() == 0 {
            return Ok(false);
        }
        if self.o.extremities {
            self.print_a_header = true;
        }

        // Don't pad unless we know a page was printed. `pv` accumulates
        // `pad_vertically` across the lines, which each start it at false.
        self.pad_vertically = false;
        let mut pv = false;

        let mut lines_left_on_page = self.lines_per_body;
        if self.o.double_space {
            lines_left_on_page *= 2;
        }

        while lines_left_on_page > 0 && self.cols_ready_to_print() > 0 {
            self.output_position = 0;
            self.spaces_not_printed = 0;
            self.separators_not_printed = 0;
            self.pad_vertically = false;
            self.align_empty_cols = false;
            self.empty_line = true;

            for pi in 0..self.columns() {
                self.input_position = 0;
                let p = &self.column_vector[pi];
                if p.lines_to_print > 0 || p.status == FileStatus::FfFound {
                    self.ff_only = false;
                    self.padding_not_printed = p.start_position;
                    if !self.print_func(pi)? {
                        self.read_rest_of_line(pi)?;
                    }
                    pv |= self.pad_vertically;

                    self.column_vector[pi].lines_to_print -= 1;
                    if self.column_vector[pi].lines_to_print <= 0 && self.cols_ready_to_print() == 0
                    {
                        break;
                    }

                    // The file changed its status to on hold or closed.
                    let status = self.column_vector[pi].status;
                    if self.o.parallel_files && status != FileStatus::Open {
                        if self.empty_line {
                            self.align_empty_cols = true;
                        } else if status == FileStatus::Closed
                            || (status == FileStatus::OnHold && self.ff_only)
                        {
                            self.align_column(pi);
                        }
                    }
                } else if self.o.parallel_files {
                    // A file on hold or closed.
                    if self.empty_line {
                        self.align_empty_cols = true;
                    } else {
                        self.align_column(pi);
                    }
                }

                // Needed with an empty column too.
                if self.o.use_col_separator {
                    self.separators_not_printed += 1;
                }
            }

            if self.pad_vertically {
                self.put(b'\n');
                lines_left_on_page -= 1;
            }
            if self.cols_ready_to_print() == 0 && !self.o.extremities {
                break;
            }
            if self.o.double_space && pv {
                self.put(b'\n');
                lines_left_on_page -= 1;
            }
        }

        if lines_left_on_page == 0 {
            for p in &mut self.column_vector {
                if p.status == FileStatus::Open {
                    p.full_page_printed = true;
                }
            }
        }

        self.pad_vertically = pv;
        if self.pad_vertically && self.o.extremities {
            self.pad_down(lines_left_on_page + LINES_PER_FOOTER);
        } else if self.o.keep_ff && self.print_a_ff {
            self.put(FF);
            self.print_a_ff = false;
        }

        // `uintmax_t`, which wraps; `print_header` refuses page 0.
        self.page_number = self.page_number.wrapping_add(1);
        if self.o.last_page_number < self.page_number {
            // Stop printing with LAST_PAGE.
            return Ok(false);
        }
        self.reset_status();
        Ok(true)
    }

    /// `init_store_cols`: room to store the columns of one page.
    fn init_store_cols(&mut self) -> Result<(), Fatal> {
        // `ckd_mul`/`ckd_add` in `int`.
        let int = |v: i64| (INT_MIN..=INT_MAX).contains(&v).then_some(v);
        let overflow = || Fatal("integer overflow".to_string());
        let total_lines = int(self.lines_per_body * self.o.columns).ok_or_else(overflow)?;
        let total_lines_1 = int(total_lines + 1).ok_or_else(overflow)?;
        let chars_per_column_1 = int(self.chars_per_column + 1).ok_or_else(overflow)?;
        int(total_lines * chars_per_column_1).ok_or_else(overflow)?;

        let exhausted = || Fatal("memory exhausted".to_string());
        let total_lines = usize::try_from(total_lines).map_err(|_| exhausted())?;
        let total_lines_1 = usize::try_from(total_lines_1).map_err(|_| exhausted())?;
        self.line_vector = Vec::new();
        self.line_vector
            .try_reserve_exact(total_lines_1)
            .map_err(|_| exhausted())?;
        self.line_vector.resize(total_lines_1, 0);
        self.end_vector = Vec::new();
        self.end_vector
            .try_reserve_exact(total_lines)
            .map_err(|_| exhausted())?;
        self.end_vector.resize(total_lines, 0);
        self.buff = Vec::new();
        Ok(())
    }

    /// `store_columns`: read the page's columns (all but the rightmost unless
    /// balancing) into `buff`, then balance them.
    fn store_columns(&mut self) -> Result<(), Fatal> {
        let mut line = 0usize;
        let mut buff_start = 0usize;
        self.buff.clear();

        let columns = self.columns();
        let last_col = if self.o.balance_columns {
            columns
        } else {
            columns - 1
        };
        for p in self.column_vector.iter_mut().take(last_col) {
            p.lines_stored = 0;
        }

        let mut i = 1;
        while i <= last_col && self.files_ready_to_read != 0 {
            let pi = i - 1;
            self.column_vector[pi].current_line = line;
            let mut j = self.lines_per_body;
            while j != 0 && self.files_ready_to_read != 0 {
                if self.column_vector[pi].status == FileStatus::Open {
                    self.input_position = 0;
                    if !self.read_line(pi)? {
                        self.read_rest_of_line(pi)?;
                    }
                    if self.column_vector[pi].status == FileStatus::Open
                        || buff_start != self.buff.len()
                    {
                        self.column_vector[pi].lines_stored += 1;
                        self.line_vector[line] = buff_start;
                        self.end_vector[line] = self.input_position;
                        line += 1;
                        buff_start = self.buff.len();
                    }
                }
                j -= 1;
            }
            i += 1;
        }

        // Where the last stored character ends.
        self.line_vector[line] = buff_start;

        if self.o.balance_columns {
            self.balance(line);
        }
        Ok(())
    }

    /// `balance`: spread the stored lines as evenly as the columns allow,
    /// the leftmost taking any extra.
    fn balance(&mut self, total_stored: usize) {
        let columns = self.columns().max(1);
        let mut first_line = 0usize;
        for (i, p) in self.column_vector.iter_mut().enumerate() {
            let mut lines = total_stored / columns;
            if i < total_stored % columns {
                lines += 1;
            }
            p.lines_stored = i64::try_from(lines).unwrap_or(INT_MAX);
            p.current_line = first_line;
            first_line += lines;
        }
    }

    /// `char_func`: print or store one character, as the column does.
    fn char_func(&mut self, pi: usize, c: u8) {
        match self.column_vector[pi].char_func {
            CharFunc::StoreChar => self.buff.push(c),
            CharFunc::PrintChar => self.print_char(c),
        }
    }

    /// `print_func`: one line of the column, read or stored.
    fn print_func(&mut self, pi: usize) -> Result<bool, Fatal> {
        match self.column_vector[pi].print_func {
            PrintFunc::ReadLine => self.read_line(pi),
            PrintFunc::PrintStored => self.print_stored(pi),
        }
    }

    /// `add_line_number`: the number, then its separator.
    fn add_line_number(&mut self, pi: usize) {
        let width = usize::try_from(self.o.chars_per_number).unwrap_or(0);
        // `%*d`, of which only the last `chars_per_number` characters are
        // kept: cutting the high-order digits off tells more than cutting the
        // low ones.
        let number = format!("{:>width$}", self.line_number);
        self.line_number += 1;
        let digits = number.as_bytes();
        let kept = digits
            .get(digits.len().saturating_sub(width)..)
            .unwrap_or(digits);
        for &c in kept {
            self.char_func(pi, c);
        }

        if self.o.columns > 1 {
            // Tabification is assumed for several columns, separators
            // included; POSIX's "the default separator is a TAB" loses to its
            // "columns are of equal width".
            if self.o.number_separator == b'\t' {
                let mut i = self.number_width - self.o.chars_per_number;
                while i > 0 {
                    i -= 1;
                    self.char_func(pi, b' ');
                }
            } else {
                self.char_func(pi, self.o.number_separator);
            }
        } else {
            // A single column has no width to keep, so POSIX's TAB is printed
            // as a TAB.
            self.char_func(pi, self.o.number_separator);
            if self.o.number_separator == b'\t' {
                self.output_position =
                    pos_after_tab(self.o.chars_per_output_tab, self.output_position);
            }
        }

        if self.o.truncate_lines && !self.o.parallel_files {
            self.input_position += self.number_width;
        }
    }

    /// `pad_across_to`: padding to horizontal `position`, deferred when
    /// tabifying.
    fn pad_across_to(&mut self, position: i64) {
        if self.o.tabify_output {
            self.spaces_not_printed = position - self.output_position;
        } else {
            let mut h = self.output_position;
            loop {
                h += 1;
                if h > position {
                    break;
                }
                self.put(b' ');
            }
            self.output_position = position;
        }
    }

    /// `pad_down`: to the bottom of the page, by a form feed if asked for.
    fn pad_down(&mut self, lines: i64) {
        if self.o.use_form_feed {
            self.put(FF);
        } else {
            for _ in 0..lines.max(0) {
                self.put(b'\n');
            }
        }
    }

    /// `read_rest_of_line`: discard the rest of a line that was truncated.
    fn read_rest_of_line(&mut self, pi: usize) -> Result<(), Fatal> {
        loop {
            match self.getc(pi) {
                Some(b'\n') => return Ok(()),
                Some(FF) => {
                    let c = self.getc(pi);
                    if c != Some(b'\n') {
                        self.ungetc(pi, c);
                    }
                    if self.o.keep_ff {
                        self.print_a_ff = true;
                    }
                    self.hold_file(pi);
                    return Ok(());
                }
                None => return self.close_file(pi),
                Some(_) => {}
            }
        }
    }

    /// `skip_read`: read a whole line of a page being skipped, counting it
    /// for `-n` and watching for the form feed that follows a full page.
    fn skip_read(&mut self, pi: usize, column_number: usize) -> Result<(), Fatal> {
        let mut single_ff = false;

        // The first character of a line, or the one after a form feed.
        let mut c = self.getc(pi);
        if c == Some(FF) && self.column_vector[pi].full_page_printed {
            // A form feed straight after a full page: dropped, or it would
            // make an extra empty page.
            c = self.getc(pi);
            if c == Some(b'\n') {
                c = self.getc(pi);
            }
        }
        self.column_vector[pi].full_page_printed = false;

        // A form feed first is a page with nothing on it: not a line for -n.
        if c == Some(FF) {
            single_ff = true;
        }

        // Maybe this page ends without a form feed.
        if self.last_line {
            self.column_vector[pi].full_page_printed = true;
        }

        while c != Some(b'\n') {
            if c == Some(FF) {
                // No coincidence possible, now or on the next page.
                if self.last_line {
                    if self.o.parallel_files {
                        self.column_vector[pi].full_page_printed = false;
                    } else {
                        for q in &mut self.column_vector {
                            q.full_page_printed = false;
                        }
                    }
                }
                let next = self.getc(pi);
                if next != Some(b'\n') {
                    self.ungetc(pi, next);
                }
                self.hold_file(pi);
                break;
            } else if c.is_none() {
                self.close_file(pi)?;
                break;
            }
            c = self.getc(pi);
        }

        if self.o.skip_count && (!self.o.parallel_files || column_number == 1) && !single_ff {
            self.line_count += 1;
        }
        Ok(())
    }

    /// `print_white_space`: the pending spaces, as tabs where they fit.
    fn print_white_space(&mut self) {
        let mut h_old = self.output_position;
        let goal = h_old + self.spaces_not_printed;
        while goal - h_old > 1 {
            let h_new = pos_after_tab(self.o.chars_per_output_tab, h_old);
            if h_new > goal {
                break;
            }
            self.put(self.o.output_tab_char);
            h_old = h_new;
        }
        loop {
            h_old += 1;
            if h_old > goal {
                break;
            }
            self.put(b' ');
        }
        self.output_position = goal;
        self.spaces_not_printed = 0;
    }

    /// `print_sep_string`: the separators counted so far.
    ///
    /// Upstream sets the separator's remaining length once, before its loop
    /// over the pending separators, and counts it down inside -- so of
    /// several pending separators only the first is written out, and the rest
    /// contribute only the white space they flush. Reproduced as it is.
    fn print_sep_string(&mut self) {
        if self.separators_not_printed <= 0 {
            // A line is starting with the margin: anything else pending?
            if self.spaces_not_printed > 0 {
                self.print_white_space();
            }
            return;
        }
        let sep = std::mem::take(&mut self.o.col_sep_string);
        let mut remaining = sep.iter();
        while self.separators_not_printed > 0 {
            for &c in remaining.by_ref() {
                // Spaces only, spaces and characters, or characters only:
                // consecutive spaces may become tabs.
                if c == b' ' {
                    self.spaces_not_printed += 1;
                } else {
                    if self.spaces_not_printed > 0 {
                        self.print_white_space();
                    }
                    self.put(c);
                    self.output_position += 1;
                }
            }
            // The separator ends with some spaces.
            if self.spaces_not_printed > 0 {
                self.print_white_space();
            }
            self.separators_not_printed -= 1;
        }
        self.o.col_sep_string = sep;
    }

    /// `print_clump`: the first `n` characters of the clump, printed or
    /// stored.
    fn print_clump(&mut self, pi: usize, n: usize) {
        let clump = std::mem::take(&mut self.clump_buff);
        for &c in clump.iter().take(n) {
            self.char_func(pi, c);
        }
        self.clump_buff = clump;
    }

    /// `print_char`: one character, keeping count of the horizontal position
    /// and holding back spaces while tabifying.
    fn print_char(&mut self, c: u8) {
        if self.o.tabify_output {
            if c == b' ' {
                self.spaces_not_printed += 1;
                return;
            } else if self.spaces_not_printed > 0 {
                self.print_white_space();
            }
            // Non-printables are taken to have no width, except backspace.
            if !isprint(c) {
                if c == 0x08 {
                    self.output_position -= 1;
                }
            } else {
                self.output_position += 1;
            }
        }
        self.put(c);
    }

    /// `skip_to_page`: read through the pages before `page`.
    fn skip_to_page(&mut self, page: u64) -> Result<bool, Fatal> {
        let mut n: u64 = 1;
        while n < page {
            let mut i = 1;
            while i < self.lines_per_body {
                for j in 0..self.columns() {
                    if self.column_vector[j].status == FileStatus::Open {
                        self.skip_read(j, j + 1)?;
                    }
                }
                i += 1;
            }
            self.last_line = true;
            for j in 0..self.columns() {
                if self.column_vector[j].status == FileStatus::Open {
                    self.skip_read(j, j + 1)?;
                }
            }

            // Form feeds found while storing become holds.
            if self.o.storing_columns {
                for p in &mut self.column_vector {
                    if p.status != FileStatus::Closed {
                        p.status = FileStatus::OnHold;
                    }
                }
            }

            self.reset_status();
            self.last_line = false;

            if self.files_ready_to_read < 1 {
                // Useful, since the number of pages is not known in advance.
                diag!("pr: starting page number {page} exceeds page count {n}");
                break;
            }
            n += 1;
        }
        Ok(self.files_ready_to_read > 0)
    }

    /// `print_header`: two blank lines, the header line, three blank lines.
    fn print_header(&mut self) -> Result<(), Fatal> {
        self.output_position = 0;
        self.pad_across_to(self.o.chars_per_margin);
        self.print_white_space();

        if self.page_number == 0 {
            return Err(Fatal("page number overflow".to_string()));
        }

        let page_text = format!("Page {}", self.page_number);
        let page_width = i64::try_from(mbswidth(page_text.as_bytes())).unwrap_or(INT_MAX);
        let available_width = (self.header_width_available - page_width).max(0);
        let lhs_spaces = available_width >> 1;
        let rhs_spaces = available_width - lhs_spaces;

        // `"\n\n%*s%s%*s%s%*s%s\n\n\n"`: a `%*s` of `" "` is never narrower
        // than its one space.
        let spaces = |n: i64| vec![b' '; usize::try_from(n).unwrap_or(0)];
        let mut header = b"\n\n".to_vec();
        header.extend(spaces(self.o.chars_per_margin));
        header.extend_from_slice(&self.date_text);
        header.extend(spaces(lhs_spaces.max(1)));
        header.extend_from_slice(&self.file_text);
        header.extend(spaces(rhs_spaces.max(1)));
        header.extend_from_slice(page_text.as_bytes());
        header.extend_from_slice(b"\n\n\n");
        self.put_bytes(&header);

        self.print_a_header = false;
        self.output_position = 0;
        Ok(())
    }

    /// `read_line`: print or store one line of the column. `false` when it
    /// had to be cut at the column's width, leaving the rest to
    /// [`Pr::read_rest_of_line`].
    fn read_line(&mut self, pi: usize) -> Result<bool, Fatal> {
        // The first character of a line, or the one after a form feed.
        let mut c = self.getc(pi);
        let mut last_input_position = self.input_position;

        if c == Some(FF) && self.column_vector[pi].full_page_printed {
            c = self.getc(pi);
            if c == Some(b'\n') {
                c = self.getc(pi);
            }
        }
        self.column_vector[pi].full_page_printed = false;

        let mut chars = 0;
        match c {
            Some(FF) => {
                let next = self.getc(pi);
                if next != Some(b'\n') {
                    self.ungetc(pi, next);
                }
                self.ff_only = true;
                if self.print_a_header && !self.o.storing_columns {
                    self.pad_vertically = true;
                    self.print_header()?;
                } else if self.o.keep_ff {
                    self.print_a_ff = true;
                }
                self.hold_file(pi);
                return Ok(true);
            }
            None => {
                self.close_file(pi)?;
                return Ok(true);
            }
            Some(b'\n') => {}
            Some(other) => chars = self.char_to_clump(other),
        }

        if self.o.truncate_lines && self.input_position > self.chars_per_column {
            self.input_position = last_input_position;
            return Ok(false);
        }

        if self.column_vector[pi].char_func != CharFunc::StoreChar {
            self.pad_vertically = true;

            if self.print_a_header && !self.o.storing_columns {
                self.print_header()?;
            }

            if self.o.parallel_files && self.align_empty_cols {
                // Align the empty columns at the start of the line.
                let k = self.separators_not_printed;
                self.separators_not_printed = 0;
                let mut j = 0;
                while j < k {
                    self.align_column(usize::try_from(j).unwrap_or(0));
                    self.separators_not_printed += 1;
                    j += 1;
                }
                self.padding_not_printed = self.column_vector[pi].start_position;
                self.spaces_not_printed = if self.o.truncate_lines {
                    self.chars_per_column
                } else {
                    0
                };
                self.align_empty_cols = false;
            }

            let sep = self.col_sep_length();
            if sep < self.padding_not_printed {
                self.pad_across_to(self.padding_not_printed - sep);
                self.padding_not_printed = ANYWHERE;
            }

            if self.o.use_col_separator {
                self.print_sep_string();
            }
        }

        if self.column_vector[pi].numbered {
            self.add_line_number(pi);
        }

        self.empty_line = false;
        if c == Some(b'\n') {
            return Ok(true);
        }

        self.print_clump(pi, chars);

        loop {
            match self.getc(pi) {
                Some(b'\n') => return Ok(true),
                Some(FF) => {
                    let next = self.getc(pi);
                    if next != Some(b'\n') {
                        self.ungetc(pi, next);
                    }
                    if self.o.keep_ff {
                        self.print_a_ff = true;
                    }
                    self.hold_file(pi);
                    return Ok(true);
                }
                None => {
                    self.close_file(pi)?;
                    return Ok(true);
                }
                Some(other) => {
                    last_input_position = self.input_position;
                    let n = self.char_to_clump(other);
                    if self.o.truncate_lines && self.input_position > self.chars_per_column {
                        self.input_position = last_input_position;
                        return Ok(false);
                    }
                    self.print_clump(pi, n);
                }
            }
        }
    }

    /// `print_stored`: print one line from `buff`. Always `Ok(true)`: a
    /// stored line was cut to width when it was stored.
    fn print_stored(&mut self, pi: usize) -> Result<bool, Fatal> {
        let line = self.column_vector[pi].current_line;
        self.column_vector[pi].current_line += 1;

        self.pad_vertically = true;

        if self.print_a_header {
            self.print_header()?;
        }

        if self.column_vector[pi].status == FileStatus::FfFound {
            for q in &mut self.column_vector {
                q.status = FileStatus::OnHold;
            }
            if self
                .column_vector
                .first()
                .is_some_and(|p| p.lines_to_print <= 0)
            {
                if !self.o.extremities {
                    self.pad_vertically = false;
                }
                // A header only.
                return Ok(true);
            }
        }

        let sep = self.col_sep_length();
        if sep < self.padding_not_printed {
            self.pad_across_to(self.padding_not_printed - sep);
            self.padding_not_printed = ANYWHERE;
        }

        if self.o.use_col_separator {
            self.print_sep_string();
        }

        // Upstream's `FIXME` reads past what was stored in one corner; this
        // reads nothing there instead.
        let first = self.line_vector.get(line).copied().unwrap_or(0);
        let last = self.line_vector.get(line + 1).copied().unwrap_or(first);
        let text = self.buff.get(first..last).unwrap_or_default().to_vec();
        for c in text {
            self.print_char(c);
        }

        if self.spaces_not_printed == 0 {
            let start = self.column_vector[pi].start_position;
            let end = self.end_vector.get(line).copied().unwrap_or(0);
            self.output_position = start + end;
            if start - sep == self.o.chars_per_margin {
                self.output_position -= sep;
            }
        }
        Ok(true)
    }

    /// `char_to_clump`: `c` as it prints -- a tab as spaces, a non-printable as
    /// an escape -- in `clump_buff`. Returns how many bytes that is, and moves
    /// `input_position` by how wide it is, which is not always the same.
    fn char_to_clump(&mut self, c: u8) -> usize {
        self.clump_buff.clear();
        let chars_per_c = if c == self.o.input_tab_char {
            self.o.chars_per_input_tab
        } else {
            8
        };

        let width: i64;
        let mut chars: usize;
        if c == self.o.input_tab_char || c == b'\t' {
            width = tab_width(chars_per_c, self.input_position);
            if self.o.untabify_input {
                let n = usize::try_from(width).unwrap_or(0);
                self.clump_buff.resize(n, b' ');
                chars = n;
            } else {
                self.clump_buff.push(c);
                chars = 1;
            }
        } else if !isprint(c) {
            if self.o.use_esc_sequence || (self.o.use_cntrl_prefix && c >= 0o200) {
                width = 4;
                chars = 4;
                self.clump_buff
                    .extend_from_slice(format!("\\{c:03o}").as_bytes());
            } else if self.o.use_cntrl_prefix {
                width = 2;
                chars = 2;
                self.clump_buff.push(b'^');
                self.clump_buff.push(c ^ 0o100);
            } else if c == 0x08 {
                width = -1;
                chars = 1;
                self.clump_buff.push(c);
            } else {
                width = 0;
                chars = 1;
                self.clump_buff.push(c);
            }
        } else {
            width = 1;
            chars = 1;
            self.clump_buff.push(c);
        }

        // Too many backspaces must leave us at position 0, never below.
        if width < 0 && self.input_position == 0 {
            chars = 0;
            self.input_position = 0;
        } else if width < 0 && self.input_position <= -width {
            self.input_position = 0;
        } else {
            self.input_position += width;
        }
        chars
    }
}

fn main() -> ExitCode {
    stdfd::close_stderr(run(), 1)
}

fn run() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(r) => r,
        Err(e) => {
            PR.report(&e);
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };
    let mut out = Stream::stdout();
    let options = match request {
        Request::Help => {
            let _ = out.write_all(help_text().as_bytes());
            return stdfd::close_stdout("pr", out, ExitCode::SUCCESS);
        }
        Request::Version => {
            let _ = out.write_all(b"pr (SlateOS coreutils) 0.1.0\n");
            return stdfd::close_stdout("pr", out, ExitCode::SUCCESS);
        }
        Request::Run(options) => *options,
    };
    let date_format = options.date_format.clone().unwrap_or_else(|| {
        // POSIX's format, `%b %e %H:%M %Y`, only when asked for and only in
        // the portable locale.
        if std::env::var_os("POSIXLY_CORRECT").is_some() && !hard_locale(Category::Time) {
            b"%b %e %H:%M %Y".to_vec()
        } else {
            b"%Y-%m-%d %H:%M".to_vec()
        }
    });
    let mut pr = Pr::new(options, date_format, localtime::Zone::from_env(), out);
    let status = pr.run();
    stdfd::close_stdout("pr", pr.out, status)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn options(words: &[&str]) -> Options {
        match parse_args(&args(words)).unwrap() {
            Request::Run(o) => *o,
            other => panic!("{other:?}"),
        }
    }

    fn error(words: &[&str]) -> String {
        parse_args(&args(words)).unwrap_err().message()
    }

    /// Run the engine over `input` as standard input, with the header's date
    /// fixed, and return what it printed.
    fn render(words: &[&str], input: &[u8]) -> Vec<u8> {
        let mut o = options(words);
        let files = std::mem::take(&mut o.files);
        assert!(files.is_empty(), "the tests feed standard input only");
        let mut pr = Pr::new(o, b"DATE".to_vec(), localtime::Zone::utc(), Vec::new());
        pr.stdin = Input::new(Reader::Bytes(io::Cursor::new(input.to_vec())), None);
        assert_eq!(pr.run(), ExitCode::SUCCESS);
        pr.out
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[test]
    fn digits_accumulate_until_another_option_comes_between() {
        assert_eq!(options(&["-12"]).columns, 12);
        assert_eq!(options(&["-1", "-2"]).columns, 12);
        assert_eq!(options(&["-1", "-a", "-2"]).columns, 2);
        assert_eq!(options(&["-3", "--columns=5"]).columns, 5);
        assert_eq!(options(&["--columns=5", "-3"]).columns, 3);
        assert!(error(&["-0"]).contains("invalid number of columns"));
    }

    #[test]
    fn a_plus_operand_is_a_page_range_only_when_it_is_one() {
        let o = options(&["+3:5", "f"]);
        assert_eq!((o.first_page_number, o.last_page_number), (3, 5));
        assert_eq!(o.files, args(&["f"]));
        // Not a range: file names.
        assert_eq!(options(&["+0"]).files, args(&["+0"]));
        assert_eq!(options(&["+5x"]).files, args(&["+5x"]));
        assert_eq!(options(&["+3:2"]).files, args(&["+3:2"]));
        // A second range once one is set, and anything after `--`.
        assert_eq!(options(&["+2", "+3"]).files, args(&["+3"]));
        assert_eq!(options(&["--", "+3"]).files, args(&["+3"]));
        // Not a number at all: fatal, and worded after the `+`.
        assert_eq!(error(&["+x"]), "invalid + argument 'x'");
        assert_eq!(error(&["+3:"]), "invalid + argument '3:'");
        assert_eq!(error(&["--pages=x"]), "invalid --pages argument 'x'");
        assert!(error(&["--pages=3:2"]).starts_with("invalid page range"));
    }

    #[test]
    fn the_old_options_are_translated() {
        // -w with columns truncates; without, it joins lines.
        let o = options(&["-2", "-w", "40"]);
        assert!(o.truncate_lines && !o.join_lines);
        let o = options(&["-w", "40"]);
        assert!(o.join_lines);
        // -s with columns joins lines, and sets -S when it names a character.
        let o = options(&["-2", "-s:"]);
        assert!(o.join_lines && o.use_col_separator);
        assert_eq!(o.col_sep_string, b":");
        let o = options(&["-2", "-s"]);
        assert!(o.join_lines && !o.use_col_separator);
        // -S dominates -s.
        let o = options(&["-2", "-S-", "-s:"]);
        assert_eq!(o.col_sep_string, b"-");
    }

    #[test]
    fn character_and_number_arguments() {
        let o = options(&["-n:3", "-e.4", "-ix2"]);
        assert_eq!((o.number_separator, o.chars_per_number), (b':', 3));
        assert_eq!((o.input_tab_char, o.chars_per_input_tab), (b'.', 4));
        assert_eq!((o.output_tab_char, o.chars_per_output_tab), (b'x', 2));
        let o = options(&["-n7"]);
        assert_eq!((o.number_separator, o.chars_per_number), (b'\t', 7));
        assert!(
            error(&["-n:0"]).contains("'-n' extra characters or invalid number in the argument")
        );
        assert!(error(&["--number-lines="]).starts_with("'-n': Invalid argument"));
        assert!(error(&["-n:99999999999"]).ends_with(
            "Value too large for defined data type\nTry 'pr --help' for more information."
        ));
    }

    #[test]
    fn conflicting_layouts_are_refused() {
        assert_eq!(
            error(&["-m", "-2"]),
            "cannot specify number of columns when printing in parallel"
        );
        assert_eq!(
            error(&["-m", "-a"]),
            "cannot specify both printing across and printing in parallel"
        );
    }

    #[test]
    fn clumps() {
        let o = Options {
            use_cntrl_prefix: true,
            ..Options::default()
        };
        let mut pr = Pr::new(o, Vec::new(), localtime::Zone::utc(), Vec::new());
        assert_eq!(pr.char_to_clump(0x07), 2);
        assert_eq!(pr.clump_buff, b"^G");
        assert_eq!(pr.char_to_clump(0x7f), 2);
        assert_eq!(pr.clump_buff, b"^?");
        assert_eq!(pr.char_to_clump(0xe9), 4);
        assert_eq!(pr.clump_buff, b"\\351");
        assert_eq!(pr.input_position, 8);
        // A tab from position 8 is eight wide, kept as one byte.
        assert_eq!(pr.char_to_clump(b'\t'), 1);
        assert_eq!(pr.input_position, 16);
        // Backspaces stop at the margin.
        pr.input_position = 1;
        pr.o.use_cntrl_prefix = false;
        assert_eq!(pr.char_to_clump(0x08), 1);
        assert_eq!(pr.input_position, 0);
        assert_eq!(pr.char_to_clump(0x08), 0);
    }

    #[test]
    fn a_short_input_is_one_padded_page() {
        let out = text(&render(&[], b"one\ntwo\n"));
        let lines: Vec<&str> = out.split('\n').collect();
        // Two blank lines, the header, two more, the text, and the padding to
        // 66 lines.
        assert_eq!(lines.len(), 67);
        assert_eq!(lines[0], "");
        assert_eq!(lines[1], "");
        assert!(lines[2].starts_with("DATE"));
        assert!(lines[2].ends_with("Page 1"));
        assert_eq!(lines[2].len(), 72);
        assert_eq!(&lines[3..5], ["", ""]);
        assert_eq!(&lines[5..7], ["one", "two"]);
        assert!(lines[7..66].iter().all(|l| l.is_empty()));
    }

    #[test]
    fn omitting_the_header_leaves_the_text_alone() {
        assert_eq!(render(&["-t"], b"a\nb\n"), b"a\nb\n");
        assert_eq!(render(&["-T"], b"a\x0cb\n"), b"a\nb\n");
        assert_eq!(render(&["-t"], b"a\x0cb\n"), b"a\n\x0cb\n");
    }

    #[test]
    fn columns_down_are_balanced() {
        let out = render(&["-t", "-2", "-w", "20"], b"1\n2\n3\n4\n5\n");
        // Several columns turn on output tabification: the nine columns to the
        // second one's start are a tab to 8 and two spaces.
        assert_eq!(text(&out), "1\t  4\n2\t  5\n3\n");
    }

    #[test]
    fn columns_across() {
        let out = render(&["-t", "-a", "-3", "-w", "15"], b"a\nb\nc\nd\n");
        assert_eq!(text(&out), "a    b\t  c\nd\n");
    }

    #[test]
    fn numbering_in_one_column_keeps_its_tab() {
        let out = render(&["-t", "-n"], b"x\ny\n");
        assert_eq!(text(&out), "    1\tx\n    2\ty\n");
        let out = render(&["-t", "-n:2"], b"x\n");
        assert_eq!(text(&out), " 1:x\n");
    }

    #[test]
    fn double_spacing_and_the_margin() {
        let out = render(&["-t", "-d", "-o", "3"], b"a\nb\n");
        assert_eq!(text(&out), "   a\n\n   b\n\n");
    }
}
