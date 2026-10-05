//! The old BSD and System V calls glibc still exports and declares, each as
//! glibc 2.39 answers it (`posix/tools/oracle/oldcalls_harness.py`,
//! `oldcalls_oracle.txt`) but where noted:
//!
//! | call | what it is here |
//! |---|---|
//! | `sigblock`, `sigsetmask`, `siggetmask` | 4.2BSD's signal masks, an `int` of signals 1 to 32 (signal n as bit n-1), over `sigprocmask` |
//! | `sigstack` | refused, `ENOSYS`: see [`sigstack`] |
//! | `sigreturn` | refused, `ENOSYS`, as glibc's |
//! | `gsignal`, `ssignal` | System V's names for `raise` and `signal` |
//! | `getwd` | 4.2BSD's `getcwd` into a buffer of `PATH_MAX` |
//! | `group_member` | whether a group is the process's own or one of its supplementary groups |
//! | `revoke`, `setlogin`, `gtty`, `stty` | refused, `ENOSYS`, as glibc's on Linux |
//! | `ttyslot` | 0: there is no `/etc/ttys` |
//! | `profil` | stopping answers 0; starting is `ENOSYS`, there being no profiling timer (`setitimer`'s `ITIMER_PROF`) to drive it |
//! | `getpw` | a `passwd` line of a user, into the caller's buffer |
//! | `isctype` | a character's classes as glibc's `<ctype.h>` masks (`_ISupper` ...) |
//! | `isfdtype` | whether a descriptor's file is of a type |
//! | `dysize` | the days in a year |
//! | `vlimit` | 4.2BSD's soft limit: `setrlimit` of the resource one below, `EINVAL` outside `LIM_CPU` to `LIM_MAXRSS` |
//! | `rpmatch` | an answer's verdict by the locale's `YESEXPR` and `NOEXPR`: 1 yes, 0 no, -1 neither |
//! | `execveat` | `execve` of a file named relative to a directory's descriptor, or of the descriptor itself |
//!
//! Written from the BSD, System V and Linux manuals; glibc run only for its
//! answers.

use crate::errno::{EBADF, EFAULT, EINVAL, ELOOP, ENOENT, ENOSYS, ENOTDIR, set_errno};
use crate::signal::{SIG_BLOCK, SIG_SETMASK, SighandlerT, SigsetT, sigprocmask};

/// -1, `errno` `e`.
fn fail(e: i32) -> i32 {
    set_errno(e);
    -1
}

// ---------------------------------------------------------------------------
// Signals
// ---------------------------------------------------------------------------

/// The signal set of a BSD mask: signal n for bit n-1, signals 1 to 32.
fn bsd_set(mask: i32) -> SigsetT {
    let mut set = SigsetT::EMPTY;
    set.bits[0] = u64::from(mask.cast_unsigned());
    set
}

/// The BSD mask of a signal set: its signals 1 to 32. Signal 32 is
/// `SIGRTMIN` here; glibc's keeps 32 and 33 for its threads and leaves bit
/// 31 clear.
fn bsd_mask(set: &SigsetT) -> i32 {
    ((set.bits[0] & 0xFFFF_FFFF) as u32).cast_signed()
}

/// `sigblock` (4.2BSD): add `mask`'s signals to those blocked, and return
/// those blocked before, as a mask. `sigprocmask`'s `SIG_BLOCK`; `SIGKILL`
/// and `SIGSTOP` are never blocked.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigblock(mask: i32) -> i32 {
    let set = bsd_set(mask);
    let mut old = SigsetT::EMPTY;
    if sigprocmask(SIG_BLOCK, &raw const set, &raw mut old) != 0 {
        return -1;
    }
    bsd_mask(&old)
}

/// `sigsetmask` (4.2BSD): block exactly `mask`'s signals -- every other,
/// the real-time ones past 32 among them, unblocked, as glibc's does -- and
/// return those blocked before, as a mask.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigsetmask(mask: i32) -> i32 {
    let set = bsd_set(mask);
    let mut old = SigsetT::EMPTY;
    if sigprocmask(SIG_SETMASK, &raw const set, &raw mut old) != 0 {
        return -1;
    }
    bsd_mask(&old)
}

/// `siggetmask` (4.2BSD): the signals blocked, as a mask.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn siggetmask() -> i32 {
    let mut old = SigsetT::EMPTY;
    if sigprocmask(SIG_BLOCK, core::ptr::null(), &raw mut old) != 0 {
        return -1;
    }
    bsd_mask(&old)
}

/// 4.2BSD's `struct sigstack`: a signal stack's pointer, and whether the
/// process is on it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Sigstack {
    /// The stack.
    pub ss_sp: *mut u8,
    /// Nonzero while a handler runs on it.
    pub ss_onstack: i32,
}

