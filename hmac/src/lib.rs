//! HMAC (RFC 2104) and PBKDF2 (RFC 8018 §5.2) over the tree's own hashes.
//!
//! HMAC is the standard way to turn a hash function into a *keyed* one: it
//! answers "was this message written by someone who holds the key, and has it
//! been altered since?" — which a bare hash cannot, because anyone can
//! recompute a bare hash over altered data. PBKDF2 is the standard way to turn
//! a *human password* into a key, by running HMAC many thousands of times so
//! that guessing passwords costs the guesser the same multiple.
//!
//! ## Why this is its own crate
//!
//! `userspace/wpa` carried a private SHA-1 and a private HMAC-SHA1 built on
//! `Vec`, written at a point when the dependency-free [`sha1`] crate already
//! existed. Three separate places now need a keyed hash — the supplicant, the
//! 802.11 key derivation in `net80211`, and TLS — and the alternative to this
//! crate is each of them growing its own.
//!
//! That is the failure `crc32` was factored out to stop, one layer up and with
//! sharper teeth. A wrong CRC-32 mismatches immediately and visibly. A wrong
//! HMAC produces a tag of exactly the right length that simply never matches
//! the other end's, and the symptom is "the WiFi password doesn't work" — a
//! report that sends you looking at the radio, the driver, and the access
//! point long before the arithmetic.
//!
//! ## Design notes
//!
//! - **No allocation, no `std`.** HMAC needs two scratch blocks, both fixed at
//!   [`MAX_BLOCK_LEN`]; PBKDF2 writes into a buffer the caller owns.
//! - **Streaming, so the message need not be contiguous.** An 802.11 MIC is
//!   computed over a frame header and a body that are not adjacent in memory,
//!   and copying them together just to hash them would mean allocating.
//! - **Constant-time comparison is provided and should be used.** Comparing
//!   two tags with `==` leaks, through how long the comparison runs, how many
//!   leading bytes an attacker guessed right — which is enough to recover a
//!   tag one byte at a time. Use [`verify`], not `==`.
//!
//! ## Usage
//!
//! ```
//! # use hmac::hmac_sha256;
//! // RFC 4231 test case 1.
//! let tag = hmac_sha256(&[0x0b; 20], b"Hi There");
//! assert_eq!(tag[0], 0xb0);
//! ```
//!
//! For a message that does not arrive all at once:
//!
//! ```
//! # use hmac::{Hmac, Sha256Hash};
//! let mut mac = Hmac::<Sha256Hash>::new(b"key");
//! mac.update(b"The quick brown fox ");
//! mac.update(b"jumps over the lazy dog");
//! let tag = mac.finalize();
//! assert_eq!(tag, hmac::hmac_sha256(b"key", b"The quick brown fox jumps over the lazy dog"));
//! ```
//!
//! ## References
//!
//! - RFC 2104 (HMAC), RFC 4231 (HMAC-SHA-2 test vectors), RFC 2202
//!   (HMAC-SHA-1 test vectors).
//! - RFC 8018 §5.2 (PBKDF2), RFC 6070 (PBKDF2-HMAC-SHA-1 test vectors).

#![no_std]
#![forbid(unsafe_code)]

/// The largest block size any supported hash compresses, in bytes.
///
/// SHA-1 and SHA-256 both use 64. The constant is stated separately rather
/// than as `Sha1Hash::BLOCK_LEN` because it sizes the scratch arrays inside
/// [`Hmac`], which must be one size for every hash the type is instantiated
/// with. SHA-384/512 would raise it to 128; nothing else here would change.
pub const MAX_BLOCK_LEN: usize = 64;

/// The largest digest any supported hash produces, in bytes.
pub const MAX_DIGEST_LEN: usize = 32;

/// The inner padding byte (RFC 2104 §2).
const IPAD: u8 = 0x36;

/// The outer padding byte (RFC 2104 §2).
const OPAD: u8 = 0x5C;

/// A hash function HMAC can be built on.
///
/// This is deliberately a trait local to this crate rather than one the hash
/// crates implement themselves: `sha1` and `sha2` have no reason to know that
/// HMAC exists, and keeping the dependency pointing this way means a hash can
/// be added to the tree without touching this crate's callers.
///
/// The block length is part of the *algorithm*, not of any particular
/// implementation of it — SHA-1 compresses 64-byte blocks wherever it is
/// written — so stating it here is not duplicating a fact that lives
/// elsewhere.
pub trait Hash {
    /// The block size the compression function consumes, in bytes. HMAC's key
    /// padding is defined in terms of this and gets a wrong answer, silently,
    /// if it is wrong.
    const BLOCK_LEN: usize;

