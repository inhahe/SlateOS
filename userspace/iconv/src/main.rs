//! `iconv` — convert text from one character encoding to another.
//!
//! glibc 2.39's `iconv/iconv_prog.c`, as Ubuntu 24.04 ships it, over the C
//! library's `iconv(3)`: on SlateOS the POSIX layer's, which converts glibc's
//! way (lane D; `known-issues.md` → `B-D-ICONV-HAD-THREE-CHARSETS`), and on the
//! Linux host the differential harness runs on, glibc's own. So
//! `scripts/iconv-diff.sh` measures the program -- its options, its files,
//! what it writes and when, its messages -- against glibc's program over the
//! same library, and lane D measures the library.
//!
//! ```text
//! $ printf 'caf\xe9\n' | iconv -f ISO-8859-1 -t UTF-8
//! café
//! $ printf 'a\377b' | iconv -f UTF-8 -t ASCII
//! aiconv: illegal input sequence at position 1
//! ```
//!
//! Until 2026-10-07 this program converted with tables of its own: ten
//! character sets, its own error handling and its own messages.
//!
//! # Deliberate differences from glibc 2.39's
//!
//! 1. **`--version`** names SlateOS, and **`--help`** ends without Ubuntu's
//!    bug-reporting paragraph, as every port here does.
//! 2. **No charmap files.** Upstream reads a `-f` or `-t` holding a `/` as the
//!    path of a locale charmap (`/usr/share/i18n/charmaps/...`) when that file
//!    can be read. SlateOS has no charmaps, so such a name is a character set
//!    name like any other, and is refused as unsupported -- which is upstream's
//!    answer too when the file is not there.
//! 3. **`-l` lists the names glibc 2.39 lists that this C library opens**, in
//!    glibc's order and spelling (`glibc-2.39-names.txt`), because the C
//!    library publishes no list of its own. Over glibc it is glibc's list;
//!    over SlateOS's it is the part SlateOS can convert.

#![cfg_attr(not(unix), allow(dead_code))]

use std::ffi::OsString;
use std::io;

use coreutils::stdfd;
use coreutils::stdio::StdioFile;
use getoptlong::{Opt, Program, Takes};

// Before `main`, so that `stdfd::restore` still sees the descriptors the
// program was given: a closed standard output is a failure to write, not the
// `/dev/null` Rust's runtime would put there.
coreutils::guard_std_fds!();

/// `argp_err_exit_status`: `EX_USAGE`.
const EX_USAGE: i32 = 64;

/// For getopt's sentences and their status.
const ICONV: Program = Program::new("iconv", EX_USAGE);

/// The short options argp builds from the option table: the program's, then
/// `-?` for help and `-V` for the version.
const SHORTS: &str = "f:t:lco:sV?";

/// The long options, in the order argp gives them to `getopt_long` -- the
/// program's, then argp's own (`help`, `usage`, and the hidden
/// `program-name` and `HANG`), then the version's. The order decides how an
/// ambiguous abbreviation lists its possibilities: `--ver` is `'--verbose'
/// '--version'`.
const LONGS: &[(&str, Takes)] = &[
    ("from-code", Takes::Required),
    ("to-code", Takes::Required),
    ("list", Takes::Nothing),
    ("output", Takes::Required),
    ("silent", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("help", Takes::Nothing),
    ("usage", Takes::Nothing),
    ("program-name", Takes::Required),
    ("HANG", Takes::Optional),
    ("version", Takes::Nothing),
];

/// `OUTBUF_SIZE`: the output buffer `process_block` converts into.
const OUTBUF_SIZE: usize = 32768;

/// The names glibc 2.39's `iconv -l` prints, one a line, in its order and
/// with its trailing slashes. See deliberate difference 3.
const GLIBC_NAMES: &str = include_str!("glibc-2.39-names.txt");

/// glibc's error numbers, which SlateOS's POSIX layer shares.
const E2BIG: i32 = 7;
const EBADF: i32 = 9;
const EINVAL: i32 = 22;
const EILSEQ: i32 = 84;

/// `LC_ALL` and `CODESET`, as glibc numbers them and the POSIX layer does.
const LC_ALL: i32 = 6;
const CODESET: i32 = 14;

// ---------------------------------------------------------------------------
// The command line
// ---------------------------------------------------------------------------

/// What the command line asked for.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Config {
    from_code: Vec<u8>,
    to_code: Vec<u8>,
    output_file: Option<Vec<u8>>,
    list: bool,
    omit_invalid: bool,
    verbose: bool,
    files: Vec<OsString>,
}

