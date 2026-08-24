//! echo — write arguments to standard output.
//!
//! A port of GNU coreutils 9.4's `src/echo.c`, read rather than recalled.
//!
//! `echo` looks like the one utility too small to get wrong, and it is the one
//! that had the most rules missing: `-E`, `\c`, `\e`, `\v`, `\xHH`, octal
//! escapes of more than one digit, `--help`, `--version`, `POSIXLY_CORRECT`,
//! and any argument holding a byte that is not valid UTF-8 — which this OS
//! allows everywhere but `/` and NUL, and which `echo` is precisely the tool
//! you would reach for to emit.
//!
//! # Three rules that are not obvious from the manual
//!
//! 1. **`POSIXLY_CORRECT` turns escapes on and options off.** The condition
//!    upstream tests is `do_v9 || posixly_correct`, so with that variable set
//!    `echo 'a\tb'` writes a tab *without* `-e`, and `-E` cannot turn it back
//!    off. In the same mode `allow_options` is false, so `-e` is not an option
//!    any more — it is a word to print. The one exception is a first argument
//!    of exactly `-n`, which keeps option parsing alive for the whole command
//!    line.
//! 2. **`\c` ends the program, not the argument.** It stops output there and
//!    then, skips every later argument, and suppresses the trailing newline
//!    whatever `-n` said.
//! 3. **An octal escape does not need the leading zero.** `\0NNN` is what
//!    `--help` documents, but the switch has arms for `'1'` through `'7'` as
//!    well, so `\101` is `A`. Each form takes at most three octal digits in
//!    total and wraps at 256, so `\0777` is `\xFF`.
//!
//! # Options are matched, not parsed
//!
//! Upstream does not call `getopt_long`, and says why: *"We directly parse
//! options, rather than use parse_long_options, in order to avoid accepting
//! abbreviations."* `echo --he` prints `--he`. `--help` and `--version` are
//! recognised only when they are the *entire* command line, so `echo --help x`
//! prints `--help x`. A short-option word is taken only if every character
//! after the `-` is one of `e`, `E`, `n`; anything else — including a bare `-`
//! — is text, and the scan stops there. That is why `echo -n -x` prints `-x`
//! with no newline while `echo -x -n` prints `-x -n` with one.
//!
//! Within and across those words the last of `-e`/`-E` wins, which is what
//! makes `echo -e -E 'a\tb'` differ from `echo -E -e 'a\tb'`.
//!
//! # How this is tested
//!
//! `scripts/echo-diff.sh` builds this file for Linux inside WSL and runs it
//! against GNU coreutils case by case — the same answer `cmp`, `tee`, `du`,
//! `find` and `ls` use (`design-decisions.md` §374). The unit tests below
//! cover the parser and the escape decoder as pure functions over bytes.

use coreutils::quote::os_bytes;
use coreutils::stdfd::{self, Stream};
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

// Before `main`, so that a caller's `echo >&-` is still a closed descriptor
// when `finish` writes to it. See `coreutils::stdfd`.
coreutils::guard_std_fds!();

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Write(Settings),
}

#[derive(Debug, Default, PartialEq, Eq, Clone)]
struct Settings {
    /// `-n`: suppress the trailing newline.
    no_newline: bool,
    /// Upstream's `do_v9 || posixly_correct`, already resolved.
    escapes: bool,
    /// Index into the original argv of the first word to print.
    first_text: usize,
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    // Presence, not value: upstream is `!!getenv ("POSIXLY_CORRECT")`, so
    // `POSIXLY_CORRECT=` (set to nothing) still counts.
    let posix = std::env::var_os("POSIXLY_CORRECT").is_some();

    match parse_args(&args, posix) {
        Request::Help => finish(help_text().as_bytes()),
        Request::Version => finish(b"echo (SlateOS coreutils) 0.1.0\n"),
        Request::Write(set) => finish(&render(&args, &set)),
    }
}

