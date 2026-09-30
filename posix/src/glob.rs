//! POSIX `glob()` and `globfree()` (XSH `glob`, XCU 2.13.3 "Patterns Used
//! for Filename Expansion"), with glibc's GNU flags; and glibc's
//! `glob_pattern_p`.
//!
//! ## What it does
//!
//! Each `/`-separated component of the pattern is matched against the
//! entries of the directories the components before it named, with
//! [`crate::fnmatch`] -- a leading `.` only by a `.` in the pattern, unless
//! `GLOB_PERIOD` -- and a component with no wildcard in it is looked up
//! rather than read. The names are sorted (the C locale's order: bytes)
//! unless `GLOB_NOSORT`.
//!
//! - POSIX's flags: `GLOB_APPEND`, `GLOB_DOOFFS`, `GLOB_ERR` (and the
//!   `errfunc`, told of every directory that cannot be read -- a stop from
//!   either is `GLOB_ABORTED`), `GLOB_MARK`, `GLOB_NOCHECK` (the pattern
//!   itself, when nothing matches), `GLOB_NOESCAPE`, `GLOB_NOSORT`.
//! - glibc's: `GLOB_PERIOD`; `GLOB_BRACE`, `{a,b}` as the patterns `a` and
//!   `b`, nested and repeated; `GLOB_TILDE` and `GLOB_TILDE_CHECK`, `~` and
//!   `~user`; `GLOB_NOMAGIC`, the pattern itself if it has no wildcard;
//!   `GLOB_ONLYDIR`; `GLOB_ALTDIRFUNC`, the program's own `opendir`,
//!   `readdir`, `closedir`, `stat` and `lstat` through `glob_t`'s
//!   `gl_opendir` ...; and `GLOB_MAGCHAR` in `gl_flags`.
//!
//! A wildcard in a directory part never matches a leading `.`, even under
//! `GLOB_PERIOD` -- which is the last component's -- as in glibc, so that
//! `*/*` does not walk through `..`. A pattern ending in a slash names the
//! directories the pattern before it names, each with its slash.
//!
//! glibc 2.39's answers are the oracle for everything neither POSIX nor
//! glibc's manual pins down: `posix/tools/oracle/glob_harness.py` runs
//! glibc's `glob` over a directory tree of its own through
//! `GLOB_ALTDIRFUNC` (`glob_oracle.txt`, 1,568 probes), and the tests replay
//! each through the same callbacks. Two of glibc's answers are not followed
//! (design-decisions §1149; `glob_deviations.txt` lists the 23 probes):
//! glibc reads `*/` and `?/` -- one character before a trailing slash -- by
//! a path of its own, so that `GLOB_MARK` doubles their slash, `GLOB_PERIOD`
//! does not apply and `GLOB_MAGCHAR` is not set, unlike `**/` and `[!x]/`,
//! which match the same names; and its `GLOB_NOCHECK` answer for `??/` is
//! `??`, where POSIX's is the pattern.
//!
//! Until 2026-09-29 this read one directory -- `src/*/foo.rs` found nothing
//! -- kept at most 512 names and dropped the rest silently, knew four of
//! POSIX's seven flags and none of glibc's, never called `errfunc`, and
//! copied the directory part into a 4096-byte stack buffer, a longer one
//! cut short and left unterminated (known-issues.md ->
//! D-POSIX-GLOB-READ-ONE-DIRECTORY-AND-KEPT-512-NAMES). Names and lists are
//! `malloc`'s now, of any length.
//!
//! ## `glob_t`
//!
//! glibc's layout, which is musl's with its reserved words named: after the
//! three POSIX fields, `gl_flags` and the five `GLOB_ALTDIRFUNC` functions.
//! `posix/include/glob.h` declares it so, where musl's names the words
//! `__dummy1` and `__dummy2`.

use core::ffi::c_void;

use crate::dirent::Dirent;
use crate::stat::Stat;

// ---------------------------------------------------------------------------
// Constants: glibc's and musl's values
// ---------------------------------------------------------------------------

/// Return on read error (stop scanning).
pub const GLOB_ERR: i32 = 0x01;
/// Append a slash to each directory name.
pub const GLOB_MARK: i32 = 0x02;
/// Do not sort the names.
pub const GLOB_NOSORT: i32 = 0x04;
/// Reserve `gl_offs` NULL slots at the front of `gl_pathv`.
pub const GLOB_DOOFFS: i32 = 0x08;
/// Return the pattern itself if nothing matches.
pub const GLOB_NOCHECK: i32 = 0x10;
/// Append to the results of an earlier call.
pub const GLOB_APPEND: i32 = 0x20;
/// Backslash is an ordinary character.
pub const GLOB_NOESCAPE: i32 = 0x40;
/// A leading `.` may be matched by a wildcard (glibc).
pub const GLOB_PERIOD: i32 = 0x80;
/// Set in `gl_flags` when wildcards were matched (glibc).
pub const GLOB_MAGCHAR: i32 = 0x100;
/// Use `gl_opendir` and its kin (glibc).
pub const GLOB_ALTDIRFUNC: i32 = 0x200;
/// Expand `{a,b}` (glibc).
pub const GLOB_BRACE: i32 = 0x400;
/// The pattern itself, if it has no wildcards and matches nothing (glibc).
pub const GLOB_NOMAGIC: i32 = 0x800;
/// Expand `~` and `~user` (glibc).
pub const GLOB_TILDE: i32 = 0x1000;
/// Only directories are wanted (glibc).
pub const GLOB_ONLYDIR: i32 = 0x2000;
/// `GLOB_TILDE`, and no match for an unknown user (glibc).
pub const GLOB_TILDE_CHECK: i32 = 0x4000;

/// Every flag a caller may pass; `GLOB_MAGCHAR` is glob's to set.
const GLOB_FLAGS: i32 = GLOB_ERR
    | GLOB_MARK
    | GLOB_NOSORT
    | GLOB_DOOFFS
    | GLOB_NOCHECK
    | GLOB_APPEND
    | GLOB_NOESCAPE
    | GLOB_PERIOD
    | GLOB_ALTDIRFUNC
    | GLOB_BRACE
    | GLOB_NOMAGIC
    | GLOB_TILDE
    | GLOB_ONLYDIR
    | GLOB_TILDE_CHECK;

/// Out of memory.
pub const GLOB_NOSPACE: i32 = 1;
/// A read error, and `GLOB_ERR` or `errfunc` asked to stop.
pub const GLOB_ABORTED: i32 = 2;
/// Nothing matched.
pub const GLOB_NOMATCH: i32 = 3;
/// Not implemented (never returned here).
pub const GLOB_NOSYS: i32 = 4;

// ---------------------------------------------------------------------------
// glob_t
// ---------------------------------------------------------------------------

/// `glob_t`, as glibc lays it out -- musl's, with its reserved words named.
#[repr(C)]
pub struct GlobT {
    /// Number of matched pathnames.
    pub gl_pathc: usize,
    /// The pathnames, after `gl_offs` NULLs, and a NULL.
    pub gl_pathv: *mut *mut u8,
    /// Slots reserved at the front of `gl_pathv` (`GLOB_DOOFFS`).
    pub gl_offs: usize,
    /// The flags, and `GLOB_MAGCHAR` (glibc).
    pub gl_flags: i32,
    /// `GLOB_ALTDIRFUNC`'s `closedir`.
    pub gl_closedir: Option<unsafe extern "C" fn(*mut c_void)>,
    /// `GLOB_ALTDIRFUNC`'s `readdir`.
    pub gl_readdir: Option<unsafe extern "C" fn(*mut c_void) -> *mut Dirent>,
    /// `GLOB_ALTDIRFUNC`'s `opendir`.
    pub gl_opendir: Option<unsafe extern "C" fn(*const u8) -> *mut c_void>,
    /// `GLOB_ALTDIRFUNC`'s `lstat`.
    pub gl_lstat: Option<unsafe extern "C" fn(*const u8, *mut Stat) -> i32>,
    /// `GLOB_ALTDIRFUNC`'s `stat`.
    pub gl_stat: Option<unsafe extern "C" fn(*const u8, *mut Stat) -> i32>,
}

const _: () = {
    assert!(
        size_of::<GlobT>() == 72,
        "glibc's and musl's glob_t is 72 bytes"
    );
};

impl GlobT {
    /// An empty `glob_t`: no names, no callbacks.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            gl_pathc: 0,
            gl_pathv: core::ptr::null_mut(),
            gl_offs: 0,
            gl_flags: 0,
            gl_closedir: None,
            gl_readdir: None,
            gl_opendir: None,
            gl_lstat: None,
            gl_stat: None,
        }
    }
}

