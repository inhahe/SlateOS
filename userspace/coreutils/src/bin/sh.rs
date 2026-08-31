//! sh — a small POSIX shell.
//!
//! ```text
//! sh [-abCefnuvx] [-c COMMAND [NAME [ARG...]] | -s [ARG...] | SCRIPT [ARG...]]
//! ```
//!
//! # What this is, and what it is not
//!
//! This is the `sh` that ships: what an init script, a `Makefile` recipe and a
//! C `system()` call all end up running. It is deliberately *small* — the
//! bash-superset shell in this tree is `userspace/oils` (`osh`, 150k lines),
//! and `design-decisions.md` §72 fixes the division: `osh` grows towards bash,
//! this file stays a POSIX baseline.
//!
//! A baseline still has to be a shell, though, and until 2026-08-30 this one
//! was not. It had no lexer and no parser: `execute_script` split the text into
//! *lines* and looked for `if `/`while `/`for ` with `str::starts_with`, so
//! `if true; then echo yes; fi` — one line — tried to execute a program named
//! `true;`. `case`, `until`, `{ }`, `( )`, `!`, here-documents, backticks,
//! `$(( ))`, globbing, `$@` and every `${name:-word}` form were simply absent;
//! `yes | head -2` deadlocked, because the pipeline waited for stage *n* before
//! spawning stage *n+1*; and every word was copied byte-by-byte through
//! `push(b as char)` **twice**, so `echo héllo` printed six bytes where two
//! were written. See `known-issues.md` → `TD-B-sh-IS-NOT-A-SHELL` for the
//! measured table. This file is the rewrite.
//!
//! # Bytes, not text
//!
//! Nothing here is a `String`. A shell handles file names, environment values
//! and command output, and our filesystem permits every byte but `/` and NUL —
//! so a shell that decodes to UTF-8 either panics or corrupts, and the old one
//! did both. Words are `Vec<u8>` from `args_os` to the `exec`, and the only
//! `OsString` in the executor is the one [`Command`] insists on, built by
//! [`os_from_bytes`], which is a cast and not a decode.
//!
//! # Shape
//!
//! The four classical stages, in the order POSIX defines them:
//!
//! | stage | what it produces |
//! |---|---|
//! | [`Lexer`] | tokens; a word arrives as a `Vec<Piece>` that has already recorded which of its bytes were quoted |
//! | [`Parser`] | a [`List`] — pipelines, compound commands, function definitions, redirections |
//! | [`Shell::expand_word`] | tilde, parameter, command and arithmetic substitution; then field splitting; then pathname expansion; then quote removal |
//! | [`Shell::run_list`] | execution, including the fd plumbing that makes a pipeline concurrent |
//!
//! The quoting flag has to be carried *through* expansion rather than resolved
//! in the lexer, and that is the one non-obvious thing about the data model.
//! `$x` splits into fields, `"$x"` does not; `*` is a pattern, `"*"` is a file
//! named `*`. Both distinctions are decided **after** substitution, on text the
//! lexer never saw — so [`Expanded`] carries one `quoted` bit per byte, and
//! field splitting and globbing read it.
//!
//! # Deliberately absent
//!
//! `trap` and job control (both need signal handling this crate does not
//! have), aliases, `getopts`, `times`, `readonly`, `local`, arrays, `select`,
//! process substitution, brace expansion (dash has none either — `echo {a,b}`
//! prints `{a,b}`), and `test`/`[` as builtins: the `test` binary beside this
//! one is the same program, so a script gets the right answer through `PATH`.
//! Each is a documented gap rather than an accident; `osh` has them all.
//!
//! # Where it still differs from dash, and why
//!
//! `scripts/sh-diff.sh` runs both shells over ~225 cases and compares stdout,
//! status, stderr and the files left on disk byte for byte. Everything passes
//! except the list above and two wordings we prefer (`echo $(( ))` is 0 here,
//! as in bash and ksh, where dash calls it a syntax error; a failed `cd` names
//! the errno the way bash does). What the harness *cannot* see, because no case
//! can reach it from a script, is the shape of three things below:
//!
//! * **`&` backgrounds only a single external command.** There is no `fork`, so
//!   a builtin, a function or a compound after `&` runs in the foreground and
//!   then reports 0. [`Shell::run_background`] draws the line.
//! * **Descriptors above 2 reach an external child on unix only.** [`Command`]
//!   names three; the rest are installed between fork and exec by
//!   [`extra_fds`], which has no Windows counterpart. In-process builtins see
//!   them everywhere, since they read the [`Io`] table directly.
//! * **`n>&-` gives a child the null device, not a closed descriptor.** See
//!   [`stdio_for`]: a child that writes to it is discarded rather than getting
//!   `EBADF`. Visible only to a program that tests for the error.

// POSIX's `strerror` text, not the host's: `Display` on an `io::Error` gives
// `The file exists. (os error 80)` on a Windows host and `File exists` on
// SlateOS, and a shell's diagnostics are an interface — a script that greps
// them, and a differential harness that compares them, both read the words.
use coreutils::errmsg::strerror;
use coreutils::fnmatch;
use coreutils::quote::quotef_os;
use coreutils::stdfd;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Write};
use std::process::{Command, ExitCode, Stdio};
use std::rc::Rc;

// ---------------------------------------------------------------------------
// bytes
// ---------------------------------------------------------------------------

/// An `OsString` over `bytes`, without decoding them.
///
/// The whole point of the byte discipline lives in this function's `#[cfg]`: on
/// a unix target it is `OsStrExt::from_bytes`, a cast, and every byte survives.
/// On the Windows build — which exists only so the unit tests can run on the
/// development host — there is no such cast, because Windows paths are UTF-16
/// and an arbitrary byte string is not one. The lossy decode there is a
/// host-only compromise; the shipped shell never takes it.
fn os_from_bytes(bytes: &[u8]) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        OsStr::from_bytes(bytes).to_os_string()
    }
    #[cfg(not(unix))]
    {
        OsString::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// The bytes of an `OsStr`, without decoding them.
fn bytes_of_os(s: &OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        s.as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        s.to_string_lossy().into_owned().into_bytes()
    }
}

/// `bytes` as a decimal integer, or `None`.
fn parse_int(bytes: &[u8]) -> Option<i64> {
    std::str::from_utf8(bytes).ok()?.trim().parse().ok()
}

/// Is `b` a byte a variable name may contain after the first?
fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Is `name` a valid shell variable name?
fn is_name(name: &[u8]) -> bool {
    match name.first() {
        None => false,
        Some(&f) => (f.is_ascii_alphabetic() || f == b'_') && name.iter().all(|&b| is_name_byte(b)),
    }
}

// ---------------------------------------------------------------------------
// words
// ---------------------------------------------------------------------------

/// One piece of a word, in the order it was written.
///
/// A word is a list of these rather than a byte string because the two things
/// expansion has to know about a byte — was it quoted, and is it a
/// substitution — are properties of *where it came from*, and are gone the
/// moment the word is flattened. The old implementation flattened first and
/// then tried to recover the quoting by re-scanning, which is what made
/// `echo "hi $X" tail` print its tail twice.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    /// Bytes written without quotes: pattern text for globbing, and subject to
    /// field splitting.
    Lit(Vec<u8>),
    /// Bytes that stood inside `'…'` or `"…"`, or were escaped with `\`. Never
    /// a pattern, never split — but still a field, which is what makes `""` an
    /// empty argument rather than no argument at all.
    Quo(Vec<u8>),
    /// `$name` or `${…}`. `dq` records whether it stood inside double quotes.
    Param { spec: Box<ParamSpec>, dq: bool },
    /// `$(…)` or `` `…` ``.
    Cmd { body: Vec<u8>, dq: bool },
    /// `$((…))`.
    Arith { body: Vec<u8>, dq: bool },
    /// A leading `~` or `~name`. The lexer recognises it only at the start of a
    /// word, and after an unquoted `:` in an assignment, because those are the
    /// only places POSIX gives it a meaning.
    Tilde(Vec<u8>),
}

/// A word: what the lexer produces and what expansion consumes.
type Word = Vec<Piece>;

/// The inside of a `${…}`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ParamSpec {
    /// `x`, `1`, `@`, `*`, `#`, `?`, `-`, `$`, `!`.
    name: Vec<u8>,
    /// `${#x}` — the *length* of the value rather than the value.
    len: bool,
    /// `${x:-word}` and its relatives.
    op: Option<(ParamOp, Word)>,
}

/// The operator in a `${name<op>word}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParamOp {
    /// `:-` / `-`. The flag is the colon: with it, an *empty* value counts as
    /// unset too.
    Default(bool),
    /// `:=` / `=`.
    Assign(bool),
    /// `:?` / `?`.
    Error(bool),
    /// `:+` / `+`.
    Alt(bool),
    /// `#` / `##` — remove a matching prefix. The flag is "longest".
    Prefix(bool),
    /// `%` / `%%` — remove a matching suffix. The flag is "longest".
    Suffix(bool),
}

// ---------------------------------------------------------------------------
// the syntax tree
// ---------------------------------------------------------------------------

/// `a; b & c` — the whole of a script, and the body of every compound command.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct List {
    /// Each and-or list, and whether it was terminated by `&`.
    items: Vec<(AndOr, bool)>,
}

/// `cmd1 && cmd2 || cmd3` — a pipeline and the pipelines chained onto it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct AndOr {
    first: Pipeline,
    /// `true` for `&&`, `false` for `||`.
    rest: Vec<(bool, Pipeline)>,
}

/// `a | b | c`, optionally negated with `!`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Pipeline {
    bang: bool,
    cmds: Vec<Cmd>,
}

/// One command, of whatever kind, with the redirections written on it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Cmd {
    kind: CmdKind,
    redirs: Vec<Redir>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CmdKind {
    /// `VAR=x cmd arg…`. Either half may be empty: a bare `VAR=x` assigns to
    /// the shell, and a bare `> f` is a command with neither.
    Simple {
        assigns: Vec<(Vec<u8>, Word)>,
        words: Vec<Word>,
    },
    /// `( … )` — runs where a `cd` or an assignment does not escape.
    Subshell(Box<List>),
    /// `{ …; }` — runs here.
    Group(Box<List>),
    /// `if … then … elif … then … else … fi`.
    If {
        arms: Vec<(List, List)>,
        otherwise: Option<Box<List>>,
    },
    /// `while`/`until … do … done`. The flag is `until`.
    Loop {
        until: bool,
        cond: Box<List>,
        body: Box<List>,
    },
    /// `for name [in word…]; do … done`. `words` is `None` for the `in`-less
    /// form, which iterates the positional parameters — not the same as an
    /// empty list, which iterates nothing.
    For {
        var: Vec<u8>,
        words: Option<Vec<Word>>,
        body: Box<List>,
    },
    /// `case word in pat|pat) … ;; esac`.
    Case {
        word: Word,
        arms: Vec<(Vec<Word>, List)>,
    },
    /// `name() compound`.
    FuncDef { name: Vec<u8>, body: Rc<Cmd> },
}

/// One redirection.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Redir {
    /// The descriptor being redirected; `None` means the operator's own default
    /// — 0 for input, 1 for output.
    fd: Option<i32>,
    op: RedirOp,
    /// A file name, a descriptor number, or a here-document delimiter — which
    /// of the three is decided by `op`.
    target: Word,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RedirOp {
    /// `<`
    Read,
    /// `>` and `>|`. They differ only under `set -C`.
    Write { clobber: bool },
    /// `>>`
    Append,
    /// `<>`
    ReadWrite,
    /// `<&`
    DupIn,
    /// `>&`
    DupOut,
    /// `<<` and `<<-`.
    ///
    /// The body is not here yet when the parser builds this node: it starts at
    /// the *next* newline, which the parser has not reached. So the cell is
    /// shared with the lexer, which fills it in when it gets there. That is why
    /// this one field is an `Rc<RefCell<…>>` and nothing else in the tree is —
    /// the alternative is walking the finished tree afterwards looking for
    /// here-documents to patch, which is the same aliasing with the sharing
    /// hidden.
    ///
    /// `expand` records whether the delimiter was quoted, which is what decides
    /// whether the body is expanded. `<<-`'s tab stripping is done by the lexer
    /// as it reads, so it leaves no trace here.
    Here { expand: bool, body: HereBody },
}

/// The body of a here-document, filled in after the node that holds it is
/// built. See [`RedirOp::Here`].
type HereBody = Rc<RefCell<Vec<u8>>>;

// ---------------------------------------------------------------------------
// the lexer
// ---------------------------------------------------------------------------

/// A shell operator token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Semi,
    DSemi,
    Amp,
    AndIf,
    OrIf,
    Pipe,
    LParen,
    RParen,
    Less,
    Great,
    DGreat,
    DLess,
    DLessDash,
    LessAnd,
    GreatAnd,
    LessGreat,
    Clobber,
}

impl Op {
    /// How the operator is spelled, for a syntax error that has to quote it.
    fn text(self) -> &'static str {
        match self {
            Op::Semi => ";",
            Op::DSemi => ";;",
            Op::Amp => "&",
            Op::AndIf => "&&",
            Op::OrIf => "||",
            Op::Pipe => "|",
            Op::LParen => "(",
            Op::RParen => ")",
            Op::Less => "<",
            Op::Great => ">",
            Op::DGreat => ">>",
            Op::DLess => "<<",
            Op::DLessDash => "<<-",
            Op::LessAnd => "<&",
            Op::GreatAnd => ">&",
            Op::LessGreat => "<>",
            Op::Clobber => ">|",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Word(Word),
    /// The digits written immediately before a redirection operator, as in
    /// `2>err`. A digit run followed by anything else is an ordinary word.
    IoNumber(i32),
    Op(Op),
    Newline,
    Eof,
}

/// What went wrong, and — the part that matters — whether more input would fix
/// it.
///
/// The interactive loop reads one line at a time and re-parses the whole
/// accumulation after each, so it needs to tell "the quote is still open, keep
/// reading" from "this can never parse". Every unterminated construct answers
/// [`ParseErr::Incomplete`]; everything else is fatal and gets a diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ParseErr {
    Incomplete,
    Syntax(String),
}

/// A here-document whose delimiter has been read and whose body has not.
struct PendingHere {
    delim: Vec<u8>,
    strip: bool,
    body: HereBody,
}

/// Turns shell source into tokens.
///
/// Words come out as [`Piece`] lists rather than as raw text, because the
/// quoting has to be recorded where it is *seen*. Recovering it later from a
/// flattened byte string is the mistake the previous implementation made, and
/// it is not recoverable: by then `'$x'` and `$x` look alike.
struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    /// Here-documents waiting for the newline that starts their bodies.
    pending: Vec<PendingHere>,
    /// Set when a word is being read for a nested context (`${x:-…}`), where
    /// operators and blanks are ordinary text and only the end terminates it.
    word_only: bool,
    /// Set for the inside of a `$(( … ))`, where `~` is bitwise complement and
    /// not a home directory.
    tilde_off: bool,
}

/// Accumulates a word, keeping literal and quoted runs apart.
///
/// Two buffers rather than one because the difference between them is the
/// difference between a glob and a file name. Pushing a byte with the wrong
/// flag flushes the other buffer first, so the pieces come out in written
/// order.
#[derive(Default)]
struct WordBuf {
    pieces: Word,
    lit: Vec<u8>,
    quo: Vec<u8>,
}

impl WordBuf {
    fn flush(&mut self) {
        if !self.lit.is_empty() {
            self.pieces.push(Piece::Lit(std::mem::take(&mut self.lit)));
        }
        if !self.quo.is_empty() {
            self.pieces.push(Piece::Quo(std::mem::take(&mut self.quo)));
        }
    }

    fn push(&mut self, b: u8, quoted: bool) {
        if quoted {
            if !self.lit.is_empty() {
                self.pieces.push(Piece::Lit(std::mem::take(&mut self.lit)));
            }
            self.quo.push(b);
        } else {
            if !self.quo.is_empty() {
                self.pieces.push(Piece::Quo(std::mem::take(&mut self.quo)));
            }
            self.lit.push(b);
        }
    }

    fn piece(&mut self, p: Piece) {
        self.flush();
        self.pieces.push(p);
    }

    /// Record that a quoted section closed.
    ///
    /// A quoted section that contributed no bytes still contributes a *field*:
    /// `cmd ''` passes one empty argument, not none. Nothing else in the word
    /// can express that, because an empty `Piece::Quo` is exactly what an empty
    /// `lit`/`quo` buffer declines to push — so it is pushed here instead.
    fn note_quotes(&mut self) {
        if self.is_empty() {
            self.pieces.push(Piece::Quo(Vec::new()));
        }
    }

    /// True when nothing at all has been added — which is how the caller tells
    /// a word from the absence of one, and is not the same as producing no
    /// pieces: `''` produces none and is a word.
    fn is_empty(&self) -> bool {
        self.pieces.is_empty() && self.lit.is_empty() && self.quo.is_empty()
    }

    fn finish(mut self) -> Word {
        self.flush();
        self.pieces
    }
}

/// Is `b` one of the bytes that can begin an operator?
fn is_op_start(b: u8) -> bool {
    matches!(b, b';' | b'&' | b'|' | b'(' | b')' | b'<' | b'>')
}

impl<'a> Lexer<'a> {
    fn new(src: &'a [u8]) -> Self {
        Lexer {
            src,
            pos: 0,
            pending: Vec::new(),
            word_only: false,
            tilde_off: false,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn at(&self, off: usize) -> Option<u8> {
        self.src.get(self.pos.saturating_add(off)).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek();
        if b.is_some() {
            self.pos = self.pos.saturating_add(1);
        }
        b
    }

    /// Skip blanks, comments and `\`-newline line continuations.
    fn skip_blanks(&mut self) {
        loop {
            match self.peek() {
                Some(b' ' | b'\t') => {
                    self.pos = self.pos.saturating_add(1);
                }
                Some(b'\\') if self.at(1) == Some(b'\n') => {
                    self.pos = self.pos.saturating_add(2);
                }
                Some(b'#') => {
                    while !matches!(self.peek(), None | Some(b'\n')) {
                        self.pos = self.pos.saturating_add(1);
                    }
                }
                _ => return,
            }
        }
    }

    fn next_token(&mut self) -> Result<Tok, ParseErr> {
        self.skip_blanks();
        let Some(b) = self.peek() else {
            // Running out with a here-document still owed is not a syntax
            // error — the body is simply empty — but it *is* incomplete input,
            // which is what lets the interactive loop keep reading.
            if self.pending.is_empty() {
                return Ok(Tok::Eof);
            }
            self.read_heredocs()?;
            return Ok(Tok::Eof);
        };
        if b == b'\n' {
            self.pos = self.pos.saturating_add(1);
            self.read_heredocs()?;
            return Ok(Tok::Newline);
        }
        if is_op_start(b) {
            return Ok(Tok::Op(self.read_op()));
        }
        // An IO number is a digit run *immediately* followed by a redirection
        // operator: `2>f` redirects, `2 >f` runs the command `2`.
        let mut i = self.pos;
        while matches!(self.src.get(i), Some(d) if d.is_ascii_digit()) {
            i = i.saturating_add(1);
        }
        if i > self.pos
            && matches!(self.src.get(i), Some(b'<' | b'>'))
            && let Some(n) = self
                .src
                .get(self.pos..i)
                .and_then(parse_int)
                .and_then(|n| i32::try_from(n).ok())
        {
            self.pos = i;
            return Ok(Tok::IoNumber(n));
        }
        let w = self.read_word()?;
        Ok(Tok::Word(w))
    }

    fn read_op(&mut self) -> Op {
        let two = (self.peek(), self.at(1));
        let (op, len) = match two {
            (Some(b';'), Some(b';')) => (Op::DSemi, 2),
            (Some(b';'), _) => (Op::Semi, 1),
            (Some(b'&'), Some(b'&')) => (Op::AndIf, 2),
            (Some(b'&'), _) => (Op::Amp, 1),
            (Some(b'|'), Some(b'|')) => (Op::OrIf, 2),
            (Some(b'|'), _) => (Op::Pipe, 1),
            (Some(b'('), _) => (Op::LParen, 1),
            (Some(b')'), _) => (Op::RParen, 1),
            (Some(b'<'), Some(b'<')) => {
                if self.at(2) == Some(b'-') {
                    (Op::DLessDash, 3)
                } else {
                    (Op::DLess, 2)
                }
            }
            (Some(b'<'), Some(b'&')) => (Op::LessAnd, 2),
            (Some(b'<'), Some(b'>')) => (Op::LessGreat, 2),
            (Some(b'<'), _) => (Op::Less, 1),
            (Some(b'>'), Some(b'>')) => (Op::DGreat, 2),
            (Some(b'>'), Some(b'&')) => (Op::GreatAnd, 2),
            (Some(b'>'), Some(b'|')) => (Op::Clobber, 2),
            _ => (Op::Great, 1),
        };
        self.pos = self.pos.saturating_add(len);
        op
    }

    /// Queue a here-document; its body is read at the next newline.
    fn queue_here(&mut self, delim: Vec<u8>, strip: bool, body: &HereBody) {
        self.pending.push(PendingHere {
            delim,
            strip,
            body: Rc::clone(body),
        });
    }

    /// Read the bodies of every queued here-document, in order.
    ///
    /// Called from the point *after* a newline has been consumed, which is
    /// where POSIX says a here-document starts — not after the redirection
    /// operator. Two on one line therefore read in the order they were written:
    /// `cat <<A <<B` takes A's body first.
    fn read_heredocs(&mut self) -> Result<(), ParseErr> {
        let pending = std::mem::take(&mut self.pending);
        for h in pending {
            let mut body = Vec::new();
            loop {
                let start = self.pos;
                while !matches!(self.peek(), None | Some(b'\n')) {
                    self.pos = self.pos.saturating_add(1);
                }
                let mut line = self.src.get(start..self.pos).unwrap_or_default();
                let had_newline = self.peek() == Some(b'\n');
                if had_newline {
                    self.pos = self.pos.saturating_add(1);
                }
                if h.strip {
                    while let Some((b'\t', rest)) = line.split_first() {
                        line = rest;
                    }
                }
                if line == h.delim.as_slice() {
                    break;
                }
                if !had_newline && line.is_empty() {
                    // End of input with the delimiter never seen. POSIX leaves
                    // this unspecified; dash takes what it has, and so do we.
                    // The `Incomplete` is for the interactive loop, which will
                    // re-parse with the next line appended.
                    *h.body.borrow_mut() = body;
                    return Err(ParseErr::Incomplete);
                }
                body.extend_from_slice(line);
                body.push(b'\n');
                if !had_newline {
                    *h.body.borrow_mut() = body;
                    return Err(ParseErr::Incomplete);
                }
            }
            *h.body.borrow_mut() = body;
        }
        Ok(())
    }
}

impl Lexer<'_> {
    /// Does the word starting here begin `name=`?
    ///
    /// Only assignments give `~` a meaning after a `:`, which is what makes
    /// `PATH=~/bin:~/opt` expand both tildes while `echo a:~/b` expands none.
    fn at_assignment(&self) -> bool {
        let mut i = self.pos;
        match self.src.get(i) {
            Some(&b) if b.is_ascii_alphabetic() || b == b'_' => {}
            _ => return false,
        }
        while matches!(self.src.get(i), Some(&b) if is_name_byte(b)) {
            i = i.saturating_add(1);
        }
        self.src.get(i) == Some(&b'=')
    }

    /// Read one word.
    ///
    /// The loop is the shell's quoting state machine, and every branch of it
    /// decides one thing: does this byte reach the word as *data* (quoted), as
    /// *pattern* (literal), or as a request to substitute something. Getting
    /// that distinction into the [`Piece`] list here is the whole reason the
    /// lexer exists; by the time a word is a byte string it is too late to ask.
    fn read_word(&mut self) -> Result<Word, ParseErr> {
        let mut buf = WordBuf::default();
        let assign = self.at_assignment();
        let mut seen_eq = false;
        let mut tilde_ok = true;
        loop {
            if tilde_ok && !self.tilde_off && self.peek() == Some(b'~') {
                self.read_tilde(&mut buf);
            }
            tilde_ok = false;
            let Some(b) = self.peek() else { break };
            // In `word_only` mode — the inside of a `${x:-…}` — blanks and
            // operators are ordinary text and only the end of input stops us.
            if !self.word_only && (b == b'\n' || b == b' ' || b == b'\t' || is_op_start(b)) {
                break;
            }
            self.pos = self.pos.saturating_add(1);
            match b {
                b'\\' => match self.bump() {
                    // A backslash at the very end of the input is itself.
                    None => buf.push(b'\\', true),
                    // `\`-newline is a line continuation and vanishes.
                    Some(b'\n') => {}
                    Some(c) => buf.push(c, true),
                },
                b'\'' => {
                    loop {
                        match self.bump() {
                            None => return Err(ParseErr::Incomplete),
                            Some(b'\'') => break,
                            Some(c) => buf.push(c, true),
                        }
                    }
                    buf.note_quotes();
                }
                b'"' => {
                    self.read_dquote(&mut buf)?;
                    buf.note_quotes();
                }
                b'$' => self.read_dollar(&mut buf, false)?,
                b'`' => {
                    let body = self.read_backtick()?;
                    buf.piece(Piece::Cmd { body, dq: false });
                }
                // `PATH=~/bin:~/opt` expands both tildes: an assignment gives
                // `~` a meaning again after its `=` and after every unquoted
                // `:`, which is the one place a tilde is special mid-word.
                b'=' if assign && !seen_eq => {
                    seen_eq = true;
                    buf.push(b'=', false);
                    tilde_ok = true;
                }
                b':' if assign && seen_eq => {
                    buf.push(b':', false);
                    tilde_ok = true;
                }
                c => buf.push(c, false),
            }
        }
        Ok(buf.finish())
    }

    /// Read the inside of a `"…"`, with the opening quote already consumed.
    fn read_dquote(&mut self, buf: &mut WordBuf) -> Result<(), ParseErr> {
        loop {
            let Some(b) = self.bump() else {
                return Err(ParseErr::Incomplete);
            };
            match b {
                b'"' => return Ok(()),
                b'\\' => match self.peek() {
                    // Inside double quotes a backslash escapes only these four
                    // and a newline; before anything else it is a literal
                    // backslash. That is why `echo "a\tb"` keeps its backslash
                    // for `echo` to interpret, while `echo "a\"b"` does not.
                    Some(c @ (b'$' | b'`' | b'"' | b'\\')) => {
                        self.pos = self.pos.saturating_add(1);
                        buf.push(c, true);
                    }
                    Some(b'\n') => {
                        self.pos = self.pos.saturating_add(1);
                    }
                    _ => buf.push(b'\\', true),
                },
                b'$' => self.read_dollar(buf, true)?,
                b'`' => {
                    let body = self.read_backtick()?;
                    buf.piece(Piece::Cmd { body, dq: true });
                }
                c => buf.push(c, true),
            }
        }
    }

    /// Read what follows a `$`, which has already been consumed.
    ///
    /// `dq` is whether we are inside double quotes, and it is recorded on the
    /// piece rather than acted on, because whether a substitution's result
    /// splits into fields is decided after the substitution has happened.
    fn read_dollar(&mut self, buf: &mut WordBuf, dq: bool) -> Result<(), ParseErr> {
        let simple = |name: Vec<u8>| Piece::Param {
            spec: Box::new(ParamSpec {
                name,
                len: false,
                op: None,
            }),
            dq,
        };
        match self.peek() {
            Some(b'{') => {
                self.pos = self.pos.saturating_add(1);
                let spec = self.read_brace()?;
                buf.piece(Piece::Param {
                    spec: Box::new(spec),
                    dq,
                });
            }
            Some(b'(') => {
                if self.at(1) == Some(b'(') {
                    // `$((` is arithmetic, even though `$( (a) )` would also
                    // parse — POSIX resolves the ambiguity this way, and a
                    // subshell that must be first can be written `$( (…) )`.
                    self.pos = self.pos.saturating_add(2);
                    let body = self.scan_arith()?;
                    buf.piece(Piece::Arith { body, dq });
                } else {
                    self.pos = self.pos.saturating_add(1);
                    let body = self.scan_nested(b')')?;
                    self.pos = self.pos.saturating_add(1);
                    buf.piece(Piece::Cmd { body, dq });
                }
            }
            // `$1`…`$9`: one digit only. `${10}` is how the tenth is spelled.
            Some(d) if d.is_ascii_digit() => {
                self.pos = self.pos.saturating_add(1);
                buf.piece(simple(vec![d]));
            }
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                let start = self.pos;
                while matches!(self.peek(), Some(n) if is_name_byte(n)) {
                    self.pos = self.pos.saturating_add(1);
                }
                buf.piece(simple(
                    self.src.get(start..self.pos).unwrap_or_default().to_vec(),
                ));
            }
            Some(c @ (b'@' | b'*' | b'#' | b'?' | b'-' | b'$' | b'!')) => {
                self.pos = self.pos.saturating_add(1);
                buf.piece(simple(vec![c]));
            }
            // A `$` before anything else is just a dollar sign.
            _ => buf.push(b'$', dq),
        }
        Ok(())
    }

