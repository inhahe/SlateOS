//! sed — the stream editor.
//!
//! ```text
//! sed [OPTION]... {SCRIPT} [FILE]...
//! sed [OPTION]... -e SCRIPT... -f SCRIPTFILE... [FILE]...
//! ```
//!
//! | | |
//! |---|---|
//! | `-n` | do not print the pattern space at the end of a cycle |
//! | `-e S` | add S to the script; may be repeated |
//! | `-f F` | add the contents of file F to the script |
//! | `-i[SUF]` | edit each file in place, keeping a `SUF` backup if given |
//! | `-E` / `-r` | patterns are Extended regular expressions |
//! | `-s` | treat the files as separate streams rather than one |
//! | `-z` | lines are separated by NUL rather than newline |
//! | `-u`, `-b` | accepted and ignored: nothing here buffers or translates |
//! | `-l N` | the width `l` wraps at; 0 never wraps, and the default is 70 |
//! | `--sandbox` | refuse `r`, `R`, `w`, `W` and `s///w` while compiling |
//! | `--` | end of options; what follows is a file |
//!
//! Long options are resolved by [`coreutils::getopt`], so they abbreviate to
//! any unambiguous prefix as every GNU utility's do: `--expr=p` works and
//! `--s` is refused as ambiguous between `--silent`, `--sandbox` and
//! `--separate`. `--posix`, `--debug` and `--follow-symlinks` are accepted and
//! do nothing yet; see `known-issues.md`.
//!
//! Exit status: 0 normally, 1 for a bad script or usage, 2 for an input file
//! that could not be opened (the rest are still processed), 4 for a failure
//! the run cannot continue past — see [`EXIT_PANIC`] — or the status given
//! to `q`.
//!
//! ## What this used to be
//!
//! Until `userspace/ere` existed, `sed` matched with `str::contains` plus a
//! hand-rolled matcher that understood `.` and `*` and nothing else. `s/^/E:/`
//! copied its input through unchanged — the shell's test suite caught that as a
//! missing `E:` prefix and blamed the shell, which had in fact done everything
//! right. There were no groups, no `\(…\)`, no bracket expressions, no
//! alternation, no back-half of the command set (no hold space, no branching,
//! no `y`, no `a`/`i`/`c`), and ranges did not track state across lines: `1,5d`
//! deleted lines 1 and 5. See `design-decisions.md` §322.
//!
//! Patterns are POSIX **Basic** regular expressions, **Extended** under `-E`,
//! matched by `ere` — the same engine `grep`, `awk`, `expr` and the shell's
//! `[[ =~ ]]` use, so all five agree about what `[a-z]` means.
//!
//! ## Lines are bytes
//!
//! A path on this system may hold any byte but `/` and NUL, so a `sed` that
//! insisted its input was UTF-8 could not edit a file listing. The pattern and
//! hold spaces are `Vec<u8>` and input is read with `read_until`, so a line
//! that is not text passes through unchanged rather than being replaced with
//! U+FFFD.
//!
//! ## The trailing newline
//!
//! If the last line of the input has no newline, neither does the output — but
//! `printf a | sed p` still prints `a\na`, because the newline is missing only
//! from the *end of the output*, not from that line wherever it appears. That
//! is why writing goes through [`Out`], which holds a newline back until it
//! knows something follows it.

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Seek, Write};
use std::process;
use std::rc::Rc;

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, os_from_bytes};
use coreutils::stdfd;
use ere::{Regex, bre};

/// The status for a failure that stops the run where it stands.
///
/// sed has three failure statuses and they are not interchangeable: 1 means
/// the command line or the script was wrong, 2 means an input file could not
/// be opened (the remaining files are still processed), and 4 — this one —
/// means something failed that sed cannot carry on past: a `-f` script file
/// that will not open, a `w` target that will not open, a read that failed
/// mid-stream, an in-place edit with nothing to edit. Upstream reaches all of
/// these through one function, `panic()`, which is where the name below comes
/// from.
const EXIT_PANIC: i32 = 4;

/// Print one diagnostic, with sed's pending output delivered first.
///
/// `error(3)` opens with `fflush (stdout)` so that a complaint about a stream
/// arrives *after* the bytes already written to it, and
/// [`coreutils::stdfd::diag_bytes`] does the same for the descriptor-level
/// buffer every converted utility writes through. sed does not use that buffer
/// — it writes through `io::stdout()`, a `LineWriter` that holds back a line
/// with no separator yet, which is exactly what [`Out`] leaves pending at end
/// of input — so the flush has to be repeated for the buffer sed does use.
fn diag_line(line: &str) {
    let _ = io::stdout().flush();
    stdfd::diag_line(line);
}

/// [`diag_line`] for a message that names a file.
///
/// GNU sed prints a name with a plain `%s`: no quoting, no substitution, so a
/// name that is not valid UTF-8 reaches the terminal unchanged. Going through
/// `format!` would render those bytes as U+FFFD, which names a different file
/// from the one that failed.
fn diag_path(before: &str, path: &OsStr, after: &str) {
    let _ = io::stdout().flush();
    let mut line = Vec::from(before.as_bytes());
    line.extend_from_slice(&os_bytes(path));
    line.extend_from_slice(after.as_bytes());
    line.push(b'\n');
    stdfd::diag_bytes(&line);
}

/// `eprintln!`-shaped diagnostic, routed through [`diag_line`].
///
/// This shadows [`coreutils::diag`] on purpose rather than adding a second
/// spelling beside it: every diagnostic sed prints owes the same flush, and a
/// rule that is enforced by the only macro in scope cannot be forgotten at a
/// call site.
macro_rules! diag {
    ($($arg:tt)*) => {
        crate::diag_line(&::std::format!($($arg)*))
    };
}

/// Report a failure sed cannot continue past, and leave with status 4.
fn panic_out(msg: &str) -> ! {
    diag!("sed: {msg}");
    process::exit(EXIT_PANIC)
}

/// [`panic_out`] for a message that names a file. See [`diag_path`].
fn panic_path(before: &str, path: &OsStr, after: &str) -> ! {
    diag_path(before, path, after);
    process::exit(EXIT_PANIC)
}

// ---------------------------------------------------------------- the script

/// One address: a way of naming input lines.
enum Addr {
    /// A line number. `0` is not a line; it exists so `0,/re/` can end on the
    /// very first line, which `1,/re/` cannot.
    Line(usize),
    /// The last line of the input.
    Last,
    /// A regular expression. `None` is the empty `//`, which means "whatever
    /// pattern was matched last" and so can only be resolved while running.
    Re(Option<Rc<Regex>>),
    /// GNU's `first~step`.
    Step(usize, usize),
}

/// The second half of a range, which may be relative to where the range began.
enum EndAddr {
    Addr(Addr),
    /// `addr,+N` — N lines after the start.
    Plus(usize),
    /// `addr,~N` — on to the next line number that is a multiple of N.
    Multiple(usize),
}

/// Which lines a command applies to.
enum Sel {
    Always,
    One(Addr),
    Range(Addr, EndAddr),
}

impl Sel {
    /// Whether this selector uses line 0 somewhere line 0 does not exist.
    ///
    /// There is no line 0, and the only reason to write one is the `0,/re/`
    /// range: it may end on line 1, which `1,/re/` cannot, because a range's end
    /// is never tested before its start line. Every other use of it — `0p`,
    /// `0,5p`, `0,$p`, `0,+2p`, `0,0p` — selects nothing at all, so GNU refuses
    /// it rather than running a script that can only be a mistake. A `0` in the
    /// *second* position is fine and means what it says: `/a/,0p` is a range
    /// that ends immediately.
    fn rejects_line_zero(&self) -> bool {
        let first = match self {
            Sel::Always => return false,
            Sel::One(a) | Sel::Range(a, _) => a,
        };
        if !matches!(first, Addr::Line(0)) {
            return false;
        }
        !matches!(self, Sel::Range(_, EndAddr::Addr(Addr::Re(_))))
    }
}

/// A piece of an `s` command's replacement text.
enum Rep {
    Lit(Vec<u8>),
    /// `&` is group 0, `\1`–`\9` are the rest.
    Group(usize),
    Case(CaseOp),
}

/// GNU's case-folding escapes in a replacement.
#[derive(Clone, Copy)]
enum CaseOp {
    /// `\u` / `\l` — the next character only.
    OneUpper,
    OneLower,
    /// `\U` / `\L` — until `\E`.
    RestUpper,
    RestLower,
    /// `\E`.
    End,
}

struct Subst {
    /// `None` for `s//…/`: the last pattern matched, resolved at run time.
    re: Option<Rc<Regex>>,
    repl: Vec<Rep>,
    global: bool,
    /// The `N` of `s/…/…/N`; 1 unless given. With `g`, the Nth match *onwards*.
    occurrence: usize,
    print: bool,
    wfile: Option<usize>,
}

enum Action {
    /// `{` — holds the index just past its matching `}`, so an address that
    /// does not select can skip the whole block in one step.
    Block(usize),
    BlockEnd,
    /// `:label` — a no-op that branches aim at.
    Label,
    /// `b`, `t`, `T`. The index is where to jump; `cmds.len()` ends the cycle.
    Branch(usize),
    BranchIfSub(usize),
    BranchIfNoSub(usize),
    Subst(Box<Subst>),
    /// `y` — a whole byte table, so transliteration cannot fail on a byte that
    /// is not a character.
    Transliterate(Box<[u8; 256]>),
    Delete,
    DeleteFirstLine,
    Print,
    PrintFirstLine,
    Next,
    AppendNext,
    Hold,
    HoldAppend,
    Get,
    GetAppend,
    Exchange,
    LineNumber,
    AppendText(Vec<u8>),
    InsertText(Vec<u8>),
    ChangeText(Vec<u8>),
    ReadFile(String),
    /// `R` — one line per cycle from a shared handle. See [`RFile`].
    ReadLine(usize),
    WriteFile(usize),
    /// `W` — like `w`, but only as far as the first separator.
    WriteFirstLine(usize),
    /// `l` — the wrap width, already resolved against `-l` and the default of
    /// 70. Zero means never wrap. Resolving here rather than at run time is
    /// sound because nothing can change `-l` once the script is compiled.
    List(usize),
    Quit {
        code: i32,
        print: bool,
    },
}

struct Command {
    sel: Sel,
    negated: bool,
    act: Action,
}

struct Script {
    cmds: Vec<Command>,
    wfiles: Vec<String>,
    rfiles: Vec<String>,
    /// Set by a `#n` first line, which is POSIX's in-script spelling of `-n`.
    suppress: bool,
    /// Where a diagnostic raised *after* the script has been read points, as
    /// the bytes that go between `sed: ` and the message — or `None` when there
    /// is no script text to point into.
    ///
    /// There is one such diagnostic, `no previous regular expression`, and it
    /// belongs to the run rather than to the parse. GNU still gives it a
    /// location, and the location is always the very end of the joined script,
    /// whichever fragment wrote the empty regex: `sed -e 's//X/' -e p` names
    /// expression #2 and `sed -f g.sed -e p` names expression #1. Measured. See
    /// [`Pos::AfterParse`] for why an expression is always `char 0` there.
    end_loc: Option<Vec<u8>>,
}

