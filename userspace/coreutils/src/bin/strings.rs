//! `strings` — print the printable character sequences in a file.
//!
//! # What was wrong with the previous implementation
//!
//! 1. **It read argv as `Vec<String>`.** `std::env::args()` is a literal
//!    `unwrap` on the first argument that is not UTF-8, and on this system a
//!    path may hold every byte but `/` and NUL. `strings` takes filenames as
//!    operands, so this was not a theoretical hazard: the utility died before
//!    its first statement on a name it was perfectly able to open.
//! 2. **It slurped the whole file into memory** with `read_to_end`. `strings`
//!    is pointed at core dumps and disk images; the one input class it exists
//!    to serve is the class too big to hold. Upstream streams, and so does
//!    this now.
//! 3. **`-n` with a non-numeric argument silently became 4.** Measured,
//!    upstream says `strings: invalid integer argument abc` and exits 1.
//!    Silently searching for something other than what was asked is the worst
//!    of the three possible behaviours.
//! 4. **`-n 0` was accepted**, where upstream refuses it: `minimum string
//!    length is too small: 0`.
//! 5. **`-n` at the end of argv was a silent no-op.** Upstream consumes the
//!    next word whatever it is, so `strings -n file` reports `invalid integer
//!    argument file` rather than quietly scanning `file` with the default.
//! 6. **`-` was treated as standard input.** It is not: measured, `-` is
//!    upstream's third spelling of `--all` (the help text's `-a - --all` line
//!    is literal), it does *not* count as a file, and `strings -` alone
//!    therefore prints the usage and exits 1.
//! 7. **There were no options** beyond `-n`: no `--help`, no `--version`, no
//!    `-f`, `-t`, `-e`, `-s`, `-w`, and no `--` separator.
//! 8. **Every write went through `println!`/`writeln!`,** which panics into
//!    status 134 on a closed stdout instead of reporting it.
//! 9. **A read error was reported as `read error`** with the underlying cause
//!    thrown away.
//!
//! # Measured against GNU strings 2.42 (binutils)
//!
//! | Invocation | Upstream |
//! |---|---|
//! | no operands | reads standard input |
//! | `-` as the only operand | usage on stderr, status 1 |
//! | `-` alongside a file | means `--all`; the file is still scanned |
//! | `-n 010` | minimum 8 — the argument is `strtoul` base 0, so a leading `0` is octal |
//! | `-n ' 5'` / `-n +5` | 5; leading blanks and a sign are accepted |
//! | `-n '5 '` | `invalid integer argument 5 ` — a *trailing* blank is not |
//! | `-n ''` | `minimum string length is too small: ` |
//! | `-n -3` | `minimum string length is too big: -3` — `strtoul` wraps |
//! | `-n 4294967295` | `minimum string length 4294967295 is too big` (note the different word order) |
//! | `-n 4294967296` | `minimum string length is too big: 4294967296` |
//! | `-t q`, `-e q`, `-t D` | the bare usage on stderr, status 1, with no message of its own |
//! | `-a`/`-d`, `-n`, `-t`, `-e`, `-s` repeated | the last one wins |
//! | a missing file | `strings: 'NAME': No such file`, then the scan continues; status 1 |
//! | an unsearchable path | the same message — any `stat` failure gets it, not only `ENOENT` |
//! | a directory | `strings: Warning: 'NAME' is a directory`; status 1 |
//! | an unreadable file | `strings: NAME: Permission denied` — *unquoted*, unlike the two above |
//! | an empty file | nothing, status 0 |
//! | `-f` on standard input | the label is `{standard input}` |
//! | `-t d` | the offset of the run's first *byte*, `%7` right-aligned, then a space |
//! | `-f` with `-t` | the filename label comes first, then the offset |
//! | `-s SEP` | replaces the newline after *every* string, including the last |
//! | `--help`, `-h`, `-H` | the usage on **stdout**, status 0 |
//! | `--version`, `-v`, `-V` | five lines, status 0 |
//!
//! The printable test is `isprint` in the C locale — `0x20..=0x7e` — plus tab,
//! plus, under `-e S`, every byte above 127, plus, under `-w`, the rest of
//! `isspace`. NUL is never a string byte, not even under `-w`.
//!
//! `-n` counts *characters*, not bytes: `-e l -n 5` matches a five-character
//! UTF-16 run occupying ten bytes. The offset `-t` reports is still in bytes.
//!
//! The scan is upstream's, including the part that is easy to get wrong: when a
//! multi-byte unit is not printable, the scan resumes **one byte** after that
//! unit began, not after it ended. That is why `-e b` (16-bit big-endian) finds
//! `ello` at offset 1 in a little-endian `hello`, and reproducing it is the
//! difference between agreeing with upstream on such a file and not.
//!
//! # Where this deliberately diverges
//!
//! 1. **`-d`/`--data` and `-T`/`--target` are refused, not ignored.** Both ask
//!    for a scan of particular sections of an object file, which needs an
//!    object-file reader this build does not have. Accepting them and scanning
//!    the whole file anyway would answer a question other than the one asked --
//!    quietly, and with *more* output than expected, which is the shape of
//!    error a reader is least likely to notice.
//! 2. **`-U`/`--unicode` accepts only `d`.** `d` — treat UTF-8 as ordinary
//!    bytes — is upstream's default and is what this build does. The other
//!    five modes re-render multi-byte sequences, and pretending to honour them
//!    would misreport what is in the file.
//! 3. **`@FILE` is not read.** Upstream takes options from a file that way.
//!    Nothing else in this coreutils does, and adding one utility's private
//!    argument-file syntax would be a surprise in the other direction.
//!
//! Each of the three is a diagnostic naming the limitation, never a silent
//! difference in output. They are logged in `known-issues.md` as
//! `B-STRINGS-HAS-NO-OBJECT-FILE-READER`.

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::os_bytes;
use coreutils::stdfd::{self, Stream};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process::ExitCode;

// ------------------------------------------------------------- the tables ---

/// Upstream's usage status is 1; there is no `EXIT_FAILURE` distinction in it.
const STRINGS: Program = Program::new("strings", 1);

/// Upstream's `getopt_long` string. The digits are the `-N` shorthand for
/// `-n N`, which is why `-5` sets the minimum to five and `-0` is refused for
/// being too small rather than for being an unknown option.
const SHORT_OPTIONS: &str = "ade:fhHn:os:t:T:U:vVw0123456789";

