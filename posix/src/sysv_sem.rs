// Semaphore values are kept within 0..=SEMVMX and undo adjustments within
// -SEMAEM-1..=SEMAEM, each sum checked against its bound before it is kept;
// counts and indices are bounded by a set's `nsems` (at most SEMMSL) and the
// total by SEMMNS.  Clippy cannot see the bounds.
#![allow(clippy::arithmetic_side_effects)]
//! System V semaphores (`<sys/sem.h>`): Linux 6.6's semantics (ipc/sem.c)
//! behind glibc 2.39's `semctl` -- inside one process.
//!
//! Sets are kept in this process: a key names the same set for every thread
//! of the program, but another program using the key gets a set of its own.
//! Sharing them between programs is open question D-Q3 (`open-questions.md`);
//! everything else is Linux's:
//!
//! - **Limits** are Linux's defaults: up to [`SEMMSL`] (32000) semaphores in
//!   a set, [`SEMMNI`] (32000) sets, [`SEMOPM`] (500) operations in one
//!   call, values up to [`SEMVMX`] (32767).
//! - **`semop`** applies its operations in order, all or none: the first one
//!   that cannot proceed decides -- `EAGAIN` if *it* carries `IPC_NOWAIT`,
//!   a sleep otherwise -- and a value past `SEMVMX` is `ERANGE`.
//!   `semtimedop`'s timeout is how long to wait, measured from the call, as
//!   Linux's is; running out is `EAGAIN`. A set removed under a blocked call
//!   makes it fail with `EIDRM`.
//! - **`semctl`** takes the fourth argument C passes -- `union semun` -- for
//!   `SETVAL`, `GETALL`, `SETALL`, `IPC_STAT`, `IPC_SET` and the info
//!   commands; `GETPID`, `GETNCNT` and `GETZCNT` report the last process to
//!   change a semaphore and how many calls wait on it, as Linux counts them
//!   (each waiter once, on the semaphore its blocking operation names).
//! - **Permissions** and ids are [`crate::sysv_ipc`]'s, as for the message
//!   queues.
//! - **`SEM_UNDO`** keeps Linux's adjustment, and its range (`ERANGE`); the
//!   adjustment is never applied, since a set cannot outlive the one process
//!   that could undo it.
//!
//! ## What changed on 2026-09-26
//!
//! The sets were a static pool: 16 sets of up to 32 semaphores. `semctl`
//! took three arguments, so a C caller's fourth never arrived: `semctl(id,
//! 0, SETVAL, 1)` -- the way every program sets a semaphore up -- failed, and
//! `IPC_STAT`, `IPC_SET`, `GETALL` and `SETALL` could not be reached at all.
//! `semtimedop` read its timeout as a `CLOCK_REALTIME` deadline, so any
//! timeout short of the date had already passed. One `IPC_NOWAIT` in a batch
//! made every operation non-blocking; `GETPID`, `GETNCNT` and `GETZCNT` were
//! always 0; a blocked call spun without yielding; permissions were not
//! checked; and the argument checks came in their own order -- a NULL
//! operation array was `EFAULT` before an empty or oversized count.
//! (`known-issues.md` -> `B-D-SYSV-SEM-SEMCTL-TIMEOUT-AND-LIMITS`.)

use crate::errno;
use crate::linux_ipc::{IPC_INFO, IpcPerm};
use crate::lowlevellock::Waited;
use crate::objtable::{Slots, Waits};
use crate::perprocess::process_global;
use crate::stat::Timespec;
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

/// Private key.
pub const IPC_PRIVATE: i32 = 0;

/// Get the PID of the last process to change a semaphore.
pub const GETPID: i32 = 11;
/// Get value of semaphore.
pub const GETVAL: i32 = 12;
/// Get all semaphore values.
pub const GETALL: i32 = 13;
/// Get the number of calls waiting for a semaphore to increase.
pub const GETNCNT: i32 = 14;
/// Get the number of calls waiting for a semaphore to reach zero.
pub const GETZCNT: i32 = 15;
/// Set value of semaphore.
pub const SETVAL: i32 = 16;
/// Set all semaphore values.
pub const SETALL: i32 = 17;
/// `semctl`: the set at a table index, not an id -- what `ipcs -s` walks.
pub const SEM_STAT: i32 = 18;
/// `semctl`: the limits, and how much of them is in use.
pub const SEM_INFO: i32 = 19;
/// `semctl`: `SEM_STAT` without its read-permission check.
pub const SEM_STAT_ANY: i32 = 20;

/// Undo flag — semaphore operations are undone on process exit.
pub const SEM_UNDO: i32 = 0x1000;

/// Maximum value a single semaphore can hold (Linux's `SEMVMX`).
pub const SEMVMX: i16 = 32_767;
/// The most semaphores in a set (`kernel.sem`'s first figure).
pub const SEMMSL: usize = 32_000;
/// The most sets a process may hold (`kernel.sem`'s fourth).
pub const SEMMNI: usize = 32_000;
/// The most operations in one `semop` (`kernel.sem`'s third).
pub const SEMOPM: usize = 500;
/// The most semaphores in all sets (`kernel.sem`'s second).
pub const SEMMNS: usize = SEMMNI * SEMMSL;
/// The largest undo adjustment.
const SEMAEM: i32 = SEMVMX as i32;
/// `struct seminfo`'s fixed figures (include/uapi/linux/sem.h).
const SEMUSZ: i32 = 20;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// `struct sembuf` — semaphore operation.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sembuf {
    /// Semaphore number (index in the set).
    pub sem_num: u16,
    /// Semaphore operation: positive (release), negative (acquire), or zero (wait).
    pub sem_op: i16,
    /// Operation flags (e.g., `IPC_NOWAIT`, `SEM_UNDO`).
    pub sem_flg: i16,
}

/// `struct semid_ds`, musl's x86-64 layout: `IPC_STAT` fills it, `IPC_SET`
/// reads its `sem_perm`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SemidDs {
    /// Ownership and permissions.
    pub sem_perm: IpcPerm,
    /// Time of the last `semop`.
    pub sem_otime: i64,
    __unused1: i64,
    /// Time of the last change: creation, `IPC_SET`, `SETVAL`, `SETALL`.
    pub sem_ctime: i64,
    __unused2: i64,
    /// Semaphores in the set.
    pub sem_nsems: u16,
    __sem_nsems_pad: [u8; 6],
    __unused3: i64,
    __unused4: i64,
}

/// `struct seminfo`: what `IPC_INFO` and `SEM_INFO` write.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Seminfo {
    /// Entries in the semaphore map (`SEMMAP`).
    pub semmap: i32,
    /// The most sets ([`SEMMNI`]).
    pub semmni: i32,
    /// The most semaphores in all sets ([`SEMMNS`]).
    pub semmns: i32,
    /// Undo structures (`SEMMNU`).
    pub semmnu: i32,
    /// The most semaphores in a set ([`SEMMSL`]).
    pub semmsl: i32,
    /// The most operations in a call ([`SEMOPM`]).
    pub semopm: i32,
    /// Undo entries per process (`SEMUME`).
    pub semume: i32,
    /// `IPC_INFO`: the undo structure size. `SEM_INFO`: sets in use.
    pub semusz: i32,
    /// The largest value ([`SEMVMX`]).
    pub semvmx: i32,
    /// `IPC_INFO`: the largest undo adjustment. `SEM_INFO`: semaphores in
    /// use.
    pub semaem: i32,
}