// ---------------------------------------------------------------- the parser

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    ere: bool,
    /// The width `l` uses when it is given no number of its own: `-l N`, or 70.
    line_len: usize,
    /// `--sandbox`. It is the *parser* that enforces it, not the executor, so
    /// that a script naming a forbidden command is refused before any input is
    /// read — a sandbox that let the first half of a script run would not be
    /// one.
    sandbox: bool,
    wfiles: Vec<String>,
    rfiles: Vec<String>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.i = self.i.saturating_add(1);
        Some(c)
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.i = self.i.saturating_add(1);
            true
        } else {
            false
        }
    }

    fn skip_blank(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.i = self.i.saturating_add(1);
        }
    }

    /// Skip whatever may sit between two commands.
    ///
    /// No `\r`: a carriage return is an ordinary character to GNU, so a script
    /// with CRLF line endings is a syntax error there (`extra characters after
    /// command`) rather than silently working. Accepting it here would be a
    /// kindness that changes what a script means, and a script that relies on
    /// it would then fail on every other sed.
    fn skip_separators(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b';')) {
            self.i = self.i.saturating_add(1);
        }
    }

    /// GNU's `read_end_of_cmd`: what may follow a command that takes no
    /// argument of its own.
    ///
    /// Blanks are skipped, then only a separator, a `}`, a comment or the end
    /// of the script is allowed. Anything else is the fault upstream calls
    /// `extra characters after command` — which is why `sed px` is an error and
    /// not a `p` followed by a mystery.
    ///
    /// A `}` or a `#` is *left* where it is, for the caller to read: `{p}`
    /// closes its block and `p }` goes on to report the unexpected `}`. The
    /// offending character, by contrast, is consumed before the error, because
    /// GNU reports one past it — `sed 'px'` says char 2.
    fn end_of_cmd(&mut self) -> Result<(), String> {
        self.skip_blank();
        match self.peek() {
            None | Some(b'}' | b'#') => Ok(()),
            Some(b';' | b'\n') => {
                self.i = self.i.saturating_add(1);
                Ok(())
            }
            Some(_) => {
                self.i = self.i.saturating_add(1);
                Err("extra characters after command".to_string())
            }
        }
    }

    fn skip_to_eol(&mut self) {
        while !matches!(self.peek(), None | Some(b'\n')) {
            self.i = self.i.saturating_add(1);
        }
    }

    /// A run of digits, or zero if there are none.
    ///
    /// No digits is deliberately not an error: GNU's `in_integer` returns 0 for
    /// an empty run, so `sed '1~'` is `1~0` — a step of zero, which is just line
    /// 1 — and the fault it then reports is the absent *command*, not the absent
    /// number. Measured: `sed '1~'` says `char 2: missing command`.
    fn number(&mut self) -> Result<usize, String> {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i = self.i.saturating_add(1);
        }
        let digits = self.s.get(start..self.i).unwrap_or_default();
        let mut n: usize = 0;
        for &d in digits {
            n = n
                .checked_mul(10)
                .and_then(|x| x.checked_add(usize::from(d.wrapping_sub(b'0'))))
                .ok_or_else(|| "line number is too large".to_string())?;
        }
        Ok(n)
    }

    /// Read up to the next unescaped `delim`.
    ///
    /// `re` selects how `\<delim>` is spelt on the way out: a replacement wants
    /// the delimiter itself, but a *pattern* wants it neutralised, since a
    /// delimiter such as `.` or `|` would otherwise become the metacharacter it
    /// looks like. Every other escape is passed through untouched — this is not
    /// the place that decides what `\w` means.
    ///
    /// `re` also turns on bracket-expression skipping, because `s/[/]/X/` is a
    /// substitution of a slash and not an unterminated command: inside `[...]`
    /// the delimiter is an ordinary character. A scanner that did not know
    /// where brackets are would have to reject that script.
    fn take_until(&mut self, delim: u8, re: bool, what: &str) -> Result<Vec<u8>, String> {
        let unterminated = || format!("unterminated {what}");
        let mut out = Vec::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(unterminated());
            };
            if c == delim {
                return Ok(out);
            }
            if c == b'\n' {
                return Err(unterminated());
            }
            if re && c == b'[' {
                out.push(b'[');
                self.take_bracket(&mut out).ok_or_else(unterminated)?;
                continue;
            }
            if c != b'\\' {
                out.push(c);
                continue;
            }
            let Some(n) = self.bump() else {
                return Err("a script may not end in a backslash".to_string());
            };
            if n == delim {
                if re {
                    out.extend_from_slice(&literal_for(delim));
                } else {
                    out.push(delim);
                }
            } else if n == b'\n' {
                out.push(b'\n');
            } else {
                out.push(b'\\');
                out.push(n);
            }
        }
    }

    /// Copy the rest of a bracket expression, `[` already consumed.
    ///
    /// Nothing inside is interpreted — not the delimiter, not a backslash,
    /// which POSIX says is an ordinary character here. The only structure that
    /// matters is where the expression *ends*, and that is what the two leading
    /// special cases are about: `[]a]` and `[^]a]` hold a literal `]`.
    fn take_bracket(&mut self, out: &mut Vec<u8>) -> Option<()> {
        if self.peek() == Some(b'^') {
            out.push(self.bump()?);
        }
        if self.peek() == Some(b']') {
            out.push(self.bump()?);
        }
        loop {
            let c = self.bump()?;
            out.push(c);
            if c == b']' {
                return Some(());
            }
            // `[:alpha:]`, `[.ch.]` and `[=e=]` may contain a `]` of their own.
            if c == b'[' && matches!(self.peek(), Some(b':' | b'.' | b'=')) {
                let kind = self.bump()?;
                out.push(kind);
                loop {
                    let x = self.bump()?;
                    out.push(x);
                    if x == kind && self.peek() == Some(b']') {
                        out.push(self.bump()?);
                        break;
                    }
                }
            }
        }
    }

    /// `I` after a pattern asks for case-insensitive matching.
    fn re_flags(&mut self) -> Result<bool, String> {
        let mut ci = false;
        loop {
            match self.peek() {
                Some(b'I') => {
                    self.i = self.i.saturating_add(1);
                    ci = true;
                }
                // `M` makes `^`/`$` match at embedded newlines, which the
                // engine cannot express. Refusing beats matching the wrong
                // lines silently.
                Some(b'M') => return Err("the `M' regex modifier is not supported".to_string()),
                _ => return Ok(ci),
            }
        }
    }

    fn compile(&self, pat: &[u8], ci: bool) -> Result<Option<Rc<Regex>>, String> {
        // An empty pattern is not an error: `s//X/` and `//d` re-use the last
        // regular expression that was *tried*, which is a run-time value — and
        // whether there was one is a run-time question too, which is why
        // nothing here looks for a preceding regex. See [`Exec::resolve`].
        if pat.is_empty() {
            // A modifier has nothing to modify, since the pattern it would apply
            // to was compiled elsewhere with modifiers of its own. This one *is*
            // decided here: GNU refuses `s//X/I` while reading the script, with
            // no input read and whatever else the script says.
            if ci {
                return Err("cannot specify modifiers on empty regexp".to_string());
            }
            return Ok(None);
        }
        // The one funnel every pattern passes through, which is why GNU's
        // byte-naming escapes are converted here rather than at each of the
        // three places a pattern is written.
        let pat = &normalize_regex(pat)?;
        let r = if self.ere {
            Regex::new_flags(pat, ci)
        } else {
            bre::compile(pat, ci)
        };
        match r {
            Ok(re) => Ok(Some(Rc::new(re))),
            // `e.message()`, not `e.detail`: GNU sed hands the pattern to glibc
            // and prints back verbatim whatever `re_compile_pattern` returned,
            // so the sentence after `char N:` is one of glibc's fourteen fixed
            // strings — `Unmatched ( or \(`, `Trailing backslash` — and never
            // sed's own words. `ere`'s `detail` is the more useful sentence but
            // it is not the one a script grepping this line was written
            // against. Measured; pinned by the error block in
            // `scripts/sed-diff.sh`.
            Err(e) => Err(e.message().to_string()),
        }
    }

    fn parse_addr(&mut self) -> Result<Option<Addr>, String> {
        match self.peek() {
            Some(b'$') => {
                self.i = self.i.saturating_add(1);
                Ok(Some(Addr::Last))
            }
            Some(b'/') => {
                self.i = self.i.saturating_add(1);
                let pat = self.take_until(b'/', true, "address regex")?;
                let ci = self.re_flags()?;
                Ok(Some(Addr::Re(self.compile(&pat, ci)?)))
            }
            // `\cREc` — any delimiter, so a pattern full of slashes need not be
            // written full of backslashes.
            Some(b'\\') => {
                self.i = self.i.saturating_add(1);
                let d = self
                    .bump()
                    .ok_or_else(|| "expected a delimiter after `\\'".to_string())?;
                let pat = self.take_until(d, true, "address regex")?;
                let ci = self.re_flags()?;
                Ok(Some(Addr::Re(self.compile(&pat, ci)?)))
            }
            Some(c) if c.is_ascii_digit() => {
                let n = self.number()?;
                if self.eat(b'~') {
                    let step = self.number()?;
                    // A step of zero is no step at all, and GNU collapses it to
                    // the plain line number rather than carrying it — which is
                    // why `0~0p` is refused for using line 0 while `0~3p` is
                    // fine, and why `1~` (an absent step, hence zero) is line 1.
                    if step > 0 {
                        return Ok(Some(Addr::Step(n, step)));
                    }
                }
                Ok(Some(Addr::Line(n)))
            }
            _ => Ok(None),
        }
    }

    fn parse_sel(&mut self) -> Result<Sel, String> {
        let Some(a1) = self.parse_addr()? else {
            return Ok(Sel::Always);
        };
        self.skip_blank();
        if !self.eat(b',') {
            return Ok(Sel::One(a1));
        }
        self.skip_blank();
        let end = if self.eat(b'+') {
            EndAddr::Plus(self.number()?)
        } else if self.eat(b'~') {
            EndAddr::Multiple(self.number()?)
        } else {
            let Some(a2) = self.parse_addr()? else {
                return Err("expected an address after `,'".to_string());
            };
            EndAddr::Addr(a2)
        };
        Ok(Sel::Range(a1, end))
    }

    /// The text argument of `a`, `i` and `c`.
    ///
    /// Both spellings are accepted — POSIX's `a\` followed by the text on the
    /// next line, and the one-liner `a text` every script actually uses.
    ///
    /// The text takes the same byte-naming escapes as everything else, so
    /// `a p\tq\x41` appends `p`, a tab, `q`, `A` — measured against GNU, which
    /// runs the same `normalize_text` over it as over a pattern. Note that the
    /// `\` in `a\x41` is the *marker* introducing the text and is eaten above,
    /// which is why GNU appends the three characters `x41` there and not `A`.
    ///
    /// # The trailing newline is part of the result
    ///
    /// GNU's text buffer ends with a newline, and that is what tells "no text
    /// at all" from "one empty line" — an empty result here means the former
    /// and emits nothing. It is also the newline itself that `a` writes, which
    /// is why `sed -z 'a X'` ends the appended line with a newline and not with
    /// the NUL every other line ends with. See [`text_out`].
    fn parse_text(&mut self) -> Result<Vec<u8>, String> {
        self.skip_blank();
        // The end of the script is the one thing that cannot introduce text.
        // Everything else can, including a bare newline (`sed $'a\np'` appends
        // an empty line and then prints) and a bare backslash at the very end
        // (`sed 'a\'`, which appends nothing) — both measured, both accepted.
        if self.peek().is_none() {
            return Err("expected \\ after `a', `c' or `i'".to_string());
        }
        let mut out = Vec::new();
        if self.peek() == Some(b'\\') {
            self.i = self.i.saturating_add(1);
            // The character after the backslash is GNU's `leadin_ch`, and the
            // two interesting values of it are the two ends of the script's
            // line. A newline introduces text on the *next* line and is not
            // part of it — and the blanks that follow it are, which is why
            // nothing skips them here: `a\` + newline + `  X` appends two
            // spaces and an `X`, while the one-liner `a   X` appends only the
            // `X`. The end of the script means there is no text at all, which
            // is what separates `sed 'a\'` (appends nothing) from a `-f` file
            // ending `a\` *and a newline* (appends one empty line). All
            // measured.
            match self.peek() {
                None => return Ok(out),
                Some(b'\n') => self.i = self.i.saturating_add(1),
                Some(_) => {}
            }
        }
        while let Some(c) = self.peek() {
            if c == b'\n' {
                self.i = self.i.saturating_add(1);
                break;
            }
            self.i = self.i.saturating_add(1);
            if c != b'\\' {
                out.push(c);
                continue;
            }
            let esc = self.i;
            let Some(n) = self.bump() else { break };
            if let Some(b) = control_byte(n) {
                out.push(b);
                continue;
            }
            match named_byte(self.s, esc) {
                Some(Named::Byte(b, next)) => {
                    out.push(b);
                    self.i = next;
                }
                Some(Named::Backslash(next)) => {
                    out.push(b'\\');
                    self.i = next;
                }
                Some(Named::Recursive) => return Err(RECURSIVE_C.to_string()),
                // A continued line — the newline is part of the text — and
                // anything else: the character itself.
                None => out.push(n),
            }
        }
        out.push(b'\n');
        Ok(out)
    }

    /// A file name argument, which runs to the end of the line — a `;` in a
    /// file name is a character in a file name.
    fn parse_filename(&mut self) -> Result<String, String> {
        self.skip_blank();
        let start = self.i;
        self.skip_to_eol();
        let raw = self.s.get(start..self.i).unwrap_or_default();
        if raw.is_empty() {
            // Upstream's wording, which names all four commands that take a
            // file rather than the one that was written. Kept verbatim because
            // scripts and test suites match on it.
            return Err("missing filename in r/R/w/W commands".to_string());
        }
        String::from_utf8(raw.to_vec())
            .map_err(|_| "file names given to sed must be text".to_string())
    }

    /// Intern a `w` target so the same file opened twice is one handle, and
    /// two writes to it therefore append rather than truncate each other.
    ///
    /// `w` and `W` share this table, as upstream's do: `sed -n 'N;W f;w f'`
    /// must produce one file holding both writes in order, not two handles
    /// racing to truncate each other.
    fn wfile(&mut self, path: String) -> usize {
        if let Some(i) = self.wfiles.iter().position(|p| *p == path) {
            return i;
        }
        self.wfiles.push(path);
        self.wfiles.len().saturating_sub(1)
    }

    /// Intern an `R` source, for the same reason [`Parser::wfile`] interns a
    /// target — but the consequence is the opposite and more visible. Two `R`s
    /// naming one file share its read position, so in one cycle the first takes
    /// line 1 and the second line 2, rather than both taking line 1.
    fn rfile(&mut self, path: String) -> usize {
        if let Some(i) = self.rfiles.iter().position(|p| *p == path) {
            return i;
        }
        self.rfiles.push(path);
        self.rfiles.len().saturating_sub(1)
    }

    /// Refuse a command that reaches outside the script, under `--sandbox`.
    ///
    /// Upstream's wording names `e/r/w` for every one of them, including `R`
    /// and `W`, so the message is a category rather than the command that
    /// tripped it. The position is the parser's own, which puts it just past
    /// the command letter — or, for `s///w`, just past the flag.
    fn deny_in_sandbox(&self) -> Result<(), String> {
        if self.sandbox {
            return Err("e/r/w commands disabled in sandbox mode".to_string());
        }
        Ok(())
    }

    /// The label of `:`, `b`, `t` and `T`.
    ///
    /// It ends at the first *whitespace*, `;`, `}`, newline or end of script —
    /// so a label cannot contain a space, and what follows one is the next
    /// command rather than more label. That is GNU's rule and it is load-bearing
    /// in both directions: `sed 'b x y'` reads `b x` and then tries to run `y`
    /// (measured: `unterminated `y' command`), and `sed '{b}'` branches to the
    /// end of the script rather than to a label called `}`.
    fn parse_label(&mut self) -> Vec<u8> {
        self.skip_blank();
        let start = self.i;
        while !matches!(
            self.peek(),
            None | Some(b'\n' | b';' | b'}' | b' ' | b'\t' | b'\r')
        ) {
            self.i = self.i.saturating_add(1);
        }
        self.s.get(start..self.i).unwrap_or_default().to_vec()
    }

    fn parse_subst(&mut self) -> Result<Subst, String> {
        let delim = self
            .bump()
            // Not a message of its own: GNU reads the delimiter with the same
            // `inchar` it reads the rest of the command with, so a missing one
            // is the same fault as a missing closing delimiter — `sed 's'` and
            // `sed 's/a/b'` both say `unterminated 's' command`. Measured.
            .ok_or_else(|| "unterminated `s' command".to_string())?;
        if delim == b'\\' || delim == b'\n' {
            return Err("`s' may not be delimited by a backslash or a newline".to_string());
        }
        let pat = self.take_until(delim, true, "`s' command")?;
        let raw_repl = self.take_until(delim, false, "`s' command")?;

        let mut global = false;
        let mut occurrence = 0usize;
        let mut print = false;
        let mut ci = false;
        let mut wfile = None;
        // Every rejection below is reported *after* consuming the offending
        // character, because that is where GNU reports it: `s/a/b/x` says char
        // 7 and the `x` is at 6. Blanks separate flags but do not end them —
        // `s/a/b/2 3` is two number flags and is refused as such, at char 9.
        loop {
            self.skip_blank();
            match self.peek() {
                Some(b'g') => {
                    self.i = self.i.saturating_add(1);
                    if global {
                        return Err("multiple `g' options to `s' command".to_string());
                    }
                    global = true;
                }
                Some(b'p') => {
                    self.i = self.i.saturating_add(1);
                    if print {
                        return Err("multiple `p' options to `s' command".to_string());
                    }
                    print = true;
                }
                // No multiple-use check on these two: GNU has none either, so
                // `s/a/b/Ii` is accepted.
                Some(b'i' | b'I') => {
                    self.i = self.i.saturating_add(1);
                    ci = true;
                }
                Some(b'm' | b'M') => {
                    self.i = self.i.saturating_add(1);
                    return Err("the `M' flag of `s' is not supported".to_string());
                }
                Some(c) if c.is_ascii_digit() => {
                    let seen = occurrence != 0;
                    occurrence = self.number()?;
                    if seen {
                        return Err("multiple number options to `s' command".to_string());
                    }
                    if occurrence == 0 {
                        return Err("number option to `s' command may not be zero".to_string());
                    }
                }
                Some(b'w') => {
                    self.i = self.i.saturating_add(1);
                    self.deny_in_sandbox()?;
                    let path = self.parse_filename()?;
                    wfile = Some(self.wfile(path));
                    break;
                }
                // What ends the flags, and nothing else does. A `}` is left
                // where it is for `parse_body` to close the block with.
                None | Some(b';' | b'\n' | b'}' | b'#') => break,
                Some(_) => {
                    self.i = self.i.saturating_add(1);
                    return Err("unknown option to `s'".to_string());
                }
            }
        }

        let re = self.compile(&pat, ci)?;
        let repl = parse_replacement(&raw_repl)?;
        // A `\N` naming a group the pattern does not have is refused, not
        // silently empty — and refused here, after the flags, because that is
        // where GNU reports it: `s/a/\9/w f.txt` says char 14, the end of the
        // whole command. Skipped when the pattern is empty, since `s//\1/` will
        // re-use a regular expression that is not known until run time.
        if let Some(re) = re.as_deref() {
            let groups = re.group_count();
            if let Some(n) = repl.iter().find_map(|r| match *r {
                Rep::Group(n) if n > groups => Some(n),
                _ => None,
            }) {
                return Err(format!("invalid reference \\{n} on `s' command's RHS"));
            }
        }
        Ok(Subst {
            re,
            repl,
            global,
            occurrence: occurrence.max(1),
            print,
            wfile,
        })
    }

    fn parse_transliterate(&mut self) -> Result<Box<[u8; 256]>, String> {
        let delim = self
            .bump()
            // As for `s`, above: a missing delimiter is an unterminated command.
            .ok_or_else(|| "unterminated `y' command".to_string())?;
        let from = unescape_y(&self.take_until(delim, false, "`y' command")?)?;
        let to = unescape_y(&self.take_until(delim, false, "`y' command")?)?;
        if from.len() != to.len() {
            return Err("strings for `y' command are different lengths".to_string());
        }
        let mut table = Box::new([0u8; 256]);
        for (i, slot) in table.iter_mut().enumerate() {
            *slot = u8::try_from(i).unwrap_or(0);
        }
        for (f, t) in from.iter().zip(to.iter()) {
            if let Some(slot) = table.get_mut(usize::from(*f)) {
                *slot = *t;
            }
        }
        Ok(table)
    }
}

/// How to write `c` into a pattern so it stands for itself in both dialects.
///
/// Backslash-escaping works for the characters POSIX names as escapable, but
/// `\+` and `\(` are *more* special in a BRE, not less, so those go inside a
/// bracket expression instead — the one spelling that is literal everywhere.
fn literal_for(c: u8) -> Vec<u8> {
    match c {
        b'.' | b'*' | b'[' | b']' | b'^' | b'$' | b'\\' => vec![b'\\', c],
        b'+' | b'?' | b'(' | b')' | b'{' | b'}' | b'|' => vec![b'[', c, b']'],
        _ => vec![c],
    }
}

/// GNU's wording for `\c\`, which it refuses rather than guessing at.
const RECURSIVE_C: &str = "recursive escaping after \\c not allowed";

/// The escapes that name a control character by letter.
///
/// GNU converts these in the same pass as the numeric ones below, which is why
/// `\t` is a tab in a regular expression, in a replacement, in `y` and in `a`
/// text alike. `\b` is deliberately absent: GNU sed 4.0 read it as a backspace,
/// but every version since reads it as a word boundary, which is `ere`'s to
/// interpret and not ours.
fn control_byte(c: u8) -> Option<u8> {
    Some(match c {
        b'a' => 0x07,
        b'f' => 0x0c,
        b'n' => b'\n',
        b'r' => b'\r',
        b't' => b'\t',
        b'v' => 0x0b,
        _ => return None,
    })
}

/// One digit of `base`, or `None` if `c` is not one.
fn digit(c: u8, base: u8) -> Option<u8> {
    let v = match c {
        b'0'..=b'9' => c.wrapping_sub(b'0'),
        b'a'..=b'f' => c.wrapping_sub(b'a').wrapping_add(10),
        b'A'..=b'F' => c.wrapping_sub(b'A').wrapping_add(10),
        _ => return None,
    };
    (v < base).then_some(v)
}

/// What one of GNU's byte-naming escapes turned out to be.
enum Named {
    /// The byte it names, and the index just past the escape.
    Byte(u8, usize),
    /// `\c` with the script ending right after it: GNU emits a lone backslash
    /// and drops the `c`.
    Backslash(usize),
    /// `\c\`, which GNU refuses — see [`RECURSIVE_C`].
    Recursive,
}

/// Read `\xNN`, `\oNNN`, `\dNNN` or `\cX`, where `raw[i]` is the character
/// *after* the backslash.
///
/// `None` means the escape is none of those and belongs to whoever asked: `\w`
/// to the regex engine, `\1` to the replacement parser, `\;` to nobody.
///
/// The three numeric forms are GNU's `convert_number`, measured rather than
/// recalled: **at most two** hexadecimal digits and **at most three** decimal
/// or octal ones, the value taken mod 256, and *no* digits at all is not an
/// error — the letter then denotes itself, which is why `sed 's/\x/Z/'`
/// replaces an `x`. So GNU reads `\x616` as `a` then `6`, `\d0977` as `a` then
/// `7`, `\x0061` as NUL then `61`, and `\d300` as `,`.
fn named_byte(raw: &[u8], i: usize) -> Option<Named> {
    let letter = raw.get(i).copied()?;
    let (base, max) = match letter {
        b'x' => (16u8, 2usize),
        b'd' => (10, 3),
        b'o' => (8, 3),
        b'c' => {
            let after = i.saturating_add(1);
            return Some(match raw.get(after).copied() {
                None => Named::Backslash(after),
                Some(b'\\') => Named::Recursive,
                // GNU's own arithmetic: fold to upper case, then flip the bit
                // that separates a control code from its printable partner, so
                // `\cI` is a tab and `\c1` is `q`.
                Some(c) => Named::Byte(c.to_ascii_uppercase() ^ 0x40, after.saturating_add(1)),
            });
        }
        _ => return None,
    };
    // Accumulating in a `u8` is the mod-256 wrap, not an accident of width:
    // GNU stores the running value in a `char`, which is why `\d300` is `,`.
    let mut n: u8 = 0;
    let first = i.saturating_add(1);
    let mut j = first;
    let end = first.saturating_add(max);
    while j < end {
        let Some(d) = raw.get(j).copied().and_then(|c| digit(c, base)) else {
            break;
        };
        n = n.wrapping_mul(base).wrapping_add(d);
        j = j.saturating_add(1);
    }
    Some(if j == first {
        Named::Byte(letter, j)
    } else {
        Named::Byte(n, j)
    })
}

/// GNU's `normalize_text` for a regular expression: every escape that names a
/// byte becomes that byte, and every other escape is left for the regex engine.
///
/// The produced byte is **not** protected, which is GNU's behaviour and is
/// surprising enough to be worth stating: `\x2e` is the metacharacter `.` and
/// not a literal dot, so `sed 's/\x2e/Z/'` replaces the first character of any
/// line. `\x5c` is a bare backslash, so `sed 's/\x5c/Z/'` is a *trailing
/// backslash* error — which is exactly what GNU reports. Both measured.
///
/// The conversion runs after the delimiter scan, so a delimiter it produces is
/// a character and not the end of the command: `sed 's/\x2f/Z/'` replaces a
/// slash.
fn normalize_regex(raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0usize;
    while let Some(&c) = raw.get(i) {
        i = i.saturating_add(1);
        if c != b'\\' {
            out.push(c);
            continue;
        }
        let Some(&n) = raw.get(i) else {
            out.push(b'\\');
            break;
        };
        if let Some(b) = control_byte(n) {
            out.push(b);
            i = i.saturating_add(1);
            continue;
        }
        match named_byte(raw, i) {
            Some(Named::Byte(b, next)) => {
                out.push(b);
                i = next;
            }
            Some(Named::Backslash(next)) => {
                out.push(b'\\');
                i = next;
            }
            Some(Named::Recursive) => return Err(RECURSIVE_C.to_string()),
            // Not ours: `\w`, `\(`, `\1`, `\.` all reach the engine as written.
            None => {
                out.push(b'\\');
                out.push(n);
                i = i.saturating_add(1);
            }
        }
    }
    Ok(out)
}

/// `y` takes text, not a pattern, so only the escapes that name a byte apply.
fn unescape_y(raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0usize;
    while let Some(&c) = raw.get(i) {
        i = i.saturating_add(1);
        if c != b'\\' {
            out.push(c);
            continue;
        }
        let Some(&n) = raw.get(i) else {
            out.push(b'\\');
            break;
        };
        if let Some(b) = control_byte(n) {
            out.push(b);
            i = i.saturating_add(1);
            continue;
        }
        match named_byte(raw, i) {
            Some(Named::Byte(b, next)) => {
                out.push(b);
                i = next;
            }
            Some(Named::Backslash(next)) => {
                out.push(b'\\');
                i = next;
            }
            Some(Named::Recursive) => return Err(RECURSIVE_C.to_string()),
            // `\\` and anything else: the character itself.
            None => {
                out.push(n);
                i = i.saturating_add(1);
            }
        }
    }
    Ok(out)
}

/// Parse an `s` command's replacement into its literal and substituted parts.
///
/// A byte an escape *names* is a literal by construction here — it goes into
/// the pending literal run rather than back into the text — which is how GNU's
/// protection of the same byte works out: `s/a/\x26/` yields a `&` and not the
/// whole match, and `s/a/\x5cn/` yields a backslash followed by an `n` and not
/// a newline. Both measured.
fn parse_replacement(raw: &[u8]) -> Result<Vec<Rep>, String> {
    let mut parts: Vec<Rep> = Vec::new();
    let mut lit: Vec<u8> = Vec::new();
    let flush = |lit: &mut Vec<u8>, parts: &mut Vec<Rep>| {
        if !lit.is_empty() {
            parts.push(Rep::Lit(std::mem::take(lit)));
        }
    };

    let mut i = 0usize;
    while let Some(&c) = raw.get(i) {
        i = i.saturating_add(1);
        if c == b'&' {
            flush(&mut lit, &mut parts);
            parts.push(Rep::Group(0));
            continue;
        }
        if c != b'\\' {
            lit.push(c);
            continue;
        }
        let Some(&n) = raw.get(i) else {
            lit.push(b'\\');
            break;
        };
        let esc = i;
        i = i.saturating_add(1);
        match n {
            b'0'..=b'9' => {
                flush(&mut lit, &mut parts);
                parts.push(Rep::Group(usize::from(n.wrapping_sub(b'0'))));
            }
            b'u' | b'l' | b'U' | b'L' | b'E' => {
                flush(&mut lit, &mut parts);
                parts.push(Rep::Case(match n {
                    b'u' => CaseOp::OneUpper,
                    b'l' => CaseOp::OneLower,
                    b'U' => CaseOp::RestUpper,
                    b'L' => CaseOp::RestLower,
                    _ => CaseOp::End,
                }));
            }
            _ => {
                if let Some(b) = control_byte(n) {
                    lit.push(b);
                    continue;
                }
                match named_byte(raw, esc) {
                    Some(Named::Byte(b, next)) => {
                        lit.push(b);
                        i = next;
                    }
                    Some(Named::Backslash(next)) => {
                        lit.push(b'\\');
                        i = next;
                    }
                    Some(Named::Recursive) => return Err(RECURSIVE_C.to_string()),
                    // `\&`, `\\`, `\<newline>` and anything else: the character
                    // itself.
                    None => lit.push(n),
                }
            }
        }
    }
    flush(&mut lit, &mut parts);
    Ok(parts)
}

