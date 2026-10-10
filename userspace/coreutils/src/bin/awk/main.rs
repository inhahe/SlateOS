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
//! | `ere::awk` | gawk's two escape layers -- a string's text, and a regex's before the compiler sees it -- shared with the kernel shell's awk |
//! | [`source`] | which file and line an offset in the program text is, as a diagnostic names it |
//! | [`ast`] | the parsed shape; names are already resolved to slots |
//! | [`parse`] | recursive descent, POSIX precedence |
//! | [`types`] | which names are arrays — decided before the run, because arrays pass by reference |
//! | [`value`] | the strnum rule: a field that looks like a number compares as one, a program literal never does |
//! | [`array`] | arrays laid out as gawk lays them -- its three layouts, hashes and growth -- so `for (k in a)` runs in gawk's order |
//! | [`fmt`] | `printf`: gawk's `format_tree` over bytes, on Rust's exact float digits |
//! | [`io`] | records (three `RS` modes) and redirections |
//! | [`compile`] | the parsed program to instructions, in gawk's order and with gawk's lines |
//! | [`interp`] | the running program's state, and the loop that runs the instructions |
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
//! to agree. The cases it exempts are recorded there with their reasons, and
//! the script reports one that stops differing. Most are these — each a
//! decision, not an omission; the rest are gaps of ours, tracked in
//! `known-issues.md` (B-AWK-GAWK-FIDELITY-SWEEP).
//!
//! A diagnostic says where it happened as gawk's does: `awk: cmd. line:2:
//! (FILENAME=f FNR=7) fatal: ...`, the program's line (or `prog.awk:2:` for a
//! `-f` file) and, once a record has been read, the input's place. See
//! [`source`] and `Interp::diagnostic_prefix`.
//!
//! | Case | Ours | `gawk --posix` |
//! |---|---|---|
//! | `length`, `substr`, `index`, `toupper` on non-ASCII text | counts and maps characters | counts and maps bytes, in the C locale |
//! | `printf "%c"` of a code point above 255 | that character | the low byte |
//! | `printf "%s"` with no argument left | empty, as in bwk and mawk | fatal |
//! | an undefined function is called | refused before the program runs | fatal when first reached |
//! | a name used as both an array and a scalar | refused before the program runs | fatal when first reached |
//! | a built-in given the wrong number of arguments | refused before the program runs | fatal when first reached |
//! | `RS` longer than one character | a regex, as in gawk without `--posix`, mawk and the one true awk | its first character only |
//! | `ENVIRON` when `AWKPATH` or `AWKLIBPATH` is unset | as the environment has it | both added, naming gawk's own library directories |
//! | `print > "/dev/stderr"` (and `"/dev/stdout"`) | the standard stream itself, which SlateOS need not have as a file | a file of that name, opened afresh -- so with descriptor 2 closed it reopens the `/dev/null` gawk's `init_fds` put there, and the print succeeds |
//! | `length(arr)` | the number of elements, as POSIX.1-2024 specifies | fatal: gawk 5.2.1's `--posix` predates it |
//! | a field past `NF`, `$(NF+1) == 0` | the uninitialized value, equal to `0` and `""` alike, as POSIX says | the empty string, unequal to `0` |
//! | gawk's own variable names: `ARGIND`, `BINMODE`, `ERRNO`, `FIELDWIDTHS`, `FPAT`, `IGNORECASE`, `LINT`, `PREC`, `ROUNDMODE`, `RT`, `TEXTDOMAIN` | the program's, unset until it sets them | predefined (`PREC` is 53, `TEXTDOMAIN` `messages`), refused as arrays and function names; `LINT = 1` turns gawk's lint warnings on |
//! | a number never yet turned into text, as an `ARGV` entry or as `FILENAME` in a diagnostic | its text, `%d` or `CONVFMT` | none: the `ARGV` entry is skipped as empty, the diagnostic leaves `FILENAME` out |
//!
//! The `RS` row is a choice POSIX leaves open ("If RS contains more than one
//! character, the results are unspecified"), and the regex is what a program
//! that sets one means: `RS = "\r\n"` for a file with CRLF line ends would
//! otherwise split at the `\r` and glue each `\n` to the next record. (A row
//! that said standard output's reader going away ended the run quietly is
//! gone: since 2026-10-07 this awk dies of `SIGPIPE` there as gawk does,
//! `trap '' PIPE` or not -- gawk's `die_via_sigpipe`.) The `ENVIRON` row: those two
//! are where gawk searches for `-f` files and loads extensions, directories of
//! gawk's own installation; this awk searches no path for `-f` and loads no
//! extensions, so the entries would name directories it never reads.
//!
//! The next three are POSIX's answer where gawk 5.2.1's `--posix` gives
//! another. `length(arr)` was standardised in POSIX.1-2024 (Austin Group
//! issue 1566), after gawk 5.2.1 was released. A nonexistent field "shall
//! evaluate to the uninitialized value". And POSIX reserves no variable names
//! but its own, so a program that keeps a running `PREC` or its own `RT` must
//! find them unset; gawk installs all its variables whatever the mode
//! (`init_vars`), which only a program written for gawk would want. The last
//! row is gawk's cache leaking out: a number has text in gawk only once
//! something has formatted it, and whether `ARGV[1] = 5` opens the file `5`
//! depends on whether an earlier statement happened to print it. Here a
//! number always has its text; keeping gawk's cache would cost every number
//! a shared cell for the one place it shows.
//!
//! The first two are the same decision twice: this system is UTF-8 throughout,
//! and gawk's byte answers are an artifact of the C locale on the development
//! host rather than something a user wants. The three refused before the
//! program runs are one decision as well — a program is checked whole before any of it runs, so a typo in a
//! branch that is rarely taken is found before the report is half printed
//! instead of after. That costs the gawk exit code for those cases (1 rather
//! than 2), which is the right trade: exit 1 already means "this program will
//! not run" and that is exactly what has happened.
//!
//! There was another row until 2026-10-01: `\1`–`\9` in a pattern was a
//! backreference here, as in GNU `grep -E`, and the octal escape `\001` in
//! gawk. It was recorded as a choice between two extensions on the belief that
//! POSIX leaves `\1` undefined, and that belief was wrong for awk: POSIX's awk
//! gives its regexes C's escapes plus the octal `\ddd`, "recognized both inside
//! and outside bracket expressions", so `\1` is the byte 0x01, exactly as gawk
//! reads it. `ere::awk` is that layer, gawk's, in front of the engine the other
//! programs share (`design-decisions.md` §333 and the entry that revisits it).
//! A pattern whose search exceeds the engine's budget is still a fatal error
//! here, not a non-match; see [`interp`]'s `From<ere::MatchLimit> for Fatal`.

