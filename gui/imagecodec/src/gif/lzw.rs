//! GIF's LZW: codes of 3 to 12 bits, least significant bit first, read across
//! the data sub-blocks of one image.
//!
//! The dictionary starts with one entry per colour index plus two control
//! codes -- *clear*, which starts the dictionary again, and *end* -- and every
//! code read adds the entry "the previous code's string plus the first
//! character of this one's". Codes widen by a bit whenever the next free code
//! would no longer fit, up to 12 bits and 4096 entries.
//!
//! Three things are where decoders go wrong, and each has a test:
//!
//! - **The code not yet in the table.** An encoder may use the entry it is
//!   about to define ("KwKwK"): the decoder has not added it yet, and must build
//!   it from the previous string and that string's own first character.
//! - **When the width grows.** The decoder's table lags the encoder's by one
//!   entry, so it widens after adding the entry that makes its next free code
//!   `1 << width`, one code later than the encoder did.
//! - **A full table.** At 4096 entries an encoder may send a clear -- or not
//!   (a "deferred clear", which the specification allows): the decoder then
//!   keeps reading 12-bit codes against the table it has, adding nothing.

use alloc::vec;
use alloc::vec::Vec;

/// The most entries a GIF dictionary holds: every 12-bit code.
const MAX_CODES: usize = 4096;

/// The widest a code gets.
const MAX_WIDTH: u32 = 12;

/// An image's data sub-blocks as one stream of bytes: each sub-block is a
/// length byte and that many bytes, and a zero length ends them.
pub(super) struct SubBlocks<'a> {
    bytes: &'a [u8],
    /// The next byte to read.
    at: usize,
    /// Bytes left in the current sub-block.
    left: usize,
    /// Whether the terminating zero-length sub-block has been read.
    ended: bool,
}

impl<'a> SubBlocks<'a> {
    /// The sub-blocks starting at `at`, which is a length byte.
    pub(super) const fn new(bytes: &'a [u8], at: usize) -> Self {
        Self {
            bytes,
            at,
            left: 0,
            ended: false,
        }
    }

    /// The next byte of data, or `None` at the terminator or the end of the
    /// file.
    fn next_byte(&mut self) -> Option<u8> {
        while self.left == 0 {
            if self.ended {
                return None;
            }
            let length = *self.bytes.get(self.at)?;
            self.at = self.at.saturating_add(1);
            if length == 0 {
                self.ended = true;
                return None;
            }
            self.left = usize::from(length);
        }
        let byte = *self.bytes.get(self.at)?;
        self.at = self.at.saturating_add(1);
        self.left = self.left.saturating_sub(1);
        Some(byte)
    }

    /// Step past whatever is left of these sub-blocks, and say where the next
    /// block of the file starts: after the terminator, or at the end of the
    /// file if it never came.
    pub(super) fn finish(mut self) -> usize {
        self.at = self.at.saturating_add(self.left);
        self.left = 0;
        while !self.ended {
            let Some(&length) = self.bytes.get(self.at) else {
                return self.bytes.len();
            };
            self.at = self.at.saturating_add(1);
            if length == 0 {
                self.ended = true;
            } else {
                self.at = self.at.saturating_add(usize::from(length));
            }
        }
        self.at.min(self.bytes.len())
    }
}

/// Codes out of the sub-blocks, least significant bit first.
struct Bits<'s, 'a> {
    source: &'s mut SubBlocks<'a>,
    /// Bits read and not yet used, the next one lowest.
    acc: u32,
    /// How many of them.
    count: u32,
}

impl Bits<'_, '_> {
    /// The next `width`-bit code, or `None` if the data ran out first.
    fn read(&mut self, width: u32) -> Option<usize> {
        while self.count < width {
            let byte = self.source.next_byte()?;
            // `count` is under 12 here, so the byte lands below bit 20.
            self.acc |= u32::from(byte).checked_shl(self.count).unwrap_or(0);
            self.count = self.count.saturating_add(8);
        }
        let mask = 1u32
            .checked_shl(width)
            .map_or(u32::MAX, |b| b.wrapping_sub(1));
        let code = self.acc & mask;
        self.acc = self.acc.checked_shr(width).unwrap_or(0);
        self.count = self.count.saturating_sub(width);
        usize::try_from(code).ok()
    }
}

