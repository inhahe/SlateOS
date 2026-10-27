//! The pools a picture's tables come from (`init_table_pools` and
//! `alloc_picture` in `h264_slice.c`, over `av_refstruct_pool_*` in
//! `libavutil/refstruct.c`), kept as FFmpeg keeps them, because what a
//! reused table still holds is read.
//!
//! FFmpeg takes each picture's six tables -- the quantizer and type of each
//! macroblock, and each list's vectors and references -- from four pools,
//! and gives them back when the last holder of the picture lets go. The
//! last given back is the next taken, and nothing is cleared: a table is
//! zeroed only when first made. So a macroblock no slice decoded keeps what
//! the table's last picture left there, and that is read -- by error
//! concealment, whose vectors decide which edges it smooths and which
//! predictions it tries, and by direct prediction from a picture with lost
//! macroblocks. Tables here come and go in FFmpeg's order, with what they
//! held.
//!
//! The four pools move in step, every picture taking all six tables and
//! giving all six back at once, so one stack of whole sets stands for them.
//! A picture gives its list 0 buffers back before its list 1 ones, and the
//! next takes list 0's first, so a reused set comes back with its lists
//! swapped.
//!
//! Besides its slot, the decoder's view of the picture being decoded
//! (`cur_pic`) holds its tables until the next picture is set up, and the
//! picture kept for concealment (`last_pic_for_ec`) holds those of the
//! picture it was taken from until the packet is done. A slot that lets go
//! before them leaves its tables to them ([`Holders`]).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "table sizes from macroblock counts the SPS bounds"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "slots are the decoder's, under SLOTS; holders are HOLD_CUR and HOLD_EC; a left \
              index is one this module handed out and has not freed"
)]

use crate::decoder::Context;
use crate::picture::LAST_PIC_FOR_EC;

/// A picture's tables: `qscale_table`, `mb_type`, `motion_val` and
/// `ref_index`, laid out as [`crate::picture::Picture`] holds them, and the
/// pool generation they were made in.
#[derive(Clone, Debug, Default)]
pub(crate) struct Tables {
    pub(crate) qscale_table: Vec<i8>,
    pub(crate) mb_type: Vec<u32>,
    pub(crate) motion_val: [Vec<[i16; 2]>; 2],
    pub(crate) ref_index: [Vec<i8>; 2],
    /// [`TablePool`]'s generation when they were made; 0 for none.
    pub(crate) generation: u32,
}

/// How large new tables are, in entries (`init_table_pools`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct TableSizes {
    pub(crate) qscale_table: usize,
    pub(crate) mb_type: usize,
    pub(crate) motion_val: usize,
    pub(crate) ref_index: usize,
}

/// FFmpeg's four table pools, as one stack of whole sets.
#[derive(Clone, Debug)]
pub(crate) struct TablePool {
    /// The sets given back, the last given back last (`available_entries`).
    free: Vec<Tables>,
    /// Counts the pools FFmpeg has made: tables of an earlier one are freed
    /// when let go, not kept.
    generation: u32,
}

impl Default for TablePool {
    fn default() -> Self {
        Self {
            free: Vec::new(),
            generation: 1,
        }
    }
}

impl TablePool {
    /// Tables for a new picture: the set given back last, its lists
    /// swapped, or new ones of `sizes`, zeroed. `None` past what can be
    /// allocated.
    pub(crate) fn get(&mut self, sizes: TableSizes) -> Option<Tables> {
        if let Some(mut t) = self.free.pop() {
            t.motion_val.swap(0, 1);
            t.ref_index.swap(0, 1);
            return Some(t);
        }
        fn zeroed<T: Clone + Default>(len: usize) -> Option<Vec<T>> {
            let mut v = Vec::new();
            v.try_reserve_exact(len).ok()?;
            v.resize(len, T::default());
            Some(v)
        }
        Some(Tables {
            qscale_table: zeroed(sizes.qscale_table)?,
            mb_type: zeroed(sizes.mb_type)?,
            motion_val: [zeroed(sizes.motion_val)?, zeroed(sizes.motion_val)?],
            ref_index: [zeroed(sizes.ref_index)?, zeroed(sizes.ref_index)?],
            generation: self.generation,
        })
    }

    /// Tables no one holds any longer: kept for the next picture if they
    /// are this pool's.
    fn put(&mut self, t: Tables) {
        if t.generation == self.generation {
            self.free.push(t);
        }
    }

