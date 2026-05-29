//! POSIX `<unistd.h>` / `<crypt.h>` password hashing.
//!
//! Implements the SHA-256 (`$5$`) and SHA-512 (`$6$`) crypt methods —
//! the modern shadow-suite defaults — following Ulrich Drepper's
//! specification ("Unix crypt using SHA-256 and SHA-512").  The hash
//! cores live in [`crate::sha2`]; this module implements the salt/rounds
//! parsing, the key-derivation rounds, and the crypt base-64 encoding.
//!
//! Previously `crypt()` returned `"$0$<key>"` — i.e. the password in
//! cleartext with a marker prefix.  Any program that hashed a password
//! and stored the result was effectively storing the plaintext.  That
//! was a security hole, now closed.
//!
//! ## Unsupported methods
//!
//! Legacy DES (two-character salt) and MD5 (`$1$`) crypt are **not**
//! implemented.  Rather than fabricate an insecure result, `crypt()`
//! fails with `EINVAL` for any setting it does not recognise — matching
//! modern glibc/libxcrypt behaviour.  (See `todo.txt` for the MD5/DES
//! follow-up.)
//!
//! `encrypt`/`setkey` (raw DES block cipher) remain unimplemented and
//! return `ENOSYS` after argument validation.

#![allow(clippy::arithmetic_side_effects)] // Bounded counters / modular round arithmetic.
#![allow(clippy::indexing_slicing)] // Fixed-size digest arrays indexed by compile-time constants.

use crate::errno;
use crate::sha2::{Digest, Sha256, Sha512};

/// Maximum length of a crypt result string (including the NUL terminator).
///
/// The longest output we generate is a SHA-512 hash with an explicit
/// rounds field: `"$6$rounds=999999999$"` (20) + 16-byte salt + `"$"` (1)
/// + 86-character hash + NUL = 124 bytes, comfortably within this bound.
const CRYPT_OUTPUT_LEN: usize = 128;

/// Static buffer for `crypt()` results (non-reentrant, per POSIX).
static mut CRYPT_BUF: [u8; CRYPT_OUTPUT_LEN] = [0u8; CRYPT_OUTPUT_LEN];

/// The crypt base-64 alphabet (note: NOT standard base64 — `.` and `/`
/// lead, and the digit/letter order differs).
const B64_ALPHABET: &[u8; 64] =
    b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Default SHA-crypt rounds when no `rounds=` field is given.
const ROUNDS_DEFAULT: u32 = 5000;
/// Minimum permitted rounds (values below are clamped up).
const ROUNDS_MIN: u32 = 1000;
/// Maximum permitted rounds (values above are clamped down).
const ROUNDS_MAX: u32 = 999_999_999;
/// Maximum salt length in bytes (longer salts are truncated).
const SALT_MAX: usize = 16;

// ---------------------------------------------------------------------------
// Fixed-capacity output builder
// ---------------------------------------------------------------------------

/// A bounded byte sink used to assemble the crypt result without heap
/// allocation.  Writes past the capacity set `overflow` instead of
/// panicking, so the caller can map the condition to `ERANGE`.
struct OutBuf {
    buf: [u8; CRYPT_OUTPUT_LEN],
    len: usize,
    overflow: bool,
}

impl OutBuf {
    fn new() -> Self {
        Self {
            buf: [0u8; CRYPT_OUTPUT_LEN],
            len: 0,
            overflow: false,
        }
    }

    fn push(&mut self, b: u8) {
        if self.len < CRYPT_OUTPUT_LEN {
            self.buf[self.len] = b;
            self.len += 1;
        } else {
            self.overflow = true;
        }
    }

    fn push_slice(&mut self, s: &[u8]) {
        for &b in s {
            self.push(b);
        }
    }

