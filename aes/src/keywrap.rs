//! RFC 3394 AES Key Wrap — encrypting one key with another.
//!
//! # What it is for
//!
//! Key wrap solves a narrow problem: you have a key, and you need to send it
//! over a channel protected by a *different* key, with integrity. It is not a
//! general-purpose cipher mode. It has no IV, no nonce and no padding, which
//! is precisely why it is safe to expose here while CBC and CTR are not —
//! there is no parameter for a caller to choose badly.
//!
//! The integrity comes from a constant: the wrapped form carries a fixed
//! 8-octet check value, and unwrapping with the wrong key produces garbage
//! whose check value will not match. That gives a 1-in-2^64 chance of a wrong
//! key being accepted, which is the standard's stated security claim.
//!
//! Two callers in this tree need it:
//!
//! - the **WiFi supplicant**: message 3 of the WPA2/WPA3 4-way handshake
//!   carries the group temporal key wrapped under the KEK, which is the second
//!   16 octets of the PTK the station just derived. A station that cannot
//!   unwrap it cannot receive broadcast or multicast traffic — so it joins the
//!   network and then appears to have no DHCP server.
//! - **disk encryption**: a volume master key wrapped under a key derived from
//!   the user's passphrase, so that changing the passphrase rewraps 32 octets
//!   instead of rewriting the disk.
//!
//! # Sizes
//!
//! Key wrap operates on 64-bit units. The plaintext must be a whole number of
//! 8-octet blocks, and at least two of them (16 octets); the wrapped form is
//! always exactly 8 octets longer. RFC 3394 genuinely does not define a
//! one-block case — RFC 5649 exists to cover shorter and non-multiple inputs
//! and is not implemented here, because neither caller needs it.
//!
//! # References
//!
//! - RFC 3394, §2.2.1 (wrap), §2.2.2 (unwrap), §4 (test vectors).
//! - IEEE Std 802.11-2020 §12.7.2 (Key Data field encryption).

use crate::{Aes, BLOCK_LEN};

/// The unit key wrap works in, in octets.
pub const SEMIBLOCK_LEN: usize = 8;

/// The default Initial Value, `A6A6A6A6A6A6A6A6` (RFC 3394 §2.2.3.1).
///
/// This is the integrity check: unwrapping recovers it only if the key, the
/// ciphertext and the length all match.
pub const DEFAULT_IV: [u8; SEMIBLOCK_LEN] = [0xA6; SEMIBLOCK_LEN];

/// The number of passes the algorithm makes over the data.
const ROUNDS: usize = 6;

/// Why a wrap or unwrap could not be performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapError {
    /// The plaintext was not a positive multiple of 8 octets, or was shorter
    /// than the two-semiblock minimum RFC 3394 defines.
    BadLength,
    /// The output buffer could not hold the result.
    OutputTooSmall,
    /// The integrity check failed on unwrap: the recovered initial value was
    /// not [`DEFAULT_IV`].
    ///
    /// In a WPA handshake this means the KEK is wrong, which in turn almost
    /// always means the pre-shared key is wrong — the same root cause as a
    /// message-4 timeout, arriving one message earlier.
    IntegrityCheckFailed,
}

/// The wrapped length for `plaintext_len` octets of key material.
#[must_use]
pub fn wrapped_len(plaintext_len: usize) -> Option<usize> {
    plaintext_len.checked_add(SEMIBLOCK_LEN)
}

/// The unwrapped length for `wrapped_len` octets of ciphertext.
#[must_use]
pub fn unwrapped_len(wrapped_len: usize) -> Option<usize> {
    wrapped_len.checked_sub(SEMIBLOCK_LEN)
}

/// Validate a plaintext length and return its semiblock count.
fn semiblocks(len: usize) -> Result<usize, WrapError> {
    if len < 2 * SEMIBLOCK_LEN || !len.is_multiple_of(SEMIBLOCK_LEN) {
        return Err(WrapError::BadLength);
    }
    Ok(len / SEMIBLOCK_LEN)
}

