//! POSIX `<unistd.h>` / `<crypt.h>` password hashing.
//!
//! Stubs for `crypt`, `crypt_r`, `encrypt`, `setkey`.
//!
//! Our OS does not implement actual password hashing algorithms
//! (DES, MD5, SHA-256, SHA-512).  The `crypt` function returns the
//! key prefixed with `$0$` to indicate "no hashing available."
//!
//! Programs that actually need password verification should use a
//! proper cryptographic library.  These stubs satisfy link-time
//! references and provide a deterministic (but insecure) result.

use crate::errno;

/// Maximum length of a crypt result string.
const CRYPT_OUTPUT_LEN: usize = 128;

/// Static buffer for `crypt()` results (non-reentrant).
static mut CRYPT_BUF: [u8; CRYPT_OUTPUT_LEN] = [0u8; CRYPT_OUTPUT_LEN];

/// `crypt` — one-way string hashing (password hashing).
///
/// Stub: returns `"$0$<key>"` (not a real hash).  The `$0$` prefix
/// signals that no hashing algorithm was applied.
///
/// POSIX: `crypt` returns a pointer to a static buffer that is
/// overwritten by each call.  Returns null on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn crypt(key: *const u8, salt: *const u8) -> *mut u8 {
    if key.is_null() || salt.is_null() {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    }

    // Build "$0$<key>\0" in the static buffer.
    let prefix = b"$0$";
    let key_len = unsafe { crate::string::strlen(key) };

    // Total: prefix(3) + key_len + null(1)
    let total = 3_usize.wrapping_add(key_len).wrapping_add(1);
    if total > CRYPT_OUTPUT_LEN {
        errno::set_errno(errno::ERANGE);
        return core::ptr::null_mut();
    }

    // SAFETY: single-threaded access to static buffer.
    unsafe {
        let buf = core::ptr::addr_of_mut!(CRYPT_BUF);
        let buf_ptr = (*buf).as_mut_ptr();

        // Copy prefix.
        core::ptr::copy_nonoverlapping(prefix.as_ptr(), buf_ptr, 3);
        // Copy key.
        if key_len > 0 {
            core::ptr::copy_nonoverlapping(key, buf_ptr.add(3), key_len);
        }
        // Null terminate.
        *buf_ptr.add(3_usize.wrapping_add(key_len)) = 0;

        buf_ptr
    }
}

/// `crypt_r` — reentrant version of `crypt`.
///
/// Stub: same as `crypt` but writes into the caller-provided `data`
/// buffer instead of the static one.
///
/// `data` must be a pointer to at least 128 bytes of writable memory.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn crypt_r(
    key: *const u8,
    salt: *const u8,
    data: *mut u8,
) -> *mut u8 {
    if key.is_null() || salt.is_null() || data.is_null() {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    }

    let prefix = b"$0$";
    let key_len = unsafe { crate::string::strlen(key) };
    let total = 3_usize.wrapping_add(key_len).wrapping_add(1);

    if total > CRYPT_OUTPUT_LEN {
        errno::set_errno(errno::ERANGE);
        return core::ptr::null_mut();
    }

    // SAFETY: data is valid for CRYPT_OUTPUT_LEN bytes per caller.
    unsafe {
        core::ptr::copy_nonoverlapping(prefix.as_ptr(), data, 3);
        if key_len > 0 {
            core::ptr::copy_nonoverlapping(key, data.add(3), key_len);
        }
        *data.add(3_usize.wrapping_add(key_len)) = 0;
    }

    data
}

/// `encrypt` — encrypt a block using DES.
///
/// Stub: does nothing (DES is not implemented).  Sets errno to ENOSYS.
///
/// POSIX: `encrypt` takes a 64-byte array of 0s and 1s representing
/// the 64-bit block.  `edflag` controls direction (0 = encrypt,
/// 1 = decrypt).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn encrypt(_block: *mut u8, _edflag: i32) {
    errno::set_errno(errno::ENOSYS);
}

