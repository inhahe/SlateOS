// Index arithmetic in this file is over a queue's own arrays -- `maxmsg`
// message entries, a free-slot stack of `maxmsg` entries, and `maxmsg *
// msgsize` bytes of data -- with `cur <= maxmsg` held by every path that
// changes `cur`, and message lengths checked against `msgsize` before any
// copy.  The bounds are established where the arrays are made; clippy
// cannot see across them.
#![allow(clippy::arithmetic_side_effects)]
//! POSIX message queues (`<mqueue.h>`): Linux 6.6's semantics (ipc/mqueue.c)
//! behind glibc 2.39's wrappers -- inside one process.
//!
//! Queues are named objects kept in this process: two processes that open
//! the same name each get their own queue.  Sharing them between programs is
//! open question D-Q3 (`open-questions.md`); everything else is Linux's:
//!
//! - **Names** are glibc's and the kernel's: a leading `/` (glibc,
//!   `EINVAL`), then a component the kernel looks up -- empty is `ENOENT`,
//!   `.`, `..` or one containing `/` is `EACCES`, longer than `NAME_MAX` is
//!   `ENAMETOOLONG`.
//! - **Sizes** are the kernel's: a queue opened without attributes holds 10
//!   messages of up to 8192 bytes; asked-for sizes may reach 10 and 8192, or
//!   65536 and 16 MiB with `CAP_SYS_RESOURCE`.  A process may hold 256
//!   queues (more with `CAP_SYS_RESOURCE`, `ENOSPC` otherwise).  Storage is
//!   allocated per queue, at its size.
//! - **Access** is the descriptor's: sending needs `O_WRONLY` or `O_RDWR`,
//!   receiving `O_RDONLY` or `O_RDWR` (`EBADF` otherwise).
//! - **Errors come in the kernel's order** -- the timeout, then the
//!   priority, the descriptor, its access, the size, and the buffer last:
//!   `mq_receive` into a NULL buffer takes the message and then fails with
//!   `EFAULT`, as Linux's `store_msg` does.
//! - **Blocking** sleeps on a futex until the queues change, and a timed
//!   call wakes at its `CLOCK_REALTIME` deadline (`ETIMEDOUT`).
//! - **`mq_notify`** registers one notification per queue (`EBUSY` for a
//!   second), fired when a message arrives in the empty queue and no
//!   receiver is waiting, then dropped -- `SIGEV_THREAD` on a new thread,
//!   `SIGEV_SIGNAL` by `raise` ([`crate::sigevent`]).
//!
//! ## What changed on 2026-09-26
//!
//! The queues were a static pool: 8 queues of 32 messages of 256 bytes, with
//! names up to 63 bytes and a default message size of 64 -- so a program that
//! opened a queue with no attributes and sent 100 bytes got `EMSGSIZE`.
//! Access modes were not kept, so sending on a read-only descriptor worked;
//! `O_WRONLY|O_RDWR` was refused before the lookup, not only for an existing
//! queue; a NULL buffer was `EFAULT` before the descriptor or the size was
//! looked at; a blocked call spun without yielding; and `mq_notify` was
//! `ENOSYS`.  (`known-issues.md` →
//! `B-D-MQUEUE-LIMITS-ACCESS-AND-ERROR-ORDER`.)
//!
//! `mqd_t` is still an index into this module's table, not a file
//! descriptor, so `poll` and `close` do not accept one (Linux's does).

use crate::errno;
use crate::perprocess::process_global;
use crate::sigevent::{SigeventView, notify};
use crate::stat::Timespec;
use core::sync::atomic::{AtomicI32, Ordering};

// ---------------------------------------------------------------------------
// Public types & constants
// ---------------------------------------------------------------------------

/// Message queue descriptor type.
pub type MqdT = i32;

/// Message queue attributes (POSIX `struct mq_attr`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MqAttr {
    /// Flags: only `O_NONBLOCK` means anything.
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

/// One more than the highest priority `mq_send` accepts (Linux's
/// `MQ_PRIO_MAX`).
pub const MQ_PRIO_MAX: u32 = 32_768;

/// The kernel's defaults and limits (`<linux/ipc_namespace.h>`).
const DFLT_MSG: usize = 10;
const DFLT_MSGSIZE: usize = 8192;
const DFLT_MSGMAX: usize = 10;
const DFLT_MSGSIZEMAX: usize = 8192;
const HARD_MSGMAX: usize = 65_536;
const HARD_MSGSIZEMAX: usize = 16 * 1024 * 1024;
const DFLT_QUEUESMAX: usize = 256;

/// The longest name component the kernel's lookup takes.
const NAME_MAX: usize = 255;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// One queued message.  Its bytes are in the queue's data block, at `slot`.
#[derive(Clone, Copy)]
struct Msg {
    prio: u32,
    len: usize,
    slot: usize,
}

/// A notification registered with `mq_notify`.
#[derive(Clone, Copy)]
struct Registration {
    sev: SigeventView,
    /// The caller's thread attributes, copied: the pointer it passed may not
    /// outlive the call (glibc copies them too).
    attr: Option<crate::pthread::PthreadAttrT>,
}

struct Queue {
    live: bool,
    unlinked: bool,
    name: [u8; NAME_MAX],
    name_len: usize,
    /// Descriptors open on the queue.
    refs: usize,
    maxmsg: usize,
    msgsize: usize,
    /// `maxmsg` entries; the first `cur` are the queue, highest priority
    /// first and in arrival order within a priority.
    msgs: *mut Msg,
    cur: usize,
    /// `maxmsg * msgsize` bytes of message data.
    data: *mut u8,
    /// A stack of the data slots not holding a message: `maxmsg - cur`
    /// entries are valid.
    free: *mut usize,
    /// Receivers blocked on this queue: a message that arrives while one
    /// waits is theirs, and fires no notification.
    recv_waiters: usize,
    notify: Option<Registration>,
}

impl Queue {
    const EMPTY: Self = Self {
        live: false,
        unlinked: false,
        name: [0; NAME_MAX],
        name_len: 0,
        refs: 0,
        maxmsg: 0,
        msgsize: 0,
        msgs: core::ptr::null_mut(),
        cur: 0,
        data: core::ptr::null_mut(),
        free: core::ptr::null_mut(),
        recv_waiters: 0,
        notify: None,
    };

    fn named(&self) -> &[u8] {
        self.name.get(..self.name_len).unwrap_or(&[])
    }
}

#[derive(Clone, Copy)]
struct Desc {
    live: bool,
    queue: usize,
    nonblock: bool,
    /// `oflag & O_ACCMODE`: 0 read, 1 write, 2 both, 3 neither.
    access: i32,
}

impl Desc {
    const EMPTY: Self = Self {
        live: false,
        queue: 0,
        nonblock: false,
        access: 0,
    };

    fn can_read(&self) -> bool {
        matches!(self.access, crate::fcntl::O_RDONLY | crate::fcntl::O_RDWR)
    }

    fn can_write(&self) -> bool {
        matches!(self.access, crate::fcntl::O_WRONLY | crate::fcntl::O_RDWR)
    }
}

/// Both tables, grown as needed.  Indices are stable; the arrays move.
struct Tables {
    queues: *mut Queue,
    queues_cap: usize,
    descs: *mut Desc,
    descs_cap: usize,
}

