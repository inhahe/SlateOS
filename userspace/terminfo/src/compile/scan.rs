//! `comp_scan.c`, the terminfo compiler's scanner, with the context
//! `comp_error.c` names in its messages.
//!
//! Source is read a line at a time: a line that starts with `#` is a
//! comment, the white space that starts a line is skipped (a line that does
//! not start with any is in "the first column", where a terminal's names
//! begin), and a CR before the newline is dropped. A token is the names of a
//! terminal, or one capability: `name`, `name#number`, `name=string` or
//! `name@`, up to the separator -- `,` in terminfo, `:` in termcap, which
//! the names line decides. A capability with a `.` before it is commented
//! out, and skipped.
//!
//! Every warning says where it is -- `"FILE", line N, col M, terminal 'T': `
//! -- from the state kept here, as `comp_error.c`'s `where_is_problem` does.
//!
//! Upstream reads through two pointers, `bufstart` and `bufptr`, into one of
//! two buffers: `next_char`'s own (`result`), into which it reads a file's
//! lines, or a string its caller gave. [`Cur`] says which; `start` and `ptr`
//! are the pointers, as offsets. The file buffer keeps its size from line to
//! line, as upstream's static one does, because a line longer than it is
//! read in pieces, and every piece counts the line's leading white space
//! into the column again.

use std::io::Read;

use super::{Abort, Diagnostics, MAX_ENTRY_SIZE, MAX_NAME_SIZE, tables};
use crate::captab;

/// `SYN_TERMINFO`.
pub const SYN_TERMINFO: i32 = 0;
/// `SYN_TERMCAP`.
pub const SYN_TERMCAP: i32 = 1;
/// `ERR`: the names line being read has not said which yet.
pub const SYN_UNKNOWN: i32 = -1;

/// `EOF` as `next_char` returns it.
pub const EOF: i32 = -1;

/// `MAXCAPLEN`: a string longer than this is warned of.
const MAXCAPLEN: usize = 600;
/// `LEXBUFSIZ`.
const LEXBUFSIZ: usize = 1024;
/// `TOK_BUF_SIZE`.
const TOK_BUF_SIZE: usize = MAX_ENTRY_SIZE;

/// What `_nc_get_token` found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenType {
    /// `BOOLEAN`: `name,`.
    Boolean,
    /// `NUMBER`: `name#number,`.
    Number,
    /// `STRING`: `name=string,`.
    String,
    /// `CANCEL`: `name@,`.
    Cancel,
    /// `NAMES`: a terminal's names, from the first column.
    Names,
    /// `UNDEF`: a name followed by something no capability is.
    Undef,
    /// `EOF`.
    Eof,
}

/// `struct token`: what the last token was.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Token {
    /// `tk_name`: the capability's name, or the whole names field.
    pub name: Vec<u8>,
    /// `tk_valnumber`.
    pub valnumber: i32,
    /// `tk_valstring`: the string, its escapes translated.
    pub valstring: Option<Vec<u8>>,
}

/// `yyin`: a file, read a byte at a time as `fgetc` reads it.
struct File {
    bytes: std::io::Bytes<std::io::BufReader<Box<dyn Read>>>,
    /// What `ftell` answers: where the file was when it was given, plus what
    /// has been read -- or `None` for one that cannot seek, where `ftell`
    /// fails with -1.
    pos: Option<u64>,
    /// `feof`.
    eof: bool,
}

/// What `bufstart` and `bufptr` point into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cur {
    /// Null pointers.
    Null,
    /// `next_char`'s `result`.
    Result,
    /// The string `_nc_reset_input` was given.
    Buffer,
}

/// The scanner, and the error context its messages and the parser's
/// share.
pub struct Scanner<'d> {
    diag: &'d mut dyn Diagnostics,
    // comp_error.c
    /// `SourceName`: the file named in messages.
    pub source_name: Option<Vec<u8>>,
    /// `TermType`: the terminal named in messages.
    term_type: Vec<u8>,
    /// `_nc_curr_line`.
    pub curr_line: i32,
    /// `_nc_curr_col`.
    pub curr_col: i32,
    /// `_nc_suppress_warnings`.
    pub suppress_warnings: bool,
    // comp_scan.c
    /// `_nc_syntax`.
    pub syntax: i32,
    /// `_nc_strict_bsd`: termcap's `\` escapes are BSD's.
    pub strict_bsd: bool,
    /// `_nc_curr_file_pos`: where the current line began.
    pub curr_file_pos: i64,
    /// `_nc_comment_start`.
    pub comment_start: i64,
    /// `_nc_comment_end`.
    pub comment_end: i64,
    /// `_nc_start_line`.
    pub start_line: i64,
    /// `_nc_disable_period` (`tic -a`): a leading `.` is a name, not a
    /// comment.
    pub disable_period: bool,
    /// `_nc_curr_token`.
    pub curr_token: Token,
    first_column: bool,
    had_newline: bool,
    separator: u8,
    /// `pushtype`, and `pushname`: the token to be read again, and the
    /// terminal it was read for.
    pushed: Option<(TokenType, Vec<u8>)>,
    yyin: Option<File>,
    /// `result`: `None` when not allocated.
    result: Option<Vec<u8>>,
    /// `allocated`: its size.
    allocated: usize,
    /// The string being read, from `_nc_reset_input (NULL, buf)`.
    buffer: Vec<u8>,
    cur: Cur,
    /// `bufstart`.
    start: usize,
    /// `bufptr`.
    ptr: usize,
}

