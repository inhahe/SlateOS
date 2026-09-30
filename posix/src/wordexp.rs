// Every index is into the words being expanded, below their length (each
// read of `s[i]` checks `i < s.len()` first, through `at`), or into a buffer
// below its own length; every sum is of such lengths. Clippy cannot see the
// bounds.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
//! POSIX word expansion (`<wordexp.h>`): `wordexp` and `wordfree` -- the
//! expansions a shell makes of a utility's arguments (XCU 2.6): tilde
//! expansion, parameter expansion, command substitution and arithmetic
//! expansion, then field splitting by `IFS`, pathname expansion and quote
//! removal. glibc 2.39's, with the fixes to it Ubuntu's 2.39 carries
//! (CVE-2025-15281, CVE-2026-6368, CVE-2026-6791), where it agrees with
//! POSIX; its answers to 320 cases are replayed by the tests
//! (posix/tools/oracle/wordexp_harness.py, posix/src/wordexp_oracle.txt).
//!
//! As glibc's:
//!
//! - **Words** are separated by unquoted blanks; an unquoted newline, `|`,
//!   `&`, `;`, `<`, `>`, `(`, `)`, `{` or `}` is `WRDE_BADCHAR`; `#` is an
//!   ordinary character.
//! - **Parameters**: `$name`, `${name}`, the operators `-` `=` `?` `+`
//!   (with and without `:`), `#` `##` `%` `%%` and `${#name}`, the
//!   positional parameters -- the program's own arguments -- and `$#`, `$@`,
//!   `$*`, `$0`, `$$`. `${name=word}` sets the variable in the environment.
//! - **Command substitution**, `$(...)` and `` `...` ``, runs `/bin/sh -c`,
//!   its standard error discarded unless `WRDE_SHOWERR`; trailing newlines
//!   are removed. A command that printed nothing and failed is checked with
//!   `sh -nc`, and a syntax error there is `WRDE_SYNTAX`. `$(` ends at the
//!   matching `)`, counted as glibc counts it (quotes respected, a `case`
//!   pattern's `)` not known).
//! - **Arithmetic**, `$((...))` and glibc's `$[...]`, in `long` (64 bits,
//!   wrapping); `$((` is command substitution when its first unmatched `)` is
//!   not followed by another, as glibc tells the two apart.
//! - **Field splitting** of unquoted expansion results by `IFS` (unset: space,
//!   tab, newline), its white space and other characters as POSIX treats
//!   them; the words' own text is not split.
//! - **Flags**: `WRDE_DOOFFS` (`we_offs` NULLs first), `WRDE_APPEND` (the
//!   list is added to, and left as it was when the call fails), `WRDE_REUSE`
//!   (the list is freed first), `WRDE_NOCMD` (a command substitution is
//!   `WRDE_CMDSUB`), `WRDE_UNDEF` (an unset parameter is `WRDE_BADVAL`),
//!   `WRDE_SHOWERR`. A call that fails leaves the structure as it was; one
//!   that runs out of memory answers `WRDE_NOSPACE` with the words expanded
//!   until then, as POSIX says.
//!
//! Where glibc contradicts POSIX, this follows POSIX (each case is marked in
//! the tests):
//!
//! - **Arithmetic** has C's operators, not only `+ - * /`: glibc's reads
//!   `$((7%3))` as 7 and `$((1<<2))` as 1, the rest dropped; bare variable
//!   names, which glibc refuses, are read, and assignments made; `08` and `0x`
//!   are not numbers (glibc's reads 0).
//! - **Pathname expansion** applies to unquoted expansion results too: glibc
//!   leaves `$var` holding `t*` as `t*`.
//! - `$?` is 0 and `$-` empty, as a fresh shell's, and `$!` is unset; glibc
//!   leaves the three as text. `${#}` is `$#`, where glibc answers
//!   `WRDE_SYNTAX`; `"$@"` with no positional parameters is no field, where
//!   glibc makes an empty one.
//! - A tilde is expanded only at a word's start: `x=~` is an argument, not
//!   an assignment, and glibc expands it; `~"root"`'s quotes are removed,
//!   where glibc keeps them.
//! - `${name:-word}` and its kin test whether the parameter is set, and are
//!   no `WRDE_UNDEF` error when it is not, as no shell's `set -u` makes one
//!   of them; glibc's answers `WRDE_BADVAL`.
//!
//! Until 2026-09-30 this was a word splitter with `$VAR` and `~`: input past
//! 4096 bytes was cut off, no more than 256 words were kept, `WRDE_APPEND`
//! and `WRDE_DOOFFS` were ignored (an append leaked the list it replaced),
//! command substitution gave back its own text, and there was no arithmetic,
//! no other `${...}` form, no `IFS` splitting, no pathname expansion and no
//! `~user`.

use crate::errno;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Reserve `we_offs` NULL slots at the start of `we_wordv`.
pub const WRDE_DOOFFS: i32 = 1;
/// Append the words to those of an earlier call.
pub const WRDE_APPEND: i32 = 2;
/// Refuse command substitution (`WRDE_CMDSUB`).
pub const WRDE_NOCMD: i32 = 4;
/// Free the words of an earlier call first.
pub const WRDE_REUSE: i32 = 8;
/// Let command substitution's standard error through.
pub const WRDE_SHOWERR: i32 = 16;
/// An unset parameter is an error (`WRDE_BADVAL`).
pub const WRDE_UNDEF: i32 = 32;

/// Out of memory.
pub const WRDE_NOSPACE: i32 = 1;
/// An unquoted newline, `|`, `&`, `;`, `<`, `>`, `(`, `)`, `{` or `}`.
pub const WRDE_BADCHAR: i32 = 2;
/// An unset parameter, with `WRDE_UNDEF`.
pub const WRDE_BADVAL: i32 = 3;
/// A command substitution, with `WRDE_NOCMD`.
pub const WRDE_CMDSUB: i32 = 4;
/// A syntax error: an unterminated quote or construct, a bad expression.
pub const WRDE_SYNTAX: i32 = 5;

// ---------------------------------------------------------------------------
// wordexp_t
// ---------------------------------------------------------------------------

/// Result of word expansion.
#[repr(C)]
pub struct WordexpT {
    /// Number of words in `we_wordv`, after its `we_offs` NULLs.
    pub we_wordc: usize,
    /// The words: `we_offs` NULLs, `we_wordc` words, and a NULL.
    pub we_wordv: *mut *mut u8,
    /// The NULLs reserved at the start of `we_wordv` (`WRDE_DOOFFS`).
    pub we_offs: usize,
}

// ---------------------------------------------------------------------------
// Buffers
// ---------------------------------------------------------------------------

/// Why an expansion failed: each a `WRDE_*` answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Error {
    NoSpace,
    BadChar,
    BadVal,
    CmdSub,
    Syntax,
}

impl Error {
    const fn code(self) -> i32 {
        match self {
            Self::NoSpace => WRDE_NOSPACE,
            Self::BadChar => WRDE_BADCHAR,
            Self::BadVal => WRDE_BADVAL,
            Self::CmdSub => WRDE_CMDSUB,
            Self::Syntax => WRDE_SYNTAX,
        }
    }
}

type R<T> = Result<T, Error>;

/// A growable array from `malloc`, its elements dropped with it.
struct List<T> {
    ptr: *mut T,
    len: usize,
    cap: usize,
}

impl<T> List<T> {
    const fn new() -> Self {
        Self {
            ptr: core::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    /// Room for `more` elements beyond those held.
    fn reserve(&mut self, more: usize) -> R<()> {
        let need = self.len.checked_add(more).ok_or(Error::NoSpace)?;
        if need <= self.cap {
            return Ok(());
        }
        let cap = need.max(self.cap.saturating_mul(2)).max(8);
        let bytes = cap
            .checked_mul(core::mem::size_of::<T>())
            .ok_or(Error::NoSpace)?;
        // SAFETY: `ptr` is NULL or this list's own block.
        let p = unsafe { crate::malloc::realloc(self.ptr.cast(), bytes) }.cast::<T>();
        if p.is_null() {
            return Err(Error::NoSpace);
        }
        self.ptr = p;
        self.cap = cap;
        Ok(())
    }

    fn push(&mut self, v: T) -> R<()> {
        self.reserve(1)?;
        // SAFETY: `len < cap`, room reserved.
        unsafe { self.ptr.add(self.len).write(v) };
        self.len += 1;
        Ok(())
    }

    fn as_slice(&self) -> &[T] {
        if self.ptr.is_null() {
            return &[];
        }
        // SAFETY: `len` initialised elements.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    fn clear(&mut self) {
        while self.len > 0 {
            self.len -= 1;
            // SAFETY: element `len`, initialised, dropped once.
            unsafe { self.ptr.add(self.len).drop_in_place() };
        }
    }

    /// The elements, handed over: the block and its length, the list left
    /// empty. The caller frees the block (and drops the elements) itself.
    fn into_raw(mut self) -> (*mut T, usize) {
        let r = (self.ptr, self.len);
        self.ptr = core::ptr::null_mut();
        self.len = 0;
        self.cap = 0;
        r
    }
}

impl<T> Drop for List<T> {
    fn drop(&mut self) {
        self.clear();
        // SAFETY: `ptr` is NULL or this list's own block.
        unsafe { crate::malloc::free(self.ptr.cast()) };
    }
}

/// A growable byte string from `malloc`.
type Bytes = List<u8>;

impl Bytes {
    fn extend(&mut self, s: &[u8]) -> R<()> {
        self.reserve(s.len())?;
        // SAFETY: room for `s` reserved; the two do not overlap.
        unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), self.ptr.add(self.len), s.len()) };
        self.len += s.len();
        Ok(())
    }

    fn of(s: &[u8]) -> R<Self> {
        let mut b = Self::new();
        b.extend(s)?;
        Ok(b)
    }

    /// The bytes and a NUL, as a C string from `malloc` the caller owns.
    fn into_c_string(mut self) -> R<*mut u8> {
        self.push(0)?;
        Ok(self.into_raw().0)
    }

    /// The bytes and a NUL, for handing to a C function; the NUL is not
    /// counted afterwards.
    fn c_str(&mut self) -> R<*const u8> {
        self.push(0)?;
        self.len -= 1;
        Ok(self.ptr)
    }
}

// ---------------------------------------------------------------------------
// The system: what the expansions ask of the process
// ---------------------------------------------------------------------------

