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
use spin::Mutex;

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

    // Resolve symlinks at open time so the handle refers to the
    // underlying file, not the symlink.  This matches Unix semantics:
    // if the symlink is later changed, existing handles still point
    // to the original target.
    let norm = crate::fs::Vfs::resolve_path(path)?;

    // Check if the file exists.
    let stat_result = crate::fs::Vfs::stat(&norm);

    match stat_result {
        Ok(entry) => {
            // File exists.
            if entry.entry_type == crate::fs::EntryType::Directory {
                return Err(KernelError::IsADirectory);
            }

            let mut size = entry.size;

            // Handle TRUNCATE flag.
            if flags.contains(OpenFlags::TRUNCATE) {
                if !flags.is_writable() {
                    return Err(KernelError::InvalidArgument);
                }
                crate::fs::Vfs::truncate(&norm, 0)?;
                size = 0;
            }

            let offset = if flags.contains(OpenFlags::APPEND) {
                size
            } else {
                0
            };

            allocate_handle(norm, offset, size, flags)
        }
        Err(KernelError::NotFound) => {
            // File doesn't exist — create if CREATE is set.
            if !flags.contains(OpenFlags::CREATE) {
                return Err(KernelError::NotFound);
            }
            if !flags.is_writable() {
                return Err(KernelError::InvalidArgument);
            }

            // Create an empty file.
            crate::fs::Vfs::write_file(&norm, &[])?;

            allocate_handle(norm, 0, 0, flags)
        }
        Err(e) => Err(e),
    }
}

/// Close an open file handle.
///
/// Frees the handle ID.  Further operations on this handle will
/// return `InvalidHandle`.
pub fn close(handle: u64) -> KernelResult<()> {
    let mut table = OPEN_FILES.lock();
    if table.remove(&handle).is_some() {
        Ok(())
    } else {
        Err(KernelError::InvalidHandle)
    }
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

    if !file.flags.is_readable() {
        return Err(KernelError::PermissionDenied);
    }

    // Nothing to read if at or past EOF.
    if file.offset >= file.size {
        return Ok(0);
    }

    // Read via VFS.  Currently reads the whole file — the default
    // `read_at` in the FileSystem trait slices the result.
    let data = crate::fs::Vfs::read_at(&file.path, file.offset, buf.len())?;
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

/// Write bytes to the file at the current offset (or at EOF if APPEND).
///
/// Advances the offset by the number of bytes written.  Grows the
/// file if writing past the current end.  Returns bytes written.
pub fn write(handle: u64, data: &[u8]) -> KernelResult<usize> {
    let mut table = OPEN_FILES.lock();
    let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;

    if !file.flags.is_writable() {
        return Err(KernelError::PermissionDenied);
    }

    let write_offset = if file.flags.contains(OpenFlags::APPEND) {
        file.size
    } else {
        file.offset
    };

    // Write via VFS.
    crate::fs::Vfs::write_at(&file.path, write_offset, data)?;

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

/// Seek to a new position in the file.
///
/// Returns the new absolute offset after seeking.
pub fn seek(handle: u64, from: SeekFrom) -> KernelResult<u64> {
    let mut table = OPEN_FILES.lock();
    let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;

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
    };

    file.offset = new_offset;
    Ok(new_offset)
}

/// Stat a file by handle (avoids redundant path lookup).
///
/// Returns `(size, entry_type_byte)` where entry_type_byte is 0=file.
pub fn fstat(handle: u64) -> KernelResult<(u64, u8)> {
    let table = OPEN_FILES.lock();
    let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;

    // Refresh size from VFS in case another handle modified the file.
    let entry = crate::fs::Vfs::stat(&file.path)?;
    // File handles only refer to files, not directories.
    Ok((entry.size, 0))
}

/// Truncate a file to a given size by handle.
///
/// Requires the handle to be opened with WRITE permission.
/// Updates the cached size and clamps the offset if it was
/// beyond the new end-of-file.
pub fn ftruncate(handle: u64, size: u64) -> KernelResult<()> {
    let mut table = OPEN_FILES.lock();
    let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;

    if !file.flags.is_writable() {
        return Err(KernelError::PermissionDenied);
    }

    crate::fs::Vfs::truncate(&file.path, size)?;

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

    // Need to drop the lock before calling allocate_handle (it
    // acquires the same lock).
    drop(table);

    allocate_handle(path, offset, size, flags)
}

/// Get the VFS path associated with an open handle.
///
/// Useful for diagnostics and `/proc/<pid>/fd` equivalent.
pub fn handle_path(handle: u64) -> KernelResult<String> {
    let table = OPEN_FILES.lock();
    let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
    Ok(file.path.clone())
}

/// Get the current number of open file handles (for diagnostics).
#[allow(dead_code)]
pub fn open_count() -> usize {
    OPEN_FILES.lock().len()
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
    let (size, etype) = fstat(hw)?;
    if size != write_data.len() as u64 || etype != 0 {
        crate::serial_println!(
            "[fs::handle]   FAIL: fstat size={} type={}, expected size={} type=0",
            size,
            etype,
            write_data.len()
        );
        close(hw).ok();
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   fstat: OK (size={}, type=file)", size);

    // 11. ftruncate.
    let trunc_size = 7u64;
    ftruncate(hw, trunc_size)?;
    let (new_size, _) = fstat(hw)?;
    if new_size != trunc_size {
        crate::serial_println!(
            "[fs::handle]   FAIL: ftruncate to {} but fstat shows {}",
            trunc_size, new_size
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

    // Cleanup test files.
    crate::fs::Vfs::remove(test_path).ok();
    crate::fs::Vfs::remove("/handle_write_test.txt").ok();

    crate::serial_println!("[fs::handle] Self-test PASSED");
    Ok(())
}
