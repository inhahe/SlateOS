// Byte and message counts in this file are bounded -- a message by `MSGMAX`,
// a queue's bytes and messages by its `qbytes`, the totals by the sum of the
// queues' -- and every sum is checked against its bound before it is made, or
// undone by a subtraction of what an earlier sum added.  Clippy cannot see
// across the checks.
#![allow(clippy::arithmetic_side_effects)]
//! System V message queues (`<sys/msg.h>`): Linux 6.6's semantics
//! (ipc/msg.c) -- inside one process.
//!
//! Queues are kept in this process: a key names the same queue for every
//! thread of the program, but another program using the key gets a queue of
//! its own. Sharing them between programs is open question D-Q3
//! (`open-questions.md`); everything else is Linux's:
//!
//! - **Limits** are Linux's defaults: a message holds up to [`MSGMAX`] (8192)
//!   bytes of text; a queue holds `msg_qbytes` bytes -- [`MSGMNB`] (16384)
//!   when it is made, more through `IPC_SET` only with `CAP_SYS_RESOURCE` --
//!   and as many messages as it has bytes; a process may hold [`MSGMNI`]
//!   (32000) queues. Each message is allocated at its own size.
//! - **Permissions** are Linux's `ipcperms`: the owner, group or other bits
//!   of the queue's mode, chosen by the caller's effective ids, with
//!   `CAP_IPC_OWNER` granting any; `msgget` on an existing key asks for the
//!   bits in its flags. `IPC_SET` and `IPC_RMID` are the owner's or the
//!   creator's, or need `CAP_SYS_ADMIN` (`EPERM`). Ids, permissions and the
//!   table of slots are [`crate::sysv_ipc`]'s, shared with the semaphores.
//! - **Errors come in the kernel's order.** `msgsnd` reads the message type
//!   before anything else, so a NULL buffer is `EFAULT` first, then judges
//!   the size, the id and the type. `msgrcv` judges the id, the permission
//!   and the queue, and reaches the buffer last: a NULL one takes the message
//!   and then fails with `EFAULT`, as Linux's `do_msg_fill` does. `IPC_SET`
//!   reads its buffer before it looks the queue up; `IPC_STAT` writes it
//!   after.
//! - **`IPC_INFO`, `MSG_INFO`, `MSG_STAT` and `MSG_STAT_ANY`** answer as
//!   Linux's do, which is what `ipcs -q` needs to list the queues.
//! - **Blocking** sleeps on a futex until the queues change; a queue removed
//!   under a blocked call makes it fail with `EIDRM`.
//!
//! ## What changed on 2026-09-26
//!
//! The queues were a static pool: 8 queues of 32 messages of up to 256
//! bytes, with a default `msg_qbytes` of 8192. `msgrcv` refused any buffer
//! longer than 256 bytes with `EINVAL`, so the ordinary
//! `msgrcv(q, &m, sizeof m.mtext, 0, 0)` with a `mtext[8192]` failed however
//! small the message. A blocked call spun without yielding; permissions were
//! stored and never checked; `IPC_SET` clamped `msg_qbytes` silently instead
//! of refusing; the timestamps and pids were always 0; `IPC_INFO`,
//! `MSG_INFO` and `MSG_STAT` were `EINVAL`; and a NULL buffer was `EFAULT`
//! before the id, the queue or (for `IPC_SET`) nothing at all was looked at.
//! (`known-issues.md` -> `B-D-SYSV-MSG-LIMITS-PERMISSIONS-AND-ERROR-ORDER`.)
//!
//! ## Message layout
//!
//! The classic Linux `msgbuf` is:
//!
//! ```c
//! struct msgbuf {
//!     long mtype;       /* Message type, must be > 0 */
//!     char mtext[1];    /* Message data */
//! };
//! ```
//!
//! `msgsz` is the size of `mtext`, **excluding** the `mtype` header.
//! `msgsnd` reads `mtype` from the first `sizeof(long)` (= 8) bytes and the
//! text from the next `msgsz`; `msgrcv` writes them back the same way.
//!
//! ## Receive selection
//!
//!   * `msgtyp == 0`  → the first (oldest) message of any type.
//!   * `msgtyp > 0`   → the first message of type `msgtyp`; with
//!     `MSG_EXCEPT`, the first of any other type.
//!   * `msgtyp < 0`   → the first message of the lowest type ≤ `|msgtyp|`
//!     (`MSG_EXCEPT` does not apply).
//!   * `MSG_COPY`     → `msgtyp` is a 0-based position in queue order, and
//!     the message is copied, not taken; it needs `IPC_NOWAIT`.
//!
//! A message longer than the buffer fails the call with `E2BIG` and stays
//! queued; with `MSG_NOERROR` it is cut to the buffer and taken.

use crate::errno;
use crate::interrupt::Mark;
use crate::linux_ipc::{IPC_INFO, IpcPerm};
use crate::lowlevellock::Waited;
use crate::objtable::{Slots, Waits};
use crate::perprocess::process_global;
use crate::sysv_ipc::{
    Caller, Perm, S_IRUGO, S_IWUGO, SEQ_MASK, caller, decode_id, encode_id, may_control, now_secs,
    permits,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Create if key doesn't exist.
pub const IPC_CREAT: i32 = 0o1000;
/// Fail if key exists.
pub const IPC_EXCL: i32 = 0o2000;
/// No wait on operations.
pub const IPC_NOWAIT: i32 = 0o4000;

/// Remove identifier.
pub const IPC_RMID: i32 = 0;
/// Set options.
pub const IPC_SET: i32 = 1;
/// Get options.
pub const IPC_STAT: i32 = 2;

/// Private key (create new unique queue).
pub const IPC_PRIVATE: i32 = 0;

/// Truncate oversized messages instead of failing.
pub const MSG_NOERROR: i32 = 0o10000;
/// `msgrcv`: read any message *except* the given type.
pub const MSG_EXCEPT: i32 = 0o20000;
/// `msgrcv`: copy message at an absolute queue index (without dequeue).
pub const MSG_COPY: i32 = 0o40000;

/// `msgctl`: the queue at an index of the table, not an id -- what `ipcs`
/// walks, from 0 to what `MSG_INFO` returns. Linux-specific.
pub const MSG_STAT: i32 = 11;
/// `msgctl`: the limits, and how much of them is in use. Linux-specific.
pub const MSG_INFO: i32 = 12;
/// `msgctl`: `MSG_STAT` without its read-permission check (Linux 4.17).
pub const MSG_STAT_ANY: i32 = 13;

/// The longest message text, in bytes (Linux's `kernel.msgmax`).
pub const MSGMAX: usize = 8192;
/// A new queue's `msg_qbytes` (`kernel.msgmnb`); `IPC_SET` may go past it
/// only with `CAP_SYS_RESOURCE`.
pub const MSGMNB: usize = 16384;
/// The most queues a process may hold (`kernel.msgmni`).
pub const MSGMNI: usize = 32000;

/// `struct msginfo`'s fixed figures, as include/uapi/linux/msg.h defines
/// them: the message segment size, the pool (in KiB), the map and the
/// header count, and the segment count, capped at `0xffff`.
const MSGSSZ: i32 = 16;
const MSGPOOL: i32 = (MSGMNI * MSGMNB / 1024) as i32;
const MSGMAP: i32 = MSGMNB as i32;
const MSGTQL: i32 = MSGMNB as i32;
const MSGSEG: u16 = {
    let segs = (MSGPOOL as usize * 1024) / MSGSSZ as usize;
    if segs <= 0xffff { segs as u16 } else { 0xffff }
};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// `struct msqid_ds` — message queue data structure.
///
/// Provides metadata about a message queue.  Used as the buffer arg to
/// `msgctl(IPC_STAT, ...)` and `msgctl(IPC_SET, ...)`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MsqidDs {
    /// Permissions, as a nested `struct ipc_perm` — the C library's layout.
    ///
    /// These five values used to be flattened into `msg_perm_uid` and friends,
    /// which made the structure 80 bytes against musl's 120 and put every
    /// field after them at the wrong offset. Found by
    /// `scripts/check-libc-abi.py`; `design-decisions.md` §1011.
    pub msg_perm: IpcPerm,
    /// Time of the last `msgsnd`.
    pub msg_stime: i64,
    /// Time of the last `msgrcv`.
    pub msg_rtime: i64,
    /// Time of the last change.
    pub msg_ctime: i64,
    /// Bytes currently queued.
    pub msg_cbytes: usize,
    /// Messages currently queued.
    pub msg_qnum: usize,
    /// Maximum bytes allowed on the queue.
    pub msg_qbytes: usize,
    /// PID of the last sender.
    pub msg_lspid: i32,
    /// PID of the last receiver.
    pub msg_lrpid: i32,
    /// musl's trailing `unsigned long __unused[2]`.
    __unused: [u64; 2],
}

/// `struct msginfo`: what `IPC_INFO` and `MSG_INFO` write where their
/// `msgctl` buffer points.
///
/// The two answer with the same limits and differ in three fields: for
/// `IPC_INFO` they are Linux's fixed figures, for `MSG_INFO` the current
/// use -- `msgpool` the queues in use, `msgmap` the messages on them,
/// `msgtql` their bytes.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Msginfo {
    /// `IPC_INFO`: the pool size in KiB. `MSG_INFO`: queues in use.
    pub msgpool: i32,
    /// `IPC_INFO`: the map size. `MSG_INFO`: messages on all queues.
    pub msgmap: i32,
    /// The longest message text ([`MSGMAX`]).
    pub msgmax: i32,
    /// A new queue's byte limit ([`MSGMNB`]).
    pub msgmnb: i32,
    /// The most queues ([`MSGMNI`]).
    pub msgmni: i32,
    /// The message segment size.
    pub msgssz: i32,
    /// `IPC_INFO`: the header count. `MSG_INFO`: bytes on all queues.
    pub msgtql: i32,
    /// The segment count.
    pub msgseg: u16,
    /// The C structure's trailing padding, spelled out so that it is written
    /// as zero, as Linux's `memset` leaves it.
    _pad: u16,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// One message: a `malloc` block of this header followed by `len` bytes of