process_global! {
    /// This process's queues and descriptors.
    ///
    /// Per-thread on the host, for test isolation: a test that counts
    /// queues is broken by any concurrent one, and no lock fixes that.
    fn tables() -> Tables = Tables {
        queues: core::ptr::null_mut(),
        queues_cap: 0,
        descs: core::ptr::null_mut(),
        descs_cap: 0,
    };

    /// Serialises every use of the tables, with the same scope as they have
    /// (see [`crate::perprocess::PoolLock`]).
    fn mq_lock() -> crate::perprocess::PoolLock = crate::perprocess::PoolLock::new();

    /// Advanced by every change a blocked call could be waiting for; they
    /// sleep on it.
    fn changes() -> AtomicI32 = AtomicI32::new(0);

    /// Calls asleep on [`changes`], so a change nobody waits for costs no
    /// syscall.
    fn sleepers() -> AtomicI32 = AtomicI32::new(0);
}

/// Holds the tables' lock; the tables are reached through it.
struct Locked {
    _guard: crate::perprocess::PoolGuard<'static>,
    t: *mut Tables,
}

fn lock() -> Locked {
    Locked {
        // SAFETY: `mq_lock()` is this context's lock, valid as long as the
        // tables it guards.
        _guard: unsafe { crate::perprocess::lock_pool(mq_lock()) },
        t: tables(),
    }
}

impl Locked {
    fn queue(&mut self, i: usize) -> Option<&mut Queue> {
        // SAFETY: the lock is held; `queues` holds `queues_cap` entries.
        unsafe {
            let t = &mut *self.t;
            if i < t.queues_cap {
                Some(&mut *t.queues.add(i))
            } else {
                None
            }
        }
    }

    fn desc(&mut self, i: usize) -> Option<&mut Desc> {
        // SAFETY: as in `queue`.
        unsafe {
            let t = &mut *self.t;
            if i < t.descs_cap {
                Some(&mut *t.descs.add(i))
            } else {
                None
            }
        }
    }

    fn queues_cap(&self) -> usize {
        // SAFETY: the lock is held.
        unsafe { (*self.t).queues_cap }
    }

    fn descs_cap(&self) -> usize {
        // SAFETY: the lock is held.
        unsafe { (*self.t).descs_cap }
    }

    /// The live queue named `name`, not unlinked.
    fn find(&mut self, name: &[u8]) -> Option<usize> {
        (0..self.queues_cap()).find(|&i| {
            self.queue(i)
                .is_some_and(|q| q.live && !q.unlinked && q.named() == name)
        })
    }

    fn live_queues(&mut self) -> usize {
        (0..self.queues_cap())
            .filter(|&i| self.queue(i).is_some_and(|q| q.live))
            .count()
    }

    /// A free descriptor slot, growing the table if there is none.
    fn free_desc(&mut self) -> Option<usize> {
        if let Some(i) = (0..self.descs_cap()).find(|&i| self.desc(i).is_some_and(|d| !d.live)) {
            return Some(i);
        }
        let old = self.descs_cap();
        let cap = old.checked_mul(2)?.max(8);
        // SAFETY: the lock is held; `grow` keeps the old entries.
        unsafe {
            let t = &mut *self.t;
            t.descs = grow(t.descs, old, cap, || Desc::EMPTY)?;
            t.descs_cap = cap;
        }
        Some(old)
    }

    /// A free queue slot, growing the table if there is none.
    fn free_queue(&mut self) -> Option<usize> {
        if let Some(i) = (0..self.queues_cap()).find(|&i| self.queue(i).is_some_and(|q| !q.live)) {
            return Some(i);
        }
        let old = self.queues_cap();
        let cap = old.checked_mul(2)?.max(4);
        // SAFETY: as in `free_desc`.
        unsafe {
            let t = &mut *self.t;
            t.queues = grow(t.queues, old, cap, || Queue::EMPTY)?;
            t.queues_cap = cap;
        }
        Some(old)
    }

    /// Resolve `mqd` to its descriptor slot.
    fn resolve(&mut self, mqd: MqdT) -> Result<usize, i32> {
        let i = usize::try_from(mqd)
            .ok()
            .and_then(|m| m.checked_sub(1))
            .ok_or(errno::EBADF)?;
        match self.desc(i) {
            Some(d) if d.live => Ok(i),
            _ => Err(errno::EBADF),
        }
    }
}

/// `realloc` `old_len` entries at `p` to `new_len`, filling the new ones.
///
/// # Safety
///
/// `p` is null or a `malloc` block holding `old_len` valid `T`s.
unsafe fn grow<T>(
    p: *mut T,
    old_len: usize,
    new_len: usize,
    fill: impl Fn() -> T,
) -> Option<*mut T> {
    let bytes = new_len.checked_mul(size_of::<T>())?;
    // SAFETY: the caller's contract; realloc(NULL, n) is malloc.
    let q = unsafe { crate::malloc::realloc(p.cast::<u8>(), bytes) }.cast::<T>();
    if q.is_null() {
        return None;
    }
    for i in old_len..new_len {
        // SAFETY: `q` holds `new_len` entries.
        unsafe { q.add(i).write(fill()) };
    }
    Some(q)
}

/// Free a queue's storage and its slot.
fn release_queue(q: &mut Queue) {
    // SAFETY: each is null or a `malloc` block this queue owns.
    unsafe {
        crate::malloc::free(q.msgs.cast());
        crate::malloc::free(q.data);
        crate::malloc::free(q.free.cast());
    }
    *q = Queue::EMPTY;
}

/// Tell the calls asleep on the queues that something changed.
fn changed() {
    // SAFETY: this process's words.
    let (seq, sleeping) = unsafe { (&*changes(), &*sleepers()) };
    seq.fetch_add(1, Ordering::SeqCst);
    if sleeping.load(Ordering::SeqCst) > 0 {
        crate::lowlevellock::futex_wake_all(seq);
    }
}

/// Sleep until the queues change from `seen`, or until `deadline`
/// (`CLOCK_REALTIME`): `Err(ETIMEDOUT)` once it has passed.
fn wait_for_change(seen: i32, deadline: Option<&Timespec>) -> Result<(), i32> {
    // SAFETY: this process's words.
    let (seq, sleeping) = unsafe { (&*changes(), &*sleepers()) };
    let timeout = match deadline {
        None => None,
        Some(at) => {
            let now = crate::lowlevellock::now_on(crate::time::CLOCK_REALTIME);
            Some(crate::lowlevellock::ns_until(&now, at).ok_or(errno::ETIMEDOUT)?)
        }
    };
    sleeping.fetch_add(1, Ordering::SeqCst);
    match timeout {
        None => crate::lowlevellock::futex_wait(seq, seen),
        Some(ns) => crate::lowlevellock::futex_wait_timeout(seq, seen, ns),
    }
    sleeping.fetch_sub(1, Ordering::SeqCst);
    Ok(())
}

