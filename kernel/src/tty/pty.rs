//! Pseudo-terminals — a terminal device whose "hardware" is another process.
//!
//! A pty is a pair of byte streams plus the [`crate::tty`] device that sits
//! between them:
//!
//! ```text
//!   master end                                              slave end
//!     write ──────► input ring ──► [ line discipline ] ─────► read
//!     read  ◄────── output ring ◄── [ echo / OPOST   ] ◄───── write
//! ```
//!
//! The *master* is held by whatever is pretending to be a terminal — a terminal
//! emulator, `script(1)`, `ssh`. What it writes is what the program on the far
//! end sees as keystrokes; what it reads is what that program printed. The
//! *slave* is an ordinary terminal: a shell opens it as stdin/stdout, runs
//! `tcsetattr` on it, has a foreground process group on it, and gets `SIGINT`
//! when someone types `^C` into the master.
//!
//! # Why this is in the kernel
//!
//! A pty could be two socketpair ends in a library, and it would be wrong. Two
//! things make it kernel state:
//!
//! * **`termios` is shared across two address spaces.** A shell holding the
//!   slave clears `ECHO` to prompt for a password; the emulator holding the
//!   master must stop echoing *immediately*, without being told. A library pty
//!   has nowhere to put that word — it lives in neither process.
//! * **`^C` must be acted on when it is typed**, not when somebody next calls
//!   `read`. A line discipline running inside a reader only runs while a reader
//!   is in it, so a program in a compute loop would be uninterruptible.
//!
//! # Design notes
//!
//! * **The pty id *is* the [`crate::tty::TtyId`].** There is no separate
//!   namespace to keep in step, and every terminal question — termios, winsize,
//!   foreground group — is asked of the tty layer with that one id.
//! * **[`create`] returns both ends**, so there is no "master opened, slave
//!   never opened" state. Linux has one, which is why it needs `TIOCSPTLCK` and
//!   an "opened at least once" flag to decide whether an empty master read is
//!   EOF or a wait; we simply do not have the state that poses the question.
//! * **Echo is best-effort.** [`master_push_output`] drops what does not fit
//!   rather than blocking, because it is called from inside the line discipline
//!   on the input path: blocking there would stall a reader on a *reader*.
//!   Linux drops echo on a full output buffer for the same reason. Real slave
//!   output ([`slave_write`]) blocks for space and is never dropped.
//!
//! # Lock ordering
//!
//! `tty::DEVICES` → `PTYS` → `SCHED`. In practice no path here holds both of
//! the first two at once, and — as everywhere in this tree — the table lock is
//! dropped before any park or wake.

use crate::error::{KernelError, KernelResult};
use crate::ipc::waiters::{
    WaiterSet, current_user_pid, deliverable_signal_pending, park_interruptible, wake_all,
};
use crate::proc::pcb::ProcessId;
use crate::sched;
use crate::sync::PreemptSpinMutex as Mutex;
use crate::tty::{self, Input, TtyId};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

// ---------------------------------------------------------------------------
// Capacities
// ---------------------------------------------------------------------------

/// Bytes of un-consumed "typing" the master may have outstanding.
///
/// 4 KiB is Linux's `N_TTY_BUF_SIZE`. It is deliberately small: it bounds how
/// far ahead of the reading program a paste can get, which is what makes flow
/// control mean anything.
const INPUT_CAPACITY: usize = 4096;

/// Bytes of program output the master may have yet to read.
///
/// Larger than the input ring because a program printing a screenful at once is
/// the normal case, and every byte that does not fit blocks the program.
const OUTPUT_CAPACITY: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// Ids and handles
// ---------------------------------------------------------------------------

/// Next pty id. Starts at 1: id 0 is [`crate::tty::CONSOLE`].
static NEXT_PTY_ID: AtomicU32 = AtomicU32::new(1);

