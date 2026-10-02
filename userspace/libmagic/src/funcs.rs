//! libmagic's `funcs.c`, and the `struct magic_set` it works on.
//!
//! The magic set is libmagic's whole state: the loaded rules, the output
//! being built, the first error, the position of the rule being matched. Every
//! test appends to the output through [`Ms::printf`], which is `file_printf`:
//! a C format and its arguments, checked and limited as upstream checks and
//! limits them -- a piece over 1024 bytes, or an output over a megabyte, is an
//! error, and after the first error nothing more is written.
//!
//! [`file_buffer`] is the order the tests run in, which decides the answer
//! wherever two of them would claim a file: encoding, compressed, tar, JSON,
//! CSV, SIMH, CDF, ELF, the magic rules, and text.

use std::cell::OnceCell;
use std::fs::File;
use std::io::Write;
use std::rc::Rc;

use crate::buffer::{Buffer, Stat};
use crate::cstd::{cstr, isdigit, isprint, isspace};
use crate::encoding::file_encoding;
use crate::magic::{BINTEST, Magic, Value, *};
use crate::printf::{self, Arg};

/// `EVENT_HAD_ERR`: an error was recorded; the output is now its message.
pub const EVENT_HAD_ERR: u8 = 0x01;

/// `FILE_SEPARATOR`: what `-k` puts between the answers it finds.
pub const FILE_SEPARATOR: &[u8] = b"\n- ";

/// `MAGIC_SETS`.
pub const MAGIC_SETS: usize = 2;

/// `struct level_info`: per continuation level, where the last match ended and
/// whether one matched.
#[derive(Clone, Copy, Debug, Default)]
pub struct LevelInfo {
    pub off: i32,
    pub got_match: bool,
}

/// `ms->search`: where the current `search` or `regex` rule looks.
///
/// `s` is an offset into the buffer the rule is reading rather than C's
/// pointer, so it is only meaningful alongside that buffer; the matcher passes
/// the buffer with it everywhere it is used.
#[derive(Clone, Copy, Debug, Default)]
pub struct Search {
    pub s: Option<usize>,
    pub s_len: usize,
    pub offset: usize,
    pub rm_len: usize,
}

/// `struct mlist`: one database's rules for one set -- borrowed in place from
/// a database the program carries, or owned -- and the regexes compiled from
/// them on first use (`magic_rxcomp`).
pub struct MList {
    pub magic: std::borrow::Cow<'static, [Magic]>,
    pub rx: Vec<OnceCell<Rc<ere::Regex>>>,
}

impl MList {
    #[must_use]
    pub fn new(magic: std::borrow::Cow<'static, [Magic]>) -> MList {
        let rx = magic.iter().map(|_| OnceCell::new()).collect();
        MList { magic, rx }
    }
}

/// `struct magic_set`.
pub struct Ms {
    /// The loaded databases, in the order they were loaded: `None` until a
    /// load has succeeded, as C's list head is NULL.
    pub mlist: [Option<Vec<Rc<MList>>>; MAGIC_SETS],
    /// `c.li`.
    pub c: Vec<LevelInfo>,
    /// `o.buf`: the answer being built, or `None` (NULL).
    pub o_buf: Option<Vec<u8>>,
    /// `o.blen`.
    pub o_blen: usize,
    pub offset: u32,
    pub eoffset: u32,
    pub error: i32,
    pub flags: u32,
    pub event_flags: u8,
    /// The magic file being read, for warnings (`ms->file`).
    pub file: Option<Vec<u8>>,
    /// The line being read or matched, for warnings (`ms->line`).
    pub line: usize,
    /// `st_mode` of the file being identified.
    pub mode: u32,
    pub search: Search,
    /// What the rule being matched read from the file.
    pub ms_value: Value,
    pub indir_max: u16,
    pub name_max: u16,
    pub elf_shnum_max: u16,
    pub elf_phnum_max: u16,
    pub elf_notes_max: u16,
    pub regex_max: u16,
    pub bytes_max: usize,
    pub encoding_max: usize,
    pub elf_shsize_max: usize,
    /// Not upstream's: keep `file_magwarn` quiet. The embedded database is
    /// read once per run, where upstream compiled it once at build time and
    /// printed its warnings then.
    pub quiet: bool,
    /// Not upstream's: whether `LC_CTYPE`'s codeset is UTF-8, which decides
    /// what `file_getbuffer` and the name printer may write unescaped. `file`
    /// calls `setlocale(LC_CTYPE, "")` before anything else.
    pub utf8: bool,
    /// Not upstream's: C's `errno` where upstream's messages read it after
    /// the fact (`file_warn`, and `file_error(ms, errno, ...)` calls that
    /// follow no failure of their own), as the last thing that would have set
    /// it. `None` is 0.
    pub errno: Option<Errno>,
    /// Not upstream's: a compiled database carried by the program -- the
    /// bytes of a `magic.mgc`, aligned for a rule ([`crate::magic::Magic`]) --
    /// used in place of the default path's `magic.mgc` when nothing is
    /// installed there.
    pub builtin: Option<&'static [u8]>,
}

