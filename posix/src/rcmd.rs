// The arithmetic in this file is on values bounded by what they count --
// a port in 512..=1023, a retry delay of at most 32 seconds, a buffer's
// length within its block, a decimal digit -- and clippy cannot see the
// bounds.
#![allow(clippy::arithmetic_side_effects)]
//! The BSD remote-execution calls of `<netdb.h>` -- `rcmd`, `rcmd_af`,
//! `rresvport`, `rresvport_af`, `ruserok`, `ruserok_af`, `iruserok`,
//! `iruserok_af`, `rexec` and `rexec_af` -- glibc 2.39's (`inet/rcmd.c`,
//! `inet/rexec.c`, `inet/ruserpass.c`), as glibc's, with the three variables
//! glibc exports beside them and declares in no header: `__rcmd_errstr`,
//! `__check_rhosts_file` and `rexecoptions`.
//!
//! - **`rresvport_af`** binds a stream socket to a port below 1024, from
//!   `*alport` down -- a start out of the range moved to its nearer end, 512
//!   wrapping to 1023 -- past those in use: `EAGAIN` when none is free, else
//!   the bind's own error (`EACCES` without the privilege).
//! - **`rcmd_af`** is `rsh`'s client: the host looked up, its canonical name
//!   left in `*ahost`; connected from a reserved port, each address tried in
//!   turn and a refused connection tried again after 1, 2, 4, 8 and 16
//!   seconds; the stderr channel's port sent, and its connection taken only
//!   from a reserved port; the two users and the command sent, and the
//!   server's answer read: a NUL, or a message copied to standard error. What
//!   goes wrong is said on standard error, in glibc's words.
//! - **`rexec_af`** is `rexec`'s client: the first address only, from any
//!   port, sending a name and password -- from `~/.netrc` when not given
//!   (glibc's `ruserpass`, kept internal here: glibc exports it but no header
//!   declares it, and the programs that use one bring their own).
//! - **`ruserok_af`** and **`iruserok_af`** say whether a remote user on a
//!   host may log in as a local one: by `/etc/hosts.equiv` (not for the
//!   superuser), then by the local user's `~/.rhosts`, read as that user and
//!   only if it is a regular file, owned by root or the user, written by no
//!   one else and linked once -- why one was refused is left in
//!   `__rcmd_errstr`. A line is a host and a user, each a name, `+`, `-name`,
//!   `+@netgroup` or `-@netgroup`; the first line to settle the question
//!   decides it, and a line that begins with a blank ends the file.
//!
//! Where glibc's would crash or overflow, this does neither. `rexec` with no
//! name or no password, given or in `~/.netrc`, fails with `EINVAL`, where
//! glibc's takes the length of the NULL. A `~/.netrc` password line seen
//! before any login, when no name was given, is taken as not `anonymous`'s,
//! where glibc's compares the NULL. And a `~/.netrc` word of more than 99
//! bytes keeps its first 99, where glibc's writes past its buffer.
//!
//! Each call reaches the system through `sys`, which the host tests replace
//! with a world of their own (`rcmd/world.rs`): a scripted server, the files,
//! standard error. The replay of glibc's answers, `rcmd_oracle.txt`, drives
//! it.

use crate::errno;
use crate::gai::{NI_NUMERICHOST, NI_NUMERICSERV};
use crate::poll::{POLLIN, Pollfd};
use crate::socket::{
    AF_INET, AF_INET6, AF_UNSPEC, AI_CANONNAME, Addrinfo, EAI_NONAME, SOCK_STREAM, Sockaddr,
    SocklenT,
};

/// `sa_family_t`.
type SaFamilyT = u16;

/// `IPPORT_RESERVED`: binding a port below it takes the privilege.
const IPPORT_RESERVED: i32 = 1024;

/// glibc's `_PATH_HEQUIV`.
const PATH_HEQUIV: &[u8] = b"/etc/hosts.equiv\0";

/// glibc's `NSS_BUFLEN_PASSWD`, what `sysconf (_SC_GETPW_R_SIZE_MAX)`
/// answers there: `ruserok`'s buffer for the local user's entry.
const PW_BUFLEN: usize = 1024;

/// `INET6_ADDRSTRLEN`: room for any numeric address and its NUL.
const ADDRSTRLEN: usize = 46;

/// A socket address big enough for any family: `struct sockaddr_storage`.
#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct Storage([u8; 128]);

impl Storage {
    const ZERO: Self = Self([0; 128]);

    fn family(&self) -> i32 {
        i32::from(u16::from_ne_bytes([self.0[0], self.0[1]]))
    }

    fn set_family(&mut self, family: i32) {
        let f = u16::try_from(family).unwrap_or(0).to_ne_bytes();
        self.0[0] = f[0];
        self.0[1] = f[1];
    }

    /// The port, host order: `sin_port` and `sin6_port` are both at 2.
    fn port(&self) -> u16 {
        u16::from_be_bytes([self.0[2], self.0[3]])
    }

    fn set_port(&mut self, port: u16) {
        let p = port.to_be_bytes();
        self.0[2] = p[0];
        self.0[3] = p[1];
    }

    fn as_sockaddr(&self) -> *const Sockaddr {
        self.0.as_ptr().cast()
    }

    #[cfg(not(test))]
    fn as_sockaddr_mut(&mut self) -> *mut Sockaddr {
        self.0.as_mut_ptr().cast()
    }
}

/// `SA_LEN`: the size of an address of `family`.
fn sa_len(family: i32) -> SocklenT {
    let len = match family {
        AF_INET => core::mem::size_of::<crate::socket::SockaddrIn>(),
        AF_INET6 => core::mem::size_of::<crate::socket::SockaddrIn6>(),
        _ => core::mem::size_of::<Sockaddr>(),
    };
    SocklenT::try_from(len).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// The variables glibc exports with these
// ---------------------------------------------------------------------------

/// glibc's `__rcmd_errstr`: why `ruserok` last refused a hosts file -- a
/// static string such as `"bad owner"` -- or NULL. netkit's `rshd` and
/// `rlogind` declare it themselves, to log it.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
#[allow(non_upper_case_globals)]
pub static mut __rcmd_errstr: *const u8 = core::ptr::null();

/// glibc's `__check_rhosts_file`: 0 makes `ruserok` skip `~/.rhosts` for all
/// but the superuser -- what `rshd -l` and `rlogind -l` set.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
#[allow(non_upper_case_globals)]
pub static mut __check_rhosts_file: i32 = 1;

/// glibc's `rexecoptions`: exported, and read by nothing -- glibc's code or
/// this.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
#[allow(non_upper_case_globals)]
pub static mut rexecoptions: i32 = 0;

/// The process-wide state: the variables above, and the two buffers glibc's
/// `rcmd` and `rexec` keep the last canonical name in. The target's are
/// globals, as glibc's are; the host build's are per test thread.
mod state {
    /// Which call's canonical-name buffer: glibc has one `ahostbuf` in
    /// `rcmd.c` and another in `rexec.c`.
    #[derive(Clone, Copy)]
    pub(super) enum Ahost {
        Rcmd,
        Rexec,
    }

    #[cfg(target_os = "none")]
    mod globals {
        use core::sync::atomic::{AtomicPtr, Ordering};

        static RCMD_AHOSTBUF: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
        static REXEC_AHOSTBUF: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());

        pub(in super::super) fn set_errstr(s: *const u8) {
            // SAFETY: one pointer-sized store to the C-visible global, which,
            // like glibc's, no lock guards.
            unsafe { core::ptr::addr_of_mut!(super::super::__rcmd_errstr).write(s) };
        }

        pub(in super::super) fn check_rhosts_file() -> bool {
            // SAFETY: one int-sized load of the C-visible global.
            unsafe { core::ptr::addr_of!(super::super::__check_rhosts_file).read() != 0 }
        }

        pub(in super::super) fn swap_ahost(which: super::Ahost, new: *mut u8) -> *mut u8 {
            let slot = match which {
                super::Ahost::Rcmd => &RCMD_AHOSTBUF,
                super::Ahost::Rexec => &REXEC_AHOSTBUF,
            };
            slot.swap(new, Ordering::AcqRel)
        }
    }

    #[cfg(not(target_os = "none"))]
    mod globals {
        use core::cell::Cell;

        std::thread_local! {
            static ERRSTR: Cell<*const u8> = const { Cell::new(core::ptr::null()) };
            static CHECK_RHOSTS: Cell<i32> = const { Cell::new(1) };
            static AHOSTBUF: Cell<[*mut u8; 2]> = const { Cell::new([core::ptr::null_mut(); 2]) };
        }

        pub(in super::super) fn set_errstr(s: *const u8) {
            // A failed `try_with` is a thread shutting down: nothing to tell.
            let _ = ERRSTR.try_with(|e| e.set(s));
        }

        pub(in super::super) fn check_rhosts_file() -> bool {
            CHECK_RHOSTS.try_with(Cell::get).unwrap_or(1) != 0
        }

        pub(in super::super) fn swap_ahost(which: super::Ahost, new: *mut u8) -> *mut u8 {
            let i = match which {
                super::Ahost::Rcmd => 0,
                super::Ahost::Rexec => 1,
            };
            AHOSTBUF
                .try_with(|b| {
                    let mut all = b.get();
                    let old = all.get(i).copied().unwrap_or(core::ptr::null_mut());
                    if let Some(slot) = all.get_mut(i) {
                        *slot = new;
                    }
                    b.set(all);
                    old
                })
                .unwrap_or(core::ptr::null_mut())
        }

        #[cfg(test)]
        pub(in super::super) fn errstr() -> *const u8 {
            ERRSTR.with(Cell::get)
        }
    }

    pub(super) use globals::*;

    /// Keep a copy of `canon` (a C string) as `which`'s canonical name,
    /// freeing the one before: glibc's `free (ahostbuf); ahostbuf = __strdup
    /// (canon)`. The copy, or NULL when there is no memory for it.
    pub(super) fn keep_canonical(which: Ahost, canon: *const u8) -> *mut u8 {
        // glibc frees the old one first, so that a failed copy leaves none.
        let old = swap_ahost(which, core::ptr::null_mut());
        // SAFETY: `old` is null or this buffer's own `strdup`.
        unsafe { crate::malloc::free(old) };
        // SAFETY: `canon` is a C string, `getaddrinfo`'s.
        let copy = unsafe { crate::string::strdup(canon) };
        swap_ahost(which, copy);
        copy
    }
}

