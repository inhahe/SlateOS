//! SHA-256, written once, checked against the FIPS 180-4 vectors.
//!
//! # Why this crate exists
//!
//! At the time of writing, **26 files in this tree contain their own SHA-256**
//! — the same round constants, the same eight initial words, the same message
//! schedule, pasted 26 times. `apps/backup`, `apps/diskimager`,
//! `apps/lockscreen`, `gui/credentials`, `init/login`, `kernel/build.rs`,
//! `kernel/src/crypto.rs`, `posix/src/sha2.rs`, and eighteen tools under
//! `userspace/` each have one.
//!
//! Every copy is correct today. That is the problem: it is 26 pieces of luck
//! rather than one piece of design. A cryptographic hash is exactly the kind
//! of code where "correct today" is not the same as "correct", because the
//! failure mode is silent. A SHA-256 with a wrong constant still produces
//! 32 plausible-looking bytes for every input; it just produces the *wrong*
//! ones, and nothing downstream can tell. Passwords stop verifying against
//! stored hashes, backup manifests stop matching the files they describe, and
//! the package store's content addresses stop addressing content — all with no
//! error anywhere. The only thing standing between the tree and that outcome
//! is that someone typed 64 hexadecimal constants correctly, 26 times.
//!
//! It is also 26 places to fix anything. The three lanes' copies have already
//! drifted in shape — one is a one-shot function, one is a streaming struct,
//! one returns hex and one returns bytes — so a fix found in any of them has
//! to be re-derived for the others rather than merged.
//!
//! # What this crate is not
//!
//! It is a hand-written implementation, like the copies it replaces. Folding
//! 26 of them into one does not make the primitive vetted; it makes it
//! *reviewable*, and gives a single place for a vetted implementation to land
//! later. Whether this project should be writing its own cryptographic
//! primitives at all is `open-questions.md` → C-Q5, and is not settled here.
//!
//! There is deliberately no HMAC, no HKDF and no truncated variant: adding
//! surface that nothing calls yet would be the same speculative work this
//! crate exists to undo.
//!
//! # Constant-time-ness
//!
//! SHA-256 as written here is data-independent in its control flow and its
//! memory access pattern — every input of a given length takes the same path
//! through the same words — so it does not leak its input through timing.
//! **Comparing two digests still can.** `a == b` on a `[u8; 32]` returns at
//! the first differing byte, which reveals how many leading bytes matched; if
//! an attacker can submit guesses and time them, that recovers a digest one
//! byte at a time. Use [`eq_constant_time`] whenever the digest is a secret or
//! is being checked against one.
//!
//! # Usage
//!
//! ```
//! # use sha2::{sha256, hex};
//! let digest = sha256(b"abc");
//! assert_eq!(
//!     hex(&digest).as_str(),
//!     "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
//! );
//! ```
//!
//! For data that does not arrive all at once — a file read in 8 KiB chunks,
//! say — use the streaming form:
//!
//! ```
//! # use sha2::Sha256;
//! let mut hasher = Sha256::new();
//! hasher.update(b"a");
//! hasher.update(b"bc");
//! assert_eq!(hasher.finalize(), sha2::sha256(b"abc"));
//! ```

#![no_std]

use core::fmt;

/// The block size SHA-256 compresses, in bytes.
const BLOCK_LEN: usize = 64;

/// Bytes of digest output.
pub const DIGEST_LEN: usize = 32;

/// Byte offset within a block at which the 64-bit length field begins.
const LENGTH_OFFSET: usize = BLOCK_LEN - 8;

/// Initial hash value: the first 32 bits of the fractional parts of the square
/// roots of the first eight primes (FIPS 180-4 §5.3.3).
const INIT: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// Round constants: the first 32 bits of the fractional parts of the cube
/// roots of the first sixty-four primes (FIPS 180-4 §4.2.2).
const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// Hash `data` in one call.
///
/// The common case, and the one 24 of the 26 copies were being used for.
#[must_use]
pub fn sha256(data: &[u8]) -> [u8; DIGEST_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize()
}

/// An incremental SHA-256 hasher, for input that does not arrive all at once.
///
/// Holds one partial block — 64 bytes — regardless of how much has been
/// hashed, so a file of any size can be digested from a fixed-size read
/// buffer without ever holding the whole thing in memory.
#[derive(Clone)]
pub struct Sha256 {
    /// Chaining value: the hash of everything compressed so far.
    state: [u32; 8],
    /// Bytes received since the last full block was compressed.
    buffer: [u8; BLOCK_LEN],
    /// How many of `buffer`'s bytes are live. Invariant: `< BLOCK_LEN`
    /// on entry to and exit from every public method, because a full buffer is
    /// compressed immediately rather than held.
    buffered: usize,
    /// Total bytes fed in, which SHA-256 appends (times eight) as the final
    /// 64-bit field. Wrapping is correct rather than merely tolerable: the
    /// field is defined modulo 2^64 by the standard, and reaching it would
    /// take 16 exabytes.
    total_len: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Sha256 {
    /// Deliberately opaque.
    ///
    /// The buffer holds up to 64 bytes of whatever is being hashed, which in
    /// this tree is routinely a password or a key. A derived `Debug` would put
    /// that in any log line that formatted the hasher.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sha256")
            .field("bytes_hashed", &self.total_len)
            .finish_non_exhaustive()
    }
}

