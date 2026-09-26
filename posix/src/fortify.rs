//! `_FORTIFY_SOURCE`'s failure path, and the rule for when a fortified call
//! fails rather than clamps.
//!
//! A program compiled against glibc's headers with `-D_FORTIFY_SOURCE` calls
//! `__memcpy_chk(dst, src, n, objsize)` where its source says
//! `memcpy(dst, src, n)`. `objsize` is the compiler's
//! `__builtin_object_size(dst)`: how large the destination object is, or
//! `(size_t)-1` when the compiler cannot tell -- which fits every operation, so
//! an unknown object is never refused. glibc's `_chk` entry points compare the
//! operation with the object and, when it does not fit, call [`__chk_fail`]:
//! a message on standard error and `abort()`.
//!
//! Nothing this project builds calls these. `zig cc` compiles against musl's
//! headers, and musl implements no `_FORTIFY_SOURCE`. They are here for an
//! object compiled against glibc's headers and linked with this libc, which
//! should get the protection it was built to have.
//!
//! # Abort, or clamp
//!
//! glibc aborts in every `_chk`. This libc aborts where glibc does for the
//! memory and string **copies**, and **clamps** the operation to the object
//! everywhere else:
//!
//! | entry points | overflow means | here |
//! |---|---|---|
//! | `__memcpy_chk` `__memmove_chk` `__mempcpy_chk` `__memset_chk` `__strcpy_chk` `__stpcpy_chk` `__strncpy_chk` `__stpncpy_chk` `__strcat_chk` `__strncat_chk` | abort, as glibc | a copy of fewer bytes than asked is a different bug, not a safe one: the caller goes on as if the rest were there |
//! | the printf family (`fortify_printf.rs`) | truncate to `objsize` | truncation is part of `snprintf`'s contract, and the return value still says how long the output wanted to be |
//! | `__read_chk` `__pread_chk` `__pread64_chk` `__fread_chk` `__fgets_chk` | read at most `objsize` | a short read is part of every one of these calls' contracts, so callers already handle it |
//! | `__getcwd_chk` `__readlink_chk` `__readlinkat_chk` | ask with at most `objsize` | `ERANGE` and truncation are, again, answers these functions already give |
//! | `__fdelt_chk` (`FD_SET` and friends) | abort, as glibc | there is no smaller call: the bit is inside the `fd_set` or it is not |
//! | `__explicit_bzero_chk`, `__poll_chk`, `__ppoll_chk` | abort, as glibc | a partial wipe is a secret left behind; `poll` on fewer descriptors ignores the rest |
//! | `__recv_chk` `__recvfrom_chk` `__gethostname_chk` `__getlogin_r_chk` `__ttyname_r_chk` `__ptsname_r_chk` `__confstr_chk` `__getgroups_chk` | ask with at most `objsize` | a short receive, a truncated name, `ERANGE`/`EINVAL`: answers each call already gives |
//! | `__open_2` `__open64_2` `__openat_2` `__openat64_2` | abort, as glibc, when the flags need a mode | not a size check: `open(path, O_CREAT)` with no mode would create the file with whatever was in a register |
//!
//! | the wide copies (`__wmemcpy_chk` … `__wcsncat_chk`, `wchar.rs`) | abort, as glibc | as the narrow copies |
//! | the multibyte conversions (`__mbstowcs_chk`, `__wcstombs_chk` and their `r`/`nr` forms, `__wcrtomb_chk`, `__wctomb_chk`, `wchar.rs`) | abort, as glibc | the caller tests the length it asked for to detect truncation, so a clamp would hide a cut-short conversion |
//! | `__fgetws_chk` (`wchar.rs`), `__vswprintf_chk`/`__swprintf_chk` (`fortify_printf.rs`) | clamp | as `__fgets_chk` and the narrow printf family |
//!
//! The rule is: **clamp when the smaller operation is still a correct call of
//! the function -- a result its callers must already handle -- and abort when
//! it would not be.** Clamping never writes past the object either way; the
//! difference is whether the program can carry on correctly afterwards.
//! design-decisions.md §1105.

/// The message glibc prints before aborting, since 2.34.
const OVERFLOW_MESSAGE: &[u8] = b"*** buffer overflow detected ***: terminated\n";

