//! awk — the pattern-scanning and processing language.
//!
//! ```text
//! awk [-F sepstring] [-v assignment]... program [argument...]
//! awk [-F sepstring] -f progfile [-f progfile]... [-v assignment]... [argument...]
//! ```
//!
//! | | |
//! |---|---|
//! | `-F S` | field separator; `FS = S`, with escapes processed |
//! | `-v N=V` | assign before BEGIN runs; may be repeated |
//! | `-f F` | read the program from file F; may be repeated |
//! | `--` | end of options |
//!
//! Exit status: whatever `exit` was given, 0 otherwise, and 2 for a usage
//! error, a program that will not parse, or a fatal error at run time.
//!
//! ## What this used to be
//!
//! Until this rewrite, `awk` was a line filter with an awk-shaped command line.
//! It had no variables — not even `NR` — no assignment, no `if`, no loops, no
//! arrays, no user functions, no `printf`, no `getline`, no output
//! redirection, and no range patterns. `/re/` was `str::contains`, so
//! `awk '/^ *#/ {next}'` skipped every line containing a hash anywhere. The
//! condition evaluator's fall-through was `true`, so a pattern it could not
//! parse — most of them — silently matched everything, which is the worst
//! possible failure: no diagnostic, and an answer that looks like an answer.
//!
//! Rewiring that onto a real regex engine would have left every one of those
//! holes in place, so this is a real awk instead: POSIX's grammar, POSIX's
//! semantics, and `ere` for the patterns — the same engine `grep`, `sed`,
//! `expr` and the shell's `[[ =~ ]]` use, so all five agree about what `[a-z]`
//! means. See `design-decisions.md` §322.
//!
//! ## How the pieces fit
//!
//! | module | what it does |
//! |---|---|
//! | [`lex`] | bytes to tokens, and the two context-sensitive decisions awk's grammar needs |
//! | [`ast`] | the parsed shape; names are already resolved to slots |
//! | [`parse`] | recursive descent, POSIX precedence |
//! | [`types`] | which names are arrays — decided before the run, because arrays pass by reference |
//! | [`value`] | the strnum rule: a field that looks like a number compares as one, a program literal never does |
//! | [`fmt`] | C's `printf` over bytes |
//! | [`io`] | records (three `RS` modes) and redirections |
//! | [`interp`] | the tree-walking interpreter |
//!
//! ## Text is bytes
//!
//! A record, a field, a file name and a program are all `Vec<u8>`. A path on
//! this system may hold any byte but `/` and NUL, so an awk that insisted its
//! input was UTF-8 could not process a file listing — and `from_utf8_lossy` on
//! a record would replace the bytes the program was written to match. Where awk
//! counts characters rather than bytes — `length`, `substr`, `index`, `RSTART`,
//! `RLENGTH` — it counts them, using `ere`'s character model.
//!
//! ## Where this deliberately differs from gawk
//!
//! `scripts/awk-diff.sh` runs both awks over the same inputs and requires them
//! to agree. Ten cases are exempted there, and these are they — each is a
//! decision, not an omission, and the script reports if one stops being true.
//!
//! | Case | Ours | `gawk --posix` |
//! |---|---|---|
//! | `length`, `substr`, `index`, `toupper` on non-ASCII text | counts and maps characters | counts and maps bytes, in the C locale |
//! | `printf "%c"` of a code point above 255 | that character | the low byte |
//! | `printf "%s"` with no argument left | empty, as in bwk and mawk | fatal |
//! | `\1`–`\9` in a pattern | a backreference, as in GNU `grep -E` | the octal escape `\001` |
//! | an undefined function is called | refused before the program runs | fatal when first reached |
//! | a name used as both an array and a scalar | refused before the program runs | fatal when first reached |
//! | a built-in given the wrong number of arguments | refused before the program runs | fatal when first reached |
//!
//! The first two are the same decision twice: this system is UTF-8 throughout,
//! and gawk's byte answers are an artifact of the C locale on the development
//! host rather than something a user wants. The last three are one decision as
//! well — a program is checked whole before any of it runs, so a typo in a
//! branch that is rarely taken is found before the report is half printed
//! instead of after. That costs the gawk exit code for those cases (1 rather
//! than 2), which is the right trade: exit 1 already means "this program will
//! not run" and that is exactly what has happened.
//!
//! The backreference row is inherited rather than chosen for awk: `ere` grew
//! backreferences because `sed` and `grep` need them (`design-decisions.md`
//! §333), and all five programs share one engine so that a pattern means one
//! thing across them. gawk's reading is not POSIX either — POSIX leaves `\1` in
//! an ERE undefined — so the choice is between two extensions, and the one that
//! agrees with the other four programs on this system wins. A pattern whose
//! backreference search exceeds the engine's budget is a fatal error here, not
//! a non-match; see [`interp`]'s `From<ere::MatchLimit> for Fatal`.