/// C's `errno`, as far as it is followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Errno {
    /// An error of this kind: `EINVAL` is `InvalidInput`, `ESPIPE`
    /// `NotSeekable`, and so on.
    Kind(std::io::ErrorKind),
    /// `ESRCH`, which has no kind of its own: the CDF reader's "no such
    /// stream".
    Srch,
    /// `ERANGE`, from a number in a rule that does not fit.
    Range,
}

impl Errno {
    /// The `errno` an I/O error would have left.
    #[must_use]
    pub fn of(e: &std::io::Error) -> Errno {
        Errno::Kind(e.kind())
    }

    /// An error whose `strerror` is this `errno`'s.
    #[must_use]
    pub fn to_error(self) -> std::io::Error {
        match self {
            Errno::Kind(k) => std::io::Error::from(k),
            Errno::Srch => std::io::Error::other("No such process"),
            Errno::Range => std::io::Error::other("Numerical result out of range"),
        }
    }
}

/// `FILE_BYTES_MAX` and the other limits `-P` can change.
pub const FILE_BYTES_MAX: usize = 7 * 1024 * 1024;
pub const FILE_ELF_NOTES_MAX: u16 = 256;
pub const FILE_ELF_PHNUM_MAX: u16 = 2048;
pub const FILE_ELF_SHNUM_MAX: u16 = 32768;
pub const FILE_ELF_SHSIZE_MAX: usize = 128 * 1024 * 1024;
pub const FILE_INDIR_MAX: u16 = 50;
pub const FILE_NAME_MAX: u16 = 50;
pub const FILE_ENCODING_MAX: usize = 64 * 1024;

impl Ms {
    /// `file_ms_alloc`.
    #[must_use]
    pub fn new(flags: u32) -> Ms {
        Ms {
            mlist: [None, None],
            c: vec![LevelInfo::default(); 10],
            o_buf: None,
            o_blen: 0,
            offset: 0,
            eoffset: 0,
            error: -1,
            flags,
            event_flags: 0,
            file: Some(b"unknown".to_vec()),
            line: 0,
            mode: 0,
            search: Search::default(),
            ms_value: Value::default(),
            indir_max: FILE_INDIR_MAX,
            name_max: FILE_NAME_MAX,
            elf_shnum_max: FILE_ELF_SHNUM_MAX,
            elf_phnum_max: FILE_ELF_PHNUM_MAX,
            elf_notes_max: FILE_ELF_NOTES_MAX,
            regex_max: FILE_REGEX_MAX_U16,
            bytes_max: FILE_BYTES_MAX,
            encoding_max: FILE_ENCODING_MAX,
            elf_shsize_max: FILE_ELF_SHSIZE_MAX,
            quiet: false,
            utf8: false,
            errno: None,
            builtin: None,
        }
    }

    /// Whether an error has been recorded.
    #[must_use]
    pub fn had_err(&self) -> bool {
        self.event_flags & EVENT_HAD_ERR != 0
    }

    /// `file_clearbuf`.
    pub fn clearbuf(&mut self) {
        self.o_buf = None;
        self.o_blen = 0;
    }

