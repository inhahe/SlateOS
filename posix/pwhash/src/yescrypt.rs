//! yescrypt and classic scrypt: the KDF behind `posix`'s `$y$` and `$7$`
//! `crypt` methods, whose settings `posix/src/yescrypt.rs` reads.
//!
//! A port of libxcrypt 4.4.36's, as design-decisions §539 asks (primitives
//! are ported, not written):
//!
//! - `alg-yescrypt-opt.c`, Alexander Peslyak's yescrypt over Colin
//!   Percival's scrypt, in the scalar form it takes when built without SSE2,
//!   whose results its SIMD forms reproduce bit for bit -- and, compiled at
//!   this crate's opt-level, whose speed too (`posix/benches/crypt.rs`).
//!   Its memory layout is kept: a 64-byte block is eight 64-bit words in the
//!   "SIMD-shuffled" order Salsa20's columns want, shuffled on the way in
//!   and out.
//! - From `alg-sha256.c`, HMAC-SHA256 and PBKDF2-SHA256, over
//!   [`crate::sha2`]'s SHA-256.
//!
//! Left out, each because `crypt` never reaches it: the shared ROM
//! (`yescrypt_shared_t` -- a setting asking for one is refused, as
//! libxcrypt's own `crypt` refuses it), the keyed hashes
//! (`yescrypt_sha256_cipher`), hash upgrades (`g`, which libxcrypt refuses
//! too), an output other than 32 bytes, and OpenMP.
//!
//! The files' notices, as their licence asks:
//!
//! ```text
//! alg-yescrypt-opt.c:
//!   Copyright 2009 Colin Percival
//!   Copyright 2012-2018 Alexander Peslyak
//! alg-sha256.c:
//!   Copyright 2005-2016 Colin Percival
//!   Copyright 2016-2018,2021 Alexander Peslyak
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
//!
//! ## Memory
//!
//! A hash works in memory its caller gives it -- this crate allocates
//! nothing: 64-bit words for V (128 r N bytes), XY (256 r) and, for
//! yescrypt, each lane's S-boxes (12 KiB), and bytes for B (128 r p), as
//! much as [`Params::memory`] says.  Blocks are addressed by word offsets
//! into V and XY, which is how the C code's pointers into one region, some
//! of them equal, become safe Rust; the S-boxes are split off as arrays of
//! their own, so a pwxform lookup, masked to the box, needs no bounds check.
//! As libxcrypt's `yescrypt_local_t` is, the memory sized for the hash is
//! reused by its prehash, which needs less.
//!
//! Indexing that could fail -- a block offset past the work area -- would be
//! a bug in this port, not something a setting can cause: every offset comes
//! from parameters `check` has bounded, by the arithmetic libxcrypt uses.

// See "Memory" above: offsets are bounded by the checked parameters, and the
// modular arithmetic is the algorithm's own -- spelled with wrapping
// operations where it wraps.
#![allow(clippy::indexing_slicing)]
#![allow(clippy::arithmetic_side_effects)]

use crate::sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Flags (alg-yescrypt.h)
// ---------------------------------------------------------------------------

const YESCRYPT_WORM: u32 = 1;
/// Flavours and modes (`yescrypt_flags_t`) a setting may name.
pub const YESCRYPT_RW: u32 = 0x002;
const YESCRYPT_ROUNDS_6: u32 = 0x004;
const YESCRYPT_GATHER_4: u32 = 0x010;
const YESCRYPT_SIMPLE_2: u32 = 0x020;
const YESCRYPT_SBOX_12K: u32 = 0x080;
const YESCRYPT_SHARED_PREALLOCATED: u32 = 0x10000;
const YESCRYPT_MODE_MASK: u32 = 0x003;
/// The bits of a flavour beyond its mode.
pub const YESCRYPT_RW_FLAVOR_MASK: u32 = 0x3fc;
const YESCRYPT_INIT_SHARED: u32 = 0x0100_0000;
const YESCRYPT_ALLOC_ONLY: u32 = 0x0800_0000;
const YESCRYPT_PREHASH: u32 = 0x1000_0000;
const YESCRYPT_KNOWN_FLAGS: u32 = YESCRYPT_MODE_MASK
    | YESCRYPT_RW_FLAVOR_MASK
    | YESCRYPT_SHARED_PREALLOCATED
    | YESCRYPT_INIT_SHARED
    | YESCRYPT_ALLOC_ONLY
    | YESCRYPT_PREHASH;
/// The one pwxform flavour libxcrypt (and so this port) implements: what
/// `$y$j` asks for.
const YESCRYPT_RW_FLAVOR: u32 =
    YESCRYPT_ROUNDS_6 | YESCRYPT_GATHER_4 | YESCRYPT_SIMPLE_2 | YESCRYPT_SBOX_12K;
/// libxcrypt's `YESCRYPT_DEFAULTS`: RW, with that flavour -- what `$y$j`
/// names.
pub const YESCRYPT_DEFAULTS: u32 = YESCRYPT_RW | YESCRYPT_RW_FLAVOR;

// pwxform, as alg-yescrypt-opt.c builds it: Swidth 8, PWXsimple 2,
// PWXgather 4, six rounds, 12 KiB of S-boxes.
const SWIDTH: u32 = 8;
const PWX_SIMPLE: usize = 2;
/// One S-box, in words: 2^Swidth entries of PWXsimple words.
const SBOX_WORDS: usize = (1 << SWIDTH) * PWX_SIMPLE;
/// The three S-boxes, in words (libxcrypt's `Sbytes`, 12 KiB, over 8).
const SWORDS: usize = 3 * SBOX_WORDS;
/// A lane's part of the work area: its S-boxes, then its pwxform state --
/// libxcrypt's `Salloc`, which keeps a `pwxform_ctx_t` after the S-boxes,
/// rounded to 64 bytes.
const SALLOC: usize = SWORDS + 8;
/// An S-box index's bits, as a byte offset (libxcrypt's `Smask`).
const SMASK: u64 = ((1 << SWIDTH) - 1) * (PWX_SIMPLE as u64) * 8;
/// [`SMASK`] in both halves of a word: two indices at once.
const SMASK2: u64 = (SMASK << 32) | SMASK;

