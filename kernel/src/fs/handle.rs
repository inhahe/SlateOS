//! File handle management — open/read/write/seek/close model.
//!
//! Provides a global open-file table that maps integer handle IDs to
//! open file state (path, cursor position, mode flags).  Syscalls use
//! handle IDs instead of paths for file I/O after opening.
//!
//! ## Design
//!
//! File handles are kernel-managed integers.  Currently they live in a
//! global table; once the capability system matures, each handle will
//! be a capability entry in the per-process capability table (handles
//! ARE capabilities per the design spec).
//!
//! ## Thread Safety
//!
//! The open-file table is behind a `spin::Mutex`.  Individual file
//! positions are mutated under this lock — this is acceptable for
//! early development but should move to per-handle locks or lock-free
//! structures on the hot path.

use alloc::collections::BTreeMap;
use alloc::string::String;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Open flags
// ---------------------------------------------------------------------------

/// Flags passed to `open()` controlling the access mode.
///
/// These are a bitfield.  Multiple flags can be OR'd together.
#[derive(Debug, Clone, Copy)]
pub struct OpenFlags(u32);

#[allow(dead_code)]
impl OpenFlags {
    /// No flags (invalid — at least READ or WRITE must be set).
    pub const NONE: Self = Self(0);
    /// Open for reading.
    pub const READ: Self = Self(1 << 0);
    /// Open for writing.
    pub const WRITE: Self = Self(1 << 1);
    /// Create the file if it doesn't exist (requires WRITE).
    pub const CREATE: Self = Self(1 << 2);
    /// Truncate the file to zero length on open (requires WRITE).
    pub const TRUNCATE: Self = Self(1 << 3);
    /// All writes go to the end of the file regardless of seek position.
    pub const APPEND: Self = Self(1 << 4);
    /// Require the path to refer to a directory.
    ///
    /// When set, `open()` will allocate a directory handle (suitable for
    /// `getdents64(2)`) if the path resolves to a directory, or return
    /// `NotADirectory` if it resolves to anything else.  When not set,
    /// directories are rejected with `IsADirectory` as before.
    pub const DIRECTORY: Self = Self(1 << 5);

    /// Create from raw bits (for syscall argument parsing).
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Get the raw bits.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Check if a flag is set.
    #[must_use]
    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0 && flag.0 != 0
    }

    /// Combine flags.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Check if readable.
    #[must_use]
    pub const fn is_readable(self) -> bool {
        self.contains(Self::READ)
    }

    /// Check if writable.
    #[must_use]
    pub const fn is_writable(self) -> bool {
        self.contains(Self::WRITE)
    }
}

// ---------------------------------------------------------------------------
// Seek origin
// ---------------------------------------------------------------------------

/// Origin for seek operations.
#[derive(Debug, Clone, Copy)]
pub enum SeekFrom {
    /// Seek to an absolute byte position.
    Start(u64),
    /// Seek relative to the current position (can be negative).
    Current(i64),
    /// Seek relative to the end of the file (can be negative).
    End(i64),
    /// Seek to the next data region at or after the given offset.
    /// Returns the offset of the next data byte.  If the file has
    /// no holes (common case), this returns the given offset if it's
    /// within the file, or an error if past EOF.
    Data(u64),
    /// Seek to the next hole at or after the given offset.
    /// A "hole" is an unallocated region (reads as zeros).
    /// If the filesystem doesn't track holes, returns EOF as the
    /// first hole (the conceptual hole after all data).
    Hole(u64),
}

// ---------------------------------------------------------------------------
// Open file entry
// ---------------------------------------------------------------------------

/// An open file tracked by the kernel.
struct OpenFile {
    /// Absolute VFS path to the file.
    path: String,
    /// Current read/write cursor position.
    offset: u64,
    /// Cached file size (updated on write/truncate/open).
    size: u64,
    /// Flags this file was opened with.
    flags: OpenFlags,
    /// Number of owners sharing this open file description.
    ///
    /// One open file description (this `OpenFile` entry, with its shared
    /// cursor) can be referenced by several handle owners after `fork()`
    /// or a shared `dup`/`dup2`.  Each owner contributes one reference;
    /// `close()` decrements and only removes the entry (and releases the
    /// advisory lock) when the last reference goes away.  A freshly
    /// opened handle starts at `1`.  This mirrors POSIX semantics where a
    /// forked child shares the parent's open file description (and its
    /// offset), not an independent copy.
    refcount: u32,
    /// `true` if this is a directory handle (opened with `OpenFlags::DIRECTORY`).
    ///
    /// Directory handles support `read_dir_at` / `set_dir_cursor` but
    /// reject byte-oriented operations (read/write/seek/read_at/write_at)
    /// with `IsADirectory`.  The `offset` field doubles as the directory
    /// cursor (entry index) for these handles.
    is_directory: bool,
}

// ---------------------------------------------------------------------------
// Global open-file table
// ---------------------------------------------------------------------------

/// Counter for generating unique handle IDs.
///
/// Starts at 1 so that 0 is never a valid handle (useful as a
/// sentinel / error indicator in userspace).
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

/// The global open-file table.
///
/// Maps handle IDs → open file state.  Protected by a spin mutex.
///
/// TODO: migrate to per-process tables once the capability system
/// tracks file handles as capabilities.  For now, a single global
/// table is correct because the kernel's few userspace processes
/// don't share handle namespaces.
static OPEN_FILES: Mutex<BTreeMap<u64, OpenFile>> = Mutex::new(BTreeMap::new());

/// Maximum number of simultaneously open files (system-wide).
///
/// Prevents runaway handle allocation from exhausting kernel heap.
const MAX_OPEN_FILES: usize = 1024;

// ---------------------------------------------------------------------------
// Capability tag enforcement
// ---------------------------------------------------------------------------