/// glibc's message for `open` with flags that need a mode and none given.
const OPEN_MESSAGE: &[u8] =
    b"*** invalid open call: O_CREAT or O_TMPFILE without mode ***: terminated\n";

/// glibc's `__fortify_fail`: write `message` -- already in glibc's
/// `*** ... ***: terminated` form -- to standard error, and abort.
fn fortify_fail(message: &[u8]) -> ! {
    // There is no one to report a failed write to: the process is ending.
    let _ = crate::file::write(2, message.as_ptr(), message.len());
    crate::unistd::abort()
}

/// glibc's `__chk_fail`: a fortified call's operation did not fit its object.
///
/// Writes glibc's message to standard error and aborts. It never returns: the
/// caller was about to write past the end of an object, and nothing it could
/// do next is safe. Fortified objects call this directly as well, which is why
/// it is exported.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __chk_fail() -> ! {
    fortify_fail(OVERFLOW_MESSAGE)
}

/// Does an operation on `len` bytes fit an object of `objsize` bytes?
///
/// `objsize == usize::MAX` -- the compiler's "unknown" -- fits everything.
#[inline]
#[must_use]
pub(crate) const fn fits(len: usize, objsize: usize) -> bool {
    len <= objsize
}

/// `n` bytes and a terminator: does that fit `objsize`? What every string copy
/// needs, and the terminator is what makes `+ 1` overflow-checked here rather
/// than at each caller.
#[inline]
#[must_use]
pub(crate) const fn fits_with_terminator(n: usize, objsize: usize) -> bool {
    match n.checked_add(1) {
        Some(total) => total <= objsize,
        None => false,
    }
}

/// The room `__strcat_chk` / `__strncat_chk` need: the length of the string
/// already in `dest`, found without reading past `objsize` bytes of it, plus
/// `append` bytes and a terminator. `false` if `dest` holds no terminator
/// within the object -- glibc aborts then too, since the concatenation would
/// start past the end.
///
/// # Safety
///
/// `dest` must be readable up to its first NUL or for `objsize` bytes,
/// whichever comes first.
#[must_use]
pub(crate) unsafe fn concatenation_fits(dest: *const u8, append: usize, objsize: usize) -> bool {
    // SAFETY: reads at most `objsize` bytes of `dest`, stopping at a NUL
    // (this function's contract).
    let have = unsafe { crate::string::strnlen(dest, objsize) };
    if have >= objsize {
        return false;
    }
    match have.checked_add(append) {
        Some(total) => fits_with_terminator(total, objsize),
        None => false,
    }
}

/// `__fdelt_chk(d)` -- glibc's `FD_SET`, `FD_CLR` and `FD_ISSET` under
/// `_FORTIFY_SOURCE`: the index of the `long` in an `fd_set` that holds
/// descriptor `d`'s bit, after checking that the bit is inside the structure at
/// all. Every fortified program that uses `select` calls this.
///
/// glibc aborts for `d < 0` or `d >= FD_SETSIZE`: the macro would otherwise
/// write outside the caller's `fd_set`. The bound here is the structure's size,
/// 1024 bits (`crate::poll::FD_SET_BITS`), which is glibc's `FD_SETSIZE`. It is
/// not this libc's descriptor limit of 256: a descriptor between the two
/// cannot exist, but setting its bit writes inside the caller's object and is
/// harmless.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __fdelt_chk(d: i64) -> i64 {
    match fd_set_word(d) {
        Some(word) => word,
        None => __chk_fail(),
    }
}

/// The same function under the second name glibc exports it by; its headers
/// call this one when they can already see that `d` is out of range.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __fdelt_warn(d: i64) -> i64 {
    __fdelt_chk(d)
}

/// The index `__fdelt_chk` returns, or `None` for a descriptor outside the
/// `fd_set` structure. 64 bits to a `long` on x86_64.
fn fd_set_word(d: i64) -> Option<i64> {
    let bits = i64::try_from(crate::poll::FD_SET_BITS).ok()?;
    if (0..bits).contains(&d) {
        d.checked_div(64)
    } else {
        None
    }
}

// -- `open` without the mode its flags need ------------------------------------

