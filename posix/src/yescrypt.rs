//! yescrypt and classic scrypt: the `$y$` and `$7$` methods of `crypt`.
//!
//! Ubuntu, Debian and Fedora hash new passwords with yescrypt -- `$y$j9T$`
//! is N = 4096, r = 32, 16 MiB of memory a hash -- so an `/etc/shadow`
//! brought from any of them names it, and until 2026-10-06 such an entry
//! could not be verified here.
//!
//! This module is the settings and the C-facing checks; the KDF itself is
//! `pwhash::yescrypt`, a crate of its own so that it compiles for speed
//! while the libc compiles for size (`pwhash`'s crate docs).  Both are a port
//! of libxcrypt 4.4.36's, as design-decisions §539 asks (primitives are
//! ported, not written).  Here:
//!
//! - `alg-yescrypt-common.c`: the `$y$` and `$7$` settings, their base-64,
//!   and `yescrypt_r`;
//! - from `crypt-yescrypt.c` and `crypt-scrypt.c`, the checks `crypt` makes
//!   before hashing: the output's room, and a `$7$` salt's characters;
//! - the memory, which `pwhash` cannot allocate: libxcrypt's
//!   `yescrypt_local_t`, from this libc's `calloc`.  A hash that cannot get
//!   its memory fails.
//!
//! The files' notices, as their licences ask:
//!
//! ```text
//! alg-yescrypt-common.c:
//!   Copyright 2013-2018 Alexander Peslyak
//!   All rights reserved.
//! crypt-scrypt.c:
//!   Copyright (C) 2013 Alexander Peslyak
//!   Copyright (C) 2018 Björn Esser <besser82@fedoraproject.org>
//! crypt-yescrypt.c:
//!   Copyright (C) 2018 vt@altlinux.org
//!
//! Redistribution and use in source and binary forms, with or without
//! modification, are permitted.
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

// Settings are short and their parsers index what they have measured;
// lengths are sums of a few small numbers.
#![allow(clippy::indexing_slicing)]
#![allow(clippy::arithmetic_side_effects)]

use crate::decfloat::MallocBuf;
use pwhash::yescrypt::{Params, YESCRYPT_RW, YESCRYPT_RW_FLAVOR_MASK};

/// The hash, 32 bytes, as base-64 characters.
const HASH_LEN: usize = 43;
/// What follows a setting in `crypt`'s output: `$`, the hash and a NUL.
const AFTER_SETTING: usize = 1 + HASH_LEN + 1;

/// Zero `bytes` where the compiler may not drop the stores: `explicit_bzero`.
fn wipe(bytes: &mut [u8]) {
    // SAFETY: the slice's own bytes, all writable.
    unsafe { crate::string::explicit_bzero(bytes.as_mut_ptr(), bytes.len()) };
}

/// `yescrypt_kdf(NULL, local, passwd, salt, params, out)` in memory of its
/// own -- libxcrypt's `yescrypt_local_t`, from `calloc` -- since
/// `pwhash::yescrypt::kdf` allocates nothing.  `None` if the parameters are
/// refused or the memory cannot be had.
fn kdf(passwd: &[u8], salt: &[u8], params: &Params, out: &mut [u8; 32]) -> Option<()> {
    let memory = params.memory()?;
    // V first: it is the large one, so a hash that cannot have its memory
    // fails before the rest is taken.
    let mut words = MallocBuf::<u64>::zeroed(memory.words)?;
    let mut bytes = MallocBuf::<u8>::zeroed(memory.bytes)?;
    pwhash::yescrypt::kdf(passwd, salt, params, bytes.as_mut(), words.as_mut(), out)
}

// ---------------------------------------------------------------------------
// Settings (alg-yescrypt-common.c)
// ---------------------------------------------------------------------------

/// The crypt base-64 alphabet (libxcrypt's `itoa64`).
const ITOA64: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// `atoi64`: a character's value, or 64 for one outside the alphabet.
fn atoi64(c: u8) -> u32 {
    u32::from(match c {
        b'.'..=b'9' => c - b'.',
        b'A'..=b'Z' => c - b'A' + 12,
        b'a'..=b'z' => c - b'a' + 38,
        _ => return 64,
    })
}

