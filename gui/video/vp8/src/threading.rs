//! Macroblock rows on several threads: libvpx's `vp8/decoder/threading.c`
//! (`vp8mt_decode_mb_rows`), for a frame of more than one token partition.
//!
//! Row `r` reads its coefficients from partition `r % count`, so rows on
//! different partitions can decode at once, each a few macroblocks behind
//! the row above: a macroblock predicts from the row above's bottom pixels
//! (up to the macroblock above and to its right) and continues its token
//! contexts. Thread `t` of `n` takes rows `t`, `t + n`, ..., as libvpx's
//! threads do, with `n` at most the number of partitions -- so a row's
//! partition is never still in use by the row before it on the same
//! partition, which finishes first (each row waits for the one above it to
//! finish) -- and at most the number of rows.
//!
//! The pictures are the single thread's, bit for bit, because each thread
//! does what the single thread does in an order that reads and writes the
//! same pixels at the same moments relative to each other:
//!
//! - **Intra prediction reads unfiltered pixels.** The single thread filters
//!   a row after decoding the next. Here each macroblock is filtered as soon
//!   as the one to its right is decoded (its left edge filter reaches into
//!   the macroblock to its left, which must then be decoded), and the row
//!   below predicts from a copy of this row's bottom pixels taken before
//!   they are filtered -- libvpx's `mt_yabove_row`.
//! - **The filters run in raster order where they overlap.** A
//!   macroblock's top edge reaches three rows into the macroblock above, and
//!   the left edge of the one above and to its right reaches three columns
//!   into the same pixels, so a macroblock is filtered only once the row
//!   above has filtered the macroblock to the right of the one above it.
//! - **The row above's bottom pixels end as the row below leaves them.**
//!   They are final in the row above once its next macroblock is filtered,
//!   and go down to the row below, which filters its top edges into them.
//!
//! No pixel is shared but by message: each thread decodes a row into a
//! private band -- the row, with four rows of the row above (the loop
//! filter's reach) on top -- and the frame is cut into pieces, one per row,
//! each written by one thread: the row's own pixels but its last four, and
//! the last four of the row above, which it holds the last word on. A
//! thread copies its band into its piece as it finishes each row.
//!
//! One macroblock cannot be decoded in a band: the version-3 macroblock
//! whose chroma libvpx leaves as its buffer held it before the frame (see
//! `inter`), since a band holds nothing of that. A thread meeting one stops,
//! and the frame decodes again on one thread. No piece of the frame holding
//! such a macroblock's pixels has been written by then -- a piece is
//! written only when its row and the row above it are finished, and a
//! finished row has none -- so the single thread finds the buffer as the
//! frame found it. Any other failure (a thread that cannot start) falls
//! back the same way; nothing else in the decoder's state has changed.
//!
//! Translated into Rust, with its design changed for threads that share
//! nothing, from libvpx v1.17.0's `vp8/decoder/threading.c` (copyright the
//! WebM project authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::indexing_slicing,
    reason = "band positions are rows and columns inside a band sized for a macroblock row of the frame; geometry is checked against the frame before any thread starts"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "positions and sizes are bounded by the frame's (at most 16384 pixels a side, plus the border); row and column counts are below 1024"
)]

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use crate::boolread::BoolDecoder;
use crate::decodeframe::{MbAt, Refs, Residual, Rows, decode_macroblock};
use crate::frame::{Frame, Target};
use crate::inter::Prior;
use crate::loopfilter;
use crate::modes::{ModeGrid, ModeInfo};
use crate::tokens::Context;

/// How many rows of the row above a row's filters reach: the normal loop
/// filter's `p3..p0` across a macroblock's top edge.
const APRON: usize = 4;

/// A macroblock's width in each plane: luma, then the two chroma planes.
const SIZES: [usize; 3] = [16, 8, 8];

/// One row of a macroblock in every plane, Y then U then V.
const EDGE: usize = 16 + 8 + 8;

/// A macroblock's last [`APRON`] rows in every plane, Y then U then V, each
/// row by row.
const TAIL: usize = APRON * EDGE;

/// One plane's layout, as the frame's.
#[derive(Clone, Copy, Debug)]
struct Geometry {
    stride: usize,
    border: usize,
    /// The picture's width, rounded up to whole macroblocks.
    width: usize,
    /// A macroblock's width and height in this plane.
    size: usize,
}

