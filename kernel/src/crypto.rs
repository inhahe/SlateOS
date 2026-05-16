//! Cryptographic primitives for the kernel.
//!
//! Provides:
//! - **SHA-256** for file content hashing and integrity verification
//! - **CRC32C** (Castagnoli) for ext4 metadata checksums
//! - **HMAC-SHA256** (RFC 2104) for keyed message authentication
//! - **HKDF-SHA256** (RFC 5869) for key derivation (TLS 1.3 key schedule)
//! - **ChaCha20** (RFC 8439) stream cipher
//! - **Poly1305** (RFC 8439) one-time authenticator
//! - **ChaCha20-Poly1305** (RFC 8439) AEAD construction (TLS 1.3 cipher)
//! - **X25519** (RFC 7748) Diffie-Hellman key exchange over Curve25519
//!
//! All implementations are pure Rust, no_std compatible, and correct.
//! Not optimized for speed (no SIMD/SHA-NI/CRC32 instructions) but
//! suitable for integrity checking at filesystem-operation frequency.
//! The ChaCha20 and Poly1305 implementations are constant-time by
//! design (no data-dependent branches or table lookups).
//!
//! ## References
//!
//! - SHA-256: FIPS 180-4 (Secure Hash Standard)
//! - CRC32C: RFC 3720 appendix B (iSCSI), polynomial 0x1EDC6F41
//! - HMAC: RFC 2104 (HMAC: Keyed-Hashing for Message Authentication)
//! - HKDF: RFC 5869 (HMAC-based Extract-and-Expand Key Derivation)
//! - ChaCha20-Poly1305: RFC 8439 (formerly RFC 7539)
//! - X25519: RFC 7748 (Elliptic Curves for Security)

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

// ===========================================================================
// HMAC-SHA256 (RFC 2104)
// ===========================================================================

/// HMAC block size (SHA-256 uses 64-byte blocks internally).
const HMAC_BLOCK_SIZE: usize = 64;

/// Compute HMAC-SHA256.
///
/// `key` may be any length — keys longer than 64 bytes are hashed first,
/// keys shorter are zero-padded.  Returns a 32-byte tag.
///
/// # References
///
/// RFC 2104 §2: HMAC(K, m) = H((K' ⊕ opad) || H((K' ⊕ ipad) || m))
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
    // Step 1: Normalize key to exactly HMAC_BLOCK_SIZE bytes.
    let mut k_prime = [0u8; HMAC_BLOCK_SIZE];
    if key.len() > HMAC_BLOCK_SIZE {
        // Keys longer than block size are hashed.
        let hashed = sha256(key);
        k_prime[..SHA256_DIGEST_SIZE].copy_from_slice(&hashed);
    } else {
        // Keys shorter are zero-padded.
        k_prime[..key.len()].copy_from_slice(key);
    }

    // Step 2: Inner hash = H((K' ⊕ ipad) || message)
    let mut ipad_key = [0x36u8; HMAC_BLOCK_SIZE];
    for i in 0..HMAC_BLOCK_SIZE {
        ipad_key[i] ^= k_prime[i];
    }
    let mut inner = Sha256::new();
    inner.update(&ipad_key);
    inner.update(message);
    let inner_hash = inner.finalize();

    // Step 3: Outer hash = H((K' ⊕ opad) || inner_hash)
    let mut opad_key = [0x5Cu8; HMAC_BLOCK_SIZE];
    for i in 0..HMAC_BLOCK_SIZE {
        opad_key[i] ^= k_prime[i];
    }
    let mut outer = Sha256::new();
    outer.update(&opad_key);
    outer.update(&inner_hash);
    outer.finalize()
}

// ===========================================================================
// HKDF-SHA256 (RFC 5869)
// ===========================================================================

/// HKDF-Extract: derive a pseudorandom key from input keying material.
///
/// PRK = HMAC-Hash(salt, IKM)
///
/// If `salt` is empty, uses a string of SHA256_DIGEST_SIZE zero bytes
/// (per RFC 5869 §2.2).
pub fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
    let effective_salt = if salt.is_empty() {
        &[0u8; SHA256_DIGEST_SIZE] as &[u8]
    } else {
        salt
    };
    hmac_sha256(effective_salt, ikm)
}

/// HKDF-Expand: expand a pseudorandom key to the desired length.
///
/// Returns up to 255 * 32 = 8160 bytes of output keying material.
/// `info` is the context/application-specific info string.
///
/// OKM = T(1) || T(2) || ... where T(i) = HMAC-Hash(PRK, T(i-1) || info || i)
pub fn hkdf_expand(prk: &[u8; SHA256_DIGEST_SIZE], info: &[u8], len: usize) -> Vec<u8> {
    // Maximum output per RFC 5869 §2.3: 255 * HashLen.
    let max_len = 255 * SHA256_DIGEST_SIZE;
    let out_len = len.min(max_len);

    let mut okm = Vec::with_capacity(out_len);
    let t_prev = [0u8; 0]; // T(0) is empty.
    let mut t_buf = [0u8; SHA256_DIGEST_SIZE];
    let mut counter = 1u8;

    while okm.len() < out_len {
        // T(i) = HMAC-Hash(PRK, T(i-1) || info || i)
        let mut hmac_input = Vec::with_capacity(SHA256_DIGEST_SIZE + info.len() + 1);
        if counter > 1 {
            hmac_input.extend_from_slice(&t_buf);
        } else {
            hmac_input.extend_from_slice(&t_prev);
        }
        hmac_input.extend_from_slice(info);
        hmac_input.push(counter);

        t_buf = hmac_sha256(prk, &hmac_input);

        let needed = out_len.saturating_sub(okm.len());
        let take = needed.min(SHA256_DIGEST_SIZE);
        okm.extend_from_slice(&t_buf[..take]);

        counter = counter.saturating_add(1);
    }

    okm
}

/// HKDF one-shot: extract then expand.
///
/// Convenience wrapper for the common case of deriving a fixed-length
/// key from an input keying material.
#[allow(dead_code)]
pub fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    let prk = hkdf_extract(salt, ikm);
    hkdf_expand(&prk, info, len)
}

// ===========================================================================
// ChaCha20 stream cipher (RFC 8439 §2.3-2.4)
// ===========================================================================

/// ChaCha20 quarter-round operation.
///
/// Operates on 4 words of the ChaCha state.  This is the core primitive
/// from which the entire cipher is built.  All operations are add-xor-rotate
/// — no table lookups, inherently constant-time.
#[inline]
fn quarter_round(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(16);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(12);

    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(8);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(7);
}