    /// `file_printf`: `fmt` applied to `args`, appended to the output.
    ///
    /// Returns -1 when the piece could not be written, which has then been
    /// recorded as the error; 0 otherwise -- including when an earlier error
    /// stops anything more being written.
    pub fn printf(&mut self, fmt: &[u8], args: &[Arg<'_>]) -> i32 {
        if self.had_err() {
            return 0;
        }
        if let Err(why) = file_checkfmt(fmt) {
            self.clearbuf();
            let mut msg = b"Bad magic format `".to_vec();
            msg.extend_from_slice(cstr(fmt));
            msg.extend_from_slice(b"' (");
            msg.extend_from_slice(&why);
            msg.push(b')');
            self.error(None, &msg);
            return -1;
        }
        let piece = printf::format(fmt, args);
        self.append(piece.ok())
    }

    /// `file_printf(ms, "%s", s)`.
    pub fn print_str(&mut self, s: &[u8]) -> i32 {
        self.printf(b"%s", &[Arg::Str(s)])
    }

    /// The end of `file_vprintf`: the limits, then the append. `None` is
    /// `vasprintf` failing.
    fn append(&mut self, piece: Option<Vec<u8>>) -> i32 {
        if self.had_err() {
            return 0;
        }
        let len: i64 = piece.as_ref().map_or(-1, |p| i64::try_from(p.len()).unwrap_or(i64::MAX));
        let blen = self.o_blen;
        let over = len < 0
            || len > 1024
            || usize::try_from(len).unwrap_or(usize::MAX).saturating_add(blen) > 1024 * 1024;
        let Some(piece) = piece.filter(|_| !over) else {
            self.clearbuf();
            let msg = format!("Output buffer space exceeded {}+{}", len.clamp(i64::from(i32::MIN), i64::from(i32::MAX)), blen);
            self.error(None, msg.as_bytes());
            return -1;
        };
        match self.o_buf.take() {
            None => {
                self.o_blen = piece.len();
                self.o_buf = Some(piece);
            }
            Some(old) => {
                // `asprintf("%s%s", old, piece)`: both are C strings here.
                let mut s = cstr(&old).to_vec();
                s.extend_from_slice(cstr(&piece));
                self.o_blen = s.len();
                self.o_buf = Some(s);
            }
        }
        0
    }

    /// `file_error_core`: record the first error -- the message replaces the
    /// output, prefixed with the magic line when there is one.
    fn error_core(&mut self, err: Option<&std::io::Error>, msg: &[u8], lineno: usize) {
        if self.had_err() {
            return;
        }
        if lineno != 0 {
            self.clearbuf();
            self.printf(b"line %zu:", &[Arg::I64(lineno as u64)]);
        }
        if self.o_buf.as_ref().is_some_and(|b| b.first().is_some_and(|&c| c != 0)) {
            self.printf(b" ", &[]);
        }
        self.append(Some(msg.to_vec()));
        if let Some(e) = err {
            let text = errmsg::strerror(e);
            self.printf(b" (%s)", &[Arg::Str(text.as_bytes())]);
        }
        self.event_flags |= EVENT_HAD_ERR;
        self.error = err.and_then(std::io::Error::raw_os_error).unwrap_or(0);
    }

    /// `file_error`.
    pub fn error(&mut self, err: Option<&std::io::Error>, msg: &[u8]) {
        self.error_core(err, msg, 0);
    }

    /// `file_magerror`: an error on the magic line being read or matched.
    pub fn magerror(&mut self, msg: &[u8]) {
        let line = self.line;
        self.error_core(None, msg, line);
    }

    /// `file_badread`.
    pub fn badread(&mut self, e: &std::io::Error) {
        self.error(Some(e), b"error reading");
    }

    /// `file_magwarn` (from `print.c`): a warning about the magic, on stderr
    /// at once, after whatever stdout holds.
    pub fn magwarn(&self, msg: &[u8]) {
        if self.quiet {
            return;
        }
        // "cuz we use stdout for most, stderr here"
        let _flushed = crate::out::flush();
        let mut w = Vec::new();
        if let Some(f) = &self.file {
            w.extend_from_slice(f);
            w.extend_from_slice(format!(", {}: ", self.line).as_bytes());
        }
        w.extend_from_slice(b"Warning: ");
        w.extend_from_slice(msg);
        w.push(b'\n');
        // A warning that cannot be written has nowhere else to go.
        let _written = std::io::stderr().write_all(&w);
    }

    /// `file_magwarn(NULL, ...)`: the same without the file and line.
    pub fn magwarn_bare(&self, msg: &[u8]) {
        if self.quiet {
            return;
        }
        let _flushed = crate::out::flush();
        let mut w = b"Warning: ".to_vec();
        w.extend_from_slice(msg);
        w.push(b'\n');
        let _written = std::io::stderr().write_all(&w);
    }

    /// `file_separator`.
    pub fn separator(&mut self) -> i32 {
        self.printf(FILE_SEPARATOR, &[])
    }

    /// `trim_separator`: drop a separator the output ends with.
    fn trim_separator(&mut self) {
        let Some(buf) = self.o_buf.as_mut() else {
            return;
        };
        let l = crate::cstd::cstrlen(buf);
        // `sizeof(FILE_SEPARATOR)` counts the NUL.
        if l < FILE_SEPARATOR.len() + 1 {
            return;
        }
        let at = l - FILE_SEPARATOR.len();
        if buf.get(at..l) == Some(FILE_SEPARATOR) {
            buf.truncate(at);
        }
    }

    /// `checkdone`: whether to stop at this answer (`-k` keeps looking).
    fn checkdone(&mut self, rv: &mut i32) -> bool {
        if self.flags & MAGIC_CONTINUE == 0 {
            return true;
        }
        if self.separator() == -1 {
            *rv = -1;
        }
        false
    }

    /// `file_default`: what to say about a file nothing recognised, when
    /// the answer is a MIME type, an Apple type or an extension.
    pub fn default_answer(&mut self, nb: usize) -> i32 {
        if self.flags & MAGIC_MIME != 0 {
            if self.flags & MAGIC_MIME_TYPE != 0 {
                let t: &[u8] = if nb != 0 { b"octet-stream" } else { b"x-empty" };
                if self.printf(b"application/%s", &[Arg::Str(t)]) == -1 {
                    return -1;
                }
            }
            return 1;
        }
        if self.flags & MAGIC_APPLE != 0 {
            if self.print_str(b"UNKNUNKN") == -1 {
                return -1;
            }
            return 1;
        }
        if self.flags & MAGIC_EXTENSION != 0 {
            if self.print_str(b"???") == -1 {
                return -1;
            }
            return 1;
        }
        0
    }

    /// `file_reset`.
    pub fn reset(&mut self, checkloaded: bool) -> i32 {
        if checkloaded && self.mlist[0].is_none() {
            self.error(None, b"no magic files loaded");
            return -1;
        }
        self.clearbuf();
        self.event_flags &= !EVENT_HAD_ERR;
        self.error = -1;
        0
    }

    /// `file_getbuffer`: the answer, with what the terminal should not be
    /// sent written as `\ooo` -- unless `-r` asked for it raw.
    #[must_use]
    pub fn getbuffer(&self) -> Option<Vec<u8>> {
        if self.had_err() {
            return None;
        }
        let buf = self.o_buf.as_ref()?;
        let s = cstr(buf);
        if self.flags & MAGIC_RAW != 0 {
            return Some(s.to_vec());
        }
        if self.utf8 {
            if let Some(out) = printable_mb(s) {
                return Some(out);
            }
        }
        let mut out = Vec::with_capacity(s.len().saturating_mul(4));
        for &c in s {
            if isprint(c) {
                out.push(c);
            } else {
                octalify(&mut out, c);
            }
        }
        Some(out)
    }

    /// `file_check_mem`: make room for continuation level `level`, and clear
    /// its match.
    pub fn check_mem(&mut self, level: usize) {
        if level >= self.c.len() {
            self.c.resize(level.saturating_add(20), LevelInfo::default());
        }
        if let Some(li) = self.c.get_mut(level) {
            li.got_match = false;
        }
    }

    /// `file_push_buffer`: set the output aside, to collect a piece of it
    /// separately. `None` when an error stops anything more being written.
    pub fn push_buffer(&mut self) -> Option<PushBuf> {
        if self.had_err() {
            return None;
        }
        let pb = PushBuf {
            buf: self.o_buf.take(),
            blen: self.o_blen,
            offset: self.offset,
        };
        self.o_blen = 0;
        self.offset = 0;
        Some(pb)
    }

    /// `file_pop_buffer`: the piece collected since the push, and the output
    /// as it was. After an error the error stays, and there is no piece.
    pub fn pop_buffer(&mut self, pb: PushBuf) -> Option<Vec<u8>> {
        if self.had_err() {
            return None;
        }
        let rbuf = self.o_buf.take();
        self.o_buf = pb.buf;
        self.o_blen = pb.blen;
        self.offset = pb.offset;
        rbuf
    }
}

/// `FILE_REGEX_MAX`, the default of `ms->regex_max`.
const FILE_REGEX_MAX_U16: u16 = 8192;

/// `file_pushbuf_t`.
pub struct PushBuf {
    buf: Option<Vec<u8>>,
    blen: usize,
    offset: u32,
}

/// `OCTALIFY`: `\` and three octal digits.
fn octalify(out: &mut Vec<u8>, c: u8) {
    out.push(b'\\');
    out.push(((c >> 6) & 3) + b'0');
    out.push(((c >> 3) & 7) + b'0');
    out.push((c & 7) + b'0');
}

/// `file_getbuffer`'s multibyte path in a UTF-8 locale: each character
/// `iswprint` accepts as it is, every byte of any other in octal. `None` when
/// a sequence does not decode, which sends the whole buffer down the bytewise
/// path instead.
fn printable_mb(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len().saturating_mul(4));
    let mut i = 0usize;
    while i < s.len() {
        let rest = s.get(i..).unwrap_or_default();
        let (ch, n) = decode_utf8(rest)?;
        let bytes = rest.get(..n).unwrap_or_default();
        if iswprint(ch) {
            out.extend_from_slice(bytes);
        } else {
            for &b in bytes {
                octalify(&mut out, b);
            }
        }
        i += n;
    }
    Some(out)
}