    /// Read the inside of a `${…}`, with the brace already consumed.
    fn read_brace(&mut self) -> Result<ParamSpec, ParseErr> {
        // `${#x}` is a length; `${#}` is the parameter named `#`, which is the
        // count of positional parameters. The `}` is what tells them apart.
        let len = self.peek() == Some(b'#') && !matches!(self.at(1), None | Some(b'}'));
        if len {
            self.pos = self.pos.saturating_add(1);
        }
        let name = match self.peek() {
            None => return Err(ParseErr::Incomplete),
            Some(d) if d.is_ascii_digit() => {
                let start = self.pos;
                while matches!(self.peek(), Some(n) if n.is_ascii_digit()) {
                    self.pos = self.pos.saturating_add(1);
                }
                self.src.get(start..self.pos).unwrap_or_default().to_vec()
            }
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                let start = self.pos;
                while matches!(self.peek(), Some(n) if is_name_byte(n)) {
                    self.pos = self.pos.saturating_add(1);
                }
                self.src.get(start..self.pos).unwrap_or_default().to_vec()
            }
            Some(c @ (b'@' | b'*' | b'#' | b'?' | b'-' | b'$' | b'!')) => {
                self.pos = self.pos.saturating_add(1);
                vec![c]
            }
            Some(_) => return Err(ParseErr::Syntax("bad substitution".into())),
        };
        // The colon variants treat an empty value as unset; the others only
        // care whether the parameter exists at all.
        let colon = self.peek() == Some(b':');
        if colon {
            self.pos = self.pos.saturating_add(1);
        }
        let op = match self.peek() {
            Some(b'-') => {
                self.pos = self.pos.saturating_add(1);
                Some(ParamOp::Default(colon))
            }
            Some(b'=') => {
                self.pos = self.pos.saturating_add(1);
                Some(ParamOp::Assign(colon))
            }
            Some(b'?') => {
                self.pos = self.pos.saturating_add(1);
                Some(ParamOp::Error(colon))
            }
            Some(b'+') => {
                self.pos = self.pos.saturating_add(1);
                Some(ParamOp::Alt(colon))
            }
            Some(b'#') if !colon => Some(ParamOp::Prefix(self.doubled(b'#'))),
            Some(b'%') if !colon => Some(ParamOp::Suffix(self.doubled(b'%'))),
            Some(b'}') if !colon => None,
            None => return Err(ParseErr::Incomplete),
            Some(_) => return Err(ParseErr::Syntax("bad substitution".into())),
        };
        let op = match op {
            None => None,
            Some(kind) => {
                let text = self.scan_nested(b'}')?;
                Some((kind, lex_word_text(&text)?))
            }
        };
        if self.bump() != Some(b'}') {
            return Err(ParseErr::Incomplete);
        }
        Ok(ParamSpec { name, len, op })
    }

    /// Consume `b`, and a second one if it is there. `##` means "longest".
    fn doubled(&mut self, b: u8) -> bool {
        self.pos = self.pos.saturating_add(1);
        if self.peek() == Some(b) {
            self.pos = self.pos.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// A leading `~` or `~name`.
    ///
    /// Whether the name is a real user is not decided here: an unknown one
    /// expands back to the text that was written, so the lexer can take every
    /// `~` and let expansion sort it out.
    fn read_tilde(&mut self, buf: &mut WordBuf) {
        let start = self.pos.saturating_add(1);
        let mut i = start;
        while matches!(self.src.get(i), Some(&c) if c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
        {
            i = i.saturating_add(1);
        }
        let name = self.src.get(start..i).unwrap_or_default().to_vec();
        self.pos = i;
        buf.piece(Piece::Tilde(name));
    }

    /// Read the inside of a `` `…` ``, with the opening backquote consumed.
    fn read_backtick(&mut self) -> Result<Vec<u8>, ParseErr> {
        let mut body = Vec::new();
        loop {
            let Some(b) = self.bump() else {
                return Err(ParseErr::Incomplete);
            };
            match b {
                b'`' => return Ok(body),
                // The one place a backslash is removed before the text is even
                // a command: inside backquotes it is how a nested `` ` `` or a
                // literal `$` is written, since there is no other bracket.
                b'\\' => match self.peek() {
                    Some(c @ (b'`' | b'$' | b'\\')) => {
                        self.pos = self.pos.saturating_add(1);
                        body.push(c);
                    }
                    _ => body.push(b'\\'),
                },
                c => body.push(c),
            }
        }
    }

    /// Copy source up to an unnested `end`, leaving `self.pos` on it.
    ///
    /// One scanner for `)` and `}` rather than one each, because the nesting
    /// rules are identical and because a scanner that does not track quoting is
    /// how a shell comes to mis-parse `$(echo ")")`.
    fn scan_nested(&mut self, end: u8) -> Result<Vec<u8>, ParseErr> {
        let open = match end {
            b')' => Some(b'('),
            b'}' => Some(b'{'),
            _ => None,
        };
        let start = self.pos;
        let mut depth = 0usize;
        loop {
            let Some(b) = self.peek() else {
                return Err(ParseErr::Incomplete);
            };
            if b == end {
                if depth == 0 {
                    break;
                }
                depth = depth.saturating_sub(1);
                self.pos = self.pos.saturating_add(1);
                continue;
            }
            if Some(b) == open {
                depth = depth.saturating_add(1);
                self.pos = self.pos.saturating_add(1);
                continue;
            }
            match b {
                b'\\' => self.pos = self.src.len().min(self.pos.saturating_add(2)),
                b'\'' => {
                    self.pos = self.pos.saturating_add(1);
                    self.skip_quoted(b'\'', false)?;
                }
                b'"' => {
                    self.pos = self.pos.saturating_add(1);
                    self.skip_quoted(b'"', true)?;
                }
                _ => self.pos = self.pos.saturating_add(1),
            }
        }
        Ok(self.src.get(start..self.pos).unwrap_or_default().to_vec())
    }

    /// Advance past a quoted run, up to and including its closing `end`.
    ///
    /// `escapes` is false for `'…'`, where a backslash is an ordinary byte and
    /// nothing but the closing quote ends the run.
    fn skip_quoted(&mut self, end: u8, escapes: bool) -> Result<(), ParseErr> {
        loop {
            match self.bump() {
                None => return Err(ParseErr::Incomplete),
                Some(b) if b == end => return Ok(()),
                Some(b'\\') if escapes => {
                    self.pos = self.src.len().min(self.pos.saturating_add(1));
                }
                Some(_) => {}
            }
        }
    }

    /// Copy the inside of a `$((…))`, consuming the closing `))`.
    fn scan_arith(&mut self) -> Result<Vec<u8>, ParseErr> {
        let start = self.pos;
        let mut depth = 0usize;
        loop {
            let Some(b) = self.peek() else {
                return Err(ParseErr::Incomplete);
            };
            match b {
                b'(' => {
                    depth = depth.saturating_add(1);
                    self.pos = self.pos.saturating_add(1);
                }
                b')' if depth > 0 => {
                    depth = depth.saturating_sub(1);
                    self.pos = self.pos.saturating_add(1);
                }
                b')' if self.at(1) == Some(b')') => {
                    let body = self.src.get(start..self.pos).unwrap_or_default().to_vec();
                    self.pos = self.pos.saturating_add(2);
                    return Ok(body);
                }
                b')' => return Err(ParseErr::Syntax("bad arithmetic expansion".into())),
                b'\\' => self.pos = self.src.len().min(self.pos.saturating_add(2)),
                _ => self.pos = self.pos.saturating_add(1),
            }
        }
    }
}

/// Lex `text` as one word — the inside of a `${x:-word}`.
fn lex_word_text(text: &[u8]) -> Result<Word, ParseErr> {
    let mut lx = Lexer::new(text);
    lx.word_only = true;
    lx.read_word()
}

/// Lex the inside of a `$(( … ))` as one word.
///
/// The same thing minus tilde expansion, because `$((~x))` is a complement and
/// `$((a-~b))` would otherwise lose its operator to a home directory.
fn lex_arith_text(text: &[u8]) -> Result<Word, ParseErr> {
    let mut lx = Lexer::new(text);
    lx.word_only = true;
    lx.tilde_off = true;
    lx.read_word()
}

/// The literal text of a word that has no quoting and no substitution.
///
/// The parser uses this for the two places where a word is not data but syntax
/// — reserved words and the name in `name()`. Both are defined to be
/// *unquoted*: `"if"` is a command, not a keyword, and this returns `None` for
/// it because the byte arrived as a [`Piece::Quo`].
fn word_literal(w: &Word) -> Option<&[u8]> {
    match w.as_slice() {
        [Piece::Lit(b)] => Some(b),
        _ => None,
    }
}

/// A here-document delimiter: its text, and whether the body is expanded.
///
/// Any quoting anywhere in the delimiter turns expansion off for the whole
/// body — `<<'EOF'`, `<<"EOF"` and `<<E\OF` are all the literal form.
fn here_delim(w: &Word) -> (Vec<u8>, bool) {
    let mut text = Vec::new();
    let mut expand = true;
    for p in w {
        match p {
            Piece::Lit(b) => text.extend_from_slice(b),
            Piece::Quo(b) => {
                text.extend_from_slice(b);
                expand = false;
            }
            // A delimiter is not expanded, so a substitution in one is text.
            Piece::Param { .. } | Piece::Cmd { .. } | Piece::Arith { .. } => {}
            Piece::Tilde(name) => {
                text.push(b'~');
                text.extend_from_slice(name);
            }
        }
    }
    (text, expand)
}

// ---------------------------------------------------------------------------
// the parser
// ---------------------------------------------------------------------------

/// The words that end a list when they appear where a command would.
///
/// They are checked here rather than in the lexer because a reserved word is
/// only reserved in command position: `echo done` prints `done`, and the
/// difference is where the word stands, not what it says.
const TERMINATORS: &[&[u8]] = &[
    b"then", b"else", b"elif", b"fi", b"do", b"done", b"esac", b"}",
];

/// Is `w` the unquoted reserved word `kw`?
fn is_kw(w: &Word, kw: &[u8]) -> bool {
    word_literal(w) == Some(kw)
}

/// A word for a diagnostic: the literal text, with substitutions elided.
fn word_hint(w: &Word) -> Vec<u8> {
    let mut out = Vec::new();
    for p in w {
        match p {
            Piece::Lit(b) | Piece::Quo(b) => out.extend_from_slice(b),
            Piece::Tilde(name) => {
                out.push(b'~');
                out.extend_from_slice(name);
            }
            Piece::Param { .. } | Piece::Cmd { .. } | Piece::Arith { .. } => {
                out.extend_from_slice(b"$...");
            }
        }
    }
    out
}

/// How a token is named in a syntax error.
fn describe(t: &Tok) -> String {
    match t {
        Tok::Word(w) => coreutils::quote::quotef(&word_hint(w)),
        Tok::IoNumber(n) => coreutils::quote::quotef(n.to_string().as_bytes()),
        Tok::Op(o) => coreutils::quote::quotef(o.text().as_bytes()),
        Tok::Newline => "newline".to_string(),
        Tok::Eof => "end of file".to_string(),
    }
}

/// Turns tokens into a [`List`].
///
/// One token of lookahead, and it is *lazy*: `cur` is filled only when someone
/// asks. That is not an optimisation — it is what makes here-documents work.
/// The lexer reads a here-document's body when it crosses the newline, so the
/// parser must call [`Lexer::queue_here`] for `<<EOF` *before* it lexes another
/// token. Eager lookahead would have already crossed the newline by then, and
/// the body would be read with nothing queued.
struct Parser<'a> {
    lx: Lexer<'a>,
    cur: Tok,
    filled: bool,
}

impl<'a> Parser<'a> {
    fn new(src: &'a [u8]) -> Self {
        Parser {
            lx: Lexer::new(src),
            cur: Tok::Eof,
            filled: false,
        }
    }

    fn peek(&mut self) -> Result<&Tok, ParseErr> {
        if !self.filled {
            self.cur = self.lx.next_token()?;
            self.filled = true;
        }
        Ok(&self.cur)
    }

    fn take(&mut self) -> Result<Tok, ParseErr> {
        self.peek()?;
        self.filled = false;
        Ok(std::mem::replace(&mut self.cur, Tok::Eof))
    }

    /// The error for "not what should be here".
    ///
    /// End of input answers [`ParseErr::Incomplete`] rather than a syntax
    /// error, because the interactive loop's whole test for "keep reading" is
    /// this distinction: `if true` with nothing after it is a prompt for more,
    /// not a mistake.
    fn unexpected(&mut self, want: &str) -> ParseErr {
        match self.peek() {
            Err(e) => e,
            Ok(Tok::Eof) => ParseErr::Incomplete,
            Ok(t) => ParseErr::Syntax(format!(
                "syntax error near {}, expected {want}",
                describe(t)
            )),
        }
    }

    fn skip_newlines(&mut self) -> Result<(), ParseErr> {
        while matches!(self.peek()?, Tok::Newline) {
            self.take()?;
        }
        Ok(())
    }

    /// Consume the reserved word `kw` if it is next, newlines aside.
    fn eat_kw(&mut self, kw: &[u8]) -> Result<bool, ParseErr> {
        self.skip_newlines()?;
        let hit = matches!(self.peek()?, Tok::Word(w) if is_kw(w, kw));
        if hit {
            self.take()?;
        }
        Ok(hit)
    }

    fn expect_kw(&mut self, kw: &[u8]) -> Result<(), ParseErr> {
        if self.eat_kw(kw)? {
            Ok(())
        } else {
            Err(self.unexpected(&coreutils::quote::quotef(kw)))
        }
    }

    fn expect_op(&mut self, op: Op) -> Result<(), ParseErr> {
        self.skip_newlines()?;
        if matches!(self.peek()?, Tok::Op(o) if *o == op) {
            self.take()?;
            Ok(())
        } else {
            Err(self.unexpected(&coreutils::quote::quotef(op.text().as_bytes())))
        }
    }

    /// The whole of the input.
    fn parse_program(&mut self) -> Result<List, ParseErr> {
        let list = self.parse_list()?;
        if matches!(self.peek()?, Tok::Eof) {
            Ok(list)
        } else {
            Err(self.unexpected("end of file"))
        }
    }

    /// Does the current token end the enclosing list rather than start a
    /// command in it?
    fn at_list_end(&mut self) -> Result<bool, ParseErr> {
        Ok(match self.peek()? {
            Tok::Eof => true,
            Tok::Op(Op::RParen | Op::DSemi) => true,
            Tok::Word(w) => TERMINATORS.iter().any(|k| is_kw(w, k)),
            _ => false,
        })
    }

    /// `a; b & c` — the body of everything.
    fn parse_list(&mut self) -> Result<List, ParseErr> {
        let mut items = Vec::new();
        loop {
            self.skip_newlines()?;
            if self.at_list_end()? {
                break;
            }
            let ao = self.parse_and_or()?;
            let sep = match self.peek()? {
                Tok::Op(Op::Amp) => Some(true),
                Tok::Op(Op::Semi) | Tok::Newline => Some(false),
                _ => None,
            };
            match sep {
                Some(bg) => {
                    self.take()?;
                    items.push((ao, bg));
                }
                // No separator: whatever follows belongs to our caller, and if
                // it belongs to nobody the caller reports it.
                None => {
                    items.push((ao, false));
                    break;
                }
            }
        }
        Ok(List { items })
    }

    fn parse_and_or(&mut self) -> Result<AndOr, ParseErr> {
        let first = self.parse_pipeline()?;
        let mut rest = Vec::new();
        loop {
            let and = match self.peek()? {
                Tok::Op(Op::AndIf) => true,
                Tok::Op(Op::OrIf) => false,
                _ => break,
            };
            self.take()?;
            self.skip_newlines()?;
            rest.push((and, self.parse_pipeline()?));
        }
        Ok(AndOr { first, rest })
    }

    fn parse_pipeline(&mut self) -> Result<Pipeline, ParseErr> {
        let mut bang = false;
        loop {
            if !matches!(self.peek()?, Tok::Word(w) if is_kw(w, b"!")) {
                break;
            }
            self.take()?;
            // `! ! cmd` is `cmd`. dash allows the repetition; so do we.
            bang = !bang;
        }
        let mut cmds = vec![self.parse_command()?];
        while matches!(self.peek()?, Tok::Op(Op::Pipe)) {
            self.take()?;
            self.skip_newlines()?;
            cmds.push(self.parse_command()?);
        }
        Ok(Pipeline { bang, cmds })
    }

    fn parse_command(&mut self) -> Result<Cmd, ParseErr> {
        enum Which {
            Sub,
            Group,
            If,
            While,
            Until,
            For,
            Case,
            Simple,
        }
        let which = match self.peek()? {
            Tok::Op(Op::LParen) => Which::Sub,
            Tok::Word(w) => match word_literal(w) {
                Some(k) if k == b"{" => Which::Group,
                Some(k) if k == b"if" => Which::If,
                Some(k) if k == b"while" => Which::While,
                Some(k) if k == b"until" => Which::Until,
                Some(k) if k == b"for" => Which::For,
                Some(k) if k == b"case" => Which::Case,
                _ => Which::Simple,
            },
            _ => Which::Simple,
        };
        let kind = match which {
            Which::Simple => return self.parse_simple(),
            Which::Sub => {
                self.take()?;
                let body = self.parse_list()?;
                self.expect_op(Op::RParen)?;
                CmdKind::Subshell(Box::new(body))
            }
            Which::Group => {
                self.take()?;
                let body = self.parse_list()?;
                self.expect_kw(b"}")?;
                CmdKind::Group(Box::new(body))
            }
            Which::If => {
                self.take()?;
                self.parse_if()?
            }
            Which::While => {
                self.take()?;
                self.parse_loop(false)?
            }
            Which::Until => {
                self.take()?;
                self.parse_loop(true)?
            }
            Which::For => {
                self.take()?;
                self.parse_for()?
            }
            Which::Case => {
                self.take()?;
                self.parse_case()?
            }
        };
        let mut redirs = Vec::new();
        while self.at_redir()? {
            redirs.push(self.parse_redir()?);
        }
        Ok(Cmd { kind, redirs })
    }

    fn parse_if(&mut self) -> Result<CmdKind, ParseErr> {
        let mut arms = Vec::new();
        let mut otherwise = None;
        loop {
            let cond = self.parse_list()?;
            self.expect_kw(b"then")?;
            arms.push((cond, self.parse_list()?));
            if self.eat_kw(b"elif")? {
                continue;
            }
            if self.eat_kw(b"else")? {
                otherwise = Some(Box::new(self.parse_list()?));
            }
            self.expect_kw(b"fi")?;
            break;
        }
        Ok(CmdKind::If { arms, otherwise })
    }

    fn parse_loop(&mut self, until: bool) -> Result<CmdKind, ParseErr> {
        let cond = self.parse_list()?;
        self.expect_kw(b"do")?;
        let body = self.parse_list()?;
        self.expect_kw(b"done")?;
        Ok(CmdKind::Loop {
            until,
            cond: Box::new(cond),
            body: Box::new(body),
        })
    }

    fn parse_for(&mut self) -> Result<CmdKind, ParseErr> {
        let var = match self.take()? {
            Tok::Word(w) => match word_literal(&w) {
                Some(n) if is_name(n) => n.to_vec(),
                _ => {
                    return Err(ParseErr::Syntax(format!(
                        "{}: bad for-loop variable name",
                        coreutils::quote::quotef(&word_hint(&w))
                    )));
                }
            },
            t => {
                let d = describe(&t);
                return Err(ParseErr::Syntax(format!(
                    "syntax error near {d}, expected a for-loop variable name"
                )));
            }
        };
        // `for x in a b c` lists words; `for x` — with no `in` — iterates the
        // positional parameters, which is not the same as `for x in` with an
        // empty list, and is why this is an `Option` rather than a `Vec`.
        let mut words = None;
        if matches!(self.peek()?, Tok::Word(w) if is_kw(w, b"in")) {
            self.take()?;
            let mut ws = Vec::new();
            loop {
                let more =
                    matches!(self.peek()?, Tok::Word(w) if !is_kw(w, b"do") && !is_kw(w, b"done"));
                if !more {
                    break;
                }
                match self.take()? {
                    Tok::Word(w) => ws.push(w),
                    // Unreachable: `more` was just checked.
                    _ => break,
                }
            }
            words = Some(ws);
        }
        while matches!(self.peek()?, Tok::Op(Op::Semi) | Tok::Newline) {
            self.take()?;
        }
        self.expect_kw(b"do")?;
        let body = self.parse_list()?;
        self.expect_kw(b"done")?;
        Ok(CmdKind::For {
            var,
            words,
            body: Box::new(body),
        })
    }

    fn parse_case(&mut self) -> Result<CmdKind, ParseErr> {
        let word = match self.take()? {
            Tok::Word(w) => w,
            t => {
                let d = describe(&t);
                return Err(ParseErr::Syntax(format!(
                    "syntax error near {d}, expected a word after `case'"
                )));
            }
        };
        self.expect_kw(b"in")?;
        let mut arms = Vec::new();
        loop {
            self.skip_newlines()?;
            if self.eat_kw(b"esac")? {
                break;
            }
            // `(pat)` is as legal as `pat)`, and is how a pattern beginning
            // with `esac` is written.
            if matches!(self.peek()?, Tok::Op(Op::LParen)) {
                self.take()?;
            }
            let mut pats = Vec::new();
            loop {
                match self.take()? {
                    Tok::Word(w) => pats.push(w),
                    t => {
                        let d = describe(&t);
                        return Err(ParseErr::Syntax(format!(
                            "syntax error near {d}, expected a case pattern"
                        )));
                    }
                }
                if matches!(self.peek()?, Tok::Op(Op::Pipe)) {
                    self.take()?;
                } else {
                    break;
                }
            }
            self.expect_op(Op::RParen)?;
            let body = self.parse_list()?;
            arms.push((pats, body));
            if matches!(self.peek()?, Tok::Op(Op::DSemi)) {
                self.take()?;
            } else {
                self.expect_kw(b"esac")?;
                break;
            }
        }
        Ok(CmdKind::Case { word, arms })
    }

    fn at_redir(&mut self) -> Result<bool, ParseErr> {
        Ok(matches!(
            self.peek()?,
            Tok::IoNumber(_)
                | Tok::Op(
                    Op::Less
                        | Op::Great
                        | Op::DGreat
                        | Op::DLess
                        | Op::DLessDash
                        | Op::LessAnd
                        | Op::GreatAnd
                        | Op::LessGreat
                        | Op::Clobber
                )
        ))
    }

    fn parse_redir(&mut self) -> Result<Redir, ParseErr> {
        let fd = match *self.peek()? {
            Tok::IoNumber(n) => {
                self.take()?;
                Some(n)
            }
            _ => None,
        };
        let op = match self.take()? {
            Tok::Op(o) => o,
            t => {
                let d = describe(&t);
                return Err(ParseErr::Syntax(format!(
                    "syntax error near {d}, expected a redirection"
                )));
            }
        };
        let target = match self.take()? {
            Tok::Word(w) => w,
            t => {
                let d = describe(&t);
                return Err(ParseErr::Syntax(format!(
                    "syntax error near {d}, expected a redirection target"
                )));
            }
        };
        let rop = match op {
            Op::Less => RedirOp::Read,
            Op::Great => RedirOp::Write { clobber: false },
            Op::Clobber => RedirOp::Write { clobber: true },
            Op::DGreat => RedirOp::Append,
            Op::LessGreat => RedirOp::ReadWrite,
            Op::LessAnd => RedirOp::DupIn,
            Op::GreatAnd => RedirOp::DupOut,
            Op::DLess | Op::DLessDash => {
                let (delim, expand) = here_delim(&target);
                let body: HereBody = Rc::new(RefCell::new(Vec::new()));
                // Queued *now*, before any further token is lexed: the body
                // begins at the next newline, and the lexer reads it as it
                // crosses one. See [`Parser`].
                self.lx.queue_here(delim, op == Op::DLessDash, &body);
                RedirOp::Here { expand, body }
            }
            other => {
                return Err(ParseErr::Syntax(format!(
                    "syntax error near {}",
                    coreutils::quote::quotef(other.text().as_bytes())
                )));
            }
        };
        Ok(Redir {
            fd,
            op: rop,
            target,
        })
    }

    /// `VAR=x cmd arg… >file`, and the function definition that begins the
    /// same way.
    fn parse_simple(&mut self) -> Result<Cmd, ParseErr> {
        let mut assigns: Vec<(Vec<u8>, Word)> = Vec::new();
        let mut words: Vec<Word> = Vec::new();
        let mut redirs = Vec::new();
        loop {
            if self.at_redir()? {
                redirs.push(self.parse_redir()?);
                continue;
            }
            if !matches!(self.peek()?, Tok::Word(_)) {
                break;
            }
            let w = match self.take()? {
                Tok::Word(w) => w,
                // Unreachable: just checked.
                _ => break,
            };
            // `name()` — a definition, not a command. Recognised here rather
            // than by lookahead because taking the word first is what keeps the
            // lookahead one token deep.
            if words.is_empty() && assigns.is_empty() && matches!(self.peek()?, Tok::Op(Op::LParen))
            {
                let Some(name) = word_literal(&w).filter(|n| is_name(n)).map(<[u8]>::to_vec) else {
                    return Err(ParseErr::Syntax(format!(
                        "{}: bad function name",
                        coreutils::quote::quotef(&word_hint(&w))
                    )));
                };
                self.take()?;
                self.expect_op(Op::RParen)?;
                self.skip_newlines()?;
                let body = self.parse_command()?;
                return Ok(Cmd {
                    kind: CmdKind::FuncDef {
                        name,
                        body: Rc::new(body),
                    },
                    redirs,
                });
            }
            // An assignment only counts before the command name: `env A=1` has
            // one argument, not one assignment.
            if words.is_empty()
                && let Some(pair) = split_assign(&w)
            {
                assigns.push(pair);
                continue;
            }
            words.push(w);
        }
        if assigns.is_empty() && words.is_empty() && redirs.is_empty() {
            return Err(self.unexpected("a command"));
        }
        Ok(Cmd {
            kind: CmdKind::Simple { assigns, words },
            redirs,
        })
    }
}

/// `NAME=word` split into its halves, or `None` if this is not an assignment.
///
/// Only the *first* piece can carry the `=`, and it has to be unquoted: `a=b`
/// assigns, `'a'=b` and `a'='b` are commands, because POSIX defines the name
/// and the `=` as unquoted text.
fn split_assign(w: &Word) -> Option<(Vec<u8>, Word)> {
    let Some(Piece::Lit(first)) = w.first() else {
        return None;
    };
    let eq = first.iter().position(|&b| b == b'=')?;
    let name = first.get(..eq)?;
    if !is_name(name) {
        return None;
    }
    let mut value: Word = Vec::new();
    let tail = first.get(eq.saturating_add(1)..).unwrap_or_default();
    if !tail.is_empty() {
        value.push(Piece::Lit(tail.to_vec()));
    }
    value.extend(w.iter().skip(1).cloned());
    Some((name.to_vec(), value))
}

/// Parse a whole script.
fn parse(src: &[u8]) -> Result<List, ParseErr> {
    Parser::new(src).parse_program()
}

// ---------------------------------------------------------------------------
// expansion: the byte string that remembers its quotes
// ---------------------------------------------------------------------------

/// Bytes, plus one bit per byte recording whether it was quoted.
///
/// This is the type that makes the rest of expansion possible to state. After
/// substitution, `$x` and `"$x"` hold the same bytes and must be treated
/// differently — split and globbed in one case, neither in the other — and the
/// only thing that distinguishes them is where each byte came from. So the
/// provenance travels with the byte, and [`split_fields`] and [`glob`] read it
/// instead of guessing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Expanded {
    bytes: Vec<u8>,
    /// Parallel to `bytes`.
    quoted: Vec<bool>,
    /// Whether any quote was *written*, even one that produced no bytes. This
    /// is what makes `cmd ''` pass an empty argument while `cmd $empty` passes
    /// none.
    quotes: bool,
}