impl Default for GlobT {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Owned bytes and lists, from malloc
// ---------------------------------------------------------------------------

/// Memory could not be had: `GLOB_NOSPACE`.
struct NoSpace;

/// Why a glob stopped early.
enum Stop {
    NoSpace,
    Aborted,
}

impl From<NoSpace> for Stop {
    fn from(_: NoSpace) -> Self {
        Self::NoSpace
    }
}

impl From<crate::fnmatch::NoMemory> for Stop {
    fn from(_: crate::fnmatch::NoMemory) -> Self {
        Self::NoSpace
    }
}

/// A NUL-terminated byte string in a block from `malloc`.
struct Owned {
    ptr: *mut u8,
    len: usize,
}

impl Owned {
    fn from_parts(parts: &[&[u8]]) -> Result<Self, NoSpace> {
        let len = parts
            .iter()
            .try_fold(0usize, |n, p| n.checked_add(p.len()))
            .ok_or(NoSpace)?;
        let ptr = crate::malloc::malloc(len.checked_add(1).ok_or(NoSpace)?);
        if ptr.is_null() {
            return Err(NoSpace);
        }
        let mut at = 0usize;
        for p in parts {
            // SAFETY: `ptr` holds `len + 1` bytes and the parts sum to `len`.
            unsafe { core::ptr::copy_nonoverlapping(p.as_ptr(), ptr.add(at), p.len()) };
            at = at.saturating_add(p.len());
        }
        // SAFETY: as above; `at == len`.
        unsafe { ptr.add(at).write(0) };
        Ok(Self { ptr, len })
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: `len` bytes this owns.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// Hand the block over: its owner frees it now.
    fn take(self) -> *mut u8 {
        let p = self.ptr;
        core::mem::forget(self);
        p
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the block is this one's own, from `malloc`.
        unsafe { crate::malloc::free(self.ptr) };
    }
}

/// A name found, and whether it is known to be a directory.
struct Found {
    path: Owned,
    is_dir: bool,
}

/// A growable array from `malloc`.
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

    fn push(&mut self, v: T) -> Result<(), NoSpace> {
        if self.len == self.cap {
            let cap = self.cap.max(4).checked_mul(2).ok_or(NoSpace)?;
            let bytes = cap.checked_mul(size_of::<T>()).ok_or(NoSpace)?;
            // SAFETY: `ptr` is NULL or this list's own block.
            let p = unsafe { crate::malloc::realloc(self.ptr.cast(), bytes) }.cast::<T>();
            if p.is_null() {
                return Err(NoSpace);
            }
            self.ptr = p;
            self.cap = cap;
        }
        // SAFETY: `len < cap`: room for one more.
        unsafe { self.ptr.add(self.len).write(v) };
        self.len = self.len.saturating_add(1);
        Ok(())
    }

    fn as_slice(&self) -> &[T] {
        if self.ptr.is_null() {
            return &[];
        }
        // SAFETY: `len` initialised elements.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        if self.ptr.is_null() {
            return &mut [];
        }
        // SAFETY: as above, and `&mut self` makes it unique.
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    /// Move every element of `other` onto the end of this one, in order.
    fn append(&mut self, mut other: Self) -> Result<(), NoSpace> {
        let n = other.len;
        let mut moved = 0usize;
        let result = loop {
            if moved == n {
                break Ok(());
            }
            // SAFETY: element `moved`, initialised and not yet moved.
            let v = unsafe { other.ptr.add(moved).read() };
            // Consumed either way: pushed, or dropped by a push that failed.
            moved = moved.saturating_add(1);
            if let Err(e) = self.push(v) {
                break Err(e);
            }
        };
        // What was moved is this list's now; what was not is dropped with
        // `other`, which must see only those.
        if moved < n {
            // SAFETY: shift the unmoved tail to the front: initialised
            // elements within the block, possibly overlapping.
            unsafe { core::ptr::copy(other.ptr.add(moved), other.ptr, n.saturating_sub(moved)) };
        }
        other.len = n.saturating_sub(moved);
        result
    }
}

impl<T> Drop for List<T> {
    fn drop(&mut self) {
        for i in 0..self.len {
            // SAFETY: each initialised element, dropped once.
            unsafe { core::ptr::drop_in_place(self.ptr.add(i)) };
        }
        // SAFETY: NULL or this list's own block.
        unsafe { crate::malloc::free(self.ptr.cast()) };
    }
}

// ---------------------------------------------------------------------------
// Where directories are read
// ---------------------------------------------------------------------------

/// The program's own directory functions (`GLOB_ALTDIRFUNC`).
#[derive(Clone, Copy)]
struct Alt {
    opendir: unsafe extern "C" fn(*const u8) -> *mut c_void,
    readdir: unsafe extern "C" fn(*mut c_void) -> *mut Dirent,
    closedir: unsafe extern "C" fn(*mut c_void),
    stat: unsafe extern "C" fn(*const u8, *mut Stat) -> i32,
    lstat: unsafe extern "C" fn(*const u8, *mut Stat) -> i32,
}

/// The C library's directory calls, or the program's.
#[derive(Clone, Copy)]
struct Fs {
    alt: Option<Alt>,
}

impl Fs {
    /// `GLOB_ALTDIRFUNC`'s functions, if the flag is set and all five are
    /// there; the C library's own otherwise (glibc's would call through a
    /// NULL one).
    fn of(g: &GlobT, flags: i32) -> Self {
        if flags & GLOB_ALTDIRFUNC == 0 {
            return Self { alt: None };
        }
        let alt = match (
            g.gl_opendir,
            g.gl_readdir,
            g.gl_closedir,
            g.gl_stat,
            g.gl_lstat,
        ) {
            (Some(opendir), Some(readdir), Some(closedir), Some(stat), Some(lstat)) => Some(Alt {
                opendir,
                readdir,
                closedir,
                stat,
                lstat,
            }),
            _ => None,
        };
        Self { alt }
    }

    /// Open `path`: the stream, or `errno`.
    fn opendir(&self, path: &Owned) -> Result<*mut c_void, i32> {
        crate::errno::set_errno(0);
        let d = match self.alt {
            // SAFETY: the program's function, given a C string.
            Some(a) => unsafe { (a.opendir)(path.ptr) },
            None => crate::dirent::opendir(path.ptr).cast(),
        };
        if d.is_null() {
            return Err(crate::errno::get_errno());
        }
        Ok(d)
    }

    /// The next entry: its name, copied, and its type.
    fn readdir(&self, d: *mut c_void) -> Result<Option<(Owned, u8)>, NoSpace> {
        let e = match self.alt {
            // SAFETY: the program's function, on the stream it opened.
            Some(a) => unsafe { (a.readdir)(d) },
            None => crate::dirent::readdir(d.cast()),
        };
        if e.is_null() {
            return Ok(None);
        }
        // SAFETY: a `struct dirent` the stream owns until its next read;
        // `d_name` is NUL-terminated within its array.
        let (name, d_type) = unsafe {
            let name = &(*e).d_name;
            let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
            (name.get(..len).unwrap_or(&[]), (*e).d_type)
        };
        Ok(Some((Owned::from_parts(&[name])?, d_type)))
    }

    fn closedir(&self, d: *mut c_void) {
        match self.alt {
            // SAFETY: the program's function, on the stream it opened.
            Some(a) => unsafe { (a.closedir)(d) },
            None => {
                // A stream this read and is done with: a failure to close it
                // loses nothing glob reports.
                let _ = crate::dirent::closedir(d.cast());
            }
        }
    }

    /// Does `path` name anything? `lstat`'s answer: a dangling link counts.
    fn exists(&self, path: &Owned) -> bool {
        // SAFETY: all-zero is a `Stat`.
        let mut st: Stat = unsafe { core::mem::zeroed() };
        let r = match self.alt {
            // SAFETY: the program's function, a C string and a `Stat`.
            Some(a) => unsafe { (a.lstat)(path.ptr, &raw mut st) },
            None => crate::file::lstat(path.ptr, &raw mut st),
        };
        r == 0
    }

