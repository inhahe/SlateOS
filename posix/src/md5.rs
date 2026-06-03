//! MD5 message digest (`RFC 1321`) — minimal `no_std` implementation.
//!
//! Used only by the legacy MD5 crypt method (`$1$`) in [`crate::crypt`].
//! MD5 is cryptographically broken and must **never** be used for new
//! security purposes; it is provided solely so the OS can verify (and
//! interoperate with) `$1$` entries in legacy `/etc/shadow` files.
//!
//! Unlike SHA-2, MD5 is little-endian throughout.  All additions use
//! `wrapping_add` because MD5 is defined over 32-bit modular arithmetic.

#![allow(clippy::arithmetic_side_effects)] // MD5 is modular arithmetic; wrapping is intended.
#![allow(clippy::indexing_slicing)] // Compile-time-bounded indices into fixed arrays.

/// Per-round left-rotation amounts.
const S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, // round 1
    5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, // round 2
    4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, // round 3
    6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, // round 4
];

/// Round constants: `K[i] = floor(2^32 * abs(sin(i + 1)))`.
const K: [u32; 64] = [
    0xd76a_a478,
    0xe8c7_b756,
    0x2420_70db,
    0xc1bd_ceee,
    0xf57c_0faf,
    0x4787_c62a,
    0xa830_4613,
    0xfd46_9501,
    0x6980_98d8,
    0x8b44_f7af,
    0xffff_5bb1,
    0x895c_d7be,
    0x6b90_1122,
    0xfd98_7193,
    0xa679_438e,
    0x49b4_0821,
    0xf61e_2562,
    0xc040_b340,
    0x265e_5a51,
    0xe9b6_c7aa,
    0xd62f_105d,
    0x0244_1453,
    0xd8a1_e681,
    0xe7d3_fbc8,
    0x21e1_cde6,
    0xc337_07d6,
    0xf4d5_0d87,
    0x455a_14ed,
    0xa9e3_e905,
    0xfcef_a3f8,
    0x676f_02d9,
    0x8d2a_4c8a,
    0xfffa_3942,
    0x8771_f681,
    0x6d9d_6122,
    0xfde5_380c,
    0xa4be_ea44,
    0x4bde_cfa9,
    0xf6bb_4b60,
    0xbebf_bc70,
    0x289b_7ec6,
    0xeaa1_27fa,
    0xd4ef_3085,
    0x0488_1d05,
    0xd9d4_d039,
    0xe6db_99e5,
    0x1fa2_7cf8,
    0xc4ac_5665,
    0xf429_2244,
    0x432a_ff97,
    0xab94_23a7,
    0xfc93_a039,
    0x655b_59c3,
    0x8f0c_cc92,
    0xffef_f47d,
    0x8584_5dd1,
    0x6fa8_7e4f,
    0xfe2c_e6e0,
    0xa301_4314,
    0x4e08_11a1,
    0xf753_7e82,
    0xbd3a_f235,
    0x2ad7_d2bb,
    0xeb86_d391,
];

const MD5_INIT: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];

/// MD5 streaming hasher.
pub struct Md5 {
    state: [u32; 4],
    total_len: u64,
    block: [u8; 64],
    block_len: usize,
}

impl Md5 {
    /// Output length in bytes.
    pub const OUTPUT_LEN: usize = 16;

    /// Create a fresh MD5 hasher.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: MD5_INIT,
            total_len: 0,
            block: [0u8; 64],
            block_len: 0,
        }
    }

    fn compress(&mut self) {
        let mut m = [0u32; 16];
        for (i, word) in m.iter_mut().enumerate() {
            let j = i * 4;
            *word = u32::from_le_bytes([
                self.block[j],
                self.block[j + 1],
                self.block[j + 2],
                self.block[j + 3],
            ]);
        }

        let [mut a, mut b, mut c, mut d] = self.state;
        for i in 0..64 {
            let (f, g) = if i < 16 {
                ((b & c) | (!b & d), i)
            } else if i < 32 {
                ((d & b) | (!d & c), (5 * i + 1) % 16)
            } else if i < 48 {
                (b ^ c ^ d, (3 * i + 5) % 16)
            } else {
                (c ^ (b | !d), (7 * i) % 16)
            };
            let f = f.wrapping_add(a).wrapping_add(K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
    }

    /// Feed `data` into the hash state.
    pub fn update(&mut self, mut data: &[u8]) {
        self.total_len = self.total_len.wrapping_add(data.len() as u64);
        while !data.is_empty() {
            let space = 64 - self.block_len;
            let take = core::cmp::min(space, data.len());
            self.block[self.block_len..self.block_len + take].copy_from_slice(&data[..take]);
            self.block_len += take;
            data = &data[take..];
            if self.block_len == 64 {
                self.compress();
                self.block_len = 0;
            }
        }
    }

    /// Consume the hasher and return the 16-byte digest.
    #[must_use]
    pub fn finalize(mut self) -> [u8; 16] {
        let bit_len = self.total_len.wrapping_mul(8);
        self.update(&[0x80]);
        while self.block_len != 56 {
            self.update(&[0x00]);
        }
        self.update(&bit_len.to_le_bytes());
        debug_assert_eq!(self.block_len, 0);
        let mut out = [0u8; 16];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        out
    }
}

impl Default for Md5 {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests — RFC 1321 test suite.
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn md5(data: &[u8]) -> [u8; 16] {
        let mut h = Md5::new();
        h.update(data);
        h.finalize()
    }

    fn hex(bytes: &[u8]) -> std::string::String {
        use std::fmt::Write as _;
        let mut s = std::string::String::new();
        for b in bytes {
            let _ = write!(s, "{b:02x}");
        }
        s
    }

    #[test]
    fn rfc1321_empty() {
        assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn rfc1321_a() {
        assert_eq!(hex(&md5(b"a")), "0cc175b9c0f1b6a831c399e269772661");
    }

    #[test]
    fn rfc1321_abc() {
        assert_eq!(hex(&md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn rfc1321_message_digest() {
        assert_eq!(
            hex(&md5(b"message digest")),
            "f96b697d7cb7938d525a2f31aaf161d0"
        );
    }

    #[test]
    fn rfc1321_alphabet() {
        assert_eq!(
            hex(&md5(b"abcdefghijklmnopqrstuvwxyz")),
            "c3fcd3d76192e4007dfb496cca67e13b"
        );
    }

    #[test]
    fn rfc1321_alnum() {
        assert_eq!(
            hex(&md5(
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
            )),
            "d174ab98d277d9f5a5611c2c9f419d9f"
        );
    }

    #[test]
    fn rfc1321_eight_numbers() {
        assert_eq!(
            hex(&md5(
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            )),
            "57edf4a22be3c955ac49da2e2107b67a"
        );
    }

    #[test]
    fn streaming_matches_oneshot() {
        let data = b"The quick brown fox jumps over the lazy dog";
        assert_eq!(hex(&md5(data)), "9e107d9d372bb6826bd81d3542a419d6");
        let mut h = Md5::new();
        for &byte in data {
            h.update(&[byte]);
        }
        assert_eq!(h.finalize(), md5(data));
    }

    #[test]
    fn multi_block() {
        // 1000 'a's spanning many blocks.
        let mut h = Md5::new();
        let chunk = [b'a'; 100];
        for _ in 0..10 {
            h.update(&chunk);
        }
        let oneshot = md5(&[b'a'; 1000]);
        assert_eq!(h.finalize(), oneshot);
    }
}