/// What word expansion asks of the process -- its environment, its users'
/// homes, a shell, a directory's names, its arguments -- so that the tests
/// can answer as the oracle's sandbox did.
trait Sys {
    /// The environment variable `name`.
    fn var(&mut self, name: &[u8]) -> R<Option<Bytes>>;
    /// Set the environment variable `name` to `value`.
    fn set_var(&mut self, name: &[u8], value: &[u8]) -> R<()>;
    /// `~`: HOME, or the home of the user the process runs as.
    fn home(&mut self) -> R<Option<Bytes>>;
    /// `~user`: that user's home.
    fn user_home(&mut self, user: &[u8]) -> R<Option<Bytes>>;
    /// `/bin/sh -c cmd` -- `-nc` when `check_only` -- its standard output
    /// and exit status.
    fn shell(&mut self, cmd: &[u8], check_only: bool, show_err: bool) -> R<(Bytes, i32)>;
    /// The names `pattern` matches, sorted, onto `out`.
    fn glob(&mut self, pattern: &[u8], out: &mut List<Bytes>) -> R<()>;
    /// The program's argument count, `$0` included.
    fn argc(&mut self) -> usize;
    /// Argument `i` (`$0` is 0).
    fn arg(&mut self, i: usize) -> R<Option<Bytes>>;
    /// `$$`.
    fn pid(&mut self) -> i32;
}

/// The process itself.
struct Process;

impl Process {
    /// A passwd entry's home: `f` looks the entry up into the buffer it is
    /// given, as `getpw*_r`; the buffer grows while `ERANGE`.
    fn passwd_home(
        f: impl Fn(*mut crate::pwd::Passwd, *mut u8, usize, *mut *const crate::pwd::Passwd) -> i32,
    ) -> R<Option<Bytes>> {
        let mut size = 1024usize;
        loop {
            let mut buf = Bytes::new();
            buf.reserve(size)?;
            // SAFETY: an all-zero passwd record is a valid one to fill.
            let mut pw: crate::pwd::Passwd = unsafe { core::mem::zeroed() };
            let mut result: *const crate::pwd::Passwd = core::ptr::null();
            let rc = f(&raw mut pw, buf.ptr, size, &raw mut result);
            if rc == errno::ERANGE && size < 1 << 20 {
                size *= 4;
                continue;
            }
            if rc != 0 || result.is_null() || pw.pw_dir.is_null() {
                return Ok(None);
            }
            // SAFETY: a found entry's home is a NUL-terminated string.
            let dir =
                unsafe { core::slice::from_raw_parts(pw.pw_dir, crate::string::strlen(pw.pw_dir)) };
            return Ok(Some(Bytes::of(dir)?));
        }
    }
}

impl Sys for Process {
    fn var(&mut self, name: &[u8]) -> R<Option<Bytes>> {
        let mut n = Bytes::of(name)?;
        // SAFETY: a NUL-terminated name.
        let v = unsafe { crate::environ::getenv(n.c_str()?) };
        if v.is_null() {
            return Ok(None);
        }
        // SAFETY: `getenv` answers a NUL-terminated string.
        Ok(Some(Bytes::of(unsafe {
            core::slice::from_raw_parts(v, crate::string::strlen(v))
        })?))
    }

    fn set_var(&mut self, name: &[u8], value: &[u8]) -> R<()> {
        let (mut n, mut v) = (Bytes::of(name)?, Bytes::of(value)?);
        // SAFETY: NUL-terminated strings.
        if unsafe { crate::environ::setenv(n.c_str()?, v.c_str()?, 1) } != 0 {
            return Err(Error::NoSpace);
        }
        Ok(())
    }

    fn home(&mut self) -> R<Option<Bytes>> {
        if let Some(h) = self.var(b"HOME")? {
            return Ok(Some(h));
        }
        let uid = crate::unistd::getuid();
        // SAFETY: the pointers are the closure's arguments, as getpwuid_r's.
        Self::passwd_home(|pw, buf, len, res| unsafe {
            crate::pwd::getpwuid_r(uid, pw, buf, len, res)
        })
    }

    fn user_home(&mut self, user: &[u8]) -> R<Option<Bytes>> {
        let mut name = Bytes::of(user)?;
        let name = name.c_str()?;
        // SAFETY: as `home`; `name` is NUL-terminated.
        Self::passwd_home(|pw, buf, len, res| unsafe {
            crate::pwd::getpwnam_r(name, pw, buf, len, res)
        })
    }

    fn shell(&mut self, cmd: &[u8], check_only: bool, show_err: bool) -> R<(Bytes, i32)> {
        let mut text = Bytes::of(cmd)?;
        let text = text.c_str()?;
        let mut fds = [-1i32; 2];
        if crate::pipe::pipe(fds.as_mut_ptr()) != 0 {
            return Ok((Bytes::new(), 127));
        }
        let [rd, wr] = fds;
        let mut out = Bytes::new();
        let mut status = 127;
        // The file actions object is initialised before use and destroyed
        // after; argv is a NULL-terminated array of C strings.
        {
            let mut acts = core::mem::MaybeUninit::<crate::spawn::PosixSpawnFileActionsT>::uninit();
            let acts = acts.as_mut_ptr();
            if crate::spawn::posix_spawn_file_actions_init(acts) == 0 {
                // A failed action fails the spawn, which answers as a shell
                // that could not be run.
                let _ = crate::spawn::posix_spawn_file_actions_adddup2(acts, wr, 1);
                let _ = crate::spawn::posix_spawn_file_actions_addclose(acts, rd);
                let _ = crate::spawn::posix_spawn_file_actions_addclose(acts, wr);
                if !show_err {
                    let _ = crate::spawn::posix_spawn_file_actions_addopen(
                        acts,
                        2,
                        c"/dev/null".as_ptr().cast(),
                        crate::fcntl::O_WRONLY,
                        0,
                    );
                }
                let opt: &core::ffi::CStr = if check_only { c"-nc" } else { c"-c" };
                let argv = [
                    c"sh".as_ptr().cast::<u8>(),
                    opt.as_ptr().cast(),
                    text,
                    core::ptr::null(),
                ];
                let mut pid: crate::types::PidT = 0;
                let rc = crate::spawn::posix_spawn(
                    &raw mut pid,
                    c"/bin/sh".as_ptr().cast(),
                    acts,
                    core::ptr::null(),
                    argv.as_ptr(),
                    crate::environ::current_environ(),
                );
                let _ = crate::spawn::posix_spawn_file_actions_destroy(acts);
                // The parent's copy of the write end, so that the read ends.
                let _ = crate::file::close(wr);
                if rc == 0 {
                    let mut chunk = [0u8; 4096];
                    loop {
                        let n = crate::file::read(rd, chunk.as_mut_ptr(), chunk.len());
                        let Ok(n) = usize::try_from(n) else {
                            if errno::get_errno() == errno::EINTR {
                                continue;
                            }
                            break;
                        };
                        if n == 0 {
                            break;
                        }
                        out.extend(&chunk[..n])?;
                    }
                    let mut st = 0i32;
                    while crate::process::waitpid(pid, &raw mut st, 0) < 0
                        && errno::get_errno() == errno::EINTR
                    {}
                    status = if crate::process::wifexited(st) {
                        crate::process::wexitstatus(st)
                    } else {
                        128
                    };
                }
            } else {
                let _ = crate::file::close(wr);
            }
            let _ = crate::file::close(rd);
        }
        Ok((out, status))
    }

    fn glob(&mut self, pattern: &[u8], out: &mut List<Bytes>) -> R<()> {
        let mut p = Bytes::of(pattern)?;
        // SAFETY: an all-zero glob_t is an empty one, as glob fills it.
        let mut g: crate::glob::GlobT = unsafe { core::mem::zeroed() };
        // SAFETY: a NUL-terminated pattern and a glob_t to fill.
        let rc = unsafe { crate::glob::glob(p.c_str()?, 0, None, &raw mut g) };
        let mut result = Ok(());
        if rc == 0 {
            for i in 0..g.gl_pathc {
                // SAFETY: `gl_pathc` names after `gl_offs` (0) NULLs.
                let name = unsafe { *g.gl_pathv.add(g.gl_offs + i) };
                // SAFETY: each a NUL-terminated string.
                let s = unsafe { core::slice::from_raw_parts(name, crate::string::strlen(name)) };
                if let Err(e) = Bytes::of(s).and_then(|b| out.push(b)) {
                    result = Err(e);
                    break;
                }
            }
        } else if rc == crate::glob::GLOB_NOSPACE {
            result = Err(Error::NoSpace);
        }
        // SAFETY: the glob_t glob filled.
        unsafe { crate::glob::globfree(&raw mut g) };
        result
    }

    fn argc(&mut self) -> usize {
        // SAFETY: a plain read of a word `__libc_start_main` wrote once.
        usize::try_from(unsafe { core::ptr::addr_of!(crate::crt::PROGRAM_ARGC).read() })
            .unwrap_or(0)
    }

    fn arg(&mut self, i: usize) -> R<Option<Bytes>> {
        if i >= self.argc() {
            return Ok(None);
        }
        // SAFETY: as `argc`; `i` is below the count of the array it names.
        let a = unsafe {
            let argv = core::ptr::addr_of!(crate::crt::PROGRAM_ARGV).read();
            if argv.is_null() {
                return Ok(None);
            }
            *argv.add(i)
        };
        if a.is_null() {
            return Ok(None);
        }
        // SAFETY: an argument is a NUL-terminated string.
        Ok(Some(Bytes::of(unsafe {
            core::slice::from_raw_parts(a, crate::string::strlen(a))
        })?))
    }

    fn pid(&mut self) -> i32 {
        crate::process::getpid()
    }
}

// ---------------------------------------------------------------------------
// Where expanded text goes
// ---------------------------------------------------------------------------

/// How a byte came into a word, which decides what the later steps do to it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Unquoted text of the words themselves: not split; a glob character.
    Literal,
    /// Quoted text, or the result of a quoted expansion (or of a tilde
    /// expansion): neither split nor globbed.
    Quoted,
    /// The result of an unquoted expansion: split by `IFS`, and globbed.
    Split,
}

