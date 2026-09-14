// Indexing and arithmetic in this file operate on:
//
//  - A fixed pool of `MAX_QUEUES` slots, each holding a fixed
//    `MAX_MSGS_PER_QUEUE` message ring buffer (head/tail/count) reduced
//    modulo the buffer size immediately after increment.
//  - Per-message `mtype`/`msz` fields bounded by `MAX_MSG_TEXT` on
//    insertion.
//  - Caller-supplied `msgsz` validated against the queue size before
//    any indexing into the message body.
//
// Bounds are established locally but clippy cannot see across the
// check.  These lints would only be useful here if we accepted
// attacker-controlled integer indices into the pool/ring, which we
// do not.
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

//! System V message queues — `<sys/msg.h>`.
//!
//! A real in-memory implementation of `msgget`, `msgsnd`, `msgrcv`,
//! and `msgctl`, modeled on the `sysv_sem` precedent.
//!
//! ## Design
//!
//! Queues live in a fixed static pool of [`MAX_QUEUES`] entries, each
//! holding up to [`MAX_MSGS_PER_QUEUE`] messages of up to
//! [`MAX_MSG_TEXT`] bytes (plus the leading `mtype` long).  A single
//! global spinlock ([`MSG_LOCK`]) serialises all mutations — coarse
//! but adequate for the access pattern (each op is a bounded
//! memcopy under the lock).
//!
//! Keys behave the same as in `sysv_sem`:
//!   * `IPC_PRIVATE` (0) → always allocate a fresh anonymous queue.
//!   * Non-zero key → look up first, optionally create per
//!     `IPC_CREAT` / `IPC_EXCL`.
//!
//! The msqid handed to userspace packs (slot_index + 1) in the low
//! 16 bits and a 15-bit per-slot generation counter in the next 15
//! bits.  When `IPC_RMID` reuses a slot, the bumped generation
//! invalidates any stale msqid still held by a caller.
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
//! Our `msgsnd` reads `mtype` from the first `sizeof(long)` (= 8) bytes
//! and the message body from the next `msgsz` bytes.  `msgrcv` writes
//! the matching message's `mtype` into the first 8 bytes of the caller
//! buffer and the body into the next `msgsz` bytes.
//!
//! ## Receive selection
//!
//!   * `msgtyp == 0`  → return the first (oldest) message regardless
//!     of type.
//!   * `msgtyp > 0`   → return the first message whose type equals
//!     `msgtyp` exactly.  With `MSG_EXCEPT`, return the first message
//!     whose type is *not* `msgtyp`.
//!   * `msgtyp < 0`   → return the message with the lowest type ≤
//!     `|msgtyp|`.  `MSG_EXCEPT` is undefined here and is ignored.
//!
//! Oversized messages: by default the call fails with `E2BIG` and the
//! message stays on the queue.  With `MSG_NOERROR`, the message is
//! truncated to `msgsz` bytes and removed.
//!
//! ## Blocking semantics
//!
//! Without `IPC_NOWAIT`, `msgsnd` on a full queue and `msgrcv` on an
//! empty / no-matching-message queue spin-yield until conditions
//! change.  With `IPC_NOWAIT`, both return `EAGAIN` immediately when
//! they would block (POSIX requires `ENOMSG` from `msgrcv`, but Linux
//! uses `EAGAIN` on overflow and `ENOMSG` on empty; we return `ENOMSG`
//! for empty/no-match to match the Linux contract — most programs
//! check for ENOMSG explicitly).
//!
//! ## Limitations
//!
//! * Single-process only — no cross-process Sys V namespace yet.
//! * Permission bits stored but unenforced (uid 0 everywhere).
//! * `IPC_STAT` populates the bare-minimum fields (mode, qbytes,
//!   qnum, cbytes); time fields stay 0 because we don't track them
//!   yet.  `IPC_SET` accepts and stores `mode` and `qbytes`.
//! * `MSG_COPY` returns the message at a given queue index *without*
//!   dequeuing it, as Linux does: `msgtyp` is a 0-based index in queue
//!   order rather than a message type, and `IPC_NOWAIT` is required.
//!   Until 2026-09-13 the flag was accepted and the receive dequeued
//!   anyway — a caller asking to look at a message destroyed it, and the
//!   recipient it was queued for never saw it.

use crate::errno;

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
    pub msg_perm: crate::linux_ipc::IpcPerm,
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

// ---------------------------------------------------------------------------
// Pool sizing
// ---------------------------------------------------------------------------

const MAX_QUEUES: usize = 8;
const MAX_MSGS_PER_QUEUE: usize = 32;
const MAX_MSG_TEXT: usize = 256;
/// Default `msg_qbytes` for a fresh queue (Linux default is 16384).
const DEFAULT_QBYTES: usize = MAX_MSG_TEXT * MAX_MSGS_PER_QUEUE;

// ---------------------------------------------------------------------------
// Internal data structures
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Message {
    in_use: bool,
    /// Insertion order — lower = older.
    seq: u32,
    /// `mtype` from the msgbuf header.
    mtype: i64,
    /// Number of valid bytes in `data`.
    len: usize,
    data: [u8; MAX_MSG_TEXT],
}

impl Message {
    const EMPTY: Self = Self {
        in_use: false,
        seq: 0,
        mtype: 0,
        len: 0,
        data: [0u8; MAX_MSG_TEXT],
    };
}

#[derive(Clone, Copy)]
struct Queue {
    in_use: bool,
    key: i32,
    mode: u16,
    /// Bumped on every IPC_RMID to invalidate stale msqids.
    generation: u32,
    /// Max bytes allowed on queue (configurable via IPC_SET).
    qbytes: usize,
    /// Current bytes (sum of `len` of all in-use messages).
    cbytes: usize,
    /// Monotonic counter for insertion order.
    next_seq: u32,
    msgs: [Message; MAX_MSGS_PER_QUEUE],
}

