//! `ptx` — produce a permuted index of file contents.
//!
//! ```text
//! Usage: ptx [OPTION]... [INPUT]...   (without -G)
//!   or:  ptx -G [OPTION]... [INPUT [OUTPUT]]
//! ```
//!
//! A port of GNU coreutils 9.4's `src/ptx.c`: every word of the input, each on
//! a line of its own with the text around it, sorted by the word -- for a
//! terminal, or as `roff` or `TeX` directives.
//!
//! # How the file is arranged
//!
//! As `pr.rs` is, and for the reason given there: upstream's globals are one
//! struct and its functions methods under their own names -- `find_occurs_in_text`,
//! `define_all_fields`, `output_one_dumb_line` -- so that `ptx.c` can be read
//! beside this file. Upstream's pointers into a file's text are byte offsets
//! into that file's buffer here, as signed numbers where upstream subtracts
//! them.
//!
//! # Regular expressions
//!
//! `-S` and `-W`, and the end-of-sentence pattern used without `-S`, are read
//! as upstream reads them: glibc's Emacs syntax through `re_compile_pattern`,
//! which also makes `^` and `$` hold at every line. That is [`ere::emacs`] and
//! [`ere::Regex::with_newline_anchor`]. Upstream searches a range of a file as
//! though it were a whole string; [`ere::Search::find_window`] and
//! [`ere::Search::longest_at`] do the same over a file decoded once.
//!
//! # Where this differs from upstream on purpose
//!
//! - **An empty keyword match makes progress.** Upstream steps over a word by
//!   `re_match`'s length, so a `-W` pattern that can match nothing (`[a-z]*`)
//!   sends it round a loop that never ends. A zero-length match steps one byte
//!   here, as no match at all does.
//! - **Equal keywords from different files sort in file order.** Upstream
//!   breaks the tie by comparing the addresses of the words, which for words in
//!   different files compares the addresses of two heap buffers: file order when
//!   the buffers come from the heap in turn, and not when a large one is mapped
//!   instead.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote, quotef};
use coreutils::stdfd::{self, Stream};
use coreutils::xnum::{self, Status};
use ere::{Regex, Search};
use std::cmp::Ordering;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

coreutils::guard_std_fds!();

const PTX: Program = Program::new("ptx", 1);

const SHORT_OPTIONS: &str = "AF:GM:ORS:TW:b:i:fg:o:trw:";

/// Upstream's `long_options`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("auto-reference", Takes::Nothing),
    ("break-file", Takes::Required),
    ("flag-truncation", Takes::Required),
    ("ignore-case", Takes::Nothing),
    ("gap-size", Takes::Required),
    ("ignore-file", Takes::Required),
    ("macro-name", Takes::Required),
    ("only-file", Takes::Required),
    ("references", Takes::Nothing),
    ("right-side-refs", Takes::Nothing),
    ("format", Takes::Required),
    ("sentence-regexp", Takes::Required),
    ("traditional", Takes::Nothing),
    ("typeset-mode", Takes::Nothing),
    ("width", Takes::Required),
    ("word-regexp", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

const LONG_TO_SHORT: &[(&str, u8)] = &[
    ("auto-reference", b'A'),
    ("break-file", b'b'),
    ("flag-truncation", b'F'),
    ("ignore-case", b'f'),
    ("gap-size", b'g'),
    ("ignore-file", b'i'),
    ("macro-name", b'M'),
    ("only-file", b'o'),
    ("references", b'r'),
    ("right-side-refs", b'R'),
    ("sentence-regexp", b'S'),
    ("traditional", b'G'),
    ("typeset-mode", b't'),
    ("width", b'w'),
    ("word-regexp", b'W'),
];

/// The end of a sentence, as GNU Emacs has it: punctuation, any closing
/// marks, then the end of a line, a tab or two spaces, then white space.
const SENTENCE_REGEX: &[u8] = b"[.?!][]\"')}]*\\($\\|\t\\|  \\)[ \t\n]*";

fn help_text() -> &'static str {
    "\
Usage: ptx [OPTION]... [INPUT]...   (without -G)
  or:  ptx -G [OPTION]... [INPUT [OUTPUT]]
Output a permuted index, including context, of the words in the input files.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -A, --auto-reference           output automatically generated references
  -G, --traditional              behave more like System V 'ptx'
  -F, --flag-truncation=STRING   use STRING for flagging line truncations.
                                 The default is '/'
  -M, --macro-name=STRING        macro name to use instead of 'xx'
  -O, --format=roff              generate output as roff directives
  -R, --right-side-refs          put references at right, not counted in -w
  -S, --sentence-regexp=REGEXP   for end of lines or end of sentences
  -T, --format=tex               generate output as TeX directives
  -W, --word-regexp=REGEXP       use REGEXP to match each keyword
  -b, --break-file=FILE          word break characters in this FILE
  -f, --ignore-case              fold lower case to upper case for sorting
  -g, --gap-size=NUMBER          gap size in columns between output fields
  -i, --ignore-file=FILE         read ignore word list from FILE
  -o, --only-file=FILE           read only word list from this FILE
  -r, --references               first field of each line is a reference
  -t, --typeset-mode               - not implemented -
  -w, --width=NUMBER             output width in columns, reference excluded
      --help        display this help and exit
      --version     output version information and exit
"
}

/// Upstream's `enum Format`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Unknown,
    Dumb,
    Roff,
    Tex,
}

/// The command line, as upstream's option loop leaves its globals.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "upstream's switches, one per option letter"
)]
struct Options {
    gnu_extensions: bool,
    auto_reference: bool,
    input_reference: bool,
    right_reference: bool,
    line_width: i64,
    gap_size: i64,
    /// `-F`, unescaped; `None` once an empty one has been dropped.
    truncation_string: Vec<u8>,
    macro_name: Vec<u8>,
    output_format: Format,
    ignore_case: bool,
    break_file: Option<OsString>,
    only_file: Option<OsString>,
    ignore_file: Option<OsString>,
    /// `-S`, unescaped, or `None` when not given.
    context_regex: Option<Vec<u8>>,
    /// `-W`, unescaped, or `None` when not given or empty.
    word_regex: Option<Vec<u8>>,
    /// The inputs; `None` is standard input.
    inputs: Vec<Option<OsString>>,
    /// `-G`'s second operand.
    output: Option<OsString>,
    /// `-G`'s third operand, refused only after the output has been opened.
    extra: Option<OsString>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            gnu_extensions: true,
            auto_reference: false,
            input_reference: false,
            right_reference: false,
            line_width: 72,
            gap_size: 3,
            truncation_string: b"/".to_vec(),
            macro_name: b"xx".to_vec(),
            output_format: Format::Unknown,
            ignore_case: false,
            break_file: None,
            only_file: None,
            ignore_file: None,
            context_regex: None,
            word_regex: None,
            inputs: Vec::new(),
            output: None,
            extra: None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Run(Box<Options>),
}

