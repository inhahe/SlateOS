//! POSIX `<unistd.h>` / `<crypt.h>` password hashing.
//!
//! Implements the SHA-256 (`$5$`) and SHA-512 (`$6$`) crypt methods —
//! the modern shadow-suite defaults — following Ulrich Drepper's
//! specification ("Unix crypt using SHA-256 and SHA-512"), plus legacy
//! MD5 crypt (`$1$`, Poul-Henning Kamp's algorithm).  The hash cores
//! live in [`crate::sha2`] and [`crate::md5`]; this module implements the
//! salt/rounds parsing, the key-derivation rounds, and the crypt base-64
//! encoding.
//!
//! Previously `crypt()` returned `"$0$<key>"` — i.e. the password in
//! cleartext with a marker prefix.  Any program that hashed a password
//! and stored the result was effectively storing the plaintext.  That
//! was a security hole, now closed.
//!
//! ## Method strength
//!
//! `$6$` (SHA-512) is the recommended default.  `$1$` (MD5) is
//! cryptographically broken and is supported only so the OS can verify
//! existing `$1$` entries in legacy `/etc/shadow` files — never use it
//! for new passwords.
//!
//! ## Failure: the failure token, as libxcrypt answers
//!
//! glibc 2.39 no longer has a `crypt`; Linux distributions ship libxcrypt,
//! and Ubuntu 24.04's (libcrypt1 4.4.36) is built with *failure tokens*: a
//! `crypt` or `crypt_r` that fails still returns a string -- `"*0"`, or
//! `"*1"` when the setting itself begins `"*0"` -- with `errno` saying why.
//! A token can never equal a stored hash, so a program that compares the
//! result without checking it for NULL refuses the login instead of
//! crashing.  This module answers the same way, and refuses what libxcrypt
//! refuses, in its order (`do_crypt` in lib/crypt.c):
//!
//! | case | errno |
//! |---|---|
//! | the passphrase or the setting is NULL | `EINVAL` |
//! | the passphrase is 512 bytes or longer | `ERANGE` |
//! | the setting has a space, a control or non-ASCII byte, or one of `! * : ; \` | `EINVAL` |
//! | the setting names no method this module implements | `EINVAL` |
//!
//! Until 2026-09-26 a NULL argument was `EFAULT` and every failure returned
//! NULL, which a program ported from Linux does not expect.
//!
//! ## Unsupported methods
//!
//! Legacy DES (two-character salt), BSDi DES, bcrypt, scrypt, yescrypt and
//! libxcrypt's other methods are **not** implemented: their settings fail
//! with the token and `EINVAL`, never with a fabricated hash.  (See
//! `todo.txt` for the DES follow-up.)
//!
//! `encrypt`/`setkey` (raw DES block cipher) remain unimplemented and
//! answer `ENOSYS`.

#![allow(clippy::arithmetic_side_effects)] // Bounded counters / modular round arithmetic.
#![allow(clippy::indexing_slicing)] // Fixed-size digest arrays indexed by compile-time constants.

use crate::errno;
use crate::md5::Md5;
use crate::sha2::{Digest, Sha256, Sha512};

/// Maximum length of a crypt result string (including the NUL terminator).
///
/// The longest output we generate is a SHA-512 hash with an explicit
/// rounds field: `"$6$rounds=999999999$"` (20) + 16-byte salt + `"$"` (1)
/// + 86-character hash + NUL = 124 bytes, comfortably within this bound.
const CRYPT_OUTPUT_LEN: usize = 128;

/// Static buffer for `crypt()` results (non-reentrant, per POSIX).
static mut CRYPT_BUF: [u8; CRYPT_OUTPUT_LEN] = [0u8; CRYPT_OUTPUT_LEN];

/// libxcrypt's `CRYPT_MAX_PASSPHRASE_SIZE`: a passphrase of this many bytes
/// or more is refused with `ERANGE`.
const CRYPT_MAX_PASSPHRASE_SIZE: usize = 512;

/// The crypt base-64 alphabet (note: NOT standard base64 — `.` and `/`
/// lead, and the digit/letter order differs).
const B64_ALPHABET: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Default SHA-crypt rounds when no `rounds=` field is given.
const ROUNDS_DEFAULT: u32 = 5000;
/// Minimum permitted rounds (values below are clamped up).
const ROUNDS_MIN: u32 = 1000;
/// Maximum permitted rounds (values above are clamped down).
const ROUNDS_MAX: u32 = 999_999_999;
/// Maximum salt length in bytes for SHA-crypt (longer salts truncated).
const SALT_MAX: usize = 16;
/// Maximum salt length in bytes for MD5 crypt (`$1$`).
const MD5_SALT_MAX: usize = 8;

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