mod array;
mod ast;
mod compile;
mod fmt;
mod interp;
mod io;
mod lex;
mod parse;
mod source;
mod types;
mod value;

use coreutils::diag;
use coreutils::stdfd;
use std::ffi::OsString;
use std::process::ExitCode;

// Before `main`, so that `stdfd::restore` still sees the descriptors awk was
// given: a closed standard input is a read error and a closed standard output
// `error writing standard output`, as they are gawk's -- not the `/dev/null`
// Rust's runtime would put on each.
coreutils::guard_std_fds!();

use value::Str;

const USAGE: &str = "usage: awk [-F sepstring] [-v assignment]... program [argument...]\n       awk [-F sepstring] -f progfile [-f progfile]... [-v assignment]... [argument...]";

/// One of the command line's assignments, as written.
enum Preassign {
    /// `-v name=value`.
    Var(String, Str),
    /// `-F value`, which POSIX defines as `-v FS=value` -- except that gawk
    /// does not elide a backslash-newline in it, so it stays distinct.
    Fs(Str),
}

/// The command line, once the options have been taken off the front.
struct Args {
    /// `-f` program files, in order. Empty means the program is an operand.
    progfiles: Vec<Str>,
    /// `-v` and `-F`, in the order given: each is an assignment to a
    /// variable, and the later of two assignments to one variable wins.
    /// (They were two lists, applied `-v`s first, so `-F: -v 'FS=;'` split on
    /// `:` where gawk splits on `;`. Measured.)
    preassigns: Vec<Preassign>,
    /// The program text, when there was no `-f`.
    program: Option<Str>,
    /// What goes into `ARGV[1..]`.
    operands: Vec<Str>,
}