/// Upstream's `unescape_string`: the C escapes of `-F`, `-S` and `-W`,
/// resolved -- and the result cut at its first NUL, since upstream goes on to
/// treat it as a C string.
fn unescape_string(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0usize;
    while let Some(&c) = s.get(i) {
        i = i.saturating_add(1);
        if c != b'\\' {
            out.push(c);
            continue;
        }
        let Some(&e) = s.get(i) else {
            // A lone backslash at the end is ignored.
            break;
        };
        match e {
            b'x' => {
                // `\xhhh`, three digits at most.
                i = i.saturating_add(1);
                let mut value = 0u32;
                let mut length = 0u8;
                while length < 3
                    && let Some(d) = s.get(i).and_then(|&d| char::from(d).to_digit(16))
                {
                    // At most 0xfff: three digits.
                    value = value.wrapping_mul(16).wrapping_add(d);
                    i = i.saturating_add(1);
                    length = length.saturating_add(1);
                }
                if length == 0 {
                    out.extend_from_slice(b"\\x");
                } else {
                    out.push(value.to_le_bytes()[0]);
                }
            }
            b'0' => {
                // `\0ooo`, three digits at most.
                i = i.saturating_add(1);
                let mut value = 0u32;
                let mut length = 0u8;
                while length < 3
                    && let Some(d) = s.get(i).filter(|&&d| (b'0'..=b'7').contains(&d))
                {
                    value = value
                        .wrapping_mul(8)
                        .wrapping_add(u32::from(d.wrapping_sub(b'0')));
                    i = i.saturating_add(1);
                    length = length.saturating_add(1);
                }
                out.push(value.to_le_bytes()[0]);
            }
            b'a' | b'b' | b'f' | b'n' | b'r' | b't' | b'v' => {
                out.push(match e {
                    b'a' => 0x07,
                    b'b' => 0x08,
                    b'f' => 0x0c,
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    _ => 0x0b,
                });
                i = i.saturating_add(1);
            }
            // Cancel the rest of the string.
            b'c' => break,
            _ => {
                out.push(b'\\');
                out.push(e);
                i = i.saturating_add(1);
            }
        }
    }
    if let Some(nul) = out.iter().position(|&b| b == 0) {
        out.truncate(nul);
    }
    out
}

/// `-g` and `-w`: `xstrtoimax` in base 0 (so `0x10` is sixteen), positive.
fn positive_number(arg: &[u8], what: &str) -> Result<i64, getopt::Error> {
    match xnum::xstrtoimax_base(arg, 0, Some(b"")) {
        (value, Status::Ok) if value > 0 => Ok(value),
        _ => Err(PTX.usage(format!("invalid {what}: {}", quote(arg)))),
    }
}

/// Upstream's option loop and its handling of the operands after it.
///
/// # Errors
///
/// Any refused option or argument, with upstream's wording and status.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut o = Options::default();
    let mut operands: Vec<OsString> = Vec::new();
    for item in PTX.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        let (letter, value) = match item? {
            Opt::Operand(word) => {
                operands.push(word.clone());
                continue;
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Long("format", v) => {
                let text = v.map(|v| os_bytes(&v).into_owned()).unwrap_or_default();
                o.output_format = PTX.argmatch(
                    &text,
                    "--format",
                    &[("roff", Format::Roff), ("tex", Format::Tex)],
                )?;
                continue;
            }
            Opt::Long(name, v) => match LONG_TO_SHORT.iter().find(|(long, _)| *long == name) {
                Some(&(_, c)) => (c, v),
                None => {
                    return Err(PTX.usage_referring(format!("option '--{name}' is unhandled")));
                }
            },
            Opt::Short(c, v) => (c, v),
        };
        let arg = value.map(|v| os_bytes(&v).into_owned()).unwrap_or_default();
        match letter {
            b'G' => o.gnu_extensions = false,
            b'b' => o.break_file = Some(os_from(&arg)),
            b'f' => o.ignore_case = true,
            b'g' => o.gap_size = positive_number(&arg, "gap width")?,
            b'i' => o.ignore_file = Some(os_from(&arg)),
            b'o' => o.only_file = Some(os_from(&arg)),
            b'r' => o.input_reference = true,
            // Upstream: "Yet to understand..."
            b't' => {}
            b'w' => o.line_width = positive_number(&arg, "line width")?,
            b'A' => o.auto_reference = true,
            b'F' => o.truncation_string = unescape_string(&arg),
            b'M' => o.macro_name = arg,
            b'O' => o.output_format = Format::Roff,
            b'R' => o.right_reference = true,
            b'S' => o.context_regex = Some(unescape_string(&arg)),
            b'T' => o.output_format = Format::Tex,
            b'W' => {
                let word = unescape_string(&arg);
                o.word_regex = (!word.is_empty()).then_some(word);
            }
            // Unreachable: every letter of the table is handled above.
            other => return Err(PTX.invalid_option(other)),
        }
    }

    // `""` and `-` are standard input.
    let input = |word: &OsString| {
        let bytes = os_bytes(word);
        (!bytes.is_empty() && *bytes != *b"-").then(|| word.clone())
    };
    let mut rest = operands.iter();
    match rest.next() {
        None => o.inputs.push(None),
        Some(first) if o.gnu_extensions => {
            o.inputs.push(input(first));
            o.inputs.extend(rest.map(input));
        }
        Some(first) => {
            o.inputs.push(input(first));
            o.output = rest.next().cloned();
            o.extra = rest.next().cloned();
        }
    }
    if o.output_format == Format::Unknown {
        o.output_format = if o.gnu_extensions {
            Format::Dumb
        } else {
            Format::Roff
        };
    }
    Ok(Request::Run(Box::new(o)))
}

fn os_from(bytes: &[u8]) -> OsString {
    coreutils::quote::os_from_bytes(bytes)
}

// --------------------------------------------------------------- engine ---

/// A failure that ends the run: the message after `ptx: `.
#[derive(Debug)]
struct Fatal(String);

fn matcher_error() -> Fatal {
    Fatal("error in regular expression matcher".to_string())
}