/// `union semun`: `semctl`'s fourth argument, one register wide. C programs
/// define it themselves (glibc and musl leave it undefined), and many pass a
/// bare `int` or pointer instead -- the same register either way.
#[repr(C)]
#[derive(Clone, Copy)]
pub union Semun {
    /// `SETVAL`'s value.
    pub val: i32,
    /// `IPC_STAT`'s and `IPC_SET`'s buffer.
    pub buf: *mut SemidDs,
    /// `GETALL`'s and `SETALL`'s values.
    pub array: *mut u16,
    /// `IPC_INFO`'s and `SEM_INFO`'s buffer.
    pub __buf: *mut Seminfo,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// One semaphore.
#[derive(Clone, Copy)]
struct Sem {
    val: i32,
    /// The last process to change it (`GETPID`).
    pid: i32,
    /// Calls waiting for it to increase (`GETNCNT`) or reach zero (`GETZCNT`).
    ncnt: u32,
    zcnt: u32,
    /// The `SEM_UNDO` adjustment.
    adj: i32,
}

impl Sem {
    const ZERO: Self = Self {
        val: 0,
        pid: 0,
        ncnt: 0,
        zcnt: 0,
        adj: 0,
    };
}

struct Set {
    live: bool,
    perm: Perm,
    nsems: usize,
    /// `nsems` semaphores, a `malloc` block this set owns.
    sems: *mut Sem,
    otime: i64,
    ctime: i64,
}

impl Set {
    const EMPTY: Self = Self {
        live: false,
        perm: Perm::EMPTY,
        nsems: 0,
        sems: core::ptr::null_mut(),
        otime: 0,
        ctime: 0,
    };

    fn sems(&mut self) -> &mut [Sem] {
        if self.sems.is_null() {
            return &mut [];
        }
        // SAFETY: `sems` holds `nsems` semaphores, owned by this set.
        unsafe { core::slice::from_raw_parts_mut(self.sems, self.nsems) }
    }
}

struct Tables {
    slots: Slots<Set>,
    /// Semaphores in all sets (`SEMMNS`, `SEM_INFO`).
    used_sems: usize,
}

process_global! {
    /// This process's semaphore sets.
    ///
    /// Per-thread on the host, for test isolation.
    fn tables() -> Tables = Tables { slots: Slots::EMPTY, used_sems: 0 };

    /// Serialises every use of the tables, with the same scope as they have
    /// (see [`crate::perprocess::PoolLock`]).
    fn sem_lock() -> crate::perprocess::PoolLock = crate::perprocess::PoolLock::new();

    /// What blocked `semop`s sleep on.
    fn waits() -> Waits = Waits::new();
}

fn wake() {
    // SAFETY: this process's counter.
    unsafe { &*waits() }.changed();
}

/// Holds the tables' lock; the tables are reached through it.
struct Locked {
    _guard: crate::perprocess::PoolGuard<'static>,
    t: *mut Tables,
}

fn lock() -> Locked {
    Locked {
        // SAFETY: `sem_lock()` is this context's lock, valid as long as the
        // tables it guards.
        _guard: unsafe { crate::perprocess::lock_pool(sem_lock()) },
        t: tables(),
    }
}

impl Locked {
    fn tables(&mut self) -> &mut Tables {
        // SAFETY: the lock is held, and `t` is this context's tables.
        unsafe { &mut *self.t }
    }

    fn set(&mut self, slot: usize) -> Option<&mut Set> {
        self.tables().slots.get(slot)
    }

    fn id_of(&mut self, slot: usize) -> i32 {
        let seq = self.set(slot).map_or(0, |s| s.perm.seq);
        encode_id(slot, seq)
    }

    /// The slot of the live set `semid` names.
    fn resolve(&mut self, semid: i32) -> Option<usize> {
        let (slot, seq) = decode_id(semid)?;
        self.set(slot)
            .is_some_and(|s| s.live && s.perm.seq & SEQ_MASK == seq)
            .then_some(slot)
    }

    /// The slot at table index `index`, if a set is there (`SEM_STAT`).
    fn at_index(&mut self, index: i32) -> Option<usize> {
        let slot = usize::try_from(index).ok()?;
        self.set(slot).is_some_and(|s| s.live).then_some(slot)
    }

    fn find_key(&mut self, key: i32) -> Option<usize> {
        let cap = self.tables().slots.cap();
        (0..cap).find(|&i| self.set(i).is_some_and(|s| s.live && s.perm.key == key))
    }

    /// Linux's `newary`: a set of `nsems` semaphores, all 0.
    fn new_set(&mut self, key: i32, nsems: usize, semflg: i32, who: Caller) -> Result<usize, i32> {
        if nsems == 0 {
            return Err(errno::EINVAL);
        }
        let t = self.tables();
        if t.used_sems + nsems > SEMMNS {
            return Err(errno::ENOSPC);
        }
        let bytes = nsems.checked_mul(size_of::<Sem>()).ok_or(errno::ENOMEM)?;
        let sems = crate::malloc::malloc(bytes).cast::<Sem>();
        if sems.is_null() {
            return Err(errno::ENOMEM);
        }
        for i in 0..nsems {
            // SAFETY: `sems` holds `nsems` entries.
            unsafe { sems.add(i).write(Sem::ZERO) };
        }
        let full = t.slots.live() >= SEMMNI;
        let Some(slot) = (if full {
            None
        } else {
            t.slots.claim(SEMMNI, || Set::EMPTY)
        }) else {
            // SAFETY: the block allocated above, not handed out.
            unsafe { crate::malloc::free(sems.cast()) };
            return Err(if full { errno::ENOSPC } else { errno::ENOMEM });
        };
        t.used_sems += nsems;
        let now = now_secs();
        let set = t.slots.get(slot).ok_or(errno::ENOMEM)?;
        let seq = set.perm.seq;
        *set = Set {
            live: true,
            perm: Perm::new(key, seq, semflg, who),
            nsems,
            sems,
            otime: 0,
            ctime: now,
        };
        Ok(slot)
    }

