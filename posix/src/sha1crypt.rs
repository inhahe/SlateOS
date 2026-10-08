//! sha1crypt (`$sha1`): NetBSD's crypt method, PBKDF1 over HMAC-SHA1 --
//! `$sha1$`, a count of iterations, a salt, and 28 characters of hash.
//! libxcrypt counts it a legacy method; it is here to verify the entries
//! NetBSD systems wrote.
//!
//! Ported from libxcrypt 4.4.36's `crypt-pbkdf1-sha1.c`, as
//! design-decisions §539 asks (primitives are ported, not written): the
//! setting's reading, the first HMAC's text, the hash's encoding and the
//! method's `crypt_gensalt`.  The HMACs themselves are `pwhash::sha1`'s.
//! The file's notice, as its licence asks:
//!
//! ```text
//! Copyright (c) 2004, Juniper Networks, Inc.
//! All rights reserved.
//!
//! Redistribution and use in source and binary forms, with or without
//! modification, are permitted provided that the following conditions
//! are met:
//! 1. Redistributions of source code must retain the above copyright
//!    notice, this list of conditions and the following disclaimer.
//! 2. Redistributions in binary form must reproduce the above copyright
//!    notice, this list of conditions and the following disclaimer in the
//!    documentation and/or other materials provided with the distribution.
//! 3. Neither the name of the copyright holders nor the names of its
//!    contributors may be used to endorse or promote products derived
//!    from this software without specific prior written permission.
//!
//! THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
//! "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
//! LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
//! A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
//! OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//! SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
//! LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
//! DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
//! THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
//! (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
//! OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
//! ```

// Settings are short and their readers index what they have measured;
// lengths are sums of a few small numbers.
#![allow(clippy::indexing_slicing)]
#![allow(clippy::arithmetic_side_effects)]

use crate::gensalt::Refused;

/// The setting's magic, and what libxcrypt matches the method by: `$sha1`.
const MAGIC: &[u8] = b"$sha1$";
/// The crypt base-64 alphabet: `itoa64`.
const ITOA64: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
/// The hash: 20 bytes in 28 characters.
const HASH_CHARS: usize = 28;
/// gensalt's iterations when asked for none: `CRYPT_SHA1_ITERATIONS`.
const DEFAULT_ITERATIONS: u64 = 262_144;
/// The longest salt gensalt writes: `CRYPT_SHA1_SALT_LENGTH`.
const SALT_MAX: usize = 64;

/// Whether `setting` names sha1crypt, as libxcrypt matches it: `$sha1`.
/// (One that goes on with anything but `$` is then refused.)
pub(crate) fn names_method(setting: &[u8]) -> bool {
    setting.starts_with(b"$sha1")
}

/// `strtoul(s, &end, 10)` for a setting: an optional sign, decimal digits,
/// the value mod 2^64 negated for a `-` and saturated at 2^64 - 1 on
/// overflow -- and where it stopped, which is `s` itself when no digit
/// follows the sign.  (No leading space can reach it: `crypt` refuses
/// settings that hold one.)
fn strtoul(s: &[u8]) -> (u64, usize) {
    let (negative, sign) = match s.first() {
        Some(b'-') => (true, 1),
        Some(b'+') => (false, 1),
        _ => (false, 0),
    };
    let digits = s[sign..].iter().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return (0, 0);
    }
    let mut value: u64 = 0;
    let mut overflow = false;
    for &c in &s[sign..sign + digits] {
        match value
            .checked_mul(10)
            .and_then(|v| v.checked_add(u64::from(c - b'0')))
        {
            Some(v) => value = v,
            None => overflow = true,
        }
    }
    let value = if overflow {
        u64::MAX
    } else if negative {
        value.wrapping_neg()
    } else {
        value
    };
    (value, sign + digits)
}