/// `isspace` in the `C` and UTF-8 locales alike, for a byte on its own.
fn isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// A yes-or-no answer for every byte value: upstream's `char` arrays of
/// `CHAR_SET_SIZE`. Indexed by a `u8`, so no lookup can miss; `get` says so
/// without a panic path.
#[derive(Clone, Copy)]
struct ByteSet([bool; 256]);

impl ByteSet {
    fn contains(&self, c: u8) -> bool {
        self.0.get(usize::from(c)).copied().unwrap_or(false)
    }

    fn set(&mut self, c: u8, on: bool) {
        if let Some(slot) = self.0.get_mut(usize::from(c)) {
            *slot = on;
        }
    }
}

/// One file, whole, and its keyword search.
struct Text<'r> {
    bytes: Vec<u8>,
    /// The name references print, or `None` for standard input.
    name: Option<Vec<u8>>,
    /// The `-W` pattern's search over `bytes`, decoded once.
    word: Option<Search<'r>>,
}

impl Text<'_> {
    /// The byte at `pos`, or NUL past the end -- upstream's buffers are
    /// NUL-terminated, and one `SKIP_SOMETHING` can look at that terminator.
    fn at(&self, pos: i64) -> u8 {
        usize::try_from(pos)
            .ok()
            .and_then(|p| self.bytes.get(p))
            .copied()
            .unwrap_or(0)
    }

    fn len(&self) -> i64 {
        i64::try_from(self.bytes.len()).unwrap_or(i64::MAX)
    }
}

/// Upstream's `OCCURS`.
#[derive(Clone, Copy, Debug)]
struct Occurs {
    key_start: i64,
    key_size: i64,
    /// Distance from the keyword to the start of its left context.
    left: i64,
    /// Distance from the keyword to the end of its right context.
    right: i64,
    /// The line number under `-A`, or the distance to the reference under
    /// `-r`.
    reference: i64,
    file_index: usize,
}

/// Upstream's `BLOCK`, within one file: `start..end`, empty when both are 0
/// as upstream's null pointers are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Block {
    start: i64,
    end: i64,
}

/// The output fields of one line.
#[derive(Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "upstream's four truncation flags, one per field"
)]
struct Fields {
    tail: Block,
    tail_truncation: bool,
    before: Block,
    before_truncation: bool,
    keyafter: Block,
    keyafter_truncation: bool,
    head: Block,
    head_truncation: bool,
    reference: Vec<u8>,
}

/// Upstream's globals and functions. See the module docs.
struct Ptx<'r> {
    o: Options,
    context_regex: Option<&'r Regex>,
    /// The `-S` text, or the default, for the one diagnostic that quotes it.
    context_regex_string: Vec<u8>,
    word_regex: Option<&'r Regex>,
    word_fastmap: ByteSet,
    ignore_table: Vec<Vec<u8>>,
    only_table: Vec<Vec<u8>>,
    texts: Vec<Text<'r>>,
    file_line_count: Vec<i64>,
    total_line_count: i64,
    maximum_word_length: i64,
    reference_max_width: i64,
    occurs: Vec<Occurs>,
    edited_flag: ByteSet,
    half_line_width: i64,
    before_max_width: i64,
    keyafter_max_width: i64,
    truncation_string_length: i64,
}

// Offsets are positions in one file's buffer and differences between them,
// widened to `i64`; widths are options held below `PTRDIFF_MAX` or sums of a
// few such. No sum here approaches `i64`'s range.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "offsets within one buffer and small sums of them; see the comment above"
)]
impl<'r> Ptx<'r> {
    fn compare_words(&self, first: &[u8], second: &[u8]) -> Ordering {
        if self.o.ignore_case {
            // `folded_chars`, which is `toupper` of each byte: ASCII's in
            // the C and UTF-8 locales alike.
            let fold = u8::to_ascii_uppercase;
            first.iter().map(fold).cmp(second.iter().map(fold))
        } else {
            first.cmp(second)
        }
    }

    /// `file_line_count[n - 1]`: how many lines the files before file `n`
    /// held between them, as upstream counts them; 0 before the first.
    fn lines_before(&self, n: usize) -> i64 {
        n.checked_sub(1)
            .and_then(|i| self.file_line_count.get(i))
            .copied()
            .unwrap_or(0)
    }