// ---------------------------------------------------------------------------
// The system's calls -- the tests' own world in the test build
// ---------------------------------------------------------------------------

/// What `sys::netrc` found at a path.
enum Netrc {
    /// No file: `ruserpass` answers 0 and says nothing.
    Missing,
    /// It could not be opened, for this `errno`.
    Error(i32),
    /// Its bytes -- as far as they could be read -- and its mode.
    Text(Buf, u32),
}

/// A `malloc` block of bytes, freed when it goes.
struct Buf {
    ptr: *mut u8,
    len: usize,
}

impl Buf {
    const EMPTY: Self = Self {
        ptr: core::ptr::null_mut(),
        len: 0,
    };

    fn bytes(&self) -> &[u8] {
        if self.ptr.is_null() {
            return &[];
        }
        // SAFETY: `ptr` holds `len` initialised bytes, owned by `self`.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// A copy of `bytes`, or `None` without the memory for it.
    fn copy_of(bytes: &[u8]) -> Option<Self> {
        Self::with_nul(bytes).map(|mut b| {
            b.len -= 1;
            b
        })
    }

    /// A copy of `bytes` and a NUL after them, which `len` counts: a C
    /// string. `None` without the memory for it.
    fn with_nul(bytes: &[u8]) -> Option<Self> {
        let len = bytes.len().checked_add(1)?;
        let ptr = crate::malloc::malloc(len);
        if ptr.is_null() {
            return None;
        }
        // SAFETY: a fresh block of `len` bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            *ptr.add(bytes.len()) = 0;
        }
        Some(Self { ptr, len })
    }
}

impl Drop for Buf {
    fn drop(&mut self) {
        // SAFETY: null, or this `Buf`'s own `malloc` block.
        unsafe { crate::malloc::free(self.ptr) };
    }
}

#[cfg(not(test))]
mod sys {
    use super::{Buf, Netrc, Storage};
    use crate::errno;
    use crate::poll::Pollfd;
    use crate::signal::SigsetT;
    use crate::socket::{Sockaddr, SocklenT};

    pub(super) fn socket(family: i32) -> i32 {
        crate::socket::socket(family, crate::socket::SOCK_STREAM, 0)
    }

    pub(super) fn bind(fd: i32, sa: &Storage, len: SocklenT) -> i32 {
        // SAFETY: `sa` is 128 bytes, at least `len` of them the address.
        unsafe { crate::socket::bind(fd, sa.as_sockaddr(), len) }
    }

    pub(super) fn connect(fd: i32, sa: *const Sockaddr, len: SocklenT) -> i32 {
        // SAFETY: `getaddrinfo`'s address, `len` bytes.
        unsafe { crate::socket::connect(fd, sa, len) }
    }

    pub(super) fn listen(fd: i32, backlog: i32) -> i32 {
        crate::socket::listen(fd, backlog)
    }

    pub(super) fn accept(fd: i32, sa: &mut Storage, len: &mut SocklenT) -> i32 {
        // SAFETY: `sa` is 128 writable bytes, and `*len` says how many at
        // most may be written.
        unsafe { crate::socket::accept(fd, sa.as_sockaddr_mut(), len) }
    }

    pub(super) fn getsockname(fd: i32, sa: &mut Storage, len: &mut SocklenT) -> i32 {
        // SAFETY: as for `accept`.
        unsafe { crate::socket::getsockname(fd, sa.as_sockaddr_mut(), len) }
    }

    pub(super) fn poll(fds: &mut [Pollfd; 2], timeout: i32) -> i32 {
        // SAFETY: two live `Pollfd`s.
        unsafe { crate::poll::poll(fds.as_mut_ptr(), 2, timeout) }
    }

    pub(super) fn read(fd: i32, buf: &mut [u8]) -> isize {
        crate::file::read(fd, buf.as_mut_ptr(), buf.len())
    }

    pub(super) fn write(fd: i32, buf: &[u8]) -> isize {
        crate::file::write(fd, buf.as_ptr(), buf.len())
    }

    pub(super) fn writev(fd: i32, parts: &[&[u8]; 3]) -> isize {
        let iov = parts.map(|p| crate::file::Iovec {
            iov_base: p.as_ptr().cast_mut().cast(),
            iov_len: p.len(),
        });
        crate::file::writev(fd, iov.as_ptr(), 3)
    }

    pub(super) fn close(fd: i32) {
        // As glibc's `(void) __close (s)`: nothing to do about a failure.
        let _ = crate::file::close(fd);
    }

    pub(super) fn set_owner(fd: i32, pid: i32) {
        /// Linux's `F_SETOWN`: who is sent `SIGURG` for the connection.
        const F_SETOWN: i32 = 8;
        // glibc ignores the answer too: out-of-band data is only a signal.
        let _ = crate::fcntl_ops::fcntl(fd, F_SETOWN, i64::from(pid));
    }

    pub(super) fn sleep(secs: u32) {
        // What is left of an interrupted sleep is not glibc's concern here
        // either.
        let _ = crate::time::sleep(secs);
    }

    pub(super) fn getpid() -> i32 {
        crate::process::getpid()
    }

    /// Block `SIGURG`, as glibc's `rcmd` does for its length; the mask
    /// before.
    pub(super) fn block_sigurg() -> SigsetT {
        let mut set = SigsetT::EMPTY;
        let mut old = SigsetT::EMPTY;
        // SAFETY: a live set, and a signal number in range.
        unsafe { crate::signal::sigaddset(&raw mut set, crate::signal::SIGURG) };
        // A mask that cannot be set leaves `SIGURG` as it was; glibc's
        // answer is unread too.
        let _ = crate::signal::sigprocmask(crate::signal::SIG_BLOCK, &raw const set, &raw mut old);
        old
    }

    pub(super) fn restore_mask(old: &SigsetT) {
        // As above.
        let _ = crate::signal::sigprocmask(crate::signal::SIG_SETMASK, old, core::ptr::null_mut());
    }

    pub(super) fn geteuid() -> u32 {
        crate::unistd::geteuid()
    }

    pub(super) fn seteuid(uid: u32) -> i32 {
        crate::unistd::seteuid(uid)
    }

    /// `gethostname`, NUL-terminated in `buf`; false when it fails.
    pub(super) fn hostname(buf: &mut [u8; 1024]) -> bool {
        let ok = crate::unistd::gethostname(buf.as_mut_ptr(), buf.len()) == 0;
        if let Some(last) = buf.last_mut() {
            *last = 0;
        }
        ok
    }

    /// `__fxprintf (NULL, "%s", ...)`: the bytes to `stderr`, its lock held.
    pub(super) fn eprint(parts: &[&[u8]]) {
        let out = crate::stdio::stderr_stream();
        crate::stdio::flockfile(out.cast());
        for part in parts {
            crate::error::put(out, part);
        }
        crate::stdio::funlockfile(out.cast());
    }

