//! POSIX byte-range advisory record locks — the kernel side of `fcntl`'s
//! `F_SETLK`, `F_SETLKW` and `F_GETLK`, for both ABIs.
//!
//! This is the table. [`crate::syscall::record_lock`] is the syscall half the
//! Linux `fcntl` and the native `SYS_FS_RECORD_LOCK` share: the caller's
//! `struct flock`, its range against the handle, and the access-mode rule.
//!
//! ## Why this is not the `flock` table in `vfs.rs`
//!
//! `vfs.rs` already has an advisory lock table, and it is whole-file: one
//! [`FileLock`](super::vfs) per `(path, owner)`, with upgrade and downgrade.
//! Pointing `F_SETLK` at it is the obvious shortcut and would be **worse than
//! the stub it replaces**, because it inverts the direction of the error:
//!
//! | | what happens |
//! |---|---|
//! | the stub (before this) | two exclusive locks on one range both succeed — a false SUCCESS, so no mutual exclusion |
//! | `F_SETLK` → whole-file `flock` | two locks on *disjoint* ranges conflict — a false FAILURE |
//!
//! A false success breaks programs that rely on locking, of which there are
//! currently none, because nothing can. A false failure breaks programs that
//! lock disjoint ranges correctly — the normal case for a database or an index
//! — so "fixing" the stub that way would break working software. They are also
//! genuinely different lock spaces in POSIX: `flock` and `fcntl` locks do not
//! see each other, and a process may hold both.
//!
//! ## Semantics implemented here
//!
//! * A lock covers `[start, start + len)`. **`len == 0` means "to end of
//!   file"**, which is POSIX's spelling for an open-ended range, so the end is
//!   [`u64::MAX`] rather than `start`.
//! * Two locks conflict when they have **different owners**, their ranges
//!   **overlap**, and **at least one is a write lock**. Read locks share.
//! * A process never conflicts with itself. Re-locking a range it already
//!   holds replaces the old coverage, which is why `set` removes the owner's
//!   overlap before inserting.
//! * An owner's locks of one type that **touch merge** into one, as Linux's
//!   `posix_lock_inode` merges them. So `F_GETLK` reports the whole range a
//!   holder has rather than whichever piece it added last, and a program that
//!   locks a file one record at a time does not run out of entries.
//! * Unlocking a range out of the middle of a held lock **splits** it, leaving
//!   the two ends held. Getting this wrong silently releases bytes the caller
//!   asked to keep, which no test of the common case would catch.
//!
//! Locks are advisory: they do not prevent I/O. Cooperating programs check.
//!
//! ## Which file: [`LockKey`]
//!
//! A key names the file a lock is on without resolving anything again: a VFS
//! file by its resolved host path and its identity, or a memfd by its object
//! id. Built from a handle ([`LockKey::for_handle`]), the path is the one
//! captured at open, which has already been through the caller's namespace.
//! Until 2026-10-01 the table took a path and passed it to
//! `Vfs::file_identity`, which applies the caller's namespace. For a jailed
//! process that applied the jail a second time, so its locks were keyed by
//! name alone, or by the identity of whatever file the doubled path named.
//!
//! ## Waiting: [`set_wait`]
//!
//! `F_SETLKW` parks the caller until nothing is in its way. Every change that
//! can free a range wakes the tasks waiting on that file: an unlock, a
//! release, a lock replaced or downgraded. A woken task takes the table lock
//! and tries again, and one that loses the race parks again with its blockers
//! recomputed. A deliverable signal ends the wait with `Interrupted`, which
//! the syscall layers restart under `SA_RESTART`, as Linux restarts
//! `F_SETLKW`.
//!
//! Before parking, the wait-for graph is searched. If an owner in the caller's
//! way is waiting on the caller, directly or through others, sleeping would
//! never end, and the answer is `Deadlock` (`EDEADLK`). The search covers every
//! waiter present; Linux follows one chain for ten steps. As in Linux an owner
//! is a process, so a cycle through one thread of a multi-threaded process is
//! reported even when another of its threads could still release the lock.
//! Also as in Linux, OFD locks take no part: their owner is an open file
//! description, which is not something that waits.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use super::path::{Path, PathBuf};
use crate::error::{KernelError, KernelResult};
use crate::sched::task::TaskId;
use crate::sync::Mutex;

/// The kind of a record lock. `F_UNLCK` is an operation, not a stored state,
/// so it is deliberately absent from this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordLockType {
    /// `F_RDLCK` — shared. Compatible with other read locks.
    Read,
    /// `F_WRLCK` — exclusive.
    Write,
}

/// One held byte-range lock.
#[derive(Debug, Clone)]
pub struct RecordLock {
    /// Who holds it: a process ([`posix_owner`]) or an open file description
    /// ([`ofd_owner`], [`memfd_ofd_owner`]).
    pub owner: u64,
    /// First byte covered.
    pub start: u64,
    /// Bytes covered, or `0` for "to end of file".
    pub len: u64,
    pub lock_type: RecordLockType,
}

impl RecordLock {
    /// One past the last byte covered, saturating.
    ///
    /// `len == 0` is POSIX's "to end of file", so it maps to [`u64::MAX`] and
    /// NOT to `start` — the mistake that would make an open-ended lock cover
    /// nothing and silently conflict with no one.
    #[must_use]
    pub fn end(&self) -> u64 {
        if self.len == 0 {
            u64::MAX
        } else {
            self.start.saturating_add(self.len)
        }
    }

