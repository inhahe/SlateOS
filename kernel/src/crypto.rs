//! Cryptographic primitives for the kernel.
//!
//! Provides:
//! - **SHA-256** for file content hashing and integrity verification
//! - **SHA-512** for Ed25519 signatures (RFC 6234)
//! - **CRC32C** (Castagnoli) for ext4 metadata checksums
//! - **HMAC-SHA256** (RFC 2104) for keyed message authentication
//! - **HKDF-SHA256** (RFC 5869) for key derivation (TLS 1.3 key schedule)
//! - **ChaCha20** (RFC 8439) stream cipher
//! - **Poly1305** (RFC 8439) one-time authenticator
//! - **ChaCha20-Poly1305** (RFC 8439) AEAD construction (TLS 1.3 cipher)
//! - **X25519** (RFC 7748) Diffie-Hellman key exchange over Curve25519
//! - **Ed25519** (RFC 8032) digital signatures over Edwards curve 25519
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
//! - SHA-512: FIPS 180-4 (Secure Hash Standard)
//! - CRC32C: RFC 3720 appendix B (iSCSI), polynomial 0x1EDC6F41
//! - HMAC: RFC 2104 (HMAC: Keyed-Hashing for Message Authentication)
//! - HKDF: RFC 5869 (HMAC-based Extract-and-Expand Key Derivation)
//! - ChaCha20-Poly1305: RFC 8439 (formerly RFC 7539)
//! - X25519: RFC 7748 (Elliptic Curves for Security)
//! - Ed25519: RFC 8032 (Edwards-Curve Digital Signature Algorithm)

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

    // Reassemble 26-bit limbs into 4 × 32-bit words, then add s.
    //
    // Following the reference (poly1305-donna): convert to 32-bit words
    // first, masking each to 32 bits, then add s with a carry chain.
    // Combining both steps into one (as in some optimized versions)
    // is error-prone because the carry from word N already contains the
    // upper limb bits that word N+1 also references, causing double-counting.
    #[allow(clippy::cast_possible_truncation)]
    let h0w = ((h[0]      ) | (h[1] << 26)) as u32;  // bits  0-31
    #[allow(clippy::cast_possible_truncation)]
    let h1w = ((h[1] >> 6 ) | (h[2] << 20)) as u32;  // bits 32-63
    #[allow(clippy::cast_possible_truncation)]
    let h2w = ((h[2] >> 12) | (h[3] << 14)) as u32;  // bits 64-95
    #[allow(clippy::cast_possible_truncation)]
    let h3w = ((h[3] >> 18) | (h[4] << 8 )) as u32;  // bits 96-127

    let mut f: u64;
    f = (h0w as u64).wrapping_add(s[0] as u64);
    let tag0 = f as u32;
    f = (f >> 32).wrapping_add(h1w as u64).wrapping_add(s[1] as u64);
    let tag1 = f as u32;
    f = (f >> 32).wrapping_add(h2w as u64).wrapping_add(s[2] as u64);
    let tag2 = f as u32;
    f = (f >> 32).wrapping_add(h3w as u64).wrapping_add(s[3] as u64);
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
        // Uses overlapping 8-byte writes at offsets matching from_bytes reads.
        // Each write produces the correct bits for its byte range; later writes
        // overwrite the tail bytes of earlier writes.  Verified as the exact
        // inverse of from_bytes (load offsets 0,6,12,19,24 with shifts 0,3,6,1,12).
        let mut out = [0u8; 32];
        let val = h[0] | (h[1] << 51);
        store_le_u64(&mut out, 0, val);
        let val = (h[0] >> 48) | (h[1] << 3) | (h[2] << 54);
        store_le_u64(&mut out, 6, val);
        let val = (h[1] >> 45) | (h[2] << 6) | (h[3] << 57);
        store_le_u64(&mut out, 12, val);
        let val = (h[2] >> 50) | (h[3] << 1) | (h[4] << 52);
        store_le_u64(&mut out, 19, val);
        let val = (h[3] >> 39) | (h[4] << 12);
        store_le_u64(&mut out, 24, val);
        out[31] &= 0x7F; // Clear top bit (bit 255 is always 0 for Curve25519).
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

// ===========================================================================
// SHA-512 (FIPS 180-4)
// ===========================================================================

/// SHA-512 digest size in bytes.
pub const SHA512_DIGEST_SIZE: usize = 64;

/// SHA-512 initial hash values (first 64 bits of the fractional parts of
/// the square roots of the first 8 primes).
#[allow(clippy::unreadable_literal)]
const SHA512_H: [u64; 8] = [
    0x6a09e667f3bcc908, 0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1,
    0x510e527fade682d1, 0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b, 0x5be0cd19137e2179,
];

/// SHA-512 round constants (first 64 bits of the fractional parts of
/// the cube roots of the first 80 primes).
#[allow(clippy::unreadable_literal)]
const SHA512_K: [u64; 80] = [
    0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc,
    0x3956c25bf348b538, 0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118,
    0xd807aa98a3030242, 0x12835b0145706fbe, 0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235, 0xc19bf174cf692694,
    0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65,
    0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5,
    0x983e5152ee66dfab, 0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2, 0xd5a79147930aa725, 0x06ca6351e003826f, 0x142929670a0e6e70,
    0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed, 0x53380d139d95b3df,
    0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
    0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc24b8b70d0f89791, 0xc76c51a30654be30,
    0xd192e819d6ef5218, 0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8, 0x1e376c085141ab53, 0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373, 0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec,
    0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b,
    0xca273eceea26619c, 0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178,
    0x06f067aa72176fba, 0x0a637dc5a2c898a6, 0x113f9804bef90dae, 0x1b710b35131c471b,
    0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc, 0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
];

