//! POSIX message queues (`<mqueue.h>`).
//!
//! A real implementation of POSIX message queues backed by an in-memory
//! table of named queues.  Provides `mq_open`, `mq_close`, `mq_unlink`,
//! `mq_send`, `mq_receive`, `mq_getattr`, `mq_setattr`, `mq_timedsend`,
//! `mq_timedreceive`.  `mq_notify` remains `ENOSYS` because we don't
//! integrate with POSIX signals or a sigevent dispatcher yet.
//!
//! ## Design
//!
//! Message queues live in two static pools shared by the whole
//! process:
//!
//! * `MQ_QUEUES` — up to `MAX_QUEUES` named queues, each with a fixed
//!   ring of `MAX_MSGS_PER_QUEUE` messages of up to `MAX_MSG_SIZE`
//!   bytes.
//! * `MQ_DESCS` — up to `MAX_DESCRIPTORS` open descriptors; each
//!   descriptor points to a queue and stores its own `O_NONBLOCK`
//!   flag.  The descriptor index (`+1`) is returned to userspace as
//!   the `mqd_t` value; `mqd_t == 0` is reserved as invalid.
//!
//! A single global spinlock (`MQ_LOCK`) protects all mutations of
//! both tables.  This is coarse but adequate for the access pattern
//! (mq_send/receive are bounded-size memcopies under the lock).
//!
//! ### Blocking behaviour
//!
//! Without `O_NONBLOCK`, `mq_send` on a full queue and `mq_receive`
//! on an empty queue spin-yield until space / a message becomes
//! available, matching POSIX blocking semantics.  Because the only
//! way for a queue's state to change is another thread modifying it,
//! a single-threaded program that fills a queue and then sends one
//! more message without `O_NONBLOCK` will deadlock; tests cover only
//! the `O_NONBLOCK` paths to avoid this.
//!
//! `mq_timedsend` / `mq_timedreceive` use `clock_gettime(CLOCK_
//! REALTIME)` to enforce the absolute deadline, returning `ETIMEDOUT`
//! when it elapses.
//!
//! ### Limitations
//!
//! * Single-process only — the static tables live in one address
//!   space.  A real cross-process implementation needs kernel-side
//!   message-queue objects accessible by name through the shared
//!   namespace; the design for that is sketched in `roadmap.md` but
//!   not yet wired up.
//! * No `mq_notify` — would require integrating with a signal /
//!   sigevent dispatch path.
//! * Fixed per-queue limits (`MAX_MSGS_PER_QUEUE`, `MAX_MSG_SIZE`)
//!   so the static pool stays bounded.  Programs requesting bigger
//!   attributes in `mq_open` get `EINVAL`.

use crate::errno;
use crate::stat::Timespec;
use core::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// Public types & constants
// ---------------------------------------------------------------------------

/// Message queue descriptor type.
pub type MqdT = i32;

/// Message queue attributes (POSIX `struct mq_attr`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MqAttr {
    /// Flags (currently only `O_NONBLOCK` is honoured).
    pub mq_flags: i64,
    /// Maximum number of messages on the queue.
    pub mq_maxmsg: i64,
    /// Maximum size of an individual message in bytes.
    pub mq_msgsize: i64,
    /// Number of messages currently queued.
    pub mq_curmsgs: i64,
    /// Padding (Linux reserves four extra slots for future use).
    _pad: [i64; 4],
}

/// Maximum priority value accepted by `mq_send` (POSIX `MQ_PRIO_MAX`
/// floor — Linux uses 32767).
pub const MQ_PRIO_MAX: u32 = 32_768;

// ---------------------------------------------------------------------------
// Pool sizing
// ---------------------------------------------------------------------------

const MAX_QUEUES: usize = 8;
const MAX_DESCRIPTORS: usize = 16;
const MAX_MSGS_PER_QUEUE: usize = 32;
const MAX_MSG_SIZE: usize = 256;
const MAX_NAME_LEN: usize = 64;

/// Default `mq_maxmsg` if the caller doesn't pass an attribute struct.
const DEFAULT_MAXMSG: usize = 10;
/// Default `mq_msgsize` if the caller doesn't pass an attribute struct.
const DEFAULT_MSGSIZE: usize = 64;

// ---------------------------------------------------------------------------
// Internal data structures
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Message {
    in_use: bool,
    priority: u32,
    len: usize,
    data: [u8; MAX_MSG_SIZE],
}

impl Message {
    const EMPTY: Self = Self {
        in_use: false,
        priority: 0,
        len: 0,
        data: [0u8; MAX_MSG_SIZE],
    };
}

#[derive(Clone, Copy)]
struct Queue {
    in_use: bool,
    /// Set by `mq_unlink`; the queue is freed when the last descriptor
    /// closes after being unlinked.
    unlinked: bool,
    name: [u8; MAX_NAME_LEN],
    name_len: usize,
    refcount: u32,
    max_msgs: usize,
    msg_size: usize,
    cur_msgs: usize,
    msgs: [Message; MAX_MSGS_PER_QUEUE],
}

impl Queue {
    const EMPTY: Self = Self {
        in_use: false,
        unlinked: false,
        name: [0u8; MAX_NAME_LEN],
        name_len: 0,
        refcount: 0,
        max_msgs: 0,
        msg_size: 0,
        cur_msgs: 0,
        msgs: [const { Message::EMPTY }; MAX_MSGS_PER_QUEUE],
    };
}

#[derive(Clone, Copy)]
struct Descriptor {
    in_use: bool,
    queue_idx: usize,
    nonblock: bool,
}

impl Descriptor {
    const EMPTY: Self = Self {
        in_use: false,
        queue_idx: 0,
        nonblock: false,
    };
}

// ---------------------------------------------------------------------------
// Static state
// ---------------------------------------------------------------------------

static MQ_LOCK: AtomicBool = AtomicBool::new(false);
static mut MQ_QUEUES: [Queue; MAX_QUEUES] = [const { Queue::EMPTY }; MAX_QUEUES];
static mut MQ_DESCS: [Descriptor; MAX_DESCRIPTORS] =
    [const { Descriptor::EMPTY }; MAX_DESCRIPTORS];

fn lock_acquire() {
    while MQ_LOCK
        .compare_exchange_weak(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn lock_release() {
    MQ_LOCK.store(false, Ordering::Release);
}

/// RAII guard that releases the global mqueue lock on drop.
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        lock_release();
    }
}

fn lock() -> Guard {
    lock_acquire();
    Guard
}

// ---------------------------------------------------------------------------
// Helpers (all callers hold the lock)
// ---------------------------------------------------------------------------

/// SAFETY: Caller must hold `MQ_LOCK`.
unsafe fn queues_ptr() -> *mut Queue {
    core::ptr::addr_of_mut!(MQ_QUEUES).cast::<Queue>()
}

/// SAFETY: Caller must hold `MQ_LOCK`.
unsafe fn descs_ptr() -> *mut Descriptor {
    core::ptr::addr_of_mut!(MQ_DESCS).cast::<Descriptor>()
}