/// `yescrypt_params_t`: what a setting asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// The mode -- classic scrypt (0), WORM (1) or [`YESCRYPT_RW`] -- and,
    /// for RW, the pwxform flavour ([`YESCRYPT_DEFAULTS`] is the one there
    /// is).
    pub flags: u32,
    /// N: blocks of 128 r bytes in V.  A power of 2.
    pub n: u64,
    /// r: a block's size, in 128 bytes.
    pub r: u32,
    /// p: the lanes.
    pub p: u32,
    /// t: extra time, beyond what N sets.
    pub t: u32,
    /// g: hash upgrades.  libxcrypt has "temporarily removed" them; one is
    /// refused.
    pub g: u32,
    /// NROM: the shared ROM's blocks.  `crypt` has no ROM; one is refused.
    pub nrom: u64,
}

// ---------------------------------------------------------------------------
// HMAC-SHA256, PBKDF2-SHA256 (alg-sha256.c)
// ---------------------------------------------------------------------------

/// HMAC-SHA256 with its key processed: the inner and outer hashes, each
/// past its padded key.
#[derive(Clone)]
struct HmacSha256 {
    inner: Sha256,
    outer: Sha256,
}

impl HmacSha256 {
    fn new(key: &[u8]) -> Self {
        let mut hashed = [0u8; 32];
        // A key longer than a block is replaced by its hash.
        let key = if key.len() > 64 {
            sha256(key, &mut hashed);
            &hashed[..]
        } else {
            key
        };
        let mut pad = [0x36u8; 64];
        for (p, k) in pad.iter_mut().zip(key) {
            *p ^= k;
        }
        let mut inner = Sha256::new();
        inner.update(&pad);
        pad = [0x5cu8; 64];
        for (p, k) in pad.iter_mut().zip(key) {
            *p ^= k;
        }
        let mut outer = Sha256::new();
        outer.update(&pad);
        wipe(&mut pad);
        wipe(&mut hashed);
        Self { inner, outer }
    }

    fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    fn finalize_into(self, out: &mut [u8; 32]) {
        let mut ihash = [0u8; 32];
        self.inner.finalize_into(&mut ihash);
        let mut outer = self.outer;
        outer.update(&ihash);
        outer.finalize_into(out);
        wipe(&mut ihash);
    }
}

/// `HMAC_SHA256_Buf`.
fn hmac_sha256(key: &[u8], data: &[u8], out: &mut [u8; 32]) {
    let mut h = HmacSha256::new(key);
    h.update(data);
    h.finalize_into(out);
}

/// `PBKDF2_SHA256(passwd, salt, c, buf)`.  `buf` is at most 2^32 - 1
/// blocks of 32 bytes: B, which [`check`]'s bound on r p keeps below 2^37
/// bytes, or the 32-byte output.
fn pbkdf2_sha256(passwd: &[u8], salt: &[u8], c: u64, buf: &mut [u8]) {
    let keyed = HmacSha256::new(passwd);
    let mut salted = keyed.clone();
    salted.update(salt);
    for (i, chunk) in buf.chunks_mut(32).enumerate() {
        // INT(i + 1), big-endian.
        let ivec = (i as u32).wrapping_add(1).to_be_bytes();
        let mut h = salted.clone();
        h.update(&ivec);
        let mut t = [0u8; 32];
        h.finalize_into(&mut t);
        let mut u = t;
        for _ in 1..c {
            let mut h = keyed.clone();
            h.update(&u);
            h.finalize_into(&mut u);
            for (tk, uk) in t.iter_mut().zip(u.iter()) {
                *tk ^= uk;
            }
        }
        let n = chunk.len();
        chunk.copy_from_slice(&t[..n]);
        wipe(&mut u);
        wipe(&mut t);
    }
}

/// `SHA256_Buf`.
fn sha256(data: &[u8], out: &mut [u8; 32]) {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize_into(out);
}