    /// Is `path` a directory? `stat`'s answer: a link to one is one.
    fn is_dir(&self, path: &Owned) -> bool {
        // SAFETY: all-zero is a `Stat`.
        let mut st: Stat = unsafe { core::mem::zeroed() };
        let r = match self.alt {
            // SAFETY: as above.
            Some(a) => unsafe { (a.stat)(path.ptr, &raw mut st) },
            None => crate::file::stat(path.ptr, &raw mut st),
        };
        r == 0 && st.st_mode & crate::fcntl::S_IFMT == crate::fcntl::S_IFDIR
    }
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

/// Does the pattern hold an unescaped `*`, `?` or `[`?
fn has_magic(p: &[u8], flags: i32) -> bool {
    let mut i = 0usize;
    while let Some(&c) = p.get(i) {
        if c == b'\\' && flags & GLOB_NOESCAPE == 0 {
            i = i.saturating_add(2);
            continue;
        }
        if matches!(c, b'*' | b'?' | b'[') {
            return true;
        }
        i = i.saturating_add(1);
    }
    false
}

/// Is the pattern matched against a directory's entries, rather than
/// looked up? When it holds a wildcard -- or an escape, as glibc reads it,
/// so that `\*` is matched as the name `*`, `GLOB_ONLYDIR` applying.
fn needs_scan(p: &[u8], flags: i32) -> bool {
    has_magic(p, flags) || (flags & GLOB_NOESCAPE == 0 && p.contains(&b'\\'))
}

/// The pattern with its escapes removed: the name it stands for.
fn unescape(p: &[u8], flags: i32) -> Result<Owned, NoSpace> {
    let out = Owned::from_parts(&[p])?;
    if flags & GLOB_NOESCAPE != 0 {
        return Ok(out);
    }
    let (mut r, mut w) = (0usize, 0usize);
    // SAFETY: `out` holds `p.len()` bytes and a NUL; `w <= r` throughout.
    unsafe {
        while r < p.len() {
            let mut c = *out.ptr.add(r);
            if c == b'\\' && r.saturating_add(1) < p.len() {
                r = r.saturating_add(1);
                c = *out.ptr.add(r);
            }
            *out.ptr.add(w) = c;
            w = w.saturating_add(1);
            r = r.saturating_add(1);
        }
        *out.ptr.add(w) = 0;
    }
    let ptr = out.take();
    Ok(Owned { ptr, len: w })
}

/// The patterns a pattern's braces stand for (`GLOB_BRACE`): its first
/// `{...}` group cut at its top-level commas, each alternative in the
/// group's place and expanded again; the pattern alone if it has no
/// balanced group.
fn brace_expand(p: &[u8], flags: i32, out: &mut List<Owned>) -> Result<(), NoSpace> {
    let escapes = flags & GLOB_NOESCAPE == 0;
    let mut i = 0usize;
    while let Some(&c) = p.get(i) {
        if c == b'\\' && escapes {
            i = i.saturating_add(2);
            continue;
        }
        if c == b'{' {
            let mut depth = 0usize;
            let mut j = i;
            let mut cuts = List::<usize>::new();
            cuts.push(i)?;
            let mut closed = false;
            while let Some(&d) = p.get(j) {
                if d == b'\\' && escapes {
                    j = j.saturating_add(2);
                    continue;
                }
                match d {
                    b'{' => depth = depth.saturating_add(1),
                    b'}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            closed = true;
                            break;
                        }
                    }
                    b',' if depth == 1 => cuts.push(j)?,
                    _ => {}
                }
                j = j.saturating_add(1);
            }
            if !closed {
                break;
            }
            cuts.push(j)?;
            let head = p.get(..i).unwrap_or(&[]);
            let tail = p.get(j.saturating_add(1)..).unwrap_or(&[]);
            for w in cuts.as_slice().windows(2) {
                let (a, b) = (
                    w.first().copied().unwrap_or(0),
                    w.get(1).copied().unwrap_or(0),
                );
                let alt = p.get(a.saturating_add(1)..b).unwrap_or(&[]);
                let joined = Owned::from_parts(&[head, alt, tail])?;
                brace_expand(joined.bytes(), flags, out)?;
            }
            return Ok(());
        }
        i = i.saturating_add(1);
    }
    out.push(Owned::from_parts(&[p])?)
}

/// Own archive member -- glibc and gnulib define `glob_pattern_p` in an
/// object of its own, apart from `glob`, and so does a program that brings
/// only one of the two. See string.rs's module header.
mod gnu_glob_pattern_p {
    /// glibc's `glob_pattern_p`: does `pattern` hold a `*`, a `?`, or a `[`
    /// that a later `]` closes -- unescaped, if `quote`?
    ///
    /// # Safety
    ///
    /// `pattern` must be NULL or a C string.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn glob_pattern_p(pattern: *const u8, quote: i32) -> i32 {
        if pattern.is_null() {
            return 0;
        }
        // SAFETY: a C string, the caller's.
        let p = unsafe { core::slice::from_raw_parts(pattern, crate::string::strlen(pattern)) };
        let mut bracket = false;
        let mut i = 0usize;
        while let Some(&c) = p.get(i) {
            match c {
                b'?' | b'*' => return 1,
                // An escape hides the byte after it; a last one, nothing.
                b'\\' if quote != 0 && i.saturating_add(1) < p.len() => i = i.saturating_add(1),
                b'[' => bracket = true,
                b']' if bracket => return 1,
                _ => {}
            }
            i = i.saturating_add(1);
        }
        0
    }
}
pub use gnu_glob_pattern_p::glob_pattern_p;

// ---------------------------------------------------------------------------
// The walk
// ---------------------------------------------------------------------------

/// A `glob` caller's `errfunc`.
type ErrFunc = unsafe extern "C" fn(*const u8, i32) -> i32;

/// One glob over one pattern: the flags it reads with, and what it saw.
struct Globber {
    flags: i32,
    fs: Fs,
    errfunc: Option<ErrFunc>,
    /// `GLOB_MAGCHAR`, glibc's way: a last component read from a directory
    /// that found something -- or that `GLOB_NOCHECK` would answer for.
    magchar: bool,
    /// For `GLOB_NOCHECK`'s answer: a wildcard directory part that found a
    /// directory.
    dir_magic_found: bool,
}

impl Globber {
    const fn fnmatch_flags(&self) -> i32 {
        let mut f = 0;
        if self.flags & GLOB_PERIOD == 0 {
            f |= crate::fnmatch::FNM_PERIOD;
        }
        if self.flags & GLOB_NOESCAPE != 0 {
            f |= crate::fnmatch::FNM_NOESCAPE;
        }
        f
    }

    /// Tell the caller a directory could not be read: should glob stop?
    fn unreadable(&self, path: &Owned, errno: i32) -> bool {
        let told = match self.errfunc {
            // SAFETY: the program's function, given a C string.
            Some(f) => unsafe { f(path.ptr, errno) != 0 },
            None => false,
        };
        told || self.flags & GLOB_ERR != 0
    }

    /// The entries of the directory `dir` ("" for the current one) that
    /// `pat` matches, as `prefix` and their names, onto `out`.
    fn in_dir(
        &mut self,
        dir: &[u8],
        prefix: &[u8],
        pat: &[u8],
        only_dirs: bool,
        out: &mut List<Found>,
    ) -> Result<(), Stop> {
        let open = Owned::from_parts(&[if dir.is_empty() { b"." } else { dir }])?;
        let d = match self.fs.opendir(&open) {
            Ok(d) => d,
            Err(errno) => {
                // A name that is not a directory holds nothing: not an error.
                if errno != crate::errno::ENOTDIR && self.unreadable(&open, errno) {
                    return Err(Stop::Aborted);
                }
                return Ok(());
            }
        };
        let before = out.len;
        let result = self.read_matches(d, prefix, pat, only_dirs, out);
        self.fs.closedir(d);
        result?;
        if out.len > before || self.flags & GLOB_NOCHECK != 0 {
            self.magchar = true;
        }
        Ok(())
    }

    /// Is it worth a `stat` to know whether a name is a directory? Only for
    /// a caller that keeps directories alone, or marks them.
    const fn wants_kind(&self, only_dirs: bool) -> bool {
        only_dirs || self.flags & GLOB_MARK != 0
    }

    fn read_matches(
        &self,
        d: *mut c_void,
        prefix: &[u8],
        pat: &[u8],
        only_dirs: bool,
        out: &mut List<Found>,
    ) -> Result<(), Stop> {
        while let Some((name, d_type)) = self.fs.readdir(d)? {
            // The entry's type, where the directory says it: a link or an
            // unknown type is `stat`'s to settle.
            let known = match d_type {
                crate::linux_dirent_types::DT_DIR => Some(true),
                crate::linux_dirent_types::DT_UNKNOWN | crate::linux_dirent_types::DT_LNK => None,
                _ => Some(false),
            };
            if only_dirs && known == Some(false) {
                continue;
            }
            if !crate::fnmatch::matches(pat, name.bytes(), self.fnmatch_flags())? {
                continue;
            }
            let path = Owned::from_parts(&[prefix, name.bytes()])?;
            let is_dir =
                known.unwrap_or_else(|| self.wants_kind(only_dirs) && self.fs.is_dir(&path));
            if only_dirs && !is_dir {
                continue;
            }
            out.push(Found { path, is_dir })?;
        }
        Ok(())
    }

    /// A name, looked up rather than read: onto `out` if it exists. The
    /// empty pathname names no file (XBD 4.13) -- the empty pattern matches
    /// nothing, whatever `GLOB_ALTDIRFUNC`'s `lstat` would say of "".
    fn look_up(&self, path: Owned, only_dirs: bool, out: &mut List<Found>) -> Result<(), Stop> {
        if !path.bytes().is_empty() && self.fs.exists(&path) {
            let is_dir = self.wants_kind(only_dirs) && self.fs.is_dir(&path);
            out.push(Found { path, is_dir })?;
        }
        Ok(())
    }