/// Why a script would not compile.
///
/// `at` is the offset the parser had reached, which is what makes a diagnostic
/// about a long one-liner usable; a failure with no position is one that is not
/// about a place in the text — an unresolved label is about the script's shape.
#[cfg_attr(test, derive(Debug))]
struct ScriptError {
    pos: Pos,
    msg: String,
    code: i32,
}

/// Where a script error is, in the joined script text.
///
/// The offset alone is not enough, because two of GNU's faults are reported
/// only once the whole program has been read. See [`Pos::AfterParse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pos {
    /// At this byte offset: the character count is the offset within whichever
    /// `-e` fragment or `-f` file the offset lands in.
    At(usize),
    /// This offset picks the fragment — and, in a `-f` file, the line — but the
    /// character offset within an `-e` fragment is 0 whatever the offset is.
    ///
    /// That is not a simplification, it is what upstream prints. `bad_prog`
    /// renders a string expression's position as `prog.cur - prog.base`, and
    /// both pointers are cleared once the expression has been read, so any
    /// diagnostic raised after parsing says char 0. Two are: the unmatched `{`,
    /// which restores the location saved when the brace was read, and the
    /// leading empty regex, which restores nothing and so lands at the end of
    /// the script. The difference between them is invisible in an `-e` fragment
    /// and plain in a `-f` file, which has real line numbers: `printf 'p\n{p\n'`
    /// reports line 2 — the brace — while `printf 'p\ns//X/\n'` reports line 3,
    /// one past the last line there is. Both measured.
    AfterParse(usize),
    /// Nowhere: an unresolvable label, which GNU reports with no location at
    /// all because it is found after the program has been read.
    Nowhere,
}

enum ScriptFail {
    Syntax(String),
    /// A syntax error somewhere other than where the parser stopped.
    SyntaxAt(Pos, String),
    Label(String),
}

impl From<String> for ScriptFail {
    fn from(msg: String) -> ScriptFail {
        ScriptFail::Syntax(msg)
    }
}

/// Compile a whole script.
///
/// The `-e` fragments and `-f` files are joined with newlines and parsed once,
/// because a `{` may be opened in one fragment and closed in the next.
fn compile_script(
    script: &[u8],
    segments: &[Segment],
    ere: bool,
    line_len: usize,
    sandbox: bool,
) -> Result<Script, ScriptError> {
    let mut p = Parser {
        s: script,
        i: 0,
        ere,
        line_len,
        sandbox,
        wfiles: Vec::new(),
        rfiles: Vec::new(),
    };
    match parse_body(&mut p, script) {
        Ok(mut s) => {
            s.end_loc = locate(script, segments, Pos::AfterParse(script.len()));
            Ok(s)
        }
        Err(ScriptFail::Syntax(msg)) => Err(ScriptError {
            pos: Pos::At(p.i),
            msg,
            code: 1,
        }),
        Err(ScriptFail::SyntaxAt(pos, msg)) => Err(ScriptError { pos, msg, code: 1 }),
        // GNU reports an unresolvable label after parsing has finished, and
        // gives it its own status. Matching that keeps a script that checks
        // `$?` behaving the same under either sed.
        Err(ScriptFail::Label(msg)) => Err(ScriptError {
            pos: Pos::Nowhere,
            msg,
            code: 4,
        }),
    }
}

fn parse_body(p: &mut Parser<'_>, script: &[u8]) -> Result<Script, ScriptFail> {
    let mut cmds: Vec<Command> = Vec::new();
    // The command index of each unclosed `{`, paired with the byte offset of the
    // brace itself — which is needed only to report an unclosed one where GNU
    // reports it. See [`Pos::AfterParse`].
    let mut open: Vec<(usize, usize)> = Vec::new();
    let mut labels: Vec<(Vec<u8>, usize)> = Vec::new();
    let mut branches: Vec<(usize, Vec<u8>)> = Vec::new();

    // POSIX: `#n` on the very first line is `-n`. A `#` anywhere else, and
    // `#no` on the first line, are ordinary comments.
    let mut suppress = false;
    if script.starts_with(b"#n") && matches!(script.get(2), None | Some(b'\n')) {
        suppress = true;
        p.i = 2;
    }

    loop {
        p.skip_separators();
        match p.peek() {
            None => break,
            Some(b'#') => {
                p.skip_to_eol();
                continue;
            }
            Some(b'}') => {
                p.i = p.i.saturating_add(1);
                let (start, _) = open.pop().ok_or_else(|| "unexpected `}'".to_string())?;
                let here = cmds.len();
                cmds.push(Command {
                    sel: Sel::Always,
                    negated: false,
                    act: Action::BlockEnd,
                });
                if let Some(c) = cmds.get_mut(start) {
                    c.act = Action::Block(here.saturating_add(1));
                }
                // A `}` takes no argument either, so `{p}p` is junk after a
                // command and not two commands — measured, char 4.
                p.end_of_cmd()?;
                continue;
            }
            Some(_) => {}
        }

        let sel = p.parse_sel()?;
        p.skip_blank();
        if sel.rejects_line_zero() {
            // GNU makes this check having already read the character that
            // follows the address — the `!` or the command letter — so the
            // offset it prints is one past it, except at the end of the script
            // where there was nothing to read. Measured: `0p` says char 2,
            // `0!p` says char 2 as well (the `!` is that character), and a bare
            // `0` says char 1.
            let at = if p.peek().is_some() {
                p.i.saturating_add(1)
            } else {
                p.i
            };
            return Err(ScriptFail::SyntaxAt(
                Pos::At(at),
                "invalid usage of line address 0".to_string(),
            ));
        }
        let mut negated = false;
        while p.eat(b'!') {
            negated = !negated;
            p.skip_blank();
        }
        let Some(c) = p.bump() else {
            return Err(ScriptFail::Syntax("missing command".to_string()));
        };
        let here = cmds.len();
        let act = match c {
            b'{' => {
                // `p.i` is just past the brace, so back up one to name it.
                open.push((here, p.i.saturating_sub(1)));
                Action::Block(0)
            }
            b'}' => return Err(ScriptFail::Syntax("unexpected `}'".to_string())),
            b':' => {
                let name = p.parse_label();
                if name.is_empty() {
                    // GNU's wording, double quotes and all.
                    return Err(ScriptFail::Syntax("\":\" lacks a label".to_string()));
                }
                labels.push((name, here));
                Action::Label
            }
            b'b' | b't' | b'T' => {
                let name = p.parse_label();
                branches.push((here, name));
                match c {
                    b'b' => Action::Branch(0),
                    b't' => Action::BranchIfSub(0),
                    _ => Action::BranchIfNoSub(0),
                }
            }
            b's' => Action::Subst(Box::new(p.parse_subst()?)),
            b'y' => Action::Transliterate(p.parse_transliterate()?),
            b'd' => Action::Delete,
            b'D' => Action::DeleteFirstLine,
            b'p' => Action::Print,
            b'P' => Action::PrintFirstLine,
            b'n' => Action::Next,
            b'N' => Action::AppendNext,
            b'h' => Action::Hold,
            b'H' => Action::HoldAppend,
            b'g' => Action::Get,
            b'G' => Action::GetAppend,
            b'x' => Action::Exchange,
            b'=' => Action::LineNumber,
            b'a' => Action::AppendText(p.parse_text()?),
            b'i' => Action::InsertText(p.parse_text()?),
            b'c' => Action::ChangeText(p.parse_text()?),
            b'r' => {
                p.deny_in_sandbox()?;
                Action::ReadFile(p.parse_filename()?)
            }
            b'R' => {
                p.deny_in_sandbox()?;
                let path = p.parse_filename()?;
                Action::ReadLine(p.rfile(path))
            }
            b'w' => {
                p.deny_in_sandbox()?;
                let path = p.parse_filename()?;
                Action::WriteFile(p.wfile(path))
            }
            b'W' => {
                p.deny_in_sandbox()?;
                let path = p.parse_filename()?;
                Action::WriteFirstLine(p.wfile(path))
            }
            b'l' => {
                // The number is optional and overrides `-l` for this command
                // alone. `l 0` and `l0` are both spellings of "never wrap",
                // which is also what `-l 0` means.
                p.skip_blank();
                let width = if p.peek().is_some_and(|d| d.is_ascii_digit()) {
                    p.number()?
                } else {
                    p.line_len
                };
                Action::List(width)
            }
            b'q' | b'Q' => {
                p.skip_blank();
                let code = if p.peek().is_some_and(|d| d.is_ascii_digit()) {
                    i32::try_from(p.number()?).unwrap_or(i32::MAX)
                } else {
                    0
                };
                Action::Quit {
                    code,
                    print: c == b'q',
                }
            }
            other => {
                return Err(ScriptFail::Syntax(format!(
                    "unknown command: `{}'",
                    other.escape_ascii()
                )));
            }
        };
        // GNU's `read_end_of_cmd`, run after every command that does not read to
        // the end of its own line. `a`, `i`, `c`, `r`, `R`, `w`, `W`, `b`, `t`,
        // `T` and `:` all take the rest of the line as their argument, so there
        // is nothing left that could be junk; `s` ends with its own flag scan,
        // which refuses junk in its own words (`unknown option to `s'`); and `{`
        // is followed by more commands by definition.
        if !matches!(
            c,
            b'{' | b'}'
                | b':'
                | b'b'
                | b't'
                | b'T'
                | b's'
                | b'a'
                | b'i'
                | b'c'
                | b'r'
                | b'R'
                | b'w'
                | b'W'
        ) {
            p.end_of_cmd()?;
        }
        cmds.push(Command { sel, negated, act });
    }

    // The *innermost* unclosed brace, not the outermost: GNU pushes each open
    // block onto the head of a list and reports the head, so of `{\np\n{\np\n`
    // it names line 3 and not line 1 — measured. With both braces in one `-e`
    // that is invisible (char 0 either way), but a `-f` file gives each its own
    // line and a second `-e` gives each its own expression number.
    if let Some(&(_, brace)) = open.last() {
        return Err(ScriptFail::SyntaxAt(
            Pos::AfterParse(brace),
            "unmatched `{'".to_string(),
        ));
    }

    let end = cmds.len();
    for (at, name) in branches {
        let target = if name.is_empty() {
            end
        } else {
            *labels
                .iter()
                .find(|(l, _)| *l == name)
                .map(|(_, i)| i)
                .ok_or_else(|| {
                    ScriptFail::Label(format!(
                        "can't find label for jump to `{}'",
                        String::from_utf8_lossy(&name)
                    ))
                })?
        };
        if let Some(cmd) = cmds.get_mut(at) {
            cmd.act = match cmd.act {
                Action::Branch(_) => Action::Branch(target),
                Action::BranchIfSub(_) => Action::BranchIfSub(target),
                _ => Action::BranchIfNoSub(target),
            };
        }
    }

    Ok(Script {
        cmds,
        wfiles: std::mem::take(&mut p.wfiles),
        rfiles: std::mem::take(&mut p.rfiles),
        suppress,
        // Filled in by [`compile_script`], which is the only caller that knows
        // the fragment map the offset has to be resolved against.
        end_loc: None,
    })
}

// ----------------------------------------------------------------- the input

struct Line {
    bytes: Vec<u8>,
    /// Whether the line ended with the separator, as opposed to end-of-file.
    had_sep: bool,
}

/// The lines of a list of files, read as one stream.
///
/// It reads one line ahead, because `$` cannot be answered without knowing
/// whether anything follows — and with several files that question crosses a
/// file boundary.
struct Input {
    paths: Vec<OsString>,
    next_path: usize,
    cur: Option<Box<dyn BufRead>>,
    /// What to call the file now open, in a read-error diagnostic. Upstream
    /// names standard input `stdin` there even though the operand was `-`.
    cur_name: OsString,
    peeked: Option<Line>,
    sep: u8,
    had_error: bool,
}

impl Input {
    fn new(paths: Vec<OsString>, sep: u8) -> Input {
        Input {
            paths,
            next_path: 0,
            cur: None,
            cur_name: OsString::from("stdin"),
            peeked: None,
            sep,
            had_error: false,
        }
    }

    fn open_next(&mut self) -> bool {
        while let Some(path) = self.paths.get(self.next_path) {
            let path = path.clone();
            self.next_path = self.next_path.saturating_add(1);
            if path == "-" {
                self.cur = Some(Box::new(BufReader::new(io::stdin())));
                self.cur_name = OsString::from("stdin");
                return true;
            }
            match File::open(&path) {
                Ok(f) => {
                    self.cur = Some(Box::new(BufReader::new(f)));
                    self.cur_name = path;
                    return true;
                }
                Err(e) => {
                    diag_path("sed: can't read ", &path, &format!(": {}", strerror(&e)));
                    self.had_error = true;
                }
            }
        }
        false
    }

    fn fill(&mut self) {
        if self.peeked.is_some() {
            return;
        }
        loop {
            if self.cur.is_none() && !self.open_next() {
                return;
            }
            let Some(r) = self.cur.as_mut() else { return };
            let mut buf = Vec::new();
            match r.read_until(self.sep, &mut buf) {
                Ok(0) => {
                    self.cur = None;
                }
                Ok(_) => {
                    let had_sep = buf.last() == Some(&self.sep);
                    if had_sep {
                        buf.pop();
                    }
                    self.peeked = Some(Line {
                        bytes: buf,
                        had_sep,
                    });
                    return;
                }
                // A read that fails part-way through a file is not the same
                // as a file that would not open: there is no sensible way to
                // carry on with the rest of the stream, so this ends the run.
                Err(e) => panic_path(
                    "sed: read error on ",
                    &self.cur_name,
                    &format!(": {}", strerror(&e)),
                ),
            }
        }
    }

    fn next_line(&mut self) -> Option<Line> {
        self.fill();
        self.peeked.take()
    }

    /// Whether the line just handed out was the last one there will be.
    fn at_end(&mut self) -> bool {
        self.fill();
        self.peeked.is_none()
    }
}

// ---------------------------------------------------------------- the output

/// A sink that holds a missing separator back.
///
/// The input's last line may have no newline, and then the output's last line
/// must have none either — but any *earlier* copy of that same line does need
/// one. Deciding at write time is impossible; deciding at the next write is
/// exactly right, and costs one flag.
struct Out<'a> {
    w: &'a mut dyn Write,
    sep: u8,
    owed: bool,
}

impl Out<'_> {
    fn line(&mut self, bytes: &[u8], sep: bool) -> io::Result<()> {
        if self.owed {
            self.w.write_all(&[self.sep])?;
            self.owed = false;
        }
        self.w.write_all(bytes)?;
        if sep {
            self.w.write_all(&[self.sep])?;
        } else {
            self.owed = true;
        }
        Ok(())
    }

    fn raw(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.owed {
            self.w.write_all(&[self.sep])?;
            self.owed = false;
        }
        self.w.write_all(bytes)
    }
}

/// Write the text of an `i` or a `c`.
///
/// Two things separate this from writing the text as it is stored. An empty
/// text is *no* text — `sed '2i\'` inserts nothing, and is not the same as the
/// one empty line `a\` followed by a newline appends — so it writes nothing at
/// all rather than a bare separator. And the newline the text carries is
/// dropped and re-supplied as the output separator, so `sed -z 'i X'` ends the
/// inserted line with a NUL like every other line.
///
/// That last point is where `i` and `c` part company with `a`, which writes its
/// text unchanged and so ends it with a newline even under `-z`. Both measured;
/// upstream splits them the same way, because `i` and `c` write through the
/// missing-newline bookkeeping and the append queue does not.
fn text_out(out: &mut Out, t: &[u8]) -> io::Result<()> {
    if t.is_empty() {
        return Ok(());
    }
    out.line(t.strip_suffix(b"\n").unwrap_or(t), true)
}

// -------------------------------------------------------------- the executor

/// What a run of the script decided about the current cycle.
enum Flow {
    /// Fell off the end: print the pattern space unless `-n`.
    Normal,
    /// `d` — the cycle ends with nothing printed.
    Deleted,
    /// `D` with an embedded newline — run the script again on what is left,
    /// without reading a new line.
    Restart,
    Quit {
        code: i32,
        print: bool,
    },
}

/// Text queued by `a` or `r`, emitted after the cycle's own output.
enum Pending {
    File(String),
    /// Bytes that already end however they should end, and so are written with
    /// nothing added: one line read by `R`, separator and all — or without one,
    /// if it was the last line of a file that had none — and the text of an
    /// `a`, which carries the newline [`Parser::parse_text`] gave it.
    Raw(Vec<u8>),
}

/// Where a `w` (or `s///w`) target sends its lines.
enum WTarget {
    /// `/dev/stdout` is not opened: it is sed's own output stream, so writing
    /// it through the same [`Out`] is what keeps `w /dev/stdout` interleaved
    /// with the pattern space rather than racing it through a second buffer.
    Stdout,
    /// `/dev/stderr` goes to descriptor 2 with one `write(2)` per line.
    Stderr,
    File(File),
}

/// One opened `w` target, with the separator it may still owe.
///
/// The `owed` flag is the same trick [`Out`] plays, for the same reason: a
/// pattern space that came from a line with no trailing separator must be
/// written without one, *unless* another line follows it in this file.
struct WFile {
    target: WTarget,
    owed: bool,
}

/// Open every `w` target the script names, before a line of input is read.
///
/// Two things depend on this happening at compile time rather than at the
/// first write. `sed 'w /nosuch/f' </dev/null` must fail even though the
/// command never runs — upstream opens the files while compiling, so an
/// unwritable target is a startup failure, not a surprise half way through a
/// long pipeline. And the handle has to outlive the per-file [`Exec`]: with
/// `-s`, or with `-i` over several files, a target reopened per input file
/// would truncate away what the previous file wrote.
fn open_wfiles(paths: &[String]) -> Vec<WFile> {
    paths
        .iter()
        .map(|path| {
            let target = match path.as_str() {
                "/dev/stdout" => WTarget::Stdout,
                "/dev/stderr" => WTarget::Stderr,
                _ => match File::create(path) {
                    Ok(f) => WTarget::File(f),
                    Err(e) => {
                        panic_out(&format!("couldn't open file {path}: {}", strerror(&e)));
                    }
                },
            };
            WFile {
                target,
                owed: false,
            }
        })
        .collect()
}

/// One `R` source: a handle that yields a single line per execution.
///
/// A missing or unopenable file is `None` rather than an error. That is
/// upstream's behaviour and it is deliberate on both sides: `R` is an
/// inclusion, and a script that runs before its optional include exists should
/// produce the text without it, not a diagnostic. (`r` agrees; `w` does not,
/// because a write that goes nowhere loses data.)
struct RFile {
    r: Option<BufReader<File>>,
}

/// Open every `R` source the script names, before a line of input is read.
///
/// Eagerly, like [`open_wfiles`], and for the second of that function's two
/// reasons: the handle carries a read position that has to survive the
/// per-input-file [`Exec`], or `R` would restart from line 1 on every input
/// file even without `-s`.
fn open_rfiles(paths: &[String]) -> Vec<RFile> {
    paths
        .iter()
        .map(|path| RFile {
            r: File::open(path).ok().map(BufReader::new),
        })
        .collect()
}

/// Return every `R` source to its first line.
///
/// `-s` (and `-i`, which implies it) makes each input file a fresh stream, and
/// upstream extends that to the `R` sources: `sed -s 'R inc' a b` pairs `inc`'s
/// first lines with *both* files rather than continuing through it. A handle
/// that cannot seek is left where it is, which is the best available answer for
/// a pipe or a device.
fn rewind_rfiles(rfiles: &mut [RFile]) {
    for f in rfiles {
        if let Some(r) = f.r.as_mut() {
            let _ = r.rewind();
        }
    }
}

struct RangeState {
    active: bool,
    /// For `addr,N`, `addr,+N` and `addr,~N`: the line the range closes on.
    end_line: Option<usize>,
}

/// What stopped a cycle before it finished.
///
/// Two things can, and they need different diagnostics: a write that failed,
/// and a regex search that gave up. The second is why this is an enum rather
/// than a bare [`io::Error`] — a match limit reported as "couldn't write" would
/// send the reader to the disk. Only a pattern holding a backreference can
/// produce one, and reporting it as "did not match" is not an option: `/re/!d`
/// deletes every line the pattern does *not* match.
enum Stop {
    Io(io::Error),
    Limit(ere::MatchLimit),
    /// An empty regular expression with nothing to repeat.
    ///
    /// Carries no message because there is only one, and no location because
    /// the location is a property of the script rather than of this cycle: see
    /// [`Script::end_loc`].
    NoRegex,
}