    fn push_decimal(&mut self, mut v: u32) {
        if v == 0 {
            self.push(b'0');
            return;
        }
        let mut tmp = [0u8; 10];
        let mut i = tmp.len();
        while v > 0 {
            i -= 1;
            tmp[i] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        self.push_slice(&tmp[i..]);
    }
}

/// Emit `n` crypt-base64 characters for the 24-bit big-endian group
/// `(b2 << 16) | (b1 << 8) | b0`, lowest 6 bits first.
fn b64_from_24bit(out: &mut OutBuf, b2: u8, b1: u8, b0: u8, n: usize) {
    let mut w = (u32::from(b2) << 16) | (u32::from(b1) << 8) | u32::from(b0);
    for _ in 0..n {
        out.push(B64_ALPHABET[(w & 0x3f) as usize]);
        w >>= 6;
    }
}

// ---------------------------------------------------------------------------
// SHA-crypt core
// ---------------------------------------------------------------------------

/// Feed `digest` into `ctx` repeatedly until `total` bytes have been
/// added (full copies followed by a final partial copy).  This realises
/// the "sequence P / sequence S" construction without materialising the
/// (potentially large) intermediate buffers.
fn add_repeated<D: Digest>(ctx: &mut D, digest: &[u8], total: usize) {
    let mut remaining = total;
    while remaining > 0 {
        let n = core::cmp::min(remaining, digest.len());
        ctx.update(&digest[..n]);
        remaining -= n;
    }
}

/// Run the SHA-crypt key-derivation and write the raw `D::OUTPUT_LEN`
/// digest into `alt`.  Implements steps 1–21 of Drepper's spec.
fn sha_crypt_raw<D: Digest>(key: &[u8], salt: &[u8], rounds: u32, alt: &mut [u8]) {
    let dl = D::OUTPUT_LEN;

    // Digest B = H(key || salt || key).
    let mut b = [0u8; 64];
    {
        let mut h = D::new();
        h.update(key);
        h.update(salt);
        h.update(key);
        h.finalize_into(&mut b);
    }

    // Digest A.
    let mut a_ctx = D::new();
    a_ctx.update(key);
    a_ctx.update(salt);
    add_repeated::<D>(&mut a_ctx, &b[..dl], key.len());
    // For each bit of key.len(), low to high: 1 -> add B, 0 -> add key.
    let mut bits = key.len();
    while bits > 0 {
        if bits & 1 != 0 {
            a_ctx.update(&b[..dl]);
        } else {
            a_ctx.update(key);
        }
        bits >>= 1;
    }
    a_ctx.finalize_into(alt);

    // Digest DP = H(key repeated key.len() times); sequence P repeats it.
    let mut dp = [0u8; 64];
    {
        let mut h = D::new();
        for _ in 0..key.len() {
            h.update(key);
        }
        h.finalize_into(&mut dp);
    }

    // Digest DS = H(salt repeated 16 + A[0] times); sequence S repeats it.
    let mut ds = [0u8; 64];
    {
        let mut h = D::new();
        let times = 16 + usize::from(alt[0]);
        for _ in 0..times {
            h.update(salt);
        }
        h.finalize_into(&mut ds);
    }

    // The deliberately-expensive stretching loop.
    for cnt in 0..rounds {
        let mut h = D::new();
        if cnt & 1 != 0 {
            add_repeated::<D>(&mut h, &dp[..dl], key.len()); // sequence P
        } else {
            h.update(&alt[..dl]);
        }
        if cnt % 3 != 0 {
            add_repeated::<D>(&mut h, &ds[..dl], salt.len()); // sequence S
        }
        if cnt % 7 != 0 {
            add_repeated::<D>(&mut h, &dp[..dl], key.len()); // sequence P
        }
        if cnt & 1 != 0 {
            h.update(&alt[..dl]);
        } else {
            add_repeated::<D>(&mut h, &dp[..dl], key.len()); // sequence P
        }
        h.finalize_into(alt);
    }
}

/// Crypt-base64 encoding for a 64-byte SHA-512 digest (86 chars).
fn encode_sha512(out: &mut OutBuf, a: &[u8]) {
    const GROUPS: [(usize, usize, usize); 21] = [
        (0, 21, 42),
        (22, 43, 1),
        (44, 2, 23),
        (3, 24, 45),
        (25, 46, 4),
        (47, 5, 26),
        (6, 27, 48),
        (28, 49, 7),
        (50, 8, 29),
        (9, 30, 51),
        (31, 52, 10),
        (53, 11, 32),
        (12, 33, 54),
        (34, 55, 13),
        (56, 14, 35),
        (15, 36, 57),
        (37, 58, 16),
        (59, 17, 38),
        (18, 39, 60),
        (40, 61, 19),
        (62, 20, 41),
    ];
    for &(i2, i1, i0) in &GROUPS {
        b64_from_24bit(out, a[i2], a[i1], a[i0], 4);
    }
    b64_from_24bit(out, 0, 0, a[63], 2);
}

/// Crypt-base64 encoding for a 32-byte SHA-256 digest (43 chars).
fn encode_sha256(out: &mut OutBuf, a: &[u8]) {
    const GROUPS: [(usize, usize, usize); 10] = [
        (0, 10, 20),
        (21, 1, 11),
        (12, 22, 2),
        (3, 13, 23),
        (24, 4, 14),
        (15, 25, 5),
        (6, 16, 26),
        (27, 7, 17),
        (18, 28, 8),
        (9, 19, 29),
    ];
    for &(i2, i1, i0) in &GROUPS {
        b64_from_24bit(out, a[i2], a[i1], a[i0], 4);
    }
    b64_from_24bit(out, 0, a[31], a[30], 3);
}

/// Parse a SHA-crypt `setting` string and, if recognised, compute the
/// full result (`"$N$[rounds=R$]salt$hash"`) into `out`.
///
/// Returns `true` if `setting` selected a supported method (`$5$`/`$6$`)
/// and the result was written; `false` if `setting` is not a SHA-crypt
/// setting (caller should report `EINVAL`).
fn sha_crypt(key: &[u8], setting: &[u8], out: &mut OutBuf) -> bool {
    let (is_512, rest) = if let Some(r) = setting.strip_prefix(b"$6$") {
        (true, r)
    } else if let Some(r) = setting.strip_prefix(b"$5$") {
        (false, r)
    } else {
        return false;
    };

    // Optional "rounds=N$" prefix.
    let mut rounds = ROUNDS_DEFAULT;
    let mut rounds_custom = false;
    let mut salt_part = rest;
    if let Some(after) = rest.strip_prefix(b"rounds=") {
        let mut val: u64 = 0;
        let mut i = 0;
        while i < after.len() && after[i].is_ascii_digit() {
            val = val
                .saturating_mul(10)
                .saturating_add(u64::from(after[i] - b'0'));
            i += 1;
        }
        // Accept only if at least one digit was consumed and the next
        // byte is '$' (mirrors glibc's strtoul + "*endp == '$'" check).
        if i > 0 && i < after.len() && after[i] == b'$' {
            rounds_custom = true;
            rounds = val.clamp(u64::from(ROUNDS_MIN), u64::from(ROUNDS_MAX)) as u32;
            salt_part = &after[i + 1..];
        }
        // Otherwise leave salt_part == rest: the malformed "rounds=..."
        // text becomes the salt (truncated below), exactly as glibc does.
    }

    // Salt = bytes up to the first '$', capped at SALT_MAX.
    let mut salt_end = 0;
    while salt_end < salt_part.len() && salt_part[salt_end] != b'$' {
        salt_end += 1;
    }
    let salt = &salt_part[..core::cmp::min(salt_end, SALT_MAX)];

    // Assemble the "$N$[rounds=R$]salt$" header.
    out.push_slice(if is_512 { b"$6$" } else { b"$5$" });
    if rounds_custom {
        out.push_slice(b"rounds=");
        out.push_decimal(rounds);
        out.push(b'$');
    }
    out.push_slice(salt);
    out.push(b'$');

    if is_512 {
        let mut alt = [0u8; 64];
        sha_crypt_raw::<Sha512>(key, salt, rounds, &mut alt);
        encode_sha512(out, &alt);
    } else {
        let mut alt = [0u8; 32];
        sha_crypt_raw::<Sha256>(key, salt, rounds, &mut alt);
        encode_sha256(out, &alt);
    }
    out.push(0); // NUL terminator
    true
}

/// View a NUL-terminated C string as a byte slice (excluding the NUL).
///
/// # Safety
///
/// `p` must be non-null and point to a valid NUL-terminated string.
unsafe fn cstr_slice<'a>(p: *const u8) -> &'a [u8] {
    // SAFETY: caller guarantees `p` is a valid NUL-terminated C string.
    let len = unsafe { crate::string::strlen(p) };
    // SAFETY: `p` is valid for `len` bytes per the strlen scan above.
    unsafe { core::slice::from_raw_parts(p, len) }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// `crypt` — one-way password hashing.
///
/// Supports `$5$` (SHA-256) and `$6$` (SHA-512) settings, with optional
/// `rounds=N$`.  Returns a pointer to a static buffer (overwritten by
/// each call), or null on error:
///
/// * `EFAULT` — `key` or `salt` is null.
/// * `EINVAL` — `salt` does not select a supported method.
/// * `ERANGE` — the formatted result would exceed the output buffer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn crypt(key: *const u8, salt: *const u8) -> *mut u8 {
    if key.is_null() || salt.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }

    // SAFETY: both pointers are non-null (checked) and, per the C
    // contract, NUL-terminated.
    let key_s = unsafe { cstr_slice(key) };
    let salt_s = unsafe { cstr_slice(salt) };

    let mut out = OutBuf::new();
    if !sha_crypt(key_s, salt_s, &mut out) {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    }
    if out.overflow {
        errno::set_errno(errno::ERANGE);
        return core::ptr::null_mut();
    }

    // SAFETY: single static buffer; per-POSIX crypt() is non-reentrant.
    unsafe {
        let buf = core::ptr::addr_of_mut!(CRYPT_BUF);
        let buf_ptr = (*buf).as_mut_ptr();
        core::ptr::copy_nonoverlapping(out.buf.as_ptr(), buf_ptr, out.len);
        buf_ptr
    }
}

/// `crypt_r` — reentrant `crypt`.
///
/// Identical to [`crypt`] but writes the result into the caller-provided
/// `data` buffer (which must be at least [`CRYPT_OUTPUT_LEN`] bytes) and
/// returns `data` on success.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn crypt_r(key: *const u8, salt: *const u8, data: *mut u8) -> *mut u8 {
    if key.is_null() || salt.is_null() || data.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }

    // SAFETY: pointers are non-null (checked) and NUL-terminated.
    let key_s = unsafe { cstr_slice(key) };
    let salt_s = unsafe { cstr_slice(salt) };

    let mut out = OutBuf::new();
    if !sha_crypt(key_s, salt_s, &mut out) {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    }
    if out.overflow {
        errno::set_errno(errno::ERANGE);
        return core::ptr::null_mut();
    }

    // SAFETY: caller guarantees `data` is valid for CRYPT_OUTPUT_LEN
    // bytes; we never write more than `out.len` (<= CRYPT_OUTPUT_LEN).
    unsafe {
        core::ptr::copy_nonoverlapping(out.buf.as_ptr(), data, out.len);
    }
    data
}