/// One character from the front of `s` as `mbrtowc` reads UTF-8: `None` for
/// an invalid or incomplete sequence (its -1 and -2).
#[must_use]
pub fn decode_utf8(s: &[u8]) -> Option<(char, usize)> {
    let head = s.get(..s.len().min(4)).unwrap_or(s);
    let valid = match std::str::from_utf8(head) {
        Ok(t) => t,
        Err(e) => std::str::from_utf8(head.get(..e.valid_up_to()).unwrap_or_default()).ok()?,
    };
    let ch = valid.chars().next()?;
    Some((ch, ch.len_utf8()))
}

/// `iswprint` in a UTF-8 locale.
///
/// glibc's rule is: assigned, not a control, and not a space other than
/// U+0020 -- where its spaces are the separators (`Zs`, `Zl`, `Zp`) less the
/// three no-break ones. This follows it except for unassigned code points,
/// which glibc does not print and this does, for want of the table: the
/// database's descriptions have none, and everything a rule read from a file
/// is escaped by `file_printable` before it reaches the output.
#[must_use]
pub fn iswprint(c: char) -> bool {
    let cp = u32::from(c);
    if charwidth::char_width(c).is_none() {
        return false;
    }
    !matches!(cp, 0x1680 | 0x2000..=0x2006 | 0x2008..=0x200a | 0x2028 | 0x2029 | 0x205f | 0x3000)
}