    /// The digest size, in bytes.
    const DIGEST_LEN: usize;

    /// The digest type — a fixed-size array, so nothing allocates.
    type Digest: AsRef<[u8]> + Copy + PartialEq + core::fmt::Debug;

    /// Start a new hash.
    fn new() -> Self;

    /// Absorb more of the message.
    fn update(&mut self, data: &[u8]);

    /// Finish and produce the digest.
    fn finish(self) -> Self::Digest;
}

/// SHA-1, for the protocols that specify it: WPA2's PRF and PBKDF2, and the
/// original HMAC-SHA1 test vectors.
///
/// See [`sha1`]'s own documentation on why SHA-1 is still present in this
/// tree. HMAC-SHA1 is, notably, *not* broken by the SHA-1 collision attacks —
/// HMAC's security rests on the compression function being a decent PRF, not
/// on collision resistance — which is why WPA2 still specifies it and why
/// this crate offers it without a warning attached.
pub struct Sha1Hash(sha1::Sha1);

impl Hash for Sha1Hash {
    const BLOCK_LEN: usize = 64;
    const DIGEST_LEN: usize = sha1::DIGEST_LEN;
    type Digest = [u8; sha1::DIGEST_LEN];

    fn new() -> Self {
        Sha1Hash(sha1::Sha1::new())
    }

    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    fn finish(self) -> Self::Digest {
        self.0.finalize()
    }
}

/// SHA-256 — the default choice for anything new.
pub struct Sha256Hash(sha2::Sha256);

impl Hash for Sha256Hash {
    const BLOCK_LEN: usize = 64;
    const DIGEST_LEN: usize = sha2::DIGEST_LEN;
    type Digest = [u8; sha2::DIGEST_LEN];

    fn new() -> Self {
        Sha256Hash(sha2::Sha256::new())
    }

    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    fn finish(self) -> Self::Digest {
        self.0.finalize()
    }
}

/// A keyed hash in progress.
///
/// Created with [`Hmac::new`], fed with [`Hmac::update`] as many times as the
/// message has pieces, and finished with [`Hmac::finalize`].
pub struct Hmac<H: Hash> {
    /// The inner hash, already primed with `key ^ ipad`.
    inner: H,
    /// `key ^ opad`, kept until the end. Only the first `H::BLOCK_LEN` bytes
    /// are meaningful.
    opad: [u8; MAX_BLOCK_LEN],
}

impl<H: Hash> Hmac<H> {
    /// Start a keyed hash under `key`.
    ///
    /// Any key length is accepted, as RFC 2104 requires: a key longer than the
    /// block is replaced by its own hash, and a shorter one is zero-padded.
    ///
    /// Note the consequence of the long-key rule, which is a property of HMAC
    /// and not of this implementation: a key and the hash of that key are
    /// interchangeable if the first is longer than a block. Callers deriving
    /// keys should keep them at or below [`Hash::BLOCK_LEN`], which every
    /// caller in this tree does.
    #[must_use]
    pub fn new(key: &[u8]) -> Self {
        // `H::BLOCK_LEN` is a per-algorithm constant that is 64 for both hashes
        // here; the clamp keeps the indexing below provably in range even if a
        // future implementor of `Hash` gets it wrong.
        let block_len = if H::BLOCK_LEN <= MAX_BLOCK_LEN {
            H::BLOCK_LEN
        } else {
            MAX_BLOCK_LEN
        };

        let mut padded = [0u8; MAX_BLOCK_LEN];
        if key.len() > block_len {
            // RFC 2104 §2: a key longer than a block is hashed first. The
            // digest is at most MAX_DIGEST_LEN and so always fits.
            let mut h = H::new();
            h.update(key);
            let digest = h.finish();
            let d = digest.as_ref();
            let n = core::cmp::min(d.len(), block_len);
            if let (Some(dst), Some(src)) = (padded.get_mut(..n), d.get(..n)) {
                dst.copy_from_slice(src);
            }
        } else if let (Some(dst), Some(src)) = (padded.get_mut(..key.len()), key.get(..)) {
            dst.copy_from_slice(src);
        }

        let mut ipad = [IPAD; MAX_BLOCK_LEN];
        let mut opad = [OPAD; MAX_BLOCK_LEN];
        for i in 0..block_len {
            if let (Some(ip), Some(op), Some(k)) = (ipad.get_mut(i), opad.get_mut(i), padded.get(i))
            {
                *ip ^= *k;
                *op ^= *k;
            }
        }

        let mut inner = H::new();
        if let Some(block) = ipad.get(..block_len) {
            inner.update(block);
        }

        Hmac { inner, opad }
    }

