//! md5crypt's digest: Poul-Henning Kamp's `$1$`, its thousand rounds.
//!
//! `posix`'s `crypt.rs` reads the salt from the setting and encodes the
//! result; this is the hashing in between, here so that it compiles for
//! speed (the crate docs).  MD5 crypt is broken and kept only so that
//! existing `$1$` entries verify.

#![allow(clippy::arithmetic_side_effects)] // counts of bytes and rounds, bounded by the key's length
#![allow(clippy::indexing_slicing)] // digests sliced to their own fixed lengths

use crate::md5::Md5;

/// md5crypt's 16-byte digest of `key` under `salt`, as the caller has read
/// it from the setting (at most 8 bytes: truncating is the caller's).
///
/// Kamp's algorithm exactly, including the deliberately obscure key-length
/// bit loop, in which the running digest has been zeroed before being mixed
/// in.
#[must_use]
pub fn digest(key: &[u8], salt: &[u8]) -> [u8; 16] {
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
    digest
}