/// Check file/directory capability tag access for the current process.
///
/// Same logic as `vfs::check_file_tags` but for the handle module.
/// Called at open time — the handle is proof of access after that.
fn check_file_tags_for_handle(path: &str) -> KernelResult<()> {
    if crate::cap::file_tags::count() == 0 {
        return Ok(());
    }

    let task_id = crate::sched::current_task_id();
    let pid = match crate::proc::thread::owner_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => return Ok(()),
    };

    let creds = match crate::proc::pcb::get_credentials(pid) {
        Some(c) => c,
        None => return Ok(()),
    };

    crate::cap::file_tags::check_access(
        creds.uid,
        creds.gid,
        &creds.groups,
        path,
    )
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Open a file and return a handle ID.
///
/// Validates that the file exists (or creates it if `CREATE` is set),
/// caches the file size, and optionally truncates.  The handle starts
/// with offset 0 (or at end-of-file if `APPEND` is set).
pub fn open(path: &str, flags: OpenFlags) -> KernelResult<u64> {
    // Must have at least READ or WRITE.
    if !flags.is_readable() && !flags.is_writable() {
        return Err(KernelError::InvalidArgument);
    }

    // Read-only volume enforcement: if this open would mutate the file
    // (write, create, truncate, or append), and the calling process has a
    // read-only volume mount covering this path, reject with EROFS before
    // touching the filesystem.  This is a cheap no-op for any process
    // without read-only volumes (the common case).
    if flags.is_writable()
        || flags.contains(OpenFlags::CREATE)
        || flags.contains(OpenFlags::TRUNCATE)
        || flags.contains(OpenFlags::APPEND)
    {
        crate::ipc::namespace::check_writable(path)?;
    }

    // Resolve symlinks at open time so the handle refers to the
    // underlying file, not the symlink.  This matches Unix semantics:
    // if the symlink is later changed, existing handles still point
    // to the original target.
    let norm = crate::fs::Vfs::resolve_path(path)?;

    // Check file capability tags — the process must be a member of
    // all required groups for this path (or any ancestor with tags).
    // This is checked at open time; subsequent reads/writes on the
    // handle are allowed without re-checking (the handle is proof
    // of access, like a file descriptor).
    check_file_tags_for_handle(&norm)?;

    // Check if the file exists.
    // Note: Vfs::stat() also checks file tags internally, but our
    // explicit check above handles the CREATE case (file doesn't
    // exist yet, so stat is never called — we still need the check).
    let stat_result = crate::fs::Vfs::stat_resolved(&norm);

    match stat_result {
        Ok(entry) => {
            // File exists.
            if entry.entry_type == crate::fs::EntryType::Directory {
                // Directory: only allowed if the caller asked for one.
                if !flags.contains(OpenFlags::DIRECTORY) {
                    return Err(KernelError::IsADirectory);
                }
                // O_DIRECTORY combined with anything that would mutate a
                // file makes no sense — TRUNCATE / CREATE / APPEND only
                // apply to regular files.  Reject early so we don't
                // silently ignore the flag.
                if flags.contains(OpenFlags::TRUNCATE)
                    || flags.contains(OpenFlags::CREATE)
                    || flags.contains(OpenFlags::APPEND)
                    || flags.is_writable()
                {
                    return Err(KernelError::IsADirectory);
                }
                return allocate_dir_handle(norm, flags);
            }

            // Regular file.
            if flags.contains(OpenFlags::DIRECTORY) {
                // Caller demanded a directory but found something else.
                return Err(KernelError::NotADirectory);
            }

            let mut size = entry.size;

            // Handle TRUNCATE flag.
            if flags.contains(OpenFlags::TRUNCATE) {
                if !flags.is_writable() {
                    return Err(KernelError::InvalidArgument);
                }
                crate::fs::Vfs::truncate_resolved(&norm, 0)?;
                size = 0;
            }

            let offset = if flags.contains(OpenFlags::APPEND) {
                size
            } else {
                0
            };

            // inotify IN_OPEN: emit only after the handle is installed so a
            // failed allocation never produces a spurious open event.  The
            // emit is gated lock-free on the OPEN interest count, so opens
            // pay nothing when no watch is watching for opens.
            let handle = allocate_handle(norm.clone(), offset, size, flags)?;
            crate::fs::notify::emit_opened(&norm);
            Ok(handle)
        }
        Err(KernelError::NotFound) => {
            // File doesn't exist — create if CREATE is set.
            // O_DIRECTORY|O_CREAT is nonsensical: directories are created
            // by mkdir, not open.  Mirror Linux which simply fails the
            // lookup with ENOENT in that combination.
            if flags.contains(OpenFlags::DIRECTORY) {
                return Err(KernelError::NotFound);
            }
            if !flags.contains(OpenFlags::CREATE) {
                return Err(KernelError::NotFound);
            }
            if !flags.is_writable() {
                return Err(KernelError::InvalidArgument);
            }

            // Create an empty file.  write_file already emits IN_CREATE; the
            // IN_OPEN below follows it, matching Linux's O_CREAT open order.
            crate::fs::Vfs::write_file_resolved(&norm, &[])?;

            let handle = allocate_handle(norm.clone(), 0, 0, flags)?;
            crate::fs::notify::emit_opened(&norm);
            Ok(handle)
        }
        Err(e) => Err(e),
    }
}

/// Close an open file handle.
///
/// Frees the handle ID and releases any advisory locks held with
/// this handle ID as owner.  Further operations on this handle
/// will return `InvalidHandle`.
pub fn close(handle: u64) -> KernelResult<()> {
    // Decrement the open file description's refcount.  Only the final
    // close (refcount → 0) removes the entry and releases advisory
    // locks — earlier closes by other owners (forked siblings, shared
    // dups) just drop their reference, leaving the shared cursor intact
    // for the remaining owners.
    let closed = {
        let mut table = OPEN_FILES.lock();
        let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;
        file.refcount = file.refcount.saturating_sub(1);
        if file.refcount > 0 {
            // Other owners still hold this description; nothing to tear
            // down yet.
            return Ok(());
        }
        // Last reference — remove the entry and capture the path, write-mode,
        // and directory flag so we can release any advisory lock and emit an
        // inotify close event below (both after dropping this lock, to keep
        // the OPEN_FILES → WATCHES lock order one-directional).
        table
            .remove(&handle)
            .map(|file| (file.path, file.flags.is_writable(), file.is_directory))
    };

    if let Some((ref p, writable, is_dir)) = closed {
        // Release any advisory lock this handle holds on the file path.
        // Using the handle ID as the owner (consistent with how flock
        // syscalls pass owner IDs).  Best-effort: ignore errors from
        // lock release.
        // `p` is the resolved host path captured at open; use the _resolved
        // worker so we don't re-apply namespace translation (double-jail).
        let _ = crate::fs::Vfs::funlock_resolved(p, handle);

        // inotify IN_CLOSE_WRITE / IN_CLOSE_NOWRITE on the final close of the
        // open file description.  Directory handles report the close too and
        // are tagged `is_dir` so the inotify adapter ORs in IN_ISDIR.  Gated
        // lock-free on the matching CLOSE interest count.
        crate::fs::notify::emit_closed(p, writable, is_dir);
    }

    Ok(())
}