/// The program's names. getopt prefixes its sentences with `argv[0]` itself,
/// which nothing changes. `error()` uses `program_invocation_name`, the whole
/// `argv[0]` to start with, and argp its last part; `--program-name` changes
/// those two and not the first.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Names {
    argv0: Vec<u8>,
    full: Vec<u8>,
    short: Vec<u8>,
}

impl Names {
    fn from_argv0(argv0: &[u8]) -> Self {
        Self {
            argv0: argv0.to_vec(),
            full: argv0.to_vec(),
            short: base_name(argv0).to_vec(),
        }
    }
}

/// `__argp_base_name`: what follows the last `/`.
fn base_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b'/') {
        Some(at) => name.get(at.saturating_add(1)..).unwrap_or_default(),
        None => name,
    }
}

/// How the command line ended the run, if it did. Bytes, because the
/// program's name is `argv[0]`'s, which need not be text.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stop {
    /// `--help`, `--usage` or `--version`: this text on standard output,
    /// status 0.
    Print(Vec<u8>),
    /// A getopt error: this text on standard error, `EX_USAGE`.
    Usage(Vec<u8>),
}

/// argp's walk over the options: each acted on as it is met, so `--help`
/// before a bad option prints the help, and after one never runs.
fn parse(args: &[OsString], names: &mut Names) -> Result<Config, Stop> {
    let mut cfg = Config::default();
    for item in ICONV.parse(args, SHORTS, LONGS) {
        let item = match item {
            Ok(item) => item,
            Err(e) => {
                return Err(Stop::Usage(
                    [
                        &names.argv0,
                        b": ".as_slice(),
                        e.sentence.as_bytes(),
                        b"\n",
                        &try_help(&names.short),
                    ]
                    .concat(),
                ));
            }
        };
        let (key, value): (&str, Option<&OsString>) = match &item {
            Opt::Operand(word) => {
                cfg.files.push((*word).clone());
                continue;
            }
            Opt::Short(c, v) => (short_key(*c), v.as_ref()),
            Opt::Long(name, v) => (*name, v.as_ref()),
        };
        let value = value.map(|v| quoting::os_bytes(v).into_owned());
        match key {
            "from-code" => cfg.from_code = value.unwrap_or_default(),
            "to-code" => cfg.to_code = value.unwrap_or_default(),
            "output" => cfg.output_file = value,
            "list" => cfg.list = true,
            "omit" => cfg.omit_invalid = true,
            "verbose" => cfg.verbose = true,
            "help" => return Err(Stop::Print(help_text(&names.short))),
            "usage" => return Err(Stop::Print(usage_text(&names.short))),
            "version" => return Err(Stop::Print(version_text())),
            "program-name" => {
                let name = value.unwrap_or_default();
                names.short = base_name(&name).to_vec();
                names.full = name;
            }
            "HANG" => hang(value.as_deref()),
            // `-s`: "Nothing, for now at least."
            _ => {}
        }
    }
    Ok(cfg)
}

/// The key a short option stands for.
fn short_key(c: u8) -> &'static str {
    match c {
        b'f' => "from-code",
        b't' => "to-code",
        b'l' => "list",
        b'c' => "omit",
        b'o' => "output",
        b's' => "silent",
        b'V' => "version",
        b'?' => "help",
        _ => "",
    }
}