    fn key<'t>(&self, texts: &'t [Text<'r>], occurs: &Occurs) -> &'t [u8] {
        let Some(text) = texts.get(occurs.file_index) else {
            return &[];
        };
        let start = usize::try_from(occurs.key_start).unwrap_or(0);
        let end = start + usize::try_from(occurs.key_size).unwrap_or(0);
        text.bytes.get(start..end).unwrap_or_default()
    }

    /// `search_table`: whether `word` is in the sorted `table`.
    fn search_table(&self, word: &[u8], table: &[Vec<u8>]) -> bool {
        table
            .binary_search_by(|probe| self.compare_words(probe, word))
            .is_ok()
    }

    fn skip_white(text: &Text<'_>, mut cursor: i64, limit: i64) -> i64 {
        while cursor < limit && isspace(text.at(cursor)) {
            cursor += 1;
        }
        cursor
    }

    fn skip_non_white(text: &Text<'_>, mut cursor: i64, limit: i64) -> i64 {
        while cursor < limit && !isspace(text.at(cursor)) {
            cursor += 1;
        }
        cursor
    }

    fn skip_white_backwards(text: &Text<'_>, mut cursor: i64, start: i64) -> i64 {
        while cursor > start && isspace(text.at(cursor - 1)) {
            cursor -= 1;
        }
        cursor
    }

    /// `SKIP_SOMETHING`: over one word -- the `-W` match at `cursor`, or a run
    /// of word characters -- or else over one byte.
    fn skip_something(&self, text: &Text<'_>, cursor: i64, limit: i64) -> Result<i64, Fatal> {
        if let Some(search) = &text.word {
            let lo = usize::try_from(cursor).unwrap_or(0);
            let hi = usize::try_from(limit).unwrap_or(0).max(lo);
            let end = search.longest_at(lo, hi).map_err(|_| matcher_error())?;
            // `cursor += count == -1 ? 1 : count`, except that a match of no
            // length steps one byte too: see the module docs.
            return Ok(match end.and_then(|e| i64::try_from(e).ok()) {
                Some(e) if e > cursor => e,
                _ => cursor + 1,
            });
        }
        let mut cursor = cursor;
        if self.word_fastmap.contains(text.at(cursor)) {
            while cursor < limit && self.word_fastmap.contains(text.at(cursor)) {
                cursor += 1;
            }
        } else {
            cursor += 1;
        }
        Ok(cursor)
    }

    /// `find_occurs_in_text`: every keyword of file `file_index`, with its
    /// context.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's one function, whose state is carried between its loops"
    )]
    fn find_occurs_in_text(&mut self, file_index: usize) -> Result<(), Fatal> {
        let Some(text) = self.texts.get(file_index) else {
            return Ok(());
        };
        let end = text.len();
        let context_search = self.context_regex.map(|re| re.search(&text.bytes));

        let mut reference_length = 0;
        let mut line_start = 0;
        let mut line_scan = 0;
        if self.o.input_reference {
            line_scan = Self::skip_non_white(text, line_scan, end);
            reference_length = line_scan - line_start;
            line_scan = Self::skip_white(text, line_scan, end);
        }

        let mut found = Vec::new();
        let mut cursor = 0;
        while cursor < end {
            let mut context_start = cursor;
            let mut next_context_start = end;
            if let Some(search) = &context_search {
                let lo = usize::try_from(cursor).unwrap_or(0);
                match search
                    .find_window(lo, text.bytes.len())
                    .map_err(|_| matcher_error())?
                {
                    None => {}
                    Some((s, _)) if s == lo => {
                        return Err(Fatal(format!(
                            "error: regular expression has a match of length zero: {}",
                            quote(&self.context_regex_string)
                        )));
                    }
                    Some((_, e)) => next_context_start = i64::try_from(e).unwrap_or(end),
                }
            }

            // The separator is in the right context, but not its trailing
            // white space.
            let context_end = Self::skip_white_backwards(text, next_context_start, context_start);

            loop {
                // A zero-length word at the end of the context steps the cursor
                // one past it. Upstream's next `re_search` is then given a
                // negative length, which matches nothing; a window here would
                // match the empty string again, for ever.
                if cursor > context_end {
                    break;
                }
                let (word_start, word_end);
                if let Some(search) = &text.word {
                    let lo = usize::try_from(cursor).unwrap_or(0);
                    let hi = usize::try_from(context_end).unwrap_or(0).max(lo);
                    let Some((s, e)) = search.find_window(lo, hi).map_err(|_| matcher_error())?
                    else {
                        break;
                    };
                    word_start = i64::try_from(s).unwrap_or(end);
                    word_end = i64::try_from(e).unwrap_or(end);
                } else {
                    let mut scan = cursor;
                    while scan < context_end && !self.word_fastmap.contains(text.at(scan)) {
                        scan += 1;
                    }
                    if scan == context_end {
                        break;
                    }
                    word_start = scan;
                    while scan < context_end && self.word_fastmap.contains(text.at(scan)) {
                        scan += 1;
                    }
                    word_end = scan;
                }

                cursor = word_start;
                // Skip a zero-length word, one position at a time.
                if word_end == word_start {
                    cursor += 1;
                    continue;
                }

                let key_start = cursor;
                let key_size = word_end - word_start;
                cursor += key_size;
                // Every word counts toward the maximum, kept or not: a backward
                // jump when printing may land in any of them.
                self.maximum_word_length = self.maximum_word_length.max(key_size);

                if self.o.input_reference {
                    while line_scan < key_start {
                        if text.at(line_scan) == b'\n' {
                            self.total_line_count += 1;
                            line_scan += 1;
                            line_start = line_scan;
                            line_scan = Self::skip_non_white(text, line_scan, end);
                            reference_length = line_scan - line_start;
                        } else {
                            line_scan += 1;
                        }
                    }
                    // The word is part of the reference.
                    if line_scan > key_start {
                        continue;
                    }
                }

                let key_bytes = text
                    .bytes
                    .get(
                        usize::try_from(key_start).unwrap_or(0)
                            ..usize::try_from(word_end).unwrap_or(0),
                    )
                    .unwrap_or_default();
                if self.o.ignore_file.is_some() && self.search_table(key_bytes, &self.ignore_table)
                {
                    continue;
                }
                if self.o.only_file.is_some() && !self.search_table(key_bytes, &self.only_table) {
                    continue;
                }

                let mut reference = 0;
                if self.o.auto_reference {
                    // Count lines on the way; under `-r` too, `line_scan` has
                    // already been moved and this does nothing.
                    while line_scan < key_start {
                        if text.at(line_scan) == b'\n' {
                            self.total_line_count += 1;
                            line_scan += 1;
                            line_start = line_scan;
                            line_scan = Self::skip_non_white(text, line_scan, end);
                        } else {
                            line_scan += 1;
                        }
                    }
                    reference = self.total_line_count;
                } else if self.o.input_reference {
                    reference = line_start - key_start;
                    self.reference_max_width = self.reference_max_width.max(reference_length);
                }

                // Leave the reference out of the context in the simple case.
                if self.o.input_reference && line_start == context_start {
                    context_start = Self::skip_non_white(text, context_start, context_end);
                    context_start = Self::skip_white(text, context_start, context_end);
                }

                found.push(Occurs {
                    key_start,
                    key_size,
                    left: context_start - key_start,
                    right: context_end - key_start,
                    reference,
                    file_index,
                });
            }
            cursor = next_context_start;
        }
        self.occurs.extend(found);
        Ok(())
    }

    /// `sort_found_occurs`: by keyword, then by where the keyword is.
    fn sort_found_occurs(&mut self) {
        let mut occurs = std::mem::take(&mut self.occurs);
        occurs.sort_by(|a, b| {
            self.compare_words(self.key(&self.texts, a), self.key(&self.texts, b))
                .then((a.file_index, a.key_start).cmp(&(b.file_index, b.key_start)))
        });
        self.occurs = occurs;
    }

    /// `fix_output_parameters`: the field widths, and which characters the
    /// output format edits.
    fn fix_output_parameters(&mut self) {
        if self.o.auto_reference {
            // The widest `FILE:LINE`, plus the colon.
            self.reference_max_width = 0;
            for (file_index, text) in self.texts.iter().enumerate() {
                let mut line_ordinal = self.lines_before(file_index + 1) + 1;
                line_ordinal -= self.lines_before(file_index);
                let mut width = i64::try_from(line_ordinal.to_string().len()).unwrap_or(0);
                if let Some(name) = &text.name {
                    width += i64::try_from(name.len()).unwrap_or(0);
                }
                self.reference_max_width = self.reference_max_width.max(width);
            }
            self.reference_max_width += 1;
        }

        // A reference on the left takes its room, and a gap, from the line.
        if (self.o.auto_reference || self.o.input_reference) && !self.o.right_reference {
            self.o.line_width -= self.reference_max_width + self.o.gap_size;
        }
        self.o.line_width = self.o.line_width.max(0);

        self.half_line_width = self.o.line_width / 2;
        self.before_max_width = self.half_line_width - self.o.gap_size;
        self.keyafter_max_width = self.half_line_width;

        // An empty truncation string means no truncation marks at all.
        self.truncation_string_length = i64::try_from(self.o.truncation_string.len()).unwrap_or(0);

        if self.o.gnu_extensions {
            // At most two marks, one either side of the keyword or both on one
            // side: room for both is kept on both.
            self.before_max_width -= 2 * self.truncation_string_length;
            self.before_max_width = self.before_max_width.max(0);
            self.keyafter_max_width -= 2 * self.truncation_string_length;
        } else {
            // "I never figured out exactly how UNIX' ptx plans the output width
            // of its various fields" -- nor does this.
            self.keyafter_max_width -= 2 * self.truncation_string_length + 1;
        }

        for c in 0..=255u8 {
            self.edited_flag.set(c, isspace(c));
        }
        match self.o.output_format {
            Format::Unknown | Format::Dumb => {}
            Format::Roff => self.edited_flag.set(b'"', true),
            Format::Tex => {
                for &c in b"$%&#_{}\\" {
                    self.edited_flag.set(c, true);
                }
            }
        }
    }

    /// `define_all_fields`: the four fields of one output line, and its
    /// reference.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's one function, whose fields depend on each other"
    )]
    fn define_all_fields(&self, text: &Text<'_>, occurs: &Occurs) -> Result<Fields, Fatal> {
        let truncating = !self.o.truncation_string.is_empty();
        let mut f = Fields::default();

        // `keyafter` is the keyword and what follows, by whole words or single
        // separators, within its width and the right context.
        f.keyafter.start = occurs.key_start;
        f.keyafter.end = f.keyafter.start + occurs.key_size;
        let left_context_start = f.keyafter.start + occurs.left;
        let right_context_end = f.keyafter.start + occurs.right;
        let buffer_start = 0;
        let buffer_end = text.len();

        let mut cursor = f.keyafter.end;
        while cursor < right_context_end && cursor <= f.keyafter.start + self.keyafter_max_width {
            f.keyafter.end = cursor;
            cursor = self.skip_something(text, cursor, right_context_end)?;
        }
        if cursor <= f.keyafter.start + self.keyafter_max_width {
            f.keyafter.end = cursor;
        }
        f.keyafter_truncation = truncating && f.keyafter.end < right_context_end;
        f.keyafter.end = Self::skip_white_backwards(text, f.keyafter.end, f.keyafter.start);

        // A wide left context is entered by a safe jump back from the keyword
        // -- half a line and the longest word -- then a skip to a word start.
        let left_field_start = if -occurs.left > self.half_line_width + self.maximum_word_length {
            let jump = f.keyafter.start - (self.half_line_width + self.maximum_word_length);
            self.skip_something(text, jump, f.keyafter.start)?
        } else {
            f.keyafter.start + occurs.left
        };

        // `before` ends at the keyword, less spaces, and starts as far left as
        // its width allows, by words or single separators.
        f.before.start = left_field_start;
        f.before.end = Self::skip_white_backwards(text, f.keyafter.start, f.before.start);
        while f.before.start + self.before_max_width < f.before.end {
            f.before.start = self.skip_something(text, f.before.start, f.before.end)?;
        }
        f.before_truncation = truncating && {
            let c = Self::skip_white_backwards(text, f.before.start, buffer_start);
            c > left_context_start
        };
        f.before.start = Self::skip_white(text, f.before.start, buffer_end);

        // `tail` takes what `before` leaves of its half, less a gap, from after
        // the right context -- whole words only.
        let tail_max_width =
            self.before_max_width - (f.before.end - f.before.start) - self.o.gap_size;
        if tail_max_width > 0 {
            f.tail.start = Self::skip_white(text, f.keyafter.end, buffer_end);
            f.tail.end = f.tail.start;
            let mut cursor = f.tail.end;
            while cursor < right_context_end && cursor < f.tail.start + tail_max_width {
                f.tail.end = cursor;
                cursor = self.skip_something(text, cursor, right_context_end)?;
            }
            if cursor < f.tail.start + tail_max_width {
                f.tail.end = cursor;
            }
            if f.tail.end > f.tail.start {
                f.keyafter_truncation = false;
                f.tail_truncation = truncating && f.tail.end < right_context_end;
            } else {
                f.tail_truncation = false;
            }
            f.tail.end = Self::skip_white_backwards(text, f.tail.end, f.tail.start);
        } else {
            f.tail = Block::default();
            f.tail_truncation = false;
        }

        // `head` takes what `keyafter` leaves of its half, less a gap, from
        // before the left context.
        let head_max_width =
            self.keyafter_max_width - (f.keyafter.end - f.keyafter.start) - self.o.gap_size;
        if head_max_width > 0 {
            f.head.end = Self::skip_white_backwards(text, f.before.start, buffer_start);
            f.head.start = left_field_start;
            while f.head.start + head_max_width < f.head.end {
                f.head.start = self.skip_something(text, f.head.start, f.head.end)?;
            }
            if f.head.end > f.head.start {
                f.before_truncation = false;
                f.head_truncation = truncating && f.head.start > left_context_start;
            } else {
                f.head_truncation = false;
            }
            f.head.start = Self::skip_white(text, f.head.start, f.head.end);
        } else {
            f.head = Block::default();
            f.head_truncation = false;
        }

        if self.o.auto_reference {
            // `FILE:LINE`, one-based, standard input's name empty.
            let line_ordinal = occurs.reference + 1 - self.lines_before(occurs.file_index);
            f.reference = text.name.clone().unwrap_or_default();
            f.reference.push(b':');
            f.reference
                .extend_from_slice(line_ordinal.to_string().as_bytes());
        } else if self.o.input_reference {
            // The first word of the line, whatever is in it.
            let start = f.keyafter.start + occurs.reference;
            let end = Self::skip_non_white(text, start, right_context_end);
            f.reference = self.slice(text, Block { start, end }).to_vec();
        }
        Ok(f)
    }

    fn slice<'t>(&self, text: &'t Text<'_>, block: Block) -> &'t [u8] {
        let (Ok(start), Ok(end)) = (usize::try_from(block.start), usize::try_from(block.end))
        else {
            return &[];
        };
        text.bytes.get(start..end.max(start)).unwrap_or_default()
    }

    /// `print_field`: a field, white space as single spaces and the output
    /// format's specials escaped.
    fn print_field(&self, out: &mut Vec<u8>, field: &[u8]) {
        for &c in field {
            if !self.edited_flag.contains(c) {
                out.push(c);
                continue;
            }
            match c {
                // roff: double any quote.
                b'"' => out.extend_from_slice(b"\"\""),
                // TeX: a backslash before these...
                b'$' | b'%' | b'&' | b'#' | b'_' => {
                    out.push(b'\\');
                    out.push(c);
                }
                // ...and these in mathematical mode...
                b'{' | b'}' => {
                    out.extend_from_slice(b"$\\");
                    out.push(c);
                    out.push(b'$');
                }
                // ...and a backslash by name.
                b'\\' => out.extend_from_slice(b"\\backslash{}"),
                // White space.
                _ => out.push(b' '),
            }
        }
    }

    fn print_spaces(out: &mut Vec<u8>, number: i64) {
        for _ in 0..number.max(0) {
            out.push(b' ');
        }
    }

    fn truncation(&self, on: bool) -> &[u8] {
        if on { &self.o.truncation_string } else { &[] }
    }

    /// `output_one_roff_line`.
    fn output_one_roff_line(&self, out: &mut Vec<u8>, text: &Text<'_>, f: &Fields) {
        out.push(b'.');
        out.extend_from_slice(&self.o.macro_name);
        out.extend_from_slice(b" \"");
        self.print_field(out, self.slice(text, f.tail));
        out.extend_from_slice(self.truncation(f.tail_truncation));
        out.push(b'"');

        out.extend_from_slice(b" \"");
        out.extend_from_slice(self.truncation(f.before_truncation));
        self.print_field(out, self.slice(text, f.before));
        out.push(b'"');

        out.extend_from_slice(b" \"");
        self.print_field(out, self.slice(text, f.keyafter));
        out.extend_from_slice(self.truncation(f.keyafter_truncation));
        out.push(b'"');

        out.extend_from_slice(b" \"");
        out.extend_from_slice(self.truncation(f.head_truncation));
        self.print_field(out, self.slice(text, f.head));
        out.push(b'"');

        if self.o.auto_reference || self.o.input_reference {
            out.extend_from_slice(b" \"");
            self.print_field(out, &f.reference);
            out.push(b'"');
        }
        out.push(b'\n');
    }

    /// `output_one_tex_line`: the keyword and what follows it are split into
    /// two arguments here.
    fn output_one_tex_line(
        &self,
        out: &mut Vec<u8>,
        text: &Text<'_>,
        f: &Fields,
    ) -> Result<(), Fatal> {
        out.push(b'\\');
        out.extend_from_slice(&self.o.macro_name);
        out.extend_from_slice(b" {");
        self.print_field(out, self.slice(text, f.tail));
        out.extend_from_slice(b"}{");
        self.print_field(out, self.slice(text, f.before));
        out.extend_from_slice(b"}{");
        let key_end = self.skip_something(text, f.keyafter.start, f.keyafter.end)?;
        let key = Block {
            start: f.keyafter.start,
            end: key_end,
        };
        let after = Block {
            start: key_end,
            end: f.keyafter.end,
        };
        self.print_field(out, self.slice(text, key));
        out.extend_from_slice(b"}{");
        self.print_field(out, self.slice(text, after));
        out.extend_from_slice(b"}{");
        self.print_field(out, self.slice(text, f.head));
        out.push(b'}');
        if self.o.auto_reference || self.o.input_reference {
            out.push(b'{');
            self.print_field(out, &f.reference);
            out.push(b'}');
        }
        out.push(b'\n');
        Ok(())
    }

    /// `output_one_dumb_line`.
    fn output_one_dumb_line(&self, out: &mut Vec<u8>, text: &Text<'_>, f: &Fields) {
        let len = |b: Block| b.end - b.start;
        let reference_len = i64::try_from(f.reference.len()).unwrap_or(0);
        let mark = |on: bool| if on { self.truncation_string_length } else { 0 };
        let refs = self.o.auto_reference || self.o.input_reference;

        if !self.o.right_reference {
            if self.o.auto_reference {
                // The reference, as GNU Emacs' `next-error` reads it; the gap
                // that follows supplies the colon's column.
                self.print_field(out, &f.reference);
                out.push(b':');
                Self::print_spaces(
                    out,
                    self.reference_max_width + self.o.gap_size - reference_len - 1,
                );
            } else {
                self.print_field(out, &f.reference);
                Self::print_spaces(
                    out,
                    self.reference_max_width + self.o.gap_size - reference_len,
                );
            }
        }

        if f.tail.start < f.tail.end {
            self.print_field(out, self.slice(text, f.tail));
            out.extend_from_slice(self.truncation(f.tail_truncation));
            Self::print_spaces(
                out,
                self.half_line_width
                    - self.o.gap_size
                    - len(f.before)
                    - mark(f.before_truncation)
                    - len(f.tail)
                    - mark(f.tail_truncation),
            );
        } else {
            Self::print_spaces(
                out,
                self.half_line_width - self.o.gap_size - len(f.before) - mark(f.before_truncation),
            );
        }

        out.extend_from_slice(self.truncation(f.before_truncation));
        self.print_field(out, self.slice(text, f.before));
        Self::print_spaces(out, self.o.gap_size);

        self.print_field(out, self.slice(text, f.keyafter));
        out.extend_from_slice(self.truncation(f.keyafter_truncation));

        if f.head.start < f.head.end {
            Self::print_spaces(
                out,
                self.half_line_width
                    - len(f.keyafter)
                    - mark(f.keyafter_truncation)
                    - len(f.head)
                    - mark(f.head_truncation),
            );
            out.extend_from_slice(self.truncation(f.head_truncation));
            self.print_field(out, self.slice(text, f.head));
        } else if refs && self.o.right_reference {
            Self::print_spaces(
                out,
                self.half_line_width - len(f.keyafter) - mark(f.keyafter_truncation),
            );
        }

        if refs && self.o.right_reference {
            Self::print_spaces(out, self.o.gap_size);
            self.print_field(out, &f.reference);
        }
        out.push(b'\n');
    }

    /// `generate_all_output`: one line per keyword occurrence, in order.
    fn generate_all_output(&self, sink: &mut dyn Write) -> Result<(), Fatal> {
        let mut line = Vec::new();
        for occurs in &self.occurs {
            let Some(text) = self.texts.get(occurs.file_index) else {
                continue;
            };
            let fields = self.define_all_fields(text, occurs)?;
            line.clear();
            match self.o.output_format {
                Format::Unknown | Format::Dumb => {
                    self.output_one_dumb_line(&mut line, text, &fields);
                }
                Format::Roff => self.output_one_roff_line(&mut line, text, &fields),
                Format::Tex => self.output_one_tex_line(&mut line, text, &fields)?,
            }
            // A failed write is the sink's to remember and report at the end.
            let _ = sink.write_all(&line);
        }
        Ok(())
    }
}