/// `value` in decimal, into `buf`: the digits.
fn decimal(mut value: u64, buf: &mut [u8; 20]) -> &[u8] {
    let mut at = buf.len();
    loop {
        at -= 1;
        buf[at] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    &buf[at..]
}

/// `to64`: `n` characters of `v`, its lowest six bits first.
fn to64(out: &mut [u8], mut v: u32, n: usize) {
    for c in &mut out[..n] {
        *c = ITOA64[(v & 0x3f) as usize];
        v >>= 6;
    }
}

/// A setting as `crypt_sha1crypt_rn` reads it: the count of iterations and
/// the salt.  `None` for one it refuses.
fn parse(setting: &[u8]) -> Option<(u64, &[u8])> {
    let rest = setting.strip_prefix(MAGIC)?;
    let (iterations, used) = strtoul(rest);
    let rest = rest[used..].strip_prefix(b"$")?;
    let salt_len = rest.iter().take_while(|c| ITOA64.contains(c)).count();
    if salt_len == 0 || rest.get(salt_len).is_some_and(|&c| c != b'$') {
        return None;
    }
    Some((iterations, &rest[..salt_len]))
}

/// The output's room was too small, or the setting is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    Range,
    Invalid,
}

/// `crypt_sha1crypt_rn`: `$sha1$`, the count as it reads, `$`, the salt,
/// `$` and the hash, into `out` -- its length, room left for a NUL.  The
/// first HMAC is of the salt, `$sha1$` and the count; each later one of the
/// last digest; the password is the key.
pub(crate) fn crypt(phrase: &[u8], setting: &[u8], out: &mut [u8]) -> Result<usize, Refusal> {
    let (iterations, salt) = parse(setting).ok_or(Refusal::Invalid)?;
    let mut digits = [0u8; 20];
    let count = decimal(iterations, &mut digits);
    // "$sha1$<count>$<salt>$" and the hash, in the room with its NUL.
    let head = MAGIC.len() + count.len() + 1 + salt.len() + 1;
    if head + HASH_CHARS >= out.len() {
        return Err(Refusal::Range);
    }
    // The first text, `<salt>$sha1$<count>`, built where the output goes:
    // it fits there, being no longer.
    let first_len = salt.len() + MAGIC.len() + count.len();
    out[..salt.len()].copy_from_slice(salt);
    out[salt.len()..salt.len() + MAGIC.len()].copy_from_slice(MAGIC);
    out[salt.len() + MAGIC.len()..first_len].copy_from_slice(count);
    let phrase = phrase
        .iter()
        .position(|&b| b == 0)
        .map_or(phrase, |end| &phrase[..end]);
    let mut digest = pwhash::sha1::sha1crypt(phrase, &out[..first_len], iterations);

    let mut at = 0;
    for part in [MAGIC, count, b"$", salt, b"$"] {
        out[at..at + part.len()].copy_from_slice(part);
        at += part.len();
    }
    // Three bytes to four characters, the first byte the top; the last two
    // bytes with the first again.
    for &[a, b, c] in digest[..18].as_chunks::<3>().0 {
        let v = (u32::from(a) << 16) | (u32::from(b) << 8) | u32::from(c);
        to64(&mut out[at..], v, 4);
        at += 4;
    }
    let v = (u32::from(digest[18]) << 16) | (u32::from(digest[19]) << 8) | u32::from(digest[0]);
    to64(&mut out[at..], v, 4);
    at += 4;
    crate::crypt::wipe(&mut digest);
    Ok(at)
}

/// Whether `stored` is a sha1crypt hash `crypt` would write: a setting it
/// takes, with its count as `crypt` writes it back -- no sign, no leading
/// zero -- then `$` and 28 characters of base-64, given `room` bytes of
/// output.
pub(crate) fn is_stored_hash(stored: &[u8], room: usize) -> bool {
    let Some((iterations, salt)) = parse(stored) else {
        return false;
    };
    let mut digits = [0u8; 20];
    let count = decimal(iterations, &mut digits);
    let head_len = MAGIC.len() + count.len() + 1 + salt.len() + 1;
    let canonical = stored.get(MAGIC.len()..MAGIC.len() + count.len()) == Some(count)
        && stored.get(MAGIC.len() + count.len()) == Some(&b'$');
    canonical
        && stored.len() == head_len + HASH_CHARS
        && stored[head_len..].iter().all(|c| ITOA64.contains(c))
        && stored.len() < room
}

