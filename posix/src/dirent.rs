//! POSIX directory entry functions.
//!
//! Implements `opendir`, `readdir`, `closedir`, `rewinddir`,
//! `seekdir`, `telldir`, `dirfd`, `alphasort`, and `scandir` for
//! directory iteration.
//!
//! Our kernel provides `SYS_FS_LIST_DIR` which returns the full directory
//! listing at once (not incremental).  We buffer the results and return
//! them one entry at a time via `readdir()`.
//!
//! ## Limitations
//!
//! - Maximum 256 entries per directory (kernel buffer limit).
//! - `dirfd` returns -1 for `opendir()`-created streams (no fd kept).
//!   For `fdopendir()`-created streams, returns the original fd.
//! - `fdopendir()` relies on path-at-open tracking and may use a stale
//!   path if the directory was renamed after the fd was opened.

use crate::errno;
use crate::syscall::*;
use crate::types::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum directory entries we can read.
const MAX_DIR_ENTRIES: usize = 256;

/// Size of one kernel directory entry (name[256] + size[4] + type[1] + pad[3]).
const DIR_ENTRY_SIZE: usize = 264;

// ---------------------------------------------------------------------------
// dirent — POSIX directory entry
// ---------------------------------------------------------------------------

/// Directory entry type constants (from kernel).
pub const DT_UNKNOWN: u8 = 0;
pub const DT_REG: u8 = 1;  // Regular file.
pub const DT_DIR: u8 = 2;  // Directory.
pub const DT_LNK: u8 = 3;  // Symbolic link.

/// POSIX directory entry.
#[repr(C)]
pub struct Dirent {
    /// Inode number (we use a synthetic value).
    pub d_ino: InoT,
    /// Offset to next entry.
    pub d_off: OffT,
    /// Length of this record.
    pub d_reclen: u16,
    /// File type (DT_REG, DT_DIR, etc.).
    pub d_type: u8,
    /// Null-terminated filename.
    pub d_name: [u8; 256],
}

/// Opaque directory stream handle.
///
/// Contains the buffered directory listing from `SYS_FS_LIST_DIR`
/// and tracks the current position for `readdir()`.
pub struct Dir {
    /// Buffered directory entries from the kernel.
    /// Each entry is DIR_ENTRY_SIZE bytes: name[256] + size[4] + type[1] + pad[3].
    buf: [u8; MAX_DIR_ENTRIES * DIR_ENTRY_SIZE],
    /// Number of entries in the buffer.
    count: usize,
    /// Current position (index of next entry to return).
    pos: usize,
    /// Scratch space for the dirent we return.
    current: Dirent,
    /// File descriptor owned by this Dir (from `fdopendir`).
    ///
    /// When >= 0, `closedir()` will close this fd.  Set to -1 for
    /// directories opened via `opendir()` (which don't own an fd).
    owned_fd: i32,
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Open a directory stream.
///
/// Returns a pointer to a Dir, or NULL on error.
///
/// The returned `Dir` is heap-like but since we're `no_std`, we use
/// a static pool of Dir structs.  Maximum 8 concurrent open directories.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn opendir(name: *const u8) -> *mut Dir {
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }

    // Resolve relative paths against CWD.
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = (unsafe { crate::unistd::resolve_path(name, &mut resolved) }) else {
        // POSIX: empty path → ENOENT, too-long path → ENAMETOOLONG.
        // SAFETY: name is non-null (checked above) and a valid C-string.
        if unsafe { *name } == 0 {
            errno::set_errno(errno::ENOENT);
        } else {
            errno::set_errno(errno::ENAMETOOLONG);
        }
        return core::ptr::null_mut();
    };

    // Allocate a Dir from the static pool.
    let dir_ptr = alloc_dir();
    if dir_ptr.is_null() {
        errno::set_errno(errno::EMFILE);
        return core::ptr::null_mut();
    }

    // SAFETY: alloc_dir returned a valid, exclusively-owned Dir pointer.
    let dir = unsafe { &mut *dir_ptr };

    // Issue SYS_FS_LIST_DIR to get all entries at once.
    let ret = syscall3(
        SYS_FS_LIST_DIR,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        dir.buf.as_mut_ptr() as u64,
    );

    if ret < 0 {
        free_dir(dir_ptr);
        let _ = errno::translate(ret); // Called for side effect: sets errno.
        return core::ptr::null_mut();
    }

    dir.count = ret as usize;
    dir.pos = 0;

    dir_ptr
}

