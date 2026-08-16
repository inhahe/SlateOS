//! Device tables: the corrections a face makes to a position at one pixel size.
//!
//! Everywhere `GPOS` states a coordinate — a value record's four fields, an
//! anchor's x and y — it may also name a `Device` table, which says "and at 11
//! pixels per em, move that half a pixel further left". They exist because a
//! design-unit coordinate scaled to a small size lands between pixels, and the
//! designer would rather say where it should land than accept the rounding.
//!
//! The correction is therefore stated *in pixels*, while everything it corrects
//! is in font units. A delta is only meaningful once a size is known, which is
//! why the reader is a [`Ppem`] value threaded down from the scaled font rather
//! than a free function: a [`Face`](crate::sfnt::Face) has no size, so it has no
//! device corrections either, and [`Ppem::NONE`] is the honest answer for it.
//!
//! # Why this is not simply ignored
//!
//! It was, until now — on the argument that the uncorrected value is the
//! designer's intent at every size. That argument is backwards. The uncorrected
//! value is the designer's intent at *large* sizes, where a fraction of a pixel
//! is invisible; the device table is the intent at the small ones, where it is
//! the difference between an accent sitting on a letter and sitting in it. A
//! face that ships one has explicitly said the scaled value is wrong there.
//!
//! # What is not here
//!
//! `deltaFormat` `0x8000` marks a `VariationIndex` rather than a device table:
//! the same eight bytes, but the first four are indices into a variable font's
//! `ItemVariationStore` instead of a pixel range. This crate does not read
//! variation stores, and — this is the point — a `VariationIndex` read *as* a
//! device table is not a wrong correction but an arbitrary one, since the
//! indices would be interpreted as a start and end ppem that happen to bracket
//! the current size. [`Ppem::delta`] checks the format before the range for
//! exactly that reason, and returns no correction. See
//! `TD-FONT-DOES-NOT-READ-VARIATION-STORES`.

use crate::sfnt::u16_at;

/// `deltaFormat` value marking a `VariationIndex` table. Not a bit count.
const VARIATION_INDEX: u16 = 0x8000;

/// The pixel size device corrections are being read at, and the em they must be
/// expressed back into.
///
/// [`NONE`](Self::NONE) — which is also [`Default`] — is "no size is known", at
/// which every device table reads as no correction. That is not a fallback but
/// the correct answer for a caller that has no size: an unscaled face's kerning
/// is its design-unit kerning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Ppem {
    /// Pixels per em, rounded to the integer the table is indexed by. Zero
    /// means no size is known.
    px: u16,
    /// The face's `unitsPerEm`, which turns a pixel delta back into the font
    /// units the rest of the positioning pass works in.
    upem: u16,
}

impl Ppem {
    /// No size, and so no correction. What a [`Face`](crate::sfnt::Face) asks
    /// with.
    pub(crate) const NONE: Self = Self { px: 0, upem: 0 };