/// `iswhite`.
fn iswhite(ch: i32) -> bool {
    ch == i32::from(b' ') || ch == i32::from(b'\t')
}

/// `isspace` in the C locale.
#[must_use]
pub fn isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `isalnum` of a `next_char` answer.
fn isalnum(ch: i32) -> bool {
    u8::try_from(ch).is_ok_and(|c| c.is_ascii_alphanumeric())
}

/// `isoctal`.
fn isoctal(ch: i32) -> bool {
    (i32::from(b'0')..=i32::from(b'7')).contains(&ch)
}

/// `isdigit`.
fn isdigit(ch: i32) -> bool {
    (i32::from(b'0')..=i32::from(b'9')).contains(&ch)
}

/// `UChar (ch)`: the low byte, so `EOF` is 255.
fn uchar(ch: i32) -> u8 {
    ch.to_le_bytes().first().copied().unwrap_or(0)
}

/// `unctrl (c)`, without a screen.
#[must_use]
pub fn unctrl(c: u8) -> &'static str {
    captab::UNCTRL.get(usize::from(c)).copied().unwrap_or("")
}

/// The bytes of a C string: up to a NUL.
#[must_use]
pub fn cstr(b: &[u8]) -> &[u8] {
    b.get(..b.iter().position(|&c| c == 0).unwrap_or(b.len()))
        .unwrap_or(b)
}

/// `IS_TIC_MAGIC`: a compiled entry's first two bytes.
fn is_tic_magic(b: &[u8]) -> bool {
    let byte = |i: usize| b.get(i).copied().map_or(0u16, u16::from);
    let v = byte(0).wrapping_add(byte(1).wrapping_mul(256));
    v == 0o432 || v == 0o1036
}

impl<'d> Scanner<'d> {
    /// A scanner with no input, its messages to `diag`.
    pub fn new(diag: &'d mut dyn Diagnostics) -> Self {
        Self {
            diag,
            source_name: None,
            term_type: Vec::new(),
            curr_line: 0,
            curr_col: 0,
            suppress_warnings: false,
            syntax: SYN_TERMINFO,
            strict_bsd: true,
            curr_file_pos: 0,
            comment_start: 0,
            comment_end: 0,
            start_line: 0,
            disable_period: false,
            curr_token: Token::default(),
            first_column: false,
            had_newline: false,
            separator: 0,
            pushed: None,
            yyin: None,
            result: None,
            allocated: 0,
            buffer: Vec::new(),
            cur: Cur::Null,
            start: 0,
            ptr: 0,
        }
    }

    // ---- comp_error.c ------------------------------------------------------

    /// `_nc_set_source`.
    pub fn set_source(&mut self, name: Option<&[u8]>) {
        self.source_name = name.map(<[u8]>::to_vec);
    }

    /// `_nc_set_type`: at most `MAX_NAME_SIZE` bytes, up to a NUL.
    pub fn set_type(&mut self, name: &[u8]) {
        let name = cstr(name);
        self.term_type = name
            .get(..name.len().min(MAX_NAME_SIZE))
            .unwrap_or(name)
            .to_vec();
    }

    /// `_nc_get_type`.
    #[must_use]
    pub fn get_type(&self) -> Vec<u8> {
        self.term_type.clone()
    }

    /// `where_is_problem`.
    fn where_is_problem(&self) -> Vec<u8> {
        let mut m = b"\"".to_vec();
        m.extend_from_slice(self.source_name.as_deref().unwrap_or(b"?"));
        m.push(b'"');
        if self.curr_line > 0 {
            m.extend_from_slice(format!(", line {}", self.curr_line).as_bytes());
        }
        if self.curr_col > 0 {
            m.extend_from_slice(format!(", col {}", self.curr_col).as_bytes());
        }
        if !self.term_type.is_empty() {
            m.extend_from_slice(b", terminal '");
            m.extend_from_slice(&self.term_type);
            m.push(b'\'');
        }
        m.extend_from_slice(b": ");
        m
    }

    /// `_nc_warning (fmt, ...)`: where, then the message.
    pub fn warning(&mut self, message: &[u8]) {
        if self.suppress_warnings {
            return;
        }
        let mut m = self.where_is_problem();
        m.extend_from_slice(message);
        m.push(b'\n');
        self.diag.emit(&m);
    }

    /// `_nc_err_abort (fmt, ...)`: as a warning, but never suppressed, and
    /// the end of the program.
    pub fn err_abort(&mut self, message: &[u8]) -> Abort {
        let mut m = self.where_is_problem();
        m.extend_from_slice(message);
        m.push(b'\n');
        self.diag.emit(&m);
        Abort
    }

    /// `_nc_syserr_abort (fmt, ...)`: the same, for what should not happen.
    pub fn syserr_abort(&mut self, message: &[u8]) -> Abort {
        self.err_abort(message)
    }

