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

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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
        if self.read { s.push('r'); }
        if self.write { s.push('w'); }
        if self.append { s.push('a'); }
        if self.nonblock { s.push('n'); }
        if self.cloexec { s.push('e'); }
        if s.is_empty() { s.push('-'); }
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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    let default_flags = FdFlags { read: true, write: true, append: false, nonblock: false, cloexec: false };
    *guard = Some(State {
        tables: alloc::vec![
            ProcessFdTable {
                pid: 1, next_fd: 4, max_fds: DEFAULT_MAX_FDS,
                entries: alloc::vec![
                    FdEntry { fd: 0, fd_type: FdType::Device, path: String::from("/dev/console"), flags: FdFlags { read: true, write: false, append: false, nonblock: false, cloexec: false }, offset: 0, ref_count: 1, opened_ns: now },
                    FdEntry { fd: 1, fd_type: FdType::Device, path: String::from("/dev/console"), flags: FdFlags { read: false, write: true, append: false, nonblock: false, cloexec: false }, offset: 0, ref_count: 1, opened_ns: now },
                    FdEntry { fd: 2, fd_type: FdType::Device, path: String::from("/dev/console"), flags: FdFlags { read: false, write: true, append: false, nonblock: false, cloexec: false }, offset: 0, ref_count: 1, opened_ns: now },
                    FdEntry { fd: 3, fd_type: FdType::RegularFile, path: String::from("/etc/init.conf"), flags: FdFlags { read: true, write: false, append: false, nonblock: false, cloexec: true }, offset: 0, ref_count: 1, opened_ns: now },
                ],
            },
            ProcessFdTable {
                pid: 100, next_fd: 5, max_fds: DEFAULT_MAX_FDS,
                entries: alloc::vec![
                    FdEntry { fd: 0, fd_type: FdType::Pipe, path: String::from("pipe:[1234]"), flags: FdFlags { read: true, write: false, append: false, nonblock: false, cloexec: false }, offset: 0, ref_count: 1, opened_ns: now },
                    FdEntry { fd: 1, fd_type: FdType::Pipe, path: String::from("pipe:[1234]"), flags: FdFlags { read: false, write: true, append: false, nonblock: false, cloexec: false }, offset: 0, ref_count: 1, opened_ns: now },
                    FdEntry { fd: 2, fd_type: FdType::Device, path: String::from("/dev/null"), flags: default_flags, offset: 0, ref_count: 1, opened_ns: now },
                    FdEntry { fd: 3, fd_type: FdType::Socket, path: String::from("socket:[5678]"), flags: FdFlags { read: true, write: true, append: false, nonblock: true, cloexec: true }, offset: 0, ref_count: 1, opened_ns: now },
                    FdEntry { fd: 4, fd_type: FdType::RegularFile, path: String::from("/var/log/sshd.log"), flags: FdFlags { read: false, write: true, append: true, nonblock: false, cloexec: true }, offset: 2048, ref_count: 1, opened_ns: now },
                ],
            },
        ],
        total_opens: 9,
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
                if state.tables.len() >= MAX_PROCESSES { return Err(KernelError::ResourceExhausted); }
                state.tables.push(ProcessFdTable {
                    pid, entries: Vec::new(), next_fd: 0, max_fds: DEFAULT_MAX_FDS,
                });
                state.tables.last_mut().ok_or(KernelError::InternalError)?
            }
        };
        if table.entries.len() as u32 >= table.max_fds { return Err(KernelError::TooManyOpenFiles); }
        let fd = table.next_fd;
        table.next_fd += 1;
        let now = crate::hpet::elapsed_ns();
        table.entries.push(FdEntry {
            fd, fd_type, path: String::from(path), flags, offset: 0, ref_count: 1, opened_ns: now,
        });
        state.total_opens += 1;
        Ok(fd)
    })
}