    /// The size a scaled font draws at.
    ///
    /// Device tables are indexed by an integer ppem, so a fractional size is
    /// rounded to the nearest — which is what a rasterizer does with the size
    /// anyway. A size that rounds to zero, or a face with no em, yields
    /// [`NONE`](Self::NONE): there is no row of the table to read.
    pub(crate) fn new(px_per_em: f32, upem: u16) -> Self {
        if !px_per_em.is_finite() || upem == 0 {
            return Self::NONE;
        }
        let rounded = px_per_em.round();
        if !(1.0..=f32::from(u16::MAX)).contains(&rounded) {
            return Self::NONE;
        }
        // In range and finite by the check above, so the cast is exact.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "range-checked immediately above"
        )]
        Self {
            px: rounded as u16,
            upem,
        }
    }

    /// The correction the device table at `from + offset` makes, in font units.
    ///
    /// `offset` is the field as it appears in the font, so `0` is the NULL
    /// offset every optional device field uses to say it has no table. `from`
    /// is whatever the format says the offset is measured from — the subtable
    /// for a value record's, the anchor itself for an anchor's.
    ///
    /// Infallible by design: a malformed device table means a position that is
    /// a fraction of a pixel off, and refusing the whole placement over it
    /// would be a far larger error than the one being avoided.
    pub(crate) fn delta(self, data: &[u8], from: usize, offset: u16) -> i16 {
        if offset == 0 || self.px == 0 || self.upem == 0 {
            return 0;
        }
        let Some(at) = from.checked_add(usize::from(offset)) else {
            return 0;
        };
        let Some(pixels) = pixel_delta(data, at, self.px) else {
            return 0;
        };
        self.to_font_units(pixels)
    }

    /// Convert a pixel correction into font units at this size.
    ///
    /// `pixels * upem / ppem`, truncating — HarfBuzz's `Device::get_delta`
    /// scaled by the em rather than by its own 26.6 font scale, which is the
    /// same arithmetic in the units this crate keeps positions in. Truncation
    /// rather than rounding is deliberate: it is what HarfBuzz does, and a
    /// sweep that disagrees with the reference by one font unit on every
    /// corrected glyph would drown out the disagreements worth looking at.
    fn to_font_units(self, pixels: i32) -> i16 {
        let scaled = pixels
            .checked_mul(i32::from(self.upem))
            .and_then(|n| n.checked_div(i32::from(self.px)));
        // Saturating rather than wrapping: a correction larger than an i16 is a
        // broken font, and clamping keeps the glyph on the page.
        match scaled {
            Some(n) => i16::try_from(n).unwrap_or(if n < 0 { i16::MIN } else { i16::MAX }),
            None => 0,
        }
    }
}

/// The pixel correction one device table makes at `ppem`, or `None` when it
/// makes none.
///
/// `None` covers four different situations that all mean the same thing to the
/// caller: the table is a `VariationIndex` this cannot read, its `deltaFormat`
/// is one the spec does not define, `ppem` is outside the range it covers, or
/// the delta array is truncated.
fn pixel_delta(data: &[u8], at: usize, ppem: u16) -> Option<i32> {
    // Format first, and only then the range: in a `VariationIndex` the two
    // fields read here as a start and end size are variation-store indices, so
    // range-checking them first would be checking against a number that means
    // something else entirely.
    let format = u16_at(data, at.checked_add(4)?)?;
    if format == VARIATION_INDEX || !(1..=3).contains(&format) {
        return None;
    }
    let start = u16_at(data, at)?;
    let end = u16_at(data, at.checked_add(2)?)?;
    if ppem < start || ppem > end {
        return None;
    }

    // `deltaFormat` is the log2 of the bits per delta: 1 → 2 bits, 2 → 4,
    // 3 → 8. Checked to be 1..=3 above, so the shift cannot overflow.
    let bits = 1u32.checked_shl(u32::from(format))?;
    let per_word = 16u32.checked_div(bits)?;
    let index = u32::from(ppem.checked_sub(start)?);

    let word_at = at.checked_add(6)?.checked_add(
        usize::try_from(index.checked_div(per_word)?)
            .ok()?
            .checked_mul(2)?,
    )?;
    let word = u16_at(data, word_at)?;

    // Deltas are packed most-significant-first within each 16-bit word.
    let slot = index.checked_rem(per_word)?;
    let shift = 16u32.checked_sub(bits.checked_mul(slot.checked_add(1)?)?)?;
    let mask = 1u32.checked_shl(bits)?.checked_sub(1)?;
    let raw = (u32::from(word).checked_shr(shift)?) & mask;

    // Sign-extend from `bits` to the full width: the deltas are signed, so the
    // top bit of the packed field is the sign.
    let sign_bit = 1u32.checked_shl(bits.checked_sub(1)?)?;
    let raw = i64::from(raw);
    let signed = if raw >= i64::from(sign_bit) {
        raw.checked_sub(i64::from(mask).checked_add(1)?)?
    } else {
        raw
    };
    i32::try_from(signed).ok()
}