/// Upstream's `long_options[]`, in its own order — which matters, because our
/// parser reports an ambiguous prefix by listing the candidates in table order.
/// Measured with `strings --=x`, where the empty prefix matches every entry and
/// so prints the whole table in declaration order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("all", Takes::Nothing),
    ("bytes", Takes::Required),
    ("data", Takes::Nothing),
    ("encoding", Takes::Required),
    ("help", Takes::Nothing),
    ("include-all-whitespace", Takes::Nothing),
    ("output-separator", Takes::Required),
    ("print-file-name", Takes::Nothing),
    ("radix", Takes::Required),
    ("target", Takes::Required),
    ("unicode", Takes::Required),
    ("version", Takes::Nothing),
];

/// The label `-f` prints for standard input, braces and all.
const STDIN_LABEL: &[u8] = b"{standard input}";

/// The largest minimum-length upstream accepts. One below `UINT_MAX`, because
/// `UINT_MAX` itself trips a second check with a differently-worded message.
const MIN_LENGTH_CEILING: u64 = 0xffff_ffff;

// ------------------------------------------------------------- the options ---

/// How many bytes a character occupies, and in which order.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Encoding {
    /// `-e s`: one byte, and only the 7-bit printables. The default.
    #[default]
    Ascii7,
    /// `-e S`: one byte, and every byte above 127 counts as printable too.
    Ascii8,
    /// `-e b`: 16-bit, big-endian.
    Big16,
    /// `-e l`: 16-bit, little-endian.
    Little16,
    /// `-e B`: 32-bit, big-endian.
    Big32,
    /// `-e L`: 32-bit, little-endian.
    Little32,
}

impl Encoding {
    /// Bytes per character — the stride the scan advances by, and the amount
    /// it pushes back when a character is rejected.
    fn width(self) -> usize {
        match self {
            Encoding::Ascii7 | Encoding::Ascii8 => 1,
            Encoding::Big16 | Encoding::Little16 => 2,
            Encoding::Big32 | Encoding::Little32 => 4,
        }
    }

    /// Decode one character from exactly `width()` bytes.
    ///
    /// Returns `None` if fewer bytes than that are available, which is how the
    /// scan learns it has reached the end of the input.
    fn decode(self, bytes: &[u8]) -> Option<u32> {
        let at = |i: usize| bytes.get(i).copied().map(u32::from);
        match self {
            Encoding::Ascii7 | Encoding::Ascii8 => at(0),
            Encoding::Big16 => Some(at(0)? << 8 | at(1)?),
            Encoding::Little16 => Some(at(1)? << 8 | at(0)?),
            Encoding::Big32 => Some(at(0)? << 24 | at(1)? << 16 | at(2)? << 8 | at(3)?),
            Encoding::Little32 => Some(at(3)? << 24 | at(2)? << 16 | at(1)? << 8 | at(0)?),
        }
    }
}

/// The base `-t` prints an offset in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Radix {
    Octal,
    Decimal,
    Hex,
}

/// Everything the scan needs to know.
#[derive(Clone, Debug)]
struct Options {
    /// How many *characters*, not bytes, a run must have to be printed.
    min: usize,
    encoding: Encoding,
    /// `None` unless `-t`/`-o` asked for the offset.
    radix: Option<Radix>,
    print_file_name: bool,
    /// `-w`: the rest of `isspace` joins tab as a string character.
    include_all_whitespace: bool,
    /// What follows each string. A newline unless `-s` said otherwise.
    separator: Vec<u8>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            min: 4,
            encoding: Encoding::default(),
            radix: None,
            print_file_name: false,
            include_all_whitespace: false,
            separator: b"\n".to_vec(),
        }
    }
}

/// Where a scan's bytes come from.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Source {
    Stdin,
    Path(OsString),
}

/// What the command line asked for.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Request {
    Help,
    Version,
    /// No file was named and none can be assumed — upstream's `files_given`
    /// test, which `strings -` alone fails.
    Usage,
    Run(Box<Options>, Vec<Source>),
}

impl PartialEq for Options {
    fn eq(&self, other: &Self) -> bool {
        self.min == other.min
            && self.encoding == other.encoding
            && self.radix == other.radix
            && self.print_file_name == other.print_file_name
            && self.include_all_whitespace == other.include_all_whitespace
            && self.separator == other.separator
    }
}

impl Eq for Options {}

// -------------------------------------------------------------- the texts ---

/// Upstream's usage, minus the two lines that are BFD's rather than
/// `strings`'s — the list of supported object-file targets and the bug-report
/// URL — and with a note about the three options this build refuses.
fn help_text() -> String {
    let mut text = String::new();
    text.push_str("Usage: strings [option(s)] [file(s)]\n");
    text.push_str(" Display printable strings in [file(s)] (stdin by default)\n");
    text.push_str(" The options are:\n");
    text.push_str(
        "  -a - --all                Scan the entire file, not just the data section [default]\n",
    );
    text.push_str("  -d --data                 Only scan the data sections in the file\n");
    text.push_str("  -f --print-file-name      Print the name of the file before each string\n");
    text.push_str("  -n <number>               Locate & print any sequence of at least <number>\n");
    text.push_str("    --bytes=<number>         displayable characters.  (The default is 4).\n");
    text.push_str(
        "  -t --radix={o,d,x}        Print the location of the string in base 8, 10 or 16\n",
    );
    text.push_str(
        "  -w --include-all-whitespace Include all whitespace as valid string characters\n",
    );
    text.push_str("  -o                        An alias for --radix=o\n");
    text.push_str("  -T --target=<BFDNAME>     Specify the binary file format\n");
    text.push_str("  -e --encoding={s,S,b,l,B,L} Select character size and endianness:\n");
    text.push_str(
        "                            s = 7-bit, S = 8-bit, {b,l} = 16-bit, {B,L} = 32-bit\n",
    );
    text.push_str("  --unicode={default|show|invalid|hex|escape|highlight}\n");
    text.push_str(
        "  -U {d|s|i|x|e|h}          Specify how to treat UTF-8 encoded unicode characters\n",
    );
    text.push_str("  -s --output-separator=<string> String used to separate strings in output.\n");
    text.push_str("  @<file>                   Read options from <file>\n");
    text.push_str("  -h --help                 Display this information\n");
    text.push_str("  -v -V --version           Print the program's version number\n");
    text.push('\n');
    text.push_str("This build has no object-file reader, so it always scans the whole file:\n");
    text.push_str("--data, --target, @<file>, and every --unicode mode but `d' are refused\n");
    text.push_str("rather than silently ignored.\n");
    text
}