/// The counter a waiter compares against, read before it looks at a queue.
fn change_seen() -> i32 {
    // SAFETY: this process's word.
    unsafe { &*changes() }.load(Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// Names and attributes
// ---------------------------------------------------------------------------

/// glibc's `name[0] != '/'` test, then the kernel's `getname` and
/// `lookup_one_len` on what follows, in that order.  The component on
/// success.
///
/// # Safety
///
/// `name` is null or a NUL-terminated string.
unsafe fn parse_name(name: *const u8) -> Result<([u8; NAME_MAX], usize), i32> {
    if name.is_null() {
        // glibc reads `name[0]` and faults.
        return Err(errno::EFAULT);
    }
    // SAFETY: non-null and NUL-terminated, by the caller's contract.
    if unsafe { *name } != b'/' {
        return Err(errno::EINVAL);
    }
    // SAFETY: the first byte was not the NUL.
    let rest = unsafe { name.add(1) };
    let mut len = 0usize;
    loop {
        if len >= crate::unistd::PATH_MAX {
            return Err(errno::ENAMETOOLONG); // `getname`
        }
        // SAFETY: every byte before the NUL is readable.
        if unsafe { *rest.add(len) } == 0 {
            break;
        }
        len = len.wrapping_add(1);
    }
    if len == 0 {
        return Err(errno::ENOENT); // `getname` of ""
    }
    // SAFETY: `len` bytes precede the NUL.
    let comp = unsafe { core::slice::from_raw_parts(rest, len) };
    if comp == b"." || comp == b".." || comp.contains(&b'/') {
        return Err(errno::EACCES); // `lookup_one_len`
    }
    let mut out = [0u8; NAME_MAX];
    out.get_mut(..len)
        .ok_or(errno::ENAMETOOLONG)? // the lookup's `NAME_MAX`
        .copy_from_slice(comp);
    Ok((out, len))
}

/// The size a new queue gets: the kernel's defaults, or `attr` within its
/// limits (`mqueue_get_inode`).
fn queue_size(attr: Option<&MqAttr>) -> Result<(usize, usize), i32> {
    let Some(a) = attr else {
        return Ok((DFLT_MSG.min(DFLT_MSGMAX), DFLT_MSGSIZE.min(DFLT_MSGSIZEMAX)));
    };
    if a.mq_maxmsg <= 0 || a.mq_msgsize <= 0 {
        return Err(errno::EINVAL);
    }
    let (max, size) =
        if crate::sys_capability::has_capability(crate::sys_capability::CAP_SYS_RESOURCE) {
            (HARD_MSGMAX, HARD_MSGSIZEMAX)
        } else {
            (DFLT_MSGMAX, DFLT_MSGSIZEMAX)
        };
    let maxmsg = usize::try_from(a.mq_maxmsg).map_err(|_| errno::EINVAL)?;
    let msgsize = usize::try_from(a.mq_msgsize).map_err(|_| errno::EINVAL)?;
    if maxmsg > max || msgsize > size {
        return Err(errno::EINVAL);
    }
    if maxmsg.checked_mul(msgsize).is_none() {
        return Err(errno::EOVERFLOW);
    }
    Ok((maxmsg, msgsize))
}

/// Allocate a queue's storage into `q`.  `false` if the memory cannot be had.
fn fill_queue(
    q: &mut Queue,
    name: &[u8; NAME_MAX],
    name_len: usize,
    maxmsg: usize,
    msgsize: usize,
) -> bool {
    let data_bytes = maxmsg.saturating_mul(msgsize);
    let msgs = crate::malloc::malloc(maxmsg.saturating_mul(size_of::<Msg>())).cast::<Msg>();
    let free = crate::malloc::malloc(maxmsg.saturating_mul(size_of::<usize>())).cast::<usize>();
    let data = crate::malloc::malloc(data_bytes.max(1));
    if msgs.is_null() || free.is_null() || data.is_null() {
        // SAFETY: each is null or a block just allocated.
        unsafe {
            crate::malloc::free(msgs.cast());
            crate::malloc::free(free.cast());
            crate::malloc::free(data);
        }
        return false;
    }
    for i in 0..maxmsg {
        // SAFETY: `free` holds `maxmsg` entries.  Slots are handed out from
        // the end of the stack, so slot 0 goes first.
        unsafe { free.add(i).write(maxmsg - 1 - i) };
    }
    *q = Queue {
        live: true,
        unlinked: false,
        name: *name,
        name_len,
        refs: 0,
        maxmsg,
        msgsize,
        msgs,
        cur: 0,
        data,
        free,
        recv_waiters: 0,
        notify: None,
    };
    true
}

// ---------------------------------------------------------------------------
// mq_open / mq_close / mq_unlink
// ---------------------------------------------------------------------------

/// Open (and optionally create) a message queue.
///
/// In Linux's order (glibc's `__mq_open`, then `SYSCALL_DEFINE4(mq_open)`,
/// `do_mq_open`, `prepare_open`, `mqueue_create_attr`, `mqueue_get_inode`):
///
/// 1. the name ([`parse_name`]);
/// 2. a free descriptor (`EMFILE`, `get_unused_fd_flags`);
/// 3. an existing queue: `O_CREAT|O_EXCL` → `EEXIST`, then an access mode of
///    `O_WRONLY|O_RDWR` → `EINVAL` -- only here, as in `prepare_open`;
/// 4. a missing one: without `O_CREAT` → `ENOENT`; past 256 queues without
///    `CAP_SYS_RESOURCE` → `ENOSPC`; then the size ([`queue_size`]).  Created
///    with `O_WRONLY|O_RDWR`, the descriptor may neither send nor receive,
///    as Linux's.
///
/// `mode` is accepted and unused: queues here have no owner to check it
/// against.  `attr` is read only with `O_CREAT`, which is when glibc passes
/// it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_open(name: *const u8, oflag: i32, _mode: u32, attr: *const MqAttr) -> MqdT {
    // SAFETY: the caller's contract makes `name` a string or NULL.
    let (comp, comp_len) = match unsafe { parse_name(name) } {
        Ok(c) => c,
        Err(e) => {
            errno::set_errno(e);
            return -1;
        }
    };
    let create = oflag & crate::fcntl::O_CREAT != 0;
    let wanted = if create && !attr.is_null() {
        // SAFETY: non-null; the caller's contract.
        Some(unsafe { core::ptr::read_unaligned(attr) })
    } else {
        None
    };
    let accmode = oflag & crate::fcntl::O_ACCMODE;
    let nonblock = oflag & crate::fcntl::O_NONBLOCK != 0;

    let mut l = lock();
    let Some(d) = l.free_desc() else {
        errno::set_errno(errno::EMFILE);
        return -1;
    };
    let name_slice = comp.get(..comp_len).unwrap_or(&[]);
    let q = if let Some(q) = l.find(name_slice) {
        if oflag & (crate::fcntl::O_CREAT | crate::fcntl::O_EXCL)
            == (crate::fcntl::O_CREAT | crate::fcntl::O_EXCL)
        {
            errno::set_errno(errno::EEXIST);
            return -1;
        }
        if accmode == crate::fcntl::O_ACCMODE {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        q
    } else {
        if !create {
            errno::set_errno(errno::ENOENT);
            return -1;
        }
        if l.live_queues() >= DFLT_QUEUESMAX
            && !crate::sys_capability::has_capability(crate::sys_capability::CAP_SYS_RESOURCE)
        {
            errno::set_errno(errno::ENOSPC);
            return -1;
        }
        let (maxmsg, msgsize) = match queue_size(wanted.as_ref()) {
            Ok(s) => s,
            Err(e) => {
                errno::set_errno(e);
                return -1;
            }
        };
        let Some(q) = l.free_queue() else {
            errno::set_errno(errno::ENOMEM);
            return -1;
        };
        let filled = l
            .queue(q)
            .is_some_and(|slot| fill_queue(slot, &comp, comp_len, maxmsg, msgsize));
        if !filled {
            errno::set_errno(errno::ENOMEM);
            return -1;
        }
        q
    };
    if let Some(queue) = l.queue(q) {
        queue.refs = queue.refs.saturating_add(1);
    }
    if let Some(desc) = l.desc(d) {
        *desc = Desc {
            live: true,
            queue: q,
            nonblock,
            access: accmode,
        };
    }
    MqdT::try_from(d.wrapping_add(1)).unwrap_or(MqdT::MAX)
}

/// Close a message queue descriptor.
///
/// The queue is freed with its last descriptor once it has been unlinked,
/// and a notification this process registered on it is dropped, as Linux's
/// `mqueue_flush_file` drops it when the owner closes any descriptor of the
/// queue.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_close(mqdes: MqdT) -> i32 {
    let mut l = lock();
    let d = match l.resolve(mqdes) {
        Ok(d) => d,
        Err(e) => {
            errno::set_errno(e);
            return -1;
        }
    };
    let qi = l.desc(d).map_or(0, |desc| desc.queue);
    if let Some(desc) = l.desc(d) {
        *desc = Desc::EMPTY;
    }
    if let Some(q) = l.queue(qi) {
        q.notify = None;
        q.refs = q.refs.saturating_sub(1);
        if q.refs == 0 && q.unlinked {
            release_queue(q);
        }
    }
    drop(l);
    changed();
    0
}

/// Remove a queue's name.  Open descriptors keep working; the queue is freed
/// with the last of them.  The name is judged as `mq_open` judges it, and a
/// name with no queue is `ENOENT`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_unlink(name: *const u8) -> i32 {
    // SAFETY: the caller's contract makes `name` a string or NULL.
    let (comp, comp_len) = match unsafe { parse_name(name) } {
        Ok(c) => c,
        Err(e) => {
            errno::set_errno(e);
            return -1;
        }
    };
    let mut l = lock();
    let Some(qi) = l.find(comp.get(..comp_len).unwrap_or(&[])) else {
        errno::set_errno(errno::ENOENT);
        return -1;
    };
    if let Some(q) = l.queue(qi) {
        q.unlinked = true;
        if q.refs == 0 {
            release_queue(q);
        }
    }
    0
}