/// Allocate a fresh pty id.
///
/// Ids are never reused, so a handle to a closed pty can never be mistaken for
/// a handle to a new one. `u32` gives 4 billion of them; exhausting it is an
/// error rather than a wrap, because wrapping is precisely the aliasing bug the
/// no-reuse rule exists to prevent.
fn alloc_pty_id() -> KernelResult<TtyId> {
    let id = NEXT_PTY_ID.fetch_add(1, Ordering::Relaxed);
    if id == u32::MAX {
        return Err(KernelError::OutOfMemory);
    }
    Ok(id)
}

/// Which end of a pty a handle names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyEnd {
    /// The side that pretends to be a terminal (emulator, `script`, `ssh`).
    Master,
    /// The side that *is* a terminal (a shell's stdin/stdout).
    Slave,
}

impl PtyEnd {
    const fn as_bit(self) -> u64 {
        match self {
            Self::Master => 0,
            Self::Slave => 1,
        }
    }
}

/// An opaque handle to one end of a pty.
///
/// Bit-packed as `(id << 1) | end`, matching [`crate::ipc::pipe::PipeHandle`],
/// so that the end is part of the handle's identity: an operation applied to
/// the wrong end is an `InvalidHandle` rather than a silent transposition of
/// input and output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PtyHandle(u64);

impl PtyHandle {
    fn new(id: TtyId, end: PtyEnd) -> Self {
        Self((u64::from(id) << 1) | end.as_bit())
    }

    /// Reconstruct a handle from its raw userspace representation.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw value handed to userspace.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// The terminal device this handle refers to.
    #[must_use]
    pub const fn id(self) -> TtyId {
        // The shift can only lose bits for a raw value userspace invented, and
        // such a value names a pty that does not exist either way.
        (self.0 >> 1) as TtyId
    }

    /// Which end this handle names.
    #[must_use]
    pub const fn end(self) -> PtyEnd {
        if self.0 & 1 == 0 {
            PtyEnd::Master
        } else {
            PtyEnd::Slave
        }
    }
}

// ---------------------------------------------------------------------------
// The object
// ---------------------------------------------------------------------------

/// A byte ring buffer.
///
/// Its own type rather than a `VecDeque` so the wrap arithmetic is written once
/// and audited once; `VecDeque`'s `make_contiguous` would defeat the point by
/// copying on every read.
struct Ring {
    buf: Vec<u8>,
    head: usize,
    len: usize,
}

impl Ring {
    fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0u8; capacity],
            head: 0,
            len: 0,
        }
    }

    const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[allow(clippy::arithmetic_side_effects)]
    fn writable(&self) -> usize {
        // `len` is only ever increased by `write` (which caps at `writable`),
        // so it can never exceed `buf.len()`.
        self.buf.len() - self.len
    }

    /// Append as much of `data` as fits. Returns how much was taken.
    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    fn write(&mut self, data: &[u8]) -> usize {
        let n = data.len().min(self.writable());
        if n == 0 {
            return 0;
        }
        let cap = self.buf.len();
        let pos = (self.head + self.len) % cap;
        let first = n.min(cap - pos);
        self.buf[pos..pos + first].copy_from_slice(&data[..first]);
        let second = n - first;
        if second > 0 {
            self.buf[..second].copy_from_slice(&data[first..n]);
        }
        self.len += n;
        n
    }

    /// Remove up to `out.len()` bytes. Returns how many were taken.
    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    fn read(&mut self, out: &mut [u8]) -> usize {
        let n = out.len().min(self.len);
        if n == 0 {
            return 0;
        }
        let cap = self.buf.len();
        let first = n.min(cap - self.head);
        out[..first].copy_from_slice(&self.buf[self.head..self.head + first]);
        let second = n - first;
        if second > 0 {
            out[first..n].copy_from_slice(&self.buf[..second]);
        }
        self.head = (self.head + n) % cap;
        self.len -= n;
        n
    }

    /// Remove exactly one byte.
    #[allow(clippy::arithmetic_side_effects)]
    fn read_byte(&mut self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }
        let b = self.buf.get(self.head).copied();
        self.head = (self.head + 1) % self.buf.len();
        self.len -= 1;
        b
    }
}