impl Expanded {
    fn push(&mut self, b: u8, quoted: bool) {
        self.bytes.push(b);
        self.quoted.push(quoted);
    }

    fn extend(&mut self, bytes: &[u8], quoted: bool) {
        for &b in bytes {
            self.push(b, quoted);
        }
    }

    fn is_quoted(&self, i: usize) -> bool {
        self.quoted.get(i).copied().unwrap_or(false)
    }

    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn slice(&self, from: usize, to: usize) -> Expanded {
        Expanded {
            bytes: self.bytes.get(from..to).unwrap_or_default().to_vec(),
            quoted: self.quoted.get(from..to).unwrap_or_default().to_vec(),
            quotes: self.quotes,
        }
    }

    /// The bytes with the quoting forgotten — which is quote removal, the last
    /// step of expansion, and is why it is a field access and not a pass.
    fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// Split `e` into fields on the unquoted bytes of `ifs`.
///
/// POSIX gives whitespace in `IFS` a different rule from everything else, and
/// the difference is visible: with the default `IFS`, `a  b` is two fields, but
/// with `IFS=:`, `a::b` is *three* — an empty one in the middle. A run of
/// `IFS` whitespace is one delimiter; any other `IFS` byte is a delimiter on
/// its own, with adjacent whitespace absorbed into it. A trailing delimiter
/// ends the last field rather than starting an empty one.
fn split_fields(e: &Expanded, ifs: &[u8]) -> Vec<Expanded> {
    // `IFS=''` turns splitting off entirely rather than splitting on nothing.
    if ifs.is_empty() {
        return vec![e.clone()];
    }
    let n = e.len();
    let byte = |i: usize| e.bytes.get(i).copied().unwrap_or(0);
    let sep = |i: usize| i < n && !e.is_quoted(i) && ifs.contains(&byte(i));
    let sep_ws = |i: usize| sep(i) && matches!(byte(i), b' ' | b'\t' | b'\n');
    let mut fields = Vec::new();
    let mut i = 0;
    while sep_ws(i) {
        i = i.saturating_add(1);
    }
    while i < n {
        let start = i;
        while i < n && !sep(i) {
            i = i.saturating_add(1);
        }
        fields.push(e.slice(start, i));
        if i >= n {
            break;
        }
        while sep_ws(i) {
            i = i.saturating_add(1);
        }
        if sep(i) {
            i = i.saturating_add(1);
            while sep_ws(i) {
                i = i.saturating_add(1);
            }
        }
    }
    fields
}

// ---------------------------------------------------------------------------
// expansion: pathnames
// ---------------------------------------------------------------------------

/// One path component of a glob pattern.
#[derive(Default)]
struct Comp {
    /// The component as an `fnmatch` pattern: quoted metacharacters escaped.
    pat: Vec<u8>,
    /// The component as plain bytes, for when it has no metacharacter.
    lit: Vec<u8>,
    meta: bool,
}

/// `base` and `name` joined with one `/`.
fn join_path(base: &[u8], name: &[u8]) -> Vec<u8> {
    let mut out = base.to_vec();
    if !out.is_empty() && out.last() != Some(&b'/') {
        out.push(b'/');
    }
    out.extend_from_slice(name);
    out
}

/// Pathname expansion.
///
/// `None` means the field holds no unquoted `*`, `?` or `[` and so is not a
/// pattern at all. That case is not an optimisation but a rule: `echo hi` must
/// not read a directory, and a shell that globbed every word would answer
/// differently depending on what happened to be on disk.
///
/// `Some(matches)` is the sorted match list, and `Some(empty)` says the pattern
/// matched nothing — which POSIX resolves by using the pattern text itself, so
/// `ls *.nosuch` reports `*.nosuch` rather than nothing at all. The caller does
/// that; this function only reports.
fn glob(field: &Expanded) -> Option<Vec<Vec<u8>>> {
    let n = field.len();
    let byte = |i: usize| field.bytes.get(i).copied().unwrap_or(0);
    let is_meta = |i: usize| matches!(byte(i), b'*' | b'?' | b'[') && !field.is_quoted(i);
    if !(0..n).any(is_meta) {
        return None;
    }
    let absolute = n > 0 && byte(0) == b'/' && !field.is_quoted(0);
    let mut comps: Vec<Comp> = Vec::new();
    let mut cur = Comp::default();
    for i in 0..n {
        let b = byte(i);
        let q = field.is_quoted(i);
        if b == b'/' && !q {
            comps.push(std::mem::take(&mut cur));
            continue;
        }
        cur.lit.push(b);
        // A quoted metacharacter is data, and the way to say so to a matcher
        // that has no side channel is to escape it.
        if q && matches!(b, b'*' | b'?' | b'[' | b']' | b'\\') {
            cur.pat.push(b'\\');
        }
        cur.pat.push(b);
        if !q && matches!(b, b'*' | b'?' | b'[') {
            cur.meta = true;
        }
    }
    // A trailing `/` makes an empty final component. It is not a component but
    // a constraint — `echo */` lists directories only — so it is kept as a
    // suffix and re-applied at the end.
    let trailing = cur.lit.is_empty() && !comps.is_empty();
    comps.push(cur);

    let mut bases: Vec<Vec<u8>> = vec![if absolute { b"/".to_vec() } else { Vec::new() }];
    for c in comps.iter().filter(|c| !c.lit.is_empty()) {
        let mut next = Vec::new();
        for base in &bases {
            if c.meta {
                let dir = if base.is_empty() {
                    b".".to_vec()
                } else {
                    base.clone()
                };
                let Ok(rd) = std::fs::read_dir(os_from_bytes(&dir)) else {
                    // Not a directory, or not readable. A glob reports what it
                    // can reach and is silent about what it cannot — the
                    // alternative is `echo */x` printing a permission error for
                    // every unreadable directory on the way.
                    continue;
                };
                let mut names: Vec<Vec<u8>> = rd
                    .filter_map(Result::ok)
                    .map(|e| bytes_of_os(&e.file_name()))
                    .collect();
                names.sort_unstable();
                for name in names {
                    if fnmatch::fnmatch(
                        &c.pat,
                        &name,
                        fnmatch::Flags::PATHNAME | fnmatch::Flags::PERIOD,
                    ) {
                        next.push(join_path(base, &name));
                    }
                }
            } else {
                next.push(join_path(base, &c.lit));
            }
        }
        bases = next;
    }
    if trailing {
        for b in &mut bases {
            b.push(b'/');
        }
    }
    // A literal component after a matched one was never checked against the
    // filesystem: `*/Makefile` has to drop the directories that have none.
    bases.retain(|p| std::fs::symlink_metadata(os_from_bytes(p)).is_ok());
    bases.sort_unstable();
    bases.dedup();
    Some(bases)
}

// ---------------------------------------------------------------------------
// shell state
// ---------------------------------------------------------------------------

/// A shell variable.
#[derive(Clone, Debug, Default)]
struct Var {
    value: Vec<u8>,
    /// Whether a child process sees it.
    exported: bool,
}

/// The letters `set` understands, in the order `$-` prints them.
const OPT_LETTERS: [u8; 8] = *b"aCefnuvx";

/// The `set -o` names, parallel to [`OPT_LETTERS`].
const OPT_NAMES: [&str; 8] = [
    "allexport",
    "noclobber",
    "errexit",
    "noglob",
    "noexec",
    "nounset",
    "verbose",
    "xtrace",
];

/// The `set -x` family.
///
/// One array rather than eight named fields because `set` addresses them by
/// letter and `$-` prints them as a set, so both operations would otherwise be
/// an eight-armed match that a ninth option would have to be added to twice.
#[derive(Clone, Copy, Debug, Default)]
struct Opts([bool; 8]);

impl Opts {
    fn index(letter: u8) -> Option<usize> {
        OPT_LETTERS.iter().position(|&l| l == letter)
    }

    fn name_index(name: &str) -> Option<usize> {
        OPT_NAMES.iter().position(|&n| n == name)
    }

    fn get(self, letter: u8) -> bool {
        Self::index(letter).is_some_and(|i| self.0.get(i).copied().unwrap_or(false))
    }

    /// Set the option `letter`; `false` if there is no such option.
    fn set(&mut self, letter: u8, on: bool) -> bool {
        let Some(i) = Self::index(letter) else {
            return false;
        };
        if let Some(slot) = self.0.get_mut(i) {
            *slot = on;
        }
        true
    }

    fn set_named(&mut self, name: &str, on: bool) -> bool {
        let Some(i) = Self::name_index(name) else {
            return false;
        };
        if let Some(slot) = self.0.get_mut(i) {
            *slot = on;
        }
        true
    }

    /// The letters of the options that are on — the value of `$-`.
    fn letters(self) -> Vec<u8> {
        OPT_LETTERS
            .iter()
            .enumerate()
            .filter(|(i, _)| self.0.get(*i).copied().unwrap_or(false))
            .map(|(_, &l)| l)
            .collect()
    }

    fn allexport(self) -> bool {
        self.get(b'a')
    }
    fn noclobber(self) -> bool {
        self.get(b'C')
    }
    fn errexit(self) -> bool {
        self.get(b'e')
    }
    fn noglob(self) -> bool {
        self.get(b'f')
    }
    fn noexec(self) -> bool {
        self.get(b'n')
    }
    fn nounset(self) -> bool {
        self.get(b'u')
    }
    fn verbose(self) -> bool {
        self.get(b'v')
    }
    fn xtrace(self) -> bool {
        self.get(b'x')
    }
}

/// A non-local exit from the middle of execution.
///
/// `break`, `continue`, `return` and `exit` all abandon whatever is running,
/// and the depth they unwind to differs — so they are an error type carried by
/// every executor method rather than a status code the caller has to test for.
/// Making them ordinary values is how the old implementation came to run the
/// body after a `break`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    /// `break n` — leave `n` enclosing loops.
    Break(u32),
    /// `continue n`.
    Continue(u32),
    /// `return n` from a function or a dotted script.
    Return(u8),
    /// `exit n`, and every fatal expansion error.
    Exit(u8),
}

/// The result of anything that executes.
type Run<T> = Result<T, Flow>;

/// A fully expanded argument vector — the command name first.
type Argv = Vec<Vec<u8>>;

/// The `name=value` assignments that prefixed a command, expanded.
type Pairs = Vec<(Vec<u8>, Vec<u8>)>;

/// Where one of a command's descriptors points.
#[derive(Clone, Debug)]
enum Fd {
    /// Descriptor `n` of the shell process itself.
    Inherit(i32),
    /// An open file, or one end of a pipe.
    Open(Rc<File>),
    /// `n>&-`.
    Closed,
}

/// The descriptor table a command runs with.
///
/// A map of overrides rather than a `dup2` of the real thing, because a builtin
/// runs *inside* the shell process: `echo hi > f` must not leave the shell's
/// own descriptor 1 pointing at `f` after it returns, and the only way to be
/// sure of that without saving and restoring real descriptors — which the
/// Windows host build cannot do, its stdout not being a descriptor at all — is
/// for the redirection never to touch them.
#[derive(Clone, Debug, Default)]
struct Io {
    fds: BTreeMap<i32, Fd>,
    /// Temporary files — here-document bodies — that must outlive the command
    /// reading them and must not outlive it by much. Holding the guard here
    /// ties the file's lifetime to the descriptor table that refers to it,
    /// which is the only thing that knows when the last reader is done.
    temps: Vec<Rc<TempPath>>,
}

impl Io {
    fn get(&self, n: i32) -> Fd {
        self.fds.get(&n).cloned().unwrap_or(Fd::Inherit(n))
    }

    fn set(&mut self, n: i32, fd: Fd) {
        self.fds.insert(n, fd);
    }
}

// `missing_const_for_thread_local` is a false positive on rustc/clippy 1.95:
// the initializer below already *is* a `const` block, and the lint fires
// anyway. Reproduced in a nine-line crate containing nothing but a
// `thread_local!` whose initializer is `const { RefCell::new(0) }`, so it is
// not something about this one. Suppressed rather than worked around because
// the only "fix" the lint would accept is the code that is already written.
thread_local! {
    /// Where a diagnostic from the shell itself goes *now*.
    ///
    /// A shell's own complaints are subject to the redirections of the command
    /// they are about: `nosuchcommand 2>err` puts `not found` in `err`, and a
    /// script that writes `exec 2>log` expects everything after it to land in
    /// the log. A builtin is handed its [`Io`] and can honour that; a
    /// diagnostic is raised from anywhere in the executor and cannot be, so the
    /// destination is kept here instead of threaded through every call.
    ///
    /// Thread-local rather than a `static` because [`Fd`] holds an `Rc`, and
    /// because the shell is single-threaded anyway — this is a stack of one
    /// value, saved and restored by [`ErrScope`].
    #[allow(
        clippy::missing_const_for_thread_local,
        reason = "false positive on rustc/clippy 1.95: the initializer already \
                  *is* a `const` block. Reproduced in a nine-line crate whose \
                  only content is a `thread_local!` initialized with \
                  `const { RefCell::new(0) }`, so it is not something about \
                  this one. The only change the lint would accept is the code \
                  that is already written."
    )]
    static CURRENT_ERR: RefCell<Fd> = const { RefCell::new(Fd::Inherit(2)) };
}

/// Point shell diagnostics at `io`'s descriptor 2 until the guard is dropped.
///
/// A guard rather than a plain setter because every path out of a command must
/// put the old destination back — including the `?` that unwinds a `Flow::Exit`
/// out of the middle of a redirected function, which no explicit restore at the
/// bottom of the function would ever run.
struct ErrScope(Fd);

impl ErrScope {
    fn new(io: &Io) -> Self {
        ErrScope(Self::point_at(io.get(2)))
    }

    /// Redirect diagnostics without a guard, returning what was there before.
    ///
    /// Used inside [`Shell::apply_redirs`], where the destination changes as
    /// each redirection is applied and the guard already exists.
    fn point_at(fd: Fd) -> Fd {
        CURRENT_ERR.with(|slot| slot.replace(fd))
    }
}

impl Drop for ErrScope {
    fn drop(&mut self) {
        let _ = ErrScope::point_at(self.0.clone());
    }
}

/// Write one diagnostic to wherever the shell's standard error points now.
///
/// The standard-output flush around the write is what keeps
/// `echo a; nosuch >out 2>&1` in order: descriptors 1 and 2 may share a
/// destination, and this shell's own standard output is buffered. It brackets
/// the write because the two arms need it on opposite sides — a diagnostic that
/// goes *into* the stdout buffer needs the flush after, one that goes to the
/// same file by another descriptor needs it before.
fn diag_line(line: &str) {
    let mut bytes = Vec::with_capacity(line.len().saturating_add(1));
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
    diag_out(&bytes);
}

/// [`diag_line`] without the newline, for a message assembled from bytes.
fn diag_out(bytes: &[u8]) {
    let fd = CURRENT_ERR.with(|slot| slot.borrow().clone());
    // Not `Io::default()` with a `set`, because that would allocate a map for
    // every diagnostic; the shape below is what `write_fd` reads.
    let mut io = Io::default();
    io.set(2, fd);
    flush_stdout();
    // Nowhere left to report a diagnostic that cannot be written; `write_fd`'s
    // `Inherit(2)` arm records the loss in `stdfd`'s flag, which is the one
    // that reaches the exit status.
    let _ = write_fd(&io, 2, bytes);
    flush_stdout();
}

/// Push this shell's buffered standard output out.
fn flush_stdout() {
    let mut out = stdfd::Stream::stdout();
    let _ = out.flush();
}

/// `eprintln!`-shaped diagnostic, routed through [`diag_line`].
///
/// This shadows [`coreutils::diag`] deliberately, exactly as `sed.rs` does with
/// its flushing variant: a shell diagnostic that ignored the current
/// redirections would be a bug, and the way to make that unwritable is for the
/// only `diag!` in scope to be the one that honours them.
macro_rules! diag {
    ($($arg:tt)*) => {
        crate::diag_line(&::std::format!($($arg)*))
    };
}

/// A path removed when the last holder lets go.
#[derive(Debug)]
struct TempPath(std::path::PathBuf);

impl TempPath {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        // Nothing useful to do if it is already gone, which is the usual case
        // on a unix target where it was unlinked as soon as it was opened.
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A fresh path in the temporary directory.
fn temp_path(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!("sh-{tag}-{}-{n}", std::process::id()))
}

/// A read/write temporary file holding `content`, positioned at its start.
///
/// A file rather than a pipe, and that is the whole point: a here-document can
/// be larger than a pipe buffer, and this shell has no `fork`, so there is
/// nobody to keep writing the far end while the reader reads. A pipe here
/// would deadlock on a long here-document, which is exactly the failure the
/// previous implementation had in its pipelines.
fn temp_file(tag: &str, content: &[u8]) -> std::io::Result<(File, TempPath)> {
    let path = temp_path(tag);
    let mut file = File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(content)?;
    file.flush()?;
    std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(0))?;
    Ok((file, TempPath(path)))
}

/// Everything a subshell must not change in its parent.
struct Snapshot {
    vars: BTreeMap<Vec<u8>, Var>,
    funcs: BTreeMap<Vec<u8>, Rc<Cmd>>,
    params: Vec<Vec<u8>>,
    status: u8,
    opts: Opts,
    cwd: Option<std::path::PathBuf>,
    exec_io: Option<Io>,
}

/// The shell.
struct Shell {
    vars: BTreeMap<Vec<u8>, Var>,
    funcs: BTreeMap<Vec<u8>, Rc<Cmd>>,
    /// `$1`, `$2`, …
    params: Vec<Vec<u8>>,
    /// `$0`.
    name: Vec<u8>,
    /// `$?`.
    status: u8,
    opts: Opts,
    /// How many loops are running, so `break` outside one can be diagnosed.
    loops: u32,
    /// How many function bodies are running, so `return` outside one can be.
    funcs_running: u32,
    /// `$!`.
    last_bg: Option<u32>,
    /// Background children, kept so `wait` can reap them — and so that
    /// dropping a [`Child`](std::process::Child) does not leave behind a
    /// zombie nobody remembers.
    bg: Vec<std::process::Child>,
    /// How deep we are inside a *condition* — the left of a `&&`, the test of
    /// an `if` or a `while`, a pipeline under `!`. `set -e` is suspended in
    /// there: a command whose failure selects a branch has not failed in the
    /// sense `-e` means, and a shell that exited on it could not write an `if`.
    cond_depth: u32,
    /// The status of the last command substitution performed while expanding
    /// the command now being run, or `None` if there was none. Read only by a
    /// command that has no command name, which POSIX says takes its status
    /// from exactly that — `x=$(false)` is a failing command.
    last_subst: Option<u8>,
    /// The table an `exec` with no command installed *in the redirection scope
    /// now running*. `None` — the ordinary case — means "whatever this construct
    /// was entered with".
    ///
    /// Scoped, not global, and that is the whole of the design. A real shell
    /// implements `{ …; } 2>outer` by saving descriptor 2, pointing it at
    /// `outer`, running, and putting the saved one back; an `exec 2>inner` in
    /// between overwrites 2 for the rest of the group and is then thrown away
    /// with it. [`Shell::in_redir_scope`] is that save-and-restore, and clearing
    /// this field on the way in is what makes [`Shell::io_now`] able to answer
    /// "did an `exec` happen *here*" without a generation counter: it is `Some`
    /// only inside the scope that set it.
    ///
    /// A table rather than real `dup2`s for the reason [`Io`] gives — this
    /// shell's builtins run in its own process, and the Windows host build has
    /// no descriptors to `dup2` in the first place. What it costs is that a
    /// descriptor above 2 reaches an external command only on unix, where
    /// [`Shell::build_command`] installs it with `pre_exec`.
    exec_io: Option<Io>,
    /// Whether the prompt is printed and errors are non-fatal.
    interactive: bool,
}

impl Shell {
    /// A shell whose variables are this process's environment.
    ///
    /// `vars_os`, not `vars`: the environment is the first thing a shell reads
    /// and the first place a non-UTF-8 byte turns up, and `env::vars()` panics
    /// on one. That panic is `sh.rs:183` in the `argv-utf8` baseline.
    fn new(name: Vec<u8>) -> Self {
        let mut vars = BTreeMap::new();
        for (k, v) in env::vars_os() {
            vars.insert(
                bytes_of_os(&k),
                Var {
                    value: bytes_of_os(&v),
                    exported: true,
                },
            );
        }
        Shell {
            vars,
            funcs: BTreeMap::new(),
            params: Vec::new(),
            name,
            status: 0,
            opts: Opts::default(),
            loops: 0,
            funcs_running: 0,
            last_bg: None,
            bg: Vec::new(),
            cond_depth: 0,
            last_subst: None,
            exec_io: None,
            interactive: false,
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            vars: self.vars.clone(),
            funcs: self.funcs.clone(),
            params: self.params.clone(),
            status: self.status,
            opts: self.opts,
            cwd: env::current_dir().ok(),
            exec_io: self.exec_io.clone(),
        }
    }