/// What a row sends the row below it, as it goes.
#[derive(Debug)]
enum Msg {
    /// Macroblock `col` is decoded: its token contexts and its bottom row,
    /// unfiltered, which the row below continues and predicts from; and the
    /// last rows of the macroblock two to its left, final now that the one
    /// between is filtered.
    Step {
        col: usize,
        above: Context,
        edge: [u8; EDGE],
        tail: Option<[u8; TAIL]>,
    },
    /// The row is filtered: the last rows of its last two macroblocks (of
    /// its one, in a frame one macroblock wide).
    End {
        before_last: Option<[u8; TAIL]>,
        last: [u8; TAIL],
    },
}

#[cfg(test)]
thread_local! {
    /// On the thread decoding: how many frames have decoded on several
    /// threads, and how many began to and fell back to one -- for tests to
    /// see which path their frames took.
    pub(crate) static TALLY: core::cell::Cell<(usize, usize)> =
        const { core::cell::Cell::new((0, 0)) };
}

/// What every thread reads and none writes.
struct Shared<'s, 'd> {
    rows: &'s Rows<'s>,
    grid: &'s ModeGrid,
    refs: &'s Refs<'s>,
    /// The token partitions, each held by the one row reading it.
    partitions: &'s [Mutex<BoolDecoder<'d>>],
    geometry: [Geometry; 3],
}

/// A row for a thread to decode, and where what it decodes goes.
struct Job<'f> {
    mb_row: usize,
    /// Per plane, the frame's piece this row writes: from four rows above
    /// the row to four rows above its end (its own last four go to the row
    /// below's piece) -- from the plane's start for the first row, and to its
    /// end for the last.
    pieces: [&'f mut [u8]; 3],
    /// Whether each macroblock of the row turned out to have no
    /// coefficients.
    skips: &'f mut [bool],
    /// For the last row: where the token contexts end, as the single thread
    /// leaves them.
    above: Option<&'f mut [Context]>,
}