impl Queue {
    const EMPTY: Self = Self {
        in_use: false,
        key: 0,
        mode: 0,
        generation: 0,
        qbytes: 0,
        cbytes: 0,
        next_seq: 0,
        msgs: [const { Message::EMPTY }; MAX_MSGS_PER_QUEUE],
    };
}

// ---------------------------------------------------------------------------
// Static state
// ---------------------------------------------------------------------------

/// Serialises every scan of `MSG_QUEUES`.
///
/// A plain `static`, matching `MSG_QUEUES`'s plain `static mut`: the lock must
/// have the same scope as the table it guards.  See
/// [`crate::perprocess::PoolLock`].
static MSG_LOCK: crate::perprocess::PoolLock = crate::perprocess::PoolLock::new();
static mut MSG_QUEUES: [Queue; MAX_QUEUES] = [const { Queue::EMPTY }; MAX_QUEUES];

/// Take `MSG_LOCK` for the duration of a queue-table scan.
fn lock() -> crate::perprocess::PoolGuard<'static> {
    // SAFETY: `MSG_LOCK` is a `static`, so it outlives the guard.
    unsafe { crate::perprocess::lock_pool((&raw const MSG_LOCK).cast_mut()) }
}

// ---------------------------------------------------------------------------
// msqid encoding
// ---------------------------------------------------------------------------

fn encode_msqid(slot: usize, generation: u32) -> i32 {
    let s = ((slot as u32) & 0xFFFF).wrapping_add(1);
    let g = generation & 0x7FFF;
    ((g << 16) | s) as i32
}

fn decode_msqid(msqid: i32) -> Option<(usize, u32)> {
    if msqid <= 0 {
        return None;
    }
    let u = msqid as u32;
    let s = (u & 0xFFFF) as usize;
    if s == 0 {
        return None;
    }
    let slot = s - 1;
    if slot >= MAX_QUEUES {
        return None;
    }
    let generation = (u >> 16) & 0x7FFF;
    Some((slot, generation))
}

// ---------------------------------------------------------------------------
// Helpers (all callers hold the lock)
// ---------------------------------------------------------------------------

/// SAFETY: caller holds the lock.
unsafe fn queues_ptr() -> *mut Queue {
    core::ptr::addr_of_mut!(MSG_QUEUES).cast::<Queue>()
}

/// SAFETY: caller holds the lock.
unsafe fn find_queue_by_key(key: i32) -> Option<usize> {
    if key == IPC_PRIVATE {
        return None;
    }
    let qs = unsafe { queues_ptr() };
    let mut i: usize = 0;
    while i < MAX_QUEUES {
        let q = unsafe { qs.add(i) };
        if unsafe { (*q).in_use } && unsafe { (*q).key } == key {
            return Some(i);
        }
        i = i.wrapping_add(1);
    }
    None
}

/// SAFETY: caller holds the lock.
unsafe fn alloc_queue(key: i32, mode: u16) -> Option<usize> {
    let qs = unsafe { queues_ptr() };
    let mut i: usize = 0;
    while i < MAX_QUEUES {
        let q = unsafe { qs.add(i) };
        if !unsafe { (*q).in_use } {
            unsafe {
                (*q).in_use = true;
                (*q).key = key;
                (*q).mode = mode;
                (*q).qbytes = DEFAULT_QBYTES;
                (*q).cbytes = 0;
                (*q).next_seq = 0;
                let mut k: usize = 0;
                while k < MAX_MSGS_PER_QUEUE {
                    (*q).msgs[k].in_use = false;
                    k = k.wrapping_add(1);
                }
            }
            return Some(i);
        }
        i = i.wrapping_add(1);
    }
    None
}

/// SAFETY: caller holds the lock.
unsafe fn resolve_msqid(msqid: i32) -> Option<usize> {
    let (slot, gen_) = decode_msqid(msqid)?;
    let qs = unsafe { queues_ptr() };
    let q = unsafe { qs.add(slot) };
    if !unsafe { (*q).in_use } {
        return None;
    }
    if unsafe { (*q).generation } & 0x7FFF != gen_ {
        return None;
    }
    Some(slot)
}

/// Find an empty slot in the queue's message ring.
///
/// SAFETY: caller holds the lock; `q` is a live in-use Queue.
unsafe fn alloc_msg_slot(q: *mut Queue) -> Option<usize> {
    let mut i: usize = 0;
    while i < MAX_MSGS_PER_QUEUE {
        if !unsafe { (*q).msgs[i].in_use } {
            return Some(i);
        }
        i = i.wrapping_add(1);
    }
    None
}

/// Select a message according to msgtyp / MSG_EXCEPT rules.
/// Returns the index of the matching message (oldest by `seq`), or `None`.
///
/// SAFETY: caller holds the lock; `q` is a live in-use Queue.
unsafe fn select_msg(q: *mut Queue, msgtyp: i64, except: bool) -> Option<usize> {
    let mut best: Option<(u32, usize)> = None; // (seq, index)
    let mut i: usize = 0;
    while i < MAX_MSGS_PER_QUEUE {
        if unsafe { (*q).msgs[i].in_use } {
            let mt = unsafe { (*q).msgs[i].mtype };
            let seq = unsafe { (*q).msgs[i].seq };
            let matches = match msgtyp.cmp(&0) {
                core::cmp::Ordering::Equal => true,
                core::cmp::Ordering::Greater => {
                    if except {
                        mt != msgtyp
                    } else {
                        mt == msgtyp
                    }
                }
                core::cmp::Ordering::Less => {
                    // msgtyp < 0: any message with 1 <= type <= |msgtyp|.
                    // Linux additionally requires returning the lowest type
                    // among the candidates (ties broken by seq).
                    let limit = msgtyp.wrapping_neg();
                    mt >= 1 && mt <= limit
                }
            };
            if matches {
                if msgtyp < 0 {
                    // For negative selection, prefer the lowest type;
                    // among ties, the lowest seq (oldest).
                    let take = match best {
                        None => true,
                        Some((bseq, bi)) => {
                            let bmt = unsafe { (*q).msgs[bi].mtype };
                            mt < bmt || (mt == bmt && seq < bseq)
                        }
                    };
                    if take {
                        best = Some((seq, i));
                    }
                } else {
                    // Otherwise: oldest matching message.
                    let take = match best {
                        None => true,
                        Some((bseq, _)) => seq < bseq,
                    };
                    if take {
                        best = Some((seq, i));
                    }
                }
            }
        }
        i = i.wrapping_add(1);
    }
    best.map(|(_, idx)| idx)
}

