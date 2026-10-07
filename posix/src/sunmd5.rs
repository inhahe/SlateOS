//! SunMD5 (`$md5`): Solaris's crypt method -- `$md5`, an optional
//! `rounds=` beyond the 4096 it always runs, a salt, and 22 characters of
//! MD5 digest.  libxcrypt counts it a legacy method; it is here to verify
//! the entries Solaris systems wrote.
//!
//! Ported from libxcrypt 4.4.36's `crypt-sunmd5.c` -- Zack Weinberg's
//! clean-room implementation of Passlib's description of the algorithm --
//! as design-decisions §539 asks (primitives are ported, not written): the
//! setting's reading, with the original's quirks libxcrypt keeps for
//! compatibility, the digest's encoding and the method's `crypt_gensalt`.
//! The rounds themselves are `pwhash::sunmd5`'s.  The file's notice, as its
//! licence asks:
//!
//! ```text
//! Copyright (c) 2018 Zack Weinberg.
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
//!
//! THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
//! ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
//! IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
//! ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
//! FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
//! DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
//! OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
//! HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
//! LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
//! OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
//! SUCH DAMAGE.
//! ```

// Settings are short and their readers index what they have measured;
// lengths are sums of a few small numbers.
#![allow(clippy::indexing_slicing)]
#![allow(clippy::arithmetic_side_effects)]

use crate::gensalt::Refused;

/// The method's prefix: `SUNMD5_PREFIX`.
const PREFIX: &[u8] = b"$md5";
/// The crypt base-64 alphabet: `itoa64`.
const ITOA64: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
/// The digest's characters, after the setting and its `$`.
const DIGEST_CHARS: usize = 22;
/// The rounds every hash runs, whatever `rounds=` adds.
const BASE_ROUNDS: u32 = 4096;
/// The most `rounds=` may ask for: `SUNMD5_MAX_ROUNDS`.
const MAX_ROUNDS: u64 = 0xFFFF_FFFF;

/// Whether `setting` names SunMD5, as libxcrypt matches it: `$md5`.  (One
/// that goes on with anything but `$` or `,` is then refused.)
pub(crate) fn names_method(setting: &[u8]) -> bool {
    setting.starts_with(PREFIX)
}

