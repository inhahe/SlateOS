//! The coefficient buffer of a multi-scan image, and block smoothing:
//! libjpeg-turbo's `jdcoefct.c`.
//!
//! A progressive or multi-scan file sends each block's coefficients in parts,
//! so they are gathered for the whole image before anything is reconstructed.
//! libjpeg keeps all 64 of every block. A reduced-size decode reads only some
//! of them -- the 4x4 transform never looks at row or column 4, the 2x2 only at
//! rows and columns 0, 1, 3, 5 and 7, the 1x1 at the DC alone -- so [`Store`]
//! keeps the values of those, and for the rest only whether each is zero,
//! which is all the decoders ever ask of a coefficient they do not produce:
//! a refinement scan reads a correction bit for exactly the coefficients that
//! are already nonzero. A thumbnail of a large progressive photograph so
//! costs a fraction of the whole picture's coefficients, and decodes to what
//! libjpeg decodes it to.
//!
//! Block smoothing (`decompress_smooth_data`) is libjpeg's interblock
//! smoothing for progressive images whose low-frequency coefficients are not
//! yet known to full precision -- which, when every scan has arrived, is only
//! a file cut off or damaged. It estimates the missing coefficients of each
//! block from the DC values of the 5x5 blocks around it, and is transcribed
//! with its sliding window of DC registers and its edge rules intact.

use alloc::vec;
use alloc::vec::Vec;

use super::idct::{self, Target};

/// What a nonzero coefficient that is not kept reads as. Nonzero, positive,
/// and with no bit below 14 set, so a refinement adding or subtracting a
/// power of two up to 2^13 never makes it zero -- which is all that is ever
/// asked of it.
const PLACEHOLDER: i16 = 0x4000;

/// A block's coefficients as the entropy decoders read and write them, by
/// natural-order position: a block of its own (`[i16; 64]`), or one of a
/// [`Store`]'s in place ([`StoredBlock`]).
pub(super) trait Coefficients {
    /// The coefficient at `position`.
    fn at(&self, position: usize) -> i16;
    /// Make the coefficient at `position` `value`.
    fn set(&mut self, position: usize, value: i16);
}

impl Coefficients for [i16; 64] {
    fn at(&self, position: usize) -> i16 {
        self.get(position).copied().unwrap_or(0)
    }

    fn set(&mut self, position: usize, value: i16) {
        if let Some(cell) = self.get_mut(position) {
            *cell = value;
        }
    }
}

/// One block of a [`Store`], read and written where it lies -- as libjpeg's
/// decoders work on its coefficient array through pointers into it. A
/// progressive AC pass decodes into one of these, so a coefficient costs
/// something only when the scan decodes it: copying each block out and
/// back, as this port first did, cost a thumbnail of a large progressive
/// photograph more than its decoding.
pub(super) struct StoredBlock<'s> {
    store: &'s mut Store,
    /// The block's index, or `None` for one outside the store, which reads
    /// as zeros and keeps nothing.
    index: Option<usize>,
}

impl Coefficients for StoredBlock<'_> {
    /// The kept value, or for a coefficient that is not kept, the placeholder
    /// if it is nonzero -- what [`Store::load`] gives.
    fn at(&self, position: usize) -> i16 {
        let Some(index) = self.index else {
            return 0;
        };
        let store = &*self.store;
        match store.slot.get(position).copied() {
            Some(slot) if slot != u8::MAX => store
                .values
                .get(
                    index
                        .saturating_mul(store.kept)
                        .saturating_add(usize::from(slot)),
                )
                .copied()
                .unwrap_or(0),
            Some(_) => {
                let mask = store.masks.get(index).copied().unwrap_or(0);
                if (mask >> position) & 1 != 0 {
                    PLACEHOLDER
                } else {
                    0
                }
            }
            None => 0,
        }
    }

    fn set(&mut self, position: usize, value: i16) {
        let Some(index) = self.index else {
            return;
        };
        if position >= 64 {
            return;
        }
        let store = &mut *self.store;
        if let Some(mask) = store.masks.get_mut(index) {
            if value == 0 {
                *mask &= !(1u64 << position);
            } else {
                *mask |= 1u64 << position;
            }
        }
        if let Some(&slot) = store.slot.get(position) {
            if slot != u8::MAX {
                let at = index
                    .saturating_mul(store.kept)
                    .saturating_add(usize::from(slot));
                if let Some(cell) = store.values.get_mut(at) {
                    *cell = value;
                }
            }
        }
    }
}