    /// Undo everything a subshell did.
    ///
    /// We have no `fork`, so a subshell is emulated: run in this process, then
    /// put the state back. It covers what POSIX says a subshell must not leak —
    /// variables, functions, positional parameters, options, the working
    /// directory and the descriptors `exec` gave the shell. What it cannot
    /// cover is a change to the *process* — `exec cmd` inside `( … )` really
    /// does replace this one, and `ulimit` is absent — both noted in the
    /// module docs rather than silently half-working.
    fn restore(&mut self, snap: Snapshot) {
        self.vars = snap.vars;
        self.funcs = snap.funcs;
        self.params = snap.params;
        self.status = snap.status;
        self.opts = snap.opts;
        self.exec_io = snap.exec_io;
        if let Some(dir) = snap.cwd {
            // Nothing useful to do if the directory we came from has been
            // removed under us; the shell keeps running where it is.
            let _ = env::set_current_dir(dir);
        }
    }

    /// The table to run the *next* command of a list with, given the table the
    /// list was entered with.
    ///
    /// A wholesale replacement rather than an overlay, because [`exec_io`] was
    /// itself built by applying `exec`'s redirections *to* the table in force
    /// when `exec` ran — it already carries whatever the enclosing construct
    /// established, with `exec`'s own changes on top. Overlaying it again, in
    /// either direction, would get one of the two orders wrong: `exec >log`
    /// before a `{ …; } >out` must lose to the group, and an `exec >log`
    /// *inside* that group must beat it.
    ///
    /// [`exec_io`]: Shell::exec_io
    fn io_now(&self, entry: &Io) -> Io {
        self.exec_io.clone().unwrap_or_else(|| entry.clone())
    }

    /// The table the shell starts from where there is no enclosing command —
    /// the top of a script, a prompt, the inside of a command substitution.
    fn base_io(&self) -> Io {
        self.exec_io.clone().unwrap_or_default()
    }

    /// Run `body` with its own `exec` scope, if `on`.
    ///
    /// `on` is "this construct has redirections of its own". Without them there
    /// is nothing to put back and `exec` inside must persist: `{ exec 2>log; };
    /// nosuch` logs, exactly as the same two commands written flat would. With
    /// them the construct owns descriptor 2 for its duration and hands it back
    /// on the way out, so `{ exec 2>inner; } 2>outer; nosuch` does not.
    ///
    /// A closure rather than a guard because the guard would need `&mut self`
    /// for its whole lifetime; taking the closure's return value means a `?`
    /// inside `body` still leaves through here rather than around it.
    fn in_redir_scope<T>(&mut self, on: bool, body: impl FnOnce(&mut Self) -> T) -> T {
        if !on {
            return body(self);
        }
        let saved = self.exec_io.take();
        let r = body(self);
        self.exec_io = saved;
        r
    }

    fn var(&self, name: &[u8]) -> Option<&Var> {
        self.vars.get(name)
    }

    /// Assign, keeping the export flag — and setting it under `set -a`.
    fn set_var(&mut self, name: &[u8], value: Vec<u8>) {
        let exported = self.opts.allexport() || self.var(name).is_some_and(|v| v.exported);
        self.vars.insert(name.to_vec(), Var { value, exported });
    }

    fn ifs(&self) -> Vec<u8> {
        self.var(b"IFS")
            .map_or_else(|| b" \t\n".to_vec(), |v| v.value.clone())
    }

    /// The byte `$*` joins with: the first of `IFS`, or nothing if `IFS` is
    /// empty — but a space when `IFS` is *unset*, which is not the same thing.
    fn star_sep(&self) -> Option<u8> {
        match self.var(b"IFS") {
            None => Some(b' '),
            Some(v) => v.value.first().copied(),
        }
    }

    /// The value of `$name`, or `None` if it is unset.
    fn param_value(&self, name: &[u8]) -> Option<Vec<u8>> {
        match name {
            b"?" => Some(self.status.to_string().into_bytes()),
            b"#" => Some(self.params.len().to_string().into_bytes()),
            b"$" => Some(std::process::id().to_string().into_bytes()),
            b"!" => self.last_bg.map(|p| p.to_string().into_bytes()),
            b"-" => Some(self.opts.letters()),
            b"0" => Some(self.name.clone()),
            b"*" | b"@" => {
                let mut out = Vec::new();
                for (i, p) in self.params.iter().enumerate() {
                    if i > 0
                        && let Some(sep) = self.star_sep()
                    {
                        out.push(sep);
                    }
                    out.extend_from_slice(p);
                }
                Some(out)
            }
            _ => {
                if !name.is_empty() && name.iter().all(u8::is_ascii_digit) {
                    let n = parse_int(name)?;
                    let n = usize::try_from(n).ok()?;
                    return self.params.get(n.checked_sub(1)?).cloned();
                }
                self.var(name).map(|v| v.value.clone())
            }
        }
    }

    /// The environment a child gets: the exported variables, and nothing else.
    fn child_env(&self) -> Vec<(OsString, OsString)> {
        self.vars
            .iter()
            .filter(|(_, v)| v.exported)
            .map(|(k, v)| (os_from_bytes(k), os_from_bytes(&v.value)))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// expansion
// ---------------------------------------------------------------------------

/// A word being expanded, split where `$@` forces a break.
///
/// Almost every word produces one of these; `"$@"` is the exception that makes
/// the type necessary, since it turns one word into as many fields as there are
/// positional parameters no matter what `IFS` says.
#[derive(Default)]
struct Fields {
    done: Vec<Expanded>,
    cur: Expanded,
}

impl Fields {
    /// End the current field and start another.
    fn brk(&mut self) {
        self.done.push(std::mem::take(&mut self.cur));
    }

    fn finish(mut self) -> Vec<Expanded> {
        self.done.push(self.cur);
        self.done
    }
}

/// Remove the shortest or longest prefix of `value` matching `pat`.
fn strip_prefix(value: &[u8], pat: &[u8], longest: bool) -> Vec<u8> {
    let tail = |i: usize| value.get(i..).unwrap_or_default().to_vec();
    let hit = |i: usize| {
        value
            .get(..i)
            .is_some_and(|head| fnmatch::fnmatch(pat, head, fnmatch::Flags::NONE))
    };
    if longest {
        for i in (0..=value.len()).rev() {
            if hit(i) {
                return tail(i);
            }
        }
    } else {
        for i in 0..=value.len() {
            if hit(i) {
                return tail(i);
            }
        }
    }
    value.to_vec()
}

/// Remove the shortest or longest suffix of `value` matching `pat`.
fn strip_suffix(value: &[u8], pat: &[u8], longest: bool) -> Vec<u8> {
    let head = |i: usize| value.get(..i).unwrap_or_default().to_vec();
    let hit = |i: usize| {
        value
            .get(i..)
            .is_some_and(|tail| fnmatch::fnmatch(pat, tail, fnmatch::Flags::NONE))
    };
    if longest {
        for i in 0..=value.len() {
            if hit(i) {
                return head(i);
            }
        }
    } else {
        for i in (0..=value.len()).rev() {
            if hit(i) {
                return head(i);
            }
        }
    }
    value.to_vec()
}

/// The home directory of `user`, from `/etc/passwd`.
///
/// Read directly rather than through `getpwnam`, because this crate links no
/// libc and because the file is the interface on this system. An unknown user,
/// or no such file, answers `None` — and the caller then leaves the `~user`
/// text alone, which is what POSIX says to do.
fn home_of(user: &[u8]) -> Option<Vec<u8>> {
    let text = std::fs::read("/etc/passwd").ok()?;
    for line in text.split(|&b| b == b'\n') {
        let mut f = line.split(|&b| b == b':');
        if f.next() == Some(user) {
            return f.nth(4).map(<[u8]>::to_vec);
        }
    }
    None
}

impl Shell {
    /// Expand a word all the way: substitution, field splitting, pathname
    /// expansion, quote removal — in that order, which is the order POSIX
    /// defines and is not interchangeable.
    fn expand_word(&mut self, w: &Word) -> Run<Vec<Vec<u8>>> {
        let pre = self.expand_pieces(w)?;
        let ifs = self.ifs();
        let mut out = Vec::new();
        for e in pre {
            let mut fields = split_fields(&e, &ifs);
            // A field that expanded to nothing is not a field — unless a quote
            // was written, in which case it is an empty one.
            if fields.is_empty() {
                if e.quotes {
                    fields.push(e);
                } else {
                    continue;
                }
            }
            // Every field splitting produced is kept, empty ones included: with
            // a non-whitespace `IFS` an empty field is *data*, and `IFS=:; set
            // -- $PATH` on a `PATH` with an empty entry has to keep the hole
            // where it was. The only expansion that yields no field at all is
            // one that produced no bytes and had no quotes, and that is the
            // case handled above.
            for f in fields {
                match if self.opts.noglob() { None } else { glob(&f) } {
                    // Not a pattern, or a pattern that matched nothing: the
                    // text stands as written.
                    None => out.push(f.into_bytes()),
                    Some(m) if m.is_empty() => out.push(f.into_bytes()),
                    Some(m) => out.extend(m),
                }
            }
        }
        Ok(out)
    }

    /// Expand a word to exactly one byte string: no splitting, no globbing.
    ///
    /// This is what a redirection target, an assignment's right-hand side and
    /// a `case` subject all need. POSIX exempts them from field splitting
    /// precisely so that `> $file` works when `$file` has a space in it.
    fn expand_to_bytes(&mut self, w: &Word) -> Run<Vec<u8>> {
        Ok(self.expand_concat(w)?.into_bytes())
    }

    /// As [`Self::expand_to_bytes`], keeping the quoting.
    fn expand_concat(&mut self, w: &Word) -> Run<Expanded> {
        let mut all = Expanded::default();
        for e in self.expand_pieces(w)? {
            all.quotes |= e.quotes;
            for i in 0..e.len() {
                all.push(e.bytes.get(i).copied().unwrap_or(0), e.is_quoted(i));
            }
        }
        Ok(all)
    }

    /// Expand a word into an `fnmatch` pattern, escaping what was quoted.
    fn expand_pattern(&mut self, w: &Word) -> Run<Vec<u8>> {
        let e = self.expand_concat(w)?;
        let mut pat = Vec::new();
        for i in 0..e.len() {
            let b = e.bytes.get(i).copied().unwrap_or(0);
            if e.is_quoted(i) && matches!(b, b'*' | b'?' | b'[' | b']' | b'\\') {
                pat.push(b'\\');
            }
            pat.push(b);
        }
        Ok(pat)
    }

    /// Substitution only: the first of the four expansion steps.
    fn expand_pieces(&mut self, w: &Word) -> Run<Vec<Expanded>> {
        let mut f = Fields::default();
        for p in w {
            match p {
                Piece::Lit(b) => f.cur.extend(b, false),
                Piece::Quo(b) => {
                    f.cur.quotes = true;
                    f.cur.extend(b, true);
                }
                Piece::Tilde(name) => {
                    // A home directory is data, not a pattern: `~` expanding to
                    // a directory with a `*` in its name must not then glob.
                    let v = self.tilde(name);
                    f.cur.extend(&v, true);
                }
                Piece::Cmd { body, dq } => {
                    let v = self.command_subst(body)?;
                    f.cur.quotes |= *dq;
                    f.cur.extend(&v, *dq);
                }
                Piece::Arith { body, dq } => {
                    let v = self.arith_expand(body)?;
                    f.cur.quotes |= *dq;
                    f.cur.extend(&v, *dq);
                }
                Piece::Param { spec, dq } => self.expand_param(spec, *dq, &mut f)?,
            }
        }
        Ok(f.finish())
    }

    fn tilde(&mut self, user: &[u8]) -> Vec<u8> {
        if user.is_empty() {
            return match self.param_value(b"HOME") {
                Some(h) if !h.is_empty() => h,
                _ => b"~".to_vec(),
            };
        }
        match home_of(user) {
            Some(h) if !h.is_empty() => h,
            _ => {
                let mut out = vec![b'~'];
                out.extend_from_slice(user);
                out
            }
        }
    }

    fn expand_param(&mut self, spec: &ParamSpec, dq: bool, f: &mut Fields) -> Run<()> {
        let name = spec.name.as_slice();
        // `$@` and `$*` are the only parameters that are a *list*, and `"$@"`
        // is the only expansion in the language that produces more than one
        // field from quoted text.
        if matches!(name, b"@" | b"*") && spec.op.is_none() && !spec.len {
            let params = self.params.clone();
            if name == b"@" && dq {
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        f.brk();
                    }
                    f.cur.quotes = true;
                    f.cur.extend(p, true);
                }
                return Ok(());
            }
            if name == b"@" {
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        f.brk();
                    }
                    f.cur.extend(p, false);
                }
                return Ok(());
            }
            let joined = self.param_value(b"*").unwrap_or_default();
            f.cur.quotes |= dq;
            f.cur.extend(&joined, dq);
            return Ok(());
        }
        let cur = self.param_value(name);
        let empty = |v: &Option<Vec<u8>>, colon: bool| match v {
            None => true,
            Some(x) => colon && x.is_empty(),
        };
        let value = match &spec.op {
            None => match cur {
                Some(v) => v,
                None => {
                    if self.opts.nounset() {
                        diag!("sh: {}: parameter not set", coreutils::quote::quotef(name));
                        return Err(Flow::Exit(2));
                    }
                    Vec::new()
                }
            },
            Some((ParamOp::Default(colon), word)) => {
                if empty(&cur, *colon) {
                    self.expand_to_bytes(word)?
                } else {
                    cur.unwrap_or_default()
                }
            }
            Some((ParamOp::Assign(colon), word)) => {
                if empty(&cur, *colon) {
                    let v = self.expand_to_bytes(word)?;
                    if is_name(name) {
                        self.set_var(name, v.clone());
                    } else {
                        diag!(
                            "sh: {}: cannot assign in this way",
                            coreutils::quote::quotef(name)
                        );
                        return Err(Flow::Exit(2));
                    }
                    v
                } else {
                    cur.unwrap_or_default()
                }
            }
            Some((ParamOp::Error(colon), word)) => {
                if empty(&cur, *colon) {
                    let msg = self.expand_to_bytes(word)?;
                    let msg = if msg.is_empty() {
                        b"parameter not set".to_vec()
                    } else {
                        msg
                    };
                    diag!(
                        "sh: {}: {}",
                        coreutils::quote::quotef(name),
                        String::from_utf8_lossy(&msg)
                    );
                    return Err(Flow::Exit(2));
                }
                cur.unwrap_or_default()
            }
            Some((ParamOp::Alt(colon), word)) => {
                if empty(&cur, *colon) {
                    Vec::new()
                } else {
                    self.expand_to_bytes(word)?
                }
            }
            Some((ParamOp::Prefix(longest), word)) => {
                let pat = self.expand_pattern(word)?;
                strip_prefix(&cur.unwrap_or_default(), &pat, *longest)
            }
            Some((ParamOp::Suffix(longest), word)) => {
                let pat = self.expand_pattern(word)?;
                strip_suffix(&cur.unwrap_or_default(), &pat, *longest)
            }
        };
        let value = if spec.len {
            value.len().to_string().into_bytes()
        } else {
            value
        };
        f.cur.quotes |= dq;
        f.cur.extend(&value, dq);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// arithmetic
// ---------------------------------------------------------------------------

/// The assignment operators, longest first so that `<<=` is not read as `<`.
///
/// The second element is the operation to apply, with `=` meaning plain
/// assignment and `<`/`>` the two shifts.
const ASSIGN_OPS: [(&[u8], u8); 11] = [
    (b"<<=", b'<'),
    (b">>=", b'>'),
    (b"+=", b'+'),
    (b"-=", b'-'),
    (b"*=", b'*'),
    (b"/=", b'/'),
    (b"%=", b'%'),
    (b"&=", b'&'),
    (b"^=", b'^'),
    (b"|=", b'|'),
    (b"=", b'='),
];

/// `$(( … ))`, evaluated over `i64`.
///
/// Recursive descent, one function per precedence level, in the order C gives
/// them — which is the order POSIX specifies for the shell, deliberately, so
/// that `$((1 + 2 * 3))` is 7 in every shell there has ever been.
///
/// Every arithmetic operation here is `wrapping_*` or a checked division. That
/// is not a style choice: `$((2**63))` is input, not a bug, and a shell that
/// aborts on it is a shell a script can crash from a text file.
struct Arith<'a> {
    src: &'a [u8],
    pos: usize,
    sh: &'a mut Shell,
}

impl Arith<'_> {
    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn at(&self, off: usize) -> Option<u8> {
        self.src.get(self.pos.saturating_add(off)).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b) if b.is_ascii_whitespace()) {
            self.pos = self.pos.saturating_add(1);
        }
    }

    fn starts_with(&self, s: &[u8]) -> bool {
        self.src
            .get(self.pos..self.pos.saturating_add(s.len()))
            .is_some_and(|t| t == s)
    }

    /// Consume `op` if it is next and is not the start of a longer operator.
    ///
    /// `bans` lists the bytes that would extend it: `&` is bitwise-and only
    /// when the next byte is neither `&` nor `=`.
    fn eat(&mut self, op: &[u8], bans: &[u8]) -> bool {
        self.skip_ws();
        if !self.starts_with(op) {
            return false;
        }
        if let Some(next) = self.at(op.len())
            && bans.contains(&next)
        {
            return false;
        }
        self.pos = self.pos.saturating_add(op.len());
        true
    }

    fn name(&mut self) -> Option<Vec<u8>> {
        self.skip_ws();
        match self.peek() {
            Some(b) if b.is_ascii_alphabetic() || b == b'_' => {}
            _ => return None,
        }
        let start = self.pos;
        while matches!(self.peek(), Some(b) if is_name_byte(b)) {
            self.pos = self.pos.saturating_add(1);
        }
        self.src.get(start..self.pos).map(<[u8]>::to_vec)
    }

    /// The numeric value of a variable: unset or empty is zero.
    fn lookup(&self, name: &[u8]) -> Result<i64, String> {
        let Some(v) = self.sh.param_value(name) else {
            return Ok(0);
        };
        if v.iter().all(u8::is_ascii_whitespace) {
            return Ok(0);
        }
        parse_int(&v)
            .ok_or_else(|| format!("{}: expression expected", coreutils::quote::quotef(&v)))
    }

    fn number(&mut self) -> Result<i64, String> {
        let (radix, start) = if self.peek() == Some(b'0') && matches!(self.at(1), Some(b'x' | b'X'))
        {
            self.pos = self.pos.saturating_add(2);
            (16, self.pos)
        } else if self.peek() == Some(b'0') && matches!(self.at(1), Some(d) if d.is_ascii_digit()) {
            self.pos = self.pos.saturating_add(1);
            (8, self.pos)
        } else {
            (10, self.pos)
        };
        while matches!(self.peek(), Some(d) if d.is_ascii_alphanumeric()) {
            self.pos = self.pos.saturating_add(1);
        }
        let text = self.src.get(start..self.pos).unwrap_or_default();
        let text =
            std::str::from_utf8(text).map_err(|_| "invalid number in arithmetic".to_string())?;
        if text.is_empty() {
            return Ok(0);
        }
        i64::from_str_radix(text, radix).map_err(|_| {
            format!(
                "{}: invalid number",
                coreutils::quote::quotef(text.as_bytes())
            )
        })
    }

    fn primary(&mut self) -> Result<i64, String> {
        self.skip_ws();
        match self.peek() {
            None => Err("expression expected".to_string()),
            Some(b'(') => {
                self.pos = self.pos.saturating_add(1);
                let v = self.expr()?;
                self.skip_ws();
                if self.peek() == Some(b')') {
                    self.pos = self.pos.saturating_add(1);
                    Ok(v)
                } else {
                    Err("expected `)'".to_string())
                }
            }
            Some(d) if d.is_ascii_digit() => self.number(),
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                let name = self.name().unwrap_or_default();
                self.lookup(&name)
            }
            Some(b) => Err(format!("unexpected {}", coreutils::quote::quotef(&[b]))),
        }
    }

    fn unary(&mut self) -> Result<i64, String> {
        if self.eat(b"!", b"=") {
            return Ok(i64::from(self.unary()? == 0));
        }
        if self.eat(b"~", b"") {
            return Ok(!self.unary()?);
        }
        if self.eat(b"-", b"=") {
            return Ok(self.unary()?.wrapping_neg());
        }
        if self.eat(b"+", b"=") {
            return self.unary();
        }
        self.primary()
    }

    fn mul(&mut self) -> Result<i64, String> {
        let mut v = self.unary()?;
        loop {
            if self.eat(b"*", b"=") {
                v = v.wrapping_mul(self.unary()?);
            } else if self.eat(b"/", b"=") {
                v = div(v, self.unary()?)?;
            } else if self.eat(b"%", b"=") {
                v = rem(v, self.unary()?)?;
            } else {
                return Ok(v);
            }
        }
    }

    fn add(&mut self) -> Result<i64, String> {
        let mut v = self.mul()?;
        loop {
            if self.eat(b"+", b"=") {
                v = v.wrapping_add(self.mul()?);
            } else if self.eat(b"-", b"=") {
                v = v.wrapping_sub(self.mul()?);
            } else {
                return Ok(v);
            }
        }
    }

    fn shift(&mut self) -> Result<i64, String> {
        let mut v = self.add()?;
        loop {
            if self.eat(b"<<", b"=") {
                let n = self.add()?;
                v = shl(v, n);
            } else if self.eat(b">>", b"=") {
                let n = self.add()?;
                v = shr(v, n);
            } else {
                return Ok(v);
            }
        }
    }

    fn rel(&mut self) -> Result<i64, String> {
        let mut v = self.shift()?;
        loop {
            if self.eat(b"<=", b"") {
                v = i64::from(v <= self.shift()?);
            } else if self.eat(b">=", b"") {
                v = i64::from(v >= self.shift()?);
            } else if self.eat(b"<", b"<=") {
                v = i64::from(v < self.shift()?);
            } else if self.eat(b">", b">=") {
                v = i64::from(v > self.shift()?);
            } else {
                return Ok(v);
            }
        }
    }

    fn eq(&mut self) -> Result<i64, String> {
        let mut v = self.rel()?;
        loop {
            if self.eat(b"==", b"") {
                v = i64::from(v == self.rel()?);
            } else if self.eat(b"!=", b"") {
                v = i64::from(v != self.rel()?);
            } else {
                return Ok(v);
            }
        }
    }

    fn bitand(&mut self) -> Result<i64, String> {
        let mut v = self.eq()?;
        while self.eat(b"&", b"&=") {
            v &= self.eq()?;
        }
        Ok(v)
    }

    fn bitxor(&mut self) -> Result<i64, String> {
        let mut v = self.bitand()?;
        while self.eat(b"^", b"=") {
            v ^= self.bitand()?;
        }
        Ok(v)
    }

    fn bitor(&mut self) -> Result<i64, String> {
        let mut v = self.bitxor()?;
        while self.eat(b"|", b"|=") {
            v |= self.bitxor()?;
        }
        Ok(v)
    }

    fn and(&mut self) -> Result<i64, String> {
        let mut v = self.bitor()?;
        while self.eat(b"&&", b"") {
            // No short-circuit skip: the right-hand side is still *parsed*,
            // because it has to be consumed either way. Only its side effects
            // — an embedded assignment — are suppressed, which is what
            // `&&` promises.
            let rhs = self.bitor()?;
            v = i64::from(v != 0 && rhs != 0);
        }
        Ok(v)
    }

    fn or(&mut self) -> Result<i64, String> {
        let mut v = self.and()?;
        while self.eat(b"||", b"") {
            let rhs = self.and()?;
            v = i64::from(v != 0 || rhs != 0);
        }
        Ok(v)
    }

    fn ternary(&mut self) -> Result<i64, String> {
        let cond = self.or()?;
        if !self.eat(b"?", b"") {
            return Ok(cond);
        }
        let yes = self.expr()?;
        if !self.eat(b":", b"") {
            return Err("expected `:'".to_string());
        }
        let no = self.ternary()?;
        Ok(if cond != 0 { yes } else { no })
    }

    /// Assignment, which is right-associative and lower than everything else.
    fn expr(&mut self) -> Result<i64, String> {
        let save = self.pos;
        if let Some(name) = self.name() {
            self.skip_ws();
            for (tok, kind) in ASSIGN_OPS {
                // `=` is an assignment; `==` is a comparison, and telling them
                // apart is the whole reason this loop backtracks.
                if !self.starts_with(tok) || (tok == b"=" && self.at(1) == Some(b'=')) {
                    continue;
                }
                self.pos = self.pos.saturating_add(tok.len());
                let rhs = self.expr()?;
                let old = if kind == b'=' { 0 } else { self.lookup(&name)? };
                let v = match kind {
                    b'=' => rhs,
                    b'+' => old.wrapping_add(rhs),
                    b'-' => old.wrapping_sub(rhs),
                    b'*' => old.wrapping_mul(rhs),
                    b'/' => div(old, rhs)?,
                    b'%' => rem(old, rhs)?,
                    b'&' => old & rhs,
                    b'^' => old ^ rhs,
                    b'|' => old | rhs,
                    b'<' => shl(old, rhs),
                    _ => shr(old, rhs),
                };
                self.sh.set_var(&name, v.to_string().into_bytes());
                return Ok(v);
            }
        }
        self.pos = save;
        self.ternary()
    }
}

/// Integer division, with the two ways it can fail reported rather than
/// aborting: division by zero, and `i64::MIN / -1`, which overflows.
fn div(a: i64, b: i64) -> Result<i64, String> {
    a.checked_div(b).ok_or_else(|| {
        if b == 0 {
            "division by zero".to_string()
        } else {
            "arithmetic overflow".to_string()
        }
    })
}

fn rem(a: i64, b: i64) -> Result<i64, String> {
    a.checked_rem(b).ok_or_else(|| {
        if b == 0 {
            "division by zero".to_string()
        } else {
            "arithmetic overflow".to_string()
        }
    })
}

/// A shift by a silly amount is zero rather than undefined.
fn shl(a: i64, n: i64) -> i64 {
    match u32::try_from(n) {
        Ok(n) if n < 64 => a.wrapping_shl(n),
        _ => 0,
    }
}

/// An arithmetic right shift: a negative value stays negative.
fn shr(a: i64, n: i64) -> i64 {
    match u32::try_from(n) {
        Ok(n) if n < 64 => a.wrapping_shr(n),
        _ => {
            if a < 0 {
                -1
            } else {
                0
            }
        }
    }
}