/// A setting as `crypt_sunmd5_rn` reads it: its rounds -- 4096 and what
/// `rounds=` adds, mod 2^32 as libxcrypt's `unsigned int` holds them -- and
/// the length of its head, the part the digest is salted by and the hash
/// repeats.  `None` for one it refuses.
fn parse(setting: &[u8]) -> Option<(u32, usize)> {
    // `$md5$` or, as the original allowed, `$md5,`.
    if !setting.starts_with(PREFIX) || !matches!(setting.get(PREFIX.len()), Some(b'$' | b',')) {
        return None;
    }
    let mut p = PREFIX.len() + 1;
    let mut rounds = BASE_ROUNDS;
    if setting[p..].starts_with(b"rounds=") {
        p += b"rounds=".len();
        // No zero, and no leading zero.
        if !matches!(setting.get(p), Some(b'1'..=b'9')) {
            return None;
        }
        let digits = setting[p..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        let mut extra: u64 = 0;
        for &c in &setting[p..p + digits] {
            // Past 2^32 - 1 is refused, and strtoul's overflow with it.
            extra = extra.checked_mul(10)?.checked_add(u64::from(c - b'0'))?;
            if extra > MAX_ROUNDS {
                return None;
            }
        }
        rounds = rounds.wrapping_add(u32::try_from(extra).ok()?);
        p += digits;
        if setting.get(p) != Some(&b'$') {
            return None;
        }
        p += 1;
    }
    p += setting[p..]
        .iter()
        .take_while(|c| ITOA64.contains(c))
        .count();
    match setting.get(p) {
        None | Some(b'$') => {}
        Some(_) => return None,
    }
    // As the original did: a `$` the salt ends with, followed by another
    // or by nothing, is the salt's.
    if setting.get(p) == Some(&b'$') && matches!(setting.get(p + 1), None | Some(b'$')) {
        p += 1;
    }
    Some((rounds, p))
}

/// `write_itoa64_4`: three bytes, the first lowest, as four characters.
fn write4(out: &mut [u8], b0: u8, b1: u8, b2: u8) {
    let value = u32::from(b0) | (u32::from(b1) << 8) | (u32::from(b2) << 16);
    for (i, c) in out[..4].iter_mut().enumerate() {
        *c = ITOA64[((value >> (6 * i)) & 0x3f) as usize];
    }
}

/// `crypt_sunmd5_rn`: the setting's head, `$` and the digest in 22
/// characters, into `out` -- its length, room left for a NUL -- or `None`
/// for a setting it refuses (`EINVAL`); `Some(Err(()))` when `out` has no
/// room for it (`ERANGE`).
pub(crate) fn crypt(phrase: &[u8], setting: &[u8], out: &mut [u8]) -> Option<Result<usize, ()>> {
    let (rounds, head) = parse(setting)?;
    if out.len() < head + DIGEST_CHARS + 2 {
        return Some(Err(()));
    }
    let phrase = phrase
        .iter()
        .position(|&b| b == 0)
        .map_or(phrase, |end| &phrase[..end]);
    let mut dg = pwhash::sunmd5::digest(phrase, &setting[..head], rounds);
    out[..head].copy_from_slice(&setting[..head]);
    out[head] = b'$';
    // The permuted order BSD's MD5 crypt (`$1$`) uses too.
    let at = head + 1;
    write4(&mut out[at..], dg[12], dg[6], dg[0]);
    write4(&mut out[at + 4..], dg[13], dg[7], dg[1]);
    write4(&mut out[at + 8..], dg[14], dg[8], dg[2]);
    write4(&mut out[at + 12..], dg[15], dg[9], dg[3]);
    write4(&mut out[at + 16..], dg[5], dg[10], dg[4]);
    // `write_itoa64_2`: the last byte alone, in two characters.
    out[at + 20] = ITOA64[usize::from(dg[11] & 0x3f)];
    out[at + 21] = ITOA64[usize::from(dg[11] >> 6)];
    crate::crypt::wipe(&mut dg);
    Some(Ok(at + DIGEST_CHARS))
}

/// Whether `stored` is a SunMD5 hash `crypt` would write: a setting it
/// takes, whose head is followed by `$` and the 22 characters -- the last
/// of them holding two bits -- given `room` bytes of output.
pub(crate) fn is_stored_hash(stored: &[u8], room: usize) -> bool {
    let Some((_, head)) = parse(stored) else {
        return false;
    };
    let tail = &stored[head..];
    tail.len() == 1 + DIGEST_CHARS
        && tail[0] == b'$'
        && tail[1..].iter().all(|c| ITOA64.contains(c))
        && ITOA64[..4].contains(&tail[DIGEST_CHARS])
        && stored.len() < room
}

/// `gensalt_sunmd5_rn`: `$md5,rounds=N$`, the rounds `count` asks for --
/// at least 32768, at most 2^32 - 65537 -- plus the first two random bytes
/// as a number, then eight characters of salt from the next six, and `$`.
pub(crate) fn gensalt(count: u64, rbytes: &[u8], out: &mut [u8]) -> Result<usize, Refused> {
    // `SUNMD5_MAX_SETTING_LEN`, 32, and the NUL.
    if out.len() < 32 + 1 {
        return Err(Refused::Range);
    }
    let [r0, r1, r2, r3, r4, r5, r6, r7, ..] = *rbytes else {
        return Err(Refused::Invalid);
    };
    let count = count.clamp(32768, MAX_ROUNDS - 65536) + (u64::from(r0) << 8) + u64::from(r1);
    let mut digits = [0u8; 20];
    let mut n = 0;
    let number = {
        let mut at = digits.len();
        let mut v = count;
        loop {
            at -= 1;
            digits[at] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        &digits[at..]
    };
    for part in [&b"$md5,rounds="[..], number, b"$"] {
        out[n..n + part.len()].copy_from_slice(part);
        n += part.len();
    }
    write4(&mut out[n..], r2, r3, r4);
    write4(&mut out[n + 4..], r5, r6, r7);
    out[n + 8] = b'$';
    Ok(n + 9)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_as_libxcrypt() {
        assert_eq!(parse(b"$md5$salt"), Some((4096, 9)));
        assert_eq!(parse(b"$md5,salt"), Some((4096, 9)));
        assert_eq!(
            parse(b"$md5$salt$"),
            Some((4096, 10)),
            "a lone `$` is the salt's"
        );
        assert_eq!(parse(b"$md5$salt$$hash"), Some((4096, 10)));
        assert_eq!(parse(b"$md5$salt$hash"), Some((4096, 9)));
        assert_eq!(parse(b"$md5,rounds=5$salt"), Some((4101, 18)));
        assert_eq!(
            parse(b"$md5,rounds=4294967295$s"),
            Some((4095, 24)),
            "mod 2^32"
        );
        for bad in [
            &b"$md5"[..],
            b"$md5x",
            b"$md5,rounds=0$salt",
            b"$md5,rounds=05$salt",
            b"$md5,rounds=$salt",
            b"$md5,rounds=4294967296$salt",
            b"$md5,rounds=5salt",
            b"$md5$sa-lt",
        ] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn hashes_and_shapes() {
        let mut out = [0u8; 256];
        let n = crypt(b"pw", b"$md5,rounds=10$salt$", &mut out)
            .unwrap()
            .unwrap();
        let h = out[..n].to_vec();
        assert!(h.starts_with(b"$md5,rounds=10$salt$$"), "{h:?}");
        assert!(is_stored_hash(&h, 256));
        let again = crypt(b"pw", &h, &mut out).unwrap().unwrap();
        assert_eq!(&out[..again], &h[..], "a hash is its own setting");
        assert_eq!(crypt(b"pw", b"$md5$salt", &mut out[..32]), Some(Err(())));
        assert_eq!(crypt(b"pw", b"$md5x", &mut out), None);
    }

    #[test]
    fn gensalts() {
        let mut out = [0u8; 64];
        let n = gensalt(0, &[1, 2, 3, 4, 5, 6, 7, 8], &mut out).unwrap();
        // 32768 + 0x0102.
        assert!(out[..n].starts_with(b"$md5,rounds=33026$"));
        assert_eq!(n, b"$md5,rounds=33026$".len() + 9);
        assert_eq!(gensalt(0, &[1; 7], &mut out), Err(Refused::Invalid));
        assert_eq!(gensalt(0, &[1; 8], &mut out[..32]), Err(Refused::Range));
        // 2^32 - 1 - 65536, and 0xffff.
        let n = gensalt(u64::MAX, &[0xff; 8], &mut out).unwrap();
        assert!(out[..n].starts_with(b"$md5,rounds=4294967294$"));
    }
}