/// One pseudo-terminal.
///
/// # The waiter rule
///
/// There are four things a task can block on here — each ring being non-empty
/// and each ring being non-full — and only two waiter sets. The mapping is not
/// free-form; it follows one rule, and the rule is what makes it checkable:
///
/// > **A task parks in the set of the ring it is blocked on, and every
/// > mutation of a ring wakes that ring's entire set.**
///
/// So `master_write` — which is blocked on the *input* ring having space —
/// parks in `input_waiters`, alongside the slave reader blocked on that same
/// ring having data. Waking both when either changes is over-broad by one
/// waiter, which costs a re-check under the lock and nothing else, because
/// every park loop re-evaluates its own condition after waking.
///
/// The rule matters more than the saving. Keyed by *role* instead ("readers"
/// and "writers"), each function has to remember which of two sets it belongs
/// to, and the first version of this file got it wrong in exactly the way that
/// is hardest to see: `slave_write` deregistered from one set and registered in
/// the other, so every signal-interrupted slave write left a stale entry behind
/// naming a task that was no longer parked — the `BUG-PIPE-SINGLE-WAITER-SLOT`
/// failure mode, which wakes an unrelated task once ids recycle. Keyed by ring,
/// each function names one set throughout and a mismatch is visible on one
/// screen.
struct Pty {
    /// What the master wrote: keystrokes awaiting the line discipline.
    input: Ring,
    /// What the slave wrote (plus echo): output awaiting the master's read.
    output: Ring,
    /// Everyone parked on the **input ring**: the slave's line discipline
    /// waiting for a byte, and a master waiting for room to write one.
    ///
    /// See the type-level note below on why the sets are keyed by *ring* and
    /// not by *role*.
    input_waiters: WaiterSet,
    /// Everyone parked on the **output ring**: a master waiting for program
    /// output, and the slave waiting for room to produce more.
    output_waiters: WaiterSet,
    /// Open master handles. `dup` adds one, `close` removes one.
    master_refs: u32,
    /// Open slave handles.
    slave_refs: u32,
}

impl Pty {
    fn new() -> Self {
        Self {
            input: Ring::new(INPUT_CAPACITY),
            output: Ring::new(OUTPUT_CAPACITY),
            input_waiters: WaiterSet::new(),
            output_waiters: WaiterSet::new(),
            master_refs: 1,
            slave_refs: 1,
        }
    }

    const fn master_gone(&self) -> bool {
        self.master_refs == 0
    }

    const fn slave_gone(&self) -> bool {
        self.slave_refs == 0
    }
}

/// Every live pty, keyed by the tty id it drives.
///
/// Boxed values because a `Pty` owns two ring buffers' worth of bookkeeping and
/// a `BTreeMap` moves its values when it rebalances.
static PTYS: Mutex<BTreeMap<TtyId, Box<Pty>>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/// Create a pty, returning `(master, slave)`.
///
/// Both ends are open from birth — see the module docs on why that removes a
/// whole class of Linux's pty state rather than merely simplifying it.
///
/// # Errors
///
/// `OutOfMemory` if the pty id space is exhausted.
pub fn create() -> KernelResult<(PtyHandle, PtyHandle)> {
    let id = alloc_pty_id()?;
    // Insert the pty before the device: between the two, `id` is not yet known
    // to anyone, but a device without a backing pty would answer reads with a
    // hangup, and a pty without a device would panic nothing but confuse
    // `tty::exists`. Neither is reachable, so this order is chosen only for
    // being the one that reads correctly.
    PTYS.lock().insert(id, Box::new(Pty::new()));
    tty::create_device(id);
    Ok((PtyHandle::new(id, PtyEnd::Master), PtyHandle::new(id, PtyEnd::Slave)))
}

/// Duplicate a handle, taking another reference to that end.
///
/// # Errors
///
/// `InvalidHandle` if the pty no longer exists.
pub fn dup(handle: PtyHandle) -> KernelResult<PtyHandle> {
    let mut table = PTYS.lock();
    let pty = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    match handle.end() {
        PtyEnd::Master => pty.master_refs = pty.master_refs.saturating_add(1),
        PtyEnd::Slave => pty.slave_refs = pty.slave_refs.saturating_add(1),
    }
    Ok(handle)
}