/// A component's coefficients for the whole image.
#[derive(Debug, Clone)]
pub(super) struct Store {
    /// Blocks across and down, padded to whole MCUs as libjpeg's arrays are.
    pub(super) blocks_w: usize,
    pub(super) blocks_h: usize,
    /// For each natural-order position, its index among the kept values, or
    /// `u8::MAX`.
    slot: [u8; 64],
    kept: usize,
    /// Which coefficients of each block are nonzero -- kept only when some
    /// coefficient's value is not (a reduced size), the one case anything
    /// reads them: at full size every value is there to ask.
    masks: Vec<u64>,
    values: Vec<i16>,
}

impl Store {
    /// The positions a block reconstructed at `size` keeps.
    fn slots(size: usize) -> ([u8; 64], usize) {
        let mut slot = [u8::MAX; 64];
        let mut kept = 0usize;
        for (position, entry) in slot.iter_mut().enumerate() {
            if idct::reads(size, position) {
                *entry = kept as u8;
                kept = kept.saturating_add(1);
            }
        }
        (slot, kept)
    }

    /// The bytes a store of `blocks` blocks takes for a component
    /// reconstructed at `size`.
    pub(super) fn bytes(blocks: usize, size: usize) -> u64 {
        let (_, kept) = Self::slots(size);
        let mask = if kept < 64 { 8u64 } else { 0 };
        (blocks as u64).saturating_mul(mask.saturating_add(2u64.saturating_mul(kept as u64)))
    }

    pub(super) fn new(blocks_w: usize, blocks_h: usize, size: usize) -> Self {
        let (slot, kept) = Self::slots(size);
        let blocks = blocks_w.saturating_mul(blocks_h);
        Self {
            blocks_w,
            blocks_h,
            slot,
            kept,
            masks: if kept < 64 {
                vec![0; blocks]
            } else {
                Vec::new()
            },
            values: vec![0; blocks.saturating_mul(kept)],
        }
    }

    fn index(&self, bx: usize, by: usize) -> Option<usize> {
        (bx < self.blocks_w && by < self.blocks_h)
            .then(|| by.saturating_mul(self.blocks_w).saturating_add(bx))
    }

    /// Block `(bx, by)`: the kept values, and a placeholder wherever a
    /// coefficient that is not kept is nonzero.
    pub(super) fn load(&self, bx: usize, by: usize) -> [i16; 64] {
        let mut block = [0i16; 64];
        let Some(index) = self.index(bx, by) else {
            return block;
        };
        let base = index.saturating_mul(self.kept);
        // At full size every coefficient is kept, in natural order: a copy.
        if self.kept == 64 {
            if let Some(values) = self.values.get(base..base.saturating_add(64)) {
                block.copy_from_slice(values);
            }
            return block;
        }
        let mask = self.masks.get(index).copied().unwrap_or(0);
        for (position, (cell, &slot)) in block.iter_mut().zip(&self.slot).enumerate() {
            *cell = if slot == u8::MAX {
                if mask & (1u64 << position) != 0 {
                    PLACEHOLDER
                } else {
                    0
                }
            } else {
                self.values
                    .get(base.saturating_add(usize::from(slot)))
                    .copied()
                    .unwrap_or(0)
            };
        }
        block
    }

    /// Block `(bx, by)` as the array it is, when every coefficient is kept
    /// (a full-size decode): what a decoder works on in place with no
    /// bookkeeping between it and the values. `None` at a reduced size, and
    /// for a block outside the store.
    pub(super) fn full_block_mut(&mut self, bx: usize, by: usize) -> Option<&mut [i16; 64]> {
        if self.kept != 64 {
            return None;
        }
        let base = self.index(bx, by)?.checked_mul(64)?;
        self.values
            .get_mut(base..base.checked_add(64)?)?
            .try_into()
            .ok()
    }