/// `file_checkfield`: a width or precision under 1024.
fn checkfield(fmt: &[u8], p: &mut usize, what: &str) -> Result<(), Vec<u8>> {
    let mut fw: u64 = 0;
    while let Some(&d) = fmt.get(*p).filter(|&&d| isdigit(d)) {
        fw = fw.saturating_mul(10).saturating_add(u64::from(d - b'0'));
        *p += 1;
    }
    if fw < 1024 {
        return Ok(());
    }
    Err(format!("field {what} too large: {}", i32::try_from(fw).unwrap_or(i32::MAX)).into_bytes())
}

/// `file_checkfmt`: the format a description may be -- no `*`, widths and
/// precisions under 1024, and a letter for every conversion.
///
/// # Errors
/// The reason, as upstream words it.
pub fn file_checkfmt(fmt: &[u8]) -> Result<(), Vec<u8>> {
    let fmt = cstr(fmt);
    let mut p = 0usize;
    while p < fmt.len() {
        if fmt[p] != b'%' {
            p += 1;
            continue;
        }
        p += 1;
        if fmt.get(p) == Some(&b'%') {
            p += 1;
            continue;
        }
        while fmt.get(p).is_some_and(|c| b"#0.'+- ".contains(c)) {
            p += 1;
        }
        if fmt.get(p) == Some(&b'*') {
            return Err(b"* not allowed in format".to_vec());
        }
        checkfield(fmt, &mut p, "width")?;
        if fmt.get(p) == Some(&b'.') {
            p += 1;
            checkfield(fmt, &mut p, "precision")?;
        }
        let c = fmt.get(p).copied().unwrap_or(0);
        if !c.is_ascii_alphabetic() {
            let mut m = b"bad format char: ".to_vec();
            m.push(c);
            return Err(m);
        }
        p += 1;
    }
    Ok(())
}

/// `file_printable`: `s` (up to `slen` bytes, and its NUL) with each byte
/// `isprint` refuses written as `\ooo` -- every byte as it is under `-r` --
/// into a buffer of `bufsiz` bytes, NUL included.
#[must_use]
pub fn file_printable(raw: bool, bufsiz: usize, s: &[u8], slen: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let eptr = bufsiz.saturating_sub(1);
    for &c in s.iter().take(slen) {
        if out.len() >= eptr || c == 0 {
            break;
        }
        if raw || isprint(c) {
            out.push(c);
            continue;
        }
        if out.len() + 3 >= eptr {
            break;
        }
        out.push(b'\\');
        out.push(((c >> 6) & 7) + b'0');
        out.push(((c >> 3) & 7) + b'0');
        out.push((c & 7) + b'0');
    }
    out
}

/// `file_strtrim`: `s` without the white space at either end.
///
/// A string of nothing but white space trims to nothing. (C's loop walks off
/// the front of such a string looking for its last non-space; what it returns
/// is still the empty string at its end.)
#[must_use]
pub fn file_strtrim(s: &[u8]) -> &[u8] {
    let s = cstr(s);
    let start = s.iter().take_while(|&&c| isspace(c)).count();
    let rest = s.get(start..).unwrap_or_default();
    let keep = rest.len() - rest.iter().rev().take_while(|&&c| isspace(c)).count();
    rest.get(..keep).unwrap_or_default()
}

/// `file_print_guid`: the 16 bytes of `struct guid` -- a little-endian
/// `uint32_t` and two `uint16_t`, then eight bytes -- as
/// `XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX`.
#[must_use]
pub fn file_print_guid(g: &[u8; 16]) -> Vec<u8> {
    let d1 = u32::from_le_bytes([g[0], g[1], g[2], g[3]]);
    let d2 = u16::from_le_bytes([g[4], g[5]]);
    let d3 = u16::from_le_bytes([g[6], g[7]]);
    format!(
        "{d1:08X}-{d2:04X}-{d3:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        g[8], g[9], g[10], g[11], g[12], g[13], g[14], g[15]
    )
    .into_bytes()
}

/// The regex libmagic compiles: an ERE in the C locale, a byte at a time,
/// with `REG_NEWLINE` and `REG_ICASE` as asked.
#[derive(Clone, Copy, Debug, Default)]
pub struct RegFlags {
    pub icase: bool,
    pub newline: bool,
}


/// The number glibc's `regcomp` returns for an error.
fn regcode_number(code: ere::RegCode) -> i32 {
    use ere::RegCode as C;
    match code {
        C::BadPattern => 2,
        C::BadCollation => 3,
        C::BadCharClass => 4,
        C::TrailingBackslash => 5,
        C::BadBackReference => 6,
        C::UnmatchedBracket => 7,
        C::UnmatchedParen => 8,
        C::UnmatchedBrace => 9,
        C::BadBraceContent => 10,
        C::BadRangeEnd => 11,
        C::BadRepeat => 13,
        C::TooBig => 15,
        C::UnmatchedRightParen => 16,
    }
}