// ---------------------------------------------------------------------------
// msgget
// ---------------------------------------------------------------------------

/// `msgget` — get a message queue identifier.
///
/// Returns the msqid on success, or `-1` with errno set
/// (ENOENT/EEXIST/ENOSPC).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn msgget(key: i32, msgflg: i32) -> i32 {
    let mode = (msgflg & 0o777) as u16;
    let _g = lock();
    if key == IPC_PRIVATE {
        let Some(slot) = (unsafe { alloc_queue(IPC_PRIVATE, mode) }) else {
            errno::set_errno(errno::ENOSPC);
            return -1;
        };
        let gen_ = unsafe {
            let qs = queues_ptr();
            (*qs.add(slot)).generation & 0x7FFF
        };
        return encode_msqid(slot, gen_);
    }
    if let Some(slot) = unsafe { find_queue_by_key(key) } {
        if msgflg & IPC_CREAT != 0 && msgflg & IPC_EXCL != 0 {
            errno::set_errno(errno::EEXIST);
            return -1;
        }
        let gen_ = unsafe {
            let qs = queues_ptr();
            (*qs.add(slot)).generation & 0x7FFF
        };
        return encode_msqid(slot, gen_);
    }
    if msgflg & IPC_CREAT == 0 {
        errno::set_errno(errno::ENOENT);
        return -1;
    }
    let Some(slot) = (unsafe { alloc_queue(key, mode) }) else {
        errno::set_errno(errno::ENOSPC);
        return -1;
    };
    let gen_ = unsafe {
        let qs = queues_ptr();
        (*qs.add(slot)).generation & 0x7FFF
    };
    encode_msqid(slot, gen_)
}

// ---------------------------------------------------------------------------
// msgsnd
// ---------------------------------------------------------------------------

/// `msgsnd` — send a message to a queue.
///
/// `msgp` points to a `struct msgbuf` whose first 8 bytes are the
/// `mtype` (an `i64`, must be > 0) and whose next `msgsz` bytes are
/// the message body.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn msgsnd(msqid: i32, msgp: *const u8, msgsz: usize, msgflg: i32) -> i32 {
    if msgp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if msgsz > MAX_MSG_TEXT {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: caller contract; the mtype prefix is always exactly 8 bytes.
    let mtype = unsafe { core::ptr::read_unaligned(msgp.cast::<i64>()) };
    if mtype <= 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Snapshot the body so we can copy it under the lock.
    let mut body: [u8; MAX_MSG_TEXT] = [0u8; MAX_MSG_TEXT];
    if msgsz > 0 {
        // SAFETY: caller contract — msgp has at least 8 + msgsz bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(msgp.add(8), body.as_mut_ptr(), msgsz);
        }
    }
    let nowait = msgflg & IPC_NOWAIT != 0;

    loop {
        {
            let _g = lock();
            let Some(slot) = (unsafe { resolve_msqid(msqid) }) else {
                errno::set_errno(errno::EINVAL);
                return -1;
            };
            let qs = unsafe { queues_ptr() };
            let q = unsafe { qs.add(slot) };
            // Check space.
            let cbytes = unsafe { (*q).cbytes };
            let qbytes = unsafe { (*q).qbytes };
            let slot_opt = unsafe { alloc_msg_slot(q) };
            let fits_bytes = cbytes.saturating_add(msgsz) <= qbytes;
            if slot_opt.is_some() && fits_bytes {
                let mslot = slot_opt.unwrap_or(0);
                let seq = unsafe { (*q).next_seq };
                unsafe {
                    (*q).next_seq = (*q).next_seq.wrapping_add(1);
                    (*q).msgs[mslot].in_use = true;
                    (*q).msgs[mslot].seq = seq;
                    (*q).msgs[mslot].mtype = mtype;
                    (*q).msgs[mslot].len = msgsz;
                    if msgsz > 0 {
                        let dst = (*q).msgs[mslot].data.as_mut_ptr();
                        core::ptr::copy_nonoverlapping(body.as_ptr(), dst, msgsz);
                    }
                    (*q).cbytes = cbytes.saturating_add(msgsz);
                }
                return 0;
            }
            if nowait {
                errno::set_errno(errno::EAGAIN);
                return -1;
            }
            // Fall through to retry.
        }
        core::hint::spin_loop();
    }
}

/// The slot holding the message at 0-based `index` in **queue order**.
///
/// Queue order is `seq` order, not array order: `msgsnd` reuses whatever slot
/// is free, so the array says nothing about arrival. The message at index `n`
/// is the one with exactly `n` live messages ahead of it, which is what this
/// counts. `select_msg` picks the lowest `seq` for the same reason.
///
/// Quadratic in the queue length and deliberately so — `MAX_MSGS_PER_QUEUE` is
/// small, this runs once per `MSG_COPY` call, and the obvious version is the
/// one whose correctness can be read off it.
unsafe fn select_msg_at_index(q: *mut Queue, index: usize) -> Option<usize> {
    let mut i: usize = 0;
    while i < MAX_MSGS_PER_QUEUE {
        if unsafe { (*q).msgs[i].in_use } {
            let seq_i = unsafe { (*q).msgs[i].seq };
            let mut before: usize = 0;
            let mut j: usize = 0;
            while j < MAX_MSGS_PER_QUEUE {
                if unsafe { (*q).msgs[j].in_use } && unsafe { (*q).msgs[j].seq } < seq_i {
                    before = before.saturating_add(1);
                }
                j = j.saturating_add(1);
            }
            if before == index {
                return Some(i);
            }
        }
        i = i.saturating_add(1);
    }
    None
}