/// Incremental SHA-512 hasher.
pub struct Sha512 {
    state: [u64; 8],
    buf: [u8; 128],
    buf_len: usize,
    total_len: u128,
}

impl Sha512 {
    /// Create a new SHA-512 hasher.
    pub fn new() -> Self {
        Self {
            state: SHA512_H,
            buf: [0u8; 128],
            buf_len: 0,
            total_len: 0,
        }
    }

    /// Feed data into the hasher.
    pub fn update(&mut self, data: &[u8]) {
        let mut offset = 0usize;
        self.total_len = self.total_len.wrapping_add(data.len() as u128);

        // Fill partial buffer.
        if self.buf_len > 0 {
            let need = 128usize.saturating_sub(self.buf_len);
            let take = data.len().min(need);
            self.buf[self.buf_len..self.buf_len.wrapping_add(take)]
                .copy_from_slice(data.get(..take).unwrap_or(&[]));
            self.buf_len = self.buf_len.wrapping_add(take);
            offset = take;
            if self.buf_len == 128 {
                let block = self.buf;
                sha512_compress(&mut self.state, &block);
                self.buf_len = 0;
            }
        }

        // Process full 128-byte blocks.
        while offset.wrapping_add(128) <= data.len() {
            let mut block = [0u8; 128];
            block.copy_from_slice(data.get(offset..offset.wrapping_add(128)).unwrap_or(&[]));
            sha512_compress(&mut self.state, &block);
            offset = offset.wrapping_add(128);
        }

        // Buffer remainder.
        let remaining = data.len().saturating_sub(offset);
        if remaining > 0 {
            self.buf[..remaining].copy_from_slice(
                data.get(offset..offset.wrapping_add(remaining)).unwrap_or(&[]),
            );
            self.buf_len = remaining;
        }
    }

    /// Finalize and return the 64-byte digest.
    pub fn finalize(mut self) -> [u8; SHA512_DIGEST_SIZE] {
        // Pad: append 0x80, then zeros, then 128-bit big-endian length.
        let total_bits = self.total_len.wrapping_mul(8);
        self.buf[self.buf_len] = 0x80;
        self.buf_len = self.buf_len.wrapping_add(1);

        if self.buf_len > 112 {
            // Not enough room for the 16-byte length — pad this block and
            // process it, then start a fresh block.
            for i in self.buf_len..128 {
                self.buf[i] = 0;
            }
            sha512_compress(&mut self.state, &self.buf);
            self.buf = [0u8; 128];
        } else {
            for i in self.buf_len..128 {
                self.buf[i] = 0;
            }
        }

        // Append 128-bit message length (big-endian) at the end.
        let len_bytes = total_bits.to_be_bytes();
        self.buf[112..128].copy_from_slice(&len_bytes);
        sha512_compress(&mut self.state, &self.buf);

        let mut out = [0u8; 64];
        for (i, &h) in self.state.iter().enumerate() {
            let off = i.wrapping_mul(8);
            out[off..off.wrapping_add(8)].copy_from_slice(&h.to_be_bytes());
        }
        out
    }
}