/// Produce one 64-byte ChaCha20 keystream block.
///
/// `key`: 256-bit (32-byte) key.
/// `nonce`: 96-bit (12-byte) nonce.
/// `counter`: 32-bit block counter (starts at 0 or 1 depending on usage).
///
/// Returns 64 bytes of keystream.
fn chacha20_block(key: &[u8; 32], nonce: &[u8; 12], counter: u32) -> [u8; 64] {
    // Initialize state: "expand 32-byte k" constant + key + counter + nonce.
    let mut state = [0u32; 16];
    state[0] = 0x6170_7865; // "expa"
    state[1] = 0x3320_646e; // "nd 3"
    state[2] = 0x7962_2d32; // "2-by"
    state[3] = 0x6b20_6574; // "te k"

    // Key (8 words, little-endian).
    for i in 0..8 {
        let off = i * 4;
        state[4 + i] = u32::from_le_bytes([key[off], key[off + 1], key[off + 2], key[off + 3]]);
    }

    // Counter.
    state[12] = counter;

    // Nonce (3 words, little-endian).
    for i in 0..3 {
        let off = i * 4;
        state[13 + i] = u32::from_le_bytes([nonce[off], nonce[off + 1], nonce[off + 2], nonce[off + 3]]);
    }

    // Save initial state for final addition.
    let initial = state;

    // 20 rounds (10 double-rounds of column + diagonal quarter-rounds).
    for _ in 0..10 {
        // Column rounds.
        quarter_round(&mut state, 0, 4,  8, 12);
        quarter_round(&mut state, 1, 5,  9, 13);
        quarter_round(&mut state, 2, 6, 10, 14);
        quarter_round(&mut state, 3, 7, 11, 15);
        // Diagonal rounds.
        quarter_round(&mut state, 0, 5, 10, 15);
        quarter_round(&mut state, 1, 6, 11, 12);
        quarter_round(&mut state, 2, 7,  8, 13);
        quarter_round(&mut state, 3, 4,  9, 14);
    }

    // Add initial state (modular addition per word).
    for i in 0..16 {
        state[i] = state[i].wrapping_add(initial[i]);
    }

    // Serialize to little-endian bytes.
    let mut out = [0u8; 64];
    for i in 0..16 {
        let bytes = state[i].to_le_bytes();
        out[i * 4]     = bytes[0];
        out[i * 4 + 1] = bytes[1];
        out[i * 4 + 2] = bytes[2];
        out[i * 4 + 3] = bytes[3];
    }
    out
}

/// Encrypt or decrypt data using ChaCha20.
///
/// XORs the plaintext/ciphertext with the ChaCha20 keystream starting
/// at `counter`.  The same function handles both encryption and decryption
/// since ChaCha20 is XOR-based.
///
/// Modifies `data` in-place.
pub fn chacha20_xor(key: &[u8; 32], nonce: &[u8; 12], counter: u32, data: &mut [u8]) {
    let mut block_counter = counter;
    let mut offset = 0usize;

    while offset < data.len() {
        let keystream = chacha20_block(key, nonce, block_counter);
        let remaining = data.len().saturating_sub(offset);
        let take = remaining.min(64);

        for i in 0..take {
            data[offset + i] ^= keystream[i];
        }

        offset = offset.saturating_add(64);
        block_counter = block_counter.wrapping_add(1);
    }
}

// ===========================================================================
// Poly1305 MAC (RFC 8439 §2.5)
// ===========================================================================