/// Do `oflag` need `open`'s third argument? glibc's `__OPEN_NEEDS_MODE`:
/// `O_CREAT`, or all of `O_TMPFILE`'s own bit (`O_TMPFILE` also carries
/// `O_DIRECTORY`, which on its own needs no mode).
const fn open_needs_mode(oflag: i32) -> bool {
    const TMPFILE_BIT: i32 = crate::fcntl::O_TMPFILE & !crate::fcntl::O_DIRECTORY;
    oflag & crate::fcntl::O_CREAT != 0 || oflag & TMPFILE_BIT == TMPFILE_BIT
}

/// `__open_2(path, oflag)` -- what a fortified `open` with two arguments
/// becomes. Such a call cannot pass a mode, so flags that need one abort, as
/// in glibc: the file would otherwise be created with whatever permission bits
/// happened to be in the register the third argument travels in.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __open_2(path: *const u8, oflag: i32) -> i32 {
    if open_needs_mode(oflag) {
        fortify_fail(OPEN_MESSAGE);
    }
    crate::file::open(path, oflag, 0)
}

/// `__open64_2`: [`__open_2`]; `off_t` is 64 bits here either way.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __open64_2(path: *const u8, oflag: i32) -> i32 {
    __open_2(path, oflag)
}

/// `__openat_2(dirfd, path, oflag)`: [`__open_2`] for `openat`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __openat_2(dirfd: i32, path: *const u8, oflag: i32) -> i32 {
    if open_needs_mode(oflag) {
        fortify_fail(OPEN_MESSAGE);
    }
    crate::file::openat(dirfd, path, oflag, 0)
}

/// `__openat64_2`: [`__openat_2`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __openat64_2(dirfd: i32, path: *const u8, oflag: i32) -> i32 {
    __openat_2(dirfd, path, oflag)
}

// -- calls that abort ---------------------------------------------------------------

/// `__explicit_bzero_chk(dst, len, dstlen)`: aborts when `len > dstlen`. A
/// wipe cut short would leave the secret it exists to erase.
///
/// # Safety
///
/// As `explicit_bzero`: `dst` must be valid for `len` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __explicit_bzero_chk(dst: *mut u8, len: usize, dstlen: usize) {
    if !fits(len, dstlen) {
        __chk_fail();
    }
    // SAFETY: forwarded from this function's contract.
    unsafe { crate::string::explicit_bzero(dst, len) }
}

/// Does an array of `nfds` `struct pollfd` fit `fdslen` bytes?
fn pollfds_fit(nfds: u64, fdslen: usize) -> bool {
    let each = core::mem::size_of::<crate::poll::Pollfd>();
    usize::try_from(nfds)
        .ok()
        .and_then(|n| n.checked_mul(each))
        .is_some_and(|bytes| fits(bytes, fdslen))
}

/// `__poll_chk(fds, nfds, timeout, fdslen)`: aborts when `nfds` entries do not
/// fit `fdslen` bytes. Clamping would poll fewer descriptors than the caller
/// is waiting on, and report nothing about the rest.
///
/// # Safety
///
/// As `poll`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __poll_chk(
    fds: *mut crate::poll::Pollfd,
    nfds: u64,
    timeout: i32,
    fdslen: usize,
) -> i32 {
    if !pollfds_fit(nfds, fdslen) {
        __chk_fail();
    }
    // SAFETY: forwarded from this function's contract.
    unsafe { crate::poll::poll(fds, nfds, timeout) }
}

/// `__ppoll_chk`: [`__poll_chk`] for `ppoll`.
///
/// # Safety
///
/// As `ppoll`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __ppoll_chk(
    fds: *mut crate::poll::Pollfd,
    nfds: u64,
    timeout: *const crate::stat::Timespec,
    sigmask: *const u64,
    fdslen: usize,
) -> i32 {
    if !pollfds_fit(nfds, fdslen) {
        __chk_fail();
    }
    // SAFETY: forwarded from this function's contract.
    unsafe { crate::poll::ppoll(fds, nfds, timeout, sigmask) }
}

// -- calls that clamp -------------------------------------------------------------

