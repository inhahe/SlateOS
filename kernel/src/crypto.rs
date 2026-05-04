//! Cryptographic primitives for the kernel.
//!
//! Provides:
//! - **SHA-256** for file content hashing and integrity verification
//! - **CRC32C** (Castagnoli) for ext4 metadata checksums
//!
//! All implementations are pure Rust, no_std compatible, and correct.
//! Not optimized for speed (no SIMD/SHA-NI/CRC32 instructions) but
//! suitable for integrity checking at filesystem-operation frequency.
//!
//! ## References
//!
//! - SHA-256: FIPS 180-4 (Secure Hash Standard)
//! - CRC32C: RFC 3720 appendix B (iSCSI), polynomial 0x1EDC6F41

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// SHA-256 constants
// ---------------------------------------------------------------------------

/// SHA-256 initial hash values (first 32 bits of fractional parts of
/// the square roots of the first 8 primes).
const H0: [u32; 8] = [
    0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a,
    0x510e_527f, 0x9b05_688c, 0x1f83_d9ab, 0x5be0_cd19,
];

/// SHA-256 round constants (first 32 bits of fractional parts of
/// the cube roots of the first 64 primes).
const K: [u32; 64] = [
    0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5,
    0x3956_c25b, 0x59f1_11f1, 0x923f_82a4, 0xab1c_5ed5,
    0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3,
    0x72be_5d74, 0x80de_b1fe, 0x9bdc_06a7, 0xc19b_f174,
    0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc,
    0x2de9_2c6f, 0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da,
    0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
    0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967,
    0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc, 0x5338_0d13,
    0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85,
    0xa2bf_e8a1, 0xa81a_664b, 0xc24b_8b70, 0xc76c_51a3,
    0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070,
    0x19a4_c116, 0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5,
    0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
    0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208,
    0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7, 0xc671_78f2,
];

/// SHA-256 output size in bytes.
pub const SHA256_DIGEST_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// SHA-256 implementation
// ---------------------------------------------------------------------------

/// SHA-256 hasher.
///
/// Create with [`Sha256::new()`], feed data with [`update()`](Sha256::update),
/// and finalize with [`finalize()`](Sha256::finalize).
pub struct Sha256 {
    /// Current hash state.
    h: [u32; 8],
    /// Partial block buffer.
    buffer: [u8; 64],
    /// Number of bytes in the buffer.
    buf_len: usize,
    /// Total bytes processed.
    total_len: u64,
}

impl Sha256 {
    /// Create a new SHA-256 hasher.
    pub fn new() -> Self {
        Self {
            h: H0,
            buffer: [0u8; 64],
            buf_len: 0,
            total_len: 0,
        }
    }

    /// Feed data into the hasher.
    pub fn update(&mut self, data: &[u8]) {
        let mut offset = 0usize;

        // If we have a partial block, try to fill it.
        if self.buf_len > 0 {
            let needed = 64usize.saturating_sub(self.buf_len);
            let copy_len = needed.min(data.len());
            if let (Some(dest), Some(src)) = (
                self.buffer.get_mut(self.buf_len..self.buf_len + copy_len),
                data.get(..copy_len),
            ) {
                dest.copy_from_slice(src);
            }
            self.buf_len += copy_len;
            offset = copy_len;

            if self.buf_len == 64 {
                let block = self.buffer;
                compress(&mut self.h, &block);
                self.buf_len = 0;
            }
        }

        // Process full blocks directly from input.
        while offset + 64 <= data.len() {
            if let Some(block_data) = data.get(offset..offset + 64) {
                let mut block = [0u8; 64];
                block.copy_from_slice(block_data);
                compress(&mut self.h, &block);
            }
            offset += 64;
        }

        // Buffer remaining bytes.
        let remaining = data.len().saturating_sub(offset);
        if remaining > 0 {
            if let (Some(dest), Some(src)) = (
                self.buffer.get_mut(..remaining),
                data.get(offset..),
            ) {
                dest.copy_from_slice(src);
            }
            self.buf_len = remaining;
        }

        self.total_len = self.total_len.wrapping_add(data.len() as u64);
    }