/// Poly1305 one-time authenticator.
///
/// Computes a 16-byte tag over `message` using a 32-byte one-time key.
/// The key MUST NOT be reused — Poly1305 security breaks completely
/// with key reuse.
///
/// Uses 130-bit arithmetic via u64 limbs to avoid big-number libraries.
/// The field is GF(2^130 - 5), which allows fast reduction via
/// multiplication by 5.
pub fn poly1305(key: &[u8; 32], message: &[u8]) -> [u8; 16] {
    // Split key into (r, s).  r is clamped per RFC 8439 §2.5.1.
    let mut r = [0u32; 5]; // 5 × 26-bit limbs = 130 bits.
    let s = [
        u32::from_le_bytes([key[16], key[17], key[18], key[19]]),
        u32::from_le_bytes([key[20], key[21], key[22], key[23]]),
        u32::from_le_bytes([key[24], key[25], key[26], key[27]]),
        u32::from_le_bytes([key[28], key[29], key[30], key[31]]),
    ];

    // Decode r into 26-bit limbs and apply clamp mask.
    // r[0] = r_bytes[0..4] & 0x0FFF_FFFC
    // r[1] = r_bytes[3..7] >> 2 & 0x0FFF_FFC0 ... etc.
    // Use the simpler approach: load as u128, split into 26-bit limbs, clamp.
    let t0 = u32::from_le_bytes([key[0],  key[1],  key[2],  key[3]]);
    let t1 = u32::from_le_bytes([key[4],  key[5],  key[6],  key[7]]);
    let t2 = u32::from_le_bytes([key[8],  key[9],  key[10], key[11]]);
    let t3 = u32::from_le_bytes([key[12], key[13], key[14], key[15]]);

    r[0] =  t0                       & 0x03FF_FFFF;
    r[1] = ((t0 >> 26) | (t1 << 6)) & 0x03FF_FF03;
    r[2] = ((t1 >> 20) | (t2 << 12))& 0x03FF_C0FF;
    r[3] = ((t2 >> 14) | (t3 << 18))& 0x03F0_3FFF;
    r[4] =  (t3 >> 8)               & 0x000F_FFFF;

    // Accumulator (5 × 26-bit limbs, up to 131 bits during computation).
    let mut h = [0u32; 5];

    // Pre-compute r * 5 for reduction.
    let r_5 = [0u32, r[1].wrapping_mul(5), r[2].wrapping_mul(5), r[3].wrapping_mul(5), r[4].wrapping_mul(5)];

    // Process message in 16-byte blocks.
    let mut i = 0usize;
    while i < message.len() {
        let remaining = message.len().saturating_sub(i);
        let block_len = remaining.min(16);

        // Read block into a 17-byte buffer (add high bit).
        let mut buf = [0u8; 17];
        buf[..block_len].copy_from_slice(&message[i..i + block_len]);
        buf[block_len] = 1; // High bit for complete blocks.

        // Add block to accumulator (as 26-bit limbs).
        let bt0 = u32::from_le_bytes([buf[0],  buf[1],  buf[2],  buf[3]]);
        let bt1 = u32::from_le_bytes([buf[4],  buf[5],  buf[6],  buf[7]]);
        let bt2 = u32::from_le_bytes([buf[8],  buf[9],  buf[10], buf[11]]);
        let bt3 = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
        let bt4 = buf[16] as u32;

        h[0] = h[0].wrapping_add(bt0        & 0x03FF_FFFF);
        h[1] = h[1].wrapping_add(((bt0 >> 26) | (bt1 << 6)) & 0x03FF_FFFF);
        h[2] = h[2].wrapping_add(((bt1 >> 20) | (bt2 << 12))& 0x03FF_FFFF);
        h[3] = h[3].wrapping_add(((bt2 >> 14) | (bt3 << 18))& 0x03FF_FFFF);
        h[4] = h[4].wrapping_add((bt3 >> 8) | (bt4 << 24));

        // Multiply h by r (mod 2^130 - 5).
        // d[i] = h[0]*r[i] + h[1]*r[i-1]*5 + h[2]*r[i-2]*5 + ...
        // Using the identity: x * r[j] mod (2^130-5) = x * r[j] for j >= i,
        //                      x * r[j] * 5          for j <  i.
        let mut d = [0u64; 5];
        d[0] = (h[0] as u64) * (r[0] as u64) + (h[1] as u64) * (r_5[4] as u64) + (h[2] as u64) * (r_5[3] as u64) + (h[3] as u64) * (r_5[2] as u64) + (h[4] as u64) * (r_5[1] as u64);
        d[1] = (h[0] as u64) * (r[1] as u64) + (h[1] as u64) * (r[0]  as u64)  + (h[2] as u64) * (r_5[4] as u64) + (h[3] as u64) * (r_5[3] as u64) + (h[4] as u64) * (r_5[2] as u64);
        d[2] = (h[0] as u64) * (r[2] as u64) + (h[1] as u64) * (r[1]  as u64)  + (h[2] as u64) * (r[0]   as u64) + (h[3] as u64) * (r_5[4] as u64) + (h[4] as u64) * (r_5[3] as u64);
        d[3] = (h[0] as u64) * (r[3] as u64) + (h[1] as u64) * (r[2]  as u64)  + (h[2] as u64) * (r[1]   as u64) + (h[3] as u64) * (r[0]   as u64) + (h[4] as u64) * (r_5[4] as u64);
        d[4] = (h[0] as u64) * (r[4] as u64) + (h[1] as u64) * (r[3]  as u64)  + (h[2] as u64) * (r[2]   as u64) + (h[3] as u64) * (r[1]   as u64) + (h[4] as u64) * (r[0]   as u64);

        // Carry propagation.
        let mut carry: u64;
        carry = d[0] >> 26; h[0] = (d[0] as u32) & 0x03FF_FFFF; d[1] = d[1].wrapping_add(carry);
        carry = d[1] >> 26; h[1] = (d[1] as u32) & 0x03FF_FFFF; d[2] = d[2].wrapping_add(carry);
        carry = d[2] >> 26; h[2] = (d[2] as u32) & 0x03FF_FFFF; d[3] = d[3].wrapping_add(carry);
        carry = d[3] >> 26; h[3] = (d[3] as u32) & 0x03FF_FFFF; d[4] = d[4].wrapping_add(carry);
        carry = d[4] >> 26; h[4] = (d[4] as u32) & 0x03FF_FFFF;
        // Wrap carry back to h[0] multiplied by 5 (2^130 ≡ 5 mod p).
        h[0] = h[0].wrapping_add((carry as u32).wrapping_mul(5));
        carry = (h[0] >> 26) as u64; h[0] &= 0x03FF_FFFF;
        h[1] = h[1].wrapping_add(carry as u32);

        i = i.saturating_add(16);
    }

    // Final reduction: fully reduce h mod 2^130 - 5.
    let mut carry: u32;
    carry = h[1] >> 26; h[1] &= 0x03FF_FFFF; h[2] = h[2].wrapping_add(carry);
    carry = h[2] >> 26; h[2] &= 0x03FF_FFFF; h[3] = h[3].wrapping_add(carry);
    carry = h[3] >> 26; h[3] &= 0x03FF_FFFF; h[4] = h[4].wrapping_add(carry);
    carry = h[4] >> 26; h[4] &= 0x03FF_FFFF; h[0] = h[0].wrapping_add(carry.wrapping_mul(5));
    carry = h[0] >> 26; h[0] &= 0x03FF_FFFF; h[1] = h[1].wrapping_add(carry);

    // Compute h + -(2^130-5) = h - p.  If h >= p, the subtraction doesn't
    // borrow, and we use the subtracted value.
    let mut g = [0u32; 5];
    g[0] = h[0].wrapping_add(5);
    carry = g[0] >> 26; g[0] &= 0x03FF_FFFF;
    g[1] = h[1].wrapping_add(carry); carry = g[1] >> 26; g[1] &= 0x03FF_FFFF;
    g[2] = h[2].wrapping_add(carry); carry = g[2] >> 26; g[2] &= 0x03FF_FFFF;
    g[3] = h[3].wrapping_add(carry); carry = g[3] >> 26; g[3] &= 0x03FF_FFFF;
    g[4] = h[4].wrapping_add(carry).wrapping_sub(1 << 26);

    // Select h or g using constant-time mask: if g[4] didn't underflow
    // (bit 31 clear), h >= p, so use g.
    let mask = ((g[4] >> 31).wrapping_sub(1)) & 0x03FF_FFFF; // 0x03FF_FFFF if g valid, 0 otherwise.
    let nmask = !mask & 0x03FF_FFFF;
    h[0] = (h[0] & nmask) | (g[0] & mask);
    h[1] = (h[1] & nmask) | (g[1] & mask);
    h[2] = (h[2] & nmask) | (g[2] & mask);
    h[3] = (h[3] & nmask) | (g[3] & mask);
    h[4] = (h[4] & nmask) | (g[4] & mask);

    // Reassemble into 4 × 32-bit words and add s.
    let mut f: u64;
    f = ((h[0] as u64) | ((h[1] as u64) << 26)).wrapping_add(s[0] as u64);
    let tag0 = f as u32;
    f = (f >> 32).wrapping_add(((h[1] >> 6) as u64) | ((h[2] as u64) << 20)).wrapping_add(s[1] as u64);
    let tag1 = f as u32;
    f = (f >> 32).wrapping_add(((h[2] >> 12) as u64) | ((h[3] as u64) << 14)).wrapping_add(s[2] as u64);
    let tag2 = f as u32;
    f = (f >> 32).wrapping_add(((h[3] >> 18) as u64) | ((h[4] as u64) << 8)).wrapping_add(s[3] as u64);
    let tag3 = f as u32;

    let mut tag = [0u8; 16];
    tag[0..4].copy_from_slice(&tag0.to_le_bytes());
    tag[4..8].copy_from_slice(&tag1.to_le_bytes());
    tag[8..12].copy_from_slice(&tag2.to_le_bytes());
    tag[12..16].copy_from_slice(&tag3.to_le_bytes());
    tag
}