/// Close a file descriptor.
pub fn close(pid: u32, fd: u32) -> KernelResult<()> {
    with_state(|state| {
        let table = state.tables.iter_mut().find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        let before = table.entries.len();
        table.entries.retain(|e| e.fd != fd);
        if table.entries.len() == before { return Err(KernelError::NotFound); }
        state.total_closes += 1;
        Ok(())
    })
}

/// Duplicate a file descriptor.
pub fn dup(pid: u32, old_fd: u32) -> KernelResult<u32> {
    with_state(|state| {
        let table = state.tables.iter_mut().find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        let entry = table.entries.iter().find(|e| e.fd == old_fd)
            .ok_or(KernelError::NotFound)?
            .clone();
        if table.entries.len() as u32 >= table.max_fds { return Err(KernelError::TooManyOpenFiles); }
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
        s.tables.iter().find(|t| t.pid == pid).map_or(Vec::new(), |t| t.entries.clone())
    })
}

/// Get a specific FD.
pub fn get(pid: u32, fd: u32) -> Option<FdEntry> {
    STATE.lock().as_ref().and_then(|s| {
        s.tables.iter().find(|t| t.pid == pid).and_then(|t| t.entries.iter().find(|e| e.fd == fd).cloned())
    })
}

/// Set FD limit for a process.
pub fn set_max_fds(pid: u32, max: u32) -> KernelResult<()> {
    with_state(|state| {
        let table = state.tables.iter_mut().find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;
        table.max_fds = max;
        Ok(())
    })
}

/// List all process FD tables (summary).
pub fn list_tables() -> Vec<(u32, usize, u32)> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.tables.iter().map(|t| (t.pid, t.entries.len(), t.max_fds)).collect()
    })
}

/// Statistics: (table_count, total_opens, total_closes, total_dups, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.tables.len(), s.total_opens, s.total_closes, s.total_dups, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("fdtable::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    let tables = list_tables();
    assert_eq!(tables.len(), 2);
    assert_eq!(list(1).len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Open.
    let flags = FdFlags { read: true, write: false, append: false, nonblock: false, cloexec: false };
    let fd = open(1, "/tmp/test.txt", FdType::RegularFile, flags).expect("open");
    assert_eq!(fd, 4);
    assert_eq!(list(1).len(), 5);
    crate::serial_println!("  [2/8] open: OK");

    // 3: Close.
    close(1, fd).expect("close");
    assert_eq!(list(1).len(), 4);
    assert!(close(1, 999).is_err());
    crate::serial_println!("  [3/8] close: OK");

    // 4: Dup.
    let new_fd = dup(100, 3).expect("dup");
    assert!(new_fd >= 5);
    let entry = get(100, new_fd).expect("get");
    assert!(!entry.flags.cloexec); // dup clears cloexec.
    assert_eq!(entry.path, "socket:[5678]");
    crate::serial_println!("  [4/8] dup: OK");

    // 5: Auto-create table.
    let flags2 = FdFlags { read: true, write: true, append: false, nonblock: false, cloexec: false };
    let fd2 = open(999, "/tmp/new_proc.txt", FdType::RegularFile, flags2).expect("open2");
    assert_eq!(fd2, 0);
    assert_eq!(list_tables().len(), 3);
    crate::serial_println!("  [5/8] auto-create: OK");

    // 6: Get.
    let entry = get(1, 0).expect("get2");
    assert_eq!(entry.fd_type, FdType::Device);
    assert_eq!(entry.path, "/dev/console");
    crate::serial_println!("  [6/8] get: OK");

    // 7: FD limit.
    set_max_fds(999, 2).expect("limit");
    open(999, "/tmp/a", FdType::RegularFile, flags2).expect("open3");
    assert!(open(999, "/tmp/b", FdType::RegularFile, flags2).is_err());
    crate::serial_println!("  [7/8] fd limit: OK");

    // 8: Stats.
    let (tables, opens, closes, dups, ops) = stats();
    assert_eq!(tables, 3);
    assert!(opens >= 12);
    assert!(closes >= 1);
    assert!(dups >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("fdtable::self_test() — all 8 tests passed");
}