/// `__recv_chk(fd, buf, n, buflen, flags)`: receives at most `buflen` bytes. A
/// short receive is part of `recv`'s contract.
///
/// # Safety
///
/// As `recv`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __recv_chk(
    fd: i32,
    buf: *mut u8,
    n: usize,
    buflen: usize,
    flags: i32,
) -> isize {
    // SAFETY: forwarded; the length only shrinks.
    unsafe { crate::socket::recv(fd, buf, n.min(buflen), flags) }
}

/// `__recvfrom_chk(fd, buf, n, buflen, flags, addr, addrlen)`: receives at
/// most `buflen` bytes, as [`__recv_chk`].
///
/// # Safety
///
/// As `recvfrom`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __recvfrom_chk(
    fd: i32,
    buf: *mut u8,
    n: usize,
    buflen: usize,
    flags: i32,
    addr: *mut crate::socket::Sockaddr,
    addrlen: *mut crate::socket::SocklenT,
) -> isize {
    // SAFETY: forwarded; the length only shrinks.
    unsafe { crate::socket::recvfrom(fd, buf, n.min(buflen), flags, addr, addrlen) }
}

/// `__gethostname_chk(buf, len, buflen)`: asks with at most `buflen` bytes;
/// a name that does not fit is truncated, as POSIX allows `gethostname` to do.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __gethostname_chk(buf: *mut u8, len: usize, buflen: usize) -> i32 {
    crate::unistd::gethostname(buf, len.min(buflen))
}

/// `__getlogin_r_chk(buf, buflen, nreal)`: asks with at most `nreal`, the
/// object's real size; a name that does not fit is `ERANGE`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __getlogin_r_chk(buf: *mut u8, buflen: usize, nreal: usize) -> i32 {
    crate::pwd::getlogin_r(buf, buflen.min(nreal))
}

/// `__ttyname_r_chk(fd, buf, buflen, nreal)`: asks with at most `nreal`; a
/// name that does not fit is `ERANGE`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __ttyname_r_chk(fd: i32, buf: *mut u8, buflen: usize, nreal: usize) -> i32 {
    crate::ioctl::ttyname_r(fd, buf, buflen.min(nreal))
}

/// `__ptsname_r_chk(fd, buf, buflen, nreal)`: asks with at most `nreal`; a
/// name that does not fit is `ERANGE`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __ptsname_r_chk(fd: i32, buf: *mut u8, buflen: usize, nreal: usize) -> i32 {
    crate::ioctl::ptsname_r(fd, buf, buflen.min(nreal))
}

/// `__confstr_chk(name, buf, len, buflen)`: asks with at most `buflen`;
/// `confstr` truncates and still returns the length it needed.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __confstr_chk(name: i32, buf: *mut u8, len: usize, buflen: usize) -> usize {
    crate::unistd::confstr(name, buf, len.min(buflen))
}