/// The process groups a [`close`] left owing a `SIGHUP` (+ `SIGCONT`).
///
/// Empty unless the *last* master handle just closed with a session still
/// holding the slave as its controlling terminal.
pub type Hangup = Vec<ProcessId>;

/// Close one handle, dropping a reference to its end.
///
/// Returns the process groups that must be hung up. Signal delivery is
/// deliberately **not** performed here: this layer knows only that the terminal
/// went away, and the syscall layer owns signal delivery — the same split
/// `pcb::ctty_release` already uses.
///
/// When the last handle of *either* end closes, readers on the other end are
/// woken so they can observe the hangup instead of parking forever. When both
/// ends are gone the device and any controlling-terminal association go with
/// them.
pub fn close(handle: PtyHandle) -> Hangup {
    let id = handle.id();
    let mut hangup: Hangup = Vec::new();

    let (input_wake, output_wake, destroy) = {
        let mut table = PTYS.lock();
        let Some(pty) = table.get_mut(&id) else {
            return hangup;
        };
        match handle.end() {
            PtyEnd::Master => pty.master_refs = pty.master_refs.saturating_sub(1),
            PtyEnd::Slave => pty.slave_refs = pty.slave_refs.saturating_sub(1),
        }
        let destroy = pty.master_gone() && pty.slave_gone();
        // Wake both sides unconditionally on any close that removed the last
        // reference: which side needs to notice depends on which end went, and
        // a waiter woken with nothing to do simply re-checks and re-parks.
        let (i, o) = if pty.master_gone() || pty.slave_gone() {
            (pty.input_waiters.take_all(), pty.output_waiters.take_all())
        } else {
            (Vec::new(), Vec::new())
        };
        if destroy {
            table.remove(&id);
        }
        (i, o, destroy)
    };

    if handle.end() == PtyEnd::Master {
        // The terminal has been unplugged from under whoever is using it.
        // Report the groups; the caller signals them.
        for (_sid, fg) in crate::proc::pcb::ctty_sessions_on(id) {
            if fg != 0 {
                hangup.push(fg);
            }
        }
    }

    wake_all(input_wake);
    wake_all(output_wake);

    if destroy {
        crate::proc::pcb::ctty_detach_tty(id);
        tty::destroy_device(id);
    }
    hangup
}

// ---------------------------------------------------------------------------
// Master side
// ---------------------------------------------------------------------------

/// Write "keystrokes" into the pty: bytes the slave's line discipline will see
/// as input.
///
/// Blocks while the input ring is full, which is what gives a paste into a slow
/// program back-pressure rather than a silent truncation.
///
/// # Errors
///
/// * `InvalidHandle` — not a master handle, or the pty is gone.
/// * `ChannelClosed` — the slave end is closed: nothing will ever read this.
/// * `Interrupted` — a deliverable signal arrived before any byte was written.
/// * `InvalidArgument` — `data` is empty.
pub fn master_write(handle: PtyHandle, data: &[u8]) -> KernelResult<usize> {
    if handle.end() != PtyEnd::Master {
        return Err(KernelError::InvalidHandle);
    }
    if data.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let pid = current_user_pid();
    let task = sched::current_task_id();

    loop {
        {
            let mut table = PTYS.lock();
            let pty = table.get_mut(&handle.id()).ok_or(KernelError::InvalidHandle)?;
            // Blocked on the input ring, so `input_waiters` throughout — see
            // the waiter rule on `Pty`. Deregister at the top of every
            // iteration: a wake does not clear our entry, and a stale entry
            // names a task that is no longer parked (see `waiters`' docs).
            pty.input_waiters.remove(task);

            if pty.slave_gone() {
                return Err(KernelError::ChannelClosed);
            }
            let n = pty.input.write(data);
            if n > 0 {
                let woken = pty.input_waiters.take_all();
                drop(table);
                wake_all(woken);
                return Ok(n);
            }
            if deliverable_signal_pending(pid) {
                return Err(KernelError::Interrupted);
            }
            // Space is freed by the slave's discipline *reading*, which wakes
            // this same set.
            pty.input_waiters.insert(task);
        }
        park_interruptible(pid, task);
    }
}

