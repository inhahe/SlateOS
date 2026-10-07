//! SHA-crypt's digest: Ulrich Drepper's "Unix crypt using SHA-256 and
//! SHA-512", steps 1 to 21 -- the rounds that make `$5$` and `$6$` slow.
//!
//! `posix`'s `crypt.rs` parses the setting (the salt, the `rounds=` field)
//! and encodes the result; this is the hashing in between, here so that it
//! compiles for speed (the crate docs).  The two functions are not generic
//! for the same reason: a generic one would be compiled in its caller's
//! crate, at its caller's opt-level.

#![allow(clippy::arithmetic_side_effects)] // counts of bytes and rounds, bounded by the caller's lengths
#![allow(clippy::indexing_slicing)] // digests sliced to their own fixed lengths

use crate::sha2::{Digest, Sha256, Sha512};

/// SHA-256 crypt's digest of `key` under `salt` and `rounds`, as the
/// caller has read them from the setting: the salt at most 16 bytes and
/// the rounds clamped, which are the caller's to apply.
pub fn sha256(key: &[u8], salt: &[u8], rounds: u32, out: &mut [u8; 32]) {
    raw::<Sha256>(key, salt, rounds, out);
}

/// SHA-512 crypt's digest, as [`sha256`]'s.
pub fn sha512(key: &[u8], salt: &[u8], rounds: u32, out: &mut [u8; 64]) {
    raw::<Sha512>(key, salt, rounds, out);
}

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
fn raw<D: Digest>(key: &[u8], salt: &[u8], rounds: u32, alt: &mut [u8]) {
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