/// Evaluate `text` as an arithmetic expression.
fn arith_eval(sh: &mut Shell, text: &[u8]) -> Result<i64, String> {
    let mut a = Arith {
        src: text,
        pos: 0,
        sh,
    };
    a.skip_ws();
    // `$(())` is zero, not a syntax error.
    if a.pos >= a.src.len() {
        return Ok(0);
    }
    let v = a.expr()?;
    a.skip_ws();
    if a.pos < a.src.len() {
        return Err(format!(
            "unexpected {}",
            coreutils::quote::quotef(a.src.get(a.pos..).unwrap_or_default())
        ));
    }
    Ok(v)
}

impl Shell {
    /// `$(( … ))`: expand the text, then evaluate it.
    ///
    /// Two passes because both are specified: `$(($x + y))` substitutes `$x`
    /// and then looks `y` up as a variable of the expression, and a shell that
    /// did only one of the two would fail one of the two spellings.
    fn arith_expand(&mut self, body: &[u8]) -> Run<Vec<u8>> {
        let word = match lex_arith_text(body) {
            Ok(w) => w,
            Err(_) => {
                diag!("sh: arithmetic expression: syntax error");
                return Err(Flow::Exit(2));
            }
        };
        let text = self.expand_to_bytes(&word)?;
        match arith_eval(self, &text) {
            Ok(v) => Ok(v.to_string().into_bytes()),
            Err(e) => {
                // dash's shape, expression text and all — `arithmetic
                // expression: division by zero: "1/0"`. The text is what makes
                // the message usable: the expression the script *wrote* was
                // `$((x/y))`, and only the substituted form says what the
                // values were.
                diag!(
                    "sh: arithmetic expression: {e}: \"{}\"",
                    String::from_utf8_lossy(&text)
                );
                Err(Flow::Exit(2))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// descriptors
// ---------------------------------------------------------------------------

/// The byte a shell reports for a wait status of `code`.
fn status_byte(code: i32) -> u8 {
    u8::try_from(code & 0xff).unwrap_or(0)
}

/// The status a shell reports for a finished child.
fn status_of(st: &std::process::ExitStatus) -> u8 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = st.signal() {
            // A shell reports a killed child as 128 + the signal, which is what
            // makes `$?` after a Ctrl-C 130 everywhere.
            return status_byte(128i32.saturating_add(sig));
        }
    }
    status_byte(st.code().unwrap_or(0))
}

/// A private copy of this process's descriptor `n`.
///
/// Needed because `2>&1` has to hand a *child* something that outlives the
/// `Command`, and the only portable way to say "wherever my own descriptor 1
/// points" is to duplicate it.
#[cfg(unix)]
fn dup_std(n: i32) -> std::io::Result<File> {
    use std::os::fd::{BorrowedFd, OwnedFd};
    // SAFETY: `borrow_raw` requires only that `n` stay open for the lifetime of
    // the borrow, which ends within this statement. Nothing is closed here:
    // `try_clone_to_owned` duplicates, so the shell's own descriptor is
    // untouched, and an `n` that is not open comes back as `EBADF` rather than
    // as undefined behaviour.
    let borrowed = unsafe { BorrowedFd::borrow_raw(n) };
    let owned: OwnedFd = borrowed.try_clone_to_owned()?;
    Ok(File::from(owned))
}

/// [`dup_std`] on the development host, where only 0, 1 and 2 exist.
///
/// Windows has no descriptor table to borrow from — the standard streams are
/// `HANDLE`s the runtime holds — so `3>&1` cannot be answered here. It is not a
/// gap in the shipped shell; this arm exists so the unit tests can run.
#[cfg(windows)]
fn dup_std(n: i32) -> std::io::Result<File> {
    use std::os::windows::io::{AsHandle, OwnedHandle};
    let owned: OwnedHandle = match n {
        0 => std::io::stdin().as_handle().try_clone_to_owned()?,
        1 => std::io::stdout().as_handle().try_clone_to_owned()?,
        2 => std::io::stderr().as_handle().try_clone_to_owned()?,
        _ => return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput)),
    };
    Ok(File::from(owned))
}

/// A connected pair of files: the read end and the write end of a pipe.
fn pipe_files() -> std::io::Result<(File, File)> {
    let (r, w) = std::io::pipe()?;
    #[cfg(unix)]
    {
        use std::os::fd::OwnedFd;
        Ok((File::from(OwnedFd::from(r)), File::from(OwnedFd::from(w))))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::OwnedHandle;
        Ok((
            File::from(OwnedHandle::from(r)),
            File::from(OwnedHandle::from(w)),
        ))
    }
}

// The three calls needed to install a descriptor above 2 in a child, and to ask
// whether one is open at all.
//
// Declared here rather than taken from `libc`, which this crate deliberately
// does not depend on: `std` already links the platform C library on every unix
// target, and these are the only symbols from it the shell wants. All three are
// async-signal-safe, which is what makes them usable after `fork`.
//
// (A plain comment, not a doc comment: rustdoc does not document an extern
// block, and `unused_doc_comments` says so.)
#[cfg(unix)]
unsafe extern "C" {
    fn dup(oldfd: i32) -> i32;
    fn dup2(oldfd: i32, newfd: i32) -> i32;
    fn close(fd: i32) -> i32;
}

/// Does this process actually hold descriptor `n` open?
///
/// Asked before `n<&m` copies `m`, because a table that only remembers what the
/// *shell* did to a descriptor cannot tell an inherited-and-open one from an
/// inherited-and-absent one: `cat <&9` in a shell started with nine descriptors
/// is legitimate, and in one started with three is an error the script must
/// hear about.
///
/// `dup` and `close` rather than `fcntl(F_GETFD)`, which would need an `F_GETFD`
/// constant this crate has no portable source for. The pair asks the kernel the
/// same question and costs two syscalls on a path that is already opening files.
#[cfg(unix)]
fn fd_is_open(n: i32) -> bool {
    // SAFETY: both calls take a descriptor number and return one; neither reads
    // or writes memory, so there are no pointer invariants to uphold. The
    // duplicate is closed on the only path that creates one, and a `dup` that
    // failed returned -1 rather than a descriptor, so nothing leaks.
    unsafe {
        let d = dup(n);
        if d < 0 {
            return false;
        }
        close(d);
        true
    }
}

/// Nothing to ask on a host that has no descriptor numbers to ask about.
///
/// The Windows build is a development convenience — see the module docs — and a
/// descriptor that is not there fails when it is used rather than when it is
/// named. Reporting "open" here is the answer that leaves that path alone.
#[cfg(not(unix))]
fn fd_is_open(_n: i32) -> bool {
    true
}

/// The descriptors above 2 a child has to be handed by hand.
///
/// [`Command`] names three and no more, so `exec 3< f; cat <&3` needs the rest
/// installed between `fork` and `exec`. Owned duplicates rather than raw
/// numbers, so that the closure which uses them owns them too and none can be
/// closed between here and the fork.
#[cfg(unix)]
fn extra_fds(io: &Io) -> std::io::Result<Vec<(i32, File)>> {
    let mut out = Vec::new();
    for (&n, fd) in &io.fds {
        if n <= 2 {
            continue;
        }
        match fd {
            Fd::Open(f) => out.push((n, f.try_clone()?)),
            Fd::Inherit(m) => out.push((n, dup_std(*m)?)),
            // Nothing to install: a descriptor the child was never given is
            // already closed, `Command` having opened everything else with
            // close-on-exec set.
            Fd::Closed => {}
        }
    }
    Ok(out)
}

/// What a child should be given for its descriptor `n`.
///
/// `Fd::Closed` becomes the null device rather than a closed descriptor, and
/// that is an approximation: POSIX `n>&-` really closes, so a child that writes
/// to it gets `EBADF`, where ours silently discards. [`Command`] has no way to
/// say "leave this one shut" without `pre_exec`, which is unix-only and unsafe;
/// the difference is visible only to a program that tests for the error.
fn stdio_for(io: &Io, n: i32) -> std::io::Result<Stdio> {
    match io.get(n) {
        Fd::Inherit(m) if m == n => Ok(Stdio::inherit()),
        Fd::Inherit(m) => Ok(Stdio::from(dup_std(m)?)),
        Fd::Open(f) => Ok(Stdio::from(f.try_clone()?)),
        Fd::Closed => Ok(Stdio::null()),
    }
}

/// Write `bytes` to descriptor `n` of a *builtin* running in this process.
///
/// Descriptors 1 and 2 go through [`stdfd`] rather than through
/// `std::io::stdout`, so that a builtin's output is buffered and flushed on the
/// same rules as every other utility in this crate — including the rule that a
/// write to standard error flushes standard output first, so `echo a; echo b
/// >&2` cannot come out backwards.
fn write_fd(io: &Io, n: i32, bytes: &[u8]) -> std::io::Result<()> {
    match io.get(n) {
        Fd::Inherit(1) => {
            let mut out = stdfd::Stream::stdout();
            out.write_all(bytes)
        }
        Fd::Inherit(2) => {
            stdfd::diag_bytes(bytes);
            Ok(())
        }
        Fd::Inherit(m) => {
            let mut out = stdfd::Stream::on(m);
            out.write_all(bytes)?;
            out.flush()
        }
        Fd::Open(f) => {
            let mut file: &File = &f;
            file.write_all(bytes)
        }
        Fd::Closed => Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe)),
    }
}

/// Read everything descriptor `n` of a builtin has, up to and including the
/// first newline.
///
/// One byte at a time, and that is not laziness: `read x; cat` must leave `cat`
/// the bytes after the newline, so the shell may not read ahead. A buffered
/// reader over a shared descriptor would consume them.
fn read_line_fd(io: &Io, n: i32) -> std::io::Result<Vec<u8>> {
    // A duplicate of the descriptor rather than `std::io::stdin()`, and that is
    // the point: `Stdin` is a `BufReader`, so asking it for one byte takes
    // eight kilobytes from the operating system and hides them where no child
    // can reach them. A dup shares the file offset, so reading here really does
    // advance the shared position and `read x; cat` sees the rest.
    let mut src: File = match io.get(n) {
        Fd::Inherit(m) => dup_std(m)?,
        Fd::Open(f) => f.try_clone()?,
        Fd::Closed => return Ok(Vec::new()),
    };
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if src.read(&mut byte)? == 0 {
            return Ok(line);
        }
        let b = byte.first().copied().unwrap_or(b'\n');
        line.push(b);
        if b == b'\n' {
            return Ok(line);
        }
    }
}

// ---------------------------------------------------------------------------
// redirection
// ---------------------------------------------------------------------------

/// How a parse failure reads in a diagnostic.
fn parse_err_text(e: &ParseErr) -> String {
    match e {
        ParseErr::Incomplete => "unexpected end of file".to_string(),
        ParseErr::Syntax(m) => m.clone(),
    }
}

impl Shell {
    /// Build the descriptor table `redirs` asks for, on top of `base`.
    ///
    /// `Ok(None)` is a redirection that failed — the file could not be opened,
    /// the here-document could not be written. The command then does not run
    /// and the shell reports 2, which is what dash does; the shell itself keeps
    /// going, because a script that cannot open one file may still be able to
    /// clean up after itself.
    fn apply_redirs(&mut self, redirs: &[Redir], base: &Io) -> Run<Option<Io>> {
        if redirs.is_empty() {
            return Ok(Some(base.clone()));
        }
        // A diagnostic from a later redirection honours an earlier one, because
        // that is the order the descriptors are established in: `2>err >/nope`
        // puts the complaint in `err`, and `>/nope 2>err` does not. The guard
        // is dropped on the way out and the caller installs its own from the
        // table this returns.
        let _err = ErrScope::new(base);
        let mut io = base.clone();
        for r in redirs {
            let default_fd = match r.op {
                RedirOp::Read | RedirOp::ReadWrite | RedirOp::DupIn | RedirOp::Here { .. } => 0,
                RedirOp::Write { .. } | RedirOp::Append | RedirOp::DupOut => 1,
            };
            let n = r.fd.unwrap_or(default_fd);
            match &r.op {
                RedirOp::Here { expand, body } => {
                    let raw = body.borrow().clone();
                    let bytes = if *expand {
                        // The body is expanded as if it stood inside double
                        // quotes: `$x` and `` `cmd` `` are live, `*` is not.
                        match lex_word_text(&raw) {
                            Ok(w) => self.expand_to_bytes(&w)?,
                            Err(e) => {
                                diag!("sh: here-document: {}", parse_err_text(&e));
                                return Ok(None);
                            }
                        }
                    } else {
                        raw
                    };
                    match temp_file("here", &bytes) {
                        Ok((f, guard)) => {
                            io.set(n, Fd::Open(Rc::new(f)));
                            io.temps.push(Rc::new(guard));
                        }
                        Err(e) => {
                            diag!("sh: cannot create here-document: {}", strerror(&e));
                            return Ok(None);
                        }
                    }
                }
                RedirOp::DupIn | RedirOp::DupOut => {
                    let t = self.expand_to_bytes(&r.target)?;
                    if t == b"-" {
                        io.set(n, Fd::Closed);
                    } else {
                        match parse_int(&t).and_then(|v| i32::try_from(v).ok()) {
                            // `2>&1` copies whatever descriptor 1 is *now*,
                            // which is why order matters: `>f 2>&1` and
                            // `2>&1 >f` differ.
                            Some(m) => {
                                let src = io.get(m);
                                // Copying a descriptor that is not open is an
                                // error, not a silent nothing: `exec 3<&-` then
                                // `cat <&3` would otherwise run `cat` on an
                                // empty input and report success, which is the
                                // worst possible answer — the script sees no
                                // data and no complaint.
                                let usable = match &src {
                                    Fd::Closed => false,
                                    Fd::Inherit(k) => fd_is_open(*k),
                                    Fd::Open(_) => true,
                                };
                                if !usable {
                                    diag!("sh: {m}: Bad file descriptor");
                                    return Ok(None);
                                }
                                io.set(n, src);
                            }
                            None => {
                                diag!("sh: {}: bad file descriptor", coreutils::quote::quotef(&t));
                                // Fatal, where a descriptor that merely is not
                                // open is not. dash draws the same line and
                                // calls this one a syntax error: a target that
                                // is not a number cannot be a typo for a
                                // descriptor the script might have opened, so
                                // there is no reading of the script under which
                                // carrying on does what it says.
                                self.special_error(2)?;
                                return Ok(None);
                            }
                        }
                    }
                }
                RedirOp::Read | RedirOp::Write { .. } | RedirOp::Append | RedirOp::ReadWrite => {
                    let name = self.expand_to_bytes(&r.target)?;
                    let path = std::path::PathBuf::from(os_from_bytes(&name));
                    let mut opts = File::options();
                    match &r.op {
                        RedirOp::Read => {
                            opts.read(true);
                        }
                        RedirOp::Write { clobber } => {
                            opts.write(true).truncate(true);
                            // `set -C` makes `>` refuse an existing file, and
                            // `>|` is the written override for it.
                            if self.opts.noclobber() && !clobber {
                                opts.create_new(true);
                            } else {
                                opts.create(true);
                            }
                        }
                        RedirOp::Append => {
                            opts.append(true).create(true);
                        }
                        _ => {
                            opts.read(true).write(true).create(true);
                        }
                    }
                    match opts.open(&path) {
                        Ok(f) => io.set(n, Fd::Open(Rc::new(f))),
                        Err(e) => {
                            diag!(
                                "sh: cannot {} {}: {}",
                                if matches!(r.op, RedirOp::Read) {
                                    "open"
                                } else {
                                    "create"
                                },
                                quotef_os(&path),
                                strerror(&e)
                            );
                            return Ok(None);
                        }
                    }
                }
            }
            // Whatever descriptor 2 has become is where the *next* redirection
            // complains. See the guard above.
            let _ = ErrScope::point_at(io.get(2));
        }
        Ok(Some(io))
    }
}

// ---------------------------------------------------------------------------
// execution
// ---------------------------------------------------------------------------

/// The special builtins, which resolve *before* functions and whose failure is
/// fatal to a non-interactive shell.
///
/// POSIX names sixteen; these are the ones this shell has. `readonly`, `times`
/// and `trap` are absent, and absent rather than stubbed on purpose: a `trap`
/// that accepted its arguments and ignored them would make a script that
/// installs a cleanup handler look as though it had one.
const SPECIAL_BUILTINS: &[&[u8]] = &[
    b":",
    b".",
    b"break",
    b"continue",
    b"eval",
    b"exec",
    b"exit",
    b"export",
    b"return",
    b"set",
    b"shift",
    b"unset",
];

/// The regular builtins: resolved after functions, and their failure is not
/// fatal.
const REGULAR_BUILTINS: &[&[u8]] = &[
    b"cd", b"echo", b"false", b"pwd", b"read", b"source", b"true", b"wait",
];

fn is_special(name: &[u8]) -> bool {
    SPECIAL_BUILTINS.contains(&name)
}

fn is_regular(name: &[u8]) -> bool {
    REGULAR_BUILTINS.contains(&name)
}

/// What one stage of a pipeline turned out to be.
///
/// Classification happens for every stage *before* any of them runs, because
/// which stages are external decides how the pipes between them are made — see
/// [`Shell::run_pipeline`]. Expanding here rather than in the stage means the
/// expansion is not done twice, which for `$(…)` would mean running a command
/// twice.
enum Plan {
    /// A compound command: `run_cmd` will expand whatever it needs itself.
    Compound,
    /// A simple command, already expanded.
    Simple {
        argv: Argv,
        pairs: Pairs,
        /// Whether it will be a separate process.
        external: bool,
    },
}

impl Plan {
    fn is_external(&self) -> bool {
        matches!(self, Plan::Simple { external: true, .. })
    }
}

impl Shell {
    /// Run a whole list, honouring `set -e` between its members.
    fn run_list(&mut self, list: &List, io: &Io) -> Run<()> {
        // Diagnostics raised between commands — a `.`-ed file that will not
        // parse, the shell's own complaints — go where this list's descriptor
        // 2 points. Dropped on the way out, so an `exec 2>log` that a subshell
        // performed and then unwound does not outlive it.
        let _err = ErrScope::new(&self.io_now(io));
        for (ao, bg) in &list.items {
            // Re-read before every member, because the one before it may have
            // been an `exec 2>log` that this one has to see.
            let cur = self.io_now(io);
            let _ = ErrScope::point_at(cur.get(2));
            if *bg {
                self.run_background(ao, &cur)?;
            } else {
                self.run_and_or(ao, &cur)?;
                // `set -e` does not fire on a pipeline written with `!`, nor on
                // one used as a condition. POSIX is explicit about both, and a
                // shell that got it wrong would exit out of its own `if`.
                let banged = ao.rest.last().map_or(ao.first.bang, |(_, p)| p.bang);
                if self.opts.errexit() && !banged && self.cond_depth == 0 && self.status != 0 {
                    return Err(Flow::Exit(self.status));
                }
            }
        }
        Ok(())
    }

    /// `a && b || c`.
    fn run_and_or(&mut self, ao: &AndOr, io: &Io) -> Run<()> {
        let tail = ao.rest.len();
        self.in_cond(tail > 0, |sh| sh.run_pipeline(&ao.first, io))?;
        for (i, (and, p)) in ao.rest.iter().enumerate() {
            let ok = self.status == 0;
            // `&&` runs when the last one succeeded, `||` when it did not; the
            // skipped case leaves the status alone, which is what makes
            // `a && b || c` run `c` when `a` failed.
            if *and != ok {
                continue;
            }
            let last = i.saturating_add(1) == tail;
            // Re-read for the same reason `run_list` does: `exec 2>log && cmd`
            // is one member of a list, and `cmd` is still after the `exec`.
            let cur = self.io_now(io);
            self.in_cond(!last, |sh| sh.run_pipeline(p, &cur))?;
        }
        Ok(())
    }

    /// Run `body` as a subshell.
    ///
    /// Two things make it one: the state it changes is put back, and `exit`,
    /// `break` and `return` end *it* rather than the shell that wrote it — an
    /// `exit 3` in a subshell is the subshell's status, not the script's.
    ///
    /// There is no `fork` behind this. See [`Shell::restore`] for what that
    /// costs and what it covers.
    fn subshell(&mut self, body: impl FnOnce(&mut Self) -> Run<()>) -> Run<()> {
        let snap = self.snapshot();
        let r = body(self);
        let status = match r {
            Ok(()) => self.status,
            Err(Flow::Exit(n) | Flow::Return(n)) => n,
            Err(Flow::Break(_) | Flow::Continue(_)) => self.status,
        };
        self.restore(snap);
        self.status = status;
        Ok(())
    }

    /// Run `body` with the condition depth raised when `cond` says so.
    fn in_cond<T>(&mut self, cond: bool, body: impl FnOnce(&mut Self) -> Run<T>) -> Run<T> {
        if cond {
            self.cond_depth = self.cond_depth.saturating_add(1);
        }
        let r = body(self);
        if cond {
            self.cond_depth = self.cond_depth.saturating_sub(1);
        }
        r
    }

    /// `cmd &`.
    ///
    /// There is no `fork` here, so only what can be *spawned* can truly run in
    /// the background: a single external command. Anything else — a builtin, a
    /// function, a loop — runs in the foreground and reports 0, which is a
    /// documented limitation rather than a silent one. Job control proper is in
    /// `osh`.
    fn run_background(&mut self, ao: &AndOr, io: &Io) -> Run<()> {
        if ao.rest.is_empty()
            && !ao.first.bang
            && ao.first.cmds.len() == 1
            && let Some(cmd) = ao.first.cmds.first()
            && let CmdKind::Simple { assigns, words } = &cmd.kind
            && !words.is_empty()
        {
            let (argv, pairs) = self.expand_simple(assigns, words)?;
            let external = argv.first().is_some_and(|n| self.is_external(n));
            if external
                && let Some(io) = self.apply_redirs(&cmd.redirs, io)?
                && let Some(child) = {
                    let _err = ErrScope::new(&io);
                    self.spawn_external(&argv, &pairs, &io)
                }
            {
                self.last_bg = Some(child.id());
                self.bg.push(child);
                self.status = 0;
                return Ok(());
            }
            if external {
                // The spawn failed and said so; `&` still reports 0.
                self.status = 0;
                return Ok(());
            }
            return self.run_resolved(&argv, &pairs, &cmd.redirs, io).map(|()| {
                self.status = 0;
            });
        }
        self.run_and_or(ao, io)?;
        self.status = 0;
        Ok(())
    }

    /// `a | b | c`, with every stage live at once.
    ///
    /// The order below is the whole point of the method, and it is what the
    /// previous implementation got wrong: it waited for stage *i* before
    /// spawning stage *i+1*, so `yes | head -2` never terminated — `yes` filled
    /// the pipe and blocked, and the reader that would have drained it had not
    /// been started.
    ///
    /// 1. classify every stage, so we know which are separate processes;
    /// 2. build the connections — a real pipe wherever a process is on either
    ///    end, and a **temporary file** where both ends run in this one, since
    ///    two builtins cannot take turns on a pipe that only holds 64 KiB;
    /// 3. spawn every external stage, dropping this shell's copy of its
    ///    descriptors immediately — the reader must see EOF, and it cannot
    ///    while the shell still holds the write end;
    /// 4. run the in-process stages in order, dropping each one's descriptors
    ///    as it finishes, for the same reason;
    /// 5. wait for the children, and report the last stage's status.
    fn run_pipeline(&mut self, p: &Pipeline, io: &Io) -> Run<()> {
        // `set -n` reads and checks and does not run — and it takes effect from
        // the command *after* the one that set it, which is why the test is
        // here and not only in front of the parse. A shell that only looked at
        // `-n` on the command line would run everything after a `set -n` in the
        // middle of a script.
        if self.opts.noexec() {
            self.status = 0;
            return Ok(());
        }
        if p.cmds.len() < 2 {
            if let Some(c) = p.cmds.first() {
                self.run_cmd(c, io)?;
            }
            if p.bang {
                self.status = u8::from(self.status == 0);
            }
            return Ok(());
        }
        let n = p.cmds.len();
        let mut plans: Vec<Plan> = Vec::with_capacity(n);
        for c in &p.cmds {
            let plan = self.plan(c)?;
            plans.push(plan);
        }
        let mut stages: Vec<Io> = vec![io.clone(); n];
        for i in 0..n.saturating_sub(1) {
            let j = i.saturating_add(1);
            let both_here = !plans.get(i).is_some_and(Plan::is_external)
                && !plans.get(j).is_some_and(Plan::is_external);
            if let Err(e) = connect(&mut stages, i, j, both_here) {
                diag!("sh: cannot create pipe: {}", strerror(&e));
                self.status = 2;
                return Ok(());
            }
        }
        let mut kids: Vec<Option<std::process::Child>> = (0..n).map(|_| None).collect();
        for i in 0..n {
            let spawned = match (plans.get(i), stages.get(i)) {
                (
                    Some(Plan::Simple {
                        argv,
                        pairs,
                        external: true,
                    }),
                    Some(stage),
                ) => {
                    let stage = stage.clone();
                    match self.apply_redirs(p.cmds.get(i).map_or(&[], |c| &c.redirs), &stage)? {
                        Some(io) => {
                            let _err = ErrScope::new(&io);
                            self.spawn_external(argv, pairs, &io)
                        }
                        None => None,
                    }
                }
                _ => continue,
            };
            if let Some(slot) = kids.get_mut(i) {
                *slot = spawned;
            }
            // Whatever the shell still holds of this stage's pipe ends has to
            // go, or the next stage never sees EOF.
            if let Some(stage) = stages.get_mut(i) {
                *stage = Io::default();
            }
        }
        let mut last_status = self.status;
        let mut flow: Run<()> = Ok(());
        for i in 0..n {
            let stage = stages.get(i).cloned().unwrap_or_default();
            // Every stage of a pipeline is a subshell — that is what makes
            // `echo hi | read x` leave `x` unset, which surprises people often
            // enough that bash has an option to turn it off. A stage that ran
            // in the shell proper would be the odd one out: the *external*
            // stages are separate processes and could never write back, so a
            // builtin that could would make the two spellings of the same
            // pipeline behave differently.
            let r = match (plans.get(i), p.cmds.get(i)) {
                (Some(Plan::Simple { external: true, .. }), _) => Ok(()),
                (Some(Plan::Simple { argv, pairs, .. }), Some(cmd)) => {
                    self.subshell(|sh| sh.run_resolved(argv, pairs, &cmd.redirs, &stage))
                }
                (Some(Plan::Compound), Some(cmd)) => self.subshell(|sh| sh.run_cmd(cmd, &stage)),
                _ => Ok(()),
            };
            if let Some(slot) = stages.get_mut(i) {
                *slot = Io::default();
            }
            if i.saturating_add(1) == n {
                last_status = self.status;
            }
            if let Err(e) = r {
                flow = Err(e);
                break;
            }
        }
        // Reaped unconditionally: leaving a child behind because an inner
        // `exit` unwound past it is how a shell accumulates zombies.
        for (i, slot) in kids.iter_mut().enumerate() {
            let Some(child) = slot.as_mut() else { continue };
            match child.wait() {
                Ok(st) if i.saturating_add(1) == n => last_status = status_of(&st),
                Ok(_) => {}
                Err(e) => {
                    diag!("sh: wait: {}", strerror(&e));
                    if i.saturating_add(1) == n {
                        last_status = 1;
                    }
                }
            }
        }
        flow?;
        self.status = if p.bang {
            u8::from(last_status == 0)
        } else {
            last_status
        };
        Ok(())
    }