/// `check_regex`: refuse a doubled repetition operator and anything outside
/// printable ASCII and white space, with a warning.
fn check_regex(ms: &Ms, pat: &[u8]) -> bool {
    let pat = cstr(pat);
    let mut oc = 0u8;
    for &c in pat {
        if c == oc && b"?*+{".contains(&c) {
            let mut w = format!("repetition-operator operand `{}' invalid in regex `", char::from(c)).into_bytes();
            w.extend_from_slice(&file_printable(ms.flags & MAGIC_RAW != 0, 512, pat, pat.len()));
            w.push(b'\'');
            ms.magwarn(&w);
            return false;
        }
        oc = c;
        if isprint(c) || isspace(c) || c == 0x08 || c == 0x8a {
            continue;
        }
        let mut w = format!("non-ascii characters in regex \\{} `", c_octal_alt(c)).into_bytes();
        w.extend_from_slice(&file_printable(ms.flags & MAGIC_RAW != 0, 512, pat, pat.len()));
        w.push(b'\'');
        ms.magwarn(&w);
        return false;
    }
    true
}

/// `%#o`: octal with a leading zero, or `0` for zero.
fn c_octal_alt(c: u8) -> String {
    if c == 0 { "0".to_string() } else { format!("0{c:o}") }
}

/// `file_regcomp`: `Err(-1)` for a pattern `check_regex` refused, `Err(rc)`
/// for one the compiler refused -- with an error recorded when checking.
///
/// # Errors
/// As above.
pub fn file_regcomp(ms: &mut Ms, pat: &[u8], fl: RegFlags) -> Result<ere::Regex, i32> {
    if !check_regex(ms, pat) {
        return Err(-1);
    }
    let pat = cstr(pat);
    let syntax = if fl.newline {
        ere::Syntax::POSIX_EXTENDED.reg_newline()
    } else {
        ere::Syntax::POSIX_EXTENDED
    };
    match ere::Regex::new_syntax(pat, fl.icase, syntax) {
        Ok(rx) => Ok(rx.with_byte_chars(true).with_newline_anchor(fl.newline)),
        Err(e) => {
            let rc = regcode_number(e.code);
            if ms.flags & MAGIC_CHECK != 0 {
                let mut msg = format!("regex error {rc} for `").into_bytes();
                msg.extend_from_slice(&file_printable(ms.flags & MAGIC_RAW != 0, 512, pat, pat.len()));
                msg.extend_from_slice(b"', (");
                msg.extend_from_slice(e.message().as_bytes());
                msg.push(b')');
                ms.magerror(&msg);
            }
            Err(rc)
        }
    }
}

/// `file_regexec` with one match wanted: the subject is a C string, so it
/// ends at its first NUL. `Ok(None)` is `REG_NOMATCH`.
///
/// # Errors
/// A search the engine abandoned, which glibc has no counterpart for; the
/// callers treat it as `regexec` failing.
pub fn file_regexec(rx: &ere::Regex, subject: &[u8]) -> Result<Option<(usize, usize)>, i32> {
    rx.find(cstr(subject)).map_err(|_| 12)
}

/// `file_replace`: replace every match of `pat` in the output with `rep`.
/// The number replaced, or -1.
pub fn file_replace(ms: &mut Ms, pat: &[u8], rep: &[u8]) -> i32 {
    let Ok(rx) = file_regcomp(ms, pat, RegFlags::default()) else {
        return -1;
    };
    let mut nm = 0;
    while let Some(buf) = ms.o_buf.clone() {
        let Ok(Some((so, eo))) = file_regexec(&rx, &buf) else {
            break;
        };
        // `ms->o.buf[rm.rm_so] = '\0'`, then append the replacement and the
        // rest after the match to what is left.
        let head = buf.get(..so).unwrap_or_default().to_vec();
        let tail = if eo != 0 { cstr(buf.get(eo..).unwrap_or_default()).to_vec() } else { Vec::new() };
        ms.o_buf = Some(head.clone());
        ms.o_blen = head.len();
        if ms.printf(b"%s%s", &[Arg::Str(rep), Arg::Str(&tail)]) == -1 {
            return -1;
        }
        nm += 1;
    }
    nm
}