    /// Absorb more of the message.
    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    /// Finish and produce the tag.
    #[must_use]
    pub fn finalize(self) -> H::Digest {
        let block_len = if H::BLOCK_LEN <= MAX_BLOCK_LEN {
            H::BLOCK_LEN
        } else {
            MAX_BLOCK_LEN
        };
        let inner_digest = self.inner.finish();

        let mut outer = H::new();
        if let Some(block) = self.opad.get(..block_len) {
            outer.update(block);
        }
        outer.update(inner_digest.as_ref());
        outer.finish()
    }
}

/// HMAC-SHA1 over a single contiguous message (RFC 2104).
#[must_use]
pub fn hmac_sha1(key: &[u8], data: &[u8]) -> [u8; sha1::DIGEST_LEN] {
    let mut mac = Hmac::<Sha1Hash>::new(key);
    mac.update(data);
    mac.finalize()
}

/// HMAC-SHA256 over a single contiguous message (RFC 2104).
#[must_use]
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; sha2::DIGEST_LEN] {
    let mut mac = Hmac::<Sha256Hash>::new(key);
    mac.update(data);
    mac.finalize()
}

/// Compare two tags without leaking, through timing, how far they matched.
///
/// A byte-by-byte `==` returns as soon as it finds a difference, so how long
/// it took tells an attacker how many leading bytes were right. Repeated a few
/// hundred times that recovers a whole tag without ever knowing the key. This
/// runs over both slices in full regardless.
///
/// Tags of different lengths are unequal, and that much *is* revealed — the
/// length is not a secret, and the alternative is reading past the end of one
/// of them.
#[must_use]
pub fn verify(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// PBKDF2 with HMAC-SHA1 (RFC 8018 §5.2), filling `out`.
///
/// This is what turns a WiFi passphrase into the 256-bit PMK: WPA2 specifies
/// exactly `PBKDF2(passphrase, ssid, 4096, 256 bits)` (IEEE 802.11-2020
/// §J.4.1). The SSID is the salt, which is why two networks with the same name
/// and password have the same PMK — a property of the standard, not of this
/// code.
///
/// `iterations` of zero is treated as one: RFC 8018 requires at least one, and
/// returning the unstretched HMAC would be a silently much weaker key than the
/// caller asked for. Deriving *nothing* is not an option either, since `out`
/// would then be left as whatever the caller had in it.
pub fn pbkdf2_hmac_sha1(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    pbkdf2::<Sha1Hash>(password, salt, iterations, out);
}

/// PBKDF2 with HMAC-SHA256 (RFC 8018 §5.2), filling `out`.
pub fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    pbkdf2::<Sha256Hash>(password, salt, iterations, out);
}