/// Validate the queue name: must be non-null, start with `/`, contain
/// no further `/`, fit in `MAX_NAME_LEN` including the null terminator,
/// and have at least one character after the leading `/`.
///
/// On success returns `Some((bytes_excluding_null, length))`.
unsafe fn validate_name(name: *const u8) -> Option<([u8; MAX_NAME_LEN], usize)> {
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return None;
    }
    let mut buf = [0u8; MAX_NAME_LEN];
    let mut i: usize = 0;
    loop {
        if i >= MAX_NAME_LEN {
            errno::set_errno(errno::ENAMETOOLONG);
            return None;
        }
        let b = unsafe { *name.add(i) };
        if b == 0 {
            break;
        }
        buf[i] = b;
        i = i.wrapping_add(1);
    }
    if i == 0 || buf[0] != b'/' {
        errno::set_errno(errno::EINVAL);
        return None;
    }
    if i == 1 {
        // Just "/" — POSIX says EINVAL.
        errno::set_errno(errno::EINVAL);
        return None;
    }
    // No further '/' allowed.
    let mut j: usize = 1;
    while j < i {
        if buf[j] == b'/' {
            errno::set_errno(errno::EINVAL);
            return None;
        }
        j = j.wrapping_add(1);
    }
    Some((buf, i))
}

/// Find a queue by name.  Returns the index in `MQ_QUEUES` or `None`.
///
/// SAFETY: Caller must hold the lock.
unsafe fn find_queue_by_name(name: &[u8]) -> Option<usize> {
    let qs = unsafe { queues_ptr() };
    let mut i: usize = 0;
    while i < MAX_QUEUES {
        let q = unsafe { qs.add(i) };
        let (used, unlinked, nlen) = unsafe { ((*q).in_use, (*q).unlinked, (*q).name_len) };
        if used && !unlinked && nlen == name.len() {
            // SAFETY: q is a live element of the static table; the
            // explicit `&raw const` borrow avoids the autoref lint
            // when reading through `*mut Queue`.
            let name_ref: &[u8; MAX_NAME_LEN] = unsafe { &*core::ptr::addr_of!((*q).name) };
            if &name_ref[..nlen] == name {
                return Some(i);
            }
        }
        i = i.wrapping_add(1);
    }
    None
}

/// Allocate an unused queue slot and copy `name` into it.  Returns the
/// index, or `None` if the pool is exhausted.
///
/// SAFETY: Caller must hold the lock.
unsafe fn alloc_queue(
    name: &[u8; MAX_NAME_LEN],
    name_len: usize,
    max_msgs: usize,
    msg_size: usize,
) -> Option<usize> {
    let qs = unsafe { queues_ptr() };
    let mut i: usize = 0;
    while i < MAX_QUEUES {
        let q = unsafe { qs.add(i) };
        if !unsafe { (*q).in_use } {
            unsafe {
                (*q).in_use = true;
                (*q).unlinked = false;
                (*q).name = *name;
                (*q).name_len = name_len;
                (*q).refcount = 0;
                (*q).max_msgs = max_msgs;
                (*q).msg_size = msg_size;
                (*q).cur_msgs = 0;
                let mut m: usize = 0;
                while m < MAX_MSGS_PER_QUEUE {
                    (*q).msgs[m].in_use = false;
                    m = m.wrapping_add(1);
                }
            }
            return Some(i);
        }
        i = i.wrapping_add(1);
    }
    None
}

/// Allocate an unused descriptor slot.
///
/// SAFETY: Caller must hold the lock.
unsafe fn alloc_descriptor(queue_idx: usize, nonblock: bool) -> Option<usize> {
    let ds = unsafe { descs_ptr() };
    let mut i: usize = 0;
    while i < MAX_DESCRIPTORS {
        let d = unsafe { ds.add(i) };
        if !unsafe { (*d).in_use } {
            unsafe {
                (*d).in_use = true;
                (*d).queue_idx = queue_idx;
                (*d).nonblock = nonblock;
            }
            return Some(i);
        }
        i = i.wrapping_add(1);
    }
    None
}

/// Resolve `mqdes` to `(descriptor_index, queue_index, nonblock)`.
/// On invalid descriptor sets errno=EBADF and returns `None`.
///
/// SAFETY: Caller must hold the lock.
unsafe fn resolve(mqdes: MqdT) -> Option<(usize, usize, bool)> {
    if mqdes <= 0 {
        errno::set_errno(errno::EBADF);
        return None;
    }
    let idx = (mqdes as usize).wrapping_sub(1);
    if idx >= MAX_DESCRIPTORS {
        errno::set_errno(errno::EBADF);
        return None;
    }
    let ds = unsafe { descs_ptr() };
    let d = unsafe { ds.add(idx) };
    if !unsafe { (*d).in_use } {
        errno::set_errno(errno::EBADF);
        return None;
    }
    Some((idx, unsafe { (*d).queue_idx }, unsafe { (*d).nonblock }))
}

/// Free a queue slot.  Called when the last descriptor closes after
/// `mq_unlink`.
///
/// SAFETY: Caller must hold the lock.
unsafe fn free_queue(qidx: usize) {
    let q = unsafe { queues_ptr().add(qidx) };
    unsafe {
        (*q).in_use = false;
        (*q).unlinked = false;
        (*q).name_len = 0;
        (*q).refcount = 0;
        (*q).cur_msgs = 0;
        // No need to zero data; in_use=false on each message marks it free.
        let mut m: usize = 0;
        while m < MAX_MSGS_PER_QUEUE {
            (*q).msgs[m].in_use = false;
            m = m.wrapping_add(1);
        }
    }
}

// ---------------------------------------------------------------------------
// mq_open
// ---------------------------------------------------------------------------

/// Open (and optionally create) a message queue.
///
/// `oflag` is a combination of `O_RDONLY`/`O_WRONLY`/`O_RDWR` plus
/// optional `O_CREAT`, `O_EXCL`, `O_NONBLOCK`.  `mode` is the
/// creation mode (currently ignored — no permission enforcement).
/// `attr` is the requested attributes when `O_CREAT` is set; if null,
/// defaults are used.
///
/// Returns a positive descriptor on success, -1 with errno on failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_open(
    name: *const u8,
    oflag: i32,
    _mode: u32,
    attr: *const MqAttr,
) -> MqdT {
    // Validate name outside the lock — it's read-only and bounded.
    let (name_buf, name_len) = match unsafe { validate_name(name) } {
        Some(v) => v,
        None => return -1,
    };
    let nonblock = (oflag & crate::fcntl::O_NONBLOCK) != 0;
    let create = (oflag & crate::fcntl::O_CREAT) != 0;
    let excl = (oflag & crate::fcntl::O_EXCL) != 0;

    let _g = lock();

    // SAFETY: Lock held; we mutate the static tables exclusively.
    let existing = unsafe { find_queue_by_name(&name_buf[..name_len]) };

    if let Some(qidx) = existing {
        if create && excl {
            errno::set_errno(errno::EEXIST);
            return -1;
        }
        let qs = unsafe { queues_ptr() };
        let q = unsafe { qs.add(qidx) };
        let didx = match unsafe { alloc_descriptor(qidx, nonblock) } {
            Some(i) => i,
            None => {
                errno::set_errno(errno::EMFILE);
                return -1;
            }
        };
        unsafe { (*q).refcount = (*q).refcount.wrapping_add(1); }
        return (didx as MqdT).wrapping_add(1);
    }

    if !create {
        errno::set_errno(errno::ENOENT);
        return -1;
    }

    // Determine attributes.
    let (max_msgs, msg_size) = if attr.is_null() {
        (DEFAULT_MAXMSG, DEFAULT_MSGSIZE)
    } else {
        // SAFETY: Caller asserts attr is a valid pointer when non-null.
        let (mm, ms) = unsafe { ((*attr).mq_maxmsg, (*attr).mq_msgsize) };
        if mm <= 0 || ms <= 0 {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        let mm_usz = mm as usize;
        let ms_usz = ms as usize;
        if mm_usz > MAX_MSGS_PER_QUEUE || ms_usz > MAX_MSG_SIZE {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        (mm_usz, ms_usz)
    };

    let qidx = match unsafe { alloc_queue(&name_buf, name_len, max_msgs, msg_size) } {
        Some(i) => i,
        None => {
            errno::set_errno(errno::ENOSPC);
            return -1;
        }
    };
    let didx = match unsafe { alloc_descriptor(qidx, nonblock) } {
        Some(i) => i,
        None => {
            // Roll back the freshly-allocated queue.
            unsafe { free_queue(qidx); }
            errno::set_errno(errno::EMFILE);
            return -1;
        }
    };
    let qs = unsafe { queues_ptr() };
    unsafe { (*qs.add(qidx)).refcount = 1; }
    (didx as MqdT).wrapping_add(1)
}

// ---------------------------------------------------------------------------
// mq_close
// ---------------------------------------------------------------------------

/// Close a message queue descriptor.
///
/// Decrements the queue's reference count; if it drops to zero and the
/// queue has been `mq_unlink`'d, the queue's storage is freed.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_close(mqdes: MqdT) -> i32 {
    let _g = lock();
    // SAFETY: Lock held.
    let (didx, qidx, _nonblock) = match unsafe { resolve(mqdes) } {
        Some(t) => t,
        None => return -1,
    };
    unsafe {
        let d = descs_ptr().add(didx);
        (*d).in_use = false;
        let q = queues_ptr().add(qidx);
        if (*q).refcount > 0 {
            (*q).refcount = (*q).refcount.wrapping_sub(1);
        }
        if (*q).refcount == 0 && (*q).unlinked {
            free_queue(qidx);
        }
    }
    0
}

// ---------------------------------------------------------------------------
// mq_unlink
// ---------------------------------------------------------------------------

/// Remove a message queue's name.
///
/// The queue is destroyed once the last open descriptor closes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_unlink(name: *const u8) -> i32 {
    let (name_buf, name_len) = match unsafe { validate_name(name) } {
        Some(v) => v,
        None => return -1,
    };
    let _g = lock();
    // SAFETY: Lock held.
    let qidx = match unsafe { find_queue_by_name(&name_buf[..name_len]) } {
        Some(i) => i,
        None => {
            errno::set_errno(errno::ENOENT);
            return -1;
        }
    };
    unsafe {
        let q = queues_ptr().add(qidx);
        (*q).unlinked = true;
        if (*q).refcount == 0 {
            free_queue(qidx);
        }
    }
    0
}

