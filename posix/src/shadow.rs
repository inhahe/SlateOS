//! `<shadow.h>` — the shadow password database.
//!
//! On a Unix system `/etc/passwd` is world-readable and therefore cannot hold
//! password hashes; the hashes live in `/etc/shadow`, which only root can read.
//! [`crate::pwd`] reflects that split already: its single root entry carries
//! `pw_passwd = "x"`, the conventional marker meaning "the real secret is in
//! the shadow file". This module is the other half of that convention.
//!
//! ## What we return, and why it is safe
//!
//! SlateOS has no user database yet (see [`crate::pwd`]'s module docs — one
//! user, `root`, uid 0). There is therefore no password hash to hand out, and
//! this module reports the one entry with **`sp_pwdp = "!"`**.
//!
//! `"!"` is not a placeholder: it is the standard *locked account* marker. A
//! password check compares `crypt(typed, hash)` against the stored field, and
//! no output of `crypt` ever begins with `!` — so a locked entry is one that
//! **no input can satisfy**. That makes exposing this database incapable of
//! granting access, which is the property we want while there is nothing real
//! behind it. The alternative shapes are both worse: an empty `sp_pwdp` means
//! *no password required* and would authenticate anybody, and reporting "no
//! such user" would make a caller that distinguishes "unknown account" from
//! "locked account" reach the wrong conclusion about a user `getpwnam` says
//! exists.
//!
//! The aging fields are all `-1`, the standard "not set" value, which callers
//! render as "no expiry / no minimum / no warning". `sp_lstchg` is `-1` rather
//! than `0`: `0` specifically means *the password must be changed at next
//! login*, which would be a lie about an account that has no password at all.
//!
//! ## Privilege
//!
//! Real implementations fail with `EACCES` for an unprivileged caller. Every
//! process here runs as uid 0 (`crate::pwd`), so there is no unprivileged
//! caller to fail, and the entry is returned unconditionally. When a real user
//! database arrives, this is the first thing that has to change — the check
//! belongs here, not in the callers.
//!
//! ## Why this exists
//!
//! Measured, not speculative: the CPython 3.12 cross-build
//! (`scripts/cpython-spike/`) links its `spwd` extension module into the
//! interpreter, and `getspnam`/`getspent`/`setspent`/`endspent` were four of
//! the six symbols standing between CPython and a successful link against our
//! `libc.a`. `login`, `su` and `passwd` want the same header.

use crate::perprocess::process_global;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// Shadow password database entry (`struct spwd`).
///
/// Field order and types match glibc and musl exactly; a mismatch here is a
/// silent memory-corruption bug in every C caller, not a compile error.
#[repr(C)]
pub struct Spwd {
    /// Login name.
    pub sp_namp: *const u8,
    /// Encrypted password, or a lock marker. See the module docs.
    pub sp_pwdp: *const u8,
    /// Date of last change, in days since the epoch; `-1` if unset.
    pub sp_lstchg: i64,
    /// Minimum days between changes; `-1` if unset.
    pub sp_min: i64,
    /// Maximum days a password stays valid; `-1` if unset.
    pub sp_max: i64,
    /// Days of warning before expiry; `-1` if unset.
    pub sp_warn: i64,
    /// Days after expiry before the account is disabled; `-1` if unset.
    pub sp_inact: i64,
    /// Account expiry date, in days since the epoch; `-1` if unset.
    pub sp_expire: i64,
    /// Reserved; must be `!0` ("unset") per the shadow suite's convention.
    pub sp_flag: u64,
}

// SAFETY: `Spwd`'s two pointers address `'static`, immutable byte strings, so
// there is nothing a second thread could observe changing.
unsafe impl Sync for Spwd {}

/// The single `root` entry, locked.
///
/// `"!"` cannot be produced by `crypt`, so no typed password matches it — see
/// the module docs for why that is the correct value rather than `""`.
static ROOT_SPWD: Spwd = Spwd {
    sp_namp: c"root".as_ptr().cast::<u8>(),
    sp_pwdp: c"!".as_ptr().cast::<u8>(),
    sp_lstchg: -1,
    sp_min: -1,
    sp_max: -1,
    sp_warn: -1,
    sp_inact: -1,
    sp_expire: -1,
    // The shadow suite writes an all-ones reserved field to mean "unset"; a
    // zero here would be read as a meaningful flag word by a caller that ever
    // learns to interpret it.
    sp_flag: u64::MAX,
};

