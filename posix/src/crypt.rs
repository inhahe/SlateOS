//! POSIX `<unistd.h>` / `<crypt.h>` password hashing.
//!
//! Implements the SHA-256 (`$5$`) and SHA-512 (`$6$`) crypt methods —
//! the shadow suite's — following Ulrich Drepper's specification ("Unix
//! crypt using SHA-256 and SHA-512"); legacy MD5 crypt (`$1$`, Poul-Henning
//! Kamp's algorithm); yescrypt (`$y$`) and scrypt (`$7$`), which Ubuntu,
//! Debian and Fedora hash new passwords with, and gost-yescrypt (`$gy$`),
//! ALT Linux's; bcrypt (`$2a$`, `$2b$`,
//! `$2x$`, `$2y$`), OpenBSD's; and the DES methods -- traditional DES (no
//! prefix), bigcrypt and BSDi's extended DES (`_`) -- the last three
//! families ported from libxcrypt (`yescrypt.rs`, `bcrypt.rs` and `des.rs`
//! read their settings).  The hashing itself -- SHA-2, MD5, the SHA-crypt
//! and md5crypt rounds, yescrypt's KDF, Streebog, Eksblowfish, DES -- is the `pwhash`
//! crate's, compiled for speed where this one is compiled for size (its
//! crate docs); this module implements the settings' parsing, the crypt
//! base-64 encoding, the C ABI, and the dispatch of a setting to its method.
//!
//! Previously `crypt()` returned `"$0$<key>"` — i.e. the password in
//! cleartext with a marker prefix.  Any program that hashed a password
//! and stored the result was effectively storing the plaintext.  That
//! was a security hole, now closed.
//!
//! ## Method strength
//!
//! `$y$` (yescrypt) is the strongest -- a guess costs 16 MiB of memory as
//! well as time -- and, as in libxcrypt, the preferred method:
//! `crypt_gensalt` makes a `$y$` setting when asked for no method in
//! particular, and `crypt_preferred_method` answers `"$y$"` (`gensalt.rs`).
//! `$1$` (MD5) is cryptographically broken and is supported only so the OS
//! can verify existing `$1$` entries in legacy `/etc/shadow` files — never
//! use it for new passwords.  `crypt_checksalt` answers
//! `CRYPT_SALT_METHOD_LEGACY` for it, as for `$5$` and `$2x$`.
//!
//! ## The rest of libxcrypt's interface
//!
//! `crypt_rn` and `crypt_ra` are here, beside `crypt_r`; `crypt_gensalt`,
//! its `_rn` and `_ra` forms, `crypt_checksalt` and `crypt_preferred_method`
//! are `gensalt.rs`'s.  C reaches them through `posix/include/crypt.h`, which
//! is libxcrypt's header in place of musl's.
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
//! | a `$5$` or `$6$` setting whose `rounds=` is not 1000 to 999999999 written plainly (no sign, no leading zero) and followed by `$` | `EINVAL` |
//! | a `$y$`, `$gy$` or `$7$` setting is 212 bytes or longer (see below) | `ERANGE` |
//! | a `$y$`, `$gy$` or `$7$` setting the method refuses, or no memory for its hash | `EINVAL` |
//! | a bcrypt setting the method refuses (a cost below 04), or its self-test failing | `EINVAL` |
//! | a DES setting with a character outside the salt alphabet where it reads one, or a `_` setting shorter than nine | `EINVAL` |
//! | a `$sha1`, `$md5` or `$3$` setting the method refuses | `EINVAL` |
//! | a `$sha1` or `$md5` setting whose hash would not fit the room (see below) | `ERANGE` |
//!
//! Until 2026-09-26 a NULL argument was `EFAULT` and every failure returned
//! NULL, which a program ported from Linux does not expect.
//!
//! ## The output's room: 256 bytes, where libxcrypt has 384
//!
//! `crypt_r` writes into its caller's `struct crypt_data`.  C here is
//! compiled against `posix/include/crypt.h`, whose `crypt_data` is
//! libxcrypt's -- 32 KiB, its `output` 384 bytes -- but an object compiled
//! against musl's header, whose `crypt_data` is 260 bytes, passes that, and
//! the two layouts share only the start of the struct.  So a result has 256
//! bytes, NUL included.  Every hash fits: the longest `$y$` one -- every parameter
//! in six characters, a 64-byte salt -- is 182 bytes.  What differs is
//! yescrypt's own test before hashing, which wants room for *all* of the
//! setting given, plus `$`, 43 characters and a NUL: here it passes for a
//! setting of up to 211 bytes, in libxcrypt for up to 339.  Every `$y$`
//! hash, used as the setting that verifies it, is within both; only a `$7$`
//! setting with a salt of over 150 characters, which nothing generates, is
//! refused here (`ERANGE`) and taken there; and so, likewise, are `$sha1`
//! and `$md5` settings whose salts run to some two hundred characters.
//!
//! ## Every method libxcrypt has
//!
//! Since 2026-10-07 every method libxcrypt 4.4.36 hashes is here, the
//! last three -- sha1crypt (`$sha1`, NetBSD's), SunMD5 (`$md5`, Solaris's)
//! and NT (`$3$`, FreeBSD's) -- in `sha1crypt.rs`, `sunmd5.rs` and
//! `nthash.rs`.  A setting that names none fails with the token and
//! `EINVAL`, never with a fabricated hash.
//!
//! `encrypt` and `setkey`, POSIX's DES block cipher, are `des.rs`'s.

#![allow(clippy::arithmetic_side_effects)] // Bounded counters / modular round arithmetic.
#![allow(clippy::indexing_slicing)] // Fixed-size digest arrays indexed by compile-time constants.

use crate::errno;

