//! The bookkeeping shared by MD5, SHA-1 and SHA-256, written once.
//!
//! # Why this crate exists
//!
//! MD5, SHA-1 and SHA-256 differ entirely in their compression functions and
//! agree almost entirely on everything around them. All three consume the
//! message in fixed-size blocks, all three have to hold a partial block
//! between calls, and all three finish by appending a `0x80` byte, then zeros,
//! then the message length in bits, arranged so the length lands flush against
//! the end of the final block. The only disagreement is byte order: MD5 writes
//! that length little-endian, the SHA family big-endian.
//!
//! That shared part is also the part that is easy to get wrong. The
//! compression function is a transcription of a published table — tedious, but
//! a mistake in it shows up on the very first known-answer vector. The
//! buffering is different: its bugs live in the seams between calls, so an
//! implementation can hash `b"abc"` correctly, hash a 1 MiB file correctly,
//! and still be wrong for the one input that ends 55 bytes into a block, or
//! for the one caller that happens to split its input at byte 64. A test suite
//! that only checks the published vectors — all of which arrive in a single
//! call — cannot see any of it.
//!
//! So this is the piece worth having exactly one copy of. [`BlockBuffer`]
//! owns the partial block, the running length and the padding rule; the hash
//! crates own their compression function and nothing else.
//!
//! (The precedent is direct: RustCrypto factors the same concern into its
//! `block-buffer` crate, used by every hash it ships.)
//!
//! # What this crate does not do
//!
//! It does not hash. It has no notion of a digest, a state vector, or an
//! initial value — it hands out blocks and counts bytes. It also does not
//! know whether the caller's compression function is correct, which is why
//! each hash crate still carries its own known-answer vectors.
//!
//! # Usage
//!
//! The compression function is passed in rather than stored, so the buffer
//! never has to name the state it is feeding:
//!
//! ```
//! # use blockbuf::{BlockBuffer, LengthOrder};
//! // A toy "hash" that just counts the blocks it is given.
//! let mut blocks = 0_u32;
//! let mut buf = BlockBuffer::<64>::new();
//! buf.update(b"the quick brown fox", |_block| blocks += 1);
//! buf.finalize(LengthOrder::BigEndian, |_block| blocks += 1);
//! // 19 bytes of message + 1 + 8 fits in a single padded block.
//! assert_eq!(blocks, 1);
//! ```

#![no_std]

/// Byte order of the message-length field written by [`BlockBuffer::finalize`].
///
/// This is the *only* structural difference between MD5's padding and the SHA
/// family's, which is why it is a parameter rather than two functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthOrder {
    /// SHA-1 and SHA-256: length is big-endian.
    BigEndian,
    /// MD5: length is little-endian.
    LittleEndian,
}

/// Accumulates a byte stream and hands it out in `N`-byte blocks.
///
/// `N` is the block size of the hash: 64 for MD5, SHA-1 and SHA-256. It must
/// be at least 9 (room for the `0x80` byte and the 8-byte length) and at most
/// [`MAX_BLOCK_LEN`]; both are checked in [`BlockBuffer::new`].
#[derive(Clone)]
pub struct BlockBuffer<const N: usize> {
    /// Bytes received since the last full block was emitted. Only the first
    /// `buffered` are meaningful.
    buffer: [u8; N],
    /// Invariant: always `< N`. A block is emitted the moment it fills, so a
    /// full block is never left sitting here.
    buffered: usize,
    /// Total bytes absorbed, for the length field. Wraps at 2^64 bytes, which
    /// is the limit of the padding format itself rather than of this counter.
    total_len: u64,
}

/// The largest block size [`BlockBuffer`] supports, in bytes.
///
/// Bounded because [`BlockBuffer::finalize`] feeds its zero padding from a
/// fixed array rather than allocating one. 128 covers SHA-512, which is the
/// widest block in the family this crate serves.
pub const MAX_BLOCK_LEN: usize = 128;

/// Zero padding, fed from here so that `finalize` needs no array whose length
/// depends on `N` (which would require unstable const generic expressions).
/// The run of zeros is always shorter than one block, so one slice suffices.
const ZEROS: [u8; MAX_BLOCK_LEN] = [0; MAX_BLOCK_LEN];

impl<const N: usize> Default for BlockBuffer<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> core::fmt::Debug for BlockBuffer<N> {
    /// Deliberately omits the buffer contents.
    ///
    /// It holds up to `N` bytes of whatever is being hashed, which in this
    /// tree is routinely a password or a key. A derived `Debug` would put that
    /// in any log line that formatted a hasher built on this.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BlockBuffer")
            .field("block_len", &N)
            .field("bytes_absorbed", &self.total_len)
            .finish_non_exhaustive()
    }
}