    /// Whether two ranges share at least one byte.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        self.start < other.end() && other.start < self.end()
    }

    /// Whether `other` would be refused because of `self`.
    ///
    /// Three conditions, all required: different owners, overlapping ranges,
    /// and at least one side writing. A process never blocks itself, which is
    /// what makes re-locking a held range legal rather than a deadlock.
    #[must_use]
    pub fn conflicts_with(&self, other: &Self) -> bool {
        self.owner != other.owner
            && self.overlaps(other)
            && (self.lock_type == RecordLockType::Write || other.lock_type == RecordLockType::Write)
    }

    /// The lock on `[start, end)`, where `end == u64::MAX` is "to end of file".
    fn spanning(owner: u64, start: u64, end: u64, lock_type: RecordLockType) -> Self {
        let len = if end == u64::MAX {
            0
        } else {
            end.saturating_sub(start)
        };
        Self {
            owner,
            start,
            len,
            lock_type,
        }
    }
}

/// Which file a record lock is on.
///
/// Built once, outside the table lock, and compared by [`Self::same_file`].
/// Resolving under the table lock would take filesystem locks inside it.
/// `/proc` reads take them the other way round for the `flock` table, which
/// is the AB/BA deadlock lockdep found there on 2026-09-26.
#[derive(Debug, Clone)]
pub enum LockKey {
    /// A file in the VFS: its resolved host path, and its identity where the
    /// filesystem has stable inode numbers. Identity is the real key, since a
    /// lock taken under one name must be seen under every other name for the
    /// same file. The path is the fallback where there is none.
    File {
        path: PathBuf,
        id: Option<crate::fs::vfs::FileId>,
    },
    /// An anonymous memory file (`memfd_create`), by its object id. It has no
    /// path, and the id names the object rather than one handle to it:
    /// `memfd::dup` returns the same value.
    MemFd(u64),
}

impl LockKey {
    /// The file behind an open file handle.
    ///
    /// The path is the one the handle was opened under, already resolved
    /// through the opener's namespace, so its identity is looked up with
    /// `file_identity_resolved` and never with `file_identity`. The latter
    /// would apply the caller's namespace a second time.
    ///
    /// # Errors
    ///
    /// `InvalidHandle` for a handle that is not open.
    pub fn for_handle(handle: u64) -> KernelResult<Self> {
        let path = crate::fs::handle::handle_path(handle)?;
        // No identity keys by the path, which is what the table has always
        // done for a file that has none: a filesystem without stable inode
        // numbers, or a name that has since been unlinked or renamed. Every
        // holder of the same open-time path then shares one key.
        let id = crate::fs::Vfs::file_identity_resolved(&path).unwrap_or(None);
        Ok(Self::File { path, id })
    }

    /// A file as the calling task names it, through its own namespace.
    ///
    /// For kernel code and self-tests, which have a path and no handle. A
    /// syscall that has a handle uses [`Self::for_handle`].
    pub fn for_path(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        // A name that resolves to nothing keys by the name: the self-tests'
        // synthetic paths name no file, and the table has always treated an
        // unresolvable name that way.
        let id = crate::fs::Vfs::file_identity(path).unwrap_or(None);
        Self::File {
            path: path.to_path_buf(),
            id,
        }
    }

    /// A memfd, by the id its handles carry.
    #[must_use]
    pub const fn for_memfd(id: u64) -> Self {
        Self::MemFd(id)
    }

    /// Do two keys name the same file?
    ///
    /// For VFS files, identity decides when both sides have one, and the
    /// path otherwise. A memfd is never a VFS file.
    fn same_file(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::File { path: a, id: ia }, Self::File { path: b, id: ib }) => match (ia, ib) {
                (Some(x), Some(y)) => x == y,
                _ => a == b,
            },
            (Self::MemFd(a), Self::MemFd(b)) => a == b,
            _ => false,
        }
    }
}

/// Every record lock held on one file.
struct FileLocks {
    key: LockKey,
    locks: Vec<RecordLock>,
}

/// A task parked in [`set_wait`].
struct Waiter {
    task: TaskId,
    /// The task's process, or 0 for a kernel task, so that a process which
    /// dies with a task parked here takes the entry with it
    /// ([`release_process`]).
    pid: u64,
    /// Who is asking: an edge's tail in the wait-for graph.
    owner: u64,
    /// The file it waits on. Only a change to that file wakes it.
    key: LockKey,
    /// The owners of the locks in its way: the edges' heads.
    blocked_on: Vec<u64>,
}

/// The locks, and the tasks waiting for them, under one lock: a waiter is
/// registered in the same critical section that found the conflict, so a
/// release between the two cannot be missed.
struct Table {
    files: Vec<FileLocks>,
    waiters: Vec<Waiter>,
}

/// The record-lock table.
///
/// Separate from `vfs::LOCK_TABLE` on purpose; see the module doc. Nothing is
/// called with it held that takes a filesystem lock: keys are resolved before.
static TABLE: Mutex<Table> = Mutex::new(Table {
    files: Vec::new(),
    waiters: Vec::new(),
});

/// How many files have record locks right now: `TABLE`'s file count, kept
/// beside it so [`holds_any`] can answer "nothing is locked" without taking
/// the table lock. Every file close asks that question, through POSIX's close
/// rule, and almost none has a lock to drop. Stored under the table lock,
/// after every change to the file list.
static LOCKED_FILES: AtomicUsize = AtomicUsize::new(0);

/// Files that may hold record locks at once.
const MAX_LOCKED_FILES: usize = 1024;

/// Locks on one file. Bounded so a runaway caller cannot exhaust the heap.
const MAX_LOCKS_PER_FILE: usize = 256;

/// Tasks that may wait at once. Each is a parked task, so the bound is one on
/// heap use rather than on anything a single caller can reach.
const MAX_WAITERS: usize = 1024;

