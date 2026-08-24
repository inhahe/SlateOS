//! wc — line, word, character, byte and display-width counts.
//!
//! The five counts are not five ways of saying the same thing, and the
//! difference between them is the whole of this program:
//!
//! | option | counts |
//! |---|---|
//! | `-l` | newline **characters** — an unterminated last line is not one |
//! | `-w` | runs of non-blank, where "blank" is a Unicode question |
//! | `-m` | characters that decode; a byte that decodes to none is not one |
//! | `-c` | bytes, whatever they are |
//! | `-L` | the widest line in **terminal columns**, so U+4E00 is two and a tab is however many reach the next multiple of eight |
//!
//! They are always printed in that order regardless of the order the options
//! were given, which is why the flags are booleans and not a list.
//!
//! Everything here that could have been guessed was instead measured against
//! GNU coreutils 9.4 under `LC_ALL=C.UTF-8` — see `scripts/wc-diff.sh`, which
//! is the executable form of this file's claims. UTF-8 rather than `C` because
//! that is settled policy for this OS: there is no non-UTF-8 locale on the
//! SlateOS target, and osh made the same choice for the same reason
//! (design-decisions.md, "osh's string layer is UTF-8, full stop").

use charwidth::char_width;
use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::filekind;
use coreutils::getopt::{self, Program, Takes};
// No `quote` here: every diagnostic `wc` prints names its file with one of the
// shell-escape styles, so none of them carry §351's curly marks.
use coreutils::quote::{quoteaf, quoteaf_os, quotef_os};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process::ExitCode;

/// Measured: `wc --zzz-bogus` exits **1**, not 2. `sort` and `ls` exit 2
/// because they have already given 1 a meaning; `wc` has not.
const WC: Program = Program::new("wc", 1);

/// The declaration order of upstream's `struct option[]`, which is observable:
/// glibc lists an ambiguous prefix's possibilities in this order, so `wc --=x`
/// prints exactly this table. Do not alphabetise.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("bytes", Takes::Nothing),
    ("chars", Takes::Nothing),
    ("lines", Takes::Nothing),
    ("words", Takes::Nothing),
    ("debug", Takes::Nothing),
    ("files0-from", Takes::Required),
    ("max-line-length", Takes::Nothing),
    ("total", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `--total=WHEN`. Resolved by `argmatch`, so `--total=al` is ambiguous
/// between `always` and `auto` while `--total=n` is not ambiguous at all.
const TOTAL_WORDS: &[(&str, Total)] = &[
    ("auto", Total::Auto),
    ("always", Total::Always),
    ("only", Total::Only),
    ("never", Total::Never),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Total {
    /// A total line only when there is more than one input.
    Auto,
    Always,
    /// The total line and nothing else — which also drops the column padding.
    Only,
    Never,
}

/// Which counts to print. Not a list, because the print order is fixed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Options {
    lines: bool,
    words: bool,
    chars: bool,
    bytes: bool,
    max_line: bool,
    total: Total,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            lines: false,
            words: false,
            chars: false,
            bytes: false,
            max_line: false,
            total: Total::Auto,
        }
    }
}

impl Options {
    /// With no selecting option at all, `wc` prints lines, words and bytes —
    /// but not characters and not the maximum line length.
    fn defaulted(mut self) -> Self {
        if !(self.lines || self.words || self.chars || self.bytes || self.max_line) {
            self.lines = true;
            self.words = true;
            self.bytes = true;
        }
        self
    }

