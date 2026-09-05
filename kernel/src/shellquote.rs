//! One quoting scanner for the whole shell — the single answer to
//! "is this byte quoted, escaped, or a live delimiter?"
//!
//! ## Why this exists
//!
//! `kernel/src/kshell.rs` grew **eleven** independent re-implementations of
//! shell quoting, and they disagree with each other. Two of the disagreements
//! are user-visible bugs today (`known-issues.md` §
//! `A-KSHELL-REDIRECT-MISSED-WHEN-AN-APOSTROPHE-PRECEDES-IT`):
//!
//! | Line typed | What kshell does | What it should do |
//! |---|---|---|
//! | `echo "it's fine" > out` | prints `it's fine > out`, writes no file | writes `it's fine` to `out` |
//! | `echo "it's $HOME"` | prints `$HOME` literally | expands `$HOME` |
//!
//! Both come from the same trigger — an apostrophe inside double quotes —
//! reaching two *different* scanners, each wrong in its own way: the redirect
//! parsers keep a single `bool` toggled by either quote character (so they
//! cannot tell which quote opened the region), and the expander tracks only
//! `'` and has no notion of `"` at all. Patching those two sites would have
//! been the third band-aid on the same fault, so `CLAUDE.md`'s
//! band-aid-accumulation rule says to fix the duplication instead: state the
//! rules **once**, here, and have all eleven callers ask this module.
//!
//! ## The rules, stated once
//!
//! Bash's three contexts, which a `bool` cannot represent:
//!
//! | Context | `'` | `"` | `\` |
//! |---|---|---|---|
//! | unquoted | opens single-quoted | opens double-quoted | escapes *any* next byte |
//! | single-quoted `'…'` | closes | ordinary | **ordinary** — no escapes exist here |
//! | double-quoted `"…"` | ordinary | closes | escapes only `"`, `` ` ``, `$`, `\`, newline |
//!
//! The double-quote rule is the subtle one and is why a two-state scanner is
//! not enough either: in `"C:\dir"` the backslash is an ordinary character and
//! `d` is *not* escaped, while in `"say \"hi\""` it is structural. A scanner
//! that escapes unconditionally inside double quotes eats path separators; one
//! that never escapes cannot quote a quote.
//!
//! ## Bytes, not `str`
//!
//! The scanner takes `&[u8]` even though nine of the eleven callers hold
//! `&str`. Every shell metacharacter (`'`, `"`, `\`, space, `>`, `<`, `,`) is
//! ASCII, so a byte offset this module returns can never land inside a
//! multi-byte UTF-8 sequence — it is always a `char` boundary, and a `&str`
//! caller may slice on it. Taking bytes now means stage (c) of
//! `TD-KSHELL-LINE-EDITOR-IS-UTF8` (moving the parser off `&str` so a file
//! named `re\xffport.txt` is typeable) inherits this scanner rather than
//! needing a second copy of the rules — which is exactly how the eleven got
//! here in the first place.
//!
//! ## Not in `bytestr`
//!
//! [`crate::bytestr`]'s charter is "the `str` methods `[u8]` lacks" —
//! mechanical, policy-free operations. Quoting is shell *policy*: which
//! character escapes what, in which region. Mixing the two would make
//! `bytestr` a place where language semantics accumulate.

use alloc::vec::Vec;

/// Which quoting region a byte sits in.
///
/// A quote character is reported as belonging to the region it delimits, so
/// both the `'` that opens and the `'` that closes a region report
/// [`Ctx::Single`]. Combined with [`Tok::structural`] that makes
/// "this offset is inside quotes" a single test — see [`Tok::is_bare`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctx {
    /// Outside any quotes. Delimiters are live here (unless escaped).
    Unquoted,
    /// Inside `'…'`. Nothing is special but the closing `'`.
    Single,
    /// Inside `"…"`. `$` and `` ` `` still expand; `\` escapes five bytes.
    Double,
}

