// Ring indices here are reduced modulo `nr_events`, which is never zero (a
// ring holds at least 511 events), and the counts beside them -- requests
// available, completions, references -- are bounded by `nr_events` or by the
// number of callers.  Every count that comes from the caller is checked or
// clamped before it meets one of them.  Clippy cannot see the bounds.
#![allow(clippy::arithmetic_side_effects)]

//! `<linux/aio_abi.h>` — Linux's kernel asynchronous I/O: `io_setup`,
//! `io_destroy`, `io_submit`, `io_cancel`, `io_getevents` and
//! `io_pgetevents`, as Linux 6.6's fs/aio.c has them, inside one process.
//!
//! ## How a program reaches it
//!
//! glibc wraps none of these calls, and neither does this library
//! (design-decisions §1114).  A C program reaches them as it does on Linux:
//! through `syscall(SYS_io_setup, …)` — which [`crate::sys_syscall::syscall`]
//! routes here — or through libaio, which makes exactly those calls.  The
//! functions here are the system calls' bodies and answer `Result`s; the
//! table turns them into `syscall()`'s -1 and `errno`.
//!
//! ## What a context is
//!
//! A context's id is the address of its ring, as on Linux: Linux 6.6's
//! `struct aio_ring` — a 32-byte header, then `nr` 32-byte `io_event`s — in
//! memory the caller can read.  libaio reads the header to decide whether it
//! may take events without a system call, and some programs reap the ring
//! themselves by advancing `head`; both work here.  The ring is sized as
//! `aio_setup_ring` sizes it, in this system's 16 KiB pages, and how many
//! requests may be outstanding is `reqs_available`'s accounting, not a fixed
//! pool.
//!
//! ## What happens to a request
//!
//! Every request runs to completion inside `io_submit`, through the ordinary
//! calls (`preadv2`, `pwritev2`, `fsync`, ...), and its result is posted to
//! the ring before `io_submit` moves on to the next — which is what Linux
//! does for buffered I/O, the only kind this system has.  Everything Linux
//! refuses at submission is refused at submission here, with Linux's errno,
//! in `__io_submit_one`'s order; only what the transfer itself answers is an
//! event.  Nothing is ever in flight to be cancelled, so `io_cancel` finds
//! nothing, as Linux's does for anything but a poll.
//!
//! `io_getevents` waits as Linux's does — for `min_nr` events, for the
//! timeout (relative, on the monotonic clock), or for `io_destroy` — on a
//! futex every completion advances, so another thread's `io_submit` wakes
//! it.  `io_destroy` waits for the calls still inside the context, as
//! Linux's waits for its requests, before the ring goes.
//!
//! ## Where it differs from Linux
//!
//! - `IOCB_CMD_POLL` is refused with `EINVAL` — fs/aio.c's own answer, "same
//!   as no support for IOCB_CMD_POLL": a poll that stays pending needs
//!   something to complete it while the program is not in a call here
//!   (`B-D-AIO-HAS-NO-POLL`).
//! - `aio-max-nr` (65536) limits this process's contexts, not the system's.
//! - A descriptor's access mode is judged by the transfer, not before it, as
//!   in the `readv` family (see [`crate::file`]'s `vectored`).
//! - `RWF_NOWAIT` is the `EAGAIN` of a transfer not attempted (see
//!   [`crate::file::plan_rw_flags`]).
//! - An address the caller passes is refused only when no mapping could
//!   have it (NULL where Linux reads, and what `access_ok` rejects); any
//!   other bad pointer faults, as in any C library.
//!
//! Until 2026-09-26 this was a pool of eight contexts of at most 256 events
//! whose ids were 1 to 8 — which libaio's `io_getevents` dereferences — every
//! refusal Linux makes at submission was an event instead, `io_getevents`
//! ignored its timeout and answered `EAGAIN` where Linux waits, a request
//! blocked in its transfer held a lock every other AIO call spun on, and the
//! functions were exported under libaio's names with `syscall()`'s
//! convention (`B-D-AIO-WAS-NOT-LINUXS`).

use crate::errno;
use crate::fdtable::{self, HandleKind};
use crate::file::{Iovec, RwPlan, has_iter_ops, plan_rw_flags};
use crate::objtable::{Slots, Waits};
use crate::perprocess::process_global;
use crate::stat::Timespec;
use core::sync::atomic::{AtomicU32, Ordering};

// ---------------------------------------------------------------------------
// AIO commands (iocb.aio_lio_opcode)
// ---------------------------------------------------------------------------

/// Read operation.
pub const IOCB_CMD_PREAD: u16 = 0;
/// Write operation.
pub const IOCB_CMD_PWRITE: u16 = 1;
/// fsync.
pub const IOCB_CMD_FSYNC: u16 = 2;
/// fdatasync.
pub const IOCB_CMD_FDSYNC: u16 = 3;
/// Poll a descriptor for readiness — refused here with `EINVAL` (see the
/// module docs).
pub const IOCB_CMD_POLL: u16 = 5;
/// No operation.  Linux 6.6 has no case for it: `EINVAL`.
pub const IOCB_CMD_NOOP: u16 = 6;
/// Vectored read.
pub const IOCB_CMD_PREADV: u16 = 7;
/// Vectored write.
pub const IOCB_CMD_PWRITEV: u16 = 8;

// ---------------------------------------------------------------------------
// AIO flags (iocb.aio_flags)
// ---------------------------------------------------------------------------

/// Set if using eventfd for notification.
pub const IOCB_FLAG_RESFD: u32 = 1 << 0;
/// Set if `aio_reqprio` is an I/O priority.
pub const IOCB_FLAG_IOPRIO: u32 = 1 << 1;

/// `KIOCB_KEY`: what `io_submit` writes into each iocb's `aio_key`, and what
/// `io_cancel` expects to find there.
const KIOCB_KEY: u32 = 0;

/// `struct aio_ring`'s `magic`.
pub const AIO_RING_MAGIC: u32 = 0xa10a_10a1;
/// `AIO_RING_COMPAT_FEATURES`.
const AIO_RING_COMPAT_FEATURES: u32 = 1;
/// `AIO_RING_INCOMPAT_FEATURES`: none, so a reader may consume the ring.
const AIO_RING_INCOMPAT_FEATURES: u32 = 0;

/// `/proc/sys/fs/aio-max-nr`'s default: the most events the contexts may
/// ask for between them.
const AIO_MAX_NR: u64 = 0x10000;

// ---------------------------------------------------------------------------
// I/O control block and event
// ---------------------------------------------------------------------------

/// Kernel AIO I/O control block (64 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Iocb {
    /// Data returned in `io_event`.
    pub aio_data: u64,
    /// Written `KIOCB_KEY` (0) by `io_submit`.
    pub aio_key: u32,
    /// Per-I/O `RWF_*` flags.
    pub aio_rw_flags: u32,
    /// Operation (`IOCB_CMD_*`).
    pub aio_lio_opcode: u16,
    /// Request priority, when `IOCB_FLAG_IOPRIO` is set.
    pub aio_reqprio: i16,
    /// File descriptor.
    pub aio_fildes: u32,
    /// Buffer address — or the `iovec` array, for the vectored commands.
    pub aio_buf: u64,
    /// Number of bytes — or of `iovec`s, for the vectored commands.
    pub aio_nbytes: u64,
    /// File offset.
    pub aio_offset: i64,
    /// Reserved: must be 0.
    pub aio_reserved2: u64,
    /// Flags (`IOCB_FLAG_*`).
    pub aio_flags: u32,
    /// eventfd to signal on completion, with `IOCB_FLAG_RESFD`.
    pub aio_resfd: u32,
}

