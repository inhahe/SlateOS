//! The Internet checksum (RFC 1071), in one place.
//!
//! Every transport and network protocol in this stack needs the same 16-bit
//! one's-complement sum. Before this module existed, each one carried its own
//! copy of it: seven verbatim copies of the data loop across `ipv4.rs`,
//! `ipv6.rs` and `tcp.rs`, four copies of the IPv4 pseudo-header prologue, and
//! four of the IPv6 one.
//!
//! That was not merely untidy — it produced a measurable, invisible bug. Each
//! copy is a separate function body as far as LLVM is concerned, so each one
//! independently wins or loses the unrolling lottery. On the `e1de4aaaa`
//! kernel, `tcp_checksum_v6`'s loop was unrolled 2× (`addq $0x4, %rcx` —
//! two 16-bit words per iteration) while `tcp_checksum`'s, inlined into a
//! different caller, was not. The two functions have byte-identical loop
//! *source*, and the benchmarks exist specifically to compare them against
//! each other, so a 1844-cycle (34%) gap over the same 1460-byte segment was
//! read as "IPv4's pseudo-header is dearer" when the pseudo-headers had
//! nothing to do with it. Duplicated source that is *supposed* to be identical
//! is worse than obviously-different source, because it invites exactly that
//! inference.
//!
//! So: one loop, compiled once, and every caller gets whatever that one loop
//! is worth. Optimising the checksum is now a single edit that lifts the whole
//! stack rather than seven edits that drift apart again.
//!
//! # The arithmetic
//!
//! The sum is over 16-bit big-endian words, in one's-complement (end-around
//! carry). Two properties are used throughout:
//!
//! - **It is associative and commutative**, so a partial sum may be
//!   accumulated in any order and carries folded at the end rather than after
//!   every add. That is why [`sum_bytes`] takes and returns a running `u32`:
//!   pseudo-header and payload are summed into one accumulator with a single
//!   fold at the end.
//! - **A `u32` accumulator cannot overflow for any segment we will ever
//!   checksum.** Each word contributes at most `0xFFFF`, so overflow needs
//!   more than 65537 words — 128 KiB — and the largest thing here is a 64 KiB
//!   IP datagram. The `wrapping_add`s are therefore documentation of intent,
//!   not a live concern; they exist so the code is correct rather than
//!   merely lucky if that ever changes.

use super::interface::Ipv4Addr;
use super::ipv6::Ipv6Addr;

/// Accumulate the one's-complement sum of `data` into `sum` and return it.
///
/// `data` may be any length; an odd trailing byte is treated as the high half
/// of a final word, per RFC 1071's "pad with zero" rule. The result is *not*
/// folded — call [`fold`] or [`finish`] when the whole message has been summed.
#[allow(clippy::arithmetic_side_effects)]
#[allow(clippy::indexing_slicing)]
#[must_use]
pub fn sum_bytes(mut sum: u32, data: &[u8]) -> u32 {
    let mut i = 0;
    while i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]);
        sum = sum.wrapping_add(u32::from(word));
        i += 2;
    }
    // Odd trailing byte: it is the *high* half of the final word, so shift it
    // left. Padding on the wrong side is the classic way to get a checksum
    // that is right for even-length messages and wrong for odd-length ones —
    // which is to say, right in most tests.
    if i < data.len() {
        sum = sum.wrapping_add(u32::from(data[i]) << 8);
    }
    sum
}

/// Fold a 32-bit accumulator down to 16 bits with end-around carry.
///
/// The result is in `0..=0xFFFF`. It is *not* complemented, so this is what
/// the verify paths want: a message whose checksum field is intact sums to
/// `0xFFFF` here.
#[allow(clippy::arithmetic_side_effects)]
#[must_use]
pub fn fold(mut sum: u32) -> u32 {
    // Two iterations always suffice for a u32 (the first fold leaves at most
    // 0x1FFFE), but the loop costs nothing and stays correct if the
    // accumulator ever widens.
    while sum > 0xFFFF {
        sum = (sum & 0xFFFF).wrapping_add(sum >> 16);
    }
    sum
}