/// Build a `Device` table covering `start..=end`, one delta per size, packed at
/// `format` bits per delta (`1` → 2 bits, `2` → 4, `3` → 8).
///
/// Here rather than in each test module that needs one because a value record's
/// device table, an anchor's and this module's own are the same eight-plus bytes
/// — and three copies of a bit-packer that only tests use is three chances to
/// pack it the way a wrong reader would read it.
#[cfg(test)]
#[allow(clippy::arithmetic_side_effects, reason = "test fixture builder")]
pub(crate) fn table(start: u16, end: u16, format: u16, deltas: &[i32]) -> alloc::vec::Vec<u8> {
    let bits = 1u32 << format;
    let per_word = 16 / bits;
    let mut t = alloc::vec::Vec::new();
    t.extend(start.to_be_bytes());
    t.extend(end.to_be_bytes());
    t.extend(format.to_be_bytes());
    let words = deltas.len().div_ceil(per_word as usize);
    for w in 0..words {
        let mut word: u16 = 0;
        for slot in 0..per_word {
            let i = w * per_word as usize + slot as usize;
            let Some(&d) = deltas.get(i) else { break };
            let mask = (1u32 << bits) - 1;
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let packed = (d as u32) & mask;
            let shift = 16 - bits * (slot + 1);
            #[allow(clippy::cast_possible_truncation)]
            {
                word |= (packed << shift) as u16;
            }
        }
        t.extend(word.to_be_bytes());
    }
    t
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    use super::table as device;

    /// The table placed at offset 4 of a buffer, so that a test exercises the
    /// `from + offset` arithmetic rather than always reading from zero.
    fn at_four(table: &[u8]) -> Vec<u8> {
        let mut d = alloc::vec![0u8; 4];
        d.extend_from_slice(table);
        d
    }

    /// The same bytes with the `deltaFormat` field overwritten.
    ///
    /// Built as a readable table first and then relabelled, so that the delta
    /// array really is there to be misread — a test that also truncated the
    /// array would pass for the wrong reason.
    fn relabelled(table: &[u8], format: u16) -> Vec<u8> {
        let mut t = table.to_vec();
        t[4..6].copy_from_slice(&format.to_be_bytes());
        t
    }

    /// The font-unit answer a `pixels` correction should come back as, written
    /// as the formula rather than as a number so that a test states the rule.
    fn units(pixels: i32, upem: i32, ppem: i32) -> i16 {
        i16::try_from(pixels * upem / ppem).unwrap()
    }

    #[test]
    fn a_two_bit_delta_is_read_and_sign_extended() {
        // Format 1 holds -2..=1, which is the whole point of two bits: a
        // correction that only ever nudges by a pixel either way.
        let t = at_four(&device(9, 12, 1, &[1, 0, -1, -2]));
        assert_eq!(Ppem::new(9.0, 1000).delta(&t, 0, 4), units(1, 1000, 9));
        assert_eq!(Ppem::new(10.0, 1000).delta(&t, 0, 4), 0);
        assert_eq!(Ppem::new(11.0, 1000).delta(&t, 0, 4), units(-1, 1000, 11));
        assert_eq!(Ppem::new(12.0, 1000).delta(&t, 0, 4), units(-2, 1000, 12));
    }

    #[test]
    fn a_four_bit_delta_spans_two_words() {
        // Five sizes at four bits each: four in the first word, one in the
        // second — the case an off-by-one in the word index gets wrong.
        let t = at_four(&device(8, 12, 2, &[7, -8, 3, -1, 5]));
        assert_eq!(Ppem::new(8.0, 2048).delta(&t, 0, 4), units(7, 2048, 8));
        assert_eq!(Ppem::new(9.0, 2048).delta(&t, 0, 4), units(-8, 2048, 9));
        assert_eq!(Ppem::new(12.0, 2048).delta(&t, 0, 4), units(5, 2048, 12));
    }

    #[test]
    fn an_eight_bit_delta_reaches_the_format_s_extremes() {
        let t = at_four(&device(16, 18, 3, &[127, -128, 0]));
        assert_eq!(Ppem::new(16.0, 1000).delta(&t, 0, 4), units(127, 1000, 16));
        assert_eq!(Ppem::new(17.0, 1000).delta(&t, 0, 4), units(-128, 1000, 17));
        assert_eq!(Ppem::new(18.0, 1000).delta(&t, 0, 4), 0);
    }

    #[test]
    fn a_size_outside_the_table_s_range_is_uncorrected() {
        let t = at_four(&device(9, 12, 1, &[1, 1, 1, 1]));
        assert_eq!(Ppem::new(8.0, 1000).delta(&t, 0, 4), 0);
        assert_eq!(Ppem::new(13.0, 1000).delta(&t, 0, 4), 0);
    }

    #[test]
    fn a_variation_index_is_not_read_as_a_device_table() {
        // A VariationIndex whose two indices, read as a device table's start and
        // end size, do bracket 12 — and which is followed by bytes that decode
        // as a perfectly good delta. Everything about it invites a misread
        // except the format word, which is the only thing that says no.
        let readable = device(9, 20, 1, &[1; 12]);
        assert_ne!(Ppem::new(12.0, 1000).delta(&at_four(&readable), 0, 4), 0);
        let t = at_four(&relabelled(&readable, VARIATION_INDEX));
        assert_eq!(Ppem::new(12.0, 1000).delta(&t, 0, 4), 0);
    }

    #[test]
    fn an_undefined_delta_format_is_declined() {
        let readable = device(9, 12, 1, &[1, 1, 1, 1]);
        for format in [0u16, 4, 5, 0x7fff, 0x8001] {
            let t = at_four(&relabelled(&readable, format));
            assert_eq!(Ppem::new(10.0, 1000).delta(&t, 0, 4), 0, "format {format}");
        }
    }

    #[test]
    fn a_null_offset_is_no_table() {
        let t = at_four(&device(9, 12, 1, &[1, 1, 1, 1]));
        assert_eq!(Ppem::new(10.0, 1000).delta(&t, 0, 0), 0);
    }

    #[test]
    fn a_truncated_delta_array_declines_rather_than_reading_past() {
        let full = at_four(&device(9, 24, 3, &[1; 16]));
        let cut = &full[..full.len() - 4];
        // The sizes whose word survives still read; the ones past the cut do not.
        assert_eq!(Ppem::new(9.0, 1000).delta(cut, 0, 4), units(1, 1000, 9));
        assert_eq!(Ppem::new(24.0, 1000).delta(cut, 0, 4), 0);
    }

    #[test]
    fn no_size_means_no_correction() {
        let t = at_four(&device(9, 12, 1, &[1, 1, 1, 1]));
        assert_eq!(Ppem::NONE.delta(&t, 0, 4), 0);
        assert_eq!(Ppem::default(), Ppem::NONE);
        // A face with no em, and a nonsense size, are the same answer.
        assert_eq!(Ppem::new(10.0, 0), Ppem::NONE);
        assert_eq!(Ppem::new(f32::NAN, 1000), Ppem::NONE);
        assert_eq!(Ppem::new(0.4, 1000), Ppem::NONE);
        assert_eq!(Ppem::new(-12.0, 1000), Ppem::NONE);
    }

    #[test]
    fn a_fractional_size_reads_the_row_it_rounds_to() {
        // 11.6px draws as 12 pixels, so it must read row 12 — not row 11, and
        // not no row at all.
        let t = at_four(&device(11, 12, 3, &[10, 20]));
        assert_eq!(Ppem::new(11.6, 1000).delta(&t, 0, 4), units(20, 1000, 12));
        assert_eq!(Ppem::new(11.4, 1000).delta(&t, 0, 4), units(10, 1000, 11));
    }
}
