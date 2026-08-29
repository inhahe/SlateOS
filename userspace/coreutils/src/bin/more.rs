//! more — file perusal filter for viewing text one screen at a time.
//!
//! Usage: more [FILE...]
//!   Displays text one screen at a time.
//!   Press Enter for next line, Space for next page, q to quit.
//!   Without files, reads from stdin.
//!
//! # A pager may not decide what its input means
//!
//! Everything here is bytes: the file names from argv, and the file contents
//! on the way to stdout. That is not stylistic. This program used to read its
//! input with [`BufRead::lines`], which yields `String` and therefore *fails*
//! on a line that is not valid UTF-8 — and the failure was handled with
//! `Err(_) => break`, so `more` on a file holding one stray byte printed the
//! lines before it, stopped, and **exited 0**. A pager that silently shows you
//! part of a file is worse than one that refuses: nothing on screen says the
//! rest exists. Reconstructing each line into a `String` and printing it with
//! `writeln!` also appended a newline the file did not have.
//!
//! The loop is now `read_until(b'\n')` and `write_all`, which copies the file's
//! bytes through unexamined and reproduces a missing final newline.
//!
//! # When it pages, and when it labels
//!
//! Both of those are decided by whether a *terminal* is on the other end, and
//! this program used to decide neither — it paged unconditionally and labelled
//! on operand count alone. Both rules below were measured against util-linux
//! `more` 2.39.3 over the full `{1,2 files} × {stdin tty, pipe} × {stdout tty,
//! pipe}` matrix; each is the only rule that fits every cell.
//!
//! - **Page only when stdout is a terminal.** `more big.txt | cat` is a copy,
//!   not a conversation. Paging into a pipe was not merely cosmetic here: the
//!   keystroke read hit EOF immediately, `read_key` mapped that to `Quit`, and
//!   the pipeline received *one screen* of a file the user asked for all of —
//!   silently, with status 0. That is the same class of bug as the UTF-8 one
//!   above, arrived at from the other direction.
//! - **Print the `::::` banner when there is more than one operand, or when
//!   stdin is not a terminal.** The second half is what makes `more f > out`
//!   label its output while `more f` on a terminal does not, and it is keyed
//!   on stdin because that is what tells `more` nobody is going to answer a
//!   prompt.
//!
//! See `known-issues.md` → `B-more-STOPPED-PAGING-AT-THE-FIRST-NON-UTF8-BYTE`.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::quote::{os_bytes, quotef};
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};

/// The banner printed above each file's name. Fourteen colons, measured
/// against util-linux `more` 2.39.3; it was thirteen here.
const BANNER: &[u8] = b"::::::::::::::";

fn main() {
    let operands: Vec<OsString> = env::args_os().skip(1).collect();

    let stdin_is_tty = io::stdin().is_terminal();
    let banners = wants_banners(operands.len(), stdin_is_tty);

    // Computed once, before any output: `keys` being `None` is what "do not
    // page" means, so it must not be re-derived per file and drift.
    let mut keys = command_source(stdin_is_tty);
    let lines_per_page = terminal_lines(env::var("LINES").ok().as_deref()).saturating_sub(1);

    let files = if operands.is_empty() {
        vec![OsString::from("-")]
    } else {
        operands
    };

    // One lock for the whole run, so the headers and the file bodies cannot
    // interleave and there is a single place to flush before returning.
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for path in &files {
        let name = os_bytes(path);

        let mut reader: Box<dyn Read> = if name.as_ref() == b"-" {
            // A deliberate divergence, and the only one: util-linux has no
            // `-` convention and reports `cannot open -: No such file or
            // directory`. Every other utility in this tree spells stdin `-`,
            // and a pager that alone refused it would be the surprise. Note
            // that stdin gets no banner even when `banners` is set — there is
            // no file name to put in one, and util-linux's own stdin copy is
            // likewise unlabelled.
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => {
                    // Opening a directory succeeds; it is the *read* that
                    // fails, with EISDIR, after the banner has already
                    // claimed a file is about to appear. util-linux stats
                    // first and says so in-band on stdout, where the reader
                    // is looking, then carries on with status 0.
                    if f.metadata().is_ok_and(|m| m.is_dir()) {
                        write_ignoring_errors(&mut out, &directory_marker(&name));
                        continue;
                    }
                    // After the open, never before: a banner printed first
                    // names a file that a failed open then never shows.
                    if banners {
                        // The name goes in as bytes rather than formatted: it
                        // is a filename, and on this OS that is any byte but
                        // `/` and NUL. util-linux prints it raw here too.
                        write_ignoring_errors(&mut out, &file_header(&name));
                    }
                    Box::new(f)
                }
                Err(e) => {
                    diag!("more: cannot open {}: {}", quotef(&name), strerror(&e));
                    continue;
                }
            }
        };

        if !page(&mut reader, &name, &mut out, &mut keys, lines_per_page) {
            break;
        }
    }

    let _ = out.flush(); // see write_ignoring_errors
}