/// Wrap `plaintext` under `kek`, writing `plaintext.len() + 8` octets to `out`
/// and returning that length.
///
/// # Errors
///
/// - [`WrapError::BadLength`] if `plaintext` is not at least two whole
///   8-octet semiblocks.
/// - [`WrapError::OutputTooSmall`] if `out` cannot hold the result.
pub fn wrap(kek: &Aes, out: &mut [u8], plaintext: &[u8]) -> Result<usize, WrapError> {
    let n = semiblocks(plaintext.len())?;
    let total = wrapped_len(plaintext.len()).ok_or(WrapError::OutputTooSmall)?;
    let buf = out.get_mut(..total).ok_or(WrapError::OutputTooSmall)?;

    // Lay the output out as `A | R[1] | ... | R[n]` and work on it in place.
    buf.get_mut(..SEMIBLOCK_LEN)
        .ok_or(WrapError::OutputTooSmall)?
        .copy_from_slice(&DEFAULT_IV);
    buf.get_mut(SEMIBLOCK_LEN..)
        .ok_or(WrapError::OutputTooSmall)?
        .copy_from_slice(plaintext);

    let mut a = DEFAULT_IV;
    for j in 0..ROUNDS {
        for i in 1..=n {
            let mut block = [0u8; BLOCK_LEN];
            block
                .get_mut(..SEMIBLOCK_LEN)
                .ok_or(WrapError::OutputTooSmall)?
                .copy_from_slice(&a);
            let at = i.checked_mul(SEMIBLOCK_LEN).ok_or(WrapError::BadLength)?;
            let end = at.checked_add(SEMIBLOCK_LEN).ok_or(WrapError::BadLength)?;
            let r = buf.get(at..end).ok_or(WrapError::OutputTooSmall)?;
            block
                .get_mut(SEMIBLOCK_LEN..)
                .ok_or(WrapError::OutputTooSmall)?
                .copy_from_slice(r);

            kek.encrypt_block(&mut block);

            // t = n*j + i, XORed into A big-endian.
            let t = n
                .checked_mul(j)
                .and_then(|x| x.checked_add(i))
                .ok_or(WrapError::BadLength)?;
            a.copy_from_slice(
                block
                    .get(..SEMIBLOCK_LEN)
                    .ok_or(WrapError::OutputTooSmall)?,
            );
            xor_counter(&mut a, u64::try_from(t).map_err(|_| WrapError::BadLength)?);

            let low = block
                .get(SEMIBLOCK_LEN..)
                .ok_or(WrapError::OutputTooSmall)?;
            buf.get_mut(at..end)
                .ok_or(WrapError::OutputTooSmall)?
                .copy_from_slice(low);
        }
    }

    buf.get_mut(..SEMIBLOCK_LEN)
        .ok_or(WrapError::OutputTooSmall)?
        .copy_from_slice(&a);
    Ok(total)
}

/// Unwrap `wrapped` under `kek`, writing `wrapped.len() - 8` octets to `out`
/// and returning that length.
///
/// # Errors
///
/// - [`WrapError::BadLength`] if `wrapped` is not at least three whole
///   semiblocks (a check value plus two data semiblocks).
/// - [`WrapError::OutputTooSmall`] if `out` cannot hold the result.
/// - [`WrapError::IntegrityCheckFailed`] if the recovered check value is not
///   [`DEFAULT_IV`] — the key is wrong, or the ciphertext was tampered with.
///   **`out` has been written to in this case and its contents are
///   meaningless**; a caller must not use them, which is why the length is
///   returned only on success.
pub fn unwrap(kek: &Aes, out: &mut [u8], wrapped: &[u8]) -> Result<usize, WrapError> {
    let plain_len = unwrapped_len(wrapped.len()).ok_or(WrapError::BadLength)?;
    let n = semiblocks(plain_len)?;
    let buf = out.get_mut(..plain_len).ok_or(WrapError::OutputTooSmall)?;

    let mut a = [0u8; SEMIBLOCK_LEN];
    a.copy_from_slice(wrapped.get(..SEMIBLOCK_LEN).ok_or(WrapError::BadLength)?);
    buf.copy_from_slice(wrapped.get(SEMIBLOCK_LEN..).ok_or(WrapError::BadLength)?);

    for j in (0..ROUNDS).rev() {
        for i in (1..=n).rev() {
            let t = n
                .checked_mul(j)
                .and_then(|x| x.checked_add(i))
                .ok_or(WrapError::BadLength)?;
            let mut block = [0u8; BLOCK_LEN];
            let mut a_xor = a;
            xor_counter(
                &mut a_xor,
                u64::try_from(t).map_err(|_| WrapError::BadLength)?,
            );
            block
                .get_mut(..SEMIBLOCK_LEN)
                .ok_or(WrapError::OutputTooSmall)?
                .copy_from_slice(&a_xor);

            // `buf` holds R[1..=n] with no leading A, so R[i] starts at
            // `(i - 1) * 8` here — one semiblock earlier than in `wrap`.
            let at = i.checked_sub(1).and_then(|k| k.checked_mul(SEMIBLOCK_LEN));
            let at = at.ok_or(WrapError::BadLength)?;
            let end = at.checked_add(SEMIBLOCK_LEN).ok_or(WrapError::BadLength)?;
            let r = buf.get(at..end).ok_or(WrapError::OutputTooSmall)?;
            block
                .get_mut(SEMIBLOCK_LEN..)
                .ok_or(WrapError::OutputTooSmall)?
                .copy_from_slice(r);

            kek.decrypt_block(&mut block);

            a.copy_from_slice(
                block
                    .get(..SEMIBLOCK_LEN)
                    .ok_or(WrapError::OutputTooSmall)?,
            );
            let low = block
                .get(SEMIBLOCK_LEN..)
                .ok_or(WrapError::OutputTooSmall)?;
            buf.get_mut(at..end)
                .ok_or(WrapError::OutputTooSmall)?
                .copy_from_slice(low);
        }
    }

    if eq_constant_time(&a, &DEFAULT_IV) {
        Ok(plain_len)
    } else {
        Err(WrapError::IntegrityCheckFailed)
    }
}