    /// Write `bytes` where the warnings go, as they are.
    pub fn emit(&mut self, bytes: &[u8]) {
        self.diag.emit(bytes);
    }

    // ---- character-stream handling -----------------------------------------

    /// `_nc_reset_input (fp, NULL)`: read from `reader`, which is at
    /// `offset` in its file -- `None` for one that cannot seek.
    pub fn reset_input_file(&mut self, reader: Box<dyn Read>, offset: Option<u64>) {
        self.pushed = None;
        self.yyin = Some(File {
            bytes: std::io::BufReader::new(reader).bytes(),
            pos: offset,
            eof: false,
        });
        self.cur = Cur::Null;
        self.start = 0;
        self.ptr = 0;
        self.curr_file_pos = 0;
        self.curr_line = 0;
        self.curr_col = 0;
    }

    /// `_nc_reset_input (NULL, buf)`: read from `buf`, up to a NUL in it.
    pub fn reset_input_buffer(&mut self, buf: &[u8]) {
        self.pushed = None;
        self.yyin = None;
        self.buffer = cstr(buf).to_vec();
        self.cur = Cur::Buffer;
        self.start = 0;
        self.ptr = 0;
        self.curr_file_pos = 0;
        self.curr_col = 0;
    }

    /// The buffer `bufptr` points into.
    fn cur_bytes(&self) -> Option<&[u8]> {
        match self.cur {
            Cur::Null => None,
            Cur::Result => self.result.as_deref(),
            Cur::Buffer => Some(&self.buffer),
        }
    }

    /// `bufptr[at]`: NUL past the end, as the C string's terminator is.
    fn at(&self, at: usize) -> u8 {
        self.cur_bytes()
            .and_then(|l| l.get(self.ptr.saturating_add(at)))
            .copied()
            .unwrap_or(0)
    }

    /// `bufptr`, as the string it points to.
    fn rest(&self) -> &[u8] {
        self.cur_bytes()
            .and_then(|l| l.get(self.ptr..))
            .map(cstr)
            .unwrap_or_default()
    }

    /// `last_char (from_end)`: the last character of the line that is not
    /// a space -- or the one `from_end` before it.
    fn last_char(&self, from_end: usize) -> i32 {
        let rest = self.rest();
        let mut len = rest.len();
        while len > 0 {
            len = len.saturating_sub(1);
            if let Some(&c) = rest.get(len)
                && !isspace(c)
            {
                if from_end <= len {
                    return i32::from(rest.get(len.saturating_sub(from_end)).copied().unwrap_or(0));
                }
                break;
            }
        }
        0
    }

    /// `fgetc (yyin)`.
    fn fgetc(&mut self) -> i32 {
        let Some(file) = &mut self.yyin else {
            return EOF;
        };
        match file.bytes.next() {
            Some(Ok(b)) => {
                file.pos = file.pos.map(|p| p.saturating_add(1));
                i32::from(b)
            }
            // A read error is EOF without the end-of-file flag.
            Some(Err(_)) => EOF,
            None => {
                file.eof = true;
                EOF
            }
        }
    }

    /// `ftell (yyin)`.
    fn ftell(&self) -> i64 {
        match &self.yyin {
            Some(File { pos: Some(pos), .. }) => i64::try_from(*pos).unwrap_or(i64::MAX),
            Some(File { pos: None, .. }) => -1,
            None => 0,
        }
    }

    /// `feof (yyin)`.
    fn feof(&self) -> bool {
        self.yyin.as_ref().is_some_and(|f| f.eof)
    }

    /// `get_text (result + used, length)`: a line onto the file buffer, as
    /// `fgets` reads one but refusing a NUL; how many bytes.
    fn get_text(&mut self, used: usize, length: usize) -> Result<usize, Abort> {
        let mut text = Vec::new();
        let mut count = 0usize;
        let mut limit = length.saturating_sub(1);
        while limit > 0 {
            limit = limit.saturating_sub(1);
            let ch = self.fgetc();
            if ch == 0 {
                return Err(self.err_abort(b"This is not a text-file"));
            } else if ch == EOF {
                break;
            }
            count = count.saturating_add(1);
            text.push(uchar(ch));
            if ch == i32::from(b'\n') {
                break;
            }
        }
        let result = self.result.get_or_insert_with(Vec::new);
        result.truncate(used);
        result.extend_from_slice(&text);
        Ok(count)
    }

    /// `next_char`: the next character of the source, comments and the
    /// white space that starts a line skipped.
    fn next_char(&mut self) -> Result<i32, Abort> {
        if self.yyin.is_none() {
            if self.result.is_some() {
                // The end of a file: its buffer freed -- and the string to
                // be read with it, if one was given before the file ended.
                self.result = None;
                self.cur = Cur::Null;
                self.start = 0;
                self.ptr = 0;
                self.allocated = 0;
            }
            // "An string with an embedded null will truncate the input."
            if self.cur == Cur::Null || self.at(0) == 0 {
                return Ok(EOF);
            }
            if self.at(0) == b'\n' {
                self.curr_line = self.curr_line.saturating_add(1);
                self.curr_col = 0;
            } else if self.at(0) == b'\t' {
                self.curr_col |= 7;
            }
        } else if self.cur == Cur::Null || self.at(0) == 0 {
            if self.read_line()? {
                return Ok(EOF);
            }
        } else if self.at(0) == b'\t' {
            self.curr_col |= 7;
        }

        self.first_column = self.ptr == self.start;
        if self.first_column {
            self.had_newline = false;
        }
        self.curr_col = self.curr_col.saturating_add(1);
        let the_char = self.at(0);
        self.ptr = self.ptr.saturating_add(1);
        Ok(i32::from(the_char))
    }

