//! Pseudo-terminals — a terminal device whose "hardware" is another process.
//!
//! A pty is a pair of byte streams plus the [`crate::tty`] device that sits
//! between them:
//!
//! ```text
//!   master end                                                  slave end
//!     write ──► [ line discipline ] ──► input queue ──────────────► read
//!                      │ echo
//!                      ▼
//!     read  ◄────────────────────── output ring ◄── [ OPOST ] ◄──── write
//! ```
//!
//! The line discipline runs **in the master's write**, when the "keystrokes"
//! arrive — not in the slave's read. That is where `^C` becomes `SIGINT`, where
//! a typed character is echoed, and where a line is edited and completed; the
//! slave's read only takes finished bytes out of the device's input queue.
//! See "When input is processed" in [`crate::tty`] for why, and for the
//! failure the other arrangement produced.
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
//! * **Echo is best-effort.** The master write drops echo that does not fit in
//!   the output ring rather than blocking, because the echo is a side effect of
//!   *input*: blocking the typist until the program's output is read would
//!   stall a writer on a reader of the other direction. Linux drops echo on a
//!   full output buffer for the same reason. Real slave output
//!   ([`slave_write`]) blocks for space and is never dropped.
//!
//! # Lock ordering
//!
//! `tty::DEVICES` → `PTYS` → `SCHED`. The master write and a slave read hold
//! the first two together — the line discipline's state is in the device, the
//! rings and waiter sets are here — and, as everywhere in this tree, both are
//! dropped before any park or wake.

use crate::error::{KernelError, KernelResult};
use crate::ipc::waiters::{
    WaiterSet, current_user_pid, deliverable_signal_pending, park_interruptible, wake_all,
};
use crate::proc::pcb::ProcessId;
use crate::sched::{self, task::TaskId};
use crate::sync::PreemptSpinMutex as Mutex;
use crate::tty::{self, Received, TtyId};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

// ---------------------------------------------------------------------------
// Capacities
// ---------------------------------------------------------------------------