/// Decode and filter every macroblock row of `new` on up to `threads`
/// threads, and fill its borders. `None` if the frame is one to decode on
/// one thread -- a single partition, a single row, a macroblock a band
/// cannot hold, or a thread that could not start -- with nothing written
/// that the single thread does not overwrite, and `grid` and `above` as
/// they were. Otherwise whether a partition ran dry or a reference was
/// corrupt, with `grid` and `above` as the single thread leaves them.
pub(crate) fn decode_mb_rows(
    rows: &Rows<'_>,
    grid: &mut ModeGrid,
    above: &mut [Context],
    partitions: &[BoolDecoder<'_>],
    new: &mut Frame,
    refs: &Refs<'_>,
    threads: usize,
) -> Option<bool> {
    let (mb_rows, mb_cols) = (rows.mb_rows, rows.mb_cols);
    let workers = threads.min(partitions.len()).min(mb_rows);
    if workers < 2 || mb_cols == 0 || above.len() != mb_cols {
        return None;
    }
    let geometry: [Geometry; 3] = core::array::from_fn(|p| {
        let plane = &new.planes[p];
        Geometry {
            stride: plane.stride,
            border: plane.border,
            width: plane.width,
            size: SIZES[p],
        }
    });
    // The frame as the band assumes it: whole macroblocks, borders wide
    // enough for the pixels intra prediction reads left and right of them.
    let fits = new.planes.iter().zip(&geometry).all(|(plane, g)| {
        g.width == mb_cols * g.size
            && plane.height == mb_rows * g.size
            && g.border >= APRON + 4
            && g.stride >= g.width + 2 * g.border
            && plane.data.len() == (plane.height + 2 * g.border) * g.stride
    });
    if !fits {
        debug_assert!(false, "a frame laid out unlike Frame::new's");
        return None;
    }

    // Fresh copies: should the threads fail, the single thread starts the
    // partitions from their beginnings.
    let decoders: Vec<Mutex<BoolDecoder<'_>>> =
        partitions.iter().cloned().map(Mutex::new).collect();
    let mut skips = vec![false; mb_rows * mb_cols];
    let outcomes = {
        let [y, u, v] = &mut new.planes;
        let pieces = [
            cut(&mut y.data, &geometry[0], mb_rows)?,
            cut(&mut u.data, &geometry[1], mb_rows)?,
            cut(&mut v.data, &geometry[2], mb_rows)?,
        ];
        let mut jobs: Vec<Vec<Job<'_>>> = (0..workers).map(|_| Vec::new()).collect();
        let [py, pu, pv] = pieces;
        let mut last_above = Some(above);
        for (mb_row, (((y, u), v), skips)) in py
            .into_iter()
            .zip(pu)
            .zip(pv)
            .zip(skips.chunks_mut(mb_cols))
            .enumerate()
        {
            jobs.get_mut(mb_row % workers)?.push(Job {
                mb_row,
                pieces: [y, u, v],
                skips,
                above: if mb_row + 1 == mb_rows {
                    last_above.take()
                } else {
                    None
                },
            });
        }
        let shared = Shared {
            rows,
            grid,
            refs,
            partitions: &decoders,
            geometry,
        };
        run_threads(&shared, jobs)
    };
    let all = outcomes.into_iter().collect::<Option<Vec<bool>>>();
    #[cfg(test)]
    TALLY.with(|t| {
        let (threaded, fell_back) = t.get();
        t.set(if all.is_some() {
            (threaded + 1, fell_back)
        } else {
            (threaded, fell_back + 1)
        });
    });
    let corrupted = all?.into_iter().any(|c| c);
    for (mb_row, flags) in skips.chunks(mb_cols).enumerate() {
        for (mb_col, &skip) in flags.iter().enumerate() {
            let idx = grid.index(mb_row, mb_col);
            if let Some(cell) = grid.cells.get_mut(idx) {
                cell.mb_skip_coeff = skip;
            }
        }
    }
    new.extend_top_bottom();
    Some(corrupted)
}

/// Cut a plane's `data` into one piece per macroblock row: piece `r` from
/// four rows above row `r` (from the plane's start for the first), to four
/// rows above row `r + 1` (to the plane's end for the last).
fn cut<'a>(data: &'a mut [u8], g: &Geometry, mb_rows: usize) -> Option<Vec<&'a mut [u8]>> {
    let mut pieces = Vec::with_capacity(mb_rows);
    let mut rest = data;
    let mut at = 0;
    for mb_row in 1..mb_rows {
        let next = (g.border + mb_row * g.size - APRON) * g.stride;
        let (piece, after) = rest.split_at_mut_checked(next.checked_sub(at)?)?;
        pieces.push(piece);
        rest = after;
        at = next;
    }
    pieces.push(rest);
    Some(pieces)
}

/// Run `jobs[t]` on thread `t`, this thread taking `jobs[0]`, each thread
/// sending to the next (the next row's) and the last to the first. Each
/// thread's outcome: `None` from one that stopped or could not start.
fn run_threads(shared: &Shared<'_, '_>, jobs: Vec<Vec<Job<'_>>>) -> Vec<Option<bool>> {
    let workers = jobs.len();
    // Channel `t` runs from thread `t` to thread `t + 1`.
    let mut senders = Vec::with_capacity(workers);
    let mut receivers = Vec::with_capacity(workers);
    for _ in 0..workers {
        let (tx, rx) = channel::<Msg>();
        senders.push(Some(tx));
        receivers.push(Some(rx));
    }
    std::thread::scope(|scope| {
        let mut jobs = jobs.into_iter();
        let first = jobs.next();
        let mut handles = Vec::with_capacity(workers);
        let mut started = true;
        for (t, work) in jobs.enumerate().map(|(i, work)| (i + 1, work)) {
            let tx = senders.get_mut(t).and_then(Option::take);
            let rx = receivers.get_mut(t - 1).and_then(Option::take);
            let (Some(tx), Some(rx)) = (tx, rx) else {
                started = false;
                break;
            };
            let thread = std::thread::Builder::new()
                .spawn_scoped(scope, move || run(shared, work, &tx, &rx));
            match thread {
                Ok(h) => handles.push(h),
                // What it was given is dropped with it: the threads waiting
                // on it stop.
                Err(_) => {
                    started = false;
                    break;
                }
            }
        }
        let tx = senders.first_mut().and_then(Option::take);
        let rx = receivers.last_mut().and_then(Option::take);
        // Channel ends no thread took, dropped: any thread waiting on one
        // stops rather than waiting for ever.
        senders.clear();
        receivers.clear();
        let mine = match (started, first, tx, rx) {
            (true, Some(work), Some(tx), Some(rx)) => run(shared, work, &tx, &rx),
            _ => None,
        };
        let mut outcomes = Vec::with_capacity(workers);
        outcomes.push(mine);
        outcomes.extend(handles.into_iter().map(|h| h.join().ok().flatten()));
        outcomes
    })
}

