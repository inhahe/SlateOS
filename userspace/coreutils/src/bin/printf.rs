//! printf — format and print data.
//!
//! # What this used to be
//!
//! The shipped `printf` recognised nine conversions — `%s %d %i %u %x %X %o %c
//! %%` — with no flags, no width, no precision and no length modifiers, and no
//! diagnostics of any kind. What that meant in practice:
//!
//! - **No floating point at all.** `printf '%f\n' 1.5` printed the literal
//!   `%f` and the argument was dropped. `%e`, `%g` and `%a` likewise. This is
//!   the conversion `printf` is most often reached for after `%s`.
//! - **No field widths.** `printf '%-10s|' x` printed `%-10s|`, so every
//!   script that used `printf` to draw a column produced garbage rather than a
//!   column.
//! - **No format reuse.** GNU restarts the format string while arguments
//!   remain, which is how `printf '%s\n' *` prints one file per line. Ours ran
//!   the format once and silently discarded the rest.
//! - **No `%b` and no `%q`.** `%b` is the reason `printf` is preferred to
//!   `echo -e`; `%q` is the shell-quoting one, and its absence has a security
//!   flavour rather than a cosmetic one.
//! - **Bad numbers were zero.** `printf '%d' abc` printed `0` and exited 0.
//!   GNU prints `0`, says `'abc': expected a numeric value`, and exits 1 — so
//!   a script checking `$?` learned nothing.
//! - **Numbers were `i64` via Rust's parser**, so `0x10`, `010` and a leading
//!   `+` were all "not a number"; C's is `strtoimax(s, &end, 0)`, which reads
//!   all three.
//!
//! # Why there is no getopt here
//!
//! Every other utility in this crate that takes options routes them through
//! [`coreutils::getopt`]. `printf` must not, and upstream says why in a
//! comment: *"We directly parse options, rather than use parse_long_options, in
//! order to avoid accepting abbreviations."* `printf --h` has to print `--h`,
//! not the help, because a format string is an arbitrary word and a utility
//! that ate a prefix of `--help` would eat formats. So `--help` and `--version`
//! are recognised only when one of them is the *entire* command line, and `--`
//! is stripped by hand.
//!
//! # The conversions
//!
//! [`coreutils::extfloat`] answers `%a %e %f %g` in 80-bit precision, which is
//! what glibc's `long double` is on x86-64 and what upstream converts to; and
//! [`coreutils::cfmt`] answers `%d %i %o %u %x %X %c %s` and the flag, width
//! and precision handling around both. `%q` is
//! [`coreutils::quote::quotef`], already in the tree because it is how every
//! utility here renders a file name into a message — the two are the same
//! function upstream too (`quotearg_style (shell_escape_quoting_style, …)`).
//!
//! # `\c` is an exit, not a break
//!
//! `print_esc_char` answers `\c` with `exit (EXIT_SUCCESS)`. Two consequences
//! that are easy to miss and are both observable: it stops the *whole* program
//! rather than the current directive, and its status is 0 even if an earlier
//! argument already failed to convert. `printf '%d\c' abc` prints `0`, prints
//! the diagnostic, and exits **0**.
//!
//! # One deliberate difference from GNU
//!
//! Two diagnostics echo bytes the caller chose — the invalid-conversion message
//! echoes the format, and the character-constant warning echoes the tail of an
//! argument. GNU writes those bytes raw, which lets a format string put a
//! newline, or a whole forged line, into `printf`'s own error stream. Here they
//! go through [`coreutils::quote::escape_unprintable`], which prints text as
//! itself and everything else as `\ooo`.
//!
//! Since that rule became character-wise the difference is smaller than it was,
//! and where it survives it is worth knowing why. `printf '%é'` still differs,
//! because a directive runs from the `%` to the **conversion byte** — so what
//! the message echoes is `%` and 0xC3 alone, the lead byte of a character whose
//! tail was never part of the directive. It decodes to nothing, so it is
//! escaped. The character-constant warning is not truncated that way, and there
//! the two agree. `scripts/printf-diff.sh` carries the remainder as expected
//! differences rather than quietly passing them.
//!
//! # Checked against GNU
//!
//! `scripts/printf-diff.sh` runs both binaries over the same command lines and
//! compares stdout, stderr and the exit status separately.

use coreutils::cfmt::{self, Value};
use coreutils::extfloat::{self, ExtF80, Spec};
use coreutils::getopt::{self, Program};
use coreutils::quote::{self, os_bytes, quote, quotef};
use coreutils::stdfd::{self, Stream};
use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

// Before `main`, so that `stdfd::restore` still sees a caller's `printf >&-` as
// the closed descriptor it is. See `coreutils::stdfd`.
coreutils::guard_std_fds!();

const PRINTF: Program = Program::new("printf", 1);

const USAGE: &str = "usage: printf FORMAT [ARGUMENT]...\n   \
                     or: printf OPTION";

/// `strerror (ERANGE)`.
///
/// A constant rather than a lookup because `ERANGE` is the only `errno`
/// `printf` can raise: the whole of its I/O is one output stream, and every
/// other failure it reports it detects itself.
const ERANGE_TEXT: &str = "Numerical result out of range";

/// The conversions that are valid before any flag has ruled one out.
const CONVERSIONS: &[u8] = b"aAcdeEfFgGiosuxX";

/// The characters `print_esc_char` knows, which is also the set that stops an
/// escape from being passed through as a backslash and a byte.
const ESC_CHARS: &[u8] = b"\"\\abcefnrtv";

/// The length modifiers a directive may carry. They are skipped rather than
/// interpreted: upstream substitutes its own (`PRIdMAX` or `L`) before handing
/// the directive to the C library, so whatever the caller wrote has no effect
/// on the output, and `%hd`, `%jd` and `%d` are one directive.
const LENGTH_MODIFIERS: &[u8] = b"lLhjtz";

/// The widest field a conversion can produce.
///
/// C's `printf` counts a width and a precision in `int`, so a field wider than
/// `INT_MAX` is one it cannot report the length of. glibc does not truncate it
/// or print it anyway: the conversion fails with `EOVERFLOW`, having written
/// nothing, and only the *stream* remembers — which is why the resulting
/// diagnostic is `write error` and arrives at exit rather than at the
/// directive. Three spellings reach it, and all three are measured in
/// `scripts/printf-cases.py`: a literal width (`%2147483648d`), a literal
/// precision (`%.2147483648d`), and a `*` width of exactly `INT_MIN`, whose
/// magnitude is one past `INT_MAX` — `printf` itself checks a `*` width
/// against `INT_MIN..=INT_MAX` and so lets that last one through.
///
/// The bound matters here more than it does in C. A C `printf` handed an
/// enormous width streams spaces at it; this one builds the field in a `Vec`
/// first, so without this check `printf '%*d' -2147483648 5` does not print a
/// wrong answer — it asks for two gigabytes and stops responding.
const MAX_FIELD: usize = i32::MAX as usize;

// --------------------------------------------------------------------- errors

/// Why the run ended before the format did.
///
/// [`Stop::Cancel`] is not a failure — it is `\c`, which upstream implements as
/// a call to `exit`. It travels with the errors because that is exactly what it
/// does to the control flow: every `?` in this file is a place `exit` could
/// have been called from, and modelling it any other way would mean threading a
/// "should I keep going" boolean through the escape printer, the format scanner
/// and the argument loop, where forgetting it in one place is silent.
#[derive(Debug)]
enum Stop {
    /// `error (EXIT_FAILURE, 0, …)`: one sentence, no `Try '… --help'`, exit 1.
    Fatal(String),
    /// `error (0, 0, "missing operand"); usage (EXIT_FAILURE)`, which does
    /// carry the referral.
    Usage(getopt::Error),
    /// A write to standard output failed.
    Write(io::Error),
    /// `\c`.
    Cancel,
}