// ---------------------------------------------------------------------------
// Sending and receiving
// ---------------------------------------------------------------------------

/// A validated `CLOCK_REALTIME` deadline (`prepare_timeout`): `EINVAL` for a
/// negative `tv_sec` or a `tv_nsec` outside `0..1e9`.
fn read_deadline(abs_timeout: *const Timespec) -> Result<Option<Timespec>, i32> {
    if abs_timeout.is_null() {
        return Ok(None);
    }
    // SAFETY: non-null; the caller's contract.
    let ts = unsafe { core::ptr::read_unaligned(abs_timeout) };
    if ts.tv_sec < 0 || !crate::time::valid_nanoseconds(ts.tv_nsec) {
        return Err(errno::EINVAL);
    }
    Ok(Some(ts))
}

/// Insert a message into `q`, keeping it ordered.  The queue has room.
fn insert(q: &mut Queue, msg: *const u8, len: usize, prio: u32) {
    // SAFETY: `cur < maxmsg`, so the free stack holds at least one slot;
    // the data block holds `maxmsg * msgsize` bytes and `len <= msgsize`;
    // `msgs` holds `maxmsg` entries.
    unsafe {
        let free_top = q.maxmsg - q.cur - 1;
        let slot = q.free.add(free_top).read();
        if len > 0 {
            core::ptr::copy_nonoverlapping(msg, q.data.add(slot * q.msgsize), len);
        }
        // After every message of a higher or equal priority.
        let mut at = q.cur;
        while at > 0 && (*q.msgs.add(at - 1)).prio < prio {
            q.msgs.add(at).write(q.msgs.add(at - 1).read());
            at -= 1;
        }
        q.msgs.add(at).write(Msg { prio, len, slot });
        q.cur += 1;
    }
}

/// Take the first message of `q` (which has one) into `out`, which holds at
/// least `msgsize` bytes or is NULL: its length and priority.
fn take(q: &mut Queue, out: *mut u8) -> (usize, u32) {
    // SAFETY: `cur > 0`; indices as in `insert`.
    unsafe {
        let m = q.msgs.read();
        if !out.is_null() && m.len > 0 {
            core::ptr::copy_nonoverlapping(q.data.add(m.slot * q.msgsize), out, m.len);
        }
        for i in 1..q.cur {
            q.msgs.add(i - 1).write(q.msgs.add(i).read());
        }
        q.cur -= 1;
        q.free.add(q.maxmsg - q.cur - 1).write(m.slot);
        (m.len, m.prio)
    }
}

/// Fire a registration outside the lock.  A failure has nowhere to go: the
/// registering process is not the one sending.
fn fire(reg: &Registration) {
    let mut sev = reg.sev;
    let attr_copy = reg.attr;
    if let Some(a) = attr_copy.as_ref() {
        sev.sigev_notify_attributes = a;
    }
    let saved = errno::get_errno();
    // The sender learns nothing from a notification it could not make, as
    // in Linux, where it is the kernel's to deliver.
    let _ = notify(&sev);
    errno::set_errno(saved);
}

/// `mq_timedsend` without its timeout parsing.
fn send(mqdes: MqdT, msg: *const u8, len: usize, prio: u32, deadline: Option<&Timespec>) -> i32 {
    if prio >= MQ_PRIO_MAX {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    loop {
        let seen = change_seen();
        let mut l = lock();
        let d = match l.resolve(mqdes) {
            Ok(d) => d,
            Err(e) => {
                errno::set_errno(e);
                return -1;
            }
        };
        let Some(desc) = l.desc(d).copied() else {
            errno::set_errno(errno::EBADF);
            return -1;
        };
        if !desc.can_write() {
            errno::set_errno(errno::EBADF);
            return -1;
        }
        let Some(q) = l.queue(desc.queue) else {
            errno::set_errno(errno::EBADF);
            return -1;
        };
        if len > q.msgsize {
            errno::set_errno(errno::EMSGSIZE);
            return -1;
        }
        if msg.is_null() && len > 0 {
            errno::set_errno(errno::EFAULT); // `load_msg`
            return -1;
        }
        if q.cur < q.maxmsg {
            let was_empty = q.cur == 0;
            insert(q, msg, len, prio);
            // `__do_notify`: the queue became non-empty and no receiver is
            // waiting for the message; the registration is used up.
            let reg = if was_empty && q.recv_waiters == 0 {
                q.notify.take()
            } else {
                None
            };
            drop(l);
            changed();
            if let Some(reg) = reg {
                fire(&reg);
            }
            return 0;
        }
        if desc.nonblock {
            errno::set_errno(errno::EAGAIN);
            return -1;
        }
        drop(l);
        if let Err(e) = wait_for_change(seen, deadline) {
            errno::set_errno(e);
            return -1;
        }
    }
}

/// Send a message to a queue: [`mq_timedsend`] with no timeout.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_send(mqdes: MqdT, msg_ptr: *const u8, msg_len: usize, msg_prio: u32) -> i32 {
    send(mqdes, msg_ptr, msg_len, msg_prio, None)
}