/// Bytes of program output the master may have yet to read.
///
/// (Input has no capacity here: it is queued in the terminal device, after the
/// line discipline, and bounded there by [`tty::INPUT_QUEUE_CAPACITY`] — Linux's
/// `N_TTY_BUF_SIZE`, deliberately small so it bounds how far ahead of the
/// reading program a paste can get, which is what makes flow control mean
/// anything.)
///
/// Larger than the input queue because a program printing a screenful at once
/// is the normal case, and every byte that does not fit blocks the program.
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
    // `as` rather than `u64::from` only because `From` is not usable in a const
    // fn; `TtyId` is a `u32`, so the widening cannot lose anything.
    #[allow(clippy::cast_lossless)]
    const fn new(id: TtyId, end: PtyEnd) -> Self {
        Self(((id as u64) << 1) | end.as_bit())
    }

    /// Reconstruct a handle from its raw userspace representation.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The slave handle for a terminal id.
    ///
    /// For the syscall layer, where a slave is addressed by *terminal* — a
    /// shell inherited its slave across `execve` and holds no handle, so
    /// `SYS_PTY_SLAVE_WRITE` resolves "my controlling terminal" to an id and
    /// needs a handle for it. Constructing one grants no authority the caller
    /// did not already have: the id came from the caller's own controlling
    /// terminal or from a handle it was proven to own.
    #[must_use]
    pub const fn new_slave(id: TtyId) -> Self {
        Self::new(id, PtyEnd::Slave)
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

    /// How many bytes are buffered.
    ///
    /// O(1) because `len` is a field rather than something derived from
    /// `head` and a tail: the count `FIONREAD` wants is already being
    /// maintained by every `write`/`read`, so reporting it costs nothing.
    const fn len(&self) -> usize {
        self.len
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

    /// Discard everything buffered.
    fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
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
}

/// One pseudo-terminal.
///
/// # The waiter rule
///
/// There are four things a task can block on here — the input queue and the
/// output ring each being non-empty and each being non-full — and only two
/// waiter sets. The mapping is not free-form; it follows one rule, and the rule
/// is what makes it checkable:
///
/// > **A task parks in the set of the buffer it is blocked on, and every
/// > mutation of a buffer wakes that buffer's entire set.**
///
/// So `master_write` — which is blocked on the *input* queue having space —
/// parks in `input_waiters`, alongside the slave reader blocked on that same
/// queue having data. Waking both when either changes is over-broad by one
/// waiter, which costs a re-check under the lock and nothing else, because
/// every park loop re-evaluates its own condition after waking.
///
/// The rule matters more than the saving. Keyed by *role* instead ("readers"
/// and "writers"), each function has to remember which of two sets it belongs
/// to, and the first version of this file got it wrong in exactly the way that
/// is hardest to see: `slave_write` deregistered from one set and registered in
/// the other, so every signal-interrupted slave write left a stale entry behind
/// naming a task that was no longer parked — the `BUG-PIPE-SINGLE-WAITER-SLOT`
/// failure mode, which wakes an unrelated task once ids recycle. Keyed by
/// buffer, each function names one set throughout and a mismatch is visible on
/// one screen.
///
/// The input queue itself lives in the terminal device (`tty::TtyDevice`), with
/// the line discipline that fills it; its waiter set lives here with the other
/// one. Both sides touch the set only while holding the device lock too, which
/// is what keeps a wake from falling between a reader's test and its park.
struct Pty {
    /// What the slave wrote (plus echo): output awaiting the master's read.
    output: Ring,
    /// Everyone parked on the terminal's **input queue**: a slave reader
    /// waiting for input, and a master waiting for room to write more.
    ///
    /// See the type-level note above on why the sets are keyed by *buffer* and
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
    Ok((
        PtyHandle::new(id, PtyEnd::Master),
        PtyHandle::new(id, PtyEnd::Slave),
    ))
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

/// Signals a master write decided are due, in the order their characters
/// arrived.
///
/// Almost always empty; one `^C` is one entry. Runs of the same signal are
/// collapsed — a standard signal is either pending or not, so `^C^C^C` in one
/// write means one `SIGINT`, and delivering it three times would only cost
/// three walks of the group.
pub type SignalsDue = Vec<u8>;

/// What a master write did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a master write can decide terminal signals, and only the caller can deliver them"]
pub struct MasterWrite {
    /// How many of the caller's bytes the terminal took — including signal
    /// characters and the typing a full canonical line had to drop, both of
    /// which were consumed exactly as a real terminal consumes them. A short
    /// count is a success; the caller resubmits the rest, as on a pipe.
    pub written: usize,
    /// Signals now due for the terminal's foreground process group.
    ///
    /// The pty layer decides them and the syscall layer delivers them
    /// (`handlers::signal_foreground_group`), the same split [`close`]'s
    /// hangups use: this layer knows which terminal and which signal, and
    /// nothing about process groups.
    pub signals: SignalsDue,
}

/// Run `data` through the terminal's line discipline as input: the shared
/// body of [`master_write`] and [`master_try_write`].
///
/// Everything happens under `DEVICES` then `PTYS`, held together: the
/// discipline's state (termios, the line being edited, the input queue) is in
/// the device, the output ring and waiter sets are here, and the echo of a
/// byte, the queueing of the line it completes and the wake of the reader
/// waiting for that line must not be separable.
///
/// Returns `Ok(Some(_))` once at least one byte was consumed. Returns
/// `Ok(None)` when the input queue is full and nothing could be — and if
/// `park_as` is given, that task has been registered in the input waiter set
/// *before* the locks were released, so a reader draining the queue in the
/// meantime cannot wake nobody.
///
/// # Errors
///
/// * `InvalidHandle` — the pty is gone.
/// * `ChannelClosed` — the slave end is closed: nothing will ever read this.
/// * `Interrupted` — only with `park_as`: a deliverable signal is pending for
///   the writer and nothing was consumed, so it must unwind rather than park.
fn receive_input(
    id: TtyId,
    data: &[u8],
    park_as: Option<(u64, TaskId)>,
) -> KernelResult<Option<MasterWrite>> {
    let mut devices = tty::DEVICES.lock();
    let dev = devices.get_mut(&id).ok_or(KernelError::InvalidHandle)?;
    let mut table = PTYS.lock();
    let pty = table.get_mut(&id).ok_or(KernelError::InvalidHandle)?;
    if let Some((_, task)) = park_as {
        // Deregister at the top of every attempt: a wake does not clear the
        // entry, and a stale one names a task that is no longer parked.
        pty.input_waiters.remove(task);
    }
    if pty.slave_gone() {
        return Err(KernelError::ChannelClosed);
    }

    let mut echo = Vec::new();
    let mut signals = SignalsDue::new();
    let mut written = 0usize;
    for &b in data {
        match tty::receive(dev, b, Some(&mut echo)) {
            Received::Consumed => {}
            Received::Signal(sig) => {
                if signals.last() != Some(&sig) {
                    signals.push(sig);
                }
            }
            Received::NoRoom => break,
        }
        written = written.saturating_add(1);
    }

    if written == 0 {
        if let Some((pid, task)) = park_as {
            if deliverable_signal_pending(pid) {
                return Err(KernelError::Interrupted);
            }
            // Room is made by a slave *reading*, which wakes this same set.
            pty.input_waiters.insert(task);
        }
        return Ok(None);
    }

    // The echo goes out now, with the input it shows: what a terminal emulator
    // draws when you type. Lossy by design — see the module notes.
    let echoed = pty.output.write(&echo);
    let output_woken = if echoed > 0 {
        pty.output_waiters.take_all()
    } else {
        Vec::new()
    };
    // Input was queued (or flushed, which also changes what a reader sees):
    // every task parked on the input queue re-checks.
    let input_woken = pty.input_waiters.take_all();
    drop(table);
    drop(devices);
    wake_all(input_woken);
    wake_all(output_woken);
    Ok(Some(MasterWrite { written, signals }))
}

/// Write "keystrokes" into the pty: bytes the slave's line discipline receives
/// as input, *now* — this is where they are echoed, edited into a line, and,
/// if one is a signal character, turned into a signal for the foreground
/// group (returned in [`MasterWrite::signals`] for the caller to deliver).
///
/// Blocks while the terminal's input queue is full, which is what gives a paste
/// into a slow program back-pressure rather than a silent truncation. Returns
/// as soon as at least one byte has been taken.
///
/// # Errors
///
/// * `InvalidHandle` — not a master handle, or the pty is gone.
/// * `ChannelClosed` — the slave end is closed: nothing will ever read this.
/// * `Interrupted` — a deliverable signal arrived before any byte was taken.
/// * `InvalidArgument` — `data` is empty.
pub fn master_write(handle: PtyHandle, data: &[u8]) -> KernelResult<MasterWrite> {
    if handle.end() != PtyEnd::Master {
        return Err(KernelError::InvalidHandle);
    }
    if data.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let pid = current_user_pid();
    let task = sched::current_task_id();
    loop {
        if let Some(done) = receive_input(handle.id(), data, Some((pid, task)))? {
            return Ok(done);
        }
        park_interruptible(pid, task);
    }
}

/// Write "keystrokes" into the pty without blocking.
///
/// As [`master_write`], but a full input queue is reported rather than waited
/// out. This exists so a caller driving several objects in one loop — sshd
/// forwarding a socket into a terminal is the case that asked for it — cannot
/// be parked on the pty while the socket has work: with only the blocking form
/// available, the choice was between stalling the whole loop and never filling
/// the queue at all.
///
/// Two asymmetries against [`master_try_read`] are deliberate, and each follows
/// this function's *blocking twin* rather than its non-blocking sibling:
///
/// 1. The hangup is checked **before** the transfer, where `master_try_read`
///    checks it after. Bytes handed to a dead slave will never be read by
///    anyone, whereas bytes a dying program already printed are still its
///    output and must be drained first (see the module note and
///    design-decisions.md §259).
/// 2. Empty `data` is `InvalidArgument`, where an empty `out` is `Ok(0)`.
///    Asking to write nothing is a caller bug; asking to read nothing is not.
///
/// # Errors
///
/// * `InvalidHandle` — not a master handle, or the pty is gone.
/// * `ChannelClosed` — the slave end is closed: nothing will ever read this.
/// * `InvalidArgument` — `data` is empty.
/// * `WouldBlock` — the input queue is full and the slave is still open. This
///   is the one state the blocking form waits out, and the whole point of this
///   call is to surface it instead.
pub fn master_try_write(handle: PtyHandle, data: &[u8]) -> KernelResult<MasterWrite> {
    if handle.end() != PtyEnd::Master {
        return Err(KernelError::InvalidHandle);
    }
    if data.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    receive_input(handle.id(), data, None)?.ok_or(KernelError::WouldBlock)
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
            let pty = table
                .get_mut(&handle.id())
                .ok_or(KernelError::InvalidHandle)?;
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
    let pty = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
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
// The input queue's waiters (called by `tty`, never by userspace)
// ---------------------------------------------------------------------------
//
// The input queue lives in the terminal device, and a slave read waits on it
// from `tty` (`pty_wait_for`) with `DEVICES` held; its waiter set lives here.
// These are the four things that read needs from this side, each taking `PTYS`
// only — the documented order, `DEVICES` then `PTYS`, is the caller's to keep.

/// Whether terminal `id` has lost its last master — nothing more will ever be
/// typed into it. A pty that no longer exists counts as hung up.
pub(crate) fn master_gone(id: TtyId) -> bool {
    PTYS.lock().get(&id).is_none_or(|p| p.master_gone())
}

/// Register `task` as parked on terminal `id`'s input queue.
pub(crate) fn insert_input_waiter(id: TtyId, task: TaskId) {
    if let Some(p) = PTYS.lock().get_mut(&id) {
        p.input_waiters.insert(task);
    }
}

/// Undo [`insert_input_waiter`]. Idempotent, and silent for a pty that is gone.
pub(crate) fn remove_input_waiter(id: TtyId, task: TaskId) {
    if let Some(p) = PTYS.lock().get_mut(&id) {
        p.input_waiters.remove(task);
    }
}

/// Take every task parked on terminal `id`'s input queue, for the caller to
/// wake once it has dropped its locks.
pub(crate) fn take_input_waiters(id: TtyId) -> Vec<TaskId> {
    PTYS.lock()
        .get_mut(&id)
        .map(|p| p.input_waiters.take_all())
        .unwrap_or_default()
}

/// Wake every task parked on terminal `id`'s input queue: the queue changed in
/// a way none of them caused — a flush, or a new termios that changes what a
/// waiting read is waiting for.
pub(crate) fn wake_input_waiters(id: TtyId) {
    wake_all(take_input_waiters(id));
}

/// Discard program output the master has not read yet — `tcflush(TCOFLUSH)`
/// on the slave.
///
/// Wakes the output ring's waiters: a slave writer parked on a full ring now
/// has room, and a master reader re-checks and finds nothing, which is the
/// truth after a flush.
pub(crate) fn flush_output(id: TtyId) {
    let woken = {
        let mut table = PTYS.lock();
        let Some(pty) = table.get_mut(&id) else {
            return;
        };
        pty.output.clear();
        pty.output_waiters.take_all()
    };
    wake_all(woken);
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Whether a read on `handle` would return without blocking.
///
/// A hung-up end counts as ready: the read returns immediately, with `EIO` or a
/// short count rather than data, and a poll loop that called this "not ready"
/// would spin forever on a dead terminal.
///
/// The slave arm asks the terminal device ([`tty::input_ready`]), which holds
/// the input queue, and the answer is exact in both modes: input is edited as
/// it arrives, so "a complete line is queued" is a lookup. (It used to be an
/// upper bound — "bytes are present" — because the editor ran in the reader;
/// a canonical slave holding half a line then reported readable, and a poller
/// that believed it issued a read that parked.)
#[must_use]
pub fn readable(handle: PtyHandle) -> bool {
    // Sampled *before* PTYS is taken: the documented lock order is `DEVICES`
    // before `PTYS`, so reaching into the device with the pty table held would
    // be the inversion. Sampling first is not a race that matters — input can
    // only be added between the two reads, and a readability answer is a hint
    // that the read itself re-checks either way.
    let input = handle.end() == PtyEnd::Slave && tty::input_ready(handle.id());
    let table = PTYS.lock();
    let Some(pty) = table.get(&handle.id()) else {
        return true;
    };
    match handle.end() {
        PtyEnd::Master => !pty.output.is_empty() || pty.slave_gone(),
        PtyEnd::Slave => input || pty.master_gone(),
    }
}

/// How many bytes a read on `handle` would find waiting — `FIONREAD`.
///
/// Exact at both ends and in both modes:
///
/// * **Master** — the output ring holds post-discipline bytes with nothing
///   further to do to them, so its length is precisely what the next read
///   delivers.
/// * **Slave** — [`tty::input_bytes`]: in canonical mode the bytes of complete
///   lines only, since the line being edited is not readable until its
///   terminator arrives; in raw mode every queued byte. (Canonical mode used to
///   report an upper bound here, counting pre-discipline bytes the editor had
///   not yet run over; the editor now runs as they arrive.)
///
/// # Hangup is not readable *bytes*
///
/// Unlike [`readable`], a hung-up end with nothing queued answers 0. The two
/// questions differ: "would a read return immediately" is yes (it returns EOF
/// or `EIO`), but "how many bytes are there" is none, and `FIONREAD` is asked
/// by callers who will believe the number. Linux answers 0 here too.
#[must_use]
pub fn readable_bytes(handle: PtyHandle) -> usize {
    // Sampled before PTYS for the lock-order reason given on `readable`.
    let input = if handle.end() == PtyEnd::Slave {
        tty::input_bytes(handle.id())
    } else {
        0
    };
    let table = PTYS.lock();
    let Some(pty) = table.get(&handle.id()) else {
        return 0;
    };
    match handle.end() {
        PtyEnd::Master => pty.output.len(),
        PtyEnd::Slave => input,
    }
}

/// Whether a write on `handle` would make progress without blocking.
///
/// For the master this is [`tty::input_room`], which under-reports in the safe
/// direction (a canonical terminal can also take ordinary typing into its line
/// while the queue is full).
#[must_use]
pub fn writable(handle: PtyHandle) -> bool {
    // Sampled before PTYS for the lock-order reason given on `readable`.
    let room = handle.end() == PtyEnd::Master && tty::input_room(handle.id());
    let table = PTYS.lock();
    let Some(pty) = table.get(&handle.id()) else {
        return true;
    };
    match handle.end() {
        PtyEnd::Master => room || pty.slave_gone(),
        PtyEnd::Slave => pty.output.writable() > 0 || pty.master_gone(),
    }
}

/// Whether `handle` names a pty that still exists.
#[must_use]
pub fn exists(handle: PtyHandle) -> bool {
    PTYS.lock().contains_key(&handle.id())
}

// ---------------------------------------------------------------------------
// Multi-object waiting
// ---------------------------------------------------------------------------

/// Park `task` on this pty so that any state change wakes it.
///
/// The multi-object (`ipc::multiwait`) counterpart to the registration
/// the blocking read/write paths above do inline: a task waiting on several
/// objects at once cannot use any one object's park loop, so it registers on
/// each of them, tests them all, and parks once.
///
/// **Both sets, whichever end the handle names.** A pty's two waiter sets are
/// named for the *rings* rather than the ends — a master reader and a slave
/// writer both park in `output_waiters` — so which set carries the wake a given
/// caller wants depends on the end *and* on the direction, and hangup
/// ([`close`]) wakes both. Registering in both cannot lose a wake, and costs
/// only a re-check of a condition the caller is about to re-check anyway.
///
/// A stale handle is a silent no-op: the pty may be closed between the caller
/// resolving the handle and this call, and the readiness queries above already
/// report a vanished pty as ready ([`readable`], [`writable`]), which is the
/// answer a poller needs.
///
/// Must be paired with [`deregister_waiter`] on every exit path — see
/// [`crate::ipc::pipe::deregister_waiter`] for what a leaked entry does.
pub fn register_waiter(handle: PtyHandle, task: TaskId) {
    let mut table = PTYS.lock();
    let Some(pty) = table.get_mut(&handle.id()) else {
        return;
    };
    pty.input_waiters.insert(task);
    pty.output_waiters.insert(task);
}

/// Undo [`register_waiter`]: remove `task` from both of this pty's waiter sets.
///
/// Idempotent and stale-handle-safe, so it can be called unconditionally from a
/// `Drop`.
pub fn deregister_waiter(handle: PtyHandle, task: TaskId) {
    let mut table = PTYS.lock();
    let Some(pty) = table.get_mut(&handle.id()) else {
        return;
    };
    pty.input_waiters.remove(task);
    pty.output_waiters.remove(task);
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
pub fn self_test() -> crate::error::KernelResult<()> {
    crate::serial_println!("[pty] Running self-test...");

    let (m, s) = create().expect("pty create");
    assert_eq!(m.id(), s.id(), "both ends name one device");
    assert_eq!(m.end(), PtyEnd::Master);
    assert_eq!(s.end(), PtyEnd::Slave);

    // The two assertions above are a *round trip*: they encode and decode with
    // the same `as_bit`, so they would pass just as happily with the bits
    // flipped. That is not enough any more, because the low bit left this
    // kernel and became an ABI.
    //
    // Since 2026-08-24 libc reconstructs an inherited pty's `HandleKind` from
    // this bit (`handle_type_to_kind_for` in posix/src/spawn.rs): one wire type
    // `fd_handle_type::PTY = 7` names both ends, so the type byte alone cannot
    // say which end a `spawn`ed child received, and the child reads the low bit
    // to decide. Lane B pinned that with its own test — against its own copy of
    // the convention. A test on that side cannot notice this side changing, so
    // reordering `enum PtyEnd` here would leave both test suites green while
    // every inherited master silently decoded as a slave.
    //
    // The consequence is specifically worse than a decode that merely fails: a
    // terminal emulator's own keystrokes, written to what it believes is its
    // master, would be delivered back to itself as terminal input. So pin the
    // numeric values, not just their round trip.
    assert_eq!(m.raw() & 1, 0, "master is bit 0 (libc decodes this)");
    assert_eq!(s.raw() & 1, 1, "slave is bit 1 (libc decodes this)");

    let id = m.id();
    assert!(tty::exists(id), "creating a pty creates its tty device");

    // A fresh pty is a canonical terminal with echo, like any other.
    assert!(tty::is_canonical(id), "pty defaults to canonical");
    assert!(tty::echo_enabled(id), "pty defaults to echo");

    // --- master -> discipline -> slave ------------------------------------
    let w = master_write(m, b"hi\n").expect("master write");
    assert_eq!(w.written, 3, "all three bytes accepted");
    assert!(w.signals.is_empty(), "no signal character, no signal");

    // The echo is there BEFORE anybody reads, because the discipline ran in
    // the write. This is what lets a terminal emulator show what you type
    // while the program on the other end is busy.
    let mut echo = [0u8; 32];
    let n = master_read(m, &mut echo).expect("master read echo");
    assert_eq!(
        echo.get(..n),
        Some(&b"hi\r\n"[..]),
        "echo is 'hi' plus a CRLF, got {:?}",
        echo.get(..n)
    );

    let mut buf = [0u8; 32];
    let got = match tty::read(id, &mut buf) {
        tty::ConsoleRead::Data(n) => n,
        other => panic!("canonical read returned {other:?}"),
    };
    assert_eq!(got, 3, "canonical line is 'hi\\n'");
    assert_eq!(buf.get(..3), Some(&b"hi\n"[..]), "line contents");

    // --- slave -> master ---------------------------------------------------
    let n = slave_write(s, b"out\n").expect("slave write");
    assert_eq!(
        n, 4,
        "all four bytes consumed even though five were emitted"
    );
    let mut got = [0u8; 32];
    let n = master_read(m, &mut got).expect("master read output");
    assert_eq!(got.get(..n), Some(&b"out\r\n"[..]), "ONLCR expanded output");

    // --- the unified write path reaches the same place ----------------------
    // `tty::write` is what `SYS_CONSOLE_WRITE` and the Linux `write(1, …)` now
    // go through, and the whole point of it is that a program need not know
    // which backend is behind its terminal.  If this ever stops routing to the
    // pty, a shell under a terminal emulator prints on the physical screen
    // instead of in its window — a failure that is invisible in a headless
    // boot, which is exactly why it is asserted here.
    let n = tty::write(id, b"via tty\n").expect("tty::write to a pty");
    assert_eq!(n, 8, "tty::write reports caller bytes, not expanded bytes");
    let n = master_read(m, &mut got).expect("master read tty::write output");
    assert_eq!(
        got.get(..n),
        Some(&b"via tty\r\n"[..]),
        "tty::write applied the pty backend's ONLCR"
    );

    // --- ^C is acted on when it is WRITTEN, with nobody reading -------------
    // The regression this arrangement exists for (known-issues.md
    // A-PTY-CTRL-C-IS-ONLY-SEEN-BY-A-READER): the write itself decides the
    // signal. No read happens between the write and the assertion, which is
    // exactly the position of a program that is busy rather than reading —
    // the only kind of program anyone wants to interrupt. Before 2026-09-24
    // this write returned 1 and decided nothing; the SIGINT waited for a read
    // that `ctest-pty`'s child never made.
    let _ = master_write(m, b"ab").expect("half a line typed ahead");
    let w = master_write(m, b"\x03").expect("master write ^C");
    assert_eq!(w.written, 1, "the ^C is consumed");
    assert_eq!(w.signals, vec![2u8], "^C is SIGINT, decided by the write");
    // ...it is not data, and it flushed what was typed ahead of it:
    assert_eq!(
        tty::try_read(id, &mut buf),
        tty::ConsoleRead::WouldBlock,
        "neither the ^C nor the flushed 'ab' is input"
    );
    let _ = master_write(m, b"\n").expect("end the (now empty) line");
    assert_eq!(
        tty::read(id, &mut buf),
        tty::ConsoleRead::Data(1),
        "only the newline survives the flush"
    );
    // ...and it is echoed as caret-C, after the typed-ahead text that was
    // already on screen when it arrived.
    let n = master_read(m, &mut got).expect("master read ^C echo");
    assert_eq!(
        got.get(..n),
        Some(&b"ab^C\r\n"[..]),
        "typed text, then ^C, then the newline; got {:?}",
        got.get(..n)
    );

    // Several signal characters in one write: each is decided, in order, and
    // a run of the same signal collapses (a standard signal is pending or not).
    let w = master_write(m, b"\x03\x03\x1c").expect("^C ^C ^\\");
    assert_eq!(w.written, 3, "all three consumed");
    assert_eq!(w.signals, vec![2u8, 3u8], "SIGINT once, then SIGQUIT");
    while master_try_read(m, &mut got).is_ok() {}

    // NOFLSH: the signal still happens, and the typed-ahead line survives it.
    let mut t = tty::get_termios(id);
    t.c_lflag |= tty::lflag::NOFLSH;
    tty::set_termios(id, t);
    let w = master_write(m, b"keep\x03\n").expect("a NOFLSH line with a ^C in it");
    assert_eq!(w.signals, vec![2u8], "NOFLSH does not suppress the signal");
    assert_eq!(
        tty::read(id, &mut buf),
        tty::ConsoleRead::Data(5),
        "NOFLSH kept the line"
    );
    assert_eq!(buf.get(..5), Some(&b"keep\n"[..]), "and all of it");
    t.c_lflag &= !tty::lflag::NOFLSH;
    tty::set_termios(id, t);
    while master_try_read(m, &mut got).is_ok() {}

    // A disabled interrupt character (c_cc 0, what `stty intr undef` writes)
    // is not "the NUL character": a NUL byte must not raise SIGINT.
    let mut t = tty::get_termios(id);
    let saved = t;
    if let Some(c) = t.c_cc.get_mut(tty::cc::VINTR) {
        *c = 0;
    }
    tty::set_termios(id, t);
    let w = master_write(m, b"\0\n").expect("a NUL with VINTR disabled");
    assert!(w.signals.is_empty(), "a disabled VINTR matches nothing");
    assert_eq!(
        tty::read(id, &mut buf),
        tty::ConsoleRead::Data(2),
        "the NUL is data"
    );
    tty::set_termios(id, saved);
    while master_try_read(m, &mut got).is_ok() {}

    // --- readable byte counts (FIONREAD): exact, in both modes -------------
    // Both ends are empty here. Zero must be exact — a caller that uses
    // FIONREAD only to test emptiness, which is what a select-less poll loop
    // does, is the majority caller and the one that must never be misled.
    assert_eq!(readable_bytes(m), 0, "drained master counts zero");
    assert_eq!(readable_bytes(s), 0, "drained slave counts zero");
    assert!(!readable(m), "drained master is not readable");
    assert!(!readable(s), "drained slave is not readable");

    // Master side: `slave_write` puts post-discipline bytes in the output
    // ring, so the count is precisely what the next read delivers — including
    // the ONLCR expansion, because the expansion has already happened by the
    // time the bytes are counted. Counting the *caller's* four bytes here
    // would be an undercount, and a reader sized by it would leave the stray
    // CR behind to be mistaken for the start of the next line.
    let n = slave_write(s, b"abc\n").expect("slave write for count");
    assert_eq!(n, 4, "four caller bytes consumed");
    assert_eq!(
        readable_bytes(m),
        5,
        "master counts the expanded bytes (abc\\r\\n), not the caller's four"
    );
    assert!(readable(m), "a master with bytes is readable");
    let n = master_read(m, &mut got).expect("drain the counted bytes");
    assert_eq!(n, 5, "the count was exact, not an estimate");
    assert_eq!(readable_bytes(m), 0, "and the ring is empty again");

    // Slave side, canonical mode: half a line is NOT readable. This used to
    // count 2 here, as a deliberate upper bound, because the editor had not
    // run yet; now it has, and a poller that trusted "2" would have issued a
    // read that parked.
    let _ = master_write(m, b"xy").expect("partial line");
    assert_eq!(readable_bytes(s), 0, "half a line is not readable input");
    assert!(!readable(s), "and poll agrees");
    let _ = master_write(m, b"\n").expect("terminate the line");
    assert_eq!(readable_bytes(s), 3, "a complete line counts exactly");
    assert!(readable(s), "and is readable");
    let got_n = match tty::read(id, &mut buf) {
        tty::ConsoleRead::Data(n) => n,
        other => panic!("read returned {other:?}"),
    };
    assert_eq!(got_n, 3, "'xy\\n' delivered");
    let _ = master_read(m, &mut got).expect("drain echo");
    assert_eq!(readable_bytes(s), 0, "slave drained");

    // A canonical line is delivered as a unit, and a reader whose buffer is
    // smaller than the line leaves the rest queued, still marked as the same
    // line. A readiness answer that forgot it would report "nothing to read"
    // while a read stood by to return four bytes — forever, if the master
    // sends nothing more.
    let _ = master_write(m, b"hello\n").expect("full line");
    let mut small = [0u8; 2];
    let got_n = match tty::read(id, &mut small) {
        tty::ConsoleRead::Data(n) => n,
        other => panic!("short read returned {other:?}"),
    };
    assert_eq!(got_n, 2, "only what fits");
    assert_eq!(
        readable_bytes(s),
        4,
        "the undelivered remainder of the line is still readable"
    );
    assert!(readable(s), "a slave holding a partial line is readable");
    let got_n = match tty::read(id, &mut buf) {
        tty::ConsoleRead::Data(n) => n,
        other => panic!("remainder read returned {other:?}"),
    };
    assert_eq!(got_n, 4, "the remainder was exactly what was counted");
    assert_eq!(
        buf.get(..4),
        Some(&b"llo\n"[..]),
        "and it is the right bytes"
    );
    assert_eq!(readable_bytes(s), 0, "nothing left");
    let _ = master_read(m, &mut got).expect("drain the echo of 'hello'");

    // --- end of file --------------------------------------------------------
    // ^D after text delivers the text without the ^D; ^D on an empty line is a
    // zero-length read. The mark is not a byte, so FIONREAD does not count it.
    let _ = master_write(m, b"ab\x04").expect("ab^D");
    assert_eq!(readable_bytes(s), 2, "the ^D is not a byte");
    assert_eq!(
        tty::read(id, &mut buf),
        tty::ConsoleRead::Data(2),
        "'ab' alone"
    );
    assert_eq!(buf.get(..2), Some(&b"ab"[..]), "without the ^D");
    let _ = master_write(m, b"\x04").expect("^D on an empty line");
    assert!(readable(s), "an end of file is readable");
    assert_eq!(
        tty::read(id, &mut buf),
        tty::ConsoleRead::Data(0),
        "^D on an empty line reads as end of file"
    );
    assert_eq!(
        tty::try_read(id, &mut buf),
        tty::ConsoleRead::WouldBlock,
        "and reading it consumed it"
    );
    while master_try_read(m, &mut got).is_ok() {}

    // --- a full line can still be ended -------------------------------------
    // Typing past MAX_CANON is consumed and dropped, never refused — refusing
    // would block the typist behind a line only a terminator can finish — and
    // the terminator always fits, because the last slot is kept for it.
    let long = vec![b'q'; tty::MAX_CANON + 100];
    let w = master_write(m, &long).expect("type past MAX_CANON");
    assert_eq!(w.written, long.len(), "typing past a full line is consumed");
    let w = master_write(m, b"\n").expect("end the full line");
    assert_eq!(w.written, 1, "the reserved slot takes the terminator");
    let mut big = vec![0u8; tty::MAX_CANON + 16];
    assert_eq!(
        tty::read(id, &mut big),
        tty::ConsoleRead::Data(tty::MAX_CANON),
        "a full line is MAX_CANON bytes, newline included"
    );
    assert_eq!(
        big.get(tty::MAX_CANON - 1).copied(),
        Some(b'\n'),
        "and it still ends in its newline"
    );
    while master_try_read(m, &mut got).is_ok() {}

    // --- a change of mode carries unread input across -----------------------
    let canonical = tty::get_termios(id);
    let mut raw = canonical;
    raw.c_lflag &= !(tty::lflag::ICANON | tty::lflag::ECHO);
    let _ = master_write(m, b"ab").expect("half a line");
    tty::set_termios(id, raw);
    assert_eq!(
        tty::try_read(id, &mut buf),
        tty::ConsoleRead::Data(2),
        "canonical -> raw: the half-typed line becomes raw input"
    );
    let _ = master_write(m, b"xyz").expect("raw input");
    tty::set_termios(id, canonical);
    assert_eq!(
        tty::try_read(id, &mut buf),
        tty::ConsoleRead::Data(3),
        "raw -> canonical: unread raw input is one complete line"
    );
    assert_eq!(
        buf.get(..3),
        Some(&b"xyz"[..]),
        "the raw bytes, as that line"
    );
    while master_try_read(m, &mut got).is_ok() {}

    // A vanished pty counts zero rather than panicking or reporting a stale
    // number: `readable` calls that end "ready" so a poll loop can observe the
    // hangup, but there are no *bytes*, and FIONREAD's caller believes the
    // number it is given.
    let (m4, s4) = create().expect("pty create 4");
    let _ = close(m4);
    let _ = close(s4);
    assert_eq!(readable_bytes(m4), 0, "a destroyed pty has no bytes");
    assert!(readable(m4), "but it is still 'ready', so a poll wakes");

    // --- window size is per-device, and a resize is distinguishable ---------
    // `SYS_PTY_SET_WINSIZE` raises SIGWINCH only when `set_winsize` reports a
    // real change; a shell re-setting the same size on every prompt must not
    // wake every full-screen program on the terminal to redraw an unchanged
    // screen.  That "only on a change" is the contract being pinned here.
    let ws = tty::WinSize {
        ws_row: 40,
        ws_col: 100,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    assert!(tty::set_winsize(id, ws), "a new size is a change");
    assert!(!tty::set_winsize(id, ws), "the same size is not a change");
    assert_eq!(tty::get_winsize(id).ws_row, 40, "rows stuck");
    assert_eq!(tty::get_winsize(id).ws_col, 100, "cols stuck");

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

    // --- hangup: master closes -> slave reads what was typed, then EOF -----
    let (m3, s3) = create().expect("pty create 3");
    let id3 = m3.id();
    let _ = master_write(m3, b"par").expect("half a line, never finished");
    let _ = close(m3);
    assert_eq!(
        tty::read(id3, &mut buf),
        tty::ConsoleRead::Data(3),
        "a hangup delivers the half-typed line as it stands"
    );
    assert_eq!(buf.get(..3), Some(&b"par"[..]), "that line");
    assert_eq!(
        tty::read(id3, &mut buf),
        tty::ConsoleRead::Data(0),
        "then the slave reads EOF"
    );
    assert_eq!(
        slave_write(s3, b"x"),
        Err(KernelError::IoError),
        "writing to an unplugged terminal is EIO"
    );
    let _ = close(s3);
    assert!(!tty::exists(id3), "device removed after both ends closed");

    // --- non-blocking master write: flow control ----------------------------
    // `master_write` cannot be exercised at its interesting point here — a full
    // input queue is exactly where it parks, and there is no second task to
    // unpark it. The non-blocking form reaches that state and returns, so this
    // is the one place the full-queue behaviour is checkable in a boot
    // self-test.
    let (m5, s5) = create().expect("pty create 5");
    let id5 = m5.id();

    // Rejections first, so a later `Ok` cannot be an accident of a handle check
    // that admits everything.
    assert_eq!(
        master_try_write(s5, b"x"),
        Err(KernelError::InvalidHandle),
        "a slave handle cannot write input, blocking or not"
    );
    // Follows `master_write`, not `master_try_read`: asking to write nothing is
    // a caller bug, asking to read nothing is not. The two non-blocking calls
    // deliberately disagree here, and this is what stops the disagreement being
    // "tidied up" into a symmetry that would silently change an ABI.
    assert_eq!(
        master_try_write(m5, b""),
        Err(KernelError::InvalidArgument),
        "an empty write follows master_write's rejection, not master_try_read's Ok(0)"
    );

    // Raw mode, so bytes go straight to the queue — a canonical terminal would
    // take this much typing into its line editor instead.
    let mut raw5 = tty::get_termios(id5);
    raw5.c_lflag &= !(tty::lflag::ICANON | tty::lflag::ECHO);
    tty::set_termios(id5, raw5);
    let block = vec![b'a'; tty::INPUT_QUEUE_CAPACITY];
    assert_eq!(
        master_try_write(m5, &block).map(|w| w.written),
        Ok(tty::INPUT_QUEUE_CAPACITY),
        "an empty queue takes the whole buffer"
    );
    assert_eq!(
        master_try_write(m5, b"z"),
        Err(KernelError::WouldBlock),
        "a full queue with a live slave is WouldBlock — not Ok(0), and not a park"
    );

    // One byte out makes room for exactly one byte in, and a short count is a
    // *success*: the caller resubmits the tail, as it would on a pipe. Reported
    // as an error instead, a caller retrying "the failed write" would resend the
    // byte that was in fact accepted.
    let mut one = [0u8; 1];
    assert_eq!(
        tty::try_read(id5, &mut one),
        tty::ConsoleRead::Data(1),
        "a raw read drains one byte"
    );
    assert_eq!(one[0], b'a', "the byte drained is the first written");
    assert_eq!(
        master_try_write(m5, b"xy").map(|w| w.written),
        Ok(1),
        "a short count is a success, not an error"
    );
    assert_eq!(
        master_try_write(m5, b"z"),
        Err(KernelError::WouldBlock),
        "and one byte refilled the queue"
    );
    // A signal character is consumed even with the queue full — it needs no
    // room, and a program that has stopped reading its input is precisely the
    // one a ^C must reach. Raw mode keeps ISIG unless it is cleared.
    let w = master_try_write(m5, b"\x03").expect("^C into a full queue");
    assert_eq!(
        w.signals,
        vec![2u8],
        "the signal is decided with the queue full"
    );
    assert_eq!(
        master_try_write(m5, b"z").map(|w| w.written),
        Ok(1),
        "and the flush it performed made room"
    );

    // The hangup outranks the full queue, which is the asymmetry against
    // `master_try_read` (that one drains before reporting EIO). Bytes handed to
    // a dead slave will never be read by anybody, so there is nothing to
    // preserve by reporting the space first — whereas a dying program's last
    // line really is its output.
    assert!(close(s5).is_empty(), "closing a slave hangs nobody up");
    assert_eq!(
        master_try_write(m5, b"z"),
        Err(KernelError::ChannelClosed),
        "a closed slave is EPIPE, checked before the queue's state"
    );
    let _ = close(m5);
    assert!(!tty::exists(id5), "device removed after both ends closed");

    // --- the original pty is still intact and gets cleaned up --------------
    let _ = close(m);
    let _ = close(s);
    assert!(!tty::exists(id), "device removed after both ends closed");
    crate::serial_println!("[pty] Self-test PASSED");
    Ok(())
}