// ----------------------------------------------------------------------- main

fn main() -> ExitCode {
    stdfd::restore();
    let args: Vec<Vec<u8>> = env::args_os()
        .skip(1)
        .map(|a| os_bytes(&a).into_owned())
        .collect();

    // `--help` and `--version` are recognised only as the whole command line.
    // A format string is an arbitrary word, so a `printf` that accepted
    // abbreviations — or accepted these anywhere — would eat formats.
    if let [only] = args.as_slice() {
        if only == b"--help" {
            return say(format!("{USAGE}\n").as_bytes());
        }
        if only == b"--version" {
            return say(b"printf (SlateOS coreutils)\n");
        }
    }

    let mut printer = Printer {
        out: Stream::stdout(),
        status: 0,
        stream_failed: false,
        // The one thing `POSIXLY_CORRECT` changes here: it silences the
        // warning about text after a character constant.
        posixly_correct: env::var_os("POSIXLY_CORRECT").is_some(),
    };
    let outcome = printer.run(&args);

    // Upstream flushes through `atexit (close_stdout)`, so output written
    // before a fatal diagnostic still reaches the stream — `printf 'a%z'`
    // prints `a` and *then* complains. Draining here, before reporting,
    // reproduces that; and, being the drain, it is also where a closed or
    // unwritable standard output is finally discovered.
    let Printer {
        out,
        status,
        stream_failed,
        ..
    } = printer;
    // `close_stdout` is not usable here: the verdict has to be in hand *before*
    // the outcome is worded, because a write error outranks a `%z` diagnostic
    // and a `\c` that asks to exit 0. The one thing folded in is the reader
    // having gone away, which GNU answers by dying of `SIGPIPE` in silence —
    // see `coreutils::stdfd::reader_gone` — so it counts as a flush that
    // succeeded and leaves the run its earned status.
    let flushed = match out.finish() {
        Err(e) if stdfd::reader_gone(&e) => Ok(()),
        verdict => verdict,
    };

    // A field too wide to render is reported the way `close_stdout` reports
    // one: after everything else has been written, with no `strerror` clause
    // because no system call failed, and outranking every other outcome —
    // including `\c`, which asks to exit 0. Upstream reaches the same place by
    // a different road: `exit (EXIT_SUCCESS)` still runs the `atexit` handler,
    // which finds the stream's error indicator set and `_exit`s with failure.
    if stream_failed {
        match flushed {
            Err(e) => stdfd::write_error("printf", &e),
            Ok(()) => eprintln!("printf: write error"),
        }
        return ExitCode::FAILURE;
    }

    match (outcome, flushed) {
        (Err(Stop::Write(e)), _) | (_, Err(e)) => {
            stdfd::write_error("printf", &e);
            ExitCode::FAILURE
        }
        (Ok(()), Ok(())) => ExitCode::from(u8::try_from(status).unwrap_or(1)),
        (Err(Stop::Cancel), Ok(())) => ExitCode::SUCCESS,
        (Err(Stop::Fatal(message)), Ok(())) => {
            eprintln!("printf: {message}");
            ExitCode::FAILURE
        }
        (Err(Stop::Usage(e)), Ok(())) => {
            eprintln!("printf: {}", e.message());
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

/// Say one thing and stop — `--help` and `--version`.
///
/// Both are ordinary writes to standard output, so both fail when there is no
/// standard output to write to: measured, `printf --help >&-` is
/// `printf: write error: Bad file descriptor` and exits 1.
fn say(bytes: &[u8]) -> ExitCode {
    let mut out = Stream::stdout();
    let _ = out.write_all(bytes);
    stdfd::close_stdout("printf", out, ExitCode::SUCCESS)
}

/// The program's state: where output goes, what it will exit with, and the one
/// environment variable it consults.
struct Printer<W: Write> {
    out: W,
    /// Upstream's `exit_status`. A bad *argument* sets it and the run
    /// continues; a bad *format* raises [`Stop::Fatal`] instead.
    status: i32,
    /// Set when a directive asked for a field wider than a conversion can
    /// produce — see [`MAX_FIELD`]. Upstream has no such flag: there, the C
    /// library's `printf` fails, leaves the stream's error indicator set, and
    /// `atexit (close_stdout)` turns that into `write error` at the end. This
    /// flag is that error indicator, and [`main`] is that `atexit`.
    stream_failed: bool,
    posixly_correct: bool,
}

impl<W: Write> Printer<W> {
    fn run(&mut self, args: &[Vec<u8>]) -> Result<(), Stop> {
        // There is no getopt call to consume it, so `--` is stripped by hand --
        // and only in the first position, because after that it is a format or
        // an argument.
        let args = match args.split_first() {
            Some((first, rest)) if first.as_slice() == b"--" => rest,
            _ => args,
        };

        let Some((format, mut remaining)) = args.split_first() else {
            return Err(Stop::Usage(
                PRINTF.usage_referring("missing operand".into()),
            ));
        };

        // The format is reused while arguments remain, which is what makes
        // `printf '%s\n' *` print one name per line. A pass that consumed
        // nothing ends it, or the loop would not terminate.
        loop {
            let used = self.print_formatted(format, remaining)?;
            remaining = remaining.get(used..).unwrap_or(&[]);
            if used == 0 || remaining.is_empty() {
                break;
            }
        }

        if let Some(extra) = remaining.first() {
            // A warning, not an error: upstream does not touch `exit_status`
            // here, so `printf hello x` still exits 0.
            self.warn(&format!(
                "warning: ignoring excess arguments, starting with {}",
                quote(extra)
            ));
        }
        Ok(())
    }

    // ------------------------------------------------------------------ output

    fn put(&mut self, byte: u8) -> Result<(), Stop> {
        self.out.write_all(&[byte]).map_err(Stop::Write)
    }

    fn emit(&mut self, bytes: &[u8]) -> Result<(), Stop> {
        self.out.write_all(bytes).map_err(Stop::Write)
    }

    /// A diagnostic that does not stop the run.
    ///
    /// Failures writing to standard error are ignored, as they are upstream:
    /// there is nowhere left to report them, and the exit status already says
    /// something went wrong.
    fn warn(&self, message: &str) {
        eprintln!("printf: {message}");
    }

    // ------------------------------------------------------------- the format

    /// One pass over the format string. Returns how many arguments it used.
    fn print_formatted(&mut self, format: &[u8], args: &[Vec<u8>]) -> Result<usize, Stop> {
        // Reading one past the end as a NUL is how the C original knows it is
        // done, and several of its scans deliberately look there: `printf '%'`
        // reaches the conversion switch with the terminator as the conversion.
        let at = |i: usize| format.get(i).copied().unwrap_or(0);

        let mut used = 0usize;
        let mut f = 0usize;
        while f < format.len() {
            match at(f) {
                b'%' => {
                    let direc_start = f;
                    f = f.saturating_add(1);

                    if at(f) == b'%' {
                        self.put(b'%')?;
                        f = f.saturating_add(1);
                        continue;
                    }

                    // `%b` and `%q` are matched before the flags, so they take
                    // none: `%5b` is not a wide `%b`, it is an invalid
                    // conversion, because `b` never enters the `ok` table.
                    if at(f) == b'b' {
                        if let Some(argument) = args.get(used) {
                            used = used.saturating_add(1);
                            self.print_esc_string(argument)?;
                        }
                        f = f.saturating_add(1);
                        continue;
                    }
                    if at(f) == b'q' {
                        if let Some(argument) = args.get(used) {
                            used = used.saturating_add(1);
                            self.emit(quotef(argument).as_bytes())?;
                        }
                        f = f.saturating_add(1);
                        continue;
                    }

                    // `ok` upstream: a 256-entry table of which conversions are
                    // still legal. It is a short list here instead, because the
                    // flags only ever remove from it and sixteen entries make
                    // the removals readable.
                    let mut allowed: Vec<u8> = CONVERSIONS.to_vec();
                    // Set by a width or precision past [`MAX_FIELD`]. It cannot
                    // be reported here: the conversion is still legal, and both
                    // the argument and the rest of the format are still
                    // processed. Only the output is dropped.
                    let mut too_wide = false;
                    let mut spec = Spec {
                        minus: false,
                        plus: false,
                        space: false,
                        hash: false,
                        zero: false,
                        width: 0,
                        precision: None,
                        conv: 0,
                    };

                    loop {
                        match at(f) {
                            // Grouping. glibc accepts it and, in the C locale,
                            // does nothing with it -- but it still rules out
                            // the conversions that have no groups.
                            b'\'' | b'I' => forbid(&mut allowed, b"aAceEosxX"),
                            b'-' => spec.minus = true,
                            b'+' => spec.plus = true,
                            b' ' => spec.space = true,
                            b'#' => {
                                spec.hash = true;
                                forbid(&mut allowed, b"cdisu");
                            }
                            b'0' => {
                                spec.zero = true;
                                forbid(&mut allowed, b"cs");
                            }
                            _ => break,
                        }
                        f = f.saturating_add(1);
                    }

                    if at(f) == b'*' {
                        f = f.saturating_add(1);
                        let width = match args.get(used) {
                            Some(argument) => {
                                used = used.saturating_add(1);
                                let v = self.strtoimax(argument);
                                if v < i64::from(i32::MIN) || v > i64::from(i32::MAX) {
                                    return Err(Stop::Fatal(format!(
                                        "invalid field width: {}",
                                        quote(argument)
                                    )));
                                }
                                v
                            }
                            None => 0,
                        };
                        // A negative `*` width is a `-` flag and the magnitude,
                        // which is what C says and what glibc does.
                        if width < 0 {
                            spec.minus = true;
                        }
                        let magnitude = usize::try_from(width.unsigned_abs()).unwrap_or(usize::MAX);
                        if magnitude > MAX_FIELD {
                            too_wide = true;
                        } else {
                            spec.width = magnitude;
                        }
                    } else {
                        let mut width = 0usize;
                        while at(f).is_ascii_digit() {
                            width = width
                                .saturating_mul(10)
                                .saturating_add(usize::from(at(f).wrapping_sub(b'0')));
                            f = f.saturating_add(1);
                        }
                        if width > MAX_FIELD {
                            too_wide = true;
                        } else {
                            spec.width = width;
                        }
                    }

                    if at(f) == b'.' {
                        f = f.saturating_add(1);
                        // A precision means nothing to `%c`, so writing one
                        // makes the directive invalid rather than being
                        // ignored.
                        forbid(&mut allowed, b"c");
                        if at(f) == b'*' {
                            f = f.saturating_add(1);
                            spec.precision = match args.get(used) {
                                Some(argument) => {
                                    used = used.saturating_add(1);
                                    let v = self.strtoimax(argument);
                                    if v < 0 {
                                        // "A negative precision is taken as if
                                        // the precision were omitted."
                                        None
                                    } else if v > i64::from(i32::MAX) {
                                        return Err(Stop::Fatal(format!(
                                            "invalid precision: {}",
                                            quote(argument)
                                        )));
                                    } else {
                                        Some(usize::try_from(v).unwrap_or(0))
                                    }
                                }
                                None => Some(0),
                            };
                        } else {
                            let mut precision = 0usize;
                            while at(f).is_ascii_digit() {
                                precision = precision
                                    .saturating_mul(10)
                                    .saturating_add(usize::from(at(f).wrapping_sub(b'0')));
                                f = f.saturating_add(1);
                            }
                            // A bare `.` is a precision of zero, not an absent
                            // one -- `%.d` of `0` prints nothing.
                            //
                            // Unlike a `*` precision, which `printf` checks
                            // itself and rejects outright, a written-out one is
                            // passed to the C library and overflows there.
                            if precision > MAX_FIELD {
                                too_wide = true;
                            } else {
                                spec.precision = Some(precision);
                            }
                        }
                    }

                    while LENGTH_MODIFIERS.contains(&at(f)) {
                        f = f.saturating_add(1);
                    }

                    let conversion = at(f);
                    if !allowed.contains(&conversion) {
                        // The message quotes the directive as written, from the
                        // `%` through the conversion character -- which may be
                        // the end of the string, when the format simply
                        // stopped.
                        let end = f.saturating_add(1).min(format.len());
                        let text = format.get(direc_start..end).unwrap_or(b"");
                        return Err(Stop::Fatal(format!(
                            "{}: invalid conversion specification",
                            quote::escape_unprintable(text)
                        )));
                    }
                    spec.conv = conversion;

                    // The argument is consumed whatever the conversion is, and
                    // an absent one is the empty string rather than an error.
                    let argument: &[u8] = match args.get(used) {
                        Some(a) => {
                            used = used.saturating_add(1);
                            a
                        }
                        None => b"",
                    };
                    if too_wide {
                        // The conversion fails, writing nothing, and the run
                        // carries on: `printf '%*d|%s|\n' -2147483648 5 tail`
                        // prints `|tail|` and *then* reports the write error.
                        // The argument was consumed above either way.
                        self.stream_failed = true;
                    } else {
                        self.print_direc(&spec, argument)?;
                    }
                    f = f.saturating_add(1);
                }
                b'\\' => {
                    let consumed = self.print_esc(format, f, false)?;
                    f = f.saturating_add(consumed).saturating_add(1);
                }
                other => {
                    self.put(other)?;
                    f = f.saturating_add(1);
                }
            }
        }
        Ok(used)
    }

    /// Convert one argument under one directive.
    fn print_direc(&mut self, spec: &Spec, argument: &[u8]) -> Result<(), Stop> {
        let rendered = match spec.conv {
            b'd' | b'i' => {
                let v = self.strtoimax(argument);
                cfmt::render(spec, Value::Signed(v))
            }
            b'o' | b'u' | b'x' | b'X' => {
                let v = self.strtoumax(argument);
                cfmt::render(spec, Value::Unsigned(v))
            }
            b'a' | b'A' | b'e' | b'E' | b'f' | b'F' | b'g' | b'G' => {
                let v = self.strtold(argument);
                cfmt::render(spec, Value::Float(v))
            }
            // `%c` takes the argument's *first byte*, and an empty argument
            // gives a NUL -- which upstream writes, so `printf '%c' ''` emits
            // one zero byte rather than nothing.
            b'c' => cfmt::render(spec, Value::Byte(argument.first().copied().unwrap_or(0))),
            _ => cfmt::render(spec, Value::Text(argument)),
        };
        self.emit(&rendered)
    }

    // ---------------------------------------------------------------- escapes

    /// `print_esc_string`: the whole of one string, escapes and all.
    ///
    /// Only `%b` reaches this, and it passes `octal_0`, so an octal escape here
    /// is written `\0ooo` — `\101` is still `A`, as an undocumented extension
    /// that matches Bash, but `\0101` is a backspace followed by `1` rather
    /// than the three-digit `\010` plus `1`.
    fn print_esc_string(&mut self, s: &[u8]) -> Result<(), Stop> {
        let mut i = 0usize;
        while i < s.len() {
            if s.get(i) == Some(&b'\\') {
                let consumed = self.print_esc(s, i, true)?;
                i = i.saturating_add(consumed).saturating_add(1);
            } else {
                self.put(s.get(i).copied().unwrap_or(0))?;
                i = i.saturating_add(1);
            }
        }
        Ok(())
    }

    /// One escape sequence beginning at the backslash at `start`. Returns how
    /// many bytes it took *besides* the backslash.
    fn print_esc(&mut self, s: &[u8], start: usize, octal_0: bool) -> Result<usize, Stop> {
        let at = |i: usize| s.get(i).copied().unwrap_or(0);
        let mut p = start.saturating_add(1);

        if at(p) == b'x' {
            // One or two hex digits, and at least one: `\x` alone is fatal,
            // which is the only escape that is.
            p = p.saturating_add(1);
            let mut value = 0u32;
            let mut length = 0u32;
            while length < 2 && at(p).is_ascii_hexdigit() {
                value = value.wrapping_mul(16).wrapping_add(hex_value(at(p)));
                length = length.saturating_add(1);
                p = p.saturating_add(1);
            }
            if length == 0 {
                return Err(Stop::Fatal("missing hexadecimal number in escape".into()));
            }
            self.put(truncate(value))?;
        } else if is_octal(at(p)) {
            // With `octal_0`, a leading `0` introduces the escape and is not
            // one of its three digits.
            if octal_0 && at(p) == b'0' {
                p = p.saturating_add(1);
            }
            let mut value = 0u32;
            let mut length = 0u32;
            while length < 3 && is_octal(at(p)) {
                value = value
                    .wrapping_mul(8)
                    .wrapping_add(u32::from(at(p).wrapping_sub(b'0')));
                length = length.saturating_add(1);
                p = p.saturating_add(1);
            }
            // `\777` is 511, and `putchar` keeps the low byte of it.
            self.put(truncate(value))?;
        } else if at(p) != 0 && ESC_CHARS.contains(&at(p)) {
            let c = at(p);
            p = p.saturating_add(1);
            self.print_esc_char(c)?;
        } else if at(p) == b'u' || at(p) == b'U' {
            let marker = at(p);
            let digits = if marker == b'u' { 4usize } else { 8usize };
            p = p.saturating_add(1);
            let mut code = 0u32;
            for _ in 0..digits {
                if !at(p).is_ascii_hexdigit() {
                    return Err(Stop::Fatal("missing hexadecimal number in escape".into()));
                }
                code = code.wrapping_mul(16).wrapping_add(hex_value(at(p)));
                p = p.saturating_add(1);
            }
            // The surrogate range is the only one refused. Everything else that
            // cannot be written falls back to printing the escape back, which
            // is why this has to be a separate check rather than left to the
            // encoder.
            if (0xd800..=0xdfff).contains(&code) {
                return Err(Stop::Fatal(format!(
                    "invalid universal character name \\{}{code:0digits$x}",
                    char::from(marker),
                )));
            }
            self.print_unicode_char(code)?;
        } else {
            // Not an escape at all: the backslash and whatever followed it are
            // both output, so `\z` is two bytes and a trailing `\` is one.
            self.put(b'\\')?;
            if at(p) != 0 {
                self.put(at(p))?;
                p = p.saturating_add(1);
            }
        }
        Ok(p.saturating_sub(start).saturating_sub(1))
    }

    fn print_esc_char(&mut self, c: u8) -> Result<(), Stop> {
        let byte = match c {
            b'a' => 0x07,
            b'b' => 0x08,
            // Not "stop printing": upstream calls `exit (EXIT_SUCCESS)`, so
            // this abandons the rest of the run and forces the status to 0.
            b'c' => return Err(Stop::Cancel),
            b'e' => 0x1b,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 0x0b,
            other => other,
        };
        self.put(byte)
    }

    /// gnulib's `print_unicode_char (stream, code, 0)` under a UTF-8 locale.
    ///
    /// gnulib converts the code point to UTF-8 and then through `iconv` to the
    /// locale's charset, falling back to printing the escape back when that
    /// fails. Under a UTF-8 charset the second step is the identity, so the
    /// conversion succeeds for every code point UTF-8 can represent and fails
    /// only above U+10FFFF. Surrogates never arrive here — the caller refuses
    /// them with a diagnostic, which is upstream's order too.
    ///
    /// **This used to implement the C locale**, where the charset is ASCII and
    /// so everything from U+0080 up came back out as the literal text
    /// `\u00E9`. That was the same mistake §351 fixed in `quote()`, in a second
    /// place: Q38 settled that SlateOS's charset is UTF-8 and nothing else, so
    /// the ASCII branch was implementing a locale that cannot occur here, and
    /// `printf '\u00e9'` printed seven bytes of backslash-escape where every
    /// modern system prints the two bytes of `é`. Measured against GNU printf
    /// 9.4 under `LC_ALL=C.UTF-8`, which is the reference `printf-diff.sh` now
    /// uses.
    ///
    /// The fallback's hex is **upper** case (`\U00110000`), while the surrogate
    /// diagnostic's is lower (`\ud800`). That is not a transcription slip: the
    /// two are different format strings in different files.
    fn print_unicode_char(&mut self, code: u32) -> Result<(), Stop> {
        if code < 0x80 {
            return self.put(truncate(code));
        }
        match char::from_u32(code) {
            Some(c) => {
                let mut buf = [0u8; 4];
                self.emit(c.encode_utf8(&mut buf).as_bytes())
            }
            // Above U+10FFFF, so no UTF-8 encoding exists and gnulib's failure
            // callback prints the escape back. gnulib's callback also has a
            // `\u%04X` arm for codes below U+10000; it is unreachable under a
            // UTF-8 charset, because every such code point encodes, so it is
            // not written out here. (`char::from_u32` also rejects surrogates,
            // which the caller has already turned into a fatal diagnostic —
            // reaching this arm with one would be a bug there, and printing
            // the escape back is the same thing gnulib would do.)
            None => self.emit(format!("\\U{code:08X}").as_bytes()),
        }
    }

    // ------------------------------------------------------------ the numbers

    /// The `'x` / `"x` extension shared by all three converters: an argument
    /// that begins with a quote and has something after it is a character
    /// constant whose value is that **character's code point**.
    ///
    /// Returns `None` when the argument is not one, so the caller falls through
    /// to the numeric parse.
    ///
    /// ## Why a code point and not a byte
    ///
    /// Upstream has two branches here and picks between them on `MB_CUR_MAX >
    /// 1` — one byte under the `C` locale, one whole multibyte character under
    /// any other. `design-decisions.md` §356, resting on Q38, settles that the
    /// string layer here is UTF-8 full stop, so the multibyte branch is the
    /// only one that can be reached and the byte branch would be dead code
    /// dressed as a choice. Measured, GNU 9.4 under `LC_ALL=C.UTF-8`:
    ///
    /// ```text
    /// printf '%d\n' "'é"    ->  233        (U+00E9, not 195)
    /// printf '%d\n' "'€"    ->  8364
    /// printf '%d\n' "'😀"   ->  128512
    /// printf '%d\n' "'\xff" ->  255        (decodes to nothing: the raw byte)
    /// ```
    ///
    /// The last line is why the fallback is the byte rather than an error:
    /// there is no character to name, and refusing to answer would turn a
    /// printable question into a fatal one.
    fn char_constant(&mut self, s: &[u8]) -> Option<u64> {
        if !matches!(s.first(), Some(b'"' | b'\'')) {
            return None;
        }
        let body = s.get(1..)?;
        let &first = body.first()?;
        let (value, width) = match quote::first_char(body) {
            Some((c, n)) => (u64::from(u32::from(c)), n),
            None => (u64::from(first), 1),
        };
        let rest = body.get(width..).unwrap_or(b"");
        if !rest.is_empty() && !self.posixly_correct {
            // Escaped, not raw: see the module header. GNU prints `rest`
            // verbatim, which lets an argument put a newline in this sentence.
            self.warn(&format!(
                "warning: {}: character(s) following character constant have been ignored",
                quote::escape_unprintable(rest)
            ));
        }
        Some(value)
    }

    /// `strtoimax (s, &end, 0)` plus upstream's checking.
    fn strtoimax(&mut self, s: &[u8]) -> i64 {
        if let Some(v) = self.char_constant(s) {
            return i64::try_from(v).unwrap_or(i64::MAX);
        }
        let scan = scan_integer(s);
        // `i64::MIN` has a magnitude one larger than `i64::MAX`, so the bound
        // depends on the sign.
        let limit = if scan.negative {
            i64::MAX.unsigned_abs().saturating_add(1)
        } else {
            i64::MAX.unsigned_abs()
        };
        let out_of_range = scan.overflowed || scan.magnitude > limit;
        self.verify_numeric(s, scan.consumed, out_of_range);
        if out_of_range {
            return if scan.negative { i64::MIN } else { i64::MAX };
        }
        let magnitude = i128::from(scan.magnitude);
        let value = if scan.negative { -magnitude } else { magnitude };
        i64::try_from(value).unwrap_or(i64::MAX)
    }

    /// `strtoumax (s, &end, 0)` plus upstream's checking.
    ///
    /// A leading `-` is not an error: C defines the result as the negation
    /// taken modulo `UINTMAX_MAX + 1`, which is why `printf '%u' -1` prints
    /// `18446744073709551615` and exits 0.
    fn strtoumax(&mut self, s: &[u8]) -> u64 {
        if let Some(v) = self.char_constant(s) {
            return v;
        }
        let scan = scan_integer(s);
        self.verify_numeric(s, scan.consumed, scan.overflowed);
        if scan.overflowed {
            return u64::MAX;
        }
        if scan.negative {
            0u64.wrapping_sub(scan.magnitude)
        } else {
            scan.magnitude
        }
    }

    /// `cl_strtold (s, &end)` plus upstream's checking.
    fn strtold(&mut self, s: &[u8]) -> ExtF80 {
        if let Some(v) = self.char_constant(s) {
            return ExtF80::from_u32(u32::try_from(v).unwrap_or(u32::MAX));
        }
        let scan = extfloat::strtold(s);
        self.verify_numeric(s, scan.consumed, scan.range_error);
        scan.value
    }

    /// Upstream's `verify_numeric`: the three things that can be wrong with an
    /// argument that was supposed to be a number.
    ///
    /// None of them stops the run — the converted value is used anyway, and
    /// only the exit status records that anything happened. That is why
    /// `printf '%d %d\n' abc 5` prints `0 5`.
    fn verify_numeric(&mut self, s: &[u8], consumed: usize, range_error: bool) {
        if range_error {
            // `error (0, errno, "%s", quote (s))`, so the sentence is the
            // quoted argument, a colon, and `strerror`.
            self.warn(&format!("{}: {ERANGE_TEXT}", quote(s)));
            self.status = 1;
        } else if consumed < s.len() {
            // `*end` is not the terminator, so something was left over. An
            // *entirely* unconsumed argument is a different sentence from a
            // partly consumed one -- and an empty argument is neither, because
            // there `*end` *is* the terminator and it converts to zero in
            // silence.
            if consumed == 0 {
                self.warn(&format!("{}: expected a numeric value", quote(s)));
            } else {
                self.warn(&format!("{}: value not completely converted", quote(s)));
            }
            self.status = 1;
        }
    }
}

// ------------------------------------------------------------------- integers

/// What one `strtoimax`/`strtoumax` call read.
///
/// The magnitude and the sign are kept apart so that one scanner can serve both
/// the signed and the unsigned conversion, which disagree about what to do with
/// a `-` and about where the range ends.
#[derive(Clone, Copy, Debug)]
struct ScannedInt {
    /// The digits' value, saturated at `u64::MAX`.
    magnitude: u64,
    negative: bool,
    /// Bytes claimed. Zero is `strtol`'s "no conversion could be performed".
    consumed: usize,
    /// The magnitude did not fit a `u64`.
    overflowed: bool,
}

/// C's `strtol` family with base 0, in the C locale.
///
/// Base 0 means the base is read from the numeral: `0x`/`0X` is hexadecimal,
/// `0b`/`0B` is binary (glibc took this from C23, and it is observable —
/// `printf '%d' 0b101` prints 5), a leading `0` is octal, and anything else is
/// decimal.
///
/// The prefixes only count when a digit follows. `0x` on its own is *not* a
/// failed hex numeral, it is the octal numeral `0` with an `x` left over, which
/// is why `printf '%d' 0x` prints `0` and complains that the value was not
/// completely converted.
fn scan_integer(s: &[u8]) -> ScannedInt {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut i = 0usize;
    while matches!(at(i), b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i = i.saturating_add(1);
    }
    let negative = match at(i) {
        b'-' => {
            i = i.saturating_add(1);
            true
        }
        b'+' => {
            i = i.saturating_add(1);
            false
        }
        _ => false,
    };

    let mut base = 10u32;
    if at(i) == b'0' {
        match at(i.saturating_add(1)) {
            b'x' | b'X' if at(i.saturating_add(2)).is_ascii_hexdigit() => {
                base = 16;
                i = i.saturating_add(2);
            }
            b'b' | b'B' if matches!(at(i.saturating_add(2)), b'0' | b'1') => {
                base = 2;
                i = i.saturating_add(2);
            }
            // The leading zero is itself the first octal digit, so `i` stays
            // where it is and a lone `0` converts to zero.
            _ => base = 8,
        }
    }

    let mut magnitude = 0u64;
    let mut overflowed = false;
    let mut digits = 0usize;
    while let Some(d) = digit_value(at(i), base) {
        digits = digits.saturating_add(1);
        magnitude = match magnitude
            .checked_mul(u64::from(base))
            .and_then(|m| m.checked_add(u64::from(d)))
        {
            Some(v) => v,
            None => {
                overflowed = true;
                u64::MAX
            }
        };
        i = i.saturating_add(1);
    }

    ScannedInt {
        magnitude,
        negative,
        // No digits is `endptr == nptr`, which means the sign and the
        // whitespace are given back too.
        consumed: if digits == 0 { 0 } else { i },
        overflowed,
    }
}

fn digit_value(c: u8, base: u32) -> Option<u32> {
    let v = match c {
        b'0'..=b'9' => u32::from(c.wrapping_sub(b'0')),
        b'a'..=b'z' => u32::from(c.wrapping_sub(b'a')).saturating_add(10),
        b'A'..=b'Z' => u32::from(c.wrapping_sub(b'A')).saturating_add(10),
        _ => return None,
    };
    if v < base { Some(v) } else { None }
}

// -------------------------------------------------------------------- helpers

fn is_octal(c: u8) -> bool {
    (b'0'..=b'7').contains(&c)
}

fn hex_value(c: u8) -> u32 {
    digit_value(c, 16).unwrap_or(0)
}

/// The low byte, which is what `putchar` keeps of an `int`.
fn truncate(value: u32) -> u8 {
    u8::try_from(value & 0xff).unwrap_or(0)
}

/// Remove a set of conversions from the still-legal list.
fn forbid(allowed: &mut Vec<u8>, remove: &[u8]) {
    allowed.retain(|c| !remove.contains(c));
}

// ---------------------------------------------------------------------- tests

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    struct Outcome {
        out: Vec<u8>,
        status: i32,
        stop: Option<String>,
        stream_failed: bool,
    }

    /// Run a command line and collect what it wrote to standard output, what it
    /// would have exited with, and which fatal stop (if any) ended it.
    ///
    /// Diagnostics go to the real standard error rather than being captured;
    /// the differential harness is what checks their wording, and duplicating
    /// that here would mean two places to update when a sentence changes.
    ///
    /// `posixly_correct` is on so that the character-constant tests do not each
    /// print a warning into the test log.
    fn run(argv: &[&str]) -> Outcome {
        let args: Vec<Vec<u8>> = argv.iter().map(|a| a.as_bytes().to_vec()).collect();
        let mut printer = Printer {
            out: Vec::new(),
            status: 0,
            stream_failed: false,
            posixly_correct: true,
        };
        let outcome = printer.run(&args);
        Outcome {
            stream_failed: printer.stream_failed,
            out: printer.out,
            status: match outcome {
                _ if printer.stream_failed => 1,
                Err(Stop::Cancel) => 0,
                _ => printer.status,
            },
            stop: match outcome {
                Ok(()) | Err(Stop::Cancel) => None,
                Err(Stop::Fatal(m)) => Some(m),
                Err(Stop::Usage(e)) => Some(e.sentence),
                Err(Stop::Write(e)) => Some(e.to_string()),
            },
        }
    }

    /// A field wider than `INT_MAX` is not a diagnosable error at the
    /// directive: the conversion silently produces nothing, the rest of the
    /// format is printed anyway, and the failure surfaces once at exit. All
    /// three spellings that reach it are checked, because only one of them —
    /// the `*` width of `INT_MIN` — is reachable without a 10-digit literal,
    /// and it is the one that used to allocate two gigabytes and hang.
    #[test]
    fn a_field_too_wide_to_render_produces_nothing_and_fails_at_exit() {
        for argv in [
            &["%*d|%s|", "-2147483648", "5", "tail"][..],
            &["%2147483648d|%s|", "5", "tail"][..],
            &["%.2147483648d|%s|", "5", "tail"][..],
            &["%99999999999999999999d|%s|", "5", "tail"][..],
        ] {
            let r = run(argv);
            assert!(r.stream_failed, "for {argv:?}");
            assert_eq!(r.out, b"|tail|", "for {argv:?}");
            assert_eq!(r.status, 1, "for {argv:?}");
            assert_eq!(r.stop, None, "for {argv:?}");
        }
        // The other side of the bound is deliberately not tested. A width of
        // exactly `INT_MAX` is *legal*, so checking that it is accepted means
        // rendering it, and rendering it means a two-gigabyte field — which is
        // what GNU does too, one space at a time. See `known-issues.md`
        // (`TD-PRINTF-BUILDS-THE-WHOLE-FIELD-IN-MEMORY`).
    }

    /// `\c` asks to exit 0, and normally gets it. A failed stream outranks it,
    /// because upstream's `atexit` handler runs after `exit (EXIT_SUCCESS)`.
    #[test]
    fn a_failed_stream_outranks_cancel() {
        let r = run(&["%*d\\c", "-2147483648", "5"]);
        assert!(r.stream_failed);
        assert_eq!(r.status, 1);
    }

    fn out(argv: &[&str]) -> String {
        let r = run(argv);
        assert!(r.stop.is_none(), "stopped: {:?}", r.stop);
        String::from_utf8(r.out).expect("utf-8")
    }

    #[test]
    fn a_format_with_no_directives_is_copied() {
        assert_eq!(out(&["hello"]), "hello");
        assert_eq!(out(&["a\\tb\\n"]), "a\tb\n");
    }

    /// The format restarts while arguments remain, which is the behaviour that
    /// makes `printf '%s\n' *` useful and that the old implementation lacked.
    #[test]
    fn the_format_is_reused_until_the_arguments_run_out() {
        assert_eq!(out(&["%s %s", "a", "b", "c", "d"]), "a bc d");
        assert_eq!(out(&["%s-%s", "a", "b", "c"]), "a-bc-");
        assert_eq!(out(&["%d", "1", "2", "3"]), "123");
    }

    /// A pass that consumes nothing must end the loop, or a format with no
    /// directives and any argument would never return.
    #[test]
    fn a_format_that_consumes_nothing_stops_after_one_pass() {
        let r = run(&["no-directive", "x", "y"]);
        assert_eq!(r.out, b"no-directive");
        assert_eq!(r.status, 0, "the excess-argument warning is not an error");
    }

    #[test]
    fn missing_arguments_are_the_empty_string() {
        assert_eq!(out(&["%s|%s"]), "|");
        assert_eq!(out(&["%d|"]), "0|");
        assert_eq!(out(&["%s|%s", "a"]), "a|");
    }

    /// An empty argument converts to zero *silently*: `strtoimax` stops at the
    /// terminator, so there is nothing left over to complain about.
    #[test]
    fn an_empty_numeric_argument_is_zero_without_a_diagnostic() {
        let r = run(&["%d", ""]);
        assert_eq!(r.out, b"0");
        assert_eq!(r.status, 0);
    }

    #[test]
    fn a_bad_number_is_reported_but_printing_continues() {
        let r = run(&["%d %d", "abc", "5"]);
        assert_eq!(r.out, b"0 5");
        assert_eq!(r.status, 1);
        let r = run(&["%d", "12x"]);
        assert_eq!(r.out, b"12");
        assert_eq!(r.status, 1);
    }

    #[test]
    fn integers_are_read_in_the_base_their_prefix_names() {
        assert_eq!(out(&["%d", "0x10"]), "16");
        assert_eq!(out(&["%d", "010"]), "8");
        assert_eq!(out(&["%d", "0b101"]), "5");
        assert_eq!(out(&["%d", "+5"]), "5");
        assert_eq!(out(&["%d", " 12"]), "12");
    }

    /// `0x` with nothing after it is the octal numeral `0` and a stray `x`,
    /// not a malformed hex numeral.
    #[test]
    fn an_unfinished_radix_prefix_converts_the_leading_zero() {
        let r = run(&["%d", "0x"]);
        assert_eq!(r.out, b"0");
        assert_eq!(r.status, 1);
        let r = run(&["%d", "0b"]);
        assert_eq!(r.out, b"0");
        assert_eq!(r.status, 1);
    }

    #[test]
    fn an_out_of_range_number_saturates_and_reports() {
        let r = run(&["%d", "99999999999999999999999"]);
        assert_eq!(r.out, b"9223372036854775807");
        assert_eq!(r.status, 1);
        let r = run(&["%d", "-99999999999999999999999"]);
        assert_eq!(r.out, b"-9223372036854775808");
        assert_eq!(r.status, 1);
        let r = run(&["%u", "-99999999999999999999999"]);
        assert_eq!(r.out, b"18446744073709551615");
        assert_eq!(r.status, 1);
    }

    /// The most negative `intmax_t` is exactly representable, so it is not a
    /// range error even though its magnitude exceeds `INTMAX_MAX`.
    #[test]
    fn the_most_negative_integer_is_in_range() {
        let r = run(&["%d", "-9223372036854775808"]);
        assert_eq!(r.out, b"-9223372036854775808");
        assert_eq!(r.status, 0);
    }

    /// C says a negative value converts modulo `UINTMAX_MAX + 1`, so this is
    /// not an error at all.
    #[test]
    fn a_negative_unsigned_argument_wraps_silently() {
        let r = run(&["%u", "-1"]);
        assert_eq!(r.out, b"18446744073709551615");
        assert_eq!(r.status, 0);
        assert_eq!(out(&["%x", "-1"]), "ffffffffffffffff");
    }

    #[test]
    fn floating_point_is_eighty_bit() {
        assert_eq!(out(&["%f", "1.5"]), "1.500000");
        assert_eq!(out(&["%.0f", "0.5"]), "0");
        assert_eq!(out(&["%e", "1234.5"]), "1.234500e+03");
        assert_eq!(out(&["%g", "0.0001"]), "0.0001");
        // A hex float, which C's `strtold` reads and Rust's parser does not.
        assert_eq!(out(&["%f", "0x1p4"]), "16.000000");
    }

    /// The old implementation had no width handling at all, so every one of
    /// these printed the directive back.
    #[test]
    fn widths_and_precisions_are_honoured() {
        assert_eq!(out(&["%5s|", "ab"]), "   ab|");
        assert_eq!(out(&["%-5s|", "ab"]), "ab   |");
        assert_eq!(out(&["%.1s", "abc"]), "a");
        assert_eq!(out(&["%5.2s|", "abcdef"]), "   ab|");
        assert_eq!(out(&["%05.2d", "7"]), "   07");
        assert_eq!(out(&["%-05d", "7"]), "7    ");
        assert_eq!(out(&["%5.3d", "7"]), "  007");
        assert_eq!(out(&["%-+8.3d|", "5"]), "+005    |");
    }

    /// A `*` takes its value from the argument list, and a negative one is a
    /// `-` flag rather than an error.
    #[test]
    fn a_star_width_comes_from_an_argument() {
        assert_eq!(out(&["%*d", "6", "5"]), "     5");
        assert_eq!(out(&["%*d", "-6", "5"]), "5     ");
        // A negative `*` precision is taken as no precision at all.
        assert_eq!(out(&["%.*f", "-1", "1.5"]), "1.500000");
        assert_eq!(out(&["%.*f", "2", "1.5"]), "1.50");
    }

    #[test]
    fn a_star_with_no_argument_left_is_zero() {
        assert_eq!(out(&["%*d"]), "0");
        assert_eq!(out(&["%.*d"]), "");
    }

    /// `%c` is one byte and an absent one is a NUL, which is written rather
    /// than skipped.
    #[test]
    fn a_char_conversion_takes_one_byte() {
        assert_eq!(run(&["[%c]", ""]).out, b"[\0]");
        assert_eq!(run(&["%c%c", "ab"]).out, b"a\0");
        assert_eq!(out(&["%5c|", "x"]), "    x|");
    }

    /// The flag/conversion combinations the `ok` table rules out, each of which
    /// is fatal and names the directive as it was written.
    #[test]
    fn invalid_conversion_specifications_are_fatal() {
        for (argv, message) in [
            (&["%"][..], "%: invalid conversion specification"),
            (&["%z"][..], "%z: invalid conversion specification"),
            (&["%'e"][..], "%'e: invalid conversion specification"),
            (&["%#d", "1"][..], "%#d: invalid conversion specification"),
            (&["%0s", "a"][..], "%0s: invalid conversion specification"),
            (&["%#s", "a"][..], "%#s: invalid conversion specification"),
            (&["%.1c", "a"][..], "%.1c: invalid conversion specification"),
        ] {
            assert_eq!(run(argv).stop.as_deref(), Some(message), "for {argv:?}");
        }
    }

    /// The combinations that stay legal, which is the same table read the other
    /// way. `%'d` and `%'f` are valid because grouping means something for
    /// them; `%'e` above is not.
    #[test]
    fn length_modifiers_and_grouping_are_accepted() {
        assert_eq!(out(&["%'d", "1000"]), "1000");
        assert_eq!(out(&["%'f", "1.5"]), "1.500000");
        assert_eq!(out(&["%ld", "5"]), "5");
        assert_eq!(out(&["%lld", "5"]), "5");
        assert_eq!(out(&["%Lf", "1.5"]), "1.500000");
        assert_eq!(out(&["%jd", "5"]), "5");
        assert_eq!(out(&["%zd", "5"]), "5");
    }

    /// `%b` and `%q` are matched before the flags, so they can carry none.
    #[test]
    fn b_and_q_take_no_flags() {
        assert_eq!(
            run(&["%5b", "x"]).stop.as_deref(),
            Some("%5b: invalid conversion specification")
        );
        assert_eq!(
            run(&["%-q", "x"]).stop.as_deref(),
            Some("%-q: invalid conversion specification")
        );
    }

    /// Escapes in the *format* are `\ooo`; escapes in a `%b` *argument* are
    /// `\0ooo`, and the difference is visible on the same input.
    #[test]
    fn octal_escapes_differ_between_the_format_and_a_b_argument() {
        assert_eq!(run(&["\\101"]).out, b"A");
        assert_eq!(run(&["\\0101"]).out, b"\x081");
        assert_eq!(run(&["%b", "\\101"]).out, b"A");
        assert_eq!(run(&["%b", "\\0101"]).out, b"A");
        assert_eq!(run(&["%b", "\\0"]).out, b"\0");
        assert_eq!(run(&["%b", "\\01"]).out, b"\x01");
    }

    #[test]
    fn hex_escapes_take_one_or_two_digits_and_at_least_one() {
        assert_eq!(run(&["\\x41"]).out, b"A");
        assert_eq!(run(&["\\x4"]).out, b"\x04");
        assert_eq!(run(&["\\x411"]).out, b"A1");
        let r = run(&["a\\xZ"]);
        assert_eq!(
            r.stop.as_deref(),
            Some("missing hexadecimal number in escape")
        );
        assert_eq!(r.out, b"a", "output before a fatal error is still written");
    }

    #[test]
    fn an_unknown_escape_is_passed_through() {
        assert_eq!(run(&["\\z"]).out, b"\\z");
        assert_eq!(run(&["a\\"]).out, b"a\\");
    }

    /// The charset is UTF-8, so a universal character name is the UTF-8
    /// encoding of its code point — including the control characters, so
    /// `\u0001` really is the one byte 0x01 and `\u0000` really is a NUL.
    /// Every value below is a measurement of GNU printf 9.4 under
    /// `LC_ALL=C.UTF-8`, not a derivation.
    #[test]
    fn universal_character_names_encode_as_utf8() {
        assert_eq!(run(&["\\u0041"]).out, b"A");
        assert_eq!(run(&["\\u0001|"]).out, b"\x01|");
        assert_eq!(run(&["\\u0000|"]).out, b"\0|");
        assert_eq!(run(&["\\u007f|"]).out, b"\x7f|");
        assert_eq!(run(&["\\u0080"]).out, b"\xc2\x80");
        assert_eq!(run(&["\\u00e9"]).out, "é".as_bytes());
        assert_eq!(run(&["\\u00ff"]).out, "ÿ".as_bytes());
        assert_eq!(run(&["\\u20ac"]).out, "€".as_bytes());
        // Non-characters, which glibc's iconv encodes rather than refusing.
        assert_eq!(run(&["\\ufffe"]).out, b"\xef\xbf\xbe");
        assert_eq!(run(&["\\uffff"]).out, b"\xef\xbf\xbf");
        assert_eq!(run(&["\\U0001F600"]).out, "😀".as_bytes());
        assert_eq!(run(&["\\U0010FFFF"]).out, b"\xf4\x8f\xbf\xbf");
    }

    /// Above U+10FFFF there is no UTF-8 encoding, so gnulib's failure callback
    /// writes the escape back — upper-case hex, eight digits.
    #[test]
    fn a_code_point_beyond_utf8_writes_the_escape_back() {
        assert_eq!(run(&["\\U00110000|"]).out, b"\\U00110000|");
        assert_eq!(run(&["\\UFFFFFFFF|"]).out, b"\\UFFFFFFFF|");
    }

    #[test]
    fn a_surrogate_is_refused_and_a_short_name_is_too() {
        assert_eq!(
            run(&["\\ud800"]).stop.as_deref(),
            Some("invalid universal character name \\ud800")
        );
        assert_eq!(
            run(&["\\u00"]).stop.as_deref(),
            Some("missing hexadecimal number in escape")
        );
    }

    /// `\c` ends the program with status 0, even when an earlier argument
    /// already failed to convert.
    #[test]
    fn cancel_exits_successfully_and_overrides_an_earlier_failure() {
        let r = run(&["%d\\c", "abc"]);
        assert_eq!(r.out, b"0");
        assert_eq!(r.status, 0);
        let r = run(&["%b", "a\\c"]);
        assert_eq!(r.out, b"a");
        assert_eq!(r.status, 0);
        // Nothing after the cancel is printed, including a later pass over the
        // format.
        let r = run(&["%s\\cX", "a", "b"]);
        assert_eq!(r.out, b"a");
    }

    /// An argument that begins with a quote is a character constant, and its
    /// value is the byte after the quote.
    #[test]
    fn a_character_constant_is_the_character_after_the_quote() {
        assert_eq!(out(&["%d", "'a"]), "97");
        assert_eq!(out(&["%d", "\"a"]), "97");
        assert_eq!(out(&["%d", "'ab"]), "97");
        assert_eq!(out(&["%f", "'a"]), "97.000000");
        // A lone quote has nothing after it, so it is not a constant -- it goes
        // to the numeric parser and fails there.
        let r = run(&["%d", "'"]);
        assert_eq!(r.out, b"0");
        assert_eq!(r.status, 1);
        // `%c` has no constant handling: it takes the quote itself.
        assert_eq!(out(&["%c", "'a"]), "'");
    }

    #[test]
    fn q_is_the_shell_quoting_used_everywhere_else() {
        assert_eq!(out(&["%q", "hello"]), "hello");
        assert_eq!(out(&["%q", ""]), "''");
        assert_eq!(out(&["%q", "a b"]), "'a b'");
    }

    /// `--help` and `--version` are only themselves when they are the whole
    /// command line; anywhere else they are a format or an argument. `main`
    /// makes that check, so what is tested here is the half `run` owns.
    #[test]
    fn help_and_version_are_formats_when_they_are_not_alone() {
        assert_eq!(out(&["--help", "x"]), "--help");
        assert_eq!(out(&["%s", "--version"]), "--version");
    }

    #[test]
    fn a_leading_double_dash_is_stripped_once() {
        assert_eq!(out(&["--", "%s", "a"]), "a");
        // Only the first one: the second is the format.
        assert_eq!(out(&["--", "--", "a"]), "--");
    }

    #[test]
    fn no_operands_is_a_usage_error() {
        assert_eq!(run(&[]).stop.as_deref(), Some("missing operand"));
        assert_eq!(run(&["--"]).stop.as_deref(), Some("missing operand"));
    }

    /// A `-x` that is not an option is a format, because there is no option
    /// parser to reject it.
    #[test]
    fn an_unknown_dash_word_is_a_format() {
        assert_eq!(out(&["-x"]), "-x");
    }

    #[test]
    fn percent_percent_is_a_literal_percent() {
        assert_eq!(out(&["%%"]), "%");
        assert_eq!(out(&["100%%"]), "100%");
        assert_eq!(out(&["%d%%", "5"]), "5%");
    }

    /// The escaping this file does that GNU does not. It is one rule, applied
    /// in both sentences that echo caller-chosen bytes.
    #[test]
    fn caller_bytes_in_a_diagnostic_are_escaped() {
        assert_eq!(quote::escape_unprintable(b"abc"), "abc");
        assert_eq!(quote::escape_unprintable(b"a\nb"), "a\\012b");
        // A *character* is not caller-chosen bytes -- it is text, and printing
        // it in octal would make the sentence unmatchable against the argument
        // it is about. Only what cannot be read as text is escaped.
        assert_eq!(quote::escape_unprintable("é".as_bytes()), "é");
        assert_eq!(quote::escape_unprintable(b"\xc3"), "\\303");
        assert_eq!(
            run(&["%\n"]).stop.as_deref(),
            Some("%\\012: invalid conversion specification")
        );
    }

    /// The character constant is a whole character, not its first byte.
    ///
    /// Measured, GNU 9.4 under `LC_ALL=C.UTF-8` — the locale matters, because
    /// upstream picks the byte branch when `MB_CUR_MAX == 1` and `C` is the one
    /// locale where that holds. §356 settles that ours is always UTF-8.
    #[test]
    fn a_character_constant_is_a_code_point() {
        assert_eq!(out(&["%d", "'A"]), "65");
        assert_eq!(out(&["%d", "'é"]), "233");
        assert_eq!(out(&["%d", "'€"]), "8364");
        assert_eq!(out(&["%d", "'😀"]), "128512");
        assert_eq!(out(&["%d", "\"é"]), "233");
        // Every converter shares the extension, so every one must share the
        // decode: a `%d` that said 233 next to a `%x` that said c3 would be
        // two answers to one question.
        assert_eq!(out(&["%x", "'é"]), "e9");
        assert_eq!(out(&["%o", "'é"]), "351");
        assert_eq!(out(&["%f", "'é"]), "233.000000");
        // A byte that decodes to no character is worth its own value: there is
        // nothing else it could be, and refusing to answer would turn a
        // printable question into a fatal one.
        assert_eq!(out(&["%d", "'\u{fffd}"]), "65533");
        assert_eq!(
            {
                let mut printer = Printer {
                    out: Vec::new(),
                    status: 0,
                    stream_failed: false,
                    posixly_correct: true,
                };
                printer.strtoimax(b"'\xff")
            },
            255
        );
        // The trailing text a warning names starts *after* the whole
        // character, not after its first byte.
        assert_eq!(out(&["%d", "'éz"]), "233");
    }

    #[test]
    fn the_integer_scanner_reports_what_it_claimed() {
        let s = scan_integer(b"12x");
        assert_eq!((s.magnitude, s.consumed, s.overflowed), (12, 2, false));
        let s = scan_integer(b"abc");
        assert_eq!((s.magnitude, s.consumed), (0, 0));
        let s = scan_integer(b"");
        assert_eq!((s.magnitude, s.consumed), (0, 0));
        let s = scan_integer(b"  -0X1F");
        assert_eq!((s.magnitude, s.negative, s.consumed), (31, true, 7));
        let s = scan_integer(b"18446744073709551616");
        assert!(s.overflowed);
    }
}