/// Copy one input to `out`, pausing every `lines_per_page` lines if there is
/// somewhere to read a keystroke from.
///
/// Returns `false` when the user asked to quit, which ends the whole run and
/// not just this file.
fn page(
    reader: &mut dyn Read,
    name: &[u8],
    out: &mut impl Write,
    keys: &mut Option<Box<dyn Read>>,
    lines_per_page: usize,
) -> bool {
    let mut buf = BufReader::new(reader);
    let mut line_count: usize = 0;
    let mut line: Vec<u8> = Vec::new();

    loop {
        line.clear();
        match buf.read_until(b'\n', &mut line) {
            // 0 bytes is end of input. A short read is not: `read_until`
            // has already looped for us and only stops at the delimiter or
            // at EOF, so a final line with no newline arrives here intact
            // and is written back without one — which is what the file
            // holds and what util-linux prints.
            Ok(0) => return true,
            Ok(_) => {}
            // A read error is the one case where stopping is right, but it
            // must not be silent: this is where the UTF-8 failure used to
            // land, mislabelled as end of file.
            Err(e) => {
                diag!("more: {}: {}", quotef(name), strerror(&e));
                return true;
            }
        }

        write_ignoring_errors(out, &line);
        line_count = line_count.saturating_add(1);

        // No keystroke source means stdout is not a terminal, so there is no
        // screen to fill and nothing to wait for.
        let Some(src) = keys.as_mut() else { continue };
        if line_count < lines_per_page {
            continue;
        }

        // The prompt goes to stdout, not stderr: paging happens only when
        // stdout *is* the terminal, so that is where the pager's screen is.
        // On descriptor 2 it would vanish under `more f 2>/dev/null` and the
        // pager would look hung.
        write_ignoring_errors(out, b"--More--");
        let _ = out.flush(); // see write_ignoring_errors

        match read_key(src.as_mut()) {
            Key::Quit => return false,
            Key::Line => line_count = lines_per_page.saturating_sub(1),
            Key::Page => line_count = 0,
        }

        write_ignoring_errors(out, b"\r        \r");
    }
}

/// Where the pager reads keystrokes — and therefore whether it pages at all.
///
/// `None` means "copy straight through": stdout is not a terminal, so there is
/// no screen to fill. Returning it is the fix for a pager that used to page
/// into a pipe, read EOF where a keystroke should have been, and treat that as
/// the user pressing `q`.
///
/// When stdin has been redirected but stdout is still a terminal, commands
/// cannot come from stdin — that descriptor is somebody's data, and reading it
/// for keystrokes would eat it. util-linux reopens the controlling terminal
/// for this and so do we; if there is none, we fall back to not paging, which
/// shows the whole file rather than a truncated one.
fn command_source(stdin_is_tty: bool) -> Option<Box<dyn Read>> {
    if !io::stdout().is_terminal() {
        return None;
    }
    if stdin_is_tty {
        return Some(Box::new(io::stdin()));
    }
    File::open("/dev/tty")
        .ok()
        .map(|f| Box::new(f) as Box<dyn Read>)
}

/// Whether each file's name is announced in a `::::` banner.
///
/// More than one operand is the obvious half. The other half — stdin not being
/// a terminal — is measured, not guessed: util-linux labels a lone file for
/// `more f < /dev/null` and leaves it unlabelled for `more f` at a prompt,
/// with stdout a pipe in both cases. Keying it on stdin rather than stdout is
/// what makes `more f | cat` from a terminal come out clean.
fn wants_banners(operands: usize, stdin_is_tty: bool) -> bool {
    operands > 1 || !stdin_is_tty
}