// ===========================================================================
// ChaCha20-Poly1305 AEAD (RFC 8439 §2.8)
// ===========================================================================

/// ChaCha20-Poly1305 AEAD encryption.
///
/// Encrypts `plaintext` in-place and returns a 16-byte authentication tag.
/// `aad` is additional authenticated data (authenticated but not encrypted).
///
/// The Poly1305 one-time key is derived from ChaCha20 block 0.
/// Data encryption uses ChaCha20 starting from block counter 1.
///
/// Returns the 16-byte tag.  Caller must transmit (ciphertext, tag, nonce).
pub fn chacha20_poly1305_encrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    plaintext: &mut [u8],
) -> [u8; 16] {
    // Step 1: Generate Poly1305 one-time key from ChaCha20 block 0.
    let otk_block = chacha20_block(key, nonce, 0);
    let mut otk = [0u8; 32];
    otk.copy_from_slice(&otk_block[..32]);

    // Step 2: Encrypt plaintext with ChaCha20 starting at counter=1.
    chacha20_xor(key, nonce, 1, plaintext);

    // Step 3: Build Poly1305 input: AAD || pad || ciphertext || pad || lengths.
    let mac_input = build_aead_mac_input(aad, plaintext);
    let tag = poly1305(&otk, &mac_input);

    tag
}

/// ChaCha20-Poly1305 AEAD decryption.
///
/// Verifies the authentication tag, then decrypts `ciphertext` in-place.
/// Returns `true` if the tag is valid (and data is decrypted), `false`
/// if authentication fails (data is NOT modified on failure).
///
/// **Constant-time tag comparison** to prevent timing attacks.
pub fn chacha20_poly1305_decrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    ciphertext: &mut [u8],
    tag: &[u8; 16],
) -> bool {
    // Step 1: Generate Poly1305 one-time key from ChaCha20 block 0.
    let otk_block = chacha20_block(key, nonce, 0);
    let mut otk = [0u8; 32];
    otk.copy_from_slice(&otk_block[..32]);

    // Step 2: Compute expected tag over AAD + ciphertext (before decryption).
    let mac_input = build_aead_mac_input(aad, ciphertext);
    let expected = poly1305(&otk, &mac_input);

    // Step 3: Constant-time tag comparison.
    let mut diff = 0u8;
    for i in 0..16 {
        diff |= expected[i] ^ tag[i];
    }
    if diff != 0 {
        return false; // Authentication failed — do NOT decrypt.
    }

    // Step 4: Decrypt (XOR with ChaCha20 keystream from counter 1).
    chacha20_xor(key, nonce, 1, ciphertext);

    true
}

/// Build the Poly1305 MAC input for AEAD (RFC 8439 §2.8).
///
/// Format: AAD || pad16(AAD) || ciphertext || pad16(CT) || len(AAD) || len(CT)
/// where pad16(x) pads x to a 16-byte boundary with zeros, and lengths
/// are 8-byte little-endian.
fn build_aead_mac_input(aad: &[u8], ct: &[u8]) -> Vec<u8> {
    let aad_pad = (16 - (aad.len() % 16)) % 16;
    let ct_pad = (16 - (ct.len() % 16)) % 16;
    let total = aad.len() + aad_pad + ct.len() + ct_pad + 16;

    let mut input = Vec::with_capacity(total);
    input.extend_from_slice(aad);
    input.extend_from_slice(&[0u8; 16][..aad_pad]);
    input.extend_from_slice(ct);
    input.extend_from_slice(&[0u8; 16][..ct_pad]);
    input.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    input.extend_from_slice(&(ct.len() as u64).to_le_bytes());
    input
}

// ===========================================================================
// Self-tests for HMAC, HKDF, ChaCha20, Poly1305, AEAD
// ===========================================================================