    /// Decide what one stage of a pipeline is, expanding it if it is simple.
    fn plan(&mut self, cmd: &Cmd) -> Run<Plan> {
        match &cmd.kind {
            CmdKind::Simple { assigns, words } => {
                let (argv, pairs) = self.expand_simple(assigns, words)?;
                let external = argv.first().is_some_and(|n| self.is_external(n));
                Ok(Plan::Simple {
                    argv,
                    pairs,
                    external,
                })
            }
            _ => Ok(Plan::Compound),
        }
    }

    /// How a *special* builtin reports an error.
    ///
    /// POSIX says an error in one of them ends a non-interactive shell, and
    /// that is not a formality: `set -- a; shift 2` leaves the parameters
    /// untouched, so a script that carried on would work on the wrong argument
    /// and quietly do the wrong thing. A prompt keeps its shell, because losing
    /// one to a typo would be unusable.
    ///
    /// It is deliberately *not* the same as a non-zero status: `false` and a
    /// `.`-ed script that ends in `return 1` are failures, not errors, and
    /// neither ends the shell.
    fn special_error(&mut self, code: u8) -> Run<()> {
        self.status = code;
        if self.interactive {
            Ok(())
        } else {
            Err(Flow::Exit(code))
        }
    }

    /// Would `name` be run as a separate process?
    fn is_external(&self, name: &[u8]) -> bool {
        !is_special(name) && !self.funcs.contains_key(name) && !is_regular(name)
    }

    /// One command of any kind.
    fn run_cmd(&mut self, cmd: &Cmd, io: &Io) -> Run<()> {
        if let CmdKind::Simple { assigns, words } = &cmd.kind {
            let (argv, pairs) = self.expand_simple(assigns, words)?;
            return self.run_resolved(&argv, &pairs, &cmd.redirs, io);
        }
        if let CmdKind::FuncDef { name, body } = &cmd.kind {
            self.funcs.insert(name.clone(), Rc::clone(body));
            self.status = 0;
            return Ok(());
        }
        // Every compound command takes its redirections for the whole of its
        // body: `while …; done > f` writes one file, not one per iteration.
        let Some(io) = self.apply_redirs(&cmd.redirs, io)? else {
            self.status = 2;
            return Ok(());
        };
        // A compound command's redirections cover its diagnostics too:
        // `{ nosuch; } 2>err` writes into `err`, not to the terminal.
        let _err = ErrScope::new(&io);
        // …and they are given back when it ends, taking any `exec` performed
        // inside with them. See [`Shell::in_redir_scope`].
        self.in_redir_scope(!cmd.redirs.is_empty(), |sh| sh.run_compound(cmd, &io))
    }

    /// The body of a compound command, with its redirections already applied.
    ///
    /// Split out of [`Shell::run_cmd`] only so that the scope guard there can
    /// take it as a closure; there is no other caller.
    fn run_compound(&mut self, cmd: &Cmd, io: &Io) -> Run<()> {
        let io = io.clone();
        match &cmd.kind {
            CmdKind::Subshell(list) => self.subshell(|sh| sh.run_list(list, &io)),
            CmdKind::Group(list) => self.run_list(list, &io),
            CmdKind::If { arms, otherwise } => {
                for (cond, body) in arms {
                    self.in_cond(true, |sh| sh.run_list(cond, &io))?;
                    if self.status == 0 {
                        return self.run_list(body, &io);
                    }
                }
                match otherwise {
                    Some(list) => self.run_list(list, &io),
                    None => {
                        // An `if` with no taken branch is a success, not the
                        // failure of its last condition.
                        self.status = 0;
                        Ok(())
                    }
                }
            }
            CmdKind::Loop { until, cond, body } => {
                let mut last = 0u8;
                loop {
                    self.in_cond(true, |sh| sh.run_list(cond, &io))?;
                    if (self.status == 0) == *until {
                        break;
                    }
                    let again = self.loop_body(body, &io)?;
                    last = self.status;
                    if !again {
                        break;
                    }
                }
                self.status = last;
                Ok(())
            }
            CmdKind::For { var, words, body } => {
                let items = match words {
                    Some(ws) => {
                        let mut v = Vec::new();
                        for w in ws {
                            v.extend(self.expand_word(w)?);
                        }
                        v
                    }
                    // `for x; do …` iterates the positional parameters, which
                    // is not the same as `for x in; do …` — that iterates
                    // nothing.
                    None => self.params.clone(),
                };
                let mut last = 0u8;
                for item in items {
                    self.set_var(var, item);
                    let again = self.loop_body(body, &io)?;
                    last = self.status;
                    if !again {
                        break;
                    }
                }
                self.status = last;
                Ok(())
            }
            CmdKind::Case { word, arms } => {
                let subject = self.expand_to_bytes(word)?;
                self.status = 0;
                for (pats, body) in arms {
                    for p in pats {
                        let pat = self.expand_pattern(p)?;
                        if fnmatch::fnmatch(&pat, &subject, fnmatch::Flags::NONE) {
                            return self.run_list(body, &io);
                        }
                    }
                }
                Ok(())
            }
            // Handled above; repeated here only because the match must be
            // exhaustive.
            CmdKind::Simple { .. } | CmdKind::FuncDef { .. } => Ok(()),
        }
    }

    /// One pass through a loop body. `Ok(false)` means `break`.
    fn loop_body(&mut self, body: &List, io: &Io) -> Run<bool> {
        self.loops = self.loops.saturating_add(1);
        let r = self.run_list(body, io);
        self.loops = self.loops.saturating_sub(1);
        match r {
            Ok(()) => Ok(true),
            // `break 2` leaves this loop *and* asks the enclosing one to leave
            // as well, which is why the count is decremented and rethrown.
            Err(Flow::Break(n)) if n > 1 => Err(Flow::Break(n.saturating_sub(1))),
            Err(Flow::Break(_)) => Ok(false),
            Err(Flow::Continue(n)) if n > 1 => Err(Flow::Continue(n.saturating_sub(1))),
            Err(Flow::Continue(_)) => Ok(true),
            Err(e) => Err(e),
        }
    }

    /// Expand a simple command: its words, then its assignments.
    fn expand_simple(&mut self, assigns: &[(Vec<u8>, Word)], words: &[Word]) -> Run<(Argv, Pairs)> {
        // Cleared here and read back in [`Shell::run_resolved`]: a command that
        // is *only* assignments takes its status from the last command
        // substitution its values contained, so `x=$(false)` answers 1 while a
        // plain `x=1` answers 0 even if some earlier command substituted.
        self.last_subst = None;
        let mut argv = Vec::new();
        for w in words {
            argv.extend(self.expand_word(w)?);
        }
        let mut pairs = Vec::with_capacity(assigns.len());
        for (name, w) in assigns {
            let v = self.expand_to_bytes(w)?;
            pairs.push((name.clone(), v));
        }
        Ok((argv, pairs))
    }

    /// Run a simple command whose words are already expanded.
    fn run_resolved(
        &mut self,
        argv: &[Vec<u8>],
        pairs: &[(Vec<u8>, Vec<u8>)],
        redirs: &[Redir],
        io: &Io,
    ) -> Run<()> {
        if argv.is_empty() {
            // `> f` with no command still creates the file, and `x=1` with no
            // command still assigns — to this shell, permanently.
            if self.apply_redirs(redirs, io)?.is_none() {
                self.status = 2;
                return Ok(());
            }
            self.status = self.last_subst.take().unwrap_or(0);
            for (name, value) in pairs {
                self.set_var(name, value.clone());
            }
            return Ok(());
        }
        if self.opts.xtrace() {
            self.trace(argv, pairs);
        }
        // `exec` with no command is the one command whose redirections outlive
        // it: POSIX says they become the shell's, from here to the end of the
        // enclosing construct. The table is built exactly as any other
        // command's would be — from the one in force here, which already
        // carries whatever an enclosing `{ … } > out` established — and then
        // kept instead of dropped. Handled here rather than in `bi_exec`
        // because only this far up are the redirections still separable from
        // the command.
        if argv.len() == 1 && argv.first().is_some_and(|a| a == b"exec") {
            match self.apply_redirs(redirs, io)? {
                Some(new) => {
                    self.exec_io = Some(new);
                    self.status = 0;
                }
                // `exec` is a special builtin, and a redirection it cannot make
                // is an error rather than a failing command: a script that went
                // on would write everything after it to the wrong place.
                None => return self.special_error(2),
            }
            return Ok(());
        }
        let Some(io) = self.apply_redirs(redirs, io)? else {
            self.status = 2;
            return Ok(());
        };
        // `nosuchcommand 2>err` must put `not found` in `err`, and the message
        // comes from this shell rather than from the command that never ran.
        let _err = ErrScope::new(&io);
        // The three in-process cases below can reach an `exec` — through a
        // function body, through `eval`, through a `.`-ed file — so a command
        // written with redirections of its own gives them back on the way out,
        // exactly as a compound one does. An external cannot: it is a different
        // process and its `exec` is its own business.
        let scoped = !redirs.is_empty();
        let name = argv.first().cloned().unwrap_or_default();
        if is_special(&name) {
            // A special builtin's assignments outlive it: `IFS=, set …` leaves
            // `IFS` changed, where `IFS=, read x` does not.
            for (n, v) in pairs {
                self.set_var(n, v.clone());
            }
            return self.in_redir_scope(scoped, |sh| sh.run_builtin(argv, &io));
        }
        if let Some(body) = self.funcs.get(name.as_slice()).cloned() {
            for (n, v) in pairs {
                self.set_var(n, v.clone());
            }
            return self.in_redir_scope(scoped, |sh| sh.run_function(&body, argv, &io));
        }
        if is_regular(&name) {
            return self.in_redir_scope(scoped, |sh| {
                sh.with_temporary(pairs, |s| s.run_builtin(argv, &io))
            });
        }
        // A `None` needs nothing done to it: `spawn_external` has already said
        // what went wrong and set the status, and 127 and 126 are the two
        // answers a script tests for.
        if let Some(mut child) = self.spawn_external(argv, pairs, &io) {
            // Before the wait, or a child that reads from a pipe this shell
            // still holds the write end of would never see the end of it.
            drop(io);
            match child.wait() {
                Ok(st) => self.status = status_of(&st),
                Err(e) => {
                    diag!("sh: wait: {}", strerror(&e));
                    self.status = 1;
                }
            }
        }
        Ok(())
    }

    /// Run `body` with `pairs` in scope, and take them back out again.
    ///
    /// This is what makes `IFS=: read a b` work and `IFS=: true` not leave
    /// `IFS` changed — measured against dash, which answers `[]` to
    /// `v=1 true; echo [$v]`.
    fn with_temporary(
        &mut self,
        pairs: &[(Vec<u8>, Vec<u8>)],
        body: impl FnOnce(&mut Self) -> Run<()>,
    ) -> Run<()> {
        let saved: Vec<(Vec<u8>, Option<Var>)> = pairs
            .iter()
            .map(|(n, _)| (n.clone(), self.vars.get(n).cloned()))
            .collect();
        for (n, v) in pairs {
            let exported = self.var(n).is_some_and(|x| x.exported);
            self.vars.insert(
                n.clone(),
                Var {
                    value: v.clone(),
                    exported,
                },
            );
        }
        let r = body(self);
        for (n, old) in saved {
            match old {
                Some(v) => {
                    self.vars.insert(n, v);
                }
                None => {
                    self.vars.remove(&n);
                }
            }
        }
        r
    }

    /// `set -x`: the command as it will actually run, on standard error.
    fn trace(&mut self, argv: &[Vec<u8>], pairs: &[(Vec<u8>, Vec<u8>)]) {
        let mut line = b"+".to_vec();
        for (n, v) in pairs {
            line.push(b' ');
            line.extend_from_slice(n);
            line.push(b'=');
            line.extend_from_slice(v);
        }
        for a in argv {
            line.push(b' ');
            line.extend_from_slice(a);
        }
        line.push(b'\n');
        stdfd::diag_bytes(&line);
    }

    /// Call a shell function.
    fn run_function(&mut self, body: &Rc<Cmd>, argv: &[Vec<u8>], io: &Io) -> Run<()> {
        // Native stack, no trampoline: a runaway recursion would overflow it
        // and die without a diagnostic, so it is bounded here instead. dash's
        // limit is the stack too, but dash has a `SIGSEGV` handler and we do
        // not.
        if self.funcs_running >= 128 {
            diag!("sh: too many nested function calls");
            self.status = 2;
            return Ok(());
        }
        let args = argv.get(1..).unwrap_or_default().to_vec();
        let saved_params = std::mem::replace(&mut self.params, args);
        // `break` inside a function does not leave a loop *outside* it.
        let saved_loops = std::mem::replace(&mut self.loops, 0);
        self.funcs_running = self.funcs_running.saturating_add(1);
        let r = self.run_cmd(body, io);
        self.funcs_running = self.funcs_running.saturating_sub(1);
        self.loops = saved_loops;
        self.params = saved_params;
        match r {
            Err(Flow::Return(n)) => {
                self.status = n;
                Ok(())
            }
            other => other,
        }
    }

    /// Find `name` on `PATH`, or say why it cannot be run.
    ///
    /// `Err(127)` is "not found" and `Err(126)` is "found but not executable" —
    /// the two statuses every shell agrees on and that `command -v`-shaped
    /// scripts test for. A name containing `/` is used as written and never
    /// searched, which is what makes `./configure` mean this directory.
    fn find_program(&self, name: &[u8]) -> Result<std::path::PathBuf, u8> {
        if name.contains(&b'/') {
            let p = std::path::PathBuf::from(os_from_bytes(name));
            return executable(&p).map(|()| p);
        }
        let path = self
            .param_value(b"PATH")
            .unwrap_or_else(|| b"/bin:/usr/bin".to_vec());
        let mut denied = false;
        for dir in path.split(|&b| b == b':') {
            // An empty `PATH` element means the current directory, which is a
            // POSIX rule and not a bug: `PATH=:/bin` really does search `.`.
            let dir: &[u8] = if dir.is_empty() { b"." } else { dir };
            let full = join_path(dir, name);
            let p = std::path::PathBuf::from(os_from_bytes(&full));
            match executable(&p) {
                Ok(()) => return Ok(p),
                Err(126) => denied = true,
                Err(_) => {}
            }
        }
        Err(if denied { 126 } else { 127 })
    }

    /// Start an external program. `None` means it could not be started, and the
    /// diagnostic and status have already been dealt with.
    fn spawn_external(
        &mut self,
        argv: &[Vec<u8>],
        pairs: &[(Vec<u8>, Vec<u8>)],
        io: &Io,
    ) -> Option<std::process::Child> {
        let mut cmd = self.build_command(argv, pairs, io)?;
        match cmd.spawn() {
            Ok(child) => Some(child),
            Err(e) => {
                let why = coreutils::errmsg::strerror(&e);
                diag!(
                    "sh: {}: {why}",
                    coreutils::quote::quotef(argv.first().map_or(b"", Vec::as_slice))
                );
                self.status = if e.kind() == std::io::ErrorKind::NotFound {
                    127
                } else {
                    126
                };
                None
            }
        }
    }

    /// Everything about running `argv` except the running: the resolved path,
    /// the argument vector, the environment and the three descriptors.
    ///
    /// Shared by [`Self::spawn_external`] and the `exec` builtin, which differ
    /// only in whether the current process survives.
    fn build_command(
        &mut self,
        argv: &[Vec<u8>],
        pairs: &[(Vec<u8>, Vec<u8>)],
        io: &Io,
    ) -> Option<Command> {
        let name = argv.first().cloned().unwrap_or_default();
        let path = match self.find_program(&name) {
            Ok(p) => p,
            Err(code) => {
                if code == 126 {
                    diag!("sh: {}: Permission denied", coreutils::quote::quotef(&name));
                } else {
                    diag!("sh: {}: not found", coreutils::quote::quotef(&name));
                }
                self.status = code;
                return None;
            }
        };
        let mut cmd = Command::new(&path);
        #[cfg(unix)]
        {
            // `argv[0]` is what was *written*, not what was found: a program
            // that changes behaviour with its own name — `sh` invoked as
            // `-sh`, busybox invoked as `ls` — must see the name the script
            // used.
            use std::os::unix::process::CommandExt;
            cmd.arg0(os_from_bytes(&name));
        }
        for a in argv.iter().skip(1) {
            cmd.arg(os_from_bytes(a));
        }
        // The child's environment is the exported variables and nothing else —
        // not this process's, which may hold variables the script unset.
        cmd.env_clear();
        for (k, v) in self.child_env() {
            cmd.env(k, v);
        }
        for (k, v) in pairs {
            cmd.env(os_from_bytes(k), os_from_bytes(v));
        }
        let (Ok(fd0), Ok(fd1), Ok(fd2)) = (stdio_for(io, 0), stdio_for(io, 1), stdio_for(io, 2))
        else {
            diag!(
                "sh: {}: bad file descriptor",
                coreutils::quote::quotef(&name)
            );
            self.status = 1;
            return None;
        };
        cmd.stdin(fd0).stdout(fd1).stderr(fd2);
        #[cfg(unix)]
        {
            let extra = match extra_fds(io) {
                Ok(v) => v,
                Err(e) => {
                    diag!("sh: {}: {}", coreutils::quote::quotef(&name), strerror(&e));
                    self.status = 1;
                    return None;
                }
            };
            if !extra.is_empty() {
                use std::os::fd::AsRawFd;
                use std::os::unix::process::CommandExt;
                // SAFETY: the closure runs in the forked child between `fork`
                // and `exec`, where only async-signal-safe calls are allowed.
                // `dup`, `dup2` and `close` are three of them, and nothing else
                // happens here — no allocation, no locking, no `std::io`
                // beyond reading `errno`. The `File`s the closure owns were
                // opened before the fork, so the descriptors it reads are open.
                unsafe {
                    cmd.pre_exec(move || {
                        for (n, f) in &extra {
                            let mut src = f.as_raw_fd();
                            // A duplicate may already *be* the target: `dup`
                            // hands out the lowest free descriptor, and the
                            // target is free precisely because nothing has
                            // claimed it. `dup2` onto itself does nothing and
                            // leaves close-on-exec set, so the child would lose
                            // the descriptor at `exec`; going through a third
                            // one forces a real `dup2`, which clears the flag.
                            let spare = if src == *n { dup(src) } else { -1 };
                            if src == *n {
                                if spare < 0 {
                                    return Err(std::io::Error::last_os_error());
                                }
                                src = spare;
                            }
                            let r = dup2(src, *n);
                            if spare >= 0 {
                                close(spare);
                            }
                            if r < 0 {
                                return Err(std::io::Error::last_os_error());
                            }
                        }
                        Ok(())
                    });
                }
            }
        }
        // Everything this shell has buffered must reach the terminal before the
        // child writes to it, or `echo a; ls` comes out with `a` last.
        let mut out = stdfd::Stream::stdout();
        let _ = out.flush();
        drop(out);
        Some(cmd)
    }

    /// `$(…)` and `` `…` ``.
    ///
    /// The output goes to a temporary file rather than a pipe for the reason
    /// [`temp_file`] gives: there is no second process to drain a pipe, so a
    /// command that writes more than the pipe holds would deadlock against the
    /// shell that is waiting to read it.
    fn command_subst(&mut self, body: &[u8]) -> Run<Vec<u8>> {
        let list = match parse(body) {
            Ok(l) => l,
            Err(e) => {
                diag!("sh: {}", parse_err_text(&e));
                return Err(Flow::Exit(2));
            }
        };
        let (file, guard) = match temp_file("subst", b"") {
            Ok(pair) => pair,
            Err(e) => {
                diag!("sh: cannot capture output: {}", strerror(&e));
                return Err(Flow::Exit(2));
            }
        };
        // On top of the shell's own table, not on nothing: `exec 2>log` before
        // a `$(…)` still sends what the substituted command complains about to
        // the log. Only descriptor 1 is the substitution's own.
        let mut io = self.base_io();
        io.set(1, Fd::Open(Rc::new(file)));
        let snap = self.snapshot();
        let r = self.run_list(&list, &io);
        // The write end has to go before the read, or a buffered tail is lost.
        drop(io);
        let status = match r {
            Ok(()) => self.status,
            Err(Flow::Exit(n) | Flow::Return(n)) => n,
            Err(Flow::Break(_) | Flow::Continue(_)) => self.status,
        };
        self.restore(snap);
        // A command substitution is a subshell — but its *status* is visible,
        // which is what makes `x=$(false); echo $?` answer 1.
        self.status = status;
        self.last_subst = Some(status);
        let mut out = Vec::new();
        match File::open(guard.path()) {
            Ok(mut f) => {
                if let Err(e) = f.read_to_end(&mut out) {
                    diag!("sh: cannot read captured output: {}", strerror(&e));
                    return Err(Flow::Exit(2));
                }
            }
            Err(e) => {
                diag!("sh: cannot read captured output: {}", strerror(&e));
                return Err(Flow::Exit(2));
            }
        }
        // POSIX strips *all* trailing newlines, not one: `x=$(printf 'a\n\n')`
        // is one byte long.
        while out.last() == Some(&b'\n') {
            out.pop();
        }
        Ok(out)
    }
}

/// Connect stage `i`'s output to stage `j`'s input.
///
/// `both_here` picks the mechanism. Two in-process stages get a file, because
/// they cannot run at the same time: the writer finishes, then the reader
/// starts, and a pipe between them would wedge the moment the writer produced
/// more than the pipe buffer. Anything with a real process on one end gets a
/// real pipe, which is what makes the two run concurrently.
fn connect(stages: &mut [Io], i: usize, j: usize, both_here: bool) -> std::io::Result<()> {
    if both_here {
        let (w, guard) = temp_file("pipe", b"")?;
        let guard = Rc::new(guard);
        let r = File::open(guard.path())?;
        if let Some(s) = stages.get_mut(i) {
            s.set(1, Fd::Open(Rc::new(w)));
            s.temps.push(Rc::clone(&guard));
        }
        if let Some(s) = stages.get_mut(j) {
            s.set(0, Fd::Open(Rc::new(r)));
            s.temps.push(guard);
        }
        return Ok(());
    }
    let (r, w) = pipe_files()?;
    if let Some(s) = stages.get_mut(i) {
        s.set(1, Fd::Open(Rc::new(w)));
    }
    if let Some(s) = stages.get_mut(j) {
        s.set(0, Fd::Open(Rc::new(r)));
    }
    Ok(())
}

