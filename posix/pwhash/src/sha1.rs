//! SHA-1 (FIPS 180-4) and HMAC-SHA1 (RFC 2104): the hashing behind
//! sha1crypt (`$sha1`), NetBSD's crypt method, whose settings
//! `posix/src/sha1crypt.rs` reads.
//!
//! A port of libxcrypt 4.4.36's `alg-sha1.c` -- Steve Reid's SHA-1 and its
//! successors', in the public domain -- and `alg-hmac-sha1.c` (Björn
//! Esser's, 2-clause BSD), as design-decisions §539 asks (primitives are
//! ported, not written); and [`sha1crypt`], the loop of
//! `crypt-pbkdf1-sha1.c` (Juniper Networks', 3-clause BSD), whose notice is
//! `sha1crypt.rs`'s.  That loop computes an HMAC under the password a few
//! hundred thousand times over; here each HMAC starts from the key's two
//! padded blocks absorbed once, as RFC 2104 itself suggests, rather than
//! absorbing them again each time as libxcrypt does -- the same answers,
//! in two compressions an HMAC where libxcrypt takes four.
//!
//! The HMAC's notice, as its licence asks:
//!
//! ```text
//! Copyright (c) 2017, Björn Esser <besser82@fedoraproject.org>
//! All rights reserved.
//!
//! Redistribution and use in source and binary forms, with or without
//! modification, are permitted provided that the following conditions
//! are met:
//!
//! 1. Redistributions of source code must retain the above copyright
//!    notice, this list of conditions and the following disclaimer.
//!
//! 2. Redistributions in binary form must reproduce the above copyright
//!    notice, this list of conditions and the following disclaimer in the
//!    documentation and/or other materials provided with the distribution.
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

#![allow(clippy::indexing_slicing)] // the block's 64 bytes and the schedule's 16 words, by fixed or masked indices
#![allow(clippy::arithmetic_side_effects)] // SHA-1's additions are modulo 2^32, spelled wrapping; the rest count a block

/// SHA-1's initial value.
const H0: [u32; 5] = [
    0x6745_2301,
    0xEFCD_AB89,
    0x98BA_DCFE,
    0x1032_5476,
    0xC3D2_E1F0,
];

/// `sha1_do_transform`: one block into the state -- 80 steps of four
/// kinds, the schedule expanded as it goes.
fn compress(state: &mut [u32; 5], block: &[u8; 64]) {
    let mut w = [0u32; 16];
    for (word, bytes) in w.iter_mut().zip(block.chunks_exact(4)) {
        *word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *state;
    for i in 0..80 {
        if i >= 16 {
            w[i & 15] =
                (w[(i + 13) & 15] ^ w[(i + 8) & 15] ^ w[(i + 2) & 15] ^ w[i & 15]).rotate_left(1);
        }
        let (f, k) = match i {
            0..=19 => (((c ^ d) & b) ^ d, 0x5A82_7999),
            20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
            40..=59 => (((b | c) & d) | (b & c), 0x8F1B_BCDC),
            _ => (b ^ c ^ d, 0xCA62_C1D6),
        };
        let t = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(w[i & 15]);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = t;
    }
    for (s, v) in state.iter_mut().zip([a, b, c, d, e]) {
        *s = s.wrapping_add(v);
    }
}

/// A SHA-1 hash in progress (`struct sha1_ctx`).
#[derive(Clone)]
pub struct Sha1 {
    state: [u32; 5],
    block: [u8; 64],
    block_len: usize,
    /// Bytes so far, mod 2^64: the length padding counts bits mod 2^64.
    total: u64,
}

impl Sha1 {
    /// A hash of nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: H0,
            block: [0; 64],
            block_len: 0,
            total: 0,
        }
    }

    /// `sha1_process_bytes`.
    pub fn update(&mut self, mut data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u64);
        while !data.is_empty() {
            let take = (64 - self.block_len).min(data.len());
            self.block[self.block_len..self.block_len + take].copy_from_slice(&data[..take]);
            self.block_len += take;
            data = &data[take..];
            if self.block_len == 64 {
                compress(&mut self.state, &self.block);
                self.block_len = 0;
            }
        }
    }

    /// `sha1_finish_ctx`: the padding -- 0x80, zeros, the length in bits --
    /// and the digest.
    #[must_use]
    pub fn finalize(mut self) -> [u8; 20] {
        let bits = self.total.wrapping_mul(8);
        self.block[self.block_len] = 0x80;
        self.block[self.block_len + 1..].fill(0);
        if self.block_len >= 56 {
            compress(&mut self.state, &self.block);
            self.block.fill(0);
        }
        self.block[56..].copy_from_slice(&bits.to_be_bytes());
        compress(&mut self.state, &self.block);
        let mut out = [0u8; 20];
        for (chunk, word) in out.chunks_exact_mut(4).zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        self.wipe();
        out
    }

    fn wipe(&mut self) {
        for b in &mut self.block {
            // SAFETY: a byte of this hash's own block, written volatile so
            // that the store is not dropped as dead.
            unsafe { core::ptr::write_volatile(b, 0) };
        }
        for w in &mut self.state {
            // SAFETY: as above, a word of this hash's own state.
            unsafe { core::ptr::write_volatile(w, 0) };
        }
    }
}

impl Default for Sha1 {
    fn default() -> Self {
        Self::new()
    }
}

/// SHA-1 of `data`.
#[must_use]
pub fn digest(data: &[u8]) -> [u8; 20] {
    let mut h = Sha1::new();
    h.update(data);
    h.finalize()
}

/// An HMAC-SHA1 key, its two padded blocks absorbed.
struct Keyed {
    inner: Sha1,
    outer: Sha1,
}