/// Run self-tests for all TLS-related crypto primitives.
pub fn self_test_tls_crypto() -> crate::error::KernelResult<()> {
    crate::serial_println!("[crypto] Running TLS crypto self-tests...");
    let mut passed = 0u32;

    // --- HMAC-SHA256 test vector (RFC 4231 §4.2, test case 1) ---
    {
        let key = [0x0Bu8; 20];
        let data = b"Hi There";
        let expected: [u8; 32] = [
            0xb0, 0x34, 0x4c, 0x61, 0xd8, 0xdb, 0x38, 0x53,
            0x5c, 0xa8, 0xaf, 0xce, 0xaf, 0x0b, 0xf1, 0x2b,
            0x88, 0x1d, 0xc2, 0x00, 0xc9, 0x83, 0x3d, 0xa7,
            0x26, 0xe9, 0x37, 0x6c, 0x2e, 0x32, 0xcf, 0xf7,
        ];
        let result = hmac_sha256(&key, data);
        assert!(result == expected, "HMAC-SHA256 test case 1");
        passed = passed.saturating_add(1);
        crate::serial_println!("[crypto]   HMAC-SHA256 test 1 (RFC 4231): PASSED");
    }

    // --- HMAC-SHA256 test vector (RFC 4231 §4.3, test case 2) ---
    {
        let key = b"Jefe";
        let data = b"what do ya want for nothing?";
        let expected: [u8; 32] = [
            0x5b, 0xdc, 0xc1, 0x46, 0xbf, 0x60, 0x75, 0x4e,
            0x6a, 0x04, 0x24, 0x26, 0x08, 0x95, 0x75, 0xc7,
            0x5a, 0x00, 0x3f, 0x08, 0x9d, 0x27, 0x39, 0x83,
            0x9d, 0xec, 0x58, 0xb9, 0x64, 0xec, 0x38, 0x43,
        ];
        let result = hmac_sha256(key, data);
        assert!(result == expected, "HMAC-SHA256 test case 2");
        passed = passed.saturating_add(1);
        crate::serial_println!("[crypto]   HMAC-SHA256 test 2 (RFC 4231): PASSED");
    }

    // --- HKDF-SHA256 test vector (RFC 5869 §A.1) ---
    {
        let ikm = [0x0Bu8; 22];
        let salt: [u8; 13] = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06,
                               0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c];
        let info: [u8; 10] = [0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6,
                               0xf7, 0xf8, 0xf9];
        let expected_prk: [u8; 32] = [
            0x07, 0x77, 0x09, 0x36, 0x2c, 0x2e, 0x32, 0xdf,
            0x0d, 0xdc, 0x3f, 0x0d, 0xc4, 0x7b, 0xba, 0x63,
            0x90, 0xb6, 0xc7, 0x3b, 0xb5, 0x0f, 0x9c, 0x31,
            0x22, 0xec, 0x84, 0x4a, 0xd7, 0xc2, 0xb3, 0xe5,
        ];
        let prk = hkdf_extract(&salt, &ikm);
        assert!(prk == expected_prk, "HKDF extract");

        let okm = hkdf_expand(&prk, &info, 42);
        let expected_okm: [u8; 42] = [
            0x3c, 0xb2, 0x5f, 0x25, 0xfa, 0xac, 0xd5, 0x7a,
            0x90, 0x43, 0x4f, 0x64, 0xd0, 0x36, 0x2f, 0x2a,
            0x2d, 0x2d, 0x0a, 0x90, 0xcf, 0x1a, 0x5a, 0x4c,
            0x5d, 0xb0, 0x2d, 0x56, 0xec, 0xc4, 0xc5, 0xbf,
            0x34, 0x00, 0x72, 0x08, 0xd5, 0xb8, 0x87, 0x18,
            0x58, 0x65,
        ];
        assert!(okm.len() == 42, "HKDF output length");
        assert!(&okm[..] == &expected_okm[..], "HKDF expand output");
        passed = passed.saturating_add(1);
        crate::serial_println!("[crypto]   HKDF-SHA256 (RFC 5869 A.1): PASSED");
    }

    // --- ChaCha20 test vector (RFC 8439 §2.4.2) ---
    {
        let key: [u8; 32] = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
            0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
            0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
        ];
        let nonce: [u8; 12] = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x4a,
            0x00, 0x00, 0x00, 0x00,
        ];
        // Plaintext: "Ladies and Gentlemen of the class of '99: ..."
        let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let mut data = plaintext.to_vec();
        chacha20_xor(&key, &nonce, 1, &mut data);

        // First few bytes of expected ciphertext from RFC 8439 §2.4.2.
        let expected_start: [u8; 16] = [
            0x6e, 0x2e, 0x35, 0x9a, 0x25, 0x68, 0xf9, 0x80,
            0x41, 0xba, 0x07, 0x28, 0xdd, 0x0d, 0x69, 0x81,
        ];
        assert!(&data[..16] == &expected_start[..], "ChaCha20 ciphertext start");

        // Decrypt and verify round-trip.
        chacha20_xor(&key, &nonce, 1, &mut data);
        assert!(&data[..] == &plaintext[..], "ChaCha20 round-trip");
        passed = passed.saturating_add(1);
        crate::serial_println!("[crypto]   ChaCha20 (RFC 8439 §2.4.2): PASSED");
    }

    // --- Poly1305 test vector (RFC 8439 §2.5.2) ---
    {
        let key: [u8; 32] = [
            0x85, 0xd6, 0xbe, 0x78, 0x57, 0x55, 0x6d, 0x33,
            0x7f, 0x44, 0x52, 0xfe, 0x42, 0xd5, 0x06, 0xa8,
            0x01, 0x03, 0x80, 0x8a, 0xfb, 0x0d, 0xb2, 0xfd,
            0x4a, 0xbf, 0xf6, 0xaf, 0x41, 0x49, 0xf5, 0x1b,
        ];
        let msg = b"Cryptographic Forum Research Group";
        let expected: [u8; 16] = [
            0xa8, 0x06, 0x1d, 0xc1, 0x30, 0x51, 0x36, 0xc6,
            0xc2, 0x2b, 0x8b, 0xaf, 0x0c, 0x01, 0x27, 0xa9,
        ];
        let tag = poly1305(&key, msg);
        assert!(tag == expected, "Poly1305 tag");
        passed = passed.saturating_add(1);
        crate::serial_println!("[crypto]   Poly1305 (RFC 8439 §2.5.2): PASSED");
    }

    // --- ChaCha20-Poly1305 AEAD test vector (RFC 8439 §2.8.2) ---
    {
        let key: [u8; 32] = [
            0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
            0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e, 0x8f,
            0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97,
            0x98, 0x99, 0x9a, 0x9b, 0x9c, 0x9d, 0x9e, 0x9f,
        ];
        let nonce: [u8; 12] = [
            0x07, 0x00, 0x00, 0x00, 0x40, 0x41, 0x42, 0x43,
            0x44, 0x45, 0x46, 0x47,
        ];
        let aad: [u8; 12] = [
            0x50, 0x51, 0x52, 0x53, 0xc0, 0xc1, 0xc2, 0xc3,
            0xc4, 0xc5, 0xc6, 0xc7,
        ];
        let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let expected_tag: [u8; 16] = [
            0x1a, 0xe1, 0x0b, 0x59, 0x4f, 0x09, 0xe2, 0x6a,
            0x7e, 0x90, 0x2e, 0xcb, 0xd0, 0x60, 0x06, 0x91,
        ];

        // Encrypt.
        let mut ct = plaintext.to_vec();
        let tag = chacha20_poly1305_encrypt(&key, &nonce, &aad, &mut ct);
        assert!(tag == expected_tag, "AEAD encrypt tag");

        // Decrypt and verify.
        let ok = chacha20_poly1305_decrypt(&key, &nonce, &aad, &mut ct, &tag);
        assert!(ok, "AEAD decrypt auth");
        assert!(&ct[..] == &plaintext[..], "AEAD round-trip");

        // Tamper test: flip a byte and verify decryption fails.
        let mut tampered = plaintext.to_vec();
        let tag2 = chacha20_poly1305_encrypt(&key, &nonce, &aad, &mut tampered);
        tampered[0] ^= 0xFF; // Tamper with ciphertext.
        let fail = chacha20_poly1305_decrypt(&key, &nonce, &aad, &mut tampered, &tag2);
        assert!(!fail, "AEAD tamper detection");

        passed = passed.saturating_add(1);
        crate::serial_println!("[crypto]   ChaCha20-Poly1305 AEAD (RFC 8439 §2.8.2): PASSED");
    }

    // --- X25519 test vector (RFC 7748 §6.1) ---
    {
        let alice_sk: [u8; 32] = [
            0x77, 0x07, 0x6d, 0x0a, 0x73, 0x18, 0xa5, 0x7d,
            0x3c, 0x16, 0xc1, 0x72, 0x51, 0xb2, 0x66, 0x45,
            0xdf, 0x4c, 0x2f, 0x87, 0xeb, 0xc0, 0x99, 0x2a,
            0xb1, 0x77, 0xfb, 0xa5, 0x1d, 0xb9, 0x2c, 0x2a,
        ];
        let bob_pk: [u8; 32] = [
            0xde, 0x9e, 0xdb, 0x7d, 0x7b, 0x7d, 0xc1, 0xb4,
            0xd3, 0x5b, 0x61, 0xc2, 0xec, 0xe4, 0x35, 0x37,
            0x3f, 0x83, 0x43, 0xc8, 0x5b, 0x78, 0x67, 0x4d,
            0xad, 0xfc, 0x7e, 0x14, 0x6f, 0x88, 0x2b, 0x4f,
        ];
        let expected_shared: [u8; 32] = [
            0x4a, 0x5d, 0x9d, 0x5b, 0xa4, 0xce, 0x2d, 0xe1,
            0x72, 0x8e, 0x3b, 0xf4, 0x80, 0x35, 0x0f, 0x25,
            0xe0, 0x7e, 0x21, 0xc9, 0x47, 0xd1, 0x9e, 0x33,
            0x76, 0xf0, 0x9b, 0x3c, 0x1e, 0x16, 0x17, 0x42,
        ];
        let shared = x25519(&alice_sk, &bob_pk);
        assert!(shared == expected_shared, "X25519 shared secret");
        passed = passed.saturating_add(1);
        crate::serial_println!("[crypto]   X25519 (RFC 7748 §6.1): PASSED");
    }

    // --- X25519 base point test (RFC 7748 §6.1) ---
    {
        // Alice's public key = x25519(alice_sk, basepoint)
        let alice_sk: [u8; 32] = [
            0x77, 0x07, 0x6d, 0x0a, 0x73, 0x18, 0xa5, 0x7d,
            0x3c, 0x16, 0xc1, 0x72, 0x51, 0xb2, 0x66, 0x45,
            0xdf, 0x4c, 0x2f, 0x87, 0xeb, 0xc0, 0x99, 0x2a,
            0xb1, 0x77, 0xfb, 0xa5, 0x1d, 0xb9, 0x2c, 0x2a,
        ];
        let expected_pk: [u8; 32] = [
            0x85, 0x20, 0xf0, 0x09, 0x89, 0x30, 0xa7, 0x54,
            0x74, 0x8b, 0x7d, 0xdc, 0xb4, 0x3e, 0xf7, 0x5a,
            0x0d, 0xbf, 0x3a, 0x0d, 0x26, 0x38, 0x1a, 0xf4,
            0xeb, 0xa4, 0xa9, 0x8e, 0xaa, 0x9b, 0x4e, 0x6a,
        ];
        let pk = x25519_base(&alice_sk);
        assert!(pk == expected_pk, "X25519 base point mult");
        passed = passed.saturating_add(1);
        crate::serial_println!("[crypto]   X25519 base point (RFC 7748 §6.1): PASSED");
    }

    crate::serial_println!("[crypto] All {} TLS crypto self-tests PASSED", passed);
    Ok(())
}