/// Fold and complement: the value to write into a checksum field.
#[must_use]
pub fn finish(sum: u32) -> u16 {
    !fold(sum) as u16
}

/// The one's-complement sum of the IPv4 pseudo-header (RFC 793 / 768).
///
/// The pseudo-header is source address, destination address, a zero byte, the
/// protocol number and the 16-bit segment length — six 16-bit words. They are
/// summed directly rather than materialised into a `[u8; 12]` that is
/// immediately read back: see design-decisions.md §251, where doing the latter
/// cost 688 cycles per TCP checksum.
#[allow(clippy::arithmetic_side_effects)]
#[must_use]
pub fn pseudo_v4(src: Ipv4Addr, dst: Ipv4Addr, protocol: u8, seg_len: u16) -> u32 {
    let mut sum: u32 = 0;
    sum = sum.wrapping_add(u32::from(u16::from_be_bytes([src.0[0], src.0[1]])));
    sum = sum.wrapping_add(u32::from(u16::from_be_bytes([src.0[2], src.0[3]])));
    sum = sum.wrapping_add(u32::from(u16::from_be_bytes([dst.0[0], dst.0[1]])));
    sum = sum.wrapping_add(u32::from(u16::from_be_bytes([dst.0[2], dst.0[3]])));
    // The zero byte and the protocol byte form one word: 0x00PP.
    sum = sum.wrapping_add(u32::from(protocol));
    sum = sum.wrapping_add(u32::from(seg_len));
    sum
}

/// The one's-complement sum of the IPv6 pseudo-header (RFC 8200 §8.1).
///
/// 40 bytes: source address (16), destination address (16), upper-layer packet
/// length (4, and genuinely 32-bit here, unlike IPv4's 16), three zero bytes
/// and the next-header value.
#[allow(clippy::arithmetic_side_effects)]
#[allow(clippy::indexing_slicing)]
#[must_use]
pub fn pseudo_v6(src: &Ipv6Addr, dst: &Ipv6Addr, next_header: u8, seg_len: u32) -> u32 {
    let mut sum: u32 = 0;
    for i in 0..8 {
        sum = sum.wrapping_add(u32::from(u16::from_be_bytes([src.0[i * 2], src.0[i * 2 + 1]])));
    }
    for i in 0..8 {
        sum = sum.wrapping_add(u32::from(u16::from_be_bytes([dst.0[i * 2], dst.0[i * 2 + 1]])));
    }
    // Upper-layer packet length is a full 32-bit field: two words.
    sum = sum.wrapping_add(seg_len >> 16);
    sum = sum.wrapping_add(seg_len & 0xFFFF);
    // Three zero bytes + next header: only the last word is non-zero.
    sum = sum.wrapping_add(u32::from(next_header));
    sum
}

/// A message with an intact checksum field folds to this value.
pub const VALID: u32 = 0xFFFF;

