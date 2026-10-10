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
//! Each thread decodes a row into a private band -- the row, with four rows
//! of the row above (the loop filter's reach) on top -- and the frame is cut
//! into pieces, one per row, each written by one thread: the row's own
//! pixels but its last four, and the last four of the row above, which it
//! holds the last word on. A thread copies its band into its piece as it
//! finishes each row. What crosses from a row to the row below -- each
//! macroblock's token contexts and unfiltered bottom row, and its last four
//! rows once final -- goes through a mailbox of atomic words, published by
//! a counter of the row's progress, which the row below waits on: libvpx's
//! `mt_current_mb_col`, without its shared frame. No lock is taken per
//! macroblock, and nothing is allocated.
//!
//! One macroblock cannot be decoded in a band: the version-3 macroblock
//! whose chroma libvpx leaves as its buffer held it before the frame (see
//! `inter`), since a band holds nothing of that. A thread meeting one stops
//! the others, and the frame decodes again on one thread. No piece of the
//! frame holding such a macroblock's pixels has been written by then -- a
//! piece is written only when its row and the row above it are finished,
//! and a finished row has none -- so the single thread finds the buffer as
//! the frame found it. Any other failure (a thread that cannot start) falls
//! back the same way; nothing else in the decoder's state has changed.
//!
//! Translated into Rust, with its design changed for threads that share no
//! pixels, from libvpx v1.17.0's `vp8/decoder/threading.c` (copyright the
//! WebM project authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::indexing_slicing,
    reason = "band positions are rows and columns inside a band sized for a macroblock row of the frame, and mailbox words a macroblock's slot within a mailbox sized for the row; geometry is checked against the frame before any thread starts"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "positions and sizes are bounded by the frame's (at most 16384 pixels a side, plus the border); row and column counts are below 1024, and progress counts below their product"
)]

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread::Thread;
use std::time::Duration;

use crate::band::{APRON, Band, EDGE, Geometry, TAIL};
use crate::boolread::BoolDecoder;
use crate::decodeframe::{MbAt, Refs, Residual, Rows, decode_macroblock};
use crate::frame::Frame;
use crate::inter::Prior;
use crate::loopfilter;
use crate::modes::{ModeGrid, ModeInfo};
use crate::tokens::Context;

/// A macroblock's slot in a mailbox, in words: its token contexts, padded
/// to two words; its bottom row; its last rows.
const CONTEXT_WORDS: usize = 2;
const EDGE_WORDS: usize = EDGE / 8;
const TAIL_WORDS: usize = TAIL / 8;
const SLOT: usize = CONTEXT_WORDS + EDGE_WORDS + TAIL_WORDS;

#[cfg(test)]
thread_local! {
    /// On the thread decoding: how many frames have decoded on several
    /// threads, and how many began to and fell back to one -- for tests to
    /// see which path their frames took.
    pub(crate) static TALLY: core::cell::Cell<(usize, usize)> =
        const { core::cell::Cell::new((0, 0)) };
}

/// What a row publishes for the row below as it goes, macroblock by
/// macroblock: libvpx's `mt_yabove_row` and `mt_current_mb_col`. Rows take
/// turns at the mailboxes, row `r` writing mailbox `r % count` -- one more
/// mailbox than threads, so a row never writes one the row below the last
/// row to write it is still reading.
struct Mailbox {
    /// How far the row writing the mailbox has got, as
    /// [`Mailbox::count`]`(row, steps)`: after step `c` (macroblock `c`
    /// decoded, and `c - 1` filtered) `c + 1` steps, and `cols + 1` once the
    /// row is filtered. Written last, with release ordering, so that what a
    /// count promises has arrived when a reader acquires it.
    progress: AtomicUsize,
    /// Per macroblock, [`SLOT`] words: its token contexts and bottom row,
    /// written at its step, and its last rows, written two steps later (or at
    /// the end, for the last two). Each is written once a row, before
    /// `progress` says so, and read only after; relaxed atomics, never a
    /// torn or racing access, cost no more than plain loads and stores.
    words: Vec<AtomicU64>,
}