/// `swallow_file_in_memory`: a whole file, or standard input for `None`.
fn swallow_file_in_memory(file_name: Option<&OsString>) -> Result<Vec<u8>, Fatal> {
    let read = match file_name {
        Some(name) => std::fs::read(name),
        None => read_stdin(),
    };
    read.map_err(|why| {
        let shown = file_name.map_or_else(|| b"-".to_vec(), |n| os_bytes(n).into_owned());
        Fatal(format!("{}: {}", quotef(&shown), strerror(&why)))
    })
}

/// All of standard input, through [`stdfd::read`] so that a closed one is
/// `EBADF`. Upstream then clears the end-of-file flag, so a second `-` reads
/// again -- and finds the end straight away when the first read to it.
fn read_stdin() -> io::Result<Vec<u8>> {
    let mut all = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = stdfd::read(0, &mut buf)?;
        if n == 0 {
            return Ok(all);
        }
        all.extend_from_slice(buf.get(..n).unwrap_or_default());
    }
}

/// `digest_word_file`: one word per line, empty lines skipped, sorted.
fn digest_word_file(bytes: &[u8], ptx: &Ptx<'_>) -> Vec<Vec<u8>> {
    let mut table: Vec<Vec<u8>> = bytes
        .split(|&b| b == b'\n')
        .filter(|w| !w.is_empty())
        .map(<[u8]>::to_vec)
        .collect();
    table.sort_by(|a, b| ptx.compare_words(a, b));
    table
}