/// `decode64_uint32(dst, src, min)`: a number in a variable-length code,
/// whose first character says how many follow.  The number, and what is
/// left of `src`.
fn decode64_uint32(src: &[u8], min: u32) -> Option<(u32, &[u8])> {
    let (&first, mut rest) = src.split_first()?;
    let c = atoi64(first);
    if c > 63 {
        return None;
    }
    let (mut start, mut end, mut chars, mut bits) = (0u32, 47u32, 1u32, 0u32);
    let mut dst = min;
    while c > end {
        dst = dst.wrapping_add((end + 1 - start) << bits);
        start = end + 1;
        end = start + (62 - end) / 2;
        chars += 1;
        bits += 6;
    }
    dst = dst.wrapping_add((c - start) << bits);
    while chars > 1 {
        chars -= 1;
        let (&next, after) = rest.split_first()?;
        let c = atoi64(next);
        if c > 63 {
            return None;
        }
        rest = after;
        bits -= 6;
        dst = dst.wrapping_add(c << bits);
    }
    Some((dst, rest))
}

/// `decode64_uint32_fixed(dst, dstbits, src)`: `dstbits` bits, six a
/// character, least significant first.
fn decode64_uint32_fixed(src: &[u8], dstbits: u32) -> Option<(u32, &[u8])> {
    let mut dst = 0u32;
    let mut rest = src;
    let mut bits = 0;
    while bits < dstbits {
        let (&c, after) = rest.split_first()?;
        let c = atoi64(c);
        if c > 63 {
            return None;
        }
        dst |= c << bits;
        rest = after;
        bits += 6;
    }
    Some((dst, rest))
}

/// `decode64(dst, &dstlen, src, srclen)`, where `yescrypt_r` requires all
/// of `src` to decode: the bytes it decodes to, into at most `dst.len()`.
/// Groups of four characters are three bytes, least significant first; a
/// short last group must hold whole bytes, its spare bits zero.
fn decode64(dst: &mut [u8], src: &[u8]) -> Option<usize> {
    let mut dstpos = 0;
    for group in src.chunks(4) {
        let mut value = 0u32;
        for (k, &c) in group.iter().enumerate() {
            let c = atoi64(c);
            // libxcrypt stops at such a character, and `yescrypt_r` then
            // fails for its not having reached the end.
            if c > 63 {
                return None;
            }
            value |= c << (6 * k);
        }
        let mut bits = 6 * group.len() as u32;
        if bits < 12 {
            return None; // not one whole byte
        }
        while bits >= 8 {
            *dst.get_mut(dstpos)? = value as u8;
            dstpos += 1;
            value >>= 8;
            bits -= 8;
        }
        if value != 0 {
            return None; // the spare bits must be zero
        }
    }
    Some(dstpos)
}

/// `encode64(dst, dstlen, src, srclen)`: `src` in base-64, three bytes to
/// four characters, least significant first -- a short last group to as
/// few characters as hold its bits.  The characters written.
fn encode64(out: &mut [u8], src: &[u8]) -> Option<usize> {
    let mut n = 0;
    for group in src.chunks(3) {
        let mut value = 0u32;
        for (k, &byte) in group.iter().enumerate() {
            value |= u32::from(byte) << (8 * k);
        }
        let mut bits = 0;
        while bits < 8 * group.len() {
            *out.get_mut(n)? = ITOA64[(value & 0x3f) as usize];
            n += 1;
            value >>= 6;
            bits += 6;
        }
    }
    Some(n)
}

/// `crypt-scrypt.c`'s `verify_salt`: past `$7$` and the parameters, salt
/// characters -- base-64 and `$` -- up to one that follows a `$`.
fn scrypt_salt_ok(setting: &[u8]) -> bool {
    let salt_char = |c: u8| atoi64(c) < 64 || c == b'$';
    let mut prev = 0;
    for &c in setting.iter().skip(3 + 1 + 5 * 2) {
        if !salt_char(c) {
            return prev == b'$';
        }
        prev = c;
    }
    true
}

