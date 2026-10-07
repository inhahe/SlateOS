//! libxcrypt's `crypt_gensalt_rn`: the setting a new password is hashed under,
//! made by the C library the program links.
//!
//! # Why through the C symbol
//!
//! Making a setting is arithmetic over the caller's bytes when the caller
//! hands over the random ones -- but `posix` offers it only as the C entry
//! points, which report a refusal through `errno`, and given no bytes it draws
//! its own from `arc4random_buf`. Reached as a Rust dependency, that is the
//! rlib copy of the library: its `errno` is one the program cannot read, and
//! its entropy source is the stubbed one design-decisions §768 was written
//! about. So it is reached the way a C program reaches it, and the `errno` is
//! returned rather than fetched (see the crate docs).
//!
//! # Which library answers
//!
//! * **SlateOS:** `libc.a`'s, which is `posix/src/gensalt.rs` -- lane D's port
//!   of libxcrypt's, held to libxcrypt's own answers by `gensalt_oracle.txt`.
//! * **A Linux development host:** libxcrypt itself, which glibc does not
//!   carry: it is `libcrypt`, the library shadow-utils calls. A differential
//!   harness there therefore compares a program with its upstream over the
//!   same code.
//! * **Anywhere else** there is no such library, and the answer is
//!   [`crate::ENOSYS`] -- never a setting made some other way, which a caller
//!   would store as a password.

use core::ffi::CStr;

/// `CRYPT_GENSALT_OUTPUT_SIZE`: room for any setting, its NUL included.
pub const GENSALT_OUTPUT_SIZE: usize = 192;

/// SlateOS's `libc.a`.
///
/// `target_vendor` rather than `target_os`: SlateOS's target says `linux`, so
/// only the vendor tells it from a Linux host, whose library this is not.
#[cfg(target_vendor = "slateos")]
mod sys {
    unsafe extern "C" {
        pub fn crypt_gensalt_rn(
            prefix: *const u8,
            count: u64,
            rbytes: *const u8,
            nrbytes: i32,
            output: *mut u8,
            output_size: i32,
        ) -> *mut u8;
    }
}

/// A Linux host's libxcrypt, in `libcrypt` -- linked here, by the one crate
/// that names it, so that no program has to know which library holds it.
#[cfg(all(target_os = "linux", not(target_vendor = "slateos")))]
mod sys {
    #[link(name = "crypt")]
    unsafe extern "C" {
        pub fn crypt_gensalt_rn(
            prefix: *const u8,
            count: u64,
            rbytes: *const u8,
            nrbytes: i32,
            output: *mut u8,
            output_size: i32,
        ) -> *mut u8;
    }
}

/// `crypt_gensalt_rn (prefix, count, rbytes, nrbytes, output, sizeof output)`:
/// the setting `prefix` names (`$6$`, `$y$`, `$1$` ...), at cost `count` -- 0
/// for the method's default -- salted with `rbytes`, written into `output`
/// with its NUL. Returns the setting's length, without the NUL.
///
/// The random bytes are the caller's, always: the library's own source is
/// never asked, so what salts a password is whatever the caller drew and
/// checked.
///
/// # Errors
///
/// The `errno` of a refusal, as the library set it -- measured from
/// libxcrypt 4.4.36: `EINVAL` for a method it does not know, a cost it will
/// not take (`$y$` at 99) or too few random bytes (one, for `$6$`); `ERANGE`
/// for a setting with no room in `output`. [`crate::EINVAL`] for more random
/// bytes than an `int` counts, which the call could not be made with.
/// [`crate::ENOSYS`] where there is no such library.
pub fn gensalt(
    prefix: &CStr,
    count: u64,
    rbytes: &[u8],
    output: &mut [u8; GENSALT_OUTPUT_SIZE],
) -> Result<usize, i32> {
    #[cfg(target_os = "linux")]
    {
        let nrbytes = i32::try_from(rbytes.len()).map_err(|_| crate::EINVAL)?;
        // 192 fits an `int`; the conversion only spells that out.
        let room = i32::try_from(GENSALT_OUTPUT_SIZE).map_err(|_| crate::EINVAL)?;
        // SAFETY: `prefix` is a C string, its terminator guaranteed by
        // `CStr`; `rbytes` is readable for the `nrbytes` bytes it holds, and
        // `output` writable for its `room` bytes, which is exactly what the
        // library is told. It keeps none of the three pointers past the call.
        let made = unsafe {
            sys::crypt_gensalt_rn(
                prefix.as_ptr().cast::<u8>(),
                count,
                rbytes.as_ptr(),
                nrbytes,
                output.as_mut_ptr(),
                room,
            )
        };
        if made.is_null() {
            return Err(crate::last_errno());
        }
        // A setting always ends in its NUL inside the buffer; the length of
        // the whole buffer is the answer only if a library broke that.
        Ok(output.iter().position(|&b| b == 0).unwrap_or(output.len()))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (prefix, count, rbytes, output);
        Err(crate::ENOSYS)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::{GENSALT_OUTPUT_SIZE, gensalt};
    use std::vec::Vec;

    #[test]
    fn the_size_agrees_with_posix() {
        assert_eq!(
            GENSALT_OUTPUT_SIZE,
            posix::gensalt::CRYPT_GENSALT_OUTPUT_SIZE
        );
    }

    /// The library's answers where there is one -- libxcrypt's, on the Linux
    /// host these run on, measured with a C program calling it with the same
    /// bytes -- and `ENOSYS` where there is not.
    #[test]
    fn a_setting_is_the_librarys() {
        let made = |prefix: &core::ffi::CStr, count: u64| -> Result<Vec<u8>, i32> {
            let mut out = [0u8; GENSALT_OUTPUT_SIZE];
            gensalt(prefix, count, &[0x5a; 16], &mut out)
                .map(|len| out.get(..len).unwrap_or_default().to_vec())
        };
        if cfg!(target_os = "linux") {
            // The default cost is left unstated; a cost under SHA-512's least
            // is raised to it.
            assert_eq!(made(c"$6$", 0), Ok(b"$6$OdZKOdZKOdZKOdZK".to_vec()));
            assert_eq!(
                made(c"$6$", 10),
                Ok(b"$6$rounds=1000$OdZKOdZKOdZKOdZK".to_vec())
            );
        } else {
            assert_eq!(made(c"$6$", 0), Err(crate::ENOSYS));
        }
    }

    /// Measured from libxcrypt 4.4.36: each of these is `EINVAL`.
    #[test]
    fn a_refusal_carries_the_librarys_errno() {
        let mut out = [0u8; GENSALT_OUTPUT_SIZE];
        let refused = [
            gensalt(c"$nosuch$", 0, &[0x5a; 16], &mut out),
            gensalt(c"$6$", 0, &[0x5a; 1], &mut out),
            gensalt(c"$y$", 99, &[0x5a; 16], &mut out),
        ];
        let want = if cfg!(target_os = "linux") {
            crate::EINVAL
        } else {
            crate::ENOSYS
        };
        assert_eq!(refused, [Err(want); 3]);
    }
}