    /// Everything `pattern` names, with whether each is a directory.
    fn expand(&mut self, pattern: &[u8], only_dirs: bool) -> Result<List<Found>, Stop> {
        let mut out = List::new();
        if !needs_scan(pattern, self.flags) {
            self.look_up(unescape(pattern, self.flags)?, only_dirs, &mut out)?;
            return Ok(out);
        }
        if pattern.len() > 1 && pattern.last() == Some(&b'/') {
            // "X/": the directories X names, each with its slash back -- X's
            // last component read with the caller's own flags.
            let inner = self.expand(
                pattern
                    .get(..pattern.len().saturating_sub(1))
                    .unwrap_or(&[]),
                true,
            )?;
            for f in inner.as_slice().iter().filter(|f| f.is_dir) {
                out.push(Found {
                    path: Owned::from_parts(&[f.path.bytes(), b"/"])?,
                    is_dir: true,
                })?;
            }
            return Ok(out);
        }
        let Some(slash) = pattern.iter().rposition(|&b| b == b'/') else {
            self.in_dir(b"", b"", pattern, only_dirs, &mut out)?;
            return Ok(out);
        };
        let dirpart = pattern.get(..slash).unwrap_or(&[]);
        let filepart = pattern.get(slash.saturating_add(1)..).unwrap_or(&[]);
        let mut dirs = List::new();
        if dirpart.is_empty() {
            dirs.push(Found {
                path: Owned::from_parts(&[b"/"])?,
                is_dir: true,
            })?;
        } else if needs_scan(dirpart, self.flags) {
            // The directories, with only the flags that say how to read them:
            // a wildcard there never matches "." or ".." by GLOB_PERIOD, and
            // nothing is marked, sorted or answered for NOCHECK -- glibc's
            // reading.
            let mut sub = Self {
                flags: (self.flags & (GLOB_ERR | GLOB_NOESCAPE | GLOB_ALTDIRFUNC))
                    | GLOB_NOSORT
                    | GLOB_ONLYDIR,
                fs: self.fs,
                errfunc: self.errfunc,
                magchar: false,
                dir_magic_found: false,
            };
            dirs = sub.expand(dirpart, true)?;
            if dirs.len > 0 {
                self.dir_magic_found = true;
            }
        } else {
            dirs.push(Found {
                path: unescape(dirpart, self.flags)?,
                is_dir: true,
            })?;
        }
        let scan = needs_scan(filepart, self.flags);
        let name = if scan {
            None
        } else {
            Some(unescape(filepart, self.flags)?)
        };
        for d in dirs.as_slice() {
            let dir = d.path.bytes();
            let prefix = if dir == b"/" {
                Owned::from_parts(&[b"/"])?
            } else {
                Owned::from_parts(&[dir, b"/"])?
            };
            match &name {
                None => self.in_dir(dir, prefix.bytes(), filepart, only_dirs, &mut out)?,
                Some(n) => self.look_up(
                    Owned::from_parts(&[prefix.bytes(), n.bytes()])?,
                    only_dirs,
                    &mut out,
                )?,
            }
        }
        Ok(out)
    }
}

/// The home directory `~user` names (`user` empty: the caller's own).
type HomeOf<'h> = &'h dyn Fn(&[u8]) -> Option<Owned>;

/// The system's answer: `HOME` for the caller's own, else the password
/// database's.
fn system_home(user: &[u8]) -> Option<Owned> {
    let dir: *const u8 = if user.is_empty() {
        // SAFETY: a C string literal.
        let h = unsafe { crate::environ::getenv(c"HOME".as_ptr().cast()) };
        // SAFETY: getenv's answer: NULL or a C string.
        if !h.is_null() && unsafe { *h } != 0 {
            h
        } else {
            let pw = crate::pwd::getpwuid(crate::unistd::getuid());
            if pw.is_null() {
                return None;
            }
            // SAFETY: getpwuid's entry, valid until its next call.
            unsafe { (*pw).pw_dir }
        }
    } else {
        let name = Owned::from_parts(&[user]).ok()?;
        // SAFETY: a C string.
        let pw = unsafe { crate::pwd::getpwnam(name.ptr) };
        if pw.is_null() {
            return None;
        }
        // SAFETY: as above.
        unsafe { (*pw).pw_dir }
    };
    if dir.is_null() {
        return None;
    }
    // SAFETY: a C string the environment or the database holds.
    let bytes = unsafe { core::slice::from_raw_parts(dir, crate::string::strlen(dir)) };
    Owned::from_parts(&[bytes]).ok()
}

/// `~` and `~user` expanded (`GLOB_TILDE`): the pattern, and whether it is
/// a bare `~` or `~user` -- returned as it is, looked for nowhere, as glibc
/// returns it. `None` for `GLOB_TILDE_CHECK`'s unknown user.
fn tilde(p: &[u8], flags: i32, home: HomeOf<'_>) -> Result<Option<(Owned, bool)>, NoSpace> {
    if p.first() != Some(&b'~') || flags & (GLOB_TILDE | GLOB_TILDE_CHECK) == 0 {
        return Ok(Some((Owned::from_parts(&[p])?, false)));
    }
    let end = p.iter().position(|&b| b == b'/').unwrap_or(p.len());
    let user = p.get(1..end).unwrap_or(&[]);
    let rest = p.get(end..).unwrap_or(&[]);
    match home(user) {
        Some(h) => Ok(Some((
            Owned::from_parts(&[h.bytes(), rest])?,
            rest.is_empty(),
        ))),
        None if flags & GLOB_TILDE_CHECK != 0 => Ok(None),
        None => Ok(Some((Owned::from_parts(&[p])?, rest.is_empty()))),
    }
}

impl Globber {
    /// Every pattern the braces make, each globbed, marked and sorted.
    fn all(&mut self, pattern: &[u8], home: HomeOf<'_>) -> Result<List<Found>, Stop> {
        let flags = self.flags;
        let mut patterns = List::new();
        if flags & GLOB_BRACE != 0 {
            brace_expand(pattern, flags, &mut patterns)?;
        } else {
            patterns.push(Owned::from_parts(&[pattern])?)?;
        }
        let mut all = List::new();
        for p in patterns.as_slice() {
            let Some((t, bare)) = tilde(p.bytes(), flags, home)? else {
                continue;
            };
            let mut r = if bare {
                let is_dir = flags & GLOB_MARK != 0 && self.fs.is_dir(&t);
                let mut one = List::new();
                one.push(Found { path: t, is_dir })?;
                one
            } else {
                self.expand(t.bytes(), flags & GLOB_ONLYDIR != 0)?
            };
            if flags & GLOB_MARK != 0 {
                for f in r.as_mut_slice() {
                    // A name ending in a slash says it is a directory already.
                    if f.is_dir && f.path.bytes().last() != Some(&b'/') {
                        f.path = Owned::from_parts(&[f.path.bytes(), b"/"])?;
                    }
                }
            }
            if flags & GLOB_NOSORT == 0 {
                r.as_mut_slice()
                    .sort_unstable_by(|a, b| a.path.bytes().cmp(b.path.bytes()));
            }
            all.append(r)?;
        }
        Ok(all)
    }
}

/// [`glob`] over a byte string, `~`'s homes from `home`.
fn glob_bytes(
    pattern: &[u8],
    flags: i32,
    errfunc: Option<ErrFunc>,
    g: &mut GlobT,
    home: HomeOf<'_>,
) -> i32 {
    // gl_offs means nothing without GLOB_DOOFFS, and is 0 then -- so that
    // globfree knows where the names start -- as glibc has it.
    if flags & GLOB_DOOFFS == 0 {
        g.gl_offs = 0;
    }
    if flags & GLOB_APPEND == 0 {
        g.gl_pathc = 0;
        g.gl_pathv = core::ptr::null_mut();
    }
    let mut globber = Globber {
        flags,
        fs: Fs::of(g, flags),
        errfunc,
        magchar: false,
        dir_magic_found: false,
    };
    let mut found = match globber.all(pattern, home) {
        Ok(f) => f,
        Err(Stop::NoSpace) => return GLOB_NOSPACE,
        Err(Stop::Aborted) => {
            g.gl_flags = flags | if globber.magchar { GLOB_MAGCHAR } else { 0 };
            return GLOB_ABORTED;
        }
    };
    let mut magchar = globber.magchar;
    if found.len == 0 {
        let nomagic =
            flags & GLOB_NOMAGIC != 0 && !pattern.is_empty() && !needs_scan(pattern, flags);
        if flags & GLOB_NOCHECK == 0 && !nomagic {
            g.gl_flags = flags;
            return GLOB_NOMATCH;
        }
        let pushed = Owned::from_parts(&[pattern]).and_then(|path| {
            found.push(Found {
                path,
                is_dir: false,
            })
        });
        if pushed.is_err() {
            return GLOB_NOSPACE;
        }
        magchar = magchar || globber.dir_magic_found;
    }
    // gl_pathv: the reserved NULLs, what an earlier call left, these, NULL.
    let offs = g.gl_offs;
    let old = if g.gl_pathv.is_null() { 0 } else { g.gl_pathc };
    let new = found.len;
    let Some(total) = offs
        .checked_add(old)
        .and_then(|n| n.checked_add(new))
        .and_then(|n| n.checked_add(1))
    else {
        return GLOB_NOSPACE;
    };
    let Some(bytes) = total.checked_mul(size_of::<*mut u8>()) else {
        return GLOB_NOSPACE;
    };
    let fresh = g.gl_pathv.is_null();
    // SAFETY: NULL or the array an earlier call made, from `malloc`.
    let v = unsafe { crate::malloc::realloc(g.gl_pathv.cast(), bytes) }.cast::<*mut u8>();
    if v.is_null() {
        return GLOB_NOSPACE;
    }
    // SAFETY: `v` has `total` slots: the reserved ones (NULLed when new),
    // the old names (kept by realloc), the new ones, and the terminator.
    unsafe {
        if fresh {
            for i in 0..offs {
                v.add(i).write(core::ptr::null_mut());
            }
        }
        let at = offs.saturating_add(old);
        for i in 0..new {
            let f = found.ptr.add(i).read();
            v.add(at.saturating_add(i)).write(f.path.take());
        }
        // Every element was moved out: the list owns none of them now.
        found.len = 0;
        v.add(at.saturating_add(new)).write(core::ptr::null_mut());
    }
    g.gl_pathv = v;
    g.gl_pathc = old.saturating_add(new);
    g.gl_flags = flags | if magchar { GLOB_MAGCHAR } else { 0 };
    0
}