/// A `$y$` or `$7$` setting, read as `yescrypt_r` reads it.
struct Setting<'s> {
    params: Params,
    /// `$7$`: classic scrypt, whose salt is its characters, not their
    /// decoding.
    scrypt: bool,
    /// The length of the `$`, the method and the parameters (`prefixlen`).
    prefix_len: usize,
    /// The salt's characters: up to the last `$`, or the end.
    salt: &'s [u8],
}

/// Read `setting`'s method, parameters and salt.  `None` if it is not one
/// `yescrypt_r` reads; its parameters are left for [`kdf`] to judge.
fn parse(setting: &[u8]) -> Option<Setting<'_>> {
    let mut params = Params {
        flags: 0,
        n: 0,
        r: 0,
        p: 1,
        t: 0,
        g: 0,
        nrom: 0,
    };
    let scrypt = match setting.get(..3)? {
        b"$7$" => true,
        b"$y$" => false,
        _ => return None,
    };
    let mut src = &setting[3..];
    if scrypt {
        let (&c, rest) = src.split_first()?;
        let n_log2 = atoi64(c);
        if !(1..=63).contains(&n_log2) {
            return None;
        }
        params.n = 1 << n_log2;
        let (r, rest) = decode64_uint32_fixed(rest, 30)?;
        let (p, rest) = decode64_uint32_fixed(rest, 30)?;
        params.r = r;
        params.p = p;
        src = rest;
    } else {
        let (flavor, rest) = decode64_uint32(src, 0)?;
        params.flags = if flavor < YESCRYPT_RW {
            flavor
        } else if flavor <= YESCRYPT_RW + (YESCRYPT_RW_FLAVOR_MASK >> 2) {
            YESCRYPT_RW + ((flavor - YESCRYPT_RW) << 2)
        } else {
            return None;
        };
        let (n_log2, rest) = decode64_uint32(rest, 1)?;
        if n_log2 > 63 {
            return None;
        }
        params.n = 1 << n_log2;
        let (r, mut rest) = decode64_uint32(rest, 1)?;
        params.r = r;
        if rest.first() != Some(&b'$') {
            let (have, after) = decode64_uint32(rest, 1)?;
            rest = after;
            if have & 1 != 0 {
                (params.p, rest) = decode64_uint32(rest, 2)?;
            }
            if have & 2 != 0 {
                (params.t, rest) = decode64_uint32(rest, 1)?;
            }
            if have & 4 != 0 {
                (params.g, rest) = decode64_uint32(rest, 1)?;
            }
            if have & 8 != 0 {
                let (nrom_log2, after) = decode64_uint32(rest, 1)?;
                if nrom_log2 > 63 {
                    return None;
                }
                params.nrom = 1 << nrom_log2;
                rest = after;
            }
        }
        src = rest.strip_prefix(b"$")?;
    }
    let salt_len = src.iter().rposition(|&c| c == b'$').unwrap_or(src.len());
    Some(Setting {
        params,
        scrypt,
        prefix_len: setting.len() - src.len(),
        salt: &src[..salt_len],
    })
}

/// Why `crypt` refused a `$y$` or `$7$` setting: it makes the first
/// `ERANGE` and the second `EINVAL`, as libxcrypt's does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// The output has no room for all of the setting, `$`, the hash and a
    /// NUL.
    Range,
    /// The setting is not one this method takes, or the hash failed.
    Invalid,
}

/// Whether `setting` names yescrypt (`$y$`) or scrypt (`$7$`), the methods
/// [`crypt`] computes.
pub(crate) fn names_method(setting: &[u8]) -> bool {
    setting.starts_with(b"$y$") || setting.starts_with(b"$7$")
}