    /// Block `(bx, by)` in place: see [`StoredBlock`].
    pub(super) fn block(&mut self, bx: usize, by: usize) -> StoredBlock<'_> {
        let index = self.index(bx, by);
        StoredBlock { store: self, index }
    }

    /// Block `(bx, by)`'s DC into `block[0]`, the only coefficient a DC scan
    /// reads or writes; the rest of `block` is left as it is.
    pub(super) fn load_dc(&self, bx: usize, by: usize, block: &mut [i16; 64]) {
        let dc = self.index(bx, by).map_or(0, |index| {
            self.values
                .get(index.saturating_mul(self.kept))
                .copied()
                .unwrap_or(0)
        });
        block[0] = dc;
    }

    /// Block `(bx, by)`'s DC back from `block[0]`.
    pub(super) fn save_dc(&mut self, bx: usize, by: usize, block: &[i16; 64]) {
        self.block(bx, by).set(0, block[0]);
    }

    /// Store block `(bx, by)` back, whole.
    pub(super) fn save(&mut self, bx: usize, by: usize, block: &[i16; 64]) {
        let Some(index) = self.index(bx, by) else {
            return;
        };
        let mut mask = 0u64;
        let base = index.saturating_mul(self.kept);
        for (position, (&value, &slot)) in block.iter().zip(&self.slot).enumerate() {
            if value != 0 {
                mask |= 1u64 << position;
            }
            if slot != u8::MAX {
                if let Some(cell) = self.values.get_mut(base.saturating_add(usize::from(slot))) {
                    *cell = value;
                }
            }
        }
        if let Some(cell) = self.masks.get_mut(index) {
            *cell = mask;
        }
    }

    /// The DC of block `(bx, by)`.
    pub(super) fn dc(&self, bx: usize, by: usize) -> i32 {
        self.index(bx, by)
            .and_then(|index| self.values.get(index.saturating_mul(self.kept)))
            .map_or(0, |&v| i32::from(v))
    }
}

/// `SAVED_COEFS`: smoothing looks at the first ten coefficients in zig-zag
/// order.
pub(super) const SAVED: usize = 10;

/// Natural-order positions of zig-zag coefficients 1 to 9.
const Q01: usize = 1;
const Q10: usize = 8;
const Q20: usize = 16;
const Q11: usize = 9;
const Q02: usize = 2;
const Q03: usize = 3;
const Q12: usize = 10;
const Q21: usize = 17;
const Q30: usize = 24;

/// `smoothing_ok`'s quantiser test: the DC's and the first nine AC
/// coefficients' divisors must be nonzero.
pub(super) fn quantisers_usable(quant: &[u16; 64]) -> bool {
    [0, Q01, Q10, Q20, Q11, Q02, Q03, Q12, Q21, Q30]
        .iter()
        .all(|&position| quant.get(position).copied().unwrap_or(0) != 0)
}

/// One estimate: the prediction libjpeg makes for a coefficient from `num`
/// (the DC terms, weighted and scaled by the DC's quantiser) and the
/// coefficient's own quantiser `q`, limited to what `al` bits could still
/// hold.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "num is at most 25 DC values of 16 bits times weights up to 152 times a 16-bit quantiser, under 2^40; the divisor is at least 1"
)]
fn predict(num: i64, q: i64, al: i32) -> i16 {
    let (magnitude, negative) = if num >= 0 { (num, false) } else { (-num, true) };
    let divisor = (q << 8).max(1);
    let mut pred = ((q << 7).wrapping_add(magnitude) / divisor) as i32;
    if al > 0 && pred >= (1 << al.min(30)) {
        pred = (1 << al.min(30)) - 1;
    }
    if negative {
        pred = pred.wrapping_neg();
    }
    pred as i16
}

/// The geometry one component's smoothing pass needs.
pub(super) struct Smoothing<'a> {
    pub(super) store: &'a Store,
    pub(super) quant: &'a [u16; 64],
    /// The latched progression status to use for this row: zig-zag
    /// coefficients 0 to 9.
    pub(super) bits: &'a [i32; SAVED],
    pub(super) width_in_blocks: usize,
    pub(super) v: usize,
    pub(super) block_rows: usize,
    pub(super) imcu_row: usize,
    pub(super) total_imcu_rows: usize,
    pub(super) size: usize,
}