/// text.
#[repr(C)]
struct Msg {
    next: *mut Msg,
    mtype: i64,
    len: usize,
}

/// Where a message's text starts: right after its header, in its block.
fn text_of(m: *mut Msg) -> *mut u8 {
    m.cast::<u8>().wrapping_add(size_of::<Msg>())
}

struct Queue {
    live: bool,
    /// Key, the slot's reuse count, owner, creator and mode.
    perm: Perm,
    qbytes: usize,
    cbytes: usize,
    qnum: usize,
    stime: i64,
    rtime: i64,
    ctime: i64,
    lspid: i32,
    lrpid: i32,
    /// The messages, oldest first.
    head: *mut Msg,
    tail: *mut Msg,
}

impl Queue {
    const EMPTY: Self = Self {
        live: false,
        perm: Perm::EMPTY,
        qbytes: 0,
        cbytes: 0,
        qnum: 0,
        stime: 0,
        rtime: 0,
        ctime: 0,
        lspid: 0,
        lrpid: 0,
        head: core::ptr::null_mut(),
        tail: core::ptr::null_mut(),
    };
}

/// The queues, and the messages and bytes on them (`MSG_INFO`).
struct Tables {
    slots: Slots<Queue>,
    msgs: usize,
    bytes: usize,
}

process_global! {
    /// This process's queues.
    ///
    /// Per-thread on the host, for test isolation: a test that counts
    /// queues is broken by any concurrent one, and no lock fixes that.
    fn tables() -> Tables = Tables { slots: Slots::EMPTY, msgs: 0, bytes: 0 };

    /// Serialises every use of the tables, with the same scope as they have
    /// (see [`crate::perprocess::PoolLock`]).
    fn msg_lock() -> crate::perprocess::PoolLock = crate::perprocess::PoolLock::new();

    /// What blocked calls sleep on.
    fn waits() -> Waits = Waits::new();
}

/// Holds the tables' lock; the tables are reached through it.
struct Locked {
    _guard: crate::perprocess::PoolGuard<'static>,
    t: *mut Tables,
}

fn lock() -> Locked {
    Locked {
        // SAFETY: `msg_lock()` is this context's lock, valid as long as the
        // tables it guards.
        _guard: unsafe { crate::perprocess::lock_pool(msg_lock()) },
        t: tables(),
    }
}

impl Locked {
    fn tables(&mut self) -> &mut Tables {
        // SAFETY: the lock is held, and `t` is this context's tables.
        unsafe { &mut *self.t }
    }

    fn queue(&mut self, slot: usize) -> Option<&mut Queue> {
        self.tables().slots.get(slot)
    }

    /// The id naming the queue in `slot` now.
    fn id_of(&mut self, slot: usize) -> i32 {
        let seq = self.queue(slot).map_or(0, |q| q.perm.seq);
        encode_id(slot, seq)
    }

    /// The slot of the live queue `msqid` names.
    fn resolve(&mut self, msqid: i32) -> Option<usize> {
        let (slot, seq) = decode_id(msqid)?;
        self.queue(slot)
            .is_some_and(|q| q.live && q.perm.seq & SEQ_MASK == seq)
            .then_some(slot)
    }

    /// The slot at table index `index`, if a queue is there (`MSG_STAT`).
    fn at_index(&mut self, index: i32) -> Option<usize> {
        let slot = usize::try_from(index).ok()?;
        self.queue(slot).is_some_and(|q| q.live).then_some(slot)
    }

    fn find_key(&mut self, key: i32) -> Option<usize> {
        let cap = self.tables().slots.cap();
        (0..cap).find(|&i| self.queue(i).is_some_and(|q| q.live && q.perm.key == key))
    }

    /// Linux's `newque`: a queue keyed `key`, with `msgflg`'s permission
    /// bits, owned and created by `who`.
    fn alloc_queue(&mut self, key: i32, msgflg: i32, who: Caller) -> Result<usize, i32> {
        let t = self.tables();
        if t.slots.live() >= MSGMNI {
            return Err(errno::ENOSPC);
        }
        let slot = t
            .slots
            .claim(MSGMNI, || Queue::EMPTY)
            .ok_or(errno::ENOMEM)?;
        let now = now_secs();
        let q = self.queue(slot).ok_or(errno::ENOMEM)?;
        let seq = q.perm.seq;
        *q = Queue {
            live: true,
            perm: Perm::new(key, seq, msgflg, who),
            qbytes: MSGMNB,
            ctime: now,
            ..Queue::EMPTY
        };
        Ok(slot)
    }

    /// Free the queue in `slot` and its messages; its id stops resolving.
    fn remove(&mut self, slot: usize) {
        let Some(q) = self.queue(slot) else { return };
        let (msgs, bytes) = (q.qnum, q.cbytes);
        let mut m = q.head;
        while !m.is_null() {
            // SAFETY: every message on the list is a live block it owns.
            let next = unsafe { (*m).next };
            free_msg(m);
            m = next;
        }
        let seq = q.perm.seq.wrapping_add(1);
        *q = Queue {
            perm: Perm { seq, ..Perm::EMPTY },
            ..Queue::EMPTY
        };
        let t = self.tables();
        t.slots.release(slot);
        t.msgs -= msgs;
        t.bytes -= bytes;
    }

    /// `msgctl_info`: the figures, and the highest index in use (0 if none).
    fn info(&mut self, current: bool) -> (Msginfo, i32) {
        let cap = self.tables().slots.cap();
        let max_idx = (0..cap)
            .rev()
            .find(|&i| self.queue(i).is_some_and(|q| q.live))
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(0);
        let t = self.tables();
        let clamp = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
        let info = Msginfo {
            msgpool: if current {
                clamp(t.slots.live())
            } else {
                MSGPOOL
            },
            msgmap: if current { clamp(t.msgs) } else { MSGMAP },
            msgmax: MSGMAX as i32,
            msgmnb: MSGMNB as i32,
            msgmni: MSGMNI as i32,
            msgssz: MSGSSZ,
            msgtql: if current { clamp(t.bytes) } else { MSGTQL },
            msgseg: MSGSEG,
            _pad: 0,
        };
        (info, max_idx)
    }
}

/// A message block of type `mtype` holding `len` bytes copied from `text`,
/// or `None` when memory runs out.
///
/// # Safety
///
/// `text` is readable for `len` bytes.
unsafe fn new_msg(mtype: i64, text: *const u8, len: usize) -> Option<*mut Msg> {
    let m = crate::malloc::malloc(size_of::<Msg>().checked_add(len)?).cast::<Msg>();
    if m.is_null() {
        return None;
    }
    // SAFETY: `m` is a fresh block of a header and `len` bytes; the caller
    // vouches for `text`.
    unsafe {
        m.write(Msg {
            next: core::ptr::null_mut(),
            mtype,
            len,
        });
        if len > 0 {
            core::ptr::copy_nonoverlapping(text, text_of(m), len);
        }
    }
    Some(m)
}

fn free_msg(m: *mut Msg) {
    // SAFETY: `m` is a block from `new_msg`, freed once.
    unsafe { crate::malloc::free(m.cast()) };
}

// ---------------------------------------------------------------------------
// Waiting
// ---------------------------------------------------------------------------

/// Tell the calls asleep on the queues that something changed.
fn changed() {
    // SAFETY: this process's counter.
    unsafe { &*waits() }.changed();
}

/// Sleep until the queues change from `seen` -- or until a signal handler
/// has run on this thread since `mark`, the call's start, `SA_RESTART` or
/// not: Linux never restarts `msgsnd` or `msgrcv`, and a signal that comes
/// at any point in them ends their next sleep ([`crate::interrupt`]).
fn wait_for_change(seen: i32, mark: Mark) -> Waited {
    // SAFETY: this process's counter.
    unsafe { &*waits() }.wait_interruptible(seen, None, mark)
}

/// The count a waiter compares against, read -- with the lock held --
/// before it looks at a queue.
fn change_seen() -> i32 {
    // SAFETY: this process's counter.
    unsafe { &*waits() }.seen()
}

// ---------------------------------------------------------------------------
// msgget
// ---------------------------------------------------------------------------

