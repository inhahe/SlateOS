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
//! | `-u` | accepted and ignored: this sed does not buffer between files |
//! | `--` | end of options; what follows is a file |
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

use coreutils::diag;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;
use std::rc::Rc;

use coreutils::errmsg::strerror;
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

/// Report a failure sed cannot continue past, and leave with status 4.
///
/// The pending output is flushed *first*: the diagnostic is about the stream
/// whose bytes were already written, so it has to arrive after them, and
/// `process::exit` does not run the runtime's own flush-at-exit.
fn panic_out(msg: &str) -> ! {
    let _ = io::stdout().flush();
    diag!("sed: {msg}");
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
    WriteFile(usize),
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
    /// Set by a `#n` first line, which is POSIX's in-script spelling of `-n`.
    suppress: bool,
}

// ---------------------------------------------------------------- the parser

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    ere: bool,
    wfiles: Vec<String>,
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
    fn skip_separators(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r' | b';')) {
            self.i = self.i.saturating_add(1);
        }
    }

    fn skip_to_eol(&mut self) {
        while !matches!(self.peek(), None | Some(b'\n')) {
            self.i = self.i.saturating_add(1);
        }
    }

    fn number(&mut self) -> Result<usize, String> {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i = self.i.saturating_add(1);
        }
        let digits = self.s.get(start..self.i).unwrap_or_default();
        if digits.is_empty() {
            return Err("expected a number".to_string());
        }
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
        // regular expression that was matched, which is a run-time value.
        if pat.is_empty() {
            return Ok(None);
        }
        let r = if self.ere {
            Regex::new_flags(pat, ci)
        } else {
            bre::compile(pat, ci)
        };
        match r {
            Ok(re) => Ok(Some(Rc::new(re))),
            Err(e) => Err(String::from_utf8_lossy(&e.detail).into_owned()),
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
                    return Ok(Some(Addr::Step(n, step)));
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
    fn parse_text(&mut self) -> Vec<u8> {
        self.skip_blank();
        if self.peek() == Some(b'\\') {
            self.i = self.i.saturating_add(1);
            self.eat(b'\n');
            self.skip_blank();
        }
        let mut out = Vec::new();
        while let Some(c) = self.peek() {
            if c == b'\n' {
                self.i = self.i.saturating_add(1);
                break;
            }
            self.i = self.i.saturating_add(1);
            if c == b'\\' {
                match self.bump() {
                    // A continued line: the newline is part of the text.
                    Some(b'\n') => out.push(b'\n'),
                    Some(n) => out.push(n),
                    None => break,
                }
                continue;
            }
            out.push(c);
        }
        out
    }

    /// A file name argument, which runs to the end of the line — a `;` in a
    /// file name is a character in a file name.
    fn parse_filename(&mut self) -> Result<String, String> {
        self.skip_blank();
        let start = self.i;
        self.skip_to_eol();
        let raw = self.s.get(start..self.i).unwrap_or_default();
        if raw.is_empty() {
            return Err("expected a file name".to_string());
        }
        String::from_utf8(raw.to_vec())
            .map_err(|_| "file names given to sed must be text".to_string())
    }

    /// Intern a `w` target so the same file opened twice is one handle, and
    /// two writes to it therefore append rather than truncate each other.
    fn wfile(&mut self, path: String) -> usize {
        if let Some(i) = self.wfiles.iter().position(|p| *p == path) {
            return i;
        }
        self.wfiles.push(path);
        self.wfiles.len().saturating_sub(1)
    }

    fn parse_label(&mut self) -> Vec<u8> {
        self.skip_blank();
        let start = self.i;
        while !matches!(self.peek(), None | Some(b'\n' | b';' | b'}')) {
            self.i = self.i.saturating_add(1);
        }
        let raw = self.s.get(start..self.i).unwrap_or_default();
        raw.iter()
            .rev()
            .skip_while(|c| matches!(**c, b' ' | b'\t' | b'\r'))
            .copied()
            .collect::<Vec<u8>>()
            .into_iter()
            .rev()
            .collect()
    }

    fn parse_subst(&mut self) -> Result<Subst, String> {
        let delim = self
            .bump()
            .ok_or_else(|| "`s' needs a delimiter".to_string())?;
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
        loop {
            match self.peek() {
                Some(b'g') => {
                    self.i = self.i.saturating_add(1);
                    global = true;
                }
                Some(b'p') => {
                    self.i = self.i.saturating_add(1);
                    print = true;
                }
                Some(b'i' | b'I') => {
                    self.i = self.i.saturating_add(1);
                    ci = true;
                }
                Some(b'm' | b'M') => {
                    return Err("the `M' flag of `s' is not supported".to_string());
                }
                Some(c) if c.is_ascii_digit() => {
                    if occurrence != 0 {
                        return Err("`s' takes only one number flag".to_string());
                    }
                    occurrence = self.number()?;
                    if occurrence == 0 {
                        return Err("`s' counts matches from 1".to_string());
                    }
                }
                Some(b'w') => {
                    self.i = self.i.saturating_add(1);
                    let path = self.parse_filename()?;
                    wfile = Some(self.wfile(path));
                    break;
                }
                _ => break,
            }
        }

        Ok(Subst {
            re: self.compile(&pat, ci)?,
            repl: parse_replacement(&raw_repl),
            global,
            occurrence: occurrence.max(1),
            print,
            wfile,
        })
    }

    fn parse_transliterate(&mut self) -> Result<Box<[u8; 256]>, String> {
        let delim = self
            .bump()
            .ok_or_else(|| "`y' needs a delimiter".to_string())?;
        let from = unescape_y(&self.take_until(delim, false, "`y' command")?);
        let to = unescape_y(&self.take_until(delim, false, "`y' command")?);
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

/// `y` takes text, not a pattern, so only the escapes that name a byte apply.
fn unescape_y(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0usize;
    while let Some(&c) = raw.get(i) {
        i = i.saturating_add(1);
        if c != b'\\' {
            out.push(c);
            continue;
        }
        match raw.get(i).copied() {
            Some(b'n') => out.push(b'\n'),
            Some(b't') => out.push(b'\t'),
            Some(b'r') => out.push(b'\r'),
            Some(b'\\') => out.push(b'\\'),
            Some(other) => out.push(other),
            None => out.push(b'\\'),
        }
        i = i.saturating_add(1);
    }
    out
}

fn parse_replacement(raw: &[u8]) -> Vec<Rep> {
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
        i = i.saturating_add(1);
        match n {
            b'0'..=b'9' => {
                flush(&mut lit, &mut parts);
                parts.push(Rep::Group(usize::from(n.wrapping_sub(b'0'))));
            }
            b'n' => lit.push(b'\n'),
            b't' => lit.push(b'\t'),
            b'r' => lit.push(b'\r'),
            b'a' => lit.push(0x07),
            b'f' => lit.push(0x0c),
            b'v' => lit.push(0x0b),
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
            // `\&`, `\\`, `\<newline>` and anything else: the character itself.
            other => lit.push(other),
        }
    }
    flush(&mut lit, &mut parts);
    parts
}

/// Why a script would not compile.
///
/// `at` is the offset the parser had reached, which is what makes a diagnostic
/// about a long one-liner usable; a failure with no position is one that is not
/// about a place in the text — an unresolved label is about the script's shape.
#[cfg_attr(test, derive(Debug))]
struct ScriptError {
    at: Option<usize>,
    msg: String,
    code: i32,
}

enum ScriptFail {
    Syntax(String),
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
fn compile_script(script: &[u8], ere: bool) -> Result<Script, ScriptError> {
    let mut p = Parser {
        s: script,
        i: 0,
        ere,
        wfiles: Vec::new(),
    };
    match parse_body(&mut p, script) {
        Ok(s) => Ok(s),
        Err(ScriptFail::Syntax(msg)) => Err(ScriptError {
            at: Some(p.i),
            msg,
            code: 1,
        }),
        // GNU reports an unresolvable label after parsing has finished, and
        // gives it its own status. Matching that keeps a script that checks
        // `$?` behaving the same under either sed.
        Err(ScriptFail::Label(msg)) => Err(ScriptError {
            at: None,
            msg,
            code: 4,
        }),
    }
}

fn parse_body(p: &mut Parser<'_>, script: &[u8]) -> Result<Script, ScriptFail> {
    let mut cmds: Vec<Command> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
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
                let start = open.pop().ok_or_else(|| "unexpected `}'".to_string())?;
                let here = cmds.len();
                cmds.push(Command {
                    sel: Sel::Always,
                    negated: false,
                    act: Action::BlockEnd,
                });
                if let Some(c) = cmds.get_mut(start) {
                    c.act = Action::Block(here.saturating_add(1));
                }
                continue;
            }
            Some(_) => {}
        }

        let sel = p.parse_sel()?;
        p.skip_blank();
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
                open.push(here);
                Action::Block(0)
            }
            b'}' => return Err(ScriptFail::Syntax("unexpected `}'".to_string())),
            b':' => {
                let name = p.parse_label();
                if name.is_empty() {
                    return Err(ScriptFail::Syntax("`:' needs a label".to_string()));
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
            b'a' => Action::AppendText(p.parse_text()),
            b'i' => Action::InsertText(p.parse_text()),
            b'c' => Action::ChangeText(p.parse_text()),
            b'r' => Action::ReadFile(p.parse_filename()?),
            b'w' => {
                let path = p.parse_filename()?;
                Action::WriteFile(p.wfile(path))
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
        cmds.push(Command { sel, negated, act });
    }

    if !open.is_empty() {
        return Err(ScriptFail::Syntax("unmatched `{'".to_string()));
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
        suppress,
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
    paths: Vec<String>,
    next_path: usize,
    cur: Option<Box<dyn BufRead>>,
    /// What to call the file now open, in a read-error diagnostic. Upstream
    /// names standard input `stdin` there even though the operand was `-`.
    cur_name: String,
    peeked: Option<Line>,
    sep: u8,
    had_error: bool,
}

impl Input {
    fn new(paths: Vec<String>, sep: u8) -> Input {
        Input {
            paths,
            next_path: 0,
            cur: None,
            cur_name: "stdin".to_string(),
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
                self.cur_name = "stdin".to_string();
                return true;
            }
            match File::open(&path) {
                Ok(f) => {
                    self.cur = Some(Box::new(BufReader::new(f)));
                    self.cur_name = path;
                    return true;
                }
                Err(e) => {
                    diag!("sed: can't read {path}: {}", strerror(&e));
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
                Err(e) => panic_out(&format!(
                    "read error on {}: {}",
                    self.cur_name,
                    strerror(&e)
                )),
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
    Text(Vec<u8>),
    File(String),
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
    suppress: bool,
    had_error: bool,
}

impl<'w> Exec<'w> {
    fn new(script: &Script, suppress: bool, wfiles: &'w mut Vec<WFile>) -> Exec<'w> {
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
            suppress,
            had_error: false,
        }
    }

    fn fail(&mut self, msg: &str) {
        diag!("sed: {msg}");
        self.had_error = true;
    }

    /// Resolve `//` and record what was matched, so the next `//` can find it.
    fn resolve(&mut self, r: Option<&Rc<Regex>>) -> Option<Rc<Regex>> {
        let re = match r {
            Some(x) => Rc::clone(x),
            None => match &self.last_re {
                Some(x) => Rc::clone(x),
                None => {
                    self.fail("no previous regular expression");
                    return None;
                }
            },
        };
        self.last_re = Some(Rc::clone(&re));
        Some(re)
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
            Addr::Re(r) => match self.resolve(r.as_ref()) {
                Some(re) => re.find(&self.pattern)?.is_some(),
                None => false,
            },
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
            EndAddr::Addr(Addr::Re(r)) => match self.resolve(r.as_ref()) {
                Some(re) => re.find(&self.pattern)?.is_some(),
                None => true,
            },
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
                Pending::Text(t) => out.line(&t, true)?,
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
        let Some(re) = self.resolve(sub.re.as_ref()) else {
            return Ok(false);
        };
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
                    return Ok(match self.pattern.iter().position(|&b| b == b'\n') {
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
                    let (bytes, sep) = match self.pattern.iter().position(|&b| b == b'\n') {
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
                            self.pattern.push(b'\n');
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
                    self.hold.push(b'\n');
                    self.hold.extend_from_slice(&self.pattern);
                }
                Action::Get => self.pattern.clone_from(&self.hold),
                Action::GetAppend => {
                    self.pattern.push(b'\n');
                    self.pattern.extend_from_slice(&self.hold);
                }
                Action::Exchange => std::mem::swap(&mut self.pattern, &mut self.hold),
                Action::LineNumber => {
                    let n = self.line_num.to_string();
                    out.line(n.as_bytes(), true)?;
                }
                Action::AppendText(t) => self.appends.push(Pending::Text(t.clone())),
                Action::InsertText(t) => out.line(t, true)?,
                Action::ChangeText(t) => {
                    // For a range, the text replaces the whole range and so is
                    // written once, when the range closes — not once per line.
                    let still_open = matches!(cmd.sel, Sel::Range(_, _))
                        && self.ranges.get(pc).is_some_and(|r| r.active);
                    if !still_open {
                        out.line(t, true)?;
                    }
                    return Ok(Flow::Deleted);
                }
                Action::ReadFile(path) => self.appends.push(Pending::File(path.clone())),
                Action::WriteFile(idx) => {
                    let bytes = self.pattern.clone();
                    self.write_wfile(*idx, &bytes, out)?;
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

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct SedArgs {
    suppress: bool,
    ere: bool,
    separate: bool,
    null_data: bool,
    /// `Some(suffix)` for `-i`; an empty suffix means no backup.
    in_place: Option<String>,
    /// The `-e` fragments and `-f` files, in the order they were given.
    script_parts: Vec<ScriptPart>,
    files: Vec<String>,
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum ScriptPart {
    Text(String),
    File(String),
}

/// Parse sed's argv.
///
/// The first bare argument is the script *only if* no `-e` or `-f` has been
/// given; otherwise every bare argument is a file. That rule is why
/// `sed -e p file` edits `file` rather than looking for a file called `p`.
fn parse_args(args: &[String]) -> Result<SedArgs, String> {
    let mut out = SedArgs::default();
    let mut i = 0usize;
    let mut no_more_options = false;
    let mut have_script = false;

    while let Some(arg) = args.get(i) {
        i = i.saturating_add(1);
        let bytes = arg.as_bytes();

        if no_more_options || bytes.first() != Some(&b'-') || arg == "-" {
            if have_script {
                out.files.push(arg.clone());
            } else {
                out.script_parts.push(ScriptPart::Text(arg.clone()));
                have_script = true;
            }
            continue;
        }

        if arg == "--" {
            no_more_options = true;
            continue;
        }

        if let Some(long) = arg.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (long, None),
            };
            match name {
                "quiet" | "silent" => out.suppress = true,
                "regexp-extended" => out.ere = true,
                "separate" => out.separate = true,
                "null-data" | "zero-terminated" => out.null_data = true,
                "posix" | "unbuffered" | "debug" | "sandbox" | "follow-symlinks" => {}
                "in-place" => {
                    out.in_place = Some(value.unwrap_or("").to_string());
                    out.separate = true;
                }
                "expression" => {
                    let v = value.ok_or_else(|| "--expression needs a script".to_string())?;
                    out.script_parts.push(ScriptPart::Text(v.to_string()));
                    have_script = true;
                }
                "file" => {
                    let v = value.ok_or_else(|| "--file needs a file name".to_string())?;
                    out.script_parts.push(ScriptPart::File(v.to_string()));
                    have_script = true;
                }
                "help" => return Err("help".to_string()),
                other => return Err(format!("unrecognized option '--{other}'")),
            }
            continue;
        }

        // Short options bundle, and `-e`/`-f`/`-i` may carry their value in the
        // same argument: `-ne p`, `-i.bak`, `-eS`.
        let mut j = 1usize;
        while let Some(&c) = bytes.get(j) {
            j = j.saturating_add(1);
            match c {
                b'n' => out.suppress = true,
                b'E' | b'r' => out.ere = true,
                b's' => out.separate = true,
                b'z' => out.null_data = true,
                b'u' => {}
                b'i' => {
                    // Unlike `-e`, `-i` never takes the *next* argument: GNU
                    // reads `-i backup` as an in-place edit of `backup`.
                    let suffix = String::from_utf8_lossy(bytes.get(j..).unwrap_or_default());
                    out.in_place = Some(suffix.into_owned());
                    out.separate = true;
                    j = bytes.len();
                }
                b'e' | b'f' => {
                    let rest = bytes.get(j..).unwrap_or_default();
                    let value = if rest.is_empty() {
                        let v = args
                            .get(i)
                            .ok_or_else(|| format!("option -{} requires an argument", c as char))?
                            .clone();
                        i = i.saturating_add(1);
                        v
                    } else {
                        String::from_utf8_lossy(rest).into_owned()
                    };
                    j = bytes.len();
                    out.script_parts.push(if c == b'e' {
                        ScriptPart::Text(value)
                    } else {
                        ScriptPart::File(value)
                    });
                    have_script = true;
                }
                other => return Err(format!("invalid option -- '{}'", other.escape_ascii())),
            }
        }
    }

    Ok(out)
}

const USAGE: &str = "\
Usage: sed [OPTION]... {script} [input-file]...

  -n, --quiet, --silent    suppress automatic printing of pattern space
  -e SCRIPT                add SCRIPT to the commands to be executed
  -f FILE                  add the contents of FILE to the commands
  -i[SUFFIX]               edit files in place (making a backup if SUFFIX)
  -E, -r                   use extended regular expressions
  -s                       consider files as separate rather than one stream
  -z                       separate lines by NUL characters
  -u                       accepted and ignored
      --help               display this help and exit";

/// Gather the script text from `-e` fragments and `-f` files.
fn collect_script(parts: &[ScriptPart]) -> Result<Vec<u8>, String> {
    let mut script: Vec<u8> = Vec::new();
    for part in parts {
        if !script.is_empty() {
            script.push(b'\n');
        }
        match part {
            ScriptPart::Text(t) => script.extend_from_slice(t.as_bytes()),
            ScriptPart::File(path) => {
                let bytes = if path == "-" {
                    let mut b = Vec::new();
                    io::stdin()
                        .read_to_end(&mut b)
                        .map(|_| b)
                        .map_err(|e| format!("couldn't read -: {}", strerror(&e)))?
                } else {
                    fs::read(path)
                        .map_err(|e| format!("couldn't open file {path}: {}", strerror(&e)))?
                };
                script.extend_from_slice(&bytes);
            }
        }
    }
    Ok(script)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) if e == "help" => {
            println!("{USAGE}");
            process::exit(0);
        }
        Err(e) => {
            diag!("sed: {e}");
            diag!("{USAGE}");
            process::exit(1);
        }
    };

    if parsed.script_parts.is_empty() {
        diag!("sed: no script specified");
        diag!("{USAGE}");
        process::exit(1);
    }

    let script_text = match collect_script(&parsed.script_parts) {
        Ok(s) => s,
        // Not status 1: the command line was well formed, and a `-f` file that
        // will not open is an I/O failure like any other.
        Err(e) => panic_out(&e),
    };

    let script = match compile_script(&script_text, parsed.ere) {
        Ok(s) => s,
        Err(e) => {
            match e.at {
                Some(at) => diag!("sed: -e expression #1, char {at}: {}", e.msg),
                None => diag!("sed: {}", e.msg),
            }
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
        files.push("-".to_string());
    }

    let mut job = Job {
        script: &script,
        // Opened before a line is read, and once for the whole run.
        wfiles: open_wfiles(&script.wfiles),
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
    suppress: bool,
    sep: u8,
}

impl Job<'_> {
    /// Wire one `Input` to one `Out` and run the script over it.
    fn run_one(&mut self, input: &mut Input, sink: &mut dyn Write) -> (Option<i32>, bool) {
        let mut exec = Exec::new(self.script, self.suppress, &mut self.wfiles);
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
        };
        (quit, exec.had_error)
    }

    /// All the files as one stream: line numbers and `$` run across them.
    fn joined(&mut self, files: &[String]) -> i32 {
        let stdout = io::stdout();
        let mut sink = stdout.lock();
        let mut input = Input::new(files.to_vec(), self.sep);
        let (quit, exec_err) = self.run_one(&mut input, &mut sink);
        let _ = sink.flush();
        status(quit, input.had_error || exec_err)
    }

    /// `-s`: each file starts again at line 1, and each has its own last line.
    fn separate(&mut self, files: &[String]) -> i32 {
        let stdout = io::stdout();
        let mut sink = stdout.lock();
        let mut bad = false;
        for path in files {
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
    fn in_place(&mut self, files: &[String], suffix: &str) -> i32 {
        let mut bad = false;
        for path in files {
            // A `-` operand is a file named `-`, not standard input: there is
            // no way to rewrite a stream in place, so it gets no special case
            // and fails as any missing file would.
            match File::open(path) {
                Err(e) => {
                    diag!("sed: can't read {path}: {}", strerror(&e));
                    bad = true;
                    continue;
                }
                // A directory opens happily and then fails to read, which
                // would end the run with a confusing "read error". Refusing
                // anything that is not a regular file names the real problem.
                Ok(f) => match f.metadata() {
                    Ok(m) if !m.is_file() => {
                        panic_out(&format!("couldn't edit {path}: not a regular file"));
                    }
                    Ok(_) => {}
                    Err(e) => {
                        diag!("sed: can't read {path}: {}", strerror(&e));
                        bad = true;
                        continue;
                    }
                },
            }
            let mut buf: Vec<u8> = Vec::new();
            let mut input = Input::new(vec![path.clone()], self.sep);
            let (quit, exec_err) = self.run_one(&mut input, &mut buf);
            bad = bad || input.had_error || exec_err;

            if !suffix.is_empty() {
                // GNU puts the file name where a `*` is, and appends otherwise.
                let backup = if suffix.contains('*') {
                    suffix.replace('*', path)
                } else {
                    format!("{path}{suffix}")
                };
                if let Err(e) = fs::copy(path, &backup) {
                    diag!("sed: cannot back up {path}: {}", strerror(&e));
                    bad = true;
                    continue;
                }
            }
            if let Err(e) = fs::write(path, &buf) {
                diag!("sed: couldn't write {path}: {}", strerror(&e));
                bad = true;
            }
            if let Some(code) = quit {
                return status(Some(code), bad);
            }
        }
        status(None, bad)
    }
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

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    /// Run a script over some text and return what it wrote.
    fn run(script: &str, input: &str) -> String {
        run_opts(script, input, false, false)
    }

    fn run_opts(script: &str, input: &str, suppress: bool, ere: bool) -> String {
        let compiled = compile_script(script.as_bytes(), ere)
            .unwrap_or_else(|e| panic!("compiling {script:?}: {}", e.msg));
        let mut sink: Vec<u8> = Vec::new();
        let mut inp = Input {
            paths: Vec::new(),
            next_path: 0,
            cur_name: "stdin".to_string(),
            cur: Some(Box::new(BufReader::new(io::Cursor::new(
                input.as_bytes().to_vec(),
            )))),
            peeked: None,
            sep: b'\n',
            had_error: false,
        };
        let mut job = Job {
            script: &compiled,
            wfiles: open_wfiles(&compiled.wfiles),
            suppress: suppress || compiled.suppress,
            sep: b'\n',
        };
        job.run_one(&mut inp, &mut sink);
        String::from_utf8_lossy(&sink).into_owned()
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
        let compiled = compile_script(pat.as_bytes(), false).unwrap();
        let line = format!("{}\n", "a".repeat(300));
        let mut sink: Vec<u8> = Vec::new();
        let mut inp = Input {
            paths: Vec::new(),
            next_path: 0,
            cur_name: "stdin".to_string(),
            cur: Some(Box::new(BufReader::new(io::Cursor::new(line.into_bytes())))),
            peeked: None,
            sep: b'\n',
            had_error: false,
        };
        let mut job = Job {
            script: &compiled,
            wfiles: Vec::new(),
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
        let compiled = compile_script(b"s/b/B/", false).unwrap();
        let raw: Vec<u8> = vec![0xff, b'a', b'b', 0xfe, b'\n'];
        let mut sink: Vec<u8> = Vec::new();
        let mut inp = Input {
            paths: Vec::new(),
            next_path: 0,
            cur_name: "stdin".to_string(),
            cur: Some(Box::new(BufReader::new(io::Cursor::new(raw)))),
            peeked: None,
            sep: b'\n',
            had_error: false,
        };
        let mut job = Job {
            script: &compiled,
            wfiles: Vec::new(),
            suppress: false,
            sep: b'\n',
        };
        job.run_one(&mut inp, &mut sink);
        assert_eq!(sink, vec![0xff, b'a', b'B', 0xfe, b'\n']);
    }

    // ---------------- compile errors ----------------

    #[test]
    fn a_bad_script_is_refused_rather_than_guessed_at() {
        assert!(compile_script(b"Z", false).is_err());
        assert!(compile_script(b"s/a", false).is_err());
        assert!(compile_script(b"{p", false).is_err());
        assert!(compile_script(b"p}", false).is_err());
        assert!(compile_script(b"b nowhere", false).is_err());
        assert!(compile_script(b"y/ab/x/", false).is_err());
        assert!(compile_script(b"s/[a/x/", false).is_err());
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
                compile_script(script.as_bytes(), false).is_ok(),
                "{script} should compile"
            );
        }
    }

    // ---------------- parse_args ----------------

    #[test]
    fn the_first_bare_argument_is_the_script_unless_e_gave_one() {
        let a = parse_args(&s(&["s/a/b/", "in.txt"])).unwrap();
        assert_eq!(a.script_parts, vec![ScriptPart::Text("s/a/b/".to_string())]);
        assert_eq!(a.files, vec!["in.txt"]);

        let a = parse_args(&s(&["-e", "p", "in.txt"])).unwrap();
        assert_eq!(a.script_parts, vec![ScriptPart::Text("p".to_string())]);
        assert_eq!(a.files, vec!["in.txt"]);
    }

    #[test]
    fn short_options_bundle() {
        let a = parse_args(&s(&["-ne", "p", "f"])).unwrap();
        assert!(a.suppress);
        assert_eq!(a.script_parts, vec![ScriptPart::Text("p".to_string())]);
        assert_eq!(a.files, vec!["f"]);
    }

    #[test]
    fn a_value_may_be_attached_to_its_option() {
        let a = parse_args(&s(&["-ep", "f"])).unwrap();
        assert_eq!(a.script_parts, vec![ScriptPart::Text("p".to_string())]);
        assert_eq!(a.files, vec!["f"]);
    }

    #[test]
    fn in_place_takes_its_suffix_attached_and_never_the_next_argument() {
        let a = parse_args(&s(&["-i.bak", "s/a/b/", "f"])).unwrap();
        assert_eq!(a.in_place, Some(".bak".to_string()));
        assert!(a.separate);
        assert_eq!(a.files, vec!["f"]);

        // `-i backup` edits `backup`; it does not name a suffix.
        let a = parse_args(&s(&["-i", "s/a/b/", "f"])).unwrap();
        assert_eq!(a.in_place, Some(String::new()));
        assert_eq!(a.files, vec!["f"]);
    }

    #[test]
    fn extended_regular_expressions_have_two_spellings() {
        assert!(parse_args(&s(&["-E", "p"])).unwrap().ere);
        assert!(parse_args(&s(&["-r", "p"])).unwrap().ere);
        assert!(parse_args(&s(&["--regexp-extended", "p"])).unwrap().ere);
    }

    #[test]
    fn long_options_may_carry_a_value() {
        let a = parse_args(&s(&["--expression=p", "--in-place=.bak", "f"])).unwrap();
        assert_eq!(a.script_parts, vec![ScriptPart::Text("p".to_string())]);
        assert_eq!(a.in_place, Some(".bak".to_string()));
        assert_eq!(a.files, vec!["f"]);
    }

    #[test]
    fn a_lone_dash_is_a_file_not_an_option() {
        let a = parse_args(&s(&["p", "-"])).unwrap();
        assert_eq!(a.files, vec!["-"]);
    }

    #[test]
    fn double_dash_ends_the_options() {
        let a = parse_args(&s(&["-n", "--", "p", "-weird"])).unwrap();
        assert!(a.suppress);
        assert_eq!(a.files, vec!["-weird"]);
    }

    #[test]
    fn an_unknown_option_is_reported() {
        assert!(parse_args(&s(&["-Z", "p"])).is_err());
        assert!(parse_args(&s(&["--nope"])).is_err());
        assert!(parse_args(&s(&["-e"])).is_err());
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
}
