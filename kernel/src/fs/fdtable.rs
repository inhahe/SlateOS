//! File Descriptor Table — per-process FD tracking.
//!
//! Manages file descriptor tables for processes. Tracks
//! open files, their types, permissions, and reference counts.
//! Supports dup, close-on-exec, and FD limits.
//!
//! ## Architecture
//!
//! ```text
//! FD table management
//!   → fdtable::open(pid, path, flags) → allocate FD
//!   → fdtable::close(pid, fd) → close FD
//!   → fdtable::dup(pid, old_fd) → duplicate FD
//!   → fdtable::list(pid) → list open FDs
//!
//! Integration:
//!   → procstat (process statistics)
//!   → vfs (virtual file system)
//!   → audit (audit logging)
//!   → ipclog (IPC logging)
//! ```

#![allow(dead_code)]

use crate::sync::Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// File descriptor type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdType {
    RegularFile,
    Directory,
    Pipe,
    Socket,
    Device,
    Epoll,
    Timer,
    Signal,
}

impl FdType {
    pub fn label(self) -> &'static str {
        match self {
            Self::RegularFile => "file",
            Self::Directory => "dir",
            Self::Pipe => "pipe",
            Self::Socket => "socket",
            Self::Device => "dev",
            Self::Epoll => "epoll",
            Self::Timer => "timer",
            Self::Signal => "signal",
        }
    }
}

/// Open file flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FdFlags {
    pub read: bool,
    pub write: bool,
    pub append: bool,
    pub nonblock: bool,
    pub cloexec: bool,
}

impl FdFlags {
    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.read {
            s.push('r');
        }
        if self.write {
            s.push('w');
        }
        if self.append {
            s.push('a');
        }
        if self.nonblock {
            s.push('n');
        }
        if self.cloexec {
            s.push('e');
        }
        if s.is_empty() {
            s.push('-');
        }
        s
    }
}

/// A file descriptor entry.
#[derive(Debug, Clone)]
pub struct FdEntry {
    pub fd: u32,
    pub fd_type: FdType,
    pub path: String,
    pub flags: FdFlags,
    pub offset: u64,
    pub ref_count: u32,
    pub opened_ns: u64,
}

/// Per-process FD table.
#[derive(Debug, Clone)]
pub struct ProcessFdTable {
    pub pid: u32,
    pub entries: Vec<FdEntry>,
    pub next_fd: u32,
    pub max_fds: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PROCESSES: usize = 256;
const DEFAULT_MAX_FDS: u32 = 1024;

struct State {
    tables: Vec<ProcessFdTable>,
    total_opens: u64,
    total_closes: u64,
    total_dups: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise an **empty** FD-table tracker.
///
/// Seeds NO process tables and zero totals.  FDs are tracked through
/// [`open`] / [`close`] / [`dup`] as the VFS/syscall layer services them; until
/// that wiring exists, `/proc/fdtable` and the `fdtable` kshell command report an
/// empty table rather than fabricated open files — the kernel's hard "never invent
/// data in procfs" rule.
///
/// (Previously this seeded two fabricated process FD tables — pid 1 with four FDs
/// (`/dev/console` ×3 as stdin/out/err plus `/etc/init.conf`) and pid 100 with
/// five (`pipe:[1234]` ×2, `/dev/null`, `socket:[5678]`, `/var/log/sshd.log`) —
/// plus an invented total_opens of 9, which `/proc/fdtable` and the `list_tables`
/// view then displayed as if they were real open file descriptors.  The
/// authoritative per-process FD table is the PCB's `linux_fd_table`
/// (`crate::proc::linux_fd::KernelFdTable`, used by the POSIX layer); none of
/// [`open`]/[`close`]/[`dup`]'s callers are real — the VFS does not call them — so
/// this parallel tracker is entirely unwired.  See the DEFERRED PROPER FIX note in
/// todo.txt for reading the aggregate view from the PCB.  The self-test now builds
/// its own fixtures via the real API — see [`self_test`].)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        tables: Vec::new(),
        total_opens: 0,
        total_closes: 0,
        total_dups: 0,
        ops: 0,
    });
}

/// Open a file descriptor.
pub fn open(pid: u32, path: &str, fd_type: FdType, flags: FdFlags) -> KernelResult<u32> {
    with_state(|state| {
        let table = state.tables.iter_mut().find(|t| t.pid == pid);
        let table = match table {
            Some(t) => t,
            None => {
                if state.tables.len() >= MAX_PROCESSES {
                    return Err(KernelError::ResourceExhausted);
                }
                state.tables.push(ProcessFdTable {
                    pid,
                    entries: Vec::new(),
                    next_fd: 0,
                    max_fds: DEFAULT_MAX_FDS,
                });
                state.tables.last_mut().ok_or(KernelError::InternalError)?
            }
        };
        if table.entries.len() as u32 >= table.max_fds {
            return Err(KernelError::TooManyOpenFiles);
        }
        let fd = table.next_fd;
        table.next_fd += 1;
        let now = crate::hpet::elapsed_ns();
        table.entries.push(FdEntry {
            fd,
            fd_type,
            path: String::from(path),
            flags,
            offset: 0,
            ref_count: 1,
            opened_ns: now,
        });
        state.total_opens += 1;
        Ok(fd)
    })
}