/// What has arrived from the row above, and what of it is waiting to be put
/// in place.
struct Inbox<'r> {
    rx: &'r Receiver<Msg>,
    /// How many of the row above's macroblocks' edges have arrived.
    edges: usize,
    /// How many of its macroblocks' last rows have arrived ...
    tails: usize,
    /// ... and those of them not yet put above this row: they go in only
    /// once the macroblock below has been predicted from the unfiltered
    /// edge they replace, just before it is filtered.
    waiting: VecDeque<(usize, [u8; TAIL])>,
}

impl Inbox<'_> {
    /// Wait until `edges` edges and `tails` tails of the row above have
    /// arrived, putting edges and token contexts in place as they do.
    /// `None` if the row above stopped, or sent out of step.
    fn wait(
        &mut self,
        (edges, tails): (usize, usize),
        band: &mut Band,
        above: &mut [Context],
    ) -> Option<()> {
        let cols = above.len();
        while self.edges < edges || self.tails < tails {
            match self.rx.recv().ok()? {
                Msg::Step {
                    col,
                    above: ctx,
                    edge,
                    tail,
                } => {
                    if col != self.edges {
                        return None;
                    }
                    *above.get_mut(col)? = ctx;
                    band.put_edge(col, &edge, col + 1 == cols);
                    self.edges += 1;
                    if let Some(tail) = tail {
                        self.waiting.push_back((self.tails, tail));
                        self.tails += 1;
                    }
                }
                Msg::End { before_last, last } => {
                    if self.edges != cols {
                        return None;
                    }
                    for tail in before_last.into_iter().chain([last]) {
                        self.waiting.push_back((self.tails, tail));
                        self.tails += 1;
                    }
                    if self.tails != cols {
                        return None;
                    }
                }
            }
        }
        Some(())
    }

    /// Put the row above's last rows over macroblocks `..=col` above this
    /// row's, once they have arrived.
    fn place_tails(&mut self, col: usize, band: &mut Band, above: &mut [Context]) -> Option<()> {
        self.wait((0, col + 1), band, above)?;
        while let Some(&(c, _)) = self.waiting.front() {
            if c > col {
                break;
            }
            let (c, tail) = self.waiting.pop_front()?;
            band.put_tail(c, &tail);
        }
        Some(())
    }
}