/// `decompress_smooth_data` for one component and one iMCU row, into
/// `plane`: that iMCU row's samples, `stride` a row.
#[allow(
    clippy::many_single_char_names,
    reason = "libjpeg's own names for the 25 DC registers are clearer as they are"
)]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "frame geometry: dimensions are at most 65500, sampling factors at most 4 and block sizes at most 8, so every product here is far below usize::MAX"
)]
pub(super) fn smooth_row(s: &Smoothing<'_>, plane: &mut [u8], stride: usize) {
    let bits = s.bits;
    let change_dc = bits.iter().skip(1).all(|&b| b == -1);
    let quant = |position: usize| i64::from(s.quant.get(position).copied().unwrap_or(0));
    let (q00, q01, q10, q20, q11, q02) = (
        quant(0),
        quant(Q01),
        quant(Q10),
        quant(Q20),
        quant(Q11),
        quant(Q02),
    );
    let (q03, q12, q21, q30) = if change_dc {
        (quant(Q03), quant(Q12), quant(Q21), quant(Q30))
    } else {
        (0, 0, 0, 0)
    };
    let image_block_rows = s.block_rows.saturating_mul(s.total_imcu_rows);
    let first_row = s.imcu_row.saturating_mul(s.v);
    for block_row in 0..s.block_rows {
        let image_block_row = s
            .imcu_row
            .saturating_mul(s.block_rows)
            .saturating_add(block_row);
        // Block rows in the store, as libjpeg's row pointers pick them.
        let here = first_row.saturating_add(block_row);
        let above = if image_block_row > 0 {
            here.wrapping_sub(1)
        } else {
            here
        };
        let above2 = if image_block_row > 1 {
            here.wrapping_sub(2)
        } else {
            above
        };
        let below = if image_block_row.saturating_add(1) < image_block_rows {
            here.saturating_add(1)
        } else {
            here
        };
        let below2 = if image_block_row.saturating_add(2) < image_block_rows {
            here.saturating_add(2)
        } else {
            below
        };
        let rows = [above2, above, here, below, below2];
        let dc = |r: usize, bx: usize| s.store.dc(bx, rows.get(r).copied().unwrap_or(here));
        // The 5x5 DC registers: `d[r][c]` is libjpeg's DC(5r + c + 1).
        let mut d = [[0i32; 5]; 5];
        for (r, row) in d.iter_mut().enumerate() {
            *row = [dc(r, 0); 5];
        }
        let last_column = s.width_in_blocks.saturating_sub(1);
        for bx in 0..s.width_in_blocks {
            let mut w = s.store.load(bx, here);
            if bx == 0 && bx < last_column {
                for (r, row) in d.iter_mut().enumerate() {
                    row[3] = dc(r, 1);
                    row[4] = row[3];
                }
            }
            if bx.saturating_add(1) < last_column {
                for (r, row) in d.iter_mut().enumerate() {
                    row[4] = dc(r, bx.saturating_add(2));
                }
            }
            smooth_block(
                &mut w,
                &d,
                bits,
                change_dc,
                q00,
                [q01, q10, q20, q11, q02],
                [q03, q12, q21, q30],
            );
            let at = block_row
                .saturating_mul(s.size)
                .saturating_mul(stride)
                .saturating_add(bx.saturating_mul(s.size));
            idct::inverse(
                s.size,
                &w,
                s.quant,
                &mut Target {
                    out: plane,
                    at,
                    stride,
                },
            );
            for row in &mut d {
                row.copy_within(1..5, 0);
            }
        }
    }
}