impl Mailbox {
    fn new(mb_cols: usize) -> Self {
        Self {
            progress: AtomicUsize::new(0),
            words: (0..mb_cols * SLOT).map(|_| AtomicU64::new(0)).collect(),
        }
    }

    /// Progress `steps` of row `mb_row`, in a frame `mb_cols` wide: a row's
    /// counts start past every count of the rows before it, so a count left
    /// by an earlier row never satisfies a reader of a later one.
    fn count(mb_row: usize, mb_cols: usize, steps: usize) -> usize {
        mb_row * (mb_cols + 2) + steps
    }

    /// Macroblock `col`'s slot.
    fn slot(&self, col: usize) -> &[AtomicU64] {
        &self.words[col * SLOT..(col + 1) * SLOT]
    }
}

/// Store `bytes`, a whole number of words, in `words`.
fn store(words: &[AtomicU64], bytes: &[u8]) {
    for (w, chunk) in words.iter().zip(bytes.chunks_exact(8)) {
        let mut word = [0; 8];
        word.copy_from_slice(chunk);
        w.store(u64::from_le_bytes(word), Ordering::Relaxed);
    }
}

/// Load `words` into `bytes`, a whole number of words.
fn load(words: &[AtomicU64], bytes: &mut [u8]) {
    for (w, chunk) in words.iter().zip(bytes.chunks_exact_mut(8)) {
        chunk.copy_from_slice(&w.load(Ordering::Relaxed).to_le_bytes());
    }
}

/// A thread that may fall asleep waiting for the row above, and how to wake
/// it.
struct Sleeper {
    /// Set, with sequentially consistent ordering, before the thread looks
    /// at the row above's progress a last time and sleeps; and read so by a
    /// row publishing progress after it. One of the two sees the other's
    /// write, so either the sleeper sees the progress or the row wakes it.
    asleep: AtomicBool,
    /// The thread, which it records when it starts.
    thread: OnceLock<Thread>,
}

impl Sleeper {
    fn wake(&self) {
        if let Some(thread) = self.thread.get() {
            thread.unpark();
        }
    }
}

/// What every thread reads, and the mailboxes and stop flag they share.
struct Shared<'s, 'd> {
    rows: &'s Rows<'s>,
    grid: &'s ModeGrid,
    refs: &'s Refs<'s>,
    /// The token partitions, each held by the one row reading it.
    partitions: &'s [Mutex<BoolDecoder<'d>>],
    geometry: [Geometry; 3],
    mailboxes: &'s [Mailbox],
    /// By thread: thread `t` takes rows `t`, `t + n`, ..., so the reader
    /// of row `r`'s mailbox is thread `(r + 1) % n`.
    sleepers: &'s [Sleeper],
    /// Set by a thread that cannot go on, and by this thread when one
    /// cannot start: every thread waiting on a row above gives up.
    stop: &'s AtomicBool,
}

impl Shared<'_, '_> {
    /// The mailbox row `mb_row` writes and the row below it reads.
    fn mailbox(&self, mb_row: usize) -> Option<&Mailbox> {
        self.mailboxes.get(mb_row % self.mailboxes.len().max(1))
    }

    /// The thread that decodes row `mb_row`.
    fn sleeper(&self, mb_row: usize) -> Option<&Sleeper> {
        self.sleepers.get(mb_row % self.sleepers.len().max(1))
    }

    /// Record that row `mb_row`, writing `mailbox`, has made `steps` steps,
    /// and wake the thread of the row below if it sleeps waiting for it.
    fn publish(&self, mailbox: &Mailbox, mb_row: usize, steps: usize) {
        let count = Mailbox::count(mb_row, self.rows.mb_cols, steps);
        mailbox.progress.store(count, Ordering::SeqCst);
        if let Some(reader) = self.sleeper(mb_row + 1) {
            if reader.asleep.load(Ordering::SeqCst) {
                reader.wake();
            }
        }
    }

