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
//! ## Per-fd Path Tracking
//!
//! A parallel path table stores the resolved absolute path used to
//! open each fd.  This enables `fchdir()` (change CWD by fd) and the
//! `*at()` family (`openat`, `fstatat`, etc.) to resolve relative
//! paths against a directory fd without a kernel-level fd-to-path
//! syscall.
//!
//! **Limitation:** if a file/directory is renamed after opening, the
//! stored path becomes stale.  Real kernels track the dentry directly
//! and follow renames; our approach doesn't.  This is acceptable for
//! POSIX compatibility — most programs that use `fchdir`/`openat` do
//! so immediately after opening the directory.
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

/// Clear the entire fd table.
///
/// Sets every slot to `None` without closing any kernel handles.
/// Used during child startup to reinitialize the table from inherited
/// fd mappings (the child's handles are different from the parent's
/// default console handles).
pub fn clear_all() {
    // SAFETY: Single-threaded access during early startup.
    unsafe {
        let table = &mut *table_ptr();
        let mut i = 0usize;
        while i < MAX_FDS {
            if let Some(slot) = table.get_mut(i) {
                *slot = None;
            }
            i = i.wrapping_add(1);
        }
    }
}

/// Install an fd entry at a specific slot.
///
/// Convenience wrapper over `install_fd` that discards the old entry.
/// Intended for child-side fd table initialization where there is no
/// old entry to close (table was just cleared).
pub fn set_fd(fd: i32, kind: HandleKind, handle: u64) {
    let _ = install_fd(fd, kind, handle);
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

// ---------------------------------------------------------------------------
// Per-fd path storage (for fchdir / *at dirfd resolution)
// ---------------------------------------------------------------------------

/// Maximum path bytes stored per fd (matches POSIX `PATH_MAX`).
///
/// Total static memory: `MAX_FDS × FD_PATH_MAX` = 256 × 4096 = 1 MiB
/// in .bss (zeroed at load, no binary size impact).  Acceptable for a
/// desktop OS process.
const FD_PATH_MAX: usize = 4096;

/// Per-fd path buffer table.
///
/// Each slot stores a null-terminated absolute path string when a path
/// is recorded for that fd.  The path is set by [`store_fd_path()`]
/// (called from `open()`) and cleared by [`clear_fd_path()`] (called
/// from `close()`).
static mut FD_PATH_TABLE: [[u8; FD_PATH_MAX]; MAX_FDS] = [[0u8; FD_PATH_MAX]; MAX_FDS];

/// Length of the stored path for each fd (0 = no path stored).
///
/// The length does NOT include the null terminator — it is the number
/// of path bytes.  Maximum storable length is `FD_PATH_MAX - 1` = 4095.
static mut FD_PATH_LENS: [u16; MAX_FDS] = [0u16; MAX_FDS];

/// Get a mutable pointer to the path table.
#[inline]
fn path_table_ptr() -> *mut [[u8; FD_PATH_MAX]; MAX_FDS] {
    core::ptr::addr_of_mut!(FD_PATH_TABLE)
}

/// Get a mutable pointer to the path length table.
#[inline]
fn path_lens_ptr() -> *mut [u16; MAX_FDS] {
    core::ptr::addr_of_mut!(FD_PATH_LENS)
}

/// Store the resolved absolute path associated with an fd.
///
/// Copies `path[..len]` bytes into the fd's path slot and adds a null
/// terminator.  If `len >= FD_PATH_MAX` (path too long for the buffer),
/// the path is silently not stored — `fchdir`/`*at` will fall back to
/// `ENOSYS` or `EBADF` for that fd.
///
/// Called by `open()` and friends after successfully allocating an fd.
///
/// # Safety contract
///
/// `path` must be valid for reading `len` bytes (guaranteed when the
/// caller passes a resolved path buffer from the stack).
pub fn store_fd_path(fd: i32, path: *const u8, len: usize) {
    if fd < 0 || fd as usize >= MAX_FDS || path.is_null() || len >= FD_PATH_MAX {
        return;
    }
    let idx = fd as usize;
    // SAFETY: Single-threaded access.  `idx < MAX_FDS` checked above.
    // `path` is valid for `len` bytes (caller contract).
    unsafe {
        let table = &mut *path_table_ptr();
        let lens = &mut *path_lens_ptr();
        if let Some(slot) = table.get_mut(idx) {
            let mut i = 0;
            while i < len {
                if let Some(dst) = slot.get_mut(i) {
                    *dst = *path.add(i);
                }
                i = i.wrapping_add(1);
            }
            // Null-terminate.
            if let Some(term) = slot.get_mut(len) {
                *term = 0;
            }
        }
        if let Some(len_slot) = lens.get_mut(idx) {
            *len_slot = len as u16;
        }
    }
}

/// Copy the stored path for an fd into `out`, null-terminated.
///
/// Returns the path length (excluding null terminator), or 0 if no
/// path is stored, the fd is invalid, or `out` is too small.
///
/// On success, `out[..return_value]` contains the path bytes and
/// `out[return_value]` is `b'\0'`.
pub fn get_fd_path(fd: i32, out: &mut [u8]) -> usize {
    if fd < 0 || fd as usize >= MAX_FDS || out.is_empty() {
        return 0;
    }
    let idx = fd as usize;
    // SAFETY: Single-threaded access.  `idx < MAX_FDS` checked above.
    unsafe {
        let lens = &*path_lens_ptr();
        let len = match lens.get(idx) {
            Some(&l) => l as usize,
            None => return 0,
        };
        if len == 0 || len >= out.len() {
            return 0; // No path stored or output buffer too small.
        }
        let table = &*path_table_ptr();
        if let Some(slot) = table.get(idx) {
            let mut i = 0;
            while i < len {
                if let (Some(&src), Some(dst)) = (slot.get(i), out.get_mut(i)) {
                    *dst = src;
                }
                i = i.wrapping_add(1);
            }
            if let Some(term) = out.get_mut(len) {
                *term = 0;
            }
        }
        len
    }
}

/// Clear the stored path for an fd.
///
/// Called from `close()`.  Sets the path length to 0 (the buffer
/// contents don't need to be zeroed — length 0 means "no path").
pub fn clear_fd_path(fd: i32) {
    if fd < 0 || fd as usize >= MAX_FDS {
        return;
    }
    let idx = fd as usize;
    // SAFETY: Single-threaded access.
    unsafe {
        let lens = &mut *path_lens_ptr();
        if let Some(len_slot) = lens.get_mut(idx) {
            *len_slot = 0;
        }
    }
}

/// Copy the stored path from one fd to another.
///
/// Called from `dup()`, `dup2()`, etc. so the duplicate fd also
/// knows the path of the resource it refers to.
pub fn copy_fd_path(src_fd: i32, dst_fd: i32) {
    if src_fd < 0 || src_fd as usize >= MAX_FDS
        || dst_fd < 0 || dst_fd as usize >= MAX_FDS
    {
        return;
    }
    let src_idx = src_fd as usize;
    let dst_idx = dst_fd as usize;
    // SAFETY: Single-threaded access.  Both indices < MAX_FDS.
    unsafe {
        let lens = &mut *path_lens_ptr();
        let src_len = match lens.get(src_idx) {
            Some(&l) => l,
            None => return,
        };
        if let Some(dst_len) = lens.get_mut(dst_idx) {
            *dst_len = src_len;
        }
        if src_len == 0 {
            return; // Nothing to copy.
        }
        let table = &mut *path_table_ptr();
        // Copy byte-by-byte to avoid aliasing issues when src == dst
        // (though that case is harmless, the code handles it correctly).
        let len = src_len as usize;
        let mut i = 0;
        while i < len {
            let byte = match table.get(src_idx).and_then(|s| s.get(i)) {
                Some(&b) => b,
                None => break,
            };
            if let Some(dst_slot) = table.get_mut(dst_idx).and_then(|s| s.get_mut(i)) {
                *dst_slot = byte;
            }
            i = i.wrapping_add(1);
        }
        // Null-terminate the destination.
        if let Some(dst_slot) = table.get_mut(dst_idx).and_then(|s| s.get_mut(len)) {
            *dst_slot = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(unused_must_use)]
mod tests {
    use super::*;

    /// Allocate an fd and immediately close it, returning the fd number.
    /// Helper for tests that need a disposable fd.
    #[allow(dead_code)]
    fn alloc_and_close(kind: HandleKind, handle: u64) -> i32 {
        let fd = alloc_fd(kind, handle).expect("alloc_fd failed");
        let _ = close_fd(fd);
        fd
    }

    // -- Constants --

    #[test]
    fn test_max_fds_value() {
        assert_eq!(MAX_FDS, 256);
    }

    #[test]
    fn test_fd_cloexec_value() {
        assert_eq!(FD_CLOEXEC, 1);
    }

    // -- HandleKind --

    #[test]
    fn test_handle_kind_equality() {
        assert_eq!(HandleKind::File, HandleKind::File);
        assert_ne!(HandleKind::File, HandleKind::Pipe);
        assert_ne!(HandleKind::Console, HandleKind::TcpStream);
    }

    // -- Pre-initialized fds 0/1/2 --

    #[test]
    fn test_stdio_fds_exist() {
        // fds 0, 1, 2 are pre-initialized as Console handles.
        let stdin = get_fd(0);
        assert!(stdin.is_some(), "fd 0 (stdin) should exist");
        assert_eq!(stdin.unwrap().kind, HandleKind::Console);

        let stdout = get_fd(1);
        assert!(stdout.is_some(), "fd 1 (stdout) should exist");
        assert_eq!(stdout.unwrap().kind, HandleKind::Console);

        let stderr = get_fd(2);
        assert!(stderr.is_some(), "fd 2 (stderr) should exist");
        assert_eq!(stderr.unwrap().kind, HandleKind::Console);
    }

    // -- get_fd boundary checks --

    #[test]
    fn test_get_fd_negative() {
        assert!(get_fd(-1).is_none());
        assert!(get_fd(i32::MIN).is_none());
    }

    #[test]
    fn test_get_fd_out_of_range() {
        assert!(get_fd(MAX_FDS as i32).is_none());
        assert!(get_fd(1000).is_none());
    }

    #[test]
    fn test_get_fd_unoccupied() {
        // A high-numbered fd should be unoccupied in a fresh table.
        assert!(get_fd(200).is_none());
    }

    // -- alloc_fd / close_fd --

    #[test]
    fn test_alloc_fd_uses_lowest_available() {
        // fds 0, 1, 2 are occupied → first free should be 3.
        let fd = alloc_fd(HandleKind::File, 100).expect("alloc_fd failed");
        assert_eq!(fd, 3, "should allocate fd 3 (lowest free)");
        // Cleanup.
        let _ = close_fd(fd);
    }

    #[test]
    fn test_alloc_fd_returns_entry_on_close() {
        let fd = alloc_fd(HandleKind::Pipe, 42).expect("alloc_fd failed");
        let entry = close_fd(fd).expect("close_fd should return entry");
        assert_eq!(entry.kind, HandleKind::Pipe);
        assert_eq!(entry.handle, 42);
    }

    #[test]
    fn test_close_fd_makes_slot_reusable() {
        let fd1 = alloc_fd(HandleKind::File, 10).unwrap();
        let _ = close_fd(fd1);
        // Allocating again should reuse the same fd.
        let fd2 = alloc_fd(HandleKind::File, 20).unwrap();
        assert_eq!(fd1, fd2, "freed fd should be reusable");
        let _ = close_fd(fd2);
    }

    #[test]
    fn test_close_fd_negative() {
        assert!(close_fd(-1).is_none());
    }

    #[test]
    fn test_close_fd_unoccupied() {
        assert!(close_fd(200).is_none());
    }

    // -- alloc_fd_from --

    #[test]
    fn test_alloc_fd_from_min() {
        let fd = alloc_fd_from(100, HandleKind::File, 55).unwrap();
        assert!(fd >= 100);
        let _ = close_fd(fd);
    }

    #[test]
    fn test_alloc_fd_from_negative() {
        assert!(alloc_fd_from(-1, HandleKind::File, 0).is_none());
    }

    // -- alloc_fd_with_flags --

    #[test]
    fn test_alloc_fd_with_status_flags() {
        let flags = 0o2000; // O_APPEND
        let fd = alloc_fd_with_flags(HandleKind::File, 99, flags).unwrap();
        let entry = get_fd(fd).unwrap();
        assert_eq!(entry.status_flags, flags);
        let _ = close_fd(fd);
    }

    // -- fd flags (FD_CLOEXEC) --

    #[test]
    fn test_get_set_fd_flags() {
        let fd = alloc_fd(HandleKind::File, 77).unwrap();

        // Initially 0.
        assert_eq!(get_fd_flags(fd), Some(0));

        // Set FD_CLOEXEC.
        assert!(set_fd_flags(fd, FD_CLOEXEC));
        assert_eq!(get_fd_flags(fd), Some(FD_CLOEXEC));

        // Clear it.
        assert!(set_fd_flags(fd, 0));
        assert_eq!(get_fd_flags(fd), Some(0));

        let _ = close_fd(fd);
    }

    #[test]
    fn test_set_fd_flags_bad_fd() {
        assert!(!set_fd_flags(-1, FD_CLOEXEC));
        assert!(!set_fd_flags(200, FD_CLOEXEC));
    }

    // -- status flags (F_GETFL/F_SETFL) --

    #[test]
    fn test_get_set_status_flags() {
        // Allocate fd with O_RDWR (2).
        let fd = alloc_fd_with_flags(HandleKind::File, 88, 2).unwrap();
        assert_eq!(get_status_flags(fd), Some(2));

        // Set O_APPEND — access mode bits should be preserved.
        assert!(set_status_flags(fd, 0o2000)); // O_APPEND
        let flags = get_status_flags(fd).unwrap();
        assert_eq!(flags & 0x3, 2, "access mode should be preserved");
        assert_ne!(flags & 0o2000, 0, "O_APPEND should be set");

        let _ = close_fd(fd);
    }

    #[test]
    fn test_set_status_flags_preserves_access_mode() {
        // O_WRONLY = 1, then try to change access mode to O_RDWR via F_SETFL.
        let fd = alloc_fd_with_flags(HandleKind::File, 66, 1).unwrap();
        // Attempt to set O_RDWR (2) via set_status_flags — should NOT change access mode.
        let _ = set_status_flags(fd, 2);
        let flags = get_status_flags(fd).unwrap();
        assert_eq!(flags & 0x3, 1, "access mode must be immutable after open");

        let _ = close_fd(fd);
    }

    #[test]
    fn test_set_status_flags_bad_fd() {
        assert!(!set_status_flags(-1, 0));
    }

    // -- install_fd --

    #[test]
    fn test_install_fd_at_specific_slot() {
        // Install at fd 200 (should be empty).
        let old = install_fd(200, HandleKind::UdpSocket, 123);
        assert!(old.is_none(), "slot 200 should have been empty");

        let entry = get_fd(200).unwrap();
        assert_eq!(entry.kind, HandleKind::UdpSocket);
        assert_eq!(entry.handle, 123);

        // Cleanup.
        let _ = close_fd(200);
    }

    #[test]
    fn test_install_fd_replaces_existing() {
        let fd = alloc_fd(HandleKind::File, 50).unwrap();
        let old = install_fd(fd, HandleKind::Pipe, 60);
        assert!(old.is_some());
        let old_entry = old.unwrap();
        assert_eq!(old_entry.kind, HandleKind::File);
        assert_eq!(old_entry.handle, 50);

        let new = get_fd(fd).unwrap();
        assert_eq!(new.kind, HandleKind::Pipe);
        assert_eq!(new.handle, 60);

        let _ = close_fd(fd);
    }

    // -- is_handle_referenced --

    #[test]
    fn test_is_handle_referenced_true() {
        let fd = alloc_fd(HandleKind::TcpStream, 999).unwrap();
        assert!(is_handle_referenced(HandleKind::TcpStream, 999));
        let _ = close_fd(fd);
    }

    #[test]
    fn test_is_handle_referenced_false_after_close() {
        let fd = alloc_fd(HandleKind::TcpStream, 888).unwrap();
        let _ = close_fd(fd);
        assert!(!is_handle_referenced(HandleKind::TcpStream, 888));
    }

    #[test]
    fn test_is_handle_referenced_wrong_kind() {
        let fd = alloc_fd(HandleKind::Pipe, 777).unwrap();
        // Same handle value but different kind → not referenced.
        assert!(!is_handle_referenced(HandleKind::File, 777));
        let _ = close_fd(fd);
    }

    // -- SETFL_MASK --

    #[test]
    fn test_setfl_mask_includes_expected_bits() {
        // O_APPEND = 0o2000, O_NONBLOCK = 0o4000.
        assert_ne!(SETFL_MASK & 0o2000, 0, "O_APPEND should be changeable");
        assert_ne!(SETFL_MASK & 0o4000, 0, "O_NONBLOCK should be changeable");
    }

    #[test]
    fn test_setfl_mask_excludes_access_mode() {
        // Access mode bits (low 2 bits) must NOT be in the changeable mask.
        assert_eq!(SETFL_MASK & 0x3, 0, "O_ACCMODE must not be changeable");
    }

    // -- fd path storage --

    #[test]
    fn test_store_and_get_fd_path() {
        let fd = alloc_fd(HandleKind::File, 200).unwrap();
        clear_fd_path(fd); // Clean slate (fd numbers reused across tests).
        let path = b"/home/user/docs";
        store_fd_path(fd, path.as_ptr(), path.len());

        let mut buf = [0u8; FD_PATH_MAX];
        let len = get_fd_path(fd, &mut buf);
        assert_eq!(len, path.len());
        assert_eq!(&buf[..len], path);
        // Should be null-terminated.
        assert_eq!(buf[len], 0);

        clear_fd_path(fd);
        let _ = close_fd(fd);
    }

    #[test]
    fn test_clear_fd_path() {
        let fd = alloc_fd(HandleKind::File, 201).unwrap();
        clear_fd_path(fd);
        let path = b"/tmp/test";
        store_fd_path(fd, path.as_ptr(), path.len());

        // Verify it's stored.
        let mut buf = [0u8; FD_PATH_MAX];
        assert_ne!(get_fd_path(fd, &mut buf), 0);

        // Clear and verify.
        clear_fd_path(fd);
        assert_eq!(get_fd_path(fd, &mut buf), 0);

        let _ = close_fd(fd);
    }

    #[test]
    fn test_copy_fd_path() {
        let fd1 = alloc_fd(HandleKind::File, 202).unwrap();
        let fd2 = alloc_fd(HandleKind::File, 203).unwrap();
        clear_fd_path(fd1);
        clear_fd_path(fd2);
        let path = b"/var/log/messages";
        store_fd_path(fd1, path.as_ptr(), path.len());

        copy_fd_path(fd1, fd2);

        let mut buf = [0u8; FD_PATH_MAX];
        let len = get_fd_path(fd2, &mut buf);
        assert_eq!(len, path.len());
        assert_eq!(&buf[..len], path);

        clear_fd_path(fd1);
        clear_fd_path(fd2);
        let _ = close_fd(fd1);
        let _ = close_fd(fd2);
    }

    #[test]
    fn test_get_fd_path_no_path_stored() {
        let fd = alloc_fd(HandleKind::File, 204).unwrap();
        clear_fd_path(fd); // Ensure clean — fd numbers reused.
        let mut buf = [0u8; FD_PATH_MAX];
        assert_eq!(get_fd_path(fd, &mut buf), 0);
        let _ = close_fd(fd);
    }

    #[test]
    fn test_get_fd_path_invalid_fd() {
        let mut buf = [0u8; FD_PATH_MAX];
        assert_eq!(get_fd_path(-1, &mut buf), 0);
        assert_eq!(get_fd_path(999, &mut buf), 0);
    }

    #[test]
    fn test_store_fd_path_null_pointer() {
        let fd = alloc_fd(HandleKind::File, 205).unwrap();
        clear_fd_path(fd);
        store_fd_path(fd, core::ptr::null(), 10);
        // Should not crash; path should not be stored.
        let mut buf = [0u8; FD_PATH_MAX];
        assert_eq!(get_fd_path(fd, &mut buf), 0);
        let _ = close_fd(fd);
    }

    #[test]
    fn test_store_fd_path_too_long() {
        let fd = alloc_fd(HandleKind::File, 206).unwrap();
        clear_fd_path(fd);
        // Attempt to store a path that's exactly FD_PATH_MAX bytes
        // (no room for null terminator).
        let long_path = [b'a'; FD_PATH_MAX];
        store_fd_path(fd, long_path.as_ptr(), FD_PATH_MAX);
        // Should be silently rejected.
        let mut buf = [0u8; FD_PATH_MAX];
        assert_eq!(get_fd_path(fd, &mut buf), 0);
        let _ = close_fd(fd);
    }

    #[test]
    fn test_get_fd_path_small_output_buffer() {
        let fd = alloc_fd(HandleKind::File, 207).unwrap();
        clear_fd_path(fd);
        let path = b"/a/long/path/here";
        store_fd_path(fd, path.as_ptr(), path.len());

        // Buffer too small to hold path + null terminator.
        let mut small_buf = [0u8; 5];
        let len = get_fd_path(fd, &mut small_buf);
        assert_eq!(len, 0, "should return 0 when buffer is too small");

        clear_fd_path(fd);
        let _ = close_fd(fd);
    }

    #[test]
    fn test_fd_path_overwrite() {
        let fd = alloc_fd(HandleKind::File, 208).unwrap();
        clear_fd_path(fd);
        let path1 = b"/first/path";
        let path2 = b"/second";
        store_fd_path(fd, path1.as_ptr(), path1.len());
        store_fd_path(fd, path2.as_ptr(), path2.len());

        let mut buf = [0u8; FD_PATH_MAX];
        let len = get_fd_path(fd, &mut buf);
        assert_eq!(len, path2.len());
        assert_eq!(&buf[..len], path2.as_slice());

        clear_fd_path(fd);
        let _ = close_fd(fd);
    }

    #[test]
    fn test_copy_fd_path_no_source_path() {
        let fd1 = alloc_fd(HandleKind::File, 209).unwrap();
        let fd2 = alloc_fd(HandleKind::File, 210).unwrap();
        clear_fd_path(fd1);
        clear_fd_path(fd2);
        // fd1 has no path stored.
        copy_fd_path(fd1, fd2);
        // fd2 should also have no path.
        let mut buf = [0u8; FD_PATH_MAX];
        assert_eq!(get_fd_path(fd2, &mut buf), 0);

        let _ = close_fd(fd1);
        let _ = close_fd(fd2);
    }

    #[test]
    fn test_clear_fd_path_invalid_fd() {
        // Should not crash.
        clear_fd_path(-1);
        clear_fd_path(999);
    }

    #[test]
    fn test_fd_path_constant() {
        assert_eq!(FD_PATH_MAX, 4096);
    }

    // -- clear_all / set_fd --

    #[test]
    fn test_clear_all_removes_entries() {
        // First verify fd 0 exists (default console).
        assert!(get_fd(0).is_some(), "fd 0 should exist before clear");

        clear_all();

        // After clear, all fds should be gone.
        assert!(get_fd(0).is_none(), "fd 0 should be gone after clear");
        assert!(get_fd(1).is_none(), "fd 1 should be gone after clear");
        assert!(get_fd(2).is_none(), "fd 2 should be gone after clear");

        // Restore defaults so other tests aren't affected.
        install_fd(0, HandleKind::Console, 0);
        install_fd(1, HandleKind::Console, 1);
        install_fd(2, HandleKind::Console, 2);
    }

    #[test]
    fn test_set_fd_installs_entry() {
        set_fd(10, HandleKind::File, 42);
        let entry = get_fd(10);
        assert!(entry.is_some());
        let e = entry.unwrap();
        assert_eq!(e.kind, HandleKind::File);
        assert_eq!(e.handle, 42);

        // Clean up.
        close_fd(10);
    }

    #[test]
    fn test_set_fd_overwrites_existing() {
        set_fd(11, HandleKind::Pipe, 100);
        set_fd(11, HandleKind::File, 200);

        let e = get_fd(11).unwrap();
        assert_eq!(e.kind, HandleKind::File);
        assert_eq!(e.handle, 200);

        // Clean up.
        close_fd(11);
    }

    // -- install_fd_with_flags --

    #[test]
    fn test_install_fd_with_flags() {
        let old = install_fd_with_flags(15, HandleKind::File, 500, 0o2);
        // Slot 15 was likely empty, so old should be None.
        // (Other tests may have left something — just check the new entry.)
        let e = get_fd(15).unwrap();
        assert_eq!(e.kind, HandleKind::File);
        assert_eq!(e.handle, 500);
        // Verify status flags were stored.
        let sf = get_status_flags(15).unwrap();
        assert_eq!(sf & 0o2, 0o2, "Status flags should include O_RDWR");
        // Clean up.
        close_fd(15);
        let _ = old;
    }

    #[test]
    fn test_install_fd_with_flags_replaces_existing() {
        install_fd(16, HandleKind::Pipe, 600);
        let old = install_fd_with_flags(16, HandleKind::File, 601, 0o1);
        assert!(old.is_some(), "Should return old entry");
        let old_entry = old.unwrap();
        assert_eq!(old_entry.kind, HandleKind::Pipe);
        assert_eq!(old_entry.handle, 600);
        // New entry check.
        let e = get_fd(16).unwrap();
        assert_eq!(e.kind, HandleKind::File);
        assert_eq!(e.handle, 601);
        // Clean up.
        close_fd(16);
    }

    #[test]
    fn test_install_fd_with_flags_out_of_range() {
        let result = install_fd_with_flags(MAX_FDS as i32, HandleKind::File, 1, 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_install_fd_with_flags_negative_fd() {
        let result = install_fd_with_flags(-1, HandleKind::File, 1, 0);
        assert!(result.is_none());
    }

    // -- alloc_fd_from_with_flags --

    #[test]
    fn test_alloc_fd_from_with_flags() {
        let fd = alloc_fd_from_with_flags(20, HandleKind::Pipe, 700, 0o4000);
        assert!(fd.is_some());
        let fd = fd.unwrap();
        assert!(fd >= 20, "Should allocate at or above min_fd=20");
        let e = get_fd(fd).unwrap();
        assert_eq!(e.kind, HandleKind::Pipe);
        assert_eq!(e.handle, 700);
        // Verify status flags.
        let sf = get_status_flags(fd).unwrap();
        assert_ne!(sf & 0o4000, 0, "O_NONBLOCK should be set");
        // Clean up.
        close_fd(fd);
    }

    #[test]
    fn test_alloc_fd_from_with_flags_negative_min() {
        let result = alloc_fd_from_with_flags(-1, HandleKind::File, 1, 0);
        assert!(result.is_none());
    }

    // -- alloc_fd_with_flags --

    #[test]
    fn test_alloc_fd_with_flags_basic() {
        let fd = alloc_fd_with_flags(HandleKind::File, 800, 0o1);
        assert!(fd.is_some());
        let fd = fd.unwrap();
        let e = get_fd(fd).unwrap();
        assert_eq!(e.kind, HandleKind::File);
        assert_eq!(e.handle, 800);
        let sf = get_status_flags(fd).unwrap();
        assert_eq!(sf & 0o3, 0o1, "O_WRONLY should be set");
        close_fd(fd);
    }

    // -- HandleKind variant coverage --

    #[test]
    fn test_alloc_fd_all_handle_kinds() {
        let kinds = [
            HandleKind::Console,
            HandleKind::File,
            HandleKind::Pipe,
            HandleKind::TcpStream,
            HandleKind::TcpListener,
            HandleKind::UdpSocket,
        ];
        for kind in kinds {
            let fd = alloc_fd(kind, 999).unwrap();
            let e = get_fd(fd).unwrap();
            assert_eq!(e.kind, kind);
            close_fd(fd);
        }
    }

    // -- is_handle_referenced edge case --

    #[test]
    fn test_is_handle_referenced_different_handle() {
        let fd1 = alloc_fd(HandleKind::Pipe, 1001).unwrap();
        let fd2 = alloc_fd(HandleKind::Pipe, 1002).unwrap();
        // fd1 and fd2 have different handles — closing one should not
        // affect is_handle_referenced for the other.
        close_fd(fd1);
        assert!(is_handle_referenced(HandleKind::Pipe, 1002));
        close_fd(fd2);
        assert!(!is_handle_referenced(HandleKind::Pipe, 1002));
    }

    // -- get_fd_flags default is zero --

    #[test]
    fn test_get_fd_flags_default_zero() {
        let fd = alloc_fd(HandleKind::File, 1003).unwrap();
        assert_eq!(get_fd_flags(fd), Some(0));
        close_fd(fd);
    }

    // -- store_fd_path empty string --

    #[test]
    fn test_store_fd_path_empty() {
        let fd = alloc_fd(HandleKind::File, 1004).unwrap();
        store_fd_path(fd, b"\0".as_ptr(), 0);
        let mut out = [0u8; 64];
        let len = get_fd_path(fd, &mut out);
        assert_eq!(len, 0, "Empty path should have length 0");
        close_fd(fd);
    }
}