/// Read program output from the pty.
///
/// # Errors
///
/// * `InvalidHandle` — not a master handle, or the pty is gone.
/// * `IoError` — every slave handle is closed and the buffer is drained. See
///   the module-level note: this is Linux's answer, chosen over BSD's EOF
///   because the failure modes are asymmetric (a program that only checks for
///   `EIO` spins forever on an unexpected `0`, whereas one that only checks for
///   `0` merely prints a spurious error on an unexpected `EIO`).
/// * `Interrupted` — a deliverable signal arrived before any byte was read.
pub fn master_read(handle: PtyHandle, out: &mut [u8]) -> KernelResult<usize> {
    if handle.end() != PtyEnd::Master {
        return Err(KernelError::InvalidHandle);
    }
    if out.is_empty() {
        return Ok(0);
    }
    let pid = current_user_pid();
    let task = sched::current_task_id();

    loop {
        {
            let mut table = PTYS.lock();
            let pty = table.get_mut(&handle.id()).ok_or(KernelError::InvalidHandle)?;
            // Blocked on the output ring, so `output_waiters` throughout.
            pty.output_waiters.remove(task);

            let n = pty.output.read(out);
            if n > 0 {
                let woken = pty.output_waiters.take_all();
                drop(table);
                wake_all(woken);
                return Ok(n);
            }
            // Drain before reporting the hangup: bytes a program printed
            // immediately before exiting are still its output.
            if pty.slave_gone() {
                return Err(KernelError::IoError);
            }
            if deliverable_signal_pending(pid) {
                return Err(KernelError::Interrupted);
            }
            pty.output_waiters.insert(task);
        }
        park_interruptible(pid, task);
    }
}

