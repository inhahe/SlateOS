//! Cryptographic primitives for the kernel.
//!
//! Provides:
//! - **SHA-256** for file content hashing and integrity verification
//!   (the shared `sha2` crate, re-exported below)
//! - **CRC32C** (Castagnoli) for ext4 metadata checksums, and **CRC-32**
//! - **HMAC-SHA256** (RFC 2104) and **ChaCha20** (RFC 8439), for the kernel
//!   shell's file encryption (`fs::encrypt`)
//!
//! The rest of what this module once held -- HKDF, Poly1305, the
//! ChaCha20-Poly1305 AEAD, X25519, SHA-512 and Ed25519 -- served the
//! kernel's own TLS and SSH, and went with the in-kernel network
//! applications (design-decisions §934). Cryptography a program needs is
//! the vetted RustCrypto crates in `rustcrypto/` (§539: primitives are
//! ported, never written here).
//!
//! ## References
//!
//! - SHA-256: FIPS 180-4 (Secure Hash Standard)
//! - CRC32C: RFC 3720 appendix B (iSCSI), polynomial 0x1EDC6F41
//! - HMAC: RFC 2104 (HMAC: Keyed-Hashing for Message Authentication)
//! - ChaCha20: RFC 8439 (formerly RFC 7539)

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// SHA-256 -- re-exported from the shared `sha2` crate
// ---------------------------------------------------------------------------

// This module used to carry its own FIPS 180-4 SHA-256: the H0/K constant
// tables, a `Sha256` struct, and a `compress` block function. It was one of 26
// hand-written copies in this tree (see known-issues.md ->
// C-SHA-256-IS-IMPLEMENTED-ELEVEN-TIMES-IN-THIS-TREE, whose title undercounts).
//
// The kernel's copy was correct, and it was not a deliberate exception -- no
// constant-time property, no assembly path, nothing the shared crate lacks.
// What made it worth deleting is the failure mode of the ones that are not
// checked: SHA-256 with a single wrong round constant still returns 32
// plausible-looking bytes and only ever disagrees with the rest of the world.
// Correctness spread over 26 copies is 26 independent pieces of luck.
//
// The digest type and the streaming state are unchanged for callers --
// `sha2::Sha256` exposes the same `new`/`update`/`finalize`, so all 45 call
// sites across fs/, net/, kshell.rs and bench.rs are untouched by this -- and
// `sha2` is no_std with no `alloc`, so nothing here allocates that did not
// before. It adds `Clone`, `Default` and `Debug`, which the local struct had
// none of.
//
// Performance is expected to be a wash, not a win. Lane C measured 22% on
// `gui/credentials`, but that came from deleting a per-call `Vec` used to hold
// the padded message; this implementation already padded into a fixed
// `[u8; 64]` with a `[u32; 8]` state and allocated nothing on the hashing
// path, so there is no allocation here to remove.
//
// One thing that DOES move: `compress` leaves `kernel::crypto` for the `sha2`
// crate, so its symbol changes from `..6crypto8compress..` to
// `..4sha28compress..` and the linker places it at a different address. Under
// QEMU's TCG that alone can shift `crypto_sha256_64B` several-fold with the
// machine code byte-identical -- see known-issues.md ->
// A-A-4x-CRYPTO-"REGRESSION"-BISECTS-TO-A-COMMIT-THAT-ONLY-EDITS-audio_mixer.rs
// and design-decisions.md 228. If the crypto benchmarks move across this
// commit, read `hot_symbols` in the benchmark record before reading the code,
// and do not pad or reorder anything to chase it. The `HOT_SYMBOLS` pattern in
// scripts/bench-history.py is updated in this same commit for exactly that
// reason: it matches on the module path, and it would otherwise go blind at
// the moment it is most useful.
pub use sha2::{Sha256, sha256};

/// SHA-256 output size in bytes.
pub const SHA256_DIGEST_SIZE: usize = 32;