/// Read the next directory entry.
///
/// Returns a pointer to a `Dirent`, or NULL when the directory
/// is exhausted (end of listing).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn readdir(dirp: *mut Dir) -> *mut Dirent {
    if dirp.is_null() {
        errno::set_errno(errno::EBADF);
        return core::ptr::null_mut();
    }

    let dir = unsafe { &mut *dirp };

    if dir.pos >= dir.count {
        return core::ptr::null_mut(); // End of directory.
    }

    // Parse the kernel entry at current position.
    let offset = dir.pos.wrapping_mul(DIR_ENTRY_SIZE);
    if offset.wrapping_add(DIR_ENTRY_SIZE) > dir.buf.len() {
        return core::ptr::null_mut();
    }

    // Copy name (first 256 bytes of the entry).
    // Bounds are guaranteed by the check above: offset + DIR_ENTRY_SIZE (264) <= buf.len().
    dir.current.d_name = [0u8; 256];
    let end = offset.wrapping_add(256);
    if let Some(name_slice) = dir.buf.get(offset..end) {
        dir.current.d_name[..256].copy_from_slice(name_slice);
    }

    // Type byte is at offset 260 within the entry.
    dir.current.d_type = dir.buf.get(offset.wrapping_add(260)).copied().unwrap_or(0);

    // Synthetic inode from position.
    dir.current.d_ino = dir.pos as u64;
    dir.current.d_off = dir.pos as i64;
    dir.current.d_reclen = core::mem::size_of::<Dirent>() as u16;

    dir.pos = dir.pos.wrapping_add(1);

    core::ptr::addr_of_mut!(dir.current)
}

/// Close a directory stream.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn closedir(dirp: *mut Dir) -> i32 {
    if dirp.is_null() {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    // If this Dir owns an fd (from fdopendir), close it.
    // SAFETY: dirp is valid (checked above).
    let owned_fd = unsafe { (*dirp).owned_fd };
    if owned_fd >= 0 {
        crate::file::close(owned_fd);
    }

    free_dir(dirp);
    0
}

/// Reset a directory stream to the beginning.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn rewinddir(dirp: *mut Dir) {
    if dirp.is_null() {
        return;
    }
    // SAFETY: dirp is valid (caller contract).
    unsafe { (*dirp).pos = 0; }
}

/// Return the current position in the directory stream.
///
/// The returned value can be passed to `seekdir` to return to this
/// position.  In our implementation, the position is simply the entry
/// index.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn telldir(dirp: *mut Dir) -> i64 {
    if dirp.is_null() {
        return -1;
    }
    // SAFETY: dirp is valid.
    unsafe { (*dirp).pos as i64 }
}

/// Set the position of the directory stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn seekdir(dirp: *mut Dir, loc: i64) {
    if dirp.is_null() || loc < 0 {
        return;
    }
    // SAFETY: dirp is valid.
    unsafe {
        let max = (*dirp).count;
        (*dirp).pos = if (loc as usize) > max { max } else { loc as usize };
    }
}

/// Get the file descriptor associated with a directory stream.
///
/// Returns the fd if the Dir was created via `fdopendir()`, or -1
/// if it was created via `opendir()` (which doesn't retain an fd).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dirfd(dirp: *mut Dir) -> i32 {
    if dirp.is_null() {
        return -1;
    }
    // SAFETY: dirp is valid (caller contract).
    unsafe { (*dirp).owned_fd }
}

/// Compare two directory entries alphabetically by name.
///
/// Suitable as a comparator for `scandir`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn alphasort(a: *const *const Dirent, b: *const *const Dirent) -> i32 {
    if a.is_null() || b.is_null() {
        return 0;
    }
    // SAFETY: a and b point to valid Dirent pointers.
    let da = unsafe { &**a };
    let db = unsafe { &**b };

    // Compare d_name byte by byte.
    let mut i: usize = 0;
    loop {
        let ca = da.d_name.get(i).copied().unwrap_or(0);
        let cb = db.d_name.get(i).copied().unwrap_or(0);
        if ca != cb {
            return i32::from(ca).wrapping_sub(i32::from(cb));
        }
        if ca == 0 {
            return 0;
        }
        i = i.wrapping_add(1);
    }
}