/// `sigstack` (4.2BSD): refused, `ENOSYS`. It names a signal stack by one
/// pointer and no size, and the kernel needs a size; glibc's answers 0 and
/// hands the kernel `MINSIGSTKSZ` bytes from the pointer up -- the stack's
/// top, by the old convention, so that signal frames land past the end of
/// the caller's buffer. `sigaltstack` is the call that works.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigstack(_ss: *const Sigstack, _oss: *mut Sigstack) -> i32 {
    fail(ENOSYS)
}

/// `sigreturn` (BSD): refused, `ENOSYS`, as glibc's is: a handler returns
/// through the restorer the kernel set up, never by this.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigreturn(_scp: *mut u8) -> i32 {
    fail(ENOSYS)
}

/// `gsignal` (System V): `raise`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gsignal(sig: i32) -> i32 {
    crate::signal::raise(sig)
}

/// `ssignal` (System V): `signal`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ssignal(sig: i32, handler: SighandlerT) -> SighandlerT {
    crate::signal::signal(sig, handler)
}

// ---------------------------------------------------------------------------
// Processes, users, terminals
// ---------------------------------------------------------------------------

/// `getwd` (4.2BSD): the working directory into `buf`, which must hold
/// `PATH_MAX` bytes, and `buf`; or NULL with a message saying why in `buf`
/// -- `strerror`'s text -- as BSD specifies. A NULL `buf` is NULL, `EINVAL`.
///
/// # Safety
///
/// `buf` is NULL or `PATH_MAX` writable bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getwd(buf: *mut u8) -> *mut u8 {
    if buf.is_null() {
        set_errno(EINVAL);
        return core::ptr::null_mut();
    }
    let max = crate::unistd::PATH_MAX;
    if !crate::unistd::copy_cwd(buf, max).is_null() {
        return buf;
    }
    let e = crate::errno::get_errno();
    // SAFETY: strerror's text is a NUL-terminated string.
    let text = unsafe { core::ffi::CStr::from_ptr(crate::string::strerror(e).cast()) }.to_bytes();
    let n = text.len().min(max.saturating_sub(1));
    // SAFETY: `buf` holds `max` bytes; `n + 1 <= max`.
    unsafe {
        core::ptr::copy_nonoverlapping(text.as_ptr(), buf, n);
        buf.add(n).write(0);
    }
    set_errno(e);
    core::ptr::null_mut()
}

/// Own archive member -- gnulib's `group-member` module defines
/// `group_member` where the C library lacks it, as musl does. See string.rs's
/// module header.
mod gnu_group_member {
    use crate::types::GidT;

    /// `group_member` (GNU): 1 if `gid` is the process's effective group or
    /// one of its supplementary groups, else 0.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn group_member(gid: GidT) -> i32 {
        if gid == crate::unistd::getegid() {
            return 1;
        }
        let mut groups = [0 as GidT; 64];
        let n = crate::unistd::getgroups(groups.len() as i32, groups.as_mut_ptr());
        let n = usize::try_from(n).unwrap_or(0).min(groups.len());
        i32::from(groups.get(..n).is_some_and(|g| g.contains(&gid)))
    }
}
pub use gnu_group_member::group_member;

/// `revoke` (4.4BSD): refused, `ENOSYS`, as glibc's is on Linux -- there is
/// no call to take a file away from the processes that have it open.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn revoke(_file: *const u8) -> i32 {
    fail(ENOSYS)
}

/// `setlogin` (4.4BSD): refused, `ENOSYS`, as glibc's is on Linux -- a
/// session has no login name to set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setlogin(_name: *const u8) -> i32 {
    fail(ENOSYS)
}

/// `ttyslot` (System V): the calling process's terminal's line in
/// `/etc/ttys`, and 0 when it has none there -- always, there being no
/// `/etc/ttys`, as glibc answers without one.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ttyslot() -> i32 {
    0
}

/// `profil` (4.2BSD): a NULL buffer or a scale of 0 stops profiling, which
/// is never running, and is 0; asking for it to start is `ENOSYS`, there
/// being no profiling timer to drive it (`setitimer`'s `ITIMER_PROF` is
/// refused the same way, design-decisions §1004).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn profil(buf: *mut u16, _bufsiz: usize, _offset: usize, scale: u32) -> i32 {
    if buf.is_null() || scale == 0 {
        return 0;
    }
    fail(ENOSYS)
}

/// `gtty` (Version 7): refused, `ENOSYS`, as glibc's is; `tcgetattr` is the
/// call that works.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gtty(_fd: i32, _params: *mut u8) -> i32 {
    fail(ENOSYS)
}