/// Zero `bytes` where the compiler may not drop the stores, as
/// `explicit_bzero` does: volatile writes, which it may not remove, then a
/// fence it may not move them past.
fn wipe(bytes: &mut [u8]) {
    for byte in bytes.iter_mut() {
        // SAFETY: `byte` is a valid, aligned and exclusive reference.
        unsafe { core::ptr::write_volatile(byte, 0) };
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

// ---------------------------------------------------------------------------
// Salsa20, in the shuffled layout
// ---------------------------------------------------------------------------

/// A 64-byte block: eight 64-bit words, SIMD-shuffled.  As 32-bit words --
/// the C code's `w[]` view of the same union, on a little-endian machine --
/// word `2k` is the low half of word `k` here, and `2k + 1` its high half.
type Blk = [u64; 8];

/// `salsa20_simd_shuffle`: sixteen words in Salsa20's order to a shuffled
/// block, whose 32-bit word `i` is Salsa20's word `5i mod 16`.
fn shuffle(w: &[u32; 16]) -> Blk {
    let c = |lo: usize, hi: usize| u64::from(w[lo]) | (u64::from(w[hi]) << 32);
    [
        c(0, 5),
        c(10, 15),
        c(4, 9),
        c(14, 3),
        c(8, 13),
        c(2, 7),
        c(12, 1),
        c(6, 11),
    ]
}

/// `salsa20_simd_unshuffle`: the inverse of [`shuffle`].
fn unshuffle(d: &Blk) -> [u32; 16] {
    let mut w = [0u32; 16];
    for (k, &word) in d.iter().enumerate() {
        w[(10 * k) % 16] = word as u32;
        w[(10 * k + 5) % 16] = (word >> 32) as u32;
    }
    w
}

/// `salsa20(&X, &Bout, doublerounds)`: the Salsa20 core over `x`, with its
/// feed-forward.  The C code leaves the result in both `X` and `Bout`; the
/// callers here store `x` where `Bout` is.
fn salsa20(x: &mut Blk, doublerounds: u32) {
    let mut w = unshuffle(x);
    for _ in 0..doublerounds {
        // Columns.
        w[4] ^= w[0].wrapping_add(w[12]).rotate_left(7);
        w[8] ^= w[4].wrapping_add(w[0]).rotate_left(9);
        w[12] ^= w[8].wrapping_add(w[4]).rotate_left(13);
        w[0] ^= w[12].wrapping_add(w[8]).rotate_left(18);
        w[9] ^= w[5].wrapping_add(w[1]).rotate_left(7);
        w[13] ^= w[9].wrapping_add(w[5]).rotate_left(9);
        w[1] ^= w[13].wrapping_add(w[9]).rotate_left(13);
        w[5] ^= w[1].wrapping_add(w[13]).rotate_left(18);
        w[14] ^= w[10].wrapping_add(w[6]).rotate_left(7);
        w[2] ^= w[14].wrapping_add(w[10]).rotate_left(9);
        w[6] ^= w[2].wrapping_add(w[14]).rotate_left(13);
        w[10] ^= w[6].wrapping_add(w[2]).rotate_left(18);
        w[3] ^= w[15].wrapping_add(w[11]).rotate_left(7);
        w[7] ^= w[3].wrapping_add(w[15]).rotate_left(9);
        w[11] ^= w[7].wrapping_add(w[3]).rotate_left(13);
        w[15] ^= w[11].wrapping_add(w[7]).rotate_left(18);
        // Rows.
        w[1] ^= w[0].wrapping_add(w[3]).rotate_left(7);
        w[2] ^= w[1].wrapping_add(w[0]).rotate_left(9);
        w[3] ^= w[2].wrapping_add(w[1]).rotate_left(13);
        w[0] ^= w[3].wrapping_add(w[2]).rotate_left(18);
        w[6] ^= w[5].wrapping_add(w[4]).rotate_left(7);
        w[7] ^= w[6].wrapping_add(w[5]).rotate_left(9);
        w[4] ^= w[7].wrapping_add(w[6]).rotate_left(13);
        w[5] ^= w[4].wrapping_add(w[7]).rotate_left(18);
        w[11] ^= w[10].wrapping_add(w[9]).rotate_left(7);
        w[8] ^= w[11].wrapping_add(w[10]).rotate_left(9);
        w[9] ^= w[8].wrapping_add(w[11]).rotate_left(13);
        w[10] ^= w[9].wrapping_add(w[8]).rotate_left(18);
        w[12] ^= w[15].wrapping_add(w[14]).rotate_left(7);
        w[13] ^= w[12].wrapping_add(w[15]).rotate_left(9);
        w[14] ^= w[13].wrapping_add(w[12]).rotate_left(13);
        w[15] ^= w[14].wrapping_add(w[13]).rotate_left(18);
    }
    let out = shuffle(&w);
    // The feed-forward, in 32-bit words.
    for (xk, ok) in x.iter_mut().zip(out.iter()) {
        let lo = (*ok as u32).wrapping_add(*xk as u32);
        let hi = ((*ok >> 32) as u32).wrapping_add((*xk >> 32) as u32);
        *xk = u64::from(lo) | (u64::from(hi) << 32);
    }
}

/// The block at word offset `at`.
fn load(mem: &[u64], at: usize) -> Blk {
    let mut b = [0u64; 8];
    b.copy_from_slice(&mem[at..at + 8]);
    b
}

fn store(mem: &mut [u64], at: usize, b: &Blk) {
    mem[at..at + 8].copy_from_slice(b);
}

/// `x ^=` the block at word offset `at`.
fn xor_from(x: &mut Blk, mem: &[u64], at: usize) {
    for (xk, mk) in x.iter_mut().zip(&mem[at..at + 8]) {
        *xk ^= mk;
    }
}

/// `INTEGERIFY`: the low 32 bits of the block's first word.
fn integerify_x(x: &Blk) -> u32 {
    x[0] as u32
}

/// `integerify(B, r)`: the low 32 bits of B's last block's first word.
fn integerify(mem: &[u64], b: usize, r: usize) -> u32 {
    mem[b + (2 * r - 1) * 8] as u32
}

/// `blockmix_salsa8(Bin, Bout, r)`: scrypt's BlockMix with Salsa20/8, over
/// the 2r blocks at word offset `bin` into those at `bout`, which do not
/// overlap them.
fn blockmix_salsa8(mem: &mut [u64], bin: usize, bout: usize, r: usize) {
    let mut x = load(mem, bin + (2 * r - 1) * 8);
    for i in 0..r {
        xor_from(&mut x, mem, bin + i * 16);
        salsa20(&mut x, 4);
        store(mem, bout + i * 8, &x);
        xor_from(&mut x, mem, bin + i * 16 + 8);
        salsa20(&mut x, 4);
        store(mem, bout + (r + i) * 8, &x);
    }
}

/// `blockmix_salsa8_xor(Bin1, Bin2, Bout, r)`: BlockMix of `Bin1 ^ Bin2`;
/// the three do not overlap.  `Integerify` of the result.
fn blockmix_salsa8_xor(mem: &mut [u64], bin1: usize, bin2: usize, bout: usize, r: usize) -> u32 {
    let mut x = load(mem, bin1 + (2 * r - 1) * 8);
    xor_from(&mut x, mem, bin2 + (2 * r - 1) * 8);
    for i in 0..r {
        xor_from(&mut x, mem, bin1 + i * 16);
        xor_from(&mut x, mem, bin2 + i * 16);
        salsa20(&mut x, 4);
        store(mem, bout + i * 8, &x);
        xor_from(&mut x, mem, bin1 + i * 16 + 8);
        xor_from(&mut x, mem, bin2 + i * 16 + 8);
        salsa20(&mut x, 4);
        store(mem, bout + (r + i) * 8, &x);
    }
    integerify_x(&x)
}

// ---------------------------------------------------------------------------
// pwxform
// ---------------------------------------------------------------------------

/// One S-box.
type Sbox = [u64; SBOX_WORDS];

/// A lane's pwxform state (`pwxform_ctx_t`): its three S-boxes, which
/// rotate after every pwxform, and where in S2 the next write goes.
struct Sboxes<'m> {
    boxes: [&'m mut Sbox; 3],
    /// Which of `boxes` is S2; S1 is the next, S0 the one after.
    s2: usize,
    /// Where the next write into S2 goes, in bytes: a multiple of 256.
    w: usize,
}

impl<'m> Sboxes<'m> {
    /// The S-boxes in `region` (exactly three), with S2 the `s2`th of them
    /// and its write position `w`.  libxcrypt starts a lane at S2 = Si,
    /// S1 = Si + Sbytes/3, S0 = Si + 2 Sbytes/3 -- `s2` 0 -- and `w` 0.
    fn new(region: &'m mut [u64], s2: usize, w: usize) -> Option<Self> {
        let (boxes, []) = region.as_chunks_mut::<SBOX_WORDS>() else {
            return None;
        };
        let [a, b, c] = boxes else {
            return None;
        };
        Some(Self {
            boxes: [a, b, c],
            s2: s2 % 3,
            w: w & (SMASK as usize),
        })
    }
}

/// `PWXFORM_ROUND`, scalar (`PWXFORM_SIMD(x0, x1)` four times): each pair
/// of words picks a 16-byte entry of S0 and one of S1 by its first word's
/// two halves, multiplies its halves, adds the one and XORs the other.
fn pwxform_round(x: &mut Blk, s0: &Sbox, s1: &Sbox) {
    for k in [0, 2, 4, 6] {
        let m = x[k] & SMASK2;
        // Byte offsets, masked to the box and 16-aligned; as word indices,
        // even, at most 510 -- so `+ 1` stays inside the 512-word box.
        let p0 = ((m as u32) >> 3) as usize;
        let p1 = (m >> 35) as usize;
        let (x0, x1) = (x[k], x[k + 1]);
        x[k] = (x0 >> 32)
            .wrapping_mul(x0 & 0xffff_ffff)
            .wrapping_add(s0[p0])
            ^ s1[p1];
        x[k + 1] = (x1 >> 32)
            .wrapping_mul(x1 & 0xffff_ffff)
            .wrapping_add(s0[p0 + 1])
            ^ s1[p1 + 1];
    }
}

/// `PWXFORM`: six rounds, `x` written to S2 after the second to fifth, then
/// the write position moved on and the S-boxes rotated.
fn pwxform(x: &mut Blk, sb: &mut Sboxes) {
    let which = sb.s2;
    // A multiple of 256 bytes below 4096: four 64-byte blocks fit.
    let w = sb.w / 8;
    {
        let [a, b, c] = &mut sb.boxes;
        let (s2, s1, s0): (&mut Sbox, &Sbox, &Sbox) = match which {
            0 => (&mut **a, &**b, &**c),
            1 => (&mut **b, &**c, &**a),
            _ => (&mut **c, &**a, &**b),
        };
        pwxform_round(x, s0, s1);
        for k in 0..4 {
            pwxform_round(x, s0, s1);
            s2[w + k * 8..w + k * 8 + 8].copy_from_slice(x);
        }
        pwxform_round(x, s0, s1);
    }
    sb.w = ((sb.w as u64 + 64 * 4) & SMASK2) as usize;
    // S2 <- S1, S1 <- S0, S0 <- S2: S2 moves on to the next box.
    sb.s2 = (which + 1) % 3;
}

/// `blockmix(Bin, Bout, r, ctx)`: yescrypt's BlockMix with pwxform, over
/// the 2r blocks at `bin` into those at `bout`, which do not overlap them.
fn blockmix(mem: &mut [u64], bin: usize, bout: usize, r: usize, sb: &mut Sboxes) {
    let last = r * 2 - 1;
    let mut x = load(mem, bin + last * 8);
    let mut i = 0;
    loop {
        xor_from(&mut x, mem, bin + i * 8);
        pwxform(&mut x, sb);
        if i >= last {
            break;
        }
        store(mem, bout + i * 8, &x);
        i += 1;
    }
    salsa20(&mut x, 1);
    store(mem, bout + i * 8, &x);
}

/// `blockmix_xor(Bin1, Bin2, Bout, r, ctx)`: BlockMix of `Bin1 ^ Bin2`.
/// `bin1` and `bout` may be the same blocks (each is read before it is
/// written); `bin2` overlaps neither.  `Integerify` of the result.
fn blockmix_xor(
    mem: &mut [u64],
    bin1: usize,
    bin2: usize,
    bout: usize,
    r: usize,
    sb: &mut Sboxes,
) -> u32 {
    let last = r * 2 - 1;
    let mut x = load(mem, bin1 + last * 8);
    xor_from(&mut x, mem, bin2 + last * 8);
    let mut i = 0;
    loop {
        xor_from(&mut x, mem, bin1 + i * 8);
        xor_from(&mut x, mem, bin2 + i * 8);
        pwxform(&mut x, sb);
        store(mem, bout + i * 8, &x);
        xor_from(&mut x, mem, bin1 + (i + 1) * 8);
        xor_from(&mut x, mem, bin2 + (i + 1) * 8);
        pwxform(&mut x, sb);
        if i >= last - 1 {
            break;
        }
        store(mem, bout + (i + 1) * 8, &x);
        i += 2;
    }
    salsa20(&mut x, 1);
    store(mem, bout + last * 8, &x);
    integerify_x(&x)
}

/// `blockmix_xor_save(Bin1out, Bin2, r, ctx)`: BlockMix of
/// `Bin1out ^ Bin2` into `Bin1out`, with `Bin1out ^ Bin2` left in `Bin2`.
/// `Integerify` of the result.
fn blockmix_xor_save(
    mem: &mut [u64],
    bin1out: usize,
    bin2: usize,
    r: usize,
    sb: &mut Sboxes,
) -> u32 {
    let last = r * 2 - 1;
    let mut x = load(mem, bin1out + last * 8);
    xor_from(&mut x, mem, bin2 + last * 8);
    let mut i = 0;
    loop {
        for k in [i, i + 1] {
            // XOR_X_WRITE_XOR_Y_2: Y = Bin2 ^ Bin1out, written to Bin2.
            let mut y = load(mem, bin2 + k * 8);
            xor_from(&mut y, mem, bin1out + k * 8);
            store(mem, bin2 + k * 8, &y);
            for (xj, yj) in x.iter_mut().zip(y.iter()) {
                *xj ^= yj;
            }
            pwxform(&mut x, sb);
            if k == i {
                store(mem, bin1out + i * 8, &x);
            }
        }
        if i >= last - 1 {
            break;
        }
        store(mem, bin1out + (i + 1) * 8, &x);
        i += 2;
    }
    salsa20(&mut x, 1);
    store(mem, bin1out + last * 8, &x);
    integerify_x(&x)
}

// ---------------------------------------------------------------------------
// SMix
// ---------------------------------------------------------------------------

/// B's 128r bytes, as little-endian words, shuffled into the 2r blocks at
/// word offset `dst`.
fn load_b(mem: &mut [u64], dst: usize, b: &[u8], r: usize) {
    for (i, bytes) in b[..128 * r].chunks_exact(64).enumerate() {
        let mut w = [0u32; 16];
        for (word, le) in w.iter_mut().zip(bytes.chunks_exact(4)) {
            *word = u32::from_le_bytes([le[0], le[1], le[2], le[3]]);
        }
        store(mem, dst + i * 8, &shuffle(&w));
    }
}

/// The 2r blocks at word offset `src`, unshuffled, as little-endian words
/// into B's 128r bytes.
fn store_b(mem: &[u64], src: usize, b: &mut [u8], r: usize) {
    for (i, bytes) in b[..128 * r].chunks_exact_mut(64).enumerate() {
        let w = unshuffle(&load(mem, src + i * 8));
        for (word, le) in w.iter().zip(bytes.chunks_exact_mut(4)) {
            le.copy_from_slice(&word.to_le_bytes());
        }
    }
}

/// A lane's S-boxes' contents: `smix1(Bp, 1, Sbytes / 128, 0, Si, ...)`,
/// classic scrypt's first loop with r = 1, run over the S-boxes as its V.
/// B's first 128 bytes are its input, and are replaced by its result.
fn sbox_init(b: &mut [u8], s: &mut [u64]) {
    // 96 pairs of blocks, 16 words each.
    const N: usize = SWORDS / 16;
    load_b(s, 0, b, 1);
    let (mut x, mut y) = (0, 16);
    let mut i = 1;
    while i < N - 1 {
        blockmix_salsa8(s, x, y, 1);
        x = y + 16;
        blockmix_salsa8(s, y, x, 1);
        y = x + 16;
        i += 2;
    }
    blockmix_salsa8(s, x, y, 1);
    // The last BlockMix's result is B's rather than V's: libxcrypt writes
    // it to XY, here to a block pair of its own.
    let mut tail = [0u64; 32];
    tail[..16].copy_from_slice(&s[y..y + 16]);
    blockmix_salsa8(&mut tail, 0, 16, 1);
    store_b(&tail, 16, b, 1);
}

/// `smix1(B, r, N, flags, V, 0, NULL, XY, ctx)`: SMix's first loop, which
/// fills V.  `v` and `xy` are word offsets into `mem`: N block pairs of 2r
/// blocks, and 2r blocks.  N is even and at least 4.
#[allow(clippy::too_many_arguments)] // smix1's own parameters
fn smix1(
    b: &mut [u8],
    r: usize,
    n: u32,
    flags: u32,
    mem: &mut [u64],
    v: usize,
    xy: usize,
    sb: Option<&mut Sboxes>,
) {
    let s = 2 * r * 8;
    let mut x = v;
    let mut y = v + s;
    load_b(mem, x, b, r);
    match sb {
        Some(sb) if flags & YESCRYPT_RW != 0 => {
            blockmix(mem, x, y, r, sb);
            x = y + s;
            blockmix(mem, y, x, r, sb);
            let mut j = integerify(mem, x, r);
            let mut n2: u32 = 2;
            while n2 < n {
                let m = if n2 < n / 2 { n2 } else { n - 1 - n2 };
                let mut i: u32 = 1;
                while i < m {
                    y = x + s;
                    j &= n2 - 1;
                    j = j.wrapping_add(i - 1);
                    j = blockmix_xor(mem, x, v + j as usize * s, y, r, sb);
                    j &= n2 - 1;
                    j = j.wrapping_add(i);
                    x = y + s;
                    j = blockmix_xor(mem, y, v + j as usize * s, x, r, sb);
                    i += 2;
                }
                n2 <<= 1;
            }
            n2 >>= 1;
            j &= n2 - 1;
            j = j.wrapping_add(n - 2 - n2);
            y = x + s;
            j = blockmix_xor(mem, x, v + j as usize * s, y, r, sb);
            j &= n2 - 1;
            j = j.wrapping_add(n - 1 - n2);
            blockmix_xor(mem, y, v + j as usize * s, xy, r, sb);
        }
        _ => {
            let mut i = 1;
            while i < n - 1 {
                blockmix_salsa8(mem, x, y, r);
                x = y + s;
                blockmix_salsa8(mem, y, x, r);
                y = x + s;
                i += 2;
            }
            blockmix_salsa8(mem, x, y, r);
            blockmix_salsa8(mem, y, xy, r);
        }
    }
    store_b(mem, xy, b, r);
}

/// `smix2(B, r, N, Nloop, flags, V, 0, NULL, XY, ctx)`: SMix's second loop,
/// `nloop` (even) rounds reading V, and -- yescrypt's RW -- writing it.
/// N is a power of 2.
#[allow(clippy::too_many_arguments)] // smix2's own parameters
fn smix2(
    b: &mut [u8],
    r: usize,
    n: u32,
    nloop: u64,
    flags: u32,
    mem: &mut [u64],
    v: usize,
    xy: usize,
    mut sb: Option<&mut Sboxes>,
) {
    if nloop == 0 {
        return;
    }
    let s = 2 * r * 8;
    let x = xy;
    let y = xy + s;
    load_b(mem, x, b, r);
    let mask = n - 1;
    let mut j = integerify(mem, x, r) & mask;
    for _ in 0..nloop / 2 {
        match &mut sb {
            Some(sb) if flags & YESCRYPT_RW != 0 => {
                j = blockmix_xor_save(mem, x, v + j as usize * s, r, sb) & mask;
                j = blockmix_xor_save(mem, x, v + j as usize * s, r, sb) & mask;
            }
            Some(sb) => {
                j = blockmix_xor(mem, x, v + j as usize * s, x, r, sb) & mask;
                j = blockmix_xor(mem, x, v + j as usize * s, x, r, sb) & mask;
            }
            None => {
                j = blockmix_salsa8_xor(mem, x, v + j as usize * s, y, r) & mask;
                j = blockmix_salsa8_xor(mem, y, v + j as usize * s, x, r) & mask;
            }
        }
    }
    store_b(mem, x, b, r);
}

/// `p2floor(x)`: the largest power of 2 not above `x`.
fn p2floor(mut x: u64) -> u64 {
    loop {
        let y = x & x.wrapping_sub(1);
        if y == 0 {
            return x;
        }
        x = y;
    }
}

/// `smix(B, r, N, p, t, flags, V, 0, NULL, XY, S, passwd)` over the `p`
/// lanes of `b`.  `mem` is V then XY (at word offset `xy`); `lanes` holds
/// each lane's [`SALLOC`] words for yescrypt.  yescrypt also keys `passwd`
/// with lane 0's B, once that lane's S-boxes are filled.
#[allow(clippy::too_many_arguments)] // SMix's own parameters
fn smix(
    b: &mut [u8],
    r: usize,
    n: u32,
    p: u32,
    t: u32,
    flags: u32,
    mem: &mut [u64],
    xy: usize,
    lanes: &mut [u64],
    passwd: &mut [u8; 32],
) -> Option<()> {
    let s = 2 * r * 8;
    let rw = flags & YESCRYPT_RW != 0;
    let mut nchunk = n / p;
    let mut nloop_all = u64::from(nchunk);
    if rw {
        if t <= 1 {
            if t != 0 {
                nloop_all *= 2; // 2/3
            }
            nloop_all = nloop_all.div_ceil(3); // 1/3, round up
        } else {
            nloop_all = nloop_all.wrapping_mul(u64::from(t - 1));
        }
    } else if t != 0 {
        if t == 1 {
            nloop_all += nloop_all.div_ceil(2); // 1.5, round up
        }
        nloop_all = nloop_all.wrapping_mul(u64::from(t));
    }
    let mut nloop_rw = if rw { nloop_all / u64::from(p) } else { 0 };
    nchunk &= !1; // round down to even
    nloop_all = nloop_all.wrapping_add(1) & !1; // round up to even
    nloop_rw = nloop_rw.wrapping_add(1) & !1; // round up to even

    for i in 0..p {
        let lane = i as usize;
        let vchunk = i * nchunk;
        let np = if i < p - 1 { nchunk } else { n - vchunk };
        let np2 = p2floor(u64::from(np)) as u32;
        let bp = &mut b[128 * r * lane..128 * r * (lane + 1)];
        let vp = vchunk as usize * s;
        if rw {
            let region = &mut lanes[SALLOC * lane..SALLOC * (lane + 1)];
            let (boxes, saved) = region.split_at_mut(SWORDS);
            sbox_init(bp, boxes);
            let mut sb = Sboxes::new(boxes, 0, 0)?;
            if i == 0 {
                let mut key = [0u8; 64];
                key.copy_from_slice(&bp[128 * r - 64..]);
                let mut keyed = [0u8; 32];
                hmac_sha256(&key, &passwd[..], &mut keyed);
                *passwd = keyed;
                wipe(&mut key);
                wipe(&mut keyed);
            }
            smix1(bp, r, np, flags, mem, vp, xy, Some(&mut sb));
            smix2(bp, r, np2, nloop_rw, flags, mem, vp, xy, Some(&mut sb));
            saved[0] = sb.s2 as u64;
            saved[1] = sb.w as u64;
        } else {
            smix1(bp, r, np, flags, mem, vp, xy, None);
            smix2(bp, r, np2, nloop_rw, flags, mem, vp, xy, None);
        }
    }
    if nloop_all > nloop_rw {
        let nloop = nloop_all - nloop_rw;
        let flags = flags & !YESCRYPT_RW;
        for lane in 0..p as usize {
            let bp = &mut b[128 * r * lane..128 * r * (lane + 1)];
            if rw {
                let region = &mut lanes[SALLOC * lane..SALLOC * (lane + 1)];
                let (boxes, saved) = region.split_at_mut(SWORDS);
                let mut sb = Sboxes::new(boxes, saved[0] as usize, saved[1] as usize)?;
                smix2(bp, r, n, nloop, flags, mem, 0, xy, Some(&mut sb));
            } else {
                smix2(bp, r, n, nloop, flags, mem, 0, xy, None);
            }
        }
    }
    Some(())
}

// ---------------------------------------------------------------------------
// The KDF
// ---------------------------------------------------------------------------

/// `yescrypt_kdf_body`'s checks of its parameters, in its order -- with no
/// shared ROM, so an `NROM` is refused, and without the one of the output's
/// length, which is always 32 bytes here.
fn check(flags: u32, n: u64, r: u32, p: u32, t: u32, nrom: u64) -> Option<()> {
    match flags & YESCRYPT_MODE_MASK {
        // Classic scrypt: nothing non-standard.
        0 if flags != 0 || t != 0 || nrom != 0 => return None,
        YESCRYPT_WORM if flags != YESCRYPT_WORM || nrom != 0 => return None,
        YESCRYPT_RW
            if flags != flags & YESCRYPT_KNOWN_FLAGS
                || flags & YESCRYPT_RW_FLAVOR_MASK != YESCRYPT_RW_FLAVOR =>
        {
            return None;
        }
        0 | YESCRYPT_WORM | YESCRYPT_RW => {}
        _ => return None,
    }
    let (r64, p64) = (u64::from(r), u64::from(p));
    let max = usize::MAX as u64;
    let refused = r64 * p64 >= 1 << 30
        || n > u64::from(u32::MAX)
        || n & n.wrapping_sub(1) != 0
        || n <= 3
        || r < 1
        || p < 1
        || r64 > max / 256 / p64
        || n > max / 128 / r64
        || (flags & YESCRYPT_RW != 0 && (n / p64 <= 3 || p64 > max / (SALLOC as u64 * 8)))
        || nrom != 0;
    (!refused).then_some(())
}

/// The memory a body of the KDF needs: B's bytes, and the words of V, XY
/// and the lanes' S-boxes.  `None` if it would not fit an address space.
fn need(flags: u32, n: u64, r: u32, p: u32) -> Option<(usize, usize)> {
    let (n, r, p) = (usize::try_from(n).ok()?, r as usize, p as usize);
    let b = r.checked_mul(128)?.checked_mul(p)?;
    let v = r.checked_mul(16)?.checked_mul(n)?;
    let xy = r.checked_mul(32)?;
    let s = if flags & YESCRYPT_RW != 0 {
        p.checked_mul(SALLOC)?
    } else {
        0
    };
    Some((b, v.checked_add(xy)?.checked_add(s)?))
}

/// The memory [`kdf`] works in, sized: libxcrypt's `yescrypt_local_t`,
/// which the caller allocates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Memory {
    /// B: 128 r p bytes.
    pub bytes: usize,
    /// V (16 r N words), XY (32 r) and, for yescrypt, each lane's S-boxes.
    pub words: usize,
}

impl Params {
    /// Whether `yescrypt_kdf` takes these: no hash upgrade (`g`), and its
    /// body's checks passed.  [`kdf`] refuses any others; a stored hash
    /// naming them can never be reproduced.
    #[must_use]
    pub fn accepted(&self) -> bool {
        self.g == 0 && check(self.flags, self.n, self.r, self.p, self.t, self.nrom).is_some()
    }

    /// The memory [`kdf`] needs for these.  `None` if they are not
    /// [accepted](Params::accepted), or the memory would not fit an address
    /// space.
    #[must_use]
    pub fn memory(&self) -> Option<Memory> {
        if !self.accepted() {
            return None;
        }
        let (bytes, words) = need(self.flags, self.n, self.r, self.p)?;
        Some(Memory { bytes, words })
    }
}

/// `yescrypt_kdf_body(NULL, local, passwd, salt, flags, N, r, p, t, NROM,
/// buf)`, in `bytes` and `words`, which [`kdf`]'s caller sized for the
/// largest body it runs.  The output is 32 bytes, all `crypt` asks for --
/// which leaves out libxcrypt's handling of a shorter one.
#[allow(clippy::too_many_arguments)] // the KDF's own parameters
fn kdf_body(
    bytes: &mut [u8],
    words: &mut [u64],
    passwd: &[u8],
    salt: &[u8],
    flags: u32,
    n: u64,
    r: u32,
    p: u32,
    t: u32,
    nrom: u64,
    buf: &mut [u8; 32],
) -> Option<()> {
    check(flags, n, r, p, t, nrom)?;
    let (b_len, words_len) = need(flags, n, r, p)?;
    let ru = r as usize;
    let b = bytes.get_mut(..b_len)?;
    let mem = words.get_mut(..words_len)?;
    // V, then XY, then the lanes' S-boxes.
    let xy = 16 * ru * n as usize;
    let (mem, lanes) = mem.split_at_mut(xy + 32 * ru);

    // yescrypt hashes the password first, and the hash stands for it.
    let mut sha = [0u8; 32];
    let prehashed = flags != 0;
    if prehashed {
        let key: &[u8] = if flags & YESCRYPT_PREHASH != 0 {
            b"yescrypt-prehash"
        } else {
            b"yescrypt"
        };
        hmac_sha256(key, passwd, &mut sha);
        pbkdf2_sha256(&sha, salt, 1, b);
        sha.copy_from_slice(&b[..32]);
    } else {
        pbkdf2_sha256(passwd, salt, 1, b);
    }

    let n = n as u32;
    let done = if p == 1 || flags & YESCRYPT_RW != 0 {
        smix(b, ru, n, p, t, flags, mem, xy, lanes, &mut sha)
    } else {
        // Classic scrypt and WORM: each lane alone, in the same V.
        (0..p as usize).try_for_each(|i| {
            let bp = &mut b[128 * ru * i..128 * ru * (i + 1)];
            smix(bp, ru, n, 1, t, flags, mem, xy, &mut [], &mut sha)
        })
    };

    let result = done.map(|()| {
        let pw: &[u8] = if prehashed { &sha } else { passwd };
        pbkdf2_sha256(pw, b, 1, buf);
        // Except for classic scrypt, SCRAM's last steps (RFC 5802), so that
        // all before them could be computed by a client: ClientKey, then
        // StoredKey, its hash.
        if prehashed && flags & YESCRYPT_PREHASH == 0 {
            let mut client = [0u8; 32];
            hmac_sha256(&buf[..], b"Client Key", &mut client);
            sha256(&client, buf);
            wipe(&mut client);
        }
    });
    wipe(&mut sha);
    wipe(b);
    result
}

/// `yescrypt_kdf(NULL, local, passwd, salt, params, out)`: the 32-byte hash
/// into `out`, working in `bytes` and `words`, which must hold at least
/// what [`Params::memory`] says -- the hash's own; its prehash, when it has
/// one, needs less and runs in the same memory, as libxcrypt's does.  B is
/// wiped before this returns; V is not, as libxcrypt's is not.
///
/// `None` if the parameters are not [accepted](Params::accepted), or the
/// memory is too small.
pub fn kdf(
    passwd: &[u8],
    salt: &[u8],
    params: &Params,
    bytes: &mut [u8],
    words: &mut [u64],
    out: &mut [u8; 32],
) -> Option<()> {
    if !params.accepted() {
        return None;
    }
    let Params {
        flags,
        n,
        r,
        p,
        t,
        nrom,
        ..
    } = *params;
    let prehash = flags & (YESCRYPT_RW | YESCRYPT_INIT_SHARED) == YESCRYPT_RW
        && p >= 1
        && n / u64::from(p) >= 0x100
        && (n / u64::from(p)).wrapping_mul(u64::from(r)) >= 0x20000;
    if !prehash {
        return kdf_body(bytes, words, passwd, salt, flags, n, r, p, t, nrom, out);
    }
    // A large hash first hashes the password with 1/64 of its memory, the
    // result standing for the password.
    let mut dk = [0u8; 32];
    let result = kdf_body(
        bytes,
        words,
        passwd,
        salt,
        flags | YESCRYPT_PREHASH,
        n >> 6,
        r,
        p,
        0,
        nrom,
        &mut dk,
    )
    .and_then(|()| kdf_body(bytes, words, &dk, salt, flags, n, r, p, t, nrom, out));
    wipe(&mut dk);
    result
}

// The hashes themselves are checked against libxcrypt through `posix`'s
// `crypt` (`crypt.rs`, `libxcrypt_answers`); these check the parts.
#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> std::vec::Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// RFC 4231's test cases 1 and 6 -- the second's key longer than a
    /// block, so hashed first -- and a key exactly a block long (Python's
    /// `hmac` for the last).
    #[test]
    fn hmac_sha256_vectors() {
        let mut out = [0u8; 32];
        hmac_sha256(&[0x0b; 20], b"Hi There", &mut out);
        assert_eq!(
            out[..],
            hex("b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7")
        );
        let data = b"Test Using Larger Than Block-Size Key - Hash Key First";
        hmac_sha256(&[0xaa; 131], data, &mut out);
        assert_eq!(
            out[..],
            hex("60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54")
        );
        let key: std::vec::Vec<u8> = (0..64).collect();
        hmac_sha256(&key, b"exactly a block", &mut out);
        assert_eq!(
            out[..],
            hex("80678d13b7f87900d2667e2407a2a7960f560b00f62b9b5c86bb90c307413e59")
        );
    }

    /// RFC 7914 section 11's PBKDF2-HMAC-SHA256 vectors, and a count of 2
    /// whose output ends inside a block (Python's `hashlib`).
    #[test]
    fn pbkdf2_sha256_vectors() {
        let mut out = [0u8; 64];
        pbkdf2_sha256(b"passwd", b"salt", 1, &mut out);
        assert_eq!(
            out[..],
            hex(
                "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc\
                 49ca9cccf179b645991664b39d77ef317c71b845b1e30bd509112041d3a19783"
            )
        );
        pbkdf2_sha256(b"Password", b"NaCl", 80000, &mut out);
        assert_eq!(
            out[..],
            hex(
                "4ddcd8f60b98be21830cee5ef22701f9641a4418d04c0414aeff08876b34ab56\
                 a1d425a1225833549adb841b51c9b3176a272bdebba1d078478f62b397f33c8d"
            )
        );
        let mut out = [0u8; 40];
        pbkdf2_sha256(b"password", b"salt", 2, &mut out);
        assert_eq!(
            out[..],
            hex("ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43830651afcb5c862f")
        );
    }

    /// With no flags the KDF is RFC 7914's scrypt: its section 12 vectors 1
    /// and 2, cut to 32 bytes, and a password longer than an HMAC block
    /// over three lanes (Python's `hashlib.scrypt`).  `crypt`'s `$7$` and
    /// `$y$.` reach this path only with libxcrypt's own vectors.
    #[test]
    fn classic_scrypt_is_rfc_7914s() {
        /// One vector: its inputs and the first 32 bytes of its output.
        struct Vector {
            password: &'static [u8],
            salt: &'static [u8],
            n: u64,
            r: u32,
            p: u32,
            want: &'static str,
        }
        let vectors = [
            Vector {
                password: b"",
                salt: b"",
                n: 16,
                r: 1,
                p: 1,
                want: "77d6576238657b203b19ca42c18a0497f16b4844e3074ae8dfdffa3fede21442",
            },
            Vector {
                password: b"password",
                salt: b"NaCl",
                n: 1024,
                r: 8,
                p: 16,
                want: "fdbabe1c9d3472007856e7190d01e9fe7c6ad7cbc8237830e77376634b373162",
            },
            Vector {
                password: &[b'x'; 100],
                salt: b"long password",
                n: 64,
                r: 2,
                p: 3,
                want: "3144c098883792cdff3fcedf856a41565b7c6ad60c963d3491c5a113e06630a2",
            },
        ];
        for Vector {
            password,
            salt,
            n,
            r,
            p,
            want,
        } in vectors
        {
            let params = Params {
                flags: 0,
                n,
                r,
                p,
                t: 0,
                g: 0,
                nrom: 0,
            };
            let memory = params.memory().unwrap();
            let mut bytes = std::vec![0u8; memory.bytes];
            let mut words = std::vec![0u64; memory.words];
            let mut out = [0u8; 32];
            assert_eq!(
                kdf(password, salt, &params, &mut bytes, &mut words, &mut out),
                Some(())
            );
            assert_eq!(out[..], hex(want), "N {n} r {r} p {p}");
        }
    }

    /// The shuffle puts Salsa20's word 5i mod 16 at 32-bit word i, and the
    /// unshuffle undoes it.
    #[test]
    fn shuffle_round_trips() {
        let w: [u32; 16] = core::array::from_fn(|i| 0x0101_0101 * i as u32 + 7);
        let d = shuffle(&w);
        assert_eq!(unshuffle(&d), w);
        for (i, want) in (0..16).map(|i| (i, w[(5 * i) % 16])) {
            let word = d[i / 2];
            let half = if i % 2 == 0 {
                word as u32
            } else {
                (word >> 32) as u32
            };
            assert_eq!(half, want, "word {i}");
        }
    }

    #[test]
    fn p2floor_is_the_largest_power_of_2_not_above() {
        for (x, want) in [
            (1, 1),
            (2, 2),
            (3, 2),
            (96, 64),
            (4096, 4096),
            (u64::MAX, 1 << 63),
        ] {
            assert_eq!(p2floor(x), want, "{x}");
        }
    }

    /// `yescrypt_kdf_body`'s checks, a parameter at a time.
    #[test]
    fn parameter_checks() {
        let rw = YESCRYPT_RW | YESCRYPT_RW_FLAVOR;
        let ok = |flags, n, r, p, t, nrom| check(flags, n, r, p, t, nrom).is_some();
        assert!(ok(rw, 4096, 32, 1, 0, 0));
        assert!(ok(0, 16, 1, 1, 0, 0));
        assert!(!ok(0, 16, 1, 1, 1, 0), "classic scrypt has no t");
        assert!(ok(YESCRYPT_WORM, 16, 1, 1, 5, 0));
        assert!(!ok(YESCRYPT_WORM | YESCRYPT_ROUNDS_6, 16, 1, 1, 0, 0));
        assert!(!ok(YESCRYPT_WORM | YESCRYPT_RW, 16, 1, 1, 0, 0));
        assert!(!ok(YESCRYPT_RW, 16, 1, 1, 0, 0), "RW with no flavour");
        assert!(!ok(rw | 0x8000_0000, 16, 1, 1, 0, 0), "an unknown flag");
        assert!(!ok(rw, 3, 1, 1, 0, 0));
        assert!(!ok(rw, 24, 1, 1, 0, 0), "N not a power of 2");
        assert!(ok(rw, 1 << 31, 1, 1, 0, 0));
        assert!(!ok(rw, 1 << 32, 1, 1, 0, 0));
        assert!(!ok(rw, 16, 0, 1, 0, 0));
        assert!(!ok(rw, 16, 1, 0, 0, 0));
        assert!(ok(rw, 16, 1, 4, 0, 0), "N/p = 4");
        assert!(!ok(rw, 16, 1, 5, 0, 0), "N/p = 3");
        assert!(ok(0, 16, 1, 5, 0, 0), "classic scrypt's lanes have N each");
        assert!(!ok(rw, 1 << 20, 1 << 15, 1 << 15, 0, 0), "r p = 2^30");
        assert!(!ok(rw, 16, 1, 1, 0, 16), "a shared ROM");
    }
}