/// `file_buffer`: identify `buf`, the first bytes of the file -- by every
/// test in turn, in upstream's order. 1 found, 0 not, -1 an error.
pub fn file_buffer(ms: &mut Ms, fd: Option<&File>, st: Option<Stat>, inname: Option<&[u8]>, buf: &[u8]) -> i32 {
    let nb = buf.len();
    let mut m = 0;
    let mut rv = 0;
    let mut looks_text = false;
    let mut code: Option<&'static str> = None;
    let mut code_mime: &'static str = "binary";
    let mut def: &[u8] = b"data";
    let mut rbuf: Option<Vec<u8>> = None;
    let b = Buffer::new(fd, st, buf);
    ms.mode = b.st.mode;
    let debug = ms.flags & MAGIC_DEBUG != 0;
    let dbg = |name: &str, m: i32| {
        if debug {
            let _written = writeln!(std::io::stderr(), "[try {name} {m}]");
        }
    };

    'tests: {
        if nb == 0 {
            def = b"empty";
            break 'tests;
        } else if nb == 1 {
            def = b"very short file (no magic)";
            break 'tests;
        }
        if ms.flags & MAGIC_NO_CHECK_ENCODING == 0 {
            let enc = file_encoding(buf, ms.encoding_max);
            looks_text = enc.looks_text;
            code = Some(enc.code);
            code_mime = enc.code_mime;
        }
        if ms.flags & MAGIC_NO_CHECK_COMPRESS == 0 {
            m = crate::compress::file_zmagic(ms, &b, inname);
            dbg("zmagic", m);
            if m != 0 {
                // `goto done_encoding`: neither the separator trim nor the
                // encoding.
                return if rv != 0 { rv } else { m };
            }
        }
        if ms.flags & MAGIC_NO_CHECK_TAR == 0 {
            m = crate::is_tar::file_is_tar(ms, &b);
            dbg("tar", m);
            if m != 0 && ms.checkdone(&mut rv) {
                return finish(ms, rv, m, code_mime);
            }
        }
        if ms.flags & MAGIC_NO_CHECK_JSON == 0 {
            m = crate::is_json::file_is_json(ms, &b);
            dbg("json", m);
            if m != 0 && ms.checkdone(&mut rv) {
                return finish(ms, rv, m, code_mime);
            }
        }
        if ms.flags & MAGIC_NO_CHECK_CSV == 0 {
            m = crate::is_csv::file_is_csv(ms, &b, looks_text, code);
            dbg("csv", m);
            if m != 0 && ms.checkdone(&mut rv) {
                return finish(ms, rv, m, code_mime);
            }
        }
        if ms.flags & MAGIC_NO_CHECK_SIMH == 0 {
            m = crate::is_simh::file_is_simh(ms, &b);
            dbg("simh", m);
            if m != 0 && ms.checkdone(&mut rv) {
                return finish(ms, rv, m, code_mime);
            }
        }
        if ms.flags & MAGIC_NO_CHECK_CDF == 0 {
            m = crate::readcdf::file_trycdf(ms, &b);
            dbg("cdf", m);
            if m != 0 && ms.checkdone(&mut rv) {
                return finish(ms, rv, m, code_mime);
            }
        }
        if ms.flags & MAGIC_NO_CHECK_ELF == 0 && nb > 5 && fd.is_some() {
            // The ELF details are collected apart and printed after the
            // rules' answer, if the rules give one.
            let Some(pb) = ms.push_buffer() else {
                return -1;
            };
            rv = crate::readelf::file_tryelf(ms, &b);
            rbuf = ms.pop_buffer(pb);
            if rv == -1 {
                rbuf = None;
            }
            // Upstream prints `m` here, not the result it just got.
            dbg("elf", m);
        }
        if ms.flags & MAGIC_NO_CHECK_SOFT == 0 {
            m = crate::softmagic::file_softmagic(ms, &b, None, BINTEST, looks_text);
            dbg("softmagic", m);
            if m == 1 {
                if let Some(r) = rbuf.as_deref() {
                    if ms.print_str(r) == -1 {
                        return finish(ms, rv, m, code_mime);
                    }
                }
            }
            if m != 0 && ms.checkdone(&mut rv) {
                return finish(ms, rv, m, code_mime);
            }
        }
        if ms.flags & MAGIC_NO_CHECK_TEXT == 0 {
            m = crate::ascmagic::file_ascmagic(ms, &b, looks_text);
            dbg("ascmagic", m);
            if m != 0 {
                return finish(ms, rv, m, code_mime);
            }
        }
    }
    // `simple:` -- give up.
    if m == 0 {
        m = 1;
        rv = ms.default_answer(nb);
        if rv == 0 && ms.print_str(def) == -1 {
            rv = -1;
        }
    }
    finish(ms, rv, m, code_mime)
}

