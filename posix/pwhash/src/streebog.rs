//! GOST R 34.11-2012 -- Streebog -- and its HMAC at 256 bits: the hashing
//! gost-yescrypt (`$gy$`, ALT Linux's default) wraps yescrypt's hash in.
//!
//! A port of libxcrypt 4.4.36's `alg-gost3411-2012-core.c` -- Alexey
//! Degtyarev's portable implementation: `GOST34112012Init`, `Update` and
//! `Final`, and the compression function `g` with its `XLPS` rounds from
//! `alg-gost3411-2012-ref.h` -- and of `alg-gost3411-2012-hmac.c`'s
//! `gost_hash256` and `gost_hmac256` (R 50.1.113-2016's HMAC), as
//! design-decisions §539 asks (primitives are ported, not written).  The
//! round function's tables and constants are libxcrypt's own, generated
//! into `streebog_tables.rs` by `posix/tools/gen_streebog_tables.py`;
//! libxcrypt's test vectors -- the standard's examples A.1 and A.2, a carry
//! case from gost-engine and R 50.1.113-2016's HMAC -- hold them here.
//!
//! The notice of the files the core comes from, which libxcrypt's
//! `LICENSING` gives as the 2-clause BSD licence (the files themselves carry
//! the copyright line only):
//!
//! ```text
//! Copyright (c) 2013, Alexey Degtyarev <alexey@renatasystems.org>.
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
//! The HMAC's files are Vitaly Chikunov's and Björn Esser's, under the
//! 0-clause BSD licence.

#![allow(clippy::indexing_slicing)] // indices masked to a byte, or below the fixed arrays' lengths
#![allow(clippy::arithmetic_side_effects)] // the buffer's fill, bounded by its 64 bytes

use crate::streebog_tables::{AX, C};

/// A 512-bit value as the hash holds it: eight words, least significant
/// first (`uint512_u`).
type U512 = [u64; 8];

/// 512, the bits in a block: what the length counter grows by.
const BLOCK_BITS: U512 = [512, 0, 0, 0, 0, 0, 0, 0];

/// `X`: `x` XOR `y`.
fn xor(x: &U512, y: &U512) -> U512 {
    core::array::from_fn(|i| x[i] ^ y[i])
}

/// `XLPS`: `x` XOR `y`, then the round function's S-box, transposition and
/// linear map, a table lookup a byte.
fn xlps(x: &U512, y: &U512) -> U512 {
    let r = xor(x, y);
    core::array::from_fn(|i| {
        let shift = 8 * i;
        let mut word = 0;
        for (table, rj) in AX.iter().zip(r) {
            word ^= table[((rj >> shift) & 0xff) as usize];
        }
        word
    })
}

/// `add512`: `x + y`, mod 2^512.
fn add512(x: &U512, y: &U512) -> U512 {
    let mut r = [0u64; 8];
    let mut carry = false;
    for (out, (&a, &b)) in r.iter_mut().zip(x.iter().zip(y)) {
        let (sum, c1) = a.overflowing_add(b);
        let (sum, c2) = sum.overflowing_add(u64::from(carry));
        *out = sum;
        carry = c1 || c2;
    }
    r
}

/// `g`: the compression function -- `h` updated by the block `m` under the
/// counter `n`, the twelve-round cipher E keyed by `h` XOR `n`.
fn g(h: &mut U512, n: &U512, m: &U512) {
    let mut k = xlps(h, n);
    let mut data = xlps(&k, m);
    for c in &C[..11] {
        k = xlps(&k, c);
        data = xlps(&k, &data);
    }
    k = xlps(&k, &C[11]);
    data = xor(&k, &data);
    data = xor(&data, h);
    *h = xor(&data, m);
}

/// A block's bytes as the hash reads them.
fn words(block: &[u8; 64]) -> U512 {
    core::array::from_fn(|i| {
        let mut w = [0u8; 8];
        w.copy_from_slice(&block[8 * i..8 * i + 8]);
        u64::from_le_bytes(w)
    })
}

/// The hash's state: `GOST34112012Context`.  Wiped when dropped.
struct Ctx {
    buffer: [u8; 64],
    bufsize: usize,
    h: U512,
    n: U512,
    sigma: U512,
}

impl Ctx {
    /// `GOST34112012Init`: the 256-bit hash's initial value is every byte
    /// 1, the 512-bit one's every byte 0.
    fn new(bits256: bool) -> Self {
        Self {
            buffer: [0; 64],
            bufsize: 0,
            h: [if bits256 { 0x0101_0101_0101_0101 } else { 0 }; 8],
            n: [0; 8],
            sigma: [0; 8],
        }
    }