// ---------------------------------------------------------------------------
// mq_send / mq_timedsend
// ---------------------------------------------------------------------------

/// Insert a message into the queue at the position determined by its
/// priority.  Messages are stored in descending priority order; among
/// equal priorities the insertion is FIFO (later messages go after
/// earlier ones).
///
/// SAFETY: Caller must hold the lock and ensure there is a free slot
/// (`cur_msgs < max_msgs`).
unsafe fn enqueue(qidx: usize, msg: *const u8, len: usize, priority: u32) {
    let q = unsafe { queues_ptr().add(qidx) };
    // Find insertion position: walk from the head; stop before the
    // first message whose priority is strictly less than ours.
    unsafe {
        let cur = (*q).cur_msgs;
        // Find a free slot first.  We store messages in slots 0..cur
        // in priority order, so shift the tail down by one.
        let mut insert_at: usize = cur; // default: append
        let mut k: usize = 0;
        while k < cur {
            if (*q).msgs[k].priority < priority {
                insert_at = k;
                break;
            }
            k = k.wrapping_add(1);
        }
        // Shift messages [insert_at..cur) one slot right.
        let mut j: usize = cur;
        while j > insert_at {
            let prev = j.wrapping_sub(1);
            (*q).msgs[j] = (*q).msgs[prev];
            j = prev;
        }
        // Fill the new slot.
        let slot = &mut (*q).msgs[insert_at];
        slot.in_use = true;
        slot.priority = priority;
        slot.len = len;
        if len > 0 {
            core::ptr::copy_nonoverlapping(msg, slot.data.as_mut_ptr(), len);
        }
        (*q).cur_msgs = cur.wrapping_add(1);
    }
}

/// Validate `mq_send` arguments after the descriptor is resolved.
/// Returns `Ok(())` if OK; on failure sets errno and returns `Err`.
///
/// SAFETY: Caller must hold the lock.
unsafe fn validate_send_args(qidx: usize, msg: *const u8, len: usize, prio: u32) -> Result<(), ()> {
    if msg.is_null() && len > 0 {
        errno::set_errno(errno::EFAULT);
        return Err(());
    }
    if prio >= MQ_PRIO_MAX {
        errno::set_errno(errno::EINVAL);
        return Err(());
    }
    let q = unsafe { queues_ptr().add(qidx) };
    if len > unsafe { (*q).msg_size } {
        errno::set_errno(errno::EMSGSIZE);
        return Err(());
    }
    Ok(())
}

/// Returns `Some(())` if the queue has space for one more message
/// (caller must immediately `enqueue`).  Returns `None` if the queue
/// is full.  Lock must be held.
unsafe fn queue_has_space(qidx: usize) -> bool {
    let q = unsafe { queues_ptr().add(qidx) };
    unsafe { (*q).cur_msgs < (*q).max_msgs }
}

/// Common path for `mq_send` and `mq_timedsend`.
///
/// `deadline_ns`: if `Some`, an absolute deadline in nanoseconds (from
/// `clock_gettime(CLOCK_REALTIME)`); if `None`, blocking is unbounded.
fn send_common(
    mqdes: MqdT,
    msg: *const u8,
    len: usize,
    prio: u32,
    deadline_ns: Option<u64>,
) -> i32 {
    loop {
        let inserted = {
            let _g = lock();
            // SAFETY: Lock held.
            let (_didx, qidx, nonblock) = match unsafe { resolve(mqdes) } {
                Some(t) => t,
                None => return -1,
            };
            if unsafe { validate_send_args(qidx, msg, len, prio) }.is_err() {
                return -1;
            }
            if unsafe { queue_has_space(qidx) } {
                unsafe { enqueue(qidx, msg, len, prio); }
                true
            } else if nonblock {
                errno::set_errno(errno::EAGAIN);
                return -1;
            } else {
                false
            }
        };
        if inserted {
            return 0;
        }
        // Blocking path: drop the lock, yield, check deadline, retry.
        if let Some(deadline) = deadline_ns
            && now_realtime_ns() >= deadline
        {
            errno::set_errno(errno::ETIMEDOUT);
            return -1;
        }
        core::hint::spin_loop();
    }
}

/// Send a message to a queue.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_send(
    mqdes: MqdT,
    msg_ptr: *const u8,
    msg_len: usize,
    msg_prio: u32,
) -> i32 {
    send_common(mqdes, msg_ptr, msg_len, msg_prio, None)
}

/// Send a message to a queue with an absolute deadline (`CLOCK_REALTIME`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_timedsend(
    mqdes: MqdT,
    msg_ptr: *const u8,
    msg_len: usize,
    msg_prio: u32,
    abs_timeout: *const Timespec,
) -> i32 {
    let deadline = match deadline_from_timespec(abs_timeout) {
        Ok(d) => d,
        Err(()) => return -1,
    };
    send_common(mqdes, msg_ptr, msg_len, msg_prio, Some(deadline))
}

// ---------------------------------------------------------------------------
// mq_receive / mq_timedreceive
// ---------------------------------------------------------------------------