    /// Finalize the hash and return the 32-byte digest.
    pub fn finalize(mut self) -> [u8; SHA256_DIGEST_SIZE] {
        // Pad the message.
        let total_bits = self.total_len.wrapping_mul(8);

        // Append 0x80 byte.
        if let Some(b) = self.buffer.get_mut(self.buf_len) {
            *b = 0x80;
        }
        self.buf_len += 1;

        // If the buffer is too full for the length field, compress and start a new block.
        if self.buf_len > 56 {
            // Zero-fill the rest of this block.
            if let Some(tail) = self.buffer.get_mut(self.buf_len..64) {
                for b in tail {
                    *b = 0;
                }
            }
            let block = self.buffer;
            compress(&mut self.h, &block);
            self.buffer = [0u8; 64];
            self.buf_len = 0;
        }

        // Zero-fill up to the length field.
        if let Some(tail) = self.buffer.get_mut(self.buf_len..56) {
            for b in tail {
                *b = 0;
            }
        }

        // Append total length in bits (big-endian, 8 bytes).
        let len_bytes = total_bits.to_be_bytes();
        if let Some(dest) = self.buffer.get_mut(56..64) {
            dest.copy_from_slice(&len_bytes);
        }

        let block = self.buffer;
        compress(&mut self.h, &block);

        // Convert hash state to bytes (big-endian).
        let mut digest = [0u8; 32];
        for (i, &word) in self.h.iter().enumerate() {
            let bytes = word.to_be_bytes();
            let offset = i * 4;
            if let Some(dest) = digest.get_mut(offset..offset + 4) {
                dest.copy_from_slice(&bytes);
            }
        }

        digest
    }
}

/// Convenience function: compute SHA-256 of a byte slice.
pub fn sha256(data: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize()
}

/// Convenience function: compute SHA-256 and return as a Vec<u8>.
pub fn sha256_vec(data: &[u8]) -> Vec<u8> {
    sha256(data).to_vec()
}

// ---------------------------------------------------------------------------
// SHA-256 compression function
// ---------------------------------------------------------------------------

/// Process a single 64-byte block.
#[allow(clippy::many_single_char_names)]
fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    // Prepare the message schedule (W).
    let mut w = [0u32; 64];

    // First 16 words: big-endian decode of the block.
    for i in 0..16 {
        let offset = i * 4;
        w[i] = u32::from_be_bytes([
            block[offset],
            block[offset + 1],
            block[offset + 2],
            block[offset + 3],
        ]);
    }

    // Extend to 64 words.
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7)
            ^ w[i - 15].rotate_right(18)
            ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17)
            ^ w[i - 2].rotate_right(19)
            ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    // Initialize working variables.
    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];

    // 64 rounds of compression.
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h.wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    // Add the compressed chunk's hash to the current state.
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Verify SHA-256 against known test vectors.
pub fn self_test() -> crate::error::KernelResult<()> {
    crate::serial_println!("[crypto] Running SHA-256 self-test...");

    // Test vector 1: empty string.
    // Expected: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
    let hash = sha256(b"");
    let expected: [u8; 32] = [
        0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14,
        0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
        0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c,
        0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
    ];
    if hash != expected {
        crate::serial_println!("[crypto]   FAIL: empty string hash mismatch");
        return Err(crate::error::KernelError::InternalError);
    }
    crate::serial_println!("[crypto]   SHA-256(\"\") = correct");

    // Test vector 2: "abc"
    // Expected: ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
    let hash = sha256(b"abc");
    let expected: [u8; 32] = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea,
        0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23,
        0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c,
        0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
    ];
    if hash != expected {
        crate::serial_println!("[crypto]   FAIL: 'abc' hash mismatch");
        return Err(crate::error::KernelError::InternalError);
    }
    crate::serial_println!("[crypto]   SHA-256(\"abc\") = correct");

    // Test vector 3: longer message (exactly 56 bytes — padding edge case).
    // "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
    // Expected: 248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1
    let hash = sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
    let expected: [u8; 32] = [
        0x24, 0x8d, 0x6a, 0x61, 0xd2, 0x06, 0x38, 0xb8,
        0xe5, 0xc0, 0x26, 0x93, 0x0c, 0x3e, 0x60, 0x39,
        0xa3, 0x3c, 0xe4, 0x59, 0x64, 0xff, 0x21, 0x67,
        0xf6, 0xec, 0xed, 0xd4, 0x19, 0xdb, 0x06, 0xc1,
    ];
    if hash != expected {
        crate::serial_println!("[crypto]   FAIL: 56-byte message hash mismatch");
        return Err(crate::error::KernelError::InternalError);
    }
    crate::serial_println!("[crypto]   SHA-256(56-byte msg) = correct");

    // Test incremental update (same as vector 2, but fed in two parts).
    let mut hasher = Sha256::new();
    hasher.update(b"ab");
    hasher.update(b"c");
    let hash = hasher.finalize();
    let expected: [u8; 32] = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea,
        0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23,
        0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c,
        0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
    ];
    if hash != expected {
        crate::serial_println!("[crypto]   FAIL: incremental update mismatch");
        return Err(crate::error::KernelError::InternalError);
    }
    crate::serial_println!("[crypto]   Incremental update = correct");

    crate::serial_println!("[crypto] SHA-256 self-test passed.");
    Ok(())
}

// ---------------------------------------------------------------------------
// CRC32C (Castagnoli) implementation
// ---------------------------------------------------------------------------