/// Convenience function: compute SHA-256 and return as a `Vec<u8>`.
///
/// Kept here rather than pushed into `sha2`: the crate is deliberately
/// `alloc`-free so the kernel can link it, and several callers here want the
/// `Vec` form. None of them are on a hot path.
pub fn sha256_vec(data: &[u8]) -> Vec<u8> {
    sha256(data).to_vec()
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
        0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9,
        0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52,
        0xb8, 0x55,
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
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22,
        0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00,
        0x15, 0xad,
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
        0x24, 0x8d, 0x6a, 0x61, 0xd2, 0x06, 0x38, 0xb8, 0xe5, 0xc0, 0x26, 0x93, 0x0c, 0x3e, 0x60,
        0x39, 0xa3, 0x3c, 0xe4, 0x59, 0x64, 0xff, 0x21, 0x67, 0xf6, 0xec, 0xed, 0xd4, 0x19, 0xdb,
        0x06, 0xc1,
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
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22,
        0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00,
        0x15, 0xad,
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
/// value processed through 8 rounds of the bit-at-a-time algorithm. A
/// `static`, read where it lies, where a `const` would be copied onto the
/// stack at each use in an unoptimised build (as crc32's 8 KiB tables were,
/// until they overflowed a task's stack).
static CRC32C_TABLE: [u32; 256] = {
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
        crate::serial_println!(
            "[crypto]   FAIL: CRC32C(\"123456789\") = {:#010X}, expected 0xE3069283",
            check
        );
        return Err(crate::error::KernelError::InternalError);
    }
    crate::serial_println!(
        "[crypto]   CRC32C(\"123456789\") = {:#010X} (correct)",
        check
    );

    // Test vector 2: empty input.
    let empty = crc32c(b"");
    if empty != 0x0000_0000 {
        crate::serial_println!(
            "[crypto]   FAIL: CRC32C(\"\") = {:#010X}, expected 0x00000000",
            empty
        );
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
        crate::serial_println!(
            "[crypto]   FAIL: CRC32C(32 zeros) = {:#010X}, expected 0x8A9136AA",
            z_crc
        );
        return Err(crate::error::KernelError::InternalError);
    }
    crate::serial_println!("[crypto]   CRC32C(32 zeros) = {:#010X} (correct)", z_crc);

    // Test vector 4: chaining (incremental computation).
    let raw_seed = crc32c_raw(!0u32, b"1234");
    let chained = crc32c_seed(raw_seed, b"56789");
    if chained != 0xE306_9283 {
        crate::serial_println!(
            "[crypto]   FAIL: chained CRC32C = {:#010X}, expected 0xE3069283",
            chained
        );
        return Err(crate::error::KernelError::InternalError);
    }
    crate::serial_println!("[crypto]   Chained CRC32C = {:#010X} (correct)", chained);

    crate::serial_println!("[crypto] CRC32C self-test passed.");
    Ok(())
}

// ===========================================================================
// CRC-32 (ISO 3309 / IEEE 802.3) — a *different* polynomial from CRC32C
// ===========================================================================

/// CRC-32 (ISO 3309 / IEEE 802.3), re-exported from the `crc32` crate.
///
/// This is the CRC of gzip, ZIP, PNG, Ethernet, RAR, 7-Zip and F2FS — *not*
/// the Castagnoli CRC32C above, which uses `0x82F63B78` and produces entirely
/// different values. The two are easy to confuse and impossible to confuse
/// safely: a checksum computed with the wrong one verifies against nothing,
/// and the failure looks like data corruption rather than like a bug. Keeping
/// them adjacent in this file is deliberate for that reason, and the
/// re-export keeps the adjacency after the move.
///
/// The table and the three functions used to live here. The comment on them
/// already recorded that four private bit-at-a-time copies of this polynomial
/// (`rar.rs`, `sevenz.rs`, `properties.rs`, `compress.rs`) had been
/// consolidated onto this one — but a module of a *binary* crate cannot be
/// depended on, so the consolidation stopped at the kernel's edge and the
/// next two callers outside it (`deflate`'s gzip trailer, `gui/imagecodec`'s
/// PNG chunk CRCs) each had to start a fifth and sixth copy. Hence a leaf
/// crate, on the model of `sha2`, `sha1` and `md5`.
///
/// [`self_test_crc32`] below stays in the kernel: the crate has its own
/// `#[cfg(test)]` vectors, but `kernel/Cargo.toml` sets `test = false`, so the
/// boot battery is the only thing that ever runs an assertion here.
pub use crc32::{crc32, crc32_raw, crc32_seed};