/// Pop the highest-priority message from a queue into `out`.
///
/// SAFETY: Caller must hold the lock and ensure `cur_msgs > 0`.
/// `out_capacity` is the caller-supplied buffer size; the message's
/// length is guaranteed `<= msg_size <= out_capacity` by the validate
/// path.  Returns `(prio, len)`.
unsafe fn dequeue(qidx: usize, out: *mut u8, out_capacity: usize) -> (u32, usize) {
    let q = unsafe { queues_ptr().add(qidx) };
    unsafe {
        let cur = (*q).cur_msgs;
        debug_assert!(cur > 0);
        let head = (*q).msgs[0];
        // Shift the remaining messages down by one.
        let mut k: usize = 1;
        while k < cur {
            (*q).msgs[k.wrapping_sub(1)] = (*q).msgs[k];
            k = k.wrapping_add(1);
        }
        (*q).msgs[cur.wrapping_sub(1)].in_use = false;
        (*q).cur_msgs = cur.wrapping_sub(1);
        // Copy out.
        let n = if head.len < out_capacity { head.len } else { out_capacity };
        if n > 0 {
            core::ptr::copy_nonoverlapping(head.data.as_ptr(), out, n);
        }
        (head.priority, head.len)
    }
}

/// Validate `mq_receive` arguments after the descriptor is resolved.
/// Returns `Ok(())` if OK; on failure sets errno and returns `Err`.
///
/// SAFETY: Caller must hold the lock.
unsafe fn validate_recv_args(qidx: usize, buf: *mut u8, buf_len: usize) -> Result<(), ()> {
    if buf.is_null() && buf_len > 0 {
        errno::set_errno(errno::EFAULT);
        return Err(());
    }
    let q = unsafe { queues_ptr().add(qidx) };
    if buf_len < unsafe { (*q).msg_size } {
        errno::set_errno(errno::EMSGSIZE);
        return Err(());
    }
    Ok(())
}

fn recv_common(
    mqdes: MqdT,
    buf: *mut u8,
    buf_len: usize,
    prio_out: *mut u32,
    deadline_ns: Option<u64>,
) -> isize {
    loop {
        let result = {
            let _g = lock();
            // SAFETY: Lock held.
            let (_didx, qidx, nonblock) = match unsafe { resolve(mqdes) } {
                Some(t) => t,
                None => return -1,
            };
            if unsafe { validate_recv_args(qidx, buf, buf_len) }.is_err() {
                return -1;
            }
            let q = unsafe { queues_ptr().add(qidx) };
            if unsafe { (*q).cur_msgs } > 0 {
                let (prio, len) = unsafe { dequeue(qidx, buf, buf_len) };
                Some((prio, len))
            } else if nonblock {
                errno::set_errno(errno::EAGAIN);
                return -1;
            } else {
                None
            }
        };
        if let Some((prio, len)) = result {
            if !prio_out.is_null() {
                // SAFETY: Caller contract — prio_out is writable if non-null.
                unsafe { *prio_out = prio; }
            }
            return len as isize;
        }
        // Blocking path.
        if let Some(deadline) = deadline_ns
            && now_realtime_ns() >= deadline
        {
            errno::set_errno(errno::ETIMEDOUT);
            return -1;
        }
        core::hint::spin_loop();
    }
}

/// Receive the highest-priority message from a queue.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_receive(
    mqdes: MqdT,
    msg_ptr: *mut u8,
    msg_len: usize,
    msg_prio: *mut u32,
) -> isize {
    recv_common(mqdes, msg_ptr, msg_len, msg_prio, None)
}

/// Receive with an absolute deadline (`CLOCK_REALTIME`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_timedreceive(
    mqdes: MqdT,
    msg_ptr: *mut u8,
    msg_len: usize,
    msg_prio: *mut u32,
    abs_timeout: *const Timespec,
) -> isize {
    let deadline = match deadline_from_timespec(abs_timeout) {
        Ok(d) => d,
        Err(()) => return -1,
    };
    recv_common(mqdes, msg_ptr, msg_len, msg_prio, Some(deadline))
}

// ---------------------------------------------------------------------------
// mq_getattr / mq_setattr
// ---------------------------------------------------------------------------

/// Get the current attributes of a message queue.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_getattr(mqdes: MqdT, attr: *mut MqAttr) -> i32 {
    if attr.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let _g = lock();
    // SAFETY: Lock held.
    let (_didx, qidx, nonblock) = match unsafe { resolve(mqdes) } {
        Some(t) => t,
        None => return -1,
    };
    let q = unsafe { queues_ptr().add(qidx) };
    // SAFETY: attr non-null.
    unsafe {
        (*attr).mq_flags = if nonblock { crate::fcntl::O_NONBLOCK as i64 } else { 0 };
        (*attr).mq_maxmsg = (*q).max_msgs as i64;
        (*attr).mq_msgsize = (*q).msg_size as i64;
        (*attr).mq_curmsgs = (*q).cur_msgs as i64;
    }
    0
}

/// Set the attributes of a message queue.
///
/// Per POSIX, only `mq_flags` (the `O_NONBLOCK` bit) is mutable.  The
/// other fields are ignored.  If `oldattr` is non-null the previous
/// attributes are written there.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_setattr(
    mqdes: MqdT,
    newattr: *const MqAttr,
    oldattr: *mut MqAttr,
) -> i32 {
    if newattr.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let _g = lock();
    // SAFETY: Lock held.
    let (didx, qidx, nonblock_old) = match unsafe { resolve(mqdes) } {
        Some(t) => t,
        None => return -1,
    };
    let q = unsafe { queues_ptr().add(qidx) };
    if !oldattr.is_null() {
        // SAFETY: caller contract.
        unsafe {
            (*oldattr).mq_flags = if nonblock_old { crate::fcntl::O_NONBLOCK as i64 } else { 0 };
            (*oldattr).mq_maxmsg = (*q).max_msgs as i64;
            (*oldattr).mq_msgsize = (*q).msg_size as i64;
            (*oldattr).mq_curmsgs = (*q).cur_msgs as i64;
        }
    }
    // SAFETY: newattr non-null.
    let flags = unsafe { (*newattr).mq_flags };
    let nonblock_new = (flags & i64::from(crate::fcntl::O_NONBLOCK)) != 0;
    let d = unsafe { descs_ptr().add(didx) };
    unsafe { (*d).nonblock = nonblock_new; }
    0
}

// ---------------------------------------------------------------------------
// mq_notify
// ---------------------------------------------------------------------------

/// Request notification on message arrival.
///
/// Returns -1 with `ENOSYS` after argument-domain validation.  Full
/// `mq_notify` requires a `sigevent` dispatcher that we don't have
/// yet (no SIGEV_SIGNAL or SIGEV_THREAD routing), but invalid
/// callers must still see Linux-matching errno values so portable
/// code (D-Bus signal-arrival hooks, async-mq tutorials) reads us
/// correctly.
///
/// Validation order matches `ipc/mqueue.c::sys_mq_notify` in Linux:
/// 1. `mqdes` must be a valid open mq descriptor → `EBADF` otherwise
///    (via `resolve`, which is the same gate every other mq_* call
///    uses; consistent errno across the API).
/// 2. After validation:
///    - `sevp == NULL` is the "deregister notification" form on Linux;
///      we accept it as valid (no notification was registered, so
///      "deregister" is observably a no-op success).
///    - `sevp != NULL` would register a notification, but we have no
///      sigevent dispatcher — return `ENOSYS` so callers fall back to
///      polling.
///
/// We can't inspect `*sevp` for `sigev_notify` validity because the
/// parameter is `*const u8` in our ABI (kept opaque to avoid pulling
/// the full `sigevent` layout through the signal crate).  Linux
/// rejects invalid `sigev_notify` with `EINVAL`; we defer that check
/// until the dispatcher exists and the struct is plumbed through.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_notify(mqdes: MqdT, sevp: *const u8) -> i32 {
    let guard = lock();
    // SAFETY: held under MQ_LOCK via `guard`.
    if unsafe { resolve(mqdes) }.is_none() {
        drop(guard);
        return -1; // errno set by resolve()
    }
    drop(guard);

    if sevp.is_null() {
        // Deregister: nothing was registered, so this is a no-op.
        // Linux returns 0 here too.
        return 0;
    }
    // sevp non-NULL — we'd register a notification, but no dispatcher.
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Timespec helpers
// ---------------------------------------------------------------------------