/// Send a message to a queue, waiting for room until an absolute
/// `CLOCK_REALTIME` deadline.
///
/// Linux's order: the deadline (`EINVAL`; NULL is no deadline), the priority
/// (`EINVAL` from `MQ_PRIO_MAX`), the descriptor and its write access
/// (`EBADF`), the size (`EMSGSIZE`), the message itself (`EFAULT` for NULL
/// with a length), then a full queue: `EAGAIN` with `O_NONBLOCK`, else a
/// wait that ends in `ETIMEDOUT` at the deadline.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_timedsend(
    mqdes: MqdT,
    msg_ptr: *const u8,
    msg_len: usize,
    msg_prio: u32,
    abs_timeout: *const Timespec,
) -> i32 {
    match read_deadline(abs_timeout) {
        Ok(deadline) => send(mqdes, msg_ptr, msg_len, msg_prio, deadline.as_ref()),
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

/// `mq_timedreceive` without its timeout parsing.
fn receive(
    mqdes: MqdT,
    buf: *mut u8,
    len: usize,
    prio_out: *mut u32,
    deadline: Option<&Timespec>,
) -> isize {
    let mut waiting_on: Option<usize> = None;
    let result = loop {
        let seen = change_seen();
        let mut l = lock();
        let d = match l.resolve(mqdes) {
            Ok(d) => d,
            Err(e) => break Err((e, l)),
        };
        let Some(desc) = l.desc(d).copied() else {
            break Err((errno::EBADF, l));
        };
        if !desc.can_read() {
            break Err((errno::EBADF, l));
        }
        let Some(q) = l.queue(desc.queue) else {
            break Err((errno::EBADF, l));
        };
        if len < q.msgsize {
            break Err((errno::EMSGSIZE, l));
        }
        if q.cur > 0 {
            let (n, prio) = take(q, buf);
            break Ok((n, prio, l));
        }
        if desc.nonblock {
            break Err((errno::EAGAIN, l));
        }
        if waiting_on.is_none() {
            q.recv_waiters = q.recv_waiters.saturating_add(1);
            waiting_on = Some(desc.queue);
        }
        drop(l);
        if let Err(e) = wait_for_change(seen, deadline) {
            break Err((e, lock()));
        }
    };
    let (outcome, mut l) = match result {
        Ok((n, prio, l)) => (Ok((n, prio)), l),
        Err((e, l)) => (Err(e), l),
    };
    if let Some(qi) = waiting_on {
        if let Some(q) = l.queue(qi) {
            q.recv_waiters = q.recv_waiters.saturating_sub(1);
        }
    }
    drop(l);
    match outcome {
        Ok((n, prio)) => {
            changed();
            if !prio_out.is_null() {
                // SAFETY: non-null; the caller's contract.
                unsafe { *prio_out = prio };
            }
            if buf.is_null() && n > 0 {
                // `store_msg` faults after the message was taken, and
                // Linux's receive then fails with it: the message is gone.
                errno::set_errno(errno::EFAULT);
                return -1;
            }
            isize::try_from(n).unwrap_or(isize::MAX)
        }
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

/// Receive the highest-priority message: [`mq_timedreceive`] with no
/// timeout.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_receive(
    mqdes: MqdT,
    msg_ptr: *mut u8,
    msg_len: usize,
    msg_prio: *mut u32,
) -> isize {
    receive(mqdes, msg_ptr, msg_len, msg_prio, None)
}

/// Receive the highest-priority message, waiting for one until an absolute
/// `CLOCK_REALTIME` deadline; its length.
///
/// Linux's order: the deadline (`EINVAL`), the descriptor and its read access
/// (`EBADF`), a buffer shorter than the queue's message size (`EMSGSIZE`),
/// then an empty queue (`EAGAIN` with `O_NONBLOCK`, else a wait ending in
/// `ETIMEDOUT`), and only then the buffer: a NULL one takes the message and
/// fails with `EFAULT`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_timedreceive(
    mqdes: MqdT,
    msg_ptr: *mut u8,
    msg_len: usize,
    msg_prio: *mut u32,
    abs_timeout: *const Timespec,
) -> isize {
    match read_deadline(abs_timeout) {
        Ok(deadline) => receive(mqdes, msg_ptr, msg_len, msg_prio, deadline.as_ref()),
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// mq_getattr / mq_setattr
// ---------------------------------------------------------------------------

/// The attributes as `mq_getsetattr` reports them.
fn attr_of(q: &Queue, nonblock: bool) -> MqAttr {
    MqAttr {
        mq_flags: if nonblock {
            i64::from(crate::fcntl::O_NONBLOCK)
        } else {
            0
        },
        mq_maxmsg: i64::try_from(q.maxmsg).unwrap_or(i64::MAX),
        mq_msgsize: i64::try_from(q.msgsize).unwrap_or(i64::MAX),
        mq_curmsgs: i64::try_from(q.cur).unwrap_or(i64::MAX),
        _pad: [0; 4],
    }
}

/// Get the current attributes of a message queue.
///
/// glibc's `mq_getattr` is `mq_setattr (mqdes, NULL, mqstat)`
/// (sysdeps/unix/sysv/linux/mq_getattr.c), and `mq_getsetattr` copies out
/// only to a non-NULL pointer (ipc/mqueue.c): a NULL `attr` is no error --
/// the descriptor is still checked, and nothing is written.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_getattr(mqdes: MqdT, attr: *mut MqAttr) -> i32 {
    mq_setattr(mqdes, core::ptr::null(), attr)
}

/// Set the attributes of a message queue: only `O_NONBLOCK`, and only the
/// descriptor's.
///
/// Linux's `mq_getsetattr`: a new attribute setting any flag but
/// `O_NONBLOCK` is `EINVAL` before the descriptor is looked up; a NULL one
/// only reads; the old attributes are written if `oldattr` is non-NULL.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_setattr(mqdes: MqdT, newattr: *const MqAttr, oldattr: *mut MqAttr) -> i32 {
    // SAFETY: the caller's contract -- `newattr` is NULL or readable.
    let new_flags =
        (!newattr.is_null()).then(|| unsafe { core::ptr::read_unaligned(newattr) }.mq_flags);
    if new_flags.is_some_and(|flags| flags & !i64::from(crate::fcntl::O_NONBLOCK) != 0) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let mut l = lock();
    let d = match l.resolve(mqdes) {
        Ok(d) => d,
        Err(e) => {
            errno::set_errno(e);
            return -1;
        }
    };
    let Some(desc) = l.desc(d).copied() else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    let old = l.queue(desc.queue).map(|q| attr_of(q, desc.nonblock));
    if let (false, Some(old)) = (oldattr.is_null(), old) {
        // SAFETY: non-null; the caller's contract.
        unsafe { core::ptr::write_unaligned(oldattr, old) };
    }
    if let (Some(flags), Some(desc)) = (new_flags, l.desc(d)) {
        desc.nonblock = flags & i64::from(crate::fcntl::O_NONBLOCK) != 0;
    }
    0
}

// ---------------------------------------------------------------------------
// mq_notify
// ---------------------------------------------------------------------------

/// Ask to be notified when a message arrives in the empty queue (Linux's
/// `do_mq_notify`).
///
/// 1. A `sevp` whose `sigev_notify` is not `SIGEV_NONE`, `SIGEV_SIGNAL` or
///    `SIGEV_THREAD`, or whose signal is not a signal (`> 64`), is `EINVAL`
///    -- before the descriptor is looked at.
/// 2. The descriptor → `EBADF`.
/// 3. NULL `sevp` drops this process's registration, if any.
/// 4. A queue already registered is `EBUSY`.
///
/// The notification fires once, when a message arrives in the empty queue
/// with no receiver waiting, and is then dropped.  `SIGEV_THREAD`'s
/// attributes are copied here, as glibc copies them, since the caller's may
/// not outlive the call.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_notify(mqdes: MqdT, sevp: *const crate::time::Sigevent) -> i32 {
    let reg = if sevp.is_null() {
        None
    } else {
        let sev = SigeventView::read(sevp.cast::<u8>());
        let valid = match sev.sigev_notify {
            crate::time::SIGEV_NONE | crate::time::SIGEV_THREAD => true,
            crate::time::SIGEV_SIGNAL => (0..=64).contains(&sev.sigev_signo),
            _ => false,
        };
        if !valid {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        let attr = (sev.sigev_notify == crate::time::SIGEV_THREAD
            && !sev.sigev_notify_attributes.is_null())
        .then(|| {
            // SAFETY: non-null; the caller's contract makes it an attribute
            // object.
            unsafe { core::ptr::read_unaligned(sev.sigev_notify_attributes) }
        });
        Some(Registration { sev, attr })
    };
    let mut l = lock();
    let d = match l.resolve(mqdes) {
        Ok(d) => d,
        Err(e) => {
            errno::set_errno(e);
            return -1;
        }
    };
    let qi = l.desc(d).map_or(0, |desc| desc.queue);
    let Some(q) = l.queue(qi) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    match reg {
        None => q.notify = None,
        Some(_) if q.notify.is_some() => {
            errno::set_errno(errno::EBUSY);
            return -1;
        }
        Some(r) => q.notify = Some(r),
    }
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcntl::{O_CREAT, O_EXCL, O_NONBLOCK, O_RDONLY, O_RDWR, O_WRONLY};

    fn attr(maxmsg: i64, msgsize: i64) -> MqAttr {
        MqAttr {
            mq_flags: 0,
            mq_maxmsg: maxmsg,
            mq_msgsize: msgsize,
            mq_curmsgs: 0,
            _pad: [0; 4],
        }
    }

    fn open(name: &[u8], oflag: i32, a: Option<&MqAttr>) -> MqdT {
        mq_open(
            name.as_ptr(),
            oflag,
            0o600,
            a.map_or(core::ptr::null(), core::ptr::from_ref),
        )
    }

    fn errno_of(ret: i64) -> i32 {
        assert_eq!(ret, -1);
        errno::get_errno()
    }

    fn getattr(q: MqdT) -> MqAttr {
        let mut a = attr(0, 0);
        assert_eq!(mq_getattr(q, &raw mut a), 0);
        a
    }

    // -- layout --

    #[test]
    fn test_mq_attr_layout() {
        assert_eq!(size_of::<MqAttr>(), 64);
        assert_eq!(core::mem::align_of::<MqAttr>(), 8);
        assert_eq!(size_of::<MqdT>(), 4);
    }

    // -- names --

    #[test]
    fn test_name_errors_in_linuxs_order() {
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_open(core::ptr::null(), O_RDWR | O_CREAT, 0, core::ptr::null()).into()),
            errno::EFAULT
        );
        for (name, e) in [
            (&b"noslash\0"[..], errno::EINVAL),
            (b"\0", errno::EINVAL),
            (b"/\0", errno::ENOENT),
            (b"/a/b\0", errno::EACCES),
            (b"/.\0", errno::EACCES),
            (b"/..\0", errno::EACCES),
        ] {
            errno::set_errno(0);
            assert_eq!(
                errno_of(open(name, O_RDWR | O_CREAT, None).into()),
                e,
                "{name:?}"
            );
            errno::set_errno(0);
            assert_eq!(
                errno_of(mq_unlink(name.as_ptr()).into()),
                e,
                "unlink {name:?}"
            );
        }
    }

    /// A component of up to `NAME_MAX` bytes is a name; one more is
    /// `ENAMETOOLONG` -- where 63 bytes used to be the limit.
    #[test]
    fn test_name_length() {
        let mut name = vec![b'/'];
        name.extend(core::iter::repeat_n(b'n', NAME_MAX));
        name.push(0);
        let q = open(&name, O_RDWR | O_CREAT, None);
        assert!(q > 0);
        assert_eq!(mq_close(q), 0);
        assert_eq!(mq_unlink(name.as_ptr()), 0);
        let mut long = vec![b'/'];
        long.extend(core::iter::repeat_n(b'n', NAME_MAX + 1));
        long.push(0);
        errno::set_errno(0);
        assert_eq!(
            errno_of(open(&long, O_RDWR | O_CREAT, None).into()),
            errno::ENAMETOOLONG
        );
    }

    // -- open --

    #[test]
    fn test_open_create_exist_and_missing() {
        errno::set_errno(0);
        assert_eq!(
            errno_of(open(b"/missing\0", O_RDWR, None).into()),
            errno::ENOENT
        );
        let q = open(b"/made\0", O_RDWR | O_CREAT, None);
        assert!(q > 0);
        errno::set_errno(0);
        assert_eq!(
            errno_of(open(b"/made\0", O_RDWR | O_CREAT | O_EXCL, None).into()),
            errno::EEXIST
        );
        let again = open(b"/made\0", O_RDONLY, None);
        assert!(again > 0 && again != q);
        assert_eq!(mq_close(again), 0);
        assert_eq!(mq_close(q), 0);
        assert_eq!(mq_unlink(b"/made\0".as_ptr()), 0);
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_unlink(b"/made\0".as_ptr()).into()),
            errno::ENOENT
        );
    }

    /// Linux's defaults: 10 messages of 8192 bytes.  They were 10 of 64.
    #[test]
    fn test_default_size_is_linuxs() {
        let q = open(b"/dflt\0", O_RDWR | O_CREAT, None);
        let a = getattr(q);
        assert_eq!((a.mq_maxmsg, a.mq_msgsize, a.mq_curmsgs), (10, 8192, 0));
        let big = [7u8; 8192];
        assert_eq!(mq_send(q, big.as_ptr(), big.len(), 0), 0);
        assert_eq!(mq_close(q), 0);
        assert_eq!(mq_unlink(b"/dflt\0".as_ptr()), 0);
    }

    #[test]
    fn test_size_limits() {
        for (m, s, e) in [
            (0, 8, errno::EINVAL),
            (8, 0, errno::EINVAL),
            (-1, 8, errno::EINVAL),
        ] {
            errno::set_errno(0);
            assert_eq!(
                errno_of(open(b"/sz\0", O_RDWR | O_CREAT, Some(&attr(m, s))).into()),
                e
            );
        }
        // Without CAP_SYS_RESOURCE: 10 and 8192.  A host test thread holds
        // every capability, so the hard limits apply here.
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_SYS_RESOURCE
        ));
        let q = open(b"/sz\0", O_RDWR | O_CREAT, Some(&attr(64, 16384)));
        assert!(q > 0);
        let a = getattr(q);
        assert_eq!((a.mq_maxmsg, a.mq_msgsize), (64, 16384));
        assert_eq!(mq_close(q), 0);
        assert_eq!(mq_unlink(b"/sz\0".as_ptr()), 0);
        errno::set_errno(0);
        assert_eq!(
            errno_of(open(b"/sz\0", O_RDWR | O_CREAT, Some(&attr(65_537, 8))).into()),
            errno::EINVAL
        );
    }

    /// `O_WRONLY|O_RDWR` is refused only for a queue that exists, after
    /// `EEXIST`; a new queue is created and its descriptor can do neither.
    #[test]
    fn test_bad_access_mode_as_linux_judges_it() {
        let both = O_WRONLY | O_RDWR;
        errno::set_errno(0);
        assert_eq!(
            errno_of(open(b"/acc\0", both, None).into()),
            errno::ENOENT,
            "missing first"
        );
        let q = open(b"/acc\0", both | O_CREAT, None);
        assert!(q > 0, "created");
        let msg = [1u8; 4];
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_send(q, msg.as_ptr(), 4, 0).into()),
            errno::EBADF
        );
        errno::set_errno(0);
        assert_eq!(
            errno_of(open(b"/acc\0", both | O_CREAT | O_EXCL, None).into()),
            errno::EEXIST
        );
        errno::set_errno(0);
        assert_eq!(errno_of(open(b"/acc\0", both, None).into()), errno::EINVAL);
        assert_eq!(mq_close(q), 0);
        assert_eq!(mq_unlink(b"/acc\0".as_ptr()), 0);
    }

    /// The queue limit is 256 and the tables grow to it; with the
    /// capability the host holds, past it too.
    #[test]
    fn test_many_queues() {
        let mut qs = Vec::new();
        for i in 0..40 {
            let name = format!("/many{i}\0");
            let q = open(name.as_bytes(), O_RDWR | O_CREAT, Some(&attr(1, 8)));
            assert!(q > 0, "queue {i}");
            qs.push((q, name));
        }
        for (q, name) in qs {
            assert_eq!(mq_close(q), 0);
            assert_eq!(mq_unlink(name.as_ptr()), 0);
        }
    }

    // -- close / unlink --

    #[test]
    fn test_close_bad_descriptors() {
        for q in [-1, 0, 1, 9999] {
            errno::set_errno(0);
            assert_eq!(errno_of(mq_close(q).into()), errno::EBADF);
        }
        let q = open(b"/twice\0", O_RDWR | O_CREAT, None);
        assert_eq!(mq_close(q), 0);
        errno::set_errno(0);
        assert_eq!(errno_of(mq_close(q).into()), errno::EBADF);
        assert_eq!(mq_unlink(b"/twice\0".as_ptr()), 0);
    }

    /// An unlinked queue lives on for its descriptors, and a new queue of
    /// the same name is a different one.
    #[test]
    fn test_unlink_while_open() {
        let q = open(b"/ul\0", O_RDWR | O_CREAT, None);
        let msg = *b"kept";
        assert_eq!(mq_send(q, msg.as_ptr(), 4, 0), 0);
        assert_eq!(mq_unlink(b"/ul\0".as_ptr()), 0);
        let fresh = open(b"/ul\0", O_RDWR | O_CREAT | O_EXCL, None);
        assert!(fresh > 0);
        assert_eq!(getattr(fresh).mq_curmsgs, 0);
        let mut buf = [0u8; 8192];
        assert_eq!(
            mq_receive(q, buf.as_mut_ptr(), buf.len(), core::ptr::null_mut()),
            4
        );
        assert_eq!(&buf[..4], b"kept");
        assert_eq!(mq_close(q), 0);
        assert_eq!(mq_close(fresh), 0);
        assert_eq!(mq_unlink(b"/ul\0".as_ptr()), 0);
    }

    // -- send / receive --

    #[test]
    fn test_priority_then_fifo_order() {
        let q = open(b"/order\0", O_RDWR | O_CREAT, Some(&attr(8, 8)));
        for (m, p) in [(b"a", 1u32), (b"b", 5), (b"c", 1), (b"d", 5), (b"e", 0)] {
            assert_eq!(mq_send(q, m.as_ptr(), 1, p), 0);
        }
        let mut got = Vec::new();
        let mut prio = 0u32;
        let mut buf = [0u8; 8];
        for _ in 0..5 {
            assert_eq!(mq_receive(q, buf.as_mut_ptr(), 8, &raw mut prio), 1);
            got.push((buf[0], prio));
        }
        assert_eq!(got, [(b'b', 5), (b'd', 5), (b'a', 1), (b'c', 1), (b'e', 0)]);
        assert_eq!(mq_close(q), 0);
        assert_eq!(mq_unlink(b"/order\0".as_ptr()), 0);
    }

    /// Slots are reused as messages come and go.
    #[test]
    fn test_slots_recycle() {
        let q = open(
            b"/cycle\0",
            O_RDWR | O_CREAT | O_NONBLOCK,
            Some(&attr(2, 4)),
        );
        let mut buf = [0u8; 4];
        for round in 0u8..20 {
            assert_eq!(mq_send(q, [round; 4].as_ptr(), 4, u32::from(round % 3)), 0);
            assert_eq!(mq_send(q, [round.wrapping_add(100); 4].as_ptr(), 4, 0), 0);
            assert_eq!(
                errno_of(mq_send(q, buf.as_ptr(), 4, 0).into()),
                errno::EAGAIN
            );
            assert_eq!(mq_receive(q, buf.as_mut_ptr(), 4, core::ptr::null_mut()), 4);
            assert_eq!(mq_receive(q, buf.as_mut_ptr(), 4, core::ptr::null_mut()), 4);
        }
        assert_eq!(mq_close(q), 0);
        assert_eq!(mq_unlink(b"/cycle\0".as_ptr()), 0);
    }

    /// The send errors, in Linux's order: priority before descriptor,
    /// descriptor and access before size, size before the message pointer.
    #[test]
    fn test_send_error_order() {
        let q = open(b"/sord\0", O_RDWR | O_CREAT, Some(&attr(2, 8)));
        let r = open(b"/sord\0", O_RDONLY, None);
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_send(-5, core::ptr::null(), 99, MQ_PRIO_MAX).into()),
            errno::EINVAL
        );
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_send(-5, core::ptr::null(), 99, 0).into()),
            errno::EBADF
        );
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_send(r, core::ptr::null(), 99, 0).into()),
            errno::EBADF,
            "read-only"
        );
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_send(q, core::ptr::null(), 99, 0).into()),
            errno::EMSGSIZE
        );
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_send(q, core::ptr::null(), 4, 0).into()),
            errno::EFAULT
        );
        assert_eq!(
            mq_send(q, core::ptr::null(), 0, 0),
            0,
            "an empty message needs no bytes"
        );
        assert_eq!(mq_close(r), 0);
        assert_eq!(mq_close(q), 0);
        assert_eq!(mq_unlink(b"/sord\0".as_ptr()), 0);
    }

    /// The receive errors: descriptor and access, then the size, then an
    /// empty queue -- and a NULL buffer only after a message was taken.
    #[test]
    fn test_receive_error_order() {
        let q = open(b"/rord\0", O_RDWR | O_CREAT | O_NONBLOCK, Some(&attr(2, 8)));
        let w = open(b"/rord\0", O_WRONLY, None);
        let mut buf = [0u8; 8];
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_receive(w, buf.as_mut_ptr(), 8, core::ptr::null_mut()) as i64),
            errno::EBADF
        );
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_receive(q, core::ptr::null_mut(), 4, core::ptr::null_mut()) as i64),
            errno::EMSGSIZE
        );
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_receive(q, core::ptr::null_mut(), 8, core::ptr::null_mut()) as i64),
            errno::EAGAIN,
            "an empty queue before the NULL buffer"
        );
        assert_eq!(mq_send(q, b"lost".as_ptr(), 4, 3), 0);
        let mut prio = 0u32;
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_receive(q, core::ptr::null_mut(), 8, &raw mut prio) as i64),
            errno::EFAULT
        );
        assert_eq!(
            prio, 3,
            "the priority is stored before the bytes, as Linux's put_user"
        );
        assert_eq!(getattr(q).mq_curmsgs, 0, "and the message is gone");
        assert_eq!(mq_close(w), 0);
        assert_eq!(mq_close(q), 0);
        assert_eq!(mq_unlink(b"/rord\0".as_ptr()), 0);
    }

    #[test]
    fn test_timed_calls() {
        let q = open(b"/timed\0", O_RDWR | O_CREAT, Some(&attr(1, 8)));
        let bad = Timespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000,
        };
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_timedsend(-1, core::ptr::null(), 0, MQ_PRIO_MAX, &raw const bad).into()),
            errno::EINVAL,
            "the deadline first"
        );
        let neg = Timespec {
            tv_sec: -1,
            tv_nsec: 0,
        };
        let mut buf = [0u8; 8];
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_timedreceive(
                -1,
                buf.as_mut_ptr(),
                8,
                core::ptr::null_mut(),
                &raw const neg
            ) as i64),
            errno::EINVAL
        );
        // A deadline already past: an empty queue times out at once.
        let past = Timespec {
            tv_sec: 1,
            tv_nsec: 0,
        };
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_timedreceive(
                q,
                buf.as_mut_ptr(),
                8,
                core::ptr::null_mut(),
                &raw const past
            ) as i64),
            errno::ETIMEDOUT
        );
        // And a full one, for a send.
        assert_eq!(mq_timedsend(q, b"x".as_ptr(), 1, 0, core::ptr::null()), 0);
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_timedsend(q, b"y".as_ptr(), 1, 0, &raw const past).into()),
            errno::ETIMEDOUT
        );
        // A deadline 20 ms off is waited for.
        let soon = {
            let now = crate::lowlevellock::now_on(crate::time::CLOCK_REALTIME);
            let ns = now.tv_nsec + 20_000_000;
            Timespec {
                tv_sec: now.tv_sec + ns / 1_000_000_000,
                tv_nsec: ns % 1_000_000_000,
            }
        };
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_timedsend(q, b"z".as_ptr(), 1, 0, &raw const soon).into()),
            errno::ETIMEDOUT
        );
        assert_eq!(mq_close(q), 0);
        assert_eq!(mq_unlink(b"/timed\0".as_ptr()), 0);
    }

    // -- attributes --

    #[test]
    fn test_getattr_and_setattr() {
        let q = open(b"/attr\0", O_RDWR | O_CREAT, Some(&attr(4, 16)));
        assert_eq!(
            mq_getattr(q, core::ptr::null_mut()),
            0,
            "NULL attr only checks the descriptor"
        );
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_getattr(-1, core::ptr::null_mut()).into()),
            errno::EBADF
        );
        let nb = MqAttr {
            mq_flags: i64::from(O_NONBLOCK),
            ..attr(99, 99)
        };
        let mut old = attr(0, 0);
        assert_eq!(mq_setattr(q, &raw const nb, &raw mut old), 0);
        assert_eq!(old.mq_flags, 0);
        let now = getattr(q);
        assert_eq!(
            (now.mq_flags, now.mq_maxmsg, now.mq_msgsize),
            (i64::from(O_NONBLOCK), 4, 16)
        );
        let other = MqAttr {
            mq_flags: 0x100,
            ..attr(0, 0)
        };
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_setattr(-1, &raw const other, core::ptr::null_mut()).into()),
            errno::EINVAL,
            "flags before the descriptor"
        );
        assert_eq!(mq_close(q), 0);
        assert_eq!(mq_unlink(b"/attr\0".as_ptr()), 0);
    }

    // -- mq_notify --

    fn sigevent(notify: i32, signo: i32) -> crate::time::Sigevent {
        let view = SigeventView {
            sigev_notify: notify,
            sigev_signo: signo,
            ..SigeventView::NONE
        };
        // SAFETY: both are 64 bytes of plain data.
        unsafe { core::mem::transmute::<SigeventView, crate::time::Sigevent>(view) }
    }

    #[test]
    fn test_notify_validation_order() {
        let bad = sigevent(crate::time::SIGEV_THREAD_ID, 0);
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_notify(-1, &raw const bad).into()),
            errno::EINVAL,
            "the sigevent first"
        );
        let bad_sig = sigevent(crate::time::SIGEV_SIGNAL, 65);
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_notify(-1, &raw const bad_sig).into()),
            errno::EINVAL
        );
        let none = sigevent(crate::time::SIGEV_NONE, 0);
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_notify(-1, &raw const none).into()),
            errno::EBADF
        );
        errno::set_errno(0);
        assert_eq!(
            errno_of(mq_notify(-1, core::ptr::null()).into()),
            errno::EBADF
        );
    }

    /// One registration per queue; NULL drops it; a message into the empty
    /// queue uses it up; closing a descriptor drops it too.
    #[test]
    fn test_notify_registration() {
        let q = open(b"/note\0", O_RDWR | O_CREAT | O_NONBLOCK, Some(&attr(4, 8)));
        let none = sigevent(crate::time::SIGEV_NONE, 0);
        assert_eq!(mq_notify(q, &raw const none), 0);
        errno::set_errno(0);
        assert_eq!(errno_of(mq_notify(q, &raw const none).into()), errno::EBUSY);
        assert_eq!(mq_notify(q, core::ptr::null()), 0, "dropped");
        assert_eq!(mq_notify(q, &raw const none), 0);
        assert_eq!(mq_send(q, b"m".as_ptr(), 1, 0), 0);
        assert_eq!(mq_notify(q, &raw const none), 0, "the arrival used it up");
        // A second message into a non-empty queue does not fire.
        assert_eq!(mq_send(q, b"n".as_ptr(), 1, 0), 0);
        errno::set_errno(0);
        assert_eq!(errno_of(mq_notify(q, &raw const none).into()), errno::EBUSY);
        let other = open(b"/note\0", O_RDONLY, None);
        assert_eq!(mq_close(other), 0);
        assert_eq!(mq_notify(q, &raw const none), 0, "a close dropped it");
        assert_eq!(mq_close(q), 0);
        assert_eq!(mq_unlink(b"/note\0".as_ptr()), 0);
    }

    /// SIGEV_SIGNAL fires by `raise`: signal 0 is a probe, so the arrival
    /// uses the registration up without a signal.
    #[test]
    fn test_notify_signal_fires_once() {
        let q = open(b"/sig\0", O_RDWR | O_CREAT | O_NONBLOCK, Some(&attr(4, 8)));
        let probe = sigevent(crate::time::SIGEV_SIGNAL, 0);
        assert_eq!(mq_notify(q, &raw const probe), 0);
        errno::set_errno(9);
        assert_eq!(mq_send(q, b"m".as_ptr(), 1, 0), 0);
        assert_eq!(errno::get_errno(), 9, "the sender's errno is its own");
        assert_eq!(mq_notify(q, &raw const probe), 0);
        assert_eq!(mq_close(q), 0);
        assert_eq!(mq_unlink(b"/sig\0".as_ptr()), 0);
    }
}