/// Parse an MD5-crypt (`$1$`) `setting` and, if recognised, compute the
/// full result (`"$1$salt$hash"`) into `out`.
///
/// Returns `true` if `setting` selected MD5 crypt and the result was
/// written; `false` otherwise (caller tries the next method).
///
/// Implements Poul-Henning Kamp's md5crypt exactly (including the
/// deliberately-obscure key-length bit loop, in which the running
/// digest has been zeroed before being mixed in).
fn md5_crypt(key: &[u8], setting: &[u8], out: &mut OutBuf) -> bool {
    let Some(rest) = setting.strip_prefix(b"$1$") else {
        return false;
    };

    // Salt = bytes up to the first '$', capped at MD5_SALT_MAX.
    let mut salt_end = 0;
    while salt_end < rest.len() && rest[salt_end] != b'$' {
        salt_end += 1;
    }
    let salt = &rest[..core::cmp::min(salt_end, MD5_SALT_MAX)];

    // Primary context: H(key || "$1$" || salt).
    let mut ctx = Md5::new();
    ctx.update(key);
    ctx.update(b"$1$");
    ctx.update(salt);

    // alt = H(key || salt || key).
    let alt = {
        let mut h = Md5::new();
        h.update(key);
        h.update(salt);
        h.update(key);
        h.finalize()
    };

    // Mix in key.len() bytes of `alt`, 16 at a time.
    let mut pl = key.len();
    while pl > 0 {
        let n = core::cmp::min(pl, Md5::OUTPUT_LEN);
        ctx.update(&alt[..n]);
        pl -= n;
    }

    // For each bit of key.len() (low -> high): set bit adds a zero byte,
    // clear bit adds key[0].  (key[0] is only reached when key is
    // non-empty, since the loop runs only while bits != 0.)
    let mut bits = key.len();
    while bits != 0 {
        if bits & 1 != 0 {
            ctx.update(&[0u8]);
        } else {
            ctx.update(&key[..1]);
        }
        bits >>= 1;
    }
    let mut digest = ctx.finalize();

    // 1000 rounds of recombination to slow brute force.
    for i in 0usize..1000 {
        let mut c = Md5::new();
        if i & 1 != 0 {
            c.update(key);
        } else {
            c.update(&digest);
        }
        if i % 3 != 0 {
            c.update(salt);
        }
        if i % 7 != 0 {
            c.update(key);
        }
        if i & 1 != 0 {
            c.update(&digest);
        } else {
            c.update(key);
        }
        digest = c.finalize();
    }

    // "$1$salt$" + 22-character md5crypt base64.
    out.push_slice(b"$1$");
    out.push_slice(salt);
    out.push(b'$');
    let f = &digest;
    b64_from_24bit(out, f[0], f[6], f[12], 4);
    b64_from_24bit(out, f[1], f[7], f[13], 4);
    b64_from_24bit(out, f[2], f[8], f[14], 4);
    b64_from_24bit(out, f[3], f[9], f[15], 4);
    b64_from_24bit(out, f[4], f[10], f[5], 4);
    b64_from_24bit(out, 0, 0, f[11], 2);
    out.push(0); // NUL terminator
    true
}

/// Dispatch a crypt `setting` to the matching method, writing the result
/// into `out`.  Returns `false` if no supported method recognises it.
fn compute_crypt(key: &[u8], setting: &[u8], out: &mut OutBuf) -> bool {
    md5_crypt(key, setting, out) || sha_crypt(key, setting, out)
}

/// libxcrypt's `check_badsalt_chars`: a setting may hold only printable ASCII
/// other than space and the five characters `passwd(5)` and `shadow(5)` use
/// as delimiters and markers (`! * : ; \`).
fn has_bad_setting_chars(setting: &[u8]) -> bool {
    setting
        .iter()
        .any(|&b| b <= 0x20 || b >= 0x7f || b"!*:;\\".contains(&b))
}

/// libxcrypt's `make_failure_token`, NUL included: `"*0"`, or `"*1"` when
/// the setting begins `"*0"` -- so a token fed back as a setting never
/// reproduces itself.
fn failure_token(setting: Option<&[u8]>) -> [u8; 3] {
    match setting {
        Some([b'*', b'0', ..]) => *b"*1\0",
        _ => *b"*0\0",
    }
}

/// libxcrypt's `do_crypt` after its NULL test: the passphrase's length, the
/// setting's characters, the method, then the hash -- the `errno` of the
/// first that fails.
fn do_crypt(key: &[u8], setting: &[u8], out: &mut OutBuf) -> Result<(), i32> {
    if key.len() >= CRYPT_MAX_PASSPHRASE_SIZE {
        return Err(errno::ERANGE);
    }
    if has_bad_setting_chars(setting) {
        return Err(errno::EINVAL);
    }
    if !compute_crypt(key, setting, out) {
        return Err(errno::EINVAL);
    }
    if out.overflow {
        return Err(errno::ERANGE);
    }
    Ok(())
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
/// Supports `$1$` (MD5), `$5$` (SHA-256), and `$6$` (SHA-512) settings;
/// the SHA methods accept an optional `rounds=N$`.  Returns a pointer to a
/// static buffer, overwritten by each call: the hash, or on failure
/// libxcrypt's failure token (`"*0"`, or `"*1"`) with `errno` set -- see the
/// module docs for when.  `crypt_r` into that buffer, as libxcrypt's is.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn crypt(key: *const u8, salt: *const u8) -> *mut u8 {
    // The one static buffer, as POSIX makes `crypt` non-reentrant; `crypt_r`
    // writes at most `CRYPT_OUTPUT_LEN` bytes into it.
    crypt_r(key, salt, (&raw mut CRYPT_BUF).cast::<u8>())
}