/// Decode `jobs`, the rows of one thread, in order: receiving from the
/// thread of the row above each on `rx`, sending to that of the row below
/// on `tx`. Whether any macroblock decoded was corrupt; `None` if the thread
/// stopped.
fn run(
    shared: &Shared<'_, '_>,
    jobs: Vec<Job<'_>>,
    tx: &Sender<Msg>,
    rx: &Receiver<Msg>,
) -> Option<bool> {
    let rows = shared.rows;
    let cols = rows.mb_cols;
    let mut band = Band::new(shared.geometry);
    let mut above: Vec<Context> = vec![[0; 9]; cols];
    let mut inbox = Inbox {
        rx,
        edges: 0,
        tails: 0,
        waiting: VecDeque::new(),
    };
    let mut residual = Residual::new();
    let mut corrupted = false;
    for mut job in jobs {
        let r = job.mb_row;
        let below = r + 1 < rows.mb_rows;
        band.prepare(r);
        if r == 0 {
            above.fill([0; 9]);
        }
        inbox.edges = 0;
        inbox.tails = 0;
        inbox.waiting.clear();
        let mut left: Context = [0; 9];
        let partition = shared.partitions.get(r % shared.partitions.len())?;
        let mut partition = Some(partition.lock().ok()?);
        // The macroblock to the left, decoded and waiting to be filtered.
        let mut pending: Option<ModeInfo> = None;
        for c in 0..cols {
            if r > 0 {
                inbox.wait(((c + 2).min(cols), 0), &mut band, &mut above)?;
            }
            let mut mi = *shared.grid.get(r, c);
            corrupted |= shared.refs.corrupted[usize::from(mi.ref_frame & 3)];
            let at = MbAt {
                mb_row: r,
                mb_col: c,
                left_available: c > 0,
                up_available: r > 0,
            };
            let bc = partition.as_deref_mut()?;
            let ctx = above.get_mut(c)?;
            let predicted = decode_macroblock(
                rows,
                &at,
                &mut mi,
                (&mut *bc, &mut *ctx, &mut left),
                &mut residual,
                &mut band.target(r),
                Prior::Unavailable,
                shared.refs,
            );
            if !predicted {
                return None;
            }
            corrupted |= bc.has_error();
            let ctx = *ctx;
            if c + 1 == cols {
                // The partition is the next row's to read it now.
                partition = None;
            }
            *job.skips.get_mut(c)? = mi.mb_skip_coeff;
            let edge = band.edge(c);
            if let Some(left_mi) = pending.replace(mi) {
                finish(
                    shared,
                    &mut band,
                    &mut inbox,
                    &mut above,
                    (r, c - 1),
                    &left_mi,
                )?;
            }
            if below {
                let tail = (c >= 2).then(|| band.tail(c - 2));
                tx.send(Msg::Step {
                    col: c,
                    above: ctx,
                    edge,
                    tail,
                })
                .ok()?;
            }
        }
        drop(partition);
        let last_mi = pending?;
        finish(
            shared,
            &mut band,
            &mut inbox,
            &mut above,
            (r, cols - 1),
            &last_mi,
        )?;
        if below {
            tx.send(Msg::End {
                before_last: (cols >= 2).then(|| band.tail(cols - 2)),
                last: band.tail(cols - 1),
            })
            .ok()?;
        }
        band.write_back(r, !below, &mut job.pieces)?;
        if let Some(out) = job.above.take() {
            out.copy_from_slice(above.get(..out.len())?);
        }
    }
    Some(corrupted)
}

/// Filter macroblock `(r, c)`, whose modes are `mi`, once the row above's
/// last rows over it are in place: libvpx's threads filter each macroblock
/// as they go.
fn finish(
    shared: &Shared<'_, '_>,
    band: &mut Band,
    inbox: &mut Inbox<'_>,
    above: &mut [Context],
    (r, c): (usize, usize),
    mi: &ModeInfo,
) -> Option<()> {
    if r > 0 {
        inbox.place_tails(c, band, above)?;
    }
    let rows = shared.rows;
    if let Some(lfi) = rows.lf {
        loopfilter::filter_mb(
            &mut band.target(r),
            mi,
            lfi,
            (r, c),
            rows.simple_filter,
            rows.key_frame,
        );
    }
    Some(())
}

/// A thread's copy of the macroblock row it is decoding: per plane, the
/// last [`APRON`] rows of the row above and then the row's own, at the
/// frame's stride, with its border.
struct Band {
    planes: [Vec<u8>; 3],
    geometry: [Geometry; 3],
}

impl Band {
    fn new(geometry: [Geometry; 3]) -> Self {
        Self {
            planes: core::array::from_fn(|p| {
                let g = &geometry[p];
                vec![0; (APRON + g.size) * g.stride]
            }),
            geometry,
        }
    }

    /// The index in plane `p` of pixel `x` of the band's row `row`.
    fn at(&self, p: usize, row: usize, x: usize) -> usize {
        let g = &self.geometry[p];
        row * g.stride + g.border + x
    }