// ---------------------------------------------------------------------------
// msgrcv
// ---------------------------------------------------------------------------

/// `msgrcv` — receive a message from a queue.
///
/// On success returns the number of bytes copied into `mtext`
/// (excluding the 8-byte mtype header).  On failure returns -1 with
/// errno set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn msgrcv(
    msqid: i32,
    msgp: *mut u8,
    msgsz: usize,
    msgtyp: i64,
    msgflg: i32,
) -> isize {
    if msgp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if msgsz > MAX_MSG_TEXT {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let nowait = msgflg & IPC_NOWAIT != 0;
    let except = msgflg & MSG_EXCEPT != 0;
    let noerror = msgflg & MSG_NOERROR != 0;
    let copy_only = msgflg & MSG_COPY != 0;

    // MSG_COPY's combinations, refused the way Linux refuses them. Both are
    // contradictions rather than unsupported features: a peek that blocks
    // waiting for a message at an index has nothing to wait for, and
    // MSG_EXCEPT selects by type while MSG_COPY selects by position, so the
    // two cannot both describe the same call.
    if copy_only && (!nowait || except) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    loop {
        {
            let _g = lock();
            let Some(slot) = (unsafe { resolve_msqid(msqid) }) else {
                errno::set_errno(errno::EINVAL);
                return -1;
            };
            let qs = unsafe { queues_ptr() };
            let q = unsafe { qs.add(slot) };
            // With MSG_COPY, msgtyp is a 0-based queue index, not a type.
            let found = if copy_only {
                usize::try_from(msgtyp)
                    .ok()
                    .and_then(|idx| unsafe { select_msg_at_index(q, idx) })
            } else {
                unsafe { select_msg(q, msgtyp, except) }
            };
            if let Some(midx) = found {
                let mlen = unsafe { (*q).msgs[midx].len };
                let mtype = unsafe { (*q).msgs[midx].mtype };
                if mlen > msgsz && !noerror {
                    errno::set_errno(errno::E2BIG);
                    return -1;
                }
                let copy_len = if mlen <= msgsz { mlen } else { msgsz };
                // Write mtype prefix.
                // SAFETY: caller contract — msgp has at least 8 + msgsz bytes.
                unsafe {
                    core::ptr::write_unaligned(msgp.cast::<i64>(), mtype);
                    if copy_len > 0 {
                        let src = (*q).msgs[midx].data.as_ptr();
                        core::ptr::copy_nonoverlapping(src, msgp.add(8), copy_len);
                    }
                    // MSG_COPY is a look, not a take. Dequeuing here is what
                    // this flag existed to avoid: the message is still owed to
                    // whoever it was queued for.
                    if !copy_only {
                        (*q).msgs[midx].in_use = false;
                        (*q).cbytes = (*q).cbytes.saturating_sub(mlen);
                    }
                }
                return copy_len as isize;
            }
            if nowait {
                errno::set_errno(errno::ENOMSG);
                return -1;
            }
            // Fall through to retry.
        }
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// msgctl
// ---------------------------------------------------------------------------

/// `msgctl` — message queue control operations.
///
/// Supported commands:
///   * `IPC_RMID` — remove the queue, invalidating its msqid.
///   * `IPC_STAT` — populate `*buf` with current queue state.
///   * `IPC_SET`  — update `msg_perm_mode` and `msg_qbytes` from `*buf`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn msgctl(msqid: i32, cmd: i32, buf: *mut MsqidDs) -> i32 {
    let _g = lock();
    let Some(slot) = (unsafe { resolve_msqid(msqid) }) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    let qs = unsafe { queues_ptr() };
    let q = unsafe { qs.add(slot) };
    match cmd {
        IPC_RMID => {
            unsafe {
                (*q).in_use = false;
                (*q).generation = (*q).generation.wrapping_add(1);
                (*q).key = 0;
                // No need to clear messages; allocation will reset them.
            }
            0
        }
        IPC_STAT => {
            if buf.is_null() {
                errno::set_errno(errno::EFAULT);
                return -1;
            }
            // Count messages currently in the ring.
            let mut qnum: usize = 0;
            let mut i: usize = 0;
            while i < MAX_MSGS_PER_QUEUE {
                if unsafe { (*q).msgs[i].in_use } {
                    qnum = qnum.wrapping_add(1);
                }
                i = i.wrapping_add(1);
            }
            // SAFETY: caller contract.
            unsafe {
                (*buf) = MsqidDs {
                    msg_perm: crate::linux_ipc::IpcPerm {
                        uid: 0,
                        gid: 0,
                        cuid: 0,
                        cgid: 0,
                        mode: u32::from((*q).mode),
                        ..crate::linux_ipc::IpcPerm::default()
                    },
                    msg_cbytes: (*q).cbytes,
                    msg_qnum: qnum,
                    msg_qbytes: (*q).qbytes,
                    msg_lspid: 0,
                    msg_lrpid: 0,
                    msg_stime: 0,
                    msg_rtime: 0,
                    msg_ctime: 0,
                    __unused: [0; 2],
                };
            }
            0
        }
        IPC_SET => {
            if buf.is_null() {
                errno::set_errno(errno::EFAULT);
                return -1;
            }
            // SAFETY: caller contract.
            let new_mode = unsafe { (*buf).msg_perm.mode };
            let new_qbytes = unsafe { (*buf).msg_qbytes };
            // Cap qbytes at the pool maximum — Linux requires
            // CAP_SYS_RESOURCE to raise it past msgmnb, we just clamp.
            let cap = DEFAULT_QBYTES;
            unsafe {
                (*q).mode = u16::try_from(new_mode & 0o7777).unwrap_or(0);
                (*q).qbytes = new_qbytes.min(cap);
            }
            0
        }
        _ => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// Test-only helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
fn test_reset_all() {
    let _g = lock();
    // SAFETY: lock held.
    let qs = unsafe { queues_ptr() };
    let mut i: usize = 0;
    while i < MAX_QUEUES {
        unsafe {
            (*qs.add(i)).in_use = false;
            (*qs.add(i)).key = 0;
            (*qs.add(i)).cbytes = 0;
            (*qs.add(i)).generation = (*qs.add(i)).generation.wrapping_add(1);
            let mut m: usize = 0;
            while m < MAX_MSGS_PER_QUEUE {
                (*qs.add(i)).msgs[m].in_use = false;
                m = m.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn with_clean<F: FnOnce()>(f: F) {
        let _g = TEST_LOCK.lock().unwrap();
        test_reset_all();
        f();
    }

    /// Build an 8-byte mtype + body buffer suitable for msgsnd.
    fn make_msg(mtype: i64, body: &[u8]) -> std::vec::Vec<u8> {
        let mut v = std::vec::Vec::with_capacity(8 + body.len());
        v.extend_from_slice(&mtype.to_ne_bytes());
        v.extend_from_slice(body);
        v
    }

    // -- Constants --

    #[test]
    fn test_constants() {
        assert_eq!(IPC_CREAT, 0o1000);
        assert_eq!(IPC_EXCL, 0o2000);
        assert_eq!(IPC_NOWAIT, 0o4000);
        assert_eq!(IPC_RMID, 0);
        assert_eq!(IPC_SET, 1);
        assert_eq!(IPC_STAT, 2);
        assert_eq!(IPC_PRIVATE, 0);
        assert_ne!(MSG_NOERROR, 0);
        assert_ne!(MSG_EXCEPT, 0);
    }

    #[test]
    fn test_msqid_ds_layout() {
        let ds = MsqidDs {
            msg_perm: crate::linux_ipc::IpcPerm {
                uid: 0,
                gid: 0,
                cuid: 0,
                cgid: 0,
                mode: 0o666,
                ..crate::linux_ipc::IpcPerm::default()
            },
            msg_cbytes: 0,
            msg_qnum: 0,
            msg_qbytes: 16384,
            msg_lspid: 0,
            msg_lrpid: 0,
            msg_stime: 0,
            msg_rtime: 0,
            msg_ctime: 0,
            __unused: [0; 2],
        };
        assert_eq!(ds.msg_perm.mode, 0o666);
        assert_eq!(ds.msg_qbytes, 16384);
    }

    // -- msqid encoding --

    // -- MSG_COPY: a peek must not consume --

    #[test]
    fn msg_copy_returns_a_message_without_dequeuing_it() {
        // THE REGRESSION PIN. MSG_COPY used to fall through to a normal
        // receive, so a caller that asked to look at a message destroyed it
        // and the recipient it was queued for never saw it.
        let qid = msgget(IPC_PRIVATE, 0o600 | IPC_CREAT);
        assert!(qid >= 0, "queue creation failed: {qid}");

        let mut out = [0u8; 64];
        for (n, body) in [b"first!!!", b"second!!"].iter().enumerate() {
            let mut buf = [0u8; 64];
            let ty = (n as i64) + 1;
            buf[..8].copy_from_slice(&ty.to_ne_bytes());
            buf[8..16].copy_from_slice(*body);
            let r = msgsnd(qid, buf.as_ptr().cast(), 8, 0);
            assert_eq!(r, 0, "msgsnd {n} failed");
        }

        // Peek index 0 twice. If the first peek consumed it, the second
        // returns the *other* message and the assertion below catches it.
        for attempt in 0..2 {
            let r = msgrcv(qid, out.as_mut_ptr().cast(), 8, 0, MSG_COPY | IPC_NOWAIT);
            assert_eq!(r, 8, "MSG_COPY attempt {attempt} returned {r}");
            let ty = i64::from_ne_bytes(out[..8].try_into().unwrap());
            assert_eq!(ty, 1, "attempt {attempt} saw the wrong message");
            assert_eq!(&out[8..16], b"first!!!", "attempt {attempt} body");
        }

        // ...and a real receive still finds it, which is the whole point.
        let r = msgrcv(qid, out.as_mut_ptr().cast(), 8, 0, IPC_NOWAIT);
        assert_eq!(r, 8);
        assert_eq!(&out[8..16], b"first!!!", "the peeked message was consumed");

        let _ = msgctl(qid, IPC_RMID, core::ptr::null_mut());
    }

    #[test]
    fn msg_copy_indexes_by_position_not_by_type() {
        let qid = msgget(IPC_PRIVATE, 0o600 | IPC_CREAT);
        assert!(qid >= 0);
        for (n, body) in [b"aaaaaaaa", b"bbbbbbbb"].iter().enumerate() {
            let mut buf = [0u8; 64];
            // Types chosen so that index 1 and type 1 name different messages.
            let ty = 10_i64 - (n as i64);
            buf[..8].copy_from_slice(&ty.to_ne_bytes());
            buf[8..16].copy_from_slice(*body);
            assert_eq!(msgsnd(qid, buf.as_ptr().cast(), 8, 0), 0);
        }
        let mut out = [0u8; 64];
        let r = msgrcv(qid, out.as_mut_ptr().cast(), 8, 1, MSG_COPY | IPC_NOWAIT);
        assert_eq!(r, 8);
        assert_eq!(
            &out[8..16],
            b"bbbbbbbb",
            "index 1 is the second message sent"
        );
        let _ = msgctl(qid, IPC_RMID, core::ptr::null_mut());
    }

    #[test]
    fn msg_copy_past_the_end_is_enomsg() {
        let qid = msgget(IPC_PRIVATE, 0o600 | IPC_CREAT);
        assert!(qid >= 0);
        let mut out = [0u8; 64];
        let r = msgrcv(qid, out.as_mut_ptr().cast(), 8, 5, MSG_COPY | IPC_NOWAIT);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOMSG);
        let _ = msgctl(qid, IPC_RMID, core::ptr::null_mut());
    }

    #[test]
    fn msg_copy_without_nowait_is_einval() {
        // A peek that blocks waiting for an index has nothing to wait for.
        let qid = msgget(IPC_PRIVATE, 0o600 | IPC_CREAT);
        assert!(qid >= 0);
        let mut out = [0u8; 64];
        assert_eq!(msgrcv(qid, out.as_mut_ptr().cast(), 8, 0, MSG_COPY), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        let _ = msgctl(qid, IPC_RMID, core::ptr::null_mut());
    }

    #[test]
    fn msg_copy_with_msg_except_is_einval() {
        // MSG_EXCEPT selects by type, MSG_COPY by position; one call cannot
        // mean both.
        let qid = msgget(IPC_PRIVATE, 0o600 | IPC_CREAT);
        assert!(qid >= 0);
        let mut out = [0u8; 64];
        let flags = MSG_COPY | IPC_NOWAIT | MSG_EXCEPT;
        assert_eq!(msgrcv(qid, out.as_mut_ptr().cast(), 8, 0, flags), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        let _ = msgctl(qid, IPC_RMID, core::ptr::null_mut());
    }

    #[test]
    fn test_msqid_encode_decode_roundtrip() {
        for slot in 0..MAX_QUEUES {
            for gen_ in [0u32, 1, 0x7FFF] {
                let id = encode_msqid(slot, gen_);
                let (s, g) = decode_msqid(id).unwrap();
                assert_eq!(s, slot);
                assert_eq!(g, gen_);
            }
        }
    }

    #[test]
    fn test_msqid_decode_rejects_zero_and_negative() {
        assert!(decode_msqid(0).is_none());
        assert!(decode_msqid(-1).is_none());
    }

    // -- msgget --

    #[test]
    fn test_msgget_private_creates_new() {
        with_clean(|| {
            let a = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            let b = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            assert_ne!(a, -1);
            assert_ne!(b, -1);
            assert_ne!(a, b);
        });
    }

    #[test]
    fn test_msgget_keyed_lookup() {
        with_clean(|| {
            let a = msgget(0xABC, IPC_CREAT | 0o600);
            let b = msgget(0xABC, 0);
            assert_eq!(a, b);
        });
    }

    #[test]
    fn test_msgget_missing_enoent() {
        with_clean(|| {
            errno::set_errno(0);
            let id = msgget(0xDEF, 0);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::ENOENT);
        });
    }

    #[test]
    fn test_msgget_excl_eexist() {
        with_clean(|| {
            let _ = msgget(0xBEEF, IPC_CREAT | 0o600);
            errno::set_errno(0);
            let id = msgget(0xBEEF, IPC_CREAT | IPC_EXCL | 0o600);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::EEXIST);
        });
    }

    #[test]
    fn test_msgget_pool_exhaustion_enospc() {
        with_clean(|| {
            for _ in 0..MAX_QUEUES {
                let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
                assert_ne!(id, -1);
            }
            errno::set_errno(0);
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::ENOSPC);
        });
    }

    // -- msgsnd / msgrcv --

    #[test]
    fn test_msgsnd_msgrcv_basic_fifo() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            let m1 = make_msg(1, b"hello");
            let m2 = make_msg(1, b"world");
            assert_eq!(msgsnd(id, m1.as_ptr(), 5, 0), 0);
            assert_eq!(msgsnd(id, m2.as_ptr(), 5, 0), 0);
            // Receive any type; should get "hello" first.
            let mut buf = [0u8; 8 + 32];
            let n = msgrcv(id, buf.as_mut_ptr(), 32, 0, 0);
            assert_eq!(n, 5);
            let mtype = i64::from_ne_bytes(buf[..8].try_into().unwrap());
            assert_eq!(mtype, 1);
            assert_eq!(&buf[8..13], b"hello");
            let n = msgrcv(id, buf.as_mut_ptr(), 32, 0, 0);
            assert_eq!(n, 5);
            assert_eq!(&buf[8..13], b"world");
        });
    }

    #[test]
    fn test_msgrcv_by_type_match() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            let m1 = make_msg(1, b"a");
            let m2 = make_msg(2, b"b");
            let m3 = make_msg(2, b"c");
            assert_eq!(msgsnd(id, m1.as_ptr(), 1, 0), 0);
            assert_eq!(msgsnd(id, m2.as_ptr(), 1, 0), 0);
            assert_eq!(msgsnd(id, m3.as_ptr(), 1, 0), 0);
            let mut buf = [0u8; 8 + 8];
            // Request type 2 only.
            let n = msgrcv(id, buf.as_mut_ptr(), 8, 2, 0);
            assert_eq!(n, 1);
            let mtype = i64::from_ne_bytes(buf[..8].try_into().unwrap());
            assert_eq!(mtype, 2);
            assert_eq!(buf[8], b'b');
        });
    }

    #[test]
    fn test_msgrcv_negative_type_returns_lowest() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            let m3 = make_msg(3, b"three");
            let m1 = make_msg(1, b"one");
            let m2 = make_msg(2, b"two");
            assert_eq!(msgsnd(id, m3.as_ptr(), 5, 0), 0);
            assert_eq!(msgsnd(id, m1.as_ptr(), 3, 0), 0);
            assert_eq!(msgsnd(id, m2.as_ptr(), 3, 0), 0);
            // msgtyp = -2 → lowest type in {1, 2}.
            let mut buf = [0u8; 8 + 32];
            let n = msgrcv(id, buf.as_mut_ptr(), 32, -2, 0);
            assert_eq!(n, 3);
            let mtype = i64::from_ne_bytes(buf[..8].try_into().unwrap());
            assert_eq!(mtype, 1);
            assert_eq!(&buf[8..11], b"one");
        });
    }

    #[test]
    fn test_msgrcv_except_skips_matching_type() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            let m1 = make_msg(7, b"seven");
            let m2 = make_msg(8, b"eight");
            assert_eq!(msgsnd(id, m1.as_ptr(), 5, 0), 0);
            assert_eq!(msgsnd(id, m2.as_ptr(), 5, 0), 0);
            let mut buf = [0u8; 8 + 32];
            // Anything except type 7 → should get the 8.
            let n = msgrcv(id, buf.as_mut_ptr(), 32, 7, MSG_EXCEPT);
            assert_eq!(n, 5);
            let mtype = i64::from_ne_bytes(buf[..8].try_into().unwrap());
            assert_eq!(mtype, 8);
        });
    }

    #[test]
    fn test_msgsnd_bad_mtype_einval() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            let m = make_msg(0, b"x");
            errno::set_errno(0);
            assert_eq!(msgsnd(id, m.as_ptr(), 1, 0), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
            let m = make_msg(-1, b"x");
            errno::set_errno(0);
            assert_eq!(msgsnd(id, m.as_ptr(), 1, 0), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_msgsnd_null_efault() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            errno::set_errno(0);
            assert_eq!(msgsnd(id, core::ptr::null(), 1, 0), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        });
    }

    #[test]
    fn test_msgsnd_bad_msqid_einval() {
        with_clean(|| {
            let m = make_msg(1, b"x");
            errno::set_errno(0);
            assert_eq!(msgsnd(0xCAFE, m.as_ptr(), 1, 0), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_msgsnd_too_big_einval() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            let m = make_msg(1, b"x");
            errno::set_errno(0);
            assert_eq!(msgsnd(id, m.as_ptr(), MAX_MSG_TEXT + 1, 0), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_msgsnd_queue_full_nowait_eagain() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            // Shrink qbytes via IPC_SET to force a quick "full".
            let mut ds = MsqidDs {
                msg_perm: crate::linux_ipc::IpcPerm {
                    uid: 0,
                    gid: 0,
                    cuid: 0,
                    cgid: 0,
                    mode: 0o600,
                    ..crate::linux_ipc::IpcPerm::default()
                },
                msg_cbytes: 0,
                msg_qnum: 0,
                msg_qbytes: 4,
                msg_lspid: 0,
                msg_lrpid: 0,
                msg_stime: 0,
                msg_rtime: 0,
                msg_ctime: 0,
                __unused: [0; 2],
            };
            assert_eq!(msgctl(id, IPC_SET, &raw mut ds), 0);
            let m = make_msg(1, b"hello"); // 5 bytes > 4 byte limit
            errno::set_errno(0);
            assert_eq!(msgsnd(id, m.as_ptr(), 5, IPC_NOWAIT), -1);
            assert_eq!(errno::get_errno(), errno::EAGAIN);
        });
    }

    #[test]
    fn test_msgrcv_empty_nowait_enomsg() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            let mut buf = [0u8; 16];
            errno::set_errno(0);
            assert_eq!(msgrcv(id, buf.as_mut_ptr(), 8, 0, IPC_NOWAIT), -1);
            assert_eq!(errno::get_errno(), errno::ENOMSG);
        });
    }

    #[test]
    fn test_msgrcv_no_matching_type_nowait_enomsg() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            let m = make_msg(1, b"x");
            assert_eq!(msgsnd(id, m.as_ptr(), 1, 0), 0);
            let mut buf = [0u8; 16];
            errno::set_errno(0);
            assert_eq!(msgrcv(id, buf.as_mut_ptr(), 8, 99, IPC_NOWAIT), -1);
            assert_eq!(errno::get_errno(), errno::ENOMSG);
        });
    }

    #[test]
    fn test_msgrcv_too_big_e2big() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            let m = make_msg(1, b"hello world");
            assert_eq!(msgsnd(id, m.as_ptr(), 11, 0), 0);
            let mut buf = [0u8; 16];
            errno::set_errno(0);
            assert_eq!(msgrcv(id, buf.as_mut_ptr(), 4, 0, 0), -1);
            assert_eq!(errno::get_errno(), errno::E2BIG);
            // Message stays on the queue.
            let mut buf2 = [0u8; 32];
            let n = msgrcv(id, buf2.as_mut_ptr(), 20, 0, 0);
            assert_eq!(n, 11);
        });
    }

    #[test]
    fn test_msgrcv_noerror_truncates() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            let m = make_msg(1, b"hello world");
            assert_eq!(msgsnd(id, m.as_ptr(), 11, 0), 0);
            let mut buf = [0u8; 16];
            let n = msgrcv(id, buf.as_mut_ptr(), 4, 0, MSG_NOERROR);
            assert_eq!(n, 4);
            assert_eq!(&buf[8..12], b"hell");
            // Truncated message is gone.
            errno::set_errno(0);
            let mut buf2 = [0u8; 16];
            assert_eq!(msgrcv(id, buf2.as_mut_ptr(), 8, 0, IPC_NOWAIT), -1);
            assert_eq!(errno::get_errno(), errno::ENOMSG);
        });
    }

    #[test]
    fn test_msgrcv_null_efault() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            errno::set_errno(0);
            assert_eq!(msgrcv(id, core::ptr::null_mut(), 8, 0, IPC_NOWAIT), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        });
    }

    #[test]
    fn test_msgrcv_bad_msqid_einval() {
        with_clean(|| {
            let mut buf = [0u8; 16];
            errno::set_errno(0);
            assert_eq!(msgrcv(0xDEAD, buf.as_mut_ptr(), 8, 0, IPC_NOWAIT), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    // -- msgctl --

    #[test]
    fn test_msgctl_stat_populates() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o644);
            let m = make_msg(1, b"xyz");
            assert_eq!(msgsnd(id, m.as_ptr(), 3, 0), 0);
            let mut ds = MsqidDs {
                msg_perm: crate::linux_ipc::IpcPerm {
                    uid: 99,
                    gid: 99,
                    cuid: 99,
                    cgid: 99,
                    mode: 0,
                    ..crate::linux_ipc::IpcPerm::default()
                },
                msg_cbytes: 99,
                msg_qnum: 99,
                msg_qbytes: 99,
                msg_lspid: 0,
                msg_lrpid: 0,
                msg_stime: 0,
                msg_rtime: 0,
                msg_ctime: 0,
                __unused: [0; 2],
            };
            assert_eq!(msgctl(id, IPC_STAT, &raw mut ds), 0);
            assert_eq!(ds.msg_perm.mode, 0o644);
            assert_eq!(ds.msg_qnum, 1);
            assert_eq!(ds.msg_cbytes, 3);
            assert!(ds.msg_qbytes > 0);
        });
    }

    #[test]
    fn test_msgctl_set_updates_qbytes_and_mode() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            let mut ds = MsqidDs {
                msg_perm: crate::linux_ipc::IpcPerm {
                    uid: 0,
                    gid: 0,
                    cuid: 0,
                    cgid: 0,
                    mode: 0o644,
                    ..crate::linux_ipc::IpcPerm::default()
                },
                msg_cbytes: 0,
                msg_qnum: 0,
                msg_qbytes: 1024,
                msg_lspid: 0,
                msg_lrpid: 0,
                msg_stime: 0,
                msg_rtime: 0,
                msg_ctime: 0,
                __unused: [0; 2],
            };
            assert_eq!(msgctl(id, IPC_SET, &raw mut ds), 0);
            // Confirm via IPC_STAT.
            let mut stat = ds;
            stat.msg_perm.mode = 0;
            stat.msg_qbytes = 0;
            assert_eq!(msgctl(id, IPC_STAT, &raw mut stat), 0);
            assert_eq!(stat.msg_perm.mode, 0o644);
            assert_eq!(stat.msg_qbytes, 1024);
        });
    }

    #[test]
    fn test_msgctl_set_clamps_qbytes_to_pool_max() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            let mut ds = MsqidDs {
                msg_perm: crate::linux_ipc::IpcPerm {
                    uid: 0,
                    gid: 0,
                    cuid: 0,
                    cgid: 0,
                    mode: 0o600,
                    ..crate::linux_ipc::IpcPerm::default()
                },
                msg_cbytes: 0,
                msg_qnum: 0,
                msg_qbytes: usize::MAX,
                msg_lspid: 0,
                msg_lrpid: 0,
                msg_stime: 0,
                msg_rtime: 0,
                msg_ctime: 0,
                __unused: [0; 2],
            };
            assert_eq!(msgctl(id, IPC_SET, &raw mut ds), 0);
            let mut stat = ds;
            stat.msg_qbytes = 0;
            assert_eq!(msgctl(id, IPC_STAT, &raw mut stat), 0);
            assert_eq!(stat.msg_qbytes, MAX_MSG_TEXT * MAX_MSGS_PER_QUEUE);
        });
    }

    #[test]
    fn test_msgctl_rmid_invalidates_id() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            assert_eq!(msgctl(id, IPC_RMID, core::ptr::null_mut()), 0);
            errno::set_errno(0);
            assert_eq!(msgctl(id, IPC_STAT, core::ptr::null_mut()), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_msgctl_rmid_frees_slot() {
        with_clean(|| {
            let mut ids = std::vec::Vec::new();
            for _ in 0..MAX_QUEUES {
                ids.push(msgget(IPC_PRIVATE, IPC_CREAT | 0o600));
            }
            assert_eq!(msgctl(ids[0], IPC_RMID, core::ptr::null_mut()), 0);
            let new_id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            assert_ne!(new_id, -1);
            assert_ne!(new_id, ids[0]);
        });
    }

    #[test]
    fn test_msgctl_stat_null_buf_efault() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            errno::set_errno(0);
            assert_eq!(msgctl(id, IPC_STAT, core::ptr::null_mut()), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        });
    }

    #[test]
    fn test_msgctl_bad_cmd_einval() {
        with_clean(|| {
            let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
            errno::set_errno(0);
            assert_eq!(msgctl(id, 9999, core::ptr::null_mut()), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_msgctl_bad_msqid_einval() {
        with_clean(|| {
            errno::set_errno(0);
            assert_eq!(msgctl(0xDEAD, IPC_RMID, core::ptr::null_mut()), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    // -- workflow --

    #[test]
    fn test_full_workflow() {
        with_clean(|| {
            let id = msgget(0xC0DE, IPC_CREAT | 0o644);
            assert_ne!(id, -1);
            let m = make_msg(42, b"payload");
            assert_eq!(msgsnd(id, m.as_ptr(), 7, 0), 0);
            let mut buf = [0u8; 8 + 32];
            let n = msgrcv(id, buf.as_mut_ptr(), 32, 42, 0);
            assert_eq!(n, 7);
            assert_eq!(&buf[8..15], b"payload");
            assert_eq!(msgctl(id, IPC_RMID, core::ptr::null_mut()), 0);
        });
    }
}