    /// `__write (STDERR_FILENO, &c, 1)`.
    pub(super) fn eprint_raw(c: u8) {
        // A message that cannot be written has nowhere else to go.
        let _ = crate::file::write(2, &raw const c, 1);
    }

    /// `perror (s)`, `s` a C string or NULL.
    pub(super) fn perror(s: *const u8) {
        // SAFETY: NULL or a C string, the caller's.
        unsafe { crate::stdio::perror(s) };
    }

    /// `warnx`'s line: the program's name, `: `, the message.
    pub(super) fn warnx(msg: &[&[u8]]) {
        let out = crate::stdio::stderr_stream();
        crate::stdio::flockfile(out.cast());
        // SAFETY: a plain read of the pointer `__libc_start_main` set.
        crate::error::put_cstr(out, unsafe { crate::crt::progname_slot().read() });
        crate::error::put(out, b": ");
        for part in msg {
            crate::error::put(out, part);
        }
        crate::error::put(out, b"\n");
        crate::stdio::funlockfile(out.cast());
    }

    /// `warn`'s line: `warnx`'s, with `: ` and `errno`'s text at its end.
    pub(super) fn warn(msg: &[u8]) {
        let text = crate::string::strerror(errno::get_errno());
        // SAFETY: `strerror` answers a C string.
        let text = unsafe { core::slice::from_raw_parts(text, crate::string::strlen(text)) };
        warnx(&[msg, b": ", text]);
    }

    /// The whole of an open descriptor, as far as it reads.
    fn read_all(fd: i32) -> Option<Buf> {
        let mut buf = Buf::EMPTY;
        let mut cap = 0usize;
        loop {
            if buf.len == cap {
                let bigger = cap.checked_mul(2)?.max(4096);
                // SAFETY: null or `buf`'s block; `realloc` keeps it on
                // failure, where `buf` still owns it.
                let p = unsafe { crate::malloc::realloc(buf.ptr, bigger) };
                if p.is_null() {
                    return None;
                }
                buf.ptr = p;
                cap = bigger;
            }
            // SAFETY: `ptr` holds `cap` bytes, `len` of them read.
            let r = crate::file::read(fd, unsafe { buf.ptr.add(buf.len) }, cap - buf.len);
            match usize::try_from(r) {
                Ok(0) => return Some(buf),
                Ok(n) => buf.len += n,
                Err(_) if errno::get_errno() == errno::EINTR => {}
                // An error ends the file, as `getc`'s `EOF` would.
                Err(_) => return Some(buf),
            }
        }
    }

    /// glibc's `iruserfopen`: the file at `path` (with its NUL) if it is a
    /// regular file, owned by root or `okuser`, written by no one else and
    /// linked once -- its whole text; else the reason, a static C string.
    pub(super) fn hosts_file(path: &[u8], okuser: u32) -> Result<Buf, &'static [u8]> {
        let mut st = crate::stat::Stat::default();
        if crate::file::lstat(path.as_ptr(), &raw mut st) != 0 {
            return Err(b"lstat failed\0");
        }
        if st.st_mode & crate::fcntl::S_IFMT != crate::fcntl::S_IFREG {
            return Err(b"not regular file\0");
        }
        let fd = crate::file::open(
            path.as_ptr(),
            crate::fcntl::O_RDONLY | crate::fcntl::O_CLOEXEC,
            0,
        );
        if fd < 0 {
            return Err(b"cannot open\0");
        }
        let verdict = if crate::file::fstat(fd, &raw mut st) < 0 {
            Err(&b"fstat failed\0"[..])
        } else if st.st_uid != 0 && st.st_uid != okuser {
            Err(&b"bad owner\0"[..])
        } else if st.st_mode & (crate::fcntl::S_IWGRP | crate::fcntl::S_IWOTH) != 0 {
            Err(&b"writeable by other than owner\0"[..])
        } else if st.st_nlink > 1 {
            Err(&b"hard linked somewhere\0"[..])
        } else {
            // A file glibc's `getline` could not read stops it where the
            // reading stopped; one that cannot be held at all is not read.
            read_all(fd).ok_or(&b"cannot open\0"[..])
        };
        close(fd);
        verdict
    }

    /// `fopen (path, "rce")` and what `ruserpass` reads of it.
    pub(super) fn netrc(path: &[u8]) -> Netrc {
        let fd = crate::file::open(
            path.as_ptr(),
            crate::fcntl::O_RDONLY | crate::fcntl::O_CLOEXEC,
            0,
        );
        if fd < 0 {
            let e = errno::get_errno();
            return if e == errno::ENOENT {
                Netrc::Missing
            } else {
                Netrc::Error(e)
            };
        }
        let mut st = crate::stat::Stat::default();
        // A mode that cannot be read counts as private: glibc's test is
        // `fstat (...) >= 0 && (mode & 077) != 0`.
        let mode = if crate::file::fstat(fd, &raw mut st) < 0 {
            0
        } else {
            st.st_mode
        };
        let text = read_all(fd);
        close(fd);
        match text {
            Some(t) => Netrc::Text(t, mode),
            None => Netrc::Error(errno::ENOMEM),
        }
    }
}

#[cfg(test)]
#[path = "rcmd/world.rs"]
mod sys;

#[cfg(test)]
#[path = "rcmd/tests.rs"]
mod tests;

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// The C string at `p`, without its NUL; `(null)` for NULL, as `%s` prints
/// it.
///
/// # Safety
///
/// `p` is NULL or a C string that outlives the answer.
unsafe fn c_text<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return b"(null)";
    }
    // SAFETY: a C string, this function's contract.
    unsafe { core::slice::from_raw_parts(p, crate::string::strlen(p)) }
}

/// The C string at `p` with its NUL.
///
/// # Safety
///
/// `p` is a C string that outlives the answer.
unsafe fn c_with_nul<'a>(p: *const u8) -> &'a [u8] {
    // SAFETY: a C string, this function's contract, so its bytes and NUL
    // are readable.
    unsafe { core::slice::from_raw_parts(p, crate::string::strlen(p) + 1) }
}

/// `errno`'s text: `%m`.
fn strerror_now() -> &'static [u8] {
    let p = crate::string::strerror(errno::get_errno());
    // SAFETY: `strerror` answers a static C string.
    unsafe { c_text(p) }
}