process_global! {
    /// Enumeration cursor for `getspent`/`setspent`/`endspent`.
    ///
    /// Per-process rather than per-thread, matching `crate::pwd`'s cursors and
    /// the C library convention that the `*ent` iterators share one position.
    fn sp_pos_ptr() -> i32 = 0;
}

/// The bytes `fill_spwd_r` copies into a caller-provided buffer: `"root\0!\0"`.
const R_STRINGS: &[u8] = b"root\0!\0";
/// Offset of the `"!"` within [`R_STRINGS`].
const R_PWDP_OFF: usize = 5;

// ---------------------------------------------------------------------------
// Lookup
// ---------------------------------------------------------------------------

/// Look up a shadow entry by login name.
///
/// Returns a pointer to library-owned storage, or NULL if the user is unknown.
/// The result is valid until the next call to any function in this module —
/// that is the POSIX contract, and why `getspnam_r` exists.
///
/// # Safety
///
/// `name`, if non-NULL, must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getspnam(name: *const u8) -> *const Spwd {
    if name.is_null() {
        return core::ptr::null();
    }
    if unsafe { is_root_name(name) } {
        return &raw const ROOT_SPWD;
    }
    core::ptr::null()
}

/// Rewind the shadow database to the first entry.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setspent() {
    // SAFETY: `sp_pos_ptr` returns a valid pointer to this process's cursor.
    unsafe {
        sp_pos_ptr().write(0);
    }
}

/// Read the next shadow entry, or NULL once the database is exhausted.
///
/// There is exactly one entry, so the first call after a rewind returns `root`
/// and every call after that returns NULL.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getspent() -> *const Spwd {
    // SAFETY: as in `setspent`.
    let pos = unsafe { *sp_pos_ptr() };
    if pos == 0 {
        // SAFETY: as in `setspent`.
        unsafe {
            sp_pos_ptr().write(1);
        }
        return &raw const ROOT_SPWD;
    }
    core::ptr::null()
}

/// Close the shadow database.
///
/// Also rewinds, matching glibc: a `getspent` after `endspent` starts over
/// rather than returning NULL forever.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endspent() {
    // SAFETY: as in `setspent`.
    unsafe {
        sp_pos_ptr().write(0);
    }
}

// ---------------------------------------------------------------------------
// Reentrant variants
// ---------------------------------------------------------------------------

/// Look up a shadow entry by name, writing the result into caller storage.
///
/// Returns 0 whether or not the user was found — "not found" is signalled by
/// `*result == NULL`, not by the return value. Returns `ERANGE` if `buf` is
/// too small and `EFAULT` if a required out-pointer is NULL.
///
/// # Safety
///
/// `spwd` and `result` must be valid for writes, `buf` must be valid for
/// `buflen` bytes, and `name`, if non-NULL, must be null-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getspnam_r(
    name: *const u8,
    spwd: *mut Spwd,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Spwd,
) -> i32 {
    if result.is_null() || spwd.is_null() || buf.is_null() {
        return crate::errno::EFAULT;
    }
    // SAFETY: `result` is non-null and valid for writes (checked above).
    unsafe {
        *result = core::ptr::null();
    }
    if name.is_null() {
        return 0;
    }
    // SAFETY: the caller guarantees `name` is null-terminated.
    if unsafe { is_root_name(name) } {
        return fill_spwd_r(spwd, buf, buflen, result);
    }
    0
}

/// Read the next shadow entry into caller storage.
///
/// Same conventions as [`getspnam_r`].
///
/// # Safety
///
/// As [`getspnam_r`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getspent_r(
    spwd: *mut Spwd,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Spwd,
) -> i32 {
    if result.is_null() || spwd.is_null() || buf.is_null() {
        return crate::errno::EFAULT;
    }
    // SAFETY: `result` is non-null and valid for writes (checked above).
    unsafe {
        *result = core::ptr::null();
    }
    // SAFETY: `sp_pos_ptr` returns a valid pointer to this process's cursor.
    let pos = unsafe { *sp_pos_ptr() };
    if pos != 0 {
        // Exhausted. glibc returns ENOENT here rather than 0, because a
        // caller looping on `getspent_r` needs to tell "no more entries" from
        // "this one did not fit".
        return crate::errno::ENOENT;
    }
    let rc = fill_spwd_r(spwd, buf, buflen, result);
    if rc == 0 {
        // Only advance on a successful read: an ERANGE caller is expected to
        // enlarge its buffer and ask again for the *same* entry.
        // SAFETY: as above.
        unsafe {
            sp_pos_ptr().write(1);
        }
    }
    rc
}