impl Keyed {
    /// `hmac_sha1_process_data`'s key: hashed first if it is longer than a
    /// block, then XORed into the pads.
    fn new(key: &[u8]) -> Self {
        let mut hashed = [0u8; 20];
        let key = if key.len() > 64 {
            hashed = digest(key);
            &hashed[..]
        } else {
            key
        };
        let mut ipad = [0x36u8; 64];
        let mut opad = [0x5cu8; 64];
        for ((i, o), k) in ipad.iter_mut().zip(opad.iter_mut()).zip(key) {
            *i ^= k;
            *o ^= k;
        }
        let mut inner = Sha1::new();
        inner.update(&ipad);
        let mut outer = Sha1::new();
        outer.update(&opad);
        for b in ipad
            .iter_mut()
            .chain(opad.iter_mut())
            .chain(hashed.iter_mut())
        {
            // SAFETY: a byte of this function's own arrays, written volatile
            // so that the key's traces are not left on the stack.
            unsafe { core::ptr::write_volatile(b, 0) };
        }
        Self { inner, outer }
    }

    /// The HMAC of `text`.
    fn mac(&self, text: &[u8]) -> [u8; 20] {
        let mut inner = self.inner.clone();
        inner.update(text);
        let inner = inner.finalize();
        let mut outer = self.outer.clone();
        outer.update(&inner);
        outer.finalize()
    }
}

impl Drop for Keyed {
    fn drop(&mut self) {
        self.inner.wipe();
        self.outer.wipe();
    }
}

/// HMAC-SHA1 of `text` under `key` (RFC 2104).
#[must_use]
pub fn hmac(key: &[u8], text: &[u8]) -> [u8; 20] {
    Keyed::new(key).mac(text)
}

/// sha1crypt's rounds (`crypt_sha1crypt_rn`'s loop, PBKDF1 over
/// HMAC-SHA1): the HMAC of `first` -- the setting's salt, `$sha1$` and the
/// count, as it builds them -- under the password, then `iterations - 1`
/// more, each of the last digest.  0 iterations are 1, as there.
#[must_use]
pub fn sha1crypt(password: &[u8], first: &[u8], iterations: u64) -> [u8; 20] {
    let key = Keyed::new(password);
    let mut digest = key.mac(first);
    for _ in 1..iterations {
        digest = key.mac(&digest);
    }
    digest
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

    fn unhex(s: &str) -> std::vec::Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// libxcrypt's `test/alg-sha1.c`: FIPS 180's three examples, the last
    /// a million 'a's fed one at a time.
    #[test]
    fn libxcrypts_sha1_vectors() {
        assert_eq!(
            hex(&digest(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(&digest(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        let mut h = Sha1::new();
        for _ in 0..1_000_000 {
            h.update(b"a");
        }
        assert_eq!(
            hex(&h.finalize()),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
        assert_eq!(
            hex(&digest(b"")),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    /// Every length around the padding's boundaries hashes as the same
    /// bytes fed in pieces.
    #[test]
    fn pieces_are_one_message() {
        let data: std::vec::Vec<u8> = (0..200u32).map(|i| (i * 13) as u8).collect();
        for n in [0, 1, 55, 56, 63, 64, 65, 119, 120, 128, 200] {
            let whole = digest(&data[..n]);
            let mut h = Sha1::new();
            for chunk in data[..n].chunks(7) {
                h.update(chunk);
            }
            assert_eq!(h.finalize(), whole, "{n}");
        }
    }

    /// libxcrypt's `test/alg-hmac-sha1.c`: RFC 2202's cases, keys longer
    /// than a block among them.
    #[test]
    fn libxcrypts_hmac_vectors() {
        let cases: [(std::vec::Vec<u8>, std::vec::Vec<u8>, &str); 7] = [
            (
                std::vec![0x0b; 20],
                b"Hi There".to_vec(),
                "b617318655057264e28bc0b6fb378c8ef146be00",
            ),
            (
                b"Jefe".to_vec(),
                b"what do ya want for nothing?".to_vec(),
                "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79",
            ),
            (
                std::vec![0xaa; 20],
                std::vec![0xdd; 50],
                "125d7342b9ac11cd91a39af48aa17b4f63f175d3",
            ),
            (
                unhex("0102030405060708090a0b0c0d0e0f10111213141516171819"),
                std::vec![0xcd; 50],
                "4c9007f4026250c6bc8414f9bf50c86c2d7235da",
            ),
            (
                std::vec![0x0c; 20],
                b"Test With Truncation".to_vec(),
                "4c1a03424b55e07fe7f27be1d58bb9324a9a5a04",
            ),
            (
                std::vec![0xaa; 80],
                b"Test Using Larger Than Block-Size Key - Hash Key First".to_vec(),
                "aa4ae5e15272d00e95705637ce8a3b55ed402112",
            ),
            (
                std::vec![0xaa; 80],
                b"Test Using Larger Than Block-Size Key and Larger Than One Block-Size Data"
                    .to_vec(),
                "e8e99d0f45237d786d6bbaa7965c7808bbff1a91",
            ),
        ];
        for (key, text, want) in cases {
            assert_eq!(hex(&hmac(&key, &text)), want);
        }
    }

    /// The rounds are HMACs in a chain: one, then each of the last.
    #[test]
    fn sha1crypt_chains_hmacs() {
        let first = b"saltsalt$sha1$3";
        let mut d = hmac(b"pw", first);
        assert_eq!(sha1crypt(b"pw", first, 1), d);
        assert_eq!(sha1crypt(b"pw", first, 0), d);
        d = hmac(b"pw", &d);
        d = hmac(b"pw", &d);
        assert_eq!(sha1crypt(b"pw", first, 3), d);
    }
}
