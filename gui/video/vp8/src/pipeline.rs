//! A frame of one token partition on two threads: its macroblocks on one,
//! its loop filter on the other, a row behind.
//!
//! A frame's coefficients are read in one pass through each partition, so a
//! frame of one partition -- what encoders make unless asked for more --
//! cannot share its rows among threads as `threading` shares a frame of
//! several, in libvpx either. Its loop filter, about a third of the work,
//! can go to a second thread: filtering row `r` touches rows `r - 1` and `r`
//! alone, and no row decoded after it reads the frame -- each predicts from
//! the unfiltered bottom pixels of the row above, which the decoding thread
//! keeps for it (libvpx's threads keep them the same way, its
//! `mt_yabove_row`). So the filter runs on row `r` while row `r + 1`
//! decodes, and the pictures are the single thread's, bit for bit. libvpx
//! does not do this.
//!
//! The decoding thread (the caller's) decodes each row into a band
//! (`band`), as the row threads do, and sends it, finished, to the filter
//! thread, which owns the frame: it copies the band in, filters the row,
//! and fills the borders of the row above, final then. Bands go back to be
//! reused. A version-3 macroblock whose chroma libvpx leaves as its buffer
//! held it cannot be decoded in a band; the frame then decodes again on one
//! thread -- which finds the rows from that macroblock's on as the frame
//! found them, since a row is copied in only once decoded, and filtering a
//! row reaches no further down than the row itself.

#![allow(
    clippy::indexing_slicing,
    reason = "token contexts and skip flags by macroblock column, within rows of the frame's width"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "row and column counts are below 1024, and their product a frame's macroblocks"
)]

use std::sync::mpsc::{Receiver, Sender, channel};

use crate::band::{Band, Geometry};
use crate::boolread::BoolDecoder;
use crate::decodeframe::{MbAt, Refs, Residual, Rows, decode_macroblock};
use crate::frame::Frame;
use crate::inter::Prior;
use crate::loopfilter::{self, LoopFilterInfo};
use crate::modes::ModeGrid;
use crate::threading::Threading;
use crate::tokens::Context;