/// Write the whole of the output, once, and report a failure the way
/// `close_stdout` does.
///
/// Upstream reaches the same place by a different route: it `putchar`s into a
/// buffered stream and registers `close_stdout` with `atexit`, so a full disk
/// is diagnosed once, at exit, after the last character is nominally written.
/// Building the bytes first and writing them once has the same two observable
/// properties — one diagnostic, and a status of 1 — and makes `\c` (which ends
/// the program early) need no special handling: it has already contributed
/// whatever it contributed.
///
/// The write goes through [`Stream`] rather than `io::stdout()` because the
/// status depends on it arriving, and std's stdout will not say when it did
/// not: `StdoutRaw`'s `Write` impl maps `EBADF` to a full success. Buffering
/// matters here for the same reason it does upstream — `echo -n >&-` owes the
/// descriptor nothing and exits 0, while `echo >&-` owes it a newline and
/// exits 1.
fn finish(bytes: &[u8]) -> ExitCode {
    let mut out = Stream::stdout();
    // `Stream::write_all` records rather than returns; the verdict is `finish`.
    let _ = out.write_all(bytes);
    if let Err(e) = out.finish() {
        // Not silence: `echo hi > /dev/full` that exits 0 is a program telling
        // a script the write happened.
        stdfd::write_error("echo", &e);
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Upstream's option scan, arm for arm.
fn parse_args(args: &[OsString], posixly_correct: bool) -> Request {
    // `! posixly_correct || (! DEFAULT_ECHO_TO_XPG && 1 < argc && STREQ (argv[1], "-n"))`.
    // `DEFAULT_ECHO_TO_XPG` is false everywhere but System V, this included.
    let allow_options =
        !posixly_correct || args.first().is_some_and(|a| os_bytes(a).as_ref() == b"-n");

    // `argc == 2`: the long options exist only as the whole command line.
    if allow_options && args.len() == 1 {
        match os_bytes(&args[0]).as_ref() {
            b"--help" => return Request::Help,
            b"--version" => return Request::Version,
            _ => {}
        }
    }

    let mut set = Settings::default();
    // `bool do_v9 = DEFAULT_ECHO_TO_XPG;` — false everywhere but System V.
    // Kept separate from `set.escapes` because `-E` clears *this*, and the
    // printing loop is entered on `do_v9 || posixly_correct`: in POSIX mode
    // `-E` therefore cannot switch escapes back off.
    let mut do_v9 = false;

    if allow_options {
        for arg in args {
            let bytes = os_bytes(arg);
            let Some(tail) = bytes.strip_prefix(b"-") else {
                break;
            };
            // A bare `-` is text (`if (i == 0) goto just_echo;`), and so is any
            // word with one character outside the set — `-en1` prints as-is.
            if tail.is_empty() || !tail.iter().all(|c| matches!(c, b'e' | b'E' | b'n')) {
                break;
            }
            for c in tail {
                match c {
                    b'e' => do_v9 = true,
                    b'E' => do_v9 = false,
                    _ => set.no_newline = true,
                }
            }
            set.first_text = set.first_text.saturating_add(1);
        }
    }

    set.escapes = do_v9 || posixly_correct;
    Request::Write(set)
}

/// Everything `echo` writes, as bytes: the words, one space between them, and
/// the newline unless something suppressed it.
fn render(args: &[OsString], set: &Settings) -> Vec<u8> {
    let mut out = Vec::new();
    let words = args.get(set.first_text..).unwrap_or(&[]);

    for (i, arg) in words.iter().enumerate() {
        if i > 0 {
            out.push(b' ');
        }
        let bytes = os_bytes(arg);
        if set.escapes {
            if !expand(&bytes, &mut out) {
                // `case 'c': return EXIT_SUCCESS;` — before the trailing
                // newline, and before any remaining argument.
                return out;
            }
        } else {
            out.extend_from_slice(&bytes);
        }
    }

    if !set.no_newline {
        out.push(b'\n');
    }
    out
}

/// Decode one argument's backslash escapes into `out`.
///
/// Returns `false` if `\c` was reached, which ends the whole program rather
/// than this argument.
fn expand(arg: &[u8], out: &mut Vec<u8>) -> bool {
    let mut i = 0usize;
    while let Some(&c) = arg.get(i) {
        i = i.saturating_add(1);
        // `if (c == '\\' && *s)` — a backslash at the very end is a backslash.
        if c != b'\\' || i >= arg.len() {
            out.push(c);
            continue;
        }
        let Some(&esc) = arg.get(i) else {
            out.push(c);
            continue;
        };
        i = i.saturating_add(1);
        let byte = match esc {
            b'a' => 0x07,
            b'b' => 0x08,
            b'c' => return false,
            b'e' => 0x1B,
            b'f' => 0x0C,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 0x0B,
            b'\\' => b'\\',
            b'x' => match hex_escape(arg, &mut i) {
                Some(v) => v,
                // `goto not_an_escape`: the backslash is printed and so is the
                // `x`, exactly as for an unknown escape.
                None => {
                    out.push(b'\\');
                    b'x'
                }
            },
            b'0'..=b'7' => octal_escape(arg, &mut i, esc),
            // `default: putchar ('\\');` then the character itself.
            other => {
                out.push(b'\\');
                other
            }
        };
        out.push(byte);
    }
    true
}

/// `\xHH`: one or two hex digits, or nothing at all — in which case the
/// sequence is not an escape.
fn hex_escape(arg: &[u8], i: &mut usize) -> Option<u8> {
    let first = arg.get(*i).copied().filter(u8::is_ascii_hexdigit)?;
    *i = i.saturating_add(1);
    let mut value = hextobin(first);
    if let Some(second) = arg.get(*i).copied().filter(u8::is_ascii_hexdigit) {
        *i = i.saturating_add(1);
        // Two hex digits cannot overflow a byte, but the octal form can, and
        // both are `unsigned char` arithmetic upstream. Wrapping, not
        // saturating: `\0777` is 0xFF because 511 wraps, not because it clamps.
        value = value.wrapping_mul(16).wrapping_add(hextobin(second));
    }
    Some(value)
}

/// `\0NNN` *and* `\NNN`: up to three octal digits in total, counting the digit
/// that opened the escape unless that digit was the `0` of `\0`.
fn octal_escape(arg: &[u8], i: &mut usize, first: u8) -> u8 {
    let mut value;
    let mut remaining = 2usize;

    if first == b'0' {
        match arg.get(*i).copied().filter(is_octal) {
            // `c = 0; if (! ('0' <= *s && *s <= '7')) break;` — a lone `\0` is
            // a NUL byte and consumes nothing further.
            None => return 0,
            Some(d) => {
                *i = i.saturating_add(1);
                value = d.wrapping_sub(b'0');
            }
        }
    } else {
        value = first.wrapping_sub(b'0');
    }

    while remaining > 0 {
        let Some(d) = arg.get(*i).copied().filter(is_octal) else {
            break;
        };
        *i = i.saturating_add(1);
        value = value.wrapping_mul(8).wrapping_add(d.wrapping_sub(b'0'));
        remaining = remaining.saturating_sub(1);
    }
    value
}

fn is_octal(c: &u8) -> bool {
    (b'0'..=b'7').contains(c)
}

/// Upstream's `hextobin`, including its `default` arm — only ever reached with
/// a character `is_ascii_hexdigit` has already accepted.
fn hextobin(c: u8) -> u8 {
    match c {
        b'a'..=b'f' => c.wrapping_sub(b'a').wrapping_add(10),
        b'A'..=b'F' => c.wrapping_sub(b'A').wrapping_add(10),
        _ => c.wrapping_sub(b'0'),
    }
}

/// GNU's `--help`, minus the project's ancillary block, as every converted
/// utility here omits it. The two NOTEs are upstream's and are kept: the first
/// is the only warning a user gets that the `echo` they just typed was almost
/// certainly their shell's.
fn help_text() -> String {
    "\
Usage: echo [SHORT-OPTION]... [STRING]...
  or:  echo LONG-OPTION
Echo the STRING(s) to standard output.

  -n             do not output the trailing newline
  -e             enable interpretation of backslash escapes
  -E             disable interpretation of backslash escapes (default)
      --help        display this help and exit
      --version     output version information and exit

If -e is in effect, the following sequences are recognized:

  \\\\      backslash
  \\a      alert (BEL)
  \\b      backspace
  \\c      produce no further output
  \\e      escape
  \\f      form feed
  \\n      new line
  \\r      carriage return
  \\t      horizontal tab
  \\v      vertical tab
  \\0NNN   byte with octal value NNN (1 to 3 digits)
  \\xHH    byte with hexadecimal value HH (1 to 2 digits)

NOTE: your shell may have its own version of echo, which usually supersedes
the version described here.  Please refer to your shell's documentation
for details about the options it supports.

NOTE: printf(1) is a preferred alternative,
which does not have issues outputting option-like strings.
"
    .to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// The whole program as a pure function, so a case reads as the command
    /// line it stands for.
    fn out(items: &[&str], posix: bool) -> Vec<u8> {
        match parse_args(&argv(items), posix) {
            Request::Write(set) => render(&argv(items), &set),
            Request::Help => help_text().into_bytes(),
            Request::Version => b"echo (SlateOS coreutils) 0.1.0\n".to_vec(),
        }
    }

    fn plain(items: &[&str]) -> Vec<u8> {
        out(items, false)
    }

    // ---------------------------------------------------------- the basics ---

    #[test]
    fn no_arguments_is_one_newline() {
        assert_eq!(plain(&[]), b"\n");
    }

    #[test]
    fn words_are_separated_by_one_space() {
        assert_eq!(plain(&["a", "b", "c"]), b"a b c\n");
    }

    #[test]
    fn an_empty_argument_still_takes_a_separator() {
        assert_eq!(plain(&["a", "", "b"]), b"a  b\n");
    }

    #[test]
    fn dash_n_drops_the_newline() {
        assert_eq!(plain(&["-n", "hi"]), b"hi");
    }

    #[test]
    fn dash_n_alone_writes_nothing_at_all() {
        assert!(plain(&["-n"]).is_empty());
    }

    // ---------------------------------------------------- what is an option ---

    #[test]
    fn the_scan_stops_at_the_first_word_that_is_not_an_option() {
        // `-n` after text is text, and the newline comes back.
        assert_eq!(plain(&["hi", "-n"]), b"hi -n\n");
    }

    #[test]
    fn a_bare_dash_is_text() {
        assert_eq!(plain(&["-", "-n"]), b"- -n\n");
    }

    #[test]
    fn an_unknown_short_option_is_text_and_ends_the_scan() {
        assert_eq!(plain(&["-x", "-n"]), b"-x -n\n");
    }

    #[test]
    fn one_bad_character_makes_the_whole_word_text() {
        // Not `-e` plus a stray `1`: the word is rejected as a unit.
        assert_eq!(plain(&["-en1", "a\\tb"]), b"-en1 a\\tb\n");
    }

    #[test]
    fn options_bundle_in_any_length_and_order() {
        assert_eq!(plain(&["-neE", "a\\tb"]), b"a\\tb");
        assert_eq!(plain(&["-nnnn", "hi"]), b"hi");
    }

    #[test]
    fn the_last_of_e_and_capital_e_wins() {
        assert_eq!(plain(&["-e", "-E", "a\\tb"]), b"a\\tb\n");
        assert_eq!(plain(&["-E", "-e", "a\\tb"]), b"a\tb\n");
        assert_eq!(plain(&["-eE", "a\\tb"]), b"a\\tb\n");
        assert_eq!(plain(&["-Ee", "a\\tb"]), b"a\tb\n");
    }

    #[test]
    fn an_empty_argument_ends_the_option_scan() {
        assert_eq!(plain(&["", "-n"]), b" -n\n");
    }

    // ------------------------------------------------- the two long options ---

    #[test]
    fn help_and_version_are_only_the_whole_command_line() {
        assert!(matches!(
            parse_args(&argv(&["--help"]), false),
            Request::Help
        ));
        assert!(matches!(
            parse_args(&argv(&["--version"]), false),
            Request::Version
        ));
        // With anything after them they are ordinary text — and note the scan
        // stops there, so the `-n` is text too.
        assert_eq!(plain(&["--help", "-n"]), b"--help -n\n");
        assert_eq!(plain(&["--version", "x"]), b"--version x\n");
    }

    #[test]
    fn long_options_are_not_abbreviated() {
        assert_eq!(plain(&["--hel"]), b"--hel\n");
        assert_eq!(plain(&["--"]), b"--\n");
    }

    // ------------------------------------------------------------- escapes ---

    #[test]
    fn escapes_are_off_until_asked_for() {
        assert_eq!(plain(&["a\\tb"]), b"a\\tb\n");
    }

    #[test]
    fn the_single_character_escapes() {
        assert_eq!(
            plain(&["-e", "\\a\\b\\e\\f\\n\\r\\t\\v\\\\"]),
            b"\x07\x08\x1b\x0c\n\r\t\x0b\\\n"
        );
    }

    #[test]
    fn backslash_c_ends_the_program_not_the_word() {
        // No newline, and `b` and the whole next argument are never written.
        assert_eq!(plain(&["-e", "a\\cb", "second"]), b"a");
        // Even with the newline already asked for.
        assert_eq!(plain(&["-e", "\\c"]), b"");
    }

    #[test]
    fn an_unknown_escape_keeps_its_backslash() {
        assert_eq!(plain(&["-e", "a\\qb"]), b"a\\qb\n");
    }

    #[test]
    fn a_trailing_backslash_is_a_backslash() {
        assert_eq!(plain(&["-e", "a\\"]), b"a\\\n");
    }

    #[test]
    fn hex_escapes_take_one_or_two_digits() {
        assert_eq!(plain(&["-e", "\\x41"]), b"A\n");
        assert_eq!(plain(&["-e", "\\x4"]), b"\x04\n");
        assert_eq!(plain(&["-e", "\\x41B"]), b"AB\n");
        assert_eq!(plain(&["-e", "\\xfF"]), b"\xff\n");
    }

    #[test]
    fn a_hex_escape_with_no_digit_is_not_an_escape() {
        assert_eq!(plain(&["-e", "\\xz"]), b"\\xz\n");
        assert_eq!(plain(&["-e", "\\x"]), b"\\x\n");
    }

    #[test]
    fn octal_escapes_do_not_need_the_leading_zero() {
        // Documented as `\0NNN`; the switch also has arms for `1`-`7`.
        assert_eq!(plain(&["-e", "\\101"]), b"A\n");
        assert_eq!(plain(&["-e", "\\0101"]), b"A\n");
    }

    #[test]
    fn a_lone_backslash_zero_is_a_nul_byte() {
        assert_eq!(plain(&["-e", "a\\0b"]), b"a\0b\n");
    }

    #[test]
    fn an_octal_escape_takes_at_most_three_digits() {
        // `\1011` is `A` followed by a literal `1`.
        assert_eq!(plain(&["-e", "\\1011"]), b"A1\n");
        // And after `\0`, three more: `\01011` is `A` then `1`.
        assert_eq!(plain(&["-e", "\\01011"]), b"A1\n");
    }

    #[test]
    fn an_octal_escape_wraps_at_a_byte() {
        assert_eq!(plain(&["-e", "\\0777"]), b"\xff\n");
    }

    #[test]
    fn an_eight_is_not_an_octal_digit() {
        assert_eq!(plain(&["-e", "\\08"]), b"\x008\n");
    }

    // -------------------------------------------------- POSIXLY_CORRECT ---

    #[test]
    fn posix_mode_decodes_escapes_without_dash_e() {
        assert_eq!(out(&["a\\tb"], true), b"a\tb\n");
    }

    #[test]
    fn posix_mode_prints_options_instead_of_obeying_them() {
        assert_eq!(out(&["-e", "x"], true), b"-e x\n");
        assert_eq!(out(&["--help"], true), b"--help\n");
    }

    #[test]
    fn posix_mode_makes_one_exception_for_a_leading_dash_n() {
        // `-n` first re-enables the whole option scan for the rest of the line.
        assert_eq!(out(&["-n", "-E", "a\\tb"], true), b"a\tb");
        // ... but `-E` still cannot switch the escapes back off, because the
        // test upstream is `do_v9 || posixly_correct`.
        assert_eq!(out(&["-n", "x"], true), b"x");
    }

    // ------------------------------------------------------------ raw bytes ---

    #[test]
    fn a_word_that_is_not_utf8_is_written_through_unchanged() {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let args = vec![OsString::from_vec(vec![0xff, 0xfe])];
            let Request::Write(set) = parse_args(&args, false) else {
                panic!("not a write");
            };
            assert_eq!(render(&args, &set), b"\xff\xfe\n");
        }
    }
}