/// `stty` (Version 7): refused, `ENOSYS`, as glibc's is; `tcsetattr` is the
/// call that works.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn stty(_fd: i32, _params: *const u8) -> i32 {
    fail(ENOSYS)
}

/// `getpw` (Version 7): user `uid`'s `/etc/passwd` line --
/// `name:password:uid:gid:gecos:dir:shell` -- into `buf`, and 0; -1 for no
/// such user (`errno` as `getpwuid` left it, as glibc's does), or `EINVAL`
/// for a NULL `buf`.
///
/// # Safety
///
/// `buf` is NULL or has room for the line: there is no size, as in
/// Version 7; `getpwuid_r` is the call that takes one.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getpw(uid: u32, buf: *mut u8) -> i32 {
    if buf.is_null() {
        return fail(EINVAL);
    }
    let pw = crate::pwd::getpwuid(uid);
    if pw.is_null() {
        return -1;
    }
    // SAFETY: getpwuid's entry, its strings NUL-terminated or NULL.
    let (pw, text) = unsafe {
        let pw = &*pw;
        let text = |p: *const u8| {
            if p.is_null() {
                &[][..]
            } else {
                core::ffi::CStr::from_ptr(p.cast()).to_bytes()
            }
        };
        (
            pw,
            [
                text(pw.pw_name),
                text(pw.pw_passwd),
                text(pw.pw_gecos),
                text(pw.pw_dir),
                text(pw.pw_shell),
            ],
        )
    };
    let [name, passwd, gecos, dir, shell] = text;
    let mut at: usize = 0;
    let mut put = |bytes: &[u8]| {
        for &b in bytes {
            // SAFETY: room for the line, by the contract.
            unsafe { buf.add(at).write(b) };
            at = at.wrapping_add(1);
        }
    };
    let mut digits = [0u8; 10];
    put(name);
    put(b":");
    put(passwd);
    put(b":");
    put(decimal(pw.pw_uid, &mut digits));
    put(b":");
    put(decimal(pw.pw_gid, &mut digits));
    put(b":");
    put(gecos);
    put(b":");
    put(dir);
    put(b":");
    put(shell);
    put(b"\0");
    0
}

/// `n` in decimal, in `digits`.
fn decimal(mut n: u32, digits: &mut [u8; 10]) -> &[u8] {
    let mut at = digits.len();
    loop {
        at = at.saturating_sub(1);
        if let Some(d) = digits.get_mut(at) {
            *d = b'0'.wrapping_add((n % 10) as u8);
        }
        n /= 10;
        if n == 0 || at == 0 {
            break;
        }
    }
    digits.get(at..).unwrap_or(&[])
}

// ---------------------------------------------------------------------------
// Characters, files, dates
// ---------------------------------------------------------------------------

/// glibc's `<ctype.h>` class bits, the ones `isctype`'s `mask` is made of:
/// on a little-endian machine a class's bit is its number past 8, or before
/// it for the four from 8 up.
#[allow(non_upper_case_globals)] // glibc's names, as the header spells them
pub mod class {
    pub const _ISupper: i32 = 0x100;
    pub const _ISlower: i32 = 0x200;
    pub const _ISalpha: i32 = 0x400;
    pub const _ISdigit: i32 = 0x800;
    pub const _ISxdigit: i32 = 0x1000;
    pub const _ISspace: i32 = 0x2000;
    pub const _ISprint: i32 = 0x4000;
    pub const _ISgraph: i32 = 0x8000;
    pub const _ISblank: i32 = 0x1;
    pub const _IScntrl: i32 = 0x2;
    pub const _ISpunct: i32 = 0x4;
    pub const _ISalnum: i32 = 0x8;
}

/// `isctype` (GNU): `c`'s classes that are in `mask` -- nonzero if it is of
/// any of them -- by glibc's class bits ([`class`]), in the C locale this
/// library's `<ctype.h>` has. A `c` that is neither a byte nor `EOF` has
/// none, where glibc's reads outside its table.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isctype(c: i32, mask: i32) -> i32 {
    if !(0..=255).contains(&c) {
        return 0;
    }
    use crate::ctype::*;
    let classes = [
        (isupper(c), class::_ISupper),
        (islower(c), class::_ISlower),
        (isalpha(c), class::_ISalpha),
        (isdigit(c), class::_ISdigit),
        (isxdigit(c), class::_ISxdigit),
        (isspace(c), class::_ISspace),
        (isprint(c), class::_ISprint),
        (isgraph(c), class::_ISgraph),
        (isblank(c), class::_ISblank),
        (iscntrl(c), class::_IScntrl),
        (ispunct(c), class::_ISpunct),
        (isalnum(c), class::_ISalnum),
    ]
    .iter()
    .filter(|(is, _)| *is != 0)
    .fold(0, |m, (_, bit)| m | bit);
    classes & mask
}