/// Read up to `buf_len` bytes from the file at the current offset.
///
/// Advances the offset by the number of bytes read.  Returns the
/// number of bytes actually read (may be less than `buf_len` if
/// near end-of-file; 0 means already at EOF).
pub fn read(handle: u64, buf: &mut [u8]) -> KernelResult<usize> {
    // We need to look up the file, read data via VFS, then update
    // the offset.  We hold the lock across the VFS call — acceptable
    // for early dev but should be improved.
    let mut table = OPEN_FILES.lock();
    let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;

    if file.is_directory {
        return Err(KernelError::IsADirectory);
    }

    if !file.flags.is_readable() {
        return Err(KernelError::PermissionDenied);
    }

    // Nothing to read if at or past EOF.
    if file.offset >= file.size {
        return Ok(0);
    }

    // Read via VFS.  Currently reads the whole file — the default
    // `read_at` in the FileSystem trait slices the result.
    let data = crate::fs::Vfs::read_at_resolved(&file.path, file.offset, buf.len())?;
    let copy_len = data.len().min(buf.len());

    if let Some(dest) = buf.get_mut(..copy_len) {
        if let Some(src) = data.get(..copy_len) {
            dest.copy_from_slice(src);
        }
    }

    // Advance offset.
    file.offset = file.offset.saturating_add(copy_len as u64);

    Ok(copy_len)
}

/// Return the file offset at which the next byte written via
/// [`write`] would land.
///
/// This is `file.size` for handles opened with [`OpenFlags::APPEND`]
/// (POSIX rule: append-mode writes always go to EOF, ignoring the
/// stored offset) and `file.offset` otherwise.
///
/// Used by the Linux ABI translation layer to enforce `RLIMIT_FSIZE`
/// against the current-offset write paths (`write(2)`, `writev(2)`)
/// — those syscalls don't carry an explicit offset, so the kernel
/// must peek at the open-file description to know where the write
/// will land before clipping or rejecting it.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] — `handle` is not in the table.
/// - [`KernelError::IsADirectory`] — `handle` is a directory handle
///   (which doesn't support byte-oriented writes).
/// - [`KernelError::PermissionDenied`] — handle was not opened for
///   writing.
pub fn peek_write_offset(handle: u64) -> KernelResult<u64> {
    let table = OPEN_FILES.lock();
    let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
    if file.is_directory {
        return Err(KernelError::IsADirectory);
    }
    if !file.flags.is_writable() {
        return Err(KernelError::PermissionDenied);
    }
    if file.flags.contains(OpenFlags::APPEND) {
        Ok(file.size)
    } else {
        Ok(file.offset)
    }
}

/// Return the file's current offset (`f_pos`) **without** modifying it.
///
/// This is the raw open-file-description position — exactly the value
/// `/proc/<pid>/fdinfo/<n>` reports as `pos:`.  Unlike
/// [`peek_write_offset`], it never applies the `APPEND`→EOF adjustment:
/// `pos` is literally `f_pos`, the same as `lseek(fd, 0, SEEK_CUR)`.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] — `handle` is not in the table.
/// - [`KernelError::IsADirectory`] — `handle` is a directory handle
///   (directories have no byte offset).
pub fn current_offset(handle: u64) -> KernelResult<u64> {
    let table = OPEN_FILES.lock();
    let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
    if file.is_directory {
        return Err(KernelError::IsADirectory);
    }
    Ok(file.offset)
}

/// Write bytes to the file at the current offset (or at EOF if APPEND).
///
/// Advances the offset by the number of bytes written.  Grows the
/// file if writing past the current end.  Returns bytes written.
pub fn write(handle: u64, data: &[u8]) -> KernelResult<usize> {
    let mut table = OPEN_FILES.lock();
    let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;

    if file.is_directory {
        return Err(KernelError::IsADirectory);
    }

    if !file.flags.is_writable() {
        return Err(KernelError::PermissionDenied);
    }

    let write_offset = if file.flags.contains(OpenFlags::APPEND) {
        file.size
    } else {
        file.offset
    };

    // Write via VFS.
    crate::fs::Vfs::write_at_resolved(&file.path, write_offset, data)?;

    let written = data.len();

    // Update offset and cached size.
    let new_end = write_offset.saturating_add(written as u64);
    if !file.flags.contains(OpenFlags::APPEND) {
        file.offset = new_end;
    } else {
        // APPEND: offset tracks end of file.
        file.offset = new_end;
    }
    if new_end > file.size {
        file.size = new_end;
    }

    Ok(written)
}

/// Read up to `buf.len()` bytes starting at an explicit `offset`,
/// **without** modifying the file's current offset.
///
/// This is the backbone of `pread64(2)` and `preadv(2)` — they're
/// defined to be atomic with respect to the current offset (no
/// observable change after the call returns).  We achieve that by
/// going straight to the VFS with the caller-supplied offset and
/// never touching `file.offset`.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] — `handle` is not in the table.
/// - [`KernelError::PermissionDenied`] — handle was not opened for
///   reading.
/// - VFS errors propagated unchanged.
pub fn read_at(handle: u64, offset: u64, buf: &mut [u8]) -> KernelResult<usize> {
    let table = OPEN_FILES.lock();
    let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
    if file.is_directory {
        return Err(KernelError::IsADirectory);
    }
    if !file.flags.is_readable() {
        return Err(KernelError::PermissionDenied);
    }
    if buf.is_empty() {
        return Ok(0);
    }
    if offset >= file.size {
        return Ok(0);
    }
    let data = crate::fs::Vfs::read_at_resolved(&file.path, offset, buf.len())?;
    let copy_len = data.len().min(buf.len());
    if let Some(dest) = buf.get_mut(..copy_len) {
        if let Some(src) = data.get(..copy_len) {
            dest.copy_from_slice(src);
        }
    }
    Ok(copy_len)
}