/// Insertion sort a `*mut Dirent` array of `count` elements using `cmp`.
///
/// # Safety
///
/// `arr` must point to `count` valid, writable `*mut Dirent` entries.
/// `cmp` must be a valid function pointer that never returns from a panic.
///
/// # Alignment note
///
/// `arr` is cast from a `*mut u8` returned by `malloc`.  Our `malloc`
/// implementation uses `mmap`, which returns page-aligned memory (≥ 4096
/// bytes), far exceeding the 8-byte alignment required for `*mut Dirent`.
#[allow(clippy::cast_ptr_alignment)]
unsafe fn scandir_sort(
    arr: *mut u8,
    count: usize,
    cmp: extern "C" fn(*const *const Dirent, *const *const Dirent) -> i32,
) {
    // SAFETY: caller guarantees arr is page-aligned and has count entries.
    let arr_typed = arr.cast::<*mut Dirent>();
    let mut i: usize = 1;
    while i < count {
        let mut j = i;
        while j > 0 {
            // SAFETY: j and j-1 are valid indices within [0, count).
            let a = unsafe { arr_typed.add(j.wrapping_sub(1)) };
            let b = unsafe { arr_typed.add(j) };
            if cmp(a.cast::<*const Dirent>(), b.cast::<*const Dirent>()) > 0 {
                // SAFETY: a and b are valid, non-overlapping, aligned pointers.
                unsafe { core::ptr::swap(a, b); }
                j = j.wrapping_sub(1);
            } else {
                break;
            }
        }
        i = i.wrapping_add(1);
    }
}

/// Scan a directory and return a sorted array of matching entries.
///
/// If `filter` is non-null, only entries for which `filter(entry)` returns
/// non-zero are included.  The resulting array is sorted using `compar`
/// (if non-null).
///
/// On success, `*namelist` is set to a `malloc`'d array of `malloc`'d
/// `Dirent` pointers, and the function returns the number of entries.
/// The caller must `free()` each entry and the array itself.
///
/// On failure, returns -1 with errno set.
///
/// # Safety
///
/// `dirname` must be a valid null-terminated path.
/// `namelist` must point to a valid `*mut *mut Dirent` location.
///
/// # Alignment note
///
/// Pointer casts from `*mut u8` (returned by `malloc`) to `*mut *mut Dirent`
/// and `*mut Dirent` are safe because our `malloc` uses `mmap`, which
/// returns page-aligned memory (≥ 4096 bytes).
#[allow(clippy::cast_ptr_alignment)]
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn scandir(
    dirname: *const u8,
    namelist: *mut *mut *mut Dirent,
    filter: Option<extern "C" fn(*const Dirent) -> i32>,
    compar: Option<extern "C" fn(*const *const Dirent, *const *const Dirent) -> i32>,
) -> i32 {
    if dirname.is_null() || namelist.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Open the directory.
    let dirp = opendir(dirname);
    if dirp.is_null() {
        return -1; // errno already set by opendir.
    }

    // First pass: count matching entries.  Two-pass approach avoids
    // over-allocating when a filter rejects many entries.
    let total = unsafe { (*dirp).count };
    let mut count: usize = 0;
    unsafe { (*dirp).pos = 0; }
    for _ in 0..total {
        let entry = readdir(dirp);
        if entry.is_null() {
            break;
        }
        if filter.is_none_or(|f| f(entry) != 0) {
            count = count.wrapping_add(1);
        }
    }

    if count == 0 {
        closedir(dirp);
        // Allocate an empty array (POSIX allows returning 0 with a non-null
        // but empty namelist).
        let arr = crate::malloc::malloc(core::mem::size_of::<*mut Dirent>());
        if arr.is_null() {
            errno::set_errno(errno::ENOMEM);
            return -1;
        }
        // SAFETY: arr is page-aligned (mmap), so align ≥ 8.
        unsafe { *namelist = arr.cast::<*mut Dirent>(); }
        return 0;
    }

    // Allocate the output array.
    let arr_size = count.wrapping_mul(core::mem::size_of::<*mut Dirent>());
    let arr = crate::malloc::malloc(arr_size);
    if arr.is_null() {
        closedir(dirp);
        errno::set_errno(errno::ENOMEM);
        return -1;
    }
    // SAFETY: arr is page-aligned (mmap), align ≥ 8.
    let arr_typed = arr.cast::<*mut Dirent>();

    // Second pass: collect matching entries into the array.
    unsafe { (*dirp).pos = 0; }
    let mut idx: usize = 0;
    for _ in 0..total {
        let entry = readdir(dirp);
        if entry.is_null() {
            break;
        }
        if filter.is_none_or(|f| f(entry) != 0) && idx < count {
            let dup = crate::malloc::malloc(core::mem::size_of::<Dirent>());
            if dup.is_null() {
                // OOM: free everything allocated so far then bail.
                let mut j: usize = 0;
                while j < idx {
                    // SAFETY: valid pointers written at indices < idx.
                    unsafe { crate::malloc::free((*arr_typed.add(j)).cast::<u8>()); }
                    j = j.wrapping_add(1);
                }
                // SAFETY: arr allocated by malloc above.
                unsafe { crate::malloc::free(arr); }
                closedir(dirp);
                errno::set_errno(errno::ENOMEM);
                return -1;
            }
            // SAFETY: entry → dir.current (valid Dirent); dup has correct size.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    entry.cast::<u8>(),
                    dup,
                    core::mem::size_of::<Dirent>(),
                );
                // SAFETY: arr_typed is page-aligned; idx < count.
                *arr_typed.add(idx) = dup.cast::<Dirent>();
            }
            idx = idx.wrapping_add(1);
        }
    }

    closedir(dirp);

    // Sort if a comparator was provided.
    if let Some(cmp) = compar {
        // SAFETY: arr is page-aligned; idx entries have been written.
        unsafe { scandir_sort(arr, idx, cmp); }
    }

    // SAFETY: arr is page-aligned (align ≥ 8).
    unsafe { *namelist = arr_typed; }
    idx as i32
}

