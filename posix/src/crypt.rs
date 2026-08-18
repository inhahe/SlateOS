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
//! ## Unsupported methods
//!
//! Legacy DES (two-character salt) crypt is **not** implemented.  Rather
//! than fabricate an insecure result, `crypt()` fails with `EINVAL` for
//! any setting it does not recognise — matching modern glibc/libxcrypt
//! behaviour.  (See `todo.txt` for the DES follow-up.)
//!
//! `encrypt`/`setkey` (raw DES block cipher) remain unimplemented and
//! return `ENOSYS` after argument validation.

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
/// the SHA methods accept an optional `rounds=N$`.  Returns a pointer to
/// a static buffer (overwritten by each call), or null on error:
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
    if !compute_crypt(key_s, salt_s, &mut out) {
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
    if !compute_crypt(key_s, salt_s, &mut out) {
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
    if !compute_crypt(key, setting, &mut ob) || ob.overflow {
        return None;
    }
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
/// Returns `None` if `setting` selects no method we implement, if the result
/// would not fit, or if the result is not valid UTF-8 — which can only
/// happen when `setting` carries a non-ASCII salt, and which anything about
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