/// Read the current `CLOCK_REALTIME` (which on our system is the same
/// monotonic clock as `CLOCK_MONOTONIC`) as nanoseconds since boot.
fn now_realtime_ns() -> u64 {
    let mut ts = Timespec { tv_sec: 0, tv_nsec: 0 };
    let r = crate::time::clock_gettime(crate::time::CLOCK_REALTIME, &raw mut ts);
    if r != 0 {
        return 0;
    }
    timespec_to_ns(&ts)
}

fn timespec_to_ns(ts: &Timespec) -> u64 {
    let sec = ts.tv_sec.max(0) as u64;
    let nsec = ts.tv_nsec.max(0) as u64;
    sec.saturating_mul(1_000_000_000).saturating_add(nsec)
}

/// Convert an absolute-timeout `Timespec` pointer to a `CLOCK_REALTIME`
/// nanosecond deadline.  Returns `Err(())` after setting errno on
/// invalid input.
fn deadline_from_timespec(p: *const Timespec) -> Result<u64, ()> {
    if p.is_null() {
        errno::set_errno(errno::EFAULT);
        return Err(());
    }
    // SAFETY: caller contract.
    let ts = unsafe { *p };
    if ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
        errno::set_errno(errno::EINVAL);
        return Err(());
    }
    Ok(timespec_to_ns(&ts))
}

// ---------------------------------------------------------------------------
// Test-only helpers
// ---------------------------------------------------------------------------