#[cfg(test)]
thread_local! {
    /// On the thread decoding: how many frames have had their loop filter on
    /// a second thread -- for tests to see that this path was taken.
    pub(crate) static PIPELINED: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

/// How many bands are in flight at most. Three would do if a thread woke
/// the moment a row arrived; but a wake-up takes tens of microseconds, as
/// long as decoding a row of a small picture, and with three the decoding
/// thread caught up with the filter and waited: 640x360 gained nothing.
/// With eight the filter thread clears several rows a wake-up (640x360 1.17
/// times as fast, 720p 1.25, 1080p 1.31; sixteen no better, and twice the
/// memory -- a band of 1080p is 63 KiB).
const BANDS: usize = 8;

/// A row, decoded: its number, its band, and which of its macroblocks turned
/// out to have no coefficients (the filter skips their inner edges).
struct Decoded {
    mb_row: usize,
    band: Band,
    skips: Vec<bool>,
}

/// Decode and filter every macroblock row of `new` on two threads, and fill
/// its borders. `None` if the frame is one to decode on one thread -- no
/// loop filter, one row, fewer macroblocks than two threads are given, a
/// macroblock a band cannot hold, or a thread that could not start -- with
/// nothing written that the single thread does not overwrite, and `grid`
/// and `above` as they were. Otherwise whether the partition ran dry or a
/// reference was corrupt, with `grid` and `above` as the single thread
/// leaves them.
pub(crate) fn decode_mb_rows(
    rows: &Rows<'_>,
    grid: &mut ModeGrid,
    above: &mut [Context],
    partition: &BoolDecoder<'_>,
    new: &mut Frame,
    refs: &Refs<'_>,
    threading: Threading,
) -> Option<bool> {
    let (mb_rows, mb_cols) = (rows.mb_rows, rows.mb_cols);
    let lfi = rows.lf?;
    let allowed = (mb_rows * mb_cols)
        .checked_div(threading.min_macroblocks)
        .unwrap_or(usize::MAX);
    if threading.threads.min(allowed) < 2 || mb_rows < 2 || above.len() != mb_cols {
        return None;
    }
    let geometry = Geometry::of(new, mb_rows, mb_cols)?;
    // Copies: should the threads fail, the single thread starts the
    // partition and the contexts from their beginnings.
    let mut bc = partition.clone();
    let mut contexts = above.to_vec();
    let mut skips = vec![false; mb_rows * mb_cols];
    let (to_filter, decoded) = channel::<Decoded>();
    let (to_decoder, returned) = channel::<Band>();
    let outcome = {
        let grid: &ModeGrid = grid;
        std::thread::scope(|scope| {
            let filter = move || filter_rows(new, grid, rows, lfi, (&decoded, &to_decoder));
            let filter = std::thread::Builder::new()
                .spawn_scoped(scope, filter)
                .ok()?;
            let corrupted = decode_rows(
                rows,
                grid,
                refs,
                (&mut bc, &mut contexts, &mut skips),
                geometry,
                (to_filter, &returned),
            );
            // The decoding thread's channel end is gone now, so the filter
            // thread runs out of rows and stops, whether or not they were
            // all decoded.
            let filtered = filter.join().ok().flatten();
            Some((corrupted?, filtered?))
        })
    };
    #[cfg(test)]
    crate::threading::TALLY.with(|t| {
        let (threaded, fell_back) = t.get();
        t.set(if outcome.is_some() {
            (threaded + 1, fell_back)
        } else {
            (threaded, fell_back + 1)
        });
    });
    #[cfg(test)]
    if outcome.is_some() {
        PIPELINED.with(|n| n.set(n.get() + 1));
    }
    let (corrupted, ()) = outcome?;
    above.copy_from_slice(&contexts);
    for (mb_row, flags) in skips.chunks(mb_cols).enumerate() {
        for (mb_col, &skip) in flags.iter().enumerate() {
            let idx = grid.index(mb_row, mb_col);
            if let Some(cell) = grid.cells.get_mut(idx) {
                cell.mb_skip_coeff = skip;
            }
        }
    }
    Some(corrupted)
}

/// Decode every row into a band and send it to the filter thread: the
/// decoding half of the pipeline, on the caller's thread. Whether any
/// macroblock was corrupt; `None` if a macroblock could not be decoded in a
/// band, or the filter thread stopped.
fn decode_rows(
    rows: &Rows<'_>,
    grid: &ModeGrid,
    refs: &Refs<'_>,
    (bc, contexts, skips): (&mut BoolDecoder<'_>, &mut [Context], &mut [bool]),
    geometry: [Geometry; 3],
    (to_filter, returned): (Sender<Decoded>, &Receiver<Band>),
) -> Option<bool> {
    let (mb_rows, mb_cols) = (rows.mb_rows, rows.mb_cols);
    let mut spare: Vec<Band> = Vec::with_capacity(BANDS);
    let mut made = 0;
    // The row above's bottom pixels, unfiltered, by plane.
    let mut above_row: [Vec<u8>; 3] = core::array::from_fn(|p| vec![0; geometry[p].width]);
    let mut residual = Residual::new();
    let mut corrupted = false;
    for r in 0..mb_rows {
        spare.extend(returned.try_iter());
        let mut band = match spare.pop() {
            Some(band) => band,
            None if made < BANDS => {
                made += 1;
                Band::new(geometry)
            }
            None => returned.recv().ok()?,
        };
        band.prepare(r);
        if r > 0 {
            band.put_above(&above_row);
        }
        let mut left: Context = [0; 9];
        let row_skips = &mut skips[r * mb_cols..(r + 1) * mb_cols];
        for c in 0..mb_cols {
            let mut mi = *grid.get(r, c);
            corrupted |= refs.corrupted[usize::from(mi.ref_frame & 3)];
            let at = MbAt {
                mb_row: r,
                mb_col: c,
                left_available: c > 0,
                up_available: r > 0,
            };
            let predicted = decode_macroblock(
                rows,
                &at,
                &mut mi,
                (&mut *bc, &mut contexts[c], &mut left),
                &mut residual,
                &mut band.target(r),
                Prior::Unavailable,
                refs,
            );
            if !predicted {
                return None;
            }
            corrupted |= bc.has_error();
            row_skips[c] = mi.mb_skip_coeff;
        }
        band.take_bottom(&mut above_row);
        to_filter
            .send(Decoded {
                mb_row: r,
                band,
                skips: row_skips.to_vec(),
            })
            .ok()?;
    }
    Some(corrupted)
}

/// Copy each decoded row into `new`, filter it, and fill the borders of the
/// row above it: the filtering half of the pipeline, on a thread of its own.
/// `None` if the rows stopped coming before the last.
fn filter_rows(
    new: &mut Frame,
    grid: &ModeGrid,
    rows: &Rows<'_>,
    lfi: &LoopFilterInfo,
    (decoded, back): (&Receiver<Decoded>, &Sender<Band>),
) -> Option<()> {
    let (mb_rows, mb_cols) = (rows.mb_rows, rows.mb_cols);
    let mut next = 0;
    for Decoded {
        mb_row,
        band,
        skips,
    } in decoded.iter()
    {
        if mb_row != next || skips.len() != mb_cols {
            return None;
        }
        band.copy_into(new, mb_row)?;
        // The decoding thread may be done with bands, and gone: then this
        // one is simply dropped.
        let _ = back.send(band);
        let mut target = new.target();
        for (c, &skip) in skips.iter().enumerate() {
            let mut mi = *grid.get(mb_row, c);
            mi.mb_skip_coeff = skip;
            loopfilter::filter_mb(
                &mut target,
                &mi,
                lfi,
                (mb_row, c),
                rows.simple_filter,
                rows.key_frame,
            );
        }
        if mb_row > 0 {
            new.extend_row_left_right(mb_row - 1);
        }
        next += 1;
    }
    if next != mb_rows {
        return None;
    }
    new.extend_row_left_right(mb_rows - 1);
    new.extend_top_bottom();
    Some(())
}