/// Copy the root entry's strings into `buf` and point `spwd` at them.
fn fill_spwd_r(spwd: *mut Spwd, buf: *mut u8, buflen: usize, result: *mut *const Spwd) -> i32 {
    if buflen < R_STRINGS.len() {
        return crate::errno::ERANGE;
    }
    let mut i: usize = 0;
    while i < R_STRINGS.len() {
        let byte = R_STRINGS.get(i).copied().unwrap_or(0);
        // SAFETY: `i < R_STRINGS.len() <= buflen`, and the caller guarantees
        // `buf` is valid for `buflen` bytes.
        unsafe {
            *buf.add(i) = byte;
        }
        i = i.wrapping_add(1);
    }

    // SAFETY: `spwd` and `result` are non-null and valid for writes (checked
    // by every caller), and the two `buf` offsets are inside the region just
    // written, so the pointers stay valid for as long as `buf` does.
    unsafe {
        (*spwd).sp_namp = buf;
        (*spwd).sp_pwdp = buf.add(R_PWDP_OFF);
        (*spwd).sp_lstchg = -1;
        (*spwd).sp_min = -1;
        (*spwd).sp_max = -1;
        (*spwd).sp_warn = -1;
        (*spwd).sp_inact = -1;
        (*spwd).sp_expire = -1;
        (*spwd).sp_flag = u64::MAX;
        *result = spwd;
    }
    0
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Is this null-terminated string exactly `"root"`?
///
/// # Safety
///
/// `s` must be non-NULL and null-terminated.
unsafe fn is_root_name(s: *const u8) -> bool {
    // Compare the whole string including its terminator, so "rootkit" — which
    // shares our four-byte prefix — does not match.
    let want = b"root\0";
    let mut i: usize = 0;
    while i < want.len() {
        let expect = want.get(i).copied().unwrap_or(0);
        // SAFETY: the caller guarantees `s` is null-terminated; the loop stops
        // at the first mismatch, so it can never read past that terminator.
        let got = unsafe { *s.add(i) };
        if got != expect {
            return false;
        }
        i = i.wrapping_add(1);
    }
    true
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::errno;

    fn cstr(p: *const u8) -> &'static [u8] {
        // SAFETY: every pointer this is applied to is a null-terminated string
        // owned either statically or by the test's own buffer.
        unsafe { core::ffi::CStr::from_ptr(p.cast()).to_bytes() }
    }

    #[test]
    fn getspnam_root_is_locked_not_empty() {
        let sp = unsafe { getspnam(b"root\0".as_ptr()) };
        assert!(!sp.is_null());
        let sp = unsafe { &*sp };
        assert_eq!(cstr(sp.sp_namp), b"root");
        // The whole security argument of this module: an empty hash would
        // authenticate anyone.
        assert_eq!(cstr(sp.sp_pwdp), b"!");
        assert_ne!(cstr(sp.sp_pwdp), b"");
    }

    #[test]
    fn getspnam_aging_fields_are_unset() {
        let sp = unsafe { &*getspnam(b"root\0".as_ptr()) };
        assert_eq!(sp.sp_lstchg, -1);
        assert_eq!(sp.sp_min, -1);
        assert_eq!(sp.sp_max, -1);
        assert_eq!(sp.sp_warn, -1);
        assert_eq!(sp.sp_inact, -1);
        assert_eq!(sp.sp_expire, -1);
        assert_eq!(sp.sp_flag, u64::MAX);
    }

    #[test]
    fn getspnam_unknown_user_is_null() {
        assert!(unsafe { getspnam(b"nobody\0".as_ptr()) }.is_null());
        assert!(unsafe { getspnam(b"\0".as_ptr()) }.is_null());
        assert!(unsafe { getspnam(core::ptr::null()) }.is_null());
    }

    #[test]
    fn getspnam_prefix_of_root_does_not_match() {
        // "rootkit" starts with "root"; a length-blind comparison would say yes.
        assert!(unsafe { getspnam(b"rootkit\0".as_ptr()) }.is_null());
        assert!(unsafe { getspnam(b"roo\0".as_ptr()) }.is_null());
    }

    #[test]
    fn spent_enumeration_yields_one_entry() {
        setspent();
        let first = getspent();
        assert!(!first.is_null());
        assert_eq!(cstr(unsafe { (*first).sp_namp }), b"root");
        assert!(getspent().is_null());
        // Still exhausted on a repeat call.
        assert!(getspent().is_null());
    }

    #[test]
    fn setspent_and_endspent_both_rewind() {
        setspent();
        assert!(!getspent().is_null());
        assert!(getspent().is_null());

        setspent();
        assert!(!getspent().is_null());

        endspent();
        assert!(!getspent().is_null());
        endspent();
    }

    #[test]
    fn getspnam_r_copies_into_caller_buffer() {
        let mut sp: Spwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 64];
        let mut result: *const Spwd = core::ptr::null();
        let rc = unsafe {
            getspnam_r(
                b"root\0".as_ptr(),
                &mut sp,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };
        assert_eq!(rc, 0);
        assert!(!result.is_null());
        assert_eq!(cstr(sp.sp_namp), b"root");
        assert_eq!(cstr(sp.sp_pwdp), b"!");
        // The strings must live in the caller's buffer, not in our statics —
        // that is the entire point of the _r form.
        assert!(core::ptr::eq(sp.sp_namp, buf.as_ptr()));
    }

    #[test]
    fn getspnam_r_unknown_user_is_not_an_error() {
        let mut sp: Spwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 64];
        let mut result: *const Spwd = core::ptr::null();
        let rc = unsafe {
            getspnam_r(
                b"nobody\0".as_ptr(),
                &mut sp,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };
        assert_eq!(rc, 0);
        assert!(result.is_null());
    }

    #[test]
    fn getspnam_r_short_buffer_is_erange() {
        let mut sp: Spwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 3];
        let mut result: *const Spwd = core::ptr::null();
        let rc = unsafe {
            getspnam_r(
                b"root\0".as_ptr(),
                &mut sp,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };
        assert_eq!(rc, errno::ERANGE);
        assert!(result.is_null());
    }

    #[test]
    fn getspnam_r_null_outputs_are_efault() {
        let mut sp: Spwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 64];
        let mut result: *const Spwd = core::ptr::null();
        let name = b"root\0".as_ptr();

        assert_eq!(
            unsafe {
                getspnam_r(
                    name,
                    &mut sp,
                    buf.as_mut_ptr(),
                    buf.len(),
                    core::ptr::null_mut(),
                )
            },
            errno::EFAULT
        );
        assert_eq!(
            unsafe {
                getspnam_r(
                    name,
                    core::ptr::null_mut(),
                    buf.as_mut_ptr(),
                    buf.len(),
                    &mut result,
                )
            },
            errno::EFAULT
        );
        assert_eq!(
            unsafe { getspnam_r(name, &mut sp, core::ptr::null_mut(), buf.len(), &mut result) },
            errno::EFAULT
        );
    }

    #[test]
    fn getspent_r_walks_then_reports_enoent() {
        setspent();
        let mut sp: Spwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 64];
        let mut result: *const Spwd = core::ptr::null();

        let rc = unsafe { getspent_r(&mut sp, buf.as_mut_ptr(), buf.len(), &mut result) };
        assert_eq!(rc, 0);
        assert!(!result.is_null());

        let rc = unsafe { getspent_r(&mut sp, buf.as_mut_ptr(), buf.len(), &mut result) };
        assert_eq!(rc, errno::ENOENT);
        assert!(result.is_null());
        endspent();
    }

    #[test]
    fn getspent_r_erange_does_not_consume_the_entry() {
        setspent();
        let mut sp: Spwd = unsafe { core::mem::zeroed() };
        let mut small = [0u8; 2];
        let mut big = [0u8; 64];
        let mut result: *const Spwd = core::ptr::null();

        let rc = unsafe { getspent_r(&mut sp, small.as_mut_ptr(), small.len(), &mut result) };
        assert_eq!(rc, errno::ERANGE);

        // Retrying with a large enough buffer must still yield root: a failed
        // read that swallowed the entry would silently drop database rows.
        let rc = unsafe { getspent_r(&mut sp, big.as_mut_ptr(), big.len(), &mut result) };
        assert_eq!(rc, 0);
        assert!(!result.is_null());
        assert_eq!(cstr(sp.sp_namp), b"root");
        endspent();
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn spwd_matches_the_c_abi_layout() {
        // 2 pointers + 6 signed longs + 1 unsigned long = 9 * 8. A mismatch
        // here corrupts memory in every C caller rather than failing to build.
        assert_eq!(core::mem::size_of::<Spwd>(), 72);
        assert_eq!(core::mem::align_of::<Spwd>(), 8);
    }
}
