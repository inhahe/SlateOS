//! The compressed data, handed over as libjpeg's source managers hand it.
//!
//! libjpeg reads its input through a "source manager" that refills a buffer
//! on demand. Every non-suspending one -- `jdatasrc.c`'s memory source, its
//! stdio source, libtiff's strip source -- does the same thing when the data
//! runs out: it warns, and inserts a fake end-of-image marker, and does so
//! again every time it is asked for more. A decoder reading past the end
//! therefore sees `FF D9 FF D9 ...` for ever, and all of libjpeg's behaviour on
//! a cut-off file follows from that one convention: the entropy decoder meets
//! a marker and fills the rest of the scan with zeros, a header that has not
//! reached its scan ends at `EOI`, and a finish finds the end it looks for.
//! [`Source`] is that convention.
//!
//! Skipping follows libtiff's `std_skip_input_data`: a skip longer than what is
//! left abandons the rest and starts a fresh fake marker. `jdatasrc.c` instead
//! counts the skip through as many fake markers as it spans, which can leave it
//! one byte into one; the next marker read then discards that byte and finds
//! the `EOI` after it. Both reach the same marker, so which is modelled changes
//! nothing that decoding produces.

use alloc::borrow::Cow;
use alloc::vec::Vec;

/// The bytes of one datastream, then `FF D9` repeated.
#[derive(Debug, Clone)]
pub(super) struct Source<'a> {
    data: Cow<'a, [u8]>,
    /// The next real byte; `data.len()` once they are all read.
    at: usize,
    /// Bytes left in the current fake marker: 2, 1, or 0 when the next read
    /// must "refill".
    fake_left: u8,
    /// How often the data has run out (a `JWRN_JPEG_EOF` each time).
    pub(super) ran_out: u32,
    /// Whether running out is a failure rather than the fake markers: the
    /// source libtiff builds for old-style JPEG, whose data can end in "no
    /// more to give" rather than in end-of-image markers.
    hard_end: bool,
    /// Set once data was asked for past a hard end.
    pub(super) overrun: bool,
}

impl<'a> Source<'a> {
    /// A source over `data`.
    pub(super) const fn new(data: &'a [u8]) -> Self {
        Self {
            data: Cow::Borrowed(data),
            at: 0,
            fake_left: 0,
            ran_out: 0,
            hard_end: false,
            overrun: false,
        }
    }

    /// A source over bytes of its own.
    pub(super) const fn owned(data: Vec<u8>) -> Source<'static> {
        Source {
            data: Cow::Owned(data),
            at: 0,
            fake_left: 0,
            ran_out: 0,
            hard_end: false,
            overrun: false,
        }
    }

    /// Make running out a failure ([`Self::overrun`]).
    pub(super) const fn set_hard_end(&mut self) {
        self.hard_end = true;
    }

    /// `INPUT_BYTE`: the next byte, real or fake.
    pub(super) fn byte(&mut self) -> u8 {
        if let Some(&byte) = self.data.get(self.at) {
            self.at = self.at.saturating_add(1);
            return byte;
        }
        if self.hard_end {
            self.overrun = true;
        }
        if self.fake_left == 0 {
            self.refill();
        }
        let byte = if self.fake_left == 2 { 0xFF } else { 0xD9 };
        self.fake_left = self.fake_left.saturating_sub(1);
        byte
    }

    /// The next eight real bytes as a big-endian word, without taking them:
    /// `None` within eight bytes of the end, where [`Self::byte`] is the way
    /// on. For a reader that takes several bytes at once and has to see
    /// them first -- the entropy decoder, which may take none of them.
    #[inline]
    pub(super) fn peek8(&self) -> Option<u64> {
        let bytes = self.data.get(self.at..self.at.checked_add(8)?)?;
        Some(u64::from_be_bytes(bytes.try_into().ok()?))
    }

    /// Take `n` real bytes that [`Self::peek8`] showed: never past the end.
    #[inline]
    pub(super) fn advance(&mut self, n: usize) {
        self.at = self.at.saturating_add(n).min(self.data.len());
    }

    /// `INPUT_2BYTES`: a big-endian 16-bit value.
    pub(super) fn word(&mut self) -> u16 {
        let high = self.byte();
        let low = self.byte();
        u16::from_be_bytes([high, low])
    }

    /// `skip_input_data`, as libtiff's source does it.
    pub(super) fn skip(&mut self, count: u64) {
        if count == 0 {
            return;
        }
        let real_left = self.data.len().saturating_sub(self.at);
        let left = if real_left > 0 {
            real_left as u64
        } else {
            u64::from(self.fake_left)
        };
        if count > left {
            self.at = self.data.len();
            self.refill();
        } else if real_left > 0 {
            // `count <= real_left`, so it fits a usize.
            self.at = self
                .at
                .saturating_add(usize::try_from(count).unwrap_or(real_left));
        } else {
            self.fake_left = self
                .fake_left
                .saturating_sub(u8::try_from(count).unwrap_or(2));
        }
    }

    /// `fill_input_buffer` with nothing left to give: a fresh `FF D9`.
    fn refill(&mut self) {
        self.fake_left = 2;
        self.ran_out = self.ran_out.saturating_add(1);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    fn take(source: &mut Source<'_>, n: usize) -> Vec<u8> {
        (0..n).map(|_| source.byte()).collect()
    }

    #[test]
    fn the_data_then_end_of_image_markers_for_ever() {
        let mut source = Source::new(&[1, 2, 3]);
        assert_eq!(
            take(&mut source, 9),
            [1, 2, 3, 0xFF, 0xD9, 0xFF, 0xD9, 0xFF, 0xD9]
        );
        assert_eq!(source.ran_out, 3);
    }

    #[test]
    fn nothing_at_all_is_an_end_of_image_marker() {
        let mut source = Source::new(&[]);
        assert_eq!(source.word(), 0xFFD9);
    }

    #[test]
    fn a_skip_within_the_data_moves_over_it() {
        let mut source = Source::new(&[1, 2, 3, 4]);
        source.skip(2);
        assert_eq!(take(&mut source, 3), [3, 4, 0xFF]);
    }

    #[test]
    fn a_skip_past_the_end_starts_a_fresh_marker() {
        let mut source = Source::new(&[1, 2, 3]);
        source.byte();
        source.skip(3);
        assert_eq!(take(&mut source, 2), [0xFF, 0xD9]);
        // Half way into a fake marker, a skip of one lands on its second byte
        // and a longer one starts another.
        source.byte();
        source.skip(1);
        assert_eq!(source.byte(), 0xFF);
        source.skip(5);
        assert_eq!(take(&mut source, 2), [0xFF, 0xD9]);
    }
}