    /// Tell every thread to stop, waking those asleep.
    fn halt(&self) {
        self.stop.store(true, Ordering::SeqCst);
        for sleeper in self.sleepers {
            sleeper.wake();
        }
    }
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
    threading: Threading,
) -> Option<bool> {
    let (mb_rows, mb_cols) = (rows.mb_rows, rows.mb_cols);
    let per_thread = (mb_rows * mb_cols)
        .checked_div(threading.min_macroblocks)
        .unwrap_or(usize::MAX);
    let workers = threading
        .threads
        .min(partitions.len())
        .min(mb_rows)
        .min(per_thread);
    if workers < 2 || mb_cols == 0 || above.len() != mb_cols {
        return None;
    }
    let geometry = Geometry::of(new, mb_rows, mb_cols)?;

    // Fresh copies: should the threads fail, the single thread starts the
    // partitions from their beginnings.
    let decoders: Vec<Mutex<BoolDecoder<'_>>> =
        partitions.iter().cloned().map(Mutex::new).collect();
    let mailboxes: Vec<Mailbox> = (0..=workers).map(|_| Mailbox::new(mb_cols)).collect();
    let sleepers: Vec<Sleeper> = (0..workers)
        .map(|_| Sleeper {
            asleep: AtomicBool::new(false),
            thread: OnceLock::new(),
        })
        .collect();
    let stop = AtomicBool::new(false);
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
            mailboxes: &mailboxes,
            sleepers: &sleepers,
            stop: &stop,
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

/// Run `jobs[t]` on thread `t`, this thread taking `jobs[0]`. Each
/// thread's outcome: `None` from one that stopped or could not start.
fn run_threads(shared: &Shared<'_, '_>, jobs: Vec<Vec<Job<'_>>>) -> Vec<Option<bool>> {
    let workers = jobs.len();
    std::thread::scope(|scope| {
        let mut jobs = jobs.into_iter();
        let first = jobs.next();
        let mut handles = Vec::with_capacity(workers);
        for (t, work) in jobs.enumerate().map(|(i, work)| (i + 1, work)) {
            let thread =
                std::thread::Builder::new().spawn_scoped(scope, move || run(shared, t, work));
            match thread {
                Ok(h) => handles.push(h),
                Err(_) => {
                    // The rows it was given will never come: the threads
                    // waiting on them must not wait for ever.
                    shared.halt();
                    break;
                }
            }
        }
        let mine = if shared.stop.load(Ordering::SeqCst) {
            None
        } else {
            first.and_then(|work| run(shared, 0, work))
        };
        let mut outcomes = Vec::with_capacity(workers);
        outcomes.push(mine);
        outcomes.extend(handles.into_iter().map(|h| h.join().ok().flatten()));
        outcomes
    })
}

/// Stops the other threads when dropped unfinished: a thread that returns
/// early, or panics (in a test build, which unwinds), stops the threads
/// waiting on its rows rather than leaving them waiting for ever.
struct Stopper<'s, 'a, 'd> {
    shared: &'a Shared<'s, 'd>,
    finished: bool,
}

impl Drop for Stopper<'_, '_, '_> {
    fn drop(&mut self) {
        if !self.finished {
            self.shared.halt();
        }
    }
}

/// How many times a thread waiting for the row above spins before it
/// sleeps: about a microsecond. A row is rarely more than a macroblock or
/// two from what the row below needs, so a short spin often saves a sleep
/// and a wake-up. libvpx's threads spin and yield for ever
/// (`vp8_atomic_spin_wait`), which starves the row they wait on when there
/// are more threads than free cores; and a yield is worse than a sleep on a
/// busy machine: Windows' `SwitchToThread` hands the core to any other
/// ready thread for the rest of its time slice, which cost this decoder 35
/// ms a frame against 0.4 on one thread while it yielded.
const SPINS: u32 = 20;

/// The fewest macroblocks a thread is given a frame's worth of, on
/// average: fewer, and starting the thread and filling the wavefront cost
/// more than the thread saves. Measured on 8-partition film (an i7-8700K):
/// 320x180, 240 macroblocks, was slower on any number of threads than on
/// one; 480x270, 510, gained 10% on three or four and lost on eight;
/// 640x360, 920, gained most on six. A decoder's own is
/// [`Threading::min_macroblocks`].
pub(crate) const MIN_MACROBLOCKS_PER_THREAD: usize = 150;