/// `msgget` — get a message queue identifier.
///
/// Returns the msqid, or `-1` with `errno`: `ENOENT` for a key with no queue
/// and no `IPC_CREAT`; `EEXIST` for one with a queue and `IPC_CREAT |
/// IPC_EXCL`; `EACCES` when the existing queue's mode refuses the bits in
/// `msgflg`; `ENOSPC` past [`MSGMNI`] queues; `ENOMEM`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn msgget(key: i32, msgflg: i32) -> i32 {
    let who = caller();
    let mut t = lock();
    let result = if key == IPC_PRIVATE {
        t.alloc_queue(key, msgflg, who)
    } else if let Some(slot) = t.find_key(key) {
        if msgflg & IPC_CREAT != 0 && msgflg & IPC_EXCL != 0 {
            Err(errno::EEXIST)
        } else if t
            .queue(slot)
            .is_some_and(|q| permits(&q.perm, who, msgflg as u32))
        {
            Ok(slot)
        } else {
            Err(errno::EACCES)
        }
    } else if msgflg & IPC_CREAT == 0 {
        Err(errno::ENOENT)
    } else {
        t.alloc_queue(key, msgflg, who)
    };
    match result {
        Ok(slot) => t.id_of(slot),
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// msgsnd
// ---------------------------------------------------------------------------

/// `msgsnd` — send a message to a queue.
///
/// `msgp` points to a `struct msgbuf` whose first 8 bytes are the `mtype`
/// (an `i64`, which must be > 0) and whose next `msgsz` bytes are the text.
///
/// Errors, in Linux's order: `EFAULT` for a NULL `msgp`; `EINVAL` for
/// `msgsz` past [`MSGMAX`], a negative `msqid`, or `mtype < 1`; `ENOMEM`;
/// `EINVAL` for an id naming no queue; `EACCES` without write permission;
/// `EAGAIN` for a full queue with `IPC_NOWAIT`; `EIDRM` when the queue is
/// removed while the call waits.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn msgsnd(msqid: i32, msgp: *const u8, msgsz: usize, msgflg: i32) -> i32 {
    let mark = Mark::now();
    match send(msqid, msgp, msgsz, msgflg, |seen| {
        wait_for_change(seen, mark)
    }) {
        Ok(()) => 0,
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

/// `msgsnd`, sleeping through `wait` (a test stands in another thread there).
fn send(
    msqid: i32,
    msgp: *const u8,
    msgsz: usize,
    msgflg: i32,
    wait: impl FnMut(i32) -> Waited,
) -> Result<(), i32> {
    if msgp.is_null() {
        // `get_user(mtype, &msgp->mtype)`, before anything is judged.
        return Err(errno::EFAULT);
    }
    // SAFETY: the caller's `msgbuf` begins with its `long` type.
    let mtype = unsafe { msgp.cast::<i64>().read_unaligned() };
    if msgsz > MSGMAX || msqid < 0 || mtype < 1 {
        return Err(errno::EINVAL);
    }
    // `load_msg`: the text is copied in before the queue is looked up.
    // SAFETY: the caller's contract -- `msgsz` bytes of text follow the type.
    let msg = unsafe { new_msg(mtype, msgp.wrapping_add(8), msgsz) }.ok_or(errno::ENOMEM)?;
    let sent = enqueue(msqid, msg, msgsz, msgflg, wait);
    if sent.is_err() {
        free_msg(msg);
    }
    sent
}

/// Linux's `msg_fits_inqueue`: the bytes, and the message count, both
/// within `qbytes` -- so a queue holds as many messages as it has bytes.
fn fits(q: &Queue, len: usize) -> bool {
    len.checked_add(q.cbytes).is_some_and(|b| b <= q.qbytes)
        && q.qnum.checked_add(1).is_some_and(|n| n <= q.qbytes)
}

/// Put `msg` (of `len` bytes) on the queue `msqid`, waiting for room unless
/// `IPC_NOWAIT`.  On `Err` the message is still the caller's.
fn enqueue(
    msqid: i32,
    msg: *mut Msg,
    len: usize,
    msgflg: i32,
    mut wait: impl FnMut(i32) -> Waited,
) -> Result<(), i32> {
    let who = caller();
    let mut t = lock();
    let mut slot = t.resolve(msqid).ok_or(errno::EINVAL)?;
    loop {
        let seen = change_seen();
        let q = t.queue(slot).ok_or(errno::EIDRM)?;
        if !permits(&q.perm, who, S_IWUGO) {
            return Err(errno::EACCES);
        }
        if fits(q, len) {
            break;
        }
        if msgflg & IPC_NOWAIT != 0 {
            return Err(errno::EAGAIN);
        }
        drop(t);
        let waited = wait(seen);
        t = lock();
        // It was there before the wait: if it is not now, it was removed.
        slot = t.resolve(msqid).ok_or(errno::EIDRM)?;
        if waited == Waited::Interrupted {
            return Err(errno::EINTR);
        }
    }
    let (now, pid) = (now_secs(), crate::process::getpid());
    let q = t.queue(slot).ok_or(errno::EIDRM)?;
    if q.tail.is_null() {
        q.head = msg;
    } else {
        // SAFETY: `tail` is the queue's live last message.
        unsafe { (*q.tail).next = msg };
    }
    q.tail = msg;
    q.qnum += 1;
    q.cbytes += len;
    q.stime = now;
    q.lspid = pid;
    let tables = t.tables();
    tables.msgs += 1;
    tables.bytes += len;
    drop(t);
    changed();
    Ok(())
}

// ---------------------------------------------------------------------------
// msgrcv
// ---------------------------------------------------------------------------

/// How `find_msg` matches a message against the type asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Search {
    /// The first message.
    Any,
    /// The message at a position (`MSG_COPY`).
    Number,
    /// The first of the lowest type up to the one asked for.
    LessEqual,
    /// The first of the type asked for.
    Equal,
    /// The first of any other type (`MSG_EXCEPT`).
    NotEqual,
}

/// Linux's `convert_mode`: how to search, and for what.
fn convert_mode(msgtyp: i64, msgflg: i32) -> (Search, i64) {
    if msgflg & MSG_COPY != 0 {
        (Search::Number, msgtyp)
    } else if msgtyp == 0 {
        (Search::Any, 0)
    } else if msgtyp < 0 {
        // -LONG_MIN does not exist; Linux searches up to LONG_MAX.
        (Search::LessEqual, msgtyp.checked_neg().unwrap_or(i64::MAX))
    } else if msgflg & MSG_EXCEPT != 0 {
        (Search::NotEqual, msgtyp)
    } else {
        (Search::Equal, msgtyp)
    }
}

/// Linux's `find_msg`: the message a receive searching `mode` for `typ`
/// takes, with the one before it (null for the first).
fn find_msg(q: &Queue, typ: i64, mode: Search) -> Option<(*mut Msg, *mut Msg)> {
    let mut typ = typ;
    let mut count: i64 = 0;
    let mut found = None;
    let mut prev: *mut Msg = core::ptr::null_mut();
    let mut m = q.head;
    while !m.is_null() {
        // SAFETY: every message on the list is a live block of this queue's.
        let (mt, next) = unsafe { ((*m).mtype, (*m).next) };
        let hit = match mode {
            Search::Any | Search::Number => true,
            Search::LessEqual => mt <= typ,
            Search::Equal => mt == typ,
            Search::NotEqual => mt != typ,
        };
        if hit {
            match mode {
                // Keep looking for a lower type; 1 is the lowest there is.
                Search::LessEqual if mt != 1 => {
                    typ = mt - 1;
                    found = Some((prev, m));
                }
                Search::Number => {
                    if typ == count {
                        return Some((prev, m));
                    }
                }
                _ => return Some((prev, m)),
            }
            count += 1;
        }
        prev = m;
        m = next;
    }
    found
}

/// `msgrcv` — receive a message from a queue.
///
/// Returns the number of text bytes written after the 8-byte type, or `-1`
/// with `errno`. Errors, in Linux's order: `EINVAL` for a negative `msqid`
/// or a `msgsz` past `isize::MAX`, and for `MSG_COPY` without `IPC_NOWAIT`
/// or with `MSG_EXCEPT`; `EFAULT` for `MSG_COPY` into a NULL buffer; `EINVAL`
/// for an id naming no queue; `EACCES` without read permission; `E2BIG` for
/// a message longer than `msgsz` without `MSG_NOERROR`; `ENOMSG` for no
/// message with `IPC_NOWAIT`; `EIDRM` when the queue is removed while the
/// call waits; and `EFAULT` for a NULL buffer -- after the message has been
/// taken, which is then lost, as on Linux.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn msgrcv(
    msqid: i32,
    msgp: *mut u8,
    msgsz: usize,
    msgtyp: i64,
    msgflg: i32,
) -> isize {
    let mark = Mark::now();
    match receive(msqid, msgp, msgsz, msgtyp, msgflg, |seen| {
        wait_for_change(seen, mark)
    }) {
        Ok(n) => isize::try_from(n).unwrap_or(isize::MAX),
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

/// Write a message's type and the first `n` bytes of its text to `msgp`.
///
/// # Safety
///
/// `msgp` is writable for 8 + `n` bytes, and `m` holds at least `n`.
unsafe fn fill(msgp: *mut u8, m: *mut Msg, n: usize) {
    // SAFETY: the caller's contract.
    unsafe {
        msgp.cast::<i64>().write_unaligned((*m).mtype);
        if n > 0 {
            core::ptr::copy_nonoverlapping(text_of(m), msgp.add(8), n);
        }
    }
}

/// `msgrcv`, sleeping through `wait` (a test stands in another thread there).
fn receive(
    msqid: i32,
    msgp: *mut u8,
    msgsz: usize,
    msgtyp: i64,
    msgflg: i32,
    mut wait: impl FnMut(i32) -> Waited,
) -> Result<usize, i32> {
    if msqid < 0 || isize::try_from(msgsz).is_err() {
        return Err(errno::EINVAL);
    }
    let copy = msgflg & MSG_COPY != 0;
    if copy {
        // A look at a position has nothing to wait for, and MSG_EXCEPT
        // selects by type where MSG_COPY selects by position.
        if msgflg & MSG_EXCEPT != 0 || msgflg & IPC_NOWAIT == 0 {
            return Err(errno::EINVAL);
        }
        // `prepare_copy` reads the caller's buffer before the queue is
        // looked up.
        if msgp.is_null() && msgsz.min(MSGMAX) > 0 {
            return Err(errno::EFAULT);
        }
    }
    let (mode, typ) = convert_mode(msgtyp, msgflg);
    let who = caller();
    let mut t = lock();
    let mut slot = t.resolve(msqid).ok_or(errno::EINVAL)?;
    let taken = loop {
        let seen = change_seen();
        let q = t.queue(slot).ok_or(errno::EIDRM)?;
        if !permits(&q.perm, who, S_IRUGO) {
            return Err(errno::EACCES);
        }
        if let Some((prev, m)) = find_msg(q, typ, mode) {
            // SAFETY: `m` is one of this queue's live messages.
            let len = unsafe { (*m).len };
            if msgsz < len && msgflg & MSG_NOERROR == 0 {
                return Err(errno::E2BIG);
            }
            if copy {
                // `copy_msg` into a buffer of `min(msgsz, MSGMAX)`: a longer
                // message does not fit, even with MSG_NOERROR.
                if len > msgsz.min(MSGMAX) {
                    return Err(errno::EINVAL);
                }
                if msgp.is_null() {
                    return Err(errno::EFAULT);
                }
                // SAFETY: the caller's buffer holds 8 + msgsz >= 8 + len.
                unsafe { fill(msgp, m, len) };
                return Ok(len);
            }
            // SAFETY: `prev` is null or the message before `m`.
            unsafe {
                let next = (*m).next;
                if prev.is_null() {
                    q.head = next;
                } else {
                    (*prev).next = next;
                }
            }
            if q.tail == m {
                q.tail = prev;
            }
            q.qnum -= 1;
            q.cbytes -= len;
            q.rtime = now_secs();
            q.lrpid = crate::process::getpid();
            break (m, len);
        }
        if msgflg & IPC_NOWAIT != 0 {
            return Err(errno::ENOMSG);
        }
        drop(t);
        let waited = wait(seen);
        t = lock();
        // It was there before the wait: if it is not now, it was removed.
        slot = t.resolve(msqid).ok_or(errno::EIDRM)?;
        if waited == Waited::Interrupted {
            return Err(errno::EINTR);
        }
    };
    let (m, len) = taken;
    let tables = t.tables();
    tables.msgs -= 1;
    tables.bytes -= len;
    drop(t);
    changed();
    // `do_msg_fill`, once the message has left the queue.
    let result = if msgp.is_null() {
        Err(errno::EFAULT)
    } else {
        let n = len.min(msgsz);
        // SAFETY: the caller's buffer holds 8 + msgsz bytes; `m` holds `len`.
        unsafe { fill(msgp, m, n) };
        Ok(n)
    };
    free_msg(m);
    result
}

// ---------------------------------------------------------------------------
// msgctl
// ---------------------------------------------------------------------------

/// `msgctl` — message queue control operations.
///
///   * `IPC_STAT` — `*buf` = the queue's state; needs read permission.
///   * `IPC_SET`  — the owner, group and mode bits, and `msg_qbytes`, from
///     `*buf`; the owner's or the creator's (else `CAP_SYS_ADMIN`), and past
///     [`MSGMNB`] needs `CAP_SYS_RESOURCE`.
///   * `IPC_RMID` — remove the queue; the same ownership as `IPC_SET`.
///     Calls blocked on it fail with `EIDRM`.
///   * `IPC_INFO`, `MSG_INFO` — `buf` is a [`Msginfo`]; returns the highest
///     table index in use.
///   * `MSG_STAT`, `MSG_STAT_ANY` — `IPC_STAT` of the queue at table index
///     `msqid`, returning its id; `MSG_STAT_ANY` needs no permission.
///
/// `EINVAL` first for a negative `msqid` or `cmd`, and for an unknown `cmd`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn msgctl(msqid: i32, cmd: i32, buf: *mut MsqidDs) -> i32 {
    match control(msqid, cmd, buf) {
        Ok(n) => n,
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

/// Linux's `ksys_msgctl`.
fn control(msqid: i32, cmd: i32, buf: *mut MsqidDs) -> Result<i32, i32> {
    if msqid < 0 || cmd < 0 {
        return Err(errno::EINVAL);
    }
    match cmd {
        IPC_INFO | MSG_INFO => {
            let (info, max_idx) = lock().info(cmd == MSG_INFO);
            if buf.is_null() {
                return Err(errno::EFAULT);
            }
            // SAFETY: for these commands `buf` is the caller's `struct
            // msginfo`, whatever its declared type.
            unsafe { buf.cast::<Msginfo>().write_unaligned(info) };
            Ok(max_idx)
        }
        IPC_STAT | MSG_STAT | MSG_STAT_ANY => {
            let (ds, ret) = stat(msqid, cmd)?;
            if buf.is_null() {
                return Err(errno::EFAULT);
            }
            // SAFETY: the caller's buffer is a `struct msqid_ds`.
            unsafe { buf.write_unaligned(ds) };
            Ok(ret)
        }
        IPC_SET => {
            if buf.is_null() {
                // `copy_msqid_from_user`, before the queue is looked up.
                return Err(errno::EFAULT);
            }
            // SAFETY: the caller's buffer is a `struct msqid_ds`.
            let ds = unsafe { buf.read_unaligned() };
            set(msqid, &ds).map(|()| 0)
        }
        IPC_RMID => rmid(msqid).map(|()| 0),
        _ => Err(errno::EINVAL),
    }
}

/// `msgctl_stat`: the state of the queue `msqid` names -- or, for `MSG_STAT`
/// and `MSG_STAT_ANY`, the one at index `msqid` -- and what the call returns.
fn stat(msqid: i32, cmd: i32) -> Result<(MsqidDs, i32), i32> {
    let who = caller();
    let mut t = lock();
    let slot = if cmd == IPC_STAT {
        t.resolve(msqid)
    } else {
        t.at_index(msqid)
    }
    .ok_or(errno::EINVAL)?;
    let q = t.queue(slot).ok_or(errno::EINVAL)?;
    if cmd != MSG_STAT_ANY && !permits(&q.perm, who, S_IRUGO) {
        return Err(errno::EACCES);
    }
    let ds = MsqidDs {
        msg_perm: q.perm.to_ipc_perm(),
        msg_stime: q.stime,
        msg_rtime: q.rtime,
        msg_ctime: q.ctime,
        msg_cbytes: q.cbytes,
        msg_qnum: q.qnum,
        msg_qbytes: q.qbytes,
        msg_lspid: q.lspid,
        msg_lrpid: q.lrpid,
        __unused: [0; 2],
    };
    let ret = if cmd == IPC_STAT { 0 } else { t.id_of(slot) };
    Ok((ds, ret))
}

/// `msgctl_down` for `IPC_SET`.
fn set(msqid: i32, ds: &MsqidDs) -> Result<(), i32> {
    let who = caller();
    let mut t = lock();
    let slot = t.resolve(msqid).ok_or(errno::EINVAL)?;
    let q = t.queue(slot).ok_or(errno::EINVAL)?;
    if !may_control(&q.perm, who) {
        return Err(errno::EPERM);
    }
    // Linux passes `msg_qbytes` on as an `int`: a value past `INT_MAX`
    // arrives negative, passes the limit, and is stored sign-extended -- an
    // unbounded queue. Kept, because that is what the call means on Linux.
    let qbytes = ds.msg_qbytes as i32;
    if qbytes > MSGMNB as i32
        && !crate::sys_capability::has_capability(crate::sys_capability::CAP_SYS_RESOURCE)
    {
        return Err(errno::EPERM);
    }
    q.perm
        .update(ds.msg_perm.uid, ds.msg_perm.gid, ds.msg_perm.mode)?;
    q.qbytes = i64::from(qbytes) as usize;
    q.ctime = now_secs();
    drop(t);
    // A larger queue may take a blocked sender's message; stricter
    // permissions may refuse a blocked receiver.
    changed();
    Ok(())
}

/// `msgctl_down` for `IPC_RMID`.
fn rmid(msqid: i32) -> Result<(), i32> {
    let who = caller();
    let mut t = lock();
    let slot = t.resolve(msqid).ok_or(errno::EINVAL)?;
    if !t.queue(slot).is_some_and(|q| may_control(&q.perm, who)) {
        return Err(errno::EPERM);
    }
    t.remove(slot);
    drop(t);
    // Calls blocked on it find it gone: `EIDRM`.
    changed();
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::vec::Vec;

    // -- helpers --

    /// An 8-byte type followed by `body`, as `msgsnd` reads it.
    fn make_msg(mtype: i64, body: &[u8]) -> Vec<u8> {
        let mut v = Vec::with_capacity(8 + body.len());
        v.extend_from_slice(&mtype.to_ne_bytes());
        v.extend_from_slice(body);
        v
    }

    fn new_queue() -> i32 {
        let q = msgget(IPC_PRIVATE, 0o600 | IPC_CREAT);
        assert!(q > 0, "msgget failed: {}", errno::get_errno());
        q
    }

    fn send_ok(q: i32, mtype: i64, body: &[u8]) {
        let m = make_msg(mtype, body);
        assert_eq!(
            msgsnd(q, m.as_ptr(), body.len(), IPC_NOWAIT),
            0,
            "msgsnd: {}",
            errno::get_errno()
        );
    }

    /// Receive into a buffer of `size` text bytes: the type and the text, or
    /// the errno.
    fn recv(q: i32, size: usize, typ: i64, flags: i32) -> Result<(i64, Vec<u8>), i32> {
        let mut buf = std::vec![0u8; 8 + size];
        let r = msgrcv(q, buf.as_mut_ptr(), size, typ, flags);
        if r < 0 {
            return Err(errno::get_errno());
        }
        let n = usize::try_from(r).unwrap();
        let ty = i64::from_ne_bytes(buf[..8].try_into().unwrap());
        Ok((ty, buf[8..8 + n].to_vec()))
    }

    fn stat_of(q: i32) -> MsqidDs {
        // SAFETY: all-zero is a valid MsqidDs.
        let mut ds: MsqidDs = unsafe { core::mem::zeroed() };
        assert_eq!(
            msgctl(q, IPC_STAT, &mut ds),
            0,
            "IPC_STAT: {}",
            errno::get_errno()
        );
        ds
    }

    fn info(cmd: i32) -> (i32, Msginfo) {
        let mut mi = Msginfo::default();
        let r = msgctl(0, cmd, (&raw mut mi).cast());
        (r, mi)
    }

    fn set_ds(q: i32, edit: impl FnOnce(&mut MsqidDs)) -> i32 {
        let mut ds = stat_of(q);
        edit(&mut ds);
        msgctl(q, IPC_SET, &mut ds)
    }

    fn rmid(q: i32) {
        assert_eq!(msgctl(q, IPC_RMID, core::ptr::null_mut()), 0);
    }

    fn slot_of(q: i32) -> usize {
        decode_id(q).unwrap().0
    }

    fn fail(r: i64) -> i32 {
        assert_eq!(r, -1, "expected failure");
        errno::get_errno()
    }

    /// Restores this test thread's effective capabilities when dropped.
    struct CapGuard {
        lo: u32,
        hi: u32,
    }

    impl CapGuard {
        fn snapshot() -> Self {
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            Self { lo, hi }
        }
    }

    fn set_effective(lo: u32, hi: u32) {
        let mut hdr = crate::sys_capability::CapUserHeader {
            version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        };
        let data = [
            crate::sys_capability::CapUserData {
                effective: lo,
                permitted: u32::MAX,
                inheritable: 0,
            },
            crate::sys_capability::CapUserData {
                effective: hi,
                permitted: u32::MAX,
                inheritable: 0,
            },
        ];
        assert_eq!(crate::sys_capability::capset(&mut hdr, data.as_ptr()), 0);
    }

    impl Drop for CapGuard {
        fn drop(&mut self) {
            set_effective(self.lo, self.hi);
        }
    }

    /// Drop `cap` from this thread's effective set until the guard goes.
    fn without(cap: u32) -> CapGuard {
        let g = CapGuard::snapshot();
        let (lo, hi) = (g.lo, g.hi);
        if cap < 32 {
            set_effective(lo & !(1 << cap), hi);
        } else {
            set_effective(lo, hi & !(1 << (cap - 32)));
        }
        assert!(!crate::sys_capability::has_capability(cap));
        g
    }

    // -- constants and layouts --

    #[test]
    fn test_constants() {
        assert_eq!(IPC_CREAT, 0o1000);
        assert_eq!(IPC_EXCL, 0o2000);
        assert_eq!(IPC_NOWAIT, 0o4000);
        assert_eq!(IPC_RMID, 0);
        assert_eq!(IPC_SET, 1);
        assert_eq!(IPC_STAT, 2);
        assert_eq!(IPC_INFO, 3);
        assert_eq!(IPC_PRIVATE, 0);
        assert_eq!(MSG_NOERROR, 0o10000);
        assert_eq!(MSG_EXCEPT, 0o20000);
        assert_eq!(MSG_COPY, 0o40000);
        assert_eq!((MSG_STAT, MSG_INFO, MSG_STAT_ANY), (11, 12, 13));
        assert_eq!((MSGMAX, MSGMNB, MSGMNI), (8192, 16384, 32000));
    }

    #[test]
    fn msginfo_is_the_c_structure() {
        // Seven ints and an unsigned short, padded to 32.
        assert_eq!(size_of::<Msginfo>(), 32);
        assert_eq!(core::mem::offset_of!(Msginfo, msgseg), 28);
    }

    #[test]
    fn test_msqid_ds_layout() {
        assert_eq!(size_of::<MsqidDs>(), 120);
        assert_eq!(core::mem::offset_of!(MsqidDs, msg_stime), 48);
    }

    // -- ids --

    #[test]
    fn a_removed_queues_id_stops_resolving_even_when_its_slot_is_reused() {
        let q1 = new_queue();
        rmid(q1);
        let q2 = new_queue();
        assert_eq!(slot_of(q1), slot_of(q2), "the slot is reused");
        assert_ne!(q1, q2, "but not the id");
        let m = make_msg(1, b"x");
        assert_eq!(
            fail(msgsnd(q1, m.as_ptr(), 1, IPC_NOWAIT).into()),
            errno::EINVAL
        );
        assert_eq!(msgsnd(q2, m.as_ptr(), 1, IPC_NOWAIT), 0);
        rmid(q2);
    }

    // -- msgget --

    #[test]
    fn test_msgget_private_creates_new() {
        let a = new_queue();
        let b = new_queue();
        assert_ne!(a, b);
        rmid(a);
        rmid(b);
    }

    #[test]
    fn msgget_keys() {
        let key = 0x5157;
        assert_eq!(fail(msgget(key, 0o600).into()), errno::ENOENT);
        let q = msgget(key, 0o600 | IPC_CREAT);
        assert!(q > 0);
        assert_eq!(msgget(key, 0o600), q, "a key names its queue");
        assert_eq!(
            msgget(key, 0o600 | IPC_CREAT),
            q,
            "IPC_CREAT alone finds it"
        );
        assert_eq!(
            fail(msgget(key, 0o600 | IPC_CREAT | IPC_EXCL).into()),
            errno::EEXIST
        );
        rmid(q);
        assert_eq!(
            fail(msgget(key, 0o600).into()),
            errno::ENOENT,
            "a removed key is gone"
        );
    }

    #[test]
    fn a_new_queue_is_linuxs() {
        let q = msgget(0x7a11, 0o7640 | IPC_CREAT);
        let ds = stat_of(q);
        assert_eq!(ds.msg_perm.mode, 0o640, "only the permission bits");
        assert_eq!(ds.msg_perm.__ipc_perm_key, 0x7a11);
        assert_eq!(ds.msg_qbytes, MSGMNB);
        assert_eq!((ds.msg_qnum, ds.msg_cbytes), (0, 0));
        assert_eq!(ds.msg_perm.uid, crate::unistd::geteuid());
        assert_eq!(ds.msg_perm.cuid, crate::unistd::geteuid());
        assert_eq!(ds.msg_perm.gid, crate::unistd::getegid());
        assert_eq!(ds.msg_perm.cgid, crate::unistd::getegid());
        assert_eq!((ds.msg_stime, ds.msg_rtime), (0, 0));
        assert!(ds.msg_ctime > 0, "created now");
        rmid(q);
    }

    #[test]
    fn the_limit_is_msgmni_queues() {
        let mut qs = Vec::with_capacity(MSGMNI);
        for _ in 0..MSGMNI {
            qs.push(new_queue());
        }
        assert_eq!(fail(msgget(IPC_PRIVATE, 0o600).into()), errno::ENOSPC);
        rmid(qs.pop().unwrap());
        qs.push(new_queue());
        for q in qs {
            rmid(q);
        }
        assert_eq!(info(MSG_INFO).1.msgpool, 0);
    }

    #[test]
    fn msgget_asks_an_existing_queue_for_the_bits_in_its_flags() {
        let q = msgget(0x7a12, 0o400 | IPC_CREAT);
        let caps = without(crate::sys_capability::CAP_IPC_OWNER);
        assert_eq!(msgget(0x7a12, 0o400), q);
        assert_eq!(msgget(0x7a12, 0), q, "asking for nothing is always granted");
        assert_eq!(fail(msgget(0x7a12, 0o200).into()), errno::EACCES);
        assert_eq!(
            fail(msgget(0x7a12, 0o002).into()),
            errno::EACCES,
            "any class asks"
        );
        drop(caps);
        assert_eq!(msgget(0x7a12, 0o200), q, "CAP_IPC_OWNER grants it");
        rmid(q);
    }

    // -- msgsnd --

    #[test]
    fn test_msgsnd_msgrcv_basic_fifo() {
        let q = new_queue();
        send_ok(q, 1, b"one");
        send_ok(q, 1, b"two");
        send_ok(q, 1, b"three");
        for want in [&b"one"[..], b"two", b"three"] {
            assert_eq!(recv(q, 64, 0, IPC_NOWAIT), Ok((1, want.to_vec())));
        }
        rmid(q);
    }

    #[test]
    fn a_message_of_msgmax_bytes_goes_through_and_one_more_is_einval() {
        let q = new_queue();
        let body = std::vec![0x5a; MSGMAX];
        send_ok(q, 3, &body);
        assert_eq!(recv(q, MSGMAX, 0, IPC_NOWAIT), Ok((3, body)));
        let m = make_msg(3, &std::vec![0; MSGMAX + 1]);
        assert_eq!(
            fail(msgsnd(q, m.as_ptr(), MSGMAX + 1, IPC_NOWAIT).into()),
            errno::EINVAL
        );
        assert_eq!(stat_of(q).msg_qnum, 0);
        rmid(q);
    }

    #[test]
    fn msgsnd_reads_the_type_before_it_judges_anything() {
        // `get_user(mtype)` comes first: a NULL buffer is EFAULT whatever
        // else is wrong.
        assert_eq!(
            fail(msgsnd(-1, core::ptr::null(), 1, 0).into()),
            errno::EFAULT
        );
        assert_eq!(
            fail(msgsnd(12345, core::ptr::null(), MSGMAX + 1, 0).into()),
            errno::EFAULT
        );
    }

    #[test]
    fn msgsnd_refuses_a_type_below_one_and_ids_that_name_nothing() {
        let q = new_queue();
        for ty in [0, -5, i64::MIN] {
            let m = make_msg(ty, b"x");
            assert_eq!(
                fail(msgsnd(q, m.as_ptr(), 1, IPC_NOWAIT).into()),
                errno::EINVAL,
                "type {ty}"
            );
        }
        let m = make_msg(1, b"x");
        assert_eq!(
            fail(msgsnd(-1, m.as_ptr(), 1, IPC_NOWAIT).into()),
            errno::EINVAL
        );
        assert_eq!(
            fail(msgsnd(q + 1, m.as_ptr(), 1, IPC_NOWAIT).into()),
            errno::EINVAL
        );
        assert_eq!(stat_of(q).msg_qnum, 0);
        rmid(q);
    }

    #[test]
    fn a_full_queue_is_eagain_with_nowait() {
        let q = new_queue();
        let big = std::vec![1u8; MSGMAX];
        send_ok(q, 1, &big);
        send_ok(q, 1, &big);
        let m = make_msg(1, b"x");
        assert_eq!(
            fail(msgsnd(q, m.as_ptr(), 1, IPC_NOWAIT).into()),
            errno::EAGAIN
        );
        let ds = stat_of(q);
        assert_eq!((ds.msg_qnum, ds.msg_cbytes), (2, 2 * MSGMAX));
        rmid(q);
    }

    #[test]
    fn a_queue_holds_as_many_messages_as_it_has_bytes() {
        // `1 + q_qnum <= q_qbytes`: empty messages still count.
        let q = new_queue();
        assert_eq!(set_ds(q, |ds| ds.msg_qbytes = 3), 0);
        for _ in 0..3 {
            send_ok(q, 1, b"");
        }
        let m = make_msg(1, b"");
        assert_eq!(
            fail(msgsnd(q, m.as_ptr(), 0, IPC_NOWAIT).into()),
            errno::EAGAIN
        );
        rmid(q);
    }

    #[test]
    fn msgsnd_needs_write_permission() {
        let q = msgget(IPC_PRIVATE, 0o400);
        let m = make_msg(1, b"x");
        {
            let _g = without(crate::sys_capability::CAP_IPC_OWNER);
            assert_eq!(
                fail(msgsnd(q, m.as_ptr(), 1, IPC_NOWAIT).into()),
                errno::EACCES
            );
        }
        assert_eq!(
            msgsnd(q, m.as_ptr(), 1, IPC_NOWAIT),
            0,
            "CAP_IPC_OWNER grants it"
        );
        rmid(q);
    }

    #[test]
    fn msgsnd_and_msgrcv_record_when_and_who() {
        let q = new_queue();
        send_ok(q, 1, b"x");
        let ds = stat_of(q);
        assert!(ds.msg_stime > 0);
        assert_eq!(ds.msg_lspid, crate::process::getpid());
        assert_eq!((ds.msg_rtime, ds.msg_lrpid), (0, 0));
        recv(q, 8, 0, IPC_NOWAIT).unwrap();
        let ds = stat_of(q);
        assert!(ds.msg_rtime > 0);
        assert_eq!(ds.msg_lrpid, crate::process::getpid());
        rmid(q);
    }

    // -- msgrcv --

    #[test]
    fn msgrcv_takes_a_buffer_of_any_size() {
        // THE REGRESSION PIN: a buffer longer than 256 bytes was EINVAL, so
        // `msgrcv(q, &m, sizeof m.mtext, 0, 0)` with a `char mtext[8192]`
        // failed however small the message.
        let q = new_queue();
        send_ok(q, 2, b"small");
        send_ok(q, 2, b"small");
        assert_eq!(recv(q, 8192, 0, IPC_NOWAIT), Ok((2, b"small".to_vec())));
        assert_eq!(recv(q, 1 << 20, 0, IPC_NOWAIT), Ok((2, b"small".to_vec())));
        rmid(q);
    }

    #[test]
    fn test_msgrcv_by_type_match() {
        let q = new_queue();
        send_ok(q, 1, b"a");
        send_ok(q, 2, b"b");
        send_ok(q, 1, b"c");
        assert_eq!(recv(q, 8, 2, IPC_NOWAIT), Ok((2, b"b".to_vec())));
        assert_eq!(recv(q, 8, 1, IPC_NOWAIT), Ok((1, b"a".to_vec())));
        assert_eq!(recv(q, 8, 1, IPC_NOWAIT), Ok((1, b"c".to_vec())));
        rmid(q);
    }

    #[test]
    fn a_negative_type_takes_the_oldest_of_the_lowest_type() {
        let q = new_queue();
        send_ok(q, 5, b"five");
        send_ok(q, 3, b"three-a");
        send_ok(q, 4, b"four");
        send_ok(q, 3, b"three-b");
        assert_eq!(recv(q, 16, -4, IPC_NOWAIT), Ok((3, b"three-a".to_vec())));
        assert_eq!(recv(q, 16, -4, IPC_NOWAIT), Ok((3, b"three-b".to_vec())));
        assert_eq!(recv(q, 16, -4, IPC_NOWAIT), Ok((4, b"four".to_vec())));
        assert_eq!(
            recv(q, 16, -4, IPC_NOWAIT),
            Err(errno::ENOMSG),
            "5 is past -4"
        );
        // -LONG_MIN does not exist; Linux reads it as "any type".
        assert_eq!(recv(q, 16, i64::MIN, IPC_NOWAIT), Ok((5, b"five".to_vec())));
        rmid(q);
    }

    #[test]
    fn msg_except_takes_the_first_of_any_other_type() {
        let q = new_queue();
        send_ok(q, 1, b"a");
        send_ok(q, 2, b"b");
        assert_eq!(
            recv(q, 8, 1, IPC_NOWAIT | MSG_EXCEPT),
            Ok((2, b"b".to_vec()))
        );
        // It applies only to a positive type.
        assert_eq!(
            recv(q, 8, 0, IPC_NOWAIT | MSG_EXCEPT),
            Ok((1, b"a".to_vec()))
        );
        rmid(q);
    }

    #[test]
    fn an_empty_or_unmatched_queue_is_enomsg_with_nowait() {
        let q = new_queue();
        assert_eq!(recv(q, 8, 0, IPC_NOWAIT), Err(errno::ENOMSG));
        send_ok(q, 1, b"x");
        assert_eq!(recv(q, 8, 9, IPC_NOWAIT), Err(errno::ENOMSG));
        rmid(q);
    }

    #[test]
    fn a_long_message_is_e2big_and_stays_or_is_cut_with_noerror() {
        let q = new_queue();
        send_ok(q, 1, b"0123456789");
        assert_eq!(recv(q, 4, 0, IPC_NOWAIT), Err(errno::E2BIG));
        assert_eq!(stat_of(q).msg_qnum, 1, "it stays");
        assert_eq!(
            recv(q, 4, 0, IPC_NOWAIT | MSG_NOERROR),
            Ok((1, b"0123".to_vec()))
        );
        let ds = stat_of(q);
        assert_eq!((ds.msg_qnum, ds.msg_cbytes), (0, 0), "taken whole");
        rmid(q);
    }

    #[test]
    fn msgrcv_reaches_its_buffer_last() {
        let q = new_queue();
        let null = core::ptr::null_mut();
        // The id, then the queue, before the buffer.
        assert_eq!(
            fail(msgrcv(-1, null, 8, 0, IPC_NOWAIT) as i64),
            errno::EINVAL
        );
        assert_eq!(
            fail(msgrcv(q + 1, null, 8, 0, IPC_NOWAIT) as i64),
            errno::EINVAL
        );
        assert_eq!(
            fail(msgrcv(q, null, 8, 0, IPC_NOWAIT) as i64),
            errno::ENOMSG
        );
        // With a message there, Linux takes it and then faults writing it.
        send_ok(q, 1, b"lost");
        assert_eq!(
            fail(msgrcv(q, null, 8, 0, IPC_NOWAIT) as i64),
            errno::EFAULT
        );
        assert_eq!(stat_of(q).msg_qnum, 0, "the message is gone");
        rmid(q);
    }

    #[test]
    fn msgrcv_refuses_a_size_past_isize_max() {
        let q = new_queue();
        let mut buf = [0u8; 16];
        assert_eq!(
            fail(msgrcv(q, buf.as_mut_ptr(), usize::MAX, 0, IPC_NOWAIT) as i64),
            errno::EINVAL
        );
        rmid(q);
    }

    #[test]
    fn msgrcv_needs_read_permission() {
        let q = msgget(IPC_PRIVATE, 0o200);
        send_ok(q, 1, b"x");
        {
            let _g = without(crate::sys_capability::CAP_IPC_OWNER);
            assert_eq!(recv(q, 8, 0, IPC_NOWAIT), Err(errno::EACCES));
        }
        assert_eq!(recv(q, 8, 0, IPC_NOWAIT), Ok((1, b"x".to_vec())));
        rmid(q);
    }

    // -- MSG_COPY --

    #[test]
    fn msg_copy_returns_a_message_without_dequeuing_it() {
        // MSG_COPY used to fall through to a normal receive, so a caller that
        // asked to look at a message destroyed it.
        let q = new_queue();
        send_ok(q, 1, b"first!!!");
        send_ok(q, 2, b"second!!");
        for _ in 0..2 {
            assert_eq!(
                recv(q, 8, 0, MSG_COPY | IPC_NOWAIT),
                Ok((1, b"first!!!".to_vec()))
            );
        }
        assert_eq!(recv(q, 8, 0, IPC_NOWAIT), Ok((1, b"first!!!".to_vec())));
        rmid(q);
    }

    #[test]
    fn msg_copy_indexes_by_position_not_by_type() {
        let q = new_queue();
        send_ok(q, 10, b"aaaaaaaa");
        send_ok(q, 9, b"bbbbbbbb");
        assert_eq!(
            recv(q, 8, 1, MSG_COPY | IPC_NOWAIT),
            Ok((9, b"bbbbbbbb".to_vec()))
        );
        rmid(q);
    }

    #[test]
    fn msg_copy_past_the_end_is_enomsg() {
        let q = new_queue();
        assert_eq!(recv(q, 8, 5, MSG_COPY | IPC_NOWAIT), Err(errno::ENOMSG));
        rmid(q);
    }

    #[test]
    fn msg_copy_needs_nowait_and_refuses_msg_except() {
        let q = new_queue();
        assert_eq!(recv(q, 8, 0, MSG_COPY), Err(errno::EINVAL));
        assert_eq!(
            recv(q, 8, 0, MSG_COPY | IPC_NOWAIT | MSG_EXCEPT),
            Err(errno::EINVAL)
        );
        // Before the id is looked at.
        assert_eq!(
            fail(msgrcv(q + 1, [0u8; 16].as_mut_ptr(), 8, 0, MSG_COPY) as i64),
            errno::EINVAL
        );
        rmid(q);
    }

    #[test]
    fn msg_copy_reads_its_buffer_before_the_id() {
        // `prepare_copy` loads the caller's buffer first.
        // 5000 names no queue in this thread's table.
        let r = msgrcv(5000, core::ptr::null_mut(), 8, 0, MSG_COPY | IPC_NOWAIT);
        assert_eq!(fail(r as i64), errno::EFAULT);
    }

    #[test]
    fn msg_copy_with_noerror_into_a_short_buffer_is_einval() {
        // E2BIG without MSG_NOERROR; with it, `copy_msg` refuses a message
        // longer than the copy it was given.
        let q = new_queue();
        send_ok(q, 1, b"0123456789");
        assert_eq!(recv(q, 4, 0, MSG_COPY | IPC_NOWAIT), Err(errno::E2BIG));
        assert_eq!(
            recv(q, 4, 0, MSG_COPY | IPC_NOWAIT | MSG_NOERROR),
            Err(errno::EINVAL)
        );
        assert_eq!(stat_of(q).msg_qnum, 1);
        rmid(q);
    }

    // -- msgctl --

    #[test]
    fn test_msgctl_stat_populates() {
        let q = new_queue();
        send_ok(q, 1, b"abc");
        send_ok(q, 2, b"defgh");
        let ds = stat_of(q);
        assert_eq!((ds.msg_qnum, ds.msg_cbytes, ds.msg_qbytes), (2, 8, MSGMNB));
        assert_eq!(ds.msg_perm.mode, 0o600);
        rmid(q);
    }

    #[test]
    fn ipc_stat_writes_its_buffer_after_the_lookup() {
        let q = new_queue();
        let null = core::ptr::null_mut();
        assert_eq!(fail(msgctl(q + 1, IPC_STAT, null).into()), errno::EINVAL);
        assert_eq!(fail(msgctl(q, IPC_STAT, null).into()), errno::EFAULT);
        rmid(q);
    }

    #[test]
    fn ipc_stat_needs_read_permission() {
        let q = msgget(IPC_PRIVATE, 0o200);
        let caps = without(crate::sys_capability::CAP_IPC_OWNER);
        // SAFETY: all-zero is a valid MsqidDs.
        let mut ds: MsqidDs = unsafe { core::mem::zeroed() };
        assert_eq!(fail(msgctl(q, IPC_STAT, &mut ds).into()), errno::EACCES);
        drop(caps);
        rmid(q);
    }

    #[test]
    fn ipc_set_sets_the_owner_the_mode_and_the_size() {
        let q = new_queue();
        let before = stat_of(q).msg_ctime;
        assert_eq!(
            set_ds(q, |ds| {
                ds.msg_perm.mode = 0o7644;
                ds.msg_perm.uid = 1234;
                ds.msg_perm.gid = 5678;
                ds.msg_perm.cuid = 99; // not settable
                ds.msg_qbytes = 4096;
            }),
            0
        );
        let ds = stat_of(q);
        assert_eq!(ds.msg_perm.mode, 0o644, "only the permission bits");
        assert_eq!((ds.msg_perm.uid, ds.msg_perm.gid), (1234, 5678));
        assert_eq!(
            ds.msg_perm.cuid,
            crate::unistd::geteuid(),
            "the creator stays"
        );
        assert_eq!(ds.msg_qbytes, 4096);
        assert!(ds.msg_ctime >= before);
        rmid(q);
    }

    #[test]
    fn ipc_set_past_msgmnb_needs_cap_sys_resource() {
        let q = new_queue();
        {
            let _g = without(crate::sys_capability::CAP_SYS_RESOURCE);
            assert_eq!(
                fail(set_ds(q, |ds| ds.msg_qbytes = MSGMNB + 1).into()),
                errno::EPERM
            );
            assert_eq!(
                set_ds(q, |ds| ds.msg_qbytes = MSGMNB),
                0,
                "up to MSGMNB is anyone's"
            );
        }
        assert_eq!(set_ds(q, |ds| ds.msg_qbytes = 1 << 20), 0);
        assert_eq!(stat_of(q).msg_qbytes, 1 << 20);
        rmid(q);
    }

    #[test]
    fn ipc_set_takes_msg_qbytes_as_an_int_as_linux_does() {
        // Past INT_MAX it arrives negative, passes the limit, and is stored
        // sign-extended.
        let q = new_queue();
        let caps = without(crate::sys_capability::CAP_SYS_RESOURCE);
        assert_eq!(set_ds(q, |ds| ds.msg_qbytes = 0x8000_0000), 0);
        assert_eq!(stat_of(q).msg_qbytes, 0xFFFF_FFFF_8000_0000);
        drop(caps);
        rmid(q);
    }

    #[test]
    fn ipc_set_refuses_no_user_and_no_group() {
        let q = new_queue();
        assert_eq!(
            fail(set_ds(q, |ds| ds.msg_perm.uid = u32::MAX).into()),
            errno::EINVAL
        );
        assert_eq!(
            fail(set_ds(q, |ds| ds.msg_perm.gid = u32::MAX).into()),
            errno::EINVAL
        );
        assert_eq!(
            stat_of(q).msg_perm.uid,
            crate::unistd::geteuid(),
            "unchanged"
        );
        rmid(q);
    }

    #[test]
    fn ipc_set_reads_its_buffer_before_the_lookup() {
        assert_eq!(
            fail(msgctl(12345, IPC_SET, core::ptr::null_mut()).into()),
            errno::EFAULT
        );
    }

    #[test]
    fn ipc_set_and_ipc_rmid_are_the_owners() {
        let q = new_queue();
        // Another owner and creator than this caller.
        {
            let mut t = lock();
            let qq = t.queue(slot_of(q)).unwrap();
            qq.perm.uid = 1000;
            qq.perm.cuid = 1000;
        }
        {
            let _g = without(crate::sys_capability::CAP_SYS_ADMIN);
            assert_eq!(
                fail(set_ds(q, |ds| ds.msg_qbytes = 10).into()),
                errno::EPERM
            );
            assert_eq!(
                fail(msgctl(q, IPC_RMID, core::ptr::null_mut()).into()),
                errno::EPERM
            );
        }
        assert_eq!(set_ds(q, |ds| ds.msg_qbytes = 10), 0, "CAP_SYS_ADMIN may");
        rmid(q);
    }

    #[test]
    fn ipc_rmid_frees_the_queue_and_its_messages() {
        let q = new_queue();
        send_ok(q, 1, b"12345");
        send_ok(q, 1, b"678");
        let (_, before) = info(MSG_INFO);
        assert_eq!((before.msgpool, before.msgmap, before.msgtql), (1, 2, 8));
        rmid(q);
        let (_, after) = info(MSG_INFO);
        assert_eq!((after.msgpool, after.msgmap, after.msgtql), (0, 0, 0));
        assert_eq!(
            fail(msgctl(q, IPC_RMID, core::ptr::null_mut()).into()),
            errno::EINVAL
        );
    }

    #[test]
    fn msgctl_refuses_negative_ids_and_commands_and_unknown_ones() {
        let q = new_queue();
        let null = core::ptr::null_mut();
        assert_eq!(fail(msgctl(-1, IPC_STAT, null).into()), errno::EINVAL);
        assert_eq!(fail(msgctl(q, -1, null).into()), errno::EINVAL);
        assert_eq!(fail(msgctl(q, 99, null).into()), errno::EINVAL);
        assert_eq!(fail(msgctl(q + 1, IPC_RMID, null).into()), errno::EINVAL);
        rmid(q);
    }

    #[test]
    fn ipc_info_reports_linuxs_figures() {
        let (r, mi) = info(IPC_INFO);
        assert_eq!(r, 0, "no queue: index 0");
        assert_eq!(
            mi,
            Msginfo {
                msgpool: 512_000,
                msgmap: 16384,
                msgmax: 8192,
                msgmnb: 16384,
                msgmni: 32000,
                msgssz: 16,
                msgtql: 16384,
                msgseg: 0xffff,
                _pad: 0,
            }
        );
        let a = new_queue();
        let b = new_queue();
        assert_eq!(
            info(IPC_INFO).0,
            i32::try_from(slot_of(a).max(slot_of(b))).unwrap()
        );
        rmid(a);
        rmid(b);
    }

    #[test]
    fn msg_info_reports_what_is_in_use() {
        let a = new_queue();
        let b = new_queue();
        send_ok(a, 1, b"abc");
        send_ok(b, 1, b"de");
        send_ok(b, 1, b"");
        let (r, mi) = info(MSG_INFO);
        assert_eq!((mi.msgpool, mi.msgmap, mi.msgtql), (2, 3, 5));
        assert_eq!((mi.msgmax, mi.msgmnb, mi.msgmni), (8192, 16384, 32000));
        assert_eq!(r, i32::try_from(slot_of(a).max(slot_of(b))).unwrap());
        rmid(a);
        rmid(b);
    }

    #[test]
    fn ipc_info_writes_a_msginfo_and_no_more() {
        let mut buf = [0xAAu8; 40];
        assert_eq!(msgctl(0, IPC_INFO, buf.as_mut_ptr().cast()), 0);
        assert_eq!(&buf[30..32], &[0, 0], "the padding is zero");
        assert!(
            buf[32..].iter().all(|&b| b == 0xAA),
            "nothing past struct msginfo"
        );
        assert_eq!(
            fail(msgctl(0, IPC_INFO, core::ptr::null_mut()).into()),
            errno::EFAULT
        );
        assert_eq!(
            msgctl(4321, IPC_INFO, buf.as_mut_ptr().cast()),
            0,
            "the id is not looked at"
        );
    }

    #[test]
    fn msg_stat_walks_the_table_as_ipcs_does() {
        let a = new_queue();
        let b = new_queue();
        rmid(a);
        let c = new_queue();
        let (max, _) = info(MSG_INFO);
        let mut found = Vec::new();
        for i in 0..=max {
            // SAFETY: all-zero is a valid MsqidDs.
            let mut ds: MsqidDs = unsafe { core::mem::zeroed() };
            let r = msgctl(i, MSG_STAT, &mut ds);
            if r >= 0 {
                found.push(r);
            } else {
                assert_eq!(errno::get_errno(), errno::EINVAL, "an unused index");
            }
        }
        found.sort_unstable();
        let mut want = std::vec![b, c];
        want.sort_unstable();
        assert_eq!(found, want, "MSG_STAT returns each queue's full id");
        rmid(b);
        rmid(c);
    }

    #[test]
    fn msg_stat_any_skips_the_permission_msg_stat_needs() {
        let q = msgget(IPC_PRIVATE, 0);
        let i = i32::try_from(slot_of(q)).unwrap();
        // SAFETY: all-zero is a valid MsqidDs.
        let mut ds: MsqidDs = unsafe { core::mem::zeroed() };
        let caps = without(crate::sys_capability::CAP_IPC_OWNER);
        assert_eq!(fail(msgctl(i, MSG_STAT, &mut ds).into()), errno::EACCES);
        assert_eq!(msgctl(i, MSG_STAT_ANY, &mut ds), q);
        assert_eq!(ds.msg_perm.mode, 0);
        drop(caps);
        rmid(q);
    }

    // -- blocking, with the other thread played by `wait` --

    #[test]
    fn a_blocked_receive_takes_the_message_sent_while_it_waits() {
        let q = new_queue();
        let mut buf = [0u8; 64];
        let mut waits = 0;
        let r = receive(q, buf.as_mut_ptr(), 56, 0, 0, |_| {
            waits += 1;
            send_ok(q, 7, b"hello");
            Waited::Woken
        });
        assert_eq!(r, Ok(5));
        assert_eq!(waits, 1);
        assert_eq!(i64::from_ne_bytes(buf[..8].try_into().unwrap()), 7);
        assert_eq!(&buf[8..13], b"hello");
        rmid(q);
    }

    #[test]
    fn a_blocked_receive_waits_past_a_message_it_did_not_ask_for() {
        let q = new_queue();
        let mut buf = [0u8; 64];
        let mut waits = 0;
        let r = receive(q, buf.as_mut_ptr(), 56, 1, 0, |_| {
            waits += 1;
            send_ok(q, if waits == 1 { 2 } else { 1 }, b"x");
            Waited::Woken
        });
        assert_eq!((r, waits), (Ok(1), 2));
        assert_eq!(
            recv(q, 8, 0, IPC_NOWAIT),
            Ok((2, b"x".to_vec())),
            "the other stays"
        );
        rmid(q);
    }

    #[test]
    fn a_blocked_send_goes_when_room_is_made() {
        let q = new_queue();
        let big = std::vec![1u8; MSGMAX];
        send_ok(q, 1, &big);
        send_ok(q, 1, &big);
        let m = make_msg(2, &big);
        let r = send(q, m.as_ptr(), MSGMAX, 0, |_| {
            recv(q, MSGMAX, 0, IPC_NOWAIT).unwrap();
            Waited::Woken
        });
        assert_eq!(r, Ok(()));
        assert_eq!(stat_of(q).msg_qnum, 2);
        rmid(q);
    }

    #[test]
    fn a_blocked_send_goes_when_ipc_set_enlarges_the_queue() {
        let q = new_queue();
        let big = std::vec![1u8; MSGMAX];
        send_ok(q, 1, &big);
        send_ok(q, 1, &big);
        let m = make_msg(2, b"more");
        let r = send(q, m.as_ptr(), 4, 0, |_| {
            assert_eq!(set_ds(q, |ds| ds.msg_qbytes = 3 * MSGMAX), 0);
            Waited::Woken
        });
        assert_eq!(r, Ok(()));
        assert_eq!(stat_of(q).msg_qnum, 3);
        rmid(q);
    }

    #[test]
    fn a_queue_removed_under_a_blocked_call_is_eidrm() {
        let q = new_queue();
        let mut buf = [0u8; 16];
        assert_eq!(
            receive(q, buf.as_mut_ptr(), 8, 0, 0, |_| {
                rmid(q);
                Waited::Woken
            }),
            Err(errno::EIDRM)
        );

        let q = new_queue();
        let big = std::vec![1u8; MSGMAX];
        send_ok(q, 1, &big);
        send_ok(q, 1, &big);
        let m = make_msg(1, b"x");
        let r = send(q, m.as_ptr(), 1, 0, |_| {
            rmid(q);
            Waited::Woken
        });
        assert_eq!(r, Err(errno::EIDRM));
        let (_, mi) = info(MSG_INFO);
        assert_eq!(
            (mi.msgpool, mi.msgmap, mi.msgtql),
            (0, 0, 0),
            "nothing kept"
        );
    }

    #[test]
    fn a_queue_replaced_under_a_blocked_call_is_still_eidrm() {
        let q = new_queue();
        let mut buf = [0u8; 16];
        let mut replacement = 0;
        let r = receive(q, buf.as_mut_ptr(), 8, 0, 0, |_| {
            rmid(q);
            replacement = new_queue();
            send_ok(replacement, 1, b"not yours");
            Waited::Woken
        });
        assert_eq!(r, Err(errno::EIDRM));
        assert_eq!(slot_of(replacement), slot_of(q), "the slot was reused");
        assert_eq!(
            stat_of(replacement).msg_qnum,
            1,
            "the new queue is untouched"
        );
        rmid(replacement);
    }

    #[test]
    fn a_blocked_receive_rechecks_permission_after_ipc_set() {
        let q = new_queue();
        let caps = without(crate::sys_capability::CAP_IPC_OWNER);
        let mut buf = [0u8; 16];
        let r = receive(q, buf.as_mut_ptr(), 8, 0, 0, |_| {
            assert_eq!(set_ds(q, |ds| ds.msg_perm.mode = 0o200), 0);
            Waited::Woken
        });
        assert_eq!(r, Err(errno::EACCES));
        drop(caps);
        rmid(q);
    }

    #[test]
    fn a_wait_a_signal_handler_ends_is_eintr_and_changes_nothing() {
        let q = new_queue();
        let mut buf = [0u8; 16];
        let r = receive(q, buf.as_mut_ptr(), 8, 0, 0, |_| Waited::Interrupted);
        assert_eq!(r, Err(errno::EINTR));

        let big = std::vec![1u8; MSGMAX];
        send_ok(q, 1, &big);
        send_ok(q, 1, &big);
        let m = make_msg(2, b"x");
        let r = send(q, m.as_ptr(), 1, 0, |_| Waited::Interrupted);
        assert_eq!(r, Err(errno::EINTR));
        assert_eq!(stat_of(q).msg_qnum, 2, "the message was not sent");
        rmid(q);
    }

    #[test]
    fn a_queue_removed_while_a_handler_ran_is_eidrm_not_eintr() {
        let q = new_queue();
        let mut buf = [0u8; 16];
        let r = receive(q, buf.as_mut_ptr(), 8, 0, 0, |_| {
            rmid(q);
            Waited::Interrupted
        });
        assert_eq!(r, Err(errno::EIDRM));
    }

    #[test]
    fn every_change_moves_the_counter_waiters_sleep_on() {
        let q = new_queue();
        let before = change_seen();
        send_ok(q, 1, b"x");
        let sent = change_seen();
        assert_ne!(sent, before);
        recv(q, 8, 0, IPC_NOWAIT).unwrap();
        assert_ne!(change_seen(), sent);
        rmid(q);
    }

    // -- the pieces --

    #[test]
    fn convert_mode_is_linuxs() {
        assert_eq!(convert_mode(0, 0), (Search::Any, 0));
        assert_eq!(convert_mode(0, MSG_EXCEPT), (Search::Any, 0));
        assert_eq!(convert_mode(5, 0), (Search::Equal, 5));
        assert_eq!(convert_mode(5, MSG_EXCEPT), (Search::NotEqual, 5));
        assert_eq!(convert_mode(-5, MSG_EXCEPT), (Search::LessEqual, 5));
        assert_eq!(convert_mode(i64::MIN, 0), (Search::LessEqual, i64::MAX));
        assert_eq!(convert_mode(3, MSG_COPY | MSG_EXCEPT), (Search::Number, 3));
    }

    #[test]
    fn find_msg_picks_what_linux_picks() {
        let q = new_queue();
        for ty in [3, 1, 2, 1] {
            send_ok(q, ty, b"");
        }
        let mut t = lock();
        let qq = t.queue(slot_of(q)).unwrap();
        let at = |typ, mode| {
            let (_, m) = find_msg(qq, typ, mode)?;
            let mut p = qq.head;
            let mut i = 0;
            while p != m {
                // SAFETY: walking the queue's own list.
                p = unsafe { (*p).next };
                i += 1;
            }
            Some(i)
        };
        assert_eq!(at(0, Search::Any), Some(0));
        assert_eq!(at(2, Search::Equal), Some(2));
        assert_eq!(at(3, Search::NotEqual), Some(1));
        assert_eq!(at(2, Search::LessEqual), Some(1), "the first of the lowest");
        assert_eq!(at(5, Search::LessEqual), Some(1));
        assert_eq!(at(0, Search::LessEqual), None);
        assert_eq!(at(3, Search::Number), Some(3));
        assert_eq!(at(4, Search::Number), None);
        assert_eq!(at(9, Search::Equal), None);
        drop(t);
        rmid(q);
    }

    // -- a workflow --

    #[test]
    fn test_full_workflow() {
        let key = 0x0f10;
        let q = msgget(key, 0o660 | IPC_CREAT);
        assert!(q > 0);
        send_ok(q, 10, b"request");
        let other = msgget(key, 0o660);
        assert_eq!(other, q);
        assert_eq!(recv(other, 64, 10, 0), Ok((10, b"request".to_vec())));
        send_ok(q, 20, b"reply");
        assert_eq!(recv(q, 64, -20, 0), Ok((20, b"reply".to_vec())));
        rmid(q);
        assert_eq!(fail(msgget(key, 0o660).into()), errno::ENOENT);
    }
}