    /// The band as the frame's macroblock row `mb_row`, for reconstruction
    /// and filtering.
    #[allow(
        clippy::cast_possible_wrap,
        reason = "offsets within planes of at most 16384 pixels a side and their border"
    )]
    fn target(&mut self, mb_row: usize) -> Target<'_> {
        let g = self.geometry;
        let origins = core::array::from_fn(|p| {
            let band_start = (g[p].border + APRON * g[p].stride) as isize;
            band_start - (mb_row * g[p].size * g[p].stride) as isize
        });
        let [y, u, v] = &mut self.planes;
        Target::new(
            [y.as_mut_slice(), u.as_mut_slice(), v.as_mut_slice()],
            g.map(|g| g.stride),
            origins,
        )
    }

    /// Set what intra prediction reads around macroblock row `mb_row` that
    /// no macroblock writes: 129 left of each of its rows and of the row
    /// above, as libvpx's `setup_intra_recon_left`; and above the frame's
    /// first row, 127 from one pixel left of it to four right of its end, as
    /// `vp8_setup_intra_recon_top_line`.
    fn prepare(&mut self, mb_row: usize) {
        for (plane, g) in self.planes.iter_mut().zip(&self.geometry) {
            for row in APRON - 1..APRON + g.size {
                plane[row * g.stride + g.border - 1] = 129;
            }
            if mb_row == 0 {
                let at = (APRON - 1) * g.stride + g.border - 1;
                plane[at..at + g.width + 5].fill(127);
            }
        }
    }

    /// Macroblock `col`'s bottom row in each plane.
    fn edge(&self, col: usize) -> [u8; EDGE] {
        let mut out = [0; EDGE];
        let mut o = 0;
        for (p, g) in self.geometry.iter().enumerate() {
            let at = self.at(p, APRON + g.size - 1, col * g.size);
            out[o..o + g.size].copy_from_slice(&self.planes[p][at..at + g.size]);
            o += g.size;
        }
        out
    }

    /// Put macroblock `col` of the row above's bottom row above the band's
    /// own rows; after the row's `last`, its last pixel four times more,
    /// where the last macroblock of the band's row looks above and to its
    /// right: libvpx's `vp8_extend_mb_row`.
    fn put_edge(&mut self, col: usize, edge: &[u8; EDGE], last: bool) {
        let mut o = 0;
        for p in 0..3 {
            let g = self.geometry[p];
            let at = self.at(p, APRON - 1, col * g.size);
            let plane = &mut self.planes[p];
            plane[at..at + g.size].copy_from_slice(&edge[o..o + g.size]);
            if last {
                let end = at + g.size;
                let v = plane[end - 1];
                plane[end..end + 4].fill(v);
            }
            o += g.size;
        }
    }

    /// Macroblock `col`'s last [`APRON`] rows in each plane.
    fn tail(&self, col: usize) -> [u8; TAIL] {
        let mut out = [0; TAIL];
        let mut o = 0;
        for (p, g) in self.geometry.iter().enumerate() {
            for row in g.size..g.size + APRON {
                let at = self.at(p, row, col * g.size);
                out[o..o + g.size].copy_from_slice(&self.planes[p][at..at + g.size]);
                o += g.size;
            }
        }
        out
    }

    /// Put macroblock `col` of the row above's last rows above the band's
    /// own.
    fn put_tail(&mut self, col: usize, tail: &[u8; TAIL]) {
        let mut o = 0;
        for p in 0..3 {
            let g = self.geometry[p];
            for row in 0..APRON {
                let at = self.at(p, row, col * g.size);
                self.planes[p][at..at + g.size].copy_from_slice(&tail[o..o + g.size]);
                o += g.size;
            }
        }
    }

    /// Copy the band's rows that are final into its row's `pieces` of the
    /// frame, and out over the left and right borders, as libvpx's
    /// `yv12_extend_frame_left_right_c`: the rows above (but above the first
    /// row, which is border) and the row's own but its last four (which the
    /// row below's piece takes), or all of them in the `last` row.
    fn write_back(&self, mb_row: usize, last: bool, pieces: &mut [&mut [u8]; 3]) -> Option<()> {
        for ((band, g), piece) in self
            .planes
            .iter()
            .zip(&self.geometry)
            .zip(pieces.iter_mut())
        {
            let first = if mb_row == 0 { APRON } else { 0 };
            let end = if last { APRON + g.size } else { g.size };
            // A piece starts four rows above its row; the first, at the
            // top of the plane's border.
            let offset = if mb_row == 0 { g.border - APRON } else { 0 };
            for row in first..end {
                let from = row * g.stride + g.border;
                let to = (row + offset) * g.stride;
                let out = piece.get_mut(to..to + 2 * g.border + g.width)?;
                let (left, rest) = out.split_at_mut(g.border);
                let (picture, right) = rest.split_at_mut(g.width);
                picture.copy_from_slice(band.get(from..from + g.width)?);
                left.fill(*picture.first()?);
                right.fill(*picture.last()?);
            }
        }
        Some(())
    }
}