/// Compress one 128-byte block into the SHA-512 state.
#[allow(clippy::many_single_char_names, clippy::arithmetic_side_effects)]
fn sha512_compress(state: &mut [u64; 8], block: &[u8; 128]) {
    // Use a 16-element circular buffer for the message schedule.
    // Computes w values on-the-fly (FIPS 180-4 §6.4.2 optimized form).
    let mut w = [0u64; 16];

    // Load 16 message words (big-endian).
    for i in 0..16 {
        let off = i * 8;
        w[i] = u64::from_be_bytes([
            block[off], block[off + 1], block[off + 2], block[off + 3],
            block[off + 4], block[off + 5], block[off + 6], block[off + 7],
        ]);
    }

    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];

    // 80 rounds. For rounds 16..80, extend the schedule in-place.
    for i in 0..80usize {
        // For i >= 16, update w[i & 15] with the schedule extension:
        // W_t = σ1(W_{t-2}) + W_{t-7} + σ0(W_{t-15}) + W_{t-16}
        if i >= 16 {
            let i0 = i & 15;
            let w_i2 = w[(i - 2) & 15];
            let w_i7 = w[(i - 7) & 15];
            let w_i15 = w[(i - 15) & 15];
            let w_i16 = w[i0]; // w[(i - 16) & 15] == w[i & 15]
            let sigma0 = w_i15.rotate_right(1) ^ w_i15.rotate_right(8) ^ (w_i15 >> 7);
            let sigma1 = w_i2.rotate_right(19) ^ w_i2.rotate_right(61) ^ (w_i2 >> 6);
            w[i0] = w_i16.wrapping_add(sigma0).wrapping_add(w_i7).wrapping_add(sigma1);
        }

        let wi = w[i & 15];

        // Compression round (FIPS 180-4 §6.4.2).
        let big_sigma1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h.wrapping_add(big_sigma1)
            .wrapping_add(ch)
            .wrapping_add(SHA512_K[i])
            .wrapping_add(wi);
        let big_sigma0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = big_sigma0.wrapping_add(maj);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

/// One-shot SHA-512 hash.
pub fn sha512(data: &[u8]) -> [u8; SHA512_DIGEST_SIZE] {
    let mut hasher = Sha512::new();
    hasher.update(data);
    hasher.finalize()
}

// ===========================================================================
// Ed25519 (RFC 8032)
// ===========================================================================

/// Ed25519 signature size in bytes.
pub const ED25519_SIGNATURE_SIZE: usize = 64;

/// Ed25519 public key size in bytes.
pub const ED25519_PUBLIC_KEY_SIZE: usize = 32;

/// Ed25519 secret key size in bytes (seed).
pub const ED25519_SECRET_KEY_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// Extended Edwards point (X:Y:Z:T where x = X/Z, y = Y/Z, X*Y = Z*T)
// on the twisted Edwards curve: -x² + y² = 1 + d*x²*y²
// where d = -121665/121666 mod p, p = 2^255 - 19.
//
// References:
//   - RFC 8032 §5.1
//   - HISIL–WONG–CARTER–DAWSON, "Twisted Edwards curves revisited"
//   - djb's ref10 implementation
// ---------------------------------------------------------------------------

/// The Ed25519 curve parameter d = -121665/121666 mod p.
///
/// Precomputed: 0x52036cee2b6ffe738cc740797779e89800700a4d4141d8ab75eb4dca135978a3
#[allow(clippy::unreadable_literal)]
const ED25519_D: Fe25519 = Fe25519([
    0x34DCA135978A3, 0x1A8283B156EBD, 0x5E7A26001C029, 0x739C663A03CBB, 0x52036CEE2B6FF,
]);

/// 2*d mod p (reserved for optimized point doubling).
#[allow(clippy::unreadable_literal, dead_code)]
const ED25519_D2: Fe25519 = Fe25519([
    0x69B9426B2F159, 0x35050762ADD7A, 0x3CF44C0038052, 0x6738CC7407977, 0x2406D9DC56DFF,
]);

/// A point in extended coordinates (X, Y, Z, T) on the Ed25519 curve.
#[derive(Clone, Copy)]
struct EdPoint {
    x: Fe25519,
    y: Fe25519,
    z: Fe25519,
    t: Fe25519,
}

impl EdPoint {
    /// The identity (neutral) point: (0, 1, 1, 0).
    const IDENTITY: Self = Self {
        x: Fe25519::ZERO,
        y: Fe25519::ONE,
        z: Fe25519::ONE,
        t: Fe25519::ZERO,
    };

    /// The Ed25519 base point B = (Bx, By, 1, Bx*By).
    ///
    /// By = 4/5 mod p (the canonical y-coordinate; x is the positive root).
    /// RFC 8032 §5.1 specifies y = 4/5.
    fn basepoint() -> Self {
        // y = 4/5 mod p = 4 * inv(5) mod p.
        let four = Fe25519([4, 0, 0, 0, 0]);
        let five = Fe25519([5, 0, 0, 0, 0]);
        let y = four.mul(five.invert());

        // x = positive sqrt of (y²-1) / (d*y²+1).
        let y2 = y.sqr();
        let numerator = y2.sub(Fe25519::ONE);
        let denominator = ED25519_D.mul(y2).add(Fe25519::ONE);
        let x = ed_sqrt_ratio(numerator, denominator);

        let t = x.mul(y);
        Self { x, y, z: Fe25519::ONE, t }
    }

    /// Point addition using the unified extended coordinates formula.
    ///
    /// RFC 8032 §5.1.4 / HISIL et al. unified addition for a = -1:
    ///   A = X1*X2, B = Y1*Y2, C = T1*d*T2, D = Z1*Z2
    ///   E = (X1+Y1)*(X2+Y2) - A - B, F = D-C, G = D+C
    ///   H = B - a*A = B + A (since a=-1)
    ///   X3 = E*F, Y3 = G*H, T3 = E*H, Z3 = F*G
    fn add_point(self, other: Self) -> Self {
        let a = self.x.mul(other.x);
        let b = self.y.mul(other.y);
        let c = self.t.mul(ED25519_D).mul(other.t);
        let d = self.z.mul(other.z);

        let e = {
            let s1 = self.x.add(self.y);
            let s2 = other.x.add(other.y);
            s1.mul(s2).sub(a).sub(b)
        };
        let f = d.sub(c);
        let g = d.add(c);
        let h = b.add(a); // B + A (since a = -1, this is B - a*A)

        Self {
            x: e.mul(f),
            y: g.mul(h),
            t: e.mul(h),
            z: f.mul(g),
        }
    }

    /// Dedicated point doubling (HWCD formula for a=-1).
    ///
    /// Uses the specialized doubling formula that does NOT involve the
    /// T coordinate as input.  This is diagnostically useful: if the unified
    /// addition formula accumulates T-coordinate drift over many doublings,
    /// this formula avoids that by computing C = 2·Z² instead of d·T².
    ///
    /// Formula (Hisil–Wong–Carter–Dawson, Table 4, a=-1):
    ///   A = X1², B = Y1², C = 2·Z1², D = a·A = -A
    ///   E = (X1+Y1)² - A - B, G = D+B, F = G-C, H = D-B
    ///   X3 = E·F, Y3 = G·H, T3 = E·H, Z3 = F·G
    fn double_point(self) -> Self {
        let aa = self.x.sqr();             // A = X1²
        let bb = self.y.sqr();             // B = Y1²
        let zz = self.z.sqr();
        let cc = zz.add(zz);              // C = 2·Z1²
        let d = Fe25519::ZERO.sub(aa);    // D = a·A = -A (since a=-1)
        let e = {
            let s = self.x.add(self.y);
            s.sqr().sub(aa).sub(bb)        // E = (X1+Y1)² - A - B = 2·X1·Y1
        };
        let g = d.add(bb);                // G = D + B = B - A
        let f = g.sub(cc);                // F = G - C = (B-A) - 2Z²
        let h = d.sub(bb);                // H = D - B = -(A+B)
        Self {
            x: e.mul(f),
            y: g.mul(h),
            t: e.mul(h),
            z: f.mul(g),
        }
    }

    /// Scalar multiplication: compute `scalar * self`.
    ///
    /// Uses MSB-first double-and-add.  The doubling step uses the
    /// dedicated doubling formula (faster, avoids T-coordinate dependency),
    /// while the addition step uses the unified formula.
    fn scalar_mul(self, scalar: &[u8; 32]) -> Self {
        let mut result = Self::IDENTITY;
        // Process bits from MSB to LSB (little-endian scalar).
        for i in (0..256).rev() {
            result = result.double_point(); // dedicated doubling
            let byte_idx = i / 8;
            let bit_idx = i % 8;
            if let Some(&b) = scalar.get(byte_idx) {
                if (b >> bit_idx) & 1 == 1 {
                    result = result.add_point(self); // unified addition
                }
            }
        }
        result
    }

    /// Encode a point to 32 bytes (RFC 8032 §5.1.2).
    ///
    /// The encoding is the y-coordinate in little-endian, with the
    /// sign of x stored in the top bit of the last byte.
    fn encode(self) -> [u8; 32] {
        let zi = self.z.invert();
        let x = self.x.mul(zi);
        let y = self.y.mul(zi);

        let mut encoded = y.to_bytes();
        // The "sign" of x is its least significant bit.
        let x_bytes = x.to_bytes();
        let x_sign = x_bytes[0] & 1;
        encoded[31] |= x_sign << 7;
        encoded
    }

    /// Decode a point from 32 bytes (RFC 8032 §5.1.3).
    ///
    /// Returns `None` if the encoding is invalid (y out of range or
    /// no valid x exists for the curve equation).
    fn decode(bytes: &[u8; 32]) -> Option<Self> {
        // Extract x sign bit from top of byte 31.
        let x_sign = (bytes[31] >> 7) & 1;

        // Decode y (clear top bit).
        let mut y_bytes = *bytes;
        y_bytes[31] &= 0x7F;
        let y = Fe25519::from_bytes(&y_bytes);

        // Compute x = sqrt((y² - 1) / (d*y² + 1)).
        // Uses ref10 approach: x = u * v^3 * (u * v^7)^(2^252-3)
        // where u = y²-1, v = d*y²+1.
        let y2 = y.sqr();
        let u = y2.sub(Fe25519::ONE);
        let v = ED25519_D.mul(y2).add(Fe25519::ONE);

        let v3 = v.sqr().mul(v);
        let v7 = v3.sqr().mul(v);
        let uv7 = u.mul(v7);
        let mut x = fe_pow_2_252_3(uv7).mul(v3).mul(u);

        // Verify: x² * v == u.  If not, try x * sqrt(-1).
        let check = x.sqr().mul(v).sub(u);
        if check.to_bytes() != [0u8; 32] {
            let sqrt_m1 = fe_sqrt_minus_one();
            x = x.mul(sqrt_m1);
            let check2 = x.sqr().mul(v).sub(u);
            if check2.to_bytes() != [0u8; 32] {
                return None; // No valid square root.
            }
        }

        // Negate x if its sign doesn't match.
        let x_bytes = x.to_bytes();
        if (x_bytes[0] & 1) != x_sign {
            x = Fe25519::ZERO.sub(x);
        }

        // If x is zero and x_sign is 1, encoding is invalid.
        if x.to_bytes() == [0u8; 32] && x_sign == 1 {
            return None;
        }

        let t = x.mul(y);
        Some(Self { x, y, z: Fe25519::ONE, t })
    }
}

/// Compute z^(2^252 - 3) mod p.
///
/// This is the candidate square root exponent: if x² = v, then
/// v^((p+3)/8) = v^(2^252-2) gives a candidate, but the standard
/// approach uses v^(2^252-3).  We use the addition chain from ref10.
fn fe_pow_2_252_3(z: Fe25519) -> Fe25519 {
    let z2 = z.sqr();
    let z9 = z2.sqr().sqr().mul(z);
    let z11 = z9.mul(z2);
    let z_5_0 = z11.sqr().mul(z9);

    let mut t = z_5_0;
    for _ in 0..5 { t = t.sqr(); }
    let z_10_0 = t.mul(z_5_0);

    t = z_10_0;
    for _ in 0..10 { t = t.sqr(); }
    let z_20_0 = t.mul(z_10_0);

    t = z_20_0;
    for _ in 0..20 { t = t.sqr(); }
    t = t.mul(z_20_0);

    for _ in 0..10 { t = t.sqr(); }
    let z_50_0 = t.mul(z_10_0);

    t = z_50_0;
    for _ in 0..50 { t = t.sqr(); }
    let z_100_0 = t.mul(z_50_0);

    t = z_100_0;
    for _ in 0..100 { t = t.sqr(); }
    t = t.mul(z_100_0);

    for _ in 0..50 { t = t.sqr(); }
    t = t.mul(z_50_0);

    t = t.sqr().sqr(); // 2^252
    t.mul(z) // z^(2^252 - 3)
}

/// Compute sqrt(-1) mod p = 2^((p-1)/4) mod p.
fn fe_sqrt_minus_one() -> Fe25519 {
    // 2^((p-1)/4) = 2^(2^253 - 5).
    // We compute this via repeated squaring of Fe(2).
    let two = Fe25519([2, 0, 0, 0, 0]);
    let mut r = two;
    // Square 252 times to get 2^(2^252).
    for _ in 0..252 { r = r.sqr(); }
    // Now r = 2^(2^252).  We need 2^(2^253 - 5) = (2^(2^253)) / (2^5).
    // Actually, let's compute it differently using the pow function.
    // sqrt(-1) mod p is a well-known constant.
    // Value: 0x2b8324804fc1df0b2b4d00993dfbd7a72f431806ad2fe478c4ee1b274a0ea0b0
    // In our limb format:
    Fe25519::from_bytes(&[
        0xB0, 0xA0, 0x0E, 0x4A, 0x27, 0x1B, 0xEE, 0xC4,
        0x78, 0xE4, 0x2F, 0xAD, 0x06, 0x18, 0x43, 0x2F,
        0xA7, 0xD7, 0xFB, 0x3D, 0x99, 0x00, 0x4D, 0x2B,
        0x0B, 0xDF, 0xC1, 0x4F, 0x80, 0x24, 0x83, 0x2B,
    ])
}

/// Compute the "positive" square root of num/den on the Ed25519 curve.
///
/// Uses the identity: sqrt(u/v) = u * v^3 * (u * v^7)^((p-5)/8)
/// where (p-5)/8 = 2^252 - 3 (computed by `fe_pow_2_252_3`).
/// This avoids a costly full field inversion.
///
/// Reference: NaCl ref10 `ge25519_frombytes_negate_vartime.c`.
fn ed_sqrt_ratio(num: Fe25519, den: Fe25519) -> Fe25519 {
    // v^3 and v^7.
    let v3 = den.sqr().mul(den);
    let v7 = v3.sqr().mul(den);
    // Candidate: x = u * v^3 * (u * v^7)^(2^252 - 3).
    let uv7 = num.mul(v7);
    let mut x = fe_pow_2_252_3(uv7).mul(v3).mul(num);

    // Check: x² * v == u. If not, try x * sqrt(-1).
    let check = x.sqr().mul(den).sub(num);
    if check.to_bytes() != [0u8; 32] {
        x = x.mul(fe_sqrt_minus_one());
    }

    // Ensure x is non-negative (least significant bit is 0).
    let x_bytes = x.to_bytes();
    if x_bytes[0] & 1 != 0 {
        x = Fe25519::ZERO.sub(x);
    }
    x
}

// ---------------------------------------------------------------------------
// Ed25519 scalar arithmetic (mod L, where L is the group order)
// ---------------------------------------------------------------------------

/// The Ed25519 group order L.
///
/// L = 2^252 + 27742317777372353535851937790883648493
#[allow(clippy::unreadable_literal)]
const ED25519_L: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58,
    0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
];