/// `isfdtype` (BSD): 1 if `fd`'s file is of type `fdtype` (`S_IFREG`,
/// `S_IFDIR` ...), 0 if not; -1 for a descriptor `fstat` refuses, with its
/// `errno` -- which glibc's puts back as it was, where its manual says it
/// is set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isfdtype(fd: i32, fdtype: i32) -> i32 {
    // SAFETY: an all-zero `struct stat` is a valid one.
    let mut st: crate::stat::Stat = unsafe { core::mem::zeroed() };
    if crate::file::fstat(fd, &raw mut st) != 0 {
        return -1;
    }
    i32::from(st.st_mode & crate::fcntl::S_IFMT == fdtype.cast_unsigned())
}

/// `dysize` (BSD): the days in `year`, 365 or 366, by the Gregorian rule --
/// proleptically, for a year before 1582 or before 1 as well.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dysize(year: i32) -> i32 {
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    if leap { 366 } else { 365 }
}

// ---------------------------------------------------------------------------
// Limits, and answers
// ---------------------------------------------------------------------------

/// `<sys/vlimit.h>`'s first and last resource that is one: `LIM_CPU` and
/// `LIM_MAXRSS`, each one past its `RLIMIT_` (`LIM_NORAISE`, 0, is none).
const LIM_CPU: i32 = 1;
const LIM_MAXRSS: i32 = 6;

/// `vlimit(resource, value)` (4.2BSD): set `resource`'s soft limit to
/// `value` -- `setrlimit` of the resource one below it, the hard limit as it
/// was -- as glibc's does; `EINVAL` for `LIM_NORAISE` and anything outside
/// `LIM_CPU` to `LIM_MAXRSS`. A negative `value` is the `rlim_t` it converts
/// to, so -1 is `RLIM_INFINITY`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn vlimit(resource: i32, value: i32) -> i32 {
    if !(LIM_CPU..=LIM_MAXRSS).contains(&resource) {
        return fail(EINVAL);
    }
    let which = resource.saturating_sub(1);
    let mut lim = crate::resource::Rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if crate::resource::getrlimit(which, &raw mut lim) < 0 {
        return -1;
    }
    // C's conversion of an `int` to `rlim_t`: sign-extended.
    lim.rlim_cur = i64::from(value).cast_unsigned();
    crate::resource::setrlimit(which, &raw const lim)
}

/// Own archive member -- gnulib's `rpmatch` module defines it wherever the C
/// library lacks it, as musl does. See string.rs's module header.
mod gnu_rpmatch {
    /// `rpmatch(response)` (glibc): 1 if `response` is a yes by the
    /// locale's `YESEXPR`, else 0 if a no by its `NOEXPR`, else -1 --
    /// each an extended regular expression, compiled for the call (glibc
    /// keeps them; this is safe to call from two threads at once). -1 as
    /// well when `YESEXPR` does not compile, as glibc's; a `NOEXPR` that
    /// does not is no answer.
    ///
    /// # Safety
    ///
    /// `response` must be a C string.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn rpmatch(response: *const u8) -> i32 {
        // SAFETY: the caller's C string.
        match unsafe { matches(response, crate::langinfo::YESEXPR) } {
            Some(true) => 1,
            None => -1,
            // SAFETY: as above.
            Some(false) => match unsafe { matches(response, crate::langinfo::NOEXPR) } {
                Some(true) => 0,
                _ => -1,
            },
        }
    }

    /// Does `response` match the locale's expression `item`? `None` when it
    /// does not compile.
    ///
    /// # Safety
    ///
    /// `response` must be a C string.
    unsafe fn matches(response: *const u8, item: i32) -> Option<bool> {
        let pattern = crate::langinfo::nl_langinfo(item);
        let mut re = crate::regex::RegexT::new();
        // SAFETY: a regex_t of this frame's and a C string, the locale's.
        if unsafe { crate::regex::regcomp(&raw mut re, pattern, crate::regex::REG_EXTENDED) } != 0 {
            return None;
        }
        // SAFETY: the compiled expression and the caller's C string; no
        // match offsets are asked for.
        let hit =
            unsafe { crate::regex::regexec(&raw const re, response, 0, core::ptr::null_mut(), 0) }
                == 0;
        // SAFETY: compiled above, freed once.
        unsafe { crate::regex::regfree(&raw mut re) };
        Some(hit)
    }
}
pub use gnu_rpmatch::rpmatch;