mod ast;
mod fmt;
mod interp;
mod io;
mod lex;
mod parse;
mod types;
mod value;

use coreutils::diag;
use std::ffi::OsString;
use std::process;

use value::Str;

const USAGE: &str = "usage: awk [-F sepstring] [-v assignment]... program [argument...]\n       awk [-F sepstring] -f progfile [-f progfile]... [-v assignment]... [argument...]";

/// The command line, once the options have been taken off the front.
struct Args {
    /// `-f` program files, in order. Empty means the program is an operand.
    progfiles: Vec<Str>,
    /// `-v` assignments, in order, applied before BEGIN.
    assigns: Vec<(String, Str)>,
    /// `-F`, if given, as an `FS=` assignment applied after the `-v`s.
    fs: Option<Str>,
    /// The program text, when there was no `-f`.
    program: Option<Str>,
    /// What goes into `ARGV[1..]`.
    operands: Vec<Str>,
}

fn main() {
    let raw: Vec<Str> = std::env::args_os().skip(1).map(|a| arg_bytes(&a)).collect();
    let args = match parse_args(&raw) {
        Ok(Some(a)) => a,
        // The help and version paths have already printed.
        Ok(None) => return,
        Err(e) => die_usage(&e),
    };

    let source = match program_source(&args) {
        Ok(s) => s,
        Err(e) => die(&e),
    };

    // A program that will not compile is a *usage* failure — the script is
    // wrong before anything ran — and exits 1. A failure once it is running
    // exits 2. That split is gawk's, and a shell script that distinguishes them
    // at all has been written against gawk.
    let mut prog = match parse::parse(&source) {
        Ok(p) => p,
        Err(e) => die_program(&e),
    };
    if let Err(e) = types::resolve(&mut prog) {
        die_program(&e);
    }

    let env: Vec<(Str, Str)> = std::env::vars_os()
        .map(|(k, v)| (arg_bytes(&k), arg_bytes(&v)))
        .collect();
    let mut it = interp::Interp::new(prog, &args.operands, &env);

    // Order matters: `-v` runs before `-F`, and both run before BEGIN, so a
    // BEGIN block can read what the command line set and can override it.
    for (name, value) in &args.assigns {
        if let Err(e) = it.assign_cli(name, interp::unescape(value)) {
            die(&e);
        }
    }
    if let Some(fs) = &args.fs
        && let Err(e) = it.assign_cli("FS", interp::unescape(fs))
    {
        die(&e);
    }

    match it.run() {
        Ok(code) => process::exit(code),
        Err(e) => die(&e.0),
    }
}

/// Split the command line into options and operands.
///
/// Returns `Ok(None)` when `--help` or `--version` has already answered.
fn parse_args(raw: &[Str]) -> Result<Option<Args>, String> {
    let mut args = Args {
        progfiles: Vec::new(),
        assigns: Vec::new(),
        fs: None,
        program: None,
        operands: Vec::new(),
    };
    let mut i = 0usize;
    while let Some(arg) = raw.get(i) {
        let bytes = arg.as_slice();
        if bytes == b"--" {
            i = i.saturating_add(1);
            break;
        }
        if bytes == b"--help" {
            println!("{USAGE}");
            return Ok(None);
        }
        if bytes == b"--version" {
            println!("awk (SlateOS coreutils)");
            return Ok(None);
        }
        // A lone `-` is standard input, which is an operand, not an option.
        if bytes.len() < 2 || bytes.first() != Some(&b'-') {
            break;
        }
        // Options may be bundled with their argument (`-F:`) or take the next
        // word (`-F :`), and short flags may be run together (`-vx=1`), so the
        // letters are walked one at a time.
        let mut rest = bytes.get(1..).unwrap_or_default();
        i = i.saturating_add(1);
        while let Some(&flag) = rest.first() {
            rest = rest.get(1..).unwrap_or_default();
            match flag {
                b'F' | b'v' | b'f' => {
                    let value = if rest.is_empty() {
                        let Some(next) = raw.get(i) else {
                            return Err(format!("option -{} requires an argument", flag as char));
                        };
                        i = i.saturating_add(1);
                        next.clone()
                    } else {
                        let v = rest.to_vec();
                        rest = &[];
                        v
                    };
                    match flag {
                        b'F' => args.fs = Some(value),
                        b'f' => args.progfiles.push(value),
                        _ => {
                            let Some((name, v)) = interp::command_assignment(&value) else {
                                return Err(format!(
                                    "invalid -v assignment: {}",
                                    String::from_utf8_lossy(&value)
                                ));
                            };
                            args.assigns.push((name, v));
                        }
                    }
                }
                // `other` is a byte, not a character. `other as char` mapped
                // 0xC3 to `Ã`, so `awk -é` named an option nobody typed --
                // the same trap `sort`'s option loop documents avoiding. One
                // octal escape per byte is what this loop can honestly say,
                // because it walks bytes: it has no way to know whether the
                // byte after it belongs to the same character or is the next
                // flag in a bundle. (The lexer's `shown_char` *can* know, and
                // so names the whole character -- see `lex.rs`.)
                other => {
                    let shown = coreutils::quote::escape_unprintable(&[other]);
                    return Err(format!("unknown option -{shown}"));
                }
            }
        }
    }

    if args.progfiles.is_empty() {
        let Some(text) = raw.get(i) else {
            return Err("no program text".to_string());
        };
        i = i.saturating_add(1);
        args.program = Some(text.clone());
    }
    args.operands = raw.get(i..).unwrap_or_default().to_vec();
    Ok(Some(args))
}