/// Write to stdout and discard the result, deliberately.
///
/// Discarding a write error is normally a defect in this tree, so the reason is
/// worth stating: `more plain.txt > /dev/full` on util-linux 2.39.3 writes no
/// diagnostic and **exits 0** (measured). More importantly, the common way a
/// pager's stdout ends is `more f | head`, where the pipe closing is the
/// pipeline working, not a failure — and unlike `cat`, `more` has no caller
/// that is going to treat its output as data to be checked. Matching the
/// reference is the right call here; the exit status stays what it was.
fn write_ignoring_errors(out: &mut impl Write, bytes: &[u8]) {
    let _ = out.write_all(bytes);
}

#[derive(Debug, PartialEq, Eq)]
enum Key {
    Page, // space
    Line, // enter
    Quit, // q
}

fn read_key(src: &mut dyn Read) -> Key {
    let mut buf = [0u8; 1];
    match src.read(&mut buf) {
        Ok(0) | Err(_) => Key::Quit,
        Ok(_) => parse_key_byte(buf.first().copied().unwrap_or(b' ')),
    }
}

/// Translate one byte of user input into a `Key` action.
fn parse_key_byte(b: u8) -> Key {
    match b {
        b'q' | b'Q' => Key::Quit,
        b' ' => Key::Page,
        b'\n' | b'\r' => Key::Line,
        _ => Key::Page,
    }
}

/// Compute the terminal line count from a `LINES` env value; falls back to 24.
fn terminal_lines(env_value: Option<&str>) -> usize {
    if let Some(val) = env_value
        && let Ok(n) = val.parse::<usize>()
        && n > 0
    {
        return n;
    }
    24
}

/// Build the three header lines printed before a file, as bytes, with the
/// trailing newline on each.
fn file_header(path: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(path.len().saturating_add(BANNER.len() * 2 + 3));
    v.extend_from_slice(BANNER);
    v.push(b'\n');
    v.extend_from_slice(path);
    v.push(b'\n');
    v.extend_from_slice(BANNER);
    v.push(b'\n');
    v
}

