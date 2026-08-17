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
//! # The other thing that can occupy the slot
//!
//! `deltaFormat` `0x8000` marks a `VariationIndex` rather than a device table:
//! the same slot and the same six-byte header, but the first four bytes are
//! indices into a variable font's
//! `ItemVariationStore` instead of a pixel range, and the correction they name
//! depends on the *instance* rather than on the size. A `VariationIndex` read
//! *as* a device table is not a wrong correction but an arbitrary one, since
//! the indices would be interpreted as a start and end ppem that happen to
//! bracket the current size — so the format is checked before the range.
//!
//! The two are read by two types, because they need different things and one
//! caller has only the first: [`Ppem`] is the pixel size alone and answers
//! device tables, [`Corrections`] adds the instance and answers both. A caller
//! that knows a size but not an instance is not making an error, so
//! `Corrections` is constructible from a bare `Ppem`.
//!
//! Note that the two corrections are *not* in the same units. A device table's
//! delta is in **pixels** and must be converted back through the em; a
//! `VariationIndex`'s is already in **font units**, so it is applied as-is —
//! and consequently applies at [`Ppem::NONE`] too, where a device table
//! correctly contributes nothing. That asymmetry is the specification's, and
//! matches HarfBuzz, whose `VariationDevice::get_x_delta` scales the store's
//! value by the font scale where `DeviceTable::get_x_delta` scales it by
//! `x_ppem`.

use crate::sfnt::u16_at;
use crate::varstore::VarStore;

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

/// Everything needed to read whichever of the two tables occupies a `Device`
/// slot: the pixel size a device table is indexed by, and the instance a
/// `VariationIndex` is evaluated at.
///
/// Carries borrows, so it is built at the point a run is positioned and passed
/// down by value rather than stored. [`NONE`](Self::NONE) — no size, default
/// instance — is what an unscaled [`Face`](crate::sfnt::Face) asks with, and
/// reads every correction as zero.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Corrections<'a> {
    /// The size, for the device-table half. May be [`Ppem::NONE`]; a
    /// `VariationIndex` is still read then, because its delta does not depend
    /// on the size.
    ppem: Ppem,
    /// `GDEF`'s `ItemVariationStore`, if the face has one *and* the caller is
    /// asking at a non-default instance. `None` means every `VariationIndex`
    /// reads as no correction, which is the right answer at the default
    /// instance — where all the deltas are zero anyway — and the only possible
    /// one for a face with no store.
    store: Option<&'a VarStore>,
    /// The normalized F2Dot14 coordinates, one per `fvar` axis. Empty when
    /// [`store`](Self::store) is `None`.
    coords: &'a [i16],
}

impl<'a> Corrections<'a> {
    /// No size and no instance: every correction reads as zero.
    pub(crate) const NONE: Self = Self {
        ppem: Ppem::NONE,
        store: None,
        coords: &[],
    };

    /// A size, at the default instance.
    ///
    /// What a caller with a scaled font but no variation axes asks with — and
    /// deliberately *not* an error, since a non-variable face drawn at 11px is
    /// exactly this case.
    pub(crate) const fn at(ppem: Ppem) -> Self {
        Self {
            ppem,
            store: None,
            coords: &[],
        }
    }

    /// A size *and* an instance.
    ///
    /// `store` is `None` for a face that has no `GDEF` store; `coords` is the
    /// normalized vector, which the caller is expected to have already
    /// suppressed to empty at the default instance so that the common case
    /// does no work.
    pub(crate) const fn varying(
        ppem: Ppem,
        store: Option<&'a VarStore>,
        coords: &'a [i16],
    ) -> Self {
        Self {
            ppem,
            store,
            coords,
        }
    }