/// The room for a crypt result, NUL included: what `crypt_r` may write into
/// a C caller's `struct crypt_data` -- libxcrypt's 32 KiB as
/// `posix/include/crypt.h` declares it, or musl's 260 bytes in an object
/// compiled against musl's header (see "The output's room" above).
///
/// The longest SHA result is 124 bytes (`"$6$rounds=999999999$"`, a 16-byte
/// salt, `"$"`, 86 characters, NUL), the longest `$y$` one 183.  It was 128
/// until yescrypt, whose test before hashing measures the whole setting.
const CRYPT_OUTPUT_LEN: usize = 256;

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
/// The fewest rounds a `rounds=` field may ask for: one below is refused.
const ROUNDS_MIN: u32 = 1000;
/// The most rounds a `rounds=` field may ask for: one above is refused.
const ROUNDS_MAX: u32 = 999_999_999;
/// Maximum salt length in bytes for SHA-crypt (longer salts truncated).
const SALT_MAX: usize = 16;
/// Maximum salt length in bytes for MD5 crypt (`$1$`).
const MD5_SALT_MAX: usize = 8;
/// The longest salt a new yescrypt (`$y$`) or scrypt (`$7$`) setting may
/// carry: libxcrypt's `crypt_gensalt` takes at most 64 bytes of randomness,
/// which are 86 base-64 characters.
const YESCRYPT_SALT_MAX: usize = 86;
/// A bcrypt (`$2b$`) setting's salt: always 22 characters, 16 bytes.
const BCRYPT_SALT_LEN: usize = 22;

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
// SHA-crypt
// ---------------------------------------------------------------------------
//
// The digests themselves -- Drepper's steps 1 to 21, and md5crypt's thousand
// rounds -- are `pwhash::shacrypt` and `pwhash::md5crypt`, compiled for speed
// (pwhash's crate docs); here are the settings and the encodings.

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
/// `None` if `setting` is not a SHA-crypt one (`$5$`/`$6$`); otherwise
/// whether it was hashed -- refused (`EINVAL`) for a `rounds=` field
/// libxcrypt refuses ([`sha_rounds`]).
fn sha_crypt(key: &[u8], setting: &[u8], out: &mut OutBuf) -> Option<Result<(), Refusal>> {
    let (is_512, rest) = if let Some(r) = setting.strip_prefix(b"$6$") {
        (true, r)
    } else if let Some(r) = setting.strip_prefix(b"$5$") {
        (false, r)
    } else {
        return None;
    };

    // Optional "rounds=N$" prefix.
    let mut rounds = ROUNDS_DEFAULT;
    let mut rounds_custom = false;
    let mut salt_part = rest;
    if let Some(after) = rest.strip_prefix(b"rounds=") {
        let Some(count) = sha_rounds(after) else {
            return Some(Err(Refusal::Invalid));
        };
        rounds_custom = true;
        rounds = count;
        salt_part = &after[count_digits(after) + 1..];
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
        pwhash::shacrypt::sha512(key, salt, rounds, &mut alt);
        encode_sha512(out, &alt);
    } else {
        let mut alt = [0u8; 32];
        pwhash::shacrypt::sha256(key, salt, rounds, &mut alt);
        encode_sha256(out, &alt);
    }
    out.push(0); // NUL terminator
    Some(Ok(()))
}

/// The digits a `rounds=` field's count runs to.
fn count_digits(field: &[u8]) -> usize {
    field.iter().take_while(|b| b.is_ascii_digit()).count()
}

/// A SHA-crypt `rounds=` field's count, `field` being what follows
/// `rounds=` -- or `None` for one libxcrypt refuses (`EINVAL`): a count
/// that does not begin with a digit 1-9 (no zero, no leading zero, no
/// sign), one outside [`ROUNDS_MIN`]..=[`ROUNDS_MAX`] -- refused, not
/// clamped -- or one not followed by `$`.  (glibc's crypt, before it had
/// none, clamped a count outside the bounds and took a malformed field as
/// the salt's beginning; this library did too until 2026-10-07.)
fn sha_rounds(field: &[u8]) -> Option<u32> {
    if !matches!(field.first(), Some(b'1'..=b'9')) {
        return None;
    }
    let digits = count_digits(field);
    if field.get(digits) != Some(&b'$') {
        return None;
    }
    let mut count: u64 = 0;
    for &d in field.get(..digits)? {
        // Past u64, strtoul's overflow: refused all the same.
        count = count.checked_mul(10)?.checked_add(u64::from(d - b'0'))?;
    }
    let count = u32::try_from(count).ok()?;
    (ROUNDS_MIN..=ROUNDS_MAX).contains(&count).then_some(count)
}

/// Parse an MD5-crypt (`$1$`) `setting` and, if recognised, compute the
/// full result (`"$1$salt$hash"`) into `out`.
///
/// Returns `true` if `setting` selected MD5 crypt and the result was
/// written; `false` otherwise (caller tries the next method).
///
/// Poul-Henning Kamp's md5crypt; its digest is `pwhash::md5crypt`'s.
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

    let digest = pwhash::md5crypt::digest(key, salt);

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

/// Dispatch a crypt `setting` to the method it names, writing the result,
/// NUL-terminated, into `out`.  [`Refusal::Invalid`] if no method this
/// module implements takes it; yescrypt's and scrypt's own refusals as
/// they make them.
fn compute_crypt(key: &[u8], setting: &[u8], out: &mut OutBuf) -> Result<(), Refusal> {
    if crate::yescrypt::names_method(setting) {
        let len =
            crate::yescrypt::crypt(key, setting, &mut out.buf).map_err(
                |refused| match refused {
                    crate::yescrypt::Refused::Range => Refusal::Range,
                    crate::yescrypt::Refused::Invalid => Refusal::Invalid,
                },
            )?;
        // `crypt` leaves room for the NUL.
        out.len = len;
        out.push(0);
        return Ok(());
    }
    if crate::bcrypt::names_method(setting) {
        // 60 characters: within the room, with the NUL.
        let len = crate::bcrypt::crypt(key, setting, &mut out.buf).ok_or(Refusal::Invalid)?;
        out.len = len;
        out.push(0);
        return Ok(());
    }
    if md5_crypt(key, setting, out) {
        return Ok(());
    }
    if let Some(result) = sha_crypt(key, setting, out) {
        return result;
    }
    if crate::sha1crypt::names_method(setting) {
        let len = crate::sha1crypt::crypt(key, setting, &mut out.buf).map_err(|r| match r {
            crate::sha1crypt::Refusal::Range => Refusal::Range,
            crate::sha1crypt::Refusal::Invalid => Refusal::Invalid,
        })?;
        out.len = len;
        out.push(0);
        return Ok(());
    }
    if crate::sunmd5::names_method(setting) {
        let len = crate::sunmd5::crypt(key, setting, &mut out.buf)
            .ok_or(Refusal::Invalid)?
            .map_err(|()| Refusal::Range)?;
        out.len = len;
        out.push(0);
        return Ok(());
    }
    if crate::nthash::names_method(setting) {
        // 36 characters: within the room, with the NUL.
        let len = crate::nthash::crypt(key, setting, &mut out.buf).ok_or(Refusal::Invalid)?;
        out.len = len;
        out.push(0);
        return Ok(());
    }
    // Last, as libxcrypt matches them: traditional DES and bigcrypt have no
    // prefix, and take what begins with two salt characters -- which no
    // method above begins with.
    if crate::des::names_method(setting) {
        // bigcrypt's longest, 178 characters, is within the room.
        let len = crate::des::crypt(key, setting, &mut out.buf).ok_or(Refusal::Invalid)?;
        out.len = len;
        out.push(0);
        return Ok(());
    }
    Err(Refusal::Invalid)
}