/// `crypt_r` — reentrant `crypt`.
///
/// As [`crypt`], but the result goes into the caller's `data` (a
/// `struct crypt_data`, whose `output` comes first; at least
/// [`CRYPT_OUTPUT_LEN`] bytes are written), and `data` is returned: the hash,
/// or the failure token with `errno` set.  The token is written before
/// anything is checked, as libxcrypt's `make_failure_token` is, so every
/// failure leaves it.
///
/// A NULL `data` returns NULL with `EFAULT`: libxcrypt faults writing the
/// token there, and a crash is the one answer this function cannot give.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn crypt_r(key: *const u8, salt: *const u8, data: *mut u8) -> *mut u8 {
    if data.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    // SAFETY: `salt` is NULL or, per the C contract, NUL-terminated.
    let setting = (!salt.is_null()).then(|| unsafe { cstr_slice(salt) });
    let token = failure_token(setting);
    // SAFETY: the caller's `data` holds at least `CRYPT_OUTPUT_LEN` bytes.
    unsafe { core::ptr::copy_nonoverlapping(token.as_ptr(), data, token.len()) };

    let Some(setting) = setting else {
        errno::set_errno(errno::EINVAL);
        return data;
    };
    if key.is_null() {
        errno::set_errno(errno::EINVAL);
        return data;
    }
    // SAFETY: non-NULL (checked) and, per the C contract, NUL-terminated.
    let key_s = unsafe { cstr_slice(key) };

    let mut out = OutBuf::new();
    if let Err(e) = do_crypt(key_s, setting, &mut out) {
        errno::set_errno(e);
        return data;
    }
    // SAFETY: `data` holds at least `CRYPT_OUTPUT_LEN` bytes, and `out.len`
    // is at most that.
    unsafe { core::ptr::copy_nonoverlapping(out.buf.as_ptr(), data, out.len) };
    data
}

/// `encrypt` — encrypt/decrypt a 64-bit block using DES.
///
/// Stub: DES is not implemented, so the answer is `ENOSYS`, POSIX's one
/// error for this function.  libxcrypt treats any non-zero `edflag` as
/// "decrypt" and refuses none, so neither does this (it said `EINVAL` for
/// one other than 0 or 1 until 2026-09-26).  A NULL `block` is `EFAULT`,
/// where libxcrypt would fault reading it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn encrypt(block: *mut u8, _edflag: i32) {
    if block.is_null() {
        errno::set_errno(errno::EFAULT);
        return;
    }
    errno::set_errno(errno::ENOSYS);
}

/// `setkey` — set the DES encryption key.
///
/// Stub: DES is not implemented, so the answer is `ENOSYS`.  A NULL `key` is
/// `EFAULT`, where libxcrypt would fault reading it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setkey(key: *const u8) {
    if key.is_null() {
        errno::set_errno(errno::EFAULT);
        return;
    }
    errno::set_errno(errno::ENOSYS);
}

// ---------------------------------------------------------------------------
// Safe Rust API
// ---------------------------------------------------------------------------
//
// `crypt()` above is the C ABI: raw pointers, a NUL terminator, and a
// process-global static buffer that the next call overwrites.  Each of those
// is a hazard for a Rust caller, and the callers that matter most are Rust:
// `passwd`, `chpasswd` and `login` all write and read the same
// `/etc/shadow`.  Before this existed all three hashed passwords themselves
// rather than reach through the C signature, and all three got it wrong in
// different ways — one invented a `$sha256$` format, two computed a made-up
// mixing function with no work factor, and one of those labelled the result
// `$5$`, which is the standard identifier for SHA-256 crypt.
//
// So the functions below are not a convenience wrapper; they are the
// interface the shadow-file tools were missing.  They are reentrant (the
// result lands in the caller's buffer), they cannot be called with a
// mismatched key/salt pointer, and `verify` removes the last thing a caller
// could still do by hand: choose which slice of the stored entry to compare.

/// The size of the scratch buffer the safe API writes into.
pub const BUF_LEN: usize = CRYPT_OUTPUT_LEN;

/// Scratch space for one crypt result.  See [`buf`].
pub type HashBuf = [u8; BUF_LEN];

/// A zeroed [`HashBuf`], for callers that would rather not name the size.
#[must_use]
pub fn buf() -> HashBuf {
    [0u8; BUF_LEN]
}

/// A password-hashing method, for building the setting of a *new* password.
///
/// Verification never needs this: a stored hash names its own method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// `$1$` — MD5 crypt.  Cryptographically broken; supported so existing
    /// entries can still be verified, never to be chosen for a new password.
    Md5,
    /// `$5$` — SHA-256 crypt.
    Sha256,
    /// `$6$` — SHA-512 crypt.  The shadow-suite default, and ours.
    Sha512,
}

impl Method {
    /// The crypt(3) identifier that names this method in `/etc/shadow`.
    #[must_use]
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Md5 => "$1$",
            Self::Sha256 => "$5$",
            Self::Sha512 => "$6$",
        }
    }

    /// How many crypt-base-64 characters this method's hash field holds.
    ///
    /// A fixed number, because the digest is a fixed size: 16 bytes for MD5
    /// (22 characters), 32 for SHA-256 (43), 64 for SHA-512 (86).  This is
    /// what [`stored_method`] checks, and it is how an entry this tree wrote
    /// before the safe API existed — 64 *hex* digits under a `$5$` label —
    /// is told apart from a genuine one, with no ambiguity in either
    /// direction.
    #[must_use]
    pub fn hash_len(self) -> usize {
        match self {
            Self::Md5 => 22,
            Self::Sha256 => 43,
            Self::Sha512 => 86,
        }
    }

    /// The longest salt this method uses.  A longer one is truncated when
    /// hashing, so an entry carrying one can never be reproduced.
    #[must_use]
    pub fn salt_max(self) -> usize {
        match self {
            Self::Md5 => MD5_SALT_MAX,
            Self::Sha256 | Self::Sha512 => SALT_MAX,
        }
    }

    /// The method named by a `$N$` prefix, if it is one we implement.
    fn from_prefix(setting: &[u8]) -> Option<Self> {
        match setting.get(..3)? {
            b"$1$" => Some(Self::Md5),
            b"$5$" => Some(Self::Sha256),
            b"$6$" => Some(Self::Sha512),
            _ => None,
        }
    }
}