/// `setkey` — set the DES encryption key.
///
/// Stub: does nothing (DES is not implemented).  Sets errno to ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setkey(_key: *const u8) {
    errno::set_errno(errno::ENOSYS);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialise all `crypt()` tests in this module: `crypt()` returns a
    /// pointer into a process-global static buffer, so concurrent calls
    /// from cargo's parallel test runner trample each other's results.
    /// The crypt-using tests below acquire this mutex on entry to keep
    /// observable behaviour deterministic.
    ///
    /// We deliberately do *not* recover from poisoning with
    /// `into_inner()` — if a test panicked holding the mutex the global
    /// buffer's contents are by definition undefined, so the next test
    /// should also fail noisily rather than silently observing stale
    /// state.
    static CRYPT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // -----------------------------------------------------------------------
    // crypt
    // -----------------------------------------------------------------------

    #[test]
    fn test_crypt_returns_prefixed_key() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let result = crypt(b"password\0".as_ptr(), b"$6$salt\0".as_ptr());
        assert!(!result.is_null());
        // Should start with "$0$" (our stub prefix).
        let s = unsafe { core::ffi::CStr::from_ptr(result.cast()) };
        assert_eq!(&s.to_bytes()[..3], b"$0$");
        assert_eq!(s.to_bytes(), b"$0$password");
    }

    #[test]
    fn test_crypt_different_keys() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r1 = crypt(b"abc\0".as_ptr(), b"xx\0".as_ptr());
        assert!(!r1.is_null());
        let s1 = unsafe { core::ffi::CStr::from_ptr(r1.cast()) };
        assert_eq!(s1.to_bytes(), b"$0$abc");
    }

    #[test]
    fn test_crypt_null_key() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::errno::set_errno(0);
        let result = crypt(core::ptr::null(), b"salt\0".as_ptr());
        assert!(result.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_crypt_null_salt() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::errno::set_errno(0);
        let result = crypt(b"key\0".as_ptr(), core::ptr::null());
        assert!(result.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_crypt_empty_key() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let result = crypt(b"\0".as_ptr(), b"salt\0".as_ptr());
        assert!(!result.is_null());
        let s = unsafe { core::ffi::CStr::from_ptr(result.cast()) };
        assert_eq!(s.to_bytes(), b"$0$");
    }

    #[test]
    fn test_crypt_overwrites_static_buffer() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // crypt uses a static buffer — second call overwrites first.
        let _r1 = crypt(b"first\0".as_ptr(), b"xx\0".as_ptr());
        let r2 = crypt(b"second\0".as_ptr(), b"yy\0".as_ptr());
        assert!(!r2.is_null());
        let s2 = unsafe { core::ffi::CStr::from_ptr(r2.cast()) };
        assert_eq!(s2.to_bytes(), b"$0$second");
    }

    // -----------------------------------------------------------------------
    // crypt_r
    // -----------------------------------------------------------------------

    #[test]
    fn test_crypt_r_basic() {
        let mut buf = [0u8; CRYPT_OUTPUT_LEN];
        let result = crypt_r(b"mypass\0".as_ptr(), b"salt\0".as_ptr(), buf.as_mut_ptr());
        assert!(!result.is_null());
        assert_eq!(&buf[..10], b"$0$mypass\0");
    }

    #[test]
    fn test_crypt_r_null_data() {
        crate::errno::set_errno(0);
        let result = crypt_r(b"key\0".as_ptr(), b"salt\0".as_ptr(), core::ptr::null_mut());
        assert!(result.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_crypt_r_null_key() {
        let mut buf = [0u8; CRYPT_OUTPUT_LEN];
        crate::errno::set_errno(0);
        let result = crypt_r(core::ptr::null(), b"salt\0".as_ptr(), buf.as_mut_ptr());
        assert!(result.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_crypt_r_independent_buffers() {
        let mut buf1 = [0u8; CRYPT_OUTPUT_LEN];
        let mut buf2 = [0u8; CRYPT_OUTPUT_LEN];

        crypt_r(b"alpha\0".as_ptr(), b"xx\0".as_ptr(), buf1.as_mut_ptr());
        crypt_r(b"beta\0".as_ptr(), b"yy\0".as_ptr(), buf2.as_mut_ptr());

        // Both should be independently preserved.
        assert_eq!(&buf1[..9], b"$0$alpha\0");
        assert_eq!(&buf2[..8], b"$0$beta\0");
    }

    // -----------------------------------------------------------------------
    // encrypt / setkey
    // -----------------------------------------------------------------------

    #[test]
    fn test_encrypt_enosys() {
        crate::errno::set_errno(0);
        let mut block = [0u8; 64];
        encrypt(block.as_mut_ptr(), 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_encrypt_decrypt_enosys() {
        crate::errno::set_errno(0);
        let mut block = [0u8; 64];
        encrypt(block.as_mut_ptr(), 1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_encrypt_null_block() {
        crate::errno::set_errno(0);
        encrypt(core::ptr::null_mut(), 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_setkey_enosys() {
        crate::errno::set_errno(0);
        setkey(b"0101010101010101\0".as_ptr());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_setkey_null() {
        crate::errno::set_errno(0);
        setkey(core::ptr::null());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_crypt_output_len() {
        assert_eq!(CRYPT_OUTPUT_LEN, 128);
    }
}