impl<const N: usize> BlockBuffer<N> {
    /// Start an empty buffer.
    ///
    /// # Panics
    ///
    /// If `N` is outside `9..=MAX_BLOCK_LEN`. This is a compile-time-constant
    /// condition — `N` is a const generic — so it is a programming error in
    /// the calling crate, caught the first time that crate is exercised at
    /// all, and cannot depend on input.
    #[must_use]
    pub fn new() -> Self {
        assert!(
            N > 8 && N <= MAX_BLOCK_LEN,
            "block length must be in 9..=MAX_BLOCK_LEN"
        );
        Self {
            buffer: [0; N],
            buffered: 0,
            total_len: 0,
        }
    }

    /// Bytes absorbed so far, not counting padding.
    #[must_use]
    pub const fn bytes_absorbed(&self) -> u64 {
        self.total_len
    }

    /// Byte offset within a block at which the 64-bit length field begins.
    const fn length_offset() -> usize {
        N.saturating_sub(8)
    }

    /// Absorb `data`, calling `compress` once per complete block.
    ///
    /// Splitting the same byte stream differently across calls produces the
    /// same sequence of blocks — that is the entire contract, and the one
    /// [`tests::every_split_of_every_length_yields_the_same_blocks`] checks
    /// exhaustively.
    pub fn update(&mut self, data: &[u8], mut compress: impl FnMut(&[u8; N])) {
        self.total_len = self
            .total_len
            .wrapping_add(data.len().try_into().unwrap_or(u64::MAX));

        let mut rest = data;

        // Top up a partial block first, so the whole-block loop below can read
        // straight out of the caller's slice instead of copying through
        // `self.buffer`.
        if self.buffered > 0 {
            let space = N.saturating_sub(self.buffered);
            let take = space.min(rest.len());
            let end = self.buffered.saturating_add(take);
            if let (Some(dst), Some(src)) = (self.buffer.get_mut(self.buffered..end), rest.get(..take))
            {
                dst.copy_from_slice(src);
            }
            self.buffered = end;
            rest = rest.get(take..).unwrap_or(&[]);

            if self.buffered < N {
                // `take` was capped by `rest.len()` rather than by `space`, so
                // `rest` is now empty and there is nothing further to do. This
                // early return is what stops the remainder handling at the end
                // of the function from overwriting the partial block we just
                // added to — the bug this crate exists to have only one copy
                // of.
                return;
            }
            let block = self.buffer;
            compress(&block);
            self.buffered = 0;
        }

        let mut blocks = rest.chunks_exact(N);
        for chunk in &mut blocks {
            if let Some(block) = chunk.first_chunk::<N>() {
                compress(block);
            }
        }

        let remainder = blocks.remainder();
        if let Some(dst) = self.buffer.get_mut(..remainder.len()) {
            dst.copy_from_slice(remainder);
        }
        self.buffered = remainder.len();
    }

    /// Append the padding and the message length, flushing the final blocks.
    ///
    /// Takes `&mut self` rather than `self` so that the calling hash can
    /// decide for itself whether finishing consumes the hasher. (`sha2::Sha256`
    /// does, because a padded state must not be extended.)
    pub fn finalize(&mut self, order: LengthOrder, mut compress: impl FnMut(&[u8; N])) {
        // Captured before any padding is absorbed, because the padding must
        // not count towards the length it encodes.
        let bit_len = self.total_len.wrapping_mul(8);

        self.update(&[0x80], &mut compress);

        // Zeros needed so the length field lands flush against a block end.
        // Measured *after* the 0x80, so `buffered` is already its post-marker
        // value; `% N` handles the case where the marker pushed us past the
        // length offset and the length has to go in the following block.
        // `checked_rem` rather than `%` because `N` is a const generic that
        // the compiler cannot see is non-zero here — `new`'s assert is what
        // guarantees it, and clippy does not follow that. `None` is
        // unreachable; a zero-length run is the harmless reading of it.
        let zeros = Self::length_offset()
            .saturating_add(N)
            .saturating_sub(self.buffered)
            .checked_rem(N)
            .unwrap_or(0);
        if let Some(run) = ZEROS.get(..zeros) {
            self.update(run, &mut compress);
        }

        let len_bytes = match order {
            LengthOrder::BigEndian => bit_len.to_be_bytes(),
            LengthOrder::LittleEndian => bit_len.to_le_bytes(),
        };
        self.update(&len_bytes, &mut compress);

        debug_assert_eq!(
            self.buffered, 0,
            "padding must end exactly on a block boundary"
        );
    }
}

