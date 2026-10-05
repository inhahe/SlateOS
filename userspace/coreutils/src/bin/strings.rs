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
//! # `-U`/`--unicode`
//!
//! Upstream's six modes, ported from `strings.c`'s `print_unicode_stream` and
//! `print_unicode_buffer` with their quirks, because each is visible: `l`
//! prints only the *first byte* of a character (`printf ("%.1s", ...)`); a
//! four-byte character is escaped through arithmetic that is not its code
//! point (U+1F600 is `\u07c600`); the help advertises `show`/`s` where the
//! parser takes `locale`/`l` and refuses `s`; and any mode but `d` forces
//! `-e S`. See [`Unicode`].
//!
//! # `@FILE`
//!
//! libiberty's `expandargv`, which upstream runs before it parses anything:
//! each `@FILE` argument that names a readable file is replaced by the words
//! in it, split and quoted as `buildargv` splits them. See
//! [`expand_at_files`].
//!
//! # Where this diverges
//!
//! **`-T`/`--target` is refused, not ignored.** It names a BFD object format
//! for `-d` to read the file as, and this build reads ELF64 only. Accepting a
//! format and reading the file as ELF anyway would answer a question other
//! than the one asked, quietly. Logged as
//! `known-issues/B-STRINGS-HAS-NO-OBJECT-FILE-READER.md`.

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Report, Takes};
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
    /// `-d`: scan only the initialised, loaded sections of an object file.
    ///
    /// For anything that is not an object file this changes nothing, which
    /// is upstream's behaviour and was measured rather than assumed: GNU
    /// `strings -d` on a text file prints exactly what `strings` does.
    data_only: bool,
    /// What follows each string. A newline unless `-s` said otherwise.
    separator: Vec<u8>,
    /// `-U`: what a UTF-8 sequence is. Anything but the default forces
    /// `-e S`, as upstream does after its option loop.
    unicode: Unicode,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            min: 4,
            encoding: Encoding::default(),
            radix: None,
            print_file_name: false,
            include_all_whitespace: false,
            data_only: false,
            separator: b"\n".to_vec(),
            unicode: Unicode::Default,
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
    /// A refusal upstream prints with `fatal`: `strings: SENTENCE`, status 1,
    /// no usage. Bytes, because it quotes an argument as given.
    Fatal(Vec<u8>),
}