/// The estimates for one block, given its 5x5 neighbourhood of DC values.
#[allow(clippy::too_many_arguments, clippy::similar_names)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "the weighted sums of 25 DC values of 16 bits, times a 16-bit quantiser, stay under 2^40; the DC register indices are the constants 1 to 25 and the block positions constants under 64"
)]
fn smooth_block(
    w: &mut [i16; 64],
    d: &[[i32; 5]; 5],
    bits: &[i32; SAVED],
    change_dc: bool,
    q00: i64,
    [q01, q10, q20, q11, q02]: [i64; 5],
    [q03, q12, q21, q30]: [i64; 4],
) {
    // libjpeg's DC01..DC25, row by row.
    let dc = |n: usize| -> i64 { i64::from(d[(n - 1) / 5][(n - 1) % 5]) };
    let bit = |k: usize| bits.get(k).copied().unwrap_or(0);
    // AC01
    let al = bit(1);
    if al != 0 && w[1] == 0 {
        let num = q00
            * if change_dc {
                -dc(1) - dc(2) + dc(4) + dc(5) - 3 * dc(6) + 13 * dc(7) - 13 * dc(9) + 3 * dc(10)
                    - 3 * dc(11)
                    + 38 * dc(12)
                    - 38 * dc(14)
                    + 3 * dc(15)
                    - 3 * dc(16)
                    + 13 * dc(17)
                    - 13 * dc(19)
                    + 3 * dc(20)
                    - dc(21)
                    - dc(22)
                    + dc(24)
                    + dc(25)
            } else {
                -7 * dc(11) + 50 * dc(12) - 50 * dc(14) + 7 * dc(15)
            };
        w[1] = predict(num, q01, al);
    }
    // AC10
    let al = bit(2);
    if al != 0 && w[8] == 0 {
        let num = q00
            * if change_dc {
                -dc(1) - 3 * dc(2) - 3 * dc(3) - 3 * dc(4) - dc(5) - dc(6)
                    + 13 * dc(7)
                    + 38 * dc(8)
                    + 13 * dc(9)
                    - dc(10)
                    + dc(16)
                    - 13 * dc(17)
                    - 38 * dc(18)
                    - 13 * dc(19)
                    + dc(20)
                    + dc(21)
                    + 3 * dc(22)
                    + 3 * dc(23)
                    + 3 * dc(24)
                    + dc(25)
            } else {
                -7 * dc(3) + 50 * dc(8) - 50 * dc(18) + 7 * dc(23)
            };
        w[8] = predict(num, q10, al);
    }
    // AC20
    let al = bit(3);
    if al != 0 && w[16] == 0 {
        let num = q00
            * if change_dc {
                dc(3) + 2 * dc(7) + 7 * dc(8) + 2 * dc(9) - 5 * dc(12) - 14 * dc(13) - 5 * dc(14)
                    + 2 * dc(17)
                    + 7 * dc(18)
                    + 2 * dc(19)
                    + dc(23)
            } else {
                -dc(3) + 13 * dc(8) - 24 * dc(13) + 13 * dc(18) - dc(23)
            };
        w[16] = predict(num, q20, al);
    }
    // AC11
    let al = bit(4);
    if al != 0 && w[9] == 0 {
        let num = q00
            * if change_dc {
                -dc(1) + dc(5) + 9 * dc(7) - 9 * dc(9) - 9 * dc(17) + 9 * dc(19) + dc(21) - dc(25)
            } else {
                dc(10) + dc(16) - 10 * dc(17) + 10 * dc(19) - dc(2) - dc(20) + dc(22) - dc(24)
                    + dc(4)
                    - dc(6)
                    + 10 * dc(7)
                    - 10 * dc(9)
            };
        w[9] = predict(num, q11, al);
    }
    // AC02
    let al = bit(5);
    if al != 0 && w[2] == 0 {
        let num = q00
            * if change_dc {
                2 * dc(7) - 5 * dc(8) + 2 * dc(9) + dc(11) + 7 * dc(12) - 14 * dc(13)
                    + 7 * dc(14)
                    + dc(15)
                    + 2 * dc(17)
                    - 5 * dc(18)
                    + 2 * dc(19)
            } else {
                -dc(11) + 13 * dc(12) - 24 * dc(13) + 13 * dc(14) - dc(15)
            };
        w[2] = predict(num, q02, al);
    }
    if change_dc {
        // AC03
        let al = bit(6);
        if al != 0 && w[3] == 0 {
            let num = q00 * (dc(7) - dc(9) + 2 * dc(12) - 2 * dc(14) + dc(17) - dc(19));
            w[3] = predict(num, q03, al);
        }
        // AC12
        let al = bit(7);
        if al != 0 && w[10] == 0 {
            let num = q00 * (dc(7) - 3 * dc(8) + dc(9) - dc(17) + 3 * dc(18) - dc(19));
            w[10] = predict(num, q12, al);
        }
        // AC21
        let al = bit(8);
        if al != 0 && w[17] == 0 {
            let num = q00 * (dc(7) - dc(9) - 3 * dc(12) + 3 * dc(14) + dc(17) - dc(19));
            w[17] = predict(num, q21, al);
        }
        // AC30
        let al = bit(9);
        if al != 0 && w[24] == 0 {
            let num = q00 * (dc(7) + 2 * dc(8) + dc(9) - dc(17) - 2 * dc(18) - dc(19));
            w[24] = predict(num, q30, al);
        }
        // The DC itself, with no limit.
        let num = q00
            * (-2 * dc(1) - 6 * dc(2) - 8 * dc(3) - 6 * dc(4) - 2 * dc(5) - 6 * dc(6)
                + 6 * dc(7)
                + 42 * dc(8)
                + 6 * dc(9)
                - 6 * dc(10)
                - 8 * dc(11)
                + 42 * dc(12)
                + 152 * dc(13)
                + 42 * dc(14)
                - 8 * dc(15)
                - 6 * dc(16)
                + 6 * dc(17)
                + 42 * dc(18)
                + 6 * dc(19)
                - 6 * dc(20)
                - 2 * dc(21)
                - 6 * dc(22)
                - 8 * dc(23)
                - 6 * dc(24)
                - 2 * dc(25));
        w[0] = predict(num, q00, 0);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_full_store_keeps_everything() {
        let mut store = Store::new(2, 2, 8);
        let mut block = [0i16; 64];
        for (i, c) in block.iter_mut().enumerate() {
            *c = i as i16 - 30;
        }
        store.save(1, 1, &block);
        assert_eq!(store.load(1, 1), block);
        // In place, it reads as `load` gives it, and writes as `save` keeps it.
        let full = store.load(1, 1);
        {
            let mut place = store.block(1, 1);
            for position in 0..64 {
                assert_eq!(place.at(position), full[position], "position {position}");
            }
            place.set(5, 0);
            place.set(6, -9);
        }
        let mut changed = full;
        changed[5] = 0;
        changed[6] = -9;
        assert_eq!(store.load(1, 1), changed);
        let mut dc = [7i16; 64];
        store.load_dc(1, 1, &mut dc);
        assert_eq!(dc[0], changed[0]);
        assert_eq!(dc[1], 7, "a DC load leaves the rest alone");
        assert_eq!(store.dc(1, 1), -30);
        assert_eq!(store.load(0, 0), [0; 64]);
    }

    #[test]
    fn a_reduced_store_keeps_what_its_transform_reads_and_whether_the_rest_is_zero() {
        let mut store = Store::new(1, 1, 2);
        let mut block = [0i16; 64];
        block[0] = 5;
        block[1] = -3; // (0, 1): read by the 2x2 transform
        block[2] = 9; // (0, 2): not read
        store.save(0, 0, &block);
        let back = store.load(0, 0);
        assert_eq!((back[0], back[1]), (5, -3));
        assert_eq!(back[2], PLACEHOLDER);
        assert_eq!(back[4], 0);
        // In place: the same reading, and a coefficient that is not kept
        // keeps only whether it is nonzero.
        {
            let mut place = store.block(0, 0);
            assert_eq!((place.at(0), place.at(1)), (5, -3));
            assert_eq!(place.at(2), PLACEHOLDER);
            assert_eq!(place.at(4), 0);
            place.set(4, 7);
            assert_eq!(place.at(4), PLACEHOLDER);
            place.set(2, 0);
            assert_eq!(place.at(2), 0);
        }
        assert!(Store::bytes(1, 2) < Store::bytes(1, 8));
        assert_eq!(Store::bytes(1, 1), 10);
    }

    #[test]
    fn the_placeholder_survives_every_refinement() {
        let mut value = PLACEHOLDER;
        for al in (0..=13).rev() {
            let p1 = 1i16 << al;
            value = if value >= 0 {
                value.wrapping_add(p1)
            } else {
                value.wrapping_sub(p1)
            };
            assert_ne!(value, 0);
        }
    }

    #[test]
    fn predictions_round_and_limit_as_libjpeg_does() {
        // (q * 128 + num) / (q * 256), for num 1000 and q 2: 1256 / 512 = 2.
        assert_eq!(predict(1000, 2, 0), 2);
        assert_eq!(predict(-1000, 2, 0), -2);
        // Limited to what two bits can still say.
        assert_eq!(predict(100_000, 2, 2), 3);
        assert_eq!(predict(-100_000, 2, 2), -3);
    }
}