/// `n` in decimal and a NUL, in `buf`; the digits and the NUL.
fn decimal(n: u32, buf: &mut [u8; 12]) -> &[u8] {
    let mut digits = [0u8; 10];
    let mut i = digits.len();
    let mut v = n;
    loop {
        i -= 1;
        if let Some(d) = digits.get_mut(i) {
            // `% 10` is a digit.
            *d = b'0' + (v % 10) as u8;
        }
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let used = digits.len() - i;
    if let (Some(dst), Some(src)) = (buf.get_mut(..used), digits.get(i..)) {
        dst.copy_from_slice(src);
    }
    if let Some(nul) = buf.get_mut(used) {
        *nul = 0;
    }
    buf.get(..=used).unwrap_or_default()
}

/// `getnameinfo (ai, NI_NUMERICHOST)` into `buf`: the text up to its NUL --
/// on failure, whatever `buf` held, as glibc's `paddr` is left.
fn numeric_host(sa: *const Sockaddr, salen: SocklenT, buf: &mut [u8; ADDRSTRLEN]) -> &[u8] {
    // SAFETY: `sa` holds `salen` bytes of address; `buf` is writable for
    // its length.
    let _ = unsafe {
        crate::gai::getnameinfo(
            sa,
            salen,
            buf.as_mut_ptr(),
            ADDRSTRLEN as SocklenT,
            core::ptr::null_mut(),
            0,
            NI_NUMERICHOST,
        )
    };
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    buf.get(..end).unwrap_or_default()
}

/// An `Addrinfo` of zeros: the hints before they are filled in.
fn zeroed_addrinfo() -> Addrinfo {
    Addrinfo {
        ai_flags: 0,
        ai_family: 0,
        ai_socktype: 0,
        ai_protocol: 0,
        ai_addrlen: 0,
        ai_addr: core::ptr::null_mut(),
        ai_canonname: core::ptr::null_mut(),
        ai_next: core::ptr::null_mut(),
    }
}

/// A `getaddrinfo` list, freed when it goes.
struct AiList(*mut Addrinfo);

impl Drop for AiList {
    fn drop(&mut self) {
        // SAFETY: `getaddrinfo`'s list, freed once.
        unsafe { crate::gai::freeaddrinfo(self.0) };
    }
}

/// `TEMP_FAILURE_RETRY (__writev (s, iov, 3))`.
fn writev_retrying(s: i32, parts: &[&[u8]; 3]) -> isize {
    loop {
        let r = sys::writev(s, parts);
        if r >= 0 || errno::get_errno() != errno::EINTR {
            return r;
        }
    }
}

/// `TEMP_FAILURE_RETRY (__read (s, buf, len))`.
fn read_retrying(s: i32, buf: &mut [u8]) -> isize {
    loop {
        let r = sys::read(s, buf);
        if r >= 0 || errno::get_errno() != errno::EINTR {
            return r;
        }
    }
}

/// `TEMP_FAILURE_RETRY (accept (s, sa, len))`.
fn accept_retrying(s: i32, sa: &mut Storage, len: &mut SocklenT) -> i32 {
    loop {
        let r = sys::accept(s, sa, len);
        if r >= 0 || errno::get_errno() != errno::EINTR {
            return r;
        }
    }
}

/// After a refusal from the server: its message, a byte at a time, copied
/// to standard error to its first newline.
fn copy_refusal(s: i32) {
    let mut c = [0u8; 1];
    while sys::read(s, &mut c) == 1 {
        sys::eprint_raw(c[0]);
        if c[0] == b'\n' {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// rresvport
// ---------------------------------------------------------------------------

/// `rresvport_af`: a stream socket of `family` bound to a reserved port, the
/// search starting at `*alport` and the port found left there.
///
/// # Safety
///
/// `alport` is a valid `int`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn rresvport_af(alport: *mut i32, family: SaFamilyT) -> i32 {
    let family = i32::from(family);
    if family != AF_INET && family != AF_INET6 {
        errno::set_errno(errno::EAFNOSUPPORT);
        return -1;
    }
    // No `SOCK_CLOEXEC`, as glibc: for the programs that hand it on.
    let s = sys::socket(family);
    if s < 0 {
        return -1;
    }
    let mut ss = Storage::ZERO;
    ss.set_family(family);
    let len = sa_len(family);
    // SAFETY: the caller's `int`.
    let port = unsafe { &mut *alport };
    if *port < IPPORT_RESERVED / 2 {
        *port = IPPORT_RESERVED / 2;
    } else if *port >= IPPORT_RESERVED {
        *port = IPPORT_RESERVED - 1;
    }
    let start = *port;
    loop {
        // In 512..=1023 here.
        ss.set_port(u16::try_from(*port).unwrap_or(0));
        if sys::bind(s, &ss, len) >= 0 {
            return s;
        }
        if errno::get_errno() != errno::EADDRINUSE {
            sys::close(s);
            return -1;
        }
        *port = if *port == IPPORT_RESERVED / 2 {
            IPPORT_RESERVED - 1
        } else {
            *port - 1
        };
        if *port == start {
            break;
        }
    }
    sys::close(s);
    errno::set_errno(errno::EAGAIN);
    -1
}

/// `rresvport`: [`rresvport_af`] for IPv4.
///
/// # Safety
///
/// As [`rresvport_af`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn rresvport(alport: *mut i32) -> i32 {
    // SAFETY: this function's contract.
    unsafe { rresvport_af(alport, AF_INET as SaFamilyT) }
}

// ---------------------------------------------------------------------------
// rcmd
// ---------------------------------------------------------------------------

/// Where `rcmd_af` goes on failing: glibc's `bad2` closes the stderr channel
/// too, `bad` the connection only.
enum Bad {
    Bad,
    Bad2,
}

/// `rcmd_af`: run `cmd` on `*ahost` through its `rshd` at `rport` (network
/// order), as `remuser` there for `locuser` here; the connection, and with
/// `fd2p` the stderr channel's in `*fd2p`. -1 on failure, said on standard
/// error. It binds reserved ports: the privilege to.
///
/// # Safety
///
/// `ahost` points to a C string or NULL, which the call replaces with its own
/// buffer; the three strings are C strings; `fd2p` is NULL or writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn rcmd_af(
    ahost: *mut *mut u8,
    rport: u16,
    locuser: *const u8,
    remuser: *const u8,
    cmd: *const u8,
    fd2p: *mut i32,
    af: SaFamilyT,
) -> i32 {
    let af = i32::from(af);
    if af != AF_INET && af != AF_INET6 && af != AF_UNSPEC {
        errno::set_errno(errno::EAFNOSUPPORT);
        return -1;
    }
    let pid = sys::getpid();
    let mut hints = zeroed_addrinfo();
    hints.ai_flags = AI_CANONNAME;
    hints.ai_family = af;
    hints.ai_socktype = SOCK_STREAM;
    let mut num = [0u8; 12];
    let num = decimal(u32::from(u16::from_be(rport)), &mut num);
    // SAFETY: the caller's pointer.
    let host = unsafe { *ahost };
    let mut res: *mut Addrinfo = core::ptr::null_mut();
    // SAFETY: `host` NULL or a C string; the rest live locals.
    let error =
        unsafe { crate::gai::getaddrinfo(host, num.as_ptr(), &raw const hints, &raw mut res) };
    if error != 0 {
        if error == EAI_NONAME && !host.is_null() {
            // SAFETY: a C string, the caller's.
            sys::eprint(&[unsafe { c_text(host) }, b": Unknown host\n"]);
        } else {
            let text = crate::gai::gai_strerror(error);
            // SAFETY: `gai_strerror` answers a C string.
            sys::eprint(&[b"rcmd: getaddrinfo: ", unsafe { c_text(text) }, b"\n"]);
        }
        return -1;
    }
    let list = AiList(res);
    // SAFETY: a successful `getaddrinfo` answers at least one entry.
    let canon = unsafe { (*res).ai_canonname };
    if canon.is_null() {
        // SAFETY: the caller's pointer.
        unsafe { *ahost = core::ptr::null_mut() };
    } else {
        let copy = state::keep_canonical(state::Ahost::Rcmd, canon);
        if copy.is_null() {
            drop(list);
            sys::eprint(&[b"rcmd: Cannot allocate memory\n"]);
            return -1;
        }
        // SAFETY: the caller's pointer.
        unsafe { *ahost = copy };
    }

    let omask = sys::block_sigurg();
    let mut ai = res;
    let mut refused = false;
    let mut timo = 1u32;
    let mut lport = IPPORT_RESERVED - 1;
    let s = loop {
        // SAFETY: an entry of the list.
        let entry = unsafe { &*ai };
        // SAFETY: a live local; a family `getaddrinfo` gave.
        let s = unsafe { rresvport_af(&raw mut lport, entry.ai_family as SaFamilyT) };
        if s < 0 {
            if errno::get_errno() == errno::EAGAIN {
                sys::eprint(&[b"rcmd: socket: All ports in use\n"]);
            } else {
                sys::eprint(&[b"rcmd: socket: ", strerror_now(), b"\n"]);
            }
            sys::restore_mask(&omask);
            return -1;
        }
        sys::set_owner(s, pid);
        if sys::connect(s, entry.ai_addr, entry.ai_addrlen) >= 0 {
            break s;
        }
        let e = errno::get_errno();
        sys::close(s);
        if e == errno::EADDRINUSE {
            lport -= 1;
            continue;
        }
        if e == errno::ECONNREFUSED {
            refused = true;
        }
        if !entry.ai_next.is_null() {
            let mut paddr = [0u8; ADDRSTRLEN];
            sys::eprint(&[
                b"connect to address ",
                numeric_host(entry.ai_addr, entry.ai_addrlen, &mut paddr),
                b": ",
            ]);
            errno::set_errno(e);
            sys::perror(core::ptr::null());
            ai = entry.ai_next;
            // SAFETY: the next entry of the list.
            let next = unsafe { &*ai };
            sys::eprint(&[
                b"Trying ",
                numeric_host(next.ai_addr, next.ai_addrlen, &mut paddr),
                b"...\n",
            ]);
            continue;
        }
        if refused && timo <= 16 {
            sys::sleep(timo);
            timo *= 2;
            ai = res;
            refused = false;
            continue;
        }
        drop(list);
        errno::set_errno(e);
        let text = strerror_now();
        // SAFETY: the caller's pointer: NULL or a C string.
        sys::eprint(&[unsafe { c_text(*ahost) }, b": ", text, b"\n"]);
        sys::restore_mask(&omask);
        return -1;
    };
    // SAFETY: the entry connected through.
    let entry = unsafe { &*ai };
    // SAFETY: this function's contract, for the strings, `fd2p` and `ahost`.
    let fail = unsafe { rcmd_session(s, entry, &mut lport, locuser, remuser, cmd, fd2p, ahost) };
    if let Some(how) = fail {
        if matches!(how, Bad::Bad2) && lport != 0 {
            // SAFETY: `fd2p` was written before `bad2` is gone to with
            // `lport` not 0.
            sys::close(unsafe { *fd2p });
        }
        sys::close(s);
        sys::restore_mask(&omask);
        drop(list);
        return -1;
    }
    sys::restore_mask(&omask);
    drop(list);
    s
}

/// `rcmd_af` from the connection on: the stderr channel, the request, the
/// answer. `None` on success, else where glibc's `goto` would go.
///
/// # Safety
///
/// As [`rcmd_af`].
#[allow(clippy::too_many_arguments)]
unsafe fn rcmd_session(
    s: i32,
    entry: &Addrinfo,
    lport: &mut i32,
    locuser: *const u8,
    remuser: *const u8,
    cmd: *const u8,
    fd2p: *mut i32,
    ahost: *mut *mut u8,
) -> Option<Bad> {
    *lport -= 1;
    if fd2p.is_null() {
        // The answer read below tells whether this went.
        let _ = sys::write(s, b"\0");
        *lport = 0;
    } else {
        // SAFETY: a live local.
        let s2 = unsafe { rresvport_af(lport, entry.ai_family as SaFamilyT) };
        if s2 < 0 {
            return Some(Bad::Bad);
        }
        // An unlistening socket shows in the `poll` below.
        let _ = sys::listen(s2, 1);
        let mut num = [0u8; 12];
        let num = decimal(u32::try_from(*lport).unwrap_or(0), &mut num);
        if sys::write(s, num) != isize::try_from(num.len()).unwrap_or(isize::MAX) {
            sys::eprint(&[b"rcmd: write (setting up stderr): ", strerror_now(), b"\n"]);
            sys::close(s2);
            return Some(Bad::Bad);
        }
        let mut pfd = [
            Pollfd {
                fd: s,
                events: POLLIN,
                revents: 0,
            },
            Pollfd {
                fd: s2,
                events: POLLIN,
                revents: 0,
            },
        ];
        errno::set_errno(0);
        if sys::poll(&mut pfd, -1) < 1 || pfd[1].revents & POLLIN == 0 {
            if errno::get_errno() != 0 {
                sys::eprint(&[b"rcmd: poll (setting up stderr): ", strerror_now(), b"\n"]);
            } else {
                sys::eprint(&[b"poll: protocol failure in circuit setup\n"]);
            }
            sys::close(s2);
            return Some(Bad::Bad);
        }
        let mut from = Storage::ZERO;
        let mut len = entry.ai_addrlen;
        let s3 = accept_retrying(s2, &mut from, &mut len);
        let from_port = match from.family() {
            AF_INET | AF_INET6 => i32::from(from.port()),
            _ => 0,
        };
        sys::close(s2);
        if s3 < 0 {
            sys::eprint(&[b"rcmd: accept: ", strerror_now(), b"\n"]);
            *lport = 0;
            return Some(Bad::Bad);
        }
        // SAFETY: the caller's `int`.
        unsafe { *fd2p = s3 };
        if from_port >= IPPORT_RESERVED || from_port < IPPORT_RESERVED / 2 {
            sys::eprint(&[b"socket: protocol failure in circuit setup\n"]);
            return Some(Bad::Bad2);
        }
    }
    // SAFETY: C strings, the caller's.
    let request = unsafe { [c_with_nul(locuser), c_with_nul(remuser), c_with_nul(cmd)] };
    // The answer read next tells whether the request went.
    let _ = writev_retrying(s, &request);
    let mut c = [0u8; 1];
    let n = read_retrying(s, &mut c);
    if n != 1 {
        // SAFETY: the caller's pointer: NULL or a C string.
        let host = unsafe { c_text(*ahost) };
        if n == 0 {
            sys::eprint(&[b"rcmd: ", host, b": short read"]);
        } else {
            sys::eprint(&[b"rcmd: ", host, b": ", strerror_now(), b"\n"]);
        }
        return Some(Bad::Bad2);
    }
    if c[0] != 0 {
        copy_refusal(s);
        return Some(Bad::Bad2);
    }
    None
}

/// `rcmd`: [`rcmd_af`] for IPv4.
///
/// # Safety
///
/// As [`rcmd_af`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn rcmd(
    ahost: *mut *mut u8,
    rport: u16,
    locuser: *const u8,
    remuser: *const u8,
    cmd: *const u8,
    fd2p: *mut i32,
) -> i32 {
    // SAFETY: this function's contract.
    unsafe {
        rcmd_af(
            ahost,
            rport,
            locuser,
            remuser,
            cmd,
            fd2p,
            AF_INET as SaFamilyT,
        )
    }
}