// ---------------------------------------------------------------------------
// versionsort — GNU extension
// ---------------------------------------------------------------------------

/// Compare two directory entries using version-number sorting.
///
/// Uses `strverscmp` on the `d_name` fields.  Like `alphasort`, this is
/// intended as a comparator for `scandir`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn versionsort(a: *const *const Dirent, b: *const *const Dirent) -> i32 {
    if a.is_null() || b.is_null() {
        return 0;
    }
    // SAFETY: a and b point to valid Dirent pointers.
    let da = unsafe { &**a };
    let db = unsafe { &**b };
    // SAFETY: d_name arrays are valid null-terminated strings within
    // allocated Dirent structs.
    unsafe { crate::string::strverscmp(da.d_name.as_ptr(), db.d_name.as_ptr()) }
}

// ---------------------------------------------------------------------------
// readdir_r — thread-safe readdir (POSIX, deprecated in POSIX.1-2008)
// ---------------------------------------------------------------------------

/// Thread-safe version of `readdir`.
///
/// Reads the next directory entry into caller-supplied `entry`, and
/// stores a pointer to `entry` in `*result` on success, or sets
/// `*result` to NULL when the directory is exhausted.
///
/// Returns 0 on success, or an error number on failure.
///
/// Note: deprecated in POSIX.1-2008 (readdir is thread-safe if each
/// thread uses its own Dir*), but still needed for legacy code.
///
/// # Safety
///
/// `dirp`, `entry`, and `result` must all be valid, non-null pointers.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn readdir_r(
    dirp: *mut Dir,
    entry: *mut Dirent,
    result: *mut *mut Dirent,
) -> i32 {
    if dirp.is_null() || entry.is_null() || result.is_null() {
        return errno::EINVAL;
    }

    let ent = readdir(dirp);
    if ent.is_null() {
        // End of directory — not an error.
        // SAFETY: result verified non-null.
        unsafe { *result = core::ptr::null_mut(); }
        return 0;
    }

    // Copy the entry into caller's buffer.
    // SAFETY: ent points to a valid Dirent (inside Dir.current),
    // and entry is caller-supplied valid storage.
    unsafe {
        core::ptr::copy_nonoverlapping(
            ent.cast::<u8>(),
            entry.cast::<u8>(),
            core::mem::size_of::<Dirent>(),
        );
        *result = entry;
    }

    0
}