    /// `freeary`: the set goes; calls blocked on it will find it gone.
    fn remove(&mut self, slot: usize) {
        let t = self.tables();
        let Some(set) = t.slots.get(slot) else { return };
        let nsems = set.nsems;
        // SAFETY: the set's own block.
        unsafe { crate::malloc::free(set.sems.cast()) };
        let seq = set.perm.seq.wrapping_add(1);
        *set = Set {
            perm: Perm { seq, ..Perm::EMPTY },
            ..Set::EMPTY
        };
        t.slots.release(slot);
        t.used_sems -= nsems;
    }
}

// ---------------------------------------------------------------------------
// semget
// ---------------------------------------------------------------------------

/// `semget` — get a semaphore set identifier.
///
/// Returns the semid, or `-1` with `errno`: `EINVAL` for `nsems` below 0 or
/// past [`SEMMSL`], for 0 when a set is made, and for more than an existing
/// set holds; `ENOENT`, `EEXIST` and `EACCES` as for `msgget`; `ENOSPC`
/// past [`SEMMNI`] sets or [`SEMMNS`] semaphores; `ENOMEM`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn semget(key: i32, nsems: i32, semflg: i32) -> i32 {
    match get(key, nsems, semflg) {
        Ok(id) => id,
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

fn get(key: i32, nsems: i32, semflg: i32) -> Result<i32, i32> {
    let nsems = usize::try_from(nsems)
        .ok()
        .filter(|&n| n <= SEMMSL)
        .ok_or(errno::EINVAL)?;
    let who = caller();
    let mut t = lock();
    let slot = if key == IPC_PRIVATE {
        t.new_set(key, nsems, semflg, who)?
    } else if let Some(slot) = t.find_key(key) {
        if semflg & IPC_CREAT != 0 && semflg & IPC_EXCL != 0 {
            return Err(errno::EEXIST);
        }
        let set = t.set(slot).ok_or(errno::EINVAL)?;
        // `sem_more_checks`, before the permission.
        if nsems > set.nsems {
            return Err(errno::EINVAL);
        }
        if !permits(&set.perm, who, semflg as u32) {
            return Err(errno::EACCES);
        }
        slot
    } else if semflg & IPC_CREAT == 0 {
        return Err(errno::ENOENT);
    } else {
        t.new_set(key, nsems, semflg, who)?
    };
    Ok(t.id_of(slot))
}

// ---------------------------------------------------------------------------
// semop / semtimedop
// ---------------------------------------------------------------------------

/// What trying a call's operations came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Done,
    /// The operation on `num` cannot proceed yet: it waits for the value to
    /// reach zero (`zero`) or to grow.
    Block {
        num: usize,
        zero: bool,
    },
    Failed(i32),
}

/// Linux's `perform_atomic_semop_slow`: apply `ops` in order, all or none.
fn perform(set: &mut Set, ops: &[Sembuf], pid: i32) -> Outcome {
    let sems = set.sems();
    let mut applied = 0usize;
    let mut outcome = Outcome::Done;
    for op in ops {
        let Some(s) = sems.get_mut(usize::from(op.sem_num)) else {
            outcome = Outcome::Failed(errno::EFBIG);
            break;
        };
        let d = i32::from(op.sem_op);
        let next = s.val + d;
        if (d == 0 && s.val != 0) || next < 0 {
            outcome = if i32::from(op.sem_flg) & IPC_NOWAIT != 0 {
                Outcome::Failed(errno::EAGAIN)
            } else {
                Outcome::Block {
                    num: usize::from(op.sem_num),
                    zero: d == 0,
                }
            };
            break;
        }
        if next > i32::from(SEMVMX) {
            outcome = Outcome::Failed(errno::ERANGE);
            break;
        }
        if i32::from(op.sem_flg) & SEM_UNDO != 0 {
            let undo = s.adj - d;
            if !(-SEMAEM - 1..=SEMAEM).contains(&undo) {
                outcome = Outcome::Failed(errno::ERANGE);
                break;
            }
            s.adj = undo;
        }
        s.val = next;
        applied += 1;
    }
    if outcome == Outcome::Done {
        for op in ops {
            if let Some(s) = sems.get_mut(usize::from(op.sem_num)) {
                s.pid = pid;
            }
        }
    } else {
        // Undo what was applied, last first.
        for op in ops.iter().take(applied).rev() {
            if let Some(s) = sems.get_mut(usize::from(op.sem_num)) {
                let d = i32::from(op.sem_op);
                s.val -= d;
                if i32::from(op.sem_flg) & SEM_UNDO != 0 {
                    s.adj += d;
                }
            }
        }
    }
    outcome
}

/// `semop` — perform semaphore operations, waiting as long as it takes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn semop(semid: i32, sops: *const Sembuf, nsops: usize) -> i32 {
    semtimedop(semid, sops, nsops, core::ptr::null())
}