/// One byte of the input, classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tok {
    /// Byte offset of this byte in the scanned slice.
    pub off: usize,
    /// The byte itself.
    pub byte: u8,
    /// The quoting region this byte belongs to.
    pub ctx: Ctx,
    /// This byte was made literal by a preceding active backslash.
    pub escaped: bool,
    /// This byte is syntax, not data: a quote that opens or closes a region,
    /// or a backslash that escapes the byte after it. Quote removal drops
    /// exactly the bytes for which this is true.
    pub structural: bool,
}

impl Tok {
    /// The byte is ordinary data outside quotes and was not escaped — i.e. if
    /// it is `>` it really is a redirect, and if it is a space it really
    /// separates words.
    ///
    /// This is the test every delimiter search wants, and getting it wrong in
    /// eleven different ways is what this module exists to end.
    #[must_use]
    pub const fn is_bare(&self) -> bool {
        matches!(self.ctx, Ctx::Unquoted) && !self.escaped && !self.structural
    }

    /// The byte survives quote removal (it is data, not syntax).
    #[must_use]
    pub const fn is_literal(&self) -> bool {
        !self.structural
    }

    /// This byte triggers expansion, if it is a `$` or a backtick.
    ///
    /// Deliberately *not* [`Tok::is_bare`]: expansion happens inside double
    /// quotes — `"$HOME"` expands, which is most of the point of double
    /// quotes — but never inside single quotes, and never when escaped.
    ///
    /// Cross-checked against real bash: in `"\$HOME"` the `$` really is in
    /// [`Ctx::Double`], and what suppresses it is [`Tok::escaped`], not the
    /// context. An expander that tests only the context gets that case wrong,
    /// which is how site #7 (`expand_vars_bytes`) came to expand `"\$HOME"`.
    #[must_use]
    pub const fn expands(&self) -> bool {
        !matches!(self.ctx, Ctx::Single) && !self.escaped
    }
}

/// Cursor over a command line, yielding one [`Tok`] per byte.
///
/// Created by [`scan`]. Cheap: no allocation, one pass, no lookbehind.
pub struct QuoteScan<'a> {
    bytes: &'a [u8],
    i: usize,
    ctx: Ctx,
    /// The previous byte was an active backslash, so the next byte is literal
    /// whatever it is — including a quote character, which must not toggle
    /// the context.
    pending_escape: bool,
}

impl QuoteScan<'_> {
    /// The quoting context in effect *after* everything yielded so far.
    ///
    /// After the iterator is exhausted this is [`Ctx::Unquoted`] iff the line
    /// had balanced quotes; callers that care about an unterminated quote
    /// (the line editor's continuation prompt, eventually) test it here.
    #[must_use]
    pub const fn context(&self) -> Ctx {
        self.ctx
    }

    /// Jump the cursor to `off` without interpreting the bytes skipped.
    ///
    /// This exists for the variable expander, which finds the end of a
    /// `$(…)`, `$((…))` or `${…}` body itself and hands the body to a
    /// *recursive* evaluation. Quotes inside a substitution belong to that
    /// inner command, not to the outer line, so skipping them is the correct
    /// reading rather than a shortcut — in bash, `echo $(echo "'")x` leaves
    /// the outer line unquoted, and a scanner that let the inner `'` toggle
    /// the outer context would swallow the rest of the line.
    ///
    /// Moving backwards is refused rather than asserted: a caller that has
    /// already consumed past `off` cannot be given back a context it did not
    /// keep, and silently rewinding would report quoting that never applied.
    pub const fn skip_to(&mut self, off: usize) {
        if off > self.i {
            self.i = off;
            // Whatever the pending backslash was going to escape is inside
            // the skipped region, so it escapes nothing out here.
            self.pending_escape = false;
        }
    }
}

/// Bytes that a backslash escapes *inside double quotes*. Anywhere else in a
/// double-quoted region the backslash is an ordinary character — this is what
/// keeps `"C:\dir"` intact.
const DQ_ESCAPABLE: [u8; 5] = [b'"', b'\\', b'$', b'`', b'\n'];