    /// How many of the five counts are printed. Upstream adds the five flags
    /// together in exactly this way, because one count of one input is printed
    /// with no padding at all.
    fn selected(self) -> usize {
        usize::from(self.lines)
            + usize::from(self.words)
            + usize::from(self.chars)
            + usize::from(self.bytes)
            + usize::from(self.max_line)
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Counts {
    lines: u64,
    words: u64,
    chars: u64,
    bytes: u64,
    max_line: u64,
}

impl Counts {
    fn add(&mut self, other: &Counts) {
        self.lines = self.lines.saturating_add(other.lines);
        self.words = self.words.saturating_add(other.words);
        self.chars = self.chars.saturating_add(other.chars);
        self.bytes = self.bytes.saturating_add(other.bytes);
        // The total's "longest line" is the longest line anywhere, not a sum.
        self.max_line = self.max_line.max(other.max_line);
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Run(Options, Source),
}

/// Where the list of inputs comes from. `--files0-from` is not just another
/// way of spelling the operands: it changes the column width, and it makes an
/// operand an error rather than an addition.
#[derive(Debug, PartialEq, Eq)]
enum Source {
    Operands(Vec<OsString>),
    Files0From(OsString),
}

/// The inputs, resolved — upstream's `files[]` and `nfiles`.
#[derive(Debug, PartialEq, Eq)]
struct Inputs {
    /// One entry per input. `None` is standard input *with no name*, which is
    /// upstream's null pointer and prints a row with no file name at all —
    /// distinct from a `-` operand, which reads the same stream but is
    /// labelled. An empty name is a zero-length entry in a `--files0-from`
    /// list, which is a diagnostic rather than an input.
    names: Vec<Option<Vec<u8>>>,
    /// Upstream's `nfiles`, which is what the column width is computed from —
    /// and which is **0**, not `names.len()`, when a `--files0-from` list was
    /// streamed rather than read into memory. Upstream reads the list up front
    /// only when it is a regular file of reasonable size; otherwise it takes a
    /// name at a time and so cannot know the sizes before it starts printing.
    /// That is why `wc --files0-from=list` pads and `cat list | wc
    /// --files0-from=-` does not.
    nfiles: usize,
    /// The `--files0-from` source as it was written, which names the list in
    /// the zero-length-name diagnostic.
    label: Option<Vec<u8>>,
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("wc (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(options, source)) => run(&options, &source),
        Err(e) => {
            // The referral, when there is one, is part of the message, and only
            // the first line carries the `wc: ` prefix — which is what GNU
            // prints.
            diag!("wc: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: wc [OPTION]... [FILE]...
  or:  wc [OPTION]... --files0-from=F
Print newline, word, and byte counts for each FILE, and a total line if
more than one FILE is specified.  A word is a non-zero-length sequence of
printable characters delimited by white space.

With no FILE, or when FILE is -, read standard input.

The options below may be used to select which counts are printed, always in
the following order: newline, word, character, byte, maximum line length.
  -c, --bytes            print the byte counts
  -m, --chars            print the character counts
  -l, --lines            print the newline counts
      --files0-from=F    read input from the files specified by
                           NUL-terminated names in file F;
                           If F is - then read names from standard input
  -L, --max-line-length  print the maximum display width
  -w, --words            print the word counts
      --total=WHEN       when to print a line with total counts;
                           WHEN can be: auto, always, only, never
      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse argv the way `getopt_long` does, so long options abbreviate to any
/// unambiguous prefix: `wc --lin` counts lines and `wc --max` is the maximum
/// line length, exactly as on any GNU system.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut options = Options::default();
    let mut files: Vec<OsString> = Vec::new();
    let mut files0_from: Option<OsString> = None;
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
            if let Some(request) =
                long_option(&bytes, args, &mut i, &mut options, &mut files0_from)?
            {
                return Ok(request);
            }
        } else {
            // Bytes, not `char`s: `-é` is two bytes, and iterating `char`s
            // would report `invalid option -- 'é'`, an option nobody typed.
            for &b in bytes.get(1..).unwrap_or_default() {
                apply_short(b, &mut options)?;
            }
        }
    }

    let source = match files0_from {
        Some(from) => {
            if let Some(extra) = files.first() {
                // Measured: this one *does* carry the `Try 'wc --help'`
                // referral, because upstream reaches it through `usage()`.
                //
                // `quoteaf`, not `quote`, and also measured (GNU wc 9.4,
                // `LC_ALL=C.UTF-8`): it prints `extra operand 'w1'` with
                // straight marks, where uniq/tr/comm/split's plain
                // `extra operand ‘c’` is curly. Upstream spells the
                // --files0-from clash with the always-quote flavour, which
                // §351 keeps straight in every locale. sort does the same.
                return Err(WC.usage_referring(format!(
                    "extra operand {}\nfile operands cannot be combined with --files0-from",
                    quoteaf_os(extra)
                )));
            }
            Source::Files0From(from)
        }
        None => Source::Operands(files),
    };
    Ok(Request::Run(options.defaulted(), source))
}

/// One `--name` argument. Returns `Some` when the option ends parsing —
/// `--help` and `--version` — and `None` when it only set something.
fn long_option(
    bytes: &[u8],
    args: &[OsString],
    i: &mut usize,
    options: &mut Options,
    files0_from: &mut Option<OsString>,
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
    let typed = std::str::from_utf8(typed).map_err(|_| WC.unrecognized_option(bytes))?;
    let (name, takes) = WC.resolve_long(typed, bytes, LONG_OPTIONS)?;

    if takes == Takes::Nothing && inline.is_some() {
        return Err(WC.long_unwanted_argument(name));
    }
    // A required value may be written `--total=only` or `--total only`.
    let value: Option<Vec<u8>> = match (takes, inline) {
        (_, Some(v)) => Some(v.to_vec()),
        (Takes::Required, None) => {
            let next = args
                .get(*i)
                .ok_or_else(|| WC.long_missing_argument(name))?
                .clone();
            *i = i.saturating_add(1);
            Some(arg_bytes(&next))
        }
        (_, None) => None,
    };

    match name {
        "bytes" => options.bytes = true,
        "chars" => options.chars = true,
        "lines" => options.lines = true,
        "words" => options.words = true,
        "max-line-length" => options.max_line = true,
        // Undocumented upstream and, measured, observable nowhere: `wc --debug`
        // prints nothing extra and exits 0. It is in the table because the
        // ambiguity list is, and accepted here so that `--d` stays ambiguous
        // rather than resolving to `--debug` alone.
        "debug" => {}
        "total" => {
            options.total = WC.argmatch(&value.unwrap_or_default(), "--total", TOTAL_WORDS)?
        }
        "files0-from" => *files0_from = Some(os_from_bytes(&value.unwrap_or_default())),
        "help" => return Ok(Some(Request::Help)),
        "version" => return Ok(Some(Request::Version)),
        // Every name in the table is above, and `resolve_long` returns only
        // names from the table.
        _ => {}
    }
    Ok(None)
}

fn apply_short(c: u8, options: &mut Options) -> Result<(), getopt::Error> {
    match c {
        b'c' => options.bytes = true,
        b'm' => options.chars = true,
        b'l' => options.lines = true,
        b'w' => options.words = true,
        b'L' => options.max_line = true,
        _ => return Err(WC.invalid_option(c)),
    }
    Ok(())
}

// --------------------------------------------------------------- counting ---

/// What a character does to the two running states `wc` keeps: whether it is
/// inside a word, and how far along the line it is.
///
/// The third case is the one that is easy to miss. A character that is not
/// *printable* is neither a separator nor part of a word — it is transparent,
/// so `a<0x01>b` is **one** word, not two, and contributes no column. Measured:
/// `printf 'a\001b\n' | wc -w` is 1 and `wc -L` is 2.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// Ends a word. Non-breaking space is one of these, and U+3000 is one two
    /// columns wide.
    Separator,
    /// Continues or starts a word. A zero-width joiner is one of these.
    WordChar,
    /// Neither, and no columns: the controls, the line and paragraph
    /// separators, and any byte that decodes to no character at all.
    Transparent,
}

/// How a decoded character is classified, and how many columns it occupies.
///
/// Upstream gates on `iswprint` and only then asks `wcwidth`, and the two
/// disagree more often than one would expect — U+2028 has a `wcwidth` of 1 but
/// is not printable. Rather than carry a second Unicode table for a predicate
/// that differs from [`char_width`] on so little, the two cases where they part
/// company are named here:
///
/// - the line and paragraph separators (U+2028, U+2029), handled below;
/// - *unassigned* code points, which glibc calls unprintable and this OS gives
///   the width its category implies. That divergence is already on the record
///   for the same tables under known-issues
///   `TD-OILS-UNASSIGNED-CODE-POINTS-TAKE-THE-UNICODE-WIDTH-NOT-THE-HOSTS`;
///   this is the same decision, not a new one.
fn classify(c: char) -> (Kind, u64) {
    // Zl and Zp. `wcwidth` says one column; `iswprint` says they are not
    // printable, and `wc` believes `iswprint`. Measured: `a\u{2028}b` is
    // 2 columns and 1 word.
    if c == '\u{2028}' || c == '\u{2029}' {
        return (Kind::Transparent, 0);
    }
    match char_width(c) {
        // C0, DEL and C1 — the characters `wcwidth` refuses outright.
        None => (Kind::Transparent, 0),
        Some(w) => {
            let width = u64::try_from(w).unwrap_or(0);
            if c.is_whitespace() {
                (Kind::Separator, width)
            } else {
                (Kind::WordChar, width)
            }
        }
    }
}

/// The next character in `data`, and how many bytes it took. `None` is a byte
/// that decodes to no character: it is counted by `-c` and by nothing else.
fn decode(data: &[u8]) -> (Option<char>, usize) {
    for len in 1..=data.len().min(4) {
        if let Some(head) = data.get(..len)
            && let Ok(s) = std::str::from_utf8(head)
            && let Some(c) = s.chars().next()
        {
            return (Some(c), len);
        }
    }
    (None, 1)
}

/// All five counts over one input's bytes.
///
/// The control characters are not uniform and cannot be folded together:
/// `\r` and `\f` end the current line for `-L` (so `ab\rcdefg` is 5 columns,
/// not 8) while `\v` does not (so `ab\vcd` is 4); `\t` advances to the next
/// multiple of eight; and all four are word separators. Every one of those is
/// measured in `scripts/wc-diff.sh`.
fn count(data: &[u8]) -> Counts {
    let mut counts = Counts {
        bytes: u64::try_from(data.len()).unwrap_or(u64::MAX),
        ..Counts::default()
    };
    let mut in_word = false;
    let mut column = 0u64;

    /// `-L` reports the longest line, so the running column is banked into
    /// `max_line` and reset at every character that ends a line.
    fn end_line(column: &mut u64, counts: &mut Counts) {
        counts.max_line = counts.max_line.max(*column);
        *column = 0;
    }

    let mut i = 0usize;
    while let Some(rest) = data.get(i..) {
        if rest.is_empty() {
            break;
        }
        let (decoded, len) = decode(rest);
        i = i.saturating_add(len);
        let Some(c) = decoded else {
            // A byte that is no character: not counted by `-m`, and transparent
            // to word splitting. Measured: `a\xffb` is one word.
            continue;
        };
        counts.chars = counts.chars.saturating_add(1);

        let separates = match c {
            '\n' => {
                counts.lines = counts.lines.saturating_add(1);
                end_line(&mut column, &mut counts);
                true
            }
            '\r' | '\x0c' => {
                end_line(&mut column, &mut counts);
                true
            }
            '\t' => {
                // To the next tab stop, which is every eighth column.
                column = column.saturating_add(8u64.saturating_sub(column.wrapping_rem(8)));
                true
            }
            // Vertical tab separates words but does not end the line and
            // occupies no column of its own.
            '\x0b' => true,
            _ => {
                let (kind, width) = classify(c);
                column = column.saturating_add(width);
                match kind {
                    Kind::Separator => true,
                    Kind::WordChar => {
                        in_word = true;
                        false
                    }
                    // Changes neither `in_word` nor the column.
                    Kind::Transparent => false,
                }
            }
        };
        if separates {
            if in_word {
                counts.words = counts.words.saturating_add(1);
            }
            in_word = false;
        }
    }
    if in_word {
        counts.words = counts.words.saturating_add(1);
    }
    // An unterminated last line still has a width.
    counts.max_line = counts.max_line.max(column);
    counts
}

// ---------------------------------------------------------------- printing ---

/// The width every count is right-aligned to.
///
/// It looks like a fixed 7 and is not: `wc w1` pads to 1 and `wc bigfile` to 3,
/// because upstream sizes the column from the *bytes it is about to read*. This
/// is upstream's `get_input_fstatus` and `compute_number_width` together, and
/// the three ways out of it are each observable:
///
/// * **1, unconditionally**, when there is nothing to align — `--total=only`
///   prints one row; a streamed `--files0-from` list (see [`Inputs::nfiles`])
///   has no known names; and a lone input with a lone count is a lone number.
///   `wc -l adir` prints `0 adir` even though `wc adir` pads to 7.
/// * **7**, when some input is stattable but has no length to sum — a
///   directory, a pipe, a terminal. `wc` with no operand at all lands here,
///   because standard input is usually a pipe; `wc - < file` does not, because
///   then it is not.
/// * **the digits of the total size** otherwise. An input that cannot be
///   stat'ed contributes nothing and is not a reason to widen: `wc /nope w1`
///   pads to 1, exactly as `wc w1` does.
fn number_width(inputs: &Inputs, options: &Options) -> usize {
    if options.total == Total::Only || inputs.nfiles == 0 {
        return 1;
    }
    if inputs.nfiles == 1 && options.selected() == 1 {
        return 1;
    }
    let mut minimum = 1usize;
    let mut regular_total: u64 = 0;
    for name in &inputs.names {
        match stat_of(name.as_deref()) {
            Stat::Failed => {}
            Stat::Regular(size) => regular_total = regular_total.saturating_add(size),
            Stat::Other => minimum = 7,
        }
    }
    decimal_digits(regular_total).max(minimum)
}

/// What `stat` of one input said, reduced to the three answers the width rule
/// asks of it.
///
/// Upstream keeps the whole `struct stat` and tests `S_ISREG` at the point of
/// use. The distinction between [`Stat::Failed`] and [`Stat::Other`] is the
/// entire difference between a width of 1 and a width of 7, so it is the one
/// thing that must survive the reduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stat {
    /// The call failed. The input contributes nothing at all — not even a
    /// widening, which is why `wc /nope w1.txt` pads exactly like `wc w1.txt`.
    Failed,
    /// A regular file of this many bytes. Its size is a lower bound on the
    /// number about to be printed, so it sets the width.
    Regular(u64),
    /// Stattable, but with no size to add: a pipe, a terminal, a directory.
    /// Upstream widens to 7 and hopes.
    Other,
}

/// The metadata upstream's `fstatus` holds for one input: `fstat` of standard
/// input for `-` and for the nameless input, `stat` otherwise.
fn stat_of(name: Option<&[u8]>) -> Stat {
    match name {
        None | Some(b"-") => stdin_stat(),
        // A zero-length name from `--files0-from`. Upstream still counts it in
        // `nfiles` and still calls `stat` on it, which fails — so it is a
        // failed stat here too, contributing nothing and widening nothing.
        Some(b"") => Stat::Failed,
        Some(name) => Stat::of(std::fs::metadata(os_from_bytes(name))),
    }
}

impl Stat {
    /// `S_ISREG` on the result of a `stat`.
    fn of(metadata: io::Result<std::fs::Metadata>) -> Self {
        match metadata {
            Err(_) => Self::Failed,
            Ok(m) if m.is_file() => Self::Regular(m.len()),
            Ok(_) => Self::Other,
        }
    }
}

/// `fstat` of standard input.
///
/// A `-` operand's size is the size of whatever standard input is attached to,
/// so `wc - < big` pads like `wc big` while `wc - < pipe` falls back to 7. The
/// descriptor is *borrowed*: `File::from_raw_fd` takes ownership, and letting
/// that `File` drop would close standard input in the middle of a program that
/// is about to read it, so the handle is wrapped in `ManuallyDrop`.
fn stdin_stat() -> Stat {
    // `filekind::regular`, not `Metadata::is_file`, and the reason is the host:
    // on Windows `is_file` means "not a directory and not a symlink", so a pipe
    // answers *yes* and reports a length of however many bytes happen to be
    // buffered in it — which would make `printf 'a b\nc\n' | wc` pad to 1 where
    // GNU pads to 7. The three-valued answer is what is needed here rather than
    // the boolean, because `Failed` and `Other` differ by six columns.
    let Some(file) = borrowed_stdin() else {
        return Stat::Failed;
    };
    match filekind::regular(&file) {
        None => Stat::Failed,
        Some(false) => Stat::Other,
        Some(true) => Stat::of(file.metadata()),
    }
}

/// Standard input as a `File` that will not close it.
///
/// `File::from_raw_fd` takes ownership, and letting that `File` drop would close
/// standard input in the middle of a program that is about to read it — hence
/// the `ManuallyDrop`. Nothing is read through the returned handle, so it cannot
/// disturb the stream position either.
fn borrowed_stdin() -> Option<std::mem::ManuallyDrop<File>> {
    #[cfg(unix)]
    {
        use std::os::fd::{AsRawFd, FromRawFd};
        let fd = io::stdin().as_raw_fd();
        // SAFETY: `fd` is standard input, which the runtime keeps open for the
        // whole process, and `ManuallyDrop` prevents the `File` from closing it.
        Some(unsafe { std::mem::ManuallyDrop::new(File::from_raw_fd(fd)) })
    }
    #[cfg(all(not(unix), windows))]
    {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        let handle = io::stdin().as_raw_handle();
        // SAFETY: as above — standard input's handle, which the runtime keeps
        // open for the whole process, kept from being closed by `ManuallyDrop`.
        Some(unsafe { std::mem::ManuallyDrop::new(File::from_raw_handle(handle)) })
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        None
    }
}

fn decimal_digits(mut n: u64) -> usize {
    let mut digits = 1usize;
    while n >= 10 {
        n /= 10;
        digits = digits.saturating_add(1);
    }
    digits
}

/// One output row: the selected counts in their fixed order, then the name if
/// there is one. The row for standard input with no operand has no name, and
/// then there is no trailing space either.
fn write_row(
    out: &mut impl Write,
    c: &Counts,
    options: &Options,
    width: usize,
    name: Option<&[u8]>,
) {
    let mut fields: Vec<u64> = Vec::new();
    if options.lines {
        fields.push(c.lines);
    }
    if options.words {
        fields.push(c.words);
    }
    if options.chars {
        fields.push(c.chars);
    }
    if options.bytes {
        fields.push(c.bytes);
    }
    if options.max_line {
        fields.push(c.max_line);
    }
    let mut line: Vec<u8> = Vec::new();
    for (n, value) in fields.iter().enumerate() {
        if n > 0 {
            line.push(b' ');
        }
        line.extend_from_slice(format!("{value:>width$}").as_bytes());
    }
    if let Some(name) = name {
        line.push(b' ');
        line.extend_from_slice(name);
    }
    line.push(b'\n');
    let _ = out.write_all(&line);
}

// ------------------------------------------------------------------ running ---

fn run(options: &Options, source: &Source) -> ExitCode {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut failed = false;

    let inputs = match resolve(source) {
        Ok(inputs) => inputs,
        Err(e) => {
            diag!("wc: {e}");
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    let width = number_width(&inputs, options);
    let mut total = Counts::default();
    let show_each = options.total != Total::Only;
    let show_total = match options.total {
        Total::Always | Total::Only => true,
        Total::Never => false,
        Total::Auto => inputs.names.len() > 1,
    };

    for (n, name) in inputs.names.iter().enumerate() {
        // A zero-length name in a `--files0-from` list. Measured: this is a
        // per-name complaint, not a fatal one — the names around it are still
        // counted, and only the exit status remembers.
        if name.as_deref() == Some(b"".as_slice()) {
            diag!(
                "wc: {}:{}: invalid zero-length file name",
                String::from_utf8_lossy(inputs.label.as_deref().unwrap_or(b"-")),
                n.saturating_add(1)
            );
            failed = true;
            continue;
        }
        let c = match read_input(name.as_deref()) {
            Ok(data) => count(&data),
            Err(message) => {
                diag!("wc: {message}");
                failed = true;
                // Upstream still prints a row for an input it could name but
                // not read — a directory reads as zero counts.
                if message.zero_row {
                    Counts::default()
                } else {
                    continue;
                }
            }
        };
        total.add(&c);
        if show_each {
            write_row(&mut out, &c, options, width, name.as_deref());
        }
    }

    if show_total {
        let label: Option<&[u8]> = if options.total == Total::Only {
            None
        } else {
            Some(b"total")
        };
        write_row(&mut out, &total, options, width, label);
    }

    let _ = out.flush();
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// A failure to read one input: what to print, and whether upstream still
/// prints a row of zeros for it.
struct ReadFailure {
    message: String,
    zero_row: bool,
}

impl std::fmt::Display for ReadFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

fn read_stdin() -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    io::stdin().read_to_end(&mut data)?;
    Ok(data)
}

/// The bytes of one input: standard input for the nameless one and for `-`,
/// the named file otherwise.
fn read_input(name: Option<&[u8]>) -> Result<Vec<u8>, ReadFailure> {
    let Some(name) = name else {
        return read_stdin().map_err(|e| ReadFailure {
            message: format!("-: {}", strerror(&e)),
            zero_row: false,
        });
    };
    if name == b"-" {
        return read_stdin().map_err(|e| ReadFailure {
            message: format!("-: {}", strerror(&e)),
            zero_row: false,
        });
    }
    let path = os_from_bytes(name);
    // Checked before opening rather than after, because a directory is not
    // openable on every host this builds for, while upstream's diagnostic —
    // and its row of zeros — is the same everywhere.
    match std::fs::metadata(&path) {
        Ok(m) if m.is_dir() => {
            return Err(ReadFailure {
                message: format!("{}: Is a directory", quotef_os(&path)),
                zero_row: true,
            });
        }
        _ => {}
    }
    let mut file = File::open(&path).map_err(|e| ReadFailure {
        message: format!("{}: {}", quotef_os(&path), strerror(&e)),
        zero_row: false,
    })?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(|e| ReadFailure {
        message: format!("{}: {}", quotef_os(&path), strerror(&e)),
        zero_row: false,
    })?;
    Ok(data)
}

/// Turn the command line's idea of where the inputs come from into the list
/// `run` walks — and, for `--files0-from`, into upstream's `nfiles`.
fn resolve(source: &Source) -> Result<Inputs, getopt::Error> {
    match source {
        // No operand at all is one nameless input, not zero inputs: upstream
        // passes a single null pointer, which is why `wc < /dev/null` prints a
        // row rather than nothing.
        Source::Operands(files) if files.is_empty() => Ok(Inputs {
            names: vec![None],
            nfiles: 1,
            label: None,
        }),
        Source::Operands(files) => {
            let names: Vec<Option<Vec<u8>>> = files.iter().map(|f| Some(arg_bytes(f))).collect();
            Ok(Inputs {
                nfiles: names.len(),
                names,
                label: None,
            })
        }
        Source::Files0From(from) => read_files0(from),
    }
}

/// The size above which upstream streams a `--files0-from` list instead of
/// reading it into memory. Upstream's bound is `MIN (10 MiB,
/// physmem_available () / 2)`; the memory half is not asked here, because it
/// binds only on a machine with under 20 MiB free, and a list that big is
/// already over the fixed bound on every other one.
const FILES0_EAGER_LIMIT: u64 = 10 * 1024 * 1024;

/// The NUL-separated names in `--files0-from=F`.
///
/// Zero-length names are kept rather than rejected. Upstream counts them in
/// `nfiles`, `stat`s them (which fails), and complains about them one at a
/// time as it reaches them — so they belong in the list, where they behave
/// exactly like a name that cannot be stat'ed.
fn read_files0(from: &OsString) -> Result<Inputs, getopt::Error> {
    let source_label = arg_bytes(from);
    let from_stdin = source_label == b"-";
    // Whether upstream would have read the list up front, which is the whole
    // of the difference between `wc --files0-from=list` and `cat list | wc
    // --files0-from=-`: the second cannot know the names in advance, so its
    // `nfiles` is 0 and its column width is 1.
    let sized = matches!(
        if from_stdin {
            stdin_stat()
        } else {
            Stat::of(std::fs::metadata(from))
        },
        Stat::Regular(size) if size <= FILES0_EAGER_LIMIT
    );

    // Both of these name the list file inside a sentence, and upstream spells
    // both with `quoteaf` — the shell-escape-always style, whose marks are
    // straight in every locale — not with `quote()`, whose marks follow §351
    // and are curly. Measured, GNU wc 9.4, `LC_ALL=C.UTF-8`:
    // `wc: cannot open 'nosuch' for reading: No such file or directory`.
    let data = if from_stdin {
        read_stdin().map_err(|e| {
            WC.usage(format!(
                "cannot read file names from {}: {}",
                quoteaf(b"-"),
                strerror(&e)
            ))
        })?
    } else {
        std::fs::read(from).map_err(|e| {
            WC.usage(format!(
                "cannot open {} for reading: {}",
                quoteaf(&source_label),
                strerror(&e)
            ))
        })?
    };

    // A trailing NUL terminates the last name rather than starting an empty
    // one, so the split is over the data with any final NUL removed.
    let body = data.strip_suffix(b"\0").unwrap_or(&data);
    let names: Vec<Option<Vec<u8>>> = if body.is_empty() {
        Vec::new()
    } else {
        body.split(|&b| b == 0).map(|n| Some(n.to_vec())).collect()
    };
    Ok(Inputs {
        nfiles: if sized { names.len() } else { 0 },
        names,
        label: Some(source_label),
    })
}

// -------------------------------------------------------------- byte paths ---

#[cfg(unix)]
fn arg_bytes(a: &OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    a.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn arg_bytes(a: &OsString) -> Vec<u8> {
    a.to_string_lossy().into_owned().into_bytes()
}

#[cfg(unix)]
fn os_from_bytes(b: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(b.to_vec())
}

#[cfg(not(unix))]
fn os_from_bytes(b: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(b).into_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// The diagnostic's own sentence, without the `Try 'wc --help'` referral
    /// every getopt sentence carries and which would triple the length of each
    /// expectation below.
    fn fail_msg(items: &[&str]) -> String {
        let e = parse_args(&args(items)).unwrap_err();
        assert_eq!(e.status, 1, "wc exits 1 on a bad command line, not 2");
        e.sentence
    }

    fn opts(items: &[&str]) -> Options {
        match parse_args(&args(items)).unwrap() {
            Request::Run(o, _) => o,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    // ---------------- counting ----------------

    #[test]
    fn the_counts_of_ordinary_text() {
        let c = count(b"a b c\nd\n");
        assert_eq!(c.lines, 2);
        assert_eq!(c.words, 4);
        assert_eq!(c.bytes, 8);
        assert_eq!(c.chars, 8);
        assert_eq!(c.max_line, 5);
    }

    #[test]
    fn an_unterminated_last_line_is_not_a_line_but_has_a_width() {
        // Measured: `printf 'abcd' | wc -l` is 0 and `wc -L` is 4.
        let c = count(b"abcd");
        assert_eq!(c.lines, 0);
        assert_eq!(c.words, 1);
        assert_eq!(c.max_line, 4);
    }

    #[test]
    fn a_character_is_not_a_byte_and_a_column_is_neither() {
        // U+4E00: three bytes, one character, two columns.
        let c = count("\u{4e00}\n".as_bytes());
        assert_eq!(c.bytes, 4);
        assert_eq!(c.chars, 2);
        assert_eq!(c.max_line, 2);
        // A combining mark is a character that occupies no column.
        let c = count("e\u{301}x\n".as_bytes());
        assert_eq!(c.chars, 4);
        assert_eq!(c.max_line, 2);
    }

    #[test]
    fn a_byte_that_is_no_character_is_counted_only_by_c() {
        // Measured: `printf 'a\377b\n' | wc` gives 1 line, 1 word, 3 chars,
        // 4 bytes, 2 columns — the bad byte joins the word rather than
        // splitting it.
        let c = count(b"a\xffb\n");
        assert_eq!(c.lines, 1);
        assert_eq!(c.words, 1);
        assert_eq!(c.chars, 3);
        assert_eq!(c.bytes, 4);
        assert_eq!(c.max_line, 2);
    }

    #[test]
    fn an_unprintable_character_splits_nothing_and_occupies_nothing() {
        // Measured: `printf 'a\001b\n' | wc -w` is 1, `-L` is 2, `-m` is 4.
        let c = count(b"a\x01b\n");
        assert_eq!(c.words, 1);
        assert_eq!(c.max_line, 2);
        assert_eq!(c.chars, 4);
        // U+2028 has a `wcwidth` of 1 but is not printable, so it behaves the
        // same way — this is the case that a plain width lookup gets wrong.
        let c = count("a\u{2028}b\n".as_bytes());
        assert_eq!(c.words, 1);
        assert_eq!(c.max_line, 2);
    }

    #[test]
    fn the_control_characters_do_four_different_things_to_a_line() {
        // Measured, each of these against GNU 9.4:
        // CR ends the line for -L, so the longest is the part after it.
        assert_eq!(count(b"ab\rcdefg\n").max_line, 5);
        // FF does too, so the longest is the part before it.
        assert_eq!(count(b"abcd\x0cef\n").max_line, 4);
        // VT does not: it separates words but the column keeps running.
        assert_eq!(count(b"ab\x0bcd\n").max_line, 4);
        assert_eq!(count(b"ab\x0bcd\n").words, 2);
        // TAB advances to the next multiple of eight.
        assert_eq!(count(b"abc\tz\n").max_line, 9);
        assert_eq!(count(b"\t\n").max_line, 8);
        // A space is one column and a separator.
        assert_eq!(count(b"a b\n").max_line, 3);
    }

    #[test]
    fn a_unicode_space_separates_words_and_still_takes_its_columns() {
        // Measured: non-breaking space is a separator worth one column…
        let c = count("a\u{a0}b\n".as_bytes());
        assert_eq!(c.words, 2);
        assert_eq!(c.max_line, 3);
        // …and the ideographic space is a separator worth two.
        let c = count("a\u{3000}b\n".as_bytes());
        assert_eq!(c.words, 2);
        assert_eq!(c.max_line, 4);
    }

    #[test]
    fn a_soft_hyphen_and_a_private_use_character_are_ordinary_word_characters() {
        // Measured 3 columns each: both are printable and one column wide,
        // which is where `iswprint` and the Unicode category part company.
        assert_eq!(count("a\u{ad}b\n".as_bytes()).max_line, 3);
        assert_eq!(count("a\u{e000}b\n".as_bytes()).max_line, 3);
        // A zero-width space is printable but occupies nothing.
        assert_eq!(count("a\u{200b}b\n".as_bytes()).max_line, 2);
    }

    // ---------------- option parsing ----------------

    #[test]
    fn with_no_option_it_counts_lines_words_and_bytes() {
        let o = opts(&[]);
        assert!(o.lines && o.words && o.bytes);
        assert!(!o.chars && !o.max_line);
    }

    #[test]
    fn long_options_abbreviate_the_way_getopt_long_does() {
        // Unambiguous prefixes, which the hand-written parser refused.
        assert!(opts(&["--lin"]).lines);
        assert!(opts(&["--max"]).max_line);
        assert!(opts(&["--w"]).words);
        // No two of wc's ten long options share a first letter, so every
        // one-letter prefix resolves — which is why the ambiguity cases in
        // `a_bad_total_argument_is_argmatchs_sentence_not_getopts` had to be
        // built from `--total`'s *argument* words instead.
        assert!(opts(&["--b"]).bytes);
        assert!(opts(&["--c"]).chars);
        assert_eq!(parse_args(&args(&["--h"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--vers"])).unwrap(), Request::Version);
    }

    #[test]
    fn every_getopt_sentence_matches_glibc() {
        assert_eq!(fail_msg(&["-x"]), "invalid option -- 'x'");
        assert_eq!(fail_msg(&["--nope"]), "unrecognized option '--nope'");
        assert_eq!(fail_msg(&["--nope=1"]), "unrecognized option '--nope=1'");
        assert_eq!(
            fail_msg(&["--total"]),
            "option '--total' requires an argument"
        );
        assert_eq!(
            fail_msg(&["--files0-from"]),
            "option '--files0-from' requires an argument"
        );
        assert_eq!(
            fail_msg(&["--debug=1"]),
            "option '--debug' doesn't allow an argument"
        );
        assert_eq!(
            fail_msg(&["--lines=1"]),
            "option '--lines' doesn't allow an argument"
        );
        // An empty prefix matches every option, so this prints the whole table
        // — in its declaration order, which is what makes that order matter.
        assert_eq!(
            fail_msg(&["--=x"]),
            "option '--=x' is ambiguous; possibilities: '--bytes' '--chars' \
             '--lines' '--words' '--debug' '--files0-from' '--max-line-length' \
             '--total' '--help' '--version'"
        );
    }

    #[test]
    fn a_bad_total_argument_is_argmatchs_sentence_not_getopts() {
        let e = parse_args(&args(&["--total=zzz"])).unwrap_err();
        assert_eq!(
            e.message(),
            "invalid argument ‘zzz’ for ‘--total’\nValid arguments are:\n  \
             - ‘auto’\n  - ‘always’\n  - ‘only’\n  - ‘never’\n\
             Try 'wc --help' for more information."
        );
        assert_eq!(e.status, 1);
        // A prefix of one of the words resolves…
        assert_eq!(opts(&["--total=o"]).total, Total::Only);
        // …and one that fits two does not.
        let e = parse_args(&args(&["--total=a"])).unwrap_err();
        assert!(
            e.sentence
                .starts_with("ambiguous argument ‘a’ for ‘--total’")
        );
    }

    #[test]
    fn operands_cannot_be_combined_with_files0_from() {
        // Measured: this is the one usage error of wc's own that still carries
        // the referral, so it is checked with the referral attached.
        let e = parse_args(&args(&["--files0-from=-", "w1"])).unwrap_err();
        assert_eq!(
            e.message(),
            // Straight: measured against GNU wc 9.4 under `LC_ALL=C.UTF-8`,
            // which spells this one with the always-quote flavour.
            "extra operand 'w1'\nfile operands cannot be combined with \
             --files0-from\nTry 'wc --help' for more information."
        );
        assert_eq!(e.status, 1);
    }

    #[test]
    fn a_short_option_is_named_by_the_byte_not_the_char() {
        // `-é` is 0xC3 0xA9. Rendering the byte through `char` would report
        // `invalid option -- 'Ã'` and re-encode it as two bytes, naming an
        // option nobody typed.
        assert_eq!(fail_msg(&["-\u{e9}"]), "invalid option -- '\\303'");
    }

    #[test]
    fn a_lone_dash_is_an_operand_and_double_dash_ends_the_options() {
        match parse_args(&args(&["-", "--", "-l"])).unwrap() {
            Request::Run(_, Source::Operands(files)) => {
                assert_eq!(files, args(&["-", "-l"]));
            }
            other => panic!("expected operands, got {other:?}"),
        }
    }

    // ---------------- column width ----------------

    /// Inputs with the given names and an `nfiles` of `names.len()`, which is
    /// every case but a streamed `--files0-from` list.
    fn inputs(names: &[&str]) -> Inputs {
        let names: Vec<Option<Vec<u8>>> =
            names.iter().map(|n| Some(n.as_bytes().to_vec())).collect();
        Inputs {
            nfiles: names.len(),
            names,
            label: None,
        }
    }

    #[test]
    fn the_column_width_comes_from_the_bytes_about_to_be_read() {
        let o = Options::default().defaulted();
        // Names that cannot be stat'ed contribute nothing, so the sum is zero
        // and the width is the one digit that zero needs. This is upstream's
        // measured behaviour and not an oversight: `wc /nope w1` pads like
        // `wc w1`, not like a file of unknown size.
        assert_eq!(number_width(&inputs(&["/nope/a", "/nope/b"]), &o), 1);

        // A real file, whose size is the width.
        let dir = std::env::temp_dir().join(format!("wc-width-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("big");
        std::fs::write(&big, vec![b'z'; 250]).unwrap();
        let big_name = big.to_string_lossy().into_owned();
        let one = inputs(&[&big_name]);
        assert_eq!(number_width(&one, &o), 3);
        // …but one count of one input is a lone number, printed unpadded.
        let lines_only = Options {
            lines: true,
            ..Options::default()
        };
        assert_eq!(number_width(&one, &lines_only), 1);
        // Two inputs, so the exemption does not apply even for one count.
        let two = inputs(&[&big_name, &big_name]);
        assert_eq!(number_width(&two, &lines_only), 3);

        // Something stattable with no length to sum falls back to 7.
        let dir_name = dir.to_string_lossy().into_owned();
        assert_eq!(number_width(&inputs(&[&dir_name, &big_name]), &o), 7);
        std::fs::remove_file(&big).unwrap();
        std::fs::remove_dir(&dir).unwrap();

        // A streamed `--files0-from` list has no `nfiles`, so nothing to sum.
        let streamed = Inputs {
            names: vec![Some(b"a".to_vec()), Some(b"b".to_vec())],
            nfiles: 0,
            label: Some(b"-".to_vec()),
        };
        assert_eq!(number_width(&streamed, &o), 1);

        // One line of output needs no alignment.
        let only = Options {
            total: Total::Only,
            ..o
        };
        assert_eq!(number_width(&two, &only), 1);
    }

    #[test]
    fn digits_are_counted_the_way_the_padding_needs() {
        assert_eq!(decimal_digits(0), 1);
        assert_eq!(decimal_digits(9), 1);
        assert_eq!(decimal_digits(10), 2);
        assert_eq!(decimal_digits(209), 3);
        assert_eq!(decimal_digits(u64::MAX), 20);
    }

    // ---------------- rows ----------------

    #[test]
    fn a_row_is_the_selected_counts_in_a_fixed_order() {
        let c = Counts {
            lines: 1,
            words: 2,
            chars: 6,
            bytes: 6,
            max_line: 5,
        };
        let all = Options {
            lines: true,
            words: true,
            chars: true,
            bytes: true,
            max_line: true,
            total: Total::Auto,
        };
        let mut buf = Vec::new();
        // Measured: `printf 'ab cd\n' | wc -L -c -m -w -l` prints them in this
        // order whatever order the options were given in.
        write_row(&mut buf, &c, &all, 7, None);
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "      1       2       6       6       5\n"
        );
    }

    #[test]
    fn a_named_row_has_the_name_after_one_space() {
        let c = Counts {
            lines: 2,
            words: 4,
            bytes: 8,
            ..Counts::default()
        };
        let o = Options::default().defaulted();
        let mut buf = Vec::new();
        write_row(&mut buf, &c, &o, 1, Some(b"w1"));
        assert_eq!(String::from_utf8(buf).unwrap(), "2 4 8 w1\n");
        // …and a row with no name has no trailing space.
        let mut buf = Vec::new();
        write_row(&mut buf, &c, &o, 1, None);
        assert_eq!(String::from_utf8(buf).unwrap(), "2 4 8\n");
    }

    #[test]
    fn the_total_takes_the_longest_line_and_not_their_sum() {
        let mut total = Counts::default();
        total.add(&count(b"abc\n"));
        total.add(&count(b"z\n"));
        assert_eq!(total.lines, 2);
        assert_eq!(total.bytes, 6);
        assert_eq!(total.max_line, 3);
    }
}