impl From<io::Error> for Stop {
    fn from(e: io::Error) -> Stop {
        Stop::Io(e)
    }
}

impl From<ere::MatchLimit> for Stop {
    fn from(e: ere::MatchLimit) -> Stop {
        Stop::Limit(e)
    }
}

type Run<T> = Result<T, Stop>;

struct Exec<'w> {
    pattern: Vec<u8>,
    hold: Vec<u8>,
    line_num: usize,
    had_sep: bool,
    sub_made: bool,
    appends: Vec<Pending>,
    last_re: Option<Rc<Regex>>,
    ranges: Vec<RangeState>,
    /// Borrowed, not owned: the `w` targets are opened once for the whole run
    /// and survive every `Exec` built over them. See [`open_wfiles`].
    wfiles: &'w mut Vec<WFile>,
    /// Borrowed for the same reason as `wfiles`, and see [`open_rfiles`].
    rfiles: &'w mut Vec<RFile>,
    suppress: bool,
}

impl<'w> Exec<'w> {
    fn new(
        script: &Script,
        suppress: bool,
        wfiles: &'w mut Vec<WFile>,
        rfiles: &'w mut Vec<RFile>,
    ) -> Exec<'w> {
        let mut ranges: Vec<RangeState> = Vec::with_capacity(script.cmds.len());
        for cmd in &script.cmds {
            // `0,/re/` is the one range that is open before any line is read;
            // that is the whole point of it, and why a line number of 0 exists.
            let (active, end_line) = match &cmd.sel {
                Sel::Range(Addr::Line(0), end) => (
                    true,
                    match end {
                        EndAddr::Addr(Addr::Line(n)) => Some(*n),
                        _ => None,
                    },
                ),
                _ => (false, None),
            };
            ranges.push(RangeState { active, end_line });
        }
        Exec {
            pattern: Vec::new(),
            hold: Vec::new(),
            line_num: 0,
            had_sep: true,
            sub_made: false,
            appends: Vec::new(),
            last_re: None,
            ranges,
            wfiles,
            rfiles,
            suppress,
        }
    }

    /// Resolve `//` and record what was tried, so the next `//` can find it.
    ///
    /// *Tried*, not matched: merely evaluating an address sets it, whether the
    /// address selected the line or not. That is why `sed '/b/{p};s//X/'`
    /// substitutes `/b/` on every line and not only on the ones `/b/` picked —
    /// measured, and the reason this records the regex before the search rather
    /// than after a successful one.
    ///
    /// An empty regex with nothing before it is a run-time failure, not a
    /// compile-time one: GNU has no static check at all, so `sed 's//X/'` over
    /// an empty file reads no line, runs no command and exits 0. Also measured.
    fn resolve(&mut self, r: Option<&Rc<Regex>>) -> Run<Rc<Regex>> {
        let re = match r {
            Some(x) => Rc::clone(x),
            None => match &self.last_re {
                Some(x) => Rc::clone(x),
                None => return Err(Stop::NoRegex),
            },
        };
        self.last_re = Some(Rc::clone(&re));
        Ok(re)
    }

    fn addr_match(&mut self, a: &Addr, input: &mut Input) -> Run<bool> {
        Ok(match a {
            Addr::Line(n) => self.line_num == *n,
            Addr::Last => input.at_end(),
            Addr::Step(first, step) => {
                if *step == 0 {
                    self.line_num == *first
                } else {
                    self.line_num >= *first
                        && self.line_num.saturating_sub(*first).is_multiple_of(*step)
                }
            }
            Addr::Re(r) => self.resolve(r.as_ref())?.find(&self.pattern)?.is_some(),
        })
    }

    fn range_select(&mut self, pc: usize, a1: &Addr, a2: &EndAddr, input: &mut Input) -> Run<bool> {
        if !self.ranges.get(pc).is_some_and(|r| r.active) {
            if !self.addr_match(a1, input)? {
                return Ok(false);
            }
            // A range whose end is already behind us selects this line alone.
            let end = match a2 {
                EndAddr::Addr(Addr::Line(n)) => {
                    if *n <= self.line_num {
                        return Ok(true);
                    }
                    Some(*n)
                }
                EndAddr::Plus(0) => return Ok(true),
                EndAddr::Plus(n) => Some(self.line_num.saturating_add(*n)),
                EndAddr::Multiple(0) => return Ok(true),
                // "on to the next line number that is a multiple of N" — the
                // *next* one, so `4,~4` on five lines runs 4 to the end rather
                // than stopping on the 4 it started on.
                EndAddr::Multiple(n) => Some(
                    self.line_num
                        .checked_div(*n)
                        .and_then(|q| q.checked_add(1))
                        .and_then(|q| q.checked_mul(*n))
                        .unwrap_or(usize::MAX),
                ),
                EndAddr::Addr(_) => None,
            };
            if let Some(r) = self.ranges.get_mut(pc) {
                r.active = true;
                r.end_line = end;
            }
            return Ok(true);
        }

        let close = match a2 {
            EndAddr::Addr(Addr::Last) => input.at_end(),
            EndAddr::Addr(Addr::Re(r)) => self.resolve(r.as_ref())?.find(&self.pattern)?.is_some(),
            EndAddr::Addr(Addr::Step(_, _)) => false,
            _ => {
                let end = self.ranges.get(pc).and_then(|r| r.end_line);
                end.is_some_and(|e| self.line_num >= e)
            }
        };
        if close && let Some(r) = self.ranges.get_mut(pc) {
            r.active = false;
            r.end_line = None;
        }
        Ok(true)
    }

    fn selected(&mut self, pc: usize, cmd: &Command, input: &mut Input) -> Run<bool> {
        let hit = match &cmd.sel {
            Sel::Always => true,
            Sel::One(a) => self.addr_match(a, input)?,
            Sel::Range(a1, a2) => self.range_select(pc, a1, a2, input)?,
        };
        Ok(hit != cmd.negated)
    }

    fn write_wfile(&mut self, idx: usize, bytes: &[u8], out: &mut Out<'_>) -> io::Result<()> {
        // The separator is written only if the line that produced this pattern
        // space had one. `printf 'a\nb' | sed -n 'w f'` leaves `f` ending in
        // `b`, not `b\n` — the missing final separator is a property of the
        // text, and every copy of it made anywhere has to keep it.
        let had_sep = self.had_sep;
        self.write_wfile_sep(idx, bytes, had_sep, out)
    }

    /// [`Exec::write_wfile`] for a caller that knows better than `had_sep`.
    ///
    /// `W` is that caller: when the pattern space holds an embedded separator,
    /// the first line ends *at* one, so it gets a separator whatever the input
    /// line ended with.
    fn write_wfile_sep(
        &mut self,
        idx: usize,
        bytes: &[u8],
        had_sep: bool,
        out: &mut Out<'_>,
    ) -> io::Result<()> {
        let sep = out.sep;
        let Some(w) = self.wfiles.get_mut(idx) else {
            return Ok(());
        };
        match &mut w.target {
            WTarget::Stdout => out.line(bytes, had_sep),
            WTarget::Stderr => {
                // Assembled and written once, because `write_all` here is one
                // `write(2)` per call and a line torn into two of them can
                // have another process's output land between the text and its
                // separator.
                //
                // The raw `write(2)` and not `io::stderr()`, whose `EBADF` the
                // runtime reports back as success: upstream gives status 4 for
                // `sed 'w /dev/stderr' f 2>&-` and for `2>/dev/full`, and it
                // can only be 4 here if the failure is visible to the caller.
                let mut line = Vec::with_capacity(bytes.len().saturating_add(2));
                if w.owed {
                    line.push(sep);
                }
                line.extend_from_slice(bytes);
                if had_sep {
                    line.push(sep);
                }
                w.owed = !had_sep;
                coreutils::stdfd::write_all(2, &line)
            }
            WTarget::File(f) => {
                let mut o = Out {
                    w: f,
                    sep,
                    owed: w.owed,
                };
                let r = o.line(bytes, had_sep);
                let owed = o.owed;
                w.owed = owed;
                r
            }
        }
    }

    fn flush_appends(&mut self, out: &mut Out<'_>) -> io::Result<()> {
        for p in std::mem::take(&mut self.appends) {
            match p {
                Pending::Raw(t) => out.raw(&t)?,
                Pending::File(path) => {
                    // GNU ignores a file it cannot read here: `r` is an
                    // inclusion, and a missing one is not an error in a script
                    // that may run before the file exists.
                    let read = if path == "/dev/stdin" {
                        let mut b = Vec::new();
                        io::stdin().read_to_end(&mut b).map(|_| b)
                    } else {
                        fs::read(&path)
                    };
                    if let Ok(bytes) = read
                        && !bytes.is_empty()
                    {
                        out.raw(&bytes)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Apply one `s` command; returns whether anything was replaced.
    fn substitute(&mut self, sub: &Subst) -> Run<bool> {
        let re = self.resolve(sub.re.as_ref())?;
        let hay = std::mem::take(&mut self.pattern);
        let mut out: Vec<u8> = Vec::with_capacity(hay.len());
        let mut last = 0usize;
        let mut seen = 0usize;
        let mut did = false;

        for caps in re.capture_spans_iter(&hay) {
            // The pattern space is restored before the error leaves, so a
            // caller that catches it still sees the line it was given rather
            // than the half-built replacement.
            let caps = match caps {
                Ok(c) => c,
                Err(e) => {
                    self.pattern = hay;
                    return Err(Stop::Limit(e));
                }
            };
            let Some((s, e)) = caps.first().copied().flatten() else {
                break;
            };
            seen = seen.saturating_add(1);
            let replace = if sub.global {
                seen >= sub.occurrence
            } else {
                seen == sub.occurrence
            };
            if !replace {
                continue;
            }
            out.extend_from_slice(hay.get(last..s).unwrap_or_default());
            render(&sub.repl, &hay, &caps, &mut out);
            last = e;
            did = true;
            if !sub.global {
                break;
            }
        }
        out.extend_from_slice(hay.get(last..).unwrap_or_default());
        self.pattern = out;
        Ok(did)
    }

    #[allow(clippy::too_many_lines)] // One dispatch over sed's command set; splitting it would hide the table.
    fn run(&mut self, cmds: &[Command], input: &mut Input, out: &mut Out<'_>) -> Run<Flow> {
        let mut pc = 0usize;
        while let Some(cmd) = cmds.get(pc) {
            if !self.selected(pc, cmd, input)? {
                pc = match cmd.act {
                    Action::Block(after) => after,
                    _ => pc.saturating_add(1),
                };
                continue;
            }
            match &cmd.act {
                Action::Block(_) | Action::BlockEnd | Action::Label => {}
                Action::Branch(t) => {
                    pc = *t;
                    continue;
                }
                Action::BranchIfSub(t) => {
                    if self.sub_made {
                        self.sub_made = false;
                        pc = *t;
                        continue;
                    }
                }
                Action::BranchIfNoSub(t) => {
                    if self.sub_made {
                        self.sub_made = false;
                    } else {
                        pc = *t;
                        continue;
                    }
                }
                Action::Subst(sub) => {
                    if self.substitute(sub)? {
                        self.sub_made = true;
                        if sub.print {
                            out.line(&self.pattern.clone(), true)?;
                        }
                        if let Some(w) = sub.wfile {
                            let bytes = self.pattern.clone();
                            self.write_wfile(w, &bytes, out)?;
                        }
                    }
                }
                Action::Transliterate(table) => {
                    for b in &mut self.pattern {
                        *b = table.get(usize::from(*b)).copied().unwrap_or(*b);
                    }
                }
                Action::Delete => return Ok(Flow::Deleted),
                Action::DeleteFirstLine => {
                    return Ok(match self.pattern.iter().position(|&b| b == out.sep) {
                        Some(i) => {
                            self.pattern.drain(..=i);
                            Flow::Restart
                        }
                        None => Flow::Deleted,
                    });
                }
                Action::Print => {
                    let bytes = self.pattern.clone();
                    out.line(&bytes, self.had_sep)?;
                }
                Action::PrintFirstLine => {
                    let (bytes, sep) = match self.pattern.iter().position(|&b| b == out.sep) {
                        Some(i) => (self.pattern.get(..i).unwrap_or_default().to_vec(), true),
                        None => (self.pattern.clone(), self.had_sep),
                    };
                    out.line(&bytes, sep)?;
                }
                Action::Next => {
                    if !self.suppress {
                        let bytes = self.pattern.clone();
                        out.line(&bytes, self.had_sep)?;
                    }
                    self.flush_appends(out)?;
                    match input.next_line() {
                        Some(l) => {
                            self.pattern = l.bytes;
                            self.had_sep = l.had_sep;
                            self.line_num = self.line_num.saturating_add(1);
                        }
                        // No more input: sed stops, and the pattern space has
                        // already been printed by this very command.
                        None => {
                            return Ok(Flow::Quit {
                                code: 0,
                                print: false,
                            });
                        }
                    }
                }
                Action::AppendNext => {
                    self.flush_appends(out)?;
                    match input.next_line() {
                        Some(l) => {
                            self.pattern.push(out.sep);
                            self.pattern.extend_from_slice(&l.bytes);
                            self.had_sep = l.had_sep;
                            self.line_num = self.line_num.saturating_add(1);
                        }
                        // GNU prints what it has rather than dropping it, which
                        // is what makes `sed '$!N;s/\n/ /'` join pairs of lines
                        // without losing an odd last one.
                        None => {
                            return Ok(Flow::Quit {
                                code: 0,
                                print: true,
                            });
                        }
                    }
                }
                Action::Hold => self.hold.clone_from(&self.pattern),
                Action::HoldAppend => {
                    self.hold.push(out.sep);
                    self.hold.extend_from_slice(&self.pattern);
                }
                Action::Get => self.pattern.clone_from(&self.hold),
                Action::GetAppend => {
                    self.pattern.push(out.sep);
                    self.pattern.extend_from_slice(&self.hold);
                }
                Action::Exchange => std::mem::swap(&mut self.pattern, &mut self.hold),
                Action::LineNumber => {
                    let n = self.line_num.to_string();
                    out.line(n.as_bytes(), true)?;
                }
                // Verbatim, newline and all — not `Pending::Text`, which would
                // end the line with the output separator. GNU's append queue
                // writes the text as it was stored, so `sed -z 'a X'` emits
                // `X\n` between NUL-terminated lines. Measured, and the reason
                // `i` and `c` below go the other way.
                Action::AppendText(t) => {
                    if !t.is_empty() {
                        self.appends.push(Pending::Raw(t.clone()));
                    }
                }
                Action::InsertText(t) => text_out(out, t)?,
                Action::ChangeText(t) => {
                    // For a range, the text replaces the whole range and so is
                    // written once, when the range closes — not once per line.
                    let still_open = matches!(cmd.sel, Sel::Range(_, _))
                        && self.ranges.get(pc).is_some_and(|r| r.active);
                    if !still_open {
                        text_out(out, t)?;
                    }
                    return Ok(Flow::Deleted);
                }
                Action::ReadFile(path) => self.appends.push(Pending::File(path.clone())),
                Action::ReadLine(idx) => {
                    // Read now, not at flush time. `R /dev/stdin` in a script
                    // whose input is a pipe interleaves with the cycle that
                    // asked for it, and deferring the read would reorder that.
                    let sep = out.sep;
                    if let Some(r) = self.rfiles.get_mut(*idx).and_then(|f| f.r.as_mut()) {
                        let mut buf = Vec::new();
                        // A read failure is as silent as a missing file, for
                        // the reason given on `RFile`. Exhaustion arrives here
                        // as `Ok(0)` and is likewise a no-op, which is what
                        // makes `R` on a short file simply stop contributing.
                        if r.read_until(sep, &mut buf).is_ok() && !buf.is_empty() {
                            self.appends.push(Pending::Raw(buf));
                        }
                    }
                }
                Action::WriteFile(idx) => {
                    let bytes = self.pattern.clone();
                    self.write_wfile(*idx, &bytes, out)?;
                }
                Action::WriteFirstLine(idx) => {
                    let (bytes, sep) = match self.pattern.iter().position(|&b| b == out.sep) {
                        Some(i) => (self.pattern.get(..i).unwrap_or_default().to_vec(), true),
                        None => (self.pattern.clone(), self.had_sep),
                    };
                    self.write_wfile_sep(*idx, &bytes, sep, out)?;
                }
                Action::List(width) => {
                    let bytes = list_escape(&self.pattern, *width, out.sep);
                    // Always with a separator: `l`'s output is a rendering, not
                    // a copy, so it does not inherit the input line's missing
                    // one. `printf ab | sed -n l` ends in `$` *and* a newline.
                    out.line(&bytes, true)?;
                }
                Action::Quit { code, print } => {
                    return Ok(Flow::Quit {
                        code: *code,
                        print: *print,
                    });
                }
            }
            pc = pc.saturating_add(1);
        }
        Ok(Flow::Normal)
    }

    /// Run the script over every line of `input`.
    ///
    /// Returns the status `q` asked for, if it did.
    fn cycle(
        &mut self,
        cmds: &[Command],
        input: &mut Input,
        out: &mut Out<'_>,
    ) -> Run<Option<i32>> {
        while let Some(line) = input.next_line() {
            self.pattern = line.bytes;
            self.had_sep = line.had_sep;
            self.line_num = self.line_num.saturating_add(1);
            self.sub_made = false;

            loop {
                match self.run(cmds, input, out)? {
                    Flow::Normal => {
                        if !self.suppress {
                            let bytes = std::mem::take(&mut self.pattern);
                            out.line(&bytes, self.had_sep)?;
                            self.pattern = bytes;
                        }
                        self.flush_appends(out)?;
                        break;
                    }
                    Flow::Deleted => {
                        self.flush_appends(out)?;
                        break;
                    }
                    Flow::Restart => {
                        self.flush_appends(out)?;
                    }
                    Flow::Quit { code, print } => {
                        if print && !self.suppress {
                            let bytes = std::mem::take(&mut self.pattern);
                            out.line(&bytes, self.had_sep)?;
                        }
                        self.flush_appends(out)?;
                        return Ok(Some(code));
                    }
                }
            }
        }
        Ok(None)
    }
}

/// Render the pattern space the way `l` shows it: every byte unambiguous, and
/// a `$` marking where the text ends.
///
/// The escaping is by *byte*, not by character, even in a UTF-8 locale —
/// `printf 'café\n' | sed -n l` prints `caf\303\251$` under GNU sed too. That
/// is the point of the command: it is asked for when a line looks right and
/// behaves wrong, which is exactly when the encoding is what one needs to see.
///
/// `width` counts output columns and 0 disables wrapping. Two details of the
/// wrap are upstream's and are load-bearing:
///
/// * The `+ 1` reserves the column the continuation `\` will occupy, so a line
///   that would exactly fill the width breaks one escape early rather than
///   letting the backslash spill past it. This is why `-l 1` opens with a bare
///   `\` and a break — no escape can ever fit in `1 - 1` columns.
/// * The test is made once per *escape*, not once per byte, so `\303` is never
///   torn in half by a break. A reader who had to reassemble an octal escape
///   from two lines would be worse off than with no wrapping at all.
///
/// `sep` is the output separator, which the wrap and the final `$` both use:
/// under `-z` the breaks are NUL-terminated like everything else.
fn list_escape(bytes: &[u8], width: usize, sep: u8) -> Vec<u8> {
    const OCTAL: &[u8; 8] = b"01234567";
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len().saturating_add(2));
    let mut col = 0usize;
    let mut esc: Vec<u8> = Vec::with_capacity(4);
    for &b in bytes {
        esc.clear();
        match b {
            b'\\' => esc.extend_from_slice(b"\\\\"),
            0x07 => esc.extend_from_slice(b"\\a"),
            0x08 => esc.extend_from_slice(b"\\b"),
            b'\t' => esc.extend_from_slice(b"\\t"),
            b'\n' => esc.extend_from_slice(b"\\n"),
            0x0b => esc.extend_from_slice(b"\\v"),
            0x0c => esc.extend_from_slice(b"\\f"),
            b'\r' => esc.extend_from_slice(b"\\r"),
            // Printable ASCII stands for itself. Space is included and DEL is
            // not, which is `isprint` in the C locale.
            0x20..=0x7e => esc.push(b),
            _ => {
                let v = usize::from(b);
                esc.push(b'\\');
                for shift in [6usize, 3, 0] {
                    let d = v
                        .checked_shr(u32::try_from(shift).unwrap_or(0))
                        .unwrap_or(0)
                        & 7;
                    esc.push(OCTAL.get(d).copied().unwrap_or(b'0'));
                }
            }
        }
        if width > 0 && col.saturating_add(esc.len()).saturating_add(1) > width {
            out.push(b'\\');
            out.push(sep);
            col = 0;
        }
        out.extend_from_slice(&esc);
        col = col.saturating_add(esc.len());
    }
    out.push(b'$');
    out
}

/// Expand a replacement against one match.
fn render(parts: &[Rep], hay: &[u8], caps: &[Option<(usize, usize)>], out: &mut Vec<u8>) {
    let mut rest: Option<bool> = None;
    let mut one: Option<bool> = None;
    let push = |bytes: &[u8], out: &mut Vec<u8>, one: &mut Option<bool>, rest: &Option<bool>| {
        for &b in bytes {
            let b = if let Some(up) = one.take() {
                if up {
                    b.to_ascii_uppercase()
                } else {
                    b.to_ascii_lowercase()
                }
            } else {
                match rest {
                    Some(true) => b.to_ascii_uppercase(),
                    Some(false) => b.to_ascii_lowercase(),
                    None => b,
                }
            };
            out.push(b);
        }
    };

    for part in parts {
        match part {
            Rep::Lit(b) => push(b, out, &mut one, &rest),
            Rep::Group(n) => {
                // A group that did not participate contributes nothing — as
                // distinct from an error, which is what a *nonexistent* group
                // would be, and which the compiler cannot see here.
                if let Some(Some((s, e))) = caps.get(*n) {
                    let text = hay.get(*s..*e).unwrap_or_default().to_vec();
                    push(&text, out, &mut one, &rest);
                }
            }
            Rep::Case(op) => match op {
                CaseOp::OneUpper => one = Some(true),
                CaseOp::OneLower => one = Some(false),
                CaseOp::RestUpper => {
                    rest = Some(true);
                    one = None;
                }
                CaseOp::RestLower => {
                    rest = Some(false);
                    one = None;
                }
                CaseOp::End => {
                    rest = None;
                    one = None;
                }
            },
        }
    }
}

// ------------------------------------------------------------------- the CLI

/// The program name and the status a usage error leaves with, for
/// [`coreutils::getopt`]. Measured: `sed --zzz-bogus; echo $?` answers 1.
const SED: Program = Program::new("sed", 1);

/// The `getopt_long` short-option string, exactly as upstream declares it.
///
/// `V` is there because upstream declares it, not because it does anything: it
/// takes a required argument and then always fails. See [`Request::BadUsage`].
const SHORT_OPTIONS: &str = "bEe:f:i::l:nrsuzV:";

/// Every long option `sed` knows, with what it takes.
///
/// **The order is GNU's declaration order, not alphabetical**, because
/// `getopt_long` lists an ambiguous prefix's candidates in table order, and an
/// empty prefix matches every option — which is the one command that shows the
/// whole table, measured rather than recalled:
///
/// ```text
/// $ sed --=x
/// sed: option '--=x' is ambiguous; possibilities: '--binary' '--regexp-extended'
///      '--debug' '--expression' '--file' '--in-place' '--line-length'
///      '--null-data' '--zero-terminated' '--quiet' '--posix' '--silent'
///      '--sandbox' '--separate' '--unbuffered' '--version' '--help'
///      '--follow-symlinks'
/// ```
///
/// That order is what makes `sed --s` ambiguous between `--silent`, `--sandbox`
/// and `--separate` *in that sequence*, which is the string GNU prints.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("binary", Takes::Nothing),
    ("regexp-extended", Takes::Nothing),
    ("debug", Takes::Nothing),
    ("expression", Takes::Required),
    ("file", Takes::Required),
    ("in-place", Takes::Optional),
    ("line-length", Takes::Required),
    ("null-data", Takes::Nothing),
    ("zero-terminated", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("posix", Takes::Nothing),
    ("silent", Takes::Nothing),
    ("sandbox", Takes::Nothing),
    ("separate", Takes::Nothing),
    ("unbuffered", Takes::Nothing),
    ("version", Takes::Nothing),
    // `--help` and `--version` are ordinary table entries rather than names
    // special-cased ahead of it, because getopt sees them too: they appear
    // among an ambiguous prefix's possibilities, and `sed --help=x` is measured
    // to be `option '--help' doesn't allow an argument`, not a printed usage.
    ("help", Takes::Nothing),
    ("follow-symlinks", Takes::Nothing),
];

/// The width `l` wraps at when `-l` says nothing. Upstream's, and not a round
/// number by accident: 70 leaves a `$` and a `\` inside a 72-column terminal.
const DEFAULT_LINE_LEN: usize = 70;

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct SedArgs {
    suppress: bool,
    ere: bool,
    separate: bool,
    null_data: bool,
    sandbox: bool,
    /// `-l N`, defaulting to [`DEFAULT_LINE_LEN`]; 0 means never wrap.
    line_len: usize,
    /// `Some(suffix)` for `-i`; an empty suffix means no backup.
    in_place: Option<OsString>,
    /// The `-e` fragments and `-f` files, in the order they were given.
    script_parts: Vec<ScriptPart>,
    files: Vec<OsString>,
}

impl Default for SedArgs {
    fn default() -> SedArgs {
        SedArgs {
            suppress: false,
            ere: false,
            separate: false,
            null_data: false,
            sandbox: false,
            line_len: DEFAULT_LINE_LEN,
            in_place: None,
            script_parts: Vec::new(),
            files: Vec::new(),
        }
    }
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum ScriptPart {
    /// A `-e` fragment, or the first operand when there is no `-e`/`-f`. Bytes,
    /// not text: a script may hold any byte, and a `y/…/…/` over binary data is
    /// a real use.
    Text(Vec<u8>),
    /// A `-f` file, named as argv named it.
    File(OsString),
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Run(SedArgs),
    /// `--help`: the usage block on standard output, status 0.
    Help,
    /// `--version`.
    Version,
    /// The usage block on standard error with no sentence above it, status 1.
    ///
    /// Two ways in, both measured. `-V` — POSIX's obsolete "version of the
    /// language" request — takes a required argument and then rejects whatever
    /// it was given. And a command line with no script at all is refused the
    /// same way, without a sentence, because upstream has nothing to say beyond
    /// the usage it prints.
    BadUsage,
}

/// Parse sed's argv.
///
/// Option resolution is [`coreutils::getopt`]'s, so long options abbreviate to
/// any unambiguous prefix the way every GNU utility's do — `sed --expr=p` and
/// `sed --sil` work, and `sed --s` is refused as ambiguous.
///
/// The script is chosen **after** the whole command line has been read, not
/// while reading it. Whether the first operand is the script depends on whether
/// a `-e` or `-f` appears *anywhere*, including after that operand: measured,
/// `sed foo -e p` edits the file `foo`, where deciding as the operand arrives
/// would have made `foo` the script.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut out = SedArgs::default();
    let mut operands: Vec<&OsString> = Vec::new();
    // A `-e` or `-f` anywhere means every operand is a file.
    let mut have_script = false;

    for item in SED.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Operand(word) => operands.push(word),
            Opt::Short(b'n', _) | Opt::Long("quiet" | "silent", _) => out.suppress = true,
            Opt::Short(b'E' | b'r', _) | Opt::Long("regexp-extended", _) => out.ere = true,
            Opt::Short(b's', _) | Opt::Long("separate", _) => out.separate = true,
            Opt::Short(b'z', _) | Opt::Long("null-data" | "zero-terminated", _) => {
                out.null_data = true;
            }
            Opt::Long("sandbox", _) => out.sandbox = true,
            // Accepted and ignored, each for its own reason. `-u` asks for less
            // buffering, and this sed already flushes at every boundary that
            // matters. `-b` asks for binary mode, which is the only mode there
            // is here: nothing translates CR+LF. `--follow-symlinks` and
            // `--posix` are answered in tranche 2c; `--debug` is a known gap,
            // see `known-issues.md`.
            Opt::Short(b'u', _)
            | Opt::Long("unbuffered" | "binary" | "posix" | "debug" | "follow-symlinks", _) => {}
            Opt::Short(b'i', value) | Opt::Long("in-place", value) => {
                // `-i` takes an *optional* value, so it is never the next word:
                // GNU reads `sed -i backup f` as an in-place edit of `backup`
                // and `f`, not as a `.backup` suffix.
                out.in_place = Some(value.unwrap_or_default());
                out.separate = true;
            }
            Opt::Short(b'e', value) | Opt::Long("expression", value) => {
                out.script_parts
                    .push(ScriptPart::Text(value.map(as_bytes).unwrap_or_default()));
                have_script = true;
            }
            Opt::Short(b'f', value) | Opt::Long("file", value) => {
                out.script_parts
                    .push(ScriptPart::File(value.unwrap_or_default()));
                have_script = true;
            }
            // Upstream reads this with `atoi`, which has no way to report a
            // failure: `sed -l x` is measured to be accepted and to mean 0
            // (never wrap), and `sed -l 3x` to mean 3. That is why this parses
            // a leading run of digits and stops, rather than validating.
            Opt::Short(b'l', value) | Opt::Long("line-length", value) => {
                out.line_len = value.as_deref().map_or(0, atoi);
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // See [`Request::BadUsage`].
            Opt::Short(b'V', _) => return Ok(Request::BadUsage),
            // Every letter of `SHORT_OPTIONS` and every name of
            // `LONG_OPTIONS` is above, and getopt yields nothing else.
            Opt::Short(_, _) | Opt::Long(_, _) => {}
        }
    }

    let mut operands = operands.into_iter();
    if !have_script {
        let Some(first) = operands.next() else {
            return Ok(Request::BadUsage);
        };
        out.script_parts
            .push(ScriptPart::Text(os_bytes(first).into_owned()));
    }
    out.files = operands.cloned().collect();
    Ok(Request::Run(out))
}

/// An argv word as the bytes it was given.
fn as_bytes(word: OsString) -> Vec<u8> {
    os_bytes(&word).into_owned()
}

/// `atoi(3)` over an argv word: leading digits, and 0 for anything else.
///
/// Deliberately not `str::parse`. Upstream reads `-l` with `atoi`, so `-l 3x`
/// means 3 and `-l x` means 0, and neither is an error. Saturating rather than
/// wrapping on a very long run of digits, which upstream leaves undefined and
/// where a wrap would silently turn "never wrap" into some narrow width.
fn atoi(word: &OsStr) -> usize {
    os_bytes(word)
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .fold(0usize, |acc, b| {
            acc.saturating_mul(10)
                .saturating_add(usize::from(b.wrapping_sub(b'0')))
        })
}

/// What `--help` prints, and what every usage error prints beneath its sentence.
///
/// Shorter than GNU's, which ends in three URLs that would be wrong here — but
/// **complete**: every option this sed accepts has a line, because a usage
/// message that omits half the options is worse than none, and the first line
/// is GNU's word for word so that a caller matching on it still matches.
const USAGE: &str = "\
Usage: sed [OPTION]... {script-only-if-no-other-script} [input-file]...

  -n, --quiet, --silent    suppress automatic printing of pattern space
      --debug              annotate program execution
  -e SCRIPT, --expression=SCRIPT
                           add SCRIPT to the commands to be executed
  -f FILE, --file=FILE     add the contents of FILE to the commands
      --follow-symlinks    follow symlinks when processing in place
  -i[SUFFIX], --in-place[=SUFFIX]
                           edit files in place (makes a backup if SUFFIX given)
  -b, --binary             accepted and ignored: nothing is translated here
  -l N, --line-length=N    the line-wrap length for the `l' command
      --posix              disable all GNU extensions
  -E, -r, --regexp-extended
                           use extended regular expressions in the script
  -s, --separate           consider the files separate rather than one stream
      --sandbox            operate in sandbox mode (disable e/r/w commands)
  -u, --unbuffered         accepted and ignored: this sed does not batch files
  -z, --null-data          separate lines by NUL characters
      --help               display this help and exit
      --version            output version information and exit

If no -e, --expression, -f, or --file option is given, then the first
non-option argument is taken as the sed script to interpret.  All
remaining arguments are names of input files; if no input files are
specified, then the standard input is read.";

/// Where one stretch of the joined script text came from.
///
/// The whole script is parsed as one buffer, because a `{` may be opened in one
/// fragment and closed in the next — GNU allows that too (`sed -e '{' -e p -e
/// '}'` runs), so it is not an accident of our implementation. But GNU reports
/// an error against the *fragment* it fell in, not against the join, and the
/// two kinds of fragment are reported quite differently. This is the map back.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Origin {
    /// The Nth `-e`, counting from 1. A bare script operand is counted too — it
    /// is `-e expression #1` to GNU as much as `-e` is. `-f` files are *not*
    /// counted: `sed -f x.sed -e Z` reports `expression #1`, not `#2`.
    Expr(usize),
    /// A `-f` file, named as its bytes, since a filename need not be text.
    /// Errors in one are located by line number, with no character offset.
    File(Vec<u8>),
}

/// One fragment's starting offset in the joined script text, and its origin.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    start: usize,
    origin: Origin,
}

/// Gather the script text from `-e` fragments and `-f` files.
///
/// Returns the joined text and the map from offset back to fragment; see
/// [`Segment`]. The error is already formatted for [`panic_out`], and carries
/// the name as bytes so a `-f` file whose name is not UTF-8 is reported as
/// itself.
fn collect_script(parts: &[ScriptPart]) -> Result<(Vec<u8>, Vec<Segment>), Vec<u8>> {
    let mut script: Vec<u8> = Vec::new();
    let mut segments: Vec<Segment> = Vec::new();
    let mut exprs = 0usize;
    for part in parts {
        if !script.is_empty() {
            script.push(b'\n');
        }
        segments.push(Segment {
            start: script.len(),
            origin: match part {
                ScriptPart::Text(_) => {
                    exprs = exprs.saturating_add(1);
                    Origin::Expr(exprs)
                }
                ScriptPart::File(path) => Origin::File(os_bytes(path).into_owned()),
            },
        });
        match part {
            ScriptPart::Text(t) => script.extend_from_slice(t),
            ScriptPart::File(path) => {
                let bytes = if path == "-" {
                    let mut b = Vec::new();
                    io::stdin()
                        .read_to_end(&mut b)
                        .map(|_| b)
                        .map_err(|e| format!("couldn't read -: {}", strerror(&e)).into_bytes())?
                } else {
                    fs::read(path).map_err(|e| {
                        let mut msg = Vec::from(&b"couldn't open file "[..]);
                        msg.extend_from_slice(&os_bytes(path));
                        msg.extend_from_slice(format!(": {}", strerror(&e)).as_bytes());
                        msg
                    })?
                };
                script.extend_from_slice(&bytes);
            }
        }
    }
    Ok((script, segments))
}

/// GNU's location prefix for a script error, without the trailing `: `.
///
/// Two shapes, because upstream has two: a `-e` fragment is located by
/// expression number and character offset within that fragment, a `-f` file by
/// name and line number within that file. Neither is the offset into the joined
/// buffer we actually parse, which is why [`Segment`] exists.
fn locate(script: &[u8], segments: &[Segment], pos: Pos) -> Option<Vec<u8>> {
    let (at, brace) = match pos {
        Pos::At(at) => (at, false),
        Pos::AfterParse(at) => (at, true),
        Pos::Nowhere => return None,
    };
    // The last segment that starts at or before `at`. A fault at the very end of
    // the script belongs to the last fragment, which this gives for free.
    let seg = segments.iter().rev().find(|s| s.start <= at)?;
    let mut out = Vec::new();
    match &seg.origin {
        Origin::Expr(n) => {
            // `char 0` for an unclosed brace whatever its offset — see
            // [`Pos::AfterParse`].
            let ch = if brace {
                0
            } else {
                at.saturating_sub(seg.start)
            };
            out.extend_from_slice(format!("-e expression #{n}, char {ch}").as_bytes());
        }
        Origin::File(name) => {
            let counted = script.get(seg.start..at).unwrap_or_default();
            let line = counted
                .iter()
                .filter(|&&b| b == b'\n')
                .count()
                .saturating_add(1);
            out.extend_from_slice(b"file ");
            out.extend_from_slice(name);
            out.extend_from_slice(format!(" line {line}").as_bytes());
        }
    }
    Some(out)
}

fn main() {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(Request::Run(p)) => p,
        Ok(Request::Help) => {
            println!("{USAGE}");
            let _ = io::stdout().flush();
            process::exit(0);
        }
        Ok(Request::Version) => {
            println!("sed (SlateOS coreutils)");
            let _ = io::stdout().flush();
            process::exit(0);
        }
        Ok(Request::BadUsage) => {
            diag!("{USAGE}");
            process::exit(1);
        }
        Err(e) => {
            // `e.sentence`, not `e.message()`: GNU sed answers a usage error
            // with the whole usage block rather than with gnulib's
            // `Try 'sed --help' for more information.` referral.
            diag!("sed: {}", e.sentence);
            diag!("{USAGE}");
            process::exit(e.status);
        }
    };

    let (script_text, segments) = match collect_script(&parsed.script_parts) {
        Ok(s) => s,
        // Not status 1: the command line was well formed, and a `-f` file that
        // will not open is an I/O failure like any other.
        Err(msg) => {
            let _ = io::stdout().flush();
            let mut line = Vec::from(&b"sed: "[..]);
            line.extend_from_slice(&msg);
            line.push(b'\n');
            stdfd::diag_bytes(&line);
            process::exit(EXIT_PANIC);
        }
    };

    let script = match compile_script(
        &script_text,
        &segments,
        parsed.ere,
        parsed.line_len,
        parsed.sandbox,
    ) {
        Ok(s) => s,
        Err(e) => {
            let mut line = Vec::from(&b"sed: "[..]);
            if let Some(where_) = locate(&script_text, &segments, e.pos) {
                line.extend_from_slice(&where_);
                line.extend_from_slice(b": ");
            }
            line.extend_from_slice(e.msg.as_bytes());
            line.push(b'\n');
            let _ = io::stdout().flush();
            stdfd::diag_bytes(&line);
            process::exit(e.code);
        }
    };

    // `-i` rewrites the files it is given, so with none it has nothing to
    // mean; upstream refuses rather than quietly editing standard input.
    if parsed.in_place.is_some() && parsed.files.is_empty() {
        panic_out("no input files");
    }

    let mut files = parsed.files.clone();
    if files.is_empty() {
        files.push(OsString::from("-"));
    }

    let mut job = Job {
        script: &script,
        // Opened before a line is read, and once for the whole run.
        wfiles: open_wfiles(&script.wfiles),
        rfiles: open_rfiles(&script.rfiles),
        suppress: parsed.suppress || script.suppress,
        sep: if parsed.null_data { 0 } else { b'\n' },
    };

    let status = if let Some(suffix) = parsed.in_place.as_ref() {
        job.in_place(&files, suffix)
    } else if parsed.separate {
        job.separate(&files)
    } else {
        job.joined(&files)
    };
    process::exit(status);
}