/// Where expanded text goes: the fields being made, or a flat string (an
/// assignment's value, a pattern).
trait Sink {
    /// One byte, of the kind it came in as.
    fn put(&mut self, b: u8, kind: Kind) -> R<()>;
    /// Quotes were seen: the field exists even if it stays empty.
    fn live(&mut self) -> R<()>;
    /// End the field here (`"$@"` between parameters).
    fn break_field(&mut self) -> R<()>;
}

/// A finished field: its text, and -- when it holds an unquoted glob
/// character -- the same as a pattern, for pathname expansion afterwards.
struct Field {
    text: Bytes,
    pattern: Option<Bytes>,
}

/// The fields being made: the one in progress -- its text, the same as a
/// glob pattern, whether it holds a glob character, whether quotes made it
/// exist -- and the finished ones.
struct Fields {
    /// `IFS`'s white space (space, tab, newline) and its other characters.
    white: [bool; 256],
    other: [bool; 256],
    out: List<Field>,
    text: Bytes,
    pattern: Bytes,
    glob: bool,
    exists: bool,
    /// A delimiter being read from an unquoted expansion: `Some(true)`
    /// once it holds a character other than white space.
    delim: Option<bool>,
}

impl Fields {
    fn new(ifs: Option<&[u8]>) -> Self {
        let mut white = [false; 256];
        let mut other = [false; 256];
        for &c in ifs.unwrap_or(b" \t\n") {
            if matches!(c, b' ' | b'\t' | b'\n') {
                white[usize::from(c)] = true;
            } else {
                other[usize::from(c)] = true;
            }
        }
        Self {
            white,
            other,
            out: List::new(),
            text: Bytes::new(),
            pattern: Bytes::new(),
            glob: false,
            exists: false,
            delim: None,
        }
    }

    /// The field in progress has anything in it.
    fn has_content(&self) -> bool {
        self.exists || self.text.len > 0
    }

    /// A delimiter read from an expansion ends the field when the next byte
    /// comes (or the word ends): always when it held a non-white `IFS`
    /// character, else only a field with something in it.
    fn resolve(&mut self) -> R<()> {
        if let Some(other) = self.delim.take() {
            if other || self.has_content() {
                self.finish()?;
            }
        }
        Ok(())
    }

    /// The field in progress is done: its pattern kept when it holds an
    /// unquoted glob character.
    fn finish(&mut self) -> R<()> {
        let text = core::mem::replace(&mut self.text, Bytes::new());
        let pattern = core::mem::replace(&mut self.pattern, Bytes::new());
        let glob = core::mem::replace(&mut self.glob, false);
        self.exists = false;
        self.out.push(Field {
            text,
            pattern: glob.then_some(pattern),
        })
    }

    /// A blank between words: the field ends if it holds anything.
    fn separate(&mut self) -> R<()> {
        self.resolve()?;
        if self.has_content() {
            self.finish()?;
        }
        Ok(())
    }
}

impl Sink for Fields {
    fn put(&mut self, b: u8, kind: Kind) -> R<()> {
        if kind == Kind::Split {
            let (white, other) = (self.white[usize::from(b)], self.other[usize::from(b)]);
            if white {
                if self.delim.is_none() {
                    self.delim = Some(false);
                }
                return Ok(());
            }
            if other {
                // A second non-white character ends the first delimiter.
                if self.delim == Some(true) {
                    self.resolve()?;
                }
                self.delim = Some(true);
                return Ok(());
            }
        }
        self.resolve()?;
        self.text.push(b)?;
        match kind {
            Kind::Quoted => {
                if matches!(b, b'*' | b'?' | b'[' | b'\\') {
                    self.pattern.push(b'\\')?;
                }
            }
            Kind::Literal | Kind::Split => {
                if matches!(b, b'*' | b'?' | b'[') {
                    self.glob = true;
                }
            }
        }
        self.pattern.push(b)
    }

    fn live(&mut self) -> R<()> {
        // Quotes after a delimiter belong to the field after it.
        self.resolve()?;
        self.exists = true;
        Ok(())
    }

    fn break_field(&mut self) -> R<()> {
        self.separate()
    }
}

/// A flat string: the bytes, their quoting gone (an assignment's value, a
/// message).
struct Flat(Bytes);

impl Sink for Flat {
    fn put(&mut self, b: u8, _kind: Kind) -> R<()> {
        self.0.push(b)
    }
    fn live(&mut self) -> R<()> {
        Ok(())
    }
    fn break_field(&mut self) -> R<()> {
        self.0.push(b' ')
    }
}

/// A pattern for `#` `##` `%` `%%`: quoted bytes that are special to it
/// escaped, the rest as they came.
struct Pattern(Bytes);

impl Sink for Pattern {
    fn put(&mut self, b: u8, kind: Kind) -> R<()> {
        if kind == Kind::Quoted && matches!(b, b'*' | b'?' | b'[' | b'\\') {
            self.0.push(b'\\')?;
        }
        self.0.push(b)
    }
    fn live(&mut self) -> R<()> {
        Ok(())
    }
    fn break_field(&mut self) -> R<()> {
        self.0.push(b' ')
    }
}

// ---------------------------------------------------------------------------
// The expansions
// ---------------------------------------------------------------------------

/// Where in the text an expansion is: at the words' top level, inside
/// double quotes, or in the word of a `${name-word}` read outside them
/// (whose unquoted text is an expansion's result, split and globbed).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctx {
    Top,
    Dquote,
    Operand,
}

impl Ctx {
    /// The kind an expansion's result takes here.
    const fn result(self) -> Kind {
        match self {
            Self::Dquote => Kind::Quoted,
            Self::Top | Self::Operand => Kind::Split,
        }
    }

    /// The kind the words' own unquoted text takes here.
    const fn text(self) -> Kind {
        match self {
            Self::Top => Kind::Literal,
            Self::Dquote => Kind::Quoted,
            Self::Operand => Kind::Split,
        }
    }
}

/// The expander: the words, the flags, and the system.
struct Expander<'s> {
    flags: i32,
    sys: &'s mut dyn Sys,
}

/// `s[i]`, or 0 past the end.
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