/// libxcrypt's `check_badsalt_chars`: a setting may hold only printable ASCII
/// other than space and the five characters `passwd(5)` and `shadow(5)` use
/// as delimiters and markers (`! * : ; \`).
pub(crate) fn has_bad_setting_chars(setting: &[u8]) -> bool {
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

/// `make_failure_token(setting, output, size)` into `size` bytes at `out`:
/// the token when there is room for it and its NUL, else as much as fits --
/// `"*"` in two bytes, a NUL in one, nothing in none.
///
/// # Safety
///
/// `out` is writable for `size` bytes; when `size` is not positive it is
/// not touched, and may be NULL.
pub(crate) unsafe fn write_failure_token(setting: &[u8], out: *mut u8, size: i32) {
    let token = failure_token(Some(setting));
    let written: &[u8] = match size {
        3.. => &token,
        2 => b"*\0",
        1 => b"\0",
        _ => return,
    };
    // SAFETY: at most `size` bytes, which the caller vouches for.
    unsafe { core::ptr::copy_nonoverlapping(written.as_ptr(), out, written.len()) };
}

/// Zero `bytes` where the compiler may not drop the stores: `explicit_bzero`.
pub(crate) fn wipe(bytes: &mut [u8]) {
    // SAFETY: the slice's own bytes, all writable.
    unsafe { crate::string::explicit_bzero(bytes.as_mut_ptr(), bytes.len()) };
}

/// Why [`do_crypt`] refused -- libxcrypt's two failures, named here rather
/// than as `errno` values so that the Rust-native API, which other crates may
/// link directly, touches nothing of `errno`'s
/// (`scripts/check-one-libc-per-process.py`).  [`crypt_r`] turns them into
/// `errno`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refusal {
    /// `ERANGE`: a passphrase too long, a hash that did not fit, or a `$y$`
    /// or `$7$` setting longer than the output's room allows.
    Range,
    /// `EINVAL`: a setting with a character no method allows, no method, or
    /// one its method refuses.
    Invalid,
}

/// libxcrypt's `do_crypt` after its NULL test: the passphrase's length, the
/// setting's characters, the method, then the hash -- the first that fails.
fn do_crypt(key: &[u8], setting: &[u8], out: &mut OutBuf) -> Result<(), Refusal> {
    if key.len() >= CRYPT_MAX_PASSPHRASE_SIZE {
        return Err(Refusal::Range);
    }
    if has_bad_setting_chars(setting) {
        return Err(Refusal::Invalid);
    }
    compute_crypt(key, setting, out)?;
    if out.overflow {
        return Err(Refusal::Range);
    }
    Ok(())
}

/// View a NUL-terminated C string as a byte slice (excluding the NUL).
///
/// # Safety
///
/// `p` must be non-null and point to a valid NUL-terminated string.
pub(crate) unsafe fn cstr_slice<'a>(p: *const u8) -> &'a [u8] {
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
/// Supports `$1$` (MD5), `$5$` (SHA-256), `$6$` (SHA-512), `$y$` (yescrypt),
/// `$gy$` (gost-yescrypt), `$7$` (scrypt), `$2a$`/`$2b$`/`$2x$`/`$2y$`
/// (bcrypt), `_` (BSDi's DES)
/// and two salt characters (traditional DES and bigcrypt) settings; the
/// SHA methods accept an optional `rounds=N$`, and the others carry their
/// own parameters.  `crypt_gensalt` (`gensalt.rs`) makes new ones.  A
/// yescrypt hash takes as much memory as its setting asks for -- 16 MiB for
/// Ubuntu's `$y$j9T$` -- and fails if it cannot have it.  Returns a pointer to a
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
    if let Err(refusal) = do_crypt(key_s, setting, &mut out) {
        errno::set_errno(match refusal {
            Refusal::Range => errno::ERANGE,
            Refusal::Invalid => errno::EINVAL,
        });
        return data;
    }
    // SAFETY: `data` holds at least `CRYPT_OUTPUT_LEN` bytes, and `out.len`
    // is at most that.
    unsafe { core::ptr::copy_nonoverlapping(out.buf.as_ptr(), data, out.len) };
    data
}

/// `sizeof (struct crypt_data)` as `posix/include/crypt.h` declares it --
/// libxcrypt's, 32 KiB: what `crypt_rn` asks for and `crypt_ra` allocates.
/// (No C constant has the name: C says `sizeof`.)
pub(crate) const CRYPT_DATA_SIZE: usize = 32768;

/// `crypt_rn` — as [`crypt_r`], into `data`'s `size` bytes, which must be
/// at least `sizeof (struct crypt_data)` ([`CRYPT_DATA_SIZE`]); and on
/// failure NULL, with `errno` set, where `crypt_r` gives the failure token.
/// The token is in `data` all the same, as much of it as `size` has room
/// for, as libxcrypt's is.
///
/// A NULL `data` is NULL with `EFAULT`, where libxcrypt faults writing the
/// token -- unless `size` gives it no room, when libxcrypt writes none and
/// answers `ERANGE`, and so does this.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn crypt_rn(key: *const u8, salt: *const u8, data: *mut u8, size: i32) -> *mut u8 {
    if data.is_null() && size > 0 {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    // SAFETY: `salt` is NULL or, per the C contract, NUL-terminated.
    let setting = (!salt.is_null()).then(|| unsafe { cstr_slice(salt) });
    // SAFETY: the caller's `size` bytes at `data`, of which this writes at
    // most three -- none when `size` is not positive, as when it is NULL.
    unsafe { write_failure_token(setting.unwrap_or_default(), data, size) };
    // Negative sizes included.
    if size < CRYPT_DATA_SIZE as i32 {
        errno::set_errno(errno::ERANGE);
        return core::ptr::null_mut();
    }
    let result = crypt_r(key, salt, data);
    // SAFETY: `crypt_r` returned `data`, whose first byte it wrote.
    if unsafe { *result } == b'*' {
        core::ptr::null_mut()
    } else {
        result
    }
}

/// `crypt_ra` — as [`crypt_rn`], into memory from `malloc`: `*data` is NULL
/// or `malloc`'s, `*size` its size, and either is made `CRYPT_DATA_SIZE`
/// bytes if it is less.  The caller frees `*data`.  NULL on failure, with
/// `errno` set, and `*data` still the caller's to free.
///
/// NULL `data` or `size` is NULL with `EFAULT`, where libxcrypt faults.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn crypt_ra(
    key: *const u8,
    salt: *const u8,
    data: *mut *mut u8,
    size: *mut i32,
) -> *mut u8 {
    if data.is_null() || size.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    // SAFETY: both non-NULL (checked), the caller's to read and write.
    unsafe {
        if (*data).is_null() {
            let fresh = crate::malloc::malloc(CRYPT_DATA_SIZE);
            if fresh.is_null() {
                return core::ptr::null_mut();
            }
            *data = fresh;
            *size = CRYPT_DATA_SIZE as i32;
        }
        if *size < CRYPT_DATA_SIZE as i32 {
            let grown = crate::malloc::realloc(*data, CRYPT_DATA_SIZE);
            if grown.is_null() {
                return core::ptr::null_mut();
            }
            *data = grown;
            *size = CRYPT_DATA_SIZE as i32;
        }
    }
    // SAFETY: `*data` is now `CRYPT_DATA_SIZE` bytes of the caller's.
    let result = crypt_r(key, salt, unsafe { *data });
    // SAFETY: `crypt_r` returned `*data`, whose first byte it wrote.
    if unsafe { *result } == b'*' {
        core::ptr::null_mut()
    } else {
        result
    }
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
    /// `$y$` — yescrypt, the default of Ubuntu, Debian and Fedora: a guess
    /// costs memory as well as time.  A new setting asks for libxcrypt's
    /// default cost, `$y$j9T$`: 16 MiB a hash.
    Yescrypt,
    /// `$7$` — scrypt, yescrypt's ancestor, as libxcrypt writes it.  A new
    /// setting asks for libxcrypt's default cost: N = 2^14, r = 32, 64 MiB a
    /// hash.
    Scrypt,
    /// `$gy$` — gost-yescrypt, ALT Linux's default: yescrypt's hash through
    /// two HMACs of GOST R 34.11-2012 (`yescrypt.rs`).  A new setting asks
    /// for yescrypt's default cost, `$gy$j9T$`.
    GostYescrypt,
    /// `$2b$` — bcrypt, OpenBSD's default, which `htpasswd -B` writes.  A
    /// new setting asks for libxcrypt's default cost, `$2b$05$`: 2^5 rounds
    /// of Eksblowfish's key setup.  Stored `$2a$`, `$2x$` and `$2y$` entries
    /// are bcrypt too, each its own history of the algorithm (`bcrypt.rs`).
    Bcrypt,
    /// Traditional DES, with no prefix: a two-character salt and eight
    /// characters of password -- and bigcrypt, its extension to longer
    /// passwords, eleven characters more of hash for each eight more of
    /// password, up to 128 (`des.rs`).  For verifying entries from before
    /// MD5 crypt; never for a new password, a DES key being 56 bits, so
    /// [`setting_into`] makes no setting for it.
    Des,
    /// `_` — BSDi's extended DES: a count of encryptions, a 24-bit salt and
    /// the whole password.  For verifying, as [`Method::Des`].
    BsdiDes,
    /// `$sha1$` — sha1crypt, NetBSD's: PBKDF1 over HMAC-SHA1
    /// (`sha1crypt.rs`).  For verifying, as [`Method::Des`]: libxcrypt
    /// counts it a legacy method.
    Sha1crypt,
    /// `$md5` — SunMD5, Solaris's (`sunmd5.rs`).  For verifying, as
    /// [`Method::Des`].
    SunMd5,
    /// `$3$` — the NT hash, MD4 of the password, as FreeBSD writes it
    /// (`nthash.rs`): no salt and no cost.  For verifying, as
    /// [`Method::Des`].
    Nt,
}