    /// The correction the table at `from + offset` makes, in font units,
    /// whichever of the two kinds it turns out to be.
    ///
    /// Same contract as [`Ppem::delta`]: `0` is the NULL offset, `from` is
    /// whatever the format measures the offset from, and it is infallible
    /// because a malformed correction is worth less than the placement it
    /// would otherwise abort.
    pub(crate) fn delta(self, data: &[u8], from: usize, offset: u16) -> i16 {
        if offset == 0 {
            return 0;
        }
        let Some(at) = from.checked_add(usize::from(offset)) else {
            return 0;
        };
        // The format word decides which reader applies, and must be consulted
        // before either one touches the first four bytes: those are a ppem
        // range in one table and a pair of store indices in the other.
        let format = at.checked_add(4).and_then(|o| u16_at(data, o));
        if format == Some(VARIATION_INDEX) {
            return self.variation_delta(data, at);
        }
        self.ppem.delta(data, from, offset)
    }

    /// The `ItemVariationStore` delta the `VariationIndex` at `at` names.
    ///
    /// Already in font units — no `to_font_units` — which is why this does not
    /// consult the ppem at all and answers just as well at [`Ppem::NONE`].
    fn variation_delta(self, data: &[u8], at: usize) -> i16 {
        let Some(store) = self.store else { return 0 };
        let (Some(outer), Some(inner)) = (
            u16_at(data, at),
            at.checked_add(2).and_then(|o| u16_at(data, o)),
        ) else {
            return 0;
        };
        store
            .delta(data, outer, inner, self.coords)
            .map_or(0, crate::varstore::round_to_i16)
    }
}