    /// `next_char`'s reading of a file's next line that is not a comment,
    /// into its buffer, and past the white space that starts it; whether
    /// that was the end.
    fn read_line(&mut self) -> Result<bool, Abort> {
        loop {
            let mut used = 0usize;
            // `bufstart = 0`.
            let mut have_start = false;
            loop {
                if used.saturating_add(LEXBUFSIZ / 4) >= self.allocated {
                    self.allocated = self
                        .allocated
                        .saturating_add(self.allocated.saturating_add(LEXBUFSIZ));
                }
                if used == 0 {
                    self.curr_file_pos = self.ftell();
                }
                if self.get_text(used, self.allocated.saturating_sub(used))? > 0 {
                    have_start = true;
                    if used == 0 {
                        if self.curr_line == 0 && self.result.as_deref().is_some_and(is_tic_magic) {
                            return Err(self.err_abort(
                                b"This is a compiled terminal description, not a source",
                            ));
                        }
                        self.curr_line = self.curr_line.saturating_add(1);
                        self.curr_col = 0;
                    }
                } else if used != 0 {
                    // `strcat (result, "\n")`: the file ended mid-line.
                    if let Some(r) = self.result.as_mut() {
                        r.push(b'\n');
                    }
                }
                if !have_start {
                    self.cur = Cur::Null;
                    self.start = 0;
                    self.ptr = 0;
                    return Ok(true);
                }
                self.cur = Cur::Result;
                self.start = 0;
                self.ptr = 0;
                let total = self.result.as_ref().map_or(0, Vec::len);
                used = total;
                if used == 0 {
                    return Ok(true);
                }
                while iswhite(i32::from(self.at(0))) {
                    if self.at(0) == b'\t' {
                        self.curr_col = (self.curr_col | 7).saturating_add(1);
                    } else {
                        self.curr_col = self.curr_col.saturating_add(1);
                    }
                    self.ptr = self.ptr.saturating_add(1);
                }
                // "Treat a trailing <cr><lf> the same as a <newline>".
                let mut end = total;
                if let Some(r) = self.result.as_mut()
                    && total.saturating_sub(self.ptr) > 1
                    && r.last() == Some(&b'\n')
                    && r.get(total.saturating_sub(2)) == Some(&b'\r')
                {
                    r.truncate(total.saturating_sub(1));
                    if let Some(last) = r.last_mut() {
                        *last = b'\n';
                    }
                    end = total.saturating_sub(1);
                }
                // `bufptr[len - 1]`: for a line of only white space, the
                // last of it.
                let last = self
                    .result
                    .as_ref()
                    .and_then(|r| r.get(end.saturating_sub(1)))
                    .copied();
                if last == Some(b'\n') {
                    break;
                }
            }
            // "ignore comments"
            if self.result.as_ref().and_then(|r| r.first()) != Some(&b'#') {
                return Ok(false);
            }
        }
    }

    /// `push_back (c)`: the character before, given back.
    fn push_back(&mut self, c: i32) -> Result<(), Abort> {
        if self.ptr == self.start {
            return Err(self.syserr_abort(b"cannot backspace off beginning of line"));
        }
        self.ptr = self.ptr.saturating_sub(1);
        let at = self.ptr;
        let slot = match self.cur {
            Cur::Null => None,
            Cur::Result => self.result.as_mut().and_then(|r| r.get_mut(at)),
            Cur::Buffer => self.buffer.get_mut(at),
        };
        if let Some(slot) = slot {
            *slot = uchar(c);
        }
        self.curr_col = self.curr_col.saturating_sub(1);
        Ok(())
    }

    /// `stream_pos`.
    fn stream_pos(&self) -> i64 {
        if self.yyin.is_some() {
            self.ftell()
        } else if self.cur == Cur::Null {
            0
        } else {
            i64::try_from(self.ptr.saturating_sub(self.start)).unwrap_or(i64::MAX)
        }
    }

    /// `end_of_stream`.
    fn end_of_stream(&self) -> bool {
        if self.yyin.is_some() {
            self.feof() && (self.cur == Cur::Null || self.at(0) == 0)
        } else {
            self.cur != Cur::Null && self.at(0) == 0
        }
    }

    /// `eat_escaped_newline`: past a backslash, newlines and white space.
    fn eat_escaped_newline(&mut self, ch: i32) -> Result<i32, Abort> {
        let mut ch = ch;
        if ch == i32::from(b'\\') {
            loop {
                ch = self.next_char()?;
                if !(ch == i32::from(b'\n') || iswhite(ch)) {
                    break;
                }
            }
        }
        Ok(ch)
    }

    // ---- tokens --------------------------------------------------------------