/// The dictionary, kept between images so an animation allocates it once.
///
/// Each entry is a string of colour indices stored as the entry it extends
/// plus one index, with its length and first index kept alongside: the length
/// so a string can be written straight into place back to front, the first
/// index because every new entry needs it.
pub(super) struct Table {
    prefix: Vec<u16>,
    suffix: Vec<u8>,
    first: Vec<u8>,
    length: Vec<u16>,
}

impl Table {
    pub(super) fn new() -> Self {
        Self {
            prefix: vec![0; MAX_CODES],
            suffix: vec![0; MAX_CODES],
            first: vec![0; MAX_CODES],
            length: vec![0; MAX_CODES],
        }
    }

    /// Decode one image's data into `out`, in the order the file sends it
    /// (interlaced or not, that is the caller's to undo), and return how many
    /// indices were written: all of `out` unless the data ended -- or stopped
    /// making sense -- first.
    ///
    /// `min_code_size` is the byte before the data, 2 to 8 (the caller refuses
    /// the rest: 1 is outside the format, and decoders that accept it disagree
    /// about when its codes widen). Anything past the
    /// end code or past the last pixel is not read; a code that names an entry
    /// that does not exist yet ends the image where it is, as a truncated file
    /// would.
    pub(super) fn decode(
        &mut self,
        min_code_size: u8,
        data: &mut SubBlocks<'_>,
        out: &mut [u8],
    ) -> usize {
        let min = u32::from(min_code_size.clamp(2, 8));
        let clear = 1usize << min;
        let end = clear.saturating_add(1);
        for index in 0..clear {
            let byte = u8::try_from(index).unwrap_or(0);
            if let Some(slot) = self.suffix.get_mut(index) {
                *slot = byte;
            }
            if let Some(slot) = self.first.get_mut(index) {
                *slot = byte;
            }
            if let Some(slot) = self.length.get_mut(index) {
                *slot = 1;
            }
        }
        let mut bits = Bits {
            source: data,
            acc: 0,
            count: 0,
        };
        let mut width = min.saturating_add(1);
        let mut next = end.saturating_add(1);
        let mut previous: Option<usize> = None;
        let mut written = 0usize;
        while written < out.len() {
            let Some(code) = bits.read(width) else {
                break;
            };
            if code == clear {
                width = min.saturating_add(1);
                next = end.saturating_add(1);
                previous = None;
                continue;
            }
            if code == end {
                break;
            }
            match previous {
                // The first code after a clear is a single index.
                None if code < clear => {}
                None => break,
                Some(previous) => {
                    // The new entry: the previous string and the first index
                    // of this one -- which, for the one code not yet in the
                    // table, is the previous string's own first index.
                    let first = match code.cmp(&next) {
                        core::cmp::Ordering::Less => self.first.get(code).copied(),
                        core::cmp::Ordering::Equal => self.first.get(previous).copied(),
                        core::cmp::Ordering::Greater => None,
                    };
                    let Some(first) = first else {
                        break;
                    };
                    if next < MAX_CODES {
                        self.add(next, previous, first);
                        next = next.saturating_add(1);
                    }
                }
            }
            written = self.write(code, out, written);
            previous = Some(code);
            if next >= (1usize << width) && width < MAX_WIDTH {
                width = width.saturating_add(1);
            }
        }
        written
    }

    /// Entry `at`: entry `prefix`'s string, then `index`.
    fn add(&mut self, at: usize, prefix: usize, index: u8) {
        let length = self.length.get(prefix).copied().unwrap_or(0);
        let first = self.first.get(prefix).copied().unwrap_or(0);
        if let Some(slot) = self.prefix.get_mut(at) {
            *slot = u16::try_from(prefix).unwrap_or(0);
        }
        if let Some(slot) = self.suffix.get_mut(at) {
            *slot = index;
        }
        if let Some(slot) = self.first.get_mut(at) {
            *slot = first;
        }
        if let Some(slot) = self.length.get_mut(at) {
            *slot = length.saturating_add(1);
        }
    }