/// Whether `b` is a character of the crypt base-64 alphabet.
fn is_b64(b: u8) -> bool {
    b == b'.' || b == b'/' || b.is_ascii_digit() || b.is_ascii_alphabetic()
}

/// Shared body of [`hash_into`] and [`verify`]: run the crypt and copy the
/// result into `out` *without* its NUL terminator, returning its length.
fn compute_into(key: &[u8], setting: &[u8], out: &mut HashBuf) -> Option<usize> {
    let mut ob = OutBuf::new();
    // `do_crypt`, so the Rust API refuses exactly what `crypt` refuses: a
    // stored entry this could verify and a C program could not would make
    // the two logins disagree about the same `/etc/shadow`.
    do_crypt(key, setting, &mut ob).ok()?;
    // `compute_crypt` NUL-terminates for the C API's benefit; the Rust API
    // reports a length instead, so the terminator is dropped here rather
    // than left for every caller to remember to strip.
    let len = ob.len.checked_sub(1)?;
    out.get_mut(..len)?.copy_from_slice(ob.buf.get(..len)?);
    Some(len)
}

/// Hash `key` under `setting`, writing the crypt string into `out`.
///
/// The safe equivalent of [`crypt`]: the same settings (`$1$`, `$5$`, `$6$`,
/// with an optional `rounds=N$`) and the same output, but reentrant — the
/// result lands in the caller's buffer, so a call on another thread cannot
/// replace it between it being computed and being read.
///
/// `setting` may be a bare `"$6$<salt>$"` (see [`setting_into`]) or a whole
/// stored hash, since the salt is read up to the first `$` either way.
///
/// Returns `None` if `crypt` would fail -- `setting` selects no method we
/// implement or holds a character `crypt` refuses, or `key` is 512 bytes or
/// longer -- if the result would not fit, or if the result is not valid
/// UTF-8, which the character check now rules out and which anything about
/// to write `/etc/shadow` wants rejected rather than stored.  Use [`verify`]
/// to check an existing entry; it works on bytes and so is unaffected.
pub fn hash_into<'o>(key: &[u8], setting: &[u8], out: &'o mut HashBuf) -> Option<&'o str> {
    let n = compute_into(key, setting, out)?;
    core::str::from_utf8(out.get(..n)?).ok()
}

/// Assemble a setting for a *new* password: `"$N$<salt>$"`.
///
/// Rejects a salt that is empty, longer than the method uses (a truncated
/// salt means the entry written is not the entry that was asked for), or
/// that holds anything outside the crypt base-64 alphabet — `$` above all,
/// which would silently end the salt early.
pub fn setting_into<'o>(method: Method, salt: &[u8], out: &'o mut HashBuf) -> Option<&'o str> {
    if salt.is_empty() || salt.len() > method.salt_max() || !salt.iter().copied().all(is_b64) {
        return None;
    }
    let prefix = method.prefix().as_bytes();
    let salt_end = prefix.len().checked_add(salt.len())?;
    let len = salt_end.checked_add(1)?;
    out.get_mut(..prefix.len())?.copy_from_slice(prefix);
    out.get_mut(prefix.len()..salt_end)?.copy_from_slice(salt);
    *out.get_mut(salt_end)? = b'$';
    core::str::from_utf8(out.get(..len)?).ok()
}

/// Assemble a setting with an explicit rounds field:
/// `"$N$rounds=R$<salt>$"`.
///
/// `hash_into` has always honoured a `rounds=` field when it found one in a
/// setting; nothing could produce a setting that carried it. The buffer was
/// even sized for `"$6$rounds=999999999$"` -- the capability was there and
/// the way to ask for it was not.
///
/// `rounds` IS CLAMPED HERE, to the same bounds `hash_into` applies when it
/// parses the field. That is the load-bearing part rather than a detail: if
/// this wrote `rounds=10` and the hash was then computed with 1000, the
/// stored entry would state a cost it was not produced at, and re-deriving
/// from the entry's own text would give a different answer. This file already
/// carries one such defect in its history -- a label and an algorithm
/// declared in different places, and disagreeing -- and the rule that came
/// out of it applies here too.
///
/// Returns `None` for [`Method::Md5`]: MD5 crypt has no rounds field, and
/// `$1$rounds=N$` would be read as a SALT beginning with `rounds=`, quietly
/// hashing a different password-salt pair than the caller asked for.
pub fn setting_rounds_into<'o>(
    method: Method,
    rounds: u32,
    salt: &[u8],
    out: &'o mut HashBuf,
) -> Option<&'o str> {
    if matches!(method, Method::Md5) {
        return None;
    }
    if salt.is_empty() || salt.len() > method.salt_max() || !salt.iter().copied().all(is_b64) {
        return None;
    }

    let rounds = rounds.clamp(ROUNDS_MIN, ROUNDS_MAX);
    let mut digits = [0u8; 10];
    let mut n = rounds;
    let mut at = digits.len();
    while at > 0 {
        at = at.checked_sub(1)?;
        *digits.get_mut(at)? = b'0'.checked_add(u8::try_from(n % 10).ok()?)?;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let digits = digits.get(at..)?;

    let prefix = method.prefix().as_bytes();
    let tag = b"rounds=";
    let mut end = 0usize;
    for part in [prefix, tag, digits, b"$", salt, b"$"] {
        let next = end.checked_add(part.len())?;
        out.get_mut(end..next)?.copy_from_slice(part);
        end = next;
    }
    core::str::from_utf8(out.get(..end)?).ok()
}