    /// `_nc_push_token (tokclass)`: the token just read, to be read again.
    ///
    /// `EOF` is upstream's `NO_PUSHBACK` too, so pushing it back pushes
    /// nothing.
    pub fn push_token(&mut self, tokclass: TokenType) {
        self.pushed = (tokclass != TokenType::Eof).then(|| (tokclass, self.get_type()));
    }

    /// `_nc_panic_mode (ch)`: everything up to `ch` skipped.
    ///
    /// # Errors
    ///
    /// An error that ends the compile, its message written.
    pub fn panic_mode(&mut self, ch: u8) -> Result<(), Abort> {
        loop {
            let c = self.next_char()?;
            if c == i32::from(ch) || c == EOF {
                return Ok(());
            }
        }
    }

    /// `_nc_get_token (silent)`.
    ///
    /// # Errors
    ///
    /// An error that ends the compile, its message written.
    pub fn get_token(&mut self, silent: bool) -> Result<TokenType, Abort> {
        // A commented-out capability is followed by the next token, as
        // upstream's tail call reads it.
        loop {
            if let Some((retval, name)) = self.pushed.take() {
                self.set_type(&name);
                return Ok(retval);
            }
            if self.end_of_stream() {
                self.yyin = None;
                // "frees its allocated memory"
                self.next_char()?;
                self.curr_token.name.clear();
                return Ok(TokenType::Eof);
            }
            let (ty, dot_flag) = self.read_token(silent)?;
            if !dot_flag {
                return Ok(ty);
            }
        }
    }

