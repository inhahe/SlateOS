//! `env` — run a command in a modified environment, or print the environment.
//!
//! ```text
//! env [-i] [-0] [-u NAME]... [-C DIR] [NAME=VALUE]... [COMMAND [ARG]...]
//! ```
//!
//! # Why this program, of all of them, must not hold a `String`
//!
//! An environment variable on this OS is a byte string, exactly like a path:
//! the design allows every byte but `/` and NUL, and the kernel hands `environ`
//! to a new process as bytes. `env`'s entire job is to carry those bytes from
//! the command line into a child's environment and to print them back out
//! again. It is the one utility in the tree whose *subject matter* is the
//! thing that must not be transcoded.
//!
//! The previous version began:
//!
//! ```ignore
//! let args: Vec<String> = env::args().skip(1).collect();
//! ```
//!
//! and printed with `for (key, value) in env::vars()`. Both of those are
//! `unwrap()` in disguise — `std::env::args`'s iterator is literally
//! `self.inner.next().map(|s| s.into_string().unwrap())`, and `vars()` is
//! documented to panic the same way. So:
//!
//! * `env` printing an environment that contained one non-UTF-8 variable
//!   **panicked**, printing a Rust panic message instead of the environment.
//! * `env LANG=$(some byte string) prog` panicked before doing anything.
//!
//! Everything here is `OsString`/`&[u8]` end to end for that reason. See
//! `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT` for the
//! other 53 utilities with the same first line.
//!
//! # The options that were missing
//!
//! POSIX defines exactly two, `-i` and `-u`, and this program had neither.
//! `env -i prog` — the standard way to run something in a clean environment,
//! which is what a build script or a privilege boundary reaches for — was
//! parsed as "run the program named `-i`". It failed loudly rather than
//! quietly, which is the one mercy, but `env -i` not working at all is a
//! bigger hole than any single wrong answer.
//!
//! Also added: `-0`/`--null` (NUL-terminate the printed output, so a value
//! containing a newline is still unambiguous — the same reason `find -print0`
//! exists), and `-C`/`--chdir`.
//!
//! Also added: `-S`/`--split-string`, GNU's shebang helper, which has a whole
//! quoting grammar of its own — measured rather than recalled, in
//! `scripts/probe-env-split-string.sh` (58 cases) and its two companions. It
//! is not the shell's grammar: `\t` puts a tab *inside* an argument where a
//! raw tab separates, only `${NAME}` expands and never re-splits, and an
//! unset variable contributes no argument where a set-but-empty one
//! contributes an empty one.

use coreutils::diag;
use coreutils::getopt::{Opt, Takes};
use coreutils::stdfd;
use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{self, ErrorKind, Write};
use std::process::ExitCode;
use std::process::{Command, ExitStatus};

use coreutils::errmsg::strerror;
use coreutils::getopt::Program;
use coreutils::quote::{os_bytes, quote_os, quoteaf_os};

/// `env`'s own failures exit 125, not 1 — GNU reserves 126 and 127 for "found
/// the command but could not run it" and "could not find it", so a third
/// number is needed for "the command line was bad". A caller that tests
/// `[ $? = 127 ]` is asking a question this distinction is the answer to.
const ENV: Program = Program::new("env", 125);

/// `env` itself could not proceed.
const EXIT_CANCELED: i32 = 125;
/// The command was found but could not be invoked.
const EXIT_CANNOT_INVOKE: i32 = 126;
/// The command was not found.
const EXIT_ENOENT: i32 = 127;

/// What terminates each line of printed output.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Sep {
    Newline,
    Nul,
}

impl Sep {
    const fn byte(&self) -> u8 {
        match self {
            Sep::Newline => b'\n',
            Sep::Nul => 0,
        }
    }
}

/// A parsed command line.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Config {
    /// `-i`: start from an empty environment rather than our own.
    ignore_env: bool,
    /// `-u NAME`, in order. Applied after `-i` and before the assignments, so
    /// `env -u FOO FOO=bar` sets `FOO`, matching GNU.
    unset: Vec<OsString>,
    /// `NAME=VALUE` operands, in order.
    assign: Vec<(OsString, OsString)>,
    /// `-C DIR`: chdir before exec.
    chdir: Option<OsString>,
    /// `-0`: NUL-terminate printed output.
    sep: Sep,
    /// The command and its arguments. Empty means "print the environment".
    command: Vec<OsString>,
}

// ---------------------------------------------------------------------------
// Byte views of an `OsStr`
// ---------------------------------------------------------------------------

/// Rebuild an `OsString` from bytes.
///
/// The inverse of [`os_bytes`], and it carries the same caveat for the same
/// reason: on the target an `OsStr` *is* bytes and this round-trips exactly,
/// while on a Windows development host there is no byte view that round-trips
/// at all. That only affects the machine the tests run on, never a running
/// SlateOS — and the alternative, refusing to split an argument into a name
/// and a value without valid UTF-8, would make the program wrong on the target
/// in order to be tidy on the host.
#[cfg(unix)]
fn os_from_bytes(b: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStrExt;
    OsStr::from_bytes(b).to_os_string()
}

#[cfg(not(unix))]
fn os_from_bytes(b: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(b).into_owned())
}

/// Split `NAME=VALUE` at its first `=`.
///
/// Returns `None` only when there is no `=` at all. The *value* may contain
/// further `=` signs and any bytes; only the first separator counts.
///
/// **An empty name is still an assignment.** This used to return `None` for
/// `=foo`, on the stated reasoning that "there is no such variable, so GNU
/// treats it as the command". That is wrong, and it was wrong in the way this
/// project keeps paying for: it is a claim about the reference with no
/// measurement behind it. GNU puts the entry in the environment and the child
/// inherits it — `env -i =novalue /usr/bin/env` prints `=novalue`, and
/// `env -i =a=b` prints `=a=b`, so the first-`=` rule applies unchanged.
/// Measured by `scripts/probe-env-empty-name.sh`; found because
/// `scripts/env-diff.sh` disagreed with the comment.
///
/// An empty name is not something a program should *create*, and `-u` still
/// refuses one — but refusing to pass one through is a different decision, and
/// GNU does not make it.
fn split_assignment(arg: &OsStr) -> Option<(OsString, OsString)> {
    let bytes = os_bytes(arg);
    let eq = bytes.iter().position(|&b| b == b'=')?;
    let name = bytes.get(..eq)?;
    let value = bytes.get(eq.saturating_add(1)..)?;
    Some((os_from_bytes(name), os_from_bytes(value)))
}

// ---------------------------------------------------------------------------
// Exit status
// ---------------------------------------------------------------------------