/// `encrypt` — encrypt/decrypt a 64-bit block using DES.
///
/// Stub: DES is not implemented.  Validates arguments per POSIX, then
/// reports `ENOSYS`:
///
/// * `EFAULT` — `block` is NULL.
/// * `EINVAL` — `edflag` is not 0 (encrypt) or 1 (decrypt).
/// * `ENOSYS` — validated, but no DES backend.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn encrypt(block: *mut u8, edflag: i32) {
    if block.is_null() {
        errno::set_errno(errno::EFAULT);
        return;
    }
    if edflag != 0 && edflag != 1 {
        errno::set_errno(errno::EINVAL);
        return;
    }
    errno::set_errno(errno::ENOSYS);
}

/// `setkey` — set the DES encryption key.
///
/// Stub: DES is not implemented.  Validates `key` (NULL → `EFAULT`) then
/// reports `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setkey(key: *const u8) {
    if key.is_null() {
        errno::set_errno(errno::EFAULT);
        return;
    }
    errno::set_errno(errno::ENOSYS);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialise all `crypt()` tests: `crypt()` returns a pointer into a
    /// process-global static buffer, so concurrent calls from cargo's
    /// parallel runner would trample each other.
    static CRYPT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Helper: call `crypt` and return the result as an owned `String`.
    fn crypt_str(key: &[u8], salt: &[u8]) -> Option<std::string::String> {
        let r = crypt(key.as_ptr(), salt.as_ptr());
        if r.is_null() {
            return None;
        }
        let s = unsafe { core::ffi::CStr::from_ptr(r.cast()) };
        Some(s.to_string_lossy().into_owned())
    }

    // -----------------------------------------------------------------------
    // SHA-512 ($6$) — canonical Drepper test vectors
    // -----------------------------------------------------------------------

    #[test]
    fn sha512_known_vector() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r = crypt_str(b"Hello world!\0", b"$6$saltstring\0").unwrap();
        assert_eq!(
            r,
            "$6$saltstring$svn8UoSVapNtMuq1ukKS4tPQd8iKwSMHWjl/O817G3uBnIFNjnQJuesI68u4OTLiBFdcbYEdFCoEOfaS35inz1"
        );
    }

    #[test]
    fn sha512_rounds_and_salt_truncation() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // 20-char salt is truncated to 16; rounds field is echoed back.
        let r = crypt_str(b"Hello world!\0", b"$6$rounds=10000$saltstringsaltstring\0").unwrap();
        assert_eq!(
            r,
            "$6$rounds=10000$saltstringsaltst$OW1/O6BYHV6BcXZu8QVeXbDWra3Oeqh0sbHbbMCVNSnCM/UrjmM0Dp8vOuZeHBy/YTBmSK6H9qs/y3RnOaw5v."
        );
    }

    // -----------------------------------------------------------------------
    // SHA-256 ($5$) — canonical Drepper test vectors
    // -----------------------------------------------------------------------

    #[test]
    fn sha256_known_vector() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r = crypt_str(b"Hello world!\0", b"$5$saltstring\0").unwrap();
        assert_eq!(r, "$5$saltstring$5B8vYYiY.CVt1RlTTf8KbXBH3hsxY/GNooZaBBGWEc5");
    }

    #[test]
    fn sha256_rounds_and_salt_truncation() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r = crypt_str(b"Hello world!\0", b"$5$rounds=10000$saltstringsaltstring\0").unwrap();
        assert_eq!(
            r,
            "$5$rounds=10000$saltstringsaltst$3xv.VbSHBb41AL9AvLeujZkZRBAwqFMz2.opqey6IcA"
        );
    }

    // -----------------------------------------------------------------------
    // Determinism / distinctness
    // -----------------------------------------------------------------------

    #[test]
    fn same_inputs_are_deterministic() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let a = crypt_str(b"secret\0", b"$6$abcdef\0").unwrap();
        let b = crypt_str(b"secret\0", b"$6$abcdef\0").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn different_keys_differ() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let a = crypt_str(b"secret1\0", b"$6$abcdef\0").unwrap();
        let b = crypt_str(b"secret2\0", b"$6$abcdef\0").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn different_salts_differ() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let a = crypt_str(b"secret\0", b"$6$saltone\0").unwrap();
        let b = crypt_str(b"secret\0", b"$6$salttwo\0").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn result_is_not_plaintext() {
        // Regression guard for the old "$0$<key>" stub: the password
        // must NOT appear verbatim in the output.
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r = crypt_str(b"plaintextpassword\0", b"$6$somesalt\0").unwrap();
        assert!(!r.contains("plaintextpassword"));
        assert!(!r.starts_with("$0$"));
    }

    // -----------------------------------------------------------------------
    // Error paths
    // -----------------------------------------------------------------------

    #[test]
    fn null_key_efault() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::errno::set_errno(0);
        let r = crypt(core::ptr::null(), b"$6$salt\0".as_ptr());
        assert!(r.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn null_salt_efault() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::errno::set_errno(0);
        let r = crypt(b"key\0".as_ptr(), core::ptr::null());
        assert!(r.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn unsupported_method_einval() {
        // Legacy DES (2-char salt) and unknown markers are rejected,
        // never silently turned into a fake hash.
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::errno::set_errno(0);
        let r = crypt(b"password\0".as_ptr(), b"ab\0".as_ptr());
        assert!(r.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn md5_method_einval() {
        // MD5 ($1$) is not implemented yet — must fail, not fabricate.
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::errno::set_errno(0);
        let r = crypt(b"password\0".as_ptr(), b"$1$salt\0".as_ptr());
        assert!(r.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn empty_key_still_hashes() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r = crypt_str(b"\0", b"$6$salt\0").unwrap();
        assert!(r.starts_with("$6$salt$"));
        // 86-char SHA-512 hash after the final '$'.
        let hash = r.rsplit('$').next().unwrap();
        assert_eq!(hash.len(), 86);
    }

    // -----------------------------------------------------------------------
    // crypt_r
    // -----------------------------------------------------------------------

    #[test]
    fn crypt_r_matches_crypt() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let via_crypt = crypt_str(b"Hello world!\0", b"$6$saltstring\0").unwrap();

        let mut buf = [0u8; CRYPT_OUTPUT_LEN];
        let r = crypt_r(b"Hello world!\0".as_ptr(), b"$6$saltstring\0".as_ptr(), buf.as_mut_ptr());
        assert!(!r.is_null());
        let via_r = unsafe { core::ffi::CStr::from_ptr(r.cast()) }
            .to_string_lossy()
            .into_owned();
        assert_eq!(via_r, via_crypt);
    }

    #[test]
    fn crypt_r_independent_buffers() {
        let mut buf1 = [0u8; CRYPT_OUTPUT_LEN];
        let mut buf2 = [0u8; CRYPT_OUTPUT_LEN];
        crypt_r(b"alpha\0".as_ptr(), b"$6$xx\0".as_ptr(), buf1.as_mut_ptr());
        crypt_r(b"beta\0".as_ptr(), b"$6$yy\0".as_ptr(), buf2.as_mut_ptr());
        let s1 = unsafe { core::ffi::CStr::from_ptr(buf1.as_ptr().cast()) };
        let s2 = unsafe { core::ffi::CStr::from_ptr(buf2.as_ptr().cast()) };
        assert!(s1.to_bytes().starts_with(b"$6$xx$"));
        assert!(s2.to_bytes().starts_with(b"$6$yy$"));
        assert_ne!(s1.to_bytes(), s2.to_bytes());
    }

    #[test]
    fn crypt_r_null_data_efault() {
        crate::errno::set_errno(0);
        let r = crypt_r(b"key\0".as_ptr(), b"$6$salt\0".as_ptr(), core::ptr::null_mut());
        assert!(r.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn crypt_r_unsupported_einval() {
        let mut buf = [0u8; CRYPT_OUTPUT_LEN];
        crate::errno::set_errno(0);
        let r = crypt_r(b"key\0".as_ptr(), b"ab\0".as_ptr(), buf.as_mut_ptr());
        assert!(r.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // rounds clamping
    // -----------------------------------------------------------------------

    #[test]
    fn rounds_below_min_are_clamped() {
        // rounds=10 -> clamped to ROUNDS_MIN (1000); the echoed field
        // must show the clamped value.
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r = crypt_str(b"x\0", b"$6$rounds=10$salt\0").unwrap();
        assert!(r.starts_with("$6$rounds=1000$salt$"));
    }

    #[test]
    fn malformed_rounds_becomes_salt() {
        // "rounds=abc" has no valid number -> treated as the salt
        // (truncated to 16 chars), no rounds field echoed.
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r = crypt_str(b"x\0", b"$6$rounds=abc$salt\0").unwrap();
        assert!(r.starts_with("$6$rounds=abc$"));
        assert!(!r.contains("rounds=abc$salt$")); // salt capped at 16: "rounds=abc" (10)
    }

    // -----------------------------------------------------------------------
    // encrypt / setkey
    // -----------------------------------------------------------------------

    #[test]
    fn encrypt_valid_reaches_enosys() {
        crate::errno::set_errno(0);
        let mut block = [0u8; 64];
        encrypt(block.as_mut_ptr(), 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn encrypt_null_block_efault() {
        crate::errno::set_errno(0);
        encrypt(core::ptr::null_mut(), 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn encrypt_bad_edflag_einval() {
        crate::errno::set_errno(0);
        let mut block = [0u8; 64];
        encrypt(block.as_mut_ptr(), 2);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn setkey_valid_reaches_enosys() {
        crate::errno::set_errno(0);
        let key = [0u8; 64];
        setkey(key.as_ptr());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn setkey_null_efault() {
        crate::errno::set_errno(0);
        setkey(core::ptr::null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn output_len_constant() {
        assert_eq!(CRYPT_OUTPUT_LEN, 128);
    }
}