impl Iocb {
    /// Create a zeroed I/O control block.
    #[must_use]
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Completion event (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IoEvent {
    /// Data from `iocb`.
    pub data: u64,
    /// `iocb` address.
    pub obj: u64,
    /// Result (bytes transferred or negative errno).
    pub res: i64,
    /// Secondary result.
    pub res2: i64,
}

impl IoEvent {
    /// Create a zeroed I/O event.
    #[must_use]
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// `struct __aio_sigset`: `io_pgetevents`' signal mask and its size.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AioSigset {
    /// The mask, or NULL for none.
    pub sigmask: *const u64,
    /// `sizeof(sigset_t)`: 8.
    pub sigsetsize: usize,
}

// ---------------------------------------------------------------------------
// The ring
// ---------------------------------------------------------------------------

/// Linux 6.6's `struct aio_ring` (fs/aio.c): the header of the memory a
/// context id names, followed by `nr` events.  The caller may read all of it
/// and write `head`, so every field is an atomic.
#[repr(C)]
struct AioRing {
    /// The context's slot, which a lookup goes through, as Linux's does.
    id: AtomicU32,
    /// How many events the ring holds.
    nr: AtomicU32,
    /// The next event to take: written by the caller or by `io_getevents`.
    head: AtomicU32,
    /// One past the last event posted.
    tail: AtomicU32,
    magic: AtomicU32,
    compat_features: AtomicU32,
    incompat_features: AtomicU32,
    header_length: AtomicU32,
}

/// The header's size; the events follow it.
const RING_HEADER: usize = size_of::<AioRing>();
/// One event's size.
const EVENT_SIZE: usize = size_of::<IoEvent>();
/// The header's size as the ring's `header_length` states it: 32.
#[allow(clippy::cast_possible_truncation)]
const HEADER_LENGTH: u32 = RING_HEADER as u32;

/// `aio_setup_ring`'s geometry for `nr_events` (already doubled, at most
/// 8388608): the bytes the ring takes, in whole pages, and the events that
/// fit in them.
fn ring_geometry(nr_events: u32) -> (usize, u32) {
    // "Compensate for the ring buffer's head/tail overlap entry": +2.
    let n = u64::from(nr_events) + 2;
    let size = RING_HEADER as u64 + EVENT_SIZE as u64 * n;
    let page = crate::unistd::PAGE_SIZE as u64;
    let bytes = size.div_ceil(page) * page;
    let events = (bytes - RING_HEADER as u64) / EVENT_SIZE as u64;
    // At most 2^28 bytes, so both fit.
    (
        usize::try_from(bytes).unwrap_or(usize::MAX),
        u32::try_from(events).unwrap_or(u32::MAX),
    )
}

/// A zeroed ring of `bytes` holding `events` events, its header filled in as
/// `aio_setup_ring` fills it; `None` when memory runs out.
fn alloc_ring(bytes: usize, events: u32) -> Option<*mut AioRing> {
    let p = crate::malloc::aligned_alloc(crate::unistd::PAGE_SIZE, bytes);
    if p.is_null() {
        return None;
    }
    // SAFETY: a fresh block of `bytes`, zeroed as Linux's ring pages are.
    unsafe { p.write_bytes(0, bytes) };
    let ring = p.cast::<AioRing>();
    // SAFETY: the block is page-aligned and holds at least the header.
    let r = unsafe { &*ring };
    r.nr.store(events, Ordering::Relaxed);
    r.id.store(u32::MAX, Ordering::Relaxed);
    r.magic.store(AIO_RING_MAGIC, Ordering::Relaxed);
    r.compat_features
        .store(AIO_RING_COMPAT_FEATURES, Ordering::Relaxed);
    r.incompat_features
        .store(AIO_RING_INCOMPAT_FEATURES, Ordering::Relaxed);
    r.header_length.store(HEADER_LENGTH, Ordering::Release);
    Some(ring)
}

/// Free a ring [`alloc_ring`] made.
fn free_ring(ring: *mut AioRing) {
    // SAFETY: `ring` is the block `alloc_ring` allocated, freed once.
    unsafe { crate::malloc::free(ring.cast::<u8>()) };
}

// ---------------------------------------------------------------------------
// Contexts
// ---------------------------------------------------------------------------

/// One context: Linux's `struct kioctx`, as much of it as a synchronous
/// executor needs.
struct Ctx {
    /// The ring; null while the slot holds no context.
    ring: *mut AioRing,
    /// `nr_events`: the ring's events, the trusted copy of `ring->nr`.
    nr_events: u32,
    /// `max_reqs`: what `io_setup` asked for, counted against `aio-max-nr`.
    max_reqs: u32,
    /// `tail`: where the next completion goes, the trusted copy.
    tail: u32,
    /// `completed_events`: completions not yet given back to
    /// `reqs_available`.
    completed_events: u32,
    /// `reqs_available`: requests that may still be taken.
    reqs_available: u32,
    /// Calls inside the context (the syscall's reference on `users`).  The
    /// ring goes only once this is 0.
    refs: u32,
    /// `dead`: `io_destroy` has begun; lookups fail from here on.
    dead: bool,
}

impl Ctx {
    const EMPTY: Self = Self {
        ring: core::ptr::null_mut(),
        nr_events: 0,
        max_reqs: 0,
        tail: 0,
        completed_events: 0,
        reqs_available: 0,
        refs: 0,
        dead: false,
    };

    fn ring(&self) -> &AioRing {
        // SAFETY: a context with a slot has a live ring until it is freed,
        // which happens only with no reference left.
        unsafe { &*self.ring }
    }

    /// Event `i` of the ring, `i < nr_events`.
    fn event(&self, i: u32) -> *mut IoEvent {
        let at = RING_HEADER + EVENT_SIZE * (i as usize);
        // SAFETY: the ring holds `nr_events` events after its header.
        unsafe { self.ring.cast::<u8>().add(at).cast::<IoEvent>() }
    }

    /// `refill_reqs_available`: give back the requests whose events the
    /// caller has taken.  `head` is the caller's and is clamped; nothing
    /// more than has completed is ever given back.
    fn refill(&mut self, head: u32, tail: u32) {
        let head = head % self.nr_events;
        let in_ring = if head <= tail {
            tail - head
        } else {
            self.nr_events - (head - tail)
        };
        let completed = self.completed_events.saturating_sub(in_ring);
        if completed == 0 {
            return;
        }
        self.completed_events -= completed;
        self.reqs_available += completed;
    }

    /// `get_reqs_available`, with `user_refill_reqs_available` when none is
    /// left: `false` is `aio_get_req`'s `EAGAIN`.
    fn take_req(&mut self) -> bool {
        if self.reqs_available == 0 && self.completed_events > 0 {
            let head = self.ring().head.load(Ordering::Acquire);
            self.refill(head, self.tail);
        }
        if self.reqs_available == 0 {
            return false;
        }
        self.reqs_available -= 1;
        true
    }

    /// `aio_complete`: post `ev` at the tail.  Its request was taken, so
    /// there is room.
    fn post(&mut self, ev: IoEvent) {
        let tail = self.tail;
        // SAFETY: `tail < nr_events`.
        unsafe { self.event(tail).write(ev) };
        let next = if tail + 1 >= self.nr_events {
            0
        } else {
            tail + 1
        };
        self.tail = next;
        let ring = self.ring();
        let head = ring.head.load(Ordering::Acquire);
        // Release: the event is visible before the tail that covers it.
        ring.tail.store(next, Ordering::Release);
        self.completed_events += 1;
        if self.completed_events > 1 {
            self.refill(head, next);
        }
    }

    /// `aio_read_events_ring`: copy up to `nr` events to `out`, from the
    /// head the caller last left.  The count copied, or `-EFAULT` for a
    /// destination that cannot be written — which leaves the ring as it was.
    fn take_events(&self, out: *mut IoEvent, nr: i64) -> i64 {
        let ring = self.ring();
        let head = ring.head.load(Ordering::Acquire);
        let tail = ring.tail.load(Ordering::Acquire);
        if head == tail {
            return 0;
        }
        let mut head = head % self.nr_events;
        let tail = tail % self.nr_events;
        let mut ret: i64 = 0;
        while ret < nr {
            if head == tail {
                break;
            }
            let run = (if head <= tail { tail } else { self.nr_events }) - head;
            let avail = i64::from(run).min(nr - ret);
            // `avail > 0` and `ret < nr`: both fit a usize.
            let (at, n) = (
                usize::try_from(ret).unwrap_or(0),
                usize::try_from(avail).unwrap_or(0),
            );
            let dst = out.wrapping_add(at);
            if dst.is_null() || !crate::uio::access_ok(dst as usize, n * EVENT_SIZE) {
                return -i64::from(errno::EFAULT);
            }
            // SAFETY: `n` events from `head` stay inside the ring (`run` ends
            // at the tail or the ring's end); the destination is the
            // caller's, which it said holds `nr`.
            unsafe { core::ptr::copy_nonoverlapping(self.event(head), dst, n) };
            ret += avail;
            head = (head + u32::try_from(avail).unwrap_or(run)) % self.nr_events;
        }
        self.ring().head.store(head, Ordering::Release);
        ret
    }
}

/// The process's contexts, and what they have asked of `aio-max-nr`.
struct Tables {
    slots: Slots<Ctx>,
    aio_nr: u64,
}

process_global! {
    /// This process's AIO contexts.  Per-thread on the host, for test
    /// isolation.
    fn tables() -> Tables = Tables { slots: Slots::EMPTY, aio_nr: 0 };

    /// Serialises every use of the tables, with the same scope as they have
    /// (see [`crate::perprocess::PoolLock`]).  Never held across a transfer.
    fn aio_lock() -> crate::perprocess::PoolLock = crate::perprocess::PoolLock::new();

    /// What `io_getevents` and `io_destroy` sleep on: advanced by every
    /// completion, destruction and last reference.
    fn waits() -> Waits = Waits::new();
}

fn waits_ref() -> &'static Waits {
    // SAFETY: this process's counter, alive for the rest of it (host: the
    // thread); only shared references are made.
    unsafe { &*waits() }
}

/// Holds the tables' lock; the tables are reached through it.
struct Locked {
    _guard: crate::perprocess::PoolGuard<'static>,
    t: *mut Tables,
}

fn lock() -> Locked {
    Locked {
        // SAFETY: `aio_lock()` is this context's lock, valid as long as the
        // tables it guards.
        _guard: unsafe { crate::perprocess::lock_pool(aio_lock()) },
        t: tables(),
    }
}

impl Locked {
    fn tables(&mut self) -> &mut Tables {
        // SAFETY: the lock is held, and `t` is this context's tables.
        unsafe { &mut *self.t }
    }

    fn ctx(&mut self, slot: usize) -> Option<&mut Ctx> {
        self.tables().slots.get(slot).filter(|c| !c.ring.is_null())
    }

    /// `lookup_ioctx`: the live context whose ring is at `ctx_id`.
    ///
    /// Linux reads `ring->id` at the caller's address and looks that slot
    /// up; a library may read only memory it knows to be a ring, so it finds
    /// the ring by address first — and then still requires `id` to name its
    /// slot, as a ring whose `id` the caller overwrote is not found by
    /// Linux's lookup either.
    fn lookup(&mut self, ctx_id: u64) -> Option<usize> {
        let slots = &mut self.tables().slots;
        for slot in 0..slots.cap() {
            let Some(c) = slots.get(slot) else {
                continue;
            };
            if c.ring.is_null() || c.dead || c.ring.addr() as u64 != ctx_id {
                continue;
            }
            let id = c.ring().id.load(Ordering::Acquire);
            return (id as usize == slot).then_some(slot);
        }
        None
    }