/// Remove `[start, end)` from `held`, splitting it if the cut is interior.
///
/// Returns the pieces that remain held. This is the operation that is easy to
/// get wrong in the direction that loses bytes: unlocking the middle of a held
/// range must leave BOTH ends held, and a version that returned an empty Vec
/// for that case would pass every test that only unlocks whole ranges.
fn subtract(held: &RecordLock, start: u64, end: u64) -> Vec<RecordLock> {
    let mut out = Vec::new();
    let h_start = held.start;
    let h_end = held.end();
    if end <= h_start || start >= h_end {
        // No overlap: the lock survives whole.
        out.push(held.clone());
        return out;
    }
    if h_start < start {
        out.push(RecordLock::spanning(
            held.owner,
            h_start,
            start,
            held.lock_type,
        ));
    }
    if end < h_end {
        // The tail keeps "to end of file" only if the original had it: a cut
        // that ends inside the lock does not make the lock's own end finite.
        out.push(RecordLock::spanning(held.owner, end, h_end, held.lock_type));
    }
    out
}

/// `held` with `want` installed for its owner.
///
/// The owner's own coverage of the range is replaced, so re-locking with the
/// other type upgrades or downgrades it, and the result is merged with the
/// owner's touching locks of the same type. Also answers whether any of the
/// owner's coverage was replaced: that is the one kind of `set` that can free
/// a waiter, since a downgrade lets readers in. More coverage frees no one.
fn install(held: &[RecordLock], want: &RecordLock) -> (Vec<RecordLock>, bool) {
    let end = want.end();
    let mut out = Vec::with_capacity(held.len().saturating_add(2));
    let mut replaced = false;
    for h in held {
        if h.owner == want.owner && h.overlaps(want) {
            replaced = true;
            out.extend(subtract(h, want.start, end));
        } else {
            out.push(h.clone());
        }
    }
    // Absorb touching locks until none is left. One pass in list order is not
    // enough: absorbing one piece can make the merged range touch another
    // that was checked before it.
    let mut merged = want.clone();
    while let Some(i) = out.iter().position(|h| {
        h.owner == merged.owner
            && h.lock_type == merged.lock_type
            && (h.end() == merged.start || merged.end() == h.start)
    }) {
        let h = out.remove(i);
        let start = h.start.min(merged.start);
        let end = h.end().max(merged.end());
        merged = RecordLock::spanning(merged.owner, start, end, merged.lock_type);
    }
    out.push(merged);
    (out, replaced)
}

impl Table {
    fn position(&self, key: &LockKey) -> Option<usize> {
        self.files.iter().position(|f| f.key.same_file(key))
    }

    /// The owners whose locks on `key` refuse `want`, each once.
    fn blockers(&self, key: &LockKey, want: &RecordLock) -> Vec<u64> {
        let mut out = Vec::new();
        if let Some(file) = self.position(key).and_then(|i| self.files.get(i)) {
            for held in file.locks.iter().filter(|h| h.conflicts_with(want)) {
                if !out.contains(&held.owner) {
                    out.push(held.owner);
                }
            }
        }
        out
    }

    /// Install `want` on `key`, or refuse with `WouldBlock` while another
    /// owner's lock is in the way. `Ok(true)` when the owner's own coverage
    /// was replaced (see [`install`]).
    fn try_set(&mut self, key: &LockKey, want: &RecordLock) -> KernelResult<bool> {
        let idx = match self.position(key) {
            Some(i) => i,
            None => {
                if self.files.len() >= MAX_LOCKED_FILES {
                    return Err(KernelError::ResourceExhausted);
                }
                self.files.push(FileLocks {
                    key: key.clone(),
                    locks: Vec::new(),
                });
                LOCKED_FILES.store(self.files.len(), Ordering::Release);
                self.files.len().saturating_sub(1)
            }
        };
        let file = self.files.get_mut(idx).ok_or(KernelError::InternalError)?;
        // A file entry only ever exists with locks in it, except for the one
        // just pushed, which has none to conflict with or to overflow; so
        // neither refusal below can leave an empty entry behind.
        if file.locks.iter().any(|h| h.conflicts_with(want)) {
            return Err(KernelError::WouldBlock);
        }
        let (locks, replaced) = install(&file.locks, want);
        // Counted after the merge, not before: changing the type of a lock
        // already held, or extending one, needs no new entry.
        if locks.len() > MAX_LOCKS_PER_FILE {
            return Err(KernelError::ResourceExhausted);
        }
        file.locks = locks;
        Ok(replaced)
    }

    /// Remove `[start, end)` from `owner`'s coverage of `key`. Whether
    /// anything was held there.
    fn unlock(&mut self, key: &LockKey, owner: u64, start: u64, end: u64) -> bool {
        let Some(idx) = self.position(key) else {
            return false;
        };
        let Some(file) = self.files.get_mut(idx) else {
            return false;
        };
        let mut changed = false;
        let mut kept = Vec::with_capacity(file.locks.len().saturating_add(1));
        for h in &file.locks {
            if h.owner == owner && h.start < end && start < h.end() {
                changed = true;
                kept.extend(subtract(h, start, end));
            } else {
                kept.push(h.clone());
            }
        }
        file.locks = kept;
        if file.locks.is_empty() {
            self.files.remove(idx);
            LOCKED_FILES.store(self.files.len(), Ordering::Release);
        }
        changed
    }

    /// Drop every lock `owner` holds, on every file; the tasks to wake.
    fn release_everywhere(&mut self, owner: u64) -> Vec<TaskId> {
        let mut freed: Vec<LockKey> = Vec::new();
        for file in &mut self.files {
            let before = file.locks.len();
            file.locks.retain(|h| h.owner != owner);
            if file.locks.len() != before {
                freed.push(file.key.clone());
            }
        }
        self.files.retain(|f| !f.locks.is_empty());
        LOCKED_FILES.store(self.files.len(), Ordering::Release);
        let mut woken = Vec::new();
        for key in &freed {
            woken.extend(self.take_waiters_on(key));
        }
        woken
    }