// ---------------------------------------------------------------------------
// fdopendir — open directory stream from file descriptor
// ---------------------------------------------------------------------------

/// Open a directory stream from a file descriptor.
///
/// Uses the path stored at open time (via `fdtable::get_fd_path()`)
/// to issue `SYS_FS_LIST_DIR`, then buffers the results.  The Dir
/// takes ownership of `fd` — `closedir()` will close it.
///
/// **Limitation:** the stored path may be stale if the directory was
/// renamed after the fd was opened.  Real kernels resolve the fd
/// directly; we rely on path-string tracking.
///
/// # Errors
///
/// - `EBADF` — `fd` is not a valid open file descriptor.
/// - `ENOTDIR` — `fd` does not refer to a directory (no stored path).
/// - `EMFILE` — directory pool exhausted.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fdopendir(fd: i32) -> *mut Dir {
    // Verify the fd is valid.
    if crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return core::ptr::null_mut();
    }

    // Look up the stored path for this fd.
    let mut path_buf = [0u8; crate::unistd::PATH_MAX];
    let path_len = crate::fdtable::get_fd_path(fd, &mut path_buf);
    if path_len == 0 {
        // No path stored — fd is a pipe, socket, or not opened via our open().
        errno::set_errno(errno::ENOTDIR);
        return core::ptr::null_mut();
    }

    // Allocate a Dir from the static pool.
    let dir_ptr = alloc_dir();
    if dir_ptr.is_null() {
        errno::set_errno(errno::EMFILE);
        return core::ptr::null_mut();
    }

    // SAFETY: alloc_dir returned a valid, exclusively-owned Dir pointer.
    let dir = unsafe { &mut *dir_ptr };

    // Issue SYS_FS_LIST_DIR with the stored path.
    let ret = syscall3(
        SYS_FS_LIST_DIR,
        path_buf.as_ptr() as u64,
        path_len as u64,
        dir.buf.as_mut_ptr() as u64,
    );

    if ret < 0 {
        free_dir(dir_ptr);
        let _ = errno::translate(ret);
        return core::ptr::null_mut();
    }

    dir.count = ret as usize;
    dir.pos = 0;
    // Take ownership of the fd — closedir() will close it.
    dir.owned_fd = fd;

    dir_ptr
}

// ---------------------------------------------------------------------------
// Static Dir pool (no heap allocator)
// ---------------------------------------------------------------------------

// Maximum concurrent open directories.
const MAX_OPEN_DIRS: usize = 8;

/// Static pool of Dir structs.
///
/// We can't heap-allocate in `no_std` without a global allocator, so
/// we use a fixed pool.  Each Dir is ~68 KiB (256 entries × 264 bytes).
/// 8 × 68 KiB = 544 KiB — acceptable for a POSIX compat layer.
static mut DIR_POOL: [DirSlot; MAX_OPEN_DIRS] = [const { DirSlot::EMPTY }; MAX_OPEN_DIRS];

struct DirSlot {
    in_use: bool,
    dir: Dir,
}

impl DirSlot {
    const EMPTY: Self = Self {
        in_use: false,
        dir: Dir {
            buf: [0u8; MAX_DIR_ENTRIES * DIR_ENTRY_SIZE],
            count: 0,
            pos: 0,
            current: Dirent {
                d_ino: 0,
                d_off: 0,
                d_reclen: 0,
                d_type: 0,
                d_name: [0u8; 256],
            },
            owned_fd: -1,
        },
    };
}