/// CRC32C lookup table, pre-computed from the Castagnoli polynomial
/// 0x82F63B78 (bit-reversed form of 0x1EDC6F41).
///
/// Generated at compile time.  Each entry is the CRC of the byte index
/// value processed through 8 rounds of the bit-at-a-time algorithm.
const CRC32C_TABLE: [u32; 256] = {
    // Castagnoli polynomial (bit-reversed).
    const POLY: u32 = 0x82F6_3B78;
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut bit = 0;
        while bit < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ POLY;
            } else {
                crc >>= 1;
            }
            bit += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
};

/// Compute CRC32C (Castagnoli) over a byte slice.
///
/// Uses the standard bit-reflected table-driven algorithm.  The initial
/// value is `!0` (all ones) and the final value is inverted.
///
/// This is the algorithm used by ext4 metadata checksums (`metadata_csum`
/// feature).
///
/// # Examples
///
/// ```
/// assert_eq!(crc32c(b"123456789"), 0xE3069283);
/// ```
pub fn crc32c(data: &[u8]) -> u32 {
    crc32c_seed(!0u32, data)
}

/// Compute CRC32C with a custom initial seed value.
///
/// The caller provides the seed (typically `!0` or a previous CRC32C value).
/// Result is XORed with `!0` on return.
///
/// ext4 uses this to chain checksums: e.g., the superblock checksum is
/// `crc32c(crc32c(~0, superblock_uuid), superblock_bytes_excluding_checksum)`.
pub fn crc32c_seed(seed: u32, data: &[u8]) -> u32 {
    let mut crc = seed;
    for &byte in data {
        let idx = ((crc ^ u32::from(byte)) & 0xFF) as usize;
        crc = CRC32C_TABLE[idx] ^ (crc >> 8);
    }
    crc ^ !0u32
}

/// Compute CRC32C without final inversion.
///
/// Returns the raw CRC accumulator (not XORed with `!0`).  This is
/// useful when chaining multiple CRC32C computations, as ext4 does
/// when computing metadata checksums with a UUID-derived seed.
///
/// Usage pattern for ext4:
/// ```ignore
/// let seed = crc32c_raw(!0, &uuid);          // raw accumulator
/// let final_crc = crc32c_seed(seed, &data);  // final with inversion
/// ```
pub fn crc32c_raw(seed: u32, data: &[u8]) -> u32 {
    let mut crc = seed;
    for &byte in data {
        let idx = ((crc ^ u32::from(byte)) & 0xFF) as usize;
        crc = CRC32C_TABLE[idx] ^ (crc >> 8);
    }
    crc
}

/// Self-test for CRC32C.
pub fn self_test_crc32c() -> Result<(), crate::error::KernelError> {
    crate::serial_println!("[crypto] Running CRC32C self-test...");

    // Test vector 1: standard check value for "123456789".
    // The CRC32C of the ASCII string "123456789" is 0xE3069283.
    let check = crc32c(b"123456789");
    if check != 0xE306_9283 {
        crate::serial_println!("[crypto]   FAIL: CRC32C(\"123456789\") = {:#010X}, expected 0xE3069283", check);
        return Err(crate::error::KernelError::InternalError);
    }
    crate::serial_println!("[crypto]   CRC32C(\"123456789\") = {:#010X} (correct)", check);

    // Test vector 2: empty input.
    let empty = crc32c(b"");
    if empty != 0x0000_0000 {
        crate::serial_println!("[crypto]   FAIL: CRC32C(\"\") = {:#010X}, expected 0x00000000", empty);
        return Err(crate::error::KernelError::InternalError);
    }
    crate::serial_println!("[crypto]   CRC32C(\"\") = {:#010X} (correct)", empty);

    // Test vector 3: 32 zero bytes.
    // Reference value verified with Python CRC32C (Castagnoli, poly 0x82F63B78).
    // Previous expected value 0xAA36918A was byte-swapped — our implementation
    // was correct all along.
    let zeros = [0u8; 32];
    let z_crc = crc32c(&zeros);
    if z_crc != 0x8A91_36AA {
        crate::serial_println!("[crypto]   FAIL: CRC32C(32 zeros) = {:#010X}, expected 0x8A9136AA", z_crc);
        return Err(crate::error::KernelError::InternalError);
    }
    crate::serial_println!("[crypto]   CRC32C(32 zeros) = {:#010X} (correct)", z_crc);

    // Test vector 4: chaining (incremental computation).
    let raw_seed = crc32c_raw(!0u32, b"1234");
    let chained = crc32c_seed(raw_seed, b"56789");
    if chained != 0xE306_9283 {
        crate::serial_println!("[crypto]   FAIL: chained CRC32C = {:#010X}, expected 0xE3069283", chained);
        return Err(crate::error::KernelError::InternalError);
    }
    crate::serial_println!("[crypto]   Chained CRC32C = {:#010X} (correct)", chained);

    crate::serial_println!("[crypto] CRC32C self-test passed.");
    Ok(())
}