    /// Take every task waiting on `key`, to be woken once the lock is dropped.
    ///
    /// Taken, not just read: a woken waiter no longer counts as waiting, so
    /// the wait-for graph a third task searches in the meantime does not
    /// report a deadlock through an edge that has already gone.
    fn take_waiters_on(&mut self, key: &LockKey) -> Vec<TaskId> {
        let mut woken = Vec::new();
        self.waiters.retain(|w| {
            if w.key.same_file(key) {
                woken.push(w.task);
                false
            } else {
                true
            }
        });
        woken
    }

    fn remove_waiter(&mut self, task: TaskId) {
        self.waiters.retain(|w| w.task != task);
    }

    /// Would `me`, waiting behind `blockers`, wait forever?
    ///
    /// A depth-first search of the wait-for graph from the owners in the way,
    /// each expanded once, through every waiter they have. It answers yes on
    /// reaching `me`. Every lock in the way must go before `me` can proceed,
    /// so any cycle is a deadlock: no one on it can move first.
    fn would_deadlock(&self, me: u64, blockers: &[u64]) -> bool {
        if owner_is_ofd(me) {
            return false;
        }
        let mut stack: Vec<u64> = blockers.to_vec();
        let mut seen: Vec<u64> = Vec::new();
        while let Some(owner) = stack.pop() {
            if owner == me {
                return true;
            }
            if owner_is_ofd(owner) || seen.contains(&owner) {
                continue;
            }
            seen.push(owner);
            for w in self.waiters.iter().filter(|w| w.owner == owner) {
                stack.extend(w.blocked_on.iter().copied());
            }
        }
        false
    }
}

/// Take a record lock, or report the first conflict.
///
/// `Err(WouldBlock)` when another owner holds an incompatible overlapping
/// range: `F_SETLK`'s contract. `F_SETLKW` is [`set_wait`].
/// `Err(ResourceExhausted)` when the table is full (POSIX `ENOLCK`).
pub fn set(
    key: &LockKey,
    owner: u64,
    start: u64,
    len: u64,
    lock_type: RecordLockType,
) -> KernelResult<()> {
    let want = RecordLock {
        owner,
        start,
        len,
        lock_type,
    };
    let woken = {
        let mut table = TABLE.lock();
        if table.try_set(key, &want)? {
            table.take_waiters_on(key)
        } else {
            Vec::new()
        }
    };
    crate::ipc::waiters::wake_all(woken);
    Ok(())
}

/// Take a record lock, waiting while another owner holds a conflicting one:
/// `F_SETLKW`.
///
/// # Errors
///
/// - `Deadlock` when an owner in the way is waiting, directly or through
///   others, on `owner`: sleeping would never end (see the module doc).
/// - `Interrupted` when a deliverable signal arrives during the wait. The
///   syscall layers turn it into a restart under `SA_RESTART`, else `EINTR`.
///   Kernel tasks have no signals and wait uninterruptibly.
/// - `ResourceExhausted` when the table, or the list of waiters, is full.
pub fn set_wait(
    key: &LockKey,
    owner: u64,
    start: u64,
    len: u64,
    lock_type: RecordLockType,
) -> KernelResult<()> {
    use crate::ipc::waiters;

    let want = RecordLock {
        owner,
        start,
        len,
        lock_type,
    };
    let task = crate::sched::current_task_id();
    let pid = waiters::current_user_pid();
    loop {
        let granted = {
            let mut table = TABLE.lock();
            // At the top of every pass, under the lock: a wake that found this
            // task has taken its entry already, and no way out of the loop may
            // leave one naming a task that is no longer here.
            table.remove_waiter(task);
            match table.try_set(key, &want) {
                Ok(replaced) => Some(if replaced {
                    table.take_waiters_on(key)
                } else {
                    Vec::new()
                }),
                Err(KernelError::WouldBlock) => {
                    // A signal ends a wait, not an acquisition: a free range
                    // is taken even with one pending, as on Linux.
                    if waiters::deliverable_signal_pending(pid) {
                        return Err(KernelError::Interrupted);
                    }
                    let blocked_on = table.blockers(key, &want);
                    if table.would_deadlock(owner, &blocked_on) {
                        return Err(KernelError::Deadlock);
                    }
                    if table.waiters.len() >= MAX_WAITERS {
                        return Err(KernelError::ResourceExhausted);
                    }
                    table.waiters.push(Waiter {
                        task,
                        pid,
                        owner,
                        key: key.clone(),
                        blocked_on,
                    });
                    None
                }
                Err(e) => return Err(e),
            }
        };
        match granted {
            Some(woken) => {
                waiters::wake_all(woken);
                return Ok(());
            }
            // A release between the registration above and this park is not
            // lost: its wake finds the task not yet blocked and leaves a
            // pending wake, which this park consumes (`sched::wake`).
            None => waiters::park_interruptible(pid, task),
        }
    }
}

/// Release `[start, len)` for one owner, splitting any lock the cut divides.
///
/// Releasing what is not held is not an error: POSIX lets a process clear a
/// range it does not hold.
pub fn unlock(key: &LockKey, owner: u64, start: u64, len: u64) {
    let end = if len == 0 {
        u64::MAX
    } else {
        start.saturating_add(len)
    };
    let woken = {
        let mut table = TABLE.lock();
        if table.unlock(key, owner, start, end) {
            table.take_waiters_on(key)
        } else {
            Vec::new()
        }
    };
    crate::ipc::waiters::wake_all(woken);
}

/// Does `owner` hold a record lock on any file?
///
/// Free while nothing is locked anywhere, which is the usual state; otherwise
/// a scan of the table. The close path asks this before resolving the closed
/// handle's file, which costs a filesystem lookup.
#[must_use]
pub fn holds_any(owner: u64) -> bool {
    if LOCKED_FILES.load(Ordering::Acquire) == 0 {
        return false;
    }
    TABLE
        .lock()
        .files
        .iter()
        .any(|f| f.locks.iter().any(|l| l.owner == owner))
}