    /// Write `code`'s string into `out` at `at`, back to front along its
    /// chain of prefixes, and return where the next string goes. What would
    /// fall past the end of `out` is dropped.
    fn write(&self, code: usize, out: &mut [u8], at: usize) -> usize {
        let length = usize::from(self.length.get(code).copied().unwrap_or(0));
        let stop = at.saturating_add(length);
        let mut position = stop;
        let mut entry = code;
        for _ in 0..length {
            position = position.saturating_sub(1);
            if let Some(slot) = out.get_mut(position) {
                *slot = self.suffix.get(entry).copied().unwrap_or(0);
            }
            entry = usize::from(self.prefix.get(entry).copied().unwrap_or(0));
        }
        stop.min(out.len())
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;

    /// `codes`, each at its width, packed least significant bit first and
    /// wrapped in sub-blocks of `chunk` bytes.
    fn pack(codes: &[(usize, u32)], chunk: usize) -> Vec<u8> {
        let (mut acc, mut count, mut bytes) = (0u64, 0u32, Vec::new());
        for &(code, width) in codes {
            acc |= (code as u64) << count;
            count += width;
            while count >= 8 {
                bytes.push(acc as u8);
                acc >>= 8;
                count -= 8;
            }
        }
        if count > 0 {
            bytes.push(acc as u8);
        }
        let mut out = Vec::new();
        for piece in bytes.chunks(chunk) {
            out.push(piece.len() as u8);
            out.extend_from_slice(piece);
        }
        out.push(0);
        out
    }

    /// A clear, `values` as single-index codes, and the end code, each at the
    /// width a decoder reads it at: every code after the first adds an entry,
    /// and the width grows once the next free code no longer fits.
    fn literals(min: u32, values: &[usize]) -> Vec<(usize, u32)> {
        let clear = 1usize << min;
        let end = clear + 1;
        let mut width = min + 1;
        let mut next = end + 1;
        let mut codes = vec![(clear, width)];
        for (i, &value) in values.iter().enumerate() {
            codes.push((value, width));
            if i > 0 && next < 4096 {
                next += 1;
            }
            if next >= (1 << width) && width < 12 {
                width += 1;
            }
        }
        codes.push((end, width));
        codes
    }

    fn decode(min: u8, data: &[u8], pixels: usize) -> (Vec<u8>, usize) {
        let mut out = vec![0xEE; pixels];
        let mut blocks = SubBlocks::new(data, 0);
        let written = Table::new().decode(min, &mut blocks, &mut out);
        (out[..written].to_vec(), blocks.finish())
    }

    #[test]
    fn a_code_not_yet_in_the_table_is_its_predecessor_plus_its_first_index() {
        // Minimum code size 2: clear 4, end 5, first free code 6. "1, 6"
        // sends index 1 and then the entry being defined by that very step:
        // 1 + first(1) = "1 1".
        let data = pack(&[(4, 3), (1, 3), (6, 3), (5, 3)], 255);
        let (got, _) = decode(2, &data, 3);
        assert_eq!(got, vec![1, 1, 1]);
    }

    #[test]
    fn the_width_grows_when_the_next_free_code_no_longer_fits() {
        // Minimum code size 2: codes start at 3 bits. Entries 6 and 7 are
        // added after the second and third codes, and after 7 the next free
        // code, 8, needs 4 bits -- so the fourth code is read at 4.
        let data = pack(&[(4, 3), (0, 3), (1, 3), (2, 3), (3, 4), (5, 4)], 255);
        let (got, _) = decode(2, &data, 4);
        assert_eq!(got, vec![0, 1, 2, 3]);
    }

    #[test]
    fn a_full_table_keeps_decoding_twelve_bit_codes_without_a_clear() {
        // Fill all 4096 entries with a run of zeros -- each code the entry
        // before it, one index longer -- then send literal 1 twice at 12 bits
        // without the clear a less patient encoder would have sent.
        let mut codes = vec![(256usize, 9u32), (0, 9)];
        let mut next = 258usize;
        let mut width = 9u32;
        let mut expected = vec![0u8];
        let mut previous = 0usize;
        while next < 4096 {
            // KwKwK every time: the code being defined.
            codes.push((next, width));
            let length = if previous == 0 { 2 } else { previous - 256 + 1 };
            expected.extend(core::iter::repeat_n(0u8, length));
            previous = next;
            next += 1;
            if next >= (1 << width) && width < 12 {
                width += 1;
            }
        }
        codes.push((1, 12));
        codes.push((1, 12));
        codes.push((257, 12));
        expected.extend([1, 1]);
        let data = pack(&codes, 255);
        let (got, _) = decode(8, &data, expected.len());
        assert_eq!(got.len(), expected.len());
        assert!(got == expected, "the tail after the full table differs");
    }

    #[test]
    fn a_clear_mid_stream_starts_the_dictionary_again() {
        let data = pack(
            &[(4, 3), (1, 3), (2, 3), (4, 3), (6, 3), (3, 3), (5, 3)],
            255,
        );
        // After the second clear, 6 is not an entry yet: the first code after a
        // clear must be a single index, so the image ends there.
        let (got, _) = decode(2, &data, 8);
        assert_eq!(got, vec![1, 2]);
        // Entry 7 is added on reading 6, which makes the next free code 8
        // and so the end code 4 bits wide.
        let data = pack(
            &[
                (4, 3),
                (1, 3),
                (2, 3),
                (4, 3),
                (3, 3),
                (3, 3),
                (6, 3),
                (5, 4),
            ],
            255,
        );
        let (got, _) = decode(2, &data, 8);
        assert_eq!(got, vec![1, 2, 3, 3, 3, 3]);
    }

    #[test]
    fn a_code_past_the_next_free_one_ends_the_image_where_it_is() {
        let data = pack(&[(4, 3), (1, 3), (7, 3), (2, 3), (5, 3)], 255);
        let (got, _) = decode(2, &data, 8);
        assert_eq!(got, vec![1]);
    }

    #[test]
    fn codes_are_read_across_sub_block_boundaries() {
        let values: Vec<usize> = (0..40).map(|i| i % 4).collect();
        let codes = literals(2, &values);
        // One byte per sub-block: every code straddles a boundary somewhere.
        let data = pack(&codes, 1);
        let (got, next) = decode(2, &data, 400);
        assert_eq!(got.len(), 40);
        assert_eq!(got, (0..40).map(|i| (i % 4) as u8).collect::<Vec<_>>());
        assert_eq!(next, data.len(), "the terminator was stepped past");
    }

    #[test]
    fn decoding_stops_at_the_last_pixel_and_the_rest_is_stepped_past() {
        let data = pack(&[(4, 3), (1, 3), (1, 3), (1, 3), (1, 3), (5, 3)], 2);
        let (got, next) = decode(2, &data, 2);
        assert_eq!(got, vec![1, 1]);
        assert_eq!(next, data.len());
    }

    #[test]
    fn a_string_that_overruns_the_image_is_cut_at_its_edge() {
        // "1 1 1" is written into a two-pixel image: the first two only.
        let data = pack(&[(4, 3), (1, 3), (6, 3), (5, 3)], 255);
        let (got, _) = decode(2, &data, 2);
        assert_eq!(got, vec![1, 1]);
    }

    #[test]
    fn data_that_stops_short_gives_what_there_was() {
        let mut data = pack(&[(4, 3), (1, 3), (2, 3), (3, 3), (0, 3), (5, 3)], 255);
        // Cut after the first data byte, terminator and all.
        data.truncate(2);
        let (got, next) = decode(2, &data, 8);
        assert!(got.len() < 4, "{got:?}");
        assert_eq!(next, data.len());
        // And nothing at all: no sub-blocks, not even the terminator.
        let (got, next) = decode(2, &[], 8);
        assert!(got.is_empty());
        assert_eq!(next, 0);
    }

    #[test]
    fn no_stream_of_codes_panics() {
        // Every byte value as a one-byte stream at every minimum code size,
        // and a spread of longer random ones.
        for min in 2..=8u8 {
            for b in 0..=255u8 {
                let _ = decode(min, &[1, b, 0], 16);
            }
        }
        let mut seed = 0x1234_5678u32;
        for _ in 0..2000 {
            let len = (seed % 64) as usize;
            let mut data = Vec::new();
            for _ in 0..len {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                data.push(seed as u8);
            }
            let min = (seed % 7) as u8 + 2;
            let _ = decode(min, &data, 300);
        }
    }
}