    /// [`Self::lookup`], taking a reference the returned guard gives back.
    fn get(&mut self, ctx_id: u64) -> Option<CtxRef> {
        let slot = self.lookup(ctx_id)?;
        let c = self.ctx(slot)?;
        c.refs += 1;
        Some(CtxRef(slot))
    }
}

/// A reference on a context, given back on drop — never while the tables'
/// lock is held, which dropping takes.
struct CtxRef(usize);

impl Drop for CtxRef {
    fn drop(&mut self) {
        let mut t = lock();
        let last = t.ctx(self.0).is_some_and(|c| {
            c.refs -= 1;
            c.refs == 0 && c.dead
        });
        drop(t);
        if last {
            waits_ref().changed();
        }
    }
}

// ---------------------------------------------------------------------------
// io_setup / io_destroy
// ---------------------------------------------------------------------------

/// `io_setup(nr_events, ctxp)` — create a context able to hold at least
/// `nr_events` events, and write its id to `*ctxp`.
///
/// In `SYSCALL_DEFINE2(io_setup)` and `ioctx_alloc`'s order:
///
/// ```text
///   ctxp NULL                                  -> EFAULT (get_user)
///   *ctxp != 0, or nr_events == 0              -> EINVAL
///   doubled nr_events > 8388608                -> EINVAL ("Prevent overflows")
///   doubled nr_events wraps to 0, or
///   nr_events > aio-max-nr                     -> EAGAIN
///   no memory for the ring                     -> ENOMEM
///   the contexts would pass aio-max-nr         -> EAGAIN
/// ```
///
/// # Safety
///
/// A non-NULL `ctxp` is the caller's readable and writable `aio_context_t`.
pub(crate) unsafe fn sys_io_setup(nr_events: u32, ctxp: *mut u64) -> Result<(), i32> {
    if ctxp.is_null() {
        return Err(errno::EFAULT);
    }
    // SAFETY: the caller's contract.
    let ctx = unsafe { ctxp.read_unaligned() };
    if ctx != 0 || nr_events == 0 {
        return Err(errno::EINVAL);
    }
    let max_reqs = nr_events;
    // At least four per CPU, then doubled "so userspace sees what they
    // expected" -- in 32-bit arithmetic, which can wrap, as upstream's does.
    let cpus = u32::try_from(crate::unistd::get_nprocs_conf())
        .unwrap_or(1)
        .max(1);
    let doubled = nr_events.max(cpus.wrapping_mul(4)).wrapping_mul(2);
    if u64::from(doubled) > 0x1000_0000 / EVENT_SIZE as u64 {
        return Err(errno::EINVAL);
    }
    if doubled == 0 || u64::from(max_reqs) > AIO_MAX_NR {
        return Err(errno::EAGAIN);
    }
    let (bytes, events) = ring_geometry(doubled);
    let ring = alloc_ring(bytes, events).ok_or(errno::ENOMEM)?;
    let mut t = lock();
    let tables = t.tables();
    let total = tables.aio_nr + u64::from(max_reqs);
    if total > AIO_MAX_NR {
        drop(t);
        free_ring(ring);
        return Err(errno::EAGAIN);
    }
    let Some(slot) = tables.slots.claim(AIO_MAX_NR as usize, || Ctx::EMPTY) else {
        drop(t);
        free_ring(ring);
        return Err(errno::ENOMEM);
    };
    tables.aio_nr = total;
    if let Some(c) = tables.slots.get(slot) {
        *c = Ctx {
            ring,
            nr_events: events,
            max_reqs,
            reqs_available: events - 1,
            ..Ctx::EMPTY
        };
    }
    // SAFETY: the ring just allocated.
    unsafe { &*ring }
        .id
        .store(u32::try_from(slot).unwrap_or(u32::MAX), Ordering::Release);
    drop(t);
    // SAFETY: the caller's contract.
    unsafe { ctxp.write_unaligned(ring.addr() as u64) };
    Ok(())
}

/// `io_destroy(ctx)` — end a context: calls blocked in `io_getevents` on it
/// return, and once no call is left inside it, the ring goes.
///
/// `EINVAL` for an id no live context has, including one already being
/// destroyed.
pub(crate) fn sys_io_destroy(ctx_id: u64) -> Result<(), i32> {
    let slot = kill(ctx_id)?;
    release_when_unused(slot, |seen| waits_ref().wait(seen, None));
    Ok(())
}

/// `kill_ioctx`: mark the context dead, so no lookup finds it again, give
/// back its share of `aio-max-nr`, and wake whoever waits on it.
fn kill(ctx_id: u64) -> Result<usize, i32> {
    let mut t = lock();
    let slot = t.lookup(ctx_id).ok_or(errno::EINVAL)?;
    let max_reqs = t.ctx(slot).map_or(0, |c| {
        c.dead = true;
        c.max_reqs
    });
    let tables = t.tables();
    tables.aio_nr = tables.aio_nr.saturating_sub(u64::from(max_reqs));
    drop(t);
    waits_ref().changed();
    Ok(slot)
}

/// Wait, through `wait`, until no call is inside the dead context in
/// `slot`, then free its ring and its slot — `io_destroy`'s
/// `wait_for_completion`.
fn release_when_unused(slot: usize, mut wait: impl FnMut(i32)) {
    loop {
        let mut t = lock();
        let seen = waits_ref().seen();
        let Some(c) = t.ctx(slot) else {
            return;
        };
        if c.refs == 0 {
            let ring = c.ring;
            *c = Ctx::EMPTY;
            t.tables().slots.release(slot);
            drop(t);
            free_ring(ring);
            return;
        }
        drop(t);
        wait(seen);
    }
}

// ---------------------------------------------------------------------------
// io_submit
// ---------------------------------------------------------------------------

/// What a request asks, once `__io_submit_one` has accepted it: the transfer
/// or sync to run, whose answer is the event's `res`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Request {
    /// The descriptor, and its kind.
    pub(crate) fd: i32,
    pub(crate) kind: HandleKind,
    /// The command.
    pub(crate) op: u16,
    /// `aio_buf`: the buffer, or the `iovec` array.
    pub(crate) buf: u64,
    /// The bytes accepted (capped at `MAX_RW_COUNT`), or the `iovec` count.
    pub(crate) len: u64,
    /// `aio_offset`.
    pub(crate) pos: i64,
    /// `aio_rw_flags`, as the transfer passes them on.
    pub(crate) flags: i32,
    /// What the flags asked.
    pub(crate) plan: RwPlan,
}

/// `io_submit(ctx_id, nr, iocbpp)` — run up to `nr` requests.
///
/// The answer is how many were submitted, if any were; otherwise the first
/// one's refusal.  In `SYSCALL_DEFINE3(io_submit)`'s order: `nr < 0` is
/// `EINVAL`, then the context (`EINVAL`), then `nr` is clamped to the ring's
/// size, and each request is judged by `io_submit_one` — see [`submit_one`].
///
/// # Safety
///
/// A non-NULL `iocbpp` points to `nr` readable pointers, each NULL or the
/// caller's readable and writable `struct iocb` with the memory its command
/// names.
pub(crate) unsafe fn sys_io_submit(
    ctx_id: u64,
    nr: i64,
    iocbpp: *const *mut Iocb,
) -> Result<i64, i32> {
    // SAFETY: the caller's contract.
    unsafe { submit(ctx_id, nr, iocbpp, &mut run_request, &mut signal_eventfd) }
}

/// [`sys_io_submit`] with the transfer and the eventfd signal passed in, so
/// the host tests can play them.
///
/// # Safety
///
/// As [`sys_io_submit`].
unsafe fn submit(
    ctx_id: u64,
    nr: i64,
    iocbpp: *const *mut Iocb,
    exec: &mut dyn FnMut(&Request) -> Result<i64, i32>,
    notify: &mut dyn FnMut(i32),
) -> Result<i64, i32> {
    if nr < 0 {
        return Err(errno::EINVAL);
    }
    let (ctx, nr_events) = {
        let mut t = lock();
        let r = t.get(ctx_id).ok_or(errno::EINVAL)?;
        let n = t.ctx(r.0).map_or(0, |c| c.nr_events);
        (r, n)
    };
    let nr = nr.min(i64::from(nr_events));
    let mut done: i64 = 0;
    let mut refusal = Ok(());
    while done < nr {
        if iocbpp.is_null() {
            refusal = Err(errno::EFAULT);
            break;
        }
        // SAFETY: the caller's contract; `done < nr`.
        let user_iocb = unsafe {
            iocbpp
                .add(usize::try_from(done).unwrap_or(0))
                .read_unaligned()
        };
        // SAFETY: the caller's contract.
        if let Err(e) = unsafe { submit_one(ctx.0, user_iocb, exec, notify) } {
            refusal = Err(e);
            break;
        }
        done += 1;
    }
    drop(ctx);
    if done > 0 {
        Ok(done)
    } else {
        refusal.map(|()| 0)
    }
}

/// `io_submit_one`: judge one request, run it, post its event.
///
/// ```text
///   iocb unreadable                           -> EFAULT (copy_from_user)
///   aio_reserved2 != 0                        -> EINVAL
///   (ssize_t)aio_nbytes < 0                   -> EINVAL ("prevent overflows")
///   no request available                      -> EAGAIN (aio_get_req)
///   then __io_submit_one — see [`prepare`]
/// ```
///
/// A request refused after it was taken is given back.
///
/// # Safety
///
/// As [`sys_io_submit`], for `user_iocb`.
unsafe fn submit_one(
    slot: usize,
    user_iocb: *mut Iocb,
    exec: &mut dyn FnMut(&Request) -> Result<i64, i32>,
    notify: &mut dyn FnMut(i32),
) -> Result<(), i32> {
    if user_iocb.is_null() || !crate::uio::access_ok(user_iocb.addr(), size_of::<Iocb>()) {
        return Err(errno::EFAULT);
    }
    // SAFETY: the caller's contract.
    let iocb = unsafe { user_iocb.read_unaligned() };
    if iocb.aio_reserved2 != 0 {
        return Err(errno::EINVAL);
    }
    if iocb.aio_nbytes.cast_signed() < 0 {
        return Err(errno::EINVAL);
    }
    {
        let mut t = lock();
        if !t.ctx(slot).is_some_and(Ctx::take_req) {
            return Err(errno::EAGAIN);
        }
    }
    // SAFETY: the caller's contract.
    let prepared = unsafe { prepare(&iocb, user_iocb) };
    let run = prepared.and_then(|(request, resfd)| Ok((exec(&request)?, resfd)));
    let (res, resfd) = match run {
        Ok(r) => r,
        Err(e) => {
            // `put_reqs_available(ctx, 1)`.
            if let Some(c) = lock().ctx(slot) {
                c.reqs_available += 1;
            }
            return Err(e);
        }
    };
    let ev = IoEvent {
        data: iocb.aio_data,
        obj: user_iocb.addr() as u64,
        res,
        res2: 0,
    };
    if let Some(c) = lock().ctx(slot) {
        c.post(ev);
    }
    // `eventfd_signal`, after the event is in the ring, so the eventfd is
    // never readable before the event it announces.
    if let Some(fd) = resfd {
        notify(fd);
    }
    waits_ref().changed();
    Ok(())
}

