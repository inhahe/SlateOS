//! Userspace file descriptor table.
//!
//! POSIX unifies all I/O resources under integer file descriptors.
//! Our kernel uses separate handle namespaces: file handles (VFS),
//! pipe handles (IPC), and channel handles.  This module bridges
//! the gap by maintaining a mapping from POSIX fd numbers to the
//! underlying kernel handle + type.
//!
//! ## Design
//!
//! A static array of 256 `FdEntry` slots.  Each slot holds:
//! - The kernel handle value (u64)
//! - The handle kind (File, Pipe, Console, TcpStream, etc.)
//! - Per-fd flags (FD_CLOEXEC, etc.) — managed by `fcntl(F_GETFD/F_SETFD)`
//! - File status flags (O_RDONLY/O_WRONLY/O_RDWR, O_APPEND, O_NONBLOCK,
//!   O_SYNC) — managed by `fcntl(F_GETFL/F_SETFL)`.  Access mode bits
//!   are set at `open()` time and immutable; status flags can be changed.
//!
//! On startup, fds 0/1/2 are pre-initialized as Console handles so
//! that `read(0, ...)` and `write(1, ...)` work out of the box.
//!
//! ## Handle Sharing and Refcounting
//!
//! File handles have kernel-level duplication (`SYS_FS_DUP`), so
//! each dup'd fd gets an independent kernel handle.  Pipe and socket
//! handles do not have kernel-level dup, so `dup()` creates a new fd
//! entry pointing to the **same** kernel handle.
//!
//! [`is_handle_referenced()`] scans the table to determine whether
//! any other fd still uses a given handle.  `close()` calls this
//! after removing the fd entry: if the handle is still referenced,
//! the kernel close is skipped.  This O(256) scan is negligible
//! since `close()` is not a hot path.
//!
//! ## Thread Safety
//!
//! Uses `static mut` with single-threaded access.  When threading is
//! added, this must be replaced with proper synchronization (a mutex
//! or per-thread fd tables).

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of open file descriptors per process.
pub const MAX_FDS: usize = 256;

/// Close-on-exec flag for `fcntl(F_SETFD)`.
pub const FD_CLOEXEC: u32 = 1;

// ---------------------------------------------------------------------------
// Handle types
// ---------------------------------------------------------------------------

/// What kind of kernel resource an fd refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleKind {
    /// A VFS file handle (uses `SYS_FS_*` syscalls).
    File,
    /// A pipe handle (uses `SYS_PIPE_*` syscalls).
    Pipe,
    /// Console I/O (stdin/stdout/stderr, uses `SYS_CONSOLE_*`).
    Console,
    /// A connected TCP socket (uses `SYS_TCP_SEND`/`SYS_TCP_RECV`/`SYS_TCP_CLOSE`).
    TcpStream,
    /// A listening TCP socket (uses `SYS_TCP_ACCEPT`/`SYS_TCP_CLOSE_LISTENER`).
    TcpListener,
    /// A UDP socket (uses `SYS_UDP_SEND`/`SYS_UDP_RECV`/`SYS_UDP_CLOSE`).
    UdpSocket,
}

/// An entry in the file descriptor table.
#[derive(Debug, Clone, Copy)]
pub struct FdEntry {
    /// The kind of kernel handle.
    pub kind: HandleKind,
    /// The raw kernel handle value.
    pub handle: u64,
    /// Per-fd flags (`FD_CLOEXEC`, etc.) — managed by `F_GETFD`/`F_SETFD`.
    pub flags: u32,
    /// File status flags (`O_RDONLY`/`O_WRONLY`/`O_RDWR` | `O_APPEND` |
    /// `O_NONBLOCK` | ...) — managed by `F_GETFL`/`F_SETFL`.
    ///
    /// The access mode bits (low 2 bits: `O_ACCMODE`) are immutable after
    /// `open()`.  `F_SETFL` can only change the status flags: `O_APPEND`,
    /// `O_NONBLOCK`, `O_SYNC`.  Stored as the original POSIX flag word so
    /// `F_GETFL` can return it directly.
    pub status_flags: i32,
}

// ---------------------------------------------------------------------------
// Static fd table
// ---------------------------------------------------------------------------