/// Allocate a Dir from the static pool.
///
/// Returns a raw pointer to an available Dir slot, or null if the pool
/// is exhausted.  Uses `addr_of_mut!` to avoid creating `&mut` references
/// to `static mut` (which is UB in Rust 2024).
fn alloc_dir() -> *mut Dir {
    // SAFETY: Single-threaded access (no threads yet).
    // When threading is added, this needs synchronization.
    unsafe {
        let pool = core::ptr::addr_of_mut!(DIR_POOL).cast::<DirSlot>();
        let mut i: usize = 0;
        while i < MAX_OPEN_DIRS {
            let slot = pool.add(i);
            if !(*slot).in_use {
                (*slot).in_use = true;
                (*slot).dir.count = 0;
                (*slot).dir.pos = 0;
                return core::ptr::addr_of_mut!((*slot).dir);
            }
            i = i.wrapping_add(1);
        }
    }
    core::ptr::null_mut()
}

/// Return a Dir to the static pool.
///
/// Uses raw pointer comparison to find the matching slot.
fn free_dir(dir: *mut Dir) {
    // SAFETY: Single-threaded access.
    unsafe {
        let pool = core::ptr::addr_of_mut!(DIR_POOL).cast::<DirSlot>();
        let mut i: usize = 0;
        while i < MAX_OPEN_DIRS {
            let slot = pool.add(i);
            let slot_dir = core::ptr::addr_of_mut!((*slot).dir);
            if core::ptr::eq(dir, slot_dir) {
                (*slot).in_use = false;
                return;
            }
            i = i.wrapping_add(1);
        }
    }
}

// ---------------------------------------------------------------------------
// LFS64 aliases — our off_t/ino_t are already 64-bit
// ---------------------------------------------------------------------------

/// `readdir64` — LFS64 alias for `readdir`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn readdir64(dirp: *mut Dir) -> *mut Dirent {
    readdir(dirp)
}

/// `readdir_r64` — LFS64 alias for `readdir_r`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn readdir_r64(
    dirp: *mut Dir,
    entry: *mut Dirent,
    result: *mut *mut Dirent,
) -> i32 {
    readdir_r(dirp, entry, result)
}

