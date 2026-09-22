//! POSIX byte-range advisory record locks — the kernel side of `fcntl`'s
//! `F_SETLK`, `F_SETLKW` and `F_GETLK`.
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
//! * Unlocking a range out of the middle of a held lock **splits** it, leaving
//!   the two ends held. Getting this wrong silently releases bytes the caller
//!   asked to keep, which no test of the common case would catch.
//!
//! Locks are advisory: they do not prevent I/O. Cooperating programs check.

use alloc::string::String;
use alloc::vec::Vec;

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
    /// Owning process id.
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
}

/// Every record lock held on one path.
struct PathEntry {
    /// Retained for `/proc` display and as the fallback key where the
    /// filesystem has no stable inode. No longer the primary key.
    path: String,
    /// Filesystem identity, when there is one. The real key: a record lock
    /// taken under one name must be seen under every other name for the
    /// same file, or two writers both believe they hold the range.
    id: Option<crate::fs::vfs::FileId>,
    locks: Vec<RecordLock>,
}

/// Does this entry describe the same file as `(path, id)`?
///
/// Identity wins when both sides have one; otherwise the path. One function
/// rather than four inline comparisons -- there are four entry points here,
/// and four hand-written comparisons is how one of them ends up different.
fn path_entry_matches(e: &PathEntry, path: &str, id: Option<crate::fs::vfs::FileId>) -> bool {
    match (e.id, id) {
        (Some(a), Some(b)) => a == b,
        _ => e.path == path,
    }
}

/// The record-lock table, keyed by resolved path.
///
/// Separate from `vfs::LOCK_TABLE` on purpose; see the module doc.
static TABLE: Mutex<Vec<PathEntry>> = Mutex::new(Vec::new());

/// Paths that may hold record locks at once.
const MAX_LOCKED_PATHS: usize = 1024;

/// Locks on one path. Bounded so a runaway caller cannot exhaust the heap.
const MAX_LOCKS_PER_PATH: usize = 256;

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
        out.push(RecordLock {
            owner: held.owner,
            start: h_start,
            len: start.saturating_sub(h_start),
            lock_type: held.lock_type,
        });
    }
    if end < h_end {
        // The tail keeps "to end of file" only if the original had it AND the
        // cut did not introduce a finite end.
        let len = if h_end == u64::MAX {
            0
        } else {
            h_end.saturating_sub(end)
        };
        out.push(RecordLock {
            owner: held.owner,
            start: end,
            len,
            lock_type: held.lock_type,
        });
    }
    out
}

/// Take a record lock, or report the first conflict.
///
/// Returns `Err(WouldBlock)` when another owner holds an incompatible
/// overlapping range — which is `F_SETLK`'s contract. A blocking `F_SETLKW`
/// belongs in the caller, which can retry; this layer never sleeps because it
/// holds the table lock.
pub fn set(
    path: &str,
    owner: u64,
    start: u64,
    len: u64,
    lock_type: RecordLockType,
) -> crate::error::KernelResult<()> {
    let want = RecordLock {
        owner,
        start,
        len,
        lock_type,
    };
    let id = crate::fs::Vfs::file_identity(path).unwrap_or(None);
    let mut table = TABLE.lock();
    let idx = match table.iter().position(|e| path_entry_matches(e, path, id)) {
        Some(i) => i,
        None => {
            if table.len() >= MAX_LOCKED_PATHS {
                return Err(crate::error::KernelError::OutOfMemory);
            }
            table.push(PathEntry {
                path: String::from(path),
                id,
                locks: Vec::new(),
            });
            table.len().saturating_sub(1)
        }
    };
    let entry = table
        .get_mut(idx)
        .ok_or(crate::error::KernelError::InternalError)?;

    if entry.locks.iter().any(|h| h.conflicts_with(&want)) {
        return Err(crate::error::KernelError::WouldBlock);
    }
    if entry.locks.len() >= MAX_LOCKS_PER_PATH {
        return Err(crate::error::KernelError::OutOfMemory);
    }

    // The caller's own overlapping coverage is replaced rather than added to,
    // so re-locking a range with a different type upgrades or downgrades it.
    let end = want.end();
    let mut kept: Vec<RecordLock> = Vec::new();
    for held in &entry.locks {
        if held.owner == owner {
            for piece in subtract(held, start, end) {
                kept.push(piece);
            }
        } else {
            kept.push(held.clone());
        }
    }
    kept.push(want);
    entry.locks = kept;
    Ok(())
}

/// Release `[start, len)` for one owner, splitting any lock the cut divides.
pub fn unlock(path: &str, owner: u64, start: u64, len: u64) -> crate::error::KernelResult<()> {
    let end = if len == 0 {
        u64::MAX
    } else {
        start.saturating_add(len)
    };
    let id = crate::fs::Vfs::file_identity(path).unwrap_or(None);
    let mut table = TABLE.lock();
    let Some(idx) = table.iter().position(|e| path_entry_matches(e, path, id)) else {
        // Unlocking a path with no locks is not an error: POSIX lets a process
        // clear a range it does not hold.
        return Ok(());
    };
    let entry = table
        .get_mut(idx)
        .ok_or(crate::error::KernelError::InternalError)?;
    let mut kept: Vec<RecordLock> = Vec::new();
    for held in &entry.locks {
        if held.owner == owner {
            for piece in subtract(held, start, end) {
                kept.push(piece);
            }
        } else {
            kept.push(held.clone());
        }
    }
    entry.locks = kept;
    if entry.locks.is_empty() {
        table.remove(idx);
    }
    Ok(())
}