/// argp's `--HANG[=SECS]`: sleep `atoi (SECS)` seconds, an hour by default --
/// a debugging hook, hidden from `--help`, kept because it is accepted.
fn hang(secs: Option<&[u8]>) {
    let n = secs.map_or(3600, atoi);
    for _ in 0..n.max(0) {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// C's `atoi`: blanks, a sign, digits, and 0 for anything else.
fn atoi(text: &[u8]) -> i64 {
    let mut rest = text
        .iter()
        .skip_while(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'))
        .peekable();
    let negative = match rest.peek() {
        Some(b'-') => {
            rest.next();
            true
        }
        Some(b'+') => {
            rest.next();
            false
        }
        _ => false,
    };
    let mut value: i64 = 0;
    for &b in rest.take_while(|b| b.is_ascii_digit()) {
        value = value
            .saturating_mul(10)
            .saturating_add(i64::from(b.wrapping_sub(b'0')));
    }
    if negative {
        value.saturating_neg()
    } else {
        value
    }
}

/// argp's `ARGP_HELP_SEE`.
fn try_help(short: &[u8]) -> Vec<u8> {
    [
        b"Try `".as_slice(),
        short,
        b" --help' or `",
        short,
        b" --usage' for more information.",
    ]
    .concat()
}

/// What follows `Usage: NAME` in argp's help for this program, as Ubuntu's
/// prints it, less the bug-reporting paragraph (deliberate difference 1).
const HELP_BODY: &str = " [OPTION...] [FILE...]\n\
     Convert encoding of given files from one encoding to another.\n\
     \n\
     \x20Input/Output format specification:\n\
     \x20 -f, --from-code=NAME       encoding of original text\n\
     \x20 -t, --to-code=NAME         encoding for output\n\
     \n\
     \x20Information:\n\
     \x20 -l, --list                 list all known coded character sets\n\
     \n\
     \x20Output control:\n\
     \x20 -c                         omit invalid characters from output\n\
     \x20 -o, --output=FILE          output file\n\
     \x20 -s, --silent               suppress warnings\n\
     \x20     --verbose              print progress information\n\
     \n\
     \x20 -?, --help                 Give this help list\n\
     \x20     --usage                Give a short usage message\n\
     \x20 -V, --version              Print program version\n\
     \n\
     Mandatory or optional arguments to long options are also mandatory or optional\n\
     for any corresponding short options.\n";

/// argp's help for this program.
fn help_text(short: &[u8]) -> Vec<u8> {
    [b"Usage: ".as_slice(), short, HELP_BODY.as_bytes()].concat()
}

/// argp's `--usage`: the options packed into lines under `Usage: NAME `.
fn usage_text(short: &[u8]) -> Vec<u8> {
    let lead = [b"Usage: ".as_slice(), short, b" "].concat();
    let pieces = [
        "[-lcs?V]",
        "[-f NAME]",
        "[-t NAME]",
        "[-o FILE]",
        "[--from-code=NAME]",
        "[--to-code=NAME]",
        "[--list]",
        "[--output=FILE]",
        "[--silent]",
        "[--verbose]",
        "[--help]",
        "[--usage]",
        "[--version]",
        "[FILE...]",
    ];
    // argp's line wrapping: the right margin is 79, and a continuation line is
    // indented by `usage_indent`, 12 -- not by the length of `Usage: NAME `,
    // which is 13 for `iconv`.
    let indent: &[u8] = b"            ";
    let mut out = lead.clone();
    let mut column = lead.len();
    let mut first = true;
    for piece in pieces {
        let needed = if first {
            piece.len()
        } else {
            piece.len().saturating_add(1)
        };
        if !first && column.saturating_add(needed) > 79 {
            out.push(b'\n');
            out.extend_from_slice(indent);
            column = indent.len();
            first = true;
        }
        if !first {
            out.push(b' ');
            column = column.saturating_add(1);
        }
        out.extend_from_slice(piece.as_bytes());
        column = column.saturating_add(piece.len());
        first = false;
    }
    out.push(b'\n');
    out
}

/// `print_version`, naming SlateOS (deliberate difference 1).
fn version_text() -> Vec<u8> {
    b"iconv (SlateOS userspace) 0.1.0\n\
     Copyright (C) 2024 Free Software Foundation, Inc.\n\
     This is free software; see the source for copying conditions.  There is NO\n\
     warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.\n\
     Written by Ulrich Drepper.\n"
        .to_vec()
}

// ---------------------------------------------------------------------------
// The C library's iconv
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod lib {
    //! The four calls this program makes of the C library, behind safe
    //! wrappers. `iconv_t` is a pointer-sized handle on both libraries.

    use std::ffi::{CStr, CString};

    unsafe extern "C" {
        fn iconv_open(tocode: *const u8, fromcode: *const u8) -> isize;
        fn iconv(
            cd: isize,
            inbuf: *mut *mut u8,
            inbytesleft: *mut usize,
            outbuf: *mut *mut u8,
            outbytesleft: *mut usize,
        ) -> usize;
        fn iconv_close(cd: isize) -> i32;
        fn setlocale(category: i32, locale: *const u8) -> *const u8;
        fn nl_langinfo(item: i32) -> *const u8;
    }

    /// An open conversion, closed when dropped.
    pub struct Cd(isize);

    impl Cd {
        /// `iconv_open (to, from)`, or the `errno` it failed with.
        pub fn open(to: &CStr, from: &CStr) -> Result<Self, i32> {
            // SAFETY: both are NUL-terminated strings that outlive the call.
            let cd = unsafe { iconv_open(to.as_ptr().cast(), from.as_ptr().cast()) };
            if cd == -1 { Err(errno()) } else { Ok(Self(cd)) }
        }

        /// One `iconv` call over `input[*at..]` into `out`: how far each got,
        /// and its result -- `Err(errno)` for `(size_t) -1`. `input` of `None`
        /// is the flush call, `iconv (cd, NULL, NULL, ...)`.
        pub fn convert(
            &mut self,
            input: Option<(&[u8], &mut usize)>,
            out: &mut [u8],
        ) -> (usize, Result<usize, i32>) {
            let mut outptr = out.as_mut_ptr();
            let mut outleft = out.len();
            let n = match input {
                Some((data, at)) => {
                    let rest = data.get(*at..).unwrap_or_default();
                    // `iconv` takes `char **` but does not write through the
                    // input pointer's target, so a pointer into a shared slice
                    // is sound here.
                    let mut inptr = rest.as_ptr().cast_mut();
                    let mut inleft = rest.len();
                    // SAFETY: `inptr` and `inleft` describe `rest`, `outptr`
                    // and `outleft` describe `out`; the library advances
                    // each pointer by what it consumed or produced, within
                    // those bounds.
                    let n = unsafe {
                        iconv(
                            self.0,
                            &raw mut inptr,
                            &raw mut inleft,
                            &raw mut outptr,
                            &raw mut outleft,
                        )
                    };
                    *at = at.saturating_add(rest.len().saturating_sub(inleft));
                    n
                }
                None => {
                    // SAFETY: as above, with no input: the reset call.
                    unsafe {
                        iconv(
                            self.0,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            &raw mut outptr,
                            &raw mut outleft,
                        )
                    }
                }
            };
            let produced = out.len().saturating_sub(outleft);
            if n == usize::MAX {
                (produced, Err(errno()))
            } else {
                (produced, Ok(n))
            }
        }
    }

    impl Drop for Cd {
        fn drop(&mut self) {
            // SAFETY: `self.0` is a handle `iconv_open` returned and nothing
            // has closed. Unchecked, as upstream's `iconv_close` is: the
            // conversion is over either way.
            unsafe { iconv_close(self.0) };
        }
    }

    /// Whether `iconv_open (to, from)` succeeds.
    pub fn opens(to: &CStr, from: &CStr) -> bool {
        Cd::open(to, from).is_ok()
    }

    /// `setlocale (LC_ALL, "")`.
    pub fn set_locale_from_environment() {
        // SAFETY: a NUL-terminated empty string; the result is not kept.
        unsafe { setlocale(super::LC_ALL, c"".as_ptr().cast()) };
    }

    /// `nl_langinfo (CODESET)`: the locale's character set.
    pub fn codeset() -> Vec<u8> {
        // SAFETY: `nl_langinfo` returns a NUL-terminated string the library
        // keeps until the next call that changes the locale; it is copied out
        // at once.
        let ptr = unsafe { nl_langinfo(super::CODESET) };
        if ptr.is_null() {
            return Vec::new();
        }
        // SAFETY: as above.
        unsafe { CStr::from_ptr(ptr.cast()) }.to_bytes().to_vec()
    }

    fn errno() -> i32 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
    }

    /// `s` as a C string, cut at a NUL as C would read it.
    pub fn c(s: &[u8]) -> CString {
        let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
        CString::new(s.get(..end).unwrap_or_default()).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Where the converted text goes: opened when there is first something to
/// write, as upstream's `write_output` opens it, so an input that converts to
/// nothing creates no output file.
struct Output {
    path: Option<Vec<u8>>,
    file: Option<StdioFile>,
}

impl Output {
    /// Whether the text goes to standard output: no `-o`, or `-o -`.
    fn is_stdout(&self) -> bool {
        !matches!(self.path.as_deref(), Some(path) if path != b"-")
    }

    /// The stdio stream `stdout`, if the text is going there and it has been
    /// opened -- what `error()` flushes. See [`error`].
    fn stdout(&mut self) -> Option<&mut StdioFile> {
        if self.is_stdout() {
            self.file.as_mut()
        } else {
            None
        }
    }

    /// `write_output`. `Err` stops the conversion; the message is said here,
    /// and a failure to open the file ends the run.
    fn write(&mut self, data: &[u8], names: &Names) -> Result<(), ()> {
        if self.file.is_none() {
            self.file = Some(match self.path.as_deref() {
                Some(path) if path != b"-" => {
                    match std::fs::File::create(std::path::Path::new(&quoting::os_from_bytes(path)))
                    {
                        Ok(file) => StdioFile::from_file(file),
                        Err(e) => die(names, &with_reason(b"cannot open output file", &e)),
                    }
                }
                _ => StdioFile::stdout(),
            });
        }
        let Some(file) = self.file.as_mut() else {
            return Err(());
        };
        if file.write(data).is_err() || file.has_error() {
            error(
                names,
                b"conversion stopped due to problem in writing the output",
                self.stdout(),
            );
            return Err(());
        }
        Ok(())
    }

    /// The `fclose` at the end, if anything was written.
    fn close(&mut self, names: &Names) {
        if let Some(mut file) = self.file.take()
            && let Err(e) = file.close()
        {
            die(names, &with_reason(b"error while closing output file", &e));
        }
    }
}

/// glibc's `error (0, ...)`: standard output flushed, then `NAME: MESSAGE` on
/// standard error, where `NAME` is the whole `argv[0]`.
///
/// `stdout` is the stdio stream `stdout` when the converted text is going
/// there, and `None` otherwise: `error()` flushes `stdout` and nothing else,
/// so the text going to an `-o` file waits in its own buffer for `fclose`, and
/// a failure to write it is reported there. A diagnostic that cannot be
/// written is not reported -- upstream has no `close_stdout`.
fn error(names: &Names, message: &[u8], stdout: Option<&mut StdioFile>) {
    if let Some(out) = stdout {
        // Unchecked here as in `error()`'s `fflush (stdout)`: a failure is
        // the stream's, and is what the close at the end reports.
        let _ = out.flush();
    }
    let line = [&names.full, b": ".as_slice(), message, b"\n"].concat();
    // Unchecked: see above.
    let _ = stdfd::write_all(2, &line);
}

/// `error (EXIT_FAILURE, ...)`, from a point where nothing is waiting to be
/// written to standard output.
fn die(names: &Names, message: &[u8]) -> ! {
    error(names, message, None);
    std::process::exit(1);
}

/// `MESSAGE: REASON`, as `error (..., errno, MESSAGE)` prints it.
fn with_reason(message: &[u8], e: &io::Error) -> Vec<u8> {
    [message, b": ", errmsg::strerror(e).as_bytes()].concat()
}

// ---------------------------------------------------------------------------
// Converting
// ---------------------------------------------------------------------------

/// `process_block`: convert `data` and write it. 0 for success, 1 when
/// characters were omitted (`-c`) and nothing else went wrong, -1 when the
/// conversion stopped and no further file should be tried.
///
/// The `ret = 1` an omission sets is overwritten by the next successful write,
/// as upstream's is, so `-c` usually ends with status 0. Measured, and kept.
#[cfg(unix)]
fn process_block(
    cd: &mut lib::Cd,
    data: &[u8],
    omit_invalid: bool,
    output: &mut Output,
    names: &Names,
) -> i32 {
    let mut at = 0usize;
    let mut ret = 0;
    let mut outbuf = vec![0u8; OUTBUF_SIZE];
    while at < data.len() {
        let (produced, mut result) = cd.convert(Some((data, &mut at)), &mut outbuf);
        if result == Err(EILSEQ) && omit_invalid {
            ret = 1;
            result = if at == data.len() { Ok(0) } else { Err(E2BIG) };
        }
        if produced > 0 {
            ret = match output.write(outbuf.get(..produced).unwrap_or_default(), names) {
                Ok(()) => 0,
                Err(()) => -1,
            };
            if ret != 0 {
                break;
            }
        }
        let errno = match result {
            Ok(_) => {
                // All of the input is converted: flush a stateful set's state.
                let (produced, flushed) = cd.convert(None, &mut outbuf);
                if produced > 0 {
                    ret = match output.write(outbuf.get(..produced).unwrap_or_default(), names) {
                        Ok(()) => 0,
                        Err(()) => -1,
                    };
                    if ret != 0 {
                        break;
                    }
                }
                match flushed {
                    Ok(_) => break,
                    Err(EILSEQ) if omit_invalid => {
                        ret = 1;
                        break;
                    }
                    Err(e) => e,
                }
            }
            Err(e) => e,
        };
        if errno != E2BIG {
            let message = match errno {
                EILSEQ => {
                    (!omit_invalid).then(|| format!("illegal input sequence at position {at}"))
                }
                EINVAL => {
                    Some("incomplete character or shift sequence at end of buffer".to_string())
                }
                EBADF => Some("internal error (illegal descriptor)".to_string()),
                other => Some(format!("unknown iconv() error {other}")),
            };
            if let Some(message) = message {
                error(names, message.as_bytes(), output.stdout());
            }
            return -1;
        }
    }
    ret
}

/// `process_fd`: all of the descriptor's input, then [`process_block`] over
/// it -- the whole of it at once, because a block boundary could fall inside
/// a character.
#[cfg(unix)]
fn process_fd(
    cd: &mut lib::Cd,
    mut input: impl io::Read,
    omit_invalid: bool,
    output: &mut Output,
    names: &Names,
) -> i32 {
    let mut data = Vec::new();
    let mut chunk = vec![0u8; 32768];
    loop {
        match input.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(chunk.get(..n).unwrap_or_default()),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                let message = with_reason(b"error while reading the input", &e);
                error(names, &message, output.stdout());
                return -1;
            }
        }
    }
    process_block(cd, &data, omit_invalid, output, names)
}

// ---------------------------------------------------------------------------
// -l
// ---------------------------------------------------------------------------

/// `strverscmp`'s order would be glibc's; the embedded list is already in it.
/// What is printed is each listed name this library opens, in either
/// direction (deliberate difference 3).
#[cfg(unix)]
fn known_names() -> Vec<&'static str> {
    let utf8 = lib::c(b"UTF-8");
    GLIBC_NAMES
        .lines()
        .filter(|line| !line.is_empty())
        .filter(|line| {
            let name = lib::c(line.trim_end_matches('/').as_bytes());
            lib::opens(&utf8, &name) || lib::opens(&name, &utf8)
        })
        .collect()
}

/// `print_known_names`: one name a line, or, on a terminal, the header and the
/// names in a filled paragraph.
fn names_text(names: &[&str], human: bool) -> String {
    if !human {
        let mut out = String::new();
        for name in names {
            out.push_str(name);
            out.push('\n');
        }
        return out;
    }
    let mut out = String::from(
        "The following list contains all the coded character sets known.  This does\n\
         not necessarily mean that all combinations of these names can be used for\n\
         the FROM and TO command line parameters.  One coded character set can be\n\
         listed with several different names (aliases).\n\n  ",
    );
    let mut column = 2usize;
    let mut first = true;
    for name in names {
        let name = name.trim_end_matches('/');
        // A name with no letter or digit in it is not printed (`do_print_human`).
        if !name.bytes().any(|b| b.is_ascii_alphanumeric()) {
            continue;
        }
        if first {
            first = false;
        } else {
            out.push(',');
            column = column.saturating_add(1);
            if column > 2 && column.saturating_add(name.len()) > 77 {
                out.push_str("\n  ");
                column = 2;
            } else {
                out.push(' ');
                column = column.saturating_add(1);
            }
        }
        out.push_str(name);
        column = column.saturating_add(name.len());
    }
    if column != 0 {
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let argv0 = argv
        .first()
        .map(|a| quoting::os_bytes(a).into_owned())
        .unwrap_or_default();
    let mut names = Names::from_argv0(&argv0);
    lib::set_locale_from_environment();

    let cfg = match parse(argv.get(1..).unwrap_or_default(), &mut names) {
        Ok(cfg) => cfg,
        Err(Stop::Print(text)) => {
            let mut out = StdioFile::stdout();
            // Unchecked, as argp's own output is: it exits 0 through `exit`,
            // which flushes without looking.
            let _ = out.write(&text);
            let _ = out.flush();
            return std::process::ExitCode::SUCCESS;
        }
        Err(Stop::Usage(text)) => {
            // Unchecked, as argp's `fprintf (stderr, ...)` is.
            let _ = stdfd::write_all(2, &[text.as_slice(), b"\n"].concat());
            return std::process::ExitCode::from(u8::try_from(EX_USAGE).unwrap_or(1));
        }
    };

    if cfg.list {
        let human = std::io::IsTerminal::is_terminal(&io::stdout());
        let text = names_text(&known_names(), human);
        let mut out = StdioFile::stdout();
        // Unchecked, as upstream's `puts` are: it exits 0 regardless.
        let _ = out.write(text.as_bytes());
        let _ = out.flush();
        return std::process::ExitCode::SUCCESS;
    }

    std::process::ExitCode::from(convert(&cfg, &names))
}

/// Everything after the options: open the conversion, convert each file,
/// close the output. The exit status.
#[cfg(unix)]
fn convert(cfg: &Config, names: &Names) -> u8 {
    let mut to = cfg.to_code.clone();
    if cfg.omit_invalid {
        // `conv_spec.ignore = true`, as `iconv_open` spells it.
        to.extend_from_slice(b"//IGNORE");
    }
    let mut cd = match lib::Cd::open(&lib::c(&to), &lib::c(&cfg.from_code)) {
        Ok(cd) => cd,
        Err(EINVAL) => unsupported(cfg, names),
        Err(e) => die(
            names,
            &with_reason(
                b"failed to start conversion processing",
                &io::Error::from_raw_os_error(e),
            ),
        ),
    };

    let mut output = Output {
        path: cfg.output_file.clone(),
        file: None,
    };
    let mut status = 0u8;
    if cfg.files.is_empty() {
        if process_fd(
            &mut cd,
            stdfd::RawStdin,
            cfg.omit_invalid,
            &mut output,
            names,
        ) != 0
        {
            status = 1;
        }
    } else {
        for file in &cfg.files {
            let shown = quoting::os_bytes(file);
            if cfg.verbose {
                let mut line = shown.to_vec();
                line.extend_from_slice(b":\n");
                // `fprintf (stderr, ...)`, unchecked.
                let _ = stdfd::write_all(2, &line);
            }
            let ret = if shown.as_ref() == b"-" {
                let ret = process_fd(
                    &mut cd,
                    stdfd::RawStdin,
                    cfg.omit_invalid,
                    &mut output,
                    names,
                );
                // Upstream closes the descriptor it read, standard input
                // included, so a second `-` reads a closed descriptor and
                // fails with `EBADF` rather than finding nothing. Unchecked,
                // as upstream's `close (fd)` is.
                let _ = stdfd::close_descriptor(0);
                ret
            } else {
                match std::fs::File::open(file) {
                    Ok(f) => process_fd(&mut cd, f, cfg.omit_invalid, &mut output, names),
                    Err(e) => {
                        // The name as it was given, byte for byte: upstream
                        // prints it with `%s`, unquoted.
                        let message =
                            with_reason(&[b"cannot open input file `", &*shown, b"'"].concat(), &e);
                        error(names, &message, output.stdout());
                        status = 1;
                        continue;
                    }
                }
            };
            if ret != 0 {
                status = 1;
                if ret < 0 {
                    break;
                }
            }
        }
    }
    output.close(names);
    status
}

/// Which name `iconv_open` refused, told the way upstream tells it, then
/// argp's referral; status 1.
#[cfg(unix)]
fn unsupported(cfg: &Config, names: &Names) -> ! {
    let utf8 = lib::c(b"UTF-8");
    // "Try to be nice with the user and tell her which of the two encoding
    // names is wrong": the one that cannot be converted to or from UTF-8,
    // by `EINVAL` -- any other failure blames neither.
    let from_wrong = matches!(lib::Cd::open(&utf8, &lib::c(&cfg.from_code)), Err(EINVAL));
    let to_wrong = matches!(lib::Cd::open(&lib::c(&cfg.to_code), &utf8), Err(EINVAL));
    // An empty name is the locale's character set, and is called that. The
    // names are printed as given, byte for byte, with `%s`.
    let pretty = |code: &[u8]| -> Vec<u8> {
        if code.is_empty() {
            lib::codeset()
        } else {
            code.to_vec()
        }
    };
    let (from, to) = (pretty(&cfg.from_code), pretty(&cfg.to_code));
    let message: Vec<u8> = match (from_wrong, to_wrong) {
        (true, true) => [
            b"conversions from `".as_slice(),
            &from,
            b"' and to `",
            &to,
            b"' are not supported",
        ]
        .concat(),
        (true, false) => [
            b"conversion from `".as_slice(),
            &from,
            b"' is not supported",
        ]
        .concat(),
        (false, true) => [b"conversion to `".as_slice(), &to, b"' is not supported"].concat(),
        (false, false) => [
            b"conversion from `".as_slice(),
            &from,
            b"' to `",
            &to,
            b"' is not supported",
        ]
        .concat(),
    };
    error(names, &message, None);
    let mut referral = try_help(&names.short);
    referral.push(b'\n');
    let _ = stdfd::write_all(2, &referral);
    std::process::exit(1);
}

/// The development host has no `iconv(3)`; the program is SlateOS's and
/// Linux's.
#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    stdfd::restore();
    stdfd::diag_line("iconv: not supported on this host");
    std::process::ExitCode::FAILURE
}

#[cfg(test)]
mod tests;