/// Compile a pattern as upstream's `compile_regex` does.
fn compile_regex(string: &[u8], ignore_case: bool) -> Result<Regex, Fatal> {
    ere::emacs::compile(string, ignore_case)
        .map(|re| re.with_newline_anchor(true))
        .map_err(|e| Fatal(format!("{} (for regexp {})", e.message(), quote(string))))
}

/// Everything after the command line: upstream's `main` from
/// `initialize_regex` on.
fn run(o: Options, sink: &mut dyn Write) -> Result<(), Fatal> {
    // `initialize_regex`.
    let context_string: Option<Vec<u8>> = match &o.context_regex {
        Some(given) if given.is_empty() => None,
        Some(given) => Some(given.clone()),
        None if o.gnu_extensions && !o.input_reference => Some(SENTENCE_REGEX.to_vec()),
        None => Some(b"\n".to_vec()),
    };
    let context_regex = context_string
        .as_deref()
        .map(|s| compile_regex(s, o.ignore_case))
        .transpose()?;
    let word_regex = o
        .word_regex
        .as_deref()
        .map(|s| compile_regex(s, o.ignore_case))
        .transpose()?;

    let mut word_fastmap = ByteSet([false; 256]);
    if word_regex.is_none() && o.break_file.is_none() {
        for c in 0..=255u8 {
            let on = if o.gnu_extensions {
                // `\w+`, as `isalpha` has it.
                c.is_ascii_alphabetic()
            } else {
                // `[^ \t\n]+`.
                !matches!(c, b' ' | b'\t' | b'\n')
            };
            word_fastmap.set(c, on);
        }
    }

    let mut ptx = Ptx {
        context_regex: context_regex.as_ref(),
        context_regex_string: context_string.unwrap_or_default(),
        word_regex: word_regex.as_ref(),
        word_fastmap,
        ignore_table: Vec::new(),
        only_table: Vec::new(),
        texts: Vec::new(),
        file_line_count: Vec::new(),
        total_line_count: 0,
        maximum_word_length: 0,
        reference_max_width: 0,
        occurs: Vec::new(),
        edited_flag: ByteSet([false; 256]),
        half_line_width: 0,
        before_max_width: 0,
        keyafter_max_width: 0,
        truncation_string_length: 0,
        o,
    };

    // `digest_break_file`: every character is a word character but those in
    // the file -- and, without GNU extensions, space, tab and newline.
    if let Some(name) = ptx.o.break_file.clone() {
        let contents = swallow_file_in_memory(Some(&name))?;
        ptx.word_fastmap = ByteSet([true; 256]);
        for &c in &contents {
            ptx.word_fastmap.set(c, false);
        }
        if !ptx.o.gnu_extensions {
            for c in [b' ', b'\t', b'\n'] {
                ptx.word_fastmap.set(c, false);
            }
        }
    }

    // An empty word list is no list at all.
    if let Some(name) = ptx.o.ignore_file.clone() {
        let contents = swallow_file_in_memory(Some(&name))?;
        ptx.ignore_table = digest_word_file(&contents, &ptx);
        if ptx.ignore_table.is_empty() {
            ptx.o.ignore_file = None;
        }
    }
    if let Some(name) = ptx.o.only_file.clone() {
        let contents = swallow_file_in_memory(Some(&name))?;
        ptx.only_table = digest_word_file(&contents, &ptx);
        if ptx.only_table.is_empty() {
            ptx.o.only_file = None;
        }
    }

    // Study each file, counting its lines as upstream does: one more at its
    // end, which is how an incomplete last line is numbered.
    let inputs = std::mem::take(&mut ptx.o.inputs);
    for (file_index, input) in inputs.iter().enumerate() {
        let bytes = swallow_file_in_memory(input.as_ref())?;
        let word = ptx.word_regex.map(|re| re.search(&bytes));
        ptx.texts.push(Text {
            bytes,
            name: input.as_ref().map(|n| os_bytes(n).into_owned()),
            word,
        });
        ptx.find_occurs_in_text(file_index)?;
        ptx.total_line_count = ptx.total_line_count.saturating_add(1);
        ptx.file_line_count.push(ptx.total_line_count);
    }

    ptx.sort_found_occurs();
    ptx.fix_output_parameters();
    ptx.generate_all_output(sink)
}