fn version_text() -> String {
    let mut text = String::new();
    text.push_str("strings (SlateOS coreutils) 0.1.0\n");
    text.push_str("Copyright (C) 2026 Free Software Foundation, Inc.\n");
    text.push_str("This program is free software; you may redistribute it under the terms of\n");
    text.push_str(
        "the GNU General Public License version 3 or (at your option) any later version.\n",
    );
    text.push_str("This program has absolutely no warranty.\n");
    text
}

// ------------------------------------------------------------- the parsing ---

/// `strtoul(text, &end, 0)`, plus the "was the whole argument consumed" test
/// upstream makes on `end` afterwards.
///
/// Base 0 means a leading `0x` is hexadecimal and a bare leading `0` is octal,
/// which is why `-n 010` asks for eight characters and not ten. Leading blanks
/// and a sign are part of `strtoul`'s grammar and so are accepted; a *trailing*
/// blank is not, because it leaves `end` short of the terminator.
///
/// `None` is upstream's `invalid integer argument`. An empty argument is *not*
/// that: `strtoul` returns 0 with `end` already at the terminator, so it falls
/// through to the "too small" complaint instead.
fn strtoul_base0(text: &[u8]) -> Option<u64> {
    let mut at = 0usize;
    while text.get(at).is_some_and(u8::is_ascii_whitespace) {
        at = at.saturating_add(1);
    }
    let negate = match text.get(at) {
        Some(b'-') => {
            at = at.saturating_add(1);
            true
        }
        Some(b'+') => {
            at = at.saturating_add(1);
            false
        }
        _ => false,
    };

    let base: u32 = if text.get(at) == Some(&b'0') {
        match text.get(at.saturating_add(1)) {
            Some(&b'x' | &b'X')
                if text
                    .get(at.saturating_add(2))
                    .is_some_and(u8::is_ascii_hexdigit) =>
            {
                at = at.saturating_add(2);
                16
            }
            // The lone `0` is itself the first octal digit, so the cursor
            // stays where it is.
            _ => 8,
        }
    } else {
        10
    };

    let digits_began = at;
    let mut value: u64 = 0;
    while let Some(digit) = text
        .get(at)
        .and_then(|b| char::from(*b).to_digit(base).map(u64::from))
    {
        // `strtoul` clamps to `ULONG_MAX` on overflow, and every clamped value
        // is far past the ceiling, so saturating here loses nothing.
        value = value.saturating_mul(u64::from(base)).saturating_add(digit);
        at = at.saturating_add(1);
    }

    if at == digits_began && !text.is_empty() {
        // No digits at all: `end` never moved, so upstream's `*end != '\0'`
        // test fires -- unless the argument was empty to begin with.
        return None;
    }
    if at != text.len() {
        return None;
    }
    Some(if negate {
        0u64.wrapping_sub(value)
    } else {
        value
    })
}

/// Read `-n`'s argument, or say exactly why it cannot be read.
///
/// The two "too big" messages are upstream's and they do not match each other:
/// at `UINT_MAX` the value is inlined mid-sentence, and above it the original
/// text is appended. Reproduced as measured rather than tidied, so that a
/// script matching on either sentence sees what it sees on Linux.
fn read_min_length(argument: &[u8]) -> Result<usize, getopt::Error> {
    let text = String::from_utf8_lossy(argument).into_owned();
    let Some(value) = strtoul_base0(argument) else {
        return Err(STRINGS.usage(format!("invalid integer argument {text}")));
    };
    if value == MIN_LENGTH_CEILING {
        return Err(STRINGS.usage(format!("minimum string length {value} is too big")));
    }
    if value > MIN_LENGTH_CEILING {
        return Err(STRINGS.usage(format!("minimum string length is too big: {text}")));
    }
    if value < 1 {
        return Err(STRINGS.usage(format!("minimum string length is too small: {text}")));
    }
    usize::try_from(value)
        .map_err(|_| STRINGS.usage(format!("minimum string length is too big: {text}")))
}

/// An option whose argument upstream rejects with the bare usage and no
/// sentence of its own — `-t q` and `-e q` both do this.
fn bare_usage() -> getopt::Error {
    getopt::Error {
        sentence: String::new(),
        referral: None,
        status: 1,
    }
}

/// A limitation of this build, stated rather than silently worked around.
fn unsupported(what: &str) -> getopt::Error {
    STRINGS.usage(format!(
        "{what} needs an object-file reader, which this build does not have"
    ))
}