/// Reduce a 64-byte scalar modulo L.
///
/// Uses Barrett reduction with the Ed25519 group order.
/// Input: 64-byte little-endian integer from SHA-512.
/// Output: 32-byte little-endian integer reduced mod L.
fn sc_reduce(input: &[u8; 64]) -> [u8; 32] {
    // Load 64 bytes into 24 limbs of 21 bits each (to avoid overflow
    // in intermediate additions).  This follows the approach from djb's
    // ref10: load into signed i64 limbs at 21-bit boundaries, then
    // Barrett-reduce.
    //
    // For simplicity we use a schoolbook approach: interpret as a 512-bit
    // number, divide by L, keep remainder.
    //
    // We implement a simple shift-and-subtract reduction.

    // Load as array of u64 words (little-endian).
    let mut val = [0u64; 8];
    for i in 0..8 {
        let off = i * 8;
        val[i] = u64::from_le_bytes([
            input[off], input[off + 1], input[off + 2], input[off + 3],
            input[off + 4], input[off + 5], input[off + 6], input[off + 7],
        ]);
    }

    // Reduce using schoolbook division with u64 limbs.
    // L fits in ~253 bits.  val is at most 512 bits.
    // We repeatedly subtract L shifted left until val < L.
    //
    // For correctness at the cost of simplicity, we'll use a
    // byte-level approach: treat the 64-byte input as a big integer
    // and reduce mod L.
    sc_reduce_bytes(input)
}