/// The status `env` exits with when the command could not be started.
///
/// GNU distinguishes these two and scripts rely on it: 127 means "there is no
/// such command, check the spelling or the PATH", 126 means "it is there but
/// you cannot run it", which is a permissions or a format problem. The old
/// code returned 127 for both, so a non-executable file looked like a missing
/// one.
const fn spawn_failure_status(kind: ErrorKind) -> i32 {
    match kind {
        ErrorKind::NotFound => EXIT_ENOENT,
        _ => EXIT_CANNOT_INVOKE,
    }
}

/// The status `env` exits with once the command has run.
///
/// A child killed by a signal has no exit code, and the old code turned that
/// into a flat `1` via `status.code().unwrap_or(1)` — so a command killed by
/// SIGKILL was indistinguishable from one that returned failure. The shell
/// convention is `128 + signal`, which is what `$?` says when the shell runs
/// the same command without `env` in front of it; `env` must not change the
/// answer merely by being in the way.
const fn child_status(code: Option<i32>, signal: Option<i32>) -> i32 {
    match (code, signal) {
        (Some(c), _) => c,
        (None, Some(sig)) => 128_i32.saturating_add(sig),
        // Neither: nothing sensible to report, and `env` did run something.
        (None, None) => EXIT_CANNOT_INVOKE,
    }
}

#[cfg(unix)]
fn exit_status_code(status: &ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    child_status(status.code(), status.signal())
}

#[cfg(not(unix))]
fn exit_status_code(status: &ExitStatus) -> i32 {
    child_status(status.code(), None)
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// A command line `env` will not run, and the status to exit with.
struct Failure {
    message: String,
    status: i32,
}

// ---------------------------------------------------------------------------
// -S / --split-string
// ---------------------------------------------------------------------------

/// The escapes `-S` accepts, and the byte each produces.
///
/// **A closed set, enumerated from GNU rather than recalled.** An escape that
/// is not in this table is a hard error, so a missing row does not degrade to
/// a missing feature — it becomes a refusal GNU does not give. `\a`, `\b` and
/// `\e` are absent *deliberately*: they are standard C escapes that `-S` does
/// not accept, and a table written from memory would have had all three.
/// `scripts/probe-env-split-escapes.sh` is the measurement; it walks 50
/// candidates so the boundary is observed rather than assumed.
///
/// `\_` and `\c` are not here because neither is a substitution: `\_` is a
/// space that *separates* when unquoted, and `\c` ends the string.
const SPLIT_ESCAPES: &[(u8, u8)] = &[
    (b'f', 0x0c),
    (b'n', b'\n'),
    (b'r', b'\r'),
    (b't', b'\t'),
    (b'v', 0x0b),
    (b'\\', b'\\'),
    (b'\'', b'\''),
    (b'"', b'"'),
    (b'#', b'#'),
    (b'$', b'$'),
];

/// Which quote, if any, the `-S` scanner is inside.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Quote {
    /// Outside quotes: whitespace separates and `#` opens a comment.
    Bare,
    /// `'…'` — every byte literal. No escapes, no expansion.
    Single,
    /// `"…"` — escapes and `${VAR}` still apply; whitespace does not separate.
    Double,
}

/// How many times a `-S` may splice before `env` gives up.
///
/// Not a limit GNU has, and not one any real command line reaches: a shebang
/// carries exactly one. It is a backstop, not a rule — see the termination
/// argument at the splice site for why the loop is already finite without it.
const MAX_SPLIT_EXPANSIONS: usize = 16;

/// A `-S` grammar error. Always 125, which is `env`'s "could not run" status.
fn split_failure(message: String) -> Failure {
    Failure {
        message,
        status: 125,
    }
}

/// Bytes as they should appear inside a diagnostic.
///
/// Exact when the bytes are UTF-8, which is every case GNU's own messages were
/// measured on. When they are not, they are *escaped* rather than decoded —
/// `from_utf8_lossy` would replace the offending bytes with U+FFFD and quietly
/// report a different string than the user typed, which the self-review
/// checklist forbids outright. Escaping is visibly different from GNU's raw
/// output, but it is different in a way a reader can see and undo.
fn bytes_for_message(b: &[u8]) -> String {
    match core::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => coreutils::quote::quote_glibc(b),
    }
}

/// Split a `-S`/`--split-string` operand into arguments, GNU's way.
///
/// This is **not** the shell's grammar, and the differences are not cosmetic.
/// `scripts/probe-env-split-string.sh` records 58 measured cases; the ones a
/// shell-shaped implementation gets wrong:
///
/// * `\t` puts a TAB *inside* an argument, while a raw tab separates — the
///   escape and the character it names do opposite things.
/// * Only `${NAME}` expands. A bare `$NAME` is an error, including inside
///   double quotes, and so is a lone `$`, `${`, or `${}`.
/// * An expansion never re-splits: a variable holding `p q` stays one argument.
/// * `\_` is a space that separates when bare and is literal inside `"`, so
///   the same escape is a separator or a character depending on context.
///
/// And one subtler still, which is why `started` exists separately from
/// `word.is_empty()`: an **unset** variable contributes no argument at all,
/// while one that is **set but empty** contributes an empty argument.
/// Measured — `-S'sh -c … ${NOPE}'` gives argc 0 and `${EMPTY}` gives argc 1.
///
/// # Errors
///
/// An unknown escape, a trailing backslash, an unterminated quote, `\c` inside
/// double quotes, or anything but `${NAME}` after a `$`.
fn split_string(s: &OsStr) -> Result<Vec<OsString>, Failure> {
    split_string_with(s, |name| env::var_os(name))
}