/// Everything a run needs that does not change from one input file to the next.
///
/// The `w` targets live here rather than in [`Exec`] because an `Exec` is built
/// per input file under `-s` and `-i`, and a `w` target must not be reopened —
/// and so truncated — when the next file starts.
struct Job<'a> {
    script: &'a Script,
    wfiles: Vec<WFile>,
    rfiles: Vec<RFile>,
    suppress: bool,
    sep: u8,
}

impl Job<'_> {
    /// Wire one `Input` to one `Out` and run the script over it.
    fn run_one(&mut self, input: &mut Input, sink: &mut dyn Write) -> (Option<i32>, bool) {
        let mut exec = Exec::new(
            self.script,
            self.suppress,
            &mut self.wfiles,
            &mut self.rfiles,
        );
        let mut out = Out {
            w: sink,
            sep: self.sep,
            owed: false,
        };
        let quit = match exec.cycle(&self.script.cmds, input, &mut out) {
            Ok(q) => q,
            Err(Stop::Io(e)) => {
                // A closed pipe is how `sed … | head` ends, not a failure.
                if e.kind() != io::ErrorKind::BrokenPipe {
                    diag!("sed: couldn't write: {}", strerror(&e));
                    return (Some(EXIT_PANIC), true);
                }
                None
            }
            // Not "couldn't write": the run stopped because a search was
            // abandoned, and sending the reader to the disk would waste their
            // time.
            Err(Stop::Limit(e)) => {
                diag!("sed: {e}");
                return (Some(EXIT_PANIC), true);
            }
            // Upstream leaves at once, and leaves *after* what it has already
            // written: a `w` file given a line on an earlier cycle keeps it, and
            // the pattern spaces already printed are on standard output. Hence
            // the flush before the diagnostic and the exit rather than a return
            // — under `-i` there is a half-written temporary file, and GNU does
            // not finish it either. Measured.
            Err(Stop::NoRegex) => {
                let _ = sink.flush();
                let mut line = Vec::from(&b"sed: "[..]);
                if let Some(at) = self.script.end_loc.as_deref() {
                    line.extend_from_slice(at);
                    line.extend_from_slice(b": ");
                }
                line.extend_from_slice(b"no previous regular expression\n");
                let _ = io::stdout().flush();
                stdfd::diag_bytes(&line);
                process::exit(1);
            }
        };
        // The cycle either finished or took one of the branches above, each of
        // which returns; there is no third way for it to have gone wrong.
        (quit, false)
    }

    /// All the files as one stream: line numbers and `$` run across them.
    fn joined(&mut self, files: &[OsString]) -> i32 {
        let stdout = io::stdout();
        let mut sink = stdout.lock();
        let mut input = Input::new(files.to_vec(), self.sep);
        let (quit, exec_err) = self.run_one(&mut input, &mut sink);
        let _ = sink.flush();
        status(quit, input.had_error || exec_err)
    }

    /// `-s`: each file starts again at line 1, and each has its own last line.
    fn separate(&mut self, files: &[OsString]) -> i32 {
        let stdout = io::stdout();
        let mut sink = stdout.lock();
        let mut bad = false;
        for path in files {
            // Upstream rewinds every `R` source when a new input file starts
            // under `-s`, so `sed -s 'R inc' a b` pairs `inc`'s first lines
            // with both files rather than running off the end of `inc` during
            // the first. See [`rewind_rfiles`].
            rewind_rfiles(&mut self.rfiles);
            let mut input = Input::new(vec![path.clone()], self.sep);
            let (quit, exec_err) = self.run_one(&mut input, &mut sink);
            bad = bad || input.had_error || exec_err;
            if let Some(code) = quit {
                let _ = sink.flush();
                return status(Some(code), bad);
            }
        }
        let _ = sink.flush();
        status(None, bad)
    }

    /// `-i`: the output of each file replaces it.
    ///
    /// The result is built in memory and written once, so a script that fails
    /// part-way through does not leave the file half-edited.
    fn in_place(&mut self, files: &[OsString], suffix: &OsStr) -> i32 {
        let mut bad = false;
        for path in files {
            // A `-` operand is a file named `-`, not standard input: there is
            // no way to rewrite a stream in place, so it gets no special case
            // and fails as any missing file would.
            match File::open(path) {
                Err(e) => {
                    diag_path("sed: can't read ", path, &format!(": {}", strerror(&e)));
                    bad = true;
                    continue;
                }
                // A directory opens happily and then fails to read, which
                // would end the run with a confusing "read error". Refusing
                // anything that is not a regular file names the real problem.
                Ok(f) => match f.metadata() {
                    Ok(m) if !m.is_file() => {
                        panic_path("sed: couldn't edit ", path, ": not a regular file");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        diag_path("sed: can't read ", path, &format!(": {}", strerror(&e)));
                        bad = true;
                        continue;
                    }
                },
            }
            // `-i` implies `-s`, including for the `R` sources. See `separate`.
            rewind_rfiles(&mut self.rfiles);
            let mut buf: Vec<u8> = Vec::new();
            let mut input = Input::new(vec![path.clone()], self.sep);
            let (quit, exec_err) = self.run_one(&mut input, &mut buf);
            bad = bad || input.had_error || exec_err;

            if let Some(backup) = backup_name(path, suffix)
                && let Err(e) = fs::copy(path, &backup)
            {
                diag_path("sed: cannot back up ", path, &format!(": {}", strerror(&e)));
                bad = true;
                continue;
            }
            if let Err(e) = fs::write(path, &buf) {
                diag_path("sed: couldn't write ", path, &format!(": {}", strerror(&e)));
                bad = true;
            }
            if let Some(code) = quit {
                return status(Some(code), bad);
            }
        }
        status(None, bad)
    }
}