/// `crypt` for a `$y$` or `$7$` setting -- `crypt_yescrypt_rn` and
/// `crypt_scrypt_rn` -- into `out`, whose length is the output's room
/// (libxcrypt's `CRYPT_OUTPUT_SIZE`): the setting through its salt, `$`,
/// and the hash in 43 characters.  The bytes written, without a NUL, which
/// there is room for.
pub(crate) fn crypt(passwd: &[u8], setting: &[u8], out: &mut [u8]) -> Result<usize, Refused> {
    // Room for all of the setting -- though only its head is repeated --
    // `$`, the hash and a NUL.
    if out.len() < setting.len() + AFTER_SETTING {
        return Err(Refused::Range);
    }
    if setting.starts_with(b"$7$") && !scrypt_salt_ok(setting) {
        return Err(Refused::Invalid);
    }
    hash(passwd, setting, out).ok_or(Refused::Invalid)
}

/// `yescrypt_r(NULL, local, passwd, setting, NULL, out)`.
fn hash(passwd: &[u8], setting: &[u8], out: &mut [u8]) -> Option<usize> {
    let parsed = parse(setting)?;
    let mut saltbin = [0u8; 64];
    let salt_len = if parsed.scrypt {
        0
    } else {
        decode64(&mut saltbin, parsed.salt)?
    };
    let salt = if parsed.scrypt {
        parsed.salt
    } else {
        &saltbin[..salt_len]
    };
    let head = parsed.prefix_len + parsed.salt.len();
    let mut hashbin = [0u8; 32];
    let hashed = if head + AFTER_SETTING > out.len() {
        None
    } else {
        kdf(passwd, salt, &parsed.params, &mut hashbin)
    };
    wipe(&mut saltbin);
    let written = hashed.and_then(|()| {
        out[..head].copy_from_slice(&setting[..head]);
        out[head] = b'$';
        Some(head + 1 + encode64(&mut out[head + 1..], &hashbin)?)
    });
    wipe(&mut hashbin);
    written
}

/// Whether `stored` is a `$y$` or `$7$` hash `crypt` would reproduce, given
/// `room` bytes of output: a setting it takes -- parameters the KDF accepts
/// and a salt that decodes -- then `$` and 43 characters of base-64.
///
/// What it cannot know is whether the memory will be there: a hash asking
/// for more than the machine has passes here and fails when computed.
pub(crate) fn is_stored_hash(stored: &[u8], room: usize) -> bool {
    let Some(parsed) = parse(stored) else {
        return false;
    };
    // The salt runs to the last `$`, so what follows it is the hash.
    let at = parsed.prefix_len + parsed.salt.len();
    let shape = stored.get(at) == Some(&b'$')
        && stored
            .get(at + 1..)
            .is_some_and(|h| h.len() == HASH_LEN && h.iter().all(|&c| atoi64(c) < 64));
    let salt = if parsed.scrypt {
        scrypt_salt_ok(stored)
    } else {
        decode64(&mut [0u8; 64], parsed.salt).is_some()
    };
    shape && salt && parsed.params.accepted() && stored.len() + AFTER_SETTING <= room
}

/// Whether `salt` is one a `$y$` setting can carry: base-64 that decodes,
/// whole, to between 1 and 64 bytes.
pub(crate) fn is_yescrypt_salt(salt: &[u8]) -> bool {
    !salt.is_empty() && decode64(&mut [0u8; 64], salt).is_some()
}
// The KDF has tests of its own (`pwhash`); these check the settings.
#[cfg(test)]
mod tests {
    use super::*;

    /// `encode64` and `decode64` undo each other at every length a salt
    /// can decode to, and `decode64` refuses what libxcrypt's does: a lone
    /// character, spare bits set, a character outside the alphabet, more
    /// than the room.
    #[test]
    fn base64_round_trips() {
        for len in 0..=64usize {
            let data: std::vec::Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let mut chars = [0u8; 100];
            let n = encode64(&mut chars, &data).unwrap();
            assert_eq!(n, (len * 8).div_ceil(6));
            let mut back = [0u8; 64];
            assert_eq!(decode64(&mut back, &chars[..n]), Some(len));
            assert_eq!(back[..len], data[..]);
        }
        let mut back = [0u8; 64];
        for bad in [&b"."[..], b"zz", b"z.z", b"....z", b"..-.", &[b'.'; 87]] {
            assert_eq!(decode64(&mut back, bad), None, "{bad:?}");
        }
        assert_eq!(decode64(&mut back, b""), Some(0));
        // Four characters need room for four.
        assert_eq!(encode64(&mut [0u8; 3], &[1, 2, 3]), None);
    }