/// [`split_string`] with the variable lookup supplied by the caller.
///
/// Split out so the expansion rules can be tested without touching the
/// process environment. A test that called `set_var` to arrange an unset
/// versus set-but-empty variable would be writing a process-global that every
/// other test in the binary shares — the exact shape
/// `scripts/check-test-order-independence.py` exists to catch, and one that
/// passes or fails depending on which test ran first.
fn split_string_with<F>(s: &OsStr, lookup: F) -> Result<Vec<OsString>, Failure>
where
    F: Fn(&OsStr) -> Option<OsString>,
{
    let bytes = os_bytes(s);
    let src: &[u8] = bytes.as_ref();

    let mut words: Vec<OsString> = Vec::new();
    let mut word: Vec<u8> = Vec::new();
    // Whether a word is in progress. Deliberately not `!word.is_empty()`:
    // `""` must yield an EMPTY argument rather than none, and an unset
    // `${VAR}` must yield none rather than an empty one. The buffer's contents
    // cannot tell those two apart; this flag can.
    let mut started = false;
    let mut quote = Quote::Bare;
    let mut i = 0usize;

    while let Some(&c) = src.get(i) {
        i = i.saturating_add(1);
        match (quote, c) {
            // --- inside '' : literal until the closing quote ----------------
            (Quote::Single, b'\'') => quote = Quote::Bare,
            (Quote::Single, _) => word.push(c),

            // --- closing " --------------------------------------------------
            (Quote::Double, b'"') => quote = Quote::Bare,

            // --- separators, only outside quotes ----------------------------
            (Quote::Bare, b' ' | b'\t' | b'\n') => {
                if started {
                    words.push(os_from_bytes(&word));
                    word.clear();
                    started = false;
                }
            }

            // --- a comment runs to the end of the string --------------------
            // Only where a word has NOT begun: `a#b` is the literal `a#b`,
            // and `a #b` is just `a`.
            (Quote::Bare, b'#') if !started => return Ok(words),

            // --- quotes open a word, even if it stays empty -----------------
            (Quote::Bare, b'\'') => {
                quote = Quote::Single;
                started = true;
            }
            (Quote::Bare, b'"') => {
                quote = Quote::Double;
                started = true;
            }

            // --- escapes (bare and inside "") -------------------------------
            (_, b'\\') => {
                let Some(&e) = src.get(i) else {
                    return Err(split_failure(
                        "invalid backslash at end of string in -S".to_string(),
                    ));
                };
                i = i.saturating_add(1);
                match e {
                    b'_' if quote == Quote::Double => word.push(b' '),
                    b'_' => {
                        if started {
                            words.push(os_from_bytes(&word));
                            word.clear();
                            started = false;
                        }
                    }
                    b'c' if quote == Quote::Double => {
                        return Err(split_failure(
                            "'\\c' must not appear in double-quoted -S string".to_string(),
                        ));
                    }
                    // Ends the whole string; everything after it is dropped.
                    b'c' => {
                        if started {
                            words.push(os_from_bytes(&word));
                        }
                        return Ok(words);
                    }
                    _ => {
                        let Some(&(_, out)) = SPLIT_ESCAPES.iter().find(|&&(from, _)| from == e)
                        else {
                            return Err(split_failure(format!(
                                "invalid sequence '\\{}' in -S",
                                bytes_for_message(&[e])
                            )));
                        };
                        word.push(out);
                        started = true;
                    }
                }
            }

            // --- ${NAME} expansion ------------------------------------------
            (_, b'$') => {
                // GNU quotes back the whole remainder from the `$`, not just
                // the offending token: `[$NOPE]` reports `$NOPE]`, bracket
                // included. Measured, because it reads like a token at first.
                let rest = src.get(i.saturating_sub(1)..).unwrap_or(&[]);
                let bad = || {
                    split_failure(format!(
                        "only ${{VARNAME}} expansion is supported, error at: {}",
                        bytes_for_message(rest)
                    ))
                };

                if src.get(i) != Some(&b'{') {
                    return Err(bad());
                }
                let open = i.saturating_add(1);
                let Some(close) = src
                    .get(open..)
                    .and_then(|t| t.iter().position(|&x| x == b'}'))
                else {
                    return Err(bad());
                };
                let end = open.saturating_add(close);
                let Some(name) = src.get(open..end) else {
                    return Err(bad());
                };
                if name.is_empty() {
                    return Err(bad());
                }
                i = end.saturating_add(1);

                // An unset variable contributes nothing AND does not begin a
                // word; a set-but-empty one begins an (empty) word. This is
                // the whole reason `started` is tracked separately.
                if let Some(value) = lookup(&os_from_bytes(name)) {
                    word.extend_from_slice(os_bytes(&value).as_ref());
                    started = true;
                }
            }

            // --- any other byte is literal ----------------------------------
            _ => {
                word.push(c);
                started = true;
            }
        }
    }

    if quote != Quote::Bare {
        return Err(split_failure(
            "no terminating quote in -S string".to_string(),
        ));
    }
    if started {
        words.push(os_from_bytes(&word));
    }
    Ok(words)
}

/// Parse `env`'s argv.
///
/// Option parsing stops at the first operand, GNU-style: after `FOO=bar` or a
/// command name, a `-i` is an argument to the command rather than an option to
/// `env`. That is not a simplification — `env FOO=1 prog -i` must pass `-i` to
/// `prog`, and there is no way to tell that case from `env FOO=1 -i` except by
/// the rule that options come first.
/// GNU `env`'s `getopt_long` string, exactly, leading `+` included.
///
/// The `+` is "stop at the first operand", which for `env` is not an
/// optimisation but the whole semantics: `env FOO=1 ls -l` must hand `-l` to
/// `ls`, not read it as `env`'s own. [`coreutils::getopt`] implements that mode
/// and its own docs name `env` as one of the callers it was written for.
const SHORT_OPTIONS: &str = "+iu:0vC:S:";