/// PBKDF2 over any [`Hash`], filling `out`.
///
/// The block index is a big-endian `u32` appended to the salt (RFC 8018 calls
/// it `INT(i)`), and blocks are numbered from one, not zero — an off-by-one
/// here produces a key that is wrong but perfectly plausible-looking.
pub fn pbkdf2<H: Hash>(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    let iterations = iterations.max(1);
    let digest_len = core::cmp::min(H::DIGEST_LEN, MAX_DIGEST_LEN);
    if digest_len == 0 {
        return;
    }

    let mut block_index: u32 = 1;
    let mut written = 0usize;

    while written < out.len() {
        // U1 = PRF(password, salt || INT(block_index))
        let mut mac = Hmac::<H>::new(password);
        mac.update(salt);
        mac.update(&block_index.to_be_bytes());
        let first = mac.finalize();

        let mut u = [0u8; MAX_DIGEST_LEN];
        let mut acc = [0u8; MAX_DIGEST_LEN];
        if let (Some(ud), Some(ad), Some(src)) = (
            u.get_mut(..digest_len),
            acc.get_mut(..digest_len),
            first.as_ref().get(..digest_len),
        ) {
            ud.copy_from_slice(src);
            ad.copy_from_slice(src);
        }

        // U2..Uc, each the PRF of the previous, all XORed together.
        for _ in 1..iterations {
            let mut mac = Hmac::<H>::new(password);
            if let Some(prev) = u.get(..digest_len) {
                mac.update(prev);
            }
            let next = mac.finalize();
            if let (Some(ud), Some(src)) =
                (u.get_mut(..digest_len), next.as_ref().get(..digest_len))
            {
                ud.copy_from_slice(src);
            }
            for i in 0..digest_len {
                if let (Some(a), Some(x)) = (acc.get_mut(i), u.get(i)) {
                    *a ^= *x;
                }
            }
        }

        let remaining = out.len().saturating_sub(written);
        let take = core::cmp::min(remaining, digest_len);
        if let (Some(dst), Some(src)) = (
            out.get_mut(written..written.saturating_add(take)),
            acc.get(..take),
        ) {
            dst.copy_from_slice(src);
        }
        written = written.saturating_add(take);
        block_index = block_index.saturating_add(1);
    }
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
    use super::*;

    /// Decode a compile-time hex literal into a fixed array, so the RFC
    /// vectors can be pasted in the form the RFC prints them.
    fn hex<const N: usize>(s: &str) -> [u8; N] {
        let bytes = s.as_bytes();
        assert_eq!(bytes.len(), N * 2, "hex literal length");
        let mut out = [0u8; N];
        for i in 0..N {
            let hi = (bytes[i * 2] as char).to_digit(16).expect("hex digit");
            let lo = (bytes[i * 2 + 1] as char).to_digit(16).expect("hex digit");
            out[i] = ((hi << 4) | lo) as u8;
        }
        out
    }

    #[test]
    fn rfc_2202_hmac_sha1_vectors() {
        // Case 1.
        assert_eq!(
            hmac_sha1(&[0x0b; 20], b"Hi There"),
            hex::<20>("b617318655057264e28bc0b6fb378c8ef146be00")
        );
        // Case 2 — a key that is plain ASCII.
        assert_eq!(
            hmac_sha1(b"Jefe", b"what do ya want for nothing?"),
            hex::<20>("effcdf6ae5eb2fa2d27416d5f184df9c259a7c79")
        );
        // Case 3 — 50 bytes of 0xdd under a 20-byte key of 0xaa.
        assert_eq!(
            hmac_sha1(&[0xaa; 20], &[0xdd; 50]),
            hex::<20>("125d7342b9ac11cd91a39af48aa17b4f63f175d3")
        );
    }

    #[test]
    fn rfc_4231_hmac_sha256_vectors() {
        // Case 1.
        assert_eq!(
            hmac_sha256(&[0x0b; 20], b"Hi There"),
            hex::<32>("b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7")
        );
        // Case 2.
        assert_eq!(
            hmac_sha256(b"Jefe", b"what do ya want for nothing?"),
            hex::<32>("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843")
        );
        // Case 3.
        assert_eq!(
            hmac_sha256(&[0xaa; 20], &[0xdd; 50]),
            hex::<32>("773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe")
        );
    }

    #[test]
    fn a_key_longer_than_a_block_is_hashed_first() {
        // RFC 4231 case 6: a 131-byte key, which exceeds SHA-256's 64-byte
        // block and so must be replaced by its own digest. Getting this wrong
        // is invisible until you talk to a correct implementation.
        assert_eq!(
            hmac_sha256(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            ),
            hex::<32>("60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54")
        );
        // RFC 2202 case 5 for SHA-1: an 80-byte key, likewise over the block.
        assert_eq!(
            hmac_sha1(
                &[0xaa; 80],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            ),
            hex::<20>("aa4ae5e15272d00e95705637ce8a3b55ed402112")
        );
    }

    #[test]
    fn a_key_of_exactly_one_block_is_not_hashed() {
        // The boundary the long-key rule turns on: `>` a block, not `>=`. A
        // 64-byte key is used as-is, and hashing it here would be wrong.
        let key = [0x0bu8; 64];
        let direct = hmac_sha256(&key, b"boundary");

        // Recompute by hand from the definition to prove which branch ran.
        let mut ipad = [0x36u8; 64];
        let mut opad = [0x5cu8; 64];
        for i in 0..64 {
            ipad[i] ^= key[i];
            opad[i] ^= key[i];
        }
        let mut inner = sha2::Sha256::new();
        inner.update(&ipad);
        inner.update(b"boundary");
        let inner = inner.finalize();
        let mut outer = sha2::Sha256::new();
        outer.update(&opad);
        outer.update(&inner);
        assert_eq!(direct, outer.finalize());
    }

    #[test]
    fn an_empty_key_and_an_empty_message_are_both_legal() {
        // Neither is a useful thing to do, but both must produce a tag rather
        // than panic or return zeros: a caller that accidentally passes an
        // empty key must not get a tag that happens to verify.
        let a = hmac_sha256(b"", b"");
        let b = hmac_sha256(b"", b"x");
        let c = hmac_sha256(b"k", b"");
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, [0u8; 32]);
    }

    #[test]
    fn streaming_matches_one_shot_at_every_split() {
        let msg = b"The quick brown fox jumps over the lazy dog";
        let want = hmac_sha256(b"key", msg);
        for split in 0..=msg.len() {
            let mut mac = Hmac::<Sha256Hash>::new(b"key");
            mac.update(&msg[..split]);
            mac.update(&msg[split..]);
            assert_eq!(mac.finalize(), want, "split at {split}");
        }
    }

    #[test]
    fn rfc_6070_pbkdf2_hmac_sha1_vectors() {
        let mut out = [0u8; 20];
        pbkdf2_hmac_sha1(b"password", b"salt", 1, &mut out);
        assert_eq!(out, hex::<20>("0c60c80f961f0e71f3a9b524af6012062fe037a6"));

        pbkdf2_hmac_sha1(b"password", b"salt", 2, &mut out);
        assert_eq!(out, hex::<20>("ea6c014dc72d6f8ccd1ed92ace1d41f0d8de8957"));

        pbkdf2_hmac_sha1(b"password", b"salt", 4096, &mut out);
        assert_eq!(out, hex::<20>("4b007901b765489abead49d926f721d065a429c1"));

        // The multi-block case: 25 octets is two SHA-1 blocks and a bit, which
        // is where an off-by-one in the block counter shows up.
        let mut long = [0u8; 25];
        pbkdf2_hmac_sha1(
            b"passwordPASSWORDpassword",
            b"saltSALTsaltSALTsaltSALTsaltSALTsalt",
            4096,
            &mut long,
        );
        assert_eq!(
            long,
            hex::<25>("3d2eec4fe41c849b80c8d83662c0e44a8b291a964cf2f07038")
        );
    }

    #[test]
    fn pbkdf2_of_a_wpa2_passphrase_is_a_thirty_two_byte_pmk() {
        // IEEE 802.11-2020 §J.4.2, test vector 1: SSID "IEEE", passphrase
        // "password", 4096 iterations. This is the exact computation the
        // supplicant performs, and the value every access point agrees on.
        let mut pmk = [0u8; 32];
        pbkdf2_hmac_sha1(b"password", b"IEEE", 4096, &mut pmk);
        assert_eq!(
            pmk,
            hex::<32>("f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e")
        );
    }

    #[test]
    fn zero_iterations_is_treated_as_one_rather_than_leaving_the_buffer_alone() {
        let mut zero = [0u8; 20];
        let mut one = [0u8; 20];
        pbkdf2_hmac_sha1(b"password", b"salt", 0, &mut zero);
        pbkdf2_hmac_sha1(b"password", b"salt", 1, &mut one);
        assert_eq!(zero, one);
        assert_ne!(zero, [0u8; 20], "the buffer must not be left untouched");
    }

    #[test]
    fn pbkdf2_output_is_a_prefix_of_any_longer_output() {
        // Blocks are independent of how many were asked for, so a short
        // derivation must agree with the start of a long one. If it does not,
        // the block counter is being seeded from the output length.
        let mut short = [0u8; 16];
        let mut long = [0u8; 64];
        pbkdf2_hmac_sha256(b"pw", b"salt", 100, &mut short);
        pbkdf2_hmac_sha256(b"pw", b"salt", 100, &mut long);
        assert_eq!(short[..], long[..16]);
    }

    #[test]
    fn pbkdf2_into_an_empty_buffer_does_nothing_and_does_not_hang() {
        let mut empty: [u8; 0] = [];
        pbkdf2_hmac_sha1(b"password", b"salt", 4096, &mut empty);
    }

    #[test]
    fn the_constant_time_comparison_still_compares() {
        assert!(verify(b"abcd", b"abcd"));
        assert!(!verify(b"abcd", b"abce"));
        // Differing only in the first byte, where an early-return `==` would
        // bail out immediately.
        assert!(!verify(b"Xbcd", b"abcd"));
        // Length mismatch, including one being a prefix of the other.
        assert!(!verify(b"abc", b"abcd"));
        assert!(verify(b"", b""));
    }
}
