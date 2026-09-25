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
//!
//! The rule is: **clamp when the smaller operation is still a correct call of
//! the function -- a result its callers must already handle -- and abort when
//! it would not be.** Clamping never writes past the object either way; the
//! difference is whether the program can carry on correctly afterwards.
//! design-decisions.md §1105.

/// The message glibc prints before aborting, since 2.34.
const OVERFLOW_MESSAGE: &[u8] = b"*** buffer overflow detected ***: terminated\n";

/// glibc's `__chk_fail`: a fortified call's operation did not fit its object.
///
/// Writes glibc's message to standard error and aborts. It never returns: the
/// caller was about to write past the end of an object, and nothing it could
/// do next is safe. Fortified objects call this directly as well, which is why
/// it is exported.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __chk_fail() -> ! {
    // There is no one to report a failed write to: the process is ending.
    let _ = crate::file::write(2, OVERFLOW_MESSAGE.as_ptr(), OVERFLOW_MESSAGE.len());
    crate::unistd::abort()
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
    fn the_message_is_glibcs() {
        assert_eq!(
            OVERFLOW_MESSAGE,
            b"*** buffer overflow detected ***: terminated\n"
        );
    }
}