/// The per-process fd table.
///
/// Each slot is either `None` (unused) or `Some(FdEntry)`.
/// Pre-initialized with console handles for fds 0, 1, 2.
static mut FD_TABLE: [Option<FdEntry>; MAX_FDS] = {
    let mut table: [Option<FdEntry>; MAX_FDS] = [None; MAX_FDS];

    // Pre-initialize stdin/stdout/stderr as console handles.
    // stdin is read-only, stdout/stderr are write-only.
    table[0] = Some(FdEntry { kind: HandleKind::Console, handle: 0, flags: 0, status_flags: 0 }); // O_RDONLY
    table[1] = Some(FdEntry { kind: HandleKind::Console, handle: 1, flags: 0, status_flags: 1 }); // O_WRONLY
    table[2] = Some(FdEntry { kind: HandleKind::Console, handle: 2, flags: 0, status_flags: 1 }); // O_WRONLY

    table
};

// ---------------------------------------------------------------------------
// Raw table pointer helper
// ---------------------------------------------------------------------------

/// Get a mutable pointer to the fd table without creating a reference.
///
/// Uses `addr_of_mut!` to avoid the Rust 2024 `static_mut_refs` restriction.
#[inline]
fn table_ptr() -> *mut [Option<FdEntry>; MAX_FDS] {
    core::ptr::addr_of_mut!(FD_TABLE)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Allocate the lowest available fd and store an entry.
///
/// Status flags default to 0 (`O_RDONLY`, no special flags).
/// Use [`set_status_flags()`] afterward if you need different flags,
/// or use [`alloc_fd_with_flags()`] to set them atomically.
///
/// Returns the fd number, or `None` if the table is full.
#[must_use]
pub fn alloc_fd(kind: HandleKind, handle: u64) -> Option<i32> {
    alloc_fd_from(0, kind, handle)
}

/// Allocate the lowest available fd and store an entry with initial
/// file status flags (e.g., `O_RDWR | O_APPEND`).
///
/// Returns the fd number, or `None` if the table is full.
#[must_use]
pub fn alloc_fd_with_flags(kind: HandleKind, handle: u64, status_flags: i32) -> Option<i32> {
    alloc_fd_from_with_flags(0, kind, handle, status_flags)
}

/// Allocate the lowest available fd >= `min_fd` and store an entry.
///
/// Used by `dup2`/`dup3` to allocate at a specific fd or higher.
#[must_use]
pub fn alloc_fd_from(min_fd: i32, kind: HandleKind, handle: u64) -> Option<i32> {
    alloc_fd_from_with_flags(min_fd, kind, handle, 0)
}

/// Allocate the lowest available fd >= `min_fd` with initial status flags.
#[must_use]
pub fn alloc_fd_from_with_flags(
    min_fd: i32,
    kind: HandleKind,
    handle: u64,
    status_flags: i32,
) -> Option<i32> {
    if min_fd < 0 {
        return None;
    }
    let start = min_fd as usize;
    // SAFETY: Single-threaded access.
    unsafe {
        let table = &mut *table_ptr();
        let mut i = start;
        while i < MAX_FDS {
            if let Some(slot) = table.get_mut(i)
                && slot.is_none()
            {
                *slot = Some(FdEntry { kind, handle, flags: 0, status_flags });
                return Some(i as i32);
            }
            i = i.wrapping_add(1);
        }
    }
    None
}

/// Install an entry at a specific fd number.
///
/// If the fd is already open, the previous entry is returned (caller
/// must close the underlying handle).
///
/// Returns `Some(old_entry)` if the fd was previously in use.
/// Returns `None` if the slot was empty (or fd out of range).
#[must_use]
pub fn install_fd(fd: i32, kind: HandleKind, handle: u64) -> Option<FdEntry> {
    install_fd_with_flags(fd, kind, handle, 0)
}

/// Install an entry at a specific fd with initial file status flags.
#[must_use]
pub fn install_fd_with_flags(
    fd: i32,
    kind: HandleKind,
    handle: u64,
    status_flags: i32,
) -> Option<FdEntry> {
    if fd < 0 || fd as usize >= MAX_FDS {
        return None;
    }
    let idx = fd as usize;
    // SAFETY: Single-threaded access.
    unsafe {
        let table = &mut *table_ptr();
        let slot = table.get_mut(idx)?;
        let old = slot.take();
        *slot = Some(FdEntry { kind, handle, flags: 0, status_flags });
        old
    }
}

/// Look up an fd in the table.
///
/// Returns the entry if the fd is valid and open, `None` otherwise.
#[must_use]
pub fn get_fd(fd: i32) -> Option<FdEntry> {
    if fd < 0 || fd as usize >= MAX_FDS {
        return None;
    }
    // SAFETY: Single-threaded access.  Read-only after bounds check.
    unsafe {
        let table = &*table_ptr();
        table.get(fd as usize).copied().flatten()
    }
}

/// Close an fd, removing it from the table.
///
/// Returns the entry that was stored (so the caller can close the
/// underlying kernel handle), or `None` if the fd was not open.
#[must_use]
pub fn close_fd(fd: i32) -> Option<FdEntry> {
    if fd < 0 || fd as usize >= MAX_FDS {
        return None;
    }
    // SAFETY: Single-threaded access.
    unsafe {
        let table = &mut *table_ptr();
        table.get_mut(fd as usize)?.take()
    }
}

/// Get the per-fd flags for an fd.
#[must_use]
pub fn get_fd_flags(fd: i32) -> Option<u32> {
    get_fd(fd).map(|e| e.flags)
}

/// Set the per-fd flags for an fd.
///
/// Returns `true` on success, `false` if the fd is not open.
#[must_use]
pub fn set_fd_flags(fd: i32, flags: u32) -> bool {
    if fd < 0 || fd as usize >= MAX_FDS {
        return false;
    }
    // SAFETY: Single-threaded access.
    unsafe {
        let table = &mut *table_ptr();
        if let Some(Some(entry)) = table.get_mut(fd as usize) {
            entry.flags = flags;
            true
        } else {
            false
        }
    }
}

/// Get the file status flags for an fd (`O_ACCMODE | O_APPEND | O_NONBLOCK | ...`).
///
/// Returns the full flags word — includes both the access mode bits
/// (read-only) and the mutable status flags.
#[must_use]
pub fn get_status_flags(fd: i32) -> Option<i32> {
    get_fd(fd).map(|e| e.status_flags)
}

/// Mask of bits that `F_SETFL` is allowed to change.
/// `O_APPEND` (0o2000) | `O_NONBLOCK` (0o4000) | `O_SYNC` (0o4_010_000).
const SETFL_MASK: i32 = 0o2000 | 0o4000 | 0o4_010_000;

/// Set the file status flags for an fd.
///
/// Per POSIX, only the status flag bits may be changed (`O_APPEND`,
/// `O_NONBLOCK`, `O_SYNC`); the access mode bits (`O_ACCMODE`) are
/// immutable after `open()`.  This function enforces that: the access
/// mode bits from the original `open()` are preserved, and only the
/// changeable bits are updated.
///
/// Returns `true` on success, `false` if the fd is not open.
#[must_use]
pub fn set_status_flags(fd: i32, new_flags: i32) -> bool {
    if fd < 0 || fd as usize >= MAX_FDS {
        return false;
    }
    // SAFETY: Single-threaded access.
    unsafe {
        let table = &mut *table_ptr();
        if let Some(Some(entry)) = table.get_mut(fd as usize) {
            // Preserve access mode, replace changeable bits.
            entry.status_flags = (entry.status_flags & !SETFL_MASK) | (new_flags & SETFL_MASK);
            true
        } else {
            false
        }
    }
}

/// Check whether any open fd references the given (kind, handle) pair.
///
/// Used after [`close_fd()`] to determine whether the underlying
/// kernel handle should actually be closed.  When multiple fds share
/// the same kernel handle (via [`dup()`] on handle types that lack
/// kernel-level duplication, such as pipes and sockets), closing one
/// fd must not destroy the handle if another fd still references it.
///
/// Returns `true` if at least one open fd matches `(kind, handle)`.
#[must_use]
pub fn is_handle_referenced(kind: HandleKind, handle: u64) -> bool {
    // SAFETY: Single-threaded access.  Read-only scan.
    unsafe {
        let table = &*table_ptr();
        let mut i = 0;
        while i < MAX_FDS {
            if let Some(Some(entry)) = table.get(i)
                && entry.kind == kind && entry.handle == handle
            {
                return true;
            }
            i = i.wrapping_add(1);
        }
    }
    false
}