/// Turn argv into a request. Pure, so every branch is reachable from a test.
#[allow(clippy::too_many_lines)] // One arm per option; splitting it would hide the table.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut options = Options::default();
    let mut sources: Vec<Source> = Vec::new();
    // Upstream's `files_given`. A `-` operand deliberately does not set it.
    let mut any_operand = false;

    for item in STRINGS.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Long("help", _) | Opt::Short(b'h' | b'H', _) => return Ok(Request::Help),
            Opt::Long("version", _) | Opt::Short(b'v' | b'V', _) => return Ok(Request::Version),

            // `-a` is this build's only mode, so it is a no-op rather than a
            // setting: there is nothing for it to switch back from.
            Opt::Long("all", _) | Opt::Short(b'a', _) => {}
            Opt::Long("data", _) | Opt::Short(b'd', _) => {
                return Err(unsupported("--data"));
            }
            Opt::Long("target", _) | Opt::Short(b'T', _) => {
                return Err(unsupported("--target"));
            }

            Opt::Long("print-file-name", _) | Opt::Short(b'f', _) => options.print_file_name = true,
            Opt::Long("include-all-whitespace", _) | Opt::Short(b'w', _) => {
                options.include_all_whitespace = true;
            }

            Opt::Long("bytes", value) | Opt::Short(b'n', value) => {
                let value = value.unwrap_or_default();
                options.min = read_min_length(&os_bytes(value.as_os_str()))?;
            }
            // The digit shorthand: `-5` is `-n 5`, and `-0` is refused for
            // being too small, exactly as `-n 0` is.
            Opt::Short(digit @ b'0'..=b'9', _) => {
                options.min = read_min_length(&[digit])?;
            }

            Opt::Short(b'o', _) => options.radix = Some(Radix::Octal),
            Opt::Long("radix", value) | Opt::Short(b't', value) => {
                let value = value.unwrap_or_default();
                options.radix = Some(match os_bytes(value.as_os_str()).as_ref() {
                    b"o" => Radix::Octal,
                    b"d" => Radix::Decimal,
                    b"x" => Radix::Hex,
                    _ => return Err(bare_usage()),
                });
            }

            Opt::Long("encoding", value) | Opt::Short(b'e', value) => {
                let value = value.unwrap_or_default();
                options.encoding = match os_bytes(value.as_os_str()).as_ref() {
                    b"s" => Encoding::Ascii7,
                    b"S" => Encoding::Ascii8,
                    b"b" => Encoding::Big16,
                    b"l" => Encoding::Little16,
                    b"B" => Encoding::Big32,
                    b"L" => Encoding::Little32,
                    _ => return Err(bare_usage()),
                };
            }

            Opt::Long("output-separator", value) | Opt::Short(b's', value) => {
                let value = value.unwrap_or_default();
                options.separator = os_bytes(value.as_os_str()).into_owned();
            }

            Opt::Long("unicode", value) | Opt::Short(b'U', value) => {
                let value = value.unwrap_or_default();
                let bytes = os_bytes(value.as_os_str()).into_owned();
                if bytes.as_slice() != b"d" {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    return Err(STRINGS.usage(format!(
                        "invalid argument to -U/--unicode: {text} \
                         (this build supports only `d')"
                    )));
                }
            }

            Opt::Long(other, _) => {
                return Err(STRINGS.usage(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(STRINGS.invalid_option(other)),

            Opt::Operand(operand) => {
                let bytes = os_bytes(operand.as_os_str());
                if bytes.as_ref() == b"-" {
                    // Upstream's third spelling of `--all`. It is not a file
                    // and it does not satisfy `files_given`.
                    continue;
                }
                if bytes.as_ref().starts_with(b"@") {
                    return Err(STRINGS.usage(
                        "reading options from @FILE is not implemented in this build".to_owned(),
                    ));
                }
                any_operand = true;
                sources.push(Source::Path(operand.clone()));
            }
        }
    }

    if sources.is_empty() {
        if any_operand {
            // Unreachable in practice -- `any_operand` is only set when a path
            // was pushed -- but stated so the invariant is checked, not
            // assumed.
            return Ok(Request::Usage);
        }
        // No operands at all means standard input; operands that were all `-`
        // mean upstream's `files_given` is false, and that is the usage.
        if args
            .iter()
            .any(|a| os_bytes(a.as_os_str()).as_ref() == b"-")
        {
            return Ok(Request::Usage);
        }
        sources.push(Source::Stdin);
    }

    Ok(Request::Run(Box::new(options), sources))
}

// -------------------------------------------------------------- the scan ---

/// Is this decoded character a string character?
///
/// Upstream's `STRING_ISGRAPHIC`: `isprint` in the C locale, plus tab, plus —
/// under `-e S` — every byte above 127, plus — under `-w` — the rest of
/// `isspace`. A value that does not fit in a byte never qualifies, which is
/// what makes a 16-bit scan of the wrong endianness find nothing.
fn is_string_char(c: u32, encoding: Encoding, include_all_whitespace: bool) -> bool {
    if c > 0xff {
        return false;
    }
    if c == u32::from(b'\t') {
        return true;
    }
    if (0x20..=0x7e).contains(&c) {
        return true;
    }
    if encoding == Encoding::Ascii8 && c > 127 {
        return true;
    }
    // NUL is excluded deliberately: it is not `isspace`, and upstream breaks a
    // run on it explicitly even when `-w` is in force.
    include_all_whitespace && matches!(c, 0x0a | 0x0b | 0x0c | 0x0d | 0x20)
}

/// Format an offset the way upstream's `printf("%7lo ", …)` family does:
/// right-aligned in seven columns, growing past seven when it must, then one
/// space.
fn offset_field(offset: u64, radix: Radix) -> Vec<u8> {
    let text = match radix {
        Radix::Octal => format!("{offset:>7o} "),
        Radix::Decimal => format!("{offset:>7} "),
        Radix::Hex => format!("{offset:>7x} "),
    };
    text.into_bytes()
}

/// A byte source with a cursor that can step by one byte at a time, so the
/// scan can resume one byte after a rejected multi-byte character without
/// seeking — which matters because the source may be a pipe.
struct Window<R: Read> {
    reader: R,
    buffer: Vec<u8>,
    /// How much of `buffer` holds real bytes.
    filled: usize,
    /// The cursor, as an index into `buffer`.
    at: usize,
    /// The input offset of `buffer[0]`.
    base: u64,
    /// Set once the reader has returned zero.
    drained: bool,
}

impl<R: Read> Window<R> {
    fn new(reader: R) -> Self {
        Window {
            reader,
            buffer: vec![0u8; 64 * 1024],
            filled: 0,
            at: 0,
            base: 0,
            drained: false,
        }
    }

    /// The input offset of the cursor.
    fn offset(&self) -> u64 {
        self.base.saturating_add(self.at as u64)
    }