// ---------------------------------------------------------------------------
// rexec
// ---------------------------------------------------------------------------

/// `rexec_af`: run `cmd` on `*ahost` through its `rexecd` at `rport` (its low
/// 16 bits, network order), as `name` with `pass` -- either, when NULL, from
/// `~/.netrc`; the connection, and with `fd2p` the stderr channel's in
/// `*fd2p`. -1 on failure.
///
/// # Safety
///
/// As [`rcmd_af`]; `name` and `pass` are NULL or C strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn rexec_af(
    ahost: *mut *mut u8,
    rport: i32,
    name: *const u8,
    pass: *const u8,
    cmd: *const u8,
    fd2p: *mut i32,
    af: SaFamilyT,
) -> i32 {
    // `ntohs (rport)`: the low 16 bits, swapped.
    #[allow(clippy::cast_possible_truncation)]
    let port = u16::from_be(rport as u16);
    let mut serv = [0u8; 12];
    let serv = decimal(u32::from(port), &mut serv);
    let mut hints = zeroed_addrinfo();
    hints.ai_family = i32::from(af);
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_flags = AI_CANONNAME;
    let mut res0: *mut Addrinfo = core::ptr::null_mut();
    // SAFETY: the caller's host, NULL or a C string; live locals.
    if unsafe { crate::gai::getaddrinfo(*ahost, serv.as_ptr(), &raw const hints, &raw mut res0) }
        != 0
    {
        return -1;
    }
    let list = AiList(res0);
    // SAFETY: at least one entry.
    let first = unsafe { &*res0 };
    if first.ai_canonname.is_null() {
        // SAFETY: the caller's pointer.
        unsafe { *ahost = core::ptr::null_mut() };
        errno::set_errno(errno::ENOENT);
        return -1;
    }
    let copy = state::keep_canonical(state::Ahost::Rexec, first.ai_canonname);
    if copy.is_null() {
        sys::perror(c"rexec: strdup".as_ptr().cast());
        return -1;
    }
    // SAFETY: the caller's pointer.
    unsafe { *ahost = copy };
    let mut found = Found::default();
    // SAFETY: `ai_canonname` is a C string.
    let canon = unsafe { c_text(first.ai_canonname) };
    // Its answer goes unread, as glibc's does: a refused `~/.netrc` leaves the
    // name and password as they were given.
    let _ = ruserpass(canon, name, pass, &mut found);
    let name = found.name.as_ref().map_or(name, |b| b.ptr.cast_const());
    let pass = found.pass.as_ref().map_or(pass, |b| b.ptr.cast_const());
    let mut timo = 1u32;
    let s = loop {
        let s = sys::socket(first.ai_family);
        if s < 0 {
            sys::perror(c"rexec: socket".as_ptr().cast());
            return -1;
        }
        if sys::connect(s, first.ai_addr, first.ai_addrlen) >= 0 {
            break s;
        }
        if errno::get_errno() == errno::ECONNREFUSED && timo <= 16 {
            sys::close(s);
            sys::sleep(timo);
            timo *= 2;
            continue;
        }
        sys::perror(first.ai_canonname);
        sys::close(s);
        return -1;
    };
    // The stderr channel's port: 0 with none.
    let mut port2 = 0u32;
    if fd2p.is_null() {
        // The answer read below tells whether this went.
        let _ = sys::write(s, b"\0");
    } else {
        let s2 = sys::socket(first.ai_family);
        if s2 < 0 {
            sys::close(s);
            return -1;
        }
        // An unlistening socket shows in the `accept` below.
        let _ = sys::listen(s2, 1);
        let mut sa2 = Storage::ZERO;
        let mut sa2len = SocklenT::try_from(core::mem::size_of::<Storage>()).unwrap_or(0);
        if sys::getsockname(s2, &mut sa2, &mut sa2len) < 0 {
            sys::perror(c"getsockname".as_ptr().cast());
            sys::close(s2);
            sys::close(s);
            return -1;
        } else if sa2len != sa_len(sa2.family()) {
            errno::set_errno(errno::EINVAL);
            sys::close(s2);
            sys::close(s);
            return -1;
        }
        let mut servbuf = [0u8; 32];
        // SAFETY: `sa2` holds `sa2len` bytes of address; `servbuf` is
        // writable for its length.
        let named = unsafe {
            crate::gai::getnameinfo(
                sa2.as_sockaddr(),
                sa2len,
                core::ptr::null_mut(),
                0,
                servbuf.as_mut_ptr(),
                32,
                NI_NUMERICSERV,
            )
        } == 0;
        if named {
            // glibc's `strtol (servbuff, NULL, 10)` of a number it wrote.
            port2 = servbuf
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .fold(0u32, |n, &d| {
                    n.wrapping_mul(10).wrapping_add(u32::from(d - b'0'))
                });
        }
        let mut num = [0u8; 12];
        let num = decimal(port2, &mut num);
        // The `accept` below tells whether this went.
        let _ = sys::write(s, num);
        let mut from = Storage::ZERO;
        let mut len = SocklenT::try_from(core::mem::size_of::<Storage>()).unwrap_or(0);
        let s3 = accept_retrying(s2, &mut from, &mut len);
        sys::close(s2);
        if s3 < 0 {
            sys::perror(c"accept".as_ptr().cast());
            sys::close(s);
            return -1;
        }
        // SAFETY: the caller's `int`.
        unsafe { *fd2p = s3 };
    }
    let bad = || {
        if port2 != 0 {
            // SAFETY: written above whenever `port2` is not 0.
            sys::close(unsafe { *fd2p });
        }
        sys::close(s);
        -1
    };
    if name.is_null() || pass.is_null() {
        // glibc's takes the length of the NULL here.
        errno::set_errno(errno::EINVAL);
        return bad();
    }
    // SAFETY: C strings: the caller's, or `ruserpass`'s copies, alive until
    // `found` goes.
    let request = unsafe { [c_with_nul(name), c_with_nul(pass), c_with_nul(cmd)] };
    let _ = writev_retrying(s, &request);
    // glibc frees `ruserpass`'s copies here.
    drop(found);
    let mut c = [0u8; 1];
    if sys::read(s, &mut c) != 1 {
        // SAFETY: `*ahost` is the copy made above, a C string.
        sys::perror(unsafe { *ahost });
        return bad();
    }
    if c[0] != 0 {
        copy_refusal(s);
        return bad();
    }
    drop(list);
    s
}

