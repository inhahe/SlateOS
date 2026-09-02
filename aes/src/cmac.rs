//! AES-CMAC (RFC 4493) — a message authentication code built from the block
//! cipher rather than from a hash.
//!
//! CMAC answers the same question HMAC does — "was this written by someone
//! holding the key, and is it unaltered?" — but is built out of AES instead of
//! SHA. It exists here because IEEE 802.11 *requires* it: the newer WPA2 and
//! all WPA3 authentication methods (the AKM suites numbered 5, 6, 8 and up)
//! specify AES-128-CMAC for the handshake MIC, where the older ones specify
//! HMAC-SHA1. A supplicant that implements only one of the two can talk to
//! only half the access points it meets.
//!
//! ## Why CMAC is not "just encrypt the last block"
//!
//! The naive construction — CBC-MAC, i.e. run the message through CBC and keep
//! the final block — is secure only for messages of one fixed length agreed in
//! advance. For variable-length messages it is trivially forgeable: given the
//! tag `T` for a one-block message `M`, the two-block message `M || (M ^ T)`
//! has the same tag, with no knowledge of the key at all.
//!
//! CMAC fixes this by XORing a key-derived value into the *final* block before
//! the last encryption, choosing between two such values (`K1` and `K2`)
//! depending on whether the message was a whole number of blocks. That is what
//! [`subkeys`] computes and it is the whole difference between CMAC and a
//! forgeable MAC. The subkeys come from encrypting an all-zero block and then
//! doubling in GF(2^128), which is the `<< 1` plus conditional `^ 0x87` below.
//!
//! ## Timing
//!
//! The underlying [`Aes`] is not constant-time — see the crate docs — so
//! neither is this. Compare tags with [`verify`], never with `==`.
//!
//! ## References
//!
//! - RFC 4493 (The AES-CMAC Algorithm), whose four test vectors are asserted
//!   below.
//! - NIST SP 800-38B, the original specification.
//! - IEEE Std 802.11-2020 §12.7.3, which selects CMAC by AKM suite.

use crate::{Aes, BLOCK_LEN};

/// The tag length, in bytes. CMAC's tag is one cipher block.
///
/// Callers frequently truncate it — 802.11 uses the first 16 bytes, which is
/// the whole thing for AES-128, and the Suite-B modes truncate a longer MIC —
/// but the algorithm itself produces exactly one block.
pub const TAG_LEN: usize = BLOCK_LEN;

/// The constant `R_b` for a 128-bit block (RFC 4493 §2.3), the low byte of the
/// GF(2^128) reduction polynomial `x^128 + x^7 + x^2 + x + 1`.
const RB: u8 = 0x87;

/// Double a block in GF(2^128): a left shift by one bit, with the reduction
/// polynomial XORed back in when a one was shifted out of the top.
///
/// This is the operation that generates `K1` from `L` and `K2` from `K1`.
fn double(block: &mut [u8; BLOCK_LEN]) {
    let overflow = (block.first().copied().unwrap_or(0) & 0x80) != 0;

    let mut carry = 0u8;
    for i in (0..BLOCK_LEN).rev() {
        if let Some(b) = block.get_mut(i) {
            let v = *b;
            *b = v.wrapping_shl(1) | carry;
            carry = v.wrapping_shr(7);
        }
    }

    if overflow {
        if let Some(last) = block.last_mut() {
            *last ^= RB;
        }
    }
}

/// Generate the two subkeys `K1` and `K2` from a key (RFC 4493 §2.3).
///
/// `K1` is used when the message is a non-empty whole number of blocks and
/// `K2` when it needs padding. Exposed because it is the part of CMAC most
/// worth testing directly against the RFC — an error here produces a MAC that
/// is wrong on exactly one of the two message shapes, which is the kind of bug
/// that passes a test suite that only ever feeds it whole blocks.
#[must_use]
pub fn subkeys(key: &Aes) -> ([u8; BLOCK_LEN], [u8; BLOCK_LEN]) {
    // L = AES-K(0^128)
    let mut l = [0u8; BLOCK_LEN];
    key.encrypt_block(&mut l);

    let mut k1 = l;
    double(&mut k1);

    let mut k2 = k1;
    double(&mut k2);

    (k1, k2)
}