    /// `stage2`: a whole block.
    fn block(&mut self, block: &[u8; 64]) {
        let m = words(block);
        g(&mut self.h, &self.n, &m);
        self.n = add512(&self.n, &BLOCK_BITS);
        self.sigma = add512(&self.sigma, &m);
    }

    /// `GOST34112012Update`.
    fn update(&mut self, mut data: &[u8]) {
        if self.bufsize > 0 {
            let take = (64 - self.bufsize).min(data.len());
            self.buffer[self.bufsize..self.bufsize + take].copy_from_slice(&data[..take]);
            self.bufsize += take;
            data = &data[take..];
            if self.bufsize == 64 {
                let buffer = self.buffer;
                self.block(&buffer);
                self.bufsize = 0;
            }
        }
        let (blocks, rest) = data.as_chunks::<64>();
        for block in blocks {
            self.block(block);
        }
        if !rest.is_empty() {
            self.buffer[..rest.len()].copy_from_slice(rest);
            self.bufsize = rest.len();
        }
    }

    /// `stage3`: the last, padded block, the length and the sum, and the
    /// value they leave.
    fn finish(&mut self) -> U512 {
        // The bufsize is below 64: whole blocks went at once.
        let bits: U512 = [(self.bufsize as u64) << 3, 0, 0, 0, 0, 0, 0, 0];
        self.buffer[self.bufsize..].fill(0);
        self.buffer[self.bufsize] = 0x01;
        let m = words(&self.buffer);
        g(&mut self.h, &self.n, &m);
        self.n = add512(&self.n, &bits);
        self.sigma = add512(&self.sigma, &m);
        let zero = [0u64; 8];
        let n = self.n;
        g(&mut self.h, &zero, &n);
        let sigma = self.sigma;
        g(&mut self.h, &zero, &sigma);
        self.h
    }
}

impl Drop for Ctx {
    fn drop(&mut self) {
        for b in &mut self.buffer {
            // SAFETY: a byte of this struct's own: written volatile so that
            // the compiler cannot drop the store as dead.
            unsafe { core::ptr::write_volatile(b, 0) };
        }
        for w in self.h.iter_mut().chain(&mut self.n).chain(&mut self.sigma) {
            // SAFETY: as above, a word of this struct's own.
            unsafe { core::ptr::write_volatile(w, 0) };
        }
    }
}

/// A value's bytes, least significant first, as `memcpy` would copy them.
fn bytes_of<const N: usize>(value: &[u64]) -> [u8; N] {
    let mut out = [0u8; N];
    for (chunk, w) in out.as_chunks_mut::<8>().0.iter_mut().zip(value) {
        *chunk = w.to_le_bytes();
    }
    out
}

/// GOST R 34.11-2012 at 256 bits -- the upper half of the last value --
/// over the concatenation of `parts`: `gost_hash256`.
#[must_use]
pub fn hash256(parts: &[&[u8]]) -> [u8; 32] {
    let mut ctx = Ctx::new(true);
    for part in parts {
        ctx.update(part);
    }
    let value = ctx.finish();
    bytes_of(&value[4..])
}

/// GOST R 34.11-2012 at 512 bits, over the concatenation of `parts`.
#[must_use]
pub fn hash512(parts: &[&[u8]]) -> [u8; 64] {
    let mut ctx = Ctx::new(false);
    for part in parts {
        ctx.update(part);
    }
    let value = ctx.finish();
    bytes_of(&value)
}

