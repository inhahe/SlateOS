//! RFC 1071 Internet checksum (the 16-bit one's-complement sum used by IPv4,
//! ICMP, UDP, and TCP headers).

/// Compute the RFC 1071 Internet checksum over `data`.
///
/// Sums the buffer as a sequence of big-endian 16-bit words (a trailing odd
/// byte is treated as the high byte of a final word), folds the carries, and
/// returns the one's complement. The result is in host order; write it to the
/// wire with [`u16::to_be_bytes`].
///
/// To *verify* a header, run this over the header including its checksum
/// field: a correct packet sums to `0`.
#[must_use]
pub fn internet(data: &[u8]) -> u16 {
    internet_continue(0, data)
}

/// Fold a running 32-bit accumulator's carries into 16 bits and return the
/// one's complement. Use with [`accumulate`] to checksum non-contiguous
/// regions (e.g. a pseudo-header followed by a payload).
#[must_use]
pub fn fold(mut sum: u32) -> u16 {
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Add `data` (as big-endian 16-bit words) into a running accumulator without
/// folding. Feed the final accumulator to [`fold`].
#[must_use]
pub fn accumulate(mut sum: u32, data: &[u8]) -> u32 {
    let mut i = 0;
    while i + 1 < data.len() {
        sum = sum.wrapping_add(u16::from_be_bytes([data[i], data[i + 1]]) as u32);
        i += 2;
    }
    if i < data.len() {
        // Trailing odd byte is the high byte of the final word.
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }
    sum
}

/// Convenience: accumulate `data` into `sum` and fold in one call.
#[must_use]
pub fn internet_continue(sum: u32, data: &[u8]) -> u16 {
    fold(accumulate(sum, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_all_ones() {
        // Sum of nothing is 0; one's complement is 0xFFFF.
        assert_eq!(internet(&[]), 0xFFFF);
    }

    #[test]
    fn known_rfc1071_example() {
        // Classic worked example: bytes 00 01 f2 03 f4 f5 f6 f7 → checksum 0x220d.
        let data = [0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
        assert_eq!(internet(&data), 0x220d);
    }

    #[test]
    fn odd_length_uses_high_byte() {
        // A single 0x42 byte is word 0x4200; complement is 0xBDFF.
        assert_eq!(internet(&[0x42]), 0xBDFF);
    }

    #[test]
    fn verify_sums_to_zero() {
        // Build a buffer, place its checksum, then re-checksum the whole thing:
        // a valid packet verifies to 0.
        let mut buf = [0u8; 20];
        for (i, b) in buf.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7).wrapping_add(1);
        }
        buf[10] = 0;
        buf[11] = 0;
        let csum = internet(&buf);
        buf[10..12].copy_from_slice(&csum.to_be_bytes());
        assert_eq!(internet(&buf), 0);
    }

    #[test]
    fn split_accumulate_matches_contiguous() {
        // Split accumulation only matches a contiguous checksum when each
        // chunk boundary is 16-bit aligned (an even byte offset). Splitting
        // mid-word would shift the trailing byte from a word's low half to its
        // high half, changing the sum — that's a property of RFC 1071, not a
        // bug, so this test splits on an even boundary.
        let a = [0x11u8, 0x22, 0x33, 0x44];
        let b = [0x55u8, 0x66, 0x77, 0x88];
        let mut whole = [0u8; 8];
        whole[..4].copy_from_slice(&a);
        whole[4..].copy_from_slice(&b);
        let split = fold(accumulate(accumulate(0, &a), &b));
        assert_eq!(split, internet(&whole));
    }
}