impl Iterator for QuoteScan<'_> {
    type Item = Tok;

    fn next(&mut self) -> Option<Tok> {
        let b = *self.bytes.get(self.i)?;
        let off = self.i;
        self.i = self.i.saturating_add(1);

        if self.pending_escape {
            self.pending_escape = false;
            return Some(Tok {
                off,
                byte: b,
                ctx: self.ctx,
                escaped: true,
                structural: false,
            });
        }

        let tok = match self.ctx {
            // No escapes exist inside `'…'`; only `'` is special. This is why
            // `'it\'s'` is not a legal way to write an apostrophe in bash
            // (and why we must not accept it here either).
            Ctx::Single => {
                let structural = b == b'\'';
                if structural {
                    self.ctx = Ctx::Unquoted;
                }
                Tok {
                    off,
                    byte: b,
                    ctx: Ctx::Single,
                    escaped: false,
                    structural,
                }
            }
            Ctx::Double => {
                // A backslash here is structural only before one of five
                // bytes; otherwise it is data and does not consume the byte
                // after it.
                if b == b'\\'
                    && self
                        .bytes
                        .get(self.i)
                        .is_some_and(|n| DQ_ESCAPABLE.contains(n))
                {
                    self.pending_escape = true;
                    Tok {
                        off,
                        byte: b,
                        ctx: Ctx::Double,
                        escaped: false,
                        structural: true,
                    }
                } else {
                    let structural = b == b'"';
                    if structural {
                        self.ctx = Ctx::Unquoted;
                    }
                    Tok {
                        off,
                        byte: b,
                        ctx: Ctx::Double,
                        escaped: false,
                        structural,
                    }
                }
            }
            Ctx::Unquoted => match b {
                // A trailing backslash escapes nothing (bash would splice the
                // next input line; kshell has no continuation, so it is data).
                b'\\' if self.i < self.bytes.len() => {
                    self.pending_escape = true;
                    Tok {
                        off,
                        byte: b,
                        ctx: Ctx::Unquoted,
                        escaped: false,
                        structural: true,
                    }
                }
                b'\'' => {
                    self.ctx = Ctx::Single;
                    Tok {
                        off,
                        byte: b,
                        ctx: Ctx::Single,
                        escaped: false,
                        structural: true,
                    }
                }
                b'"' => {
                    self.ctx = Ctx::Double;
                    Tok {
                        off,
                        byte: b,
                        ctx: Ctx::Double,
                        escaped: false,
                        structural: true,
                    }
                }
                _ => Tok {
                    off,
                    byte: b,
                    ctx: Ctx::Unquoted,
                    escaped: false,
                    structural: false,
                },
            },
        };
        Some(tok)
    }
}

/// Scan `bytes` as a shell command line.
#[must_use]
pub fn scan(bytes: &[u8]) -> QuoteScan<'_> {
    QuoteScan {
        bytes,
        i: 0,
        ctx: Ctx::Unquoted,
        pending_escape: false,
    }
}

/// The quoting context left open at the end of `bytes`.
///
/// [`Ctx::Unquoted`] means the quotes balanced. Anything else means a region
/// was opened and never closed, which is not an error on its own: a line
/// being *typed* is unbalanced most of the time it is looked at. Tab
/// completion uses this to close the quote it is completing inside, and the
/// continuation prompt will use it to decide it needs a second line.
///
/// A trailing backslash is not reported here — it escapes the byte that has
/// not been typed yet, which is a question about the *next* line rather than
/// about the context of this one.
#[must_use]
pub fn trailing_context(bytes: &[u8]) -> Ctx {
    let mut sc = scan(bytes);
    // Drain it: the context is only final once every byte has been read.
    for _ in sc.by_ref() {}
    sc.context()
}

/// Offset of the first bare (unquoted, unescaped) occurrence of `needle`.
#[must_use]
pub fn find_bare(bytes: &[u8], needle: u8) -> Option<usize> {
    scan(bytes)
        .find(|t| t.byte == needle && t.is_bare())
        .map(|t| t.off)
}