/// `gensalt_sha1crypt_rn`: `$sha1$`, the count -- `count`, 262144 for 0,
/// within 4 and 2^32 - 1, less up to a quarter of itself by the first four
/// random bytes -- `$`, the salt from the rest, four characters for each
/// three bytes while more than three remain, at most 64, and `$`.
pub(crate) fn gensalt(count: u64, rbytes: &[u8], out: &mut [u8]) -> Result<usize, Refused> {
    if rbytes.len() < 12 + 4 {
        return Err(Refused::Invalid);
    }
    // "$sha1$<10 digits>$<salt>$" and a NUL: sizeof "$sha1$$$" is 9.
    if out.len() < (rbytes.len() - 4) * 4 / 3 + 9 + 10 {
        return Err(Refused::Range);
    }
    let random = u32::from_le_bytes([rbytes[0], rbytes[1], rbytes[2], rbytes[3]]);
    let count = if count == 0 {
        DEFAULT_ITERATIONS
    } else {
        count
    }
    .clamp(4, u64::from(u32::MAX));
    let rounds = count - u64::from(random) % (count / 4);
    let mut digits = [0u8; 20];
    let mut n = 0;
    for part in [MAGIC, decimal(rounds, &mut digits), b"$"] {
        out[n..n + part.len()].copy_from_slice(part);
        n += part.len();
    }
    // At most 64 characters of salt, and two bytes of room kept for `$`
    // and the NUL.
    let limit = (n + SALT_MAX).min(out.len() - 2);
    let mut o = n;
    let mut r = 4;
    while r + 3 < rbytes.len() && o + 4 < limit {
        let v = (u32::from(rbytes[r]) << 16)
            | (u32::from(rbytes[r + 1]) << 8)
            | u32::from(rbytes[r + 2]);
        to64(&mut out[o..], v, 4);
        r += 3;
        o += 4;
    }
    out[o] = b'$';
    Ok(o + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strtoul_as_libc() {
        assert_eq!(strtoul(b"123$"), (123, 3));
        assert_eq!(strtoul(b"+5$"), (5, 2));
        assert_eq!(strtoul(b"-1$"), (u64::MAX, 2));
        assert_eq!(strtoul(b"007"), (7, 3));
        assert_eq!(strtoul(b"$"), (0, 0));
        assert_eq!(strtoul(b"+$"), (0, 0));
        assert_eq!(strtoul(b"99999999999999999999999$"), (u64::MAX, 23));
    }

    #[test]
    fn parses() {
        assert_eq!(parse(b"$sha1$5$salt"), Some((5, &b"salt"[..])));
        assert_eq!(parse(b"$sha1$5$salt$hash"), Some((5, &b"salt"[..])));
        assert_eq!(parse(b"$sha1$$salt"), Some((0, &b"salt"[..])));
        for bad in [
            &b"$sha1"[..],
            b"$sha1$5",
            b"$sha1$5$",
            b"$sha1$5$$",
            b"$sha1$5$a-b",
            b"$sha1x5$salt",
            b"$sha1$+$salt",
        ] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn hashes_and_shapes() {
        let mut out = [0u8; 256];
        let n = crypt(b"pw", b"$sha1$+0007$salt$old", &mut out).unwrap();
        let h = out[..n].to_vec();
        assert!(h.starts_with(b"$sha1$7$salt$"), "{h:?}");
        assert_eq!(h.len(), b"$sha1$7$salt$".len() + 28);
        assert!(is_stored_hash(&h, 256));
        let again = crypt(b"pw", &h, &mut out).unwrap();
        assert_eq!(&out[..again], &h[..], "a hash is its own setting");
        let mut sign = b"$sha1$+7$salt$".to_vec();
        sign.extend_from_slice(&h[13..]);
        assert!(!is_stored_hash(&sign, 256));
        assert_eq!(
            crypt(b"pw", b"$sha1$7$salt", &mut out[..40]),
            Err(Refusal::Range)
        );
    }

    #[test]
    fn gensalts() {
        let bytes: std::vec::Vec<u8> = (1..=20).collect();
        let mut out = [0u8; 192];
        let n = gensalt(0, &bytes, &mut out).unwrap();
        let s = core::str::from_utf8(&out[..n]).unwrap();
        // 262144 less 0x04030201 % 65536 = 0x0201.
        assert!(s.starts_with("$sha1$261631$"), "{s}");
        assert!(s.ends_with('$'));
        assert_eq!(gensalt(0, &bytes[..15], &mut out), Err(Refused::Invalid));
        assert_eq!(gensalt(0, &bytes, &mut out[..39]), Err(Refused::Range));
        assert!(gensalt(0, &bytes, &mut out[..40]).is_ok());
        assert!(gensalt(1, &bytes, &mut out).unwrap() > 0);
    }
}
