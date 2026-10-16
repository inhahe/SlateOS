//! SipHash-2-4: a keyed 64-bit hash (Aumasson and Bernstein, 2012).
//!
//! For values an off-path attacker must not be able to predict without the
//! key: the TCP initial sequence numbers of RFC 6528 and the ephemeral-port
//! offsets of RFC 6056, both of which call for a keyed hash of a connection's
//! addresses (`crate::tcp_ids`). SipHash-2-4 is what the RFCs' "cryptographic
//! hash with a secret" has come to mean in practice, and what Linux uses for
//! both.
//!
//! Written out here rather than borrowed from `core::hash::SipHasher`, which is
//! the same function but deprecated, with no promise that it stays SipHash-2-4.
//! The tests hold this one to that type byte for byte, and to the reference
//! vectors of the SipHash paper.

/// The keyed hash of `data` under the 128-bit `key`.
#[must_use]
pub fn siphash24(key: &[u8; 16], data: &[u8]) -> u64 {
    let (k0, k1) = split_key(key);
    let mut v = [
        0x736f_6d65_7073_6575 ^ k0,
        0x646f_7261_6e64_6f6d ^ k1,
        0x6c79_6765_6e65_7261 ^ k0,
        0x7465_6462_7974_6573 ^ k1,
    ];
    // `as_chunks`, not `chunks_exact(8)`: newer clippy flags the latter for a
    // constant size, and the fixed-size arrays make each word a plain read.
    let (words, tail) = data.as_chunks::<8>();
    for word in words {
        let m = u64::from_le_bytes(*word);
        v[3] ^= m;
        round(&mut v);
        round(&mut v);
        v[0] ^= m;
    }
    // The last word: the remaining bytes, little-endian, with the message
    // length modulo 256 in the top byte.
    #[allow(clippy::cast_possible_truncation)] // only the low 8 bits are wanted
    let mut last = u64::from(data.len() as u8) << 56;
    for (i, &b) in tail.iter().enumerate() {
        last |= u64::from(b) << (8 * i);
    }
    v[3] ^= last;
    round(&mut v);
    round(&mut v);
    v[0] ^= last;
    v[2] ^= 0xff;
    for _ in 0..4 {
        round(&mut v);
    }
    v[0] ^ v[1] ^ v[2] ^ v[3]
}

/// One SipRound.
fn round(v: &mut [u64; 4]) {
    let [mut v0, mut v1, mut v2, mut v3] = *v;
    v0 = v0.wrapping_add(v1);
    v1 = v1.rotate_left(13);
    v1 ^= v0;
    v0 = v0.rotate_left(32);
    v2 = v2.wrapping_add(v3);
    v3 = v3.rotate_left(16);
    v3 ^= v2;
    v0 = v0.wrapping_add(v3);
    v3 = v3.rotate_left(21);
    v3 ^= v0;
    v2 = v2.wrapping_add(v1);
    v1 = v1.rotate_left(17);
    v1 ^= v2;
    v2 = v2.rotate_left(32);
    *v = [v0, v1, v2, v3];
}

/// The key's two little-endian 64-bit halves.
fn split_key(key: &[u8; 16]) -> (u64, u64) {
    let (lo, hi) = key.split_at(8);
    (le_word(lo), le_word(hi))
}

/// Up to eight bytes as a little-endian word (short input is zero-filled;
/// `split_key` only ever passes exact eights).
fn le_word(bytes: &[u8]) -> u64 {
    let mut w = [0u8; 8];
    for (dst, src) in w.iter_mut().zip(bytes) {
        *dst = *src;
    }
    u64::from_le_bytes(w)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The key the paper's vectors use: bytes 0..16.
    fn paper_key() -> [u8; 16] {
        core::array::from_fn(|i| u8::try_from(i).unwrap())
    }

    /// The message the paper's vectors use: bytes 0..n.
    fn paper_message(n: usize) -> Vec<u8> {
        (0..n).map(|i| u8::try_from(i).unwrap()).collect()
    }

    #[test]
    fn the_papers_worked_example() {
        // Appendix A of the SipHash paper: key 00..0f, message 00..0e.
        assert_eq!(
            siphash24(&paper_key(), &paper_message(15)),
            0xa129_ca61_49be_45e5
        );
    }

    #[test]
    fn the_reference_vector_for_an_empty_message() {
        // vectors[0] of the reference implementation, read little-endian:
        // 31 0e 0e dd 47 db 6f 72.
        assert_eq!(siphash24(&paper_key(), &[]), 0x726f_db47_dd0e_0e31);
    }

    /// `core`'s deprecated `SipHasher` is SipHash-2-4 over the bytes written
    /// to it, so the two must agree on every input.
    #[allow(deprecated)]
    fn core_siphash(key: &[u8; 16], data: &[u8]) -> u64 {
        use core::hash::Hasher;
        let (k0, k1) = split_key(key);
        let mut h = core::hash::SipHasher::new_with_keys(k0, k1);
        h.write(data);
        h.finish()
    }

    #[test]
    fn agrees_with_core_at_every_length_across_two_words() {
        let key = paper_key();
        for n in 0..=64 {
            let msg = paper_message(n);
            assert_eq!(
                siphash24(&key, &msg),
                core_siphash(&key, &msg),
                "length {n}"
            );
        }
    }

    #[test]
    fn agrees_with_core_under_other_keys_and_bytes() {
        let mut key = [0u8; 16];
        let mut msg = Vec::new();
        let mut x: u32 = 0x1234_5678;
        for round in 0..200 {
            // xorshift: a varied key and message without a dependency.
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            key[round % 16] = x.to_le_bytes()[0];
            msg.push(x.to_le_bytes()[1]);
            assert_eq!(
                siphash24(&key, &msg),
                core_siphash(&key, &msg),
                "round {round}"
            );
        }
    }

    #[test]
    fn the_key_changes_the_hash() {
        let msg = paper_message(12);
        let mut other = paper_key();
        other[0] ^= 1;
        assert_ne!(siphash24(&paper_key(), &msg), siphash24(&other, &msg));
    }
}