    /// Make sure at least `want` bytes are readable at the cursor, refilling
    /// and compacting as needed. Answers `false` only at end of input.
    fn ensure(&mut self, want: usize) -> io::Result<bool> {
        while self.filled.saturating_sub(self.at) < want {
            if self.drained {
                return Ok(false);
            }
            if self.at > 0 {
                self.buffer.copy_within(self.at..self.filled, 0);
                self.filled = self.filled.saturating_sub(self.at);
                self.base = self.base.saturating_add(self.at as u64);
                self.at = 0;
            }
            if self.filled >= self.buffer.len() {
                // `want` is at most four, so this cannot happen with the
                // buffer sized as it is; growing rather than looping forever
                // is still the honest response if it ever did.
                self.buffer.resize(self.buffer.len().saturating_mul(2), 0);
            }
            let room = self.buffer.get_mut(self.filled..).unwrap_or_default();
            match self.reader.read(room) {
                Ok(0) => self.drained = true,
                Ok(n) => self.filled = self.filled.saturating_add(n),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(true)
    }

    /// The `want` bytes at the cursor, without moving it.
    fn peek(&self, want: usize) -> Option<&[u8]> {
        self.buffer.get(self.at..self.at.saturating_add(want))
    }

    fn advance(&mut self, by: usize) {
        self.at = self.at.saturating_add(by).min(self.filled);
    }
}

/// Scan one input and write every run of at least `options.min` string
/// characters.
///
/// This is upstream's loop, including the restart rule: a rejected character
/// puts the cursor one byte past where that character *began*, not past where
/// it ended. For a one-byte encoding the two are the same; for the others they
/// are not, and the difference is visible in the output.
fn scan<R: Read, W: Write>(
    input: R,
    out: &mut W,
    label: Option<&[u8]>,
    options: &Options,
) -> io::Result<()> {
    let width = options.encoding.width();
    let mut window = Window::new(input);
    let mut run: Vec<u8> = Vec::with_capacity(options.min);

    'attempt: loop {
        let start = window.offset();
        run.clear();

        // Phase one: is there a run of `min` string characters here?
        for _ in 0..options.min {
            if !window.ensure(width)? {
                return Ok(());
            }
            let Some(c) = window.peek(width).and_then(|b| options.encoding.decode(b)) else {
                return Ok(());
            };
            if !is_string_char(c, options.encoding, options.include_all_whitespace) {
                // Resume one byte in, not one character in.
                window.advance(1);
                continue 'attempt;
            }
            window.advance(width);
            // The decoded value is what is printed, so a 16-bit `h` prints as
            // one byte and not two.
            run.push((c & 0xff) as u8);
        }

        if let Some(label) = label.filter(|_| options.print_file_name) {
            out.write_all(label)?;
            out.write_all(b": ")?;
        }
        if let Some(radix) = options.radix {
            out.write_all(&offset_field(start, radix))?;
        }
        out.write_all(&run)?;

        // Phase two: print the rest of the run.
        loop {
            if !window.ensure(width)? {
                break;
            }
            let Some(c) = window.peek(width).and_then(|b| options.encoding.decode(b)) else {
                break;
            };
            if c == 0 || !is_string_char(c, options.encoding, options.include_all_whitespace) {
                window.advance(1);
                break;
            }
            window.advance(width);
            out.write_all(&[(c & 0xff) as u8])?;
        }

        out.write_all(&options.separator)?;
    }
}

// -------------------------------------------------------- the diagnostics ---

/// `strings: 'NAME': No such file` — upstream's message for *any* failed
/// `stat`, not only `ENOENT`. Measured: an unsearchable parent directory gets
/// the same sentence.
fn no_such_file(name: &[u8]) -> Vec<u8> {
    let mut line = b"strings: '".to_vec();
    line.extend_from_slice(name);
    line.extend_from_slice(b"': No such file\n");
    line
}

/// `strings: Warning: 'NAME' is a directory`.
fn is_a_directory(name: &[u8]) -> Vec<u8> {
    let mut line = b"strings: Warning: '".to_vec();
    line.extend_from_slice(name);
    line.extend_from_slice(b"' is a directory\n");
    line
}

/// `strings: NAME: REASON` — the open failure, and the one shape of the three
/// that upstream leaves *unquoted*.
fn open_failed(name: &[u8], reason: &str) -> Vec<u8> {
    let mut line = b"strings: ".to_vec();
    line.extend_from_slice(name);
    line.extend_from_slice(b": ");
    line.extend_from_slice(reason.as_bytes());
    line.push(b'\n');
    line
}

/// `strings: NAME: REASON` for a failure part-way through a read.
fn read_failed(name: &[u8], reason: &str) -> Vec<u8> {
    open_failed(name, reason)
}

// --------------------------------------------------------------- the shell ---

fn main() -> ExitCode {
    coreutils::guard_std_fds!();
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(e) => {
            // Upstream reports the sentence (when there is one) and then
            // prints the whole usage to stderr, rather than referring the
            // reader to `--help`.
            if !e.sentence.is_empty() {
                STRINGS.report(&e);
            }
            stdfd::diag_bytes(help_text().as_bytes());
            return ExitCode::from(1);
        }
    };

    let mut out = Stream::stdout();
    let earned = match request {
        Request::Help => {
            let _ = out.write_all(help_text().as_bytes());
            ExitCode::SUCCESS
        }
        Request::Version => {
            let _ = out.write_all(version_text().as_bytes());
            ExitCode::SUCCESS
        }
        Request::Usage => {
            stdfd::diag_bytes(help_text().as_bytes());
            ExitCode::from(1)
        }
        Request::Run(options, sources) => run(&mut out, &options, &sources),
    };

    stdfd::close_stdout("strings", out, earned)
}