/// Read up to `buf.len()` bytes at an explicit `offset` **directly from the
/// backing filesystem**, bypassing the page cache.
///
/// Identical to [`read_at`] except it routes through
/// [`crate::fs::Vfs::read_at_uncached`].  Used by the `mmap` fault path's
/// page-cache fill closure, which must read a file's data *without* re-entering
/// [`crate::mm::page_cache::get_or_fill`] — calling the cached [`read_at`] there
/// would recurse on the very page being filled (design-decisions §38).
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] — `handle` is not in the table.
/// - [`KernelError::IsADirectory`] — `handle` refers to a directory.
/// - [`KernelError::PermissionDenied`] — handle was not opened for reading.
/// - VFS errors propagated unchanged.
pub fn read_at_uncached(handle: u64, offset: u64, buf: &mut [u8]) -> KernelResult<usize> {
    let (path, size) = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        if file.is_directory {
            return Err(KernelError::IsADirectory);
        }
        if !file.flags.is_readable() {
            return Err(KernelError::PermissionDenied);
        }
        (file.path.clone(), file.size)
    };
    if buf.is_empty() || offset >= size {
        return Ok(0);
    }
    let data = crate::fs::Vfs::read_at_uncached_resolved(&path, offset, buf.len())?;
    let copy_len = data.len().min(buf.len());
    if let Some(dest) = buf.get_mut(..copy_len) {
        if let Some(src) = data.get(..copy_len) {
            dest.copy_from_slice(src);
        }
    }
    Ok(copy_len)
}

/// Write bytes at an explicit `offset`, **without** modifying the
/// file's current offset.
///
/// Backbone of `pwrite64(2)` and `pwritev(2)`.  Linux ignores the
/// `O_APPEND` flag for `pwrite` (POSIX: "the offset argument shall be
/// used and the file offset shall not be changed") — we follow that
/// rule.  Grows the cached size if the write extends past the
/// current end-of-file.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] — `handle` is not in the table.
/// - [`KernelError::PermissionDenied`] — handle was not opened for
///   writing.
/// - VFS errors propagated unchanged.
pub fn write_at(handle: u64, offset: u64, data: &[u8]) -> KernelResult<usize> {
    let mut table = OPEN_FILES.lock();
    let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;
    if file.is_directory {
        return Err(KernelError::IsADirectory);
    }
    if !file.flags.is_writable() {
        return Err(KernelError::PermissionDenied);
    }
    if data.is_empty() {
        return Ok(0);
    }
    crate::fs::Vfs::write_at_resolved(&file.path, offset, data)?;
    let written = data.len();
    let new_end = offset.saturating_add(written as u64);
    if new_end > file.size {
        file.size = new_end;
    }
    Ok(written)
}

/// Read a page of directory entries starting at the handle's current
/// cursor, **without** advancing the cursor.
///
/// Returns `(start_cursor, entries)` where `start_cursor` is the cursor
/// value that was in effect when the read began (so the caller can
/// compute the next cursor by adding the number of entries consumed
/// and then invoke [`set_dir_cursor`]).
///
/// Directory handles store the cursor in the same `offset` field used
/// by file handles for byte position — interpreted here as an entry
/// index into the underlying VFS listing.  The split between "read
/// page" and "advance cursor" matches the `getdents64(2)` contract:
/// the caller may consume only a prefix of the returned entries when
/// the userspace buffer fills up mid-page, and must then advance the
/// cursor by exactly that prefix length, not by the full page.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] — `handle` is not in the table.
/// - [`KernelError::NotADirectory`] — `handle` refers to a file.
/// - VFS errors propagated unchanged.
pub fn read_dir_at(
    handle: u64,
    max_entries: usize,
) -> KernelResult<(u64, alloc::vec::Vec<crate::fs::DirEntry>)> {
    let (path, cursor) = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        if !file.is_directory {
            return Err(KernelError::NotADirectory);
        }
        (file.path.clone(), file.offset)
    };

    let start = usize::try_from(cursor).map_err(|_| KernelError::InvalidArgument)?;
    let (entries, _total) = crate::fs::Vfs::readdir_at_resolved(&path, start, max_entries)?;
    Ok((cursor, entries))
}

/// Advance (or rewind) a directory handle's cursor.
///
/// `cursor` is an entry index — typically the previous cursor plus the
/// number of entries actually consumed by userspace.  Passing 0 rewinds
/// the iteration to the start, mirroring `rewinddir(3)`.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] — `handle` is not in the table.
/// - [`KernelError::NotADirectory`] — `handle` refers to a file.
pub fn set_dir_cursor(handle: u64, cursor: u64) -> KernelResult<()> {
    let mut table = OPEN_FILES.lock();
    let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;
    if !file.is_directory {
        return Err(KernelError::NotADirectory);
    }
    file.offset = cursor;
    Ok(())
}