// ===========================================================================
// X25519 Diffie-Hellman (RFC 7748)
// ===========================================================================
//
// Field arithmetic in GF(2^255 - 19) using 5 × 51-bit limbs stored in u64.
// Products use u128 intermediates.  This is the classic "donna64" approach.
//
// All operations are constant-time: no data-dependent branches, no
// data-dependent memory access.  This prevents timing side channels.

/// A field element in GF(2^255-19), represented as 5 × 51-bit limbs.
#[derive(Clone, Copy)]
struct Fe25519([u64; 5]);

impl Fe25519 {
    const ZERO: Self = Self([0; 5]);

    const ONE: Self = Self([1, 0, 0, 0, 0]);

    /// Load from 32 bytes (little-endian), reducing mod p.
    fn from_bytes(s: &[u8; 32]) -> Self {
        let mut h = [0u64; 5];
        // Load 5 limbs at 51-bit boundaries.
        h[0] =  load_le_u64(s, 0)        & 0x7FFFFFFFFFFFF;
        h[1] = (load_le_u64(s, 6) >> 3)  & 0x7FFFFFFFFFFFF;
        h[2] = (load_le_u64(s, 12) >> 6) & 0x7FFFFFFFFFFFF;
        h[3] = (load_le_u64(s, 19) >> 1) & 0x7FFFFFFFFFFFF;
        h[4] = (load_le_u64(s, 24) >> 12)& 0x7FFFFFFFFFFFF;
        Self(h)
    }

    /// Serialize to 32 bytes (little-endian), fully reduced mod p.
    fn to_bytes(self) -> [u8; 32] {
        let mut h = self.0;
        // Full carry chain to ensure limbs are < 2^51.
        let mut carry: u64;
        carry = h[0] >> 51; h[0] &= 0x7FFFFFFFFFFFF; h[1] = h[1].wrapping_add(carry);
        carry = h[1] >> 51; h[1] &= 0x7FFFFFFFFFFFF; h[2] = h[2].wrapping_add(carry);
        carry = h[2] >> 51; h[2] &= 0x7FFFFFFFFFFFF; h[3] = h[3].wrapping_add(carry);
        carry = h[3] >> 51; h[3] &= 0x7FFFFFFFFFFFF; h[4] = h[4].wrapping_add(carry);
        carry = h[4] >> 51; h[4] &= 0x7FFFFFFFFFFFF; h[0] = h[0].wrapping_add(carry.wrapping_mul(19));
        carry = h[0] >> 51; h[0] &= 0x7FFFFFFFFFFFF; h[1] = h[1].wrapping_add(carry);

        // Conditional subtract p: if h >= p, subtract p.
        // q = (h[0] + 19) >> 51; propagate; check if h[4] overflows.
        let mut q = (h[0].wrapping_add(19)) >> 51;
        q = (h[1].wrapping_add(q)) >> 51;
        q = (h[2].wrapping_add(q)) >> 51;
        q = (h[3].wrapping_add(q)) >> 51;
        q = (h[4].wrapping_add(q)) >> 51;

        // q is 0 or 1.  If 1, h >= p, subtract p (add 19, propagate).
        h[0] = h[0].wrapping_add(q.wrapping_mul(19));
        carry = h[0] >> 51; h[0] &= 0x7FFFFFFFFFFFF;
        h[1] = h[1].wrapping_add(carry); carry = h[1] >> 51; h[1] &= 0x7FFFFFFFFFFFF;
        h[2] = h[2].wrapping_add(carry); carry = h[2] >> 51; h[2] &= 0x7FFFFFFFFFFFF;
        h[3] = h[3].wrapping_add(carry); carry = h[3] >> 51; h[3] &= 0x7FFFFFFFFFFFF;
        h[4] = h[4].wrapping_add(carry);                     h[4] &= 0x7FFFFFFFFFFFF;

        // Pack 5 × 51-bit limbs into 32 bytes (little-endian).
        let mut out = [0u8; 32];
        let val = h[0] | (h[1] << 51);
        store_le_u64(&mut out, 0, val);
        let val = (h[1] >> 13) | (h[2] << 38);
        store_le_u64(&mut out, 6, val);
        let val = (h[2] >> 26) | (h[3] << 25);
        store_le_u64(&mut out, 13, val);
        let val = (h[3] >> 39) | (h[4] << 12);
        store_le_u64(&mut out, 19, val);
        out[31] &= 0x7F; // Clear top bit.
        out
    }