/// gawk's start: its descriptors and its `SIGPIPE`, then the run.
///
/// There is no `stdfd::close_stderr` funnel here, unlike in the coreutils:
/// that is gnulib's `close_stdout`, which gawk does not use, and a diagnostic
/// gawk cannot write never changes its status. Measured, with standard error
/// full or closed alike: a warning is still 0, a syntax error 1, a fatal error
/// 2. So every way out of this program exits with the status it earned --
/// [`exit`] -- and not through `stdfd::exit_now`, which would make a lost
/// diagnostic 2.
fn main() -> ExitCode {
    stdfd::restore();
    // gawk's `init_fds`: a standard descriptor that was closed is opened on
    // `/dev/null` the wrong way round, so that it still fails as a closed one
    // does -- a read of descriptor 0, a write of 1 or 2 -- and a file opened
    // later cannot become it: `awk '{ print > "out" }' >&-` must not send its
    // standard output into `out`. Unchecked, as gawk's is: a descriptor it
    // could not fill stays closed, and fails as one.
    let _ = stdfd::stdopen();
    // gawk ignores `SIGPIPE` and handles `EPIPE` itself: a pipe to a command
    // whose reader has gone is `fatal: print to "CMD" failed: Broken pipe`,
    // and only standard output's dies of the signal, through
    // `die_via_sigpipe`. Children get the default back, as gawk's do: the
    // standard library resets it in every process it starts.
    stdfd::ignore_sigpipe();
    run_main()
}