/// `scandir64` — LFS64 alias for `scandir`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::cast_ptr_alignment)]
pub extern "C" fn scandir64(
    dirname: *const u8,
    namelist: *mut *mut *mut Dirent,
    filter: Option<extern "C" fn(*const Dirent) -> i32>,
    compar: Option<extern "C" fn(*const *const Dirent, *const *const Dirent) -> i32>,
) -> i32 {
    scandir(dirname, namelist, filter, compar)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- DT_* type constants --

    #[test]
    fn test_dt_constants() {
        assert_eq!(DT_UNKNOWN, 0);
        assert_eq!(DT_REG, 1);
        assert_eq!(DT_DIR, 2);
        assert_eq!(DT_LNK, 3);
    }

    #[test]
    fn test_dt_types_distinct() {
        let types = [DT_UNKNOWN, DT_REG, DT_DIR, DT_LNK];
        for (i, &a) in types.iter().enumerate() {
            for &b in &types[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    // -- Dirent struct layout --

    #[test]
    fn test_dirent_d_name_size() {
        // d_name must be at least 256 bytes for POSIX compliance.
        let d = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        assert_eq!(d.d_name.len(), 256);
    }

    #[test]
    fn test_dirent_fields() {
        let d = Dirent {
            d_ino: 42,
            d_off: 100,
            d_reclen: 280,
            d_type: DT_REG,
            d_name: [0u8; 256],
        };
        assert_eq!(d.d_ino, 42);
        assert_eq!(d.d_off, 100);
        assert_eq!(d.d_reclen, 280);
        assert_eq!(d.d_type, DT_REG);
    }

    // -- Internal constants --

    #[test]
    fn test_max_dir_entries() {
        assert_eq!(MAX_DIR_ENTRIES, 256);
    }

    #[test]
    fn test_dir_entry_size() {
        // Kernel entry: name[256] + size[4] + type[1] + pad[3] = 264.
        assert_eq!(DIR_ENTRY_SIZE, 264);
    }

    // -- alphasort (pure function — can test without kernel) --

    #[test]
    fn test_alphasort_equal() {
        let mut a = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        a.d_name[0] = b'f'; a.d_name[1] = b'o'; a.d_name[2] = b'o';
        b.d_name[0] = b'f'; b.d_name[1] = b'o'; b.d_name[2] = b'o';
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;
        assert_eq!(alphasort(&pa, &pb), 0);
    }

    #[test]
    fn test_alphasort_less() {
        let mut a = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        a.d_name[0] = b'a'; a.d_name[1] = b'b'; a.d_name[2] = b'c';
        b.d_name[0] = b'x'; b.d_name[1] = b'y'; b.d_name[2] = b'z';
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;
        assert!(alphasort(&pa, &pb) < 0);
    }

    #[test]
    fn test_alphasort_greater() {
        let mut a = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        a.d_name[0] = b'z';
        b.d_name[0] = b'a';
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;
        assert!(alphasort(&pa, &pb) > 0);
    }

    #[test]
    fn test_alphasort_null_outer() {
        // Null outer pointer (the *const *const Dirent itself).
        let d = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        let pd: *const Dirent = &d;
        assert_eq!(alphasort(core::ptr::null(), &pd), 0);
        assert_eq!(alphasort(&pd, core::ptr::null()), 0);
        assert_eq!(alphasort(core::ptr::null(), core::ptr::null()), 0);
    }

    #[test]
    fn test_alphasort_prefix() {
        // "ab" < "abc"
        let mut a = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        a.d_name[0] = b'a'; a.d_name[1] = b'b';
        b.d_name[0] = b'a'; b.d_name[1] = b'b'; b.d_name[2] = b'c';
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;
        assert!(alphasort(&pa, &pb) < 0); // "ab\0" < "abc\0"
    }

    // -- Dirent struct size and offsets --

    #[test]
    fn test_dirent_struct_size() {
        // d_ino(8) + d_off(8) + d_reclen(2) + d_type(1) + padding(5) + d_name(256)
        // = 280 bytes on x86_64.
        let size = core::mem::size_of::<Dirent>();
        assert!(size >= 275, "Dirent too small: {size}"); // at least the fields
        // d_ino must be at offset 0.
        assert_eq!(core::mem::offset_of!(Dirent, d_ino), 0);
        // d_name must be inside the struct and fit 256 bytes.
        let name_offset = core::mem::offset_of!(Dirent, d_name);
        assert!(name_offset + 256 <= size, "d_name doesn't fit in Dirent");
    }

    // -- versionsort (pure function — can test without kernel) --

    #[test]
    fn test_versionsort_numeric_ordering() {
        // "file2" < "file10" under version sorting.
        let mut a = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        let name_a = b"file2\0";
        let name_b = b"file10\0";
        a.d_name[..name_a.len()].copy_from_slice(name_a);
        b.d_name[..name_b.len()].copy_from_slice(name_b);
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;
        // Under version sort, file2 < file10.
        assert!(versionsort(&pa, &pb) < 0, "file2 should sort before file10");
    }

    #[test]
    fn test_versionsort_equal() {
        let mut a = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        a.d_name[0] = b'x'; a.d_name[1] = b'1';
        b.d_name[0] = b'x'; b.d_name[1] = b'1';
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;
        assert_eq!(versionsort(&pa, &pb), 0);
    }

    #[test]
    fn test_versionsort_null_outer() {
        let d = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        let pd: *const Dirent = &d;
        assert_eq!(versionsort(core::ptr::null(), &pd), 0);
        assert_eq!(versionsort(&pd, core::ptr::null()), 0);
    }

    #[test]
    fn test_versionsort_vs_alphasort() {
        // alphasort("file10", "file2") > 0 (lexicographic: '1' < '2')
        // versionsort("file10", "file2") > 0 (numeric: 10 > 2)
        // Both agree on ordering direction here, but the reason differs.
        let mut a = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0,
            d_name: [0u8; 256],
        };
        let name_a = b"file10\0";
        let name_b = b"file2\0";
        a.d_name[..name_a.len()].copy_from_slice(name_a);
        b.d_name[..name_b.len()].copy_from_slice(name_b);
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;

        // versionsort: file10 > file2 (numeric 10 > 2)
        assert!(versionsort(&pa, &pb) > 0, "versionsort: file10 > file2");

        // alphasort: "file10" < "file2" (lexicographic: '1' < '2')
        assert!(alphasort(&pa, &pb) < 0, "alphasort: file10 < file2 (lexicographic)");
    }

    // -- Dir pool constants --

    #[test]
    fn test_max_open_dirs() {
        assert_eq!(MAX_OPEN_DIRS, 8);
    }
}