/// `__getgroups_chk(size, list, listlen)`: asks for at most as many groups as
/// `listlen` bytes hold; `getgroups` then answers `EINVAL` if there are more,
/// as it would for any small array.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __getgroups_chk(size: i32, list: *mut crate::types::GidT, listlen: usize) -> i32 {
    // `size_of::<GidT>()` is 4, so the division cannot fail.
    let room = listlen
        .checked_div(core::mem::size_of::<crate::types::GidT>())
        .unwrap_or(0);
    let size = if size < 0 {
        size
    } else {
        size.min(i32::try_from(room).unwrap_or(i32::MAX))
    };
    crate::unistd::getgroups(size, list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_object_fits_everything() {
        assert!(fits(usize::MAX, usize::MAX));
        assert!(fits(0, usize::MAX));
        assert!(fits_with_terminator(usize::MAX - 1, usize::MAX));
    }

    #[test]
    fn fits_is_inclusive_at_the_object_size() {
        assert!(fits(16, 16));
        assert!(!fits(17, 16));
        assert!(fits(0, 0));
        assert!(!fits(1, 0));
    }

    #[test]
    fn a_string_needs_room_for_its_terminator() {
        assert!(fits_with_terminator(15, 16));
        assert!(!fits_with_terminator(16, 16));
        assert!(!fits_with_terminator(0, 0));
        assert!(
            !fits_with_terminator(usize::MAX, usize::MAX),
            "the +1 must not wrap"
        );
    }

    #[test]
    fn concatenation_counts_what_is_already_there() {
        let mut buf = [0u8; 8];
        buf[..3].copy_from_slice(b"abc");
        // SAFETY: `buf` is 8 bytes, NUL-terminated at 3.
        unsafe {
            assert!(concatenation_fits(buf.as_ptr(), 4, 8), "abc + 4 + NUL = 8");
            assert!(!concatenation_fits(buf.as_ptr(), 5, 8), "abc + 5 + NUL = 9");
            assert!(concatenation_fits(buf.as_ptr(), 0, 4));
            assert!(
                !concatenation_fits(buf.as_ptr(), 0, 3),
                "no room for even the NUL"
            );
        }
    }

    /// A destination with no terminator inside the object cannot be appended
    /// to: the append would begin past its end.
    #[test]
    fn concatenation_refuses_an_unterminated_destination() {
        let buf = [b'x'; 8];
        // SAFETY: `strnlen` reads at most the 8 bytes `buf` has.
        assert!(!unsafe { concatenation_fits(buf.as_ptr(), 0, 8) });
    }

    #[test]
    fn fd_set_word_indexes_the_structure_and_refuses_outside_it() {
        assert_eq!(fd_set_word(0), Some(0));
        assert_eq!(fd_set_word(63), Some(0));
        assert_eq!(fd_set_word(64), Some(1));
        assert_eq!(fd_set_word(255), Some(3), "this libc's last descriptor");
        assert_eq!(fd_set_word(1023), Some(15), "the structure's last bit");
        assert_eq!(fd_set_word(1024), None);
        assert_eq!(fd_set_word(-1), None);
        assert_eq!(fd_set_word(i64::MIN), None);
        // In range, the entry point is the helper.
        assert_eq!(__fdelt_chk(200), 3);
        assert_eq!(__fdelt_warn(1000), 15);
    }

    #[test]
    fn open_needs_a_mode_for_create_and_tmpfile_only() {
        use crate::fcntl::{O_CREAT, O_DIRECTORY, O_RDONLY, O_RDWR, O_TMPFILE, O_TRUNC, O_WRONLY};
        assert!(open_needs_mode(O_CREAT | O_WRONLY));
        assert!(open_needs_mode(O_TMPFILE | O_RDWR));
        assert!(!open_needs_mode(O_RDONLY));
        assert!(!open_needs_mode(O_WRONLY | O_TRUNC));
        assert!(
            !open_needs_mode(O_DIRECTORY),
            "O_TMPFILE's other half alone"
        );
    }

    #[test]
    fn pollfds_fit_counts_whole_entries() {
        let each = core::mem::size_of::<crate::poll::Pollfd>();
        assert!(pollfds_fit(2, 2 * each));
        assert!(!pollfds_fit(3, 2 * each));
        assert!(pollfds_fit(0, 0));
        assert!(pollfds_fit(u64::from(u32::MAX), usize::MAX), "unknown size");
        assert!(
            !pollfds_fit(u64::MAX, usize::MAX - 1),
            "the product must not wrap"
        );
    }

    /// The clamping entry points pass the smaller length through: a name that
    /// does not fit its object comes back as the call's own answer for a small
    /// buffer, never as a write past it.
    #[test]
    fn the_clamping_calls_never_write_past_the_object() {
        let mut buf = [0xeeu8; 4];
        // Claims 64 bytes; the object is 3, so at most 3 are written.
        let _ = __gethostname_chk(buf.as_mut_ptr(), 64, 3);
        assert_eq!(buf[3], 0xee, "gethostname");
        buf = [0xee; 4];
        let _ = __confstr_chk(crate::unistd::_CS_PATH, buf.as_mut_ptr(), 64, 3);
        assert_eq!(buf[3], 0xee, "confstr");
        // confstr still reports how long the value wanted to be.
        assert!(__confstr_chk(crate::unistd::_CS_PATH, buf.as_mut_ptr(), 64, 3) > 3);
    }

    #[test]
    fn the_open_message_is_glibcs() {
        assert_eq!(
            OPEN_MESSAGE,
            b"*** invalid open call: O_CREAT or O_TMPFILE without mode ***: terminated\n"
        );
    }

    #[test]
    fn the_message_is_glibcs() {
        assert_eq!(
            OVERFLOW_MESSAGE,
            b"*** buffer overflow detected ***: terminated\n"
        );
    }
}