    /// libxcrypt's `encode64_uint32`, which only a test needs here.
    fn encode_u32(src: u32, min: u32) -> Option<std::vec::Vec<u8>> {
        let mut src = src.checked_sub(min)?;
        let (mut start, mut end, mut chars, mut bits) = (0u32, 47u32, 1u32, 0u32);
        loop {
            let count = (end + 1 - start) << bits;
            if src < count {
                break;
            }
            if start >= 63 {
                return None;
            }
            start = end + 1;
            end = start + (62 - end) / 2;
            src -= count;
            chars += 1;
            bits += 6;
        }
        let mut out = std::vec![ITOA64[(start + (src >> bits)) as usize]];
        while chars > 1 {
            chars -= 1;
            bits -= 6;
            out.push(ITOA64[((src >> bits) & 0x3f) as usize]);
        }
        Some(out)
    }

    /// `decode64_uint32` reads what `encode64_uint32` writes, at every
    /// length of its code, one character to six, and leaves what follows.
    #[test]
    fn variable_length_numbers_round_trip() {
        let large = [
            1 << 20,
            (1 << 24) + 5,
            (1 << 30) - 1,
            1 << 30,
            (1 << 31) + 12345,
        ];
        for min in [0, 1, 2] {
            let mut lengths = [false; 7];
            for v in (min..70_000).chain(large) {
                let Some(mut code) = encode_u32(v, min) else {
                    continue;
                };
                lengths[code.len()] = true;
                code.push(b'$');
                assert_eq!(decode64_uint32(&code, min), Some((v, &b"$"[..])), "{v}");
            }
            assert_eq!(
                lengths,
                [false, true, true, true, true, true, true],
                "min {min}"
            );
        }
        // A code cut short, and a character outside the alphabet.
        assert_eq!(decode64_uint32(b"z", 0), None);
        assert_eq!(decode64_uint32(b"-", 0), None);
        assert_eq!(decode64_uint32(b"", 0), None);
    }

    /// A setting read as `yescrypt_r` reads it: parameters, prefix, salt.
    #[test]
    fn settings_parse() {
        let s = parse(b"$y$j9T$PKXc3hCOSyMqdaEQArI62/$6ks28kkbpf7JiBlPEqR8s9sTF8ybIkPXN7OLBaSqEg7")
            .unwrap();
        assert!(!s.scrypt);
        let p = s.params;
        assert_eq!(
            (p.flags, p.n, p.r, p.p, p.t, p.g, p.nrom),
            (pwhash::yescrypt::YESCRYPT_DEFAULTS, 4096, 32, 1, 0, 0, 0)
        );
        assert_eq!((s.prefix_len, s.salt), (7, &b"PKXc3hCOSyMqdaEQArI62/"[..]));
        let s = parse(b"$7$CU..../....SodiumChloride").unwrap();
        assert!(s.scrypt);
        assert_eq!((s.params.n, s.params.r, s.params.p), (16384, 32, 1));
        assert_eq!((s.prefix_len, s.salt), (14, &b"SodiumChloride"[..]));
        // p, t, g and NROM, each behind its bit of `have` (15, 'C').
        let s = parse(b"$y$j75C0/.0$salt").unwrap();
        assert_eq!(
            (s.params.p, s.params.t, s.params.g, s.params.nrom),
            (4, 2, 1, 8)
        );
        assert!(parse(b"$5$salt").is_none());
    }

    /// The stored-hash check takes the output's room as its limit: the whole
    /// entry, `$`, 43 characters and a NUL.
    #[test]
    fn a_stored_hash_needs_the_room_to_verify() {
        let hash = b"$y$j75$.......$/FQush7wojITE6a6KwAF4pUyKZAyYRCYJdWcgqjTeS7";
        assert!(is_stored_hash(hash, hash.len() + 45));
        assert!(!is_stored_hash(hash, hash.len() + 44));
    }
}