/// How many threads a frame's rows may decode on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Threading {
    /// At most this many...
    pub(crate) threads: usize,
    /// ... and at most one per this many macroblocks (0: no limit).
    pub(crate) min_macroblocks: usize,
}

/// Wait, as the thread of row `me`, until the row writing `mailbox` as
/// `mb_row` has made `steps` steps, returning how many it has made; `None`
/// if the threads are stopping.
fn wait(
    shared: &Shared<'_, '_>,
    mailbox: &Mailbox,
    (mb_row, steps): (usize, usize),
    me: &Sleeper,
) -> Option<usize> {
    let start = Mailbox::count(mb_row, shared.rows.mb_cols, 0);
    let mut tries = 0u32;
    loop {
        let now = mailbox.progress.load(Ordering::Acquire);
        if now >= start + steps {
            return Some(now - start);
        }
        if shared.stop.load(Ordering::Relaxed) {
            return None;
        }
        if tries < SPINS {
            std::hint::spin_loop();
        } else {
            me.asleep.store(true, Ordering::SeqCst);
            let now = mailbox.progress.load(Ordering::SeqCst);
            if now < start + steps && !shared.stop.load(Ordering::SeqCst) {
                // Woken by the row above's next step, by a thread stopping,
                // or now and then by nothing: each is looked at again. The
                // timeout only bounds a wait whose wake-up went to a sleep
                // that had already ended.
                std::thread::park_timeout(Duration::from_millis(1));
            }
            me.asleep.store(false, Ordering::SeqCst);
        }
        tries = tries.saturating_add(1);
    }
}

/// The steps the row above must have made before its macroblock `col`'s
/// last rows are final: two steps on, or the end of the row for its last
/// two.
fn tail_steps(col: usize, cols: usize) -> usize {
    if col + 2 < cols { col + 3 } else { cols + 1 }
}

/// Decode `jobs`, the rows of thread `t`, in order. Whether any macroblock
/// decoded was corrupt; `None` if the thread stopped.
fn run(shared: &Shared<'_, '_>, t: usize, jobs: Vec<Job<'_>>) -> Option<bool> {
    let mut stopper = Stopper {
        shared,
        finished: false,
    };
    let me = shared.sleepers.get(t)?;
    // Recorded before this thread can sleep, so that a row waking it finds
    // it. Set once: a thread runs once a frame.
    let _ = me.thread.set(std::thread::current());
    let rows = shared.rows;
    let cols = rows.mb_cols;
    let mut band = Band::new(shared.geometry);
    let mut above: Vec<Context> = vec![[0; 9]; cols];
    let mut residual = Residual::new();
    let mut corrupted = false;
    for mut job in jobs {
        let r = job.mb_row;
        // Where this row publishes, if a row below reads it.
        let outbox = if r + 1 < rows.mb_rows {
            Some(shared.mailbox(r)?)
        } else {
            None
        };
        // Where the row above publishes, and how far it is known to be.
        let mut inbox = match r.checked_sub(1) {
            Some(above_row) => Some(Inbox {
                mailbox: shared.mailbox(above_row)?,
                mb_row: above_row,
                steps: 0,
                edges: 0,
                me,
            }),
            None => None,
        };
        band.prepare(r);
        if r == 0 {
            above.fill([0; 9]);
        }
        let mut left: Context = [0; 9];
        let partition = shared.partitions.get(r % shared.partitions.len())?;
        let mut partition = Some(partition.lock().ok()?);
        // The macroblock to the left, decoded and waiting to be filtered.
        let mut pending: Option<ModeInfo> = None;
        for c in 0..cols {
            if let Some(inbox) = &mut inbox {
                // The row above's macroblocks up to the one above and to
                // the right: their bottom rows and token contexts.
                inbox.take_edges((c + 2).min(cols), shared, &mut band, &mut above)?;
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
            if c + 1 == cols {
                // The partition is the next row's to read it now.
                partition = None;
            }
            *job.skips.get_mut(c)? = mi.mb_skip_coeff;
            if let Some(outbox) = outbox {
                let slot = outbox.slot(c);
                let mut context = [0u8; CONTEXT_WORDS * 8];
                context[..9].copy_from_slice(ctx);
                store(&slot[..CONTEXT_WORDS], &context);
                store(
                    &slot[CONTEXT_WORDS..CONTEXT_WORDS + EDGE_WORDS],
                    &band.edge(c),
                );
            }
            if let Some(left_mi) = pending.replace(mi) {
                finish(shared, &mut band, inbox.as_mut(), (r, c - 1), &left_mi)?;
            }
            if let Some(outbox) = outbox {
                if c >= 2 {
                    store_tail(outbox, c - 2, &band);
                }
                shared.publish(outbox, r, c + 1);
            }
        }
        let last_mi = pending?;
        finish(shared, &mut band, inbox.as_mut(), (r, cols - 1), &last_mi)?;
        if let Some(outbox) = outbox {
            for c in cols.saturating_sub(2)..cols {
                store_tail(outbox, c, &band);
            }
            shared.publish(outbox, r, cols + 1);
        }
        band.write_back(r, outbox.is_none(), &mut job.pieces)?;
        if let Some(out) = job.above.take() {
            out.copy_from_slice(above.get(..out.len())?);
        }
    }
    stopper.finished = true;
    Some(corrupted)
}

/// Publish macroblock `col`'s last rows, final.
fn store_tail(outbox: &Mailbox, col: usize, band: &Band) {
    store(
        &outbox.slot(col)[CONTEXT_WORDS + EDGE_WORDS..],
        &band.tail(col),
    );
}

/// The row above, as the row below reads it.
struct Inbox<'m> {
    mailbox: &'m Mailbox,
    mb_row: usize,
    /// How many steps the row above is known to have made.
    steps: usize,
    /// How many of its macroblocks' bottom rows are above the band.
    edges: usize,
    /// The thread reading, should it sleep.
    me: &'m Sleeper,
}