    /// Field addition: a + b mod p (lazy, no reduction).
    fn add(self, rhs: Self) -> Self {
        Self([
            self.0[0].wrapping_add(rhs.0[0]),
            self.0[1].wrapping_add(rhs.0[1]),
            self.0[2].wrapping_add(rhs.0[2]),
            self.0[3].wrapping_add(rhs.0[3]),
            self.0[4].wrapping_add(rhs.0[4]),
        ])
    }

    /// Field subtraction: a - b mod p.
    /// Adds 2p before subtracting to ensure no underflow.
    fn sub(self, rhs: Self) -> Self {
        // 2p in limb form: each limb is 2 * (2^51 - 1) except last is 2*(2^51-19).
        Self([
            self.0[0].wrapping_add(0xFFFFFFFFFFFDA).wrapping_sub(rhs.0[0]),
            self.0[1].wrapping_add(0xFFFFFFFFFFFFE).wrapping_sub(rhs.0[1]),
            self.0[2].wrapping_add(0xFFFFFFFFFFFFE).wrapping_sub(rhs.0[2]),
            self.0[3].wrapping_add(0xFFFFFFFFFFFFE).wrapping_sub(rhs.0[3]),
            self.0[4].wrapping_add(0xFFFFFFFFFFFFE).wrapping_sub(rhs.0[4]),
        ])
    }

    /// Field multiplication: a * b mod p.
    fn mul(self, rhs: Self) -> Self {
        let a = self.0;
        let b = rhs.0;

        // Pre-multiply b limbs by 19 for reduction.
        let b1_19 = b[1].wrapping_mul(19);
        let b2_19 = b[2].wrapping_mul(19);
        let b3_19 = b[3].wrapping_mul(19);
        let b4_19 = b[4].wrapping_mul(19);

        // Schoolbook multiplication with lazy reduction.
        // c[i] = Σ a[j] * b[k] where (j+k) mod 5 == i,
        // with b[k]*19 when j+k >= 5 (wrap around, * 2^255 = *19).
        let c0 = (a[0] as u128) * (b[0] as u128)
               + (a[1] as u128) * (b4_19 as u128)
               + (a[2] as u128) * (b3_19 as u128)
               + (a[3] as u128) * (b2_19 as u128)
               + (a[4] as u128) * (b1_19 as u128);

        let c1 = (a[0] as u128) * (b[1] as u128)
               + (a[1] as u128) * (b[0]  as u128)
               + (a[2] as u128) * (b4_19 as u128)
               + (a[3] as u128) * (b3_19 as u128)
               + (a[4] as u128) * (b2_19 as u128);

        let c2 = (a[0] as u128) * (b[2] as u128)
               + (a[1] as u128) * (b[1]  as u128)
               + (a[2] as u128) * (b[0]  as u128)
               + (a[3] as u128) * (b4_19 as u128)
               + (a[4] as u128) * (b3_19 as u128);

        let c3 = (a[0] as u128) * (b[3] as u128)
               + (a[1] as u128) * (b[2]  as u128)
               + (a[2] as u128) * (b[1]  as u128)
               + (a[3] as u128) * (b[0]  as u128)
               + (a[4] as u128) * (b4_19 as u128);

        let c4 = (a[0] as u128) * (b[4] as u128)
               + (a[1] as u128) * (b[3]  as u128)
               + (a[2] as u128) * (b[2]  as u128)
               + (a[3] as u128) * (b[1]  as u128)
               + (a[4] as u128) * (b[0]  as u128);

        // Carry propagation.
        fe_carry5(c0, c1, c2, c3, c4)
    }

    /// Field squaring: a^2 mod p.
    /// Slightly more efficient than mul(self, self) because we can
    /// double the cross-terms.
    fn sqr(self) -> Self {
        let a = self.0;
        let a0_2 = a[0].wrapping_mul(2);
        let a1_2 = a[1].wrapping_mul(2);
        let a2_2 = a[2].wrapping_mul(2);
        let a3_2 = a[3].wrapping_mul(2);

        let a3_19 = a[3].wrapping_mul(19);
        let a4_19 = a[4].wrapping_mul(19);

        // Cross-terms are doubled (a[i]*a[j] appears twice for i≠j).
        // Wrapped terms (i+j >= 5) use *19 reduction.
        let c0 = (a[0] as u128) * (a[0] as u128)
               + (a1_2 as u128) * (a4_19 as u128)
               + (a2_2 as u128) * (a3_19 as u128);

        let c1 = (a0_2 as u128) * (a[1] as u128)
               + (a2_2 as u128) * (a4_19 as u128)
               + (a[3] as u128) * (a3_19 as u128);

        let c2 = (a0_2 as u128) * (a[2] as u128)
               + (a[1] as u128) * (a[1] as u128)
               + (a3_2 as u128) * (a4_19 as u128);

        let c3 = (a0_2 as u128) * (a[3] as u128)
               + (a1_2 as u128) * (a[2] as u128)
               + (a[4] as u128) * (a4_19 as u128);

        let c4 = (a0_2 as u128) * (a[4] as u128)
               + (a1_2 as u128) * (a[3] as u128)
               + (a[2] as u128) * (a[2] as u128);

        fe_carry5(c0, c1, c2, c3, c4)
    }