/// `semtimedop` — perform semaphore operations, waiting at most `timeout`
/// (relative; NULL waits as long as it takes).
///
/// Errors, in Linux's order: `E2BIG` for more than [`SEMOPM`] operations,
/// `EINVAL` for none; `EFAULT` for a NULL `sops`; `EINVAL` for a negative
/// `semid` or a malformed timeout, and for an id naming no set; `EFBIG` for
/// a semaphore number past the set; `EACCES` without the permission the
/// operations need (write if any changes a value, read otherwise); `ERANGE`
/// for a value or undo adjustment out of range; `EAGAIN` when the operation
/// that cannot proceed carries `IPC_NOWAIT`, or when the timeout runs out;
/// `EIDRM` when the set is removed while the call waits.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn semtimedop(
    semid: i32,
    sops: *const Sembuf,
    nsops: usize,
    timeout: *const Timespec,
) -> i32 {
    // SAFETY: the counter is this process's.
    let waits = unsafe { &*waits() };
    // Any signal handler that runs on this thread from the call's start ends
    // the wait, `SA_RESTART` or not: Linux never restarts `semop` or
    // `semtimedop`, and a signal that comes at any point in them ends their
    // next sleep ([`crate::interrupt`]).
    let mark = crate::interrupt::Mark::now();
    let wait = |seen, ns| waits.wait_interruptible(seen, ns, mark);
    match timed_op(semid, sops, nsops, timeout, wait) {
        Ok(()) => 0,
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

/// `semtimedop`, sleeping through `wait` (a test stands in another thread
/// there), which says whether a signal handler ended the sleep: `EINTR`,
/// once the operation is no longer counted as waiting -- unless the set was
/// removed meanwhile, which is `EIDRM`, as `freeary`'s status is on Linux.
fn timed_op(
    semid: i32,
    sops: *const Sembuf,
    nsops: usize,
    timeout: *const Timespec,
    mut wait: impl FnMut(i32, Option<u64>) -> Waited,
) -> Result<(), i32> {
    // `ksys_semtimedop` reads the timeout before anything else.
    let timeout = if timeout.is_null() {
        None
    } else {
        // SAFETY: a non-null timeout is the caller's `struct timespec`.
        Some(unsafe { timeout.read_unaligned() })
    };
    if nsops > SEMOPM {
        return Err(errno::E2BIG);
    }
    if nsops < 1 {
        return Err(errno::EINVAL);
    }
    if sops.is_null() {
        return Err(errno::EFAULT);
    }
    // Copied in, as `do_semtimedop` copies them: the caller's array may
    // change while this call waits.
    let mut copy = [Sembuf {
        sem_num: 0,
        sem_op: 0,
        sem_flg: 0,
    }; SEMOPM];
    let ops = copy.get_mut(..nsops).ok_or(errno::E2BIG)?;
    // SAFETY: the caller's `nsops` operations; `ops` holds as many.
    unsafe { core::ptr::copy_nonoverlapping(sops, ops.as_mut_ptr(), nsops) };
    let ops: &[Sembuf] = ops;
    if semid < 0 {
        return Err(errno::EINVAL);
    }
    let deadline = match timeout {
        None => None,
        Some(ts) => {
            if ts.tv_sec < 0 || !(0..1_000_000_000).contains(&ts.tv_nsec) {
                return Err(errno::EINVAL);
            }
            // `ktime_add_safe(ktime_get(), timeout)`: from now, on the
            // monotonic clock, saturating.
            let now = crate::lowlevellock::now_on(crate::time::CLOCK_MONOTONIC);
            let mut at = Timespec {
                tv_sec: now.tv_sec.saturating_add(ts.tv_sec),
                tv_nsec: now.tv_nsec + ts.tv_nsec,
            };
            if at.tv_nsec >= 1_000_000_000 {
                at.tv_nsec -= 1_000_000_000;
                at.tv_sec = at.tv_sec.saturating_add(1);
            }
            Some(at)
        }
    };
    let max = ops
        .iter()
        .map(|op| usize::from(op.sem_num))
        .max()
        .unwrap_or(0);
    let alter = ops.iter().any(|op| op.sem_op != 0);
    let who = caller();
    let pid = crate::process::getpid();
    // SAFETY: the counter is this process's.
    let waits = unsafe { &*waits() };
    let mut t = lock();
    let mut slot = t.resolve(semid).ok_or(errno::EINVAL)?;
    {
        let set = t.set(slot).ok_or(errno::EINVAL)?;
        if max >= set.nsems {
            return Err(errno::EFBIG);
        }
        if !permits(&set.perm, who, if alter { S_IWUGO } else { S_IRUGO }) {
            return Err(errno::EACCES);
        }
    }
    loop {
        let seen = waits.seen();
        let set = t.set(slot).ok_or(errno::EIDRM)?;
        match perform(set, ops, pid) {
            Outcome::Done => {
                set.otime = now_secs();
                drop(t);
                if alter {
                    wake();
                }
                return Ok(());
            }
            Outcome::Failed(e) => return Err(e),
            Outcome::Block { num, zero } => {
                let left = match deadline {
                    None => None,
                    Some(at) => {
                        let now = crate::lowlevellock::now_on(crate::time::CLOCK_MONOTONIC);
                        Some(crate::lowlevellock::ns_until(&now, &at).ok_or(errno::EAGAIN)?)
                    }
                };
                // Counted as waiting, on the semaphore its blocking operation
                // names (`GETNCNT`, `GETZCNT`).
                if let Some(s) = set.sems().get_mut(num) {
                    if zero {
                        s.zcnt += 1;
                    } else {
                        s.ncnt += 1;
                    }
                }
                drop(t);
                let waited = wait(seen, left);
                t = lock();
                // It was there before the wait: if it is not now, it was
                // removed, and its counts with it.
                slot = t.resolve(semid).ok_or(errno::EIDRM)?;
                if let Some(s) = t.set(slot).and_then(|set| set.sems().get_mut(num)) {
                    if zero {
                        s.zcnt -= 1;
                    } else {
                        s.ncnt -= 1;
                    }
                }
                if waited == Waited::Interrupted {
                    return Err(errno::EINTR);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// semctl
// ---------------------------------------------------------------------------

/// `semctl` — semaphore control operations.
///
/// `arg` is C's fourth argument, `union semun`, read only by the commands
/// that take it. glibc refuses an unknown command before anything else;
/// then Linux's `ksys_semctl` refuses a negative `semid`, and dispatches:
///
///   * `IPC_STAT`, `SEM_STAT`, `SEM_STAT_ANY` — `*arg.buf` = the set's state
///     (`SEM_STAT` names the set by table index and returns its id).
///   * `IPC_SET` — owner, group and mode from `*arg.buf`, read before the
///     set is looked up; the owner's or creator's, or `CAP_SYS_ADMIN`.
///   * `IPC_RMID` — remove the set; calls blocked on it fail with `EIDRM`.
///   * `GETVAL`, `GETPID`, `GETNCNT`, `GETZCNT` — of semaphore `semnum`.
///   * `SETVAL` — semaphore `semnum` = `arg.val` (`ERANGE` outside
///     `0..=SEMVMX`, before the set is looked up).
///   * `GETALL`, `SETALL` — every value, through `arg.array`.
///   * `IPC_INFO`, `SEM_INFO` — the limits into `*arg.__buf`; returns the
///     highest table index in use.
///
/// # Safety
///
/// `arg` holds whatever the command reads from it: a valid pointer for the
/// commands that take one.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn semctl(semid: i32, semnum: i32, cmd: i32, arg: Semun) -> i32 {
    // SAFETY: the caller's contract.
    match unsafe { control(semid, semnum, cmd, arg) } {
        Ok(n) => n,
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

/// # Safety
///
/// As [`semctl`].
unsafe fn control(semid: i32, semnum: i32, cmd: i32, arg: Semun) -> Result<i32, i32> {
    // glibc's `__semctl64` takes the fourth argument only for the commands
    // it knows, and refuses the others before the call.
    let known = matches!(
        cmd,
        SETVAL
            | GETALL
            | SETALL
            | IPC_STAT
            | IPC_SET
            | SEM_STAT
            | SEM_STAT_ANY
            | IPC_INFO
            | SEM_INFO
            | IPC_RMID
            | GETNCNT
            | GETPID
            | GETVAL
            | GETZCNT
    );
    if !known || semid < 0 {
        return Err(errno::EINVAL);
    }
    match cmd {
        IPC_INFO | SEM_INFO => {
            let (info, max_idx) = self::info(cmd == SEM_INFO);
            // SAFETY: for these commands the argument is a pointer.
            let buf = unsafe { arg.__buf };
            if buf.is_null() {
                return Err(errno::EFAULT);
            }
            // SAFETY: the caller's `struct seminfo`.
            unsafe { buf.write_unaligned(info) };
            Ok(max_idx)
        }
        IPC_STAT | SEM_STAT | SEM_STAT_ANY => {
            let (ds, ret) = stat(semid, cmd)?;
            // SAFETY: as above.
            let buf = unsafe { arg.buf };
            if buf.is_null() {
                return Err(errno::EFAULT);
            }
            // SAFETY: the caller's `struct semid_ds`.
            unsafe { buf.write_unaligned(ds) };
            Ok(ret)
        }
        SETVAL => {
            // SAFETY: SETVAL's argument is the int (the register's low half).
            setval(semid, semnum, unsafe { arg.val }).map(|()| 0)
        }
        GETALL | SETALL => {
            // SAFETY: as above.
            unsafe { all(semid, cmd, arg.array) }.map(|()| 0)
        }
        GETVAL | GETPID | GETNCNT | GETZCNT => one(semid, semnum, cmd),
        IPC_SET => {
            // SAFETY: as above.
            let buf = unsafe { arg.buf };
            if buf.is_null() {
                // `copy_semid_from_user`, before the set is looked up.
                return Err(errno::EFAULT);
            }
            // SAFETY: the caller's `struct semid_ds`.
            let ds = unsafe { buf.read_unaligned() };
            down(semid, Some(&ds)).map(|()| 0)
        }
        _ => down(semid, None).map(|()| 0),
    }
}

/// `semctl_info`: the figures, and the highest table index in use (0 if
/// none).
fn info(current: bool) -> (Seminfo, i32) {
    let mut t = lock();
    let cap = t.tables().slots.cap();
    let max_idx = (0..cap)
        .rev()
        .find(|&i| t.set(i).is_some_and(|s| s.live))
        .and_then(|i| i32::try_from(i).ok())
        .unwrap_or(0);
    let tables = t.tables();
    let clamp = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
    let info = Seminfo {
        semmap: clamp(SEMMNS),
        semmni: clamp(SEMMNI),
        semmns: clamp(SEMMNS),
        semmnu: clamp(SEMMNS),
        semmsl: clamp(SEMMSL),
        semopm: clamp(SEMOPM),
        semume: clamp(SEMOPM),
        semusz: if current {
            clamp(tables.slots.live())
        } else {
            SEMUSZ
        },
        semvmx: i32::from(SEMVMX),
        semaem: if current {
            clamp(tables.used_sems)
        } else {
            SEMAEM
        },
    };
    (info, max_idx)
}

/// `semctl_stat`: the state of the set `semid` names -- or, for `SEM_STAT`
/// and `SEM_STAT_ANY`, the one at index `semid` -- and what the call returns.
fn stat(semid: i32, cmd: i32) -> Result<(SemidDs, i32), i32> {
    let who = caller();
    let mut t = lock();
    let slot = if cmd == IPC_STAT {
        t.resolve(semid)
    } else {
        t.at_index(semid)
    }
    .ok_or(errno::EINVAL)?;
    let set = t.set(slot).ok_or(errno::EINVAL)?;
    if cmd != SEM_STAT_ANY && !permits(&set.perm, who, S_IRUGO) {
        return Err(errno::EACCES);
    }
    let ds = SemidDs {
        sem_perm: set.perm.to_ipc_perm(),
        sem_otime: set.otime,
        __unused1: 0,
        sem_ctime: set.ctime,
        __unused2: 0,
        sem_nsems: u16::try_from(set.nsems).unwrap_or(u16::MAX),
        __sem_nsems_pad: [0; 6],
        __unused3: 0,
        __unused4: 0,
    };
    let ret = if cmd == IPC_STAT { 0 } else { t.id_of(slot) };
    Ok((ds, ret))
}

/// `semctl_setval`.
fn setval(semid: i32, semnum: i32, val: i32) -> Result<(), i32> {
    if !(0..=i32::from(SEMVMX)).contains(&val) {
        return Err(errno::ERANGE);
    }
    let who = caller();
    let pid = crate::process::getpid();
    let mut t = lock();
    let slot = t.resolve(semid).ok_or(errno::EINVAL)?;
    let set = t.set(slot).ok_or(errno::EINVAL)?;
    let num = usize::try_from(semnum)
        .ok()
        .filter(|&n| n < set.nsems)
        .ok_or(errno::EINVAL)?;
    if !permits(&set.perm, who, S_IWUGO) {
        return Err(errno::EACCES);
    }
    if let Some(s) = set.sems().get_mut(num) {
        s.val = val;
        s.pid = pid;
        s.adj = 0;
    }
    set.ctime = now_secs();
    drop(t);
    wake();
    Ok(())
}

/// `semctl_main`'s `GETALL` and `SETALL`.
///
/// # Safety
///
/// `array` is null or holds the set's `nsems` values.
unsafe fn all(semid: i32, cmd: i32, array: *mut u16) -> Result<(), i32> {
    let who = caller();
    let pid = crate::process::getpid();
    let mut t = lock();
    let slot = t.resolve(semid).ok_or(errno::EINVAL)?;
    let set = t.set(slot).ok_or(errno::EINVAL)?;
    if !permits(
        &set.perm,
        who,
        if cmd == SETALL { S_IWUGO } else { S_IRUGO },
    ) {
        return Err(errno::EACCES);
    }
    if array.is_null() {
        // The copy to or from the caller faults.
        return Err(errno::EFAULT);
    }
    let n = set.nsems;
    if cmd == GETALL {
        for (i, s) in set.sems().iter().enumerate() {
            // SAFETY: the caller's array holds `n` values; values fit u16.
            unsafe {
                array
                    .add(i)
                    .write_unaligned(u16::try_from(s.val).unwrap_or(0))
            };
        }
        return Ok(());
    }
    // SETALL: every value checked before any is set.
    for i in 0..n {
        // SAFETY: as above.
        if i32::from(unsafe { array.add(i).read_unaligned() }) > i32::from(SEMVMX) {
            return Err(errno::ERANGE);
        }
    }
    for (i, s) in set.sems().iter_mut().enumerate() {
        // SAFETY: as above.
        s.val = i32::from(unsafe { array.add(i).read_unaligned() });
        s.pid = pid;
        s.adj = 0;
    }
    set.ctime = now_secs();
    drop(t);
    wake();
    Ok(())
}

/// `semctl_main`'s `GETVAL`, `GETPID`, `GETNCNT` and `GETZCNT`.
fn one(semid: i32, semnum: i32, cmd: i32) -> Result<i32, i32> {
    let who = caller();
    let mut t = lock();
    let slot = t.resolve(semid).ok_or(errno::EINVAL)?;
    let set = t.set(slot).ok_or(errno::EINVAL)?;
    if !permits(&set.perm, who, S_IRUGO) {
        return Err(errno::EACCES);
    }
    let num = usize::try_from(semnum)
        .ok()
        .filter(|&n| n < set.nsems)
        .ok_or(errno::EINVAL)?;
    let s = set.sems().get(num).copied().ok_or(errno::EINVAL)?;
    let clamp = |n: u32| i32::try_from(n).unwrap_or(i32::MAX);
    Ok(match cmd {
        GETVAL => s.val,
        GETPID => s.pid,
        GETNCNT => clamp(s.ncnt),
        _ => clamp(s.zcnt),
    })
}

/// `semctl_down`: `IPC_SET` with `ds`, `IPC_RMID` without.
fn down(semid: i32, ds: Option<&SemidDs>) -> Result<(), i32> {
    let who = caller();
    let mut t = lock();
    let slot = t.resolve(semid).ok_or(errno::EINVAL)?;
    let set = t.set(slot).ok_or(errno::EINVAL)?;
    if !may_control(&set.perm, who) {
        return Err(errno::EPERM);
    }
    match ds {
        Some(ds) => {
            set.perm
                .update(ds.sem_perm.uid, ds.sem_perm.gid, ds.sem_perm.mode)?;
            set.ctime = now_secs();
        }
        None => {
            t.remove(slot);
            drop(t);
            wake();
        }
    }
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

    fn op(num: u16, d: i16, flg: i32) -> Sembuf {
        Sembuf {
            sem_num: num,
            sem_op: d,
            sem_flg: flg as i16,
        }
    }

    fn new_set(n: i32) -> i32 {
        let id = semget(IPC_PRIVATE, n, 0o600);
        assert!(id > 0, "semget: {}", errno::get_errno());
        id
    }

    fn fail(r: i32) -> i32 {
        assert_eq!(r, -1, "expected failure");
        errno::get_errno()
    }

    fn ctl(id: i32, num: i32, cmd: i32, arg: Semun) -> i32 {
        // SAFETY: every argument these tests pass is what the command reads.
        unsafe { semctl(id, num, cmd, arg) }
    }

    const NOARG: Semun = Semun { val: 0 };

    fn val(v: i32) -> Semun {
        Semun { val: v }
    }

    fn getval(id: i32, num: i32) -> i32 {
        ctl(id, num, GETVAL, NOARG)
    }

    fn rmid(id: i32) {
        assert_eq!(ctl(id, 0, IPC_RMID, NOARG), 0);
    }

    fn stat_of(id: i32) -> SemidDs {
        // SAFETY: all-zero is a valid SemidDs.
        let mut ds: SemidDs = unsafe { core::mem::zeroed() };
        assert_eq!(
            ctl(id, 0, IPC_STAT, Semun { buf: &mut ds }),
            0,
            "IPC_STAT: {}",
            errno::get_errno()
        );
        ds
    }

    fn slot_of(id: i32) -> usize {
        decode_id(id).unwrap().0
    }

    fn sem_ops(id: i32, ops: &[Sembuf]) -> i32 {
        semop(id, ops.as_ptr(), ops.len())
    }

    /// Restores this test thread's effective capabilities when dropped.
    struct CapGuard(u32, u32);

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
            set_effective(self.0, self.1);
        }
    }

    fn without(cap: u32) -> CapGuard {
        let (lo, hi) = crate::sys_capability::current_caps_effective();
        if cap < 32 {
            set_effective(lo & !(1 << cap), hi);
        } else {
            set_effective(lo, hi & !(1 << (cap - 32)));
        }
        CapGuard(lo, hi)
    }

    // -- constants and layouts --

    #[test]
    fn constants_are_linuxs() {
        assert_eq!(
            (GETPID, GETVAL, GETALL, GETNCNT, GETZCNT, SETVAL, SETALL),
            (11, 12, 13, 14, 15, 16, 17)
        );
        assert_eq!((SEM_STAT, SEM_INFO, SEM_STAT_ANY), (18, 19, 20));
        assert_eq!((SEMMSL, SEMMNI, SEMOPM, SEMVMX), (32000, 32000, 500, 32767));
        assert_eq!(SEM_UNDO, 0x1000);
    }

    #[test]
    fn the_structures_are_musls() {
        assert_eq!(size_of::<Sembuf>(), 6);
        assert_eq!(size_of::<SemidDs>(), 104);
        assert_eq!(core::mem::offset_of!(SemidDs, sem_otime), 48);
        assert_eq!(core::mem::offset_of!(SemidDs, sem_ctime), 64);
        assert_eq!(core::mem::offset_of!(SemidDs, sem_nsems), 80);
        assert_eq!(size_of::<Seminfo>(), 40);
        assert_eq!(size_of::<Semun>(), 8, "one register");
    }

    // -- semget --

    #[test]
    fn semget_keys_and_sizes() {
        let key = 0x5e0;
        assert_eq!(fail(semget(key, 2, 0o600)), errno::ENOENT);
        let id = semget(key, 3, 0o600 | IPC_CREAT);
        assert!(id > 0);
        assert_eq!(semget(key, 3, 0o600), id);
        assert_eq!(semget(key, 0, 0), id, "0 asks for no particular size");
        assert_eq!(
            fail(semget(key, 4, 0o600)),
            errno::EINVAL,
            "more than the set holds"
        );
        assert_eq!(fail(semget(key, 1, IPC_CREAT | IPC_EXCL)), errno::EEXIST);
        rmid(id);
    }

    #[test]
    fn semget_refuses_sizes_linux_refuses() {
        assert_eq!(fail(semget(IPC_PRIVATE, -1, 0o600)), errno::EINVAL);
        assert_eq!(fail(semget(IPC_PRIVATE, 32001, 0o600)), errno::EINVAL);
        assert_eq!(
            fail(semget(IPC_PRIVATE, 0, 0o600)),
            errno::EINVAL,
            "a new set needs a semaphore"
        );
        let big = semget(IPC_PRIVATE, 32000, 0o600);
        assert!(big > 0, "SEMMSL semaphores: {}", errno::get_errno());
        rmid(big);
    }

    #[test]
    fn semget_checks_the_size_before_the_permission() {
        let id = semget(0x5e1, 2, 0o400 | IPC_CREAT);
        let caps = without(crate::sys_capability::CAP_IPC_OWNER);
        assert_eq!(fail(semget(0x5e1, 5, 0o200)), errno::EINVAL);
        assert_eq!(fail(semget(0x5e1, 2, 0o200)), errno::EACCES);
        assert_eq!(semget(0x5e1, 2, 0o400), id);
        drop(caps);
        rmid(id);
    }

    #[test]
    fn a_new_set_is_zero_and_stated() {
        let id = semget(0x5e2, 4, 0o7640 | IPC_CREAT);
        let ds = stat_of(id);
        assert_eq!(ds.sem_nsems, 4);
        assert_eq!(ds.sem_perm.mode, 0o640);
        assert_eq!(ds.sem_perm.__ipc_perm_key, 0x5e2);
        assert_eq!(ds.sem_otime, 0);
        assert!(ds.sem_ctime > 0);
        for i in 0..4 {
            assert_eq!(getval(id, i), 0);
        }
        rmid(id);
    }

    // -- semctl: the fourth argument --

    #[test]
    fn setval_reaches_semctl_through_its_fourth_argument() {
        // THE REGRESSION PIN: semctl took three arguments, so
        // semctl(id, 0, SETVAL, 1) never saw its 1.
        let id = new_set(2);
        assert_eq!(ctl(id, 1, SETVAL, val(7)), 0);
        assert_eq!(getval(id, 1), 7);
        assert_eq!(getval(id, 0), 0);
        assert_eq!(ctl(id, 1, GETPID, NOARG), crate::process::getpid());
        assert_eq!(ctl(id, 0, GETPID, NOARG), 0, "untouched");
        rmid(id);
    }

    #[test]
    fn setval_range_comes_before_the_lookup() {
        let id = new_set(1);
        assert_eq!(fail(ctl(id + 1, 0, SETVAL, val(-1))), errno::ERANGE);
        assert_eq!(fail(ctl(id, 0, SETVAL, val(32768))), errno::ERANGE);
        assert_eq!(ctl(id, 0, SETVAL, val(32767)), 0);
        assert_eq!(fail(ctl(id + 1, 0, SETVAL, val(1))), errno::EINVAL);
        assert_eq!(
            fail(ctl(id, 1, SETVAL, val(1))),
            errno::EINVAL,
            "no semaphore 1"
        );
        assert_eq!(fail(ctl(id, -1, GETVAL, NOARG)), errno::EINVAL);
        rmid(id);
    }

    #[test]
    fn getall_and_setall_go_through_the_array() {
        let id = new_set(3);
        let mut vals = [4u16, 5, 6];
        assert_eq!(
            ctl(
                id,
                0,
                SETALL,
                Semun {
                    array: vals.as_mut_ptr()
                }
            ),
            0
        );
        let mut out = [0u16; 3];
        assert_eq!(
            ctl(
                id,
                0,
                GETALL,
                Semun {
                    array: out.as_mut_ptr()
                }
            ),
            0
        );
        assert_eq!(out, [4, 5, 6]);
        // A value past SEMVMX sets none of them.
        let mut bad = [1u16, 40000, 1];
        assert_eq!(
            fail(ctl(
                id,
                0,
                SETALL,
                Semun {
                    array: bad.as_mut_ptr()
                }
            )),
            errno::ERANGE
        );
        assert_eq!(
            ctl(
                id,
                0,
                GETALL,
                Semun {
                    array: out.as_mut_ptr()
                }
            ),
            0
        );
        assert_eq!(out, [4, 5, 6]);
        // The array is reached after the lookup.
        let null = Semun {
            array: core::ptr::null_mut(),
        };
        assert_eq!(fail(ctl(id + 1, 0, GETALL, null)), errno::EINVAL);
        assert_eq!(fail(ctl(id, 0, GETALL, null)), errno::EFAULT);
        assert_eq!(fail(ctl(id, 0, SETALL, null)), errno::EFAULT);
        rmid(id);
    }

    #[test]
    fn ipc_set_changes_the_owner_and_the_mode() {
        let id = new_set(1);
        let mut ds = stat_of(id);
        ds.sem_perm.mode = 0o7644;
        ds.sem_perm.uid = 1234;
        assert_eq!(ctl(id, 0, IPC_SET, Semun { buf: &mut ds }), 0);
        let now = stat_of(id);
        assert_eq!((now.sem_perm.mode, now.sem_perm.uid), (0o644, 1234));
        ds.sem_perm.gid = u32::MAX;
        assert_eq!(
            fail(ctl(id, 0, IPC_SET, Semun { buf: &mut ds })),
            errno::EINVAL
        );
        let null = Semun {
            buf: core::ptr::null_mut(),
        };
        assert_eq!(
            fail(ctl(id + 1, 0, IPC_SET, null)),
            errno::EFAULT,
            "the buffer is read first"
        );
        assert_eq!(
            fail(ctl(id + 1, 0, IPC_STAT, null)),
            errno::EINVAL,
            "the lookup comes first"
        );
        assert_eq!(fail(ctl(id, 0, IPC_STAT, null)), errno::EFAULT);
        rmid(id);
    }

    #[test]
    fn ipc_set_and_rmid_are_the_owners() {
        let id = new_set(1);
        {
            let mut t = lock();
            let set = t.set(slot_of(id)).unwrap();
            set.perm.uid = 1000;
            set.perm.cuid = 1000;
        }
        let mut ds = stat_of(id);
        {
            let _g = without(crate::sys_capability::CAP_SYS_ADMIN);
            assert_eq!(
                fail(ctl(id, 0, IPC_SET, Semun { buf: &mut ds })),
                errno::EPERM
            );
            assert_eq!(fail(ctl(id, 0, IPC_RMID, NOARG)), errno::EPERM);
        }
        rmid(id);
        assert_eq!(fail(getval(id, 0)), errno::EINVAL);
    }

    #[test]
    fn semctl_refuses_unknown_commands_and_negative_ids() {
        let id = new_set(1);
        assert_eq!(fail(ctl(id, 0, 99, NOARG)), errno::EINVAL);
        assert_eq!(fail(ctl(-1, 0, GETVAL, NOARG)), errno::EINVAL);
        assert_eq!(fail(ctl(id + 1, 0, GETVAL, NOARG)), errno::EINVAL);
        rmid(id);
    }

    #[test]
    fn the_info_commands() {
        let mut si = Seminfo::default();
        assert_eq!(
            ctl(0, 0, IPC_INFO, Semun { __buf: &mut si }),
            0,
            "no set: index 0"
        );
        assert_eq!(
            (
                si.semmni, si.semmsl, si.semopm, si.semvmx, si.semusz, si.semaem
            ),
            (32000, 32000, 500, 32767, 20, 32767)
        );
        assert_eq!(si.semmns, 1_024_000_000);
        let a = new_set(3);
        let b = new_set(2);
        let max = ctl(0, 0, SEM_INFO, Semun { __buf: &mut si });
        assert_eq!((si.semusz, si.semaem), (2, 5), "sets and semaphores in use");
        assert_eq!(max, i32::try_from(slot_of(a).max(slot_of(b))).unwrap());
        assert_eq!(
            fail(ctl(
                0,
                0,
                IPC_INFO,
                Semun {
                    __buf: core::ptr::null_mut()
                }
            )),
            errno::EFAULT
        );
        rmid(a);
        rmid(b);
    }

    #[test]
    fn sem_stat_walks_the_table() {
        let a = new_set(1);
        let b = new_set(1);
        rmid(a);
        let mut si = Seminfo::default();
        let max = ctl(0, 0, SEM_INFO, Semun { __buf: &mut si });
        let mut found = Vec::new();
        for i in 0..=max {
            // SAFETY: all-zero is a valid SemidDs.
            let mut ds: SemidDs = unsafe { core::mem::zeroed() };
            let r = ctl(i, 0, SEM_STAT, Semun { buf: &mut ds });
            if r >= 0 {
                found.push(r);
            }
        }
        assert_eq!(found, [b]);
        // SEM_STAT_ANY skips the read permission SEM_STAT needs.
        let c = semget(IPC_PRIVATE, 1, 0);
        let i = i32::try_from(slot_of(c)).unwrap();
        // SAFETY: as above.
        let mut ds: SemidDs = unsafe { core::mem::zeroed() };
        {
            let _g = without(crate::sys_capability::CAP_IPC_OWNER);
            assert_eq!(
                fail(ctl(i, 0, SEM_STAT, Semun { buf: &mut ds })),
                errno::EACCES
            );
            assert_eq!(ctl(i, 0, SEM_STAT_ANY, Semun { buf: &mut ds }), c);
        }
        rmid(b);
        rmid(c);
    }

    // -- semop --

    #[test]
    fn semop_increments_decrements_and_records() {
        let id = new_set(2);
        assert_eq!(sem_ops(id, &[op(0, 2, 0), op(1, 1, 0)]), 0);
        assert_eq!((getval(id, 0), getval(id, 1)), (2, 1));
        assert_eq!(sem_ops(id, &[op(0, -1, 0)]), 0);
        assert_eq!(getval(id, 0), 1);
        assert!(stat_of(id).sem_otime > 0);
        assert_eq!(ctl(id, 0, GETPID, NOARG), crate::process::getpid());
        rmid(id);
    }

    #[test]
    fn a_batch_is_applied_in_order_all_or_none() {
        let id = new_set(2);
        // The second operation may rely on the first.
        assert_eq!(sem_ops(id, &[op(0, 1, 0), op(0, -1, 0)]), 0);
        // One that cannot proceed undoes those before it.
        assert_eq!(
            fail(sem_ops(id, &[op(0, 5, 0), op(1, -1, IPC_NOWAIT)])),
            errno::EAGAIN
        );
        assert_eq!(getval(id, 0), 0, "undone");
        rmid(id);
    }

    #[test]
    fn only_the_blocking_operations_ipc_nowait_counts() {
        // THE REGRESSION PIN: IPC_NOWAIT on one operation made the whole
        // batch non-blocking.  In Linux it is the operation that cannot
        // proceed that decides.
        let id = new_set(2);
        let ops = [op(0, 1, IPC_NOWAIT), op(1, -1, 0)];
        let mut waits = 0;
        let r = timed_op(id, ops.as_ptr(), 2, core::ptr::null(), |_, _| {
            waits += 1;
            assert_eq!(ctl(id, 1, SETVAL, val(1)), 0);
            Waited::Woken
        });
        assert_eq!((r, waits), (Ok(()), 1), "it blocked, then went");
        assert_eq!((getval(id, 0), getval(id, 1)), (1, 0));
        rmid(id);
    }

    #[test]
    fn values_and_undo_stay_in_range() {
        let id = new_set(1);
        assert_eq!(ctl(id, 0, SETVAL, val(32767)), 0);
        assert_eq!(fail(sem_ops(id, &[op(0, 1, IPC_NOWAIT)])), errno::ERANGE);
        assert_eq!(ctl(id, 0, SETVAL, val(0)), 0);
        // SEM_UNDO's adjustment runs from -32768 to 32767: +32767 and +1
        // reach its end, and one more passes it.
        assert_eq!(sem_ops(id, &[op(0, 32767, SEM_UNDO)]), 0);
        assert_eq!(sem_ops(id, &[op(0, -32767, 0)]), 0);
        assert_eq!(sem_ops(id, &[op(0, 1, SEM_UNDO)]), 0);
        assert_eq!(
            fail(sem_ops(id, &[op(0, 1, SEM_UNDO)])),
            errno::ERANGE,
            "adjustment past -32768"
        );
        assert_eq!(getval(id, 0), 1, "the refused one changed nothing");
        // SETVAL clears it.
        assert_eq!(ctl(id, 0, SETVAL, val(0)), 0);
        assert_eq!(sem_ops(id, &[op(0, 1, SEM_UNDO)]), 0);
        rmid(id);
    }

    #[test]
    fn semop_checks_come_in_linuxs_order() {
        let id = new_set(2);
        let null = core::ptr::null();
        assert_eq!(fail(semop(-1, null, 501)), errno::E2BIG, "the count first");
        assert_eq!(fail(semop(-1, null, 0)), errno::EINVAL);
        assert_eq!(fail(semop(-1, null, 1)), errno::EFAULT, "then the array");
        let one = [op(0, 1, 0)];
        assert_eq!(
            fail(semop(-1, one.as_ptr(), 1)),
            errno::EINVAL,
            "then the id"
        );
        assert_eq!(fail(semop(id + 1, one.as_ptr(), 1)), errno::EINVAL);
        assert_eq!(
            fail(sem_ops(id, &[op(2, 1, 0)])),
            errno::EFBIG,
            "no semaphore 2"
        );
        rmid(id);
    }

    #[test]
    fn semop_needs_the_permission_its_operations_need() {
        let id = semget(IPC_PRIVATE, 1, 0o400);
        let caps = without(crate::sys_capability::CAP_IPC_OWNER);
        assert_eq!(
            fail(sem_ops(id, &[op(0, 1, 0)])),
            errno::EACCES,
            "changing needs write"
        );
        assert_eq!(
            sem_ops(id, &[op(0, 0, 0)]),
            0,
            "waiting for zero needs read"
        );
        drop(caps);
        rmid(id);
    }

    // -- blocking, with the other thread played by `wait` --

    #[test]
    fn a_blocked_decrement_goes_when_the_value_rises() {
        let id = new_set(1);
        let ops = [op(0, -1, 0)];
        let mut waits = 0;
        let r = timed_op(id, ops.as_ptr(), 1, core::ptr::null(), |_, timeout| {
            assert_eq!(timeout, None);
            waits += 1;
            // While it waits, it is counted.
            assert_eq!(ctl(id, 0, GETNCNT, NOARG), 1);
            assert_eq!(ctl(id, 0, GETZCNT, NOARG), 0);
            assert_eq!(sem_ops(id, &[op(0, 1, 0)]), 0);
            Waited::Woken
        });
        assert_eq!((r, waits), (Ok(()), 1));
        assert_eq!(getval(id, 0), 0);
        assert_eq!(ctl(id, 0, GETNCNT, NOARG), 0, "and no longer");
        rmid(id);
    }

    #[test]
    fn a_wait_for_zero_is_counted_by_getzcnt() {
        let id = new_set(1);
        assert_eq!(ctl(id, 0, SETVAL, val(2)), 0);
        let ops = [op(0, 0, 0)];
        let r = timed_op(id, ops.as_ptr(), 1, core::ptr::null(), |_, _| {
            assert_eq!(ctl(id, 0, GETZCNT, NOARG), 1);
            assert_eq!(ctl(id, 0, SETVAL, val(0)), 0);
            Waited::Woken
        });
        assert_eq!(r, Ok(()));
        rmid(id);
    }

    #[test]
    fn semtimedops_timeout_is_relative() {
        // THE REGRESSION PIN: the timeout was read as a CLOCK_REALTIME
        // deadline, so "wait five seconds" had passed in 1970.
        let id = new_set(1);
        let ops = [op(0, -1, 0)];
        let five = Timespec {
            tv_sec: 5,
            tv_nsec: 0,
        };
        let mut asked = None;
        let r = timed_op(id, ops.as_ptr(), 1, &five, |_, timeout| {
            asked = timeout;
            assert_eq!(ctl(id, 0, SETVAL, val(1)), 0);
            Waited::Woken
        });
        assert_eq!(r, Ok(()));
        let ns = asked.expect("it waited, with a timeout");
        assert!(
            ns > 4_000_000_000 && ns <= 5_000_000_000,
            "about five seconds: {ns}"
        );
        rmid(id);
    }

    #[test]
    fn a_timeout_that_has_run_out_is_eagain() {
        let id = new_set(1);
        let ops = [op(0, -1, 0)];
        let zero = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let r = timed_op(id, ops.as_ptr(), 1, &zero, |_, _| panic!("no time to wait"));
        assert_eq!(r, Err(errno::EAGAIN));
        assert_eq!(ctl(id, 0, GETNCNT, NOARG), 0, "not left counted");
        // A malformed timeout is EINVAL.
        let bad = Timespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000,
        };
        assert_eq!(semtimedop(id, ops.as_ptr(), 1, &bad), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        let neg = Timespec {
            tv_sec: -1,
            tv_nsec: 0,
        };
        assert_eq!(semtimedop(id, ops.as_ptr(), 1, &neg), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        rmid(id);
    }

    #[test]
    fn a_wait_a_signal_handler_ends_is_eintr_and_no_longer_counted() {
        let id = new_set(1);
        for ops in [[op(0, -1, 0)], [op(0, 0, 0)]] {
            if ops[0].sem_op == 0 {
                assert_eq!(ctl(id, 0, SETVAL, val(1)), 0);
            }
            let r = timed_op(id, ops.as_ptr(), 1, core::ptr::null(), |_, _| {
                Waited::Interrupted
            });
            assert_eq!(r, Err(errno::EINTR));
            assert_eq!(ctl(id, 0, GETNCNT, NOARG), 0);
            assert_eq!(ctl(id, 0, GETZCNT, NOARG), 0);
        }
        assert_eq!(getval(id, 0), 1, "nothing was performed");
        rmid(id);
    }

    #[test]
    fn a_set_removed_while_a_handler_ran_is_eidrm_not_eintr() {
        let id = new_set(1);
        let ops = [op(0, -1, 0)];
        let r = timed_op(id, ops.as_ptr(), 1, core::ptr::null(), |_, _| {
            rmid(id);
            Waited::Interrupted
        });
        assert_eq!(r, Err(errno::EIDRM));
    }

    #[test]
    fn a_set_removed_under_a_blocked_call_is_eidrm() {
        let id = new_set(1);
        let ops = [op(0, -1, 0)];
        assert_eq!(
            timed_op(id, ops.as_ptr(), 1, core::ptr::null(), |_, _| {
                rmid(id);
                Waited::Woken
            }),
            Err(errno::EIDRM)
        );
        // Removed and replaced in the same slot: still EIDRM, and the new
        // set's counts are left alone.
        let id = new_set(1);
        let mut replacement = 0;
        let r = timed_op(id, ops.as_ptr(), 1, core::ptr::null(), |_, _| {
            rmid(id);
            replacement = new_set(1);
            Waited::Woken
        });
        assert_eq!(r, Err(errno::EIDRM));
        assert_eq!(slot_of(replacement), slot_of(id));
        assert_eq!(ctl(replacement, 0, GETNCNT, NOARG), 0);
        rmid(replacement);
    }

    #[test]
    fn many_sets_up_to_semmni() {
        let mut ids = Vec::with_capacity(SEMMNI);
        for _ in 0..SEMMNI {
            ids.push(new_set(1));
        }
        assert_eq!(fail(semget(IPC_PRIVATE, 1, 0o600)), errno::ENOSPC);
        for id in ids {
            rmid(id);
        }
    }
}