/// The first lock that would block this request, or `None` if it would succeed.
///
/// This is `F_GETLK`, and it returns the CONFLICTING LOCK rather than a
/// boolean, because the caller has to report the holder's pid and range. A
/// boolean answer would be the same shape as a stub that always says "free".
#[must_use]
pub fn query(
    path: &str,
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
    let id = crate::fs::Vfs::file_identity(path).unwrap_or(None);
    let table = TABLE.lock();
    let entry = table.iter().find(|e| path_entry_matches(e, path, id))?;
    entry
        .locks
        .iter()
        .find(|h| h.conflicts_with(&want))
        .cloned()
}

/// Drop every lock held by one owner, on every path.
///
/// Called when a process exits. POSIX also drops a process's record locks when
/// it closes ANY descriptor for the file, which is a famous wart and belongs in
/// the close path rather than here.
pub fn release_all(owner: u64) {
    let mut table = TABLE.lock();
    for entry in table.iter_mut() {
        entry.locks.retain(|h| h.owner != owner);
    }
    table.retain(|e| !e.locks.is_empty());
}

/// Locks currently held on a path, for tests and `/proc`.
#[must_use]
pub fn list(path: &str) -> Vec<RecordLock> {
    // Its own resolution. This is the fourth entry point into the table and
    // the second time a missing one was caught by the compiler rather than by
    // my own assertions -- the checks verify text, not scope.
    let id = crate::fs::Vfs::file_identity(path).unwrap_or(None);
    let table = TABLE.lock();
    table
        .iter()
        .find(|e| path_entry_matches(e, path, id))
        .map_or_else(Vec::new, |e| e.locks.clone())
}

/// Exercise the cases a naive implementation passes and a correct one must.
///
/// Residue-free: the table is global, so every case uses paths this test owns
/// and `release_all` clears both owners at the end. A self-test that left locks
/// behind would make `/proc` report holders that are not there -- the same
/// failure this module exists to stop.
/// A record lock taken under one name must be visible under another.
///
/// **The only rung here that exercises identity keying.** The others use
/// synthetic paths that do not exist, so `file_identity` returns `NotFound`,
/// the key falls back to the name, and they pass exactly as they did before
/// the 2026-09-21 conversion -- no evidence for it at all.
/// Otherwise two writers each believe they hold the same byte range.
fn test_record_lock_follows_the_file() -> crate::error::KernelResult<()> {
    use super::path::Path;
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

    set(
        core::str::from_utf8(A).unwrap_or(""),
        1,
        0,
        16,
        RecordLockType::Write,
    )?;
    let seen = list(core::str::from_utf8(B).unwrap_or(""));
    // NEGATIVE CONTROL: an unrelated real file must report no locks. Without
    // it, a matcher that matched anything would satisfy the assertion below
    // while proving nothing about identity (dd-954).
    const C: &[u8] = b"/tmp/reclock-id-c";
    let _ = Vfs::remove(Path::new(C));
    let unrelated = match Vfs::write_file(Path::new(C), b"x") {
        Ok(()) => {
            let l = list(core::str::from_utf8(C).unwrap_or(""));
            let _ = Vfs::remove(Path::new(C));
            l
        }
        Err(_) => Vec::new(),
    };
    let _ = unlock(core::str::from_utf8(A).unwrap_or(""), 1, 0, 16);
    let _ = Vfs::remove(Path::new(B));
    let _ = Vfs::remove(Path::new(A));
    if !unrelated.is_empty() {
        crate::serial_println!(
            "reclock: ERROR: control failed -- an UNRELATED file reports A's lock"
        );
        return Err(crate::error::KernelError::InternalError);
    }
    if seen.is_empty() {
        crate::serial_println!("reclock: FAIL -- a lock set on one name is invisible on another");
        return Err(crate::error::KernelError::InternalError);
    }
    crate::serial_println!("reclock: identity rung OK -- a record lock follows the file");
    Ok(())
}
pub fn self_test() -> crate::error::KernelResult<()> {
    test_record_lock_follows_the_file()?;
    use crate::error::KernelError;

    const A: u64 = 9001;
    const B: u64 = 9002;
    let p = "/reclock-selftest";
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
    crate::serial_println!("  [1/7] disjoint writers do not conflict: OK");

    // 2. Overlapping, one writing: refused.
    if set(p, B, 50, 100, RecordLockType::Write) != Err(KernelError::WouldBlock) {
        crate::serial_println!("reclock:   FAIL: an overlapping write was granted");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("  [2/7] overlapping write refused: OK");

    // 3. Read locks share.
    release_all(A);
    release_all(B);
    set(p, A, 0, 100, RecordLockType::Read)?;
    if set(p, B, 0, 100, RecordLockType::Read).is_err() {
        crate::serial_println!("reclock:   FAIL: two read locks conflicted");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("  [3/7] read locks share: OK");

    // 4. THE SPLIT. Unlocking the middle of a held range must leave BOTH ends
    //    held. A version that dropped the whole lock passes every test that
    //    only unlocks what it locked, and silently releases bytes the caller
    //    asked to keep.
    release_all(A);
    release_all(B);
    set(p, A, 0, 100, RecordLockType::Write)?;
    unlock(p, A, 40, 10)?;
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
    crate::serial_println!("  [4/7] interior unlock splits, both ends held: OK");

    // 5. The hole is genuinely free: another owner can take it.
    if set(p, B, 40, 10, RecordLockType::Write).is_err() {
        crate::serial_println!("reclock:   FAIL: the unlocked hole was still held");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("  [5/7] the hole is takeable: OK");

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
    crate::serial_println!("  [6/7] len==0 reaches EOF: OK");

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
    crate::serial_println!("  [7/7] query names the conflicting holder: OK");

    release_all(A);
    release_all(B);
    if !list(p).is_empty() {
        crate::serial_println!("reclock:   FAIL: the test left locks behind");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("reclock::self_test() -- all 7 tests passed, table clean");
    Ok(())
}