/// `glob(pattern, flags, errfunc, pglob)`: the pathnames `pattern` matches,
/// in `pglob->gl_pathv`. 0, or [`GLOB_NOMATCH`], [`GLOB_ABORTED`],
/// [`GLOB_NOSPACE`]; -1 with `errno` `EINVAL` for a NULL argument or a flag
/// not glob's, as glibc.
///
/// # Safety
///
/// `pattern` must be a C string and `pglob` a valid `glob_t` -- with
/// `GLOB_APPEND`, one an earlier `glob` filled; with `GLOB_ALTDIRFUNC`, its
/// functions valid to call.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn glob(
    pattern: *const u8,
    flags: i32,
    errfunc: Option<ErrFunc>,
    pglob: *mut GlobT,
) -> i32 {
    if pattern.is_null() || pglob.is_null() || flags & !GLOB_FLAGS != 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    // SAFETY: a C string, the caller's.
    let p = unsafe { core::slice::from_raw_parts(pattern, crate::string::strlen(pattern)) };
    // SAFETY: the caller's `glob_t`.
    glob_bytes(p, flags, errfunc, unsafe { &mut *pglob }, &system_home)
}

/// `globfree(pglob)`: free what `glob` put in it.
///
/// # Safety
///
/// `pglob` must be NULL or a `glob_t` `glob` filled (or an empty one).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn globfree(pglob: *mut GlobT) {
    if pglob.is_null() {
        return;
    }
    // SAFETY: the caller's `glob_t`.
    let g = unsafe { &mut *pglob };
    if !g.gl_pathv.is_null() {
        for i in 0..g.gl_pathc {
            // SAFETY: each name glob stored, after the reserved slots.
            unsafe { crate::malloc::free(*g.gl_pathv.add(g.gl_offs.saturating_add(i))) };
        }
        // SAFETY: the array glob made.
        unsafe { crate::malloc::free(g.gl_pathv.cast()) };
    }
    g.gl_pathv = core::ptr::null_mut();
    g.gl_pathc = 0;
}

/// `glob64` -- [`glob`] by glibc's large-file name: its `glob64_t` is
/// `glob_t` with `struct dirent64` and `struct stat64` in the
/// `GLOB_ALTDIRFUNC` functions' types, which on x86_64 are `struct dirent`
/// and `struct stat`. In `glob`'s archive member, as glibc's is an alias in
/// `glob`'s object and as `scripts/check-libc-shape.py`'s glob family has
/// it: a program has all of the family from itself or all from here, and
/// one that brings its own `glob` and still calls `glob64` fails to link
/// instead of running half of each.
///
/// # Safety
///
/// As [`glob`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn glob64(
    pattern: *const u8,
    flags: i32,
    errfunc: Option<ErrFunc>,
    pglob: *mut GlobT,
) -> i32 {
    // SAFETY: the caller's contract is glob's.
    unsafe { glob(pattern, flags, errfunc, pglob) }
}