/// Offsets of the first and last bare occurrences of `needle`, in one pass.
#[must_use]
pub fn bare_positions(bytes: &[u8], needle: u8) -> (Option<usize>, Option<usize>) {
    let mut first = None;
    let mut last = None;
    for t in scan(bytes).filter(|t| t.byte == needle && t.is_bare()) {
        if first.is_none() {
            first = Some(t.off);
        }
        last = Some(t.off);
    }
    (first, last)
}

/// Offset of the first bare ASCII space or tab.
#[must_use]
pub fn find_bare_space(bytes: &[u8]) -> Option<usize> {
    scan(bytes)
        .find(|t| (t.byte == b' ' || t.byte == b'\t') && t.is_bare())
        .map(|t| t.off)
}

/// Split on bare occurrences of `sep`, returning `(start, end)` byte ranges of
/// the pieces, verbatim (quotes and escapes still present).
///
/// Always returns at least one range, so `len() > 1` is the test for "the
/// separator actually occurred outside quotes".
#[must_use]
pub fn split_bare_ranges(bytes: &[u8], sep: u8) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for t in scan(bytes) {
        if t.byte == sep && t.is_bare() {
            out.push((start, t.off));
            start = t.off.saturating_add(1);
        }
    }
    out.push((start, bytes.len()));
    out
}

/// Remove quoting: drop the structural quote and escape bytes, keep the data.
///
/// This is the *dispatch-time* operation. Expansion must run before it and
/// must preserve quoting, or a value that legitimately contains a quote
/// character gets unquoted twice.
#[must_use]
pub fn strip_quotes(bytes: &[u8]) -> Vec<u8> {
    scan(bytes)
        .filter(Tok::is_literal)
        .map(|t| t.byte)
        .collect()
}

/// A word produced by [`split_bare_words`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Word {
    /// Byte range of the word in the input, quotes and escapes still present.
    pub start: usize,
    /// End offset (exclusive).
    pub end: usize,
    /// The word contained at least one quote or escape character. This is how
    /// an explicitly quoted empty word (`''`) is distinguished from no word at
    /// all: a run of bare whitespace never becomes a `Word`, so a `Word` that
    /// strips to nothing is necessarily one the user wrote quotes around.
    /// `kshell::split_words` keeps empty words on exactly this condition — it
    /// dropped them unconditionally until TD-KSHELL (b′).
    pub quoted: bool,
}

/// Split into words on bare whitespace, keeping the pieces verbatim.
///
/// An explicitly quoted empty word (`cmd ''`) is a word: `''` is an argument,
/// and dropping it changes the command's arity.
#[must_use]
pub fn split_bare_words(bytes: &[u8]) -> Vec<Word> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut quoted = false;
    for t in scan(bytes) {
        if (t.byte == b' ' || t.byte == b'\t') && t.is_bare() {
            if let Some(s) = start.take() {
                out.push(Word {
                    start: s,
                    end: t.off,
                    quoted,
                });
            }
            quoted = false;
        } else {
            if start.is_none() {
                start = Some(t.off);
            }
            if t.structural {
                quoted = true;
            }
        }
    }
    if let Some(s) = start {
        out.push(Word {
            start: s,
            end: bytes.len(),
            quoted,
        });
    }
    out
}

/// Start offset of the word containing `cursor` — the word-start scan tab
/// completion needs and does not currently have (it uses `rfind(' ')`, which
/// splits `cat "My Doc` at the space inside the quotes).
#[must_use]
pub fn word_start_at(bytes: &[u8], cursor: usize) -> usize {
    let mut start = 0usize;
    for t in scan(bytes) {
        if t.off >= cursor {
            break;
        }
        if (t.byte == b' ' || t.byte == b'\t') && t.is_bare() {
            start = t.off.saturating_add(1);
        }
    }
    start
}