fn run_main() -> ExitCode {
    let raw: Vec<Str> = std::env::args_os().skip(1).map(|a| arg_bytes(&a)).collect();
    let args = match parse_args(&raw) {
        Ok(Parsed::Run(a)) => a,
        // gawk's `usage` dies of `SIGPIPE` when its reader has gone, whatever
        // the disposition -- `{ sleep 1; awk --help; } | true` is 141 under
        // `trap '' PIPE` too -- and its version exits 1 without a word.
        // Measured, both.
        Ok(Parsed::Help) => print_and_exit(format!("{USAGE}\n").as_bytes(), ReaderGone::Signal),
        Ok(Parsed::Version) => print_and_exit(b"awk (SlateOS coreutils)\n", ReaderGone::Quiet),
        Err(ArgError::Usage(e)) => die_usage(&e),
        Err(ArgError::Fatal(e)) => die(&e),
    };

    let (text, map) = match program_source(&args) {
        Ok(s) => s,
        Err((message, status)) => {
            say(&message);
            exit(status)
        }
    };

    // gawk resolves the command line's assignments as it reads its options,
    // before it parses the program: each is refused there if its name is
    // reserved or its value holds a newline, and its escape warnings are said
    // there -- through the run's one table of warnings, which the parse and
    // then the run carry on with. `-v` elides a backslash-newline and `-F`
    // does not: gawk's `arg_assign` and `cmdline_fs` differ in exactly that.
    let mut warnings = ere::awk::Warnings::default();
    let mut preassigns: Vec<(&str, Str)> = Vec::with_capacity(args.preassigns.len());
    for p in &args.preassigns {
        let (name, value) = match p {
            Preassign::Var(name, value) => {
                if let Some(refusal) = interp::cli_refusal(name, value) {
                    die(refusal.as_bytes());
                }
                (name.as_str(), ere::awk::string(value, true, &mut warnings))
            }
            Preassign::Fs(value) => ("FS", ere::awk::string(value, false, &mut warnings)),
        };
        interp::emit_warnings(&mut warnings, b"");
        preassigns.push((name, value));
    }

    // A program that will not compile is a *usage* failure — the script is
    // wrong before anything ran — and exits 1. A failure once it is running
    // exits 2. That split is gawk's, and a shell script that distinguishes them
    // at all has been written against gawk. A few things gawk finds while
    // parsing are fatal rather than syntax errors, and those say so, after
    // where they were.
    let mut said = Vec::new();
    let parsed = parse::parse(&text, &map, &mut warnings, &mut said);
    let names = map.names();
    for (loc, message) in said {
        let mut line = b"awk: ".to_vec();
        line.extend_from_slice(&source::prefix(&names, loc));
        line.extend_from_slice(b"warning: ");
        line.extend_from_slice(&message);
        line.push(b'\n');
        stdfd::diag_bytes(&line);
    }
    let mut prog = match parsed {
        Ok(p) => p,
        // gawk can report several `error:`s before it stops; each is a line.
        Err(e) => {
            for message in &e.messages {
                let mut line = b"awk: ".to_vec();
                line.extend_from_slice(message);
                line.push(b'\n');
                stdfd::diag_bytes(&line);
            }
            exit(if e.fatal { 2 } else { 1 })
        }
    };
    if let Err(e) = types::resolve(&mut prog) {
        die_program(&e);
    }
    // A `-v` name that the program defines as a function: gawk made the name
    // a variable before it parsed, so the definition is what it refuses --
    // `error: function name `f' previously defined`, where the name is.
    let mut clashed = false;
    for p in &args.preassigns {
        if let Preassign::Var(name, _) = p
            && let Some(f) = prog.funcs.iter().find(|f| &f.name == name)
        {
            let mut line = b"awk: ".to_vec();
            line.extend_from_slice(&source::prefix(&names, f.loc));
            line.extend_from_slice(
                format!("error: function name `{name}' previously defined\n").as_bytes(),
            );
            stdfd::diag_bytes(&line);
            clashed = true;
        }
    }
    if clashed {
        exit(1);
    }

    let env: Vec<(Str, Str)> = std::env::vars_os()
        .map(|(k, v)| (arg_bytes(&k), arg_bytes(&v)))
        .collect();
    let mut it = interp::Interp::new(prog, &args.operands, &env);
    it.adopt_warnings(warnings);

    // In the order given, and all before BEGIN, so a BEGIN block can read
    // what the command line set and can override it.
    for (name, value) in preassigns {
        if let Err(interp::Fatal::Said(message)) = it.assign_cli(name, value) {
            die(&message);
        }
    }

    match it.run() {
        // Only the low byte of an `exit` expression survives into the wait
        // status, which is why `awk 'BEGIN{exit 300}'` leaves `$?` at 44.
        Ok(code) => ExitCode::from(u8::try_from(code & 0xff).unwrap_or(0)),
        // Said where the program was when it stopped, as gawk's `err()` does:
        // `awk: cmd. line:2: (FILENAME=f FNR=7) fatal: division by zero
        // attempted`. The prefix holds `FILENAME`, which is any bytes.
        Err(interp::Fatal::Said(message)) => {
            let mut line = b"awk: ".to_vec();
            line.extend_from_slice(&it.diagnostic_prefix());
            line.extend_from_slice(&message);
            line.push(b'\n');
            stdfd::diag_bytes(&line);
            exit(2)
        }
    }
}

/// Why the command line could not be used. Bytes, because two of the
/// refusals quote a `-v` argument back, as gawk does with `%s`, and an
/// argument need not be text.
enum ArgError {
    /// Malformed: said with the usage, exit 1.
    Usage(Str),
    /// A `-v` gawk refuses as it reads it: fatal, exit 2.
    Fatal(Str),
}

impl From<String> for ArgError {
    fn from(e: String) -> ArgError {
        ArgError::Usage(e.into_bytes())
    }
}

/// What the command line asks for.
enum Parsed {
    /// `--help`: the usage, on standard output.
    Help,
    /// `--version`.
    Version,
    /// A program to run.
    Run(Args),
}