/// Close a file descriptor.
pub fn close(pid: u32, fd: u32) -> KernelResult<()> {
    with_state(|state| {
        let table = state
            .tables
            .iter_mut()
            .find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        let before = table.entries.len();
        table.entries.retain(|e| e.fd != fd);
        if table.entries.len() == before {
            return Err(KernelError::NotFound);
        }
        state.total_closes += 1;
        Ok(())
    })
}

/// Duplicate a file descriptor.
pub fn dup(pid: u32, old_fd: u32) -> KernelResult<u32> {
    with_state(|state| {
        let table = state
            .tables
            .iter_mut()
            .find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        let entry = table
            .entries
            .iter()
            .find(|e| e.fd == old_fd)
            .ok_or(KernelError::NotFound)?
            .clone();
        if table.entries.len() as u32 >= table.max_fds {
            return Err(KernelError::TooManyOpenFiles);
        }
        let new_fd = table.next_fd;
        table.next_fd += 1;
        let mut new_entry = entry;
        new_entry.fd = new_fd;
        new_entry.flags.cloexec = false; // dup clears cloexec.
        table.entries.push(new_entry);
        state.total_dups += 1;
        Ok(new_fd)
    })
}

/// List FDs for a process.
pub fn list(pid: u32) -> Vec<FdEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.tables
            .iter()
            .find(|t| t.pid == pid)
            .map_or(Vec::new(), |t| t.entries.clone())
    })
}

/// Get a specific FD.
pub fn get(pid: u32, fd: u32) -> Option<FdEntry> {
    STATE.lock().as_ref().and_then(|s| {
        s.tables
            .iter()
            .find(|t| t.pid == pid)
            .and_then(|t| t.entries.iter().find(|e| e.fd == fd).cloned())
    })
}

/// Tear down a process's whole FD table — what happens when the process exits.
///
/// Returns the number of descriptors that were still open, and counts every one
/// of them in `total_closes`, because they *were* closed: a teardown that did
/// not show up in the close tally would make the aggregate disagree with the
/// per-process history for no reason the reader could discover.
///
/// This exists because the module had no teardown path of any kind. `open`
/// pushes a new `ProcessFdTable` for any pid it has not seen, nothing ever
/// removes one, and `MAX_PROCESSES` caps the vector at 256 — so after 256
/// distinct pids had ever opened a single descriptor, every subsequent `open`
/// returned `ResourceExhausted` forever, whether or not any of those processes
/// still existed. Process exit is the most common event in an fd table's life
/// and it was the one operation the module could not represent.
///
/// `NotFound` when the pid has no table, so an exit reported twice is an error
/// rather than a silent success — the caller has lost track of the process
/// either way, and saying so is more useful than pretending.
pub fn close_all(pid: u32) -> KernelResult<usize> {
    with_state(|state| {
        let idx = state
            .tables
            .iter()
            .position(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        // `remove`, not `swap_remove`: `/proc/fdtable` and `fdtable list` print
        // the tables in vector order, and a process exiting should not silently
        // reorder the ones that did not.
        let table = state.tables.remove(idx);
        let n = table.entries.len();
        state.total_closes = state
            .total_closes
            .saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
        Ok(n)
    })
}

/// Set FD limit for a process.
pub fn set_max_fds(pid: u32, max: u32) -> KernelResult<()> {
    with_state(|state| {
        let table = state
            .tables
            .iter_mut()
            .find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        table.max_fds = max;
        Ok(())
    })
}

/// List all process FD tables (summary).
pub fn list_tables() -> Vec<(u32, usize, u32)> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.tables
            .iter()
            .map(|t| (t.pid, t.entries.len(), t.max_fds))
            .collect()
    })
}