    /// FFmpeg's pools freed (`ff_h264_free_tables`): the free sets go, and
    /// those still held go when let go.
    pub(crate) fn reset(&mut self) {
        self.free.clear();
        self.generation = self.generation.wrapping_add(1).max(1);
    }
}

/// The holders of a picture's tables besides its slot.
pub(crate) const HOLD_CUR: usize = 0;
pub(crate) const HOLD_EC: usize = 1;

/// What a holder holds: nothing, the tables of a slot still holding them,
/// or tables the slot has let go of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Hold {
    #[default]
    None,
    Slot(usize),
    Left(usize),
}

/// `cur_pic`'s and `last_pic_for_ec`'s holds ([`HOLD_CUR`], [`HOLD_EC`]),
/// and the tables slots left to them, with how many still hold each.
#[derive(Clone, Debug, Default)]
pub(crate) struct Holders {
    holds: [Hold; 2],
    left: Vec<Option<(Tables, u8)>>,
}

impl Holders {
    /// The tables holder `h` holds, if they are no longer a slot's.
    fn left_tables(&self, h: usize) -> Option<&Tables> {
        match self.holds[h] {
            Hold::Left(i) => self.left[i].as_ref().map(|(t, _)| t),
            _ => None,
        }
    }
}

impl Context {
    /// Picture `slot` let go (`ff_h264_unref_picture`): its tables back to
    /// the pool, or left to whoever else still holds them. The picture kept
    /// for concealment holds no tables of its own; letting it go lets go of
    /// its hold.
    pub(crate) fn unref_pic(&mut self, slot: usize) {
        if slot == LAST_PIC_FOR_EC {
            self.release_hold(HOLD_EC);
            // Its tables are empty, never a pool's: its picture's are held
            // through the hold just let go.
            let _ = self.pics[slot].unref();
            return;
        }
        let Some(t) = self.pics[slot].unref() else {
            return;
        };
        let holders: Vec<usize> = (0..2)
            .filter(|&h| self.holders.holds[h] == Hold::Slot(slot))
            .collect();
        if holders.is_empty() {
            self.pool.put(t);
            return;
        }
        let n = u8::try_from(holders.len()).unwrap_or(u8::MAX);
        let i = match self.holders.left.iter().position(Option::is_none) {
            Some(i) => {
                self.holders.left[i] = Some((t, n));
                i
            }
            None => {
                self.holders.left.push(Some((t, n)));
                self.holders.left.len() - 1
            }
        };
        for h in holders {
            self.holders.holds[h] = Hold::Left(i);
        }
    }

    /// Holder `h` lets go: tables a slot left to it go back to the pool
    /// when no one holds them any more.
    pub(crate) fn release_hold(&mut self, h: usize) {
        if let Hold::Left(i) = std::mem::take(&mut self.holders.holds[h]) {
            if let Some((t, n)) = self.holders.left[i].take() {
                if n > 1 {
                    self.holders.left[i] = Some((t, n - 1));
                } else {
                    self.pool.put(t);
                }
            }
        }
    }

    /// Holder `h` takes hold of picture `slot`'s tables, letting go of
    /// what it held.
    pub(crate) fn hold(&mut self, h: usize, slot: usize) {
        self.release_hold(h);
        self.holders.holds[h] = Hold::Slot(slot);
    }

    /// The list 0 vectors and references of picture `slot` -- for the
    /// picture kept for concealment, of the picture it holds -- if it has
    /// any.
    pub(crate) fn list0_tables(&self, slot: usize) -> Option<(&[[i16; 2]], &[i8])> {
        let (mv, r) = if slot == LAST_PIC_FOR_EC {
            match self.holders.holds[HOLD_EC] {
                Hold::Slot(s) => (&self.pics[s].motion_val[0], &self.pics[s].ref_index[0]),
                Hold::Left(_) => {
                    let t = self.holders.left_tables(HOLD_EC)?;
                    (&t.motion_val[0], &t.ref_index[0])
                }
                Hold::None => return None,
            }
        } else {
            (
                &self.pics[slot].motion_val[0],
                &self.pics[slot].ref_index[0],
            )
        };
        (!mv.is_empty() && !r.is_empty()).then_some((mv.as_slice(), r.as_slice()))
    }
}

#[cfg(test)]
mod tests;