impl Inbox<'_> {
    /// Wait for the row above to make `steps` steps.
    fn reach(&mut self, steps: usize, shared: &Shared<'_, '_>) -> Option<()> {
        if self.steps < steps {
            self.steps = wait(shared, self.mailbox, (self.mb_row, steps), self.me)?;
        }
        Some(())
    }

    /// Put the bottom rows of the row above's first `count` macroblocks
    /// above the band, and their token contexts in `above`.
    fn take_edges(
        &mut self,
        count: usize,
        shared: &Shared<'_, '_>,
        band: &mut Band,
        above: &mut [Context],
    ) -> Option<()> {
        self.reach(count, shared)?;
        let cols = above.len();
        while self.edges < count {
            let c = self.edges;
            let slot = self.mailbox.slot(c);
            let mut context = [0u8; CONTEXT_WORDS * 8];
            load(&slot[..CONTEXT_WORDS], &mut context);
            above.get_mut(c)?.copy_from_slice(&context[..9]);
            let mut edge = [0u8; EDGE];
            load(&slot[CONTEXT_WORDS..CONTEXT_WORDS + EDGE_WORDS], &mut edge);
            band.put_edge(c, &edge, c + 1 == cols);
            self.edges += 1;
        }
        Some(())
    }

    /// Put the row above's macroblock `col`'s last rows above the band,
    /// once final. They go in only now, just before the macroblock below is
    /// filtered: the one to its right has been predicted from the unfiltered
    /// bottom row they replace.
    fn take_tail(&mut self, col: usize, shared: &Shared<'_, '_>, band: &mut Band) -> Option<()> {
        self.reach(tail_steps(col, shared.rows.mb_cols), shared)?;
        let mut tail = [0u8; TAIL];
        load(
            &self.mailbox.slot(col)[CONTEXT_WORDS + EDGE_WORDS..],
            &mut tail,
        );
        band.put_tail(col, &tail);
        Some(())
    }
}

/// Filter macroblock `(r, c)`, whose modes are `mi`, once the row above's
/// last rows over it are in place: libvpx's threads filter each macroblock
/// as they go.
fn finish(
    shared: &Shared<'_, '_>,
    band: &mut Band,
    inbox: Option<&mut Inbox<'_>>,
    (r, c): (usize, usize),
    mi: &ModeInfo,
) -> Option<()> {
    if let Some(inbox) = inbox {
        inbox.take_tail(c, shared, band)?;
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