/// Can `p` be run as a program?
///
/// On the development host a missing `.exe` is retried with one, because
/// Windows keeps the suffix in the file name where unix does not, and without
/// this no test on the host could run a program at all. The shipped shell never
/// takes that branch.
fn executable(p: &std::path::Path) -> Result<(), u8> {
    #[cfg(windows)]
    if !p.is_file() && p.extension().is_none() {
        let mut with_ext = p.as_os_str().to_os_string();
        with_ext.push(".exe");
        let alt = std::path::PathBuf::from(with_ext);
        if alt.is_file() {
            return Ok(());
        }
    }
    let Ok(md) = std::fs::metadata(p) else {
        return Err(127);
    };
    if md.is_dir() {
        return Err(126);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if md.permissions().mode() & 0o111 == 0 {
            return Err(126);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// builtins
// ---------------------------------------------------------------------------

/// `bytes` in a form the shell will read back as itself.
///
/// Single quotes throughout, because they are the only quoting in the language
/// with no escapes inside them: whatever is between them is exactly itself. The
/// `'` that cannot appear there is written by leaving the quotes and coming
/// back — `'\''` — which is what every shell does and what `set` and
/// `export -p` output has to survive being fed back in.
fn shell_quote(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![b'\''];
    for &b in bytes {
        if b == b'\'' {
            out.extend_from_slice(b"'\\''");
        } else {
            out.push(b);
        }
    }
    out.push(b'\'');
    out
}

/// dash's `echo`: backslash escapes are always live, and `\c` ends the output.
///
/// Not GNU's `echo`, and the difference is measured: dash prints `-e x` for
/// `echo -e x` and a tab for `echo 'a\tb'`, where GNU prints `x` and `a\tb`.
/// A script written for `/bin/sh` expects the dash reading, and this is
/// `/bin/sh`.
fn echo_escapes(src: &[u8], out: &mut Vec<u8>) -> bool {
    let mut i = 0usize;
    while let Some(&b) = src.get(i) {
        i = i.saturating_add(1);
        if b != b'\\' {
            out.push(b);
            continue;
        }
        let Some(&e) = src.get(i) else {
            out.push(b'\\');
            break;
        };
        i = i.saturating_add(1);
        match e {
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'c' => return true,
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0b),
            b'\\' => out.push(b'\\'),
            b'0' => {
                let mut v: u32 = 0;
                let mut k = 0u32;
                while k < 3
                    && let Some(&d) = src.get(i)
                    && d.is_ascii_digit()
                    && d < b'8'
                {
                    v = v.saturating_mul(8).saturating_add(u32::from(d - b'0'));
                    i = i.saturating_add(1);
                    k = k.saturating_add(1);
                }
                out.push(u8::try_from(v & 0xff).unwrap_or(0));
            }
            // An unknown escape is not an error: the backslash stands.
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }
    false
}

/// Remove `read`'s backslashes, recording which bytes they protected.
///
/// The mask matters as much as the removal: `read a b` given `a\ b` must set
/// `a` to `a b`, so the escaped space has to survive field splitting even
/// though it is in `IFS`.
fn unescape(line: &[u8]) -> (Vec<u8>, Vec<bool>) {
    let mut data = Vec::with_capacity(line.len());
    let mut esc = Vec::with_capacity(line.len());
    let mut i = 0usize;
    while let Some(&b) = line.get(i) {
        i = i.saturating_add(1);
        if b != b'\\' {
            data.push(b);
            esc.push(false);
            continue;
        }
        match line.get(i) {
            Some(&n) => {
                i = i.saturating_add(1);
                data.push(n);
                esc.push(true);
            }
            None => {
                data.push(b'\\');
                esc.push(false);
            }
        }
    }
    (data, esc)
}

/// Split a line the way `read` does: at most `max` fields, the last one taking
/// whatever is left.
///
/// Not [`split_fields`], which is the *expansion* rule and produces as many
/// fields as it finds. `read a b` given `1 2 3` sets `b` to `2 3`.
fn split_read(data: &[u8], esc: &[bool], ifs: &[u8], max: usize) -> Vec<Vec<u8>> {
    let white = |b: u8| matches!(b, b' ' | b'\t' | b'\n') && ifs.contains(&b);
    let delim = |i: usize| -> bool {
        !esc.get(i).copied().unwrap_or(false)
            && data.get(i).copied().is_some_and(|b| ifs.contains(&b))
    };
    let space = |i: usize| -> bool {
        !esc.get(i).copied().unwrap_or(false) && data.get(i).copied().is_some_and(white)
    };
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut i = 0usize;
    while i < data.len() && space(i) {
        i = i.saturating_add(1);
    }
    while i < data.len() {
        if out.len().saturating_add(1) >= max {
            let mut rest = data.get(i..).unwrap_or_default().to_vec();
            let mut end = data.len();
            while end > i && space(end.saturating_sub(1)) {
                end = end.saturating_sub(1);
            }
            rest.truncate(end.saturating_sub(i));
            out.push(rest);
            return out;
        }
        let start = i;
        while i < data.len() && !delim(i) {
            i = i.saturating_add(1);
        }
        out.push(data.get(start..i).unwrap_or_default().to_vec());
        while i < data.len() && space(i) {
            i = i.saturating_add(1);
        }
        if i < data.len() && delim(i) {
            i = i.saturating_add(1);
            while i < data.len() && space(i) {
                i = i.saturating_add(1);
            }
        }
    }
    out
}

impl Shell {
    /// Write to a builtin's standard output. `false` means it did not arrive.
    fn emit(&mut self, io: &Io, bytes: &[u8]) -> bool {
        match write_fd(io, 1, bytes) {
            Ok(()) => true,
            // A closed reader is how `… | head -1` ends, not an error to
            // report — every utility in this crate treats it the same way.
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => false,
            Err(e) => {
                diag!("sh: write error: {}", strerror(&e));
                false
            }
        }
    }

    /// Assign and export, for the variables the shell maintains itself.
    fn set_exported(&mut self, name: &[u8], value: Vec<u8>) {
        self.vars.insert(
            name.to_vec(),
            Var {
                value,
                exported: true,
            },
        );
    }

    fn run_builtin(&mut self, argv: &[Vec<u8>], io: &Io) -> Run<()> {
        let name = argv.first().cloned().unwrap_or_default();
        let args = argv.get(1..).unwrap_or_default();
        match name.as_slice() {
            b":" | b"true" => {
                self.status = 0;
                Ok(())
            }
            b"false" => {
                self.status = 1;
                Ok(())
            }
            b"echo" => self.bi_echo(args, io),
            b"pwd" => self.bi_pwd(io),
            b"cd" => self.bi_cd(args, io),
            b"exit" => {
                let n = self.number_arg(args, self.status)?;
                Err(Flow::Exit(n))
            }
            b"return" => {
                let n = self.number_arg(args, self.status)?;
                Err(Flow::Return(n))
            }
            b"break" => self.bi_loopctl(args, false),
            b"continue" => self.bi_loopctl(args, true),
            b"shift" => self.bi_shift(args),
            b"export" => self.bi_export(args, io),
            b"unset" => self.bi_unset(args),
            b"set" => self.bi_set(args, io),
            b"eval" => self.bi_eval(args, io),
            b"exec" => self.bi_exec(args, io),
            b"read" => self.bi_read(args, io),
            b"." | b"source" => self.bi_dot(args, io),
            b"wait" => self.bi_wait(args),
            // `run_builtin` is only reached through the two tables above, so
            // this is unreachable in practice — and answering 127 rather than
            // panicking is what a shell does with a name it cannot run.
            _ => {
                self.status = 127;
                Ok(())
            }
        }
    }

    /// The optional numeric argument of `exit`, `return` and friends.
    fn number_arg(&mut self, args: &[Vec<u8>], default: u8) -> Run<u8> {
        match args.first() {
            None => Ok(default),
            Some(a) => match parse_int(a) {
                // `exit 300` is `exit 44`: the status is a byte, and truncating
                // is what every shell does with a wider number.
                Some(v) => Ok(status_byte(i32::try_from(v).unwrap_or(0))),
                None => {
                    diag!("sh: illegal number: {}", String::from_utf8_lossy(a));
                    Ok(2)
                }
            },
        }
    }

    fn bi_echo(&mut self, args: &[Vec<u8>], io: &Io) -> Run<()> {
        let mut newline = true;
        let mut rest = args;
        // One `-n`, and only as the first word: `echo -n -n` prints `-n`.
        if args.first().is_some_and(|a| a.as_slice() == b"-n") {
            newline = false;
            rest = args.get(1..).unwrap_or_default();
        }
        let mut out = Vec::new();
        let mut stopped = false;
        for (i, a) in rest.iter().enumerate() {
            if i > 0 {
                out.push(b' ');
            }
            if echo_escapes(a, &mut out) {
                stopped = true;
                break;
            }
        }
        if newline && !stopped {
            out.push(b'\n');
        }
        let ok = self.emit(io, &out);
        self.status = u8::from(!ok);
        Ok(())
    }

    fn bi_pwd(&mut self, io: &Io) -> Run<()> {
        match env::current_dir() {
            Ok(d) => {
                let mut line = bytes_of_os(d.as_os_str());
                line.push(b'\n');
                let ok = self.emit(io, &line);
                self.status = u8::from(!ok);
            }
            Err(e) => {
                diag!("sh: pwd: {}", strerror(&e));
                self.status = 1;
            }
        }
        Ok(())
    }

    fn bi_cd(&mut self, args: &[Vec<u8>], io: &Io) -> Run<()> {
        // `-L` and `-P` are accepted and ignored: this shell has no logical
        // path to keep, since it tracks `PWD` from the kernel's answer.
        let target = args
            .iter()
            .find(|a| !matches!(a.as_slice(), b"-L" | b"-P" | b"--"))
            .cloned();
        let mut announce = false;
        let dir = match target {
            None => match self.param_value(b"HOME") {
                Some(h) if !h.is_empty() => h,
                _ => {
                    diag!("sh: cd: HOME not set");
                    self.status = 2;
                    return Ok(());
                }
            },
            Some(t) if t == b"-" => {
                // `cd -` prints where it went, which is what makes it usable
                // interactively.
                announce = true;
                match self.param_value(b"OLDPWD") {
                    Some(p) if !p.is_empty() => p,
                    _ => {
                        diag!("sh: cd: OLDPWD not set");
                        self.status = 2;
                        return Ok(());
                    }
                }
            }
            Some(t) => t,
        };
        let was = env::current_dir().ok();
        let path = std::path::PathBuf::from(os_from_bytes(&dir));
        if let Err(e) = env::set_current_dir(&path) {
            diag!("sh: cd: {}: {}", quotef_os(&path), strerror(&e));
            self.status = 2;
            return Ok(());
        }
        if let Some(old) = was {
            self.set_exported(b"OLDPWD", bytes_of_os(old.as_os_str()));
        }
        if let Ok(now) = env::current_dir() {
            let bytes = bytes_of_os(now.as_os_str());
            if announce {
                let mut line = bytes.clone();
                line.push(b'\n');
                self.emit(io, &line);
            }
            self.set_exported(b"PWD", bytes);
        }
        self.status = 0;
        Ok(())
    }

    fn bi_loopctl(&mut self, args: &[Vec<u8>], cont: bool) -> Run<()> {
        let word = if cont { "continue" } else { "break" };
        let n = match args.first() {
            None => 1u32,
            Some(a) => match parse_int(a).and_then(|v| u32::try_from(v).ok()) {
                Some(v) if v > 0 => v,
                _ => {
                    diag!("sh: {word}: illegal number: {}", String::from_utf8_lossy(a));
                    self.status = 2;
                    return Ok(());
                }
            },
        };
        // Outside a loop this is a no-op rather than an error, which is what
        // dash does — a function that ends in `break` is common enough.
        if self.loops == 0 {
            self.status = 0;
            return Ok(());
        }
        let n = n.min(self.loops);
        Err(if cont {
            Flow::Continue(n)
        } else {
            Flow::Break(n)
        })
    }

    fn bi_shift(&mut self, args: &[Vec<u8>]) -> Run<()> {
        let n = match args.first() {
            None => 1usize,
            Some(a) => match parse_int(a).and_then(|v| usize::try_from(v).ok()) {
                Some(v) => v,
                None => {
                    diag!("sh: shift: illegal number: {}", String::from_utf8_lossy(a));
                    return self.special_error(2);
                }
            },
        };
        if n > self.params.len() {
            diag!("sh: shift: can't shift that many");
            return self.special_error(2);
        }
        self.params.drain(0..n);
        self.status = 0;
        Ok(())
    }

    fn bi_export(&mut self, args: &[Vec<u8>], io: &Io) -> Run<()> {
        if args.is_empty() || args.first().is_some_and(|a| a.as_slice() == b"-p") {
            let mut out = Vec::new();
            for (k, v) in &self.vars {
                if !v.exported {
                    continue;
                }
                out.extend_from_slice(b"export ");
                out.extend_from_slice(k);
                out.push(b'=');
                out.extend_from_slice(&shell_quote(&v.value));
                out.push(b'\n');
            }
            let ok = self.emit(io, &out);
            self.status = u8::from(!ok);
            return Ok(());
        }
        self.status = 0;
        for a in args {
            let (name, value) = match a.iter().position(|&b| b == b'=') {
                Some(i) => (
                    a.get(..i).unwrap_or_default().to_vec(),
                    Some(a.get(i.saturating_add(1)..).unwrap_or_default().to_vec()),
                ),
                None => (a.clone(), None),
            };
            if !is_name(&name) {
                diag!(
                    "sh: export: {}: bad variable name",
                    coreutils::quote::quotef(&name)
                );
                return self.special_error(2);
            }
            // `export X` on an unset variable exports it *empty*, so that a
            // child sees the name — POSIX is explicit and scripts rely on it.
            let existing = self.var(&name).map(|v| v.value.clone());
            let v = value.or(existing).unwrap_or_default();
            self.set_exported(&name, v);
        }
        Ok(())
    }

    /// `unset [-fv] NAME…`.
    ///
    /// With neither option the name is a *variable if there is one and a
    /// function otherwise*, which is POSIX's rule and not the obvious one:
    /// `unset f` after `f() { … }` really does remove the function, so a
    /// script can retract a definition without knowing it was one.
    fn bi_unset(&mut self, args: &[Vec<u8>]) -> Run<()> {
        // `None` is "neither option given" — the fall-back rule above.
        let mut kind: Option<bool> = None;
        let mut i = 0usize;
        while let Some(a) = args.get(i) {
            match a.as_slice() {
                b"-v" => kind = Some(true),
                b"-f" => kind = Some(false),
                b"--" => {
                    i = i.saturating_add(1);
                    break;
                }
                _ => break,
            }
            i = i.saturating_add(1);
        }
        self.status = 0;
        for a in args.get(i..).unwrap_or_default() {
            match kind {
                Some(true) => {
                    self.vars.remove(a);
                }
                Some(false) => {
                    self.funcs.remove(a);
                }
                None => {
                    if self.vars.remove(a).is_none() {
                        self.funcs.remove(a);
                    }
                }
            }
        }
        Ok(())
    }

    fn bi_set(&mut self, args: &[Vec<u8>], io: &Io) -> Run<()> {
        if args.is_empty() {
            let mut out = Vec::new();
            for (k, v) in &self.vars {
                out.extend_from_slice(k);
                out.push(b'=');
                out.extend_from_slice(&shell_quote(&v.value));
                out.push(b'\n');
            }
            let ok = self.emit(io, &out);
            self.status = u8::from(!ok);
            return Ok(());
        }
        let mut i = 0usize;
        let mut params: Option<Vec<Vec<u8>>> = None;
        while let Some(a) = args.get(i).cloned() {
            let Some(&first) = a.first() else { break };
            if (first != b'-' && first != b'+') || a.len() == 1 {
                break;
            }
            if a == b"--" {
                i = i.saturating_add(1);
                params = Some(args.get(i..).unwrap_or_default().to_vec());
                i = args.len();
                break;
            }
            let on = first == b'-';
            let mut j = 1usize;
            while let Some(&c) = a.get(j) {
                j = j.saturating_add(1);
                if c == b'o' {
                    match args.get(i.saturating_add(1)) {
                        Some(nm) => {
                            i = i.saturating_add(1);
                            let text = String::from_utf8_lossy(nm).into_owned();
                            if !self.opts.set_named(&text, on) {
                                diag!("sh: set: illegal option name: {text}");
                                return self.special_error(2);
                            }
                        }
                        // `set -o` with nothing after it lists the settings.
                        None => {
                            let mut out = Vec::new();
                            for (k, name) in OPT_LETTERS.iter().zip(OPT_NAMES.iter()) {
                                let state = if self.opts.get(*k) { "on" } else { "off" };
                                out.extend_from_slice(format!("{name:<12}{state}\n").as_bytes());
                            }
                            let ok = self.emit(io, &out);
                            self.status = u8::from(!ok);
                            return Ok(());
                        }
                    }
                    continue;
                }
                if !self.opts.set(c, on) {
                    diag!(
                        "sh: set: illegal option: {}{}",
                        char::from(first),
                        char::from(c)
                    );
                    return self.special_error(2);
                }
            }
            i = i.saturating_add(1);
        }
        if params.is_none() && i < args.len() {
            params = Some(args.get(i..).unwrap_or_default().to_vec());
        }
        if let Some(p) = params {
            self.params = p;
        }
        self.status = 0;
        Ok(())
    }

    fn bi_eval(&mut self, args: &[Vec<u8>], io: &Io) -> Run<()> {
        let mut text = Vec::new();
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                text.push(b' ');
            }
            text.extend_from_slice(a);
        }
        if text.iter().all(u8::is_ascii_whitespace) {
            self.status = 0;
            return Ok(());
        }
        match parse(&text) {
            Ok(list) => self.run_list(&list, io),
            Err(e) => {
                diag!("sh: eval: {}", parse_err_text(&e));
                self.special_error(2)
            }
        }
    }

    /// `exec [command [arg…]]`.
    ///
    /// With a command this really does replace the process on unix, which is
    /// what makes `exec "$@"` in a wrapper script leave no shell behind.
    ///
    /// *Without* one it never gets here: [`Shell::run_resolved`] intercepts
    /// that form, because the redirections then belong to the shell rather than
    /// to the command, and by this point the two are no longer distinguishable.
    /// The arm below is what is left — `exec` reached with neither a command
    /// nor redirections, which does nothing and succeeds.
    fn bi_exec(&mut self, args: &[Vec<u8>], io: &Io) -> Run<()> {
        if args.is_empty() {
            self.status = 0;
            return Ok(());
        }
        let Some(mut cmd) = self.build_command(args, &[], io) else {
            return Err(Flow::Exit(self.status));
        };
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let e = cmd.exec();
            let why = coreutils::errmsg::strerror(&e);
            diag!(
                "sh: exec: {}: {why}",
                coreutils::quote::quotef(args.first().map_or(b"", Vec::as_slice))
            );
            Err(Flow::Exit(if e.kind() == std::io::ErrorKind::NotFound {
                127
            } else {
                126
            }))
        }
        #[cfg(not(unix))]
        {
            // No `exec` on the development host: spawn, wait, and exit with the
            // child's status, which is observationally the same but for the
            // process id.
            match cmd.spawn() {
                Ok(mut child) => match child.wait() {
                    Ok(st) => Err(Flow::Exit(status_of(&st))),
                    Err(e) => {
                        diag!("sh: exec: {}", strerror(&e));
                        Err(Flow::Exit(126))
                    }
                },
                Err(e) => {
                    diag!("sh: exec: {}", strerror(&e));
                    Err(Flow::Exit(126))
                }
            }
        }
    }

    fn bi_read(&mut self, args: &[Vec<u8>], io: &Io) -> Run<()> {
        let mut raw = false;
        let mut i = 0usize;
        while let Some(a) = args.get(i) {
            match a.as_slice() {
                b"-r" => raw = true,
                b"--" => {
                    i = i.saturating_add(1);
                    break;
                }
                _ => break,
            }
            i = i.saturating_add(1);
        }
        let mut names: Vec<Vec<u8>> = args.get(i..).unwrap_or_default().to_vec();
        if names.is_empty() {
            names.push(b"REPLY".to_vec());
        }
        for n in &names {
            if !is_name(n) {
                diag!(
                    "sh: read: {}: bad variable name",
                    coreutils::quote::quotef(n)
                );
                self.status = 2;
                return Ok(());
            }
        }
        let mut line = Vec::new();
        let mut eof = false;
        loop {
            let chunk = match read_line_fd(io, 0) {
                Ok(c) => c,
                Err(e) => {
                    diag!("sh: read: {}", strerror(&e));
                    self.status = 2;
                    return Ok(());
                }
            };
            if chunk.is_empty() {
                if line.is_empty() {
                    eof = true;
                }
                break;
            }
            let mut chunk = chunk;
            let had_newline = chunk.last() == Some(&b'\n');
            if had_newline {
                chunk.pop();
            }
            // A trailing backslash joins the next line — unless `-r`, whose
            // whole meaning is that a backslash is an ordinary byte.
            if !raw && had_newline && chunk.last() == Some(&b'\\') {
                chunk.pop();
                line.extend_from_slice(&chunk);
                continue;
            }
            line.extend_from_slice(&chunk);
            break;
        }
        let (data, esc) = if raw {
            let n = line.len();
            (line, vec![false; n])
        } else {
            unescape(&line)
        };
        let ifs = self.ifs();
        let fields = split_read(&data, &esc, &ifs, names.len());
        for (k, n) in names.iter().enumerate() {
            let v = fields.get(k).cloned().unwrap_or_default();
            self.set_var(n, v);
        }
        self.status = u8::from(eof);
        Ok(())
    }

    /// The file `.` should read: `PATH` is searched, but the file need not be
    /// executable — it is read, not run.
    fn find_source(&self, name: &[u8]) -> Option<std::path::PathBuf> {
        if name.contains(&b'/') {
            let p = std::path::PathBuf::from(os_from_bytes(name));
            return p.is_file().then_some(p);
        }
        let path = self
            .param_value(b"PATH")
            .unwrap_or_else(|| b"/bin:/usr/bin".to_vec());
        for dir in path.split(|&b| b == b':') {
            let dir: &[u8] = if dir.is_empty() { b"." } else { dir };
            let p = std::path::PathBuf::from(os_from_bytes(&join_path(dir, name)));
            if p.is_file() {
                return Some(p);
            }
        }
        None
    }

    fn bi_dot(&mut self, args: &[Vec<u8>], io: &Io) -> Run<()> {
        let Some(file) = args.first() else {
            diag!("sh: .: filename argument required");
            return self.special_error(2);
        };
        let Some(path) = self.find_source(file) else {
            // `cannot open`, and status 2, because `.` is not a command lookup:
            // 127 would say "no such command" about a *file*, and a script that
            // tested for it would be testing the wrong thing. dash uses the same
            // two, and POSIX asks only that the status be non-zero and that a
            // non-interactive shell end — which `special_error` does.
            diag!(
                "sh: .: cannot open {}: {}",
                coreutils::quote::quotef(file),
                strerror(&std::io::Error::from(std::io::ErrorKind::NotFound))
            );
            return self.special_error(2);
        };
        let text = match std::fs::read(&path) {
            Ok(t) => t,
            Err(e) => {
                diag!("sh: {}: {}", quotef_os(&path), strerror(&e));
                return self.special_error(2);
            }
        };
        let list = match parse(&text) {
            Ok(l) => l,
            Err(e) => {
                diag!("sh: {}: {}", quotef_os(&path), parse_err_text(&e));
                return self.special_error(2);
            }
        };
        let saved = args
            .get(1..)
            .filter(|extra| !extra.is_empty())
            .map(|extra| std::mem::replace(&mut self.params, extra.to_vec()));
        // A dotted script may `return`, which is why this counts as a call.
        self.funcs_running = self.funcs_running.saturating_add(1);
        let r = self.run_list(&list, io);
        self.funcs_running = self.funcs_running.saturating_sub(1);
        if let Some(p) = saved {
            self.params = p;
        }
        match r {
            Err(Flow::Return(n)) => {
                self.status = n;
                Ok(())
            }
            other => other,
        }
    }

    fn bi_wait(&mut self, args: &[Vec<u8>]) -> Run<()> {
        let mut children = std::mem::take(&mut self.bg);
        if args.is_empty() {
            for mut c in children {
                // A child that cannot be waited for is already gone, which is
                // the outcome `wait` wanted.
                let _ = c.wait();
            }
            self.status = 0;
            return Ok(());
        }
        let mut status = 0u8;
        for a in args {
            let Some(pid) = parse_int(a).and_then(|v| u32::try_from(v).ok()) else {
                diag!("sh: wait: {}: bad process id", String::from_utf8_lossy(a));
                self.bg = children;
                self.status = 2;
                return Ok(());
            };
            match children.iter().position(|c| c.id() == pid) {
                Some(k) => {
                    let mut c = children.remove(k);
                    status = match c.wait() {
                        Ok(st) => status_of(&st),
                        Err(_) => 127,
                    };
                }
                // POSIX: an unknown process id is 127, not an error.
                None => status = 127,
            }
        }
        self.bg = children;
        self.status = status;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// entry
// ---------------------------------------------------------------------------

impl Shell {
    /// Parse and run a whole script.
    fn run_text(&mut self, text: &[u8]) -> u8 {
        if self.opts.verbose() {
            diag_out(text);
        }
        let list = match parse(text) {
            Ok(l) => l,
            Err(e) => {
                diag!("sh: {}", parse_err_text(&e));
                return 2;
            }
        };
        // `-n` reads and checks and does not run, which is how a script is
        // syntax-checked before it is installed.
        if self.opts.noexec() {
            return 0;
        }
        let io = self.base_io();
        let r = self.run_list(&list, &io);
        self.finish(r)
    }

    /// Turn the outcome of a top-level list into an exit status.
    fn finish(&self, r: Run<()>) -> u8 {
        match r {
            Ok(()) => self.status,
            Err(Flow::Exit(n) | Flow::Return(n)) => n,
            // `break` outside a loop reached the top: the shell keeps its
            // status rather than inventing one.
            Err(Flow::Break(_) | Flow::Continue(_)) => self.status,
        }
    }

    /// Read commands until end of input.
    ///
    /// Line at a time, re-parsing the accumulation after each, because that is
    /// the only way to know whether more input is needed: a quote or a
    /// here-document can leave a construct open across any number of lines, and
    /// only the parser knows. [`ParseErr::Incomplete`] is exactly that answer.
    ///
    /// That is for a *prompt*. A script arriving on standard input is read
    /// whole first, because the script and the commands in it share descriptor
    /// 0: `printf 'read x\necho [$x]\nvalue-line\n' | sh` must print `[]` and
    /// then complain about `value-line`, and a shell that read a line at a time
    /// would hand `value-line` to `read` as data instead of running it. Both
    /// dash and bash slurp for the same reason.
    fn repl(&mut self) -> u8 {
        if !self.interactive {
            let mut text = Vec::new();
            if let Err(e) = std::io::stdin().read_to_end(&mut text) {
                diag!("sh: read error: {}", strerror(&e));
                return 2;
            }
            return self.run_text(&text);
        }
        let stdin = self.base_io();
        let mut pending: Vec<u8> = Vec::new();
        loop {
            let which: &[u8] = if pending.is_empty() { b"PS1" } else { b"PS2" };
            let fallback: &[u8] = if pending.is_empty() { b"$ " } else { b"> " };
            let prompt = self.param_value(which).unwrap_or_else(|| fallback.to_vec());
            // The prompt goes to standard error, as dash's does, so that
            // `sh -i < script > out` does not put it in the output.
            diag_out(&prompt);
            let line = match read_line_fd(&stdin, 0) {
                Ok(l) => l,
                Err(e) => {
                    diag!("sh: read error: {}", strerror(&e));
                    return 2;
                }
            };
            if line.is_empty() {
                if !pending.is_empty() {
                    diag!("sh: unexpected end of file");
                    return 2;
                }
                return self.status;
            }
            pending.extend_from_slice(&line);
            match parse(&pending) {
                Err(ParseErr::Incomplete) => continue,
                Err(e) => {
                    // A syntax error only skips a line at a prompt; the script
                    // case is `run_text`'s, and it stops there.
                    diag!("sh: {}", parse_err_text(&e));
                    pending.clear();
                    self.status = 2;
                }
                Ok(list) => {
                    if self.opts.verbose() {
                        diag_out(&pending);
                    }
                    pending.clear();
                    if self.opts.noexec() {
                        continue;
                    }
                    let io = self.base_io();
                    match self.run_list(&list, &io) {
                        Ok(()) => {}
                        Err(Flow::Exit(n) | Flow::Return(n)) => return n,
                        Err(Flow::Break(_) | Flow::Continue(_)) => {}
                    }
                    // Show what has been done before asking for more.
                    flush_stdout();
                }
            }
        }
    }
}

coreutils::guard_std_fds!();

fn main() -> ExitCode {
    run_main()
}

/// Everything the shell does, so that the flush and its verdict are one place.
fn run_main() -> ExitCode {
    stdfd::restore();
    // `args_os`, not `args`: a script's name and its arguments are file names
    // and file contents, and `env::args()` panics on a byte that is not UTF-8.
    // That panic is `sh.rs:55` in the `argv-utf8` baseline.
    let argv: Vec<Vec<u8>> = env::args_os().map(|a| bytes_of_os(&a)).collect();
    let name = argv.first().cloned().unwrap_or_else(|| b"sh".to_vec());
    let mut sh = Shell::new(name);
    let mut command: Option<Vec<u8>> = None;
    let mut from_stdin = false;
    let mut force_interactive = false;
    let mut i = 1usize;
    while let Some(a) = argv.get(i).cloned() {
        if a == b"--" {
            i = i.saturating_add(1);
            break;
        }
        let Some(&first) = a.first() else { break };
        if (first != b'-' && first != b'+') || a.len() == 1 {
            break;
        }
        let on = first == b'-';
        let mut j = 1usize;
        while let Some(&c) = a.get(j) {
            j = j.saturating_add(1);
            match c {
                b'c' if on => {
                    i = i.saturating_add(1);
                    match argv.get(i) {
                        Some(text) => command = Some(text.clone()),
                        None => {
                            diag!("sh: -c requires an argument");
                            return ExitCode::from(2);
                        }
                    }
                }
                b's' => from_stdin = on,
                b'i' => force_interactive = on,
                b'o' => {
                    i = i.saturating_add(1);
                    match argv.get(i) {
                        Some(nm) => {
                            let text = String::from_utf8_lossy(nm).into_owned();
                            if !sh.opts.set_named(&text, on) {
                                diag!("sh: illegal option name: {text}");
                                return ExitCode::from(2);
                            }
                        }
                        None => {
                            diag!("sh: -o requires an argument");
                            return ExitCode::from(2);
                        }
                    }
                }
                _ => {
                    if !sh.opts.set(c, on) {
                        diag!("sh: illegal option: {}{}", char::from(first), char::from(c));
                        return ExitCode::from(2);
                    }
                }
            }
        }
        i = i.saturating_add(1);
        if command.is_some() {
            break;
        }
    }
    let rest: Vec<Vec<u8>> = argv.get(i..).unwrap_or_default().to_vec();
    let code = if let Some(text) = command {
        // `sh -c CMD [NAME [ARG…]]`: the word after the command is `$0`, not
        // `$1`. Scripts that get this wrong are why the form exists.
        let mut it = rest.into_iter();
        if let Some(n) = it.next() {
            sh.name = n;
        }
        sh.params = it.collect();
        sh.run_text(&text)
    } else if let Some(script) = rest.first().filter(|_| !from_stdin).cloned() {
        let path = std::path::PathBuf::from(os_from_bytes(&script));
        sh.name = script;
        sh.params = rest.get(1..).unwrap_or_default().to_vec();
        match std::fs::read(&path) {
            Ok(text) => sh.run_text(&text),
            Err(e) => {
                diag!("sh: {}: {}", quotef_os(&path), strerror(&e));
                127
            }
        }
    } else {
        sh.params = rest;
        sh.interactive = force_interactive || {
            use std::io::IsTerminal;
            std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
        };
        sh.repl()
    };
    stdfd::close_stdout("sh", stdfd::Stream::stdout(), ExitCode::from(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run a script with its standard output captured.
    ///
    /// A file rather than a pipe for the same reason the shell itself uses one:
    /// nothing is draining the far end while this thread runs the script.
    fn run(script: &[u8]) -> (u8, Vec<u8>) {
        let list = parse(script).expect("the test script should parse");
        let (file, guard) = temp_file("selftest", b"").expect("a temporary file");
        let mut io = Io::default();
        io.set(1, Fd::Open(Rc::new(file)));
        let mut sh = Shell::new(b"sh".to_vec());
        let r = sh.run_list(&list, &io);
        drop(io);
        let status = match r {
            Ok(()) => sh.status,
            Err(Flow::Exit(n) | Flow::Return(n)) => n,
            Err(Flow::Break(_) | Flow::Continue(_)) => sh.status,
        };
        let out = std::fs::read(guard.path()).expect("the captured output");
        (status, out)
    }

    /// What a script printed.
    fn out(script: &[u8]) -> String {
        String::from_utf8_lossy(&run(script).1).into_owned()
    }

    /// What a script exited with.
    fn code(script: &[u8]) -> u8 {
        run(script).0
    }

    fn first_word(src: &[u8]) -> Word {
        let list = parse(src).expect("parse");
        let (ao, _) = list.items.first().expect("one item");
        let cmd = ao.first.cmds.first().expect("one command");
        match &cmd.kind {
            CmdKind::Simple { words, .. } => words.first().cloned().expect("one word"),
            _ => Vec::new(),
        }
    }

    // -- the lexer ------------------------------------------------------

    #[test]
    fn a_single_quoted_run_is_one_quoted_piece() {
        assert_eq!(first_word(b"'a b'"), vec![Piece::Quo(b"a b".to_vec())]);
    }

    #[test]
    fn an_empty_quoted_word_is_still_a_word() {
        // The distinction the old line-scanner could not make: `cmd ''` passes
        // one empty argument, not none.
        assert_eq!(first_word(b"''"), vec![Piece::Quo(Vec::new())]);
        assert_eq!(out(b"set -- ''; echo $#"), "1\n");
    }

    #[test]
    fn a_backslash_quotes_exactly_one_byte() {
        assert_eq!(
            first_word(b"a\\ b"),
            vec![
                Piece::Lit(b"a".to_vec()),
                Piece::Quo(b" ".to_vec()),
                Piece::Lit(b"b".to_vec())
            ]
        );
    }

    #[test]
    fn a_dollar_inside_single_quotes_is_a_dollar() {
        assert_eq!(first_word(b"'$x'"), vec![Piece::Quo(b"$x".to_vec())]);
    }

    #[test]
    fn a_tilde_is_only_special_at_the_start() {
        assert!(matches!(
            first_word(b"~").first(),
            Some(Piece::Tilde(name)) if name.is_empty()
        ));
        assert_eq!(first_word(b"a~b"), vec![Piece::Lit(b"a~b".to_vec())]);
    }

    #[test]
    fn an_unterminated_quote_asks_for_more_input() {
        // What makes the interactive loop able to read a second line.
        assert_eq!(parse(b"echo 'a"), Err(ParseErr::Incomplete));
        assert_eq!(parse(b"if true; then"), Err(ParseErr::Incomplete));
        assert_eq!(parse(b"echo $(echo"), Err(ParseErr::Incomplete));
    }

    // -- the parser -----------------------------------------------------

    #[test]
    fn a_compound_command_on_one_line_parses() {
        // The whole reason for the rewrite: this line used to be looked up as a
        // program named `true;`.
        assert_eq!(out(b"if true; then echo yes; fi"), "yes\n");
        assert_eq!(out(b"if false; then echo y; else echo n; fi"), "n\n");
        assert_eq!(
            out(b"if false; then echo a; elif true; then echo b; fi"),
            "b\n"
        );
    }

    // Every test here uses builtins only. External programs belong in
    // `scripts/sh-diff.sh`, which runs the shell against dash under a real
    // Unix: the host this suite compiles on has no `[`, no `cat` and no
    // `/dev/null`, and a test that reaches for one of them is testing the
    // host rather than the shell. `until [ … ]` in particular does not fail
    // here — it *hangs*, because a missing `[` leaves a non-zero status and
    // `until` loops on exactly that.

    #[test]
    fn a_loop_runs_and_stops() {
        assert_eq!(
            out(b"i=0; while true; do i=$((i+1)); echo $i; case $i in 3) break;; esac; done"),
            "1\n2\n3\n"
        );
        assert_eq!(out(b"for x in a b; do echo $x; done"), "a\nb\n");
        assert_eq!(out(b"set -- p q; for x; do echo $x; done"), "p\nq\n");
        assert_eq!(out(b"for x in; do echo $x; done; echo end"), "end\n");
        // A `while` whose condition is false the first time runs nothing.
        assert_eq!(out(b"while false; do echo never; done; echo end"), "end\n");
    }

    #[test]
    fn until_is_while_inverted() {
        assert_eq!(
            out(b"i=0; at2() { case $i in 2) return 0;; esac; return 1; }\n\
                  until at2; do echo $i; i=$((i+1)); done"),
            "0\n1\n"
        );
    }

    #[test]
    fn break_and_continue_take_a_count() {
        assert_eq!(
            out(b"for a in 1 2; do for b in x y; do echo $a$b; break 2; done; done"),
            "1x\n"
        );
        assert_eq!(
            out(b"for a in 1 2 3; do case $a in 2) continue;; esac; echo $a; done"),
            "1\n3\n"
        );
        // `continue 2` restarts the outer loop, so the inner one runs once per
        // outer iteration and never reaches its second value.
        assert_eq!(
            out(b"for a in 1 2; do for b in x y; do echo $a$b; continue 2; done; done"),
            "1x\n2x\n"
        );
    }

    #[test]
    fn case_matches_patterns_in_order() {
        assert_eq!(
            out(b"case abc in a*) echo one;; *) echo two;; esac"),
            "one\n"
        );
        assert_eq!(
            out(b"case zzz in a*) echo one;; *) echo two;; esac"),
            "two\n"
        );
        assert_eq!(out(b"case b in a|b) echo alt;; esac"), "alt\n");
        // A quoted pattern is text, not a pattern.
        assert_eq!(out(b"case '*' in '*') echo lit;; esac"), "lit\n");
    }

    #[test]
    fn a_function_takes_arguments_and_returns() {
        assert_eq!(out(b"f() { echo $1-$2; }; f a b"), "a-b\n");
        assert_eq!(code(b"f() { return 3; }; f"), 3);
        // `$#` inside a function is the function's own count.
        assert_eq!(out(b"set -- x y z; f() { echo $#; }; f a"), "1\n");
    }

    #[test]
    fn a_group_runs_here_and_a_subshell_does_not() {
        assert_eq!(out(b"x=1; { x=2; }; echo $x"), "2\n");
        assert_eq!(out(b"x=1; ( x=2 ); echo $x"), "1\n");
        // `exit` inside a subshell ends the subshell only.
        assert_eq!(out(b"( exit 4 ); echo $?"), "4\n");
    }

    #[test]
    fn bang_inverts_a_pipeline() {
        assert_eq!(out(b"! false; echo $?"), "0\n");
        assert_eq!(out(b"! true; echo $?"), "1\n");
    }

    #[test]
    fn and_or_short_circuits() {
        assert_eq!(out(b"true && echo yes || echo no"), "yes\n");
        assert_eq!(out(b"false && echo yes || echo no"), "no\n");
    }

    // -- expansion ------------------------------------------------------

    #[test]
    fn parameters_expand_and_split() {
        assert_eq!(out(b"x='a b'; set -- $x; echo $#"), "2\n");
        assert_eq!(out(b"x='a b'; set -- \"$x\"; echo $#"), "1\n");
    }

    #[test]
    fn the_at_parameter_is_one_field_per_argument() {
        assert_eq!(
            out(b"set -- 'a b' c; for x in \"$@\"; do echo [$x]; done"),
            "[a b]\n[c]\n"
        );
        // Unquoted, it splits like anything else.
        assert_eq!(
            out(b"set -- 'a b' c; for x in $@; do echo [$x]; done"),
            "[a]\n[b]\n[c]\n"
        );
    }

    #[test]
    fn the_star_parameter_joins_with_the_first_byte_of_ifs() {
        assert_eq!(out(b"set -- a b c; IFS=:; echo \"$*\""), "a:b:c\n");
        assert_eq!(out(b"set -- a b c; IFS=; echo \"$*\""), "abc\n");
    }

    #[test]
    fn the_word_operators_all_work() {
        assert_eq!(out(b"echo ${u:-def}"), "def\n");
        assert_eq!(out(b"x=; echo [${x:-def}] [${x-def}]"), "[def] []\n");
        assert_eq!(out(b"echo ${u:=set}; echo $u"), "set\nset\n");
        assert_eq!(out(b"x=v; echo ${x:+yes}"), "yes\n");
        assert_eq!(out(b"x=abcdef; echo ${#x}"), "6\n");
        // The length of an unset parameter is zero, not an error.
        assert_eq!(out(b"echo ${#nosuch}"), "0\n");
        assert_eq!(out(b"f=a.b.c; echo ${f#*.} ${f##*.}"), "b.c c\n");
        assert_eq!(out(b"f=a.b.c; echo ${f%.*} ${f%%.*}"), "a.b a\n");
    }

    #[test]
    fn an_unset_parameter_under_nounset_is_fatal() {
        assert_eq!(code(b"set -u; echo $nosuch; echo reached"), 2);
        assert_eq!(out(b"set -u; echo $nosuch; echo reached"), "");
    }

    #[test]
    fn a_colon_error_reports_and_stops() {
        assert_eq!(code(b"echo ${nosuch?}; echo reached"), 2);
    }

    #[test]
    fn field_splitting_follows_the_ifs_rules() {
        // IFS whitespace collapses; an IFS non-whitespace byte does not, so
        // `a::b` is three fields and `a:` is one.
        let e = Expanded {
            bytes: b"a::b".to_vec(),
            quoted: vec![false; 4],
            quotes: false,
        };
        assert_eq!(split_fields(&e, b":").len(), 3);
        let e = Expanded {
            bytes: b"a:".to_vec(),
            quoted: vec![false; 2],
            quotes: false,
        };
        assert_eq!(split_fields(&e, b":").len(), 1);
        let e = Expanded {
            bytes: b"  a  b  ".to_vec(),
            quoted: vec![false; 8],
            quotes: false,
        };
        assert_eq!(split_fields(&e, b" \t\n").len(), 2);
    }

    #[test]
    fn command_substitution_strips_every_trailing_newline() {
        assert_eq!(out(b"echo [$(echo hi)]"), "[hi]\n");
        assert_eq!(out(b"echo a$(echo b)c"), "abc\n");
        assert_eq!(out(b"echo [`echo hi`]"), "[hi]\n");
        // Its status is visible even though its variables are not.
        assert_eq!(out(b"x=$(false); echo $?"), "1\n");
        // …but only the *last* one, and only for the command it belongs to.
        assert_eq!(out(b"x=$(false); y=1; echo $?"), "0\n");
        assert_eq!(out(b"x=$(false)$(true); echo $?"), "0\n");
        assert_eq!(out(b"y=$(v=5); echo [$v]"), "[]\n");
    }

    #[test]
    fn nested_command_substitution_parses() {
        assert_eq!(out(b"echo $(echo nested $(echo deep))"), "nested deep\n");
    }

    #[test]
    fn quoting_survives_expansion() {
        // The bug that made `echo \"hi $X\" tail` print its tail twice: the
        // quoting has to be recorded per byte, not recovered by re-scanning.
        assert_eq!(out(b"X=v; echo \"hi $X\" tail"), "hi v tail\n");
        assert_eq!(out(b"X='a b'; echo \"[$X]\""), "[a b]\n");
    }

    // -- arithmetic -----------------------------------------------------

    #[test]
    fn arithmetic_follows_c_precedence() {
        let mut sh = Shell::new(b"sh".to_vec());
        for (src, want) in [
            (&b"2+3*4"[..], 14),
            (b"(2+3)*4", 20),
            (b"7/2", 3),
            (b"-3%2", -1),
            (b"1<<4", 16),
            (b"-16>>2", -4),
            (b"1 && 0", 0),
            (b"0 || 3", 1),
            (b"!0", 1),
            (b"~0", -1),
            (b"1 == 1", 1),
            (b"2 != 2", 0),
            (b"1 ? 5 : 6", 5),
            (b"0 ? 5 : 6", 6),
            (b"0x10", 16),
            (b"010", 8),
            (b"", 0),
        ] {
            assert_eq!(
                arith_eval(&mut sh, src),
                Ok(want),
                "{}",
                String::from_utf8_lossy(src)
            );
        }
    }

    #[test]
    fn arithmetic_assigns_to_shell_variables() {
        assert_eq!(out(b"echo $((x = 3)); echo $x"), "3\n3\n");
        assert_eq!(out(b"x=5; echo $((x += 2)); echo $x"), "7\n7\n");
        assert_eq!(out(b"echo $((2+3))"), "5\n");
    }

    #[test]
    fn division_by_zero_is_an_error_and_not_a_crash() {
        let mut sh = Shell::new(b"sh".to_vec());
        assert!(arith_eval(&mut sh, b"1/0").is_err());
        assert!(arith_eval(&mut sh, b"1%0").is_err());
        // A silly shift count answers, rather than reaching undefined
        // behaviour in the host.
        assert_eq!(arith_eval(&mut sh, b"1 << 200"), Ok(0));
        assert_eq!(arith_eval(&mut sh, b"-1 >> 200"), Ok(-1));
    }

    // -- builtins -------------------------------------------------------

    #[test]
    fn echo_is_dashs_echo_and_not_gnus() {
        // Measured against dash: escapes are always live, `-e` is not an
        // option, and only a leading `-n` is one.
        assert_eq!(out(b"echo 'a\\tb'"), "a\tb\n");
        assert_eq!(out(b"echo -e x"), "-e x\n");
        assert_eq!(out(b"echo -n x"), "x");
        assert_eq!(out(b"echo a b"), "a b\n");
        assert_eq!(out(b"echo 'a\\cb'"), "a");
    }

    #[test]
    fn set_manages_options_and_parameters() {
        assert_eq!(out(b"set -- a b c; echo $#; echo $@"), "3\na b c\n");
        assert_eq!(out(b"set -- ; echo $#"), "0\n");
        assert_eq!(
            out(b"set -f; case $- in *f*) echo noglob;; esac"),
            "noglob\n"
        );
    }

    #[test]
    fn shift_moves_the_parameters_along() {
        assert_eq!(out(b"set -- a b c; shift; echo $@"), "b c\n");
        assert_eq!(out(b"set -- a b c; shift 2; echo $@"), "c\n");
        assert_eq!(code(b"set -- a; shift 2"), 2);
    }

    #[test]
    fn unset_removes_a_variable_and_a_function() {
        assert_eq!(out(b"x=1; unset x; echo [$x]"), "[]\n");
        // Once the function is gone the name is looked up as a program, and
        // there is none — 127, the status POSIX reserves for "not found".
        // (That lookup prints one line to the real standard error. It is the
        // shell reporting correctly, not a failure of the test.)
        assert_eq!(code(b"f() { return 0; }; unset -f f; f"), 127);
        // With no option the variable goes first and the function only if
        // there was no variable — so one `unset f` here still leaves `f`
        // callable, and a second one does not.
        assert_eq!(out(b"f() { echo f; }; f=v; unset f; f; unset f; f"), "f\n");
        assert_eq!(code(b"f() { return 0; }; f=v; unset f; unset f; f"), 127);
    }

    #[test]
    fn a_temporary_assignment_does_not_outlive_a_regular_builtin() {
        // Measured: dash answers `[]`.
        assert_eq!(out(b"v=1 true; echo [$v]"), "[]\n");
        // But it does outlive a special one.
        assert_eq!(out(b"v=1 :; echo [$v]"), "[1]\n");
    }

    #[test]
    fn read_splits_on_ifs_and_the_last_variable_takes_the_rest() {
        assert_eq!(
            split_read(b"1 2 3", &[false; 5], b" \t\n", 2),
            vec![b"1".to_vec(), b"2 3".to_vec()]
        );
        assert_eq!(
            split_read(b"a:b", &[false; 3], b":", 3),
            vec![b"a".to_vec(), b"b".to_vec()]
        );
        assert_eq!(
            split_read(b"  x  ", &[false; 5], b" \t\n", 1),
            vec![b"x".to_vec()]
        );
    }

    #[test]
    fn read_takes_a_here_document() {
        assert_eq!(out(b"read x <<EOF\nhello\nEOF\necho [$x]"), "[hello]\n");
        assert_eq!(
            out(b"read a b <<EOF\none two three\nEOF\necho [$a][$b]"),
            "[one][two three]\n"
        );
    }

    #[test]
    fn an_escaped_space_survives_read() {
        let (data, esc) = unescape(b"a\\ b");
        assert_eq!(data, b"a b".to_vec());
        assert_eq!(esc, vec![false, true, false]);
        assert_eq!(split_read(&data, &esc, b" \t\n", 9), vec![b"a b".to_vec()]);
    }

    #[test]
    fn eval_parses_its_argument_as_a_command() {
        assert_eq!(out(b"x='echo hi'; eval $x"), "hi\n");
        assert_eq!(out(b"eval 'a=1; echo $a'"), "1\n");
    }

    #[test]
    fn exit_status_is_a_byte() {
        assert_eq!(code(b"exit 300"), 44);
        assert_eq!(code(b"false; exit"), 1);
        assert_eq!(code(b"exit"), 0);
    }

    // -- here-documents and redirection ---------------------------------

    #[test]
    fn a_quoted_here_delimiter_stops_expansion() {
        assert_eq!(out(b"x=v; read y <<EOF\n$x\nEOF\necho [$y]"), "[v]\n");
        assert_eq!(out(b"x=v; read y <<'EOF'\n$x\nEOF\necho [$y]"), "[$x]\n");
        // A backslash still quotes inside an unquoted body.
        assert_eq!(out(b"x=v; read y <<EOF\n\\$x\nEOF\necho [$y]"), "[$x]\n");
    }

    #[test]
    fn a_dash_here_delimiter_strips_leading_tabs() {
        assert_eq!(
            out(b"read y <<-EOF\n\tindented\n\tEOF\necho [$y]"),
            "[indented]\n"
        );
    }

    #[test]
    fn a_here_document_can_exceed_a_pipe_buffer() {
        // A pipe would wedge here: nothing is draining the far end while the
        // shell writes, so anything past the 64 KiB kernel buffer would block
        // forever. A temporary file cannot. 140 KB of body proves it.
        let body = "x\n".repeat(70_000);
        let script =
            format!("n=0\nwhile read l; do n=$((n+1)); done <<'EOF'\n{body}EOF\necho $n\n");
        let (status, got) = run(script.as_bytes());
        assert_eq!(status, 0);
        assert_eq!(String::from_utf8_lossy(&got), "70000\n");
    }

    /// A scratch directory that removes itself, so a failing assertion cannot
    /// leave one behind — an early `panic!` skips any cleanup written after it.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = env::temp_dir().join(format!("sh-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("a scratch directory");
            Self(dir)
        }

        /// A path inside it, quoted so the shell reads it as one word —
        /// on this host it is full of backslashes and may hold a space.
        fn quoted(&self, name: &str) -> Vec<u8> {
            shell_quote(&bytes_of_os(self.0.join(name).as_os_str()))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_redirection_writes_truncates_appends_and_reads() {
        let dir = Scratch::new("redir");
        let f = dir.quoted("out");
        let f = String::from_utf8_lossy(&f).into_owned();
        // `>` truncates, `>>` appends, `<` reads back — and the second `>`
        // must not see the first line.
        let script = format!(
            "echo one > {f}\necho two > {f}\necho three >> {f}\n\
             while read l; do echo [$l]; done < {f}\n"
        );
        assert_eq!(out(script.as_bytes()), "[two]\n[three]\n");
    }

    #[test]
    fn noclobber_refuses_an_existing_file() {
        let dir = Scratch::new("noclobber");
        let f = dir.quoted("out");
        let f = String::from_utf8_lossy(&f).into_owned();
        let script =
            format!("echo one > {f}\nset -C\necho two > {f}\necho [$?]\nread l < {f}\necho [$l]\n");
        // The redirection fails, so the command never runs and the file keeps
        // its first line. Status 2 is what dash reports for a failed
        // redirection on a simple command.
        assert_eq!(out(script.as_bytes()), "[2]\n[one]\n");
    }

    #[test]
    fn a_redirection_that_cannot_open_leaves_the_command_unrun() {
        let dir = Scratch::new("noent");
        let f = dir.quoted("missing/deeper/file");
        let f = String::from_utf8_lossy(&f).into_owned();
        // One diagnostic on the real standard error, and nothing on standard
        // output: `echo` must not run at all.
        let script = format!("echo never < {f}\necho [$?]\n");
        assert_eq!(out(script.as_bytes()), "[2]\n");
    }

    #[test]
    fn a_pipeline_of_builtins_passes_its_output_along() {
        // Both stages run in this one process, so the connection between them
        // is a temporary file rather than a pipe — two builtins cannot take
        // turns on a 64 KiB buffer.
        assert_eq!(out(b"echo hello | { read x; echo [$x]; }"), "[hello]\n");
        assert_eq!(
            out(b"echo a b c | { read x y; echo [$x][$y]; }"),
            "[a][b c]\n"
        );
        // A pipeline reports the status of its *last* stage.
        assert_eq!(out(b"false | true; echo $?"), "0\n");
        assert_eq!(out(b"true | false; echo $?"), "1\n");
    }

    // -- helpers --------------------------------------------------------

    #[test]
    fn shell_quote_round_trips_an_awkward_value() {
        assert_eq!(shell_quote(b"a b"), b"'a b'".to_vec());
        assert_eq!(shell_quote(b"it's"), b"'it'\\''s'".to_vec());
        let script = [
            b"x=".to_vec(),
            shell_quote(b"a 'b' c"),
            b"; echo \"[$x]\"".to_vec(),
        ]
        .concat();
        assert_eq!(out(&script), "[a 'b' c]\n");
    }

    #[test]
    fn status_of_a_wide_code_is_the_low_byte() {
        assert_eq!(status_byte(0), 0);
        assert_eq!(status_byte(300), 44);
        assert_eq!(status_byte(-1), 255);
    }
}