/// Quote `word` so that scanning the result and stripping quotes yields
/// `word` back, byte for byte.
///
/// Tab completion must insert *this*, not the raw filename: today completing
/// `My Doc.txt` inserts `cat My Doc.txt`, which is two arguments. Single
/// quotes are used because inside them nothing is special, so an arbitrary
/// byte (including `\xff`, `$`, `\`) needs no further thought; an apostrophe
/// in the name is handled by closing, emitting `\'`, and reopening — the
/// standard `'\''` idiom.
#[must_use]
pub fn quote_word(word: &[u8]) -> Vec<u8> {
    let needs = word.is_empty()
        || word.iter().any(|&b| {
            matches!(
                b,
                b' ' | b'\t'
                    | b'\n'
                    | b'\''
                    | b'"'
                    | b'\\'
                    | b'$'
                    | b'`'
                    | b'>'
                    | b'<'
                    | b'|'
                    | b'&'
                    | b';'
                    | b'('
                    | b')'
                    | b'*'
                    | b'?'
                    | b'['
                    | b']'
                    | b'{'
                    | b'}'
                    | b'~'
                    | b'#'
                    | b'!'
                    | b','
            )
        });
    if !needs {
        return word.to_vec();
    }
    let mut out = Vec::with_capacity(word.len().saturating_add(2));
    out.push(b'\'');
    for &b in word {
        if b == b'\'' {
            // close, escaped apostrophe, reopen
            out.extend_from_slice(b"'\\''");
        } else {
            out.push(b);
        }
    }
    out.push(b'\'');
    out
}