/// Read program output without blocking.
///
/// # Errors
///
/// As [`master_read`], plus `WouldBlock` when nothing is buffered and the slave
/// is still open.
pub fn master_try_read(handle: PtyHandle, out: &mut [u8]) -> KernelResult<usize> {
    if handle.end() != PtyEnd::Master {
        return Err(KernelError::InvalidHandle);
    }
    if out.is_empty() {
        return Ok(0);
    }
    let mut table = PTYS.lock();
    let pty = table.get_mut(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    let n = pty.output.read(out);
    if n > 0 {
        let woken = pty.output_waiters.take_all();
        drop(table);
        wake_all(woken);
        return Ok(n);
    }
    if pty.slave_gone() {
        return Err(KernelError::IoError);
    }
    Err(KernelError::WouldBlock)
}

// ---------------------------------------------------------------------------
// Slave side
// ---------------------------------------------------------------------------

/// Write program output to the pty, applying output post-processing.
///
/// `OPOST`/`ONLCR` is applied here rather than by the caller because it is a
/// property of the terminal, which this layer owns: a program that writes `\n`
/// to a terminal in the default mode is asking for a line break, and only the
/// terminal knows that a line break is two bytes.
///
/// # Errors
///
/// * `InvalidHandle` — not a slave handle, or the pty is gone.
/// * `IoError` — the master is closed: the terminal has been unplugged.
/// * `Interrupted` — a deliverable signal arrived before any byte was written.
/// * `InvalidArgument` — `data` is empty.
pub fn slave_write(handle: PtyHandle, data: &[u8]) -> KernelResult<usize> {
    if handle.end() != PtyEnd::Slave {
        return Err(KernelError::InvalidHandle);
    }
    if data.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let id = handle.id();
    let t = tty::get_termios(id);
    let onlcr = t.opost_nl_is_crlf();

    let pid = current_user_pid();
    let task = sched::current_task_id();
    // How many of the caller's bytes have been accepted. The expansion of `\n`
    // to CRLF means accepted-bytes and written-bytes differ, and it is the
    // former the caller must be told about: a short count it does not
    // understand would make it re-send half a line.
    let mut consumed = 0usize;

    loop {
        {
            let mut table = PTYS.lock();
            let pty = table.get_mut(&id).ok_or(KernelError::InvalidHandle)?;
            // Blocked on the output ring, so `output_waiters` throughout.
            // This used to remove from `input_waiters` while inserting into
            // `output_waiters`, which left a stale entry behind on every exit
            // that was not the success path.
            pty.output_waiters.remove(task);

            if pty.master_gone() {
                // Report a partial success rather than an error: those bytes
                // really were delivered.
                return if consumed > 0 {
                    Ok(consumed)
                } else {
                    Err(KernelError::IoError)
                };
            }

            // Feed byte by byte so a `\n` that needs two slots is never split
            // across the ring's capacity boundary — half a CRLF reaching the
            // emulator would leave the cursor in the wrong column.
            while let Some(&b) = data.get(consumed) {
                let expanded: &[u8] = if onlcr && b == b'\n' { b"\r\n" } else { &[b] };
                if pty.output.writable() < expanded.len() {
                    break;
                }
                let _ = pty.output.write(expanded);
                consumed = consumed.saturating_add(1);
            }

            if consumed > 0 {
                let woken = pty.output_waiters.take_all();
                drop(table);
                wake_all(woken);
                return Ok(consumed);
            }
            if deliverable_signal_pending(pid) {
                return Err(KernelError::Interrupted);
            }
            // Space is freed by the master *reading*, which wakes this set.
            pty.output_waiters.insert(task);
        }
        park_interruptible(pid, task);
    }
}

// ---------------------------------------------------------------------------
// Line-discipline hooks (called by `tty`, never by userspace)
// ---------------------------------------------------------------------------

/// Take one input byte for the slave's line discipline, blocking.
///
/// Called by [`crate::tty::read`] with no device lock held, which is what
/// allows it to park.
pub(crate) fn slave_read_input_blocking(id: TtyId) -> Input {
    let pid = current_user_pid();
    let task = sched::current_task_id();
    loop {
        {
            let mut table = PTYS.lock();
            let Some(pty) = table.get_mut(&id) else {
                return Input::Hangup;
            };
            pty.input_waiters.remove(task);

            if let Some(b) = pty.input.read_byte() {
                // Draining the input ring frees space, so wake that ring's set
                // (which holds masters blocked on a full input ring).
                let woken = pty.input_waiters.take_all();
                drop(table);
                wake_all(woken);
                return Input::Byte(b);
            }
            if pty.master_gone() {
                return Input::Hangup;
            }
            if deliverable_signal_pending(pid) {
                return Input::Interrupted;
            }
            pty.input_waiters.insert(task);
        }
        park_interruptible(pid, task);
    }
}

/// Take one input byte if one is ready, without blocking.
pub(crate) fn slave_try_read_input(id: TtyId) -> Input {
    // No task id is taken here on purpose: this is the non-blocking path, so it
    // never parks and therefore never joins a waiter set.  It still *wakes* the
    // input set below, because draining the ring frees space for a blocked
    // master whether the drain blocked or not.
    let mut table = PTYS.lock();
    let Some(pty) = table.get_mut(&id) else {
        return Input::Hangup;
    };
    if let Some(b) = pty.input.read_byte() {
        let woken = pty.input_waiters.take_all();
        drop(table);
        wake_all(woken);
        return Input::Byte(b);
    }
    if pty.master_gone() {
        Input::Hangup
    } else {
        Input::Empty
    }
}

/// Take one input byte, blocking no later than `deadline_ns`.
pub(crate) fn slave_read_input_timeout(id: TtyId, deadline_ns: u64) -> Input {
    let pid = current_user_pid();
    let task = sched::current_task_id();

    /// Wake the parked task when the deadline fires.
    fn timeout_wake(tid: u64) {
        if !sched::try_wake(tid) {
            sched::defer_wake(tid);
        }
    }

    let now = crate::hrtimer::now_ns();
    if now >= deadline_ns {
        return slave_try_read_input(id);
    }
    let timer = crate::hrtimer::schedule_ns(deadline_ns.saturating_sub(now), timeout_wake, task);

    loop {
        {
            let mut table = PTYS.lock();
            let Some(pty) = table.get_mut(&id) else {
                crate::hrtimer::cancel(timer);
                return Input::Hangup;
            };
            pty.input_waiters.remove(task);

            if let Some(b) = pty.input.read_byte() {
                let woken = pty.input_waiters.take_all();
                crate::hrtimer::cancel(timer);
                drop(table);
                wake_all(woken);
                return Input::Byte(b);
            }
            if pty.master_gone() {
                crate::hrtimer::cancel(timer);
                return Input::Hangup;
            }
            if crate::hrtimer::now_ns() >= deadline_ns {
                crate::hrtimer::cancel(timer);
                return Input::Empty;
            }
            if deliverable_signal_pending(pid) {
                crate::hrtimer::cancel(timer);
                return Input::Interrupted;
            }
            pty.input_waiters.insert(task);
        }
        park_interruptible(pid, task);
    }
}

/// Push echo bytes towards the master.
///
/// **Lossy by design**: called from the line discipline's *input* path, so
/// blocking here would park a reader waiting on a reader. What does not fit is
/// dropped, exactly as Linux drops echo on a full output buffer — losing the
/// visual copy of a keystroke is a cosmetic failure, and the alternative is a
/// deadlock.
pub(crate) fn master_push_output(id: TtyId, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let readers = {
        let mut table = PTYS.lock();
        let Some(pty) = table.get_mut(&id) else {
            return;
        };
        if pty.master_gone() {
            return;
        }
        let n = pty.output.write(bytes);
        if n == 0 {
            return;
        }
        pty.output_waiters.take_all()
    };
    wake_all(readers);
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Whether a read on `handle` would return without blocking.
///
/// A hung-up end counts as ready: the read returns immediately, with `EIO` or a
/// short count rather than data, and a poll loop that called this "not ready"
/// would spin forever on a dead terminal.
#[must_use]
pub fn readable(handle: PtyHandle) -> bool {
    let table = PTYS.lock();
    let Some(pty) = table.get(&handle.id()) else {
        return true;
    };
    match handle.end() {
        PtyEnd::Master => !pty.output.is_empty() || pty.slave_gone(),
        PtyEnd::Slave => !pty.input.is_empty() || pty.master_gone(),
    }
}

/// Whether a write on `handle` would make progress without blocking.
#[must_use]
pub fn writable(handle: PtyHandle) -> bool {
    let table = PTYS.lock();
    let Some(pty) = table.get(&handle.id()) else {
        return true;
    };
    match handle.end() {
        PtyEnd::Master => pty.input.writable() > 0 || pty.slave_gone(),
        PtyEnd::Slave => pty.output.writable() > 0 || pty.master_gone(),
    }
}

/// Whether `handle` names a pty that still exists.
#[must_use]
pub fn exists(handle: PtyHandle) -> bool {
    PTYS.lock().contains_key(&handle.id())
}

// ---------------------------------------------------------------------------
// Boot self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test for the pty object.
///
/// Exercises the parts that need no second task: creation, the round trip in
/// both directions through the real line discipline, `ONLCR` expansion, echo,
/// and both hangup directions. The blocking paths are covered by the
/// interactive terminal itself once `SYS_PTY_*` lands; what is checked here is
/// the state machine, which is where the bugs are.
///
/// # Panics
///
/// On any assertion failure — this runs during bring-up, where a broken
/// terminal layer is not something to continue past.
pub fn self_test() {
    crate::serial_println!("[pty] Running self-test...");

    let (m, s) = create().expect("pty create");
    assert_eq!(m.id(), s.id(), "both ends name one device");
    assert_eq!(m.end(), PtyEnd::Master);
    assert_eq!(s.end(), PtyEnd::Slave);
    let id = m.id();
    assert!(tty::exists(id), "creating a pty creates its tty device");

    // A fresh pty is a canonical terminal with echo, like any other.
    assert!(tty::is_canonical(id), "pty defaults to canonical");
    assert!(tty::echo_enabled(id), "pty defaults to echo");

    // --- master -> discipline -> slave ------------------------------------
    let n = master_write(m, b"hi\n").expect("master write");
    assert_eq!(n, 3, "all three bytes accepted");

    let mut buf = [0u8; 32];
    let got = match tty::read(id, &mut buf) {
        tty::ConsoleRead::Data(n) => n,
        other => panic!("canonical read returned {other:?}"),
    };
    assert_eq!(got, 3, "canonical line is 'hi\\n'");
    assert_eq!(buf.get(..3), Some(&b"hi\n"[..]), "line contents");

    // Typing it echoed it back to the master, with the newline expanded by
    // ONLCR (the default `OPOST|ONLCR` terminal).
    let mut echo = [0u8; 32];
    let n = master_read(m, &mut echo).expect("master read echo");
    assert_eq!(
        echo.get(..n),
        Some(&b"hi\r\n"[..]),
        "echo is 'hi' plus a CRLF, got {:?}",
        echo.get(..n)
    );

    // --- slave -> master ---------------------------------------------------
    let n = slave_write(s, b"out\n").expect("slave write");
    assert_eq!(n, 4, "all four bytes consumed even though five were emitted");
    let mut got = [0u8; 32];
    let n = master_read(m, &mut got).expect("master read output");
    assert_eq!(got.get(..n), Some(&b"out\r\n"[..]), "ONLCR expanded output");

    // --- an interrupt character generates a signal, not data ---------------
    let _ = master_write(m, b"\x03").expect("master write ^C");
    match tty::read(id, &mut buf) {
        tty::ConsoleRead::Signal(sig) => assert_eq!(sig, 2, "^C is SIGINT"),
        other => panic!("^C should signal, got {other:?}"),
    }
    // ...and Linux echoes it, so the emulator shows `^C`.
    let n = master_read(m, &mut got).expect("master read ^C echo");
    assert_eq!(got.get(..n), Some(&b"^C"[..]), "^C is echoed as caret-C");

    // --- wrong end is a wrong handle, not a transposition ------------------
    assert!(
        master_write(s, b"x").is_err(),
        "a slave handle cannot write input"
    );
    assert!(
        slave_write(m, b"x").is_err(),
        "a master handle cannot write output"
    );

    // --- hangup: slave closes -> master reads EIO --------------------------
    let (m2, s2) = create().expect("pty create 2");
    let _ = slave_write(s2, b"bye").expect("slave write 2");
    assert!(close(s2).is_empty(), "closing a slave hangs nobody up");
    let mut tail = [0u8; 8];
    let n = master_read(m2, &mut tail).expect("drain before hangup");
    assert_eq!(tail.get(..n), Some(&b"bye"[..]), "buffered output survives");
    assert_eq!(
        master_read(m2, &mut tail),
        Err(KernelError::IoError),
        "a drained pty with no slave is EIO"
    );
    let id2 = m2.id();
    let _ = close(m2);
    assert!(!tty::exists(id2), "both ends closed removes the device");

    // --- hangup: master closes -> slave reads EOF, writes EIO --------------
    let (m3, s3) = create().expect("pty create 3");
    let id3 = m3.id();
    let _ = close(m3);
    assert_eq!(
        tty::read(id3, &mut buf),
        tty::ConsoleRead::Data(0),
        "a slave whose master went away reads EOF"
    );
    assert_eq!(
        slave_write(s3, b"x"),
        Err(KernelError::IoError),
        "writing to an unplugged terminal is EIO"
    );
    let _ = close(s3);
    assert!(!tty::exists(id3), "device removed after both ends closed");

    // --- the original pty is still intact and gets cleaned up --------------
    let _ = close(m);
    let _ = close(s);
    assert!(!tty::exists(id), "device removed after both ends closed");

    crate::serial_println!("[pty] Self-test PASSED");
}