/// `rexec`: [`rexec_af`] for IPv4.
///
/// # Safety
///
/// As [`rexec_af`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn rexec(
    ahost: *mut *mut u8,
    rport: i32,
    name: *const u8,
    pass: *const u8,
    cmd: *const u8,
    fd2p: *mut i32,
) -> i32 {
    // SAFETY: this function's contract.
    unsafe { rexec_af(ahost, rport, name, pass, cmd, fd2p, AF_INET as SaFamilyT) }
}

// ---------------------------------------------------------------------------
// ruserpass: ~/.netrc
// ---------------------------------------------------------------------------

/// What `ruserpass` found: its copies of a name and password, NUL-terminated,
/// where the caller gave none.
#[derive(Default)]
struct Found {
    name: Option<Buf>,
    pass: Option<Buf>,
}

/// A `Found` string without its NUL.
fn found_text(b: &Buf) -> &[u8] {
    b.bytes().strip_suffix(b"\0").unwrap_or(b.bytes())
}

/// `.netrc`'s words: glibc's `toktab`, and what is none of them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tok {
    /// The end of the file, or a word that is empty.
    End,
    Default,
    Login,
    Passwd,
    Account,
    Machine,
    Macdef,
    Id,
}

/// glibc's `token`, over the file read whole: the next word, quoted or not,
/// in `tokval` -- 99 bytes at most, where glibc's buffer of 100 would
/// overflow.
struct Tokens<'a> {
    text: &'a [u8],
    at: usize,
    tokval: [u8; 100],
    toklen: usize,
}

impl Tokens<'_> {
    /// `getc`: the next byte, or `None` at the end.
    fn getc(&mut self) -> Option<u8> {
        let c = self.text.get(self.at).copied();
        if c.is_some() {
            self.at += 1;
        }
        c
    }

    fn push(&mut self, c: u8) {
        if self.toklen < self.tokval.len() - 1 {
            if let Some(slot) = self.tokval.get_mut(self.toklen) {
                *slot = c;
            }
            self.toklen += 1;
        }
    }

    /// The word, as far as a NUL in it: glibc's is a C string.
    fn word(&self) -> &[u8] {
        let w = self.tokval.get(..self.toklen).unwrap_or_default();
        let end = w.iter().position(|&b| b == 0).unwrap_or(w.len());
        w.get(..end).unwrap_or_default()
    }

    fn token(&mut self) -> Tok {
        let is_sep = |c: u8| matches!(c, b'\n' | b'\t' | b' ' | b',');
        let mut c = loop {
            match self.getc() {
                Some(c) if is_sep(c) => {}
                Some(c) => break c,
                None => return Tok::End,
            }
        };
        self.toklen = 0;
        if c == b'"' {
            loop {
                match self.getc() {
                    None | Some(b'"') => break,
                    Some(b'\\') => {
                        // A backslash at the end stores glibc's `EOF`, as the
                        // byte it truncates to.
                        let next = self.getc().unwrap_or(0xff);
                        self.push(next);
                    }
                    Some(other) => self.push(other),
                }
            }
        } else {
            loop {
                if c == b'\\' {
                    c = self.getc().unwrap_or(0xff);
                }
                self.push(c);
                match self.getc() {
                    Some(n) if !is_sep(n) => c = n,
                    _ => break,
                }
            }
        }
        match self.word() {
            b"" => Tok::End,
            b"default" => Tok::Default,
            b"login" => Tok::Login,
            b"password" | b"passwd" => Tok::Passwd,
            b"account" => Tok::Account,
            b"machine" => Tok::Machine,
            b"macdef" => Tok::Macdef,
            _ => Tok::Id,
        }
    }
}

/// glibc's `ruserpass`: `~/.netrc`'s login and password for `host` -- from
/// the first `machine` entry naming it, or a `default` -- into `found`, for
/// each of `name` and `pass` the caller left NULL. A login that is not the
/// name in force moves on to the next entry. A password in a file others can
/// read refuses the file, unless the login is `anonymous`, said with `warnx`.
/// -1 without `$HOME`, on a refusal, or without memory; else 0.
fn ruserpass(host: &[u8], name: *const u8, pass: *const u8, found: &mut Found) -> i32 {
    // glibc's `__libc_secure_getenv`.
    // SAFETY: a NUL-terminated name.
    let home = unsafe { crate::environ::secure_lookup(c"HOME".as_ptr().cast()) };
    if home.is_null() {
        return -1;
    }
    // SAFETY: the environment's C string.
    let home = unsafe { c_text(home) };
    let tail = b"/.netrc\0";
    let Some(path) = Buf::copy_of(&[home, tail].concat_into::<4104>()) else {
        return -1;
    };
    let path = path.bytes();
    let (text, mode) = match sys::netrc(path) {
        Netrc::Missing => return 0,
        Netrc::Error(e) => {
            errno::set_errno(e);
            sys::warn(path.strip_suffix(b"\0").unwrap_or(path));
            return 0;
        }
        Netrc::Text(t, mode) => (t, mode),
    };
    let mut myname = [0u8; 1024];
    if !sys::hostname(&mut myname) {
        myname[0] = 0;
    }
    let len = myname.iter().position(|&b| b == 0).unwrap_or(myname.len());
    let myname = myname.get(..len).unwrap_or_default();
    // glibc's `__strchrnul (myname, '.')`.
    let mydomain = myname
        .iter()
        .position(|&b| b == b'.')
        .map_or(&b""[..], |i| myname.get(i..).unwrap_or_default());
    // SAFETY: NULL or the caller's C string.
    let given = (!name.is_null()).then(|| unsafe { c_text(name) });
    let mut tokens = Tokens {
        text: text.bytes(),
        at: 0,
        tokval: [0; 100],
        toklen: 0,
    };
    let mut usedefault = false;
    'next: loop {
        let t = tokens.token();
        match t {
            Tok::End => return 0,
            Tok::Default | Tok::Machine => {
                if t == Tok::Default {
                    usedefault = true;
                }
                if !usedefault
                    && (tokens.token() != Tok::Id || !host_matches(host, tokens.word(), mydomain))
                {
                    continue 'next;
                }
            }
            _ => continue 'next,
        }
        // glibc's `match:`.
        loop {
            match tokens.token() {
                Tok::End | Tok::Machine | Tok::Default => return 0,
                Tok::Login => {
                    if tokens.token() != Tok::End {
                        match given.or_else(|| found.name.as_ref().map(found_text)) {
                            None => {
                                let Some(copy) = Buf::with_nul(tokens.word()) else {
                                    sys::warnx(&[b"out of memory"]);
                                    return -1;
                                };
                                found.name = Some(copy);
                            }
                            Some(n) if n != tokens.word() => continue 'next,
                            Some(_) => {}
                        }
                    }
                }
                Tok::Passwd => {
                    let current = given.or_else(|| found.name.as_ref().map(found_text));
                    if current != Some(&b"anonymous"[..]) && mode & 0o077 != 0 {
                        sys::warnx(&[b"Error: .netrc file is readable by others."]);
                        sys::warnx(&[b"Remove 'password' line or make file unreadable by others."]);
                        return -1;
                    }
                    if tokens.token() != Tok::End && pass.is_null() && found.pass.is_none() {
                        let Some(copy) = Buf::with_nul(tokens.word()) else {
                            sys::warnx(&[b"out of memory"]);
                            return -1;
                        };
                        found.pass = Some(copy);
                    }
                }
                Tok::Account | Tok::Macdef => {}
                Tok::Id => sys::warnx(&[b"Unknown .netrc keyword ", tokens.word()]),
            }
        }
    }
}