/// `__io_submit_one`, up to the transfer: the descriptor, the eventfd, the
/// key, and the command's own checks.  `Ok` is the request to run and the
/// eventfd owed its completion.
///
/// ```text
///   aio_fildes not open, or O_PATH            -> EBADF (fget)
///   RESFD: aio_resfd not open                 -> EBADF
///          not an eventfd                     -> EINVAL
///   aio_key := KIOCB_KEY                         (put_user)
///   PREAD/PWRITE/PREADV/PWRITEV               -> see [`prepare_rw`]
///   FSYNC/FDSYNC                              -> see [`prepare_fsync`]
///   anything else, POLL included              -> EINVAL
/// ```
///
/// # Safety
///
/// `user_iocb` is the caller's writable iocb `iocb` was read from.
unsafe fn prepare(iocb: &Iocb, user_iocb: *mut Iocb) -> Result<(Request, Option<i32>), i32> {
    // `fget` takes an `unsigned int`: past `INT_MAX` is no descriptor.
    let fd = i32::try_from(iocb.aio_fildes).map_err(|_| errno::EBADF)?;
    let entry = open_entry(fd).ok_or(errno::EBADF)?;
    let resfd = if iocb.aio_flags & IOCB_FLAG_RESFD != 0 {
        // `eventfd_ctx_fdget` takes an `int`.
        let rfd = i32::try_from(iocb.aio_resfd).map_err(|_| errno::EBADF)?;
        let e = open_entry(rfd).ok_or(errno::EBADF)?;
        if e.kind != HandleKind::Eventfd {
            return Err(errno::EINVAL);
        }
        Some(rfd)
    } else {
        None
    };
    // SAFETY: the caller's contract.
    unsafe { core::ptr::addr_of_mut!((*user_iocb).aio_key).write_unaligned(KIOCB_KEY) };
    let request = match iocb.aio_lio_opcode {
        IOCB_CMD_PREAD | IOCB_CMD_PREADV | IOCB_CMD_PWRITE | IOCB_CMD_PWRITEV => {
            // SAFETY: the caller's contract, for the vector.
            unsafe { prepare_rw(iocb, fd, entry.kind) }?
        }
        IOCB_CMD_FSYNC | IOCB_CMD_FDSYNC => prepare_fsync(iocb, fd, entry.kind)?,
        // IOCB_CMD_POLL: "same as no support for IOCB_CMD_POLL".
        _ => return Err(errno::EINVAL),
    };
    Ok((request, resfd))
}

/// The descriptor `fdget` would find: open, and not `O_PATH`.
fn open_entry(fd: i32) -> Option<fdtable::FdEntry> {
    fdtable::get_fd(fd).filter(|e| !crate::file::is_path_fd_entry(e))
}

/// `aio_read`/`aio_write`'s checks, in their order.
///
/// ```text
///   IOCB_FLAG_IOPRIO, aio_reqprio refused     -> EPERM/EINVAL (ioprio_check_cap)
///   aio_rw_flags refused                      -> EOPNOTSUPP (kiocb_set_rw_flags)
///   no read_iter/write_iter                   -> EINVAL
///   the buffer (import_single_range) or
///   the vector (__import_iovec, count as an
///   unsigned)                                 -> EFAULT/EINVAL
///   offset < 0, or offset + bytes overflows   -> EINVAL (rw_verify_area)
/// ```
///
/// `FMODE_READ`/`FMODE_WRITE` come between the flags and `read_iter`
/// upstream; here the transfer judges them (see the module docs).
///
/// # Safety
///
/// For a vectored command, `aio_buf` is NULL, refused by `access_ok`, or the
/// caller's readable array of `aio_nbytes` (as an `unsigned`) `iovec`s.
unsafe fn prepare_rw(iocb: &Iocb, fd: i32, kind: HandleKind) -> Result<Request, i32> {
    let op = iocb.aio_lio_opcode;
    let is_write = matches!(op, IOCB_CMD_PWRITE | IOCB_CMD_PWRITEV);
    let vectored = matches!(op, IOCB_CMD_PREADV | IOCB_CMD_PWRITEV);
    if iocb.aio_flags & IOCB_FLAG_IOPRIO != 0 {
        crate::process::ioprio_check_cap(i32::from(iocb.aio_reqprio))?;
    }
    // `__kernel_rwf_t` is an `int`.
    let flags = iocb.aio_rw_flags.cast_signed();
    let plan = plan_rw_flags(flags, is_write, true)?;
    if !has_iter_ops(kind, is_write) {
        return Err(errno::EINVAL);
    }
    let bytes = if vectored {
        // `__import_iovec`'s `unsigned nr_segs`, from `aio_nbytes`.
        #[allow(clippy::cast_possible_truncation)]
        let nr_segs = iocb.aio_nbytes as u32;
        // SAFETY: the caller's contract.
        unsafe { crate::uio::import_iovec(iocb.aio_buf as *const Iovec, nr_segs) }?
    } else {
        let len = usize::try_from(iocb.aio_nbytes).unwrap_or(usize::MAX);
        crate::uio::import_ubuf(usize::try_from(iocb.aio_buf).unwrap_or(usize::MAX), len)?
    };
    let pos = iocb.aio_offset;
    let end = i64::try_from(bytes).ok().and_then(|b| pos.checked_add(b));
    if pos < 0 || end.is_none() {
        return Err(errno::EINVAL);
    }
    let len = if vectored {
        iocb.aio_nbytes & u64::from(u32::MAX)
    } else {
        bytes as u64
    };
    Ok(Request {
        fd,
        kind,
        op,
        buf: iocb.aio_buf,
        len,
        pos,
        flags,
        plan,
    })
}

/// `aio_fsync`'s checks: nothing but the descriptor may be set (`EINVAL`),
/// and the file must have `fsync` — only files here do (`EINVAL`).
fn prepare_fsync(iocb: &Iocb, fd: i32, kind: HandleKind) -> Result<Request, i32> {
    if iocb.aio_buf != 0 || iocb.aio_offset != 0 || iocb.aio_nbytes != 0 || iocb.aio_rw_flags != 0 {
        return Err(errno::EINVAL);
    }
    if kind != HandleKind::File {
        return Err(errno::EINVAL);
    }
    Ok(Request {
        fd,
        kind,
        op: iocb.aio_lio_opcode,
        buf: 0,
        len: 0,
        pos: 0,
        flags: 0,
        plan: RwPlan::PLAIN,
    })
}

/// Run a request with the ordinary calls.  `Ok` is the event's `res` —
/// bytes, or a negative errno; `Err` is a refusal of the submission.
///
/// A file takes the transfer at the request's offset through `preadv2`/
/// `pwritev2`, which honour `RWF_APPEND` and the sync flags as Linux's
/// `read_iter`/`write_iter` do.  A stream — pipe, socket, terminal, eventfd
/// — has no position, and `readv`/`writev` move its bytes, as Linux's
/// `read_iter` ignores `ki_pos` for one.
///
/// A directory is a file here, but has no `read_iter` on Linux, whose
/// `aio_read` refuses it at submission with `EINVAL`.  Nothing tells a
/// directory from a file before the read, and the read of one fails with
/// `EISDIR` having done nothing — so that answer becomes the refusal.
fn run_request(r: &Request) -> Result<i64, i32> {
    use crate::file::{fdatasync, fsync, preadv2, pwritev2, readv, writev};
    let answer = |n: i64| -> i64 {
        if n >= 0 {
            return n;
        }
        let e = errno::get_errno();
        -i64::from(if e == 0 { errno::EIO } else { e })
    };
    match r.op {
        IOCB_CMD_FSYNC => return Ok(answer(i64::from(fsync(r.fd)))),
        IOCB_CMD_FDSYNC => return Ok(answer(i64::from(fdatasync(r.fd)))),
        _ => {}
    }
    if r.plan.nowait {
        return Ok(-i64::from(errno::EAGAIN));
    }
    let is_write = matches!(r.op, IOCB_CMD_PWRITE | IOCB_CMD_PWRITEV);
    let one;
    let (iov, cnt) = if matches!(r.op, IOCB_CMD_PREADV | IOCB_CMD_PWRITEV) {
        (
            r.buf as *const Iovec,
            i32::try_from(r.len).unwrap_or(i32::MAX),
        )
    } else {
        one = Iovec {
            iov_base: r.buf as *mut u8,
            iov_len: usize::try_from(r.len).unwrap_or(usize::MAX),
        };
        (&raw const one, 1)
    };
    let moved = match (r.kind == HandleKind::File, is_write) {
        (true, false) => preadv2(r.fd, iov, cnt, r.pos, r.flags),
        (true, true) => pwritev2(r.fd, iov, cnt, r.pos, r.flags),
        (false, false) => readv(r.fd, iov, cnt),
        (false, true) => writev(r.fd, iov, cnt),
    };
    let res = answer(i64::try_from(moved).unwrap_or(i64::MAX));
    if !is_write && res == -i64::from(errno::EISDIR) {
        return Err(errno::EINVAL);
    }
    Ok(res)
}

/// `eventfd_signal(ctx, 1)`.
fn signal_eventfd(fd: i32) {
    // The event is posted whatever the eventfd does: the request completed,
    // and `io_getevents` returns it.  `prepare` checked the descriptor was an
    // eventfd; one closed since has nobody left to tell.
    let _ = crate::epoll::eventfd_write(fd, 1);
}

// ---------------------------------------------------------------------------
// io_cancel
// ---------------------------------------------------------------------------