#[cfg(test)]
mod tests {
    // The five defensive lints the workspace turns on are for production code:
    // a test that indexes a fixed-size fixture, or unwraps a value it just
    // constructed, is *asserting*, and a panic there is the failure being
    // reported rather than a bug being introduced.
    #![allow(
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_used
    )]

    extern crate std;
    use super::{BlockBuffer, LengthOrder, MAX_BLOCK_LEN};
    use std::vec::Vec;

    /// Collect the blocks a whole message produces, in order.
    fn blocks_of(data: &[u8], order: LengthOrder) -> Vec<[u8; 64]> {
        let mut out = Vec::new();
        let mut buf = BlockBuffer::<64>::new();
        buf.update(data, |b| out.push(*b));
        buf.finalize(order, |b| out.push(*b));
        out
    }

    #[test]
    fn an_empty_message_still_produces_one_padded_block() {
        let blocks = blocks_of(b"", LengthOrder::BigEndian);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0][0], 0x80);
        assert!(blocks[0][1..].iter().all(|&b| b == 0));
    }

    #[test]
    fn the_length_is_in_bits_not_bytes() {
        // 3 bytes = 24 bits = 0x18, in the last byte for big-endian.
        let blocks = blocks_of(b"abc", LengthOrder::BigEndian);
        assert_eq!(blocks[0][63], 24);
        assert!(blocks[0][56..63].iter().all(|&b| b == 0));
    }

    #[test]
    fn length_order_flips_only_the_length_field() {
        let be = blocks_of(b"abc", LengthOrder::BigEndian);
        let le = blocks_of(b"abc", LengthOrder::LittleEndian);
        assert_eq!(be[0][..56], le[0][..56], "message and padding must match");
        assert_eq!(be[0][63], 24);
        assert_eq!(le[0][56], 24);
    }

    #[test]
    fn a_message_ending_at_the_length_offset_needs_a_second_block() {
        // 55 bytes leaves exactly room for the 0x80 and the length: one block.
        assert_eq!(blocks_of(&[b'y'; 55], LengthOrder::BigEndian).len(), 1);
        // 56 bytes leaves room for the 0x80 but not the length: two blocks.
        // This is the off-by-one that every hand-written padding gets wrong.
        assert_eq!(blocks_of(&[b'z'; 56], LengthOrder::BigEndian).len(), 2);
        // A message that is exactly one block still needs a second for the
        // length, even though nothing is left over.
        assert_eq!(blocks_of(&[b'x'; 64], LengthOrder::BigEndian).len(), 2);
    }

    #[test]
    fn every_split_of_every_length_yields_the_same_blocks() {
        // The contract of `update`. Checked at every length up to three blocks
        // and, within each length, at every possible split point — because
        // buffering bugs live in the seam between two calls and are invisible
        // to any test that hashes its input in one go.
        for len in 0..=(64 * 3 + 9) {
            let msg: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let whole = blocks_of(&msg, LengthOrder::BigEndian);
            for split in 0..=len {
                let mut split_blocks = Vec::new();
                let mut buf = BlockBuffer::<64>::new();
                buf.update(&msg[..split], |b| split_blocks.push(*b));
                buf.update(&msg[split..], |b| split_blocks.push(*b));
                buf.finalize(LengthOrder::BigEndian, |b| split_blocks.push(*b));
                assert_eq!(
                    split_blocks, whole,
                    "len {len} split at {split} disagreed with the whole message"
                );
            }
        }
    }

    #[test]
    fn a_byte_at_a_time_matches_all_at_once() {
        let msg: Vec<u8> = (0..200_u32).map(|i| (i % 251) as u8).collect();
        let whole = blocks_of(&msg, LengthOrder::BigEndian);

        let mut drip = Vec::new();
        let mut buf = BlockBuffer::<64>::new();
        for byte in &msg {
            buf.update(&[*byte], |b| drip.push(*b));
        }
        buf.finalize(LengthOrder::BigEndian, |b| drip.push(*b));
        assert_eq!(drip, whole);
    }

    #[test]
    fn empty_updates_change_nothing() {
        let mut with_empties = Vec::new();
        let mut buf = BlockBuffer::<64>::new();
        buf.update(b"", |b| with_empties.push(*b));
        buf.update(b"ab", |b| with_empties.push(*b));
        buf.update(b"", |b| with_empties.push(*b));
        buf.update(b"c", |b| with_empties.push(*b));
        buf.finalize(LengthOrder::BigEndian, |b| with_empties.push(*b));
        assert_eq!(with_empties, blocks_of(b"abc", LengthOrder::BigEndian));
    }

    #[test]
    fn bytes_absorbed_does_not_count_padding() {
        let mut buf = BlockBuffer::<64>::new();
        buf.update(&[0u8; 100], |_| {});
        assert_eq!(buf.bytes_absorbed(), 100);
    }

    #[test]
    fn other_block_sizes_pad_to_their_own_boundary() {
        // SHA-512's block size, to check nothing assumes 64.
        let mut count = 0_usize;
        let mut buf = BlockBuffer::<MAX_BLOCK_LEN>::new();
        buf.update(&[0u8; MAX_BLOCK_LEN], |_| count += 1);
        buf.finalize(LengthOrder::BigEndian, |_| count += 1);
        assert_eq!(count, 2);
    }

    #[test]
    #[should_panic(expected = "block length")]
    fn a_block_too_small_to_hold_the_length_is_rejected() {
        let _ = BlockBuffer::<8>::new();
    }
}