/// Where `-i`'s backup of `path` goes, or `None` for no backup at all.
///
/// Upstream builds this in two steps, and both are visible in the result.
/// `-i SUFFIX` first becomes the *pattern* `*SUFFIX` unless the suffix already
/// contains a `*`, and `-i` with no suffix becomes the bare pattern `*`. The
/// pattern is then expanded by putting the whole file name — directories
/// included — wherever a `*` stands, so `-i.bak` gives `f.bak`, `-i 'old/*'`
/// gives `old/f`, and `-i '*.*'` gives `f.f`.
///
/// The bare `*` is how upstream spells "no backup": it is the one pattern that
/// expands to the file itself, and copying a file over itself would empty it.
/// Comparing the expansion rather than the suffix catches every spelling of it,
/// including a `-i` whose suffix is literally `*`.
fn backup_name(path: &OsStr, suffix: &OsStr) -> Option<OsString> {
    let name = os_bytes(path);
    let suffix = os_bytes(suffix);
    let pattern: Vec<u8> = if suffix.contains(&b'*') {
        suffix.into_owned()
    } else {
        let mut p = vec![b'*'];
        p.extend_from_slice(&suffix);
        p
    };
    let mut backup: Vec<u8> = Vec::with_capacity(pattern.len().saturating_add(name.len()));
    for &b in &pattern {
        if b == b'*' {
            backup.extend_from_slice(&name);
        } else {
            backup.push(b);
        }
    }
    if backup == *name {
        return None;
    }
    Some(os_from_bytes(&backup))
}

fn status(quit: Option<i32>, bad: bool) -> i32 {
    match quit {
        Some(code) if code != 0 => code,
        // An unreadable input file is status 2, as GNU has it — distinct from
        // status 1, which means the script itself was wrong.
        _ if bad => 2,
        Some(code) => code,
        None => 0,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::expect_used
)]
mod tests {
    use super::*;

