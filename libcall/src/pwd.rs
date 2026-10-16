//! `getpwuid(3)` through the C library: an account's entry in the password
//! database, found by uid.
//!
//! `struct passwd` is glibc's x86_64 layout, which SlateOS's C library keeps.
//! Its strings live in the library's static storage until the next lookup,
//! so what a caller reads is copied into its own buffer before returning.

/// `getpwuid (uid)`'s home directory (`pw_dir`), copied into `buf`: how many
/// bytes it took, or `None` when the database has no such uid (or could not
/// be read -- the C library tells those apart only by `errno`), when the
/// entry names no home, or when `buf` is too short for it. Off Unix there is
/// no database: always `None`.
#[must_use]
pub fn home_directory(uid: u32, buf: &mut [u8]) -> Option<usize> {
    imp::home_directory(uid, buf)
}

#[cfg(unix)]
mod imp {
    use core::ffi::{CStr, c_char};

    /// `struct passwd`, x86_64.
    #[repr(C)]
    struct Raw {
        pw_name: *mut c_char,
        pw_passwd: *mut c_char,
        pw_uid: u32,
        pw_gid: u32,
        pw_gecos: *mut c_char,
        pw_dir: *mut c_char,
        pw_shell: *mut c_char,
    }

    unsafe extern "C" {
        fn getpwuid(uid: u32) -> *mut Raw;
    }

    pub fn home_directory(uid: u32, buf: &mut [u8]) -> Option<usize> {
        // SAFETY: `getpwuid` returns null or a pointer to a `struct passwd`
        // in the library's static storage, whose `pw_dir` is null or a
        // NUL-terminated string; it is copied out before anything else can
        // call into the passwd family and reuse that storage.
        let dir = unsafe {
            let raw = getpwuid(uid);
            if raw.is_null() || (*raw).pw_dir.is_null() {
                return None;
            }
            CStr::from_ptr((*raw).pw_dir).to_bytes()
        };
        let dst = buf.get_mut(..dir.len())?;
        dst.copy_from_slice(dir);
        Some(dir.len())
    }
}

#[cfg(not(unix))]
mod imp {
    pub fn home_directory(_uid: u32, _buf: &mut [u8]) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]
    use super::*;

    #[cfg(unix)]
    #[test]
    fn root_has_a_home_and_a_short_buffer_is_refused() {
        let mut buf = [0u8; 4096];
        let n = home_directory(0, &mut buf).expect("every Unix has a uid 0");
        assert!(n > 0 && buf[0] == b'/', "{:?}", &buf[..n]);
        assert_eq!(home_directory(0, &mut buf[..n - 1]), None);
    }

    #[cfg(not(unix))]
    #[test]
    fn there_is_no_database_off_unix() {
        assert_eq!(home_directory(0, &mut [0u8; 64]), None);
    }

    #[test]
    fn an_unused_uid_has_no_entry() {
        assert_eq!(home_directory(4_000_000_000, &mut [0u8; 64]), None);
    }
}