/// GNU `env`'s `longopts[]`, in upstream's declaration order.
///
/// **Including the six we do not implement**, which is deliberate and is the
/// lesson `uname` paid for: the table is what decides whether an abbreviation
/// is ambiguous and which option a diagnostic names first. A table missing a
/// name does not merely fail to offer it — every abbreviation of the missing
/// name resolves to some *other* option and is acted on, where GNU would have
/// refused. `--s` is ambiguous here only because `--split-string` is present.
///
/// Read from GNU's own binary rather than from its documentation, with
/// `env --=x`: the empty prefix matches every entry, so the ambiguity message
/// lists the whole table in declaration order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("ignore-environment", Takes::Nothing),
    ("null", Takes::Nothing),
    ("unset", Takes::Required),
    ("chdir", Takes::Required),
    ("default-signal", Takes::Optional),
    ("ignore-signal", Takes::Optional),
    ("block-signal", Takes::Optional),
    ("list-signal-handling", Takes::Nothing),
    ("debug", Takes::Nothing),
    ("split-string", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// Parse `env`'s argv.
///
/// # Why this goes through [`coreutils::getopt`]
///
/// It used to walk `argv` itself, matching long names with `match name { … }`.
/// `scripts/env-diff.sh` found that eight of its twenty differences from GNU
/// were downstream of that: no long-option abbreviation, so `--unse`, `--ign`
/// and `--nu` were `unrecognized option` where GNU accepts them, and no
/// ambiguity rule at all. The same finding, and the same fix, as `uname`.
///
/// # Errors
///
/// An unknown option, an ambiguous abbreviation, a missing option argument, or
/// one of the six options GNU has and this does not.
fn parse_args(args: &[OsString]) -> Result<Config, Failure> {
    let mut cfg = Config {
        ignore_env: false,
        unset: Vec::new(),
        assign: Vec::new(),
        chdir: None,
        sep: Sep::Newline,
        command: Vec::new(),
    };

    // Operands come back through the parser rather than being sliced out of
    // `args` by index: with `+` the first one ends the options, and everything
    // after it is an operand too, so the split is the parser's to make.
    let mut operands: Vec<OsString> = Vec::new();

    // `-S` splices the words it splits into argv *in place of itself*, so the
    // parse restarts on the rewritten vector while everything already parsed
    // is kept. That is what makes options inside the string work, which they
    // do: `env -S'-i cmd'` really does clear the environment, and
    // `-S'--unset=FOO cmd'` really does drop FOO. Both measured — treating the
    // split words as plain operands would have been simpler and wrong.
    //
    // Options parsed *before* the `-S` survive because `cfg` is not rebuilt,
    // and argv *after* the `-S` word is appended, so `env -i -S'x' y` keeps
    // the `-i` and the `y`.
    //
    // This terminates on its own: each pass consumes one `-S` word and
    // replaces it with words whose total length is strictly less, so the
    // vector shrinks. The bound is kept anyway, because that argument rests on
    // the splitter never growing its input — an invariant a later edit could
    // break, leaving an unbounded loop driven by argv.
    let mut current: Vec<OsString> = args.to_vec();
    let mut expansions = 0usize;

    loop {
        let mut spliced: Option<Vec<OsString>> = None;
        let mut parser = ENV.parse(&current, SHORT_OPTIONS, LONG_OPTIONS);
        // `while let` on the parser rather than `for`: the loop body needs
        // `parser.optind()` to know where the `-S` word ended, and a `for`
        // consumes the parser.
        while let Some(item) = parser.next() {
            match item.map_err(fail)? {
                Opt::Long("ignore-environment", _) | Opt::Short(b'i', _) => cfg.ignore_env = true,
                Opt::Long("null", _) | Opt::Short(b'0', _) => cfg.sep = Sep::Nul,
                Opt::Long("unset", v) | Opt::Short(b'u', v) => {
                    cfg.unset.push(v.unwrap_or_default());
                }
                Opt::Long("chdir", v) | Opt::Short(b'C', v) => {
                    cfg.chdir = Some(v.unwrap_or_default())
                }
                Opt::Long("split-string", v) | Opt::Short(b'S', v) => {
                    let mut next = split_string(&v.unwrap_or_default())?;
                    // Everything argv still holds after the `-S` word follows the
                    // split words, which is what makes a shebang's script path
                    // land after the arguments the `#!` line supplied.
                    next.extend_from_slice(current.get(parser.optind()..).unwrap_or(&[]));
                    spliced = Some(next);
                    break;
                }
                // Present in the table so abbreviation and ambiguity match GNU's,
                // refused explicitly because they are not implemented. §1006 --
                // "a command that does not work is deleted, not kept as a refusing
                // stub" -- is about whole commands; an option that says plainly
                // that it is absent is better than one that silently does nothing,
                // and better than a table that lies about what GNU offers.
                Opt::Long(other, _) => {
                    return Err(fail(
                        ENV.usage_referring(format!("option '--{other}' is not implemented")),
                    ));
                }
                Opt::Short(other, _) => return Err(fail(ENV.invalid_option(other))),
                Opt::Operand(arg) => operands.push(arg.clone()),
            }
        }
        drop(parser);
        let Some(next) = spliced else { break };
        expansions = expansions.saturating_add(1);
        if expansions > MAX_SPLIT_EXPANSIONS {
            return Err(split_failure(format!(
                "-S nested more than {MAX_SPLIT_EXPANSIONS} deep"
            )));
        }
        current = next;
    }

    // --- NAME=VALUE operands, then the command and everything after it ---
    let mut rest = operands.iter();
    let mut command: Vec<OsString> = Vec::new();
    let mut first = true;
    for arg in rest.by_ref() {
        // A bare `-` is GNU's historical synonym for `-i`, and it is accepted
        // only as the FIRST operand. Measured against GNU 9.4 rather than
        // assumed, because the rule is narrower than it looks:
        //
        //     env -            ->  empty environment, rc 0
        //     env - FOO=1      ->  FOO=1
        //     env FOO=1 -      ->  rc 127, `-`: No such file or directory
        //     env - -          ->  rc 127, the SECOND `-` is the command
        //
        // So it is not "a `-` anywhere means -i"; once anything has been taken
        // as an operand, a later `-` is a command name like any other.
        //
        // This arm is here rather than in the option loop because `getopt`
        // hands a lone `-` back as an operand, which is what POSIX says it is.
        // Losing it was a REGRESSION introduced by moving this parser onto the
        // shared module -- `env -` passed before that change and failed after,
        // and `scripts/env-diff.sh` is what said so.
        if first && os_bytes(arg).as_ref() == b"-" {
            cfg.ignore_env = true;
            first = false;
            continue;
        }
        first = false;
        match split_assignment(arg) {
            Some(pair) => cfg.assign.push(pair),
            None => {
                command.push(arg.clone());
                break;
            }
        }
    }
    command.extend(rest.cloned());
    cfg.command = command;
    Ok(cfg)
}

fn fail(e: coreutils::getopt::Error) -> Failure {
    Failure {
        message: e.message(),
        status: e.status,
    }
}

// ---------------------------------------------------------------------------
// Doing it
// ---------------------------------------------------------------------------

/// The environment `cfg` describes, as name/value pairs in application order.
///
/// Returned rather than applied so the print path and the exec path build the
/// same thing from the same code — the old version applied assignments with
/// `set_var` on one path and `Command::env` on the other, which is two chances
/// to disagree about a question (does `-u FOO FOO=bar` set `FOO`?) that has
/// one answer.
fn effective_env(cfg: &Config, inherited: Vec<(OsString, OsString)>) -> Vec<(OsString, OsString)> {
    let mut out: Vec<(OsString, OsString)> = if cfg.ignore_env {
        Vec::new()
    } else {
        inherited
    };
    for name in &cfg.unset {
        out.retain(|(k, _)| k != name);
    }
    for (name, value) in &cfg.assign {
        match out.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => slot.1 = value.clone(),
            None => out.push((name.clone(), value.clone())),
        }
    }
    out
}

/// Render an environment for printing, one `NAME=VALUE` per separator.
fn render(vars: &[(OsString, OsString)], sep: &Sep) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in vars {
        out.extend_from_slice(&os_bytes(name));
        out.push(b'=');
        out.extend_from_slice(&os_bytes(value));
        out.push(sep.byte());
    }
    out
}