    /// The body of `_nc_get_token`, from `start_token`: the token, and
    /// whether it was commented out.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's _nc_get_token, in one piece so it reads against it"
    )]
    fn read_token(&mut self, silent: bool) -> Result<(TokenType, bool), Abort> {
        const TERMINFO_PUNCT: &[u8] = b"@%&*!#";

        let mut dot_flag = false;
        let mut ch;
        'start_token: loop {
            let token_start = self.stream_pos();
            loop {
                ch = self.next_char()?;
                if ch == i32::from(b'\n') {
                    self.had_newline = true;
                } else if !iswhite(ch) {
                    break;
                }
            }
            ch = self.eat_escaped_newline(ch)?;
            self.curr_token.valstring = None;

            if ch == EOF {
                return Ok((TokenType::Eof, dot_flag));
            }
            // "if this is a termcap entry, skip a leading separator"
            if self.separator == b':' && ch == i32::from(b':') {
                ch = self.next_char()?;
            }
            if ch == i32::from(b'.') && !self.disable_period {
                dot_flag = true;
                loop {
                    ch = self.next_char()?;
                    if !(ch == i32::from(b'.') || iswhite(ch)) {
                        break;
                    }
                }
            }
            if ch == EOF {
                return Ok((TokenType::Eof, dot_flag));
            }

            // "have to make some punctuation chars legal for terminfo"
            if !isalnum(ch)
                && (ch != i32::from(b'.') || !self.disable_period)
                && !TERMINFO_PUNCT.contains(&uchar(ch))
            {
                if !silent {
                    let m = format!(
                        "Illegal character (expected alphanumeric or @%&*!#) - '{}'",
                        unctrl(uchar(ch))
                    );
                    self.warning(m.as_bytes());
                }
                let sep = self.separator;
                self.panic_mode(sep)?;
                continue 'start_token;
            }

            let mut tok: Vec<u8> = vec![uchar(ch)];
            let ok_to_add = |tok: &Vec<u8>| tok.len() < TOK_BUF_SIZE.saturating_sub(2);

            if self.first_column {
                self.comment_start = token_start;
                self.comment_end = self.curr_file_pos;
                self.start_line = i64::from(self.curr_line);

                self.syntax = SYN_UNKNOWN;
                let mut after_name: Option<usize> = None;
                let mut after_list: Option<usize> = None;
                loop {
                    ch = self.next_char()?;
                    if ch == i32::from(b'\n') {
                        break;
                    }
                    if ch == EOF {
                        return Err(self.err_abort(b"Premature EOF"));
                    } else if ch == i32::from(b'|') {
                        after_list = Some(tok.len());
                        if after_name.is_none() {
                            after_name = Some(tok.len());
                        }
                    } else if ch == i32::from(b':') && self.last_char(0) != i32::from(b',') {
                        self.syntax = SYN_TERMCAP;
                        self.separator = b':';
                        break;
                    } else if ch == i32::from(b',') {
                        self.syntax = SYN_TERMINFO;
                        self.separator = b',';
                        // "If we did not see a '|', then we found a name with
                        // no aliases or description."
                        if after_name.is_none() {
                            break;
                        }
                        let c0 = self.last_char(0);
                        let c1 = self.last_char(1);
                        if c1 != i32::from(b':')
                            && c0 != i32::from(b'\\')
                            && c0 != i32::from(b':')
                            && self.looks_like_capability()
                        {
                            break;
                        }
                    } else {
                        ch = self.eat_escaped_newline(ch)?;
                    }
                    if ok_to_add(&tok) {
                        tok.push(uchar(ch));
                    } else {
                        break;
                    }
                }
                if self.syntax == SYN_UNKNOWN {
                    // "Grrr...": "a couple of name fields in the 8.2 termcap
                    // file end with |\".
                    self.syntax = SYN_TERMCAP;
                    self.separator = b':';
                } else if self.syntax == SYN_TERMINFO {
                    // "throw away trailing /, *$/"
                    while let Some(&c) = tok.last()
                        && (iswhite(i32::from(c)) || c == b',')
                    {
                        tok.pop();
                    }
                }

                // "This is the soonest we have the terminal name fetched."
                if let Some(n) = after_name {
                    let name = tok.get(..n).unwrap_or(&tok).to_vec();
                    self.set_type(&name);
                }

                let list_end = match after_list {
                    Some(at) => {
                        if !silent {
                            let next = tok.get(at.saturating_add(1)).copied().unwrap_or(0);
                            if tok.get(at).is_none_or(|&c| c == 0) || next == 0 || next == b'|' {
                                self.warning(b"empty longname field");
                            } else if !tok.get(at..).unwrap_or_default().contains(&b' ') {
                                self.warning(
                                    b"older tic versions may treat the description field as an alias",
                                );
                            }
                        }
                        at
                    }
                    None => tok.len(),
                };

                // "Whitespace in a name field other than the long name can
                // confuse rdist and some termcap tools."
                let names = tok.get(..list_end).unwrap_or(&tok).to_vec();
                for c in names {
                    if isspace(c) {
                        if !silent {
                            self.warning(b"whitespace in name or alias field");
                        }
                        break;
                    } else if c == b'/' {
                        if !silent {
                            self.warning(b"slashes aren't allowed in names or aliases");
                        }
                        break;
                    } else if b"$[]!*?".contains(&c) {
                        if !silent {
                            let mut m = b"dubious character `".to_vec();
                            m.push(c);
                            m.extend_from_slice(b"' in name or alias field");
                            self.warning(&m);
                        }
                        break;
                    }
                }

                self.curr_token.name = tok;
                return Ok((TokenType::Names, dot_flag));
            }

            if self.had_newline && self.syntax == SYN_TERMCAP {
                self.warning(b"Missing backslash before newline");
                self.had_newline = false;
            }
            loop {
                ch = self.next_char()?;
                if ch == EOF {
                    break;
                }
                if !isalnum(ch) {
                    if self.syntax == SYN_TERMINFO {
                        if ch != i32::from(b'_') {
                            break;
                        }
                    } else if ch != i32::from(b';') {
                        // "allow ';' for "k;""
                        break;
                    }
                }
                if ok_to_add(&tok) {
                    tok.push(uchar(ch));
                } else {
                    ch = EOF;
                    break;
                }
            }

            let sep = i32::from(self.separator);
            let ty = if ch == EOF {
                // `tk_name` points at the token buffer, which now holds
                // this name, whether or not the case sets it.
                self.curr_token.name = tok;
                TokenType::Eof
            } else if ch == i32::from(b',') || ch == i32::from(b':') {
                if ch != sep {
                    return Err(self.err_abort(b"Separator inconsistent with syntax"));
                }
                self.curr_token.name = tok;
                TokenType::Boolean
            } else if ch == i32::from(b'@') {
                ch = self.next_char()?;
                if ch != sep && !silent {
                    let mut m = b"Missing separator after `".to_vec();
                    m.extend_from_slice(&tok);
                    m.extend_from_slice(b"', have ");
                    m.extend_from_slice(unctrl(uchar(ch)).as_bytes());
                    self.warning(&m);
                }
                self.curr_token.name = tok;
                TokenType::Cancel
            } else if ch == i32::from(b'#') {
                self.number_token(tok, silent)?;
                TokenType::Number
            } else if ch == i32::from(b'=') {
                let room = TOK_BUF_SIZE.saturating_sub(tok.len().saturating_add(1));
                let (value, ended) = self.trans_string(room)?;
                if !silent && ended != sep {
                    self.warning(b"Missing separator");
                }
                self.curr_token.name = tok;
                self.curr_token.valstring = Some(value);
                TokenType::String
            } else {
                if !silent {
                    let m = format!("Illegal character - '{}'", unctrl(uchar(ch)));
                    self.warning(m.as_bytes());
                }
                self.curr_token.name = tok;
                TokenType::Undef
            };
            return Ok((ty, dot_flag));
        }
    }

    /// The `#` case of `_nc_get_token`: the number after the name `tok`.
    fn number_token(&mut self, tok: Vec<u8>, silent: bool) -> Result<(), Abort> {
        let mut numbuf: Vec<u8> = Vec::new();
        let mut ch;
        loop {
            ch = self.next_char()?;
            if !isalnum(ch) {
                break;
            }
            numbuf.push(uchar(ch));
            // `sizeof (numbuf) - 1`.
            if numbuf.len() >= 79 {
                break;
            }
        }
        let (mut number, used) = cstrtol::strtol(&numbuf, 0);
        if !silent {
            let quoted = |head: &[u8], tail: &[u8]| -> Vec<u8> { [head, &tok, tail].concat() };
            if used == 0 {
                let m = quoted(b"no value given for `", b"'");
                self.warning(&m);
            }
            if used != numbuf.len() || ch != i32::from(self.separator) {
                let m = quoted(b"Missing separator for `", b"'");
                self.warning(&m);
            }
            if number < 0 {
                let m = quoted(b"value of `", b"' cannot be negative");
                self.warning(&m);
            }
            // `MAX_OF_TYPE (NCURSES_INT2)`.
            if number > i64::from(i32::MAX) {
                let tail = format!("' from {number:#x} to {:#x}", i32::MAX);
                let m = quoted(b"limiting value of `", tail.as_bytes());
                self.warning(&m);
                number = i64::from(i32::MAX);
            }
        }
        self.curr_token.name = tok;
        // `(int) number`.
        self.curr_token.valnumber = cstrtol::low_i32(number);
        Ok(())
    }

    /// The names line's lookahead past a comma: whether what follows looks
    /// like a capability -- a lower-case word then `#`, `=` or `@`, or a
    /// known terminfo name then a comma -- so that the comma ends the names
    /// rather than being part of the description.
    fn looks_like_capability(&self) -> bool {
        let rest = self.rest();
        let mut s = 0usize;
        while rest.get(s).is_some_and(|&c| isspace(c)) {
            s = s.saturating_add(1);
        }
        if !rest.get(s).is_some_and(u8::is_ascii_lowercase) {
            return false;
        }
        let name_start = s;
        while rest.get(s).is_some_and(u8::is_ascii_alphanumeric) {
            s = s.saturating_add(1);
        }
        match rest.get(s) {
            Some(b'#' | b'=' | b'@') => true,
            Some(b',') => {
                let name = rest.get(name_start..s).unwrap_or_default();
                tables::find_entry(name, false).is_some()
            }
            _ => false,
        }
    }

    /// `_nc_trans_string (ptr, last)`: a string's characters up to the
    /// separator, a newline (termcap) or the end -- at most `room - 1` of
    /// them -- escapes translated; and the character that ended it.
    fn trans_string(&mut self, room: usize) -> Result<(Vec<u8>, i32), Abort> {
        let mut out: Vec<u8> = Vec::new();
        let mut count = 0usize;
        let mut last_ch: i32 = 0;
        let mut long_warning = false;
        let sep = i32::from(self.separator);
        let mut c;
        loop {
            c = self.next_char()?;
            if c == sep || c == EOF {
                break;
            }
            if out.len() >= room.saturating_sub(1) {
                // Full: the rest of it skipped.
                while c != sep && c != EOF {
                    c = self.next_char()?;
                }
                break;
            }
            if self.syntax == SYN_TERMCAP && c == i32::from(b'\n') {
                break;
            }
            let mut ignored = false;
            if c == i32::from(b'^') && last_ch != i32::from(b'%') {
                c = self.next_char()?;
                if c == EOF {
                    return Err(self.err_abort(b"Premature EOF"));
                }
                let uc = uchar(c);
                if !(0x20..0x7f).contains(&uc) {
                    let m = format!("Illegal ^ character - '{}'", unctrl(uc));
                    self.warning(m.as_bytes());
                }
                if c == i32::from(b'?') && self.syntax != SYN_TERMCAP {
                    out.push(0o177);
                } else {
                    c &= 0o37;
                    if c == 0 {
                        c = 128;
                    }
                    out.push(uchar(c));
                }
            } else if c == i32::from(b'\\') {
                let strict_bsd = self.syntax == SYN_TERMCAP && self.strict_bsd;
                c = self.next_char()?;
                if c == EOF {
                    return Err(self.err_abort(b"Premature EOF"));
                }
                if isoctal(c) || (strict_bsd && isdigit(c)) {
                    let mut number = c.wrapping_sub(i32::from(b'0'));
                    for _ in 0..2 {
                        c = self.next_char()?;
                        if c == EOF {
                            return Err(self.err_abort(b"Premature EOF"));
                        }
                        if !isoctal(c) {
                            if isdigit(c) {
                                if !strict_bsd {
                                    let mut m = b"Non-octal digit `".to_vec();
                                    m.push(uchar(c));
                                    m.extend_from_slice(b"' in \\ sequence");
                                    self.warning(&m);
                                }
                            } else {
                                self.push_back(c)?;
                                break;
                            }
                        }
                        number = number
                            .wrapping_mul(8)
                            .wrapping_add(c)
                            .wrapping_sub(i32::from(b'0'));
                    }
                    let mut number = i32::from(uchar(number));
                    if number == 0 && !strict_bsd {
                        number = 0o200;
                    }
                    out.push(uchar(number));
                } else {
                    match uchar(c) {
                        b'E' => out.push(0o33),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(0o10),
                        b'f' => out.push(0o14),
                        b't' => out.push(b'\t'),
                        b'\\' => out.push(b'\\'),
                        b'^' => out.push(b'^'),
                        b',' => out.push(b','),
                        // A backslash-newline: the string goes on, and
                        // nothing about this character counts.
                        b'\n' => continue,
                        b'|' => out.push(b'|'),
                        _ => {
                            if self.syntax == SYN_TERMINFO || !self.strict_bsd {
                                match uchar(c) {
                                    b'a' => c = 0o7,
                                    b'e' => c = 0o33,
                                    b'l' => c = i32::from(b'\n'),
                                    b's' => c = i32::from(b' '),
                                    b':' => {}
                                    other => {
                                        let m = format!(
                                            "Illegal character '{}' in \\ sequence",
                                            unctrl(other)
                                        );
                                        self.warning(m.as_bytes());
                                    }
                                }
                            }
                            out.push(uchar(c));
                        }
                    }
                }
            } else if c == i32::from(b'\n') && self.syntax == SYN_TERMINFO {
                // "Newlines embedded in a terminfo string are ignored,
                // provided that the next line begins with whitespace."
                ignored = true;
            } else {
                out.push(uchar(c));
            }

            if !ignored {
                if self.curr_col <= 1 {
                    // A character in the first column: a new entry began.
                    self.push_back(c)?;
                    c = i32::from(b'\n');
                    break;
                }
                last_ch = c;
                count = count.saturating_add(1);
            }
            if count > MAXCAPLEN && !long_warning {
                self.warning(b"Very long string found.  Missing separator?");
                long_warning = true;
            }
        }
        Ok((out, c))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// What reading a source gave: its tokens, what was said about them,
    /// and how the reading ended.
    type Lexed = (Vec<(TokenType, Token)>, Vec<u8>, Result<(), Abort>);

    /// Every token of `source`, read from it as a file is, and what was
    /// said about them.
    fn tokens(source: &[u8]) -> Lexed {
        let mut diag = Vec::new();
        let mut out = Vec::new();
        let status;
        {
            let mut s = Scanner::new(&mut diag);
            s.set_source(Some(b"t.src"));
            s.reset_input_file(Box::new(std::io::Cursor::new(source.to_vec())), Some(0));
            loop {
                match s.get_token(false) {
                    Ok(TokenType::Eof) => {
                        status = Ok(());
                        break;
                    }
                    Ok(t) => out.push((t, s.curr_token.clone())),
                    Err(e) => {
                        status = Err(e);
                        break;
                    }
                }
            }
        }
        (out, diag, status)
    }

    fn names(t: &[(TokenType, Token)]) -> Vec<(TokenType, String)> {
        t.iter()
            .map(|(ty, tok)| (*ty, String::from_utf8(tok.name.clone()).unwrap()))
            .collect()
    }

    #[test]
    fn a_terminfo_entry_reads_as_its_names_then_each_capability() {
        let (t, diag, status) =
            tokens(b"# a comment\nx|xterm|the terminal,\n\tam, cols#80, bel=^G, .km, kf1@,\n");
        assert_eq!(status, Ok(()));
        assert_eq!(diag, b"");
        assert_eq!(
            names(&t),
            vec![
                (TokenType::Names, "x|xterm|the terminal".to_owned()),
                (TokenType::Boolean, "am".to_owned()),
                (TokenType::Number, "cols".to_owned()),
                (TokenType::String, "bel".to_owned()),
                (TokenType::Cancel, "kf1".to_owned()),
            ]
        );
        assert_eq!(t[2].1.valnumber, 80);
        assert_eq!(t[3].1.valstring.as_deref(), Some(&b"\x07"[..]));
    }

    #[test]
    fn escapes_translate_as_upstream_translates_them() {
        let (t, _, _) = tokens(b"x|y z,\n\ts=\\E\\0\\101^?^@\\s\\l\\,\\^\\\\%^c,\n");
        assert_eq!(
            t[1].1.valstring.as_deref(),
            Some(&b"\x1b\x80A\x7f\x80 \n,^\\%^c"[..])
        );
    }

    #[test]
    fn warnings_name_the_file_line_column_and_terminal() {
        let (_, diag, _) = tokens(b"x|y z,\n\tcols#8x,\n");
        assert_eq!(
            String::from_utf8(diag).unwrap(),
            // Measured: the reference counts to the comma that ended the
            // number, past the tab's eight columns.
            "\"t.src\", line 2, col 16, terminal 'x': Missing separator for `cols'\n"
        );
    }

    #[test]
    fn a_compiled_entry_or_a_nul_is_refused() {
        let (_, diag, status) = tokens(b"\x1a\x01\n");
        assert_eq!(status, Err(Abort));
        assert!(
            String::from_utf8(diag)
                .unwrap()
                .ends_with("This is a compiled terminal description, not a source\n")
        );
        let (_, diag, status) = tokens(b"x|y,\0");
        assert_eq!(status, Err(Abort));
        assert!(
            String::from_utf8(diag)
                .unwrap()
                .ends_with("This is not a text-file\n")
        );
    }

    #[test]
    fn termcap_syntax_is_recognised_by_its_colon() {
        let (t, _, _) = tokens(b"x|y z:co#80:bl=^G:\n");
        assert_eq!(
            names(&t),
            vec![
                (TokenType::Names, "x|y z".to_owned()),
                (TokenType::Number, "co".to_owned()),
                (TokenType::String, "bl".to_owned()),
            ]
        );
    }

    #[test]
    fn pushing_back_eof_pushes_nothing() {
        let mut diag = Vec::new();
        let mut s = Scanner::new(&mut diag);
        s.push_token(TokenType::Eof);
        assert!(s.pushed.is_none());
        s.push_token(TokenType::Names);
        assert!(s.pushed.is_some());
    }
}