/// Reset the entire mqueue subsystem to its cold-boot state.  Used by
/// tests to avoid cross-test contamination of the static tables.
#[cfg(test)]
fn reset_all() {
    let _g = lock();
    // SAFETY: Lock held.
    unsafe {
        let qs = queues_ptr();
        let mut i: usize = 0;
        while i < MAX_QUEUES {
            (*qs.add(i)).in_use = false;
            (*qs.add(i)).unlinked = false;
            (*qs.add(i)).refcount = 0;
            (*qs.add(i)).cur_msgs = 0;
            (*qs.add(i)).name_len = 0;
            let mut m: usize = 0;
            while m < MAX_MSGS_PER_QUEUE {
                (*qs.add(i)).msgs[m].in_use = false;
                m = m.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        let ds = descs_ptr();
        let mut j: usize = 0;
        while j < MAX_DESCRIPTORS {
            (*ds.add(j)).in_use = false;
            j = j.wrapping_add(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcntl::{O_CREAT, O_EXCL, O_NONBLOCK, O_RDWR};

    /// Tests that mutate the global mqueue tables serialize on this
    /// lock to avoid cross-test races (the static state is shared
    /// process-wide).
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock_tests() -> std::sync::MutexGuard<'static, ()> {
        let g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_all();
        g
    }

    fn open_default(name: &[u8], oflag: i32) -> MqdT {
        mq_open(name.as_ptr(), oflag | O_CREAT | O_RDWR, 0o600, core::ptr::null())
    }

    // -- MqAttr layout --

    #[test]
    fn test_mq_attr_size() {
        assert_eq!(core::mem::size_of::<MqAttr>(), 64);
    }

    #[test]
    fn test_mq_attr_alignment() {
        assert!(core::mem::align_of::<MqAttr>() >= 8);
    }

    #[test]
    fn test_mqd_t_is_i32() {
        assert_eq!(core::mem::size_of::<MqdT>(), 4);
    }

    // -- mq_open: name validation --

    #[test]
    fn test_open_null_name() {
        let _g = lock_tests();
        let r = mq_open(core::ptr::null(), O_CREAT | O_RDWR, 0, core::ptr::null());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_open_name_without_leading_slash() {
        let _g = lock_tests();
        let r = open_default(b"nolead\0", O_NONBLOCK);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_open_name_with_internal_slash() {
        let _g = lock_tests();
        let r = open_default(b"/a/b\0", O_NONBLOCK);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_open_name_just_slash() {
        let _g = lock_tests();
        let r = open_default(b"/\0", O_NONBLOCK);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_open_creates_new_queue() {
        let _g = lock_tests();
        let r = open_default(b"/q1\0", O_NONBLOCK);
        assert!(r > 0, "expected positive mqd, got {r}");
        assert_eq!(mq_close(r), 0);
    }

    #[test]
    fn test_open_existing_without_o_creat_succeeds() {
        let _g = lock_tests();
        let a = open_default(b"/qexist\0", O_NONBLOCK);
        assert!(a > 0);
        // Reopen without O_CREAT.
        let b = mq_open(b"/qexist\0".as_ptr(), O_RDWR | O_NONBLOCK, 0, core::ptr::null());
        assert!(b > 0, "reopen without O_CREAT should succeed (got {b})");
        assert_eq!(mq_close(a), 0);
        assert_eq!(mq_close(b), 0);
    }

    #[test]
    fn test_open_missing_without_o_creat_enoent() {
        let _g = lock_tests();
        let r = mq_open(b"/no_such\0".as_ptr(), O_RDWR | O_NONBLOCK, 0, core::ptr::null());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    #[test]
    fn test_open_o_excl_on_existing_eexist() {
        let _g = lock_tests();
        let a = open_default(b"/qexcl\0", O_NONBLOCK);
        assert!(a > 0);
        let b = mq_open(
            b"/qexcl\0".as_ptr(),
            O_CREAT | O_EXCL | O_RDWR | O_NONBLOCK,
            0,
            core::ptr::null(),
        );
        assert_eq!(b, -1);
        assert_eq!(errno::get_errno(), errno::EEXIST);
        assert_eq!(mq_close(a), 0);
    }

    #[test]
    fn test_open_with_attr_too_big() {
        let _g = lock_tests();
        let attr = MqAttr {
            mq_flags: 0,
            mq_maxmsg: (MAX_MSGS_PER_QUEUE as i64) + 1,
            mq_msgsize: 64,
            mq_curmsgs: 0,
            _pad: [0; 4],
        };
        let r = mq_open(
            b"/qbig\0".as_ptr(),
            O_CREAT | O_RDWR | O_NONBLOCK,
            0,
            &raw const attr,
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_open_with_attr_negative_eINVAL() {
        let _g = lock_tests();
        let attr = MqAttr {
            mq_flags: 0,
            mq_maxmsg: -1,
            mq_msgsize: 64,
            mq_curmsgs: 0,
            _pad: [0; 4],
        };
        let r = mq_open(
            b"/qneg\0".as_ptr(),
            O_CREAT | O_RDWR | O_NONBLOCK,
            0,
            &raw const attr,
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_open_exhaust_queues() {
        let _g = lock_tests();
        let mut fds = [0i32; MAX_QUEUES];
        let mut names: [[u8; 8]; MAX_QUEUES] = [[0u8; 8]; MAX_QUEUES];
        for i in 0..MAX_QUEUES {
            names[i][0] = b'/';
            names[i][1] = b'a' + (i as u8);
            names[i][2] = 0;
            fds[i] = open_default(&names[i], O_NONBLOCK);
            assert!(fds[i] > 0, "open {i} failed");
        }
        // One more should fail with ENOSPC.
        let extra = open_default(b"/zzz\0", O_NONBLOCK);
        assert_eq!(extra, -1);
        assert_eq!(errno::get_errno(), errno::ENOSPC);
        for fd in fds {
            assert_eq!(mq_close(fd), 0);
        }
    }

    // -- mq_close --

    #[test]
    fn test_close_invalid_descriptor() {
        let _g = lock_tests();
        assert_eq!(mq_close(0), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
        assert_eq!(mq_close(-1), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
        assert_eq!(mq_close(9999), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_close_twice_second_is_ebadf() {
        let _g = lock_tests();
        let fd = open_default(b"/qcl\0", O_NONBLOCK);
        assert!(fd > 0);
        assert_eq!(mq_close(fd), 0);
        assert_eq!(mq_close(fd), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -- mq_unlink --

    #[test]
    fn test_unlink_missing_enoent() {
        let _g = lock_tests();
        let r = mq_unlink(b"/no_such\0".as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    #[test]
    fn test_unlink_existing() {
        let _g = lock_tests();
        let fd = open_default(b"/qun\0", O_NONBLOCK);
        assert!(fd > 0);
        assert_eq!(mq_unlink(b"/qun\0".as_ptr()), 0);
        // Reopen without O_CREAT should now fail.
        let r = mq_open(b"/qun\0".as_ptr(), O_RDWR | O_NONBLOCK, 0, core::ptr::null());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
        // Original descriptor still works until closed.
        assert_eq!(mq_close(fd), 0);
    }

    // -- mq_send / mq_receive basic --

    #[test]
    fn test_send_receive_roundtrip() {
        let _g = lock_tests();
        let fd = open_default(b"/qsr\0", O_NONBLOCK);
        assert!(fd > 0);
        assert_eq!(mq_send(fd, b"hello".as_ptr(), 5, 7), 0);
        let mut buf = [0u8; 64];
        let mut prio: u32 = 0;
        let n = mq_receive(fd, buf.as_mut_ptr(), 64, &raw mut prio);
        assert_eq!(n, 5);
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(prio, 7);
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_priority_ordering() {
        let _g = lock_tests();
        let fd = open_default(b"/qpri\0", O_NONBLOCK);
        assert!(fd > 0);
        // Send messages out of priority order.
        assert_eq!(mq_send(fd, b"low".as_ptr(), 3, 1), 0);
        assert_eq!(mq_send(fd, b"high".as_ptr(), 4, 10), 0);
        assert_eq!(mq_send(fd, b"mid".as_ptr(), 3, 5), 0);
        // Expect highest priority first.
        let mut buf = [0u8; 64];
        let mut p: u32 = 0;
        let n = mq_receive(fd, buf.as_mut_ptr(), 64, &raw mut p);
        assert_eq!(n, 4);
        assert_eq!(&buf[..4], b"high");
        assert_eq!(p, 10);
        let n = mq_receive(fd, buf.as_mut_ptr(), 64, &raw mut p);
        assert_eq!(n, 3);
        assert_eq!(&buf[..3], b"mid");
        assert_eq!(p, 5);
        let n = mq_receive(fd, buf.as_mut_ptr(), 64, &raw mut p);
        assert_eq!(n, 3);
        assert_eq!(&buf[..3], b"low");
        assert_eq!(p, 1);
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_fifo_within_same_priority() {
        let _g = lock_tests();
        let fd = open_default(b"/qfifo\0", O_NONBLOCK);
        assert!(fd > 0);
        assert_eq!(mq_send(fd, b"a".as_ptr(), 1, 5), 0);
        assert_eq!(mq_send(fd, b"b".as_ptr(), 1, 5), 0);
        assert_eq!(mq_send(fd, b"c".as_ptr(), 1, 5), 0);
        let mut buf = [0u8; 64];
        let mut p: u32 = 0;
        assert_eq!(mq_receive(fd, buf.as_mut_ptr(), 64, &raw mut p), 1);
        assert_eq!(buf[0], b'a');
        assert_eq!(mq_receive(fd, buf.as_mut_ptr(), 64, &raw mut p), 1);
        assert_eq!(buf[0], b'b');
        assert_eq!(mq_receive(fd, buf.as_mut_ptr(), 64, &raw mut p), 1);
        assert_eq!(buf[0], b'c');
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_send_oversize_emsgsize() {
        let _g = lock_tests();
        let fd = open_default(b"/qsz\0", O_NONBLOCK);
        assert!(fd > 0);
        let big = [0u8; DEFAULT_MSGSIZE + 1];
        let r = mq_send(fd, big.as_ptr(), big.len(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EMSGSIZE);
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_send_priority_too_high_einval() {
        let _g = lock_tests();
        let fd = open_default(b"/qprbig\0", O_NONBLOCK);
        assert!(fd > 0);
        let r = mq_send(fd, b"x".as_ptr(), 1, MQ_PRIO_MAX);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_send_when_full_nonblock_eagain() {
        let _g = lock_tests();
        let attr = MqAttr {
            mq_flags: 0,
            mq_maxmsg: 2,
            mq_msgsize: 16,
            mq_curmsgs: 0,
            _pad: [0; 4],
        };
        let fd = mq_open(
            b"/qfull\0".as_ptr(),
            O_CREAT | O_RDWR | O_NONBLOCK,
            0,
            &raw const attr,
        );
        assert!(fd > 0);
        assert_eq!(mq_send(fd, b"a".as_ptr(), 1, 0), 0);
        assert_eq!(mq_send(fd, b"b".as_ptr(), 1, 0), 0);
        let r = mq_send(fd, b"c".as_ptr(), 1, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EAGAIN);
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_receive_when_empty_nonblock_eagain() {
        let _g = lock_tests();
        let fd = open_default(b"/qempty\0", O_NONBLOCK);
        assert!(fd > 0);
        let mut buf = [0u8; 64];
        let r = mq_receive(fd, buf.as_mut_ptr(), 64, core::ptr::null_mut());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EAGAIN);
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_receive_small_buffer_emsgsize() {
        let _g = lock_tests();
        let fd = open_default(b"/qsmall\0", O_NONBLOCK);
        assert!(fd > 0);
        assert_eq!(mq_send(fd, b"x".as_ptr(), 1, 0), 0);
        // Buffer smaller than mq_msgsize (default 64) should fail.
        let mut buf = [0u8; 8];
        let r = mq_receive(fd, buf.as_mut_ptr(), 8, core::ptr::null_mut());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EMSGSIZE);
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_null_prio_pointer_ok() {
        let _g = lock_tests();
        let fd = open_default(b"/qnp\0", O_NONBLOCK);
        assert!(fd > 0);
        assert_eq!(mq_send(fd, b"x".as_ptr(), 1, 3), 0);
        let mut buf = [0u8; 64];
        let n = mq_receive(fd, buf.as_mut_ptr(), 64, core::ptr::null_mut());
        assert_eq!(n, 1);
        assert_eq!(mq_close(fd), 0);
    }

    // -- mq_getattr / mq_setattr --

    #[test]
    fn test_getattr_defaults() {
        let _g = lock_tests();
        let fd = open_default(b"/qa\0", O_NONBLOCK);
        assert!(fd > 0);
        let mut a = MqAttr {
            mq_flags: 0, mq_maxmsg: 0, mq_msgsize: 0, mq_curmsgs: 0, _pad: [0; 4],
        };
        assert_eq!(mq_getattr(fd, &raw mut a), 0);
        assert_eq!(a.mq_maxmsg, DEFAULT_MAXMSG as i64);
        assert_eq!(a.mq_msgsize, DEFAULT_MSGSIZE as i64);
        assert_eq!(a.mq_curmsgs, 0);
        assert_eq!(a.mq_flags, crate::fcntl::O_NONBLOCK as i64);
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_getattr_curmsgs_updates() {
        let _g = lock_tests();
        let fd = open_default(b"/qc\0", O_NONBLOCK);
        assert!(fd > 0);
        assert_eq!(mq_send(fd, b"a".as_ptr(), 1, 0), 0);
        assert_eq!(mq_send(fd, b"b".as_ptr(), 1, 0), 0);
        let mut a = MqAttr {
            mq_flags: 0, mq_maxmsg: 0, mq_msgsize: 0, mq_curmsgs: 0, _pad: [0; 4],
        };
        assert_eq!(mq_getattr(fd, &raw mut a), 0);
        assert_eq!(a.mq_curmsgs, 2);
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_setattr_toggles_nonblock() {
        let _g = lock_tests();
        let fd = open_default(b"/qsa\0", O_NONBLOCK);
        assert!(fd > 0);
        // Clear O_NONBLOCK via setattr.
        let new = MqAttr {
            mq_flags: 0, mq_maxmsg: 99, mq_msgsize: 99, mq_curmsgs: 0, _pad: [0; 4],
        };
        let mut old = MqAttr {
            mq_flags: 0, mq_maxmsg: 0, mq_msgsize: 0, mq_curmsgs: 0, _pad: [0; 4],
        };
        assert_eq!(mq_setattr(fd, &raw const new, &raw mut old), 0);
        assert_eq!(old.mq_flags, crate::fcntl::O_NONBLOCK as i64);
        // Confirm via getattr (maxmsg/msgsize should NOT have changed).
        let mut now = MqAttr {
            mq_flags: 0, mq_maxmsg: 0, mq_msgsize: 0, mq_curmsgs: 0, _pad: [0; 4],
        };
        assert_eq!(mq_getattr(fd, &raw mut now), 0);
        assert_eq!(now.mq_flags, 0);
        assert_eq!(now.mq_maxmsg, DEFAULT_MAXMSG as i64);
        assert_eq!(now.mq_msgsize, DEFAULT_MSGSIZE as i64);
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_setattr_null_old_ok() {
        let _g = lock_tests();
        let fd = open_default(b"/qsno\0", O_NONBLOCK);
        assert!(fd > 0);
        let new = MqAttr {
            mq_flags: crate::fcntl::O_NONBLOCK as i64,
            mq_maxmsg: 0, mq_msgsize: 0, mq_curmsgs: 0, _pad: [0; 4],
        };
        assert_eq!(mq_setattr(fd, &raw const new, core::ptr::null_mut()), 0);
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_setattr_null_new_efault() {
        let _g = lock_tests();
        let fd = open_default(b"/qse\0", O_NONBLOCK);
        assert!(fd > 0);
        let r = mq_setattr(fd, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_getattr_null_attr_efault() {
        let _g = lock_tests();
        let fd = open_default(b"/qge\0", O_NONBLOCK);
        assert!(fd > 0);
        let r = mq_getattr(fd, core::ptr::null_mut());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        assert_eq!(mq_close(fd), 0);
    }

    // -- mq_timedsend / mq_timedreceive --

    #[test]
    fn test_timedreceive_expired_etimedout() {
        let _g = lock_tests();
        // Use a blocking descriptor (no O_NONBLOCK), empty queue, and a
        // past-deadline timespec.  Expect ETIMEDOUT.
        let fd = open_default(b"/qto\0", 0);
        assert!(fd > 0);
        let past = Timespec { tv_sec: 0, tv_nsec: 0 };
        let mut buf = [0u8; 64];
        let r = mq_timedreceive(fd, buf.as_mut_ptr(), 64, core::ptr::null_mut(), &raw const past);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ETIMEDOUT);
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_timedsend_success_immediate() {
        let _g = lock_tests();
        let fd = open_default(b"/qts\0", O_NONBLOCK);
        assert!(fd > 0);
        // Future deadline; queue is empty, so the send happens immediately.
        let mut now = Timespec { tv_sec: 0, tv_nsec: 0 };
        crate::time::clock_gettime(crate::time::CLOCK_REALTIME, &raw mut now);
        let deadline = Timespec {
            tv_sec: now.tv_sec + 60,
            tv_nsec: now.tv_nsec,
        };
        let r = mq_timedsend(fd, b"x".as_ptr(), 1, 0, &raw const deadline);
        assert_eq!(r, 0);
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_timedsend_invalid_nsec_einval() {
        let _g = lock_tests();
        let fd = open_default(b"/qtinv\0", O_NONBLOCK);
        assert!(fd > 0);
        let bad = Timespec { tv_sec: 0, tv_nsec: 2_000_000_000 };
        let r = mq_timedsend(fd, b"x".as_ptr(), 1, 0, &raw const bad);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(mq_close(fd), 0);
    }

    #[test]
    fn test_timedreceive_null_timespec_efault() {
        let _g = lock_tests();
        let fd = open_default(b"/qtnull\0", O_NONBLOCK);
        assert!(fd > 0);
        let mut buf = [0u8; 64];
        let r = mq_timedreceive(fd, buf.as_mut_ptr(), 64, core::ptr::null_mut(), core::ptr::null());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        assert_eq!(mq_close(fd), 0);
    }

    // -- mq_notify reaches ENOSYS on the register form --
    //
    // Original (pre-Phase 63) test asserted ENOSYS for `mq_notify(1, NULL)`
    // because mq_notify was an unconditional ENOSYS stub.  After
    // Phase 63 added validation:
    //   - NULL sevp is the "deregister" form, which Linux returns 0 for.
    //   - Only non-NULL sevp on a valid mqdes reaches ENOSYS (no
    //     sigevent dispatcher).
    // So the meaningful ENOSYS path now requires both an open queue and
    // a non-NULL sevp pointer.

    #[test]
    fn test_notify_enosys() {
        let _g = lock_tests();
        let fd = open_default(b"/qenosys\0", O_NONBLOCK);
        assert!(fd > 0);
        // Dummy sevp — content unread by the stub.
        let dummy: [u8; 64] = [0; 64];
        errno::set_errno(0);
        let r = mq_notify(fd, dummy.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
        let _ = mq_close(fd);
        let _ = mq_unlink(b"/qenosys\0".as_ptr());
    }

    // -- Reference counting through close/unlink --

    #[test]
    fn test_two_opens_one_unlink_close_both() {
        let _g = lock_tests();
        let a = open_default(b"/qrc\0", O_NONBLOCK);
        let b = mq_open(b"/qrc\0".as_ptr(), O_RDWR | O_NONBLOCK, 0, core::ptr::null());
        assert!(a > 0 && b > 0);
        assert_ne!(a, b);
        // Send via a, receive via b.
        assert_eq!(mq_send(a, b"shared".as_ptr(), 6, 0), 0);
        let mut buf = [0u8; 64];
        let n = mq_receive(b, buf.as_mut_ptr(), 64, core::ptr::null_mut());
        assert_eq!(n, 6);
        assert_eq!(&buf[..6], b"shared");
        // Unlink then close both — the queue should be fully gone.
        assert_eq!(mq_unlink(b"/qrc\0".as_ptr()), 0);
        assert_eq!(mq_close(a), 0);
        assert_eq!(mq_close(b), 0);
        // Reopen without O_CREAT must now fail.
        let r = mq_open(b"/qrc\0".as_ptr(), O_RDWR | O_NONBLOCK, 0, core::ptr::null());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    // -- Sanity: messages survive across close+reopen as long as the
    //    queue isn't unlinked + last-closed. --

    #[test]
    fn test_messages_persist_across_descriptor_close() {
        let _g = lock_tests();
        let a = open_default(b"/qpersist\0", O_NONBLOCK);
        assert!(a > 0);
        assert_eq!(mq_send(a, b"keep".as_ptr(), 4, 0), 0);
        let b = mq_open(b"/qpersist\0".as_ptr(), O_RDWR | O_NONBLOCK, 0, core::ptr::null());
        assert!(b > 0);
        // Close a; queue stays alive because b holds it open.
        assert_eq!(mq_close(a), 0);
        let mut buf = [0u8; 64];
        let n = mq_receive(b, buf.as_mut_ptr(), 64, core::ptr::null_mut());
        assert_eq!(n, 4);
        assert_eq!(&buf[..4], b"keep");
        assert_eq!(mq_close(b), 0);
        assert_eq!(mq_unlink(b"/qpersist\0".as_ptr()), 0);
    }

    // -- Empty (zero-length) message --

    #[test]
    fn test_zero_length_message() {
        let _g = lock_tests();
        let fd = open_default(b"/qzero\0", O_NONBLOCK);
        assert!(fd > 0);
        assert_eq!(mq_send(fd, core::ptr::null(), 0, 0), 0);
        let mut buf = [0u8; 64];
        let n = mq_receive(fd, buf.as_mut_ptr(), 64, core::ptr::null_mut());
        assert_eq!(n, 0);
        assert_eq!(mq_close(fd), 0);
    }

    // -- Send with null buf and non-zero length is EFAULT --

    #[test]
    fn test_send_null_msg_nonzero_len_efault() {
        let _g = lock_tests();
        let fd = open_default(b"/qnb\0", O_NONBLOCK);
        assert!(fd > 0);
        let r = mq_send(fd, core::ptr::null(), 4, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        assert_eq!(mq_close(fd), 0);
    }

    // -- Receive with null buf and non-zero len is EFAULT --

    #[test]
    fn test_receive_null_buf_nonzero_len_efault() {
        let _g = lock_tests();
        let fd = open_default(b"/qnrb\0", O_NONBLOCK);
        assert!(fd > 0);
        let r = mq_receive(fd, core::ptr::null_mut(), 100, core::ptr::null_mut());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        assert_eq!(mq_close(fd), 0);
    }

    // -- Descriptor exhaustion --

    #[test]
    fn test_descriptor_exhaustion_emfile() {
        let _g = lock_tests();
        let fd0 = open_default(b"/qex\0", O_NONBLOCK);
        assert!(fd0 > 0);
        // Open the same queue MAX_DESCRIPTORS-1 more times (fd0 already
        // consumed one slot).  We'll reach EMFILE on the next.
        let mut extras: Vec<i32> = Vec::new();
        for _ in 1..MAX_DESCRIPTORS {
            let r = mq_open(b"/qex\0".as_ptr(), O_RDWR | O_NONBLOCK, 0, core::ptr::null());
            assert!(r > 0);
            extras.push(r);
        }
        // Next open should EMFILE.
        let over = mq_open(b"/qex\0".as_ptr(), O_RDWR | O_NONBLOCK, 0, core::ptr::null());
        assert_eq!(over, -1);
        assert_eq!(errno::get_errno(), errno::EMFILE);
        assert_eq!(mq_close(fd0), 0);
        for e in extras {
            assert_eq!(mq_close(e), 0);
        }
    }

    // -----------------------------------------------------------------------
    // Phase 63: mq_notify argument-domain validation
    // -----------------------------------------------------------------------

    #[test]
    fn test_mq_notify_negative_mqdes_ebadf() {
        let _g = lock_tests();
        errno::set_errno(0);
        let ret = mq_notify(-1, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_mq_notify_zero_mqdes_ebadf() {
        let _g = lock_tests();
        // resolve() rejects mqdes <= 0 with EBADF.
        errno::set_errno(0);
        let ret = mq_notify(0, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_mq_notify_unopened_mqdes_ebadf() {
        let _g = lock_tests();
        // A value within range but not associated with an open queue.
        errno::set_errno(0);
        let ret = mq_notify(5, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_mq_notify_huge_mqdes_ebadf() {
        let _g = lock_tests();
        errno::set_errno(0);
        let ret = mq_notify(i32::MAX, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_mq_notify_null_sevp_deregister_success() {
        // NULL sevp is the "deregister notification" form.  Linux
        // returns 0 even when no notification was registered.
        let _g = lock_tests();
        let fd = open_default(b"/qnotify_dereg\0", O_NONBLOCK);
        assert!(fd > 0);
        errno::set_errno(0);
        let ret = mq_notify(fd, core::ptr::null());
        assert_eq!(ret, 0);
        let _ = mq_close(fd);
        let _ = mq_unlink(b"/qnotify_dereg\0".as_ptr());
    }

    #[test]
    fn test_mq_notify_nonnull_sevp_returns_enosys() {
        // Valid mqdes + non-NULL sevp would register a notification,
        // but we have no sigevent dispatcher — return ENOSYS.
        let _g = lock_tests();
        let fd = open_default(b"/qnotify_reg\0", O_NONBLOCK);
        assert!(fd > 0);
        // Dummy "sigevent" — content unread by our stub.
        let dummy: [u8; 64] = [0; 64];
        errno::set_errno(0);
        let ret = mq_notify(fd, dummy.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
        let _ = mq_close(fd);
        let _ = mq_unlink(b"/qnotify_reg\0".as_ptr());
    }

    #[test]
    fn test_mq_notify_after_close_ebadf() {
        // Closed descriptors must not register notifications.
        let _g = lock_tests();
        let fd = open_default(b"/qnotify_closed\0", O_NONBLOCK);
        assert!(fd > 0);
        assert_eq!(mq_close(fd), 0);
        errno::set_errno(0);
        let ret = mq_notify(fd, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
        let _ = mq_unlink(b"/qnotify_closed\0".as_ptr());
    }

    #[test]
    fn test_mq_notify_ordering_fd_before_sevp() {
        // Bad mqdes AND non-NULL sevp — EBADF wins because we cannot
        // even reach the sevp-NULL check without a valid descriptor.
        let _g = lock_tests();
        let dummy: [u8; 64] = [0; 64];
        errno::set_errno(0);
        let ret = mq_notify(-1, dummy.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_mq_notify_workflow_register_then_deregister() {
        // Real workflow: program registers (unsupported → ENOSYS),
        // falls back to polling, and on shutdown deregisters (NULL
        // sevp → 0).  Both must work on the same valid descriptor.
        let _g = lock_tests();
        let fd = open_default(b"/qnotify_wf\0", O_NONBLOCK);
        assert!(fd > 0);

        let sevp: [u8; 64] = [0; 64];
        errno::set_errno(0);
        assert_eq!(mq_notify(fd, sevp.as_ptr()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);

        errno::set_errno(0);
        assert_eq!(mq_notify(fd, core::ptr::null()), 0);

        let _ = mq_close(fd);
        let _ = mq_unlink(b"/qnotify_wf\0".as_ptr());
    }

    #[test]
    fn test_mq_notify_buggy_caller_passes_close_fd_value() {
        // Caller stores the result of mq_close() (which is 0 on
        // success) into a variable they later think is a descriptor.
        // mqdes == 0 is EBADF, not silent success.
        let _g = lock_tests();
        let fd = open_default(b"/qnotify_bug\0", O_NONBLOCK);
        assert!(fd > 0);
        let closed = mq_close(fd);
        assert_eq!(closed, 0);
        errno::set_errno(0);
        // Caller mistakenly treats `closed` (0) as the mqdes.
        let ret = mq_notify(closed, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
        let _ = mq_unlink(b"/qnotify_bug\0".as_ptr());
    }
}