/// AES-CMAC over a single contiguous message.
///
/// `message` may be empty; RFC 4493 defines a tag for the empty string and
/// asserts it as example 1, and the 802.11 case never has one but a MAC that
/// panicked on an empty input would be a denial of service the first time a
/// peer sent a zero-length frame.
#[must_use]
pub fn cmac(key: &Aes, message: &[u8]) -> [u8; TAG_LEN] {
    let mut mac = Cmac::new(key);
    mac.update(message);
    mac.finalize()
}

/// AES-CMAC in progress, for a message that arrives in pieces.
///
/// The 802.11 MIC is computed over an EAPOL frame's header and body, which are
/// not necessarily adjacent in memory, and this crate cannot allocate a buffer
/// to join them.
///
/// The implementation holds back a full block rather than processing eagerly,
/// because the *last* block is treated differently from every other and there
/// is no way to know a block is last until either more data arrives or
/// [`Cmac::finalize`] is called.
pub struct Cmac<'a> {
    key: &'a Aes,
    /// The running CBC chaining value, `X` in the RFC.
    x: [u8; BLOCK_LEN],
    /// Buffered input not yet known to be non-final.
    buf: [u8; BLOCK_LEN],
    /// How much of `buf` is filled.
    buf_len: usize,
}

impl<'a> Cmac<'a> {
    /// Start a CMAC under `key`.
    #[must_use]
    pub fn new(key: &'a Aes) -> Self {
        Cmac {
            key,
            x: [0u8; BLOCK_LEN],
            buf: [0u8; BLOCK_LEN],
            buf_len: 0,
        }
    }

    /// Encrypt one full block into the chaining value.
    fn absorb(&mut self, block: &[u8]) {
        for i in 0..BLOCK_LEN {
            if let (Some(x), Some(b)) = (self.x.get_mut(i), block.get(i)) {
                *x ^= *b;
            }
        }
        self.key.encrypt_block(&mut self.x);
    }

    /// Absorb more of the message.
    pub fn update(&mut self, mut data: &[u8]) {
        // Top up the buffer first, but only flush it once we know it is not the
        // final block — that is, once there is at least one more byte after it.
        if self.buf_len > 0 {
            let space = BLOCK_LEN.saturating_sub(self.buf_len);
            let take = core::cmp::min(space, data.len());
            if let (Some(dst), Some(src)) = (
                self.buf
                    .get_mut(self.buf_len..self.buf_len.saturating_add(take)),
                data.get(..take),
            ) {
                dst.copy_from_slice(src);
            }
            self.buf_len = self.buf_len.saturating_add(take);
            data = data.get(take..).unwrap_or(&[]);

            if self.buf_len == BLOCK_LEN && !data.is_empty() {
                let block = self.buf;
                self.absorb(&block);
                self.buf_len = 0;
            }
        }

        // Consume whole blocks, always leaving at least one byte behind so the
        // final block is still in the buffer when `finalize` runs.
        while data.len() > BLOCK_LEN {
            if let Some(block) = data.get(..BLOCK_LEN) {
                let mut b = [0u8; BLOCK_LEN];
                b.copy_from_slice(block);
                self.absorb(&b);
            }
            data = data.get(BLOCK_LEN..).unwrap_or(&[]);
        }

        if !data.is_empty() {
            if let (Some(dst), Some(src)) = (
                self.buf
                    .get_mut(self.buf_len..self.buf_len.saturating_add(data.len())),
                data.get(..),
            ) {
                dst.copy_from_slice(src);
            }
            self.buf_len = self.buf_len.saturating_add(data.len());
        }
    }