/// Statistics: (table_count, total_opens, total_closes, total_dups, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.tables.len(),
            s.total_opens,
            s.total_closes,
            s.total_dups,
            s.ops,
        ),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("fdtable::self_test() — running tests...");
    // Start from a clean slate so the fixtures built below can never leak into
    // the live /proc/fdtable table (this self-test now runs at boot).
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no fabricated FD tables.
    assert_eq!(list_tables().len(), 0);
    assert_eq!(list(1).len(), 0);
    let (t0, o0, c0, d0, _ops0) = stats();
    assert_eq!((t0, o0, c0, d0), (0, 0, 0, 0));
    crate::serial_println!("  [1/9] empty defaults: OK");

    // 2: Open — auto-creates the process table; first fd is 0.
    let rdonly = FdFlags {
        read: true,
        write: false,
        append: false,
        nonblock: false,
        cloexec: false,
    };
    let fd = open(1, "/tmp/test.txt", FdType::RegularFile, rdonly).expect("open");
    assert_eq!(fd, 0);
    assert_eq!(list(1).len(), 1);
    assert_eq!(list_tables().len(), 1);
    crate::serial_println!("  [2/9] open: OK");

    // 3: Close — a second fd is allocated, then the first is closed.
    let fd1 = open(1, "/tmp/two.txt", FdType::RegularFile, rdonly).expect("open_b");
    assert_eq!(fd1, 1);
    close(1, fd).expect("close");
    assert_eq!(list(1).len(), 1);
    assert!(close(1, 999).is_err());
    crate::serial_println!("  [3/9] close: OK");

    // 4: Dup — clears cloexec and copies the path.
    let sock_flags = FdFlags {
        read: true,
        write: true,
        append: false,
        nonblock: true,
        cloexec: true,
    };
    let sock_fd = open(100, "socket:[5678]", FdType::Socket, sock_flags).expect("open_sock");
    let new_fd = dup(100, sock_fd).expect("dup");
    assert!(new_fd > sock_fd);
    let entry = get(100, new_fd).expect("get");
    assert!(!entry.flags.cloexec); // dup clears cloexec.
    assert_eq!(entry.path, "socket:[5678]");
    crate::serial_println!("  [4/9] dup: OK");

    // 5: Auto-create another table.
    let rw = FdFlags {
        read: true,
        write: true,
        append: false,
        nonblock: false,
        cloexec: false,
    };
    let fd2 = open(999, "/tmp/new_proc.txt", FdType::RegularFile, rw).expect("open2");
    assert_eq!(fd2, 0);
    assert_eq!(list_tables().len(), 3); // pid 1, 100, 999.
    crate::serial_println!("  [5/9] auto-create: OK");

    // 6: Get — type and path round-trip.
    let entry = get(1, fd1).expect("get2");
    assert_eq!(entry.fd_type, FdType::RegularFile);
    assert_eq!(entry.path, "/tmp/two.txt");
    crate::serial_println!("  [6/9] get: OK");

    // 7: FD limit — pid 999 holds 1 fd; cap at 2 admits one more, then errors.
    set_max_fds(999, 2).expect("limit");
    open(999, "/tmp/a", FdType::RegularFile, rw).expect("open3");
    assert!(open(999, "/tmp/b", FdType::RegularFile, rw).is_err());
    crate::serial_println!("  [7/9] fd limit: OK");

    // 8: Stats — exact totals (3 tables, 5 opens, 1 close, 1 dup).
    let (tables, opens, closes, dups, ops) = stats();
    assert_eq!(tables, 3);
    assert_eq!(opens, 5);
    assert_eq!(closes, 1);
    assert_eq!(dups, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/9] stats: OK");

    // 9: Process exit — the table goes away, its descriptors are counted as
    // closed, and the pid can then be opened again from scratch. Deliberately
    // after test 8 so the exact totals it asserts stay meaningful.
    assert_eq!(list(999).len(), 2);
    assert_eq!(close_all(999).expect("close_all"), 2);
    assert_eq!(list_tables().len(), 2); // pid 1 and 100 remain.
    assert_eq!(list(999).len(), 0);
    let (_, _, closes_after, _, _) = stats();
    assert_eq!(closes_after, closes + 2);
    // Twice is an error, not a silent success.
    assert!(close_all(999).is_err());
    // And the slot is genuinely reusable: `open` re-creates the table, and the
    // fd numbering starts over because `next_fd` went away with it.
    assert_eq!(
        open(999, "/tmp/again", FdType::RegularFile, rw).expect("reopen"),
        0
    );
    assert_eq!(close_all(999).expect("close_all2"), 1);
    crate::serial_println!("  [9/9] process exit: OK");

    // Leave the table EMPTY, not DEAD: clear the fixtures, then re-open it.
    // Clearing alone would switch this module off for the rest of the boot
    // -- `init_defaults` runs once, that once is here, and every later write
    // would take the `NotSupported` arm and be dropped by a caller that must
    // not let statistics fail a real operation.  known-issues.md:
    // A-FS-ACCOUNTING-TABLES-ARE-CLOSED-FOR-THE-WHOLE-BOOT.
    *STATE.lock() = None;
    init_defaults();

    crate::serial_println!("fdtable::self_test() — all 9 tests passed");
}