/// Where the index goes: standard output, or `-G`'s OUTPUT file in its place.
enum Sink {
    Stdout(Stream),
    File(io::BufWriter<std::fs::File>),
}

fn main() -> ExitCode {
    stdfd::close_stderr(real_main(), 1)
}

fn real_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let options = match parse_args(&args) {
        Ok(Request::Run(o)) => *o,
        Ok(request) => {
            let mut out = Stream::stdout();
            let text: &[u8] = match request {
                Request::Help => help_text().as_bytes(),
                _ => b"ptx (SlateOS coreutils) 0.1.0\n",
            };
            let _ = out.write_all(text);
            return stdfd::close_stdout("ptx", out, ExitCode::SUCCESS);
        }
        Err(e) => {
            PTX.report(&e);
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    // `freopen (OUTPUT, "w", stdout)`, which upstream does before it reads
    // anything -- and before it refuses a third operand.
    let mut sink = match &options.output {
        None => Sink::Stdout(Stream::stdout()),
        Some(name) => match std::fs::File::create(name) {
            Ok(f) => Sink::File(io::BufWriter::new(f)),
            Err(why) => {
                diag!("ptx: {}: {}", quotef(&os_bytes(name)), strerror(&why));
                return ExitCode::FAILURE;
            }
        },
    };
    if let Some(extra) = &options.extra {
        PTX.report(&PTX.usage_referring(format!("extra operand {}", quote(&os_bytes(extra)))));
        return ExitCode::FAILURE;
    }

    let result = match &mut sink {
        Sink::Stdout(out) => run(options, out),
        Sink::File(out) => run(options, out),
    };
    let status = match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(Fatal(message)) => {
            diag!("ptx: {message}");
            ExitCode::FAILURE
        }
    };
    match sink {
        Sink::Stdout(out) => stdfd::close_stdout("ptx", out, status),
        Sink::File(mut out) => match out.flush() {
            Ok(()) => status,
            Err(why) => {
                diag!("ptx: write error: {}", strerror(&why));
                ExitCode::FAILURE
            }
        },
    }
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
    use scratchdir::ScratchDir;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn options(words: &[&str]) -> Options {
        match parse_args(&args(words)).unwrap() {
            Request::Run(o) => *o,
            other => panic!("{other:?}"),
        }
    }

    /// Run the whole program over `input` as the one file `in`.
    fn index(words: &[&str], input: &str) -> String {
        let mut o = options(words);
        let scratch = ScratchDir::new("ptx-index");
        let path = scratch.path("in");
        std::fs::write(&path, input).unwrap();
        o.inputs = vec![Some(path.into_os_string())];
        let mut out = Vec::new();
        run(o, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn escapes_are_resolved_and_a_nul_ends_the_string() {
        assert_eq!(unescape_string(br"a\tb\n"), b"a\tb\n");
        // Three hex digits at most, the value then cut to a byte: 0x4a4 is 0xa4.
        assert_eq!(unescape_string(br"\x41\x4a4\xg"), b"A\xa4\\xg");
        assert_eq!(unescape_string(br"\0101\01"), b"A\x01");
        assert_eq!(unescape_string(br"a\0b"), b"a");
        assert_eq!(unescape_string(br"ab\cdef"), b"ab");
        assert_eq!(unescape_string(br"\(\|\w"), br"\(\|\w");
        assert_eq!(unescape_string(b"end\\"), b"end");
    }

    #[test]
    fn operands_under_gnu_extensions_and_without() {
        assert_eq!(options(&[]).inputs, vec![None]);
        assert_eq!(options(&["-", ""]).inputs, vec![None, None]);
        let g = options(&["-G", "in", "out"]);
        assert_eq!(g.inputs, vec![Some(OsString::from("in"))]);
        assert_eq!(g.output, Some(OsString::from("out")));
        assert_eq!(g.output_format, Format::Roff);
        assert_eq!(
            options(&["-G", "a", "b", "c"]).extra,
            Some(OsString::from("c"))
        );
    }

    #[test]
    fn numbers_are_read_in_any_base() {
        assert_eq!(options(&["-w", "0x10"]).line_width, 16);
        assert_eq!(options(&["-g", "010"]).gap_size, 8);
        let e = parse_args(&args(&["-w", "0"])).unwrap_err();
        assert!(e.message().starts_with("invalid line width"));
        let e = parse_args(&args(&["-g", "x"])).unwrap_err();
        assert!(e.message().starts_with("invalid gap width"));
    }

    #[test]
    fn the_format_is_an_argmatch() {
        assert_eq!(options(&["--format=t"]).output_format, Format::Tex);
        assert_eq!(options(&["--format", "roff"]).output_format, Format::Roff);
        assert!(parse_args(&args(&["--format=x"])).is_err());
    }

    #[test]
    fn every_word_on_a_line_of_its_own_in_order() {
        let out = index(&["-w", "30"], "the quick fox\n");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        // Sorted by keyword: fox, quick, the.
        assert!(lines[0].contains("fox"));
        assert!(lines[1].contains("quick fox"));
        assert!(lines[2].contains("the quick fox"));
    }

    #[test]
    fn an_ignore_list_and_an_only_list() {
        let scratch = ScratchDir::new("ptx-lists");
        let ignore = scratch.path("ignore");
        std::fs::write(&ignore, "the\n\nfox\n").unwrap();
        let out = index(&["-i", ignore.to_str().unwrap()], "the quick fox\n");
        assert_eq!(out.lines().count(), 1);
        assert!(out.contains("quick"));
        let only = scratch.path("only");
        std::fs::write(&only, "fox\n").unwrap();
        let out = index(&["-o", only.to_str().unwrap()], "the quick fox\n");
        assert_eq!(out.lines().count(), 1);
        assert!(out.contains("fox"));
    }

    /// A keyword pattern that can match nothing ends: upstream loops for ever
    /// on it (see the module docs), and the window search had a loop of its
    /// own past the end of a context.
    #[test]
    fn a_keyword_pattern_that_can_match_nothing_terminates() {
        let out = index(&["-W", "[a-z]*"], "ab, cd\nef\n");
        assert_eq!(out.lines().count(), 3);
        for word in ["ab", "cd", "ef"] {
            assert!(out.contains(word), "{word} missing from {out:?}");
        }
    }

    #[test]
    fn roff_output_quotes_its_fields() {
        let out = index(&["-O"], "say \"hi\"\n");
        assert!(out.starts_with(".xx \""));
        assert!(out.contains("\"\"hi\"\""));
    }

    #[test]
    fn a_bad_pattern_is_reported_as_glibc_words_it() {
        let o = options(&["-W", r"\("]);
        let Err(Fatal(message)) = run(o, &mut Vec::new()) else {
            panic!("the pattern compiled");
        };
        assert_eq!(
            message,
            format!(r"Unmatched ( or \( (for regexp {})", quote(br"\("))
        );
    }
}