/// Split the command line into options and operands. Prints nothing: the
/// help and the version are answered by the caller, each at the point gawk
/// answers it.
fn parse_args(raw: &[Str]) -> Result<Parsed, ArgError> {
    let mut args = Args {
        progfiles: Vec::new(),
        preassigns: Vec::new(),
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
            return Ok(Parsed::Help);
        }
        if bytes == b"--version" {
            return Ok(Parsed::Version);
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
                            return Err(ArgError::from(format!(
                                "option -{} requires an argument",
                                flag as char
                            )));
                        };
                        i = i.saturating_add(1);
                        next.clone()
                    } else {
                        let v = rest.to_vec();
                        rest = &[];
                        v
                    };
                    match flag {
                        b'F' => args.preassigns.push(Preassign::Fs(value)),
                        b'f' => args.progfiles.push(value),
                        _ => {
                            // gawk's two refusals, worded as gawk words them:
                            // no `=` is a usage error, a name that is not one
                            // is fatal.
                            let Some(eq) = value.iter().position(|b| *b == b'=') else {
                                let mut said = b"`".to_vec();
                                said.extend_from_slice(&value);
                                said.extend_from_slice(
                                    b"' argument to `-v' not in `var=value' form",
                                );
                                return Err(ArgError::Usage(said));
                            };
                            let Some((name, v)) = interp::command_assignment(&value) else {
                                let mut said = b"fatal: `".to_vec();
                                said.extend_from_slice(value.get(..eq).unwrap_or_default());
                                said.extend_from_slice(b"' is not a legal variable name");
                                return Err(ArgError::Fatal(said));
                            };
                            args.preassigns.push(Preassign::Var(name, v));
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
                    return Err(ArgError::from(format!("unknown option -{shown}")));
                }
            }
        }
    }

    if args.progfiles.is_empty() {
        let Some(text) = raw.get(i) else {
            return Err(ArgError::Usage(b"no program text".to_vec()));
        };
        i = i.saturating_add(1);
        args.program = Some(text.clone());
    }
    args.operands = raw.get(i..).unwrap_or_default().to_vec();
    Ok(Parsed::Run(args))
}