fn is_name_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_name(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// The end of a `$(`'s command, `s[i]` the first byte after `$(`: the index
/// of its `)`, counted as glibc counts it -- parentheses outside quotes,
/// single and double quotes, nothing else.
fn command_end(s: &[u8], mut i: usize) -> R<usize> {
    let mut depth = 1usize;
    let mut quote = 0u8;
    while i < s.len() {
        let c = s[i];
        match (quote, c) {
            (0, b'\'' | b'"') => quote = c,
            (q, _) if q == c && q != 0 => quote = 0,
            (0, b'(') => depth += 1,
            (0, b')') => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    Err(Error::Syntax)
}

/// For `$((`, `s[i]` the first byte after it: the index of the `))` ending an
/// arithmetic expansion -- or `None`, the text being a command substitution
/// of a subshell, when the first `)` not matched within it is not followed
/// by another (glibc's test).
fn arith_end(s: &[u8], mut i: usize) -> Option<usize> {
    let mut depth = 0usize;
    while i < s.len() {
        match s[i] {
            b'(' => depth += 1,
            b')' if depth == 0 => return (at(s, i + 1) == b')').then_some(i),
            b')' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    None
}

/// The end of `$[`'s expression, `s[i]` its first byte: the matching `]`.
fn bracket_end(s: &[u8], mut i: usize) -> R<usize> {
    let mut depth = 1usize;
    while i < s.len() {
        match s[i] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    Err(Error::Syntax)
}

/// The end of a backquoted command, `s[i]` the first byte after the opening
/// backquote: the command's text -- `\$`, `` \` ``, `\\` as the character,
/// a `\` and newline gone, other backslashes kept, as POSIX has it -- and the
/// index after the closing backquote.
fn backquoted(s: &[u8], mut i: usize) -> R<(Bytes, usize)> {
    let mut cmd = Bytes::new();
    while i < s.len() {
        match s[i] {
            b'`' => return Ok((cmd, i + 1)),
            b'\\' => {
                let n = at(s, i + 1);
                match n {
                    0 => return Err(Error::Syntax),
                    b'$' | b'`' | b'\\' => cmd.push(n)?,
                    b'\n' => {}
                    _ => {
                        cmd.push(b'\\')?;
                        cmd.push(n)?;
                    }
                }
                i += 2;
            }
            c => {
                cmd.push(c)?;
                i += 1;
            }
        }
    }
    Err(Error::Syntax)
}

/// The end of a `${...}` operand's word, `s[i]` its first byte: the index of
/// the `}` closing the expansion -- quotes and nested expansions stepped
/// over, nothing expanded.
fn operand_end(s: &[u8], mut i: usize) -> R<usize> {
    let mut depth = 0usize;
    while i < s.len() {
        match s[i] {
            b'\\' => i += 1,
            b'\'' => {
                i += 1;
                while i < s.len() && s[i] != b'\'' {
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                while i < s.len() && s[i] != b'"' {
                    if s[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'`' => {
                i += 1;
                while i < s.len() && s[i] != b'`' {
                    if s[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'$' if at(s, i + 1) == b'(' => {
                i = command_end(s, i + 2)?;
            }
            b'$' if at(s, i + 1) == b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                if depth == 0 {
                    return Ok(i);
                }
                depth -= 1;
            }
            _ => {}
        }
        i += 1;
    }
    Err(Error::Syntax)
}

/// A parameter's name in `${...}`, `s[i]` its first byte: a name, digits
/// (a positional parameter), or one special character.
fn param_name(s: &[u8], i: usize) -> Option<usize> {
    let c = at(s, i);
    if is_name_start(c) {
        let mut j = i + 1;
        while is_name(at(s, j)) {
            j += 1;
        }
        return Some(j);
    }
    if c.is_ascii_digit() {
        let mut j = i + 1;
        while at(s, j).is_ascii_digit() {
            j += 1;
        }
        return Some(j);
    }
    matches!(c, b'#' | b'@' | b'*' | b'?' | b'-' | b'!' | b'$').then_some(i + 1)
}

/// What a parameter holds.
enum Value {
    Unset,
    Set(Bytes),
    /// `$@` or `$*`: the positional parameters, each apart.
    Params(List<Bytes>),
}

impl Expander<'_> {
    /// The parameter `name`'s value.
    fn param(&mut self, name: &[u8]) -> R<Value> {
        let c = at(name, 0);
        if c.is_ascii_digit() {
            // A positional parameter; one past the arguments is unset.
            let n = name.iter().try_fold(0usize, |n, &d| {
                n.checked_mul(10)?.checked_add(usize::from(d - b'0'))
            });
            return Ok(match n {
                Some(n) => self.sys.arg(n)?.map_or(Value::Unset, Value::Set),
                None => Value::Unset,
            });
        }
        if name.len() == 1 {
            let argc = self.sys.argc();
            match c {
                b'#' => {
                    return Ok(Value::Set(decimal(
                        i64::try_from(argc.saturating_sub(1)).unwrap_or(0),
                    )?));
                }
                b'@' | b'*' => {
                    let mut all = List::new();
                    for k in 1..argc {
                        all.push(self.sys.arg(k)?.unwrap_or_else(Bytes::new))?;
                    }
                    return Ok(Value::Params(all));
                }
                b'$' => return Ok(Value::Set(decimal(i64::from(self.sys.pid()))?)),
                b'?' => return Ok(Value::Set(Bytes::of(b"0")?)),
                b'-' => return Ok(Value::Set(Bytes::new())),
                b'!' => return Ok(Value::Unset),
                _ => {}
            }
        }
        Ok(self.sys.var(name)?.map_or(Value::Unset, Value::Set))
    }

    /// A value into the sink, as an expansion's result in `ctx`: `$@` and
    /// `$*` as POSIX has them -- each parameter its own field, or (`"$*"`)
    /// all joined by `IFS`'s first character.
    fn put_value(&mut self, v: &Value, star: bool, ctx: Ctx, sink: &mut dyn Sink) -> R<()> {
        match v {
            Value::Unset => Ok(()),
            Value::Set(b) => {
                if ctx == Ctx::Dquote {
                    sink.live()?;
                }
                for &c in b.as_slice() {
                    sink.put(c, ctx.result())?;
                }
                Ok(())
            }
            Value::Params(all) => {
                let joined = star && ctx == Ctx::Dquote;
                let sep = if joined {
                    match self.sys.var(b"IFS")? {
                        Some(ifs) => ifs.as_slice().first().copied(),
                        None => Some(b' '),
                    }
                } else {
                    None
                };
                for (k, p) in all.as_slice().iter().enumerate() {
                    if k > 0 {
                        if joined {
                            if let Some(c) = sep {
                                sink.put(c, Kind::Quoted)?;
                            }
                        } else {
                            sink.break_field()?;
                        }
                    }
                    if ctx == Ctx::Dquote {
                        sink.live()?;
                    }
                    for &c in p.as_slice() {
                        sink.put(c, ctx.result())?;
                    }
                }
                Ok(())
            }
        }
    }

    /// The text `s[i..end]` of an operand or an arithmetic expression,
    /// expanded into `sink` in `ctx`.
    fn expand_text(&mut self, s: &[u8], ctx: Ctx, sink: &mut dyn Sink) -> R<()> {
        let mut i = 0;
        let mut start = true;
        while i < s.len() {
            i = self.step(s, i, ctx, sink, &mut start)?;
        }
        Ok(())
    }

    /// One step of the words' text at `s[i]` in `ctx`, into `sink`: the
    /// index after what it took. `start` is whether this is a word's first
    /// byte, where a tilde is expanded.
    #[allow(clippy::too_many_lines)] // one arm a construct, as the grammar has them
    fn step(
        &mut self,
        s: &[u8],
        i: usize,
        ctx: Ctx,
        sink: &mut dyn Sink,
        start: &mut bool,
    ) -> R<usize> {
        let first = core::mem::replace(start, false);
        let c = s[i];
        match c {
            b'\\' => {
                let n = at(s, i + 1);
                if i + 1 >= s.len() {
                    return Err(Error::Syntax);
                }
                if ctx == Ctx::Dquote {
                    // In double quotes only `$ ` " \` and newline are escaped.
                    match n {
                        b'\n' => {}
                        b'$' | b'`' | b'"' | b'\\' => sink.put(n, Kind::Quoted)?,
                        _ => {
                            sink.put(b'\\', Kind::Quoted)?;
                            sink.put(n, Kind::Quoted)?;
                        }
                    }
                    return Ok(i + 2);
                }
                if n != b'\n' {
                    sink.put(n, Kind::Quoted)?;
                }
                Ok(i + 2)
            }
            b'\'' if ctx != Ctx::Dquote => {
                let mut j = i + 1;
                while j < s.len() && s[j] != b'\'' {
                    j += 1;
                }
                if j >= s.len() {
                    return Err(Error::Syntax);
                }
                sink.live()?;
                for &q in &s[i + 1..j] {
                    sink.put(q, Kind::Quoted)?;
                }
                Ok(j + 1)
            }
            b'"' if ctx != Ctx::Dquote => {
                // `"$@"` with no positional parameters is no field at all, as
                // POSIX has it: the quotes around it alone make none.
                for only in [&b"$@\""[..], b"${@}\""] {
                    if s[i + 1..].starts_with(only) && self.sys.argc() <= 1 {
                        return Ok(i + 1 + only.len());
                    }
                }
                sink.live()?;
                let mut j = i + 1;
                loop {
                    if j >= s.len() {
                        return Err(Error::Syntax);
                    }
                    if s[j] == b'"' {
                        return Ok(j + 1);
                    }
                    let mut inner = false;
                    j = self.step(s, j, Ctx::Dquote, sink, &mut inner)?;
                }
            }
            b'$' => self.dollar(s, i, ctx, sink),
            b'`' => {
                // Refused before it is read, as glibc refuses it.
                if self.flags & WRDE_NOCMD != 0 {
                    return Err(Error::CmdSub);
                }
                let (cmd, end) = backquoted(s, i + 1)?;
                self.command(cmd.as_slice(), ctx, sink)?;
                Ok(end)
            }
            b'~' if first && ctx != Ctx::Dquote => self.tilde(s, i, ctx, sink),
            _ => {
                sink.put(c, ctx.text())?;
                Ok(i + 1)
            }
        }
    }

    /// A tilde-prefix at `s[i]`: the characters to the first `/` (or the
    /// word's end) a login name, the tilde and the name replaced by that
    /// user's home -- `~` alone by `HOME` -- quoted, when none of them is
    /// quoted and the user is known; else the tilde as it stands.
    fn tilde(&mut self, s: &[u8], i: usize, ctx: Ctx, sink: &mut dyn Sink) -> R<usize> {
        let mut j = i + 1;
        while j < s.len() && !matches!(s[j], b'/' | b' ' | b'\t' | b'\n') {
            j += 1;
        }
        let name = &s[i + 1..j];
        let plain = name.iter().all(|&c| {
            !matches!(
                c,
                b'\''
                    | b'"'
                    | b'\\'
                    | b'$'
                    | b'`'
                    | b'|'
                    | b'&'
                    | b';'
                    | b'<'
                    | b'>'
                    | b'('
                    | b')'
                    | b'{'
                    | b'}'
            )
        });
        if plain {
            let home = if name.is_empty() {
                self.sys.home()?
            } else {
                self.sys.user_home(name)?
            };
            if let Some(h) = home {
                sink.live()?;
                for &c in h.as_slice() {
                    sink.put(c, Kind::Quoted)?;
                }
                return Ok(j);
            }
        }
        sink.put(b'~', ctx.text())?;
        Ok(i + 1)
    }

    /// A `$` at `s[i]`: an expansion, or the `$` itself when none follows.
    fn dollar(&mut self, s: &[u8], i: usize, ctx: Ctx, sink: &mut dyn Sink) -> R<usize> {
        match at(s, i + 1) {
            b'(' => {
                if at(s, i + 2) == b'(' {
                    if let Some(end) = arith_end(s, i + 3) {
                        self.arith(&s[i + 3..end], ctx, sink)?;
                        return Ok(end + 2);
                    }
                }
                if self.flags & WRDE_NOCMD != 0 {
                    return Err(Error::CmdSub);
                }
                let end = command_end(s, i + 2)?;
                self.command(&s[i + 2..end], ctx, sink)?;
                Ok(end + 1)
            }
            b'[' => {
                let end = bracket_end(s, i + 2)?;
                self.arith(&s[i + 2..end], ctx, sink)?;
                Ok(end + 1)
            }
            b'{' => self.braced(s, i, ctx, sink),
            c if is_name_start(c) => {
                let mut j = i + 2;
                while is_name(at(s, j)) {
                    j += 1;
                }
                let v = self.param(&s[i + 1..j])?;
                if matches!(v, Value::Unset) && self.flags & WRDE_UNDEF != 0 {
                    return Err(Error::BadVal);
                }
                self.put_value(&v, false, ctx, sink)?;
                Ok(j)
            }
            c if c.is_ascii_digit()
                || matches!(c, b'#' | b'@' | b'*' | b'?' | b'-' | b'!' | b'$') =>
            {
                let v = self.param(&s[i + 1..i + 2])?;
                if matches!(v, Value::Unset) && self.flags & WRDE_UNDEF != 0 {
                    return Err(Error::BadVal);
                }
                self.put_value(&v, c == b'*', ctx, sink)?;
                Ok(i + 2)
            }
            _ => {
                sink.put(b'$', ctx.text())?;
                Ok(i + 1)
            }
        }
    }

    /// `${...}` at `s[i]`: the index after its `}`.
    #[allow(clippy::too_many_lines)] // the operators, one arm each
    fn braced(&mut self, s: &[u8], i: usize, ctx: Ctx, sink: &mut dyn Sink) -> R<usize> {
        let j = i + 2;
        // `${#name}`: the length -- glibc's, what follows the name up to the
        // `}` read over. `${#}` alone is `$#`.
        if at(s, j) == b'#' && at(s, j + 1) != b'}' {
            let Some(k) = param_name(s, j + 1) else {
                return Err(Error::Syntax);
            };
            let v = self.param(&s[j + 1..k])?;
            if matches!(v, Value::Unset) && self.flags & WRDE_UNDEF != 0 {
                return Err(Error::BadVal);
            }
            let end = if at(s, k) == b'}' {
                k
            } else {
                operand_end(s, k)?
            };
            let len = match &v {
                Value::Set(b) => b.len,
                Value::Unset => 0,
                Value::Params(all) => all.len,
            };
            self.put_value(
                &Value::Set(decimal(i64::try_from(len).unwrap_or(i64::MAX))?),
                false,
                ctx,
                sink,
            )?;
            return Ok(end + 1);
        }
        let Some(k) = param_name(s, j) else {
            return Err(Error::Syntax);
        };
        let name = &s[j..k];
        let star = name == b"*";
        let (colon, op, word_at) = match (at(s, k), at(s, k + 1)) {
            (b'}', _) => (false, 0u8, k),
            (b':', o @ (b'-' | b'=' | b'?' | b'+')) => (true, o, k + 2),
            (o @ (b'-' | b'=' | b'?' | b'+'), _) => (false, o, k + 1),
            (b'#', b'#') => (false, b'H', k + 2),
            (b'#', _) => (false, b'h', k + 1),
            (b'%', b'%') => (false, b'P', k + 2),
            (b'%', _) => (false, b'p', k + 1),
            _ => return Err(Error::Syntax),
        };
        let end = if op == 0 { k } else { operand_end(s, word_at)? };
        let word = &s[word_at..end];
        let v = self.param(name)?;
        let null = match &v {
            Value::Unset => true,
            Value::Set(b) => b.len == 0,
            Value::Params(all) => all.len == 0,
        };
        let unset = matches!(v, Value::Unset);
        // The operand's word is expanded in the expansion's own context:
        // quoted when the expansion is.
        let word_ctx = if ctx == Ctx::Dquote {
            Ctx::Dquote
        } else {
            Ctx::Operand
        };
        match op {
            0 => {
                if unset && self.flags & WRDE_UNDEF != 0 {
                    return Err(Error::BadVal);
                }
                self.put_value(&v, star, ctx, sink)?;
            }
            b'-' => {
                if unset || (colon && null) {
                    self.expand_text(word, word_ctx, sink)?;
                } else {
                    self.put_value(&v, star, ctx, sink)?;
                }
            }
            b'+' => {
                if !(unset || (colon && null)) {
                    self.expand_text(word, word_ctx, sink)?;
                }
            }
            b'=' => {
                if unset || (colon && null) {
                    // Only a variable can be assigned.
                    if !is_name_start(at(name, 0)) {
                        return Err(Error::Syntax);
                    }
                    let mut value = Flat(Bytes::new());
                    self.expand_text(word, word_ctx, &mut value)?;
                    self.sys.set_var(name, value.0.as_slice())?;
                    self.put_value(&Value::Set(value.0), false, ctx, sink)?;
                } else {
                    self.put_value(&v, star, ctx, sink)?;
                }
            }
            b'?' => {
                // glibc's: an unset or null parameter is no error, and the
                // word is not expanded.
                if !(unset || (colon && null)) {
                    self.put_value(&v, star, ctx, sink)?;
                }
            }
            _ => {
                if unset && self.flags & WRDE_UNDEF != 0 {
                    return Err(Error::BadVal);
                }
                let mut pat = Pattern(Bytes::new());
                self.expand_text(word, word_ctx, &mut pat)?;
                let value = match &v {
                    Value::Set(b) => b.as_slice(),
                    _ => &[],
                };
                let kept = strip(value, pat.0.as_slice(), op)?;
                let kept = Bytes::of(kept)?;
                self.put_value(&Value::Set(kept), false, ctx, sink)?;
            }
        }
        Ok(end + 1)
    }

    /// A command substitution of `cmd`, its output into the sink as an
    /// expansion's result in `ctx`.
    fn command(&mut self, cmd: &[u8], ctx: Ctx, sink: &mut dyn Sink) -> R<()> {
        if self.flags & WRDE_NOCMD != 0 {
            return Err(Error::CmdSub);
        }
        if ctx == Ctx::Dquote {
            sink.live()?;
        }
        // glibc runs nothing for an empty command.
        if cmd.is_empty() {
            return Ok(());
        }
        let show = self.flags & WRDE_SHOWERR != 0;
        let (out, status) = self.sys.shell(cmd, false, show)?;
        if out.len == 0 && status != 0 {
            let (_, check) = self.sys.shell(cmd, true, show)?;
            if check != 0 {
                return Err(Error::Syntax);
            }
        }
        let mut text = out.as_slice();
        while let [rest @ .., b'\n'] = text {
            text = rest;
        }
        for &c in text {
            sink.put(c, ctx.result())?;
        }
        Ok(())
    }

    /// An arithmetic expansion of `expr`, its value into the sink.
    fn arith(&mut self, expr: &[u8], ctx: Ctx, sink: &mut dyn Sink) -> R<()> {
        let mut text = Flat(Bytes::new());
        self.expand_text(expr, Ctx::Dquote, &mut text)?;
        let mut a = Arith {
            s: text.0.as_slice(),
            i: 0,
            sys: &mut *self.sys,
        };
        let v = a.full()?;
        if ctx == Ctx::Dquote {
            sink.live()?;
        }
        for &c in decimal(v)?.as_slice() {
            sink.put(c, ctx.result())?;
        }
        Ok(())
    }
}

/// `v` in decimal.
fn decimal(v: i64) -> R<Bytes> {
    let mut b = [0u8; 21];
    let mut k = b.len();
    let mut m = v.unsigned_abs();
    loop {
        k -= 1;
        // A digit.
        #[allow(clippy::cast_possible_truncation)]
        let d = (m % 10) as u8;
        b[k] = b'0' + d;
        m /= 10;
        if m == 0 {
            break;
        }
    }
    if v < 0 {
        k -= 1;
        b[k] = b'-';
    }
    Bytes::of(&b[k..])
}

/// What `${name#pattern}` and its kin leave of `value`: `op` `h` `H` the
/// shortest and longest prefix matching removed, `p` `P` the shortest and
/// longest suffix.
fn strip<'v>(value: &'v [u8], pattern: &[u8], op: u8) -> R<&'v [u8]> {
    let m = |t: &[u8]| crate::fnmatch::matches(pattern, t, 0).map_err(|_| Error::NoSpace);
    let n = value.len();
    match op {
        b'h' => {
            for k in 0..=n {
                if m(&value[..k])? {
                    return Ok(&value[k..]);
                }
            }
        }
        b'H' => {
            for k in (0..=n).rev() {
                if m(&value[..k])? {
                    return Ok(&value[k..]);
                }
            }
        }
        b'p' => {
            for k in (0..=n).rev() {
                if m(&value[k..])? {
                    return Ok(&value[..k]);
                }
            }
        }
        _ => {
            for k in 0..=n {
                if m(&value[k..])? {
                    return Ok(&value[..k]);
                }
            }
        }
    }
    Ok(value)
}

// ---------------------------------------------------------------------------
// Arithmetic: C's integer expressions, in `long`
// ---------------------------------------------------------------------------

/// An arithmetic expression being read and evaluated: POSIX's (XCU 2.6.4),
/// C's operators but `++` and `--`, variables read and assigned through the
/// system, `+ - *` and `<<` wrapping, a division by zero (or of the least
/// `long` by -1) `WRDE_SYNTAX`.
struct Arith<'a, 's> {
    s: &'a [u8],
    i: usize,
    sys: &'s mut dyn Sys,
}

impl Arith<'_, '_> {
    fn ws(&mut self) {
        while matches!(at(self.s, self.i), b' ' | b'\t' | b'\n') {
            self.i += 1;
        }
    }

    /// The operator `op` next, consumed; not when a longer one begins with
    /// it (`<` of `<<`, `&` of `&&`, `=` of `==`).
    fn eat(&mut self, op: &[u8]) -> bool {
        self.ws();
        let rest = &self.s[self.i.min(self.s.len())..];
        if !rest.starts_with(op) {
            return false;
        }
        let next = at(rest, op.len());
        let longer = match op {
            b"<" | b">" => next == op[0] || next == b'=',
            b"&" | b"|" => next == op[0] || next == b'=',
            b"=" | b"!" => next == b'=',
            b"*" | b"/" | b"%" | b"+" | b"-" | b"^" | b"<<" | b">>" => next == b'=',
            _ => false,
        };
        if longer {
            return false;
        }
        self.i += op.len();
        true
    }

    /// The whole text: an empty one is 0, as glibc's.
    fn full(&mut self) -> R<i64> {
        self.ws();
        if self.i >= self.s.len() {
            return Ok(0);
        }
        let v = self.assign(true)?;
        self.ws();
        if self.i < self.s.len() {
            return Err(Error::Syntax);
        }
        Ok(v)
    }

    /// `name op= value`, or a conditional expression.
    fn assign(&mut self, eval: bool) -> R<i64> {
        self.ws();
        let save = self.i;
        if is_name_start(at(self.s, self.i)) {
            let start = self.i;
            while is_name(at(self.s, self.i)) {
                self.i += 1;
            }
            let name_end = self.i;
            self.ws();
            let ops: [&[u8]; 11] = [
                b"<<=", b">>=", b"*=", b"/=", b"%=", b"+=", b"-=", b"&=", b"^=", b"|=", b"=",
            ];
            for op in ops {
                let rest = &self.s[self.i..];
                if rest.starts_with(op) && !(op == b"=" && at(rest, 1) == b'=') {
                    self.i += op.len();
                    let rhs = self.assign(eval)?;
                    if !eval {
                        return Ok(0);
                    }
                    let name = &self.s[start..name_end];
                    let v = if op == b"=" {
                        rhs
                    } else {
                        let old = self.variable(name)?;
                        binary(&op[..op.len() - 1], old, rhs)?
                    };
                    let text = decimal(v)?;
                    let name = Bytes::of(name)?;
                    self.sys.set_var(name.as_slice(), text.as_slice())?;
                    return Ok(v);
                }
            }
        }
        self.i = save;
        self.cond(eval)
    }

    fn cond(&mut self, eval: bool) -> R<i64> {
        let c = self.lor(eval)?;
        if !self.eat(b"?") {
            return Ok(c);
        }
        let a = self.assign(eval && c != 0)?;
        if !self.eat(b":") {
            return Err(Error::Syntax);
        }
        let b = self.cond(eval && c == 0)?;
        Ok(if c != 0 { a } else { b })
    }

    fn lor(&mut self, eval: bool) -> R<i64> {
        let mut v = self.land(eval)?;
        while self.eat(b"||") {
            let r = self.land(eval && v == 0)?;
            v = i64::from(v != 0 || r != 0);
        }
        Ok(v)
    }

    fn land(&mut self, eval: bool) -> R<i64> {
        let mut v = self.level(0, eval)?;
        while self.eat(b"&&") {
            let r = self.level(0, eval && v != 0)?;
            v = i64::from(v != 0 && r != 0);
        }
        Ok(v)
    }

    /// The binary operators from `|` to `*`, by precedence level.
    fn level(&mut self, n: usize, eval: bool) -> R<i64> {
        const LEVELS: [&[&[u8]]; 8] = [
            &[b"|"],
            &[b"^"],
            &[b"&"],
            &[b"==", b"!="],
            &[b"<=", b">=", b"<", b">"],
            &[b"<<", b">>"],
            &[b"+", b"-"],
            &[b"*", b"/", b"%"],
        ];
        if n == LEVELS.len() {
            return self.unary(eval);
        }
        let mut v = self.level(n + 1, eval)?;
        'more: loop {
            for &op in LEVELS[n] {
                if self.eat(op) {
                    let r = self.level(n + 1, eval)?;
                    v = if eval { binary(op, v, r)? } else { 0 };
                    continue 'more;
                }
            }
            return Ok(v);
        }
    }

    fn unary(&mut self, eval: bool) -> R<i64> {
        self.ws();
        match at(self.s, self.i) {
            b'+' | b'-' if at(self.s, self.i + 1) == at(self.s, self.i) => Err(Error::Syntax),
            b'+' => {
                self.i += 1;
                self.unary(eval)
            }
            b'-' => {
                self.i += 1;
                // The least `long` has no positive: read it whole.
                self.ws();
                if at(self.s, self.i).is_ascii_digit() {
                    let v = self.number(true)?;
                    return Ok(v);
                }
                Ok(self.unary(eval)?.wrapping_neg())
            }
            b'~' => {
                self.i += 1;
                Ok(!self.unary(eval)?)
            }
            b'!' if at(self.s, self.i + 1) != b'=' => {
                self.i += 1;
                Ok(i64::from(self.unary(eval)? == 0))
            }
            _ => self.primary(eval),
        }
    }

    fn primary(&mut self, eval: bool) -> R<i64> {
        self.ws();
        let c = at(self.s, self.i);
        if c == b'(' {
            self.i += 1;
            let v = self.assign(eval)?;
            if !self.eat(b")") {
                return Err(Error::Syntax);
            }
            return Ok(v);
        }
        if c.is_ascii_digit() {
            return self.number(false);
        }
        if is_name_start(c) {
            let start = self.i;
            while is_name(at(self.s, self.i)) {
                self.i += 1;
            }
            if !eval {
                return Ok(0);
            }
            let name = Bytes::of(&self.s[start..self.i])?;
            return self.variable(name.as_slice());
        }
        Err(Error::Syntax)
    }

    /// A constant -- decimal, octal after `0`, hexadecimal after `0x` --
    /// negated when `negative` (so that the least `long` can be written);
    /// one that is no C constant, or does not fit, is `WRDE_SYNTAX`.
    fn number(&mut self, negative: bool) -> R<i64> {
        let start = self.i;
        while is_name(at(self.s, self.i)) {
            self.i += 1;
        }
        let v = constant(&self.s[start..self.i], negative).ok_or(Error::Syntax)?;
        Ok(v)
    }

    /// A variable's value: 0 when unset or empty, else the constant (with
    /// an optional sign) it holds.
    fn variable(&mut self, name: &[u8]) -> R<i64> {
        let Some(v) = self.sys.var(name)? else {
            return Ok(0);
        };
        let t = trim(v.as_slice());
        if t.is_empty() {
            return Ok(0);
        }
        let (neg, digits) = match t {
            [b'-', rest @ ..] => (true, rest),
            [b'+', rest @ ..] => (false, rest),
            _ => (false, t),
        };
        constant(digits, neg).ok_or(Error::Syntax)
    }
}

/// `t` without the white space around it.
fn trim(t: &[u8]) -> &[u8] {
    let is_ws = |c: &u8| matches!(c, b' ' | b'\t' | b'\n');
    let start = t.iter().position(|c| !is_ws(c)).unwrap_or(t.len());
    let end = t.iter().rposition(|c| !is_ws(c)).map_or(start, |e| e + 1);
    &t[start..end.max(start)]
}

/// A C integer constant's value, negated when `negative`; `None` for one
/// that is not a constant or does not fit a `long`.
fn constant(t: &[u8], negative: bool) -> Option<i64> {
    let (digits, radix) = match t {
        [b'0', b'x' | b'X', rest @ ..] => (rest, 16),
        [b'0', rest @ ..] if !rest.is_empty() => (rest, 8),
        _ => (t, 10),
    };
    if digits.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    for &c in digits {
        let d = u64::from(char::from(c).to_digit(radix)?);
        v = v.checked_mul(u64::from(radix))?.checked_add(d)?;
    }
    if negative {
        if v == 1 << 63 {
            return Some(i64::MIN);
        }
        return i64::try_from(v).ok().map(i64::wrapping_neg);
    }
    i64::try_from(v).ok()
}

/// A binary operator's value: `+ - * <<` wrapping; `/` and `%` by zero, or
/// of the least `long` by -1, `WRDE_SYNTAX`.
fn binary(op: &[u8], a: i64, b: i64) -> R<i64> {
    Ok(match op {
        b"+" => a.wrapping_add(b),
        b"-" => a.wrapping_sub(b),
        b"*" => a.wrapping_mul(b),
        b"/" => a.checked_div(b).ok_or(Error::Syntax)?,
        b"%" => a.checked_rem(b).ok_or(Error::Syntax)?,
        // A shift count past the width is taken modulo 64, as x86 takes it.
        b"<<" => a.wrapping_shl((b & 63) as u32),
        b">>" => a.wrapping_shr((b & 63) as u32),
        b"<" => i64::from(a < b),
        b"<=" => i64::from(a <= b),
        b">" => i64::from(a > b),
        b">=" => i64::from(a >= b),
        b"==" => i64::from(a == b),
        b"!=" => i64::from(a != b),
        b"&" => a & b,
        b"^" => a ^ b,
        b"|" => a | b,
        _ => return Err(Error::Syntax),
    })
}

// ---------------------------------------------------------------------------
// wordexp / wordfree
// ---------------------------------------------------------------------------

/// The unquoted characters that are an error at the words' top level.
fn is_bad(c: u8) -> bool {
    matches!(
        c,
        b'\n' | b'|' | b'&' | b';' | b'<' | b'>' | b'(' | b')' | b'{' | b'}'
    )
}

/// The fields `words` expand to, globbed; on failure the error, and -- for
/// `WRDE_NOSPACE` -- the fields finished before it (their text, unglobbed).
fn expand(
    words: &[u8],
    flags: i32,
    sys: &mut dyn Sys,
) -> Result<List<Bytes>, (Error, List<Bytes>)> {
    let ifs = sys.var(b"IFS").map_err(|e| (e, List::new()))?;
    let mut fields = Fields::new(ifs.as_ref().map(List::as_slice));
    let parsed = {
        let mut ex = Expander {
            flags,
            sys: &mut *sys,
        };
        let (mut i, mut start) = (0usize, true);
        let mut r = Ok(());
        while i < words.len() {
            let c = words[i];
            if c == b' ' || c == b'\t' {
                if let Err(e) = fields.separate() {
                    r = Err(e);
                    break;
                }
                start = true;
                i += 1;
                continue;
            }
            if is_bad(c) {
                r = Err(Error::BadChar);
                break;
            }
            match ex.step(words, i, Ctx::Top, &mut fields, &mut start) {
                Ok(next) => i = next,
                Err(e) => {
                    r = Err(e);
                    break;
                }
            }
        }
        r.and_then(|()| fields.separate())
    };
    let finished = core::mem::replace(&mut fields.out, List::new());
    if let Err(e) = parsed {
        if e != Error::NoSpace {
            return Err((e, List::new()));
        }
        let mut partial = List::new();
        for f in consume(finished) {
            if partial.push(f.text).is_err() {
                break;
            }
        }
        return Err((Error::NoSpace, partial));
    }
    // Pathname expansion: a field with an unquoted glob character is the
    // names it matches, sorted -- or itself, when none match.
    let mut out = List::new();
    let mut pending = consume(finished);
    for f in pending.by_ref() {
        let r = match f.pattern {
            Some(p) => {
                let before = out.len;
                sys.glob(p.as_slice(), &mut out).and_then(|()| {
                    if out.len == before {
                        out.push(f.text)
                    } else {
                        Ok(())
                    }
                })
            }
            None => out.push(f.text),
        };
        if r.is_err() {
            return Err((Error::NoSpace, out));
        }
    }
    Ok(out)
}

/// A list's elements, taken out one by one; those not taken are dropped
/// with the iterator.
fn consume<T>(list: List<T>) -> Drain<T> {
    let (ptr, len) = list.into_raw();
    Drain { ptr, len, at: 0 }
}

/// What [`consume`] answers.
struct Drain<T> {
    ptr: *mut T,
    len: usize,
    at: usize,
}

impl<T> Iterator for Drain<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        if self.at >= self.len {
            return None;
        }
        // SAFETY: element `at`, initialised and not yet taken.
        let v = unsafe { self.ptr.add(self.at).read() };
        self.at += 1;
        Some(v)
    }
}

impl<T> Drop for Drain<T> {
    fn drop(&mut self) {
        while self.at < self.len {
            // SAFETY: element `at`, initialised and not taken: dropped once.
            unsafe { self.ptr.add(self.at).drop_in_place() };
            self.at += 1;
        }
        // SAFETY: the list's own block, or NULL.
        unsafe { crate::malloc::free(self.ptr.cast()) };
    }
}

/// `wordfree`'s body: the words from `we_offs` to the NULL freed, and the
/// array; `we_offs` kept.
fn free_words(we: &mut WordexpT) {
    if !we.we_wordv.is_null() {
        let mut i = we.we_offs;
        loop {
            // SAFETY: the array holds `we_offs` NULLs, the words, and a NULL,
            // as `wordexp` made it.
            let w = unsafe { *we.we_wordv.add(i) };
            if w.is_null() {
                break;
            }
            // SAFETY: each word is a block from `malloc`, freed once.
            unsafe { crate::malloc::free(w) };
            i += 1;
        }
        // SAFETY: the array is a block from `malloc`.
        unsafe { crate::malloc::free(we.we_wordv.cast()) };
    }
    we.we_wordv = core::ptr::null_mut();
    we.we_wordc = 0;
}

/// `wordexp`'s body, on the system `sys`: the answer, and `we` updated as
/// the flags say -- or, on failure (but `WRDE_NOSPACE`), as it was.
fn wordexp_with(words: &[u8], we: &mut WordexpT, flags: i32, sys: &mut dyn Sys) -> i32 {
    if flags & WRDE_REUSE != 0 {
        free_words(we);
    }
    let append = flags & WRDE_APPEND != 0 && !we.we_wordv.is_null();
    let (fields, err) = match expand(words, flags, sys) {
        Ok(f) => (f, None),
        Err((e, partial)) => {
            if e != Error::NoSpace {
                return e.code();
            }
            (partial, Some(e))
        }
    };
    // The offsets: an append keeps the list's own; otherwise `we_offs` NULLs
    // with `WRDE_DOOFFS`, none without.
    let offs = if append || flags & WRDE_DOOFFS != 0 {
        we.we_offs
    } else {
        0
    };
    let old: &[*mut u8] = if append {
        // SAFETY: an earlier call's array: `we_offs` NULLs, `we_wordc` words.
        unsafe { core::slice::from_raw_parts(we.we_wordv.add(we.we_offs), we.we_wordc) }
    } else {
        &[]
    };
    let old_len = old.len();
    let mut array: List<*mut u8> = List::new();
    let Some(total) = offs
        .checked_add(old.len())
        .and_then(|n| n.checked_add(fields.len))
        .and_then(|n| n.checked_add(1))
    else {
        return WRDE_NOSPACE;
    };
    if array.reserve(total).is_err() {
        return WRDE_NOSPACE;
    }
    let mut result = err;
    for _ in 0..offs {
        // Room reserved: cannot fail.
        let _ = array.push(core::ptr::null_mut());
    }
    for &w in old {
        let _ = array.push(w);
    }
    let mut added = 0usize;
    for f in consume(fields) {
        match f.into_c_string() {
            Ok(c) => {
                let _ = array.push(c);
                added += 1;
            }
            Err(e) => {
                result = Some(e);
                break;
            }
        }
    }
    let _ = array.push(core::ptr::null_mut());
    let (ptr, _) = array.into_raw();
    if append {
        // The old array's words are in the new one; only the block goes.
        // SAFETY: the earlier call's array, from `malloc`.
        unsafe { crate::malloc::free(we.we_wordv.cast()) };
    }
    we.we_wordv = ptr;
    we.we_wordc = old_len + added;
    we.we_offs = offs;
    result.map_or(0, Error::code)
}

/// Perform word expansion on `words` into `*pwordexp`, as the flags say: 0,
/// or a `WRDE_*` error. See the module documentation for what is expanded
/// and how. A NULL `words` or `pwordexp` answers `WRDE_BADCHAR`, where
/// glibc's would fault.
///
/// # Safety
///
/// `words` is NULL or a NUL-terminated string; `pwordexp` is NULL or a
/// `wordexp_t` -- holding an earlier call's words for `WRDE_APPEND` and
/// `WRDE_REUSE`, and its `we_offs` for `WRDE_DOOFFS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wordexp(words: *const u8, pwordexp: *mut WordexpT, flags: i32) -> i32 {
    if words.is_null() || pwordexp.is_null() {
        return WRDE_BADCHAR;
    }
    // SAFETY: the caller's contract.
    let (text, we) = unsafe {
        (
            core::slice::from_raw_parts(words, crate::string::strlen(words)),
            &mut *pwordexp,
        )
    };
    wordexp_with(text, we, flags, &mut Process)
}

/// Free the words of a `wordexp` call, and their array; `we_wordv` becomes
/// NULL and `we_wordc` 0. NULL, or a structure with no array, is left alone.
///
/// # Safety
///
/// `pwordexp` is NULL or a `wordexp_t` a successful `wordexp` filled.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wordfree(pwordexp: *mut WordexpT) {
    if pwordexp.is_null() {
        return;
    }
    // SAFETY: the caller's contract.
    free_words(unsafe { &mut *pwordexp });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// glibc 2.39's answers, one line a case (`wordexp_harness.py`).
    const ORACLE: &str = include_str!("wordexp_oracle.txt");

    /// Where this library answers otherwise than glibc, following POSIX
    /// (see the module documentation): each case, and this library's answer.
    const DEVIATIONS: &[(&str, &str)] = &[
        // Arithmetic has C's operators, bare names, and C's constants.
        (
            r"0 \x20\x0a\x09 - /dummy/home - $((7%3))",
            "0 1 0 1 | - | -",
        ),
        (
            r"0 \x20\x0a\x09 4 /dummy/home - $((var+1))",
            "0 1 0 5 | 4 | -",
        ),
        (
            r"0 \x20\x0a\x09 - /dummy/home - $((1<<2))",
            "0 1 0 4 | - | -",
        ),
        (r"0 \x20\x0a\x09 - /dummy/home - $((08))", "5 kept | - | -"),
        (r"0 \x20\x0a\x09 - /dummy/home - $((0x))", "5 kept | - | -"),
        // A fresh shell's special parameters.
        (r"0 \x20\x0a\x09 - /dummy/home - $?", "0 1 0 0 | - | -"),
        (r"0 \x20\x0a\x09 - /dummy/home - $-", "0 0 0 | - | -"),
        (r"0 \x20\x0a\x09 - /dummy/home - $!", "0 0 0 | - | -"),
        (r"0 \x20\x0a\x09 - /dummy/home - ${#}", "0 1 0 0 | - | -"),
        (r#"0 \x20\x0a\x09 - /dummy/home - "$@""#, "0 0 0 | - | -"),
        // Pathname expansion of expansion results.
        (
            r"0 \x20\x0a\x09 t* /dummy/home - $var",
            "0 2 0 three two | t* | -",
        ),
        (
            r"0 \x20\x0a\x09 ??? /dummy/home - $var",
            "0 2 0 one two | ??? | -",
        ),
        // A tilde only at a word's start, and quote removal after one.
        (r"0 \x20\x0a\x09 - /dummy/home - x=~", "0 1 0 x=~ | - | -"),
        (
            r#"0 \x20\x0a\x09 - /dummy/home - ~"root""#,
            "0 1 0 ~root | - | -",
        ),
        // `${var:-x}` tests for unset: no WRDE_UNDEF error, as no shell's
        // `set -u` makes one of it.
        (
            r"32 \x20\x0a\x09 - /dummy/home - ${var:-x}",
            "0 1 0 x | - | -",
        ),
    ];

    /// A command the oracle's shell was asked: a run's output and status,
    /// or a syntax check's status.
    #[derive(Clone)]
    struct Call {
        check: bool,
        text: Vec<u8>,
        out: Vec<u8>,
        status: i32,
    }

    /// The oracle's sandbox, for the tests: an environment, root's and
    /// wexuser's homes, a directory holding `one`, `two` and `three`, one
    /// argument, and a shell that answers what the oracle's did.
    struct Fake {
        env: Vec<(Vec<u8>, Vec<u8>)>,
        calls: Vec<Call>,
        next: usize,
        asked: Vec<(bool, Vec<u8>)>,
    }

    impl Sys for Fake {
        fn var(&mut self, name: &[u8]) -> R<Option<Bytes>> {
            match self.env.iter().find(|(n, _)| n == name) {
                Some((_, v)) => Ok(Some(Bytes::of(v)?)),
                None => Ok(None),
            }
        }

        fn set_var(&mut self, name: &[u8], value: &[u8]) -> R<()> {
            match self.env.iter_mut().find(|(n, _)| n == name) {
                Some(slot) => slot.1 = value.to_vec(),
                None => self.env.push((name.to_vec(), value.to_vec())),
            }
            Ok(())
        }

        fn home(&mut self) -> R<Option<Bytes>> {
            match self.var(b"HOME")? {
                Some(h) => Ok(Some(h)),
                None => Ok(Some(Bytes::of(b"/root")?)),
            }
        }

        fn user_home(&mut self, user: &[u8]) -> R<Option<Bytes>> {
            Ok(match user {
                b"root" => Some(Bytes::of(b"/root")?),
                b"wexuser" => Some(Bytes::of(b"/home/wex")?),
                _ => None,
            })
        }

        fn shell(&mut self, cmd: &[u8], check_only: bool, _show_err: bool) -> R<(Bytes, i32)> {
            self.asked.push((check_only, cmd.to_vec()));
            if let Some(c) = self.calls.get(self.next) {
                if c.check == check_only && c.text == cmd {
                    self.next += 1;
                    return Ok((Bytes::of(&c.out)?, c.status));
                }
            }
            Ok((Bytes::new(), 127))
        }

        fn glob(&mut self, pattern: &[u8], out: &mut List<Bytes>) -> R<()> {
            for name in [&b"one"[..], b"three", b"two"] {
                if crate::fnmatch::matches(pattern, name, crate::fnmatch::FNM_PERIOD)
                    .map_err(|_| Error::NoSpace)?
                {
                    out.push(Bytes::of(name)?)?;
                }
            }
            Ok(())
        }

        fn argc(&mut self) -> usize {
            1
        }

        fn arg(&mut self, i: usize) -> R<Option<Bytes>> {
            Ok(if i == 0 {
                Some(Bytes::of(b"../wordexp")?)
            } else {
                None
            })
        }

        fn pid(&mut self) -> i32 {
            1
        }
    }

    /// A text as the harness writes it: `\xHH` for a byte outside `!`..`~`
    /// and for `\`, `\x` for none.
    fn token(b: &[u8]) -> String {
        use core::fmt::Write;
        if b.is_empty() {
            return "\\x".into();
        }
        b.iter().fold(String::new(), |mut s, &c| {
            if (0x21..=0x7e).contains(&c) && c != b'\\' {
                s.push(c as char);
            } else {
                // Writing into a String cannot fail.
                let _ = write!(s, "\\x{c:02x}");
            }
            s
        })
    }

    fn untoken(t: &str) -> Vec<u8> {
        if t == "\\x" {
            return Vec::new();
        }
        let b = t.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && b.get(i + 1) == Some(&b'x') {
                out.push(u8::from_str_radix(&t[i + 2..i + 4], 16).unwrap());
                i += 4;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }

    fn opt(t: &str) -> Option<Vec<u8>> {
        (t != "-").then(|| untoken(t))
    }

    /// The oracle's commands field: `-`, or `c <text> <output> <status>` and
    /// `n <text> <status>` space apart.
    fn calls(field: &str) -> Vec<Call> {
        if field == "-" {
            return Vec::new();
        }
        let t: Vec<&str> = field.split(' ').collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < t.len() {
            if t[i] == "c" {
                out.push(Call {
                    check: false,
                    text: untoken(t[i + 1]),
                    out: untoken(t[i + 2]),
                    status: t[i + 3].parse().unwrap(),
                });
                i += 4;
            } else {
                out.push(Call {
                    check: true,
                    text: untoken(t[i + 1]),
                    out: Vec::new(),
                    status: t[i + 2].parse().unwrap(),
                });
                i += 3;
            }
        }
        out
    }

    /// What the harness prints after ` = ` for `case`, the shell answering
    /// as `expected` says it did.
    fn replay(case: &str, expected: &[Call], expected_field: &str) -> String {
        let f: Vec<&str> = case.splitn(6, ' ').collect();
        let flags: i32 = f[0].parse().unwrap();
        let mut env = Vec::new();
        for (name, v) in [(&b"IFS"[..], f[1]), (b"var", f[2]), (b"HOME", f[3])] {
            if let Some(v) = opt(v) {
                env.push((name.to_vec(), v));
            }
        }
        let pre = opt(f[4]);
        let words = untoken(f[5]);
        let mut fake = Fake {
            env,
            calls: expected.to_vec(),
            next: 0,
            asked: Vec::new(),
        };
        let mut dummy: *mut u8 = core::ptr::null_mut();
        let mut we = WordexpT {
            we_wordc: 99,
            we_wordv: &raw mut dummy,
            we_offs: 3,
        };
        if let Some(pre) = &pre {
            assert_eq!(
                wordexp_with(pre, &mut we, flags & !(WRDE_APPEND | WRDE_REUSE), &mut fake),
                0,
                "setup {case}"
            );
        }
        let saved = (we.we_wordc, we.we_wordv, we.we_offs);
        let rc = wordexp_with(&words, &mut we, flags, &mut fake);
        let mut out = format!("{rc}");
        if rc == 0 || rc == WRDE_NOSPACE {
            out.push_str(&format!(" {} {}", we.we_wordc, we.we_offs));
            for i in 0..we.we_offs {
                // SAFETY: the array wordexp made.
                if !unsafe { *we.we_wordv.add(i) }.is_null() {
                    out.push_str(" OFFS-NOT-NULL");
                }
            }
            for i in 0..we.we_wordc {
                // SAFETY: as above; each word a C string.
                let w =
                    unsafe { core::ffi::CStr::from_ptr((*we.we_wordv.add(we.we_offs + i)).cast()) };
                out.push(' ');
                out.push_str(&token(w.to_bytes()));
            }
            // SAFETY: as above.
            if !unsafe { *we.we_wordv.add(we.we_offs + we.we_wordc) }.is_null() {
                out.push_str(" UNTERMINATED");
            }
            free_words(&mut we);
        } else {
            let kept = (we.we_wordc, we.we_wordv, we.we_offs) == saved;
            out.push_str(if kept { " kept" } else { " changed" });
            if pre.is_some() {
                free_words(&mut we);
            }
        }
        let after = fake
            .env
            .iter()
            .find(|(n, _)| n == b"var")
            .map_or("-".into(), |(_, v)| token(v));
        // The commands: the oracle's field when this library asked the
        // shell exactly what glibc did, else what it asked.
        let same = fake.asked.len() == expected.len()
            && fake
                .asked
                .iter()
                .zip(expected)
                .all(|((check, text), c)| *check == c.check && *text == c.text);
        let commands = if same {
            expected_field.to_string()
        } else {
            fake.asked
                .iter()
                .map(|(check, text)| format!("{} {}", if *check { 'n' } else { 'c' }, token(text)))
                .collect::<Vec<_>>()
                .join(" ")
        };
        format!("{out} | {after} | {commands}")
    }

    /// Every case `wordexp_harness.py` ran against glibc, answered alike --
    /// but where this library follows POSIX instead ([`DEVIATIONS`]).
    #[test]
    fn every_case_is_glibcs_or_a_recorded_deviation() {
        let mut wrong = Vec::new();
        let mut n = 0;
        let mut deviated = 0;
        for line in ORACLE
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let (case, glibc) = line.split_once(" = ").unwrap();
            let field = glibc.rsplit_once(" | ").unwrap().1;
            let want = match DEVIATIONS.iter().find(|(c, _)| *c == case) {
                Some((_, ours)) => {
                    deviated += 1;
                    (*ours).to_string()
                }
                None => glibc.to_string(),
            };
            n += 1;
            let got = replay(case, &calls(field), field);
            if got != want {
                wrong.push(format!("{case}\n  want: {want}\n  ours: {got}"));
            }
        }
        assert!(n >= 320, "the oracle has {n} lines");
        assert_eq!(
            deviated,
            DEVIATIONS.len(),
            "a deviation names a case the oracle does not hold"
        );
        assert!(
            wrong.is_empty(),
            "{} of {n}:\n{}",
            wrong.len(),
            wrong
                .iter()
                .take(15)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// The expansion of `words` in a sandbox of its own, as `replay`'s.
    fn words_of(words: &str, var: Option<&str>) -> (i32, Vec<String>, Option<String>) {
        let mut env = Vec::new();
        if let Some(v) = var {
            env.push((b"var".to_vec(), v.as_bytes().to_vec()));
        }
        let mut fake = Fake {
            env,
            calls: Vec::new(),
            next: 0,
            asked: Vec::new(),
        };
        let mut we = WordexpT {
            we_wordc: 0,
            we_wordv: core::ptr::null_mut(),
            we_offs: 0,
        };
        let rc = wordexp_with(words.as_bytes(), &mut we, 0, &mut fake);
        let mut out = Vec::new();
        for i in 0..we.we_wordc {
            // SAFETY: the array wordexp made; each word a C string.
            let w = unsafe { core::ffi::CStr::from_ptr((*we.we_wordv.add(i)).cast()) };
            out.push(String::from_utf8_lossy(w.to_bytes()).into_owned());
        }
        free_words(&mut we);
        let after = fake
            .env
            .iter()
            .find(|(n, _)| n == b"var")
            .map(|(_, v)| String::from_utf8_lossy(v).into_owned());
        (rc, out, after)
    }

    #[test]
    fn arithmetic_is_c_s() {
        for (expr, v) in [
            ("1+2*3", "7"),
            ("(1+2)*3", "9"),
            ("7%3", "1"),
            ("-7%3", "-1"),
            ("1<<4", "16"),
            ("-16>>2", "-4"),
            ("3<4", "1"),
            ("4<=3", "0"),
            ("2==2", "1"),
            ("2!=2", "0"),
            ("6&3", "2"),
            ("6|3", "7"),
            ("6^3", "5"),
            ("~0", "-1"),
            ("!0", "1"),
            ("!5", "0"),
            ("0||2", "1"),
            ("0&&x", "0"),
            ("1?2:3", "2"),
            ("0?2:3", "3"),
            ("1+(2?3:4)*2", "7"),
            ("0x7fffffffffffffff", "9223372036854775807"),
            ("- 5", "-5"),
        ] {
            let (rc, w, _) = words_of(&format!("$(({expr}))"), None);
            assert_eq!((rc, w), (0, std::vec![v.to_string()]), "{expr}");
        }
        for bad in [
            "1/0",
            "1%0",
            "2+",
            "(1",
            "08",
            "0x",
            "1 2",
            "--1",
            "9223372036854775808",
        ] {
            assert_eq!(
                words_of(&format!("$(({bad}))"), None).0,
                WRDE_SYNTAX,
                "{bad}"
            );
        }
        // `$((1)))`: the expansion ends at the first `))`, and the `)` left
        // over is outside it.
        assert_eq!(words_of("$((1)))", None).0, WRDE_BADCHAR);
    }

    #[test]
    fn arithmetic_reads_and_assigns_variables() {
        assert_eq!(
            words_of("$((var*2))", Some(" 21 ")),
            (0, std::vec!["42".into()], Some(" 21 ".into()))
        );
        assert_eq!(
            words_of("$((var=5)) $var", None),
            (0, std::vec!["5".into(), "5".into()], Some("5".into()))
        );
        assert_eq!(
            words_of("$((var+=2))", Some("-3")),
            (0, std::vec!["-1".into()], Some("-1".into()))
        );
        assert_eq!(
            words_of("$((var<<=1))", Some("0x10")),
            (0, std::vec!["32".into()], Some("32".into()))
        );
        assert_eq!(words_of("$((var))", None), (0, std::vec!["0".into()], None));
        assert_eq!(words_of("$((var))", Some("abc")).0, WRDE_SYNTAX);
        // The branch not taken assigns nothing.
        assert_eq!(
            words_of("$((0 && (var=1)))", None),
            (0, std::vec!["0".into()], None)
        );
    }

    #[test]
    fn wordfree_leaves_an_empty_structure_alone() {
        // SAFETY: NULL is allowed.
        unsafe { wordfree(core::ptr::null_mut()) };
        let mut we = WordexpT {
            we_wordc: 0,
            we_wordv: core::ptr::null_mut(),
            we_offs: 2,
        };
        // SAFETY: a structure with no array.
        unsafe { wordfree(&raw mut we) };
        assert!(we.we_wordv.is_null());
        assert_eq!((we.we_wordc, we.we_offs), (0, 2));
    }

    #[test]
    fn null_arguments_are_refused() {
        let mut we = WordexpT {
            we_wordc: 0,
            we_wordv: core::ptr::null_mut(),
            we_offs: 0,
        };
        // SAFETY: NULL is refused before anything is read.
        assert_eq!(
            unsafe { wordexp(core::ptr::null(), &raw mut we, 0) },
            WRDE_BADCHAR
        );
        // SAFETY: as above.
        assert_eq!(
            unsafe { wordexp(c"x".as_ptr().cast(), core::ptr::null_mut(), 0) },
            WRDE_BADCHAR
        );
    }

    #[test]
    fn many_words_and_long_ones() {
        let long = "w".repeat(100_000);
        let (rc, w, _) = words_of(&long, None);
        assert_eq!((rc, w.len(), w[0].len()), (0, 1, 100_000));
        let many: Vec<String> = (0..5000).map(|i| format!("w{i}")).collect();
        let (rc, w, _) = words_of(&many.join(" "), None);
        assert_eq!(rc, 0);
        assert_eq!(w, many);
    }
}