    /// Field inversion: 1/a mod p.
    /// Uses the Fermat method: a^(p-2) mod p.
    /// p-2 = 2^255 - 21 = special form that allows efficient chained squarings.
    fn invert(self) -> Self {
        // Based on the addition chain from djb's ref10 code.
        let z2 = self.sqr();                          // z^2
        let z9 = z2.sqr().sqr();                      // z^8
        let z9 = z9.mul(self);                         // z^9
        let z11 = z9.mul(z2);                          // z^11
        let z_5_0 = z11.sqr().mul(z9);                // z^(2^5-1)

        let mut t = z_5_0;
        for _ in 0..5 { t = t.sqr(); }
        let z_10_0 = t.mul(z_5_0);                    // z^(2^10-1)

        t = z_10_0;
        for _ in 0..10 { t = t.sqr(); }
        let z_20_0 = t.mul(z_10_0);                   // z^(2^20-1)

        t = z_20_0;
        for _ in 0..20 { t = t.sqr(); }
        t = t.mul(z_20_0);                             // z^(2^40-1)

        for _ in 0..10 { t = t.sqr(); }
        let z_50_0 = t.mul(z_10_0);                   // z^(2^50-1)

        t = z_50_0;
        for _ in 0..50 { t = t.sqr(); }
        let z_100_0 = t.mul(z_50_0);                  // z^(2^100-1)

        t = z_100_0;
        for _ in 0..100 { t = t.sqr(); }
        t = t.mul(z_100_0);                            // z^(2^200-1)

        for _ in 0..50 { t = t.sqr(); }
        t = t.mul(z_50_0);                             // z^(2^250-1)

        for _ in 0..5 { t = t.sqr(); }
        t.mul(z11)                                      // z^(2^255-21)
    }

    /// Conditional swap: if swap != 0, exchange self and other.
    /// Constant-time.
    fn cswap(&mut self, other: &mut Self, swap: u64) {
        let mask = 0u64.wrapping_sub(swap); // All-ones if swap=1, zero if swap=0.
        for i in 0..5 {
            let t = mask & (self.0[i] ^ other.0[i]);
            self.0[i] ^= t;
            other.0[i] ^= t;
        }
    }
}

/// Carry propagation for 5 × u128 products → 5 × u64 limbs.
fn fe_carry5(c0: u128, c1: u128, c2: u128, c3: u128, c4: u128) -> Fe25519 {
    let mut r = [0u64; 5];
    let carry = (c0 >> 51) as u64;
    r[0] = (c0 as u64) & 0x7FFFFFFFFFFFF;
    let c1 = c1 + carry as u128;

    let carry = (c1 >> 51) as u64;
    r[1] = (c1 as u64) & 0x7FFFFFFFFFFFF;
    let c2 = c2 + carry as u128;

    let carry = (c2 >> 51) as u64;
    r[2] = (c2 as u64) & 0x7FFFFFFFFFFFF;
    let c3 = c3 + carry as u128;

    let carry = (c3 >> 51) as u64;
    r[3] = (c3 as u64) & 0x7FFFFFFFFFFFF;
    let c4 = c4 + carry as u128;

    let carry = (c4 >> 51) as u64;
    r[4] = (c4 as u64) & 0x7FFFFFFFFFFFF;
    // Wrap carry: 2^255 ≡ 19 mod p.
    r[0] = r[0].wrapping_add(carry.wrapping_mul(19));
    // One more carry from r[0].
    let carry = r[0] >> 51;
    r[0] &= 0x7FFFFFFFFFFFF;
    r[1] = r[1].wrapping_add(carry);

    Fe25519(r)
}

/// Load u64 from a byte slice at the given offset (little-endian, unaligned).
fn load_le_u64(s: &[u8], off: usize) -> u64 {
    let end = (off + 8).min(s.len());
    let mut buf = [0u8; 8];
    buf[..end - off].copy_from_slice(&s[off..end]);
    u64::from_le_bytes(buf)
}

/// Store u64 to a byte slice at the given offset (little-endian, unaligned).
/// Overwrites 8 bytes starting at `off`.  If this would go past the slice
/// end, only the fitting bytes are written.
fn store_le_u64(s: &mut [u8], off: usize, val: u64) {
    let bytes = val.to_le_bytes();
    let end = (off + 8).min(s.len());
    let n = end.saturating_sub(off);
    s[off..off + n].copy_from_slice(&bytes[..n]);
}

/// The Curve25519 base point (little-endian): u = 9.
const X25519_BASEPOINT: [u8; 32] = {
    let mut bp = [0u8; 32];
    bp[0] = 9;
    bp
};

/// X25519 Diffie-Hellman function (RFC 7748 §5).
///
/// Computes the shared secret from a scalar (private key) and a point
/// (peer's public key).  Both are 32 bytes.
///
/// The scalar is clamped per RFC 7748: bits 0-2 cleared (multiple of 8),
/// bit 254 set (ensure high bit for constant-time Montgomery ladder),
/// bit 255 cleared (stays in field).
///
/// Returns 32 bytes.  The result MUST be checked for the all-zero output
/// (which indicates a low-order point — reject the handshake).
pub fn x25519(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
    // Clamp scalar per RFC 7748.
    let mut k = *scalar;
    k[0]  &= 248;   // Clear bottom 3 bits.
    k[31] &= 127;   // Clear top bit.
    k[31] |= 64;    // Set bit 254.

    let u = Fe25519::from_bytes(point);

    // Montgomery ladder: constant-time scalar multiplication.
    let x_1 = u;
    let mut x_2 = Fe25519::ONE;
    let mut z_2 = Fe25519::ZERO;
    let mut x_3 = u;
    let mut z_3 = Fe25519::ONE;
    let mut swap: u64 = 0;

    // Process bits from top to bottom (bit 254 down to bit 0).
    let mut t = 254i32;
    while t >= 0 {
        let byte_idx = (t >> 3) as usize;
        let bit_idx = (t & 7) as u32;
        let k_t = ((k[byte_idx] >> bit_idx) & 1) as u64;

        swap ^= k_t;
        x_2.cswap(&mut x_3, swap);
        z_2.cswap(&mut z_3, swap);
        swap = k_t;

        let a = x_2.add(z_2);
        let aa = a.sqr();
        let b = x_2.sub(z_2);
        let bb = b.sqr();
        let e = aa.sub(bb);
        let c = x_3.add(z_3);
        let d = x_3.sub(z_3);
        let da = d.mul(a);
        let cb = c.mul(b);
        x_3 = da.add(cb).sqr();
        z_3 = da.sub(cb).sqr().mul(x_1);
        x_2 = aa.mul(bb);
        // a24 = 121665 for Curve25519.
        let a24 = Fe25519([121665, 0, 0, 0, 0]);
        z_2 = e.mul(aa.add(a24.mul(e)));

        t -= 1;
    }

    x_2.cswap(&mut x_3, swap);
    z_2.cswap(&mut z_3, swap);

    // Result = x_2 * z_2^(-1).
    let result = x_2.mul(z_2.invert());
    result.to_bytes()
}

/// Compute X25519 public key from a private key.
///
/// Equivalent to `x25519(scalar, basepoint)` where basepoint = 9.
pub fn x25519_base(scalar: &[u8; 32]) -> [u8; 32] {
    x25519(scalar, &X25519_BASEPOINT)
}