    /// Finish and produce the tag.
    #[must_use]
    pub fn finalize(mut self) -> [u8; TAG_LEN] {
        let (k1, k2) = subkeys(self.key);

        let mut last = [0u8; BLOCK_LEN];
        if self.buf_len == BLOCK_LEN {
            // A whole final block: XOR K1, no padding.
            for i in 0..BLOCK_LEN {
                if let (Some(dst), Some(b), Some(k)) = (last.get_mut(i), self.buf.get(i), k1.get(i))
                {
                    *dst = *b ^ *k;
                }
            }
        } else {
            // A partial (possibly empty) final block: append 0x80, zero-fill,
            // XOR K2. The 0x80 is what makes "abc" and "abc\0" distinct.
            let mut padded = [0u8; BLOCK_LEN];
            if let (Some(dst), Some(src)) =
                (padded.get_mut(..self.buf_len), self.buf.get(..self.buf_len))
            {
                dst.copy_from_slice(src);
            }
            if let Some(b) = padded.get_mut(self.buf_len) {
                *b = 0x80;
            }
            for i in 0..BLOCK_LEN {
                if let (Some(dst), Some(b), Some(k)) = (last.get_mut(i), padded.get(i), k2.get(i)) {
                    *dst = *b ^ *k;
                }
            }
        }

        self.absorb(&last);
        self.x
    }
}