/// Drop every lock one owner holds on one file.
///
/// POSIX's rule for closing a descriptor: when a process closes ANY descriptor
/// for a file, all of its locks on that file go, whichever descriptor took
/// them. It is a famous wart, and programs rely on it.
pub fn release_on(key: &LockKey, owner: u64) {
    unlock(key, owner, 0, 0);
}

/// The first lock that would block this request, or `None` if it would succeed.
///
/// This is `F_GETLK`, and it returns the CONFLICTING LOCK rather than a
/// boolean, because the caller has to report the holder's pid and range. A
/// boolean answer would be the same shape as a stub that always says "free".
#[must_use]
pub fn query(
    key: &LockKey,
    owner: u64,
    start: u64,
    len: u64,
    lock_type: RecordLockType,
) -> Option<RecordLock> {
    let want = RecordLock {
        owner,
        start,
        len,
        lock_type,
    };
    let table = TABLE.lock();
    let file = table.files.iter().find(|f| f.key.same_file(key))?;
    file.locks.iter().find(|h| h.conflicts_with(&want)).cloned()
}

/// The first of `owner`'s own locks that overlaps `[start, len)`.
///
/// Linux's `F_OFD_GETLK` with `l_type == F_UNLCK`: "which of this open file
/// description's locks cover this range?" (`posix_test_locks_conflict`).
#[must_use]
pub fn query_own(key: &LockKey, owner: u64, start: u64, len: u64) -> Option<RecordLock> {
    let probe = RecordLock {
        owner,
        start,
        len,
        lock_type: RecordLockType::Read,
    };
    let table = TABLE.lock();
    let file = table.files.iter().find(|f| f.key.same_file(key))?;
    file.locks
        .iter()
        .find(|h| h.owner == owner && h.overlaps(&probe))
        .cloned()
}

/// Tag bit distinguishing an OFD lock owner from a POSIX one.
///
/// This module owns the `owner: u64` space, so it owns the encoding. The bit
/// started life as a constant in `syscall/linux.rs`, which would have meant
/// `fs::handle` -- the module that has to RELEASE these locks -- agreeing about
/// it by hand. An owner space with two authors is one that will eventually
/// disagree, and the disagreement is silent: `conflicts_with` compares owners
/// for equality, so a mismatched tag makes a lock invisible to its own holder.
const OFD_OWNER_TAG: u64 = 1 << 63;

/// With [`OFD_OWNER_TAG`]: the open file description is a memfd.
///
/// Memfd ids and file handles are two counters that both start at 1. Without
/// this bit, memfd 5's OFD locks and file handle 5's were one owner, and the
/// final close of either released the other's.
const MEMFD_OWNER_TAG: u64 = 1 << 62;

/// The owner value for a lock held by a file's **open file description**
/// (`F_OFD_*`), by its handle.
#[must_use]
pub fn ofd_owner(handle: u64) -> u64 {
    (handle & !MEMFD_OWNER_TAG) | OFD_OWNER_TAG
}

/// The owner value for a lock held by a memfd's open file description. A
/// memfd is one description: `dup` and `fork` share it, offset included.
#[must_use]
pub fn memfd_ofd_owner(id: u64) -> u64 {
    id | OFD_OWNER_TAG | MEMFD_OWNER_TAG
}

/// The owner value for a lock held by a **process** (plain `F_SETLK`).
///
/// Masked rather than passed through: a pid with bit 63 set would otherwise
/// impersonate an open file description.
#[must_use]
pub fn posix_owner(pid: u64) -> u64 {
    pid & !OFD_OWNER_TAG
}

/// Does this owner identify an open file description rather than a process?
///
/// Used to report `l_pid = -1` from `F_GETLK`, which is what POSIX requires
/// when the holder is an OFD lock: an open file description has no pid.
#[must_use]
pub fn owner_is_ofd(owner: u64) -> bool {
    owner & OFD_OWNER_TAG != 0
}

/// Release every record lock held by one open file description.
///
/// Called from [`crate::fs::handle::close`] on the **final** close of a
/// description, which is exactly when an OFD lock ends -- that is the whole
/// definition of an OFD lock, as against a POSIX one which ends when the
/// process does. Without this a holder that dies wedges the range until
/// reboot, which is the property that makes byte-range locking safe to rely
/// on rather than merely present.
pub fn release_ofd(handle: u64) {
    release_all(ofd_owner(handle));
}

/// [`release_ofd`] for a memfd: called by `memfd::close` when the last
/// reference to the memfd goes.
pub fn release_memfd_ofd(id: u64) {
    release_all(memfd_ofd_owner(id));
}

/// Drop every lock held by one owner, on every file, waking whoever waited
/// on them.
pub fn release_all(owner: u64) {
    let woken = TABLE.lock().release_everywhere(owner);
    crate::ipc::waiters::wake_all(woken);
}

/// A process has exited: its POSIX locks go, and so does any waiter entry
/// one of its tasks left behind.
///
/// A task leaves its entry itself on every way out of [`set_wait`], so this
/// only matters for one torn down while parked. Its entry would otherwise
/// name a task that no longer exists, and feed the deadlock search an edge
/// from a process that no longer waits.
pub fn release_process(pid: u64) {
    let woken = {
        let mut table = TABLE.lock();
        if pid != 0 {
            table.waiters.retain(|w| w.pid != pid);
        }
        table.release_everywhere(posix_owner(pid))
    };
    crate::ipc::waiters::wake_all(woken);
}

/// Locks currently held on a file, for tests and `/proc`.
#[must_use]
pub fn list(key: &LockKey) -> Vec<RecordLock> {
    let table = TABLE.lock();
    table
        .files
        .iter()
        .find(|f| f.key.same_file(key))
        .map_or_else(Vec::new, |f| f.locks.clone())
}