/// Byte-level modular reduction of a 512-bit integer mod L.
///
/// Simple but correct: converts to double-width, subtracts multiples of L.
fn sc_reduce_bytes(input: &[u8; 64]) -> [u8; 32] {
    // We use a simple approach: process in 21-bit signed limbs, as in
    // the original NaCl/ref10 code.  This is the well-tested method.
    //
    // Load into signed i64 limbs at various bit widths for the 512-bit
    // input, reduce modulo L using the constants from the NaCl reference.
    //
    // For a straightforward implementation, we use carry-propagation
    // arithmetic on the 64-byte input treated as limbs.

    // Actually, the simplest correct approach for our purposes:
    // Load as 512-bit, subtract L repeatedly.  Since the max input is
    // < 2^512 and L ≈ 2^252, the quotient fits in ~260 bits.
    //
    // We use a wide multiply-free approach.

    // Simple but slow reduction: byte-array subtraction loop.
    let mut result = [0u8; 64];
    result.copy_from_slice(input);

    // Subtract L * 2^(8*i) for i = 32 down to 0 while result >= L * 2^(8*i).
    // This is O(32 * 32) byte operations — perfectly fine for signing.
    for shift in (0..=32).rev() {
        loop {
            // Check if result >= L << (shift*8) by comparing the relevant bytes.
            if !gte_shifted(&result, &ED25519_L, shift) {
                break;
            }
            sub_shifted(&mut result, &ED25519_L, shift);
        }
    }

    let mut out = [0u8; 32];
    out.copy_from_slice(&result[..32]);
    out
}

/// Check if a >= b << (shift * 8) (big-integer comparison).
fn gte_shifted(a: &[u8; 64], b: &[u8; 32], shift: usize) -> bool {
    // The effective length of the shifted b is shift + 32 bytes.
    let top = shift.wrapping_add(32);
    if top > 64 { return false; }

    // Check that all bytes above the shifted range are zero in a.
    for i in top..64 {
        if a[i] != 0 { return true; }
    }

    // Compare from MSB to LSB in the overlapping range.
    for i in (0usize..32).rev() {
        let a_byte = a[i.wrapping_add(shift)];
        let b_byte = b[i];
        if a_byte > b_byte { return true; }
        if a_byte < b_byte { return false; }
    }
    // Check lower bytes of a (below shift) — if they exist, a >= b.
    true
}