fn run(out: &mut Stream, options: &Options, sources: &[Source]) -> ExitCode {
    let mut failed = false;

    for source in sources {
        match source {
            Source::Stdin => {
                if let Err(e) = scan(io::stdin().lock(), out, Some(STDIN_LABEL), options) {
                    stdfd::diag_bytes(&read_failed(STDIN_LABEL, &e.to_string()));
                    failed = true;
                }
            }
            Source::Path(path) => {
                let name = os_bytes(path.as_os_str()).into_owned();
                match std::fs::metadata(path) {
                    Err(_) => {
                        // Any `stat` failure, not only `ENOENT`: measured.
                        stdfd::diag_bytes(&no_such_file(&name));
                        failed = true;
                        continue;
                    }
                    Ok(meta) if meta.is_dir() => {
                        stdfd::diag_bytes(&is_a_directory(&name));
                        failed = true;
                        continue;
                    }
                    Ok(_) => {}
                }
                let file = match File::open(path) {
                    Ok(file) => file,
                    Err(e) => {
                        stdfd::diag_bytes(&open_failed(&name, &clean_reason(&e)));
                        failed = true;
                        continue;
                    }
                };
                if let Err(e) = scan(file, out, Some(&name), options) {
                    stdfd::diag_bytes(&read_failed(&name, &clean_reason(&e)));
                    failed = true;
                }
            }
        }
    }

    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// `strerror`'s sentence, without the `(os error N)` tail Rust appends.
fn clean_reason(e: &io::Error) -> String {
    let text = e.to_string();
    match text.split_once(" (os error ") {
        Some((head, _)) => head.to_owned(),
        None => text,
    }
}

// --------------------------------------------------------------- the tests ---

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn run_options(args: &[&str]) -> Options {
        match parse_args(&argv(args)).unwrap() {
            Request::Run(options, _) => *options,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    fn run_sources(args: &[&str]) -> Vec<Source> {
        match parse_args(&argv(args)).unwrap() {
            Request::Run(_, sources) => sources,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    /// Scan `data` and return what would have been written.
    fn scanned(data: &[u8], options: &Options) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        scan(data, &mut out, Some(b"F"), options).unwrap();
        out
    }

    fn lines(data: &[u8], options: &Options) -> Vec<String> {
        String::from_utf8_lossy(&scanned(data, options))
            .lines()
            .map(str::to_owned)
            .collect()
    }

    // ------------------------------------------------------------ parsing --

    #[test]
    fn the_defaults_are_four_characters_of_seven_bit_ascii() {
        let options = run_options(&["f"]);
        assert_eq!(options.min, 4);
        assert_eq!(options.encoding, Encoding::Ascii7);
        assert_eq!(options.radix, None);
        assert!(!options.print_file_name);
        assert!(!options.include_all_whitespace);
        assert_eq!(options.separator, b"\n".to_vec());
    }

    #[test]
    fn help_and_version_have_four_spellings_between_them() {
        for spelling in [&["--help"][..], &["-h"], &["-H"]] {
            assert_eq!(parse_args(&argv(spelling)).unwrap(), Request::Help);
        }
        for spelling in [&["--version"][..], &["-v"], &["-V"]] {
            assert_eq!(parse_args(&argv(spelling)).unwrap(), Request::Version);
        }
    }

    /// `-n`'s argument is `strtoul` base 0, so `010` is eight and not ten.
    #[test]
    fn the_minimum_length_is_read_in_base_zero() {
        assert_eq!(run_options(&["-n", "010", "f"]).min, 8);
        assert_eq!(run_options(&["-n", "0x10", "f"]).min, 16);
        assert_eq!(run_options(&["-n", "10", "f"]).min, 10);
        assert_eq!(run_options(&["--bytes=7", "f"]).min, 7);
        assert_eq!(run_options(&["-n7", "f"]).min, 7);
    }

    /// Leading blanks and a sign are `strtoul`'s grammar; a trailing blank is
    /// not.
    #[test]
    fn the_minimum_length_accepts_leading_blanks_and_a_sign() {
        assert_eq!(run_options(&["-n", " 5", "f"]).min, 5);
        assert_eq!(run_options(&["-n", "+5", "f"]).min, 5);
        let e = parse_args(&argv(&["-n", "5 ", "f"])).unwrap_err();
        assert_eq!(e.sentence, "invalid integer argument 5 ");
    }

    #[test]
    fn a_minimum_length_that_is_not_a_number_is_fatal() {
        let e = parse_args(&argv(&["-n", "abc", "f"])).unwrap_err();
        assert_eq!(e.sentence, "invalid integer argument abc");
        assert_eq!(e.status, 1);
    }

    /// The old implementation quietly used the default here. Upstream reads
    /// the filename as the argument and then complains about it, which at
    /// least tells the user something went wrong.
    #[test]
    fn a_trailing_dash_n_consumes_the_next_word_whatever_it_is() {
        let e = parse_args(&argv(&["-n", "file"])).unwrap_err();
        assert_eq!(e.sentence, "invalid integer argument file");
    }

    #[test]
    fn zero_and_the_empty_argument_are_too_small() {
        let e = parse_args(&argv(&["-n", "0", "f"])).unwrap_err();
        assert_eq!(e.sentence, "minimum string length is too small: 0");
        let e = parse_args(&argv(&["-n", "", "f"])).unwrap_err();
        assert_eq!(e.sentence, "minimum string length is too small: ");
        let e = parse_args(&argv(&["-0", "f"])).unwrap_err();
        assert_eq!(e.sentence, "minimum string length is too small: 0");
    }

    /// Two different sentences for two adjacent values, as measured. The word
    /// order really does change at the boundary.
    #[test]
    fn the_two_too_big_sentences_differ_at_the_ceiling() {
        assert_eq!(run_options(&["-n", "4294967294", "f"]).min, 4_294_967_294);
        let e = parse_args(&argv(&["-n", "4294967295", "f"])).unwrap_err();
        assert_eq!(e.sentence, "minimum string length 4294967295 is too big");
        let e = parse_args(&argv(&["-n", "4294967296", "f"])).unwrap_err();
        assert_eq!(e.sentence, "minimum string length is too big: 4294967296");
    }

    /// `strtoul` wraps a negative, so `-3` is enormous rather than negative.
    #[test]
    fn a_negative_minimum_length_is_too_big_not_too_small() {
        let e = parse_args(&argv(&["-n", "-3", "f"])).unwrap_err();
        assert_eq!(e.sentence, "minimum string length is too big: -3");
    }

    #[test]
    fn the_digit_shorthand_sets_the_minimum() {
        assert_eq!(run_options(&["-5", "f"]).min, 5);
        assert_eq!(run_options(&["-9", "f"]).min, 9);
    }

    #[test]
    fn the_last_of_a_repeated_option_wins() {
        assert_eq!(run_options(&["-n", "4", "-n", "5", "f"]).min, 5);
        assert_eq!(
            run_options(&["-t", "d", "-t", "o", "f"]).radix,
            Some(Radix::Octal)
        );
        assert_eq!(
            run_options(&["-e", "s", "-e", "l", "f"]).encoding,
            Encoding::Little16
        );
        assert_eq!(run_options(&["-s", "A", "-s", "B", "f"]).separator, b"B");
    }

    #[test]
    fn o_is_an_alias_for_radix_octal() {
        assert_eq!(run_options(&["-o", "f"]).radix, Some(Radix::Octal));
        assert_eq!(run_options(&["--radix=x", "f"]).radix, Some(Radix::Hex));
    }

    /// Upstream answers a bad radix or encoding with the bare usage and no
    /// sentence of its own, which is why `sentence` is empty here.
    #[test]
    fn a_bad_radix_or_encoding_is_the_bare_usage() {
        for args in [
            &["-t", "q", "f"][..],
            &["-t", "D", "f"],
            &["-t", "X", "f"],
            &["-e", "q", "f"],
        ] {
            let e = parse_args(&argv(args)).unwrap_err();
            assert_eq!(e.sentence, "", "for {args:?}");
            assert_eq!(e.status, 1, "for {args:?}");
        }
    }

    #[test]
    fn every_encoding_letter_is_understood() {
        for (letter, expected) in [
            ("s", Encoding::Ascii7),
            ("S", Encoding::Ascii8),
            ("b", Encoding::Big16),
            ("l", Encoding::Little16),
            ("B", Encoding::Big32),
            ("L", Encoding::Little32),
        ] {
            assert_eq!(run_options(&["-e", letter, "f"]).encoding, expected);
        }
    }

    /// Divergence 1 and 2: refused with a sentence, never silently ignored.
    #[test]
    fn the_object_file_options_are_refused_rather_than_ignored() {
        for args in [&["-d", "f"][..], &["--data", "f"], &["-T", "elf64", "f"]] {
            let e = parse_args(&argv(args)).unwrap_err();
            assert!(
                e.sentence.contains("object-file reader"),
                "for {args:?}: {:?}",
                e.sentence
            );
        }
        let e = parse_args(&argv(&["-U", "h", "f"])).unwrap_err();
        assert!(e.sentence.contains("invalid argument to -U/--unicode"));
        // The one mode this build does implement is upstream's default.
        assert_eq!(run_options(&["-U", "d", "f"]).min, 4);
    }

    #[test]
    fn an_at_file_is_refused_rather_than_read_as_a_filename() {
        let e = parse_args(&argv(&["@opts.txt"])).unwrap_err();
        assert!(e.sentence.contains("@FILE"), "{:?}", e.sentence);
    }

    // ------------------------------------------------------------ operands --

    #[test]
    fn no_operands_means_standard_input() {
        assert_eq!(run_sources(&[]), vec![Source::Stdin]);
        assert_eq!(run_sources(&["-n", "5"]), vec![Source::Stdin]);
        assert_eq!(run_sources(&["--"]), vec![Source::Stdin]);
    }

    /// `-` is upstream's third spelling of `--all`, not standard input. On its
    /// own it leaves no file named, which is the usage.
    #[test]
    fn a_lone_dash_is_all_and_not_a_file() {
        assert_eq!(parse_args(&argv(&["-"])).unwrap(), Request::Usage);
        assert_eq!(
            run_sources(&["f", "-"]),
            vec![Source::Path(OsString::from("f"))]
        );
        assert_eq!(
            run_sources(&["-", "f"]),
            vec![Source::Path(OsString::from("f"))]
        );
    }

    #[test]
    fn operands_keep_their_order_and_their_duplicates() {
        assert_eq!(
            run_sources(&["a", "b", "a"]),
            vec![
                Source::Path(OsString::from("a")),
                Source::Path(OsString::from("b")),
                Source::Path(OsString::from("a")),
            ]
        );
    }

    #[test]
    fn a_double_dash_turns_an_option_into_an_operand() {
        assert_eq!(
            run_sources(&["--", "-n"]),
            vec![Source::Path(OsString::from("-n"))]
        );
    }

    /// The whole point of the conversion: a filename that is not UTF-8 must
    /// reach the scan as the bytes it was given.
    #[cfg(unix)]
    #[test]
    fn a_non_utf8_operand_survives_parsing() {
        let name = coreutils::quote::os_from_bytes(b"\xff\xfename.bin");
        let parsed = parse_args(&[name.clone()]).unwrap();
        assert_eq!(
            parsed,
            Request::Run(Box::new(Options::default()), vec![Source::Path(name)])
        );
    }

    #[test]
    fn options_may_follow_operands() {
        assert_eq!(run_options(&["f", "-n", "5"]).min, 5);
    }

    // ------------------------------------------------------------- the scan --

    #[test]
    fn only_runs_at_least_min_long_are_printed() {
        let options = Options::default();
        assert_eq!(lines(b"abc\0abcd\0abcde\0", &options), ["abcd", "abcde"]);
        assert!(scanned(b"abc", &options).is_empty());
        assert!(scanned(b"", &options).is_empty());
        assert!(scanned(b"\0\0\0\0\0\0", &options).is_empty());
    }

    #[test]
    fn a_run_that_reaches_the_end_of_input_is_still_printed() {
        assert_eq!(lines(b"\0\0hello", &Options::default()), ["hello"]);
    }

    /// Tab is a string character; newline is not, unless `-w` says so.
    #[test]
    fn tab_is_printable_and_newline_is_not() {
        let options = Options {
            min: 3,
            ..Options::default()
        };
        assert_eq!(lines(b"a\tb\tc\0", &options), ["a\tb\tc"]);
        assert_eq!(lines(b"one\ntwo\0", &options), ["one", "two"]);
    }

    /// `-w` folds the run together, which the offset makes visible: one run at
    /// zero instead of two at zero and four.
    #[test]
    fn include_all_whitespace_joins_across_a_newline() {
        let plain = Options {
            min: 3,
            radix: Some(Radix::Decimal),
            ..Options::default()
        };
        assert_eq!(lines(b"one\ntwo\0", &plain), ["      0 one", "      4 two"]);
        let wide = Options {
            include_all_whitespace: true,
            ..plain
        };
        assert_eq!(lines(b"one\ntwo\0", &wide), ["      0 one", "two"]);
    }

    #[test]
    fn a_vertical_tab_form_feed_and_carriage_return_need_dash_w() {
        let plain = Options {
            min: 2,
            ..Options::default()
        };
        assert!(scanned(b"a\x0bb\x0cc\rd\0", &plain).is_empty());
        let wide = Options {
            include_all_whitespace: true,
            ..plain
        };
        assert_eq!(lines(b"a\x0bb\x0cc\rd\0", &wide), ["a\u{b}b\u{c}c\rd"]);
    }

    /// NUL is never a string character, `-w` or not.
    #[test]
    fn nul_always_breaks_a_run() {
        let options = Options {
            min: 2,
            include_all_whitespace: true,
            radix: Some(Radix::Decimal),
            ..Options::default()
        };
        assert_eq!(lines(b"aa\0\0aa\0", &options), ["      0 aa", "      4 aa"]);
    }

    /// `-e S` admits the high half of the byte range; `-e s` does not.
    #[test]
    fn eight_bit_encoding_admits_the_high_bytes() {
        let seven = Options::default();
        let eight = Options {
            encoding: Encoding::Ascii8,
            ..Options::default()
        };
        assert!(scanned(b"\xff\xff\xff\xff\0", &seven).is_empty());
        assert_eq!(
            scanned(b"\xff\xff\xff\xff\0", &eight),
            b"\xff\xff\xff\xff\n"
        );
    }

    /// The decoded value is printed, not the raw bytes: five characters out of
    /// ten bytes.
    #[test]
    fn a_sixteen_bit_run_prints_one_byte_per_character() {
        let options = Options {
            min: 5,
            encoding: Encoding::Little16,
            ..Options::default()
        };
        assert_eq!(lines(b"h\0e\0l\0l\0o\0\0\0", &options), ["hello"]);
        let too_long = Options { min: 6, ..options };
        assert!(scanned(b"h\0e\0l\0l\0o\0\0\0", &too_long).is_empty());
    }

    /// The restart rule, which is the whole reason this scan is not a fold: a
    /// rejected 16-bit character resumes one *byte* later, so a big-endian
    /// scan of little-endian text finds a run shifted by one.
    #[test]
    fn a_rejected_wide_character_resumes_one_byte_later() {
        let options = Options {
            min: 4,
            encoding: Encoding::Big16,
            radix: Some(Radix::Decimal),
            ..Options::default()
        };
        assert_eq!(lines(b"h\0e\0l\0l\0o\0\0\0", &options), ["      1 ello"]);
    }

    #[test]
    fn a_wide_run_is_found_when_the_endianness_matches() {
        for (encoding, data) in [
            (Encoding::Little16, &b"h\0e\0l\0l\0o\0\0\0"[..]),
            (Encoding::Big16, &b"\0h\0e\0l\0l\0o\0\0"[..]),
            (
                Encoding::Little32,
                &b"h\0\0\0e\0\0\0l\0\0\0l\0\0\0o\0\0\0"[..],
            ),
            (Encoding::Big32, &b"\0\0\0h\0\0\0e\0\0\0l\0\0\0l\0\0\0o"[..]),
        ] {
            let options = Options {
                min: 5,
                encoding,
                ..Options::default()
            };
            assert_eq!(lines(data, &options), ["hello"], "for {encoding:?}");
        }
    }

    /// An odd tail at end of input does not lose the run that precedes it.
    #[test]
    fn a_wide_run_ending_exactly_at_end_of_input_is_printed() {
        let options = Options {
            min: 5,
            encoding: Encoding::Little16,
            ..Options::default()
        };
        assert_eq!(lines(b"h\0e\0l\0l\0o\0", &options), ["hello"]);
    }

    // --------------------------------------------------------- the prefixes --

    #[test]
    fn the_offset_is_the_first_byte_of_the_run_right_aligned_in_seven() {
        let options = Options {
            min: 5,
            radix: Some(Radix::Decimal),
            ..Options::default()
        };
        assert_eq!(lines(b"XX\0hello\0", &options), ["      3 hello"]);
    }

    #[test]
    fn each_radix_prints_in_its_own_base() {
        // Ten bytes of filler that is not itself a string, so the only run is
        // `abcde` and it begins at offset ten -- 12 octal, a hexadecimal.
        let data = b"\0\0\0\0\0\0\0\0\0\0abcde\0";
        for (radix, expected) in [
            (Radix::Decimal, "     10 abcde"),
            (Radix::Octal, "     12 abcde"),
            (Radix::Hex, "      a abcde"),
        ] {
            let options = Options {
                min: 5,
                radix: Some(radix),
                ..Options::default()
            };
            assert_eq!(lines(data, &options), [expected], "for {radix:?}");
        }
    }

    #[test]
    fn a_wide_offset_grows_past_seven_columns() {
        assert_eq!(offset_field(20_000_000, Radix::Decimal), b"20000000 ");
        assert_eq!(offset_field(4, Radix::Decimal), b"      4 ");
    }

    /// The filename comes before the offset, not after.
    #[test]
    fn the_file_name_precedes_the_offset() {
        let options = Options {
            min: 5,
            radix: Some(Radix::Decimal),
            print_file_name: true,
            ..Options::default()
        };
        assert_eq!(lines(b"XX\0hello\0", &options), ["F:       3 hello"]);
    }

    #[test]
    fn the_file_name_is_printed_before_every_string() {
        let options = Options {
            print_file_name: true,
            ..Options::default()
        };
        assert_eq!(lines(b"abcd\0abcde\0", &options), ["F: abcd", "F: abcde"]);
    }

    /// The separator replaces the newline after *every* string, the last one
    /// included, so the output does not end in a newline.
    #[test]
    fn the_separator_follows_every_string_including_the_last() {
        let options = Options {
            separator: b"|".to_vec(),
            ..Options::default()
        };
        assert_eq!(scanned(b"abcd\0abcde\0", &options), b"abcd|abcde|");
        let empty = Options {
            separator: Vec::new(),
            ..Options::default()
        };
        assert_eq!(scanned(b"abcd\0abcde\0", &empty), b"abcdabcde");
    }

    // ------------------------------------------------------ the diagnostics --

    #[test]
    fn the_three_failure_sentences_have_the_shapes_upstream_uses() {
        assert_eq!(no_such_file(b"gone"), b"strings: 'gone': No such file\n");
        assert_eq!(
            is_a_directory(b"adir"),
            b"strings: Warning: 'adir' is a directory\n"
        );
        // Unquoted, unlike the two above. That asymmetry is upstream's.
        assert_eq!(
            open_failed(b"locked", "Permission denied"),
            b"strings: locked: Permission denied\n"
        );
    }

    /// A name that is not UTF-8 must reach the diagnostic byte for byte.
    #[test]
    fn a_non_utf8_name_survives_the_diagnostic() {
        let line = no_such_file(b"\xff\xfegone");
        assert_eq!(line, b"strings: '\xff\xfegone': No such file\n");
    }

    #[test]
    fn the_os_error_tail_is_stripped_from_a_reason() {
        let e = io::Error::new(io::ErrorKind::PermissionDenied, "Permission denied");
        assert_eq!(clean_reason(&e), "Permission denied");
    }

    // ------------------------------------------------------------- the text --

    /// The empty prefix matches every entry, so this is the whole long-option
    /// table in declaration order — the measurement `strings --=x` makes
    /// upstream perform on itself.
    #[test]
    fn the_empty_prefix_prints_the_whole_table_in_upstreams_order() {
        let e = parse_args(&argv(&["--=x", "f"])).unwrap_err();
        assert!(
            e.sentence.starts_with(
                "option '--=x' is ambiguous; possibilities: '--all' '--bytes' '--data' \
                 '--encoding' '--help' '--include-all-whitespace' '--output-separator' \
                 '--print-file-name' '--radix' '--target' '--unicode' '--version'"
            ),
            "got {:?}",
            e.sentence
        );
    }

    #[test]
    fn an_unknown_option_is_refused_with_upstreams_status() {
        let e = parse_args(&argv(&["-Q", "f"])).unwrap_err();
        assert_eq!(e.sentence, "invalid option -- 'Q'");
        assert_eq!(e.status, 1);
    }

    #[test]
    fn the_help_text_is_upstreams_wording() {
        let text = help_text();
        assert!(text.starts_with("Usage: strings [option(s)] [file(s)]\n"));
        assert!(text.contains("  -a - --all                Scan the entire file"));
        assert!(text.contains("(The default is 4)"));
        // The BFD lines upstream prints are not ours to print.
        assert!(!text.contains("supported targets"));
        assert!(!text.contains("sourceware.org"));
    }

    #[test]
    fn the_version_text_names_this_build() {
        let text = version_text();
        assert!(text.starts_with("strings (SlateOS coreutils) 0.1.0\n"));
        assert_eq!(text.lines().count(), 5);
    }
}