/// XOR a big-endian counter into the low octets of `a`.
fn xor_counter(a: &mut [u8; SEMIBLOCK_LEN], t: u64) {
    let bytes = t.to_be_bytes();
    for (dst, src) in a.iter_mut().zip(bytes.iter()) {
        *dst ^= *src;
    }
}

/// Compare two byte strings without an early exit.
///
/// The integrity check compares a value an attacker controls against a
/// constant. A short-circuiting `==` leaks, through timing, how many leading
/// octets matched, which turns a 2^64 search into eight 2^8 searches. The
/// difference matters here in a way it would not for, say, comparing two
/// SSIDs.
fn eq_constant_time(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// `00 01 02 ... (N-1)` — the KEK in every RFC 3394 vector.
    fn counting<const N: usize>() -> [u8; N] {
        let mut k = [0u8; N];
        for (i, b) in k.iter_mut().enumerate() {
            *b = u8::try_from(i).expect("N <= 32");
        }
        k
    }

    /// The 128-bit key data used by RFC 3394 §4.1-4.3.
    const KEY_DATA_128: [u8; 16] = [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE,
        0xFF,
    ];

    #[test]
    fn rfc3394_section_4_1_wraps_128_bits_under_a_128_bit_kek() {
        let kek = Aes::new(&counting::<16>()).unwrap();
        let mut out = [0u8; 24];
        let n = wrap(&kek, &mut out, &KEY_DATA_128).unwrap();
        assert_eq!(n, 24);
        assert_eq!(
            out,
            [
                0x1F, 0xA6, 0x8B, 0x0A, 0x81, 0x12, 0xB4, 0x47, 0xAE, 0xF3, 0x4B, 0xD8, 0xFB, 0x5A,
                0x7B, 0x82, 0x9D, 0x3E, 0x86, 0x23, 0x71, 0xD2, 0xCF, 0xE5
            ]
        );

        let mut back = [0u8; 16];
        assert_eq!(unwrap(&kek, &mut back, &out).unwrap(), 16);
        assert_eq!(back, KEY_DATA_128);
    }

    #[test]
    fn rfc3394_section_4_3_wraps_128_bits_under_a_256_bit_kek() {
        let kek = Aes::new(&counting::<32>()).unwrap();
        let mut out = [0u8; 24];
        wrap(&kek, &mut out, &KEY_DATA_128).unwrap();
        assert_eq!(
            out,
            [
                0x64, 0xE8, 0xC3, 0xF9, 0xCE, 0x0F, 0x5B, 0xA2, 0x63, 0xE9, 0x77, 0x79, 0x05, 0x81,
                0x8A, 0x2A, 0x93, 0xC8, 0x19, 0x1E, 0x7D, 0x6E, 0x8A, 0xE7
            ]
        );
    }

    #[test]
    fn rfc3394_section_4_6_wraps_256_bits_under_a_256_bit_kek() {
        // The case disk encryption will use: a 256-bit volume key.
        let kek = Aes::new(&counting::<32>()).unwrap();
        let mut key_data = [0u8; 32];
        key_data[..16].copy_from_slice(&KEY_DATA_128);
        key_data[16..].copy_from_slice(&counting::<16>());
        let mut out = [0u8; 40];
        wrap(&kek, &mut out, &key_data).unwrap();
        assert_eq!(
            out,
            [
                0x28, 0xC9, 0xF4, 0x04, 0xC4, 0xB8, 0x10, 0xF4, 0xCB, 0xCC, 0xB3, 0x5C, 0xFB, 0x87,
                0xF8, 0x26, 0x3F, 0x57, 0x86, 0xE2, 0xD8, 0x0E, 0xD3, 0x26, 0xCB, 0xC7, 0xF0, 0xE7,
                0x1A, 0x99, 0xF4, 0x3B, 0xFB, 0x98, 0x8B, 0x9B, 0x7A, 0x02, 0xDD, 0x21
            ]
        );
        let mut back = [0u8; 32];
        assert_eq!(unwrap(&kek, &mut back, &out).unwrap(), 32);
        assert_eq!(back, key_data);
    }

    #[test]
    fn the_wrong_kek_is_rejected_rather_than_returning_garbage() {
        // The WPA case: a wrong pre-shared key gives a wrong KEK. Returning
        // the garbage plaintext would install a random GTK and turn a
        // diagnosable "wrong password" into "the network drops multicast".
        let kek = Aes::new(&counting::<16>()).unwrap();
        let mut out = [0u8; 24];
        wrap(&kek, &mut out, &KEY_DATA_128).unwrap();

        let mut wrong_key = counting::<16>();
        wrong_key[0] ^= 1;
        let wrong = Aes::new(&wrong_key).unwrap();
        let mut back = [0u8; 16];
        assert_eq!(
            unwrap(&wrong, &mut back, &out),
            Err(WrapError::IntegrityCheckFailed)
        );
    }

    #[test]
    fn a_flipped_bit_anywhere_in_the_ciphertext_is_caught() {
        let kek = Aes::new(&counting::<16>()).unwrap();
        let mut out = [0u8; 24];
        wrap(&kek, &mut out, &KEY_DATA_128).unwrap();
        for byte in 0..out.len() {
            for bit in 0..8u32 {
                let mut corrupt = out;
                corrupt[byte] ^= 1u8 << bit;
                let mut back = [0u8; 16];
                assert_eq!(
                    unwrap(&kek, &mut back, &corrupt),
                    Err(WrapError::IntegrityCheckFailed),
                    "bit {bit} of octet {byte} flipped and the check still passed"
                );
            }
        }
    }

    #[test]
    fn only_whole_semiblocks_of_at_least_two_are_accepted() {
        let kek = Aes::new(&counting::<16>()).unwrap();
        let mut out = [0u8; 64];
        let plain = [0u8; 48];
        for len in 0..48usize {
            let got = wrap(&kek, &mut out, &plain[..len]);
            if len >= 16 && len % 8 == 0 {
                assert_eq!(got, Ok(len + 8), "{len} octets is a valid key-wrap input");
            } else {
                assert_eq!(
                    got,
                    Err(WrapError::BadLength),
                    "{len} octets must be refused"
                );
            }
        }
    }

    #[test]
    fn every_gtk_size_round_trips() {
        // The sizes an 802.11 Key Data field actually carries: a 16-octet
        // CCMP GTK and a 32-octet TKIP one, each inside a GTK KDE with its
        // 6-octet header, padded to a multiple of 8.
        let kek = Aes::new(&counting::<16>()).unwrap();
        for len in [16usize, 24, 32, 40, 48] {
            let mut plain = [0u8; 48];
            for (i, b) in plain.iter_mut().enumerate().take(len) {
                *b = u8::try_from(i * 3 % 251).expect("small");
            }
            let mut wrapped = [0u8; 56];
            let n = wrap(&kek, &mut wrapped, &plain[..len]).unwrap();
            assert_eq!(n, len + 8);
            let mut back = [0u8; 48];
            assert_eq!(unwrap(&kek, &mut back[..len], &wrapped[..n]).unwrap(), len);
            assert_eq!(&back[..len], &plain[..len]);
        }
    }

    #[test]
    fn a_short_output_buffer_is_an_error_not_a_truncated_key() {
        let kek = Aes::new(&counting::<16>()).unwrap();
        for short in 0..24usize {
            let mut out = [0u8; 24];
            assert_eq!(
                wrap(&kek, &mut out[..short], &KEY_DATA_128),
                Err(WrapError::OutputTooSmall),
                "{short} octets must not suffice"
            );
        }
    }

    #[test]
    fn the_constant_time_comparison_still_compares() {
        assert!(eq_constant_time(&DEFAULT_IV, &[0xA6; 8]));
        assert!(!eq_constant_time(&DEFAULT_IV, &[0xA7; 8]));
        assert!(!eq_constant_time(&DEFAULT_IV, &[0xA6; 7]));
        // Differing only in the last octet must still be caught — the case a
        // buggy early-exit loop would get right and a buggy accumulator wrong.
        let mut nearly = DEFAULT_IV;
        nearly[7] ^= 0x01;
        assert!(!eq_constant_time(&DEFAULT_IV, &nearly));
    }
}