/// `io_cancel(ctx_id, iocb, result)` — cancel a request in flight.
///
/// In `SYSCALL_DEFINE3(io_cancel)`'s order: `iocb` unreadable is `EFAULT`,
/// an `aio_key` other than `KIOCB_KEY` is `EINVAL`, an unknown context is
/// `EINVAL` — and then the search of the requests that can be cancelled,
/// which only a poll ever joins.  This executor refuses polls and finishes
/// everything else inside `io_submit`, so the search finds nothing: `EINVAL`.
/// `result` is not used, as upstream no longer uses it.
///
/// # Safety
///
/// A non-NULL `iocb` that `access_ok` admits is the caller's readable iocb.
pub(crate) unsafe fn sys_io_cancel(ctx_id: u64, iocb: *mut Iocb) -> Result<(), i32> {
    let key_at = iocb.wrapping_byte_add(8).cast::<u32>();
    if iocb.is_null() || !crate::uio::access_ok(key_at.addr(), size_of::<u32>()) {
        return Err(errno::EFAULT);
    }
    // SAFETY: the caller's contract.
    let key = unsafe { key_at.read_unaligned() };
    if key != KIOCB_KEY {
        return Err(errno::EINVAL);
    }
    let found = lock().lookup(ctx_id).is_some();
    if !found {
        return Err(errno::EINVAL);
    }
    Err(errno::EINVAL)
}

// ---------------------------------------------------------------------------
// io_getevents / io_pgetevents
// ---------------------------------------------------------------------------

/// `io_getevents(ctx_id, min_nr, nr, events, timeout)` — take at least
/// `min_nr` and at most `nr` events, waiting for them up to `timeout`
/// (relative; NULL is no limit).
///
/// In upstream's order: the timeout is read (and not judged), the context
/// looked up (`EINVAL`), then `0 <= min_nr <= nr` (`EINVAL`).  Then
/// `read_events`: take what is there; answer if that is `min_nr`, or the
/// timeout is `{0, 0}`, or the take failed; otherwise wait and take again
/// until `min_nr` are taken, the timeout passes, or the context is destroyed.
/// The answer is the count taken — possibly fewer than `min_nr` when the
/// time ran out — or, with none taken, `EFAULT` for an `events` that cannot
/// be written and `EINVAL` for a context destroyed meanwhile.
///
/// # Safety
///
/// A non-NULL `timeout` is readable; `events` is NULL, refused by
/// `access_ok`, or holds `nr` events.
pub(crate) unsafe fn sys_io_getevents(
    ctx_id: u64,
    min_nr: i64,
    nr: i64,
    events: *mut IoEvent,
    timeout: *const Timespec,
) -> Result<i64, i32> {
    // SAFETY: the caller's contract.
    unsafe {
        getevents(ctx_id, min_nr, nr, events, timeout, &mut |seen, ns| {
            waits_ref().wait(seen, ns);
        })
    }
}

/// `io_pgetevents`: [`sys_io_getevents`] under a signal mask.
///
/// `usig` is read after the timeout and before the context: a mask with a
/// size other than `sizeof(sigset_t)` (8) is `EINVAL` (`set_user_sigmask`);
/// no mask is no change.  The mask would be the thread's for the wait, and
/// nothing here delivers a signal to a waiting thread, so there is nothing
/// for it to hold off — as with `ppoll` and `epoll_pwait`.
///
/// # Safety
///
/// As [`sys_io_getevents`]; a non-NULL `usig` that `access_ok` admits is
/// readable.
pub(crate) unsafe fn sys_io_pgetevents(
    ctx_id: u64,
    min_nr: i64,
    nr: i64,
    events: *mut IoEvent,
    timeout: *const Timespec,
    usig: *const AioSigset,
) -> Result<i64, i32> {
    if !usig.is_null() {
        if !crate::uio::access_ok(usig.addr(), size_of::<AioSigset>()) {
            return Err(errno::EFAULT);
        }
        // SAFETY: the caller's contract.
        let sig = unsafe { usig.read_unaligned() };
        if !sig.sigmask.is_null() {
            if sig.sigsetsize != size_of::<u64>() {
                return Err(errno::EINVAL);
            }
            if !crate::uio::access_ok(sig.sigmask.addr(), size_of::<u64>()) {
                return Err(errno::EFAULT);
            }
        }
    }
    // SAFETY: the caller's contract.
    unsafe { sys_io_getevents(ctx_id, min_nr, nr, events, timeout) }
}

/// `timespec64_to_ktime`: nanoseconds, `None` for `KTIME_MAX` — no timeout —
/// from `KTIME_SEC_MAX` seconds on, and wrapping below it, as the kernel's
/// arithmetic does.  Nothing is judged: a negative timeout has passed.
fn ktime(ts: Timespec) -> Option<i64> {
    const KTIME_SEC_MAX: i64 = i64::MAX / 1_000_000_000;
    if ts.tv_sec >= KTIME_SEC_MAX {
        return None;
    }
    let ns = ts
        .tv_sec
        .wrapping_mul(1_000_000_000)
        .wrapping_add(ts.tv_nsec);
    (ns != i64::MAX).then_some(ns)
}

/// [`sys_io_getevents`] with the sleep passed in, so the host tests can
/// play the other thread.
///
/// # Safety
///
/// As [`sys_io_getevents`].
unsafe fn getevents(
    ctx_id: u64,
    min_nr: i64,
    nr: i64,
    events: *mut IoEvent,
    timeout: *const Timespec,
    wait: &mut dyn FnMut(i32, Option<u64>),
) -> Result<i64, i32> {
    let until = if timeout.is_null() {
        None
    } else {
        if !crate::uio::access_ok(timeout.addr(), size_of::<Timespec>()) {
            return Err(errno::EFAULT);
        }
        // SAFETY: the caller's contract.
        ktime(unsafe { timeout.read_unaligned() })
    };
    let ctx = lock().get(ctx_id).ok_or(errno::EINVAL)?;
    if !(min_nr <= nr && min_nr >= 0) {
        return Err(errno::EINVAL);
    }
    let got = read_events(ctx.0, min_nr, nr, events, until, wait);
    drop(ctx);
    if got < 0 {
        // `got` is a negated errno, which fits an i32.
        Err(i32::try_from(-got).unwrap_or(errno::EINVAL))
    } else {
        Ok(got)
    }
}

/// `read_events`: take, and wait while that is not yet enough.  Answers
/// the count taken, or a negated errno when none was.
fn read_events(
    slot: usize,
    min_nr: i64,
    nr: i64,
    events: *mut IoEvent,
    until: Option<i64>,
    wait: &mut dyn FnMut(i32, Option<u64>),
) -> i64 {
    let mut got: i64 = 0;
    let (_, mut seen) = take(slot, min_nr, nr, events, &mut got);
    if until == Some(0) || got < 0 || got >= min_nr {
        return got;
    }
    // The timer starts now, relative, on the monotonic clock.
    let now = crate::lowlevellock::now_on(crate::time::CLOCK_MONOTONIC);
    let deadline = until.map(|ns| add_ns(&now, ns));
    loop {
        let left = match deadline {
            None => None,
            Some(at) => {
                let now = crate::lowlevellock::now_on(crate::time::CLOCK_MONOTONIC);
                match crate::lowlevellock::ns_until(&now, &at) {
                    Some(ns) => Some(ns),
                    None => return got,
                }
            }
        };
        wait(seen, left);
        let (done, s) = take(slot, min_nr, nr, events, &mut got);
        seen = s;
        if done {
            return got;
        }
    }
}

/// `now + ns`, saturating.
fn add_ns(now: &Timespec, ns: i64) -> Timespec {
    let sec = now.tv_sec.saturating_add(ns.div_euclid(1_000_000_000));
    let nsec = now.tv_nsec + ns.rem_euclid(1_000_000_000);
    if nsec >= 1_000_000_000 {
        Timespec {
            tv_sec: sec.saturating_add(1),
            tv_nsec: nsec - 1_000_000_000,
        }
    } else {
        Timespec {
            tv_sec: sec,
            tv_nsec: nsec,
        }
    }
}