/// The program text: the `-f` files joined by newlines, or the operand.
fn program_source(args: &Args) -> Result<Str, String> {
    if let Some(text) = &args.program {
        return Ok(text.clone());
    }
    let mut out = Str::new();
    for name in &args.progfiles {
        let text = if name.as_slice() == b"-" {
            let mut buf = Str::new();
            std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf)
                .map_err(|e| format!("can't open file -: {}", coreutils::errmsg::strerror(&e)))?;
            buf
        } else {
            std::fs::read(io::os_path(name)).map_err(|e| {
                format!(
                    "can't open file {}: {}",
                    String::from_utf8_lossy(name),
                    coreutils::errmsg::strerror(&e)
                )
            })?
        };
        out.extend_from_slice(&text);
        // A `-f` file that does not end in a newline must not run its last
        // statement into the first statement of the next file.
        if !out.ends_with(b"\n") {
            out.push(b'\n');
        }
    }
    Ok(out)
}

/// An argument as bytes.
///
/// On a platform whose arguments are already bytes — SlateOS, and Unix — this
/// is exact. On the development host, where they are UTF-16, an argument that
/// is not valid Unicode cannot be expressed and the lossy conversion is all
/// that is left; it affects only the host.
#[cfg(unix)]
fn arg_bytes(a: &OsString) -> Str {
    use std::os::unix::ffi::OsStrExt;
    a.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn arg_bytes(a: &OsString) -> Str {
    a.to_string_lossy().into_owned().into_bytes()
}

/// A failure once the program is running, or before it because a file it named
/// could not be opened.
fn die(msg: &str) -> ! {
    diag!("awk: {msg}");
    process::exit(2)
}

/// A program that will not compile.
fn die_program(msg: &str) -> ! {
    diag!("awk: {msg}");
    process::exit(1)
}

/// A command line that does not make sense.
fn die_usage(msg: &str) -> ! {
    diag!("awk: {msg}");
    diag!("{USAGE}");
    process::exit(1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    /// The refusal `parse_args` gives for `argv`, or a panic if it accepted it.
    ///
    /// Written as a `match` rather than `unwrap_err` so that `Args` need not
    /// derive `Debug` purely to satisfy that method's bound on the `Ok` type —
    /// a test should not widen a production type's API to be writable.
    fn err(argv: &[&[u8]]) -> String {
        let raw: Vec<Str> = argv.iter().map(|a| a.to_vec()).collect();
        match parse_args(&raw) {
            Err(message) => message,
            Ok(_) => panic!("expected these arguments to be refused: {argv:?}"),
        }
    }

    /// An unknown option byte is escaped, not cast to a `char`.
    ///
    /// `other as char` interpreted the byte as a code point, so the first byte
    /// of `é` (0xC3) came back as `Ã` — a letter that is not on the command
    /// line and not on the keyboard the user typed it with. `sort`'s option
    /// loop carries a comment about this exact trap; awk's had the bug.
    #[test]
    fn an_unknown_option_byte_is_escaped_rather_than_cast_to_a_character() {
        // `-é` is two bytes, both of them unknown options. The loop stops at
        // the first and escapes it; it does not reach for the second, because
        // walking bytes it cannot tell a continuation byte from the next flag
        // in a bundle. `lex.rs`'s `shown_char` has the whole program text and
        // so can, and does, name the character instead.
        assert_eq!(err(&["-é".as_bytes()]), r"unknown option -\303");
        assert_eq!(err(&[b"-\xff"]), r"unknown option -\377");
        assert_eq!(err(&[b"-\x01"]), r"unknown option -\001");
        // ASCII is unchanged, which is every option anyone actually mistypes.
        assert_eq!(err(&[b"-q"]), "unknown option -q");
        assert_eq!(err(&[b"-@"]), "unknown option -@");
    }
}