/// `done:` -- drop a trailing separator, add the encoding when asked, and
/// return `rv` if it is set, else `m`.
fn finish(ms: &mut Ms, mut rv: i32, m: i32, code_mime: &str) -> i32 {
    ms.trim_separator();
    if ms.flags & MAGIC_MIME_ENCODING != 0 {
        if ms.flags & MAGIC_MIME_TYPE != 0 && ms.print_str(b"; charset=") == -1 {
            rv = -1;
        }
        if ms.print_str(code_mime.as_bytes()) == -1 {
            rv = -1;
        }
    }
    if rv != 0 { rv } else { m }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn output_is_built_and_limited_as_libmagic_limits_it() {
        let mut ms = Ms::new(0);
        assert_eq!(ms.printf(b"%s, %d", &[Arg::Str(b"ELF"), Arg::I32(64)]), 0);
        assert_eq!(ms.getbuffer().unwrap(), b"ELF, 64");
        // A piece over 1024 bytes is an error, and replaces the output.
        let big = vec![b'x'; 1025];
        assert_eq!(ms.print_str(&big), -1);
        assert!(ms.had_err());
        assert_eq!(ms.o_buf.as_deref(), Some(&b"Output buffer space exceeded 1025+7"[..]));
        assert_eq!(ms.getbuffer(), None);
        // Nothing more is written after an error.
        assert_eq!(ms.print_str(b"more"), 0);
    }

    #[test]
    fn a_bad_description_format_is_refused() {
        let mut ms = Ms::new(0);
        assert_eq!(ms.printf(b"%*d", &[Arg::I32(1), Arg::I32(2)]), -1);
        assert_eq!(ms.o_buf.as_deref(), Some(&b"Bad magic format `%*d' (* not allowed in format)"[..]));
        assert!(file_checkfmt(b"%-10.3s and %#x").is_ok());
        assert_eq!(file_checkfmt(b"%2000d").unwrap_err(), b"field width too large: 2000");
        // `.` is among the flags skipped first, so this is a width.
        assert_eq!(file_checkfmt(b"%.2000d").unwrap_err(), b"field width too large: 2000");
        assert_eq!(file_checkfmt(b"%5.2000d").unwrap_err(), b"field precision too large: 2000");
        assert_eq!(file_checkfmt(b"%5!").unwrap_err(), b"bad format char: !");
    }

    #[test]
    fn errors_name_the_magic_line() {
        let mut ms = Ms::new(0);
        ms.print_str(b"partial");
        ms.line = 12;
        ms.magerror(b"zerodivide in mconvert()");
        assert_eq!(ms.o_buf.as_deref(), Some(&b"line 12: zerodivide in mconvert()"[..]));
        // Only the first error is kept.
        ms.error(None, b"second");
        assert_eq!(ms.o_buf.as_deref(), Some(&b"line 12: zerodivide in mconvert()"[..]));
    }

    #[test]
    fn the_answer_is_escaped_for_the_terminal() {
        let mut ms = Ms::new(0);
        ms.print_str(b"a\x01b\xe9");
        assert_eq!(ms.getbuffer().unwrap(), b"a\\001b\\351");
        ms.utf8 = true;
        ms.reset(false);
        ms.print_str("5.25\u{2033} \x01".as_bytes());
        assert_eq!(ms.getbuffer().unwrap(), "5.25\u{2033} \\001".as_bytes());
        // One undecodable byte sends the whole answer down the byte path.
        ms.reset(false);
        let mixed = b"\xe2\x80\xb3\xff";
        ms.print_str(mixed);
        assert_eq!(ms.getbuffer().unwrap(), b"\\342\\200\\263\\377");
        ms.flags |= MAGIC_RAW;
        assert_eq!(ms.getbuffer().unwrap(), mixed);
    }

    #[test]
    fn separators_are_trimmed_from_the_end_only() {
        let mut ms = Ms::new(MAGIC_CONTINUE);
        ms.print_str(b"one");
        ms.separator();
        ms.print_str(b"two");
        ms.separator();
        ms.trim_separator();
        assert_eq!(ms.o_buf.as_deref(), Some(&b"one\n- two"[..]));
    }

    #[test]
    fn printable_and_trim_match_upstream() {
        assert_eq!(file_printable(false, 512, b"ab\x7f\0cd", 6), b"ab\\177");
        assert_eq!(file_printable(false, 6, b"abcdefgh", 8), b"abcde");
        assert_eq!(file_printable(false, 6, b"ab\x01", 3), b"ab");
        assert_eq!(file_printable(true, 512, b"\x01\xff", 2), b"\x01\xff");
        assert_eq!(file_strtrim(b"  hi there \t\0junk"), b"hi there");
        assert_eq!(file_strtrim(b"   "), b"");
    }

    #[test]
    fn guids_print_as_microsoft_writes_them() {
        let g = [0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
        assert_eq!(file_print_guid(&g), b"00112233-4455-6677-8899-AABBCCDDEEFF");
    }

    #[test]
    fn regexes_are_checked_then_compiled_in_the_c_locale() {
        let mut ms = Ms::new(MAGIC_CHECK);
        ms.quiet = true;
        assert_eq!(file_regcomp(&mut ms, b"a**", RegFlags::default()).err(), Some(-1));
        assert_eq!(file_regcomp(&mut ms, b"caf\xc3\xa9", RegFlags::default()).err(), Some(-1));
        let rx = file_regcomp(&mut ms, b"^x.y$", RegFlags { icase: true, newline: true }).unwrap();
        assert_eq!(file_regexec(&rx, b"ab\nX-Y\nz").unwrap(), Some((3, 6)));
        assert_eq!(file_regexec(&rx, b"x\ny").unwrap(), None);
        // The subject ends at its NUL.
        assert_eq!(file_regexec(&rx, b"q\0x-y").unwrap(), None);
        assert_eq!(file_regcomp(&mut ms, b"a[b", RegFlags::default()).err(), Some(7));
        // Line 0 is no line: no prefix.
        assert_eq!(ms.o_buf.as_deref(), Some(&b"regex error 7 for `a[b', (Unmatched [, [^, [:, [., or [=)"[..]));
    }
}