/// Subtract b << (shift * 8) from a (in place).
fn sub_shifted(a: &mut [u8; 64], b: &[u8; 32], shift: usize) {
    let mut borrow: u16 = 0;
    for i in 0usize..32 {
        let idx = i.wrapping_add(shift);
        if idx >= 64 { break; }
        let diff = (a[idx] as u16).wrapping_sub(b[i] as u16).wrapping_sub(borrow);
        a[idx] = diff as u8;
        borrow = (diff >> 8) & 1;
    }
    // Propagate borrow beyond the 32-byte range.
    let mut idx = shift.wrapping_add(32);
    while borrow > 0 && idx < 64 {
        let diff = (a[idx] as u16).wrapping_sub(borrow);
        a[idx] = diff as u8;
        borrow = (diff >> 8) & 1;
        idx = idx.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Ed25519 public API
// ---------------------------------------------------------------------------

/// Ed25519 key pair (seed + public key).
pub struct Ed25519KeyPair {
    /// Secret seed (32 bytes, random).
    pub secret: [u8; ED25519_SECRET_KEY_SIZE],
    /// Public key (32 bytes, compressed Edwards y-coordinate).
    pub public: [u8; ED25519_PUBLIC_KEY_SIZE],
}

/// Generate an Ed25519 key pair from a 32-byte seed.
///
/// The seed should be cryptographically random (e.g., from `rng::fill`).
///
/// # Algorithm (RFC 8032 §5.1.5)
///
/// 1. Hash the 32-byte seed with SHA-512 → 64 bytes.
/// 2. The first 32 bytes (after clamping) become the scalar `a`.
/// 3. The public key is `A = a * B` (scalar multiply with base point).
pub fn ed25519_keypair(seed: &[u8; 32]) -> Ed25519KeyPair {
    let h = sha512(seed);

    let mut a = [0u8; 32];
    a.copy_from_slice(&h[..32]);
    // Clamp: clear low 3 bits, clear top bit, set second-to-top bit.
    a[0] &= 248;
    a[31] &= 127;
    a[31] |= 64;

    let base = EdPoint::basepoint();
    let public_point = base.scalar_mul(&a);
    let public_key = public_point.encode();

    Ed25519KeyPair { secret: *seed, public: public_key }
}

/// Compute the Ed25519 public key from a secret seed.
pub fn ed25519_public_key(seed: &[u8; 32]) -> [u8; ED25519_PUBLIC_KEY_SIZE] {
    ed25519_keypair(seed).public
}

/// Sign a message with Ed25519 (RFC 8032 §5.1.6).
///
/// Returns a 64-byte signature (R || S).
///
/// # Algorithm
///
/// 1. Hash seed → (a, prefix) via SHA-512.
/// 2. r = SHA-512(prefix || message) mod L.
/// 3. R = r * B.
/// 4. S = (r + SHA-512(R || A || message) * a) mod L.
pub fn ed25519_sign(seed: &[u8; 32], message: &[u8]) -> [u8; ED25519_SIGNATURE_SIZE] {
    let h = sha512(seed);

    let mut a = [0u8; 32];
    a.copy_from_slice(&h[..32]);
    a[0] &= 248;
    a[31] &= 127;
    a[31] |= 64;

    let prefix = &h[32..64];

    // Compute public key A.
    let base = EdPoint::basepoint();
    let a_point = base.scalar_mul(&a);
    let a_enc = a_point.encode();

    // r = SHA-512(prefix || message) mod L.
    let mut hasher = Sha512::new();
    hasher.update(prefix);
    hasher.update(message);
    let r_hash = hasher.finalize();
    let r = sc_reduce(&r_hash);

    // R = r * B.
    let r_point = base.scalar_mul(&r);
    let r_enc = r_point.encode();

    // k = SHA-512(R || A || message) mod L.
    let mut hasher = Sha512::new();
    hasher.update(&r_enc);
    hasher.update(&a_enc);
    hasher.update(message);
    let k_hash = hasher.finalize();
    let k = sc_reduce(&k_hash);

    // S = (r + k * a) mod L.
    let s = sc_muladd(&k, &a, &r);

    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&r_enc);
    sig[32..].copy_from_slice(&s);
    sig
}

/// Verify an Ed25519 signature (RFC 8032 §5.1.7).
///
/// Returns `true` if the signature is valid.
///
/// # Algorithm
///
/// 1. Decode R and A from the signature and public key.
/// 2. k = SHA-512(R || A || message) mod L.
/// 3. Check: S * B == R + k * A.
pub fn ed25519_verify(
    public_key: &[u8; ED25519_PUBLIC_KEY_SIZE],
    message: &[u8],
    signature: &[u8; ED25519_SIGNATURE_SIZE],
) -> bool {
    // Decode R.
    let mut r_bytes = [0u8; 32];
    r_bytes.copy_from_slice(&signature[..32]);
    let r_point = match EdPoint::decode(&r_bytes) {
        Some(p) => p,
        None => return false,
    };

    // Decode A.
    let a_point = match EdPoint::decode(public_key) {
        Some(p) => p,
        None => return false,
    };

    // Extract S.
    let mut s_bytes = [0u8; 32];
    s_bytes.copy_from_slice(&signature[32..]);

    // Check S < L (reject non-canonical signatures).
    if !sc_is_canonical(&s_bytes) {
        return false;
    }

    // k = SHA-512(R || A || message) mod L.
    let mut hasher = Sha512::new();
    hasher.update(&r_bytes);
    hasher.update(public_key);
    hasher.update(message);
    let k_hash = hasher.finalize();
    let k = sc_reduce(&k_hash);

    // Check: S * B == R + k * A.
    let base = EdPoint::basepoint();
    let sb = base.scalar_mul(&s_bytes);
    let ka = a_point.scalar_mul(&k);
    let rhs = r_point.add_point(ka);

    // Compare encoded points.
    sb.encode() == rhs.encode()
}

/// Scalar multiply-add: (a * b + c) mod L.
///
/// All inputs are 32-byte little-endian scalars.
fn sc_muladd(a: &[u8; 32], b: &[u8; 32], c: &[u8; 32]) -> [u8; 32] {
    // Multiply a * b into a 64-byte result, then add c, then reduce mod L.
    let mut product = [0u16; 64];

    // Schoolbook multiply: a * b → 64 bytes.
    for i in 0usize..32 {
        let mut carry = 0u16;
        for j in 0usize..32 {
            let idx = i.wrapping_add(j);
            if idx < 64 {
                let v = product[idx]
                    .wrapping_add((a[i] as u16).wrapping_mul(b[j] as u16))
                    .wrapping_add(carry);
                product[idx] = v & 0xFF;
                carry = v >> 8;
            }
        }
        // Propagate remaining carry.
        let mut idx = i.wrapping_add(32);
        while carry > 0 && idx < 64 {
            let v = product[idx].wrapping_add(carry);
            product[idx] = v & 0xFF;
            carry = v >> 8;
            idx = idx.wrapping_add(1);
        }
    }

    // Add c.
    let mut carry = 0u16;
    for i in 0..32 {
        let v = product[i].wrapping_add(c[i] as u16).wrapping_add(carry);
        product[i] = v & 0xFF;
        carry = v >> 8;
    }
    let mut idx = 32;
    while carry > 0 && idx < 64 {
        let v = product[idx].wrapping_add(carry);
        product[idx] = v & 0xFF;
        carry = v >> 8;
        idx = idx.wrapping_add(1);
    }

    // Convert to bytes.
    let mut result = [0u8; 64];
    for i in 0..64 {
        result[i] = product[i] as u8;
    }

    sc_reduce(&result)
}

/// Check that a scalar is canonical (< L).
fn sc_is_canonical(s: &[u8; 32]) -> bool {
    // Compare s < L byte by byte from MSB.
    for i in (0..32).rev() {
        if s[i] < ED25519_L[i] { return true; }
        if s[i] > ED25519_L[i] { return false; }
    }
    false // Equal to L is not canonical.
}

// ---------------------------------------------------------------------------
// Ed25519 + SHA-512 self-tests
// ---------------------------------------------------------------------------

/// Self-test for SHA-512 and Ed25519.
pub fn self_test_ed25519() -> crate::error::KernelResult<()> {
    use crate::serial_println;
    serial_println!("[crypto] Running Ed25519/SHA-512 self-test...");

    // Test 1: SHA-512 standard test vectors (NIST FIPS 180-4).
    {
        let hash = sha512(b"");
        assert_eq!(hash[0..8], [0xcf, 0x83, 0xe1, 0x35, 0x7e, 0xef, 0xb8, 0xbd]);
        serial_println!("[crypto]   SHA-512 empty: OK");

        let hash = sha512(b"abc");
        assert_eq!(hash[0..4], [0xdd, 0xaf, 0x35, 0xa1]);
        serial_println!("[crypto]   SHA-512 abc: OK");

        let msg = b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu";
        let hash = sha512(msg);
        assert_eq!(hash[0], 0x8e);
        serial_println!("[crypto]   SHA-512 multi-block: OK");
    }

    // Test 2: Ed25519 point arithmetic sanity checks.
    {
        let bp = EdPoint::basepoint();
        let bp_enc = bp.encode();

        // Basepoint encoding: 5866...66 (y = 4/5 mod p).
        assert_eq!(bp_enc[0], 0x58, "Basepoint byte 0");
        for i in 1..32 {
            assert_eq!(bp_enc[i], 0x66, "Basepoint byte {}", i);
        }
        serial_println!("[crypto]   Basepoint encoding: OK");

        // Basepoint x-coordinate (known value).
        let expected_bx: [u8; 32] = [
            0x1a, 0xd5, 0x25, 0x8f, 0x60, 0x2d, 0x56, 0xc9,
            0xb2, 0xa7, 0x25, 0x95, 0x60, 0xc7, 0x2c, 0x69,
            0x5c, 0xdc, 0xd6, 0xfd, 0x31, 0xe2, 0xa4, 0xc0,
            0xfe, 0x53, 0x6e, 0xcd, 0xd3, 0x36, 0x69, 0x21,
        ];
        assert_eq!(bp.x.to_bytes(), expected_bx, "Basepoint Bx");
        serial_println!("[crypto]   Basepoint Bx: OK");

        // [1]*B == B.
        let scalar_one: [u8; 32] = { let mut s = [0u8; 32]; s[0] = 1; s };
        assert_eq!(bp.scalar_mul(&scalar_one).encode(), bp_enc, "[1]*B must equal B");
        serial_println!("[crypto]   [1]*B == B: OK");

        // B+B == [2]*B (both methods agree).
        let two_b_add = bp.add_point(bp).encode();
        let scalar_two: [u8; 32] = { let mut s = [0u8; 32]; s[0] = 2; s };
        let two_b_scalar = bp.scalar_mul(&scalar_two).encode();
        assert_eq!(two_b_add, two_b_scalar, "B+B must equal [2]*B");

        // [2]*B matches known reference value.
        let two_b_expected: [u8; 32] = [
            0xc9, 0xa3, 0xf8, 0x6a, 0xae, 0x46, 0x5f, 0x0e,
            0x56, 0x51, 0x38, 0x64, 0x51, 0x0f, 0x39, 0x97,
            0x56, 0x1f, 0xa2, 0xc9, 0xe8, 0x5e, 0xa2, 0x1d,
            0xc2, 0x29, 0x23, 0x09, 0xf3, 0xcd, 0x60, 0x22,
        ];
        assert_eq!(two_b_scalar, two_b_expected, "[2]*B absolute value");
        serial_println!("[crypto]   [2]*B: OK");

        // [4]*B consistency: [2]*B + [2]*B == [4]*B.
        let two_b_pt = bp.add_point(bp);
        let scalar_four: [u8; 32] = { let mut s = [0u8; 32]; s[0] = 4; s };
        assert_eq!(two_b_pt.add_point(two_b_pt).encode(),
                   bp.scalar_mul(&scalar_four).encode(), "[4]*B consistency");
        serial_println!("[crypto]   [4]*B: OK");

        // [8]*B on curve.
        let scalar_eight: [u8; 32] = { let mut s = [0u8; 32]; s[0] = 8; s };
        let eight_enc = bp.scalar_mul(&scalar_eight).encode();
        assert!(EdPoint::decode(&eight_enc).is_some(), "[8]*B must be on curve");
        serial_println!("[crypto]   [8]*B on curve: OK");

        // [L-1]*B == -B (verifies large scalar multiplication).
        let l_minus_1: [u8; 32] = [
            0xec, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58,
            0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
        ];
        let neg_b = bp.scalar_mul(&l_minus_1).encode();
        let mut expected_neg_b = bp_enc;
        expected_neg_b[31] ^= 0x80; // Negate = flip x sign bit.
        assert_eq!(neg_b, expected_neg_b, "[L-1]*B must equal -B");
        serial_println!("[crypto]   [L-1]*B == -B: OK");
    }

    // Test 3: Ed25519 key generation — RFC 8032 §7.1 test vector 1.
    {
        let seed: [u8; 32] = [
            0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60,
            0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
            0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19,
            0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
        ];
        // Correct public key from RFC 8032 §7.1 TEST 1.
        let expected_pub: [u8; 32] = [
            0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7,
            0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
            0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25,
            0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
        ];

        let kp = ed25519_keypair(&seed);
        assert_eq!(kp.public, expected_pub, "Ed25519 TV1 public key");
        serial_println!("[crypto]   Ed25519 keygen TV1: OK");

        // Sign empty message (RFC 8032 §7.1 TEST 1).
        let sig = ed25519_sign(&seed, b"");
        let expected_sig: [u8; 64] = [
            0xe5, 0x56, 0x43, 0x00, 0xc3, 0x60, 0xac, 0x72,
            0x90, 0x86, 0xe2, 0xcc, 0x80, 0x6e, 0x82, 0x8a,
            0x84, 0x87, 0x7f, 0x1e, 0xb8, 0xe5, 0xd9, 0x74,
            0xd8, 0x73, 0xe0, 0x65, 0x22, 0x49, 0x01, 0x55,
            0x5f, 0xb8, 0x82, 0x15, 0x90, 0xa3, 0x3b, 0xac,
            0xc6, 0x1e, 0x39, 0x70, 0x1c, 0xf9, 0xb4, 0x6b,
            0xd2, 0x5b, 0xf5, 0xf0, 0x59, 0x5b, 0xbe, 0x24,
            0x65, 0x51, 0x41, 0x43, 0x8e, 0x7a, 0x10, 0x0b,
        ];
        assert_eq!(sig, expected_sig, "Ed25519 TV1 signature");
        serial_println!("[crypto]   Ed25519 sign TV1: OK");

        // Verify signature.
        assert!(ed25519_verify(&kp.public, b"", &sig), "Ed25519 TV1 verify");
        serial_println!("[crypto]   Ed25519 verify TV1: OK");
    }

    // Test 4: Ed25519 test vector 2 (1-byte message: 0x72).
    {
        let seed: [u8; 32] = [
            0x4c, 0xcd, 0x08, 0x9b, 0x28, 0xff, 0x96, 0xda,
            0x9d, 0xb6, 0xc3, 0x46, 0xec, 0x11, 0x4e, 0x0f,
            0x5b, 0x8a, 0x31, 0x9f, 0x35, 0xab, 0xa6, 0x24,
            0xda, 0x8c, 0xf6, 0xed, 0x4f, 0xb8, 0xa6, 0xfb,
        ];
        let expected_pub: [u8; 32] = [
            0x3d, 0x40, 0x17, 0xc3, 0xe8, 0x43, 0x89, 0x5a,
            0x92, 0xb7, 0x0a, 0xa7, 0x4d, 0x1b, 0x7e, 0xbc,
            0x9c, 0x98, 0x2c, 0xcf, 0x2e, 0xc4, 0x96, 0x8c,
            0xc0, 0xcd, 0x55, 0xf1, 0x2a, 0xf4, 0x66, 0x0c,
        ];

        let kp = ed25519_keypair(&seed);
        assert_eq!(kp.public, expected_pub, "Ed25519 TV2 public key");
        serial_println!("[crypto]   Ed25519 keygen TV2: OK");

        let sig = ed25519_sign(&seed, &[0x72]);
        assert!(ed25519_verify(&kp.public, &[0x72], &sig), "Ed25519 TV2 sign+verify");
        serial_println!("[crypto]   Ed25519 sign+verify TV2: OK");

        // Verify with wrong message fails.
        assert!(!ed25519_verify(&kp.public, &[0x73], &sig), "Ed25519 TV2 reject");
        serial_println!("[crypto]   Ed25519 verify reject: OK");
    }

    // Test 5: Scalar reduction (sc_reduce produces canonical output).
    {
        let mut input = [0u8; 64];
        input[0] = 0xFF;
        input[31] = 0xFF;
        let reduced = sc_reduce(&input);
        assert!(sc_is_canonical(&reduced) || reduced == [0u8; 32],
            "sc_reduce must produce canonical scalar");
        serial_println!("[crypto]   Scalar reduction: OK");
    }

    serial_println!("[crypto] Ed25519/SHA-512 self-test PASSED (5 tests)");
    Ok(())
}