/// Boot-time self test. Registered next to [`crate::bytestr::self_test`].
///
/// # Errors
/// Never returns `Err`; the signature matches the boot battery's convention.
#[allow(clippy::too_many_lines)]
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::serial_println;

    serial_println!("  shellquote::self_test 1: three contexts, not a bool");
    // The bug this module exists to kill: an apostrophe inside double quotes
    // must not open a single-quoted region, so the `>` after it is a redirect.
    assert_eq!(find_bare(b"echo \"it's fine\" > out", b'>'), Some(17));
    // And the same line's `$` must still be seen as unquoted-enough to expand
    // (double quotes do not suppress `$`).
    let toks: Vec<Tok> = scan(b"echo \"it's $HOME\"").collect();
    let dollar = toks
        .iter()
        .find(|t| t.byte == b'$')
        .ok_or(crate::error::KernelError::InternalError)?;
    assert_eq!(dollar.ctx, Ctx::Double);
    // Inside single quotes `$` must NOT expand, and `"` is ordinary.
    let toks: Vec<Tok> = scan(b"echo 'it \"is\" $HOME'").collect();
    let dollar = toks
        .iter()
        .find(|t| t.byte == b'$')
        .ok_or(crate::error::KernelError::InternalError)?;
    assert_eq!(dollar.ctx, Ctx::Single);
    for t in &toks {
        if t.byte == b'"' {
            assert!(!t.structural, "a double quote inside '…' is ordinary data");
        }
    }

    serial_println!("  shellquote::self_test 2: backslash, per context");
    // Unquoted: escapes anything, and the escaped byte is not a delimiter.
    assert_eq!(find_bare(b"echo a\\ b > out", b' '), Some(4));
    assert_eq!(find_bare(b"echo a\\>b", b'>'), None);
    assert_eq!(strip_quotes(b"a\\'b"), b"a'b".to_vec());
    // `echo \'` must not flip quoting for the rest of the line: the escaped
    // apostrophe is data, so the context after it is still Unquoted.
    let mut s = scan(b"echo \\' $HOME");
    let toks: Vec<Tok> = s.by_ref().collect();
    assert_eq!(s.context(), Ctx::Unquoted);
    let dollar = toks
        .iter()
        .find(|t| t.byte == b'$')
        .ok_or(crate::error::KernelError::InternalError)?;
    assert_eq!(dollar.ctx, Ctx::Unquoted);
    // Double-quoted: `\` is data before an ordinary byte -- a Windows-style
    // path must survive -- but structural before one of the five.
    assert_eq!(strip_quotes(b"\"C:\\dir\""), b"C:\\dir".to_vec());
    assert_eq!(strip_quotes(b"\"say \\\"hi\\\"\""), b"say \"hi\"".to_vec());
    // Single-quoted: no escapes at all, so `'it\'` ends at that apostrophe and
    // the backslash is literal.
    assert_eq!(strip_quotes(b"'it\\'"), b"it\\".to_vec());
    // A trailing backslash is data, not a dangling escape.
    assert_eq!(strip_quotes(b"a\\"), b"a\\".to_vec());

    serial_println!("  shellquote::self_test 3: delimiters and splitting");
    assert_eq!(find_bare(b"echo 'a > b'", b'>'), None);
    assert_eq!(find_bare(b"cat < \"don't.txt\"", b'<'), Some(4));
    assert_eq!(bare_positions(b"a>b>c", b'>'), (Some(1), Some(3)));
    assert_eq!(bare_positions(b"a'>'b", b'>'), (None, None));
    assert_eq!(find_bare_space(b"FOO='a b' cmd"), Some(9));
    assert_eq!(split_bare_ranges(b"a,'b,c',d", b',').len(), 3);
    assert_eq!(split_bare_ranges(b"nocomma", b',').len(), 1);

    serial_println!("  shellquote::self_test 4: words, incl. the quoted empty one");
    let w = split_bare_words(b"cp a\\ b dst");
    assert_eq!(w.len(), 3, "`a\\ b` is ONE word");
    assert_eq!(strip_quotes(b"a\\ b"), b"a b".to_vec());
    let w = split_bare_words(b"cmd '' x");
    assert_eq!(
        w.len(),
        3,
        "an explicitly quoted empty string is an argument"
    );
    assert!(
        w.get(1)
            .is_some_and(|x| x.quoted && x.start.saturating_add(2) == x.end)
    );
    assert_eq!(split_bare_words(b"   ").len(), 0);
    // Word start: the space inside the quotes must not start a new word.
    assert_eq!(word_start_at(b"cat \"My Doc", 11), 4);
    assert_eq!(word_start_at(b"cat My Doc", 10), 7);

    serial_println!("  shellquote::self_test 5: quote_word round-trips any byte");
    for name in [
        &b"plain.txt"[..],
        &b"My Doc.txt"[..],
        &b"don't.txt"[..],
        &b"re\xffport.txt"[..],
        &b"$HOME"[..],
        &b"a\\b"[..],
        &b"a\"b"[..],
        &b""[..],
        &b"*"[..],
    ] {
        let q = quote_word(name);
        assert_eq!(strip_quotes(&q), name.to_vec(), "quote_word round-trip");
        // A quoted word is exactly one word, whatever it contains.
        assert_eq!(split_bare_words(&q).len(), 1, "quoted word stays one word");
        // ...and holds no live delimiter.
        assert_eq!(find_bare(&q, b'>'), None);
        assert_eq!(find_bare_space(&q), None);
    }

    serial_println!("  shellquote::self_test 6: what expands is not what is bare");
    // Every one of these was cross-checked against real bash (WSL) before
    // being written down; `"\\$HOME"` in particular is Ctx::Double + escaped,
    // not Ctx::Unquoted, and only `escaped` suppresses it.
    for (line, want) in [
        (&b"echo \"it's $HOME\""[..], true), // the known-issues bug: must expand
        (&b"echo 'it \"is\" $HOME'"[..], false),
        (&b"echo $HOME"[..], true),
        (&b"echo \"\\$HOME\""[..], false),
        (&b"echo \\$HOME"[..], false),
    ] {
        let t = scan(line)
            .find(|t| t.byte == b'$')
            .ok_or(crate::error::KernelError::InternalError)?;
        assert_eq!(t.expands(), want, "expansion verdict for a `$`");
    }

    serial_println!("  shellquote::self_test 7: unterminated quotes are reported, not guessed");
    let mut s = scan(b"echo 'oops");
    let _: Vec<Tok> = s.by_ref().collect();
    assert_eq!(s.context(), Ctx::Single);
    let mut s = scan(b"echo \"oops");
    let _: Vec<Tok> = s.by_ref().collect();
    assert_eq!(s.context(), Ctx::Double);

    serial_println!("  shellquote::self_test PASSED");
    Ok(())
}