/// Boot-time self-test. **This is the whole test suite for this module** —
/// there is deliberately no `#[cfg(test)] mod tests`.
///
/// `kernel/Cargo.toml` sets `test = false` on the kernel binary (the kernel
/// supplies its own `panic_impl` and other `no_std` lang items, which cannot
/// link against host `std`). A `#[cfg(test)]` module here would therefore never
/// be compiled, let alone run: it would look like coverage, contribute none,
/// and rot into non-compiling code without anything noticing. Tests that cannot
/// run are worse than no tests, because they get counted as tests.
///
/// Everything consequently lives here, where the target actually executes it.
/// That matters more than usual for this module: since the unification, *every*
/// checksum in the stack goes through [`sum_bytes`], so one wrong fold or one
/// mis-padded odd byte corrupts TCP, UDP, ICMP, ICMPv6 and the IPv4 header
/// check simultaneously.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    // RFC 1071 §3's own worked example. An external vector, so it cannot
    // agree with a bug that this code and a test written from this code
    // would share.
    let sum = sum_bytes(0, &[0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7]);
    if fold(sum) != 0xddf2 || finish(sum) != 0x220d {
        crate::serial_println!(
            "[cksum]   FAIL: RFC 1071 vector: fold={:#x} finish={:#x} (want 0xddf2/0x220d)",
            fold(sum),
            finish(sum)
        );
        return Err(KernelError::InternalError);
    }

    // The odd trailing byte pads on the *high* side. Padding low is the
    // classic form of this bug: it is correct for every even-length message,
    // so it survives any test suite that forgets odd lengths.
    if sum_bytes(0, &[0xAB]) != 0xAB00 {
        crate::serial_println!("[cksum]   FAIL: odd trailing byte padded on the wrong side");
        return Err(KernelError::InternalError);
    }

    // Splitting at an even boundary must not change the sum — the property
    // that lets pseudo-header and payload share one accumulator. The message is
    // odd-length so the tail byte is exercised through the split path too.
    let msg = [0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07];
    #[allow(clippy::indexing_slicing)]
    if sum_bytes(0, &msg) != sum_bytes(sum_bytes(0, &msg[..4]), &msg[4..]) {
        crate::serial_println!("[cksum]   FAIL: accumulator is not split-invariant");
        return Err(KernelError::InternalError);
    }

    // An empty slice is the identity: `sum_bytes` must pass its accumulator
    // straight through. Callers rely on this for zero-length payloads (a bare
    // TCP ACK is a header and nothing else), where the segment loop runs zero
    // times and the pseudo-header sum must survive untouched.
    if sum_bytes(0x1234, &[]) != 0x1234 {
        crate::serial_println!("[cksum]   FAIL: empty slice is not the identity");
        return Err(KernelError::InternalError);
    }

    // Fold boundaries: the exact values where end-around carry does and does
    // not fire. `0x1_FFFE` is the boundary case — one round takes it to
    // exactly `0xFFFF`, the largest folded value, so an off-by-one in the loop
    // condition (`>=` for `>`) would spin it to 0 here and nowhere else.
    // `0xFFFF_FFFF` in the idempotence loop below is the companion: it is the
    // only input needing *two* rounds (-> 0x1_FFFE -> 0xFFFF), so a fold
    // written as a single `if` rather than a loop fails there and passes
    // everything else.
    if fold(0) != 0 || fold(0xFFFF) != 0xFFFF || fold(0x1_0000) != 1 || fold(0x1_FFFE) != 0xFFFF {
        crate::serial_println!("[cksum]   FAIL: fold boundary values");
        return Err(KernelError::InternalError);
    }
    for v in [0u32, 1, 0x7FFF, 0xFFFF, 0x1_0000, 0xFFFF_FFFF] {
        if fold(v) != fold(fold(v)) {
            crate::serial_println!("[cksum]   FAIL: fold is not idempotent at {v:#x}");
            return Err(KernelError::InternalError);
        }
    }

    // Round trip: stamp a computed checksum back into a message and the whole
    // thing must fold to VALID. This is the invariant every verify path in the
    // stack relies on, so it is the one worth proving on the target.
    let mut hdr = [0x45u8, 0x00, 0x00, 0x3c, 0x1c, 0x46, 0x40, 0x00, 0x00, 0x00];
    let cksum = finish(sum_bytes(0, &hdr));
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    {
        hdr[8] = (cksum >> 8) as u8;
        hdr[9] = cksum as u8;
    }
    if fold(sum_bytes(0, &hdr)) != VALID {
        crate::serial_println!("[cksum]   FAIL: stamped checksum does not verify");
        return Err(KernelError::InternalError);
    }

    // A 64 KiB message of 0xFF is the worst case a u32 accumulator can meet
    // (32768 words x 0xFFFF = 0x7FFF_8000). The module docs claim it cannot
    // overflow; this is that claim, executed. It also happens to be the only
    // long-input case here, so it exercises whatever unrolled form LLVM chose
    // for the loop rather than just its scalar tail.
    let big = alloc::vec![0xFFu8; 65536];
    let big_sum = sum_bytes(0, &big);
    if big_sum != 0x7FFF_8000 || fold(big_sum) != 0xFFFF {
        crate::serial_println!(
            "[cksum]   FAIL: 64 KiB worst case gave {big_sum:#x} (want 0x7fff8000)"
        );
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[cksum] Internet checksum self-test PASSED (7 checks)");
    Ok(())
}