/// The program text: the `-f` files joined by newlines, or the operand.
///
/// # Errors
///
/// A program file that would not open or read, as gawk says it -- the message
/// after `awk: `, and the status. The two are told apart, as gawk's `srcopen`
/// and `get_src_buf` tell them apart: a file that will not *open* is fatal,
/// `fatal: cannot open source file `F' for reading: R`, status 2; one that
/// opens and will not *read* -- a directory, or a closed standard input for
/// `-f -` -- is a parse error at its first line, `F:1: error: cannot read
/// source file `F': R`, status 1.
fn program_source(args: &Args) -> Result<(Str, source::SourceMap), (Str, u8)> {
    use std::io::Read;

    if let Some(text) = &args.program {
        return Ok((text.clone(), source::SourceMap::operand(text)));
    }
    let mut out = Str::new();
    // Where each file begins in the joined text, so a diagnostic can name the
    // file and its own line, as gawk's `prog.awk:3:` does.
    let mut spans: Vec<(usize, Option<Str>)> = Vec::new();
    for name in &args.progfiles {
        spans.push((out.len(), Some(name.clone())));
        // `source file`, not `file`: gawk distinguishes a program it could not
        // read from an *input* file it could not read, and the two failures are
        // worth telling apart — one is a broken command line, the other a
        // broken argument to a working one. Every name is as given, which need
        // not be text.
        let mut source: Box<dyn Read> = if name.as_slice() == b"-" {
            // Descriptor 0 itself: `io::stdin()` would answer a closed one
            // with end of file -- an empty program -- where gawk's read fails.
            Box::new(stdfd::RawStdin)
        } else {
            match std::fs::File::open(io::os_path(name)) {
                Ok(file) => Box::new(file),
                Err(e) => {
                    let mut said = b"fatal: cannot open source file `".to_vec();
                    said.extend_from_slice(name);
                    said.extend_from_slice(
                        format!("' for reading: {}", coreutils::errmsg::strerror(&e)).as_bytes(),
                    );
                    return Err((said, 2));
                }
            }
        };
        let mut text = Str::new();
        if let Err(e) = source.read_to_end(&mut text) {
            let mut said = name.clone();
            said.extend_from_slice(b":1: error: cannot read source file `");
            said.extend_from_slice(name);
            said.extend_from_slice(format!("': {}", coreutils::errmsg::strerror(&e)).as_bytes());
            return Err((said, 1));
        }
        out.extend_from_slice(&text);
        // A `-f` file that does not end in a newline must not run its last
        // statement into the first statement of the next file.
        if !out.ends_with(b"\n") {
            out.push(b'\n');
        }
    }
    let map = source::SourceMap::new(&out, spans);
    Ok((out, map))
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
fn die(msg: &[u8]) -> ! {
    say(msg);
    exit(2)
}

/// `awk: MESSAGE` on standard error, as bytes.
fn say(msg: &[u8]) {
    let mut line = b"awk: ".to_vec();
    line.extend_from_slice(msg);
    line.push(b'\n');
    stdfd::diag_bytes(&line);
}

/// A program that will not compile.
fn die_program(msg: &str) -> ! {
    diag!("awk: {msg}");
    exit(1)
}

/// A command line that does not make sense.
fn die_usage(msg: &[u8]) -> ! {
    say(msg);
    diag!("{USAGE}");
    exit(1)
}

/// The end of the run, with the status it earned: gawk's `exit`, which no
/// lost diagnostic changes (see [`main`]).
fn exit(status: u8) -> ! {
    std::process::exit(i32::from(status))
}

/// What [`print_and_exit`] does when standard output's reader has gone.
#[derive(Clone, Copy)]
enum ReaderGone {
    /// gawk's `usage`: `die_via_sigpipe`, status 141 -- and where the signal
    /// is blocked and that returns, the warning any other failure gets.
    Signal,
    /// gawk's version: status 1, nothing said.
    Quiet,
}

/// `text` on standard output, flushed, and the end: gawk's `usage (EXIT_SUCCESS,
/// stdout)` and its version, which check what they wrote. A write that failed
/// is `warning: error writing standard output: R`, status 1 -- measured for a
/// full disk and for standard output closed (`init_fds` left it open the wrong
/// way round). A reader gone (`EPIPE`: the signal is ignored here) is
/// `gone`'s answer, measured for each under each disposition, blocked
/// included.
fn print_and_exit(text: &[u8], gone: ReaderGone) -> ! {
    let mut out = coreutils::stdio::StdioFile::stdout();
    let Err(e) = out.write(text).and_then(|()| out.flush()) else {
        exit(0)
    };
    if e.kind() == std::io::ErrorKind::BrokenPipe {
        match gone {
            ReaderGone::Signal => stdfd::die_via_sigpipe(),
            ReaderGone::Quiet => exit(1),
        }
    }
    say(format!(
        "warning: error writing standard output: {}",
        coreutils::errmsg::strerror(&e)
    )
    .as_bytes());
    exit(1)
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
            Err(ArgError::Usage(message) | ArgError::Fatal(message)) => {
                String::from_utf8(message).unwrap()
            }
            Ok(_) => panic!("expected these arguments to be refused: {argv:?}"),
        }
    }

    /// `--help` and `--version` come back to the caller, which answers each
    /// where gawk does (see `run_main`): asking prints nothing and ends
    /// nothing here, and nothing after them is read.
    #[test]
    fn help_and_version_are_returned_not_printed() {
        let parse = |argv: &[&[u8]]| {
            let raw: Vec<Str> = argv.iter().map(|a| a.to_vec()).collect();
            parse_args(&raw)
        };
        assert!(matches!(parse(&[b"--help"]), Ok(Parsed::Help)));
        assert!(matches!(parse(&[b"--version"]), Ok(Parsed::Version)));
        assert!(matches!(parse(&[b"--help", b"-Q"]), Ok(Parsed::Help)));
        assert!(matches!(parse(&[b"1"]), Ok(Parsed::Run(_))));
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