impl Method {
    /// The crypt(3) identifier that names this method in `/etc/shadow`:
    /// for traditional DES, none.
    #[must_use]
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Md5 => "$1$",
            Self::Sha256 => "$5$",
            Self::Sha512 => "$6$",
            Self::Yescrypt => "$y$",
            Self::GostYescrypt => "$gy$",
            Self::Scrypt => "$7$",
            Self::Bcrypt => "$2b$",
            Self::Des => "",
            Self::BsdiDes => "_",
            Self::Sha1crypt => "$sha1$",
            Self::SunMd5 => "$md5",
            Self::Nt => "$3$",
        }
    }

    /// What a new setting for this method holds before its salt: the
    /// identifier and, for yescrypt and scrypt, their parameters --
    /// libxcrypt's defaults (`crypt_gensalt` with a count of 0).  (The DES
    /// methods make no new setting: [`Method::takes_salt`] refuses them.)
    fn setting_head(self) -> &'static str {
        match self {
            Self::Yescrypt => "$y$j9T$",
            Self::GostYescrypt => "$gy$j9T$",
            // N = 2^14 ('C'), then r = 32 and p = 1 in five characters each.
            Self::Scrypt => "$7$CU..../....",
            // Cost 5, in two digits.
            Self::Bcrypt => "$2b$05$",
            Self::Md5
            | Self::Sha256
            | Self::Sha512
            | Self::Des
            | Self::BsdiDes
            | Self::Sha1crypt
            | Self::SunMd5
            | Self::Nt => self.prefix(),
        }
    }

    /// How many crypt-base-64 characters this method's hash field holds.
    ///
    /// A fixed number, because the digest is a fixed size: 16 bytes for MD5
    /// (22 characters), 23 for bcrypt (31), 32 for SHA-256, yescrypt and
    /// scrypt (43), 64 for SHA-512 (86), and a DES block (11) -- for a
    /// bigcrypt entry, each of its one to sixteen.  This is what
    /// [`stored_method`] checks, and it is how
    /// an entry this tree wrote before the safe API existed — 64 *hex*
    /// digits under a `$5$` label — is told apart from a genuine one, with
    /// no ambiguity in either direction.
    #[must_use]
    pub fn hash_len(self) -> usize {
        match self {
            Self::Md5 => 22,
            Self::Sha256 | Self::Yescrypt | Self::GostYescrypt | Self::Scrypt => 43,
            Self::Bcrypt => 31,
            Self::Sha512 => 86,
            Self::Des | Self::BsdiDes => 11,
            Self::Sha1crypt => 28,
            Self::SunMd5 => 22,
            Self::Nt => 32,
        }
    }

    /// The longest salt a new setting for this method may carry.
    ///
    /// For MD5 and the SHA methods it is the longest they use: a longer one
    /// is truncated when hashing, so an entry carrying one can never be
    /// reproduced.  For yescrypt and scrypt it is libxcrypt's: 64 bytes of
    /// randomness, in 86 characters.  For bcrypt it is also the shortest: a
    /// salt is always 16 bytes, 22 characters.  For the DES methods it is
    /// what an entry carries -- two characters, or BSDi's eight of count and
    /// salt -- though they make no new setting.
    #[must_use]
    pub fn salt_max(self) -> usize {
        match self {
            Self::Md5 => MD5_SALT_MAX,
            Self::Sha256 | Self::Sha512 => SALT_MAX,
            Self::Yescrypt | Self::GostYescrypt | Self::Scrypt => YESCRYPT_SALT_MAX,
            Self::Bcrypt => BCRYPT_SALT_LEN,
            Self::Des => 2,
            Self::BsdiDes => 8,
            Self::Sha1crypt => 64,
            Self::SunMd5 => 8,
            Self::Nt => 0,
        }
    }

    /// Whether `salt` may follow this method's setting head: crypt base-64,
    /// non-empty, no longer than [`Method::salt_max`] -- and, for yescrypt,
    /// whose salt is the bytes the characters decode to, characters that
    /// decode; for bcrypt, exactly 22 characters, the last as bcrypt writes
    /// it (it holds two bits, so `crypt` would rewrite any other).  None,
    /// for the DES methods: no new password should be hashed with them.
    fn takes_salt(self, salt: &[u8]) -> bool {
        !matches!(
            self,
            Self::Des | Self::BsdiDes | Self::Sha1crypt | Self::SunMd5 | Self::Nt
        ) && !salt.is_empty()
            && salt.len() <= self.salt_max()
            && salt.iter().copied().all(is_b64)
            && (!matches!(self, Self::Yescrypt | Self::GostYescrypt)
                || crate::yescrypt::is_yescrypt_salt(salt))
            && (self != Self::Bcrypt || crate::bcrypt::is_salt(salt))
    }

    /// The method a setting or a stored entry names, if it is one we
    /// implement: by its `$N$` prefix -- or `_`, BSDi's -- and, with none,
    /// by two salt characters first, traditional DES's.
    fn from_prefix(setting: &[u8]) -> Option<Self> {
        if !setting.is_empty() && crate::des::names_method(setting) {
            return Some(if setting[0] == b'_' {
                Self::BsdiDes
            } else {
                Self::Des
            });
        }
        if setting.starts_with(b"$gy$") {
            return Some(Self::GostYescrypt);
        }
        if crate::sha1crypt::names_method(setting) {
            return Some(Self::Sha1crypt);
        }
        if crate::sunmd5::names_method(setting) {
            return Some(Self::SunMd5);
        }
        if crate::nthash::names_method(setting) {
            return Some(Self::Nt);
        }
        match setting.get(..3)? {
            b"$1$" => Some(Self::Md5),
            b"$5$" => Some(Self::Sha256),
            b"$6$" => Some(Self::Sha512),
            b"$y$" => Some(Self::Yescrypt),
            b"$7$" => Some(Self::Scrypt),
            _ if crate::bcrypt::names_method(setting) => Some(Self::Bcrypt),
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
/// with an optional `rounds=N$`, and `$y$` and `$7$`) and the same output,
/// but reentrant — the result lands in the caller's buffer, so a call on
/// another thread cannot replace it between it being computed and being
/// read.
///
/// `setting` may be a bare `"$6$<salt>$"` (see [`setting_into`]) or a whole
/// stored hash, since the salt is read up to the first `$` either way --
/// for yescrypt and scrypt, up to the last, which in a stored hash is the
/// one before the hash; bcrypt's is always 22 characters, and what follows
/// them is not read.
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

/// Assemble a setting for a *new* password: `"$N$<salt>$"` -- for yescrypt
/// `"$y$j9T$<salt>$"`, for scrypt `"$7$CU..../....<salt>$"` and for bcrypt
/// `"$2b$05$<salt>$"`, libxcrypt's default costs (see [`Method`]).
///
/// Rejects a salt that is empty, longer than [`Method::salt_max`] (for MD5
/// and SHA, a truncated salt means the entry written is not the entry that
/// was asked for), or that holds anything outside the crypt base-64
/// alphabet — `$` above all, which would silently end the salt early.  A
/// yescrypt salt is the bytes its characters decode to, so it must also
/// decode: a whole number of bytes, the spare bits of its last character
/// zero, as `crypt_gensalt` writes them.
pub fn setting_into<'o>(method: Method, salt: &[u8], out: &'o mut HashBuf) -> Option<&'o str> {
    if !method.takes_salt(salt) {
        return None;
    }
    let prefix = method.setting_head().as_bytes();
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
/// `rounds` IS CLAMPED HERE, to the bounds `hash_into` takes a field within
/// -- it refuses one outside them, as libxcrypt does (until 2026-10-07 it
/// clamped, as glibc's crypt did). That is the load-bearing part rather
/// than a detail: a setting stating `rounds=10` would be no setting at all,
/// and one hashed at a cost other than the one it states would not
/// reproduce from its own text. This file already
/// carries one such defect in its history -- a label and an algorithm
/// declared in different places, and disagreeing -- and the rule that came
/// out of it applies here too.
///
/// Returns `None` for [`Method::Md5`]: MD5 crypt has no rounds field, and
/// `$1$rounds=N$` would be read as a SALT beginning with `rounds=`, quietly
/// hashing a different password-salt pair than the caller asked for.  And
/// for [`Method::Yescrypt`], [`Method::Scrypt`] and [`Method::Bcrypt`],
/// whose cost is in their parameters, not a rounds field.
pub fn setting_rounds_into<'o>(
    method: Method,
    rounds: u32,
    salt: &[u8],
    out: &'o mut HashBuf,
) -> Option<&'o str> {
    if !matches!(method, Method::Sha256 | Method::Sha512) || !method.takes_salt(salt) {
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

/// The bounds a `rounds=` field must be within, low then high: `crypt`
/// refuses one outside them, and [`setting_rounds_into`] clamps to them.
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
/// exactly [`Method::hash_len`] characters of crypt base-64.  A yescrypt or
/// scrypt entry carries its parameters, so for those it reads them as
/// `crypt` would and requires ones it accepts -- whether the memory they
/// ask for will be there is the one thing it cannot tell.  A DES entry has
/// no `$`: two salt characters and one to sixteen blocks of eleven
/// (traditional DES's one, bigcrypt's more), or BSDi's `_`, eight
/// characters of count and salt, and one block.
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
    if matches!(method, Method::Des | Method::BsdiDes) {
        return crate::des::stored_kind(stored).is_some().then_some(method);
    }
    if method == Method::Sha1crypt {
        return crate::sha1crypt::is_stored_hash(stored, CRYPT_OUTPUT_LEN).then_some(method);
    }
    if method == Method::SunMd5 {
        return crate::sunmd5::is_stored_hash(stored, CRYPT_OUTPUT_LEN).then_some(method);
    }
    if method == Method::Nt {
        return crate::nthash::is_stored_hash(stored).then_some(method);
    }
    if matches!(
        method,
        Method::Yescrypt | Method::GostYescrypt | Method::Scrypt
    ) {
        return crate::yescrypt::is_stored_hash(stored, CRYPT_OUTPUT_LEN).then_some(method);
    }
    if method == Method::Bcrypt {
        return crate::bcrypt::is_stored_hash(stored).then_some(method);
    }
    let mut rest = stored.get(3..)?;

    // An explicit rounds field, which only the SHA methods accept -- and
    // only as `sha_crypt` reads it: one it refuses makes the entry one that
    // can never verify.
    if method != Method::Md5 {
        if let Some(after) = rest.strip_prefix(b"rounds=") {
            sha_rounds(after)?;
            rest = after.get(count_digits(after).checked_add(1)?..)?;
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
    // libxcrypt as the oracle
    // -----------------------------------------------------------------------

    /// `name`'s errno value, as the oracle spells it.
    fn errno_named(name: &str) -> i32 {
        match name {
            "0" => 0,
            "EINVAL" => crate::errno::EINVAL,
            "ERANGE" => crate::errno::ERANGE,
            other => panic!("an errno the oracle should not give: {other}"),
        }
    }

    /// libxcrypt's own answers (`posix/tools/oracle/crypt_harness.py`): its
    /// table of known answers for every method this module implements, and
    /// yescrypt's, scrypt's and bcrypt's parameters, salts and refusals.
    /// Each through `crypt` -- the result, and `errno` when it fails -- and
    /// each hash's stored form through `stored_method`, which names the
    /// method it begins with, and for yescrypt, scrypt and bcrypt through
    /// `verify`.
    #[test]
    fn libxcrypt_answers() {
        const ORACLE: &str = include_str!("crypt_oracle.txt");
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut lines = 0;
        for line in ORACLE.lines().filter(|l| !l.starts_with('#')) {
            lines += 1;
            let (call, answer) = line.split_once(" = ").expect("`call = answer`");
            let (hex, setting) = call.split_once(' ').expect("`password setting`");
            let (result, errno) = answer.rsplit_once(' ').expect("`result errno`");
            let mut key: std::vec::Vec<u8> = if hex == "-" {
                std::vec::Vec::new()
            } else {
                (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
                    .collect()
            };
            let password = key.clone();
            key.push(0);
            let c_setting = std::format!("{setting}\0");
            let (got, got_errno) = crypt_raw(key.as_ptr(), c_setting.as_ptr());
            assert_eq!(got, result, "{line}");
            // errno says something only of a failure (C11 7.5): libxcrypt
            // leaves ENOMEM behind a hash of 32 MiB or more that it got by
            // asking for huge pages first, and being refused.
            if result.starts_with('*') {
                assert_eq!(got_errno, errno_named(errno), "{line}");
                continue;
            }
            let named = Method::from_prefix(result.as_bytes());
            assert!(named.is_some(), "{line}");
            assert_eq!(stored_method(result.as_bytes()), named, "{line}");
            // A stored yescrypt, gost-yescrypt, scrypt, bcrypt or DES hash
            // is its own setting -- yescrypt's salt read to the last `$`,
            // bcrypt's 22 characters and nothing after, DES's two characters
            // (and its length, which tells a bigcrypt hash's blocks from
            // one), BSDi's nine: new here, so each is verified too.  (A
            // second hash each; the SHA and MD5 entries' verification has
            // tests of its own, and would double this test's time.)
            if matches!(
                named,
                Some(
                    Method::Yescrypt
                        | Method::GostYescrypt
                        | Method::Scrypt
                        | Method::Bcrypt
                        | Method::Des
                        | Method::BsdiDes
                        | Method::Sha1crypt
                        | Method::SunMd5
                        | Method::Nt
                )
            ) {
                assert!(verify(&password, result.as_bytes()), "{line}");
            }
        }
        assert_eq!(lines, 6969, "the oracle's every line");
    }

    // -----------------------------------------------------------------------
    // yescrypt and scrypt
    // -----------------------------------------------------------------------

    /// yescrypt's test before hashing wants room for all of the setting,
    /// `$`, 43 characters and a NUL: 256 bytes here, so a setting of 211
    /// bytes is hashed and one of 212 is `ERANGE` (libxcrypt's 384 bytes
    /// take settings to 339: "The output's room" in the module docs).
    #[test]
    fn a_setting_too_long_for_the_room_is_erange() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // `$7$`, N = 16, r = 1, p = 1, and a salt of raw characters.
        let head = "$7$2/..../....";
        let fits = std::format!("{head}{}\0", "s".repeat(211 - head.len()));
        let (got, e) = crypt_raw(b"pw\0".as_ptr(), fits.as_ptr());
        assert_eq!(e, 0);
        assert_eq!(got.len(), 211 + 1 + 43);
        let over = std::format!("{head}{}\0", "s".repeat(212 - head.len()));
        let got = crypt_raw(b"pw\0".as_ptr(), over.as_ptr());
        assert_eq!(got, ("*0".into(), crate::errno::ERANGE));
    }

    /// A hash whose memory cannot be had fails, with `EINVAL`, and at once:
    /// N = 2^31 blocks of r = 2^20, 256 PiB, which no allocator gives.  Its
    /// parameters are ones the KDF takes, so the entry's shape is sound.
    #[test]
    fn a_hash_without_its_memory_is_einval() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Flavour 'j', N_log2 31 ('S'), and r 2^20 in the variable-length
        // code (`crypt_harness.py`'s `y(RW, 31, 1 << 20)`).
        let setting = "$y$jSy/vrD$LdJMENpBABJJ3hIHjB1Bi.";
        let c_setting = std::format!("{setting}\0");
        let got = crypt_raw(b"pw\0".as_ptr(), c_setting.as_ptr());
        assert_eq!(got, ("*0".into(), crate::errno::EINVAL));
        let stored = std::format!("{setting}${}", "A".repeat(43));
        assert_eq!(stored_method(stored.as_bytes()), Some(Method::Yescrypt));
    }

    /// A new setting asks for libxcrypt's default cost, and hashes as
    /// libxcrypt does: Ubuntu's `$y$j9T$`, checked against the oracle's
    /// answer for the same salt.
    #[test]
    fn a_new_yescrypt_setting_is_ubuntus() {
        let mut sb = buf();
        let setting = setting_into(Method::Yescrypt, b"PKXc3hCOSyMqdaEQArI62/", &mut sb).unwrap();
        assert_eq!(setting, "$y$j9T$PKXc3hCOSyMqdaEQArI62/$");
        let setting = std::string::String::from(setting);
        let mut hb = buf();
        assert_eq!(
            hash_into(b"pleaseletmein", setting.as_bytes(), &mut hb),
            Some("$y$j9T$PKXc3hCOSyMqdaEQArI62/$6ks28kkbpf7JiBlPEqR8s9sTF8ybIkPXN7OLBaSqEg7")
        );
        let mut sb = buf();
        assert_eq!(
            setting_into(Method::Scrypt, b"SodiumChloride", &mut sb),
            Some("$7$CU..../....SodiumChloride$")
        );
    }

    /// A yescrypt salt is the bytes its characters decode to, so a new
    /// setting's must decode; scrypt's is its characters.  Neither has a
    /// rounds field.
    #[test]
    fn yescrypt_salts_must_decode() {
        let mut sb = buf();
        // One character is not a whole byte; "zz" leaves bits set past one.
        for salt in [&b"z"[..], b"zz", b"ab-c", b"$abc"] {
            assert_eq!(
                setting_into(Method::Yescrypt, salt, &mut sb),
                None,
                "{salt:?}"
            );
        }
        assert!(setting_into(Method::Yescrypt, b"z.", &mut sb).is_some());
        assert!(setting_into(Method::Yescrypt, &[b'.'; 86], &mut sb).is_some());
        assert_eq!(setting_into(Method::Yescrypt, &[b'.'; 87], &mut sb), None);
        assert!(setting_into(Method::Scrypt, b"z", &mut sb).is_some());
        assert_eq!(setting_into(Method::Scrypt, b"", &mut sb), None);
        assert_eq!(
            setting_rounds_into(Method::Yescrypt, 5000, b"z.", &mut sb),
            None
        );
        assert_eq!(
            setting_rounds_into(Method::Scrypt, 5000, b"z", &mut sb),
            None
        );
    }

    /// What `stored_method` takes as a yescrypt or scrypt entry: one `crypt`
    /// would reproduce, parameters and salt included.
    #[test]
    fn stored_yescrypt_entries() {
        let hash = "$y$j9T$PKXc3hCOSyMqdaEQArI62/$6ks28kkbpf7JiBlPEqR8s9sTF8ybIkPXN7OLBaSqEg7";
        assert_eq!(stored_method(hash.as_bytes()), Some(Method::Yescrypt));
        let scrypt = "$7$66..../....SodiumChloride$SpJsFY2pIFcsdECgiLhE7VnInSJAT3kTfdlS6S6xFq9";
        assert_eq!(stored_method(scrypt.as_bytes()), Some(Method::Scrypt));
        for bad in [
            // A hash a character short, and one long.
            "$y$j9T$PKXc3hCOSyMqdaEQArI62/$6ks28kkbpf7JiBlPEqR8s9sTF8ybIkPXN7OLBaSqEg",
            "$y$j9T$PKXc3hCOSyMqdaEQArI62/$6ks28kkbpf7JiBlPEqR8s9sTF8ybIkPXN7OLBaSqEg77",
            // No salt's `$`: the hash is read as the salt.
            "$y$j9T$6ks28kkbpf7JiBlPEqR8s9sTF8ybIkPXN7OLBaSqEg7",
            // A salt that does not decode.
            "$y$j9T$zz$6ks28kkbpf7JiBlPEqR8s9sTF8ybIkPXN7OLBaSqEg7",
            // A flavour libxcrypt does not implement; N = 2; an upgrade (g).
            "$y$k.9T$PKXc3hCOSyMqdaEQArI62/$6ks28kkbpf7JiBlPEqR8s9sTF8ybIkPXN7OLBaSqEg7",
            "$y$j.T$PKXc3hCOSyMqdaEQArI62/$6ks28kkbpf7JiBlPEqR8s9sTF8ybIkPXN7OLBaSqEg7",
            "$y$j9T1.$PKXc3hCOSyMqdaEQArI62/$6ks28kkbpf7JiBlPEqR8s9sTF8ybIkPXN7OLBaSqEg7",
            // A hex hash, as this tree once wrote.
            "$y$j9T$PKXc3hCOSyMqdaEQArI62/$0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            // scrypt: r = 0, and a salt character `crypt` refuses.
            "$7$6...../....SodiumChloride$SpJsFY2pIFcsdECgiLhE7VnInSJAT3kTfdlS6S6xFq9",
            "$7$66..../....Sodium-Chloride$SpJsFY2pIFcsdECgiLhE7VnInSJAT3kTfdlS6S6xFq9",
        ] {
            assert_eq!(stored_method(bad.as_bytes()), None, "{bad}");
        }
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
        // Settings that name no method -- here or in libxcrypt -- are
        // rejected, never silently turned into a fake hash: unknown markers,
        // a method's prefix in another case or cut short, and what begins
        // with a salt character and then another character, which is no DES
        // setting.
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for setting in [
            &b"$9$x\0"[..],
            b"$Y$j9T$abc\0",
            b"$2c$05$abcdefghijklmnopqrstuu\0",
            b"$gy\0",
            b"$sha\0",
            b"$3\0",
            b"a{\0",
        ] {
            let got = crypt_raw(b"password\0".as_ptr(), setting.as_ptr());
            assert_eq!(got, ("*0".into(), crate::errno::EINVAL), "{setting:?}");
        }
    }

    /// Traditional DES's best-known answer, which every Unix gave.
    #[test]
    fn des_password_ab() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let got = crypt_raw(b"password\0".as_ptr(), b"ab\0".as_ptr());
        assert_eq!(got.0, "abJnggxhB/yWI");
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
    // crypt_rn and crypt_ra, as libxcrypt 4.4.36's lib/crypt.c has them
    // -----------------------------------------------------------------------

    /// `bytes` up to its first NUL.
    fn c_bytes(bytes: &[u8]) -> &[u8] {
        &bytes[..bytes.iter().position(|&b| b == 0).unwrap()]
    }

    /// `sizeof (struct crypt_data)`, as the C `int` `crypt_rn` takes.
    fn data_size() -> i32 {
        i32::try_from(CRYPT_DATA_SIZE).unwrap()
    }

    /// `crypt_rn` is `crypt_r` into a whole `struct crypt_data`, but NULL
    /// on failure where `crypt_r` returns the token -- which is in the
    /// buffer all the same.
    #[test]
    fn crypt_rn_is_crypt_r_with_null_for_a_failure() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = std::vec![0x55u8; CRYPT_DATA_SIZE];
        let got = crypt_rn(
            b"pw\0".as_ptr(),
            b"$6$salt$\0".as_ptr(),
            data.as_mut_ptr(),
            data_size(),
        );
        assert_eq!(got, data.as_mut_ptr());
        let mut want = buf();
        let want = hash_into(b"pw", b"$6$salt$", &mut want).unwrap();
        assert_eq!(c_bytes(&data), want.as_bytes());

        for (key, setting, token, why) in [
            (&b"pw\0"[..], &b"a{\0"[..], "*0", crate::errno::EINVAL),
            (b"pw\0", b"*0\0", "*1", crate::errno::EINVAL),
            (b"pw\0", b"$6$a:b$\0", "*0", crate::errno::EINVAL),
        ] {
            data.fill(0x55);
            crate::errno::set_errno(0);
            let got = crypt_rn(
                key.as_ptr(),
                setting.as_ptr(),
                data.as_mut_ptr(),
                data_size(),
            );
            assert!(got.is_null(), "{setting:?}");
            assert_eq!(crate::errno::get_errno(), why, "{setting:?}");
            assert_eq!(c_bytes(&data), token.as_bytes(), "{setting:?}");
        }
        for (key, setting) in [
            (core::ptr::null(), b"$6$salt$\0".as_ptr()),
            (b"pw\0".as_ptr(), core::ptr::null()),
        ] {
            data.fill(0x55);
            crate::errno::set_errno(0);
            assert!(crypt_rn(key, setting, data.as_mut_ptr(), data_size()).is_null());
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
            assert_eq!(c_bytes(&data), b"*0");
        }
    }

    /// Less than `sizeof (struct crypt_data)` is `ERANGE` -- however little
    /// the hash would need -- with as much of the token as there is room for.
    #[test]
    fn crypt_rn_wants_a_whole_crypt_data() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = std::vec![0x55u8; CRYPT_DATA_SIZE];
        for (size, written) in [
            (data_size() - 1, &b"*0\0"[..]),
            (3, b"*0\0"),
            (2, b"*\0"),
            (1, b"\0"),
            (0, b""),
            (-1, b""),
            (i32::MIN, b""),
        ] {
            data.fill(0x55);
            crate::errno::set_errno(0);
            let got = crypt_rn(
                b"pw\0".as_ptr(),
                b"$6$salt$\0".as_ptr(),
                data.as_mut_ptr(),
                size,
            );
            assert!(got.is_null(), "{size}");
            assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE, "{size}");
            assert_eq!(&data[..written.len()], written, "{size}");
            assert_eq!(data[written.len()], 0x55, "nothing more: {size}");
        }
    }

    /// A NULL buffer: `ERANGE` where libxcrypt has no room to write its
    /// token into and so writes none; `EFAULT` where it faults writing one.
    #[test]
    fn crypt_rn_into_null() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for (size, want) in [
            (0, crate::errno::ERANGE),
            (-7, crate::errno::ERANGE),
            (1, crate::errno::EFAULT),
            (data_size(), crate::errno::EFAULT),
        ] {
            crate::errno::set_errno(0);
            let got = crypt_rn(
                b"pw\0".as_ptr(),
                b"$6$salt$\0".as_ptr(),
                core::ptr::null_mut(),
                size,
            );
            assert!(got.is_null(), "{size}");
            assert_eq!(crate::errno::get_errno(), want, "{size}");
        }
    }

    /// `crypt_ra` allocates the `struct crypt_data` when given none, grows
    /// one that is smaller (a negative size counting as smaller), keeps one
    /// that is not, and leaves it the caller's to free whatever happens.
    #[test]
    fn crypt_ra_allocates_and_grows() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut want = buf();
        let want = hash_into(b"pw", b"$5$salt$", &mut want)
            .unwrap()
            .as_bytes()
            .to_vec();
        let ra = |data: &mut *mut u8, size: &mut i32| {
            crypt_ra(
                b"pw\0".as_ptr(),
                b"$5$salt$\0".as_ptr(),
                core::ptr::from_mut(data),
                core::ptr::from_mut(size),
            )
        };

        let mut data: *mut u8 = core::ptr::null_mut();
        let mut size = -99;
        let got = ra(&mut data, &mut size);
        assert!(!data.is_null());
        assert_eq!(size, data_size());
        assert_eq!(got, data);
        // SAFETY: `got` is `data`'s C string, which crypt_ra wrote.
        assert_eq!(
            unsafe { core::ffi::CStr::from_ptr(got.cast()) }.to_bytes(),
            &want[..]
        );
        // Enough already: kept, not reallocated.
        let kept = data;
        let got = ra(&mut data, &mut size);
        assert_eq!((data, size, got), (kept, data_size(), kept));
        // SAFETY: crypt_ra's allocation, freed once.
        unsafe { crate::malloc::free(data) };

        for given in [16, -1] {
            let mut data = crate::malloc::malloc(16);
            assert!(!data.is_null());
            let mut size = given;
            let got = ra(&mut data, &mut size);
            assert_eq!(size, data_size(), "{given}");
            assert_eq!(got, data, "{given}");
            // SAFETY: `got` is `data`'s C string, which crypt_ra wrote.
            assert_eq!(
                unsafe { core::ffi::CStr::from_ptr(got.cast()) }.to_bytes(),
                &want[..]
            );
            // SAFETY: crypt_ra's reallocation of our allocation, freed once.
            unsafe { crate::malloc::free(data) };
        }
    }

    /// On failure `crypt_ra` is NULL with `errno`, the token in the memory,
    /// which is still the caller's; NULL `data` or `size` is `EFAULT`, where
    /// libxcrypt faults.
    #[test]
    fn crypt_ra_failures() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut data: *mut u8 = core::ptr::null_mut();
        let mut size = 0;
        crate::errno::set_errno(0);
        let got = crypt_ra(
            b"pw\0".as_ptr(),
            b"*0\0".as_ptr(),
            &raw mut data,
            &raw mut size,
        );
        assert!(got.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        assert!(!data.is_null());
        // SAFETY: crypt_ra allocated `size` bytes at `data` and wrote the
        // token into them.
        assert_eq!(
            unsafe { core::ffi::CStr::from_ptr(data.cast()) }.to_bytes(),
            b"*1"
        );
        // SAFETY: crypt_ra's allocation, freed once.
        unsafe { crate::malloc::free(data) };

        crate::errno::set_errno(0);
        let got = crypt_ra(
            b"pw\0".as_ptr(),
            b"$5$salt$\0".as_ptr(),
            core::ptr::null_mut(),
            &raw mut size,
        );
        assert!(got.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        let mut data: *mut u8 = core::ptr::null_mut();
        crate::errno::set_errno(0);
        let got = crypt_ra(
            b"pw\0".as_ptr(),
            b"$5$salt$\0".as_ptr(),
            &raw mut data,
            core::ptr::null_mut(),
        );
        assert!(got.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        assert!(data.is_null(), "nothing allocated");
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
            (b"key\0".as_ptr(), b"$9$x\0".as_ptr(), crate::errno::EINVAL),
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

    /// A `rounds=` field outside the bounds, or malformed, is refused, as
    /// libxcrypt refuses it -- not clamped, nor taken as the salt, as glibc's
    /// old crypt did and this one did until 2026-10-07.  (The oracle,
    /// `libxcrypt_answers`, has every case; these say what the rule is.)
    #[test]
    fn rounds_fields_libxcrypt_refuses_are_refused() {
        let _g = CRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for setting in [
            &b"$6$rounds=10$salt\0"[..],
            b"$6$rounds=999$salt\0",
            b"$5$rounds=1000000000$salt\0",
            b"$6$rounds=abc$salt\0",
            b"$6$rounds=01000$salt\0",
            b"$5$rounds=+1000$salt\0",
            b"$6$rounds=1000\0",
            b"$6$rounds=\0",
            b"$5$rounds=18446744073709551616$salt\0",
        ] {
            let got = crypt_raw(b"x\0".as_ptr(), setting.as_ptr());
            assert_eq!(got, ("*0".into(), crate::errno::EINVAL), "{setting:?}");
            assert_eq!(stored_method(&setting[..setting.len() - 1]), None);
        }
        // The bounds themselves are taken, and stated as given.
        let r = crypt_str(b"x\0", b"$6$rounds=1000$salt\0").unwrap();
        assert!(r.starts_with("$6$rounds=1000$salt$"), "{r}");
        // A field that is not `rounds=` is the salt's.
        let r = crypt_str(b"x\0", b"$6$roundsx=1000$salt\0").unwrap();
        assert!(r.starts_with("$6$roundsx=1000$"), "{r}");
    }

    /// 256: the most `crypt_r` may write into a `struct crypt_data` as
    /// musl's `<crypt.h>` declares it (an `int` and 256 bytes), and enough
    /// for the longest `$y$` hash.
    #[test]
    fn output_len_constant() {
        assert_eq!(CRYPT_OUTPUT_LEN, 256);
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
        assert_eq!(hash_into(b"pw", b"p", &mut b), None);
        assert_eq!(hash_into(b"pw", b"p-lain", &mut b), None);
        assert_eq!(hash_into(b"pw", b"", &mut b), None);
    }

    /// A word of two salt characters and more is a DES setting -- `crypt`
    /// hashes by its first two -- but no stored DES entry unless it has a
    /// DES hash's shape: a cleartext password in `/etc/shadow` is still no
    /// password (`authlib`'s `check_stored` asks this before verifying).
    #[test]
    fn a_word_is_no_stored_des_hash() {
        let mut b = buf();
        assert!(hash_into(b"pw", b"plain", &mut b).is_some());
        assert_eq!(stored_method(b"plain"), None);
        assert!(!verify(b"plain", b"plain"));
        let des = hash_into(b"pw", b"pl", &mut b).unwrap().to_owned();
        assert_eq!(stored_method(des.as_bytes()), Some(Method::Des));
        assert!(verify(b"pw", des.as_bytes()));
        assert!(!verify(b"px", des.as_bytes()));
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

    /// `setting_into`'s heads are `crypt_gensalt`'s defaults, so the Rust
    /// API and the C one ask for the same cost for a new password: each
    /// writes its own, and this keeps the two from drifting apart.
    #[test]
    fn setting_heads_are_gensalts_defaults() {
        let bytes = [0x5au8; 16];
        for method in [
            Method::Md5,
            Method::Sha256,
            Method::Sha512,
            Method::Yescrypt,
            Method::GostYescrypt,
            Method::Scrypt,
            Method::Bcrypt,
        ] {
            let mut out = [0u8; crate::gensalt::CRYPT_GENSALT_OUTPUT_SIZE];
            let n = crate::gensalt::gensalt_into(
                Some(method.prefix().as_bytes()),
                0,
                Some(&bytes),
                &mut out,
            )
            .unwrap();
            assert!(
                out[..n].starts_with(method.setting_head().as_bytes()),
                "{method:?}: {:?}",
                core::str::from_utf8(&out[..n])
            );
        }
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
