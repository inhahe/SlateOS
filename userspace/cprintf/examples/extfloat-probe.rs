//! Expose [`cprintf::extfloat`] to the differential harness.
//!
//! `scripts/extfloat-diff.sh` runs this and a C program built against glibc
//! over the same cases and compares byte for byte. It is an *example* rather
//! than a binary because a binary is installed into the image, and this is a
//! test instrument.
//!
//! Reading and writing have modes of their own, because they fail in different
//! ways and a harness that only checked the round trip would let a parse error
//! and a formatting error cancel out:
//!
//! ```text
//! extfloat-probe read     # one line per case: LITERAL
//! extfloat-probe readd    # the same, through strtod
//! extfloat-probe readf    # the same, through strtof
//! extfloat-probe write    # one line per case: FORMAT<TAB>LITERAL
//! ```
//!
//! `read` answers what `strtold` did — how much of the string it consumed,
//! whether it set `ERANGE`, and the exact value it produced, printed as `%La`
//! so the answer is the bits rather than a rounded view of them. `write`
//! answers what `printf` would print for a value the C side parses the same
//! way, which is only meaningful once `read` agrees. `readd` and `readf` answer
//! the `read` question for `strtod` and `strtof`, with the value as its bits in
//! hex: `%a` would print every NaN as `nan`, and these keep a NaN's payload.
//!
//! Every mode reads stdin to end and write **one line per input line**, so a
//! mismatch's line number is the case's line number. That includes empty input
//! lines: an empty numeral is a case — `strtold("")` consumes nothing — and a
//! probe that silently dropped it would shift every later answer up by one and
//! report the whole file as different.

use std::ffi::OsString;
use std::io::{Read, Write};

use cprintf::extfloat::{self, Spec};
use quoting::os_bytes;

fn main() {
    // `args_os`, not `args`: the iterator that yields `String` unwraps, so it
    // aborts the probe on an argument that is not UTF-8 before the mode is
    // even looked at -- and a harness invoking it wrongly deserves the
    // "expected 'read' or 'write'" line, not a panic message.
    let mut args = std::env::args_os().skip(1);
    let mode: OsString = args.next().unwrap_or_default();
    let mode = os_bytes(&mode);

    let mut input = Vec::new();
    if std::io::stdin().read_to_end(&mut input).is_err() {
        std::process::exit(1);
    }

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    let mut ok = true;

    // Splitting on the separator leaves one empty piece after a trailing
    // newline, which is not a case; an interior empty line is.
    let body = input.strip_suffix(b"\n").unwrap_or(&input);
    for line in body
        .split(|&c| c == b'\n')
        .take(if input.is_empty() { 0 } else { usize::MAX })
    {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let rendered = match &*mode {
            b"read" => read_case(line),
            b"readd" => read_double_case(line),
            b"readf" => read_float_case(line),
            b"write" => write_case(line),
            _ => {
                eprintln!("extfloat-probe: expected 'read', 'readd', 'readf' or 'write'");
                std::process::exit(2);
            }
        };
        if out.write_all(rendered.as_bytes()).is_err() || out.write_all(b"\n").is_err() {
            ok = false;
            break;
        }
    }

    if out.flush().is_err() || !ok {
        std::process::exit(1);
    }
}

/// What `strtold` made of the whole line.
fn read_case(line: &[u8]) -> String {
    let got = extfloat::strtold(line);
    // `%La` is the only conversion that shows every significand bit without
    // going through a decimal expansion, so a disagreement here is a
    // disagreement about the value and not about how it was printed.
    let bits = extfloat::render(
        &Spec {
            minus: false,
            plus: false,
            space: false,
            hash: false,
            zero: false,
            width: 0,
            precision: None,
            conv: b'a',
        },
        got.value,
    );
    format!(
        "consumed={} range={} value={bits}",
        got.consumed,
        u8::from(got.range_error)
    )
}

/// What `strtod` made of the whole line.
fn read_double_case(line: &[u8]) -> String {
    let got = extfloat::strtod(line);
    format!(
        "consumed={} range={} bits={:016x}",
        got.consumed,
        u8::from(got.range_error),
        got.value.to_bits()
    )
}

/// What `strtof` made of the whole line.
fn read_float_case(line: &[u8]) -> String {
    let got = extfloat::strtof(line);
    format!(
        "consumed={} range={} bits={:08x}",
        got.consumed,
        u8::from(got.range_error),
        got.value.to_bits()
    )
}

/// What `printf(FORMAT, strtold(LITERAL))` would write.
fn write_case(line: &[u8]) -> String {
    let Some(tab) = line.iter().position(|&c| c == b'\t') else {
        return "!no-tab".to_string();
    };
    let (fmt, literal) = line.split_at(tab);
    let literal = literal.get(1..).unwrap_or_default();
    let Some((spec, used)) = Spec::parse(fmt) else {
        return "!bad-format".to_string();
    };
    if used != fmt.len() {
        return "!trailing-format".to_string();
    }
    let Some(v) = extfloat::xstrtold(literal) else {
        return "!bad-literal".to_string();
    };
    extfloat::render(&spec, v)
}