/// Seek to a new position in the file.
///
/// Returns the new absolute offset after seeking.
pub fn seek(handle: u64, from: SeekFrom) -> KernelResult<u64> {
    let mut table = OPEN_FILES.lock();
    let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;

    if file.is_directory {
        return Err(KernelError::IsADirectory);
    }

    let new_offset = match from {
        SeekFrom::Start(pos) => pos,
        SeekFrom::Current(delta) => {
            if delta >= 0 {
                #[allow(clippy::cast_sign_loss)]
                let d = delta as u64;
                file.offset.checked_add(d).ok_or(KernelError::InvalidArgument)?
            } else {
                #[allow(clippy::cast_sign_loss)]
                let d = delta.unsigned_abs();
                file.offset.checked_sub(d).ok_or(KernelError::InvalidArgument)?
            }
        }
        SeekFrom::End(delta) => {
            if delta >= 0 {
                #[allow(clippy::cast_sign_loss)]
                let d = delta as u64;
                file.size.checked_add(d).ok_or(KernelError::InvalidArgument)?
            } else {
                #[allow(clippy::cast_sign_loss)]
                let d = delta.unsigned_abs();
                file.size.checked_sub(d).ok_or(KernelError::InvalidArgument)?
            }
        }
        SeekFrom::Data(pos) => {
            // For non-sparse filesystems, any offset within the file is "data".
            // Return the requested offset if it's within the file.
            if pos >= file.size {
                return Err(KernelError::InvalidArgument);
            }
            pos
        }
        SeekFrom::Hole(pos) => {
            // For non-sparse filesystems, the first "hole" is at EOF.
            // If pos is already past EOF, that's an error.
            if pos > file.size {
                return Err(KernelError::InvalidArgument);
            }
            // Return EOF as the first hole.
            file.size
        }
    };

    file.offset = new_offset;
    Ok(new_offset)
}

/// Stat an open file handle, returning the full file metadata.
///
/// Resolves the handle to its backing path and queries the VFS for
/// rich metadata (size, type, timestamps, ownership, permissions,
/// link count, block count).  Avoids a redundant user-side path
/// lookup; the kernel already holds the resolved path for the handle.
pub fn fstat(handle: u64) -> KernelResult<crate::fs::FileMeta> {
    let table = OPEN_FILES.lock();
    let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;

    // metadata() follows symlinks, but an open handle already refers to
    // the resolved target, so this returns the correct underlying object.
    crate::fs::Vfs::metadata_resolved(&file.path)
}

/// Returns whether an open handle refers to a directory.
///
/// Cheap table lookup of the cached `is_directory` flag (no VFS round-trip).
/// Used by the `fstat`/`statx` syscall translators to report `S_IFDIR` for a
/// directory fd — without this, an `open(O_DIRECTORY)` handle (which is a
/// `HandleKind::File` fd) would stat as a regular file, breaking glibc's
/// `opendir`, which `fstat`s the fd and bails with `ENOTDIR` unless it sees
/// `S_ISDIR`.  Returns `false` for an unknown handle (the caller then reports
/// the default regular-file type, matching the pre-existing behaviour).
#[must_use]
pub fn is_directory(handle: u64) -> bool {
    let table = OPEN_FILES.lock();
    table.get(&handle).is_some_and(|file| file.is_directory)
}

/// Truncate a file to a given size by handle.
///
/// Requires the handle to be opened with WRITE permission.
/// Updates the cached size and clamps the offset if it was
/// beyond the new end-of-file.
pub fn ftruncate(handle: u64, size: u64) -> KernelResult<()> {
    let mut table = OPEN_FILES.lock();
    let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;

    if file.is_directory {
        return Err(KernelError::IsADirectory);
    }

    if !file.flags.is_writable() {
        return Err(KernelError::PermissionDenied);
    }

    crate::fs::Vfs::truncate_resolved(&file.path, size)?;

    file.size = size;
    // Clamp offset: if the cursor was beyond the new EOF, move it back.
    if file.offset > size {
        file.offset = size;
    }

    Ok(())
}

/// Duplicate a file handle, creating a new handle that refers to the
/// same file with the same flags and an independent cursor position.
///
/// The new handle starts at the same offset as the original.
pub fn dup(handle: u64) -> KernelResult<u64> {
    let table = OPEN_FILES.lock();
    let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;

    let path = file.path.clone();
    let offset = file.offset;
    let size = file.size;
    let flags = file.flags;
    let is_directory = file.is_directory;

    // Need to drop the lock before calling allocate_handle (it
    // acquires the same lock).
    drop(table);

    if is_directory {
        // Preserve the cursor on the duplicate so callers iterating a
        // directory through a dup'd handle see the same position.
        let id = allocate_dir_handle(path, flags)?;
        // set_dir_cursor takes the same lock; safe now that allocate
        // already released it.
        set_dir_cursor(id, offset)?;
        Ok(id)
    } else {
        allocate_handle(path, offset, size, flags)
    }
}

/// Duplicate a file handle by sharing the *same* open file description.
///
/// Unlike [`dup`], this does not allocate a new handle id or a fresh
/// independent cursor — it bumps the refcount on the existing open file
/// description and returns the **same** handle id.  Both owners then
/// share one cursor: a read or write through either id advances the
/// offset for both.
///
/// This is the operation `fork()` needs: the child's userspace fd table
/// is copy-on-write cloned and therefore references the same kernel
/// handle ids as the parent, so the kernel must bump the refcount on
/// those exact ids rather than mint new ones (which the child's table
/// would never see).  It also matches POSIX `fork()` / `dup()` semantics
/// where the descriptions — and offsets — are shared.
///
/// # Returns
///
/// - `Ok(handle)` — refcount incremented; the same handle id is returned.
/// - `Err(InvalidHandle)` — no such open file, or the refcount would
///   overflow `u32::MAX`.
pub fn dup_shared(handle: u64) -> KernelResult<u64> {
    let mut table = OPEN_FILES.lock();
    let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;
    file.refcount = file.refcount.checked_add(1).ok_or(KernelError::InvalidHandle)?;
    Ok(handle)
}

/// Get the VFS path associated with an open handle.
///
/// Useful for diagnostics and `/proc/<pid>/fd` equivalent.
pub fn handle_path(handle: u64) -> KernelResult<String> {
    let table = OPEN_FILES.lock();
    let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
    Ok(file.path.clone())
}

/// Resolve the stable system-wide file identity of an open handle.
///
/// Returns the backing file's [`crate::fs::vfs::FileId`] (mount `fs_id`
/// plus inode number) by querying the VFS for the handle's cached
/// resolved path.  Used at mmap time to key the shared read-only page
/// cache.
///
/// - `Ok(Some(id))` — the backing filesystem exposes a stable inode.
/// - `Ok(None)` — no stable identity (`ino == 0`: FAT, ISO9660, pseudo
///   filesystems); the caller must fall back to the per-mapping read path.
/// - `Err(_)` — the handle is invalid or the path no longer resolves.
pub fn file_identity(handle: u64) -> KernelResult<Option<crate::fs::vfs::FileId>> {
    let path = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        file.path.clone()
    };
    crate::fs::Vfs::file_identity_resolved(&path)
}

