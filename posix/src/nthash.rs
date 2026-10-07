//! The NT method (`$3$`): FreeBSD's crypt name for Windows' NT hash, MD4 of
//! the password in UTF-16LE -- each byte taken as the character of that
//! number, as libxcrypt takes it -- written `$3$$` and 32 hex digits.  It
//! has no salt and no cost: it is here to verify entries that hold it (Samba
//! and FreeBSD wrote them), and nothing should hash a new password with it.
//!
//! Ported from libxcrypt 4.4.36's `crypt-nthash.c`, as design-decisions
//! §539 asks (primitives are ported, not written); MD4 is `pwhash::md4`.
//! Its notice, as its licence asks:
//!
//! ```text
//! Copyright (c) 1998-1999 Whistle Communications, Inc.
//! Copyright (c) 1998-1999 Archie Cobbs <archie@freebsd.org>
//! Copyright (c) 2003 Michael Bretterklieber
//! Copyright (c) 2017-2019 Björn Esser <besser82@fedoraproject.org>
//! Copyright (c) 2017-2019 Zack Weinberg <zackw at panix.com>
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

// A hash's 37 bytes, indexed by their fixed places.
#![allow(clippy::indexing_slicing)]
#![allow(clippy::arithmetic_side_effects)]

use crate::gensalt::Refused;

/// The method's prefix -- and its whole setting.
const MAGIC: &[u8] = b"$3$";
/// A hash: `$3$$` and the digest in 32 lowercase hex digits.
const HASH_LEN: usize = 4 + 32;

/// Whether `setting` names the NT method.
pub(crate) fn names_method(setting: &[u8]) -> bool {
    setting.starts_with(MAGIC)
}

/// `crypt_nt_rn`: the hash of `phrase` -- any setting that begins `$3$`
/// gives the same, the rest not being read -- into `out`: its length, room
/// left for a NUL.  `phrase` is read as a C string, to its first NUL.
pub(crate) fn crypt(phrase: &[u8], setting: &[u8], out: &mut [u8]) -> Option<usize> {
    if !names_method(setting) || out.len() <= HASH_LEN {
        return None;
    }
    let phrase = phrase
        .iter()
        .position(|&b| b == 0)
        .map_or(phrase, |end| &phrase[..end]);
    // UTF-16LE, each byte the character of its number: libxcrypt's
    // `unipw`, which a passphrase of the libc's 511 bytes at most fills to
    // 1022.
    let mut wide = [0u8; 2 * 512];
    let wide = wide.get_mut(..2 * phrase.len())?;
    for (pair, &b) in wide.as_chunks_mut::<2>().0.iter_mut().zip(phrase) {
        *pair = [b, 0];
    }
    let digest = pwhash::md4::digest(&[wide]);
    crate::crypt::wipe(wide);
    out[..4].copy_from_slice(b"$3$$");
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (i, b) in digest.iter().enumerate() {
        out[4 + 2 * i] = HEX[usize::from(b >> 4)];
        out[5 + 2 * i] = HEX[usize::from(b & 0x0f)];
    }
    Some(HASH_LEN)
}

/// Whether `stored` is an NT hash `crypt` would write: `$3$$` and 32
/// lowercase hex digits.
pub(crate) fn is_stored_hash(stored: &[u8]) -> bool {
    stored.len() == HASH_LEN
        && stored.starts_with(b"$3$$")
        && stored[4..]
            .iter()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
}

/// `gensalt_nt_rn`: `$3$`, the whole of an NT setting; no cost to ask for,
/// and no random bytes read.
pub(crate) fn gensalt(count: u64, _rbytes: &[u8], out: &mut [u8]) -> Result<usize, Refused> {
    if out.len() < MAGIC.len() + 1 {
        return Err(Refused::Range);
    }
    if count != 0 {
        return Err(Refused::Invalid);
    }
    out[..MAGIC.len()].copy_from_slice(MAGIC);
    Ok(MAGIC.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashed(phrase: &[u8], setting: &[u8]) -> Option<std::string::String> {
        let mut out = [0u8; 256];
        let n = crypt(phrase, setting, &mut out)?;
        Some(std::string::String::from_utf8(out[..n].to_vec()).unwrap())
    }

    /// The well-known NT hash of "password", and the rest of the setting
    /// unread.
    #[test]
    fn known_answer() {
        assert_eq!(
            hashed(b"password", b"$3$").as_deref(),
            Some("$3$$8846f7eaee8fb117ad06bdd830b7586c")
        );
        assert_eq!(
            hashed(b"password", b"$3$$anything"),
            hashed(b"password", b"$3$")
        );
        assert_eq!(hashed(b"pw\0ignored", b"$3$"), hashed(b"pw", b"$3$"));
        assert_eq!(hashed(b"pw", b"$3"), None);
    }

    #[test]
    fn shapes_and_gensalt() {
        let h = hashed(b"pw", b"$3$").unwrap();
        assert!(is_stored_hash(h.as_bytes()));
        assert!(!is_stored_hash(h.to_uppercase().as_bytes()));
        assert!(!is_stored_hash(&h.as_bytes()[..35]));
        let mut out = [0u8; 8];
        assert_eq!(gensalt(0, &[], &mut out), Ok(3));
        assert_eq!(&out[..3], b"$3$");
        assert_eq!(gensalt(1, &[], &mut out), Err(Refused::Invalid));
        assert_eq!(gensalt(0, &[], &mut out[..3]), Err(Refused::Range));
    }
}
