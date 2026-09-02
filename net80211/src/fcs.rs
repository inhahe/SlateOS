//! The 802.11 Frame Check Sequence (§9.2.4.8).
//!
//! The FCS is the ordinary IEEE 802.3 CRC-32 — the same polynomial and the
//! same framing as an Ethernet FCS, a gzip trailer or a PNG chunk — computed
//! over the MAC header and the frame body, and appended as four octets in
//! **little-endian** order, like every other fixed field in 802.11.
//!
//! # Why this module exists at all
//!
//! On real hardware the FCS is computed and checked in the MAC, and the driver
//! never sees it: frames arrive already verified and already stripped. Two
//! callers still need it.
//!
//! - A **monitor-mode** capture usually keeps the FCS, and a capture whose
//!   checksum is not checked will happily hand up frames the radio itself
//!   would have discarded.
//! - A **simulated radio**, which is the only kind this project can test
//!   against — QEMU emulates no 802.11 device — has to generate the trailer
//!   itself, because there is no MAC underneath to do it.
//!
//! # The residue trick, and why it is not used here
//!
//! CRC-32 has the property that running it over a message *including* its own
//! correct checksum yields a fixed residue (`0x2144_DF1C` for this framing).
//! Verification via that constant is one comparison shorter than recomputing
//! and comparing, and it is a classic source of silent breakage: the residue
//! depends on the initial value, on whether the output is inverted, and on the
//! byte order the checksum was stored in, so a change to any of those three
//! turns the check into a no-op that still compiles. This module recomputes
//! and compares instead. It is the same cost to within a few nanoseconds on a
//! path that is already doing a per-frame CRC.

/// The length of the FCS trailer, in octets.
pub const FCS_LEN: usize = 4;

/// Compute the FCS over a frame that does *not* yet carry one.
#[must_use]
pub fn compute(frame: &[u8]) -> u32 {
    crc32::crc32(frame)
}

/// The four octets to append to `frame`, in transmission order.
#[must_use]
pub fn trailer(frame: &[u8]) -> [u8; FCS_LEN] {
    compute(frame).to_le_bytes()
}

/// Append an FCS to `frame` inside `out`, returning the total length written.
///
/// `None` if `out` cannot hold `frame.len() + 4` octets.
#[must_use]
pub fn append(out: &mut [u8], frame: &[u8]) -> Option<usize> {
    let total = frame.len().checked_add(FCS_LEN)?;
    let dst = out.get_mut(..total)?;
    dst.get_mut(..frame.len())?.copy_from_slice(frame);
    dst.get_mut(frame.len()..)?.copy_from_slice(&trailer(frame));
    Some(total)
}

/// Verify the trailing FCS of `buf` and return the frame without it.
///
/// Returns `None` if `buf` is not longer than the trailer — a frame that is
/// *only* a checksum is not a zero-length frame with a valid FCS, it is
/// garbage — or if the checksum does not match.
#[must_use]
pub fn verify_and_strip(buf: &[u8]) -> Option<&[u8]> {
    // Strictly greater: the shortest real frame is a 10-octet ACK, so a buffer
    // of exactly four octets cannot be a frame plus its FCS.
    if buf.len() <= FCS_LEN {
        return None;
    }
    let split = buf.len().checked_sub(FCS_LEN)?;
    let (frame, tail) = buf.split_at(split);
    let got = u32::from_le_bytes([*tail.first()?, *tail.get(1)?, *tail.get(2)?, *tail.get(3)?]);
    if got == compute(frame) {
        Some(frame)
    } else {
        None
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

    #[test]
    fn append_then_verify_round_trips() {
        let frame = b"a short 802.11 frame body";
        let mut buf = [0u8; 64];
        let n = append(&mut buf, frame).expect("fits");
        assert_eq!(n, frame.len() + FCS_LEN);
        assert_eq!(verify_and_strip(&buf[..n]), Some(&frame[..]));
    }

    #[test]
    fn the_trailer_is_little_endian() {
        // The FCS is stored least-significant octet first, like every other
        // fixed 802.11 field. Storing it big-endian is a bug that only shows
        // up against a real peer, so pin the byte order explicitly.
        let frame = b"123456789";
        assert_eq!(compute(frame), 0xCBF4_3926);
        assert_eq!(trailer(frame), [0x26, 0x39, 0xF4, 0xCB]);
    }

    #[test]
    fn a_flipped_bit_anywhere_is_caught() {
        let frame = b"a short 802.11 frame body";
        let mut buf = [0u8; 64];
        let n = append(&mut buf, frame).expect("fits");
        for byte in 0..n {
            for bit in 0..8u32 {
                let mut corrupt = buf;
                corrupt[byte] ^= 1u8 << bit;
                assert!(
                    verify_and_strip(&corrupt[..n]).is_none(),
                    "bit {bit} of octet {byte} flipped and the FCS still verified"
                );
            }
        }
    }

    #[test]
    fn a_buffer_no_longer_than_the_trailer_is_rejected() {
        // Including the case that would otherwise "verify": the CRC of the
        // empty string, stored as the whole buffer.
        let mut only_fcs = [0u8; FCS_LEN];
        only_fcs.copy_from_slice(&trailer(b""));
        assert!(verify_and_strip(&only_fcs).is_none());
        for short in 0..=FCS_LEN {
            assert!(verify_and_strip(&[0u8; FCS_LEN][..short]).is_none());
        }
    }

    #[test]
    fn appending_into_a_short_buffer_fails() {
        let frame = b"body";
        for short in 0..frame.len() + FCS_LEN {
            let mut buf = [0u8; 8];
            assert!(
                append(&mut buf[..short], frame).is_none(),
                "{short} octets must not suffice"
            );
        }
    }
}