/// `ruserpass`'s host test: the entry names `host` (case aside), or `host`'s
/// first label when the rest of `host` is this host's domain.
fn host_matches(host: &[u8], entry: &[u8], mydomain: &[u8]) -> bool {
    if host.eq_ignore_ascii_case(entry) {
        return true;
    }
    let Some(dot) = host.iter().position(|&b| b == b'.') else {
        return false;
    };
    let (label, domain) = host.split_at(dot);
    domain.eq_ignore_ascii_case(mydomain) && label.eq_ignore_ascii_case(entry)
}

/// Joining byte strings into a fixed array, without an allocator.
trait ConcatInto {
    /// The pieces end to end, in a buffer of `N` -- cut short past it.
    fn concat_into<const N: usize>(&self) -> FixedBytes<N>;
}

/// Bytes in an array, and how many.
struct FixedBytes<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> core::ops::Deref for FixedBytes<N> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or_default()
    }
}

impl ConcatInto for [&[u8]] {
    fn concat_into<const N: usize>(&self) -> FixedBytes<N> {
        let mut out = FixedBytes {
            buf: [0; N],
            len: 0,
        };
        for piece in self {
            for &b in *piece {
                if let Some(slot) = out.buf.get_mut(out.len) {
                    *slot = b;
                    out.len += 1;
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// ruserok
// ---------------------------------------------------------------------------

/// `ruserok_af`: 0 when `ruser` on `rhost` may log in as `luser` here -- by
/// `/etc/hosts.equiv` (not for the `superuser`) or `luser`'s `~/.rhosts` --
/// for any of `rhost`'s addresses of family `af`; else -1.
///
/// # Safety
///
/// The strings are C strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ruserok_af(
    rhost: *const u8,
    superuser: i32,
    ruser: *const u8,
    luser: *const u8,
    af: SaFamilyT,
) -> i32 {
    let mut hints = zeroed_addrinfo();
    hints.ai_family = i32::from(af);
    let mut res0: *mut Addrinfo = core::ptr::null_mut();
    // SAFETY: the caller's C string; live locals.
    if unsafe { crate::gai::getaddrinfo(rhost, core::ptr::null(), &raw const hints, &raw mut res0) }
        != 0
    {
        return -1;
    }
    let list = AiList(res0);
    let mut res = list.0;
    while !res.is_null() {
        // SAFETY: an entry of the list.
        let entry = unsafe { &*res };
        // SAFETY: the caller's C strings.
        if unsafe {
            ruserok2_sa(
                entry.ai_addr,
                entry.ai_addrlen,
                superuser != 0,
                ruser,
                luser,
                rhost,
            )
        } == 0
        {
            return 0;
        }
        res = entry.ai_next;
    }
    -1
}

/// `ruserok`: [`ruserok_af`] for IPv4.
///
/// # Safety
///
/// As [`ruserok_af`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ruserok(
    rhost: *const u8,
    superuser: i32,
    ruser: *const u8,
    luser: *const u8,
) -> i32 {
    // SAFETY: this function's contract.
    unsafe { ruserok_af(rhost, superuser, ruser, luser, AF_INET as SaFamilyT) }
}

/// `iruserok_af`: [`ruserok_af`] for the address `raddr` -- 4 bytes for
/// `AF_INET`, 16 for `AF_INET6`; any other family answers 0, as glibc's
/// does, without a look at either file.
///
/// # Safety
///
/// `raddr` is readable for the family's address; the strings are C strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn iruserok_af(
    raddr: *const core::ffi::c_void,
    superuser: i32,
    ruser: *const u8,
    luser: *const u8,
    af: SaFamilyT,
) -> i32 {
    let family = i32::from(af);
    // Where the address goes in its `sockaddr`, and its size.
    let (offset, size) = match family {
        AF_INET => (4, 4),
        AF_INET6 => (8, 16),
        _ => return 0,
    };
    let mut ra = Storage::ZERO;
    ra.set_family(family);
    if let Some(dst) = ra.0.get_mut(offset..offset + size) {
        // SAFETY: `size` readable bytes, this function's contract.
        dst.copy_from_slice(unsafe { core::slice::from_raw_parts(raddr.cast::<u8>(), size) });
    }
    // SAFETY: the caller's C strings; `-` stands for the host name it has
    // none of, as in glibc.
    unsafe {
        ruserok2_sa(
            ra.as_sockaddr(),
            sa_len(family),
            superuser != 0,
            ruser,
            luser,
            c"-".as_ptr().cast(),
        )
    }
}

/// `iruserok`: [`iruserok_af`] for an IPv4 address, network order.
///
/// # Safety
///
/// The strings are C strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn iruserok(
    raddr: u32,
    superuser: i32,
    ruser: *const u8,
    luser: *const u8,
) -> i32 {
    // SAFETY: four bytes of a live local; the caller's strings.
    unsafe {
        iruserok_af(
            (&raw const raddr).cast(),
            superuser,
            ruser,
            luser,
            AF_INET as SaFamilyT,
        )
    }
}

/// glibc's `ruserok2_sa`: `/etc/hosts.equiv` unless `superuser`, then
/// `luser`'s `~/.rhosts` read with `luser`'s effective uid. 0 to let in.
///
/// # Safety
///
/// `ra` holds `ralen` bytes of address; the strings are C strings.
unsafe fn ruserok2_sa(
    ra: *const Sockaddr,
    ralen: SocklenT,
    superuser: bool,
    ruser: *const u8,
    luser: *const u8,
    rhost: *const u8,
) -> i32 {
    let mut isbad = -1;
    if !superuser {
        match sys::hosts_file(PATH_HEQUIV, 0) {
            Ok(text) => {
                // SAFETY: this function's contract.
                isbad = unsafe { validuser2_sa(text.bytes(), ra, ralen, luser, ruser, rhost) };
                if isbad == 0 {
                    return 0;
                }
            }
            Err(why) => state::set_errstr(why.as_ptr()),
        }
    }
    if !(state::check_rhosts_file() || superuser) {
        return -1;
    }
    let mut pwd = core::mem::MaybeUninit::<crate::pwd::Passwd>::uninit();
    let mut buf = [0u8; PW_BUFLEN];
    let mut result: *const crate::pwd::Passwd = core::ptr::null();
    // SAFETY: the caller's C string; live locals of the sizes given.
    let rc = unsafe {
        crate::pwd::getpwnam_r(
            luser,
            pwd.as_mut_ptr(),
            buf.as_mut_ptr(),
            PW_BUFLEN,
            &raw mut result,
        )
    };
    if rc != 0 || result.is_null() {
        return -1;
    }
    // SAFETY: `getpwnam_r` filled the entry, `result` says so.
    let pw = unsafe { &*result };
    // SAFETY: a C string in `buf`.
    let dir = unsafe { c_text(pw.pw_dir) };
    let path = [dir, b"/.rhosts\0"].concat_into::<{ PW_BUFLEN + 16 }>();
    let uid = sys::geteuid();
    if sys::seteuid(pw.pw_uid) < 0 {
        return -1;
    }
    match sys::hosts_file(&path, pw.pw_uid) {
        // SAFETY: this function's contract.
        Ok(text) => isbad = unsafe { validuser2_sa(text.bytes(), ra, ralen, luser, ruser, rhost) },
        Err(why) => state::set_errstr(why.as_ptr()),
    }
    if sys::seteuid(uid) < 0 {
        return -1;
    }
    isbad
}

/// The C locale's `isspace`.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// glibc's `__validuser2_sa`: 0 when a line of `text` lets `ruser` on the
/// host at `ra` in as `luser`, -1 when none does or one shuts them out.
///
/// # Safety
///
/// As [`ruserok2_sa`].
unsafe fn validuser2_sa(
    text: &[u8],
    ra: *const Sockaddr,
    ralen: SocklenT,
    luser: *const u8,
    ruser: *const u8,
    rhost: *const u8,
) -> i32 {
    // glibc's `getline`: each line with its newline; a last without one.
    let mut rest = text;
    while !rest.is_empty() {
        let end = rest
            .iter()
            .position(|&b| b == b'\n')
            .map_or(rest.len(), |i| i + 1);
        let (line, after) = rest.split_at(end);
        rest = after;
        // The line is a C string to glibc: it ends at a NUL.
        let line = line
            .iter()
            .position(|&b| b == 0)
            .map_or(line, |i| line.get(..i).unwrap_or_default());
        // Skip an empty line or a comment.
        let first = line.iter().position(|&b| !is_space(b));
        if first.is_none_or(|i| line.get(i) == Some(&b'#')) {
            continue;
        }
        // A writable copy to cut in place, as glibc cuts its buffer.
        let Some(mut copy) = Buf::with_nul(line) else {
            return -1;
        };
        // SAFETY: the copy's own `len` bytes, the NUL among them.
        let b = unsafe { core::slice::from_raw_parts_mut(copy.ptr, copy.len) };
        let mut p = 0usize;
        // The host, lower-cased.
        while let Some(c) = b.get_mut(p).filter(|c| **c != 0 && !is_space(**c)) {
            *c = c.to_ascii_lowercase();
            p += 1;
        }
        let user_at = if matches!(b.get(p), Some(b' ' | b'\t')) {
            if let Some(c) = b.get_mut(p) {
                *c = 0;
            }
            p += 1;
            while b.get(p).is_some_and(|&c| c != 0 && is_space(c)) {
                p += 1;
            }
            let at = p;
            while b.get(p).is_some_and(|&c| c != 0 && !is_space(c)) {
                p += 1;
            }
            at
        } else {
            p
        };
        if let Some(c) = b.get_mut(p) {
            *c = 0;
        }
        // A line beginning with a blank has an empty host, and ends the file.
        if b.first() == Some(&0) {
            break;
        }
        let host = copy.ptr.cast_const();
        // SAFETY: `user_at` is within the copy, at a C string.
        let user_entry = unsafe { copy.ptr.add(user_at) }.cast_const();
        // SAFETY: as above.
        let user = if unsafe { *user_entry } == 0 {
            luser
        } else {
            user_entry
        };
        // SAFETY: C strings.
        let ucheck = unsafe { icheckuser(user, ruser) };
        if ucheck != 0 || b.first() == Some(&b'-') {
            // SAFETY: C strings, and the caller's address.
            let hcheck = unsafe { checkhost_sa(ra, ralen, host, rhost) };
            if hcheck < 0 {
                break;
            }
            if hcheck > 0 && ucheck > 0 {
                return 0;
            }
            if hcheck > 0 && ucheck < 0 {
                break;
            }
        }
        // `copy` goes here, as glibc reuses its buffer.
        drop(core::mem::replace(&mut copy, Buf::EMPTY));
    }
    -1
}

/// `strcmp (a, b) == 0`, of two C strings.
///
/// # Safety
///
/// Both are C strings.
unsafe fn same(a: *const u8, b: *const u8) -> bool {
    // SAFETY: C strings, this function's contract.
    unsafe { c_text(a) == c_text(b) }
}

/// glibc's `__checkhost_sa`: 1 when the entry `lhost` names the host at
/// `ra` -- by netgroup, `+`, its numeric address, or an address of the name
/// -- -1 when `-lhost` does, 0 when neither.
///
/// # Safety
///
/// As [`ruserok2_sa`]; `lhost` is a C string.
unsafe fn checkhost_sa(
    ra: *const Sockaddr,
    ralen: SocklenT,
    lhost: *const u8,
    rhost: *const u8,
) -> i32 {
    // SAFETY: a C string.
    let text = unsafe { c_text(lhost) };
    if text.starts_with(b"+@") {
        // SAFETY: C strings, NULL for the fields not asked about.
        return unsafe {
            crate::netgroup::innetgr(lhost.add(2), rhost, core::ptr::null(), core::ptr::null())
        };
    }
    if text.starts_with(b"-@") {
        // SAFETY: as above.
        return -unsafe {
            crate::netgroup::innetgr(lhost.add(2), rhost, core::ptr::null(), core::ptr::null())
        };
    }
    let mut negate = 1;
    let mut lhost = lhost;
    if text.starts_with(b"-") {
        negate = -1;
        // SAFETY: past the `-`, still the C string.
        lhost = unsafe { lhost.add(1) };
    } else if text == b"+" {
        return 1;
    }
    // SAFETY: a C string.
    let name = unsafe { c_text(lhost) };
    let mut raddr = [0u8; ADDRSTRLEN];
    // SAFETY: the caller's address; `raddr` is writable for its length.
    let numeric = unsafe {
        crate::gai::getnameinfo(
            ra,
            ralen,
            raddr.as_mut_ptr(),
            ADDRSTRLEN as SocklenT,
            core::ptr::null_mut(),
            0,
            NI_NUMERICHOST,
        )
    } == 0;
    if numeric
        && raddr
            .iter()
            .position(|&b| b == 0)
            .and_then(|n| raddr.get(..n))
            == Some(name)
    {
        return negate;
    }
    // Better be a host name.
    let mut hints = zeroed_addrinfo();
    // SAFETY: the caller's address, its family first.
    hints.ai_family = i32::from(unsafe { *ra.cast::<u16>() });
    let mut res0: *mut Addrinfo = core::ptr::null_mut();
    // SAFETY: a C string; live locals.
    if unsafe { crate::gai::getaddrinfo(lhost, core::ptr::null(), &raw const hints, &raw mut res0) }
        != 0
    {
        return 0;
    }
    let list = AiList(res0);
    let mut res = list.0;
    while !res.is_null() {
        // SAFETY: an entry of the list.
        let entry = unsafe { &*res };
        let len = usize::try_from(entry.ai_addrlen).unwrap_or(0);
        // glibc's `memcmp (res->ai_addr, ra, res->ai_addrlen)`: the caller's
        // address is a whole `sockaddr` of the family, as long.
        // SAFETY: `len` bytes of each: the entry's, and the caller's address
        // of the same family.
        if entry.ai_family == hints.ai_family
            && unsafe {
                core::slice::from_raw_parts(entry.ai_addr.cast::<u8>(), len)
                    == core::slice::from_raw_parts(ra.cast::<u8>(), len)
            }
        {
            return negate;
        }
        res = entry.ai_next;
    }
    0
}

/// glibc's `__icheckuser`: 1 when the entry `luser` takes in `ruser` -- by
/// netgroup, `+` or name -- -1 when `-luser` shuts them out, 0 when neither.
///
/// # Safety
///
/// Both are C strings.
unsafe fn icheckuser(luser: *const u8, ruser: *const u8) -> i32 {
    // SAFETY: a C string.
    let text = unsafe { c_text(luser) };
    if text.starts_with(b"+@") {
        // SAFETY: C strings, NULL for the fields not asked about.
        return unsafe {
            crate::netgroup::innetgr(luser.add(2), core::ptr::null(), ruser, core::ptr::null())
        };
    }
    if text.starts_with(b"-@") {
        // SAFETY: as above.
        return -unsafe {
            crate::netgroup::innetgr(luser.add(2), core::ptr::null(), ruser, core::ptr::null())
        };
    }
    if text.starts_with(b"-") {
        // SAFETY: past the `-`, still the C string.
        return -i32::from(unsafe { same(luser.add(1), ruser) });
    }
    if text == b"+" {
        return 1;
    }
    // SAFETY: C strings.
    i32::from(unsafe { same(ruser, luser) })
}