// ---------------------------------------------------------------------------
// execveat
// ---------------------------------------------------------------------------

/// `execveat` (Linux 3.19): `execve` of the file `pathname` names --
/// absolute, or relative to the directory `dirfd` is open on (the working
/// directory for `AT_FDCWD`) -- or, with `AT_EMPTY_PATH` and an empty
/// `pathname`, of the file `dirfd` itself, as `fexecve`. With
/// `AT_SYMLINK_NOFOLLOW` a `pathname` naming a symbolic link is `ELOOP`.
/// Returns only on failure: -1, `EINVAL` for another flag, `EBADF` for a
/// `dirfd` that is not open, `ENOTDIR` for one that is not a directory,
/// `ENOENT` for an empty `pathname` without `AT_EMPTY_PATH`, or `execve`'s.
///
/// # Safety
///
/// `pathname` is NULL or NUL-terminated; `argv` and `envp` as for `execve`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn execveat(
    dirfd: i32,
    pathname: *const u8,
    argv: *const *const u8,
    envp: *const *const u8,
    flags: i32,
) -> i32 {
    use crate::file::{AT_EMPTY_PATH, AT_FDCWD, AT_SYMLINK_NOFOLLOW};
    if flags & !(AT_EMPTY_PATH | AT_SYMLINK_NOFOLLOW) != 0 {
        return fail(EINVAL);
    }
    if pathname.is_null() {
        return fail(EFAULT);
    }
    // SAFETY: the caller's NUL-terminated path.
    let path = unsafe { core::ffi::CStr::from_ptr(pathname.cast()) }.to_bytes();
    if path.is_empty() {
        if flags & AT_EMPTY_PATH == 0 {
            return fail(ENOENT);
        }
        return crate::spawn::fexecve(dirfd, argv, envp);
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let full_len = if path.first() == Some(&b'/') || dirfd == AT_FDCWD {
        // An absolute path ignores `dirfd`, as Linux's does; a relative
        // one with AT_FDCWD is the working directory's.
        path.len()
    } else {
        // SAFETY: an all-zero `struct stat` is a valid one.
        let mut st: crate::stat::Stat = unsafe { core::mem::zeroed() };
        if crate::file::fstat(dirfd, &raw mut st) != 0 {
            return fail(EBADF);
        }
        if st.st_mode & crate::fcntl::S_IFMT != crate::fcntl::S_IFDIR {
            return fail(ENOTDIR);
        }
        let dir = crate::fdtable::get_fd_path(dirfd, &mut full);
        if dir == 0 {
            // A directory this library opened without keeping its name.
            return fail(EBADF);
        }
        let Some(slot) = full.get_mut(dir) else {
            return fail(crate::errno::ENAMETOOLONG);
        };
        *slot = b'/';
        dir.saturating_add(1).saturating_add(path.len())
    };
    // The whole path, NUL-terminated, in `full`.
    let start = full_len.saturating_sub(path.len());
    let Some(dst) = full.get_mut(start..full_len.saturating_add(1)) else {
        return fail(crate::errno::ENAMETOOLONG);
    };
    let (body, nul) = dst.split_at_mut(path.len());
    body.copy_from_slice(path);
    if let Some(n) = nul.first_mut() {
        *n = 0;
    }
    if flags & AT_SYMLINK_NOFOLLOW != 0 {
        // SAFETY: an all-zero `struct stat` is a valid one.
        let mut st: crate::stat::Stat = unsafe { core::mem::zeroed() };
        if crate::file::lstat(full.as_ptr(), &raw mut st) == 0
            && st.st_mode & crate::fcntl::S_IFMT == crate::fcntl::S_IFLNK
        {
            return fail(ELOOP);
        }
    }
    crate::spawn::execve(full.as_ptr(), argv, envp)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// glibc 2.39's answers (`posix/tools/oracle/oldcalls_harness.py`).
    const ORACLE: &str = include_str!("oldcalls_oracle.txt");

    fn oracle(name: &str) -> &'static str {
        ORACLE
            .lines()
            .find_map(|l| l.strip_prefix(name)?.strip_prefix(" ="))
            .unwrap_or_else(|| panic!("no `{name}` in the oracle"))
    }

    /// `errno`'s name as the harness prints it.
    fn en(e: i32) -> &'static str {
        match e {
            0 => "0",
            EINVAL => "EINVAL",
            ENOSYS => "ENOSYS",
            ENOENT => "ENOENT",
            EBADF => "EBADF",
            ENOTDIR => "ENOTDIR",
            ELOOP => "ELOOP",
            _ => "other",
        }
    }

    /// `n` as C's `%#x` prints it: `0` for zero, `0x...` for the rest.
    fn cx(n: i32) -> String {
        if n == 0 {
            "0".to_string()
        } else {
            format!("{n:#x}")
        }
    }

    /// A result as the harness's RC prints it.
    fn rc(r: i64) -> String {
        if r == -1 {
            format!(" {r}!{}", en(crate::errno::get_errno()))
        } else {
            format!(" {r}")
        }
    }

    /// Run `f` from an empty signal mask, and put the mask back. The mask
    /// and the handlers are the test thread's own on the host
    /// (`process_global!`), so no other test sees them change.
    fn with_mask(f: impl FnOnce()) {
        let before = siggetmask();
        sigsetmask(0);
        f();
        sigsetmask(before);
    }

    #[test]
    fn the_bsd_masks_are_glibcs() {
        use crate::signal::{SIGHUP, SIGINT, SIGKILL, SIGSTOP, SIGTERM, SIGUSR1, SIGUSR2};
        let m = |s: i32| 1i32 << (s - 1);
        with_mask(|| {
            assert_eq!(format!(" {}", siggetmask()), oracle("siggetmask-initial"));
            let a = sigblock(m(SIGUSR1) | m(SIGINT));
            let b = siggetmask();
            let c = sigblock(m(SIGUSR2));
            let d = siggetmask();
            assert_eq!(
                format!(" {a} {} {} {}", cx(b), cx(c), cx(d)),
                oracle("sigblock")
            );
        });
        with_mask(|| {
            let a = sigsetmask(m(SIGHUP) | m(SIGTERM));
            let b = sigsetmask(0);
            let c = siggetmask();
            assert_eq!(
                format!(" {} {} {}", cx(a), cx(b), cx(c)),
                oracle("sigsetmask")
            );
        });
        with_mask(|| {
            sigblock(m(SIGKILL) | m(SIGSTOP) | m(SIGUSR1));
            assert_eq!(
                format!(" {}", cx(siggetmask())),
                oracle("sigblock-kill-stop")
            );
        });
        with_mask(|| {
            // Every signal but KILL and STOP -- and 32, SIGRTMIN here, where
            // glibc keeps 32 for its threads: bit 31 is the one difference.
            sigblock(!0);
            let glibc = u32::from_str_radix(
                oracle("sigblock-all")
                    .split(' ')
                    .nth(1)
                    .unwrap()
                    .trim_start_matches("0x"),
                16,
            )
            .unwrap();
            assert_eq!(siggetmask().cast_unsigned(), glibc | 1 << 31);
        });
        with_mask(|| {
            // sigsetmask replaces the whole mask: a real-time signal past 32
            // blocked before is not after, as glibc's.
            let mut set = SigsetT::EMPTY;
            set.bits[0] = 1 << (crate::signal::SIGRTMIN + 1 - 1);
            sigprocmask(SIG_BLOCK, &raw const set, core::ptr::null_mut());
            sigsetmask(m(SIGUSR1));
            let mut now = SigsetT::EMPTY;
            sigprocmask(SIG_BLOCK, core::ptr::null(), &raw mut now);
            let rt = now.bits[0] >> crate::signal::SIGRTMIN & 1;
            let usr1 = now.bits[0] >> (SIGUSR1 - 1) & 1;
            assert_eq!(format!(" rt{rt} usr1{usr1}"), oracle("sigsetmask-keeps-rt"));
        });
    }

    #[test]
    fn the_refusals_are_glibcs() {
        crate::errno::set_errno(0);
        assert_eq!(
            rc(i64::from(sigreturn(core::ptr::null_mut()))),
            oracle("sigreturn")
        );
        assert_eq!(
            rc(i64::from(revoke(b"/dev/null\0".as_ptr()))),
            oracle("revoke")
        );
        assert_eq!(
            rc(i64::from(setlogin(b"nobody\0".as_ptr()))),
            oracle("setlogin")
        );
        assert_eq!(rc(i64::from(ttyslot())), oracle("ttyslot"));
        assert_eq!(
            rc(i64::from(profil(core::ptr::null_mut(), 0, 0, 0))),
            oracle("profil-off")
        );
        let mut b = [0u8; 64];
        assert_eq!(rc(i64::from(gtty(0, b.as_mut_ptr()))), oracle("gtty"));
        assert_eq!(rc(i64::from(stty(0, b.as_ptr()))), oracle("stty"));
        assert_eq!(rc(i64::from(gtty(-1, b.as_mut_ptr()))), oracle("gtty-badf"));
        // Starting profiling: no timer to drive it.
        let mut counts = [0u16; 16];
        assert_eq!(profil(counts.as_mut_ptr(), 32, 0, 0x10000), -1);
        assert_eq!(crate::errno::get_errno(), ENOSYS);
        // sigstack: refused, where glibc's answers 0 (the module's reasons).
        assert_eq!(oracle("sigstack"), " 0 0 onstack=1");
        let mut old = Sigstack {
            ss_sp: core::ptr::null_mut(),
            ss_onstack: 0,
        };
        assert_eq!(sigstack(core::ptr::null(), &raw mut old), -1);
        assert_eq!(crate::errno::get_errno(), ENOSYS);
    }

    #[test]
    fn gsignal_and_ssignal_are_raise_and_signal() {
        use crate::signal::{SIG_DFL, SIG_ERR, SIG_IGN, SIGKILL, SIGUSR1};
        let before = ssignal(SIGUSR1, SIG_IGN);
        let mut got = rc(i64::from(gsignal(0)));
        got += &rc(i64::from(gsignal(SIGUSR1)));
        got += &rc(i64::from(gsignal(-1)));
        got += &rc(i64::from(gsignal(65)));
        assert_eq!(got, oracle("gsignal"));
        extern "C" fn handler(_: i32) {}
        let h = handler as *const () as usize;
        ssignal(SIGUSR1, SIG_DFL);
        let a = ssignal(SIGUSR1, h);
        let b = ssignal(SIGUSR1, SIG_DFL);
        crate::errno::set_errno(0);
        let c = ssignal(SIGKILL, h);
        let ce = en(crate::errno::get_errno());
        crate::errno::set_errno(0);
        let d = ssignal(0, h);
        let de = en(crate::errno::get_errno());
        let got = format!(
            " {} {} {}!{ce} {}!{de}",
            i32::from(a == SIG_DFL),
            i32::from(b == h),
            i32::from(c == SIG_ERR),
            i32::from(d == SIG_ERR)
        );
        assert_eq!(got, oracle("ssignal"));
        ssignal(SIGUSR1, before);
    }

    #[test]
    fn getwd_is_getcwd() {
        let mut buf = vec![0u8; crate::unistd::PATH_MAX];
        let mut cwd = vec![0u8; crate::unistd::PATH_MAX];
        let r = unsafe { getwd(buf.as_mut_ptr()) };
        assert_eq!(r, buf.as_mut_ptr());
        assert!(!crate::unistd::getcwd(cwd.as_mut_ptr(), cwd.len()).is_null());
        let n = buf.iter().position(|&b| b == 0).unwrap();
        assert_eq!(buf[..=n], cwd[..=n]);
        crate::errno::set_errno(0);
        let r = unsafe { getwd(core::ptr::null_mut()) };
        assert_eq!(
            format!(
                " {}!{}",
                i32::from(r.is_null()),
                en(crate::errno::get_errno())
            ),
            oracle("getwd-null")
        );
    }

    #[test]
    fn group_member_knows_the_process_groups() {
        assert_eq!(group_member(crate::unistd::getegid()), 1);
        assert_eq!(group_member(987_654), 0);
        let mut groups = [0u32; 64];
        let n = crate::unistd::getgroups(64, groups.as_mut_ptr());
        for &g in &groups[..usize::try_from(n).unwrap()] {
            assert_eq!(group_member(g), 1);
        }
    }

    #[test]
    fn getpw_is_the_passwd_line() {
        let mut buf = [0u8; 1024];
        crate::errno::set_errno(0);
        assert_eq!(
            rc(i64::from(unsafe { getpw(0, core::ptr::null_mut()) })),
            oracle("getpw-null")
        );
        // A user there is not: -1, errno as getpwuid left it.
        assert_eq!(unsafe { getpw(987_654, buf.as_mut_ptr()) }, -1);
        let pw = crate::pwd::getpwuid(0);
        if pw.is_null() {
            return; // no user 0 in this test environment's database
        }
        assert_eq!(unsafe { getpw(0, buf.as_mut_ptr()) }, 0);
        let line = core::ffi::CStr::from_bytes_until_nul(&buf)
            .unwrap()
            .to_bytes();
        let fields: Vec<&[u8]> = line.split(|&b| b == b':').collect();
        assert_eq!(fields.len(), 7, "{line:?}");
        assert_eq!(fields[2], b"0");
        let name = unsafe { core::ffi::CStr::from_ptr((*pw).pw_name.cast()) }.to_bytes();
        assert_eq!(fields[0], name);
    }

    #[test]
    fn isctype_is_glibcs() {
        use class::*;
        let masks = [
            _ISupper, _ISlower, _ISalpha, _ISdigit, _ISxdigit, _ISspace, _ISprint, _ISgraph,
            _ISblank, _IScntrl, _ISpunct, _ISalnum,
        ];
        let got = format!(
            " upper={_ISupper:#x} lower={_ISlower:#x} alpha={_ISalpha:#x} digit={_ISdigit:#x} xdigit={_ISxdigit:#x} \
             space={_ISspace:#x} print={_ISprint:#x} graph={_ISgraph:#x} blank={_ISblank:#x} cntrl={_IScntrl:#x} \
             punct={_ISpunct:#x} alnum={_ISalnum:#x}"
        );
        assert_eq!(got, oracle("isctype-masks"));
        let mut got = String::new();
        for c in [65, 122, 53, 102, 32, 9, 10, 33, 127, 0, 0xE9, -1, 200] {
            got += &format!(" {c}:");
            for m in masks {
                got += if isctype(c, m) != 0 { "1" } else { "0" };
            }
        }
        assert_eq!(got, oracle("isctype"));
        assert_eq!(
            isctype(b'A'.into(), _ISupper | _ISdigit),
            _ISupper,
            "the classes in the mask"
        );
        assert_eq!(isctype(1000, !0), 0);
        assert_eq!(isctype(-129, !0), 0);
    }

    #[test]
    fn isfdtype_is_fstats() {
        crate::errno::set_errno(0);
        assert_eq!(isfdtype(-1, crate::fcntl::S_IFREG.cast_signed()), -1);
        // glibc's puts errno back; this leaves fstat's.
        assert_eq!(oracle("isfdtype").rsplit(' ').next(), Some("-1!0"));
        assert_eq!(crate::errno::get_errno(), EBADF);
    }

    #[test]
    fn dysize_is_glibcs() {
        let got: String = [1900, 1996, 1999, 2000, 2023, 2024, 2100, 2400, 0, -4, -100]
            .iter()
            .map(|&y| format!(" {}", dysize(y)))
            .collect::<Vec<_>>()
            .concat();
        assert_eq!(got, oracle("dysize"));
    }

    #[test]
    fn vlimit_is_glibcs() {
        use crate::resource::{RLIMIT_CPU, RLIMIT_FSIZE, Rlimit, getrlimit};
        let soft = |r| {
            let mut l = Rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            assert_eq!(getrlimit(r, &raw mut l), 0);
            format!(" {}", l.rlim_cur.cast_signed())
        };
        let rc = |r: i32| {
            if r == -1 {
                format!(" -1!{}", en(crate::errno::get_errno()))
            } else {
                format!(" {r}")
            }
        };
        let mut got = String::new();
        got += &rc(vlimit(LIM_CPU, 100));
        got += &soft(RLIMIT_CPU);
        got += &rc(vlimit(2, 4096));
        got += &soft(RLIMIT_FSIZE);
        got += &rc(vlimit(LIM_CPU, -1));
        got += &soft(RLIMIT_CPU);
        got += &rc(vlimit(0, 0));
        got += &rc(vlimit(LIM_MAXRSS + 1, 0));
        got += &rc(vlimit(-1, 0));
        assert_eq!(got, oracle("vlimit"));
    }

    #[test]
    fn rpmatch_is_glibcs() {
        let rs: [&[u8]; 12] = [
            b"y\0", b"Y\0", b"yes\0", b"n\0", b"N\0", b"no\0", b"x\0", b"\0", b" y\0", b"yn\0",
            b"ny\0", b"\xff\0",
        ];
        let mut got = String::new();
        for r in rs {
            // SAFETY: a C string.
            got += &format!(" {}", unsafe { rpmatch(r.as_ptr()) });
        }
        assert_eq!(got, oracle("rpmatch"));
    }

    #[test]
    fn execveat_refuses_as_glibcs() {
        let argv = [b"x\0".as_ptr(), core::ptr::null()];
        let call = |dirfd: i32, path: &[u8], flags: i32| {
            crate::errno::set_errno(0);
            let r = unsafe {
                execveat(
                    dirfd,
                    path.as_ptr(),
                    argv.as_ptr(),
                    core::ptr::null(),
                    flags,
                )
            };
            rc(i64::from(r))
        };
        assert_eq!(call(-5, b"x\0", 0), oracle("execveat-badf"));
        assert_eq!(
            call(crate::file::AT_FDCWD, b"/bin/true\0", 0x40000),
            oracle("execveat-flags")
        );
        assert_eq!(
            call(crate::file::AT_FDCWD, b"\0", 0),
            oracle("execveat-empty-no-flag")
        );
        assert_eq!(
            unsafe { execveat(0, core::ptr::null(), argv.as_ptr(), core::ptr::null(), 0) },
            -1
        );
        assert_eq!(crate::errno::get_errno(), EFAULT);
    }
}