/// Get the current number of open file handles (for diagnostics).
pub fn open_count() -> usize {
    OPEN_FILES.lock().len()
}

/// Snapshot of an open file handle's state (for /proc/fdinfo).
pub struct HandleInfo {
    /// Handle ID.
    pub id: u64,
    /// VFS path.
    pub path: String,
    /// Current offset.
    pub offset: u64,
    /// Cached file size.
    pub size: u64,
    /// Open flags (raw bits).
    pub flags: u32,
}

/// Enumerate all open file handles for diagnostics.
///
/// Returns a snapshot of every open handle.  Used by `/proc/fdinfo`.
pub fn list_handles() -> alloc::vec::Vec<HandleInfo> {
    let table = OPEN_FILES.lock();
    let mut result = alloc::vec::Vec::with_capacity(table.len());
    for (&id, file) in table.iter() {
        result.push(HandleInfo {
            id,
            path: file.path.clone(),
            offset: file.offset,
            size: file.size,
            flags: file.flags.bits(),
        });
    }
    result
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Allocate a handle in the global table.
fn allocate_handle(
    path: alloc::string::String,
    offset: u64,
    size: u64,
    flags: OpenFlags,
) -> KernelResult<u64> {
    let mut table = OPEN_FILES.lock();

    if table.len() >= MAX_OPEN_FILES {
        return Err(KernelError::OutOfMemory);
    }

    let id = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);

    table.insert(
        id,
        OpenFile {
            path,
            offset,
            size,
            flags,
            refcount: 1,
            is_directory: false,
        },
    );

    Ok(id)
}

/// Allocate a directory handle in the global table.
///
/// Directory handles use `offset` as an entry cursor and `size = 0`
/// (the underlying VFS doesn't track a stable entry count cheaply, so
/// we don't cache one — `read_dir_at` queries the VFS each time).
fn allocate_dir_handle(
    path: alloc::string::String,
    flags: OpenFlags,
) -> KernelResult<u64> {
    let mut table = OPEN_FILES.lock();

    if table.len() >= MAX_OPEN_FILES {
        return Err(KernelError::OutOfMemory);
    }

    let id = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);

    table.insert(
        id,
        OpenFile {
            path,
            offset: 0,
            size: 0,
            flags,
            refcount: 1,
            is_directory: true,
        },
    );

    Ok(id)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Test the file handle system end-to-end.