impl PartialEq for Options {
    fn eq(&self, other: &Self) -> bool {
        self.min == other.min
            && self.encoding == other.encoding
            && self.radix == other.radix
            && self.data_only == other.data_only
            && self.print_file_name == other.print_file_name
            && self.include_all_whitespace == other.include_all_whitespace
            && self.separator == other.separator
            && self.unicode == other.unicode
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
    text.push_str("This build reads ELF64 objects only, for --data, so --target is refused\n");
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
///
/// The refusal is upstream's `fatal`, the argument quoted back as given, so it
/// is bytes: an argument need not be text.
fn read_min_length(argument: &[u8]) -> Result<usize, Vec<u8>> {
    let with_argument = |head: &str| {
        let mut sentence = head.as_bytes().to_vec();
        sentence.extend_from_slice(argument);
        sentence
    };
    let Some(value) = strtoul_base0(argument) else {
        return Err(with_argument("invalid integer argument "));
    };
    if value == MIN_LENGTH_CEILING {
        return Err(format!("minimum string length {value} is too big").into_bytes());
    }
    if value > MIN_LENGTH_CEILING {
        return Err(with_argument("minimum string length is too big: "));
    }
    if value < 1 {
        return Err(with_argument("minimum string length is too small: "));
    }
    usize::try_from(value).map_err(|_| with_argument("minimum string length is too big: "))
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

    // Upstream's `numeric_opt`: see the digit arm.
    let mut numeric: Option<OsString> = None;
    let mut parser = STRINGS.parse(args, SHORT_OPTIONS, LONG_OPTIONS);
    // Not a `for`: the digit arm asks the parser which word it is in, between
    // items, and a `for` loop holds the parser for the loop's whole length.
    #[allow(clippy::while_let_on_iterator)]
    while let Some(item) = parser.next() {
        match item? {
            Opt::Long("help", _) | Opt::Short(b'h' | b'H', _) => return Ok(Request::Help),
            Opt::Long("version", _) | Opt::Short(b'v' | b'V', _) => return Ok(Request::Version),

            // `-a` is this build's only mode, so it is a no-op rather than a
            // setting: there is nothing for it to switch back from.
            Opt::Long("all", _) | Opt::Short(b'a', _) => {}
            Opt::Long("data", _) | Opt::Short(b'd', _) => options.data_only = true,
            Opt::Long("target", _) | Opt::Short(b'T', _) => {
                return Err(unsupported("--target"));
            }

            Opt::Long("print-file-name", _) | Opt::Short(b'f', _) => options.print_file_name = true,
            Opt::Long("include-all-whitespace", _) | Opt::Short(b'w', _) => {
                options.include_all_whitespace = true;
            }

            Opt::Long("bytes", value) | Opt::Short(b'n', value) => {
                let value = value.unwrap_or_default();
                match read_min_length(&os_bytes(value.as_os_str())) {
                    Ok(min) => options.min = min,
                    Err(sentence) => return Ok(Request::Fatal(sentence)),
                }
            }
            // The digit shorthand, upstream's `numeric_opt`: the word that held
            // the last digit option, read whole after its `-` once every other
            // option has been. So `-12` is twelve, not two; `-5 -n 3` is five,
            // whichever comes first; and `-a5` is `invalid integer argument
            // a5`. Measured, binutils 2.42.
            Opt::Short(b'0'..=b'9', _) => numeric = parser.current_word().cloned(),

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
                let bytes = os_bytes(value.as_os_str());
                let Some(mode) = Unicode::parse(&bytes) else {
                    // `fatal (_("invalid argument to -U/--unicode: %s"),
                    // optarg)`: the argument as given, and no usage.
                    let mut sentence = b"invalid argument to -U/--unicode: ".to_vec();
                    sentence.extend_from_slice(&bytes);
                    return Ok(Request::Fatal(sentence));
                };
                options.unicode = mode;
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
                any_operand = true;
                sources.push(Source::Path(operand.clone()));
            }
        }
    }

    if let Some(word) = numeric {
        let bytes = os_bytes(&word);
        match read_min_length(bytes.get(1..).unwrap_or_default()) {
            Ok(min) => options.min = min,
            Err(sentence) => return Ok(Request::Fatal(sentence)),
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

    // Upstream's `if (unicode_display != unicode_default) encoding = 'S';`,
    // after the whole option loop, so it wins over any `-e`.
    if options.unicode != Unicode::Default {
        options.encoding = Encoding::Ascii8;
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
/// The `SHF_ALLOC` flag: this section occupies memory at run time.
const SHF_ALLOC: u64 = 0x2;
/// `SHT_NOBITS`: the section occupies no file space -- `.bss` is the one that
/// matters here. It is `SHF_ALLOC` *and* `SHT_NOBITS`, which is exactly why
/// both halves of the test are needed: scanning it would read whatever the file
/// happens to hold at that offset, which is the next section.
const SHT_NOBITS: u32 = 8;

/// Byte ranges `-d` scans: the initialised, loaded sections of an ELF64.
///
/// `None` means "this is not a file I can take apart", and the caller then
/// scans the whole thing. That is not a fallback to paper over a gap -- it is
/// upstream's behaviour, measured: GNU `strings -d` on a text file prints
/// exactly what `strings` does. Only ELF64 little-endian is parsed, which is
/// what this OS builds and what the differential harness compares against; a
/// 32-bit or big-endian object would be scanned whole, which is wrong but
/// wrong in the direction of printing more rather than less.
///
/// The rule was checked against binutils before it was written, not after: on
/// `/bin/true`, scanning every `SHF_ALLOC` non-`NOBITS` section independently
/// yields 136 strings, and `strings -d` yields 136.
///
/// Every offset and length here is attacker-chosen -- an object file is input
/// like any other -- so each is range-checked against the buffer with
/// `checked_add` and `get`, never indexed.
fn elf_alloc_ranges(buf: &[u8]) -> Option<Vec<(usize, usize)>> {
    const ELF_MAGIC: &[u8] = b"\x7fELF";
    const ELFCLASS64: u8 = 2;
    const ELFDATA2LSB: u8 = 1;
    const SH_ENTRY_MIN: usize = 40;

    if !buf.starts_with(ELF_MAGIC) {
        return None;
    }
    if *buf.get(4)? != ELFCLASS64 || *buf.get(5)? != ELFDATA2LSB {
        return None;
    }

    let u16_at = |at: usize| -> Option<u16> {
        let end = at.checked_add(2)?;
        Some(u16::from_le_bytes(buf.get(at..end)?.try_into().ok()?))
    };
    let u32_at = |at: usize| -> Option<u32> {
        let end = at.checked_add(4)?;
        Some(u32::from_le_bytes(buf.get(at..end)?.try_into().ok()?))
    };
    let u64_at = |at: usize| -> Option<u64> {
        let end = at.checked_add(8)?;
        Some(u64::from_le_bytes(buf.get(at..end)?.try_into().ok()?))
    };

    let sh_off = usize::try_from(u64_at(0x28)?).ok()?;
    let sh_entsize = usize::from(u16_at(0x3A)?);
    let sh_num = usize::from(u16_at(0x3C)?);
    if sh_entsize < SH_ENTRY_MIN || sh_num == 0 {
        return None;
    }

    let mut ranges = Vec::new();
    for index in 0..sh_num {
        let at = sh_off.checked_add(index.checked_mul(sh_entsize)?)?;
        let sh_type = u32_at(at.checked_add(4)?)?;
        let sh_flags = u64_at(at.checked_add(8)?)?;
        let offset = usize::try_from(u64_at(at.checked_add(24)?)?).ok()?;
        let size = usize::try_from(u64_at(at.checked_add(32)?)?).ok()?;
        if sh_flags & SHF_ALLOC == 0 || sh_type == SHT_NOBITS || size == 0 {
            continue;
        }
        // A header that names bytes the file does not have is a malformed
        // object, not a reason to read past the end.
        let end = offset.checked_add(size)?;
        if end > buf.len() {
            return None;
        }
        ranges.push((offset, size));
    }
    if ranges.is_empty() {
        None
    } else {
        Some(ranges)
    }
}

/// Read a file whole and scan only its loaded, initialised sections.
///
/// Falls back to scanning everything when the file is not an ELF64 this can
/// take apart, which is upstream's behaviour for a non-object file.
fn read_and_scan_sections<W: Write>(
    mut file: File,
    out: &mut W,
    name: &[u8],
    options: &Options,
    tty: bool,
) -> io::Result<()> {
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    match elf_alloc_ranges(&buf) {
        None if options.unicode == Unicode::Default => {
            scan(buf.as_slice(), out, Some(name), options, 0)
        }
        // Not an object: upstream falls back to reading it as a stream.
        None => scan_unicode_stream(buf.as_slice(), out, Some(name), options, 0, tty),
        Some(ranges) => {
            for (offset, size) in ranges {
                // Bounds were checked when the range was built; `get` rather
                // than a slice index keeps that true if either ever changes.
                let Some(section) = buf.get(offset..offset.saturating_add(size)) else {
                    continue;
                };
                // Each section is its own stream: a run of printable bytes
                // that happens to straddle a boundary is two strings, not one,
                // which is what upstream reports.
                if options.unicode == Unicode::Default {
                    scan(section, out, Some(name), options, offset as u64)?;
                } else {
                    scan_unicode_buffer(section, out, Some(name), options, offset as u64, tty)?;
                }
            }
            Ok(())
        }
    }
}

fn scan<R: Read, W: Write>(
    input: R,
    out: &mut W,
    label: Option<&[u8]>,
    options: &Options,
    // Added to every reported offset. Non-zero only under `-d`, where each
    // section is scanned as its own stream and `-t` must still report where
    // the string is in the FILE, not in the section.
    base: u64,
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
            out.write_all(&offset_field(start.saturating_add(base), radix))?;
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

// ------------------------------------------------------------ -U/--unicode ---

/// `-U`/`--unicode`: what a scan does with a UTF-8 sequence.
///
/// Upstream's spellings are what its *code* accepts, which is not quite what
/// its `--help` advertises: the help lists `show`/`s`, and the parser takes
/// `locale`/`l` for that mode and refuses `s`. Measured; the code wins.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Unicode {
    /// `d`, `default`: no special treatment. A byte above 127 is judged by
    /// `-e` like any other.
    #[default]
    Default,
    /// `l`, `locale`: the character as it is -- except that upstream prints it
    /// with `printf ("%.1s", buffer)`, which is its *first byte* only, and
    /// that is what this prints too.
    Locale,
    /// `e`, `escape`: `\uXXXX`.
    Escape,
    /// `x`, `hex`: `<0xC3A9>`, the encoded bytes.
    Hex,
    /// `h`, `highlight`: as `escape`, in red on white when standard output is
    /// a terminal.
    Highlight,
    /// `i`, `invalid`: a valid sequence ends a run, as a non-graphic byte does.
    Invalid,
}

impl Unicode {
    /// Upstream's `-U` argument table, exact spellings only.
    fn parse(value: &[u8]) -> Option<Self> {
        Some(match value {
            b"default" | b"d" => Unicode::Default,
            b"locale" | b"l" => Unicode::Locale,
            b"escape" | b"e" => Unicode::Escape,
            b"invalid" | b"i" => Unicode::Invalid,
            b"hex" | b"x" => Unicode::Hex,
            b"highlight" | b"h" => Unicode::Highlight,
            _ => return None,
        })
    }
}

/// How long the UTF-8 sequence led by `lead` is, by upstream's rule: the two
/// bits below the top two. A lead it has already accepted is the only input.
fn utf8_len(lead: u8) -> usize {
    match lead & 0x30 {
        0x00 | 0x10 => 2,
        0x20 => 3,
        _ => 4,
    }
}

/// Upstream's `is_valid_utf8`: how many bytes the sequence at the front of
/// `bytes` takes, or `None`. Only the lead's top bits and the continuation
/// bytes' `10` prefix are checked -- not overlong forms, not surrogates, not a
/// lead above `0xF4` -- and that leniency is reproduced, because it decides
/// which runs are printed.
fn valid_utf8(bytes: &[u8]) -> Option<usize> {
    let lead = *bytes.first()?;
    let continues = |i: usize| bytes.get(i).is_some_and(|b| b & 0xc0 == 0x80);
    if lead < 0xc0 || !continues(1) {
        return None;
    }
    if lead & 0x20 == 0 {
        return Some(2);
    }
    if !continues(2) {
        return None;
    }
    if lead & 0x10 == 0 {
        return Some(3);
    }
    if !continues(3) {
        return None;
    }
    Some(4)
}

/// Upstream's `display_utf8_char`: render the sequence at the front of
/// `bytes`, which is known to be valid, and say how many bytes it was.
///
/// The arithmetic is upstream's to the bit, including where it is wrong: a
/// four-byte sequence is split into three fields that are not the code
/// point's bytes, and the first can overflow `%02x`'s two digits. Measured
/// against GNU strings 2.42: U+1F600 prints as `߆00` and U+10FFFF as
/// `ြfff`, where U+00E9 and U+20AC print as `é` and `€`.
fn display_utf8_char<W: Write>(
    out: &mut W,
    bytes: &[u8],
    mode: Unicode,
    tty: bool,
) -> io::Result<usize> {
    let byte = |i: usize| u32::from(bytes.get(i).copied().unwrap_or(0));
    let len = utf8_len(bytes.first().copied().unwrap_or(0));
    match mode {
        Unicode::Escape | Unicode::Highlight => {
            let paint = mode == Unicode::Highlight && tty;
            if paint {
                out.write_all(b"\x1b[31;47m")?;
            }
            match len {
                2 => write!(
                    out,
                    "\\u{:02x}{:02x}",
                    (byte(0) & 0x1c) >> 2,
                    ((byte(0) & 0x03) << 6) | (byte(1) & 0x3f)
                )?,
                3 => write!(
                    out,
                    "\\u{:02x}{:02x}",
                    ((byte(0) & 0x0f) << 4) | ((byte(1) & 0x3c) >> 2),
                    ((byte(1) & 0x03) << 6) | (byte(2) & 0x3f)
                )?,
                _ => write!(
                    out,
                    "\\u{:02x}{:02x}{:02x}",
                    ((byte(0) & 0x07) << 6) | ((byte(1) & 0x3c) >> 2),
                    ((byte(1) & 0x03) << 6) | ((byte(2) & 0x3c) >> 2),
                    ((byte(2) & 0x03) << 6) | (byte(3) & 0x3f)
                )?,
            }
            if paint {
                out.write_all(b"\x1b[0m")?;
            }
        }
        Unicode::Hex => {
            out.write_all(b"<0x")?;
            for i in 0..len {
                write!(out, "{:02x}", byte(i))?;
            }
            out.write_all(b">")?;
        }
        // `printf ("%.1s", buffer)`: one byte.
        Unicode::Locale => out.write_all(bytes.get(..1).unwrap_or_default())?,
        // Upstream never calls this in either mode.
        Unicode::Default | Unicode::Invalid => {}
    }
    Ok(len)
}

/// A byte at a time, as `getc` gives them, with upstream's pushback stack in
/// front: `get_unicode_byte`. `read` counts only the bytes that came from the
/// stream, not those taken back off the stack -- upstream's `num_read`, which
/// is how it computes addresses, quirks and all.
struct UnicodeReader<R: Read> {
    window: Window<R>,
    putback: Vec<u8>,
}

impl<R: Read> UnicodeReader<R> {
    fn get(&mut self, read: &mut u32) -> io::Result<Option<u8>> {
        if let Some(b) = self.putback.pop() {
            return Ok(Some(b));
        }
        *read = read.wrapping_add(1);
        if !self.window.ensure(1)? {
            return Ok(None);
        }
        let b = self.window.peek(1).and_then(|s| s.first().copied());
        self.window.advance(1);
        Ok(b)
    }

    /// Push bytes back, in the order given: the last one pushed comes out first.
    fn unget(&mut self, bytes: &[u8]) {
        self.putback.extend_from_slice(bytes);
    }
}

/// Upstream's `print_unicode_stream` and its tail-recursive body, as a loop.
///
/// The first phase looks for `min` characters, keeping them; the second prints
/// them and then everything after them up to the first byte that is not part
/// of a string. Two of upstream's quirks are kept because they are visible:
///
/// - A four-byte sequence refused under `invalid` is pushed back in the order
///   4th, 2nd, 3rd -- so it is re-read as 3rd, 2nd, 4th -- where every other
///   case pushes back in reverse. Harmless for the string it ends, and kept
///   for the next one it may start.
/// - The address of a string is `num_read - 1` past the start of the body's
///   pass, where `num_read` counts bytes read from the *stream*. A string
///   whose first byte was a pushed-back one is therefore placed at
///   `UINT_MAX` past the pass's start -- `-t` shows it as a huge offset.
fn scan_unicode_stream<R: Read, W: Write>(
    input: R,
    out: &mut W,
    label: Option<&[u8]>,
    options: &Options,
    base: u64,
    tty: bool,
) -> io::Result<()> {
    let mode = options.unicode;
    let graphic = |c: u8| {
        is_string_char(
            u32::from(c),
            Encoding::Ascii8,
            options.include_all_whitespace,
        )
    };
    let mut reader = UnicodeReader {
        window: Window::new(input),
        putback: Vec::with_capacity(5),
    };
    let mut address = base;
    let mut kept: Vec<u8> = Vec::with_capacity(options.min.saturating_mul(4).saturating_add(1));

    loop {
        let mut start_point: u64 = 0;
        let mut read: u32 = 0;
        let mut chars: usize = 0;
        kept.clear();
        let mut c: Option<u8> = Some(0);

        // Phase one: `min` characters in a row.
        loop {
            if chars >= options.min {
                break;
            }
            c = reader.get(&mut read)?;
            let Some(b) = c else { break };
            if !graphic(b) {
                chars = 0;
                kept.clear();
                continue;
            }
            if chars == 0 {
                start_point = u64::from(read.wrapping_sub(1));
            }
            if b < 127 {
                kept.push(b);
                chars = chars.saturating_add(1);
                continue;
            }
            if b < 0xc0 {
                chars = 0;
                kept.clear();
                continue;
            }
            let mut seq = [b, 0, 0, 0];
            c = reader.get(&mut read)?;
            let Some(b1) = c else { break };
            seq[1] = b1;
            if b1 & 0xc0 != 0x80 {
                reader.unget(&[b1]);
                chars = 0;
                kept.clear();
                continue;
            }
            if b & 0x20 == 0 {
                if mode == Unicode::Invalid {
                    reader.unget(&[b1]);
                    chars = 0;
                    kept.clear();
                } else {
                    kept.extend_from_slice(&seq[..2]);
                    chars = chars.saturating_add(1);
                }
                continue;
            }
            c = reader.get(&mut read)?;
            let Some(b2) = c else { break };
            seq[2] = b2;
            if b2 & 0xc0 != 0x80 {
                reader.unget(&[b2, b1]);
                chars = 0;
                kept.clear();
                continue;
            }
            if b & 0x10 == 0 {
                if mode == Unicode::Invalid {
                    reader.unget(&[b2, b1]);
                    chars = 0;
                    kept.clear();
                } else {
                    kept.extend_from_slice(&seq[..3]);
                    chars = chars.saturating_add(1);
                }
                continue;
            }
            c = reader.get(&mut read)?;
            let Some(b3) = c else { break };
            seq[3] = b3;
            if b3 & 0xc0 != 0x80 {
                reader.unget(&[b3, b2, b1]);
                chars = 0;
                kept.clear();
            } else if mode == Unicode::Invalid {
                // Upstream's order, not the reverse: see the doc comment.
                reader.unget(&[b3, b1, b2]);
                chars = 0;
                kept.clear();
            } else {
                kept.extend_from_slice(&seq);
                chars = chars.saturating_add(1);
            }
        }

        if chars >= options.min {
            if let Some(label) = label.filter(|_| options.print_file_name) {
                out.write_all(label)?;
                out.write_all(b": ")?;
            }
            if let Some(radix) = options.radix {
                out.write_all(&offset_field(address.wrapping_add(start_point), radix))?;
            }
            let mut i = 0;
            while let Some(&b) = kept.get(i) {
                if b < 127 {
                    out.write_all(&[b])?;
                    i = i.saturating_add(1);
                } else {
                    let len = display_utf8_char(out, kept.get(i..).unwrap_or_default(), mode, tty)?;
                    i = i.saturating_add(len);
                }
            }

            // Phase two: the rest of the run, unchecked against `min`.
            loop {
                c = reader.get(&mut read)?;
                let Some(b) = c else { break };
                if !graphic(b) {
                    break;
                }
                if b < 127 {
                    out.write_all(&[b])?;
                    continue;
                }
                if b < 0xc0 {
                    break;
                }
                let mut seq = [b, 0, 0, 0];
                c = reader.get(&mut read)?;
                let Some(b1) = c else { break };
                seq[1] = b1;
                if b1 & 0xc0 != 0x80 {
                    reader.unget(&[b1]);
                    break;
                }
                if b & 0x20 == 0 {
                    if mode == Unicode::Invalid {
                        reader.unget(&[b1]);
                        break;
                    }
                    display_utf8_char(out, &seq, mode, tty)?;
                    continue;
                }
                c = reader.get(&mut read)?;
                let Some(b2) = c else { break };
                seq[2] = b2;
                if b2 & 0xc0 != 0x80 {
                    reader.unget(&[b2, b1]);
                    break;
                }
                if b & 0x10 == 0 {
                    if mode == Unicode::Invalid {
                        reader.unget(&[b2, b1]);
                        break;
                    }
                    display_utf8_char(out, &seq, mode, tty)?;
                    continue;
                }
                c = reader.get(&mut read)?;
                let Some(b3) = c else { break };
                seq[3] = b3;
                if b3 & 0xc0 != 0x80 || mode == Unicode::Invalid {
                    reader.unget(&[b3, b2, b1]);
                    break;
                }
                display_utf8_char(out, &seq, mode, tty)?;
            }
            out.write_all(&options.separator)?;
        }

        if c.is_none() {
            return Ok(());
        }
        address = address.wrapping_add(u64::from(read));
    }
}

/// Upstream's `print_unicode_buffer`: the same search over bytes already in
/// memory -- an object file's section under `-d` -- where a sequence is checked
/// by looking ahead rather than by reading and pushing back. Its rule differs
/// from the stream's in one place that shows: a valid sequence *starts* a run
/// here only if it is not refused, and the search resumes one byte later.
fn scan_unicode_buffer<W: Write>(
    buf: &[u8],
    out: &mut W,
    label: Option<&[u8]>,
    options: &Options,
    address: u64,
    tty: bool,
) -> io::Result<()> {
    let mode = options.unicode;
    let graphic = |c: u8| {
        is_string_char(
            u32::from(c),
            Encoding::Ascii8,
            options.include_all_whitespace,
        )
    };
    let mut from = 0usize;
    loop {
        let rest = buf.get(from..).unwrap_or_default();
        if rest.is_empty() {
            return Ok(());
        }
        let mut start = 0usize;
        let mut found = 0usize;
        let mut i = 0usize;
        while i < rest.len() {
            let c = rest.get(i).copied().unwrap_or(0);
            let mut len = 1;
            if !graphic(c) {
                found = 0;
                i = i.saturating_add(len);
                continue;
            }
            if c > 126 {
                if c < 0xc0 {
                    found = 0;
                    i = i.saturating_add(len);
                    continue;
                }
                match valid_utf8(rest.get(i..).unwrap_or_default()) {
                    None => {
                        found = 0;
                        i = i.saturating_add(1);
                        continue;
                    }
                    Some(n) => {
                        len = n;
                        if mode == Unicode::Invalid {
                            found = 0;
                            i = i.saturating_add(len);
                            continue;
                        }
                    }
                }
            }
            if found == 0 {
                start = i;
            }
            found = found.saturating_add(1);
            if found >= options.min {
                break;
            }
            i = i.saturating_add(len);
        }
        if found < options.min {
            return Ok(());
        }

        if let Some(label) = label.filter(|_| options.print_file_name) {
            out.write_all(label)?;
            out.write_all(b": ")?;
        }
        if let Some(radix) = options.radix {
            let at = address.wrapping_add(from as u64).wrapping_add(start as u64);
            out.write_all(&offset_field(at, radix))?;
        }
        let mut i = start;
        while let Some(&c) = rest.get(i) {
            if !graphic(c) {
                break;
            }
            if c < 127 {
                out.write_all(&[c])?;
                i = i.saturating_add(1);
                continue;
            }
            if valid_utf8(rest.get(i..).unwrap_or_default()).is_none() || mode == Unicode::Invalid {
                break;
            }
            let len = display_utf8_char(out, rest.get(i..).unwrap_or_default(), mode, tty)?;
            i = i.saturating_add(len);
        }
        out.write_all(&options.separator)?;
        from = from.saturating_add(i);
    }
}

// ------------------------------------------------------------- @FILE ---

/// Why `@FILE` expansion stopped the program: libiberty's two refusals, which
/// it prints as `ARGV0: error: ...` and exits 1 on, before any option is read.
#[derive(Debug, PartialEq, Eq)]
enum AtFileError {
    /// More than 2000 `@FILE`s, counted across nesting: the guard against a
    /// file that names itself.
    TooMany,
    /// An `@FILE` that names a directory.
    Directory,
}

impl AtFileError {
    fn sentence(&self) -> &'static str {
        match self {
            AtFileError::TooMany => "error: too many @-files encountered",
            AtFileError::Directory => "error: @-file refers to a directory",
        }
    }
}

/// libiberty's `expandargv`: replace every argument that is `@FILE`, where
/// `FILE` can be read, by the arguments the file holds -- and those, in turn,
/// if they are `@FILE`s themselves.
///
/// Every argument is looked at, options and operands alike and after `--`
/// too, because the expansion runs before the option parser has seen any of
/// them. An `@FILE` whose file cannot be `stat`ed or opened is left as it is,
/// so it reaches the parser and, as an operand, is reported as a file that
/// does not exist. A file holding nothing but white space expands to nothing.
fn expand_at_files(args: &[OsString]) -> Result<Vec<OsString>, AtFileError> {
    let mut out: Vec<OsString> = args.to_vec();
    let mut limit: u32 = 2000;
    let mut i = 0usize;
    while i < out.len() {
        let Some(arg) = out.get(i) else { break };
        let bytes = os_bytes(arg.as_os_str()).into_owned();
        let Some(name) = bytes.strip_prefix(b"@") else {
            i = i.saturating_add(1);
            continue;
        };
        limit = limit.saturating_sub(1);
        if limit == 0 {
            return Err(AtFileError::TooMany);
        }
        let path = coreutils::quote::os_from_bytes(name);
        let Ok(meta) = std::fs::metadata(&path) else {
            i = i.saturating_add(1);
            continue;
        };
        if meta.is_dir() {
            return Err(AtFileError::Directory);
        }
        let Ok(content) = std::fs::read(&path) else {
            i = i.saturating_add(1);
            continue;
        };
        // Upstream reads the file into a C string, so a NUL ends it.
        let text = content.split(|&b| b == 0).next().unwrap_or_default();
        let words: Vec<OsString> = if text.iter().all(|&b| is_c_space(b)) {
            Vec::new()
        } else {
            build_argv(text)
                .into_iter()
                .map(|w| coreutils::quote::os_from_bytes(&w))
                .collect()
        };
        // Replace the `@FILE` and look again from its first word, so that a
        // file naming another file is expanded too.
        out.splice(i..=i, words);
    }
    Ok(out)
}

/// C's `isspace` in the C locale, which is what libiberty's `ISSPACE` is.
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// libiberty's `buildargv`: split `text` into words at white space, where
/// `'...'` and `"..."` quote white space -- and may start or end mid-word --
/// and a backslash makes the next byte literal, inside quotes or out. The
/// quotes themselves are dropped. The input is not empty and not all white
/// space; the caller checks.
fn build_argv(text: &[u8]) -> Vec<Vec<u8>> {
    let mut words = Vec::new();
    let mut at = text.iter().take_while(|&&b| is_c_space(b)).count();
    let (mut squote, mut dquote, mut bsquote) = (false, false, false);
    loop {
        let mut word = Vec::new();
        while let Some(&b) = text.get(at) {
            if is_c_space(b) && !squote && !dquote && !bsquote {
                break;
            }
            if bsquote {
                bsquote = false;
                word.push(b);
            } else if b == b'\\' {
                bsquote = true;
            } else if squote {
                if b == b'\'' {
                    squote = false;
                } else {
                    word.push(b);
                }
            } else if dquote {
                if b == b'"' {
                    dquote = false;
                } else {
                    word.push(b);
                }
            } else if b == b'\'' {
                squote = true;
            } else if b == b'"' {
                dquote = true;
            } else {
                word.push(b);
            }
            at = at.saturating_add(1);
        }
        words.push(word);
        at = at.saturating_add(
            text.get(at..)
                .unwrap_or_default()
                .iter()
                .take_while(|&&b| is_c_space(b))
                .count(),
        );
        if at >= text.len() {
            return words;
        }
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

    let mut argv = std::env::args_os();
    // Upstream names itself by `argv[0]` as given in `expandargv`'s two
    // refusals, which run before anything else.
    let argv0 = argv
        .next()
        .map_or_else(|| b"strings".to_vec(), |a| os_bytes(&a).into_owned());
    let given: Vec<OsString> = argv.collect();
    let args = match expand_at_files(&given) {
        Ok(args) => args,
        Err(e) => {
            let mut line = argv0;
            line.extend_from_slice(b": ");
            line.extend_from_slice(e.sentence().as_bytes());
            line.push(b'\n');
            stdfd::diag_bytes(&line);
            return ExitCode::from(1);
        }
    };
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(e) => {
            // Upstream has THREE shapes here and this build had one, which is
            // where its last diagnostic differences came from. Measured against
            // binutils 2.42 rather than reasoned about:
            //
            //   -n 0            sentence only, no usage
            //   -n  (no value)  sentence, then the whole usage
            //   -t q            the usage alone, no sentence
            //
            // `referral` is what separates the first two. A getopt error --
            // a missing argument, an unknown option -- carries one; a value
            // this program rejected itself does not. So the field that decides
            // whether to print `Try '... --help'` elsewhere in the tree decides
            // whether to print the usage here.
            //
            // And the referral itself is never printed: `strings` refers the
            // reader to the usage by SHOWING it, where coreutils proper refers
            // to `--help` by naming it. Printing both is this build's own
            // invention and appeared in six of the thirteen differences.
            let getopt_error = e.referral.is_some();
            if !e.sentence.is_empty() {
                let bare = getopt::Error {
                    sentence: e.sentence.clone(),
                    referral: None,
                    ..e
                };
                STRINGS.report(&bare);
            }
            if getopt_error || e.sentence.is_empty() {
                stdfd::diag_bytes(help_text().as_bytes());
            }
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
        Request::Fatal(sentence) => {
            let mut line = b"strings: ".to_vec();
            line.extend_from_slice(&sentence);
            line.push(b'\n');
            stdfd::diag_bytes(&line);
            ExitCode::from(1)
        }
    };

    stdfd::close_stdout("strings", out, earned)
}

fn run(out: &mut Stream, options: &Options, sources: &[Source]) -> ExitCode {
    let mut failed = false;
    // `-U h` colours only for a terminal, as upstream's `isatty (1)` decides.
    let tty = stdfd::is_tty(1);

    for source in sources {
        match source {
            Source::Stdin => {
                let result = if options.unicode == Unicode::Default {
                    scan(io::stdin().lock(), out, Some(STDIN_LABEL), options, 0)
                } else {
                    scan_unicode_stream(io::stdin().lock(), out, Some(STDIN_LABEL), options, 0, tty)
                };
                if let Err(e) = result {
                    stdfd::diag_bytes(&read_failed(STDIN_LABEL, &strerror(&e)));
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
                        stdfd::diag_bytes(&open_failed(&name, &strerror(&e)));
                        failed = true;
                        continue;
                    }
                };
                // `-d` needs the whole file in hand before it can find the
                // section table, so it reads rather than streams. Only under
                // `-d`: the default path still streams, because a file that
                // does not fit in memory is exactly the kind `strings` is
                // pointed at.
                let result = if options.data_only {
                    read_and_scan_sections(file, out, &name, options, tty)
                } else if options.unicode == Unicode::Default {
                    scan(file, out, Some(&name), options, 0)
                } else {
                    scan_unicode_stream(file, out, Some(&name), options, 0, tty)
                };
                if let Err(e) = result {
                    stdfd::diag_bytes(&read_failed(&name, &strerror(&e)));
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
        scan(data, &mut out, Some(b"F"), options, 0).unwrap();
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
        assert_eq!(
            parse_args(&argv(&["-n", "5 ", "f"])).unwrap(),
            Request::Fatal(b"invalid integer argument 5 ".to_vec())
        );
    }

    #[test]
    fn a_minimum_length_that_is_not_a_number_is_fatal() {
        assert_eq!(
            parse_args(&argv(&["-n", "abc", "f"])).unwrap(),
            Request::Fatal(b"invalid integer argument abc".to_vec())
        );
    }

    /// The old implementation quietly used the default here. Upstream reads
    /// the filename as the argument and then complains about it, which at
    /// least tells the user something went wrong.
    #[test]
    fn a_trailing_dash_n_consumes_the_next_word_whatever_it_is() {
        assert_eq!(
            parse_args(&argv(&["-n", "file"])).unwrap(),
            Request::Fatal(b"invalid integer argument file".to_vec())
        );
    }

    #[test]
    fn zero_and_the_empty_argument_are_too_small() {
        assert_eq!(
            parse_args(&argv(&["-n", "0", "f"])).unwrap(),
            Request::Fatal(b"minimum string length is too small: 0".to_vec())
        );
        assert_eq!(
            parse_args(&argv(&["-n", "", "f"])).unwrap(),
            Request::Fatal(b"minimum string length is too small: ".to_vec())
        );
        assert_eq!(
            parse_args(&argv(&["-0", "f"])).unwrap(),
            Request::Fatal(b"minimum string length is too small: 0".to_vec())
        );
    }

    /// Two different sentences for two adjacent values, as measured. The word
    /// order really does change at the boundary.
    #[test]
    fn the_two_too_big_sentences_differ_at_the_ceiling() {
        assert_eq!(run_options(&["-n", "4294967294", "f"]).min, 4_294_967_294);
        assert_eq!(
            parse_args(&argv(&["-n", "4294967295", "f"])).unwrap(),
            Request::Fatal(b"minimum string length 4294967295 is too big".to_vec())
        );
        assert_eq!(
            parse_args(&argv(&["-n", "4294967296", "f"])).unwrap(),
            Request::Fatal(b"minimum string length is too big: 4294967296".to_vec())
        );
    }

    #[test]
    fn the_digit_shorthand_is_its_whole_word_read_after_the_rest() {
        assert_eq!(run_options(&["-12", "f"]).min, 12);
        assert_eq!(run_options(&["-5", "-n", "3", "f"]).min, 5);
        assert_eq!(run_options(&["-n", "3", "-5", "f"]).min, 5);
        assert_eq!(run_options(&["-1", "-6", "f"]).min, 6);
        // Base 0: a leading 0 is octal.
        assert_eq!(run_options(&["-010", "f"]).min, 8);
        assert_eq!(
            parse_args(&argv(&["-a5", "f"])).unwrap(),
            Request::Fatal(b"invalid integer argument a5".to_vec())
        );
    }

    /// `strtoul` wraps a negative, so `-3` is enormous rather than negative.
    #[test]
    fn a_negative_minimum_length_is_too_big_not_too_small() {
        assert_eq!(
            parse_args(&argv(&["-n", "-3", "f"])).unwrap(),
            Request::Fatal(b"minimum string length is too big: -3".to_vec())
        );
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

    /// `--target` is still refused with a sentence, never silently ignored.
    ///
    /// `-d`/`--data` used to be refused beside it and is now implemented, so
    /// this test lost one of its two subjects rather than being deleted. The
    /// remaining half still earns its keep: an option that asks for something
    /// this build cannot do must say so, because silently ignoring `--target`
    /// would hand a caller output for the wrong architecture and look right.
    #[test]
    fn an_unimplemented_object_file_option_is_refused_rather_than_ignored() {
        for args in [&["-T", "elf64", "f"][..], &["--target", "elf64", "f"]] {
            let e = parse_args(&argv(args)).unwrap_err();
            assert!(
                e.sentence.contains("object-file reader"),
                "for {args:?}: {:?}",
                e.sentence
            );
        }

        // And the one that is no longer refused is accepted, so this test also
        // fails if `-d` is ever quietly put back behind the refusal.
        assert!(run_options(&["-d", "f"]).data_only);
        assert!(run_options(&["--data", "f"]).data_only);
        assert!(!run_options(&["f"]).data_only);
    }

    // ------------------------------------------------------------ -U ---

    #[test]
    fn the_unicode_modes_are_the_ones_the_parser_takes() {
        for (arg, mode) in [
            ("d", Unicode::Default),
            ("default", Unicode::Default),
            ("l", Unicode::Locale),
            ("locale", Unicode::Locale),
            ("e", Unicode::Escape),
            ("escape", Unicode::Escape),
            ("x", Unicode::Hex),
            ("hex", Unicode::Hex),
            ("h", Unicode::Highlight),
            ("highlight", Unicode::Highlight),
            ("i", Unicode::Invalid),
            ("invalid", Unicode::Invalid),
        ] {
            assert_eq!(run_options(&["-U", arg, "f"]).unicode, mode, "{arg}");
        }
        // The help says `show`/`s`; the parser refuses both, as upstream's
        // does, without the usage.
        for bad in ["s", "show", "D", ""] {
            let request = parse_args(&argv(&["-U", bad, "f"])).unwrap();
            let mut want = b"invalid argument to -U/--unicode: ".to_vec();
            want.extend_from_slice(bad.as_bytes());
            assert_eq!(request, Request::Fatal(want), "{bad}");
        }
    }

    #[test]
    fn a_unicode_mode_forces_eight_bit_characters() {
        assert_eq!(run_options(&["-U", "e", "f"]).encoding, Encoding::Ascii8);
        assert_eq!(
            run_options(&["-e", "l", "-U", "x", "f"]).encoding,
            Encoding::Ascii8
        );
        assert_eq!(
            run_options(&["-U", "d", "-e", "l", "f"]).encoding,
            Encoding::Little16
        );
    }

    fn unicode_out(input: &[u8], mode: Unicode, extra: impl Fn(&mut Options)) -> Vec<u8> {
        let mut options = Options {
            unicode: mode,
            encoding: Encoding::Ascii8,
            ..Options::default()
        };
        extra(&mut options);
        let mut out = Vec::new();
        scan_unicode_stream(input, &mut out, None, &options, 0, false).unwrap();
        out
    }

    /// Each measured with GNU strings 2.42 on the same bytes.
    #[test]
    fn each_mode_renders_a_character_as_upstream_does() {
        let input =
            b"abc\xc3\xa9def\0xyz\xe2\x82\xacuvw\0pq\xf0\x9f\x98\x80rs\0ab\xf4\x8f\xbf\xbfcd\n";
        assert_eq!(
            unicode_out(input, Unicode::Escape, |_| {}),
            b"abc\\u00e9def\nxyz\\u20acuvw\npq\\u07c600rs\nab\\u103cfffcd\n"
        );
        assert_eq!(
            unicode_out(input, Unicode::Hex, |_| {}),
            b"abc<0xc3a9>def\nxyz<0xe282ac>uvw\npq<0xf09f9880>rs\nab<0xf48fbfbf>cd\n"
        );
        // `%.1s`: the lead byte alone.
        assert_eq!(
            unicode_out(input, Unicode::Locale, |_| {}),
            b"abc\xc3def\nxyz\xe2uvw\npq\xf0rs\nab\xf4cd\n"
        );
        // Off a terminal, highlighting is escaping.
        assert_eq!(
            unicode_out(input, Unicode::Highlight, |_| {}),
            unicode_out(input, Unicode::Escape, |_| {})
        );
        assert_eq!(unicode_out(input, Unicode::Invalid, |_| {}), b"");
    }

    #[test]
    fn a_character_counts_once_toward_the_minimum() {
        // `\xc3\xa9` is one character, so `a\u00e9b` is three, not four.
        let input = b"a\xc3\xa9b\0";
        assert_eq!(unicode_out(input, Unicode::Escape, |o| o.min = 4), b"");
        assert_eq!(
            unicode_out(input, Unicode::Escape, |o| o.min = 3),
            b"a\\u00e9b\n"
        );
    }

    #[test]
    fn a_broken_sequence_ends_a_run() {
        // A lead byte without its continuation, and a stray continuation.
        assert_eq!(
            unicode_out(b"abcd\xc3Xefgh\0", Unicode::Escape, |_| {}),
            b"abcd\nXefgh\n"
        );
        assert_eq!(
            unicode_out(b"abcd\xa9efgh\0", Unicode::Escape, |_| {}),
            b"abcd\nefgh\n"
        );
    }

    #[test]
    fn valid_utf8_is_upstream_s_lenient_test() {
        assert_eq!(valid_utf8(b"\xc3\xa9"), Some(2));
        assert_eq!(valid_utf8(b"\xe2\x82\xac"), Some(3));
        assert_eq!(valid_utf8(b"\xf0\x9f\x98\x80"), Some(4));
        // Overlong and out-of-range forms pass, as upstream's do.
        assert_eq!(valid_utf8(b"\xc0\x80"), Some(2));
        assert_eq!(valid_utf8(b"\xff\x80\x80\x80"), Some(4));
        assert_eq!(valid_utf8(b"\xc3"), None);
        assert_eq!(valid_utf8(b"\xc3A"), None);
        assert_eq!(valid_utf8(b"\xa9"), None);
        assert_eq!(valid_utf8(b"a"), None);
    }

    // ------------------------------------------------------------ @FILE ---

    #[test]
    fn buildargv_splits_and_quotes_as_libiberty_does() {
        let words = |t: &[u8]| build_argv(t);
        assert_eq!(
            words(b"-n 5 f"),
            [b"-n".to_vec(), b"5".to_vec(), b"f".to_vec()]
        );
        assert_eq!(words(b"  a\t\nb  "), [b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(words(b"'a b' \"c d\""), [b"a b".to_vec(), b"c d".to_vec()]);
        assert_eq!(words(b"x'a b'y"), [b"xa by".to_vec()]);
        assert_eq!(words(b"a\\ b"), [b"a b".to_vec()]);
        assert_eq!(words(b"'it\\'s'"), [b"it's".to_vec()]);
        assert_eq!(words(b"\"\""), [Vec::new()]);
        // An unterminated quote runs to the end of the file.
        assert_eq!(words(b"'a b"), [b"a b".to_vec()]);
    }

    #[test]
    fn an_at_file_becomes_the_words_in_it() {
        let dir = std::env::temp_dir().join(format!("strings-atfile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let opts = dir.join("opts");
        let nested = dir.join("nested");
        let blank = dir.join("blank");
        // Paths written into a file are spelled with `/`: a backslash there
        // is buildargv's escape, and the Windows host's separator is one.
        let spelled = |p: &std::path::Path| {
            p.display()
                .to_string()
                .replace(std::path::MAIN_SEPARATOR, "/")
        };
        std::fs::write(&opts, b"-n 6 '-s' :").unwrap();
        std::fs::write(&blank, b" \n\t ").unwrap();
        std::fs::write(&nested, format!("-f @{}", spelled(&opts))).unwrap();
        let at = |p: &std::path::Path| OsString::from(format!("@{}", p.display()));
        let names = |v: Vec<OsString>| -> Vec<String> {
            v.into_iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };

        let got = expand_at_files(&[at(&opts), OsString::from("f")]).unwrap();
        assert_eq!(names(got), ["-n", "6", "-s", ":", "f"]);
        // A file naming a file is expanded too.
        let got = expand_at_files(&[at(&nested)]).unwrap();
        assert_eq!(names(got), ["-f", "-n", "6", "-s", ":"]);
        // White space alone is nothing at all.
        let got = expand_at_files(&[at(&blank), OsString::from("f")]).unwrap();
        assert_eq!(names(got), ["f"]);
        // A file that is not there leaves the argument as it was.
        let missing = OsString::from(format!("@{}", dir.join("nope").display()));
        assert_eq!(
            expand_at_files(std::slice::from_ref(&missing)).unwrap(),
            [missing]
        );
        // A directory is refused outright.
        assert_eq!(expand_at_files(&[at(&dir)]), Err(AtFileError::Directory));
        // A file that names itself is stopped by the count.
        std::fs::write(&opts, format!("@{}", spelled(&opts))).unwrap();
        assert_eq!(expand_at_files(&[at(&opts)]), Err(AtFileError::TooMany));
        std::fs::remove_dir_all(&dir).unwrap();
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
        let parsed = parse_args(std::slice::from_ref(&name)).unwrap();
        assert_eq!(
            parsed,
            Request::Run(Box::default(), vec![Source::Path(name)])
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