/// The pixel correction one device table makes at `ppem`, or `None` when it
/// makes none.
///
/// `None` covers four different situations that all mean the same thing to the
/// caller: the table is a `VariationIndex`, which is not a device table and is
/// [`Corrections`]' business rather than this one's; its `deltaFormat` is one
/// the spec does not define; `ppem` is outside the range it covers; or the
/// delta array is truncated.
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

    // --- the other table that can occupy the slot ---

    /// `F2Dot14` for 1.0 and 0.5 — the axis positions the fixture store's one
    /// region is defined against.
    const ONE: i16 = 16384;
    const HALF: i16 = 8192;

    /// A `VariationIndex` naming `(outer, inner)`, as the six bytes a font
    /// hangs off a value record's field.
    fn variation_index(outer: u16, inner: u16) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend(outer.to_be_bytes());
        t.extend(inner.to_be_bytes());
        t.extend(VARIATION_INDEX.to_be_bytes());
        t
    }

    /// A buffer with `table` at offset 4 and a one-axis store after it, plus
    /// the parsed store.
    ///
    /// Laid out this way so the store is at a different offset from the table
    /// that indexes into it — a reader that confused the two would still pass
    /// if both lived at zero.
    fn with_store(table: &[u8], rows: &[i32]) -> (Vec<u8>, VarStore) {
        let store_at = 4 + table.len();
        let mut data = alloc::vec![0u8; 4];
        data.extend_from_slice(table);
        data.extend_from_slice(&crate::varstore::one_axis_store(rows));
        let store = VarStore::parse(&data, store_at, 1).expect("the fixture store parses");
        (data, store)
    }

    #[test]
    fn a_variation_index_is_read_through_the_store() {
        let (data, store) = with_store(&variation_index(0, 1), &[7, 40]);
        // The fixture's one region peaks at the far end of its one axis, so
        // the delta is the stored row scaled by where the caller sits on it.
        for (coord, want) in [(0i16, 0i16), (HALF, 20), (ONE, 40)] {
            let coords = [coord];
            let c = Corrections::varying(Ppem::NONE, Some(&store), &coords);
            assert_eq!(c.delta(&data, 0, 4), want, "at coord {coord}");
        }
        // And the inner index really selects the row, rather than every index
        // landing on the first one.
        let c = Corrections::varying(Ppem::NONE, Some(&store), &[ONE]);
        assert_eq!(c.delta(&data, 0, 4), 40);
        let (data, store) = with_store(&variation_index(0, 0), &[7, 40]);
        let c = Corrections::varying(Ppem::NONE, Some(&store), &[ONE]);
        assert_eq!(c.delta(&data, 0, 4), 7);
    }

    #[test]
    fn a_variation_index_is_in_font_units_rather_than_pixels() {
        // The whole difference between the two tables. A device table's 40
        // would be 40 *pixels*, i.e. 40 * upem / ppem font units — nearly 4000
        // at 11px on a 1000-unit em. A VariationIndex's 40 is 40 font units,
        // at every size and at no size at all.
        let (data, store) = with_store(&variation_index(0, 1), &[7, 40]);
        for ppem in [Ppem::NONE, Ppem::new(11.0, 1000), Ppem::new(96.0, 2048)] {
            let c = Corrections::varying(ppem, Some(&store), &[ONE]);
            assert_eq!(c.delta(&data, 0, 4), 40, "at {ppem:?}");
        }
    }

    #[test]
    fn a_variation_index_rounds_the_way_the_other_stores_round() {
        // 41 at half the axis is 20.5, which goes to 21 — away from zero, not
        // to the even neighbour. Shared with HVAR and MVAR so that three
        // tables reading one store cannot disagree about a half unit.
        let (data, store) = with_store(&variation_index(0, 0), &[41]);
        let c = Corrections::varying(Ppem::NONE, Some(&store), &[HALF]);
        assert_eq!(c.delta(&data, 0, 4), 21);
        let (data, store) = with_store(&variation_index(0, 0), &[-41]);
        let c = Corrections::varying(Ppem::NONE, Some(&store), &[HALF]);
        assert_eq!(c.delta(&data, 0, 4), -21);
    }

    #[test]
    fn a_variation_index_without_a_store_is_no_correction() {
        // What a non-variable face drawn at 11px asks with, and what a
        // variable one at its default instance asks with. Not an error: a
        // VariationIndex in a face we hold no store for has no value to give.
        let (data, _store) = with_store(&variation_index(0, 1), &[7, 40]);
        assert_eq!(Corrections::at(Ppem::new(11.0, 1000)).delta(&data, 0, 4), 0);
        assert_eq!(Corrections::NONE.delta(&data, 0, 4), 0);
        assert_eq!(Corrections::default().delta(&data, 0, 4), 0);
    }

    #[test]
    fn a_pair_naming_no_row_is_no_correction() {
        let (data, store) = with_store(&variation_index(0, 9), &[7, 40]);
        let c = Corrections::varying(Ppem::NONE, Some(&store), &[ONE]);
        assert_eq!(c.delta(&data, 0, 4), 0);
        let (data, store) = with_store(&variation_index(3, 0), &[7, 40]);
        let c = Corrections::varying(Ppem::NONE, Some(&store), &[ONE]);
        assert_eq!(c.delta(&data, 0, 4), 0);
    }

    #[test]
    fn a_device_table_still_reads_when_a_store_is_present() {
        // The two arms must not cross: holding a store does not turn a device
        // table into a variation index, and the pixel conversion still applies
        // to it. The device table's first four bytes (9, 12) would name a
        // perfectly plausible (outer, inner) pair if they were misread.
        let (data, store) = with_store(&device(9, 12, 1, &[1, 0, -1, -2]), &[7, 40]);
        let c = Corrections::varying(Ppem::new(9.0, 1000), Some(&store), &[ONE]);
        assert_eq!(c.delta(&data, 0, 4), units(1, 1000, 9));
        // And a size outside the table's range is still uncorrected, rather
        // than falling through to the store.
        let c = Corrections::varying(Ppem::new(20.0, 1000), Some(&store), &[ONE]);
        assert_eq!(c.delta(&data, 0, 4), 0);
    }

    #[test]
    fn a_null_offset_is_no_table_of_either_kind() {
        let (data, store) = with_store(&variation_index(0, 1), &[7, 40]);
        let c = Corrections::varying(Ppem::new(11.0, 1000), Some(&store), &[ONE]);
        assert_eq!(c.delta(&data, 0, 0), 0);
    }

    #[test]
    fn a_truncated_variation_index_declines_rather_than_reading_past() {
        let (full, store) = with_store(&variation_index(0, 1), &[7, 40]);
        // Cut so the format word is gone: what is left is not a table at all,
        // and must not be read as the device table its first bytes resemble.
        let cut = &full[..6];
        let c = Corrections::varying(Ppem::new(11.0, 1000), Some(&store), &[ONE]);
        assert_eq!(c.delta(cut, 0, 4), 0);
    }
}