/// Write to stdout, treating a closed pipe as success and anything else as the
/// failure it is. `println!` panics on a write error; `env | head -1` must not
/// produce a panic message.
fn write_out(bytes: &[u8]) -> i32 {
    let mut out = io::stdout().lock();
    match out.write_all(bytes).and_then(|()| out.flush()) {
        Ok(()) => 0,
        Err(e) if e.kind() == ErrorKind::BrokenPipe => 0,
        Err(e) => {
            diag!("env: write error: {}", strerror(&e));
            EXIT_CANCELED
        }
    }
}

/// A status as `main` must return it.
///
/// Every status in this file is an `i32`, because that is what
/// [`coreutils::getopt::Error`] and [`ExitStatus::code`] deal in — but only a
/// byte of it survives into the `$?` the caller reads. A value that does not
/// fit is not a status at all, so it becomes `EXIT_CANCELED` ("`env` itself
/// failed") rather than being silently truncated into some other command's
/// meaning.
fn status(code: i32) -> ExitCode {
    ExitCode::from(u8::try_from(code).unwrap_or(125))
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 125)
}

fn run_main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let cfg = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            diag!("env: {}", e.message);
            return status(e.status);
        }
    };

    // A NAME THAT CANNOT NAME A VARIABLE IS REFUSED, not quietly ignored, and
    // before anything else happens. Measured:
    //
    //     env -u ''          env: cannot unset ‘’: Invalid argument       rc 125
    //     env -u 'WITH=EQ'   env: cannot unset ‘WITH=EQ’: Invalid argument rc 125
    //
    // This accepted both and exited 0 having unset nothing -- so a script that
    // built the name from a variable and got it empty was told the unset had
    // happened. Curly marks here, unlike `--chdir`'s ASCII ones below; both
    // spellings were measured rather than made uniform.
    for name in &cfg.unset {
        let bytes = os_bytes(name);
        if bytes.is_empty() || bytes.contains(&b'=') {
            diag!(
                "env: cannot unset {}: {}",
                quote_os(name),
                strerror(&io::Error::from_raw_os_error(22))
            );
            return status(EXIT_CANCELED);
        }
    }

    let vars = effective_env(&cfg, env::vars_os().collect());

    let Some(program) = cfg.command.first() else {
        // `-C` WITHOUT A COMMAND IS AN ERROR, measured:
        //
        //     $ env -C /tmp
        //     env: must specify command with --chdir (-C)      rc 125
        //
        // The comment that stood here said the opposite -- that `env -C /tmp`
        // with no command "is how a caller asks what the environment looks
        // like there" -- and this printed the environment, exit 0. It is a
        // reasonable-sounding thing for the option to mean and it is not what
        // it means.
        if cfg.chdir.is_some() {
            diag!("env: must specify command with --chdir (-C)");
            diag!("Try 'env --help' for more information.");
            return status(EXIT_CANCELED);
        }
        return status(write_out(&render(&vars, &cfg.sep)));
    };

    let mut cmd = Command::new(program);
    cmd.args(cfg.command.get(1..).unwrap_or(&[]));
    cmd.env_clear();
    cmd.envs(vars);
    if let Some(dir) = &cfg.chdir {
        // CHECKED HERE, not left to the spawn. `Command::current_dir` defers
        // the chdir, so a directory that does not exist surfaced as a failure
        // to run the PROGRAM -- `env: ‘/bin/pwd’: No such file or directory`,
        // naming a file that is perfectly present. GNU names the directory:
        //
        //     env: cannot change directory to '/nosuch': No such file...
        //
        // `quoteaf_os`, with ASCII apostrophes, because that is what GNU uses
        // HERE -- unlike `cannot unset ‘’` and ‘program’ above, which are curly.
        // Measured all three rather than assumed from one; coreutils is not
        // uniform about this and picking one spelling for the file would be
        // wrong twice.
        if let Err(e) = std::fs::metadata(dir) {
            diag!(
                "env: cannot change directory to {}: {}",
                quoteaf_os(dir),
                strerror(&e)
            );
            return status(EXIT_CANCELED);
        }
        cmd.current_dir(dir);
    }

    match cmd.status() {
        Ok(finished) => status(exit_status_code(&finished)),
        Err(e) => {
            // `quote_os`, not `quotef_os`: GNU quotes the name ALWAYS here,
            // in the curly marks its own `quote()` produces --
            // `env: ‘novalue’: No such file or directory`. `quotef_os` quotes
            // only a name that needs it, so a plain one came out bare and one
            // holding `=` came out in ASCII apostrophes; both differ from GNU,
            // in opposite directions, which is why one spelling could not be
            // right for both.
            diag!("env: {}: {}", quote_os(program), strerror(&e));
            status(spawn_failure_status(e.kind()))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn o(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        items
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    fn cfg(args: &[&str]) -> Config {
        match parse_args(&o(args)) {
            Ok(c) => c,
            Err(e) => panic!("expected a parse, got {}", e.message),
        }
    }

    // ---------------- -S / --split-string ----------------
    //
    // Every expectation here is a row measured from GNU coreutils 9.4 by
    // scripts/probe-env-split-string.sh and its two companions, not a reading
    // of the documentation. The case numbers in comments are that script's.

    /// Split with a fixed environment: FOO=bar, WITHSPACE="p q", EMPTY set to
    /// the empty string, and everything else unset.
    ///
    /// EMPTY being *set* is the point of it — an unset variable and a
    /// set-but-empty one produce different argument counts, and no test can
    /// show that without controlling both.
    fn sp(s: &str) -> Result<Vec<OsString>, Failure> {
        split_string_with(OsStr::new(s), |name| match name.to_str() {
            Some("FOO") => Some(OsString::from("bar")),
            Some("WITHSPACE") => Some(OsString::from("p q")),
            Some("EMPTY") => Some(OsString::new()),
            _ => None,
        })
    }

    fn sp_ok(s: &str) -> Vec<OsString> {
        match sp(s) {
            Ok(w) => w,
            Err(e) => panic!("expected a split of {s:?}, got {}", e.message),
        }
    }

    fn sp_err(s: &str) -> String {
        match sp(s) {
            Ok(w) => panic!("expected {s:?} to be refused, got {w:?}"),
            Err(e) => {
                assert_eq!(e.status, 125, "-S errors are 125");
                e.message
            }
        }
    }

    #[test]
    fn split_separates_on_runs_of_whitespace() {
        assert_eq!(sp_ok("a b"), o(&["a", "b"])); // case 3
        assert_eq!(sp_ok("a     b"), o(&["a", "b"])); // case 4
        assert_eq!(sp_ok("   a"), o(&["a"])); // case 5
        assert_eq!(sp_ok("a   "), o(&["a"])); // case 6
        assert_eq!(sp_ok("a\tb"), o(&["a", "b"])); // case 7
        assert_eq!(sp_ok("a\nb"), o(&["a", "b"])); // case 57
        assert_eq!(sp_ok(""), Vec::<OsString>::new());
    }

    #[test]
    fn split_escapes_are_the_measured_closed_set() {
        assert_eq!(sp_ok("a\\\\b"), o(&["a\\b"])); // case 10
        assert_eq!(sp_ok("a\\tb"), o(&["a\tb"])); // case 11 -- a TAB, not a split
        assert_eq!(sp_ok("a\\nb"), o(&["a\nb"])); // case 12
        assert_eq!(sp_ok("a\\fb"), o(&["a\u{0c}b"]));
        assert_eq!(sp_ok("a\\rb"), o(&["a\rb"]));
        assert_eq!(sp_ok("a\\vb"), o(&["a\u{0b}b"]));
        assert_eq!(sp_ok("a\\#b"), o(&["a#b"])); // case 14
        assert_eq!(sp_ok("a\\$b"), o(&["a$b"])); // case 15
        assert_eq!(sp_ok("a\\\"b"), o(&["a\"b"]));
        assert_eq!(sp_ok("a\\'b"), o(&["a'b"]));
    }

    #[test]
    fn split_rejects_escapes_gnu_rejects() {
        // \a, \b and \e are the ones worth asserting: they are standard C
        // escapes, they are NOT in GNU's -S set, and an implementation
        // written from memory has all three. Round 4 measured 50 candidates.
        for bad in ["a\\ab", "a\\bb", "a\\eb", "a\\0b", "a\\xb", "a\\qb"] {
            let m = sp_err(bad);
            assert!(m.contains("invalid sequence"), "{bad}: {m}");
            assert!(m.ends_with("in -S"), "{bad}: {m}");
        }
        assert_eq!(sp_err("a\\qb"), "invalid sequence '\\q' in -S"); // case 17
        assert_eq!(sp_err("a\\ b"), "invalid sequence '\\ ' in -S"); // case 16
        assert_eq!(sp_err("ab\\"), "invalid backslash at end of string in -S");
    }

    #[test]
    fn split_underscore_separates_bare_and_is_a_space_quoted() {
        // The same escape means different things in the two contexts, which
        // is why it is not in the substitution table.
        assert_eq!(sp_ok("a\\_b"), o(&["a", "b"])); // case 9
        assert_eq!(sp_ok("\"a\\_b\""), o(&["a b"])); // case 51
        assert_eq!(sp_ok("'a\\_b'"), o(&["a\\_b"])); // case 52 -- literal
    }

    #[test]
    fn split_backslash_c_ends_the_string() {
        assert_eq!(sp_ok("a \\c b"), o(&["a"])); // case 13
        assert_eq!(sp_ok("'a\\cb' z"), o(&["a\\cb", "z"])); // case 54
        assert_eq!(
            sp_err("\"a\\cb\""), // case 53
            "'\\c' must not appear in double-quoted -S string"
        );
    }

    #[test]
    fn split_quoting() {
        assert_eq!(sp_ok("'a b'"), o(&["a b"])); // case 18
        assert_eq!(sp_ok("\"a b\""), o(&["a b"])); // case 19
        assert_eq!(sp_ok("\"a 'b\""), o(&["a 'b"])); // case 20
        assert_eq!(sp_ok("'a \"b'"), o(&["a \"b"])); // case 21
        assert_eq!(sp_ok("a\"b c\"d"), o(&["ab cd"])); // case 24 -- joins
        assert_eq!(sp_ok("'a\\tb'"), o(&["a\\tb"])); // case 25 -- no escapes in ''
        assert_eq!(sp_ok("\"a\\tb\""), o(&["a\tb"])); // case 26 -- escapes in ""
        assert_eq!(sp_err("'a b"), "no terminating quote in -S string"); // 22
        assert_eq!(sp_err("\"a b"), "no terminating quote in -S string"); // 23
    }

    #[test]
    fn split_empty_quotes_make_an_empty_argument() {
        // `""` yields an argument; it is not the same as yielding nothing.
        assert_eq!(sp_ok("x \"\""), o(&["x", ""]));
        assert_eq!(sp_ok("x ''"), o(&["x", ""]));
        assert_eq!(sp_ok("x \"\" \"\""), o(&["x", "", ""]));
    }

    #[test]
    fn split_comments_start_only_at_a_word_boundary() {
        assert_eq!(sp_ok("a #b c"), o(&["a"])); // case 27
        assert_eq!(sp_ok("a # b c"), o(&["a"])); // case 28
        assert_eq!(sp_ok("a#b c"), o(&["a#b", "c"])); // case 29 -- literal
        assert_eq!(sp_ok("\"a # b\""), o(&["a # b"])); // case 56
        assert_eq!(sp_ok("# nothing"), Vec::<OsString>::new()); // case 55
    }

    #[test]
    fn split_expands_only_braced_names() {
        assert_eq!(sp_ok("${FOO}"), o(&["bar"])); // case 31
        assert_eq!(sp_ok("x${FOO}y"), o(&["xbary"])); // case 32
        assert_eq!(sp_ok("\"${FOO}\""), o(&["bar"])); // case 47
        assert_eq!(sp_ok("'${FOO}'"), o(&["${FOO}"])); // case 49 -- literal
        // The bare form is an error even though it is what everyone writes.
        assert_eq!(
            sp_err("$FOO"), // case 30
            "only ${VARNAME} expansion is supported, error at: $FOO"
        );
        // The diagnostic carries the whole REMAINDER, not just the token --
        // the `]` here is part of GNU's message.
        assert_eq!(
            sp_err("[$NOPE]"), // case 33
            "only ${VARNAME} expansion is supported, error at: $NOPE]"
        );
        for bad in ["${FOO", "${}", "$"] {
            assert!(
                sp_err(bad).starts_with("only ${VARNAME} expansion is supported"),
                "{bad}"
            );
        }
    }

    #[test]
    fn split_expansion_never_resplits() {
        // "p q" is one argument, not two -- the difference between this and
        // the shell's word splitting.
        assert_eq!(sp_ok("${WITHSPACE}"), o(&["p q"])); // case 46
        assert_eq!(sp_ok("\"${WITHSPACE}\""), o(&["p q"])); // case 48
    }

    #[test]
    fn split_unset_yields_no_argument_but_empty_yields_one() {
        // The subtlest measured rule, and the reason the scanner tracks
        // "a word has begun" separately from "the buffer is non-empty".
        assert_eq!(sp_ok("x ${NOPE}"), o(&["x"]));
        assert_eq!(sp_ok("x ${EMPTY}"), o(&["x", ""]));
        // A quote begins the word, so a following unset variable still
        // leaves an (empty) argument behind.
        assert_eq!(sp_ok("x \"\"${NOPE}"), o(&["x", ""]));
        assert_eq!(sp_ok("x [${NOPE}]"), o(&["x", "[]"])); // case 44
    }

    #[test]
    fn split_string_option_is_parsed_and_spliced() {
        // The option reaches the splitter at all...
        let c = cfg(&["-S/bin/echo hi"]);
        assert_eq!(c.command, o(&["/bin/echo", "hi"]));
        // ...in its long form, attached...
        let c = cfg(&["--split-string=/bin/echo hi"]);
        assert_eq!(c.command, o(&["/bin/echo", "hi"]));
        // ...and detached.
        let c = cfg(&["-S", "/bin/echo one two"]);
        assert_eq!(c.command, o(&["/bin/echo", "one", "two"]));
    }

    #[test]
    fn split_string_honours_options_inside_it() {
        // Measured (round 3): -i inside the string really does clear the
        // environment. This is the behaviour that forced the splice design
        // -- treating the split words as operands would leave `-i` as an
        // argument to the command.
        let c = cfg(&["-S-i /bin/echo hi"]);
        assert!(c.ignore_env);
        assert_eq!(c.command, o(&["/bin/echo", "hi"]));

        let c = cfg(&["-S--unset=FOO /bin/echo"]);
        assert_eq!(c.unset, o(&["FOO"]));

        // An assignment inside the string is an assignment.
        let c = cfg(&["-SX=1 /bin/echo"]);
        assert_eq!(c.assign, pairs(&[("X", "1")]));
    }

    #[test]
    fn split_string_keeps_options_before_and_argv_after() {
        let c = cfg(&["-i", "-S/bin/echo a", "b"]);
        assert!(c.ignore_env);
        // `b` came after the -S word in argv and lands after the split words.
        assert_eq!(c.command, o(&["/bin/echo", "a", "b"]));
    }

    #[test]
    fn split_string_that_splits_to_nothing_leaves_no_command() {
        // Measured: env then prints the environment, rc 0, which is its
        // ordinary no-command behaviour rather than anything -S-specific.
        assert!(cfg(&["-S   "]).command.is_empty());
        assert!(cfg(&["-S# only a comment"]).command.is_empty());
    }

    // ---------------- assignments ----------------

    #[test]
    fn assignments_then_command() {
        let c = cfg(&["FOO=bar", "ls", "-la"]);
        assert_eq!(c.assign, pairs(&[("FOO", "bar")]));
        assert_eq!(c.command, o(&["ls", "-la"]));
    }

    #[test]
    fn every_argument_an_assignment_means_print() {
        let c = cfg(&["A=1", "B=2"]);
        assert_eq!(c.assign, pairs(&[("A", "1"), ("B", "2")]));
        assert!(c.command.is_empty());
    }

    #[test]
    fn only_the_first_equals_splits() {
        let c = cfg(&["KEY=a=b=c"]);
        assert_eq!(c.assign, pairs(&[("KEY", "a=b=c")]));
    }

    #[test]
    fn an_empty_value_is_still_an_assignment() {
        let c = cfg(&["FOO="]);
        assert_eq!(c.assign, pairs(&[("FOO", "")]));
        assert!(c.command.is_empty());
    }

    #[test]
    fn a_leading_equals_is_an_assignment_with_an_empty_name() {
        // This test asserted the opposite until 2026-09-14, on the reasoning
        // that there is no variable with an empty name to assign to. True of
        // what a program *should* create, false of what GNU does: measured,
        // `env -i =novalue /usr/bin/env` prints `=novalue`, so the entry is
        // made and the child inherits it. See scripts/probe-env-empty-name.sh.
        let c = cfg(&["=foo", "bar"]);
        assert_eq!(c.assign, pairs(&[("", "foo")]));
        assert_eq!(c.command, o(&["bar"]));
    }

    #[test]
    fn an_empty_name_still_splits_at_the_first_equals() {
        let c = cfg(&["=a=b"]);
        assert_eq!(c.assign, pairs(&[("", "a=b")]));
    }

    #[test]
    fn an_assignment_after_the_command_belongs_to_the_command() {
        let c = cfg(&["FOO=bar", "ls", "BAR=baz"]);
        assert_eq!(c.assign, pairs(&[("FOO", "bar")]));
        assert_eq!(c.command, o(&["ls", "BAR=baz"]));
    }

    // ---------------- -i ----------------

    #[test]
    fn dash_i_is_an_option_not_a_program_name() {
        // The old parser had no options at all, so this ran a program called
        // `-i`. `env -i` is the standard way to get a clean environment.
        let c = cfg(&["-i", "prog"]);
        assert!(c.ignore_env);
        assert_eq!(c.command, o(&["prog"]));
    }

    #[test]
    fn a_bare_dash_means_the_same_as_dash_i() {
        assert!(cfg(&["-", "prog"]).ignore_env);
    }

    #[test]
    fn long_ignore_environment() {
        assert!(cfg(&["--ignore-environment", "prog"]).ignore_env);
    }

    #[test]
    fn ignore_env_drops_everything_inherited() {
        let c = cfg(&["-i", "PATH=/bin", "prog"]);
        let got = effective_env(&c, pairs(&[("HOME", "/root"), ("PATH", "/usr/bin")]));
        assert_eq!(got, pairs(&[("PATH", "/bin")]));
    }

    // ---------------- -u ----------------

    #[test]
    fn dash_u_separate_and_attached() {
        assert_eq!(cfg(&["-u", "FOO", "prog"]).unset, o(&["FOO"]));
        assert_eq!(cfg(&["-uFOO", "prog"]).unset, o(&["FOO"]));
        assert_eq!(cfg(&["--unset=FOO", "prog"]).unset, o(&["FOO"]));
        assert_eq!(cfg(&["--unset", "FOO", "prog"]).unset, o(&["FOO"]));
    }

    #[test]
    fn several_unsets_accumulate() {
        let c = cfg(&["-u", "A", "-u", "B", "prog"]);
        assert_eq!(c.unset, o(&["A", "B"]));
        let got = effective_env(&c, pairs(&[("A", "1"), ("B", "2"), ("C", "3")]));
        assert_eq!(got, pairs(&[("C", "3")]));
    }

    #[test]
    fn unset_then_assign_sets_it() {
        // GNU's order: `-u` first, assignments after, so this leaves FOO=bar.
        let c = cfg(&["-u", "FOO", "FOO=bar", "prog"]);
        let got = effective_env(&c, pairs(&[("FOO", "old")]));
        assert_eq!(got, pairs(&[("FOO", "bar")]));
    }

    #[test]
    fn an_assignment_replaces_rather_than_duplicates() {
        // Two entries with the same name is not an environment, it is a bug
        // that surfaces as "the variable has the wrong value, sometimes".
        let c = cfg(&["PATH=/bin", "prog"]);
        let got = effective_env(&c, pairs(&[("PATH", "/usr/bin"), ("HOME", "/root")]));
        assert_eq!(got, pairs(&[("PATH", "/bin"), ("HOME", "/root")]));
    }

    // ---------------- -0, -C, bundling ----------------

    #[test]
    fn dash_zero_selects_nul() {
        assert_eq!(cfg(&["-0"]).sep, Sep::Nul);
        assert_eq!(cfg(&["--null"]).sep, Sep::Nul);
        assert_eq!(cfg(&[]).sep, Sep::Newline);
    }

    #[test]
    fn dash_c_takes_a_directory() {
        assert_eq!(
            cfg(&["-C", "/tmp", "prog"]).chdir,
            Some(OsString::from("/tmp"))
        );
        assert_eq!(cfg(&["-C/tmp", "prog"]).chdir, Some(OsString::from("/tmp")));
        assert_eq!(
            cfg(&["--chdir=/tmp", "prog"]).chdir,
            Some(OsString::from("/tmp"))
        );
    }

    #[test]
    fn short_options_bundle() {
        let c = cfg(&["-i0", "prog"]);
        assert!(c.ignore_env);
        assert_eq!(c.sep, Sep::Nul);
        let c = cfg(&["-iuFOO", "prog"]);
        assert!(c.ignore_env);
        assert_eq!(c.unset, o(&["FOO"]));
    }

    // ---------------- option termination ----------------

    #[test]
    fn double_dash_ends_the_options() {
        // Without it there is no way to run a program actually called `-i`.
        let c = cfg(&["--", "-i"]);
        assert!(!c.ignore_env);
        assert_eq!(c.command, o(&["-i"]));
    }

    #[test]
    fn options_stop_at_the_first_operand() {
        // `-i` here is an argument to `prog`, not an option to `env`. GNU
        // stops option parsing at the first operand for exactly this reason.
        let c = cfg(&["FOO=1", "prog", "-i"]);
        assert!(!c.ignore_env);
        assert_eq!(c.command, o(&["prog", "-i"]));
    }

    #[test]
    fn a_command_that_looks_like_an_assignment_later_is_left_alone() {
        let c = cfg(&["prog", "A=1"]);
        assert!(c.assign.is_empty());
        assert_eq!(c.command, o(&["prog", "A=1"]));
    }

    // ---------------- diagnostics ----------------

    #[test]
    fn an_unknown_option_is_rejected_with_gnu_wording_and_status_125() {
        let e = parse_args(&o(&["-Z", "prog"])).unwrap_err();
        assert!(e.message.contains("invalid option -- 'Z'"), "{}", e.message);
        // 125, not 1: 126 and 127 already mean "could not run the command",
        // so `env`'s own failure needs a number of its own.
        assert_eq!(e.status, EXIT_CANCELED);
    }

    #[test]
    fn an_unknown_long_option_is_rejected() {
        let e = parse_args(&o(&["--zzz"])).unwrap_err();
        assert!(e.message.contains("unrecognized option"), "{}", e.message);
        assert!(e.message.contains("--zzz"), "{}", e.message);
    }

    #[test]
    fn an_option_missing_its_argument_is_rejected() {
        assert!(
            parse_args(&o(&["-u"]))
                .unwrap_err()
                .message
                .contains("requires an argument")
        );
        assert!(
            parse_args(&o(&["--chdir"]))
                .unwrap_err()
                .message
                .contains("requires an argument")
        );
    }

    // ---------------- exit status ----------------

    #[test]
    fn missing_and_unrunnable_commands_get_different_statuses() {
        // The old code returned 127 for both, so a file that exists but is not
        // executable was reported as one that does not exist.
        assert_eq!(spawn_failure_status(ErrorKind::NotFound), 127);
        assert_eq!(spawn_failure_status(ErrorKind::PermissionDenied), 126);
    }

    #[test]
    fn a_signalled_child_reports_128_plus_the_signal() {
        // `status.code().unwrap_or(1)` made a SIGKILLed command look like one
        // that merely returned 1. The shell says 137; `env` must not change
        // the answer by being in the way.
        assert_eq!(child_status(None, Some(9)), 137);
        assert_eq!(child_status(None, Some(15)), 143);
        assert_eq!(child_status(Some(3), None), 3);
        assert_eq!(child_status(Some(0), None), 0);
    }

    // ---------------- rendering ----------------

    #[test]
    fn render_uses_the_chosen_separator() {
        let vars = pairs(&[("A", "1"), ("B", "2")]);
        assert_eq!(render(&vars, &Sep::Newline), b"A=1\nB=2\n");
        assert_eq!(render(&vars, &Sep::Nul), b"A=1\0B=2\0");
    }

    #[test]
    fn a_value_containing_a_newline_survives() {
        // This is what `-0` is for: with newline separators the two lines are
        // indistinguishable from two variables.
        let vars = pairs(&[("A", "one\ntwo")]);
        assert_eq!(render(&vars, &Sep::Nul), b"A=one\ntwo\0");
    }

    // ---------------- bytes ----------------

    #[test]
    fn an_assignment_splits_on_bytes_not_characters() {
        // The name and the value are both byte strings on this OS. The split
        // must land on the first `=` byte and preserve everything else, which
        // is why this program never builds a `String`.
        let (name, value) = split_assignment(OsStr::new("Kü=vä=lue")).unwrap();
        assert_eq!(name, OsString::from("Kü"));
        assert_eq!(value, OsString::from("vä=lue"));
    }

    #[test]
    fn something_with_no_equals_is_not_an_assignment() {
        assert!(split_assignment(OsStr::new("plain")).is_none());
        // `=novalue` DOES split -- an absent `=` is the only thing that makes
        // this `None`. It was asserted as `is_none()` here until 2026-09-14.
        assert_eq!(
            split_assignment(OsStr::new("=novalue")),
            Some((OsString::new(), OsString::from("novalue")))
        );
    }
}