/// Self-test for CRC-32 (ISO 3309).
///
/// # Errors
///
/// [`crate::error::KernelError::InternalError`] if any vector mismatches.
pub fn self_test_crc32() -> Result<(), crate::error::KernelError> {
    crate::serial_println!("[crypto] Running CRC-32 (ISO 3309) self-test...");

    // The standard check value. Every reflected-IEEE implementation agrees on
    // it, which is what makes it the right vector to guard the table against a
    // transcription error in the polynomial.
    let check = crc32(b"123456789");
    if check != 0xCBF4_3926 {
        crate::serial_println!(
            "[crypto]   FAIL: CRC32(\"123456789\") = {:#010X}, expected 0xCBF43926",
            check
        );
        return Err(crate::error::KernelError::InternalError);
    }

    // The empty input must be 0, not `!0`: it proves both inversions are
    // applied and that they are applied in the right order.
    if crc32(b"") != 0 {
        crate::serial_println!("[crypto]   FAIL: CRC32(\"\") is not zero");
        return Err(crate::error::KernelError::InternalError);
    }

    // Chaining across a split must equal the one-shot value, which is the
    // property F2FS's checkpoint checksum depends on — it covers the block in
    // two pieces, skipping the four bytes that hold the checksum itself.
    let chained = crc32_seed(crc32_raw(!0u32, b"1234"), b"56789");
    if chained != 0xCBF4_3926 {
        crate::serial_println!(
            "[crypto]   FAIL: chained CRC32 = {:#010X}, expected 0xCBF43926",
            chained
        );
        return Err(crate::error::KernelError::InternalError);
    }

    // CRC-32 and CRC32C must not agree on a non-trivial input. A single
    // mistyped constant that made one table equal the other would leave every
    // vector above passing while silently checksumming F2FS with Castagnoli.
    if crc32(b"123456789") == crc32c(b"123456789") {
        crate::serial_println!("[crypto]   FAIL: CRC-32 and CRC32C agree — same table?");
        return Err(crate::error::KernelError::InternalError);
    }

    crate::serial_println!(
        "[crypto] CRC-32 self-test passed (check value {:#010X}).",
        check
    );
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
        state[13 + i] =
            u32::from_le_bytes([nonce[off], nonce[off + 1], nonce[off + 2], nonce[off + 3]]);
    }

    // Save initial state for final addition.
    let initial = state;

    // 20 rounds (10 double-rounds of column + diagonal quarter-rounds).
    for _ in 0..10 {
        // Column rounds.
        quarter_round(&mut state, 0, 4, 8, 12);
        quarter_round(&mut state, 1, 5, 9, 13);
        quarter_round(&mut state, 2, 6, 10, 14);
        quarter_round(&mut state, 3, 7, 11, 15);
        // Diagonal rounds.
        quarter_round(&mut state, 0, 5, 10, 15);
        quarter_round(&mut state, 1, 6, 11, 12);
        quarter_round(&mut state, 2, 7, 8, 13);
        quarter_round(&mut state, 3, 4, 9, 14);
    }

    // Add initial state (modular addition per word).
    for i in 0..16 {
        state[i] = state[i].wrapping_add(initial[i]);
    }

    // Serialize to little-endian bytes.
    let mut out = [0u8; 64];
    for i in 0..16 {
        let bytes = state[i].to_le_bytes();
        out[i * 4] = bytes[0];
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
// Self-tests for HMAC-SHA256 and ChaCha20
// ===========================================================================

/// One test vector's verdict, `ok`, reported under `what`: an error, which
/// the self-test returns, rather than a panic, which would halt the boot.
fn vector(ok: bool, what: &str) -> crate::error::KernelResult<()> {
    if ok {
        crate::serial_println!("[crypto]   {}: PASSED", what);
        Ok(())
    } else {
        crate::serial_println!("[crypto]   FAIL: {}", what);
        Err(crate::error::KernelError::InternalError)
    }
}

/// Run the HMAC-SHA256 and ChaCha20 self-tests: their RFC test vectors.
pub fn self_test_hmac_chacha20() -> crate::error::KernelResult<()> {
    crate::serial_println!("[crypto] Running HMAC-SHA256 and ChaCha20 self-tests...");

    // --- HMAC-SHA256 test vector (RFC 4231 §4.2, test case 1) ---
    {
        let key = [0x0Bu8; 20];
        let data = b"Hi There";
        let expected: [u8; 32] = [
            0xb0, 0x34, 0x4c, 0x61, 0xd8, 0xdb, 0x38, 0x53, 0x5c, 0xa8, 0xaf, 0xce, 0xaf, 0x0b,
            0xf1, 0x2b, 0x88, 0x1d, 0xc2, 0x00, 0xc9, 0x83, 0x3d, 0xa7, 0x26, 0xe9, 0x37, 0x6c,
            0x2e, 0x32, 0xcf, 0xf7,
        ];
        vector(
            hmac_sha256(&key, data) == expected,
            "HMAC-SHA256 test 1 (RFC 4231)",
        )?;
    }

    // --- HMAC-SHA256 test vector (RFC 4231 §4.3, test case 2) ---
    {
        let key = b"Jefe";
        let data = b"what do ya want for nothing?";
        let expected: [u8; 32] = [
            0x5b, 0xdc, 0xc1, 0x46, 0xbf, 0x60, 0x75, 0x4e, 0x6a, 0x04, 0x24, 0x26, 0x08, 0x95,
            0x75, 0xc7, 0x5a, 0x00, 0x3f, 0x08, 0x9d, 0x27, 0x39, 0x83, 0x9d, 0xec, 0x58, 0xb9,
            0x64, 0xec, 0x38, 0x43,
        ];
        vector(
            hmac_sha256(key, data) == expected,
            "HMAC-SHA256 test 2 (RFC 4231)",
        )?;
    }

    // --- ChaCha20 test vector (RFC 8439 §2.4.2) ---
    {
        let key: [u8; 32] = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
            0x1c, 0x1d, 0x1e, 0x1f,
        ];
        let nonce: [u8; 12] = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x4a, 0x00, 0x00, 0x00, 0x00,
        ];
        // Plaintext: "Ladies and Gentlemen of the class of '99: ..."
        let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let mut data = plaintext.to_vec();
        chacha20_xor(&key, &nonce, 1, &mut data);

        // First few bytes of expected ciphertext from RFC 8439 §2.4.2.
        let expected_start: [u8; 16] = [
            0x6e, 0x2e, 0x35, 0x9a, 0x25, 0x68, 0xf9, 0x80, 0x41, 0xba, 0x07, 0x28, 0xdd, 0x0d,
            0x69, 0x81,
        ];
        vector(
            data.get(..16) == Some(&expected_start[..]),
            "ChaCha20 ciphertext (RFC 8439 §2.4.2)",
        )?;

        // Decrypt and verify round-trip.
        chacha20_xor(&key, &nonce, 1, &mut data);
        vector(
            data.as_slice() == plaintext.as_slice(),
            "ChaCha20 round-trip",
        )?;
    }

    crate::serial_println!("[crypto] All HMAC-SHA256 and ChaCha20 self-tests PASSED");
    Ok(())
}