/// What is printed in place of a directory's contents.
///
/// On stdout, not stderr, and byte-for-byte util-linux's: `\n*** NAME:
/// directory ***\n\n`. A pager's user is reading stdout; a note about why one
/// of the requested files produced nothing belongs in the same stream as the
/// files that did.
fn directory_marker(path: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(path.len().saturating_add(22));
    v.extend_from_slice(b"\n*** ");
    v.extend_from_slice(path);
    v.extend_from_slice(b": directory ***\n\n");
    v
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn quit_keys() {
        assert_eq!(parse_key_byte(b'q'), Key::Quit);
        assert_eq!(parse_key_byte(b'Q'), Key::Quit);
    }

    #[test]
    fn space_is_page() {
        assert_eq!(parse_key_byte(b' '), Key::Page);
    }

    #[test]
    fn newline_is_line() {
        assert_eq!(parse_key_byte(b'\n'), Key::Line);
        assert_eq!(parse_key_byte(b'\r'), Key::Line);
    }

    #[test]
    fn unknown_byte_defaults_to_page() {
        assert_eq!(parse_key_byte(b'x'), Key::Page);
        assert_eq!(parse_key_byte(0), Key::Page);
        assert_eq!(parse_key_byte(255), Key::Page);
    }

    #[test]
    fn terminal_lines_default_when_unset() {
        assert_eq!(terminal_lines(None), 24);
    }

    #[test]
    fn terminal_lines_parses_env() {
        assert_eq!(terminal_lines(Some("40")), 40);
    }

    #[test]
    fn terminal_lines_falls_back_on_garbage() {
        assert_eq!(terminal_lines(Some("notanumber")), 24);
    }

    #[test]
    fn terminal_lines_falls_back_on_zero() {
        assert_eq!(terminal_lines(Some("0")), 24);
    }

    #[test]
    fn terminal_lines_falls_back_on_empty() {
        assert_eq!(terminal_lines(Some("")), 24);
    }

    #[test]
    fn file_header_contains_path() {
        assert_eq!(
            file_header(b"data.txt"),
            b"::::::::::::::\ndata.txt\n::::::::::::::\n"
        );
    }

    #[test]
    fn file_header_banner_is_fourteen_colons() {
        // Measured against util-linux more 2.39.3, which prints fourteen.
        // This was thirteen, which is the sort of difference nothing catches
        // by eye and every diff catches immediately.
        assert_eq!(BANNER.len(), 14);
        assert!(BANNER.iter().all(|&b| b == b':'));
    }

    #[test]
    fn file_header_with_unusual_chars() {
        let h = file_header(b"a b/c.txt");
        assert!(h.windows(9).any(|w| w == b"a b/c.txt"));
    }

    #[test]
    fn file_header_passes_a_non_utf8_name_through() {
        // The banner names the file being paged, so a lossy copy here would
        // name a file the user does not have. util-linux prints the bytes.
        let h = file_header(b"caf\xe9.txt");
        assert_eq!(h, b"::::::::::::::\ncaf\xe9.txt\n::::::::::::::\n");
    }

    #[test]
    fn directory_marker_matches_util_linux() {
        assert_eq!(directory_marker(b"dir"), b"\n*** dir: directory ***\n\n");
    }

    #[test]
    fn directory_marker_passes_a_non_utf8_name_through() {
        assert_eq!(
            directory_marker(b"caf\xe9"),
            b"\n*** caf\xe9: directory ***\n\n"
        );
    }

    #[test]
    fn banner_rule_matches_util_linux() {
        // Every cell of the measured matrix. The interesting ones are the
        // two single-file rows: the banner appears with stdin redirected and
        // not at an interactive prompt.
        assert!(!wants_banners(1, true));
        assert!(wants_banners(1, false));
        assert!(wants_banners(2, true));
        assert!(wants_banners(2, false));
        // No operands means stdin, which is never banner-ed; the count is
        // still zero here rather than one, because `-` is substituted after
        // this decision is made.
        assert!(!wants_banners(0, true));
    }

    #[test]
    fn page_copies_everything_when_there_is_no_keystroke_source() {
        // The regression this whole rework exists for: with `keys` at `None`
        // -- stdout not a terminal -- a file longer than a screen must come
        // out whole, not one page of it.
        let body: Vec<u8> = (1..=60)
            .flat_map(|n| format!("{n}\n").into_bytes())
            .collect();
        let mut input = body.as_slice();
        let mut out: Vec<u8> = Vec::new();
        let mut keys: Option<Box<dyn Read>> = None;

        assert!(page(&mut input, b"sixty", &mut out, &mut keys, 23));
        assert_eq!(out, body);
    }

    #[test]
    fn page_reproduces_a_missing_final_newline() {
        let mut input: &[u8] = b"no-newline";
        let mut out: Vec<u8> = Vec::new();
        let mut keys: Option<Box<dyn Read>> = None;

        assert!(page(&mut input, b"nn", &mut out, &mut keys, 23));
        assert_eq!(out, b"no-newline");
    }

    #[test]
    fn page_copies_bytes_that_are_not_utf8() {
        let mut input: &[u8] = b"one\ntw\xffo\nthree\n";
        let mut out: Vec<u8> = Vec::new();
        let mut keys: Option<Box<dyn Read>> = None;

        assert!(page(&mut input, b"bad", &mut out, &mut keys, 23));
        assert_eq!(out, b"one\ntw\xffo\nthree\n");
    }

    #[test]
    fn page_stops_the_run_when_the_key_is_q() {
        let mut input: &[u8] = b"a\nb\nc\n";
        let mut out: Vec<u8> = Vec::new();
        let mut keys: Option<Box<dyn Read>> = Some(Box::new(&b"q"[..]));

        assert!(!page(&mut input, b"abc", &mut out, &mut keys, 1));
        assert_eq!(out, b"a\n--More--");
    }

    #[test]
    fn page_continues_on_space() {
        let mut input: &[u8] = b"a\nb\n";
        let mut out: Vec<u8> = Vec::new();
        let mut keys: Option<Box<dyn Read>> = Some(Box::new(&b"  "[..]));

        assert!(page(&mut input, b"ab", &mut out, &mut keys, 1));
        assert_eq!(out, b"a\n--More--\r        \rb\n--More--\r        \r");
    }
}