/// Compare two tags without leaking, through timing, how far they matched.
///
/// See [`hmac::verify`](../../hmac/fn.verify.html) for the reasoning; it is the
/// same, and duplicated here only because `aes` does not depend on `hmac`.
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

    /// The key shared by all four RFC 4493 examples.
    const KEY: [u8; 16] = [
        0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf, 0x4f,
        0x3c,
    ];

    /// The 64-byte message the examples take prefixes of.
    const MSG: [u8; 64] = [
        0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93, 0x17,
        0x2a, 0xae, 0x2d, 0x8a, 0x57, 0x1e, 0x03, 0xac, 0x9c, 0x9e, 0xb7, 0x6f, 0xac, 0x45, 0xaf,
        0x8e, 0x51, 0x30, 0xc8, 0x1c, 0x46, 0xa3, 0x5c, 0xe4, 0x11, 0xe5, 0xfb, 0xc1, 0x19, 0x1a,
        0x0a, 0x52, 0xef, 0xf6, 0x9f, 0x24, 0x45, 0xdf, 0x4f, 0x9b, 0x17, 0xad, 0x2b, 0x41, 0x7b,
        0xe6, 0x6c, 0x37, 0x10,
    ];

    fn key() -> Aes {
        Aes::new(&KEY).expect("128-bit key")
    }

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
    fn rfc_4493_subkeys() {
        // §4, the worked subkey generation. Checked separately from the tags
        // because a wrong K1 or K2 breaks only one of the two message shapes.
        let (k1, k2) = subkeys(&key());
        assert_eq!(k1, hex::<16>("fbeed618357133667c85e08f7236a8de"));
        assert_eq!(k2, hex::<16>("f7ddac306ae266ccf90bc11ee46d513b"));
    }

    #[test]
    fn rfc_4493_example_1_empty_message() {
        assert_eq!(
            cmac(&key(), &[]),
            hex::<16>("bb1d6929e95937287fa37d129b756746")
        );
    }

    #[test]
    fn rfc_4493_example_2_one_whole_block() {
        assert_eq!(
            cmac(&key(), &MSG[..16]),
            hex::<16>("070a16b46b4d4144f79bdd9dd04a287c")
        );
    }

    #[test]
    fn rfc_4493_example_3_a_partial_final_block() {
        // 40 bytes: two whole blocks and a half. This is the K2 path.
        assert_eq!(
            cmac(&key(), &MSG[..40]),
            hex::<16>("dfa66747de9ae63030ca32611497c827")
        );
    }

    #[test]
    fn rfc_4493_example_4_four_whole_blocks() {
        assert_eq!(
            cmac(&key(), &MSG),
            hex::<16>("51f0bebf7e3b9d92fc49741779363cfe")
        );
    }

    #[test]
    fn streaming_matches_one_shot_at_every_split() {
        // The buffering has to hold back the final block without knowing in
        // advance that it is final, so every split is a distinct code path
        // through `update`. 40 bytes exercises the partial-block finish.
        for len in [0usize, 1, 15, 16, 17, 31, 32, 40, 63, 64] {
            let want = cmac(&key(), &MSG[..len]);
            for split in 0..=len {
                let k = key();
                let mut mac = Cmac::new(&k);
                mac.update(&MSG[..split]);
                mac.update(&MSG[split..len]);
                assert_eq!(mac.finalize(), want, "len {len} split at {split}");
            }
        }
    }

    #[test]
    fn streaming_one_byte_at_a_time_matches() {
        // The pathological case for a buffer that flushes eagerly: every call
        // adds a byte, so the "is this the last block?" decision is deferred
        // sixty-four times.
        let k = key();
        let mut mac = Cmac::new(&k);
        for b in &MSG {
            mac.update(core::slice::from_ref(b));
        }
        assert_eq!(mac.finalize(), cmac(&key(), &MSG));
    }

    #[test]
    fn padding_makes_a_short_message_distinct_from_its_zero_extension() {
        // The 0x80 padding byte exists precisely so that these differ. Without
        // it, "ab" and "ab\0" would authenticate identically and a message
        // could be extended with zeros undetected.
        let a = cmac(&key(), b"ab");
        let b = cmac(&key(), b"ab\0");
        assert_ne!(a, b);
    }

    #[test]
    fn a_whole_block_and_a_padded_block_use_different_subkeys() {
        // A 16-byte message takes the K1 path; a 15-byte one takes K2. If the
        // subkey choice were inverted, both RFC examples 2 and 3 would fail —
        // but so would this, more legibly, and without needing the vectors.
        let (k1, k2) = subkeys(&key());
        assert_ne!(k1, k2);

        // Recompute example 2 by hand through the K1 path.
        let mut block = [0u8; 16];
        for i in 0..16 {
            block[i] = MSG[i] ^ k1[i];
        }
        key().encrypt_block(&mut block);
        assert_eq!(block, cmac(&key(), &MSG[..16]));
    }

    #[test]
    fn cbc_mac_length_extension_does_not_forge_a_cmac() {
        // The attack CMAC exists to stop, run as a test. For plain CBC-MAC,
        // the two-block message `M || (M ^ T)` has the same tag as `M`. Here
        // it must not.
        let m = &MSG[..16];
        let t = cmac(&key(), m);

        let mut forged = [0u8; 32];
        forged[..16].copy_from_slice(m);
        for i in 0..16 {
            forged[16 + i] = m[i] ^ t[i];
        }
        assert_ne!(cmac(&key(), &forged), t);
    }

    #[test]
    fn cmac_is_defined_for_every_key_size() {
        // 802.11 uses AES-128 here, but nothing in the construction is
        // 128-specific and a Suite-B caller will want 256.
        for key_len in [16usize, 24, 32] {
            let k = Aes::new(&[0x11u8; 32][..key_len]).expect("valid key length");
            let tag = cmac(&k, b"message");
            assert_ne!(tag, [0u8; 16], "a tag of zeros means nothing ran");
        }
    }

    #[test]
    fn the_constant_time_comparison_still_compares() {
        assert!(verify(b"abcd", b"abcd"));
        assert!(!verify(b"abcd", b"abce"));
        assert!(!verify(b"Xbcd", b"abcd"));
        assert!(!verify(b"abc", b"abcd"));
        assert!(verify(b"", b""));
    }
}