impl Sha256 {
    /// Start a new hash.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: INIT,
            buffer: [0; BLOCK_LEN],
            buffered: 0,
            total_len: 0,
        }
    }

    /// Feed `data` in. Splitting the same input differently across calls gives
    /// the same digest.
    pub fn update(&mut self, data: &[u8]) {
        self.total_len = self
            .total_len
            .wrapping_add(data.len().try_into().unwrap_or(u64::MAX));

        let mut rest = data;

        // Top up a partial block first, so that the whole-block loop below can
        // read straight out of the caller's slice.
        if self.buffered > 0 {
            let space = BLOCK_LEN.saturating_sub(self.buffered);
            let take = space.min(rest.len());
            let end = self.buffered.saturating_add(take);
            if let (Some(dst), Some(src)) = (self.buffer.get_mut(self.buffered..end), rest.get(..take))
            {
                dst.copy_from_slice(src);
            }
            self.buffered = end;
            rest = rest.get(take..).unwrap_or(&[]);

            if self.buffered < BLOCK_LEN {
                // `take` was capped by `rest.len()` rather than by `space`, so
                // `rest` is now empty and there is nothing further to do. This
                // early return is what keeps the remainder handling at the end
                // of the function from overwriting the partial block.
                return;
            }
            let block = self.buffer;
            compress(&mut self.state, &block);
            self.buffered = 0;
        }

        let mut blocks = rest.chunks_exact(BLOCK_LEN);
        for chunk in &mut blocks {
            if let Some(block) = chunk.first_chunk::<BLOCK_LEN>() {
                compress(&mut self.state, block);
            }
        }

        let remainder = blocks.remainder();
        if let Some(dst) = self.buffer.get_mut(..remainder.len()) {
            dst.copy_from_slice(remainder);
        }
        self.buffered = remainder.len();
    }

    /// Finish, returning the 32-byte digest.
    ///
    /// Consumes the hasher: SHA-256's padding is part of the hashed message,
    /// so a state that has been finalised cannot be extended. (Taking `self`
    /// by value is what makes that a compile error rather than a wrong
    /// answer.)
    #[must_use]
    pub fn finalize(mut self) -> [u8; DIGEST_LEN] {
        // Captured before padding is fed in, because padding must not count
        // towards the length it encodes.
        let bit_len = self.total_len.wrapping_mul(8);

        // The padding is a 1 bit, then zeros, then the 64-bit length, arranged
        // so the length lands flush against the end of a block. Longest case:
        // one 0x80 byte, 63 zeros, 8 length bytes.
        let mut pad = [0u8; 1 + BLOCK_LEN - 1 + 8];
        if let Some(first) = pad.first_mut() {
            *first = 0x80;
        }
        // Zeros needed so that `buffered + 1 + zeros ≡ LENGTH_OFFSET (mod 64)`.
        // `buffered` is `< 64` by the struct invariant, so the subtraction
        // cannot go negative: the smallest numerator is 119 - 63 = 56.
        let zeros = (BLOCK_LEN
            .saturating_sub(1)
            .saturating_add(LENGTH_OFFSET)
            .saturating_sub(self.buffered))
            % BLOCK_LEN;
        let len_at = zeros.saturating_add(1);
        let total = len_at.saturating_add(8);
        if let Some(dst) = pad.get_mut(len_at..total) {
            dst.copy_from_slice(&bit_len.to_be_bytes());
        }
        if let Some(padding) = pad.get(..total) {
            self.update(padding);
        }

        let mut out = [0u8; DIGEST_LEN];
        for (slot, word) in out.chunks_exact_mut(4).zip(self.state) {
            slot.copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

/// One application of the SHA-256 compression function to a 64-byte block.
fn compress(state: &mut [u32; 8], block: &[u8; BLOCK_LEN]) {
    // Message schedule. The first sixteen words are the block, big-endian.
    let mut w = [0u32; 64];
    for (word, bytes) in w.iter_mut().zip(block.chunks_exact(4)) {
        *word = u32::from_be_bytes(<[u8; 4]>::try_from(bytes).unwrap_or([0; 4]));
    }

    // The remaining forty-eight are a recurrence over the previous sixteen.
    // Taking those sixteen as a `last_chunk` of the filled prefix, rather than
    // as `w[i - 15]` and friends, is what lets the offsets be constants: the
    // window is a `&[u32; 16]`, so `prev[1]` is checked at compile time where
    // `w[i - 15]` would be a bounds check plus a subtraction that has to be
    // argued about.
    for i in 16..64 {
        let Some(prev) = w.get(..i).and_then(<[u32]>::last_chunk::<16>) else {
            // Unreachable: `i < 64 == w.len()` and `i >= 16`.
            continue;
        };
        // prev[0] is w[i-16], prev[1] is w[i-15], prev[9] is w[i-7],
        // prev[14] is w[i-2].
        let s0 = prev[1].rotate_right(7) ^ prev[1].rotate_right(18) ^ (prev[1] >> 3);
        let s1 = prev[14].rotate_right(17) ^ prev[14].rotate_right(19) ^ (prev[14] >> 10);
        let next = prev[0]
            .wrapping_add(s0)
            .wrapping_add(prev[9])
            .wrapping_add(s1);
        if let Some(slot) = w.get_mut(i) {
            *slot = next;
        }
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;

    // Sixty-four rounds, one per (constant, schedule word) pair. Zipping the
    // two arrays keeps the round counter out of it entirely.
    for (k, word) in K.iter().zip(w) {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(*k)
            .wrapping_add(word);
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

    let round = [a, b, c, d, e, f, g, h];
    for (acc, delta) in state.iter_mut().zip(round) {
        *acc = acc.wrapping_add(delta);
    }
}

/// Compare two byte strings without revealing *where* they first differ.
///
/// `a == b` stops at the first mismatch, so it takes measurably longer the
/// more leading bytes agree. Against a check an attacker can repeat — a token,
/// a password verifier, a file digest they get to supply — that timing
/// recovers the secret one byte at a time. This reads every byte of both,
/// always.
///
/// Length is not secret here and is compared normally; two digests are always
/// the same length anyway.
#[must_use]
pub fn eq_constant_time(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

/// A digest rendered as 64 lowercase hexadecimal characters.
///
/// A wrapper around a fixed array rather than a `String` so that the `no_std`,
/// no-`alloc` callers — the kernel, the bare-metal services — can print a
/// digest too. Derefs to `str` via [`Hex::as_str`] and implements [`Display`],
/// so it substitutes for a `String` at nearly every use site.
///
/// [`Display`]: fmt::Display
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Hex([u8; DIGEST_LEN * 2]);

impl Hex {
    /// The hexadecimal text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Every byte written by `hex` is from `b"0123456789abcdef"`, so this
        // is ASCII by construction; the fallback is unreachable.
        core::str::from_utf8(&self.0).unwrap_or("")
    }
}

impl fmt::Display for Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Render a digest as lowercase hexadecimal.
#[must_use]
pub fn hex(digest: &[u8; DIGEST_LEN]) -> Hex {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = [b'0'; DIGEST_LEN * 2];
    for (pair, byte) in out.chunks_exact_mut(2).zip(digest) {
        if let (Some(hi), Some(lo)) = (pair.first_mut(), DIGITS.get(usize::from(byte >> 4))) {
            *hi = *lo;
        }
        if let (Some(lo_slot), Some(lo)) = (pair.last_mut(), DIGITS.get(usize::from(byte & 0x0f))) {
            *lo_slot = *lo;
        }
    }
    Hex(out)
}

/// Hash `data` and render the result as hexadecimal, in one call.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> Hex {
    hex(&sha256(data))
}

// The five defensive lints the workspace turns on are for production code: a
// test that indexes a fixed-size fixture, or unwraps a value it just
// constructed, is *asserting*, and an assertion that fails by panicking is a
// test doing its job rather than a robustness hole.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
#[cfg(test)]
mod tests {
    // The crate is `no_std`; the test harness is not, and a couple of these
    // tests want a `String` to assert against.
    extern crate std;

    use super::*;

    // -- The FIPS 180-4 vectors --

    #[test]
    fn fips_empty_string() {
        assert_eq!(
            sha256_hex(b"").as_str(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn fips_abc() {
        assert_eq!(
            sha256_hex(b"abc").as_str(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn fips_two_block_message() {
        // 56 bytes: one byte too long for a single block once the padding is
        // added, so this is the vector that catches a broken block boundary.
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq").as_str(),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn fips_multi_block_message() {
        assert_eq!(
            sha256_hex(
                b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmno\
                  ijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
            )
            .as_str(),
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1"
        );
    }

    #[test]
    fn fips_one_million_a() {
        // The long vector. Also the only test here that exercises the
        // streaming path across many blocks.
        let mut hasher = Sha256::new();
        let chunk = [b'a'; 1000];
        for _ in 0..1000 {
            hasher.update(&chunk);
        }
        assert_eq!(
            hex(&hasher.finalize()).as_str(),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    // -- Boundaries around the block size and the length field --

    #[test]
    fn every_length_up_to_three_blocks_streams_the_same_as_one_shot() {
        // The padding arithmetic has three regimes — the length field fits in
        // the current block, it does not and needs a whole extra one, or the
        // input ended exactly on a boundary — and they are three lines apart.
        // Rather than pick representatives, check all of them.
        let data = [0x5au8; 200];
        for len in 0..=200 {
            let one_shot = sha256(&data[..len]);
            for split in 0..=len {
                let mut hasher = Sha256::new();
                hasher.update(&data[..split]);
                hasher.update(&data[split..len]);
                assert_eq!(
                    hasher.finalize(),
                    one_shot,
                    "len {len} split at {split} disagreed with the one-shot digest"
                );
            }
        }
    }

    #[test]
    fn a_byte_at_a_time_equals_all_at_once() {
        let data: [u8; 130] = core::array::from_fn(|i| (i % 251) as u8);
        let mut hasher = Sha256::new();
        for byte in &data {
            hasher.update(&[*byte]);
        }
        assert_eq!(hasher.finalize(), sha256(&data));
    }

    #[test]
    fn an_empty_update_changes_nothing() {
        let mut hasher = Sha256::new();
        hasher.update(b"");
        hasher.update(b"abc");
        hasher.update(b"");
        assert_eq!(hasher.finalize(), sha256(b"abc"));
    }

    #[test]
    fn exactly_one_block_needs_a_second_one_for_the_length() {
        // 64 bytes fills a block exactly, so the 0x80 and the length field
        // have nowhere to go but a block of their own. A padding calculation
        // that computed `56 - buffered` without wrapping produces the wrong
        // answer here and nowhere else.
        let data = [b'x'; 64];
        // Cross-checked against `python -c "import hashlib;
        // print(hashlib.sha256(b'x'*64).hexdigest())"`.
        assert_eq!(
            sha256_hex(&data).as_str(),
            "7ce100971f64e7001e8fe5a51973ecdfe1ced42befe7ee8d5fd6219506b5393c"
        );
    }

    // -- The digest is a hash, not an encoding --

    #[test]
    fn a_single_flipped_bit_changes_the_whole_digest() {
        let a = sha256(b"the quick brown fox");
        let b = sha256(b"the quick brown foy");
        let differing = a.iter().zip(&b).filter(|(x, y)| x != y).count();
        // Avalanche: half the *bits* should change, so essentially every byte
        // should differ. Asserting 24 of 32 rather than 32 keeps this from
        // being a flaky test of a fixed input, while still failing loudly for
        // anything that merely permutes the input.
        assert!(
            differing >= 24,
            "only {differing} of 32 bytes changed — that is not a hash"
        );
    }

    // -- Hex --

    #[test]
    fn hex_pads_low_bytes() {
        let digest = [0x00u8; DIGEST_LEN];
        assert_eq!(hex(&digest).as_str(), "0".repeat(64));

        let mut digest = [0u8; DIGEST_LEN];
        digest[0] = 0x0f;
        digest[31] = 0xf0;
        let text = hex(&digest);
        assert!(text.as_str().starts_with("0f"));
        assert!(text.as_str().ends_with("f0"));
        assert_eq!(text.as_str().len(), 64);
    }

    #[test]
    fn hex_is_lowercase() {
        let text = sha256_hex(b"abc");
        assert!(
            text.as_str()
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
    }

    // -- Constant-time comparison --

    #[test]
    fn constant_time_eq_agrees_with_eq() {
        let a = sha256(b"one");
        let b = sha256(b"two");
        assert!(eq_constant_time(&a, &a));
        assert!(!eq_constant_time(&a, &b));
        assert!(!eq_constant_time(&a, &b[..16]));
        assert!(eq_constant_time(b"", b""));
    }

    #[test]
    fn constant_time_eq_catches_a_difference_in_the_last_byte() {
        // The one an early-exit comparison gets right and a broken
        // accumulate-and-forget gets wrong.
        let a = [1u8; 32];
        let mut b = a;
        b[31] = 2;
        assert!(!eq_constant_time(&a, &b));
    }

    // -- Debug is opaque --

    #[test]
    fn debug_does_not_print_the_buffer() {
        let mut hasher = Sha256::new();
        hasher.update(b"hunter2");
        let rendered = alloc_format(&hasher);
        assert!(
            !rendered.contains("hunter2") && !rendered.contains("104"),
            "Debug leaked buffered input: {rendered}"
        );
        assert!(rendered.contains('7'), "expected the byte count: {rendered}");
    }

    /// `no_std` crate, but the test harness links `std`, so formatting to a
    /// `String` in a test is fine.
    fn alloc_format(value: &Sha256) -> std::string::String {
        std::format!("{value:?}")
    }
}