/// The rounds a SHA-crypt setting uses when it does not say.
#[must_use]
pub const fn rounds_default() -> u32 {
    ROUNDS_DEFAULT
}

/// The bounds `rounds` is clamped to, low then high.
#[must_use]
pub const fn rounds_bounds() -> (u32, u32) {
    (ROUNDS_MIN, ROUNDS_MAX)
}

/// Check `key` against a stored crypt hash, in constant time.
///
/// The stored hash *is* the setting — crypt's defining property is that
/// re-running it on the same password reproduces the entry exactly — so this
/// one call is the whole of password verification: no salt parsing, no
/// method dispatch, and no opportunity for a caller to compare the wrong
/// slice of the entry.
///
/// Returns `false` for anything this build cannot recompute.  That covers
/// every locked (`!`, `!!`, `*`) and empty entry, every entry in a format we
/// do not implement, and every entry whose salt is too long to reproduce.
/// Refusing to authenticate is the only safe answer to "I cannot check
/// this"; a caller that wants to *report* why should ask [`stored_method`]
/// first.
///
/// The comparison runs over the recomputed string, whose length is fixed by
/// the method named in the entry's own prefix, so returning early on a
/// length mismatch discloses nothing that reading the entry did not.
#[must_use]
pub fn verify(key: &[u8], stored: &[u8]) -> bool {
    let mut scratch = buf();
    let Some(len) = compute_into(key, stored, &mut scratch) else {
        return false;
    };
    let Some(computed) = scratch.get(..len) else {
        return false;
    };
    if computed.len() != stored.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in computed.iter().zip(stored.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// The method a stored entry names, if the entry has the exact shape that
/// method produces.
///
/// A *shape* check, not a verification: it reads the `$N$` prefix, skips an
/// optional `rounds=N$`, skips the salt, and requires what remains to be
/// exactly [`Method::hash_len`] characters of crypt base-64.
///
/// It exists to tell a genuine entry from one this tree wrote before the
/// safe API existed.  `chpasswd` labelled its output `$5$` while computing
/// something that was not SHA-crypt, and `passwd` invented `$sha256$`
/// outright; both wrote a 64-hex-digit hash field, against SHA-256 crypt's
/// 43 base-64 characters.  A caller that gets `None` here knows the entry
/// can never verify, and can say so instead of reporting a wrong password.
#[must_use]
pub fn stored_method(stored: &[u8]) -> Option<Method> {
    let method = Method::from_prefix(stored)?;
    let mut rest = stored.get(3..)?;

    // An explicit rounds field, which only the SHA methods accept.  A
    // malformed one is deliberately not skipped: `sha_crypt` lets it become
    // part of the salt, so the shape check has to agree.
    if method != Method::Md5 {
        if let Some(after) = rest.strip_prefix(b"rounds=") {
            let digits = after.iter().take_while(|b| b.is_ascii_digit()).count();
            if digits > 0 && after.get(digits) == Some(&b'$') {
                rest = after.get(digits.checked_add(1)?..)?;
            }
        }
    }

    let salt_end = rest.iter().position(|&b| b == b'$')?;
    if salt_end > method.salt_max() {
        return None;
    }
    let hash = rest.get(salt_end.checked_add(1)?..)?;
    if hash.len() != method.hash_len() || !hash.iter().copied().all(is_b64) {
        return None;
    }
    Some(method)
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

    /// Helper: call `crypt` and return the result as an owned `String`, or
    /// `None` for a failure -- the token, with `errno` set.
    fn crypt_str(key: &[u8], salt: &[u8]) -> Option<std::string::String> {
        let r = crypt(key.as_ptr(), salt.as_ptr());
        assert!(!r.is_null(), "crypt returns its buffer, the token included");
        let s = unsafe { core::ffi::CStr::from_ptr(r.cast()) };
        let s = s.to_string_lossy().into_owned();
        (!s.starts_with('*')).then_some(s)
    }

    /// `crypt(key, salt)`'s result and `errno`, NULL arguments allowed.
    fn crypt_raw(key: *const u8, salt: *const u8) -> (std::string::String, i32) {
        crate::errno::set_errno(0);
        let r = crypt(key, salt);
        let e = crate::errno::get_errno();
        assert!(!r.is_null());
        let s = unsafe { core::ffi::CStr::from_ptr(r.cast()) };
        (s.to_string_lossy().into_owned(), e)
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
        assert_eq!(
            r,
            "$5$saltstring$5B8vYYiY.CVt1RlTTf8KbXBH3hsxY/GNooZaBBGWEc5"
        );
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

    // What Ubuntu 24.04's libxcrypt answers, probed there on 2026-09-26:
    // `crypt(NULL, "$6$salt$")` and `crypt("pw", NULL)` are "*0" with EINVAL.

    #[test]
    fn null_key_is_the_token_and_einval() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let got = crypt_raw(core::ptr::null(), b"$6$salt\0".as_ptr());
        assert_eq!(got, ("*0".into(), crate::errno::EINVAL));
    }

    #[test]
    fn null_salt_is_the_token_and_einval() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let got = crypt_raw(b"key\0".as_ptr(), core::ptr::null());
        assert_eq!(got, ("*0".into(), crate::errno::EINVAL));
    }

    #[test]
    fn unsupported_method_is_the_token_and_einval() {
        // Legacy DES (2-char salt) and unknown markers are rejected, never
        // silently turned into a fake hash.
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let got = crypt_raw(b"password\0".as_ptr(), b"ab\0".as_ptr());
        assert_eq!(got, ("*0".into(), crate::errno::EINVAL));
    }

    #[test]
    fn a_token_as_the_setting_gives_the_other_token() {
        // So a failure fed back in as a setting cannot reproduce itself.
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let got = crypt_raw(b"pw\0".as_ptr(), b"*0\0".as_ptr());
        assert_eq!(got, ("*1".into(), crate::errno::EINVAL));
        let got = crypt_raw(b"pw\0".as_ptr(), b"*1\0".as_ptr());
        assert_eq!(got, ("*0".into(), crate::errno::EINVAL));
    }

    #[test]
    fn a_setting_with_a_delimiter_or_a_space_is_refused() {
        // Ubuntu: "$6$a b$" and "$6$a:b$" are "*0" with EINVAL.  They used to
        // hash, with the space or colon in the salt -- an entry that would
        // have split a line of /etc/shadow.
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for setting in [
            &b"$6$a b$\0"[..],
            b"$6$a:b$\0",
            b"$6$a;b$\0",
            b"$6$a!b$\0",
            b"$6$a*b$\0",
            b"$6$a\\b$\0",
            b"$6$a\tb$\0",
            b"$6$a\x7fb$\0",
            b"$6$a\xc3\xa9b$\0",
        ] {
            let got = crypt_raw(b"pw\0".as_ptr(), setting.as_ptr());
            assert_eq!(got, ("*0".into(), crate::errno::EINVAL), "{setting:?}");
        }
    }

    #[test]
    fn a_passphrase_of_512_bytes_is_erange() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut key = [b'a'; 513];
        key[511] = 0;
        let (h, e) = crypt_raw(key.as_ptr(), b"$6$salt$\0".as_ptr());
        assert!(h.starts_with("$6$salt$"), "511 bytes hash: {h} (errno {e})");
        key[511] = b'a';
        key[512] = 0;
        let got = crypt_raw(key.as_ptr(), b"$6$salt$\0".as_ptr());
        assert_eq!(got, ("*0".into(), crate::errno::ERANGE));
    }

    // -----------------------------------------------------------------------
    // MD5 ($1$) — vectors verified against OpenSSL 3.5 `passwd -1`
    // -----------------------------------------------------------------------

    #[test]
    fn md5_known_vector() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // salt "saltstri" (8 chars, the MD5 max).
        let r = crypt_str(b"Hello world!\0", b"$1$saltstri\0").unwrap();
        assert_eq!(r, "$1$saltstri$YMyguxXMBpd2TEZ.vS/3q1");
    }

    #[test]
    fn md5_known_vector_password() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r = crypt_str(b"password\0", b"$1$abcdefgh\0").unwrap();
        assert_eq!(r, "$1$abcdefgh$G//4keteveJp0qb8z2DxG/");
    }

    #[test]
    fn md5_empty_salt() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r = crypt_str(b"test\0", b"$1$\0").unwrap();
        assert_eq!(r, "$1$$whuMjZj.HMFoaTaZRRtkO0");
    }

    #[test]
    fn md5_salt_truncated_to_eight() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Passing a >8-char salt must behave as if truncated to 8 chars.
        let full = crypt_str(b"pw\0", b"$1$abcdefghIGNORED\0").unwrap();
        let trunc = crypt_str(b"pw\0", b"$1$abcdefgh\0").unwrap();
        assert_eq!(full, trunc);
        assert!(full.starts_with("$1$abcdefgh$"));
    }

    #[test]
    fn md5_not_plaintext() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r = crypt_str(b"plaintextpassword\0", b"$1$somesalt\0").unwrap();
        assert!(!r.contains("plaintextpassword"));
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
        let r = crypt_r(
            b"Hello world!\0".as_ptr(),
            b"$6$saltstring\0".as_ptr(),
            buf.as_mut_ptr(),
        );
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
        // libxcrypt faults writing its token through a NULL `data`.
        crate::errno::set_errno(0);
        let r = crypt_r(
            b"key\0".as_ptr(),
            b"$6$salt\0".as_ptr(),
            core::ptr::null_mut(),
        );
        assert!(r.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn crypt_r_failures_leave_the_token_in_data() {
        for (key, salt, want) in [
            (b"key\0".as_ptr(), b"ab\0".as_ptr(), crate::errno::EINVAL),
            (
                core::ptr::null(),
                b"$6$salt\0".as_ptr(),
                crate::errno::EINVAL,
            ),
            (b"key\0".as_ptr(), core::ptr::null(), crate::errno::EINVAL),
        ] {
            let mut buf = [0xAAu8; CRYPT_OUTPUT_LEN];
            crate::errno::set_errno(0);
            let r = crypt_r(key, salt, buf.as_mut_ptr());
            assert_eq!(r, buf.as_mut_ptr(), "data is returned");
            assert_eq!(&buf[..3], b"*0\0");
            assert_eq!(crate::errno::get_errno(), want);
        }
    }

    // -----------------------------------------------------------------------
    // rounds clamping
    // -----------------------------------------------------------------------

    /// A setting carries the rounds it was actually hashed at.
    ///
    /// The clamp is the point. If this wrote `rounds=10` while `hash_into`
    /// computed 1000, the stored entry would state a cost it was not produced
    /// at -- and re-deriving from the entry's own text would give a different
    /// answer than the tool that wrote it. So the assertion is not "it
    /// clamped" but "what it SAYS is what re-hashing the same input uses".
    #[test]
    fn setting_rounds_states_the_cost_it_was_hashed_at() {
        let mut out = buf();
        let s = setting_rounds_into(Method::Sha512, 12345, b"abcdefgh", &mut out)
            .expect("a valid salt and method");
        assert_eq!(s, "$6$rounds=12345$abcdefgh$");

        // Below the minimum: the field must say the clamped value, not the
        // one that was asked for.
        let mut out = buf();
        let s = setting_rounds_into(Method::Sha256, 10, b"abcdefgh", &mut out)
            .expect("a valid salt and method");
        let (lo, hi) = rounds_bounds();
        assert_eq!(s, format!("$5$rounds={lo}$abcdefgh$"));

        let mut out = buf();
        let s = setting_rounds_into(Method::Sha512, u32::MAX, b"abcdefgh", &mut out)
            .expect("a valid salt and method");
        assert_eq!(s, format!("$6$rounds={hi}$abcdefgh$"));

        // THE ROUND TRIP: hashing with the clamped setting and hashing with
        // the setting that names the clamped value must agree, or the entry
        // is mislabelled.
        let mut a = buf();
        let asked = setting_rounds_into(Method::Sha512, 10, b"abcdefgh", &mut a)
            .expect("valid")
            .to_string();
        let mut b = buf();
        let explicit = setting_rounds_into(Method::Sha512, lo, b"abcdefgh", &mut b)
            .expect("valid")
            .to_string();
        assert_eq!(asked, explicit);

        let mut h1 = buf();
        let mut h2 = buf();
        assert_eq!(
            hash_into(b"secret", asked.as_bytes(), &mut h1).map(str::to_string),
            hash_into(b"secret", explicit.as_bytes(), &mut h2).map(str::to_string),
        );
    }

    /// MD5 crypt has no rounds field, so asking for one is refused.
    ///
    /// `$1$rounds=N$` would be read as a SALT beginning with `rounds=`,
    /// hashing a different password-salt pair than the caller asked for --
    /// silently, and with a plausible-looking entry as the result.
    #[test]
    fn setting_rounds_refuses_md5_and_bad_salts() {
        let mut out = buf();
        assert!(setting_rounds_into(Method::Md5, 5000, b"abcdefgh", &mut out).is_none());
        // The same salt rules as `setting_into`.
        let mut out = buf();
        assert!(setting_rounds_into(Method::Sha512, 5000, b"", &mut out).is_none());
        let mut out = buf();
        assert!(setting_rounds_into(Method::Sha512, 5000, b"has$dollar", &mut out).is_none());
    }

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
    fn encrypt_any_edflag_is_accepted() {
        // libxcrypt reads a non-zero edflag as "decrypt" and refuses none,
        // so the answer is the stub's ENOSYS, not EINVAL.
        for edflag in [2, 7, -1] {
            crate::errno::set_errno(0);
            let mut block = [0u8; 64];
            encrypt(block.as_mut_ptr(), edflag);
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS, "{edflag}");
        }
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

    // -----------------------------------------------------------------------
    // Safe Rust API
    // -----------------------------------------------------------------------
    //
    // These need no `CRYPT_TEST_LOCK`: the whole point of the safe API is
    // that the result lands in the caller's buffer, so there is no shared
    // state for cargo's parallel runner to trample.  A test that *did* need
    // the lock would be evidence of a bug.

    /// The same Drepper vector the C API is checked against, so a divergence
    /// between the two paths shows up as a failure here rather than as an
    /// unexplained difference in `/etc/shadow`.
    #[test]
    fn hash_into_matches_the_drepper_vector() {
        let mut b = buf();
        assert_eq!(
            hash_into(b"Hello world!", b"$6$saltstring", &mut b),
            Some(
                "$6$saltstring$svn8UoSVapNtMuq1ukKS4tPQd8iKwSMHWjl/O817G3uBnIFNjnQJuesI68u4OTLiBFdcbYEdFCoEOfaS35inz1"
            )
        );
    }

    #[test]
    fn hash_into_rejects_an_unsupported_setting() {
        let mut b = buf();
        assert_eq!(hash_into(b"pw", b"$sha256$0123456789abcdef", &mut b), None);
        assert_eq!(hash_into(b"pw", b"plain", &mut b), None);
        assert_eq!(hash_into(b"pw", b"", &mut b), None);
    }

    #[test]
    fn hash_into_refuses_what_crypt_refuses() {
        let mut b = buf();
        assert_eq!(hash_into(b"pw", b"$6$a:b$", &mut b), None, "a delimiter");
        assert_eq!(
            hash_into(&[b'a'; 512], b"$6$salt$", &mut b),
            None,
            "512 bytes"
        );
        assert!(
            hash_into(&[b'a'; 511], b"$6$salt$", &mut b).is_some(),
            "511 bytes"
        );
    }

    /// Verification is defined by re-running crypt on the stored entry, so
    /// the entry a fresh hash produces must verify against itself.
    #[test]
    fn a_fresh_hash_verifies_against_itself() {
        for method in [Method::Md5, Method::Sha256, Method::Sha512] {
            let mut sb = buf();
            let setting = setting_into(method, b"aBcD1234", &mut sb)
                .unwrap_or_else(|| panic!("{method:?} setting"));
            let mut hb = buf();
            let hash = hash_into(b"correct horse", setting.as_bytes(), &mut hb)
                .unwrap_or_else(|| panic!("{method:?} hash"));
            assert!(
                verify(b"correct horse", hash.as_bytes()),
                "{method:?} did not verify its own output: {hash}"
            );
            assert!(!verify(b"correct hors", hash.as_bytes()), "{method:?}");
            assert!(!verify(b"", hash.as_bytes()), "{method:?}");
        }
    }

    /// The failure lane C reported: a password set by one tool could not be
    /// used by another, because the two disagreed about the format.  Going
    /// through this API there is only one format, so the round trip closes.
    #[test]
    fn a_password_set_through_the_api_verifies_through_the_api() {
        let mut sb = buf();
        let setting = setting_into(Method::Sha512, b"0123456789abcdef", &mut sb).expect("setting");
        let mut hb = buf();
        let stored = hash_into(b"correct horse", setting.as_bytes(), &mut hb).expect("hash");
        assert!(stored.starts_with("$6$0123456789abcdef$"));
        assert_eq!(stored_method(stored.as_bytes()), Some(Method::Sha512));
        assert!(verify(b"correct horse", stored.as_bytes()));
    }

    /// Locked and empty entries are not passwords, and must never
    /// authenticate — including against the empty password.
    #[test]
    fn verify_refuses_locked_and_unrecomputable_entries() {
        for stored in [
            &b"!"[..],
            b"!!",
            b"*",
            b"",
            b"x",
            b"!$6$salt$hash",
            b"$sha256$0123456789abcdef$0000",
        ] {
            assert!(!verify(b"", stored), "{stored:?}");
            assert!(!verify(b"correct horse", stored), "{stored:?}");
        }
    }

    /// An entry whose salt exceeds the method's maximum cannot be
    /// reproduced — hashing truncates the salt, so the recomputed header
    /// differs — and must therefore fail rather than half-match.
    #[test]
    fn verify_refuses_an_over_long_salt() {
        let over = b"$6$0123456789abcdefXYZ$";
        let mut hb = buf();
        let hash = hash_into(b"pw", over, &mut hb).expect("hash");
        assert!(hash.starts_with("$6$0123456789abcdef$"), "{hash}");
        let mut forged = std::string::String::from("$6$0123456789abcdefXYZ$");
        forged.push_str(hash.rsplit('$').next().expect("hash field"));
        assert!(!verify(b"pw", forged.as_bytes()));
        assert_eq!(stored_method(forged.as_bytes()), None);
    }

    /// The discriminator that makes the migration decidable: the entries
    /// this tree used to write carry a 64-hex-digit hash field, which is not
    /// the length any real method produces.
    #[test]
    fn stored_method_rejects_the_formats_this_tree_used_to_write() {
        let bogus = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(bogus.len(), 64);
        for prefix in ["$5$", "$6$", "$1$", "$sha256$"] {
            let entry = std::format!("{prefix}0123456789abcdef${bogus}");
            assert_eq!(
                stored_method(entry.as_bytes()),
                None,
                "{entry} was accepted as well-formed"
            );
        }
    }

    #[test]
    fn stored_method_accepts_genuine_entries() {
        for (entry, want) in [
            (
                "$6$saltstring$svn8UoSVapNtMuq1ukKS4tPQd8iKwSMHWjl/O817G3uBnIFNjnQJuesI68u4OTLiBFdcbYEdFCoEOfaS35inz1",
                Method::Sha512,
            ),
            (
                "$5$saltstring$5B8vYYiY.CVt1RlTTf8KbXBH3hsxY/GNooZaBBGWEc5",
                Method::Sha256,
            ),
            (
                "$5$rounds=10000$saltstringsaltst$3xv.VbSHBb41AL9AvLeujZkZRBAwqFMz2.opqey6IcA",
                Method::Sha256,
            ),
        ] {
            assert_eq!(stored_method(entry.as_bytes()), Some(want), "{entry}");
            // A shape this build calls well-formed must also be one it can
            // reproduce, or the two checks would disagree about the same
            // entry.
            let mut hb = buf();
            let again = hash_into(b"Hello world!", entry.as_bytes(), &mut hb).expect("rehash");
            assert_eq!(again.len(), entry.len(), "{entry}");
        }
    }

    #[test]
    fn setting_into_rejects_a_salt_it_cannot_carry_verbatim() {
        let mut b = buf();
        assert_eq!(setting_into(Method::Sha512, b"", &mut b), None);
        // `$` would end the salt early, so the entry would not name the salt
        // that was asked for.
        assert_eq!(setting_into(Method::Sha512, b"ab$cd", &mut b), None);
        assert_eq!(setting_into(Method::Sha512, b"ab cd", &mut b), None);
        assert_eq!(setting_into(Method::Sha512, b"\xffbad", &mut b), None);
        // 17 characters, one over SHA-crypt's maximum.
        assert_eq!(
            setting_into(Method::Sha512, b"0123456789abcdefg", &mut b),
            None
        );
        // MD5 truncates at 8, so 9 is over for it while fine for SHA.
        assert_eq!(setting_into(Method::Md5, b"012345678", &mut b), None);
        assert_eq!(
            setting_into(Method::Sha512, b"012345678", &mut b),
            Some("$6$012345678$")
        );
    }

    #[test]
    fn method_hash_lengths_match_what_the_methods_emit() {
        for method in [Method::Md5, Method::Sha256, Method::Sha512] {
            let mut sb = buf();
            let setting = setting_into(method, b"salt", &mut sb).expect("setting");
            let mut hb = buf();
            let hash = hash_into(b"pw", setting.as_bytes(), &mut hb).expect("hash");
            let field = hash.rsplit('$').next().expect("hash field");
            assert_eq!(field.len(), method.hash_len(), "{method:?}: {hash}");
        }
    }
}