///
/// Requires a mounted filesystem (skips gracefully if none available).
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[fs::handle] Running self-test...");

    // Try to create a test file.  If no FS is mounted, skip.
    let test_path = "/handle_test.txt";
    let test_data = b"Hello from handle test!";

    if crate::fs::Vfs::write_file(test_path, test_data).is_err() {
        crate::serial_println!("[fs::handle] Self-test SKIPPED (no FS mounted)");
        return Ok(());
    }

    // 1. Open for reading.
    let h = open(test_path, OpenFlags::READ)?;
    crate::serial_println!("[fs::handle]   open(READ) → handle {}", h);

    // 2. Read and verify.
    let mut buf = [0u8; 64];
    let n = read(h, &mut buf)?;
    if n != test_data.len() {
        crate::serial_println!(
            "[fs::handle]   FAIL: read returned {}, expected {}",
            n,
            test_data.len()
        );
        close(h).ok();
        return Err(KernelError::InternalError);
    }
    if buf.get(..n) != Some(test_data.as_slice()) {
        crate::serial_println!("[fs::handle]   FAIL: read data mismatch");
        close(h).ok();
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   read {} bytes: OK", n);

    // 3. Read again — should be at EOF, return 0.
    let n2 = read(h, &mut buf)?;
    if n2 != 0 {
        crate::serial_println!("[fs::handle]   FAIL: expected 0 at EOF, got {}", n2);
        close(h).ok();
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   read at EOF: 0 bytes (correct)");

    // 4. Seek back to start.
    let pos = seek(h, SeekFrom::Start(0))?;
    if pos != 0 {
        crate::serial_println!("[fs::handle]   FAIL: seek Start(0) returned {}", pos);
        close(h).ok();
        return Err(KernelError::InternalError);
    }

    // 5. Seek forward from current.
    let pos2 = seek(h, SeekFrom::Current(5))?;
    if pos2 != 5 {
        crate::serial_println!("[fs::handle]   FAIL: seek Current(5) returned {}", pos2);
        close(h).ok();
        return Err(KernelError::InternalError);
    }

    // 6. Read from offset 5.
    let n3 = read(h, &mut buf)?;
    if let Some(expected) = test_data.get(5..) {
        if n3 != expected.len() || buf.get(..n3) != Some(expected) {
            crate::serial_println!("[fs::handle]   FAIL: read from offset 5 mismatch");
            close(h).ok();
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[fs::handle]   seek + read from offset 5: OK");

    // 7. Close.
    close(h)?;
    crate::serial_println!("[fs::handle]   close: OK");

    // 8. Verify closed handle is rejected.
    let result = read(h, &mut buf);
    if result != Err(KernelError::InvalidHandle) {
        crate::serial_println!("[fs::handle]   FAIL: read on closed handle should return InvalidHandle");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   read after close: InvalidHandle (correct)");

    // 9. Test write via handle.
    let hw = open(
        "/handle_write_test.txt",
        OpenFlags::WRITE.union(OpenFlags::CREATE).union(OpenFlags::READ),
    )?;
    let write_data = b"Written via handle!";
    let nw = write(hw, write_data)?;
    if nw != write_data.len() {
        crate::serial_println!("[fs::handle]   FAIL: write returned {}", nw);
        close(hw).ok();
        return Err(KernelError::InternalError);
    }

    // Seek back and verify.
    seek(hw, SeekFrom::Start(0))?;
    let nr = read(hw, &mut buf)?;
    if nr != write_data.len() || buf.get(..nr) != Some(write_data.as_slice()) {
        crate::serial_println!("[fs::handle]   FAIL: write+read verification failed");
        close(hw).ok();
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   write + read-back: OK");

    // 10. fstat.
    let stat_result = fstat(hw)?;
    if stat_result.size != write_data.len() as u64
        || stat_result.entry_type != crate::fs::EntryType::File
    {
        crate::serial_println!(
            "[fs::handle]   FAIL: fstat size={} type={:?}, expected size={} type=File",
            stat_result.size,
            stat_result.entry_type,
            write_data.len()
        );
        close(hw).ok();
        return Err(KernelError::InternalError);
    }
    crate::serial_println!(
        "[fs::handle]   fstat: OK (size={}, type=file, nlinks={})",
        stat_result.size, stat_result.nlinks
    );

    // 11. ftruncate.
    let trunc_size = 7u64;
    ftruncate(hw, trunc_size)?;
    let trunc_stat = fstat(hw)?;
    if trunc_stat.size != trunc_size {
        crate::serial_println!(
            "[fs::handle]   FAIL: ftruncate to {} but fstat shows {}",
            trunc_size, trunc_stat.size
        );
        close(hw).ok();
        return Err(KernelError::InternalError);
    }
    // Read back the truncated content.
    seek(hw, SeekFrom::Start(0))?;
    let nt = read(hw, &mut buf)?;
    if nt != trunc_size as usize {
        crate::serial_println!("[fs::handle]   FAIL: read after truncate returned {} bytes", nt);
        close(hw).ok();
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   ftruncate to {} bytes: OK", trunc_size);

    // 12. dup — duplicate handle, verify independent cursor.
    seek(hw, SeekFrom::Start(0))?;
    let hdup = dup(hw)?;
    // Read 3 bytes from original — advances original cursor.
    let n_orig = read(hw, &mut buf[..3])?;
    if n_orig != 3 {
        crate::serial_println!("[fs::handle]   FAIL: read 3 from original got {}", n_orig);
        close(hdup).ok();
        close(hw).ok();
        return Err(KernelError::InternalError);
    }
    // Dup'd handle was at offset 0 when dup'd — read should start there.
    let n_dup = read(hdup, &mut buf[..3])?;
    if n_dup != 3 {
        crate::serial_println!("[fs::handle]   FAIL: read 3 from dup got {}", n_dup);
        close(hdup).ok();
        close(hw).ok();
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   dup: independent cursor OK");

    // 13. handle_path.
    let path_check = handle_path(hw)?;
    if path_check != "/handle_write_test.txt" {
        crate::serial_println!("[fs::handle]   FAIL: handle_path = '{}'", path_check);
        close(hdup).ok();
        close(hw).ok();
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   handle_path: '{}' OK", path_check);

    close(hdup)?;
    close(hw)?;

    // 13b. dup_shared — fork-style shared open file description.
    //
    // Both ids share one cursor (a read through either advances both)
    // and refcounted close keeps the description alive until the last
    // owner closes it.
    let hs = open("/handle_write_test.txt", OpenFlags::READ)?;
    seek(hs, SeekFrom::Start(0))?;
    let hs2 = dup_shared(hs)?;
    // dup_shared returns the SAME id (shared description).
    if hs2 != hs {
        crate::serial_println!(
            "[fs::handle]   FAIL: dup_shared returned new id {} (expected same {})",
            hs2, hs
        );
        close(hs).ok();
        return Err(KernelError::InternalError);
    }
    // Read 3 bytes through hs — advances the shared cursor.
    let s_a = read(hs, &mut buf[..3])?;
    // Read 3 bytes through hs2 — continues from the SAME (advanced)
    // offset, since the description is shared.  If cursors were
    // independent this would re-read the first 3 bytes.
    let off_after = seek(hs2, SeekFrom::Current(0))?;
    if s_a != 3 || off_after != 3 {
        crate::serial_println!(
            "[fs::handle]   FAIL: shared cursor: read {} bytes, offset now {} (expected 3/3)",
            s_a, off_after
        );
        close(hs).ok();
        return Err(KernelError::InternalError);
    }
    // First close drops one reference; the description must survive.
    close(hs2)?;
    let still_open = read(hs, &mut buf[..1]);
    if still_open.is_err() {
        crate::serial_println!(
            "[fs::handle]   FAIL: description freed after first of two closes: {:?}",
            still_open
        );
        close(hs).ok();
        return Err(KernelError::InternalError);
    }
    // Second close drops the last reference; now it must be gone.
    close(hs)?;
    if read(hs, &mut buf[..1]) != Err(KernelError::InvalidHandle) {
        crate::serial_println!("[fs::handle]   FAIL: description not freed after final close");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   dup_shared: shared cursor + refcounted close OK");

    // 14. Lock-on-close: verify advisory locks are released when handle closes.
    let lock_path = "/handle_lock_test.txt";
    crate::fs::Vfs::write_file(lock_path, b"lock test data")?;

    let hlock = open(lock_path, OpenFlags::READ.union(OpenFlags::WRITE))?;

    // Acquire an exclusive lock using the handle ID as owner.
    crate::fs::Vfs::flock(lock_path, hlock, crate::fs::LockType::Exclusive)?;

    // Verify lock is held.
    match crate::fs::Vfs::lock_query(lock_path) {
        Ok(Some(_)) => {} // Lock is held — expected.
        other => {
            crate::serial_println!("[fs::handle]   FAIL: lock not held after flock: {:?}", other);
            close(hlock).ok();
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[fs::handle]   flock(exclusive) on handle {}: OK", hlock);

    // Close the handle — this should auto-release the lock.
    close(hlock)?;

    // Verify lock was released.
    match crate::fs::Vfs::lock_query(lock_path) {
        Ok(None) => {} // Lock released — expected.
        other => {
            crate::serial_println!("[fs::handle]   FAIL: lock still held after close: {:?}", other);
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[fs::handle]   lock-on-close: auto-released OK");

    // 15. Directory-handle self-test.
    //
    // Create a small directory, populate it with two files, then exercise
    // open(DIRECTORY) / read_dir_at / set_dir_cursor and the rejection
    // paths (open without DIRECTORY → IsADirectory; open(file, DIRECTORY)
    // → NotADirectory; read/write on dir handle → IsADirectory).
    let dir_path = "/handle_dir_test";
    let file_a = "/handle_dir_test/a.txt";
    let file_b = "/handle_dir_test/b.txt";
    let file_outside = "/handle_outside.txt";

    // Cleanup any leftovers from a prior run.
    crate::fs::Vfs::remove(file_a).ok();
    crate::fs::Vfs::remove(file_b).ok();
    crate::fs::Vfs::remove(dir_path).ok();
    crate::fs::Vfs::remove(file_outside).ok();

    if crate::fs::Vfs::mkdir(dir_path).is_ok() {
        crate::fs::Vfs::write_file(file_a, b"a")?;
        crate::fs::Vfs::write_file(file_b, b"b")?;
        crate::fs::Vfs::write_file(file_outside, b"o")?;

        // (a) open(dir, READ) without DIRECTORY → IsADirectory.
        match open(dir_path, OpenFlags::READ) {
            Err(KernelError::IsADirectory) => {}
            other => {
                crate::serial_println!(
                    "[fs::handle]   FAIL: open(dir, READ) not IsADirectory: {:?}", other
                );
                return Err(KernelError::InternalError);
            }
        }

        // (b) open(file, READ|DIRECTORY) → NotADirectory.
        match open(file_outside, OpenFlags::READ.union(OpenFlags::DIRECTORY)) {
            Err(KernelError::NotADirectory) => {}
            other => {
                crate::serial_println!(
                    "[fs::handle]   FAIL: open(file, DIRECTORY) not NotADirectory: {:?}", other
                );
                return Err(KernelError::InternalError);
            }
        }

        // (c) open(dir, READ|DIRECTORY) → ok.
        let hd = open(dir_path, OpenFlags::READ.union(OpenFlags::DIRECTORY))?;

        // (d) byte-oriented ops on dir handle → IsADirectory.
        if read(hd, &mut buf) != Err(KernelError::IsADirectory) {
            crate::serial_println!("[fs::handle]   FAIL: read(dir-handle) not IsADirectory");
            close(hd).ok();
            return Err(KernelError::InternalError);
        }
        if write(hd, b"x") != Err(KernelError::IsADirectory) {
            crate::serial_println!("[fs::handle]   FAIL: write(dir-handle) not IsADirectory");
            close(hd).ok();
            return Err(KernelError::InternalError);
        }
        if read_at(hd, 0, &mut buf) != Err(KernelError::IsADirectory) {
            crate::serial_println!("[fs::handle]   FAIL: read_at(dir-handle) not IsADirectory");
            close(hd).ok();
            return Err(KernelError::InternalError);
        }
        if seek(hd, SeekFrom::Start(0)) != Err(KernelError::IsADirectory) {
            crate::serial_println!("[fs::handle]   FAIL: seek(dir-handle) not IsADirectory");
            close(hd).ok();
            return Err(KernelError::InternalError);
        }

        // (e) read_dir_at returns the two files (order is FS-defined; just
        //     verify both names appear).
        let (start, entries) = read_dir_at(hd, 16)?;
        if start != 0 {
            crate::serial_println!("[fs::handle]   FAIL: initial cursor not 0: {}", start);
            close(hd).ok();
            return Err(KernelError::InternalError);
        }
        let mut saw_a = false;
        let mut saw_b = false;
        for e in &entries {
            if e.name == "a.txt" { saw_a = true; }
            if e.name == "b.txt" { saw_b = true; }
        }
        if !(saw_a && saw_b) {
            crate::serial_println!(
                "[fs::handle]   FAIL: dir listing missing entries (saw_a={}, saw_b={})",
                saw_a, saw_b
            );
            close(hd).ok();
            return Err(KernelError::InternalError);
        }

        // (f) advance cursor past the entries, next read_dir_at returns empty.
        set_dir_cursor(hd, entries.len() as u64)?;
        let (_, page2) = read_dir_at(hd, 16)?;
        if !page2.is_empty() {
            crate::serial_println!(
                "[fs::handle]   FAIL: dir listing after exhaustion not empty ({} entries)",
                page2.len()
            );
            close(hd).ok();
            return Err(KernelError::InternalError);
        }

        // (g) rewind via set_dir_cursor(0).
        set_dir_cursor(hd, 0)?;
        let (_, page3) = read_dir_at(hd, 16)?;
        if page3.len() != entries.len() {
            crate::serial_println!(
                "[fs::handle]   FAIL: rewind+read mismatch ({} vs {})",
                page3.len(), entries.len()
            );
            close(hd).ok();
            return Err(KernelError::InternalError);
        }

        // (h) read_dir_at on a file handle → NotADirectory.
        let hf = open(file_outside, OpenFlags::READ)?;
        if let Err(e) = read_dir_at(hf, 16) {
            if e != KernelError::NotADirectory {
                crate::serial_println!(
                    "[fs::handle]   FAIL: read_dir_at(file-handle) wrong err: {:?}", e
                );
                close(hf).ok();
                close(hd).ok();
                return Err(KernelError::InternalError);
            }
        } else {
            crate::serial_println!("[fs::handle]   FAIL: read_dir_at(file-handle) not Err");
            close(hf).ok();
            close(hd).ok();
            return Err(KernelError::InternalError);
        }
        close(hf)?;
        close(hd)?;

        // Cleanup.
        crate::fs::Vfs::remove(file_a).ok();
        crate::fs::Vfs::remove(file_b).ok();
        crate::fs::Vfs::remove(dir_path).ok();
        crate::fs::Vfs::remove(file_outside).ok();

        crate::serial_println!("[fs::handle]   directory handle ops: OK");
    } else {
        crate::serial_println!("[fs::handle]   directory handle test SKIPPED (mkdir failed)");
    }

    // Cleanup test files.
    crate::fs::Vfs::remove(lock_path).ok();
    crate::fs::Vfs::remove(test_path).ok();
    crate::fs::Vfs::remove("/handle_write_test.txt").ok();

    crate::serial_println!("[fs::handle] Self-test PASSED");
    Ok(())
}