/// `aio_read_events`: take what is there into `events + *got`, then decide
/// whether the wait is over.  Also answers the count to wait on next, read
/// under the lock before the ring is looked at.
fn take(slot: usize, min_nr: i64, nr: i64, events: *mut IoEvent, got: &mut i64) -> (bool, i32) {
    let mut t = lock();
    let seen = waits_ref().seen();
    let Some(c) = t.ctx(slot) else {
        // Held by our reference: cannot happen.  Treat as destroyed.
        if *got == 0 {
            *got = -i64::from(errno::EINVAL);
        }
        return (true, seen);
    };
    // `*got >= 0` here: a negative one ended the wait.
    let dst = events.wrapping_add(usize::try_from(*got).unwrap_or(0));
    let mut ret = c.take_events(dst, nr - *got);
    if ret > 0 {
        *got += ret;
    }
    if c.dead {
        ret = -i64::from(errno::EINVAL);
    }
    if *got == 0 {
        *got = ret;
    }
    (ret < 0 || *got >= min_nr, seen)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
mod tests {
    use super::*;
    use crate::fcntl::{O_PATH, O_RDONLY, O_RDWR};

    // The context table is per-thread on host builds, so every test starts
    // from an empty table no other test can reach.  The host has no kernel,
    // so the transfer is played by `exec` closures; `run_request` itself is
    // exercised on the target.

    fn setup(nr: u32) -> u64 {
        let mut ctx: u64 = 0;
        // SAFETY: a local context id.
        unsafe { sys_io_setup(nr, &raw mut ctx) }.unwrap();
        assert_ne!(ctx, 0);
        ctx
    }

    fn ring(ctx: u64) -> &'static AioRing {
        // SAFETY: a live context's ring, read while the test holds it.
        unsafe { &*(ctx as *const AioRing) }
    }

    fn fd(kind: HandleKind, status: i32) -> i32 {
        fdtable::alloc_fd_with_flags(kind, 0x7000, status).unwrap()
    }

    fn iocb(op: u16, fd: i32) -> Iocb {
        let mut c = Iocb::zeroed();
        c.aio_lio_opcode = op;
        c.aio_fildes = fd as u32;
        c.aio_data = 0xDA7A;
        c
    }

    /// A sync request on a file: the smallest one `prepare` accepts.
    fn fsync_on(fd: i32) -> Iocb {
        iocb(IOCB_CMD_FSYNC, fd)
    }

    /// Submit `iocbs` with `exec` playing the transfer; the answer and the
    /// eventfds signalled.
    fn submit_with(
        ctx: u64,
        iocbs: &mut [Iocb],
        exec: &mut dyn FnMut(&Request) -> Result<i64, i32>,
    ) -> (Result<i64, i32>, Vec<i32>) {
        let ptrs: Vec<*mut Iocb> = iocbs.iter_mut().map(core::ptr::from_mut).collect();
        let mut notified = Vec::new();
        // SAFETY: `ptrs` holds `iocbs.len()` live iocbs.
        let r = unsafe {
            submit(ctx, ptrs.len() as i64, ptrs.as_ptr(), exec, &mut |fd| {
                notified.push(fd)
            })
        };
        (r, notified)
    }

    fn submit_ok(ctx: u64, iocbs: &mut [Iocb], res: i64) -> Result<i64, i32> {
        submit_with(ctx, iocbs, &mut |_| Ok(res)).0
    }

    fn no_wait(_: i32, _: Option<u64>) {
        panic!("this call must not wait");
    }

    fn take_now(ctx: u64, min_nr: i64, nr: i64, out: *mut IoEvent) -> Result<i64, i32> {
        // SAFETY: `out` is NULL, refused, or holds `nr` events; no timeout.
        unsafe { getevents(ctx, min_nr, nr, out, core::ptr::null(), &mut no_wait) }
    }

    // -- layout --

    #[test]
    fn layouts_are_linuxs() {
        assert_eq!(size_of::<Iocb>(), 64);
        assert_eq!(size_of::<IoEvent>(), 32);
        assert_eq!(RING_HEADER, 32);
        assert_eq!(HEADER_LENGTH, 32);
        assert_eq!(core::mem::offset_of!(Iocb, aio_key), 8);
        assert_eq!(core::mem::offset_of!(Iocb, aio_reserved2), 48);
        assert_eq!(core::mem::offset_of!(Iocb, aio_resfd), 60);
        assert_eq!(core::mem::offset_of!(AioRing, head), 8);
        assert_eq!(core::mem::offset_of!(AioRing, magic), 16);
        assert_eq!(core::mem::offset_of!(AioRing, header_length), 28);
        assert_eq!(size_of::<AioSigset>(), 16);
    }

    #[test]
    fn ring_geometry_is_aio_setup_rings() {
        // 16 KiB pages: a small context gets a whole page of events.
        assert_eq!(ring_geometry(16), (16384, 511));
        // 509 events and the two "for good luck" fill one page exactly.
        assert_eq!(ring_geometry(509), (16384, 511));
        assert_eq!(ring_geometry(510), (32768, 1023));
    }

    // -- io_setup --

    #[test]
    fn io_setup_refusals_in_order() {
        let mut ctx: u64 = 0;
        // SAFETY (each): a local context id, or NULL.
        unsafe {
            assert_eq!(sys_io_setup(8, core::ptr::null_mut()), Err(errno::EFAULT));
            assert_eq!(sys_io_setup(0, &raw mut ctx), Err(errno::EINVAL));
            ctx = 42;
            assert_eq!(sys_io_setup(8, &raw mut ctx), Err(errno::EINVAL), "*ctxp");
            assert_eq!(ctx, 42);
            ctx = 0;
            // Doubled past 8388608: EINVAL, whatever pr_debug says.
            assert_eq!(sys_io_setup(5_000_000, &raw mut ctx), Err(errno::EINVAL));
            assert_eq!(sys_io_setup(u32::MAX, &raw mut ctx), Err(errno::EINVAL));
            // Doubled wraps to 0: EAGAIN.
            assert_eq!(sys_io_setup(0x8000_0000, &raw mut ctx), Err(errno::EAGAIN));
            // More than aio-max-nr on its own: EAGAIN.
            assert_eq!(sys_io_setup(100_000, &raw mut ctx), Err(errno::EAGAIN));
        }
        assert_eq!(ctx, 0, "nothing written on a refusal");
    }

    #[test]
    fn io_setup_writes_the_rings_address_and_fills_its_header() {
        let ctx = setup(8);
        let r = ring(ctx);
        assert_eq!(r.magic.load(Ordering::Relaxed), AIO_RING_MAGIC);
        assert_eq!(r.compat_features.load(Ordering::Relaxed), 1);
        assert_eq!(r.incompat_features.load(Ordering::Relaxed), 0);
        assert_eq!(r.header_length.load(Ordering::Relaxed), 32);
        assert_eq!(r.head.load(Ordering::Relaxed), 0);
        assert_eq!(r.tail.load(Ordering::Relaxed), 0);
        assert!(r.nr.load(Ordering::Relaxed) >= 8 * 2);
        assert_eq!(ctx % crate::unistd::PAGE_SIZE as u64, 0, "page-aligned");
        let other = setup(8);
        assert_ne!(ctx, other);
        assert_eq!(sys_io_destroy(ctx), Ok(()));
        assert_eq!(sys_io_destroy(other), Ok(()));
    }

    #[test]
    fn aio_max_nr_counts_what_was_asked_and_is_given_back() {
        let a = setup(40_000);
        let mut b: u64 = 0;
        // SAFETY: a local context id.
        assert_eq!(
            unsafe { sys_io_setup(40_000, &raw mut b) },
            Err(errno::EAGAIN)
        );
        assert_eq!(sys_io_destroy(a), Ok(()));
        // SAFETY: as above.
        assert_eq!(unsafe { sys_io_setup(40_000, &raw mut b) }, Ok(()));
        assert_eq!(sys_io_destroy(b), Ok(()));
    }

    // -- io_destroy --

    #[test]
    fn io_destroy_refuses_what_is_not_a_live_context() {
        assert_eq!(sys_io_destroy(0), Err(errno::EINVAL));
        assert_eq!(sys_io_destroy(0x1234_5678), Err(errno::EINVAL));
        let ctx = setup(8);
        assert_eq!(sys_io_destroy(ctx), Ok(()));
        assert_eq!(sys_io_destroy(ctx), Err(errno::EINVAL), "twice");
        let mut c = [fsync_on(0)];
        assert_eq!(submit_ok(ctx, &mut c, 0), Err(errno::EINVAL), "io_submit");
    }

    #[test]
    fn a_ring_whose_id_the_caller_overwrote_is_not_found() {
        let ctx = setup(8);
        let id = ring(ctx).id.load(Ordering::Relaxed);
        ring(ctx).id.store(id + 1, Ordering::Relaxed);
        assert_eq!(submit_ok(ctx, &mut [], 0), Err(errno::EINVAL));
        ring(ctx).id.store(id, Ordering::Relaxed);
        assert_eq!(submit_ok(ctx, &mut [], 0), Ok(0));
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    // -- io_submit: the batch --

    #[test]
    fn io_submit_negative_nr_is_einval_before_the_context() {
        let ptrs: [*mut Iocb; 0] = [];
        // SAFETY: nothing is read.
        let r = unsafe { submit(0, -1, ptrs.as_ptr(), &mut |_| Ok(0), &mut |_| {}) };
        assert_eq!(r, Err(errno::EINVAL));
    }

    #[test]
    fn io_submit_null_array_and_null_iocb_fault() {
        let ctx = setup(8);
        // SAFETY: the NULL array is refused, not read.
        let r = unsafe { submit(ctx, 1, core::ptr::null(), &mut |_| Ok(0), &mut |_| {}) };
        assert_eq!(r, Err(errno::EFAULT));
        let ptrs: [*mut Iocb; 1] = [core::ptr::null_mut()];
        // SAFETY: a local array of one NULL.
        let r = unsafe { submit(ctx, 1, ptrs.as_ptr(), &mut |_| Ok(0), &mut |_| {}) };
        assert_eq!(
            r,
            Err(errno::EFAULT),
            "a NULL iocb was EINVAL until 2026-09-26"
        );
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn io_submit_answers_the_count_once_any_went_in() {
        let ctx = setup(8);
        let fd = fd(HandleKind::File, O_RDWR);
        let mut bad = fsync_on(fd);
        bad.aio_reserved2 = 1;
        let mut batch = [fsync_on(fd), bad, fsync_on(fd)];
        assert_eq!(submit_ok(ctx, &mut batch, 0), Ok(1));
        let mut first_bad = [bad];
        assert_eq!(submit_ok(ctx, &mut first_bad, 0), Err(errno::EINVAL));
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn io_submit_clamps_to_the_ring_and_says_eagain_when_it_is_full() {
        let ctx = setup(8);
        let events = ring(ctx).nr.load(Ordering::Relaxed) as usize;
        let fd = fd(HandleKind::File, O_RDWR);
        let mut many = vec![fsync_on(fd); events + 50];
        // `nr` clamps to the ring's size, and the ring holds one fewer.
        assert_eq!(submit_ok(ctx, &mut many, 0), Ok(events as i64 - 1));
        assert_eq!(submit_ok(ctx, &mut many[..1], 0), Err(errno::EAGAIN));
        // The caller reaps ten itself, by moving the head.
        ring(ctx).head.store(10, Ordering::Release);
        assert_eq!(submit_ok(ctx, &mut many[..20], 0), Ok(10));
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn a_refused_request_gives_its_slot_back() {
        let ctx = setup(8);
        let events = ring(ctx).nr.load(Ordering::Relaxed) as usize;
        let fd = fd(HandleKind::File, O_RDWR);
        let mut noop = [iocb(IOCB_CMD_NOOP, fd)];
        for _ in 0..(events + 5) {
            assert_eq!(submit_ok(ctx, &mut noop, 0), Err(errno::EINVAL));
        }
        let mut many = vec![fsync_on(fd); events];
        assert_eq!(submit_ok(ctx, &mut many, 0), Ok(events as i64 - 1));
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    // -- io_submit: one request, in __io_submit_one's order --

    #[test]
    fn the_iocb_itself_reserved2_and_nbytes() {
        let ctx = setup(8);
        let fd = fd(HandleKind::File, O_RDWR);
        let mut c = fsync_on(fd);
        c.aio_reserved2 = 7;
        assert_eq!(submit_ok(ctx, &mut [c], 0), Err(errno::EINVAL));
        let mut c = iocb(IOCB_CMD_PREAD, fd);
        c.aio_nbytes = u64::MAX;
        assert_eq!(
            submit_ok(ctx, &mut [c], 0),
            Err(errno::EINVAL),
            "nbytes < 0"
        );
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn the_descriptor_then_the_eventfd() {
        let ctx = setup(8);
        let file = fd(HandleKind::File, O_RDWR);
        let path = fd(HandleKind::File, O_RDONLY | O_PATH);
        let pipe = fd(HandleKind::Pipe, O_RDONLY);
        let efd = fd(HandleKind::Eventfd, O_RDWR);
        assert_eq!(submit_ok(ctx, &mut [fsync_on(200)], 0), Err(errno::EBADF));
        let mut huge = fsync_on(file);
        huge.aio_fildes = u32::MAX;
        assert_eq!(submit_ok(ctx, &mut [huge], 0), Err(errno::EBADF));
        assert_eq!(
            submit_ok(ctx, &mut [fsync_on(path)], 0),
            Err(errno::EBADF),
            "O_PATH"
        );
        let mut c = fsync_on(file);
        c.aio_flags = IOCB_FLAG_RESFD;
        c.aio_resfd = 201;
        assert_eq!(submit_ok(ctx, &mut [c], 0), Err(errno::EBADF), "not open");
        c.aio_resfd = pipe as u32;
        assert_eq!(
            submit_ok(ctx, &mut [c], 0),
            Err(errno::EINVAL),
            "not eventfd"
        );
        c.aio_resfd = u32::MAX;
        assert_eq!(submit_ok(ctx, &mut [c], 0), Err(errno::EBADF));
        // A bad descriptor outranks a bad eventfd.
        let mut both = c;
        both.aio_fildes = 200;
        assert_eq!(submit_ok(ctx, &mut [both], 0), Err(errno::EBADF));
        c.aio_resfd = efd as u32;
        let tail_at_notify = core::cell::Cell::new(None);
        let mut ptrs = [core::ptr::from_mut(&mut c)];
        // SAFETY: a local array of one live iocb.
        let r = unsafe {
            submit(ctx, 1, ptrs.as_mut_ptr(), &mut |_| Ok(0), &mut |fd| {
                tail_at_notify.set(Some((fd, ring(ctx).tail.load(Ordering::Acquire))));
            })
        };
        assert_eq!(r, Ok(1));
        assert_eq!(
            tail_at_notify.get(),
            Some((efd, 1)),
            "signalled once the event is in the ring"
        );
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn the_key_is_written_after_the_descriptor_and_before_the_command() {
        let ctx = setup(8);
        let file = fd(HandleKind::File, O_RDWR);
        let mut refused_early = fsync_on(200);
        refused_early.aio_key = 0xDEAD;
        let mut refused_late = iocb(IOCB_CMD_NOOP, file);
        refused_late.aio_key = 0xDEAD;
        let mut batch = [refused_early];
        assert_eq!(submit_ok(ctx, &mut batch, 0), Err(errno::EBADF));
        assert_eq!(batch[0].aio_key, 0xDEAD, "EBADF comes before put_user");
        let mut batch = [refused_late];
        assert_eq!(submit_ok(ctx, &mut batch, 0), Err(errno::EINVAL));
        assert_eq!(batch[0].aio_key, KIOCB_KEY);
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn commands_linux_66_has_no_case_for_are_einval_at_submission() {
        let ctx = setup(8);
        let file = fd(HandleKind::File, O_RDWR);
        for op in [4, IOCB_CMD_POLL, IOCB_CMD_NOOP, 9, u16::MAX] {
            let mut ran = false;
            let (r, _) = submit_with(ctx, &mut [iocb(op, file)], &mut |_| {
                ran = true;
                Ok(0)
            });
            assert_eq!(r, Err(errno::EINVAL), "opcode {op}");
            assert!(!ran, "opcode {op} must not run");
        }
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn a_read_or_write_is_judged_as_aio_read_and_aio_write_judge_it() {
        let ctx = setup(8);
        let file = fd(HandleKind::File, O_RDWR);
        let timer = fd(HandleKind::Timerfd, O_RDONLY);
        let efd = fd(HandleKind::Eventfd, O_RDWR);
        let mut buf = [0u8; 8];
        let base = buf.as_mut_ptr() as u64;
        let read = |fd: i32| {
            let mut c = iocb(IOCB_CMD_PREAD, fd);
            c.aio_buf = base;
            c.aio_nbytes = 8;
            c
        };
        // IOPRIO: a class of 4 is EINVAL; best effort at level 7 is fine.
        let mut c = read(file);
        c.aio_flags = IOCB_FLAG_IOPRIO;
        c.aio_reqprio = (4 << 13) as i16;
        assert_eq!(submit_ok(ctx, &mut [c], 0), Err(errno::EINVAL));
        c.aio_reqprio = (2 << 13) | 7;
        assert_eq!(submit_ok(ctx, &mut [c], 8), Ok(1));
        // RWF_*: an unknown bit is EOPNOTSUPP, before the file's operations.
        let mut c = read(timer);
        c.aio_rw_flags = 1 << 20;
        assert_eq!(submit_ok(ctx, &mut [c], 0), Err(errno::EOPNOTSUPP));
        // No read_iter: timerfd.  An eventfd reads, and does not write.
        assert_eq!(submit_ok(ctx, &mut [read(timer)], 0), Err(errno::EINVAL));
        assert_eq!(submit_ok(ctx, &mut [read(efd)], 8), Ok(1));
        let mut w = read(efd);
        w.aio_lio_opcode = IOCB_CMD_PWRITE;
        assert_eq!(submit_ok(ctx, &mut [w], 0), Err(errno::EINVAL));
        // The buffer: import_single_range's access_ok.
        let mut c = read(file);
        c.aio_buf = 1 << 63;
        assert_eq!(submit_ok(ctx, &mut [c], 0), Err(errno::EFAULT));
        // rw_verify_area: a negative offset, and one the transfer overflows.
        let mut c = read(file);
        c.aio_offset = -1;
        assert_eq!(submit_ok(ctx, &mut [c], 0), Err(errno::EINVAL));
        c.aio_offset = i64::MAX;
        assert_eq!(submit_ok(ctx, &mut [c], 0), Err(errno::EINVAL));
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn nowait_is_passed_on_to_the_transfer() {
        let ctx = setup(8);
        let file = fd(HandleKind::File, O_RDWR);
        let mut buf = [0u8; 4];
        let mut c = iocb(IOCB_CMD_PREAD, file);
        c.aio_buf = buf.as_mut_ptr() as u64;
        c.aio_nbytes = 4;
        c.aio_rw_flags = crate::file::RWF_NOWAIT as u32;
        let mut plan = None;
        let (r, _) = submit_with(ctx, &mut [c], &mut |req| {
            plan = Some(req.plan);
            Ok(-i64::from(errno::EAGAIN))
        });
        assert_eq!(
            r,
            Ok(1),
            "a would-block is the event's, not the submission's"
        );
        assert!(plan.unwrap().nowait);
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn the_vectored_commands_import_their_vector() {
        let ctx = setup(8);
        let file = fd(HandleKind::File, O_RDWR);
        let mut a = [0u8; 4];
        let mut b = [0u8; 6];
        let iov = [
            Iovec {
                iov_base: a.as_mut_ptr(),
                iov_len: 4,
            },
            Iovec {
                iov_base: b.as_mut_ptr(),
                iov_len: 6,
            },
        ];
        let mut c = iocb(IOCB_CMD_PREADV, file);
        c.aio_buf = iov.as_ptr() as u64;
        c.aio_nbytes = 2;
        c.aio_offset = 100;
        let mut seen = None;
        let (r, _) = submit_with(ctx, &mut [c], &mut |req| {
            seen = Some((req.op, req.len, req.pos, req.buf));
            Ok(10)
        });
        assert_eq!(r, Ok(1));
        assert_eq!(seen, Some((IOCB_CMD_PREADV, 2, 100, iov.as_ptr() as u64)));
        let mut too_many = c;
        too_many.aio_nbytes = 1025;
        assert_eq!(submit_ok(ctx, &mut [too_many], 0), Err(errno::EINVAL));
        let mut null = c;
        null.aio_buf = 0;
        assert_eq!(submit_ok(ctx, &mut [null], 0), Err(errno::EFAULT));
        // `__import_iovec` takes an `unsigned`: 2^32 segments are none.
        let mut wide = null;
        wide.aio_nbytes = 1 << 32;
        assert_eq!(submit_ok(ctx, &mut [wide], 0), Ok(1));
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn fsync_takes_nothing_but_a_file() {
        let ctx = setup(8);
        let file = fd(HandleKind::File, O_RDWR);
        let pipe = fd(HandleKind::Pipe, O_RDWR);
        for set in 0..4 {
            let mut c = iocb(IOCB_CMD_FDSYNC, file);
            match set {
                0 => c.aio_buf = 1,
                1 => c.aio_offset = 1,
                2 => c.aio_nbytes = 1,
                _ => c.aio_rw_flags = 2,
            }
            assert_eq!(
                submit_ok(ctx, &mut [c], 0),
                Err(errno::EINVAL),
                "field {set}"
            );
        }
        assert_eq!(
            submit_ok(ctx, &mut [fsync_on(pipe)], 0),
            Err(errno::EINVAL),
            "no f_op->fsync"
        );
        assert_eq!(submit_ok(ctx, &mut [fsync_on(file)], 0), Ok(1));
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn a_transfer_that_refuses_is_a_refusal_not_an_event() {
        let ctx = setup(8);
        let file = fd(HandleKind::File, O_RDWR);
        let (r, _) = submit_with(ctx, &mut [fsync_on(file)], &mut |_| Err(errno::EINVAL));
        assert_eq!(r, Err(errno::EINVAL));
        let mut out = [IoEvent::zeroed(); 1];
        assert_eq!(
            take_now(ctx, 0, 1, out.as_mut_ptr()),
            Ok(0),
            "nothing posted"
        );
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    // -- io_getevents --

    #[test]
    fn getevents_refusals() {
        let ctx = setup(8);
        let mut out = [IoEvent::zeroed(); 2];
        assert_eq!(take_now(0, 0, 1, out.as_mut_ptr()), Err(errno::EINVAL));
        assert_eq!(take_now(ctx, 2, 1, out.as_mut_ptr()), Err(errno::EINVAL));
        assert_eq!(take_now(ctx, -1, 1, out.as_mut_ptr()), Err(errno::EINVAL));
        assert_eq!(take_now(ctx, -2, -1, out.as_mut_ptr()), Err(errno::EINVAL));
        assert_eq!(take_now(ctx, 0, 0, core::ptr::null_mut()), Ok(0));
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn events_carry_data_obj_and_res_in_order() {
        let ctx = setup(8);
        let file = fd(HandleKind::File, O_RDWR);
        let mut batch = [fsync_on(file), fsync_on(file), fsync_on(file)];
        for (i, c) in batch.iter_mut().enumerate() {
            c.aio_data = 100 + i as u64;
        }
        let objs: Vec<u64> = batch
            .iter()
            .map(|c| core::ptr::from_ref(c) as u64)
            .collect();
        let mut n = 0;
        let (r, _) = submit_with(ctx, &mut batch, &mut |_| {
            n += 1;
            Ok(-i64::from(n))
        });
        assert_eq!(r, Ok(3));
        let mut out = [IoEvent::zeroed(); 4];
        assert_eq!(take_now(ctx, 1, 2, out.as_mut_ptr()), Ok(2));
        assert_eq!(take_now(ctx, 1, 2, out[2..].as_mut_ptr()), Ok(1));
        for (i, ev) in out[..3].iter().enumerate() {
            assert_eq!(ev.data, 100 + i as u64);
            assert_eq!(ev.obj, objs[i]);
            assert_eq!(ev.res, -(i as i64) - 1);
        }
        assert_eq!(ring(ctx).head.load(Ordering::Relaxed), 3);
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn the_ring_wraps() {
        let ctx = setup(8);
        let events = ring(ctx).nr.load(Ordering::Relaxed) as usize;
        let file = fd(HandleKind::File, O_RDWR);
        let mut out = vec![IoEvent::zeroed(); events];
        let mut serial = 0i64;
        for round in 0..3 {
            let k = events - 11;
            let mut batch = vec![fsync_on(file); k];
            let (r, _) = submit_with(ctx, &mut batch, &mut |_| {
                serial += 1;
                Ok(serial)
            });
            assert_eq!(r, Ok(k as i64), "round {round}");
            let got = take_now(ctx, k as i64, k as i64, out.as_mut_ptr());
            assert_eq!(got, Ok(k as i64));
            let first = serial - k as i64 + 1;
            for (i, ev) in out[..k].iter().enumerate() {
                assert_eq!(ev.res, first + i as i64, "round {round}, event {i}");
            }
        }
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn a_fault_leaves_the_event_where_it_was() {
        let ctx = setup(8);
        let file = fd(HandleKind::File, O_RDWR);
        assert_eq!(submit_ok(ctx, &mut [fsync_on(file)], 5), Ok(1));
        assert_eq!(
            take_now(ctx, 1, 1, core::ptr::null_mut()),
            Err(errno::EFAULT)
        );
        let kernel = (1usize << 63) as *mut IoEvent;
        assert_eq!(take_now(ctx, 1, 1, kernel), Err(errno::EFAULT));
        assert_eq!(ring(ctx).head.load(Ordering::Relaxed), 0);
        let mut out = [IoEvent::zeroed(); 1];
        assert_eq!(take_now(ctx, 1, 1, out.as_mut_ptr()), Ok(1));
        assert_eq!(out[0].res, 5);
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn a_zero_or_passed_timeout_answers_without_waiting() {
        let ctx = setup(8);
        let mut out = [IoEvent::zeroed(); 1];
        for ts in [
            Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            Timespec {
                tv_sec: -1,
                tv_nsec: 0,
            },
        ] {
            // SAFETY: a local timeout and buffer.
            let r = unsafe { getevents(ctx, 1, 1, out.as_mut_ptr(), &raw const ts, &mut no_wait) };
            assert_eq!(r, Ok(0), "{}", ts.tv_sec);
        }
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn getevents_waits_for_another_threads_submission() {
        // THE REGRESSION PIN: this answered EAGAIN instead of waiting.
        let ctx = setup(8);
        let file = fd(HandleKind::File, O_RDWR);
        let mut out = [IoEvent::zeroed(); 2];
        let mut asked = Vec::new();
        let ts = Timespec {
            tv_sec: 5,
            tv_nsec: 0,
        };
        // SAFETY: a local timeout and buffer.
        let r = unsafe {
            getevents(ctx, 2, 2, out.as_mut_ptr(), &raw const ts, &mut |_, ns| {
                asked.push(ns);
                // The other thread: one request per wake-up.
                let mut c = [fsync_on(file)];
                assert_eq!(submit_ok(ctx, &mut c, 7), Ok(1));
            })
        };
        assert_eq!(r, Ok(2));
        assert_eq!(asked.len(), 2, "one wait per missing event");
        for ns in asked {
            let ns = ns.expect("a timeout was asked for");
            assert!(ns > 0 && ns <= 5_000_000_000, "relative: {ns}");
        }
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn a_null_timeout_waits_without_one() {
        let ctx = setup(8);
        let file = fd(HandleKind::File, O_RDWR);
        let mut out = [IoEvent::zeroed(); 1];
        let mut asked = None;
        // SAFETY: a local buffer.
        let r = unsafe {
            getevents(
                ctx,
                1,
                1,
                out.as_mut_ptr(),
                core::ptr::null(),
                &mut |_, ns| {
                    asked = Some(ns);
                    let mut c = [fsync_on(file)];
                    assert_eq!(submit_ok(ctx, &mut c, 1), Ok(1));
                },
            )
        };
        assert_eq!(r, Ok(1));
        assert_eq!(asked, Some(None));
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    #[test]
    fn a_context_destroyed_under_a_waiter_is_einval_to_it() {
        let ctx = setup(8);
        let mut out = [IoEvent::zeroed(); 1];
        let mut slot = None;
        // SAFETY: a local buffer.
        let r = unsafe {
            getevents(
                ctx,
                1,
                1,
                out.as_mut_ptr(),
                core::ptr::null(),
                &mut |_, _| {
                    // The other thread's io_destroy, up to its own wait.
                    slot = Some(kill(ctx).unwrap());
                },
            )
        };
        assert_eq!(r, Err(errno::EINVAL));
        // Its reference gone, the ring goes without a wait.
        release_when_unused(slot.unwrap(), |_| panic!("no call is left inside"));
        assert_eq!(sys_io_destroy(ctx), Err(errno::EINVAL));
    }

    #[test]
    fn destroy_waits_for_a_request_still_running() {
        let ctx = setup(8);
        let file = fd(HandleKind::File, O_RDWR);
        let mut killed = None;
        let (r, _) = submit_with(ctx, &mut [fsync_on(file)], &mut |_| {
            // io_destroy from another thread, while this request runs: it
            // must find the submission still inside, and wait.
            let s = kill(ctx).unwrap();
            let mut waited = false;
            let mut t = lock();
            let refs = t.ctx(s).map_or(0, |c| c.refs);
            drop(t);
            if refs > 0 {
                waited = true;
            }
            killed = Some((s, waited));
            Ok(0)
        });
        // The request completed into the dead ring and was counted.
        assert_eq!(r, Ok(1));
        let (s, waited) = killed.unwrap();
        assert!(waited, "destroy had to wait for the submission");
        release_when_unused(s, |_| panic!("the submission has left"));
    }

    // -- io_cancel --

    #[test]
    fn io_cancel_finds_nothing_to_cancel() {
        let ctx = setup(8);
        let mut c = Iocb::zeroed();
        // SAFETY (each): a local iocb, or NULL.
        unsafe {
            assert_eq!(
                sys_io_cancel(ctx, core::ptr::null_mut()),
                Err(errno::EFAULT)
            );
            c.aio_key = 1;
            assert_eq!(sys_io_cancel(ctx, &raw mut c), Err(errno::EINVAL), "key");
            c.aio_key = KIOCB_KEY;
            assert_eq!(sys_io_cancel(0, &raw mut c), Err(errno::EINVAL), "context");
            assert_eq!(
                sys_io_cancel(ctx, &raw mut c),
                Err(errno::EINVAL),
                "nothing"
            );
        }
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    // -- io_pgetevents --

    #[test]
    fn io_pgetevents_judges_the_mask_size_only_with_a_mask() {
        let ctx = setup(8);
        let mut out = [IoEvent::zeroed(); 1];
        let zero = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let mask: u64 = 0;
        let bad = AioSigset {
            sigmask: &raw const mask,
            sigsetsize: 7,
        };
        let no_mask = AioSigset {
            sigmask: core::ptr::null(),
            sigsetsize: 7,
        };
        let good = AioSigset {
            sigmask: &raw const mask,
            sigsetsize: 8,
        };
        let evp = out.as_mut_ptr();
        let call = |u: *const AioSigset| {
            // SAFETY: local arguments.
            unsafe { sys_io_pgetevents(ctx, 1, 1, evp, &raw const zero, u) }
        };
        assert_eq!(call(&raw const bad), Err(errno::EINVAL));
        assert_eq!(call(&raw const no_mask), Ok(0));
        assert_eq!(call(&raw const good), Ok(0));
        assert_eq!(call(core::ptr::null()), Ok(0));
        assert_eq!(sys_io_destroy(ctx), Ok(()));
    }

    // -- the timeout --

    #[test]
    fn ktime_is_the_kernels_arithmetic() {
        let ts = |s, n| Timespec {
            tv_sec: s,
            tv_nsec: n,
        };
        assert_eq!(ktime(ts(0, 0)), Some(0));
        assert_eq!(ktime(ts(1, 5)), Some(1_000_000_005));
        assert_eq!(
            ktime(ts(0, 5_000_000_000)),
            Some(5_000_000_000),
            "not judged"
        );
        assert_eq!(ktime(ts(-1, 0)), Some(-1_000_000_000));
        assert_eq!(
            ktime(ts(i64::MAX / 1_000_000_000, 0)),
            None,
            "KTIME_SEC_MAX"
        );
        assert_eq!(ktime(ts(i64::MAX, 0)), None);
    }
}