/// `globfree64` -- [`globfree`] by glibc's large-file name.
///
/// # Safety
///
/// As [`globfree`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn globfree64(pglob: *mut GlobT) {
    // SAFETY: the caller's contract is globfree's.
    unsafe { globfree(pglob) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux_dirent_types::{DT_DIR, DT_REG};
    use std::borrow::ToOwned;
    use std::boxed::Box;
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, HashSet};
    use std::format;
    use std::string::{String, ToString};
    use std::sync::OnceLock;
    use std::vec;
    use std::vec::Vec;

    const ORACLE: &str = include_str!("glob_oracle.txt");
    const DEVIATIONS: &str = include_str!("glob_deviations.txt");

    // -- The oracle's escapes -----------------------------------------------

    /// `\xNN` escapes back to bytes, as `glob_harness.py` writes them; `\x`
    /// alone is the empty string.
    fn from_oracle(t: &str) -> Vec<u8> {
        let b = t.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && b.get(i + 1) == Some(&b'x') {
                if i + 4 <= b.len() {
                    out.push(u8::from_str_radix(&t[i + 2..i + 4], 16).unwrap());
                    i += 4;
                } else {
                    i += 2;
                }
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }

    /// Bytes as the harness writes them.
    fn to_oracle(b: &[u8]) -> String {
        if b.is_empty() {
            return "\\x".to_owned();
        }
        let mut s = String::new();
        for &c in b {
            if c == b'\\' || c <= b' ' || c > b'~' {
                s.push_str(&format!("\\x{c:02x}"));
            } else {
                s.push(char::from(c));
            }
        }
        s
    }

    fn c_bytes<'a>(p: *const u8) -> &'a [u8] {
        // SAFETY: glob hands its callbacks C strings.
        unsafe { core::slice::from_raw_parts(p, crate::string::strlen(p)) }
    }

    // -- A directory tree in memory, through GLOB_ALTDIRFUNC ----------------

    /// A directory tree as the harness's is: each directory's names in the
    /// tree's order, and which nodes are directories. Node "" is the
    /// current directory, "/" the root; "noperm" cannot be opened.
    struct Tree {
        children: HashMap<Vec<u8>, Vec<Vec<u8>>>,
        dirs: HashSet<Vec<u8>>,
    }

    impl Tree {
        /// From paths, directories with a trailing slash, parents first.
        fn new(entries: &[Vec<u8>]) -> Self {
            let mut children: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();
            let mut dirs = HashSet::new();
            for node in [&b""[..], b"/"] {
                children.insert(node.to_vec(), Vec::new());
                dirs.insert(node.to_vec());
            }
            for entry in entries {
                let is_dir = entry.last() == Some(&b'/');
                let path = &entry[..entry.len() - usize::from(is_dir)];
                let (parent, name) = match path.iter().rposition(|&b| b == b'/') {
                    Some(0) => (b"/".to_vec(), path[1..].to_vec()),
                    Some(i) => (path[..i].to_vec(), path[i + 1..].to_vec()),
                    None => (Vec::new(), path.to_vec()),
                };
                children.entry(parent).or_default().push(name);
                if is_dir {
                    children.entry(path.to_vec()).or_default();
                    dirs.insert(path.to_vec());
                }
            }
            Self { children, dirs }
        }

        /// The harness's, from the oracle's `# tree:` line.
        fn oracle() -> &'static Self {
            static TREE: OnceLock<Tree> = OnceLock::new();
            TREE.get_or_init(|| {
                let line = ORACLE
                    .lines()
                    .find_map(|l| l.strip_prefix("# tree: "))
                    .unwrap();
                Self::new(&line.split(' ').map(from_oracle).collect::<Vec<_>>())
            })
        }

        fn child(parent: &[u8], name: &[u8]) -> Vec<u8> {
            match parent {
                b"" => name.to_vec(),
                b"/" => [b"/", name].concat(),
                _ => [parent, b"/", name].concat(),
            }
        }

        /// The node `path` names, as the harness's lookup finds it: "." and
        /// ".." resolved, ".." back along the path taken and staying at the
        /// top; "" is the current directory. Else the errno it gives.
        fn lookup(&self, path: &[u8]) -> Result<Vec<u8>, i32> {
            let mut node = if path.first() == Some(&b'/') {
                b"/".to_vec()
            } else {
                Vec::new()
            };
            let mut taken = Vec::new();
            for comp in path.split(|&b| b == b'/') {
                match comp {
                    b"" | b"." => {}
                    b".." => {
                        if let Some(up) = taken.pop() {
                            node = up;
                        }
                    }
                    _ => {
                        if !self.dirs.contains(&node) {
                            return Err(crate::errno::ENOTDIR);
                        }
                        if !self.children[&node].iter().any(|n| n == comp) {
                            return Err(crate::errno::ENOENT);
                        }
                        let next = Self::child(&node, comp);
                        taken.push(core::mem::replace(&mut node, next));
                    }
                }
            }
            if path.last() == Some(&b'/') && !self.dirs.contains(&node) {
                return Err(crate::errno::ENOTDIR);
            }
            Ok(node)
        }
    }

    thread_local! {
        /// The tree this thread's callbacks read.
        static CURRENT: Cell<Option<&'static Tree>> = const { Cell::new(None) };
        /// What `on_error` was told, as the harness logs it.
        static ERRLOG: RefCell<String> = const { RefCell::new(String::new()) };
        /// Whether `on_error` asks glob to stop.
        static STOP: Cell<bool> = const { Cell::new(false) };
    }

    fn tree() -> &'static Tree {
        CURRENT.get().unwrap_or_else(Tree::oracle)
    }

    struct Stream {
        entries: Vec<(Vec<u8>, bool)>,
        next: usize,
        entry: Dirent,
    }

    unsafe extern "C" fn tree_opendir(path: *const u8) -> *mut c_void {
        let t = tree();
        let node = match t.lookup(c_bytes(path)) {
            Ok(n) => n,
            Err(e) => {
                crate::errno::set_errno(e);
                return core::ptr::null_mut();
            }
        };
        if !t.dirs.contains(&node) {
            crate::errno::set_errno(crate::errno::ENOTDIR);
            return core::ptr::null_mut();
        }
        if node == b"noperm" {
            crate::errno::set_errno(crate::errno::EACCES);
            return core::ptr::null_mut();
        }
        let mut entries = vec![(b".".to_vec(), true), (b"..".to_vec(), true)];
        for n in &t.children[&node] {
            entries.push((n.clone(), t.dirs.contains(&Tree::child(&node, n))));
        }
        // SAFETY: all-zero is a `Dirent`.
        let entry = unsafe { core::mem::zeroed() };
        Box::into_raw(Box::new(Stream {
            entries,
            next: 0,
            entry,
        }))
        .cast()
    }

    unsafe extern "C" fn tree_readdir(d: *mut c_void) -> *mut Dirent {
        // SAFETY: a stream `tree_opendir` made.
        let s = unsafe { &mut *d.cast::<Stream>() };
        let Some((name, is_dir)) = s.entries.get(s.next) else {
            return core::ptr::null_mut();
        };
        s.next += 1;
        s.entry.d_name = [0; 256];
        s.entry.d_name[..name.len()].copy_from_slice(name);
        s.entry.d_type = if *is_dir { DT_DIR } else { DT_REG };
        &raw mut s.entry
    }

    unsafe extern "C" fn tree_closedir(d: *mut c_void) {
        // SAFETY: a stream `tree_opendir` made, closed once.
        drop(unsafe { Box::from_raw(d.cast::<Stream>()) });
    }

    unsafe extern "C" fn tree_stat(path: *const u8, st: *mut Stat) -> i32 {
        let t = tree();
        match t.lookup(c_bytes(path)) {
            Ok(node) => {
                // SAFETY: glob's `Stat`; all-zero is one.
                unsafe {
                    st.write(core::mem::zeroed());
                    (*st).st_mode = if t.dirs.contains(&node) {
                        crate::fcntl::S_IFDIR | 0o755
                    } else {
                        crate::fcntl::S_IFREG | 0o644
                    };
                }
                0
            }
            Err(e) => {
                crate::errno::set_errno(e);
                -1
            }
        }
    }

    unsafe extern "C" fn on_error(path: *const u8, errno: i32) -> i32 {
        ERRLOG.with_borrow_mut(|l| {
            l.push(' ');
            l.push_str(&to_oracle(c_bytes(path)));
            l.push(':');
            l.push_str(&errno.to_string());
        });
        i32::from(STOP.get())
    }

    /// A `glob_t` reading the tree.
    fn tree_glob_t() -> GlobT {
        let mut g = GlobT::new();
        g.gl_opendir = Some(tree_opendir);
        g.gl_readdir = Some(tree_readdir);
        g.gl_closedir = Some(tree_closedir);
        g.gl_stat = Some(tree_stat);
        g.gl_lstat = Some(tree_stat);
        g
    }

    /// The homes the harness's system has: `HOME` is /home/u, and root's.
    fn harness_home(user: &[u8]) -> Option<Owned> {
        match user {
            b"" => Owned::from_parts(&[b"/home/u"]).ok(),
            b"root" => Owned::from_parts(&[b"/root"]).ok(),
            _ => None,
        }
    }

    /// `glob` over the tree, flags as `glob_t`'s callbacks want them.
    fn run(g: &mut GlobT, pattern: &[u8], flags: i32) -> i32 {
        glob_bytes(
            pattern,
            flags | GLOB_ALTDIRFUNC,
            Some(on_error),
            g,
            &harness_home,
        )
    }

    /// The names glob returned.
    fn names(g: &GlobT) -> Vec<String> {
        (0..g.gl_pathc)
            .map(|i| {
                // SAFETY: glob's names, after the reserved slots.
                let p = unsafe { *g.gl_pathv.add(g.gl_offs + i) };
                String::from_utf8(c_bytes(p).to_vec()).unwrap()
            })
            .collect()
    }

    /// The oracle's flag names: the flags, and whether the errfunc stops.
    fn flag_set(names: &str) -> (i32, bool) {
        names
            .split('|')
            .fold((0, false), |(f, stop), name| match name {
                "0" => (f, stop),
                "ERR" => (f | GLOB_ERR, stop),
                "MARK" => (f | GLOB_MARK, stop),
                "NOSORT" => (f | GLOB_NOSORT, stop),
                "NOCHECK" => (f | GLOB_NOCHECK, stop),
                "NOESCAPE" => (f | GLOB_NOESCAPE, stop),
                "PERIOD" => (f | GLOB_PERIOD, stop),
                "BRACE" => (f | GLOB_BRACE, stop),
                "NOMAGIC" => (f | GLOB_NOMAGIC, stop),
                "TILDE" => (f | GLOB_TILDE, stop),
                "TILDE_CHECK" => (f | GLOB_TILDE_CHECK, stop),
                "ONLYDIR" => (f | GLOB_ONLYDIR, stop),
                "ERRSTOP" => (f, true),
                other => panic!("{other}"),
            })
    }

    /// One probe, answered here and written as the harness writes glibc's:
    /// `<flags> <pattern> = <return> <magchar> <path>... |<errfunc calls>`.
    fn probe(fname: &str, pattern: &[u8]) -> String {
        let (flags, stop) = flag_set(fname);
        let mut g = tree_glob_t();
        ERRLOG.with_borrow_mut(String::clear);
        STOP.set(stop);
        let r = run(&mut g, pattern, flags);
        let mut line = format!(
            "{fname} {} = {r} {}",
            to_oracle(pattern),
            i32::from(g.gl_flags & GLOB_MAGCHAR != 0)
        );
        if r == 0 || r == GLOB_NOMATCH || r == GLOB_ABORTED {
            for name in names(&g) {
                line.push(' ');
                line.push_str(&to_oracle(name.as_bytes()));
            }
        }
        line.push_str(" |");
        line.push_str(&ERRLOG.with_borrow(Clone::clone));
        // SAFETY: what glob filled.
        unsafe { globfree(&raw mut g) };
        line
    }

    // -- glibc 2.39's answers, and where they are not these ----------------

    /// Every probe of `glob_oracle.txt` answered as glibc 2.39 answers it --
    /// its return, `GLOB_MAGCHAR`, the names in their order, and each call
    /// of the errfunc -- except those `glob_deviations.txt` lists, answered
    /// as it says (design-decisions §1149).
    #[test]
    fn glob_is_glibcs_but_where_glibc_is_not_its_flags() {
        let mut deviations = HashMap::new();
        let mut lines = DEVIATIONS.lines().filter(|l| !l.starts_with('#'));
        while let Some(glibc) = lines.next() {
            let here = lines.next().unwrap();
            deviations.insert(
                glibc.strip_prefix("glibc ").unwrap(),
                here.strip_prefix("here  ").unwrap(),
            );
        }
        let (mut n, mut used) = (0usize, 0usize);
        let mut bad = Vec::new();
        for line in ORACLE.lines() {
            if line.starts_with('#') || line.starts_with("pattern_p ") {
                continue;
            }
            let (head, _) = line.split_once(" = ").unwrap();
            let (fname, pat) = head.split_once(' ').unwrap();
            let want = match deviations.get(line) {
                Some(here) => {
                    used += 1;
                    *here
                }
                None => line,
            };
            let got = probe(fname, &from_oracle(pat));
            n += 1;
            if got != want {
                bad.push(format!("want {want}\n got {got}"));
            }
        }
        assert_eq!(
            used,
            deviations.len(),
            "every listed deviation is a line of the oracle: the list is stale"
        );
        assert!(n >= 1504, "{n} probes");
        assert!(
            bad.is_empty(),
            "{} of {n} probes wrong, the first:\n{}",
            bad.len(),
            bad.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
        );
    }

    /// The departures, each reasoned from what it follows (the deviation
    /// list is `glob_model.py`'s; these are not generated).
    #[test]
    fn where_glibc_departs_this_follows_the_flags() {
        // glibc reads `*/` and `?/` by a path of their own; here they are
        // answered as glibc answers `**/` and `[!x]/`, which match the same
        // names -- the oracle holds this library to glibc's answers for
        // those two, whatever the flags.
        for fname in [
            "0",
            "MARK",
            "NOSORT",
            "NOCHECK",
            "NOESCAPE",
            "PERIOD",
            "BRACE",
            "NOMAGIC",
            "TILDE",
            "ONLYDIR",
            "MARK|ONLYDIR",
            "ERR",
            "ERRSTOP",
        ] {
            let answer = |pat: &str| {
                probe(fname, pat.as_bytes())
                    .split_once(" = ")
                    .unwrap()
                    .1
                    .replace(pat, "P")
            };
            assert_eq!(answer("*/"), answer("**/"), "{fname}");
            assert_eq!(answer("?/"), answer("[!x]/"), "{fname}");
        }
        // So GLOB_MARK does not double the slash of a name that ends in one,
        assert_eq!(
            probe("MARK", b"*/"),
            "MARK */ = 0 1 dir1/ dir2/ empty/ noperm/ |"
        );
        // GLOB_PERIOD lets the star match "." and "..", as it does in `*`,
        assert_eq!(
            probe("PERIOD", b"*/"),
            "PERIOD */ = 0 1 ../ ./ dir1/ dir2/ empty/ noperm/ |"
        );
        assert_eq!(probe("PERIOD", b"?/"), "PERIOD ?/ = 0 1 ./ |");
        assert!(probe("PERIOD", b"*").starts_with("PERIOD * = 0 1 * . .. .h2"));
        // and GLOB_MAGCHAR is set: the star was matched.
        assert_eq!(probe("0", b"*/"), "0 */ = 0 1 dir1/ dir2/ empty/ noperm/ |");
        // GLOB_NOCHECK: "a list consisting of only pattern" -- all of it,
        // the slash too, as glibc answers for `a/` and `?/` and not `??/`.
        assert_eq!(probe("NOCHECK", b"??/"), "NOCHECK ??/ = 0 1 ??/ |");
        assert_eq!(probe("NOCHECK", b"a/"), "NOCHECK a/ = 0 0 a/ |");
    }

    /// glibc's `glob_pattern_p`, as the oracle has it, for both `quote`s.
    #[test]
    fn glob_pattern_p_is_glibcs() {
        let mut n = 0;
        for line in ORACLE.lines().filter_map(|l| l.strip_prefix("pattern_p ")) {
            let (head, want) = line.split_once(" = ").unwrap();
            let (quote, pat) = head.split_once(' ').unwrap();
            let mut p = from_oracle(pat);
            p.push(0);
            // SAFETY: a C string.
            let got = unsafe { glob_pattern_p(p.as_ptr(), quote.parse().unwrap()) };
            assert_eq!(got.to_string(), want, "{line}");
            n += 1;
        }
        assert_eq!(n, 50);
        // SAFETY: NULL is allowed.
        assert_eq!(unsafe { glob_pattern_p(core::ptr::null(), 1) }, 0);
    }

    // -- gl_pathv, gl_offs and GLOB_APPEND ----------------------------------

    /// GLOB_DOOFFS reserves gl_offs NULLs; GLOB_APPEND adds after the names
    /// already there, keeping them and the NULLs; a later NOMATCH leaves
    /// them alone; globfree frees it all and empties the `glob_t`.
    #[test]
    fn dooffs_reserves_and_append_adds() {
        let mut g = tree_glob_t();
        g.gl_offs = 2;
        assert_eq!(run(&mut g, b"a*", GLOB_DOOFFS), 0);
        assert_eq!(names(&g), ["a", "ab", "abc"]);
        // SAFETY: gl_offs + gl_pathc + 1 slots.
        unsafe {
            assert!((*g.gl_pathv).is_null() && (*g.gl_pathv.add(1)).is_null());
            assert!((*g.gl_pathv.add(5)).is_null());
        }
        assert_eq!(run(&mut g, b"*.c", GLOB_DOOFFS | GLOB_APPEND), 0);
        assert_eq!(names(&g), ["a", "ab", "abc", "x.c", "y.c"]);
        // SAFETY: as above.
        unsafe {
            assert!((*g.gl_pathv).is_null() && (*g.gl_pathv.add(1)).is_null());
            assert!((*g.gl_pathv.add(7)).is_null());
        }
        assert_eq!(
            run(&mut g, b"nothing*", GLOB_DOOFFS | GLOB_APPEND),
            GLOB_NOMATCH
        );
        assert_eq!(names(&g), ["a", "ab", "abc", "x.c", "y.c"]);
        // SAFETY: what glob filled.
        unsafe { globfree(&raw mut g) };
        assert!(g.gl_pathv.is_null());
        assert_eq!(g.gl_pathc, 0);
        // SAFETY: globfree again, and on NULL, is harmless.
        unsafe {
            globfree(&raw mut g);
            globfree(core::ptr::null_mut());
        }
    }

    /// Without GLOB_APPEND the `glob_t` starts empty, whatever it held;
    /// without GLOB_DOOFFS gl_offs is 0, as glibc makes it.
    #[test]
    fn a_new_glob_starts_empty() {
        let mut g = tree_glob_t();
        g.gl_offs = 7;
        g.gl_pathc = 99;
        g.gl_pathv = core::ptr::dangling_mut();
        assert_eq!(run(&mut g, b"dir1/f*", 0), 0);
        assert_eq!(g.gl_offs, 0);
        assert_eq!(names(&g), ["dir1/f1", "dir1/f2"]);
        // SAFETY: what glob filled.
        unsafe { globfree(&raw mut g) };
        // NOMATCH: nothing, and nothing to free.
        assert_eq!(run(&mut g, b"zz*", 0), GLOB_NOMATCH);
        assert_eq!(g.gl_pathc, 0);
        assert!(g.gl_pathv.is_null());
    }

    /// GLOB_NOCHECK's pattern is the pattern as given, escapes and all.
    #[test]
    fn nocheck_returns_the_pattern_itself() {
        let mut g = tree_glob_t();
        assert_eq!(run(&mut g, b"no\\*such", GLOB_NOCHECK), 0);
        assert_eq!(names(&g), ["no\\*such"]);
        // SAFETY: what glob filled.
        unsafe { globfree(&raw mut g) };
    }

    /// A NULL argument or a flag glob does not take is EINVAL, -1, as glibc
    /// answers -- GLOB_MAGCHAR included: it is glob's to set.
    #[test]
    fn bad_arguments_are_einval() {
        let mut g = GlobT::new();
        for (pattern, pglob, flags) in [
            (core::ptr::null(), &raw mut g, 0),
            (c"*".as_ptr().cast::<u8>(), core::ptr::null_mut(), 0),
            (c"*".as_ptr().cast(), &raw mut g, GLOB_MAGCHAR),
            (c"*".as_ptr().cast(), &raw mut g, 0x8000),
        ] {
            crate::errno::set_errno(0);
            // SAFETY: NULLs and a valid glob_t; glob refuses before reading.
            assert_eq!(unsafe { glob(pattern, flags, None, pglob) }, -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        }
    }

    /// glob64 and globfree64 are glob and globfree: the same answers and
    /// names for the same patterns over the tree, and the same refusals.
    #[test]
    fn the_large_file_names_are_glob_and_globfree() {
        for pattern in [c"*", c"*/*", c"/*", c"nomatch*", c"[a-c]*", c"*/"] {
            for flags in [0, GLOB_MARK | GLOB_NOCHECK, GLOB_NOSORT | GLOB_PERIOD] {
                let (mut a, mut b) = (tree_glob_t(), tree_glob_t());
                // SAFETY: C strings, and glob_ts whose functions are the
                // tree's.
                let (ra, rb) = unsafe {
                    (
                        glob64(
                            pattern.as_ptr().cast(),
                            flags | GLOB_ALTDIRFUNC,
                            Some(on_error),
                            &raw mut a,
                        ),
                        glob(
                            pattern.as_ptr().cast(),
                            flags | GLOB_ALTDIRFUNC,
                            Some(on_error),
                            &raw mut b,
                        ),
                    )
                };
                assert_eq!(ra, rb, "{pattern:?} {flags:#x}");
                assert_eq!(names(&a), names(&b), "{pattern:?} {flags:#x}");
                assert_eq!(a.gl_flags, b.gl_flags);
                // SAFETY: glob_ts glob filled.
                unsafe {
                    globfree64(&raw mut a);
                    globfree(&raw mut b);
                }
                assert!(a.gl_pathv.is_null());
                assert_eq!(a.gl_pathc, 0);
            }
        }
        let mut g = GlobT::new();
        crate::errno::set_errno(0);
        // SAFETY: a NULL pattern, refused before anything is read.
        assert_eq!(
            unsafe { glob64(core::ptr::null(), 0, None, &raw mut g) },
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        // SAFETY: NULL is globfree's no-op.
        unsafe { globfree64(core::ptr::null_mut()) };
    }

    /// gl_offs so large the array cannot be sized is GLOB_NOSPACE, not an
    /// overflowed allocation.
    #[test]
    fn an_impossible_gl_offs_is_nospace() {
        for offs in [usize::MAX, usize::MAX / 8] {
            let mut g = tree_glob_t();
            g.gl_offs = offs;
            assert_eq!(run(&mut g, b"a", GLOB_DOOFFS), GLOB_NOSPACE);
            assert!(g.gl_pathv.is_null());
        }
    }

    // -- Beyond what the old one could hold ---------------------------------

    /// Thousands of names, all of them, sorted -- the glob this replaced
    /// kept 512 and dropped the rest -- and a path longer than 4096 bytes,
    /// which it could not build.
    #[test]
    fn many_names_and_long_paths() {
        let mut entries = vec![b"big/".to_vec()];
        for i in (0..3000).rev() {
            entries.push(format!("big/f{i:04}").into_bytes());
        }
        let long = [b'n'; 250];
        let mut deep = Vec::new();
        for _ in 0..20 {
            deep.extend_from_slice(&long);
            deep.push(b'/');
            entries.push(deep.clone());
        }
        let mut leaf = deep.clone();
        leaf.extend_from_slice(b"leaf");
        entries.push(leaf.clone());
        CURRENT.set(Some(Box::leak(Box::new(Tree::new(&entries)))));

        let mut g = tree_glob_t();
        assert_eq!(run(&mut g, b"big/f*", 0), 0);
        let got = names(&g);
        assert_eq!(got.len(), 3000);
        assert!(got.windows(2).all(|w| w[0] < w[1]), "sorted");
        assert_eq!(got[0], "big/f0000");
        assert_eq!(got[2999], "big/f2999");
        // SAFETY: what glob filled.
        unsafe { globfree(&raw mut g) };

        let mut pattern = Vec::new();
        for _ in 0..20 {
            pattern.extend_from_slice(b"n*/");
        }
        pattern.extend_from_slice(b"l?af");
        assert_eq!(run(&mut g, &pattern, 0), 0);
        let got = names(&g);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].as_bytes(), &leaf[..]);
        assert!(leaf.len() > 5000);
        // SAFETY: what glob filled.
        unsafe { globfree(&raw mut g) };
        CURRENT.set(None);
    }

    // -- The pieces ----------------------------------------------------------

    fn braces(p: &[u8], flags: i32) -> Vec<String> {
        let mut out = List::new();
        assert!(brace_expand(p, flags, &mut out).is_ok());
        out.as_slice()
            .iter()
            .map(|o| String::from_utf8(o.bytes().to_vec()).unwrap())
            .collect()
    }

    /// GLOB_BRACE: every group, nested ones inside out, in order; an
    /// unbalanced or escaped brace is itself.
    #[test]
    fn braces_expand_in_order() {
        assert_eq!(braces(b"a{b,c{d,e}}f", 0), ["abf", "acdf", "acef"]);
        assert_eq!(braces(b"{a,b}{c,d}", 0), ["ac", "ad", "bc", "bd"]);
        assert_eq!(braces(b"x{,y}", 0), ["x", "xy"]);
        assert_eq!(braces(b"{}", 0), [""]);
        assert_eq!(braces(b"{a,b", 0), ["{a,b"]);
        assert_eq!(braces(b"\\{a,b}", 0), ["\\{a,b}"]);
        assert_eq!(braces(b"{a\\,b,c}", 0), ["a\\,b", "c"]);
        // Without escapes, a backslash is a character like another.
        assert_eq!(braces(b"{a\\,b,c}", GLOB_NOESCAPE), ["a\\", "b", "c"]);
        assert_eq!(braces(b"plain", 0), ["plain"]);
    }

    /// `~` and `~user`: the home, with the rest after it; an unknown user
    /// is the pattern as it is -- or nothing, for GLOB_TILDE_CHECK.
    #[test]
    fn tilde_expands_homes() {
        let t = |p: &[u8], flags| {
            tilde(p, flags, &harness_home)
                .ok()
                .unwrap()
                .map(|(o, bare)| (String::from_utf8(o.bytes().to_vec()).unwrap(), bare))
        };
        assert_eq!(t(b"~", GLOB_TILDE), Some(("/home/u".to_owned(), true)));
        assert_eq!(t(b"~/x", GLOB_TILDE), Some(("/home/u/x".to_owned(), false)));
        assert_eq!(t(b"~root", GLOB_TILDE), Some(("/root".to_owned(), true)));
        assert_eq!(t(b"~who/x", GLOB_TILDE), Some(("~who/x".to_owned(), false)));
        assert_eq!(t(b"~who", GLOB_TILDE_CHECK), None);
        assert_eq!(t(b"~/x", 0), Some(("~/x".to_owned(), false)));
        assert_eq!(t(b"a~", GLOB_TILDE), Some(("a~".to_owned(), false)));
    }

    /// The pattern's escapes removed; not, with GLOB_NOESCAPE.
    #[test]
    fn unescape_removes_escapes() {
        let u = |p: &[u8], flags| unescape(p, flags).ok().unwrap().bytes().to_vec();
        assert_eq!(u(b"a\\*b\\\\c", 0), b"a*b\\c");
        assert_eq!(u(b"trailing\\", 0), b"trailing\\");
        assert_eq!(u(b"a\\*b", GLOB_NOESCAPE), b"a\\*b");
        assert!(needs_scan(b"a\\b", 0) && !needs_scan(b"a\\b", GLOB_NOESCAPE));
        assert!(has_magic(b"a[", 0) && !has_magic(b"a\\[", 0));
    }

    /// `append` moves every element over, in order.
    #[test]
    fn list_append_keeps_order() {
        let mut a = List::new();
        let mut b = List::new();
        for i in 0..3 {
            assert!(a.push(i).is_ok());
        }
        for i in 3..40 {
            assert!(b.push(i).is_ok());
        }
        assert!(a.append(b).is_ok());
        assert_eq!(a.as_slice(), (0..40).collect::<Vec<_>>());
    }

    // -- The C library's own directories --------------------------------------

    /// Without GLOB_ALTDIRFUNC -- or with it and no functions given -- the
    /// directories are the C library's to read. On the host there is no
    /// kernel behind its system calls (syscall.rs), so the current
    /// directory cannot be opened: glob tells the errfunc so, with the
    /// errno `opendir` gave, and stops for GLOB_ERR.
    #[test]
    fn the_c_librarys_directories_without_altdirfunc() {
        for flags in [0, GLOB_ALTDIRFUNC] {
            let mut g = GlobT::new();
            ERRLOG.with_borrow_mut(String::clear);
            STOP.set(false);
            // SAFETY: a C string and a valid glob_t.
            let r = unsafe { glob(c"*".as_ptr().cast(), flags, Some(on_error), &raw mut g) };
            assert_eq!(r, GLOB_NOMATCH);
            let told = ERRLOG.with_borrow(Clone::clone);
            let errno: i32 = told.strip_prefix(" .:").unwrap().parse().unwrap();
            assert!(errno > 0, "{told}");
            assert!(crate::dirent::opendir(c".".as_ptr().cast()).is_null());
            assert_eq!(crate::errno::get_errno(), errno, "opendir's own errno");
            // SAFETY: as above.
            let r = unsafe { glob(c"*".as_ptr().cast(), flags | GLOB_ERR, None, &raw mut g) };
            assert_eq!(r, GLOB_ABORTED);
        }
    }

    // -- ABI -------------------------------------------------------------------

    /// `glob_t` is glibc's: 72 bytes, each field where glibc's header puts
    /// it (check-libc-abi.py holds the overlay's <glob.h> to the same).
    #[test]
    fn glob_t_is_glibcs() {
        assert_eq!(size_of::<GlobT>(), 72);
        assert_eq!(align_of::<GlobT>(), 8);
        assert_eq!(core::mem::offset_of!(GlobT, gl_pathc), 0);
        assert_eq!(core::mem::offset_of!(GlobT, gl_pathv), 8);
        assert_eq!(core::mem::offset_of!(GlobT, gl_offs), 16);
        assert_eq!(core::mem::offset_of!(GlobT, gl_flags), 24);
        assert_eq!(core::mem::offset_of!(GlobT, gl_closedir), 32);
        assert_eq!(core::mem::offset_of!(GlobT, gl_readdir), 40);
        assert_eq!(core::mem::offset_of!(GlobT, gl_opendir), 48);
        assert_eq!(core::mem::offset_of!(GlobT, gl_lstat), 56);
        assert_eq!(core::mem::offset_of!(GlobT, gl_stat), 64);
    }

    /// The flags and returns are glibc's values, which are musl's.
    #[test]
    fn constants_are_glibcs() {
        let flags = [
            GLOB_ERR,
            GLOB_MARK,
            GLOB_NOSORT,
            GLOB_DOOFFS,
            GLOB_NOCHECK,
            GLOB_APPEND,
            GLOB_NOESCAPE,
            GLOB_PERIOD,
            GLOB_MAGCHAR,
            GLOB_ALTDIRFUNC,
            GLOB_BRACE,
            GLOB_NOMAGIC,
            GLOB_TILDE,
            GLOB_ONLYDIR,
            GLOB_TILDE_CHECK,
        ];
        for (bit, f) in flags.iter().enumerate() {
            assert_eq!(*f, 1 << bit);
        }
        assert_eq!(GLOB_FLAGS, 0x7fff & !GLOB_MAGCHAR);
        assert_eq!(
            (GLOB_NOSPACE, GLOB_ABORTED, GLOB_NOMATCH, GLOB_NOSYS),
            (1, 2, 3, 4)
        );
    }
}