/// `gost_hmac256`: R 50.1.113-2016's HMAC with the 256-bit hash, of `msg`
/// under `key`, of 32 to 64 bytes as the recommendation allows -- `None`
/// for another length, where libxcrypt asserts (its only callers give 32).
#[must_use]
pub fn hmac256(key: &[u8], msg: &[u8]) -> Option<[u8; 32]> {
    if !(32..=64).contains(&key.len()) {
        return None;
    }
    let mut kstar = [0u8; 64];
    kstar[..key.len()].copy_from_slice(key);
    let mut pad = kstar.map(|b| b ^ 0x36);
    let mut inner = hash256(&[&pad, msg]);
    pad = kstar.map(|b| b ^ 0x5c);
    let out = hash256(&[&pad, &inner]);
    for b in kstar.iter_mut().chain(&mut pad).chain(&mut inner) {
        // SAFETY: a byte of this function's own arrays, written volatile so
        // that the key's traces are not left on the stack.
        unsafe { core::ptr::write_volatile(b, 0) };
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> std::string::String {
        use core::fmt::Write;
        let mut s = std::string::String::new();
        for b in bytes {
            write!(s, "{b:02x}").unwrap();
        }
        s
    }

    const M1: &[u8] = b"012345678901234567890123456789012345678901234567890123456789012";
    const M2: &[u8] = b"\xD1\xE5\x20\xE2\xE5\xF2\xF0\xE8\x2C\x20\xD1\xF2\xF0\xE8\xE1\xEE\
\xE6\xE8\x20\xE2\xED\xF3\xF6\xE8\x2C\x20\xE2\xE5\xFE\xF2\xFA\x20\
\xF1\x20\xEC\xEE\xF0\xFF\x20\xF1\xF2\xF0\xE5\xEB\xE0\xEC\xE8\x20\
\xED\xE0\x20\xF5\xF0\xE0\xE1\xF0\xFB\xFF\x20\xEF\xEB\xFA\xEA\xFB\
\x20\xC8\xE3\xEE\xF0\xE5\xE2\xFB";

    /// gost-engine's carry case: a sum that carries through every word.
    fn carry() -> std::vec::Vec<u8> {
        let mut m = std::vec![0xEEu8; 64];
        m.push(0x16);
        m.extend_from_slice(&[0x11; 62]);
        m.push(0x16);
        m
    }

    /// libxcrypt's `test/alg-gost3411-2012.c`, byte for byte as it prints
    /// them: the standard's examples A.1 and A.2 and gost-engine's carry
    /// case, at 256 and 512 bits -- the 512-bit ones fed in two halves, as
    /// libxcrypt's test feeds them.
    #[test]
    fn libxcrypts_vectors() {
        assert_eq!(
            hex(&hash256(&[M1])),
            "9d151eefd8590b89daa6ba6cb74af9275dd051026bb149a452fd84e5e57b5500"
        );
        assert_eq!(
            hex(&hash256(&[M2])),
            "9dd2fe4e90409e5da87f53976d7405b0c0cac628fc669a741d50063c557e8f50"
        );
        assert_eq!(
            hex(&hash256(&[&carry()])),
            "81bb632fa31fcc38b4c379a662dbc58b9bed83f50d3a1b2ce7271ab02d25babb"
        );
        for (message, want) in [
            (
                M1.to_vec(),
                "1b54d01a4af5b9d5cc3d86d68d285462b19abc2475222f35c085122be4ba1ffa\
                 00ad30f8767b3a82384c6574f024c311e2a481332b08ef7f41797891c1646f48",
            ),
            (
                M2.to_vec(),
                "1e88e62226bfca6f9994f1f2d51569e0daf8475a3b0fe61a5300eee46d961376\
                 035fe83549ada2b8620fcd7c496ce5b33f0cb9dddc2b6460143b03dabac9fb28",
            ),
            (
                carry(),
                "8b06f41e59907d9636e892caf5942fcdfb71fa31169a5e70f0edb873664df41c\
                 2cce6e06dc6755d15a61cdeb92bd607cc4aaca6732bf3568a23a210dd520fd41",
            ),
        ] {
            let (a, b) = message.split_at(message.len() / 2);
            assert_eq!(hex(&hash512(&[a, b])), want);
        }
    }

    /// However a message is cut, its hash is one.
    #[test]
    fn parts_are_one_message() {
        let message: std::vec::Vec<u8> = (0..300u32).map(|i| (i * 7) as u8).collect();
        let whole = hash256(&[&message]);
        for cut in [0, 1, 63, 64, 65, 128, 299, 300] {
            let (a, b) = message.split_at(cut);
            assert_eq!(hash256(&[a, b]), whole, "{cut}");
        }
    }

    /// R 50.1.113-2016's example, as libxcrypt's `test/alg-gost3411-2012-hmac.c`
    /// has it; and the key lengths the recommendation does not allow.
    #[test]
    fn hmac() {
        let key: std::vec::Vec<u8> = (0..32).collect();
        let msg = b"\x01\x26\xbd\xb8\x78\x00\xaf\x21\x43\x41\x45\x65\x63\x78\x01\x00";
        assert_eq!(
            hex(&hmac256(&key, msg).unwrap()),
            "a1aa5f7de402d7b3d323f2991c8d4534013137010a83754fd0af6d7cd4922ed9"
        );
        assert_eq!(hmac256(&key[..31], msg), None);
        assert_eq!(hmac256(&[0; 65], msg), None);
        assert!(hmac256(&[0; 64], msg).is_some());
    }
}