    fn os(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// [`compile_script`] with what `main` passes for a bare `sed 'script'`:
    /// the default `l` width, and no sandbox. The two tests that care about
    /// either of those call `compile_script` directly.
    fn compile(script: &[u8], ere: bool) -> Result<Script, ScriptError> {
        compile_script(script, &[], ere, DEFAULT_LINE_LEN, false)
    }

    /// Why [`compile`] refused `script`. Panics if it did not — `Script` has no
    /// `Debug`, so `unwrap_err` is not available and would be less informative
    /// than naming the script anyway.
    fn compile_err(script: &[u8]) -> String {
        match compile(script, false) {
            Ok(_) => panic!("{} compiled", String::from_utf8_lossy(script)),
            Err(e) => e.msg,
        }
    }

    /// Run a script over some text and return what it wrote.
    fn run(script: &str, input: &str) -> String {
        run_opts(script, input, false, false)
    }

    fn run_opts(script: &str, input: &str, suppress: bool, ere: bool) -> String {
        let out = run_sep(script.as_bytes(), input.as_bytes(), suppress, ere, b'\n');
        String::from_utf8_lossy(&out).into_owned()
    }

    /// [`run_opts`] over bytes, with the separator `-z` would have chosen.
    ///
    /// Separate because every caller but the `-z` tests wants text, and a
    /// separator of NUL makes the input and the output both unprintable.
    fn run_sep(script: &[u8], input: &[u8], suppress: bool, ere: bool, sep: u8) -> Vec<u8> {
        let compiled = compile(script, ere)
            .unwrap_or_else(|e| panic!("compiling {}: {}", String::from_utf8_lossy(script), e.msg));
        let mut sink: Vec<u8> = Vec::new();
        let mut inp = Input {
            paths: Vec::new(),
            next_path: 0,
            cur_name: OsString::from("stdin"),
            cur: Some(Box::new(BufReader::new(io::Cursor::new(input.to_vec())))),
            peeked: None,
            sep,
            had_error: false,
        };
        let mut job = Job {
            script: &compiled,
            wfiles: open_wfiles(&compiled.wfiles),
            rfiles: open_rfiles(&compiled.rfiles),
            suppress: suppress || compiled.suppress,
            sep,
        };
        job.run_one(&mut inp, &mut sink);
        sink
    }

    /// [`run`] for a script that may hit the empty-regex fault.
    ///
    /// [`Job::run_one`] answers that fault the way upstream does — a diagnostic
    /// and `exit(1)` — which a test cannot survive, so this drives the [`Exec`]
    /// directly instead. Either way it returns what had been written when the
    /// run ended, since "what stands before the failure" is half of what is
    /// being checked.
    fn run_stopping(script: &str, input: &str) -> Result<Vec<u8>, Vec<u8>> {
        let compiled = compile(script.as_bytes(), false)
            .unwrap_or_else(|e| panic!("compiling {script}: {}", e.msg));
        let mut inp = Input {
            paths: Vec::new(),
            next_path: 0,
            cur_name: OsString::from("stdin"),
            cur: Some(Box::new(BufReader::new(io::Cursor::new(
                input.as_bytes().to_vec(),
            )))),
            peeked: None,
            sep: b'\n',
            had_error: false,
        };
        let mut wfiles = open_wfiles(&compiled.wfiles);
        let mut rfiles = open_rfiles(&compiled.rfiles);
        let mut sink: Vec<u8> = Vec::new();
        // Scoped so the borrow of `sink` ends before it is returned.
        let stopped = {
            let mut exec = Exec::new(&compiled, compiled.suppress, &mut wfiles, &mut rfiles);
            let mut out = Out {
                w: &mut sink,
                sep: b'\n',
                owed: false,
            };
            match exec.cycle(&compiled.cmds, &mut inp, &mut out) {
                Ok(_) => false,
                Err(Stop::NoRegex) => true,
                Err(_) => panic!("{script} failed for some other reason"),
            }
        };
        if stopped { Err(sink) } else { Ok(sink) }
    }

    // ---------------- the defect that started this ----------------

    #[test]
    fn an_anchored_substitution_actually_substitutes() {
        // The shell's `builtin_diagnostics_honor_stderr_redirect` test ran
        // `sed 's/^/E:/'` and got its input back unchanged, because `^` was
        // matched with `str::contains`. This is that test, in one line.
        assert_eq!(run("s/^/E:/", "oops\n"), "E:oops\n");
        assert_eq!(run("s/$/!/", "hi\n"), "hi!\n");
    }

    #[test]
    fn a_bracket_expression_is_a_set_not_a_string() {
        assert_eq!(run("s/[abc]/X/g", "cabbage\n"), "XXXXXge\n");
        assert_eq!(run("s/[^abc]/./g", "cabbage\n"), "cabba..\n");
    }

    #[test]
    fn groups_and_backreferences_work() {
        assert_eq!(run(r"s/\(a*\)\(b*\)/[\2\1]/", "aaabb\n"), "[bbaaa]\n");
        assert_eq!(
            run_opts("s/(a+)(b+)/[\\2\\1]/", "aaabb\n", false, true),
            "[bbaaa]\n"
        );
    }

    #[test]
    fn a_backreference_works_in_the_pattern_and_not_only_the_replacement() {
        // `\1` on the left of the `/` is a different feature from `\1` on the
        // right: it asks the *matcher* to compare against what the group took.
        assert_eq!(run(r"s/\(ab\)\1/X/", "abab\n"), "X\n");
        assert_eq!(run(r"s/\(ab\)\1/X/", "abcd\n"), "abcd\n");
        assert_eq!(run(r"/\(.\)\1/d", "aa\nab\n"), "ab\n");

        // The classic "squeeze repeated lines" one-liner, which is the reason
        // this was implemented: it holds two lines and deletes the first when
        // the second repeats it. GNU sed prints `x` then `y`.
        assert_eq!(run(r"$!N;/^\(.*\)\n\1$/!P;D", "x\nx\ny\n"), "x\ny\n");
    }

    #[test]
    fn a_pathological_backreference_stops_rather_than_hanging() {
        // The matcher spends a budget and then declines to answer. `sed` must
        // not read that as "did not match": `/re/!d` deletes every line the
        // pattern does *not* match, so a wrong answer here destroys data.
        let pat = r"/\(a*\)\(a*\)\(a*\)\(a*\)\(a*\)\1\2\3\4\5b/!d";
        let compiled = compile(pat.as_bytes(), false).unwrap();
        let line = format!("{}\n", "a".repeat(300));
        let mut sink: Vec<u8> = Vec::new();
        let mut inp = Input {
            paths: Vec::new(),
            next_path: 0,
            cur_name: OsString::from("stdin"),
            cur: Some(Box::new(BufReader::new(io::Cursor::new(line.into_bytes())))),
            peeked: None,
            sep: b'\n',
            had_error: false,
        };
        let mut job = Job {
            script: &compiled,
            wfiles: Vec::new(),
            rfiles: Vec::new(),
            suppress: false,
            sep: b'\n',
        };
        let (status, _) = job.run_one(&mut inp, &mut sink);
        assert_eq!(status, Some(4), "a declined search must fail the run");
        assert!(
            sink.is_empty(),
            "nothing may be printed on a declined search"
        );
    }

    #[test]
    fn a_basic_regular_expression_is_not_an_extended_one() {
        // `a+b` is three literal characters in a BRE and a repetition in an
        // ERE. A sed that had one parser for both would have to be wrong here.
        assert_eq!(run("s/a+b/X/", "a+b\n"), "X\n");
        assert_eq!(run_opts("s/a+b/X/", "aab\n", false, true), "X\n");
    }

    // ---------------- addresses ----------------

    #[test]
    fn a_range_covers_the_lines_between_its_ends() {
        // The old sed selected the two *endpoints* of `1,3d` and nothing in
        // between, because it kept no state across lines.
        assert_eq!(run("2,4d", "1\n2\n3\n4\n5\n"), "1\n5\n");
        assert_eq!(
            run_opts("/b/,/d/p", "a\nb\nc\nd\ne\n", true, false),
            "b\nc\nd\n"
        );
    }

    #[test]
    fn a_range_can_reopen() {
        assert_eq!(
            run_opts("/s/,/e/p", "s\nx\ne\nq\ns\ny\ne\n", true, false),
            "s\nx\ne\ns\ny\ne\n"
        );
    }

    #[test]
    fn a_range_that_never_ends_runs_to_the_last_line() {
        assert_eq!(run_opts("/c/,/zz/p", "a\nb\nc\nd\n", true, false), "c\nd\n");
    }

    #[test]
    fn the_last_line_has_a_name() {
        assert_eq!(run_opts("$p", "a\nb\nc\n", true, false), "c\n");
        assert_eq!(run("$d", "a\nb\nc\n"), "a\nb\n");
    }

    #[test]
    fn relative_and_stepped_ranges() {
        assert_eq!(
            run_opts("2,+2p", "1\n2\n3\n4\n5\n", true, false),
            "2\n3\n4\n"
        );
        // `~4` runs to the *next* multiple of four, so starting on line 4 goes
        // past it to line 8 — which this input never reaches.
        assert_eq!(
            run_opts("2,~4p", "1\n2\n3\n4\n5\n", true, false),
            "2\n3\n4\n"
        );
        assert_eq!(run_opts("4,~4p", "1\n2\n3\n4\n5\n", true, false), "4\n5\n");
        assert_eq!(
            run_opts("1~2p", "1\n2\n3\n4\n5\n", true, false),
            "1\n3\n5\n"
        );
    }

    #[test]
    fn a_range_starting_at_zero_can_end_on_the_first_line() {
        // `1,/a/` cannot stop on line 1 — the end address is only looked at
        // from the line after the start. `0,/a/` is the spelling that can.
        assert_eq!(run_opts("0,/a/p", "a\nb\na\n", true, false), "a\n");
        assert_eq!(run_opts("1,/a/p", "a\nb\na\n", true, false), "a\nb\na\n");
    }

    #[test]
    fn an_address_can_be_negated() {
        assert_eq!(run("2!d", "a\nb\nc\n"), "b\n");
        assert_eq!(run_opts("/b/!p", "a\nb\nc\n", true, false), "a\nc\n");
    }

    #[test]
    fn an_address_may_use_its_own_delimiter() {
        assert_eq!(run_opts(r"\%/usr%p", "/usr\n/tmp\n", true, false), "/usr\n");
    }

    #[test]
    fn an_address_can_ignore_case() {
        assert_eq!(run_opts("/abc/Ip", "ABC\nxyz\n", true, false), "ABC\n");
    }

    // ---------------- substitution ----------------

    #[test]
    fn substitution_replaces_the_first_match_or_all_of_them() {
        assert_eq!(run("s/o/0/", "foo boo\n"), "f0o boo\n");
        assert_eq!(run("s/o/0/g", "foo boo\n"), "f00 b00\n");
    }

    #[test]
    fn substitution_can_start_from_the_nth_match() {
        assert_eq!(run("s/o/0/2", "foo boo\n"), "fo0 boo\n");
        assert_eq!(run("s/o/0/2g", "foo boo\n"), "fo0 b00\n");
    }

    #[test]
    fn an_empty_match_is_not_reported_twice_at_the_same_place() {
        // `s/a*/-/g` on `aaa` is `-`, not `--`: after `a*` has taken the whole
        // run there is an empty match available at the end, and it is the same
        // position the previous match reached rather than a new one.
        assert_eq!(run("s/a*/-/g", "aaa\n"), "-\n");
        assert_eq!(run("s/x*/-/g", "axb\n"), "-a-b-\n");
    }

    #[test]
    fn the_replacement_can_name_the_match_and_its_parts() {
        assert_eq!(run("s/b*/[&]/", "bbc\n"), "[bb]c\n");
        assert_eq!(run(r"s/b/[\&]/", "abc\n"), "a[&]c\n");
        assert_eq!(run(r"s/\(a\)\(b\)/\2\1/", "ab\n"), "ba\n");
    }

    #[test]
    fn the_replacement_understands_case_folding() {
        assert_eq!(run(r"s/.*/\U&/", "shout\n"), "SHOUT\n");
        assert_eq!(run(r"s/\(.\)\(.*\)/\u\1\2/", "word\n"), "Word\n");
        assert_eq!(run(r"s/\(.*\)/\U\1\E!/", "hi\n"), "HI!\n");
    }

    #[test]
    fn a_substitution_may_use_any_delimiter() {
        assert_eq!(run("s|/usr|/opt|", "/usr/bin\n"), "/opt/bin\n");
        // A delimiter that is also a metacharacter must not become one: with
        // `.` as the delimiter, `\.` is a full stop and not "any character".
        assert_eq!(run(r"s.a\.b.X.", "a.b\n"), "X\n");
        assert_eq!(run(r"s.a\.b.X.", "axb\n"), "axb\n");
    }

    #[test]
    fn an_empty_pattern_reuses_the_last_one() {
        assert_eq!(
            run_opts("/foo/s//bar/", "a foo z\n", false, false),
            "a bar z\n"
        );
    }

    #[test]
    fn substitution_flags_p_and_i() {
        assert_eq!(run_opts("s/a/X/p", "abc\n", true, false), "Xbc\n");
        assert_eq!(run("s/ABC/x/I", "abc\n"), "x\n");
    }

    // ---------------- the rest of the command set ----------------

    #[test]
    fn print_and_delete() {
        assert_eq!(run("p", "a\n"), "a\na\n");
        assert_eq!(run("d", "a\nb\n"), "");
    }

    #[test]
    fn the_hold_space_can_reverse_a_file() {
        // `tac`, written in sed. It exercises hold, exchange, append and `$`.
        assert_eq!(
            run_opts("1!G;h;$!d", "a\nb\nc\n", false, false),
            "c\nb\na\n"
        );
    }

    #[test]
    fn n_and_capital_n_read_ahead() {
        assert_eq!(run(r"$!N;s/\n/ /", "a\nb\nc\nd\n"), "a b\nc d\n");
        // An odd last line must still come out, which is why `N` at end of
        // input prints rather than dropping the pattern space.
        assert_eq!(run(r"N;s/\n/ /", "a\nb\nc\n"), "a b\nc\n");
        assert_eq!(run_opts("n;p", "a\nb\nc\nd\n", true, false), "b\nd\n");
    }

    #[test]
    fn capital_d_restarts_without_reading() {
        // Squeeze runs of blank lines: the classic `D` idiom.
        assert_eq!(run("/^$/{N;/^\\n$/D}", "a\n\n\n\nb\n"), "a\n\nb\n");
    }

    #[test]
    fn branching_loops_until_no_substitution_is_left() {
        // Turn every run of spaces into one, by looping.
        assert_eq!(run(":a;s/  / /;ta", "a    b\n"), "a b\n");
        assert_eq!(
            run("s/x/y/;T end;s/$/ (changed)/;:end", "x\nz\n"),
            "y (changed)\nz\n"
        );
    }

    #[test]
    fn a_block_groups_commands_under_one_address() {
        assert_eq!(run_opts("/b/{s/b/B/;p}", "a\nb\nc\n", true, false), "B\n");
        // An unselected block is skipped whole, not entered and re-tested.
        assert_eq!(run("/zz/{s/a/X/;s/b/Y/}", "ab\n"), "ab\n");
    }

    #[test]
    fn transliteration_maps_bytes() {
        assert_eq!(run("y/abc/xyz/", "cab\n"), "zxy\n");
        assert_eq!(run(r"y/a\n/\nA/", "a\n"), "\n\n");
    }

    #[test]
    fn append_insert_and_change() {
        assert_eq!(run("2i\\\nbefore", "a\nb\n"), "a\nbefore\nb\n");
        assert_eq!(run("1a after", "a\nb\n"), "a\nafter\nb\n");
        assert_eq!(run("2c\\\nnew", "a\nb\nc\n"), "a\nnew\nc\n");
        // For a range, `c` writes its text once, when the range closes.
        assert_eq!(run("1,2c\\\nnew", "a\nb\nc\n"), "new\nc\n");
    }

    // ---- the escapes that name a byte -------------------------------------
    //
    // Every expectation below was measured against GNU sed 4.9 under
    // `LC_ALL=C.UTF-8`, not read off the manual, which does not say how many
    // digits each form takes or what happens when there are none.

    #[test]
    fn a_numeric_escape_names_a_byte_in_a_pattern() {
        assert_eq!(run("s/\\x61/Z/", "abc\n"), "Zbc\n");
        assert_eq!(run("s/\\o141/Z/", "abc\n"), "Zbc\n");
        assert_eq!(run("s/\\d097/Z/", "abc\n"), "Zbc\n");
        assert_eq!(run("s/\\cI/T/", "a\tb\n"), "aTb\n");
        // In an address as much as in `s`, and in the `y` command's arguments.
        assert_eq!(run_opts("/\\x61/p", "abc\n", true, false), "abc\n");
        assert_eq!(run("y/\\x61\\x62/XY/", "abc\n"), "XYc\n");
        assert_eq!(run("y/a/\\x58/", "abc\n"), "Xbc\n");
    }

    #[test]
    fn a_numeric_escape_names_a_byte_in_a_replacement() {
        assert_eq!(run("s/a/\\x41/", "abc\n"), "Abc\n");
        assert_eq!(run("s/a/\\o101/", "abc\n"), "Abc\n");
        assert_eq!(run("s/a/\\d065/", "abc\n"), "Abc\n");
        assert_eq!(run("s/a/\\cI/", "abc\n"), "\tbc\n");
        // And the byte it names is a *literal*, where the same character
        // written plainly would not be: `&` is the whole match and `\n` is a
        // newline, but `\x26` and `\x5cn` are neither.
        assert_eq!(run("s/a/\\x26/", "abc\n"), "&bc\n");
        assert_eq!(run("s/a/\\d092/", "abc\n"), "\\bc\n");
        assert_eq!(run("s/a/\\o134n/", "abc\n"), "\\nbc\n");
        assert_eq!(run("s/a/\\x31/", "abc\n"), "1bc\n");
    }

    #[test]
    fn a_pattern_does_not_protect_the_byte_an_escape_names() {
        // This is the surprising half and it is GNU's: the byte goes into the
        // pattern raw, so `\x2e` is the metacharacter and matches anything.
        assert_eq!(run("s/\\x2e/Z/", "abc\n"), "Zbc\n");
        assert_eq!(run("s/\\x2e/Z/", "a.c\n"), "Z.c\n");
        assert_eq!(run("s/a\\x2a/Z/", "aaa\n"), "Z\n");
        assert_eq!(run("s/\\x5b\\x61\\x5d/Z/", "abc\n"), "Zbc\n");
        // A bare backslash, therefore the error a bare backslash always is —
        // in glibc's words, since that is what GNU prints.
        assert_eq!(compile_err(b"s/\\x5c/Z/"), "Trailing backslash");
        // The conversion runs after the delimiter scan, so a delimiter it
        // produces is a character and not the end of the command.
        assert_eq!(run("s/\\x2f/Z/", "a/c\n"), "aZc\n");
    }

    #[test]
    fn a_numeric_escape_takes_the_digits_gnu_takes_and_no_more() {
        // Two hexadecimal digits, three decimal or octal ones.
        assert_eq!(run("s/\\x616/Z/", "a6\n"), "Z\n");
        assert_eq!(run("s/\\d0977/Z/", "a7\n"), "Z\n");
        assert_eq!(run("s/\\o1418/Z/", "a8\n"), "Z\n");
        assert_eq!(run("s/\\x0061/Z/", "a1\n"), "a1\n");
        // No digits at all is not an error: the letter denotes itself.
        assert_eq!(run("s/\\x/Z/", "xbc\n"), "Zbc\n");
        assert_eq!(run("s/\\xg/Z/", "xgc\n"), "Zc\n");
        assert_eq!(run("s/\\d/Z/", "dbc\n"), "Zbc\n");
        assert_eq!(run("s/\\o/Z/", "obc\n"), "Zbc\n");
        // The value wraps mod 256 rather than saturating or erroring.
        assert_eq!(run("s/\\d300/Z/", "a,b\n"), "aZb\n");
    }

    #[test]
    fn control_x_folds_case_then_flips_the_control_bit() {
        assert_eq!(run("s/a/[\\c1]/", "abc\n"), "[q]bc\n");
        assert_eq!(run("s/a/[\\cq]/", "abc\n"), "[\x11]bc\n");
        // `\c` at the very end of the script: GNU emits a lone backslash and
        // drops the `c`.
        assert_eq!(run("s/a/[\\c/", "abc\n"), "[\\bc\n");
        // `\c\` it refuses outright rather than guessing.
        assert_eq!(
            compile_err(b"s/a/[\\c\\t]/"),
            "recursive escaping after \\c not allowed"
        );
    }

    #[test]
    fn the_text_of_a_i_and_c_takes_the_same_escapes() {
        assert_eq!(run("1a\\\np\\tq\\x41", "z\n"), "z\np\tqA\n");
        assert_eq!(run("1a\\\np\\nq", "z\n"), "z\np\nq\n");
        // The `\` in `i\x41` is the marker introducing the text, so it is eaten
        // and the three characters `x41` are what is left — GNU's answer too.
        assert_eq!(run("1i\\x41", "z\n"), "x41\nz\n");
    }

    #[test]
    fn equals_prints_the_line_number() {
        assert_eq!(run_opts("=", "a\nb\n", true, false), "1\n2\n");
    }

    #[test]
    fn quit_stops_and_can_choose_a_status() {
        assert_eq!(run("2q", "a\nb\nc\n"), "a\nb\n");
        assert_eq!(run("2Q", "a\nb\nc\n"), "a\n");
    }

    #[test]
    fn a_comment_and_the_hash_n_first_line() {
        assert_eq!(run("# nothing\np", "a\n"), "a\na\n");
        assert_eq!(run("#n\np", "a\n"), "a\n");
        // `#no` is a comment, not the `-n` spelling.
        assert_eq!(run("#no\np", "a\n"), "a\na\n");
    }

    // ---------------- bytes and newlines ----------------

    #[test]
    fn a_missing_final_newline_stays_missing() {
        assert_eq!(run("p", "a"), "a\na");
        assert_eq!(run("s/a/b/", "a"), "b");
        assert_eq!(run("p", "a\n"), "a\na\n");
    }

    #[test]
    fn a_line_that_is_not_text_passes_through() {
        let compiled = compile(b"s/b/B/", false).unwrap();
        let raw: Vec<u8> = vec![0xff, b'a', b'b', 0xfe, b'\n'];
        let mut sink: Vec<u8> = Vec::new();
        let mut inp = Input {
            paths: Vec::new(),
            next_path: 0,
            cur_name: OsString::from("stdin"),
            cur: Some(Box::new(BufReader::new(io::Cursor::new(raw)))),
            peeked: None,
            sep: b'\n',
            had_error: false,
        };
        let mut job = Job {
            script: &compiled,
            wfiles: Vec::new(),
            rfiles: Vec::new(),
            suppress: false,
            sep: b'\n',
        };
        job.run_one(&mut inp, &mut sink);
        assert_eq!(sink, vec![0xff, b'a', b'B', 0xfe, b'\n']);
    }

    // ---------------- `l` ----------------

    /// [`list_escape`] as a string, for tests whose expectation is readable.
    fn listed(input: &[u8], width: usize) -> String {
        String::from_utf8_lossy(&list_escape(input, width, b'\n')).into_owned()
    }

    #[test]
    fn l_escapes_by_byte_and_marks_the_end() {
        // Every one of these was checked against GNU sed rather than recalled.
        assert_eq!(listed(b"a\tb\\c\r", 0), "a\\tb\\\\c\\r$");
        assert_eq!(listed(b"\x07\x08\x0b\x0c\n", 0), "\\a\\b\\v\\f\\n$");
        // Byte-wise even for text that is perfectly good UTF-8: `l` is asked
        // for precisely when one needs to see the encoding.
        assert_eq!(listed("café".as_bytes(), 0), "caf\\303\\251$");
        // Space is printable and DEL is not, which is `isprint` in C.
        assert_eq!(listed(b" ~\x7f", 0), " ~\\177$");
    }

    #[test]
    fn l_wraps_whole_escapes_and_reserves_the_continuation_column() {
        // `-l 1` can never fit an escape in `1 - 1` columns, so it opens with a
        // bare `\` and a break. GNU does this too; it is the `+ 1` showing.
        assert_eq!(listed(b"\x01abc", 1), "\\\n\\001\\\na\\\nb\\\nc$");
        // At 3, `\001` still does not fit beside anything, but `ab` does.
        assert_eq!(listed(b"\x01abc", 3), "\\\n\\001\\\nab\\\nc$");
        // At 5 the four-column escape fits on the first line — the escape is
        // never torn in half to fill the width exactly.
        assert_eq!(listed(b"\x01abc", 5), "\\001\\\nabc$");
        assert_eq!(listed(b"aaaaaaaa", 3), "aa\\\naa\\\naa\\\naa$");
        // 0 is not "wrap at 0", it is "never wrap".
        assert_eq!(listed(&[b'a'; 200], 0), format!("{}$", "a".repeat(200)));
        // The default width breaks after 69, leaving the 70th column for `\`.
        assert_eq!(
            listed(&[b'a'; 80], DEFAULT_LINE_LEN),
            format!("{}\\\n{}$", "a".repeat(69), "a".repeat(11))
        );
    }

    #[test]
    fn l_wraps_on_the_output_separator_so_dash_z_stays_nul_terminated() {
        // Measured: `printf 'abc\0' | sed -z -n 'l 2'` gives `a\<NUL>b\<NUL>c$`.
        // One column per line, because the second is reserved for the `\`.
        assert_eq!(list_escape(b"abc", 2, 0), b"a\\\0b\\\0c$".to_vec());
    }

    #[test]
    fn l_takes_a_width_of_its_own_that_overrides_dash_l() {
        // `l 0` beats a narrow `-l`, and `l` with no number defers to it.
        // The width a one-command script's `l` resolved to, with `-l` set to 3.
        let width_of = |script: &[u8]| -> usize {
            let s = compile_script(script, &[], false, 3, false).expect("compiling");
            match &s.cmds.first().expect("one command").act {
                Action::List(w) => *w,
                _ => panic!("{} did not parse as `l`", String::from_utf8_lossy(script)),
            }
        };
        assert_eq!(width_of(b"l 0"), 0);
        assert_eq!(width_of(b"l0"), 0);
        assert_eq!(width_of(b"l 9"), 9);
        // With no number of its own, `l` takes `-l`'s.
        assert_eq!(width_of(b"l"), 3);
    }

    #[test]
    fn l_prints_a_separator_even_when_the_input_line_had_none() {
        // `l` renders rather than copies, so it does not inherit a missing
        // final newline the way `p` does — `p` on the same input does.
        assert_eq!(run_opts("l", "ab", true, false), "ab$\n");
        assert_eq!(run_opts("p", "ab", true, false), "ab");
    }

    // ---------------- `W` ----------------

    #[test]
    fn capital_w_writes_as_far_as_the_first_separator() {
        let dir = std::env::temp_dir().join("sed-w-test");
        let _ = fs::create_dir_all(&dir);
        let target = dir.join("first.txt");
        let path = target.to_string_lossy().into_owned();
        let _ = fs::remove_file(&target);

        // Two lines joined by `N`, so the pattern space holds an embedded
        // separator: `W` takes the first line and terminates it, whatever the
        // input's own final separator was.
        let script = format!("$!N;W {path}");
        run_opts(&script, "a\nb", true, false);
        assert_eq!(fs::read(&target).expect("W wrote nothing"), b"a\n");

        // With no embedded separator, `W` writes the whole pattern space and
        // inherits the input line's missing separator, exactly like `w`.
        let _ = fs::remove_file(&target);
        run_opts(&format!("W {path}"), "a", true, false);
        assert_eq!(fs::read(&target).expect("W wrote nothing"), b"a");
        let _ = fs::remove_file(&target);
    }

    // ---------------- `R` ----------------

    #[test]
    fn capital_r_takes_one_line_per_cycle_from_a_shared_position() {
        let dir = std::env::temp_dir().join("sed-r-test");
        let _ = fs::create_dir_all(&dir);
        let inc = dir.join("inc.txt");
        fs::write(&inc, b"A\nB\nC\n").expect("writing the include");
        let path = inc.to_string_lossy().into_owned();

        // One `R` per cycle, in order.
        assert_eq!(run(&format!("R {path}"), "1\n2\n"), "1\nA\n2\nB\n");

        // Two `R`s naming one file share the position, so a single cycle takes
        // two *different* lines rather than the same one twice.
        assert_eq!(
            run(&format!("R {path}\nR {path}"), "1\n2\n"),
            "1\nA\nB\n2\nC\n"
        );

        // Running off the end is a silent no-op, not an error and not a blank.
        assert_eq!(
            run(&format!("R {path}"), "1\n2\n3\n4\n"),
            "1\nA\n2\nB\n3\nC\n4\n"
        );

        // A file with no final separator hands its last line over as it is.
        let short = dir.join("short.txt");
        fs::write(&short, b"Z").expect("writing the include");
        assert_eq!(
            run(&format!("R {}", short.to_string_lossy()), "1\n"),
            "1\nZ"
        );

        // A missing file is an inclusion that is not there, which is not a
        // failure: the text comes out without it.
        assert_eq!(run(&format!("R {path}.nosuch"), "1\n2\n"), "1\n2\n");
        let _ = fs::remove_file(&inc);
        let _ = fs::remove_file(&short);
    }

    // ---------------- `--sandbox` ----------------

    #[test]
    fn the_sandbox_refuses_every_command_that_reaches_outside_the_script() {
        // `e` and `s///e` belong in this list and are absent only because they
        // are not implemented at all yet; see `known-issues.md`. When they
        // arrive they must be added here and to `deny_in_sandbox`'s callers.
        for script in [&b"r f"[..], b"R f", b"w f", b"W f", b"s/a/b/w f"] {
            assert!(
                compile_script(script, &[], false, DEFAULT_LINE_LEN, true).is_err(),
                "{} should be refused under --sandbox",
                String::from_utf8_lossy(script)
            );
            // …and is a perfectly good script without it, so the refusal is
            // the sandbox's doing and not a parse failure in disguise.
            assert!(
                compile_script(script, &[], false, DEFAULT_LINE_LEN, false).is_ok(),
                "{} should compile without --sandbox",
                String::from_utf8_lossy(script)
            );
        }
        // A script that reaches nowhere is unaffected.
        assert!(compile_script(b"s/a/b/p", &[], false, DEFAULT_LINE_LEN, true).is_ok());
    }

    // ---------------- `-z` ----------------

    /// Every command that joins or splits the pattern space has to do it on the
    /// *buffer delimiter*, not on a newline. Five of them used a literal `\n`,
    /// so under `-z` they silently worked on the wrong byte: `N` produced a
    /// pattern space joined by a newline that `D` and `P` could then not find,
    /// and `G`/`H` corrupted the hold space the same way.
    ///
    /// Each expectation below is what GNU sed printed for the same script.
    #[test]
    fn the_separator_that_joins_and_splits_is_the_one_dash_z_chose() {
        let z = |script: &str, input: &[u8]| run_sep(script.as_bytes(), input, false, false, 0);
        let zn = |script: &str, input: &[u8]| run_sep(script.as_bytes(), input, true, false, 0);

        // `N` joins with NUL. This is a discriminating test on its own: joined
        // with a newline the output would be `a\nb\0`, one byte in the middle
        // different, which is exactly the bug.
        assert_eq!(z("N", b"a\0b\0"), b"a\0b\0".to_vec());
        // `G` appends the hold space after a NUL.
        assert_eq!(z("x;s/^/H/;x;G", b"a\0"), b"a\0H\0".to_vec());
        // `H` appends to the hold space after a NUL — including the first time,
        // when the hold space is empty, which is why this starts with one.
        assert_eq!(zn("H;${x;p}", b"a\0b\0"), b"\0a\0b\0".to_vec());
        // `P` prints as far as the first NUL, and terminates with one.
        assert_eq!(zn("N;P", b"a\0b\0"), b"a\0".to_vec());
        // `D` deletes as far as the first NUL and restarts without reading.
        assert_eq!(zn("$!{N;D};p", b"a\0b\0c\0"), b"c\0".to_vec());
    }

    #[test]
    fn capital_w_splits_on_the_separator_too() {
        let dir = std::env::temp_dir().join("sed-wz-test");
        let _ = fs::create_dir_all(&dir);
        let target = dir.join("wz.txt");
        let _ = fs::remove_file(&target);
        let script = format!("N;W {}", target.to_string_lossy());
        run_sep(script.as_bytes(), b"a\0b\0", true, false, 0);
        assert_eq!(fs::read(&target).expect("W wrote nothing"), b"a\0");
        let _ = fs::remove_file(&target);
    }

    // ---------------- compile errors ----------------

    #[test]
    fn a_bad_script_is_refused_rather_than_guessed_at() {
        assert!(compile(b"Z", false).is_err());
        assert!(compile(b"s/a", false).is_err());
        assert!(compile(b"{p", false).is_err());
        assert!(compile(b"p}", false).is_err());
        assert!(compile(b"b nowhere", false).is_err());
        assert!(compile(b"y/ab/x/", false).is_err());
        assert!(compile(b"s/[a/x/", false).is_err());
    }

    #[test]
    fn a_good_script_compiles() {
        for script in [
            "s/a/b/",
            "1,$s/a/b/g",
            "/x/,/y/{s/a/b/;p}",
            ":top;$!{N;btop}",
            "s/a/b/w out.txt",
            r"\,x,d",
            "2{h;d}",
        ] {
            assert!(
                compile(script.as_bytes(), false).is_ok(),
                "{script} should compile"
            );
        }
    }

    // ---------------- parse_args ----------------

    /// The `SedArgs` of a command line that is expected to be a run.
    fn run_args(items: &[&str]) -> SedArgs {
        match parse_args(&os(items)) {
            Ok(Request::Run(a)) => a,
            other => panic!("{items:?} parsed as {other:?}"),
        }
    }

    fn text(t: &str) -> ScriptPart {
        ScriptPart::Text(t.as_bytes().to_vec())
    }

    fn names(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_first_bare_argument_is_the_script_unless_e_gave_one() {
        let a = run_args(&["s/a/b/", "in.txt"]);
        assert_eq!(a.script_parts, vec![text("s/a/b/")]);
        assert_eq!(a.files, names(&["in.txt"]));

        let a = run_args(&["-e", "p", "in.txt"]);
        assert_eq!(a.script_parts, vec![text("p")]);
        assert_eq!(a.files, names(&["in.txt"]));
    }

    /// The rule is about the *whole* command line, not about what has been seen
    /// so far. Measured: `sed foo -e p` reads the file `foo`, so an operand
    /// cannot be classified until the last option has been parsed.
    #[test]
    fn an_e_after_the_first_operand_still_makes_that_operand_a_file() {
        let a = run_args(&["foo", "-e", "p"]);
        assert_eq!(a.script_parts, vec![text("p")]);
        assert_eq!(a.files, names(&["foo"]));
    }

    #[test]
    fn short_options_bundle() {
        let a = run_args(&["-ne", "p", "f"]);
        assert!(a.suppress);
        assert_eq!(a.script_parts, vec![text("p")]);
        assert_eq!(a.files, names(&["f"]));
    }

    #[test]
    fn a_value_may_be_attached_to_its_option() {
        let a = run_args(&["-ep", "f"]);
        assert_eq!(a.script_parts, vec![text("p")]);
        assert_eq!(a.files, names(&["f"]));
    }

    #[test]
    fn in_place_takes_its_suffix_attached_and_never_the_next_argument() {
        let a = run_args(&["-i.bak", "s/a/b/", "f"]);
        assert_eq!(a.in_place, Some(OsString::from(".bak")));
        assert!(a.separate);
        assert_eq!(a.files, names(&["f"]));

        // `-i backup` edits `backup`; it does not name a suffix.
        let a = run_args(&["-i", "s/a/b/", "f"]);
        assert_eq!(a.in_place, Some(OsString::new()));
        assert_eq!(a.files, names(&["f"]));
    }

    #[test]
    fn extended_regular_expressions_have_two_spellings() {
        assert!(run_args(&["-E", "p"]).ere);
        assert!(run_args(&["-r", "p"]).ere);
        assert!(run_args(&["--regexp-extended", "p"]).ere);
    }

    #[test]
    fn long_options_may_carry_a_value() {
        let a = run_args(&["--expression=p", "--in-place=.bak", "f"]);
        assert_eq!(a.script_parts, vec![text("p")]);
        assert_eq!(a.in_place, Some(OsString::from(".bak")));
        assert_eq!(a.files, names(&["f"]));
    }

    /// Every GNU utility's long options abbreviate, and sed's are no exception.
    #[test]
    fn long_options_abbreviate_to_an_unambiguous_prefix() {
        let a = run_args(&["--expr=p", "f"]);
        assert_eq!(a.script_parts, vec![text("p")]);
        assert!(run_args(&["--sil", "p"]).suppress);
    }

    /// Measured: `sed --s` lists its candidates in the table's order, which is
    /// GNU's declaration order rather than alphabetical.
    #[test]
    fn an_ambiguous_prefix_lists_the_candidates_in_gnus_order() {
        let e = parse_args(&os(&["--s", "-e", "p"])).unwrap_err();
        assert_eq!(
            e.sentence,
            "option '--s' is ambiguous; possibilities: '--silent' '--sandbox' '--separate'"
        );
    }

    #[test]
    fn a_lone_dash_is_a_file_not_an_option() {
        let a = run_args(&["p", "-"]);
        assert_eq!(a.files, names(&["-"]));
    }

    #[test]
    fn double_dash_ends_the_options() {
        let a = run_args(&["-n", "--", "p", "-weird"]);
        assert!(a.suppress);
        assert_eq!(a.files, names(&["-weird"]));
    }

    #[test]
    fn an_unknown_option_is_reported() {
        // The sentences are glibc's, measured against GNU sed under `C.UTF-8`.
        let e = parse_args(&os(&["-Z", "p"])).unwrap_err();
        assert_eq!(e.sentence, "invalid option -- 'Z'");
        let e = parse_args(&os(&["--nope"])).unwrap_err();
        assert_eq!(e.sentence, "unrecognized option '--nope'");
        let e = parse_args(&os(&["-e"])).unwrap_err();
        assert_eq!(e.sentence, "option requires an argument -- 'e'");
        let e = parse_args(&os(&["--file"])).unwrap_err();
        assert_eq!(e.sentence, "option '--file' requires an argument");
        let e = parse_args(&os(&["--help=x"])).unwrap_err();
        assert_eq!(e.sentence, "option '--help' doesn't allow an argument");
    }

    #[test]
    fn help_and_version_are_requests_of_their_own() {
        assert_eq!(parse_args(&os(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&os(&["--version"])).unwrap(), Request::Version);
    }

    /// Both spellings of "print the usage and stop, with nothing above it":
    /// nothing to run, and the obsolete `-V`, which takes a required argument
    /// and rejects it whatever it is.
    #[test]
    fn a_command_line_with_no_script_is_a_bare_usage_error() {
        assert_eq!(parse_args(&os(&[])).unwrap(), Request::BadUsage);
        assert_eq!(parse_args(&os(&["-n"])).unwrap(), Request::BadUsage);
        assert_eq!(
            parse_args(&os(&["-V4.2", "-e", "p"])).unwrap(),
            Request::BadUsage
        );
        // …but `-V` alone is still a missing argument, not a usage block.
        let e = parse_args(&os(&["-V"])).unwrap_err();
        assert_eq!(e.sentence, "option requires an argument -- 'V'");
    }

    /// The options sed takes and does nothing with are still options: they must
    /// not fall through to `invalid option`, and must not become the script.
    #[test]
    fn the_accepted_and_ignored_options_are_accepted() {
        let a = run_args(&[
            "-u",
            "-b",
            "--posix",
            "--debug",
            "--follow-symlinks",
            "p",
            "f",
        ]);
        assert_eq!(a.script_parts, vec![text("p")]);
        assert_eq!(a.files, names(&["f"]));
    }

    #[test]
    fn the_options_that_l_and_the_sandbox_read_reach_the_compiler() {
        let a = run_args(&["--sandbox", "-l", "5", "p", "f"]);
        assert!(a.sandbox);
        assert_eq!(a.line_len, 5);
        assert_eq!(a.script_parts, vec![text("p")]);
        assert_eq!(a.files, names(&["f"]));
        // Absent, `-l` is 70 and the sandbox is open.
        let a = run_args(&["p"]);
        assert!(!a.sandbox);
        assert_eq!(a.line_len, DEFAULT_LINE_LEN);
    }

    /// `-l` is read with `atoi`, which cannot report a failure, so none of these
    /// is an error. Every one was measured against GNU sed.
    #[test]
    fn a_line_length_is_read_the_way_atoi_reads_it() {
        assert_eq!(atoi(OsStr::new("5")), 5);
        assert_eq!(atoi(OsStr::new("3x")), 3);
        assert_eq!(atoi(OsStr::new("x")), 0);
        assert_eq!(atoi(OsStr::new("")), 0);
        assert_eq!(atoi(OsStr::new("-1")), 0);
        // Saturating rather than wrapping: a wrap here would turn a number
        // meaning "very wide" into some narrow width, which would silently
        // mangle output rather than merely disagree about it.
        assert_eq!(atoi(OsStr::new(&"9".repeat(40))), usize::MAX);
        assert_eq!(run_args(&["-l", "3x", "p"]).line_len, 3);
        assert_eq!(run_args(&["-lx", "p"]).line_len, 0);
    }

    /// The usage block is what a usage error prints, so its first line has to
    /// be GNU's word for word — a caller that matches on it should still match.
    #[test]
    fn the_usage_first_line_is_gnus() {
        assert_eq!(
            USAGE.lines().next(),
            Some("Usage: sed [OPTION]... {script-only-if-no-other-script} [input-file]...")
        );
    }

    // ---------------- -i backup names ----------------

    #[test]
    fn a_backup_suffix_is_appended_and_a_star_is_the_whole_name() {
        let name = |p: &str, sfx: &str| {
            backup_name(OsStr::new(p), OsStr::new(sfx)).map(|b| b.to_string_lossy().into_owned())
        };
        assert_eq!(name("f", ".bak"), Some("f.bak".to_string()));
        assert_eq!(name("d/f", ".bak"), Some("d/f.bak".to_string()));
        assert_eq!(name("f", "old/*"), Some("old/f".to_string()));
        assert_eq!(name("f", "*.*"), Some("f.f".to_string()));
        // The two spellings of "no backup": no suffix, and the bare `*` that
        // upstream turns a missing suffix into.
        assert_eq!(name("f", ""), None);
        assert_eq!(name("f", "*"), None);
    }

    // ---------------- exit status ----------------

    #[test]
    fn the_status_distinguishes_a_bad_file_from_a_bad_script() {
        assert_eq!(status(None, false), 0);
        assert_eq!(status(None, true), 2);
        assert_eq!(status(Some(5), false), 5);
        // `q5` is louder than "a file was missing", so it wins.
        assert_eq!(status(Some(5), true), 5);
    }

    // ---------------- what may follow a command ----------------

    /// GNU's `read_end_of_cmd`. Every offset below was measured: upstream reads
    /// the offending character before deciding it is one, so it reports one
    /// past it.
    #[test]
    fn a_command_that_takes_no_argument_is_followed_by_a_separator_or_nothing() {
        for script in [
            &b"px"[..],
            b"p x",
            b"d x",
            b"=x",
            b"q5x",
            b"l5x",
            b"y/a/b/x",
            b"{p} x",
            b"{p}p",
        ] {
            assert_eq!(
                compile_err(script),
                "extra characters after command",
                "{}",
                String::from_utf8_lossy(script)
            );
        }
        // …and the spellings that are not junk, which is the other half of it.
        for script in [
            &b"p;p"[..],
            b"p #c",
            b"{p}",
            b"{ p }",
            b"{p};p",
            b"q 5",
            b"y/a/b/;p",
        ] {
            assert!(
                compile(script, false).is_ok(),
                "{}",
                String::from_utf8_lossy(script)
            );
        }
    }

    /// A label ends at whitespace, so what follows a space is the next command.
    /// `sed 'b x y'` is a branch to `x` and then an unterminated `y`.
    #[test]
    fn a_label_ends_at_the_first_blank() {
        assert_eq!(compile_err(b"b x y"), "unterminated `y' command");
        assert_eq!(compile_err(b":x y"), "unterminated `y' command");
        assert_eq!(compile_err(b"b x s"), "unterminated `s' command");
        // A `}` is not label text either: `{b}` branches to the end of the
        // script and closes its block.
        assert!(compile(b"{b}", false).is_ok());
        assert!(compile(b"{:x}", false).is_ok());
        assert_eq!(compile_err(b": "), "\":\" lacks a label");
    }

    /// A missing delimiter is the same fault as a missing closing one, because
    /// upstream reads both with the same call.
    #[test]
    fn s_and_y_report_a_missing_delimiter_as_an_unterminated_command() {
        assert_eq!(compile_err(b"s"), "unterminated `s' command");
        assert_eq!(compile_err(b"s/a"), "unterminated `s' command");
        assert_eq!(compile_err(b"s/a/b"), "unterminated `s' command");
        assert_eq!(compile_err(b"y"), "unterminated `y' command");
        assert_eq!(compile_err(b"y/ab/cd"), "unterminated `y' command");
    }

    /// Every rejection the `s` flag scan can make, in GNU's words.
    #[test]
    fn the_s_flags_are_scanned_the_way_gnu_scans_them() {
        assert_eq!(compile_err(b"s/a/b/x"), "unknown option to `s'");
        assert_eq!(
            compile_err(b"s/a/b/gg"),
            "multiple `g' options to `s' command"
        );
        assert_eq!(
            compile_err(b"s/a/b/pp"),
            "multiple `p' options to `s' command"
        );
        assert_eq!(
            compile_err(b"s/a/b/p2p"),
            "multiple `p' options to `s' command"
        );
        assert_eq!(
            compile_err(b"s/a/b/2 3"),
            "multiple number options to `s' command"
        );
        assert_eq!(
            compile_err(b"s/a/b/0"),
            "number option to `s' command may not be zero"
        );
        // Blanks separate flags without ending them; `I` and `i` are the same
        // flag and GNU does not count them; `}` and `#` end them.
        for script in [
            &b"s/a/b/ "[..],
            b"s/a/b/ p",
            b"s/a/b/  2",
            b"s/a/b/Ii",
            b"s/a/b/#c",
            b"{s/a/b/}",
            b"s/a/b/2gp",
        ] {
            assert!(
                compile(script, false).is_ok(),
                "{}",
                String::from_utf8_lossy(script)
            );
        }
    }

    /// `\N` on the right-hand side may only name a group the pattern has.
    #[test]
    fn a_replacement_may_not_name_a_group_the_pattern_lacks() {
        assert_eq!(
            compile_err(br"s/a/\9/g"),
            "invalid reference \\9 on `s' command's RHS"
        );
        assert_eq!(
            compile_err(br"s/\(a\)/\2/"),
            "invalid reference \\2 on `s' command's RHS"
        );
        assert!(compile(br"s/\(a\)/\1/", false).is_ok());
        assert!(compile(br"s/a/\0/", false).is_ok());
        // Nothing to check against: `s//\1/` re-uses a pattern that is not
        // known until the command runs.
        assert!(compile(br"s/a/b/;s//\1/", false).is_ok());
    }

    /// Line 0 exists only so `0,/re/` can end on line 1.
    #[test]
    fn line_address_zero_is_refused_everywhere_but_the_start_of_a_regex_range() {
        for script in [
            &b"0"[..],
            b"0p",
            b"0!p",
            b"0,3p",
            b"0,3!p",
            b"0,$p",
            b"0,0p",
            b"0,+2p",
            b"0,~2p",
            b"0~0p",
        ] {
            assert_eq!(
                compile_err(script),
                "invalid usage of line address 0",
                "{}",
                String::from_utf8_lossy(script)
            );
        }
        for script in [&b"0,/a/p"[..], b"0,/a/!p", b"0~3p", b"/a/,0p", b"$,0p"] {
            assert!(
                compile(script, false).is_ok(),
                "{}",
                String::from_utf8_lossy(script)
            );
        }
    }

    /// An absent number is zero rather than an error, so what these lack is a
    /// command and not a number.
    #[test]
    fn a_missing_step_or_count_is_zero() {
        for script in [&b"1"[..], b"1,2", b"/a/", b"1~", b"1,+", b"1,~"] {
            assert_eq!(
                compile_err(script),
                "missing command",
                "{}",
                String::from_utf8_lossy(script)
            );
        }
        // `1~` is `1~0`, which is line 1 — not "every line from 1".
        assert_eq!(run("1~p", "a\nb\nc\n"), "a\na\nb\nc\n");
    }

    // ---------------- where an error is ----------------

    /// The two shapes GNU has, and the rule that tells them apart. A `-e`
    /// fragment is numbered and gets a character offset; a `-f` file is named
    /// and gets a line number, and does not advance the expression counter.
    #[test]
    fn an_error_is_located_in_the_fragment_it_fell_in() {
        // The map a `-e s/a/A/ -f x.sed -e p` would produce, where `x.sed` holds
        // two lines. Built by hand rather than through `collect_script`, which
        // would have to read a real file.
        let script = b"s/a/A/\np\np\np";
        let segments = [
            Segment {
                start: 0,
                origin: Origin::Expr(1),
            },
            Segment {
                start: 7,
                origin: Origin::File(b"x.sed".to_vec()),
            },
            Segment {
                start: 11,
                origin: Origin::Expr(2),
            },
        ];
        let at =
            |pos| locate(script, &segments, pos).map(|b| String::from_utf8_lossy(&b).into_owned());
        assert_eq!(at(Pos::At(3)), Some("-e expression #1, char 3".to_string()));
        assert_eq!(at(Pos::At(7)), Some("file x.sed line 1".to_string()));
        assert_eq!(at(Pos::At(9)), Some("file x.sed line 2".to_string()));
        assert_eq!(
            at(Pos::At(11)),
            Some("-e expression #2, char 0".to_string())
        );
        assert_eq!(
            at(Pos::At(12)),
            Some("-e expression #2, char 1".to_string())
        );
        // A fault raised after the script has been read has no offset within an
        // expression, but still has a line within a file.
        assert_eq!(
            at(Pos::AfterParse(12)),
            Some("-e expression #2, char 0".to_string())
        );
        assert_eq!(
            at(Pos::AfterParse(9)),
            Some("file x.sed line 2".to_string())
        );
        assert_eq!(at(Pos::Nowhere), None);
    }

    /// `-f` files are not counted as expressions, and a bare script operand is.
    #[test]
    fn only_dash_e_fragments_are_numbered() {
        let (script, segments) = collect_script(&[
            ScriptPart::Text(b"p".to_vec()),
            ScriptPart::Text(b"d".to_vec()),
        ])
        .expect("no file to read");
        assert_eq!(script, b"p\nd");
        assert_eq!(
            segments,
            [
                Segment {
                    start: 0,
                    origin: Origin::Expr(1)
                },
                Segment {
                    start: 2,
                    origin: Origin::Expr(2)
                },
            ]
        );
    }

    // ---------------- the text of a, i and c ----------------

    /// The two spellings disagree about leading whitespace, and "no text" is
    /// not the same as "one empty line". All measured against GNU.
    #[test]
    fn the_text_keeps_the_blanks_of_the_backslash_form_and_not_the_one_liners() {
        assert_eq!(run("a   X", "1\n"), "1\nX\n");
        assert_eq!(run("a\\\n  X", "1\n"), "1\n  X\n");
        // A backslash in the text is dropped, which is how a text line can
        // begin with something that would otherwise be eaten.
        assert_eq!(run("a\\\n\\  X", "1\n"), "1\n  X\n");
        assert_eq!(run("a\\\nX\\\nY", "1\n"), "1\nX\nY\n");
        // Nothing at all after the backslash: no text, so nothing is appended.
        assert_eq!(run("a\\", "1\n"), "1\n");
        assert_eq!(run("i\\", "1\n"), "1\n");
        assert_eq!(run("c\\", "1\n"), "");
        // A newline after it: a text of one empty line, which *is* appended.
        assert_eq!(run("a\\\n", "1\n"), "1\n\n");
        assert_eq!(run("i\\\n", "1\n"), "\n1\n");
    }

    /// `a` writes its text as it was stored, newline and all, so under `-z` it
    /// is the one line a NUL does not end. `i` and `c` re-supply the separator.
    #[test]
    fn the_text_of_a_ends_in_a_newline_even_under_dash_z() {
        // Spelled as a join so the NUL that ends `X` cannot be read as part of
        // an escape belonging to the byte after it.
        let joined = |a: &[u8], b: &[u8]| [a, b].concat();
        assert_eq!(
            run_sep(b"a X", b"1\0", false, false, 0),
            joined(b"1\0", b"X\n")
        );
        assert_eq!(
            run_sep(b"i X", b"1\0", false, false, 0),
            joined(b"X\0", b"1\0")
        );
        assert_eq!(run_sep(b"c X", b"1\0", false, false, 0), b"X\0".to_vec());
    }

    // ---------------- the empty regular expression ----------------

    /// `//` re-uses the last regex that was *tried*, not the last that matched:
    /// evaluating an address sets it whether or not the address picked the
    /// line. Measured — `sed '/b/{p};s//X/'` substitutes on every line.
    #[test]
    fn an_empty_regex_re_uses_the_last_one_evaluated() {
        assert_eq!(run("/b/{p};s//X/", "a\nb\nc\n"), "a\nb\nX\nc\n");
        // The second `s` re-uses `/a/` and so takes the `a` the first one left.
        assert_eq!(run("s/a/b/;s//X/", "aa\n"), "bX\n");
        assert_eq!(run("/a/p;//p", "a\nb\n"), "a\na\na\nb\n");
    }

    /// With nothing tried before it, an empty regex is a *run-time* failure —
    /// GNU has no static check, so a script that never reaches the command is
    /// not a failure at all.
    #[test]
    fn an_empty_regex_with_nothing_before_it_fails_only_when_it_runs() {
        // Compiles: whether it can run is not a question about the text.
        assert!(compile(b"p;s//X/", false).is_ok());
        assert!(compile(b"//p", false).is_ok());
        // No input, so no cycle, so no failure.
        assert_eq!(run_stopping("s//X/", ""), Ok(Vec::new()));
        // One cycle's output stands before the failure.
        assert_eq!(run_stopping("p;s//X/", "a\nb\n"), Err(b"a\n".to_vec()));
        assert_eq!(run_stopping("2{s//X/}", "a\nb\n"), Err(b"a\n".to_vec()));
        // A modifier on an empty regex *is* refused while reading the script.
        assert_eq!(
            compile_err(b"s//X/I"),
            "cannot specify modifiers on empty regexp"
        );
        assert_eq!(
            compile_err(b"//Ip"),
            "cannot specify modifiers on empty regexp"
        );
    }
}
