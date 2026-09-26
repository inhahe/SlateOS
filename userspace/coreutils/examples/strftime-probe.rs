//! Expose `localtime::strftime` to the differential harness.
//!
//! `scripts/strftime-diff.sh` runs this and `scripts/strftime-probe.c`, a C
//! program calling glibc's `strftime`, over the same cases and compares them
//! byte for byte. It is an *example* rather than a `src/bin/*.rs` because
//! everything in that directory is a coreutil installed into the image, and
//! this is a test instrument. (gnulib's `nstrftime` needs no probe: GNU `date`
//! formats with it, so the harness compares `date` against `date`.)
//!
//! One case per stdin line, `SECONDS<TAB>FORMAT`; one line out per case, in
//! the C probe's escaping -- `[`, the bytes with `\\` doubled and control
//! characters as `\xNN`, `]` -- or `ERR` where `localtime` would fail.

use std::io::{BufRead, Write};

use localtime::{Zone, strftime};

fn main() {
    let zone = Zone::from_env();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut line = Vec::new();
    let mut reader = stdin.lock();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        let answer = case(&zone, &line);
        if out.write_all(&answer).is_err() {
            break;
        }
    }
}

/// One case's line of output.
fn case(zone: &Zone, line: &[u8]) -> Vec<u8> {
    let Some(tab) = line.iter().position(|&b| b == b'\t') else {
        return b"BAD\n".to_vec();
    };
    let (secs, format) = line.split_at(tab);
    let format = format.get(1..).unwrap_or(&[]);
    // A C string: the format ends at its first NUL.
    let format = format.split(|&b| b == 0).next().unwrap_or(&[]);
    let secs = std::str::from_utf8(secs)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(0);
    if zone.localtime_r(secs).is_none() {
        return b"ERR\n".to_vec();
    }
    let text = strftime(format, &zone.local(secs, 0));
    let mut out = vec![b'['];
    for &c in &text {
        match c {
            b'\\' => out.extend_from_slice(b"\\\\"),
            c if c < 0x20 || c == 0x7f => out.extend_from_slice(format!("\\x{c:02x}").as_bytes()),
            c => out.push(c),
        }
    }
    out.extend_from_slice(b"]\n");
    out
}