/// How many tasks are waiting on `key`, for the self-tests.
fn waiting_on(key: &LockKey) -> usize {
    TABLE
        .lock()
        .waiters
        .iter()
        .filter(|w| w.key.same_file(key))
        .count()
}

/// A record lock taken under one name must be visible under another.
///
/// **The only rung here that exercises identity keying.** The others use
/// synthetic paths that do not exist, so `file_identity` returns `NotFound`,
/// the key falls back to the name, and they pass exactly as they did before
/// the 2026-09-21 conversion -- no evidence for it at all.
/// Otherwise two writers each believe they hold the same byte range.
fn test_record_lock_follows_the_file() -> KernelResult<()> {
    use crate::fs::Vfs;
    const A: &[u8] = b"/tmp/reclock-id-a";
    const B: &[u8] = b"/tmp/reclock-id-b";

    let _ = Vfs::remove(Path::new(A));
    let _ = Vfs::remove(Path::new(B));
    Vfs::write_file(Path::new(A), b"x")?;
    match crate::fs::selftest::classify(Vfs::link(Path::new(A), Path::new(B))) {
        crate::fs::selftest::Setup::Ready => {}
        // Only NotSupported/ReadOnlyFilesystem/NoSuchDevice reach here.
        crate::fs::selftest::Setup::Unsupported(e) => {
            crate::serial_println!(
                "reclock: identity rung SKIPPED -- link() unsupported here: {:?}",
                e
            );
            let _ = Vfs::remove(Path::new(A));
            return Ok(());
        }
        // The system was ASKED and REFUSED. Reporting that as 'no hard
        // links here' would announce a cause never established.
        crate::fs::selftest::Setup::Failed(e) => {
            crate::serial_println!("reclock: FAIL: link() refused with {:?}, which is not", e);
            crate::serial_println!("reclock:       'this system cannot'");
            let _ = Vfs::remove(Path::new(A));
            return Err(e);
        }
    }
    let (ida, idb) = (
        Vfs::file_identity(Path::new(A))?,
        Vfs::file_identity(Path::new(B))?,
    );
    if ida.is_none() || ida != idb {
        crate::serial_println!("reclock: identity rung SKIPPED -- {:?} vs {:?}", ida, idb);
        let _ = Vfs::remove(Path::new(B));
        let _ = Vfs::remove(Path::new(A));
        return Ok(());
    }

    let key_a = LockKey::for_path(Path::new(A));
    set(&key_a, 1, 0, 16, RecordLockType::Write)?;
    let seen = list(&LockKey::for_path(Path::new(B)));
    // NEGATIVE CONTROL: an unrelated real file must report no locks. Without
    // it, a matcher that matched anything would satisfy the assertion below
    // while proving nothing about identity (dd-954).
    const C: &[u8] = b"/tmp/reclock-id-c";
    let _ = Vfs::remove(Path::new(C));
    let unrelated = match Vfs::write_file(Path::new(C), b"x") {
        Ok(()) => {
            let l = list(&LockKey::for_path(Path::new(C)));
            let _ = Vfs::remove(Path::new(C));
            l
        }
        Err(_) => Vec::new(),
    };
    unlock(&key_a, 1, 0, 16);
    let _ = Vfs::remove(Path::new(B));
    let _ = Vfs::remove(Path::new(A));
    if !unrelated.is_empty() {
        crate::serial_println!(
            "reclock: ERROR: control failed -- an UNRELATED file reports A's lock"
        );
        return Err(KernelError::InternalError);
    }
    if seen.is_empty() {
        crate::serial_println!("reclock: FAIL -- a lock set on one name is invisible on another");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("reclock: identity rung OK -- a record lock follows the file");
    Ok(())
}

/// The synthetic file the waiting rungs lock. It names nothing, so the key
/// is the name: the rungs are about waiting, not identity.
const WAIT_TEST_PATH: &str = "/reclock-selftest-wait";

/// What the waiting rungs' tasks returned, by slot: 0 while still waiting,
/// 1 for `Ok`, `0x100 | -code` for an error.
static WAIT_RESULT: [core::sync::atomic::AtomicU64; 2] = [
    core::sync::atomic::AtomicU64::new(0),
    core::sync::atomic::AtomicU64::new(0),
];

/// The argument for [`wait_task`]: result slot, owner, range, and type, each
/// small enough for its field (the rungs' own constants).
fn wait_arg(slot: u64, owner: u64, start: u64, len: u64, write: bool) -> u64 {
    slot.wrapping_shl(48)
        | owner.wrapping_shl(32)
        | start.wrapping_shl(16)
        | len.wrapping_shl(1)
        | u64::from(write)
}

/// The result slot a [`wait_arg`] names.
fn wait_slot(arg: u64) -> usize {
    usize::from(arg.wrapping_shr(48) & 1 != 0)
}

/// [`set_wait`] on [`WAIT_TEST_PATH`] in a task of its own, recording what it
/// returned in its [`WAIT_RESULT`] slot.
extern "C" fn wait_task(arg: u64) {
    let slot = wait_slot(arg);
    let owner = arg.wrapping_shr(32) & 0xFFFF;
    let start = arg.wrapping_shr(16) & 0xFFFF;
    let len = arg.wrapping_shr(1) & 0x7FFF;
    let lock_type = if arg & 1 != 0 {
        RecordLockType::Write
    } else {
        RecordLockType::Read
    };
    let got = match set_wait(
        &LockKey::for_path(WAIT_TEST_PATH),
        owner,
        start,
        len,
        lock_type,
    ) {
        Ok(()) => 1,
        Err(e) => 0x100 | u64::from(e.code().unsigned_abs()),
    };
    if let Some(r) = WAIT_RESULT.get(slot) {
        r.store(got, core::sync::atomic::Ordering::SeqCst);
    }
}

/// Start [`wait_task`] and give it time to park.
fn spawn_waiter(arg: u64) -> KernelResult<()> {
    if let Some(r) = WAIT_RESULT.get(wait_slot(arg)) {
        r.store(0, core::sync::atomic::Ordering::SeqCst);
    }
    crate::sched::spawn(b"reclock-wait", 16, wait_task, arg, 0)?;
    crate::sched::sleep_ns_interruptible(20_000_000);
    Ok(())
}

/// The value in a [`WAIT_RESULT`] slot once it is set, or 0 after about a
/// second without one.
fn await_wait_result(slot: usize) -> u64 {
    let deadline = crate::hrtimer::now_ns().saturating_add(1_000_000_000);
    loop {
        let got = WAIT_RESULT
            .get(slot)
            .map_or(0, |r| r.load(core::sync::atomic::Ordering::SeqCst));
        if got != 0 || crate::hrtimer::now_ns() >= deadline {
            return got;
        }
        crate::sched::yield_now();
    }
}

/// `F_SETLKW` waits, wakes, and refuses a deadlock (rungs 10 to 12).
///
/// Every outcome is read before anything is judged, and both owners are
/// released first, so a failing rung still frees its parked tasks rather
/// than leaving them waiting for the rest of the boot.
fn test_waiting(a: u64, b: u64) -> KernelResult<()> {
    let key = LockKey::for_path(WAIT_TEST_PATH);
    let ok = 1;
    let deadlock = 0x100 | u64::from(KernelError::Deadlock.code().unsigned_abs());

    // 10. A waiter parks behind a write lock, and the unlock wakes it into
    //     the lock. Parked means registered and not yet returned, checked
    //     before the unlock, so a wait that returned at once fails here.
    set(&key, a, 0, 10, RecordLockType::Write)?;
    spawn_waiter(wait_arg(0, b, 5, 1, true))?;
    let parked = waiting_on(&key) == 1 && await_nothing(0);
    unlock(&key, a, 0, 10);
    let woke = await_wait_result(0);
    let holds = list(&key).iter().any(|h| h.owner == b && h.start == 5);
    release_all(a);
    release_all(b);
    if !parked || woke != ok || !holds {
        crate::serial_println!(
            "reclock:   FAIL: F_SETLKW: parked {}, returned {:#x}, holds {}",
            parked,
            woke,
            holds
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("  [10/12] F_SETLKW parks, and the unlock wakes it into the lock: OK");

    // 11. A downgrade frees a reader: the writer replacing its own write lock
    //     with a read lock is a change that must wake.
    set(&key, a, 0, 10, RecordLockType::Write)?;
    spawn_waiter(wait_arg(0, b, 0, 10, false))?;
    let parked = waiting_on(&key) == 1 && await_nothing(0);
    set(&key, a, 0, 10, RecordLockType::Read)?;
    let woke = await_wait_result(0);
    release_all(a);
    release_all(b);
    if !parked || woke != ok {
        crate::serial_println!(
            "reclock:   FAIL: a downgrade did not free a waiting reader (parked {}, returned {:#x})",
            parked,
            woke
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("  [11/12] a downgrade wakes a waiting reader: OK");

    // 12. Deadlock. A holds [0,10), B holds [100,110); B waits for A's range.
    //     A asking for B's range would sleep forever, so it is refused at
    //     once -- in a task of its own, so a broken check parks that task
    //     rather than the boot. Then A's release lets B in.
    set(&key, a, 0, 10, RecordLockType::Write)?;
    set(&key, b, 100, 10, RecordLockType::Write)?;
    spawn_waiter(wait_arg(0, b, 0, 10, true))?;
    let b_parked = waiting_on(&key) == 1 && await_nothing(0);
    spawn_waiter(wait_arg(1, a, 100, 10, true))?;
    let a_got = await_wait_result(1);
    release_all(a);
    let b_got = await_wait_result(0);
    release_all(b);
    // An A that parked instead of refusing is woken by B's release just above
    // and takes B's range: read its slot once more, so the rung does not
    // outlive it, and release what it took.
    let a_late = await_wait_result(1);
    release_all(a);
    if !b_parked || a_got != deadlock || b_got != ok {
        crate::serial_println!(
            "reclock:   FAIL: deadlock: B parked {}, A returned {:#x} (want {:#x}, later {:#x}), B {:#x}",
            b_parked,
            a_got,
            deadlock,
            a_late,
            b_got
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("  [12/12] a request that would deadlock is refused, not parked: OK");
    Ok(())
}

/// True when slot `slot` is still empty: its task has not returned.
fn await_nothing(slot: usize) -> bool {
    WAIT_RESULT
        .get(slot)
        .is_some_and(|r| r.load(core::sync::atomic::Ordering::SeqCst) == 0)
}

/// Exercise the cases a naive implementation passes and a correct one must.
///
/// Residue-free: the table is global, so every case uses paths this test owns
/// and `release_all` clears both owners at the end. A self-test that left locks
/// behind would make `/proc` report holders that are not there -- the same
/// failure this module exists to stop.
pub fn self_test() -> KernelResult<()> {
    test_record_lock_follows_the_file()?;

    const A: u64 = 9001;
    const B: u64 = 9002;
    let p = LockKey::for_path("/reclock-selftest");
    let p = &p;
    release_all(A);
    release_all(B);

    crate::serial_println!("reclock::self_test() -- running tests...");

    // 1. THE CASE THAT JUSTIFIES THE WHOLE MODULE. Two writers on DISJOINT
    //    ranges must both succeed. Pointing F_SETLK at the whole-file flock
    //    table would refuse the second, turning the stub's false success into
    //    a false failure and breaking programs that lock disjoint ranges
    //    correctly -- which is the normal case for a database or an index.
    set(p, A, 0, 100, RecordLockType::Write)?;
    if set(p, B, 200, 100, RecordLockType::Write).is_err() {
        crate::serial_println!("reclock:   FAIL: two writers on disjoint ranges conflicted");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("  [1/12] disjoint writers do not conflict: OK");

    // 2. Overlapping, one writing: refused.
    if set(p, B, 50, 100, RecordLockType::Write) != Err(KernelError::WouldBlock) {
        crate::serial_println!("reclock:   FAIL: an overlapping write was granted");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("  [2/12] overlapping write refused: OK");

    // 3. Read locks share.
    release_all(A);
    release_all(B);
    set(p, A, 0, 100, RecordLockType::Read)?;
    if set(p, B, 0, 100, RecordLockType::Read).is_err() {
        crate::serial_println!("reclock:   FAIL: two read locks conflicted");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("  [3/12] read locks share: OK");

    // 4. THE SPLIT. Unlocking the middle of a held range must leave BOTH ends
    //    held. A version that dropped the whole lock passes every test that
    //    only unlocks what it locked, and silently releases bytes the caller
    //    asked to keep.
    release_all(A);
    release_all(B);
    set(p, A, 0, 100, RecordLockType::Write)?;
    unlock(p, A, 40, 10);
    let held = list(p);
    if held.len() != 2 {
        crate::serial_println!(
            "reclock:   FAIL: interior unlock left {} lock(s), want 2",
            held.len()
        );
        return Err(KernelError::IoError);
    }
    let lo = held.first().ok_or(KernelError::InternalError)?;
    let hi = held.get(1).ok_or(KernelError::InternalError)?;
    if lo.start != 0 || lo.len != 40 || hi.start != 50 || hi.len != 50 {
        crate::serial_println!(
            "reclock:   FAIL: split gave [{},{}) and [{},{}), want [0,40) and [50,100)",
            lo.start,
            lo.end(),
            hi.start,
            hi.end()
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("  [4/12] interior unlock splits, both ends held: OK");

    // 5. The hole is genuinely free: another owner can take it.
    if set(p, B, 40, 10, RecordLockType::Write).is_err() {
        crate::serial_println!("reclock:   FAIL: the unlocked hole was still held");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("  [5/12] the hole is takeable: OK");

    // 6. len == 0 means to END OF FILE, not an empty range. A lock at [1000,0)
    //    must block a far-away write; the natural mistake makes it cover
    //    nothing and conflict with no one.
    release_all(A);
    release_all(B);
    set(p, A, 1000, 0, RecordLockType::Write)?;
    if set(p, B, 1_000_000, 1, RecordLockType::Write) != Err(KernelError::WouldBlock) {
        crate::serial_println!(
            "reclock:   FAIL: len==0 did not extend to EOF, so a far write was granted"
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("  [6/12] len==0 reaches EOF: OK");

    // 7. F_GETLK names the holder rather than answering a boolean -- a boolean
    //    is the same shape as the stub that always said the range was free.
    let who = query(p, B, 1_000_000, 1, RecordLockType::Write).ok_or(KernelError::IoError)?;
    if who.owner != A {
        crate::serial_println!(
            "reclock:   FAIL: query named owner {}, want {}",
            who.owner,
            A
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("  [7/12] query names the conflicting holder: OK");

    // 8. Touching locks of one type MERGE, so F_GETLK reports the whole range
    //    held and not the last piece added. Two pieces of [0,10) and [10,20)
    //    must come back as one [0,20); a read lock touching them must not
    //    join them, and unlocking the middle must split the merged lock.
    release_all(A);
    release_all(B);
    set(p, A, 10, 10, RecordLockType::Write)?;
    set(p, A, 0, 10, RecordLockType::Write)?;
    set(p, A, 20, 10, RecordLockType::Read)?;
    let held = list(p);
    let merged = held
        .iter()
        .any(|h| h.start == 0 && h.len == 20 && h.lock_type == RecordLockType::Write);
    let reported = query(p, B, 15, 1, RecordLockType::Read).map(|h| (h.start, h.len));
    unlock(p, A, 5, 10);
    let split = list(p).len();
    release_all(A);
    if held.len() != 2 || !merged || reported != Some((0, 20)) || split != 3 {
        crate::serial_println!(
            "reclock:   FAIL: merge: {} lock(s) {:?}, F_GETLK saw {:?}, {} after the cut (want 2, [0,20) W, Some((0, 20)), 3)",
            held.len(),
            held,
            reported,
            split
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("  [8/12] touching locks of one type merge: OK");

    // 9. A memfd is its own key: a lock on a memfd neither sees a lock on a
    //    file nor is seen as one, and two owners on the memfd do conflict.
    //    An id far above any the memfd counter has reached, so no real
    //    memfd's locks can be in the way.
    let m = LockKey::for_memfd(u64::MAX >> 2);
    set(p, A, 0, 0, RecordLockType::Write)?;
    let beside_file = set(&m, B, 0, 0, RecordLockType::Write);
    let second_owner = set(&m, A, 0, 1, RecordLockType::Write);
    let on_memfd = list(&m).len();
    release_all(A);
    release_all(B);
    if beside_file.is_err() || second_owner != Err(KernelError::WouldBlock) || on_memfd != 1 {
        crate::serial_println!(
            "reclock:   FAIL: memfd key: beside a file {:?}, a second owner {:?}, {} lock(s)",
            beside_file,
            second_owner,
            on_memfd
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("  [9/12] a memfd is a key of its own: OK");

    test_waiting(A, B)?;

    release_all(A);
    release_all(B);
    if !list(p).is_empty()
        || !list(&m).is_empty()
        || !list(&LockKey::for_path(WAIT_TEST_PATH)).is_empty()
    {
        crate::serial_println!("reclock:   FAIL: the test left locks behind");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("reclock::self_test() -- all 12 tests passed, table clean");
    Ok(())
}
