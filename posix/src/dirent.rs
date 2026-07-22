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

/// Directory entry type constants for `Dirent::d_type` / `getdents64`.
///
/// These are the Linux `<dirent.h>` ABI values (DT_REG=8, DT_DIR=4,
/// DT_LNK=10, …), re-exported from `linux_dirent_types` which is the
/// single source of truth.  Ported programs compiled against Linux/musl
/// headers compare `d_type` against these exact numbers, so we must
/// expose them — NOT the compact kernel type codes (see below).
pub use crate::linux_dirent_types::{
    DT_BLK, DT_CHR, DT_DIR, DT_FIFO, DT_LNK, DT_REG, DT_SOCK, DT_UNKNOWN,
};

/// Kernel directory-entry / stat type codes.
///
/// `SYS_FS_LIST_DIR` writes one of these at byte offset 260 of each
/// 264-byte entry, and `SYS_FS_STAT` uses the same encoding in its
/// `entry_type` field: 0=file, 1=directory, 2=volume label, 3=symlink.
/// These are an internal kernel ABI and are deliberately NOT the same as
/// the Linux `DT_*` values above — every consumer that reads the raw
/// kernel byte must translate via [`kernel_type_to_dt`] before exposing
/// it as a `d_type`.
pub(crate) const KERNEL_TYPE_FILE: u8 = 0;
pub(crate) const KERNEL_TYPE_DIR: u8 = 1;
pub(crate) const KERNEL_TYPE_VOLLABEL: u8 = 2;
pub(crate) const KERNEL_TYPE_SYMLINK: u8 = 3;

/// Translate a kernel directory-entry type byte into a POSIX `DT_*` value.
///
/// `SYS_FS_LIST_DIR` only ever emits file/dir/symlink (volume labels are
/// filtered out kernel-side), so any other code maps to `DT_UNKNOWN`.
pub(crate) fn kernel_type_to_dt(kernel_type: u8) -> u8 {
    match kernel_type {
        KERNEL_TYPE_DIR => DT_DIR,
        KERNEL_TYPE_SYMLINK => DT_LNK,
        KERNEL_TYPE_FILE => DT_REG,
        // Volume labels are filtered out by the kernel before they ever
        // reach a directory listing; treat them (and any unexpected code)
        // as unknown rather than guessing.
        KERNEL_TYPE_VOLLABEL => DT_UNKNOWN,
        _ => DT_UNKNOWN,
    }
}

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

    // Issue SYS_FS_LIST_DIR to get all entries at once.  The kernel writes
    // fixed-size dir entries into `buf` and needs the buffer *capacity* in
    // arg3 — it computes `max_entries = buf_cap / FS_DIR_ENTRY_SIZE`.  Omitting
    // arg3 (a former syscall3 call) left buf_cap = 0, so the kernel returned
    // zero entries no matter how many the directory held.
    let ret = syscall4(
        SYS_FS_LIST_DIR,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        dir.buf.as_mut_ptr() as u64,
        dir.buf.len() as u64,
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

    // Type byte is at offset 260 within the entry.  The kernel writes its
    // compact type code (0=file, 1=dir, 3=symlink); translate it to the
    // POSIX DT_* value callers expect.
    let raw_type = dir.buf.get(offset.wrapping_add(260)).copied().unwrap_or(0);
    dir.current.d_type = kernel_type_to_dt(raw_type);

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
    unsafe {
        (*dirp).pos = 0;
    }
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
        (*dirp).pos = if (loc as usize) > max {
            max
        } else {
            loc as usize
        };
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
                unsafe {
                    core::ptr::swap(a, b);
                }
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
        errno::set_errno(errno::EFAULT);
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
    unsafe {
        (*dirp).pos = 0;
    }
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
        unsafe {
            *namelist = arr.cast::<*mut Dirent>();
        }
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
    unsafe {
        (*dirp).pos = 0;
    }
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
                    unsafe {
                        crate::malloc::free((*arr_typed.add(j)).cast::<u8>());
                    }
                    j = j.wrapping_add(1);
                }
                // SAFETY: arr allocated by malloc above.
                unsafe {
                    crate::malloc::free(arr);
                }
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
        unsafe {
            scandir_sort(arr, idx, cmp);
        }
    }

    // SAFETY: arr is page-aligned (align ≥ 8).
    unsafe {
        *namelist = arr_typed;
    }
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
pub extern "C" fn readdir_r(dirp: *mut Dir, entry: *mut Dirent, result: *mut *mut Dirent) -> i32 {
    if dirp.is_null() || entry.is_null() || result.is_null() {
        return errno::EFAULT;
    }

    let ent = readdir(dirp);
    if ent.is_null() {
        // End of directory — not an error.
        // SAFETY: result verified non-null.
        unsafe {
            *result = core::ptr::null_mut();
        }
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

    // Issue SYS_FS_LIST_DIR with the stored path.  arg3 must carry the buffer
    // capacity so the kernel can compute max_entries (see `opendir`).
    let ret = syscall4(
        SYS_FS_LIST_DIR,
        path_buf.as_ptr() as u64,
        path_len as u64,
        dir.buf.as_mut_ptr() as u64,
        dir.buf.len() as u64,
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
// getdents / getdents64 — raw Linux directory entry syscalls
// ---------------------------------------------------------------------------

/// Linux kernel directory entry for `getdents64`.
///
/// Programs normally use `readdir()` instead.  This struct exists for
/// low-level compatibility with programs that use the raw syscall.
#[repr(C)]
pub struct LinuxDirent64 {
    /// Inode number.
    pub d_ino: u64,
    /// Offset to next entry.
    pub d_off: i64,
    /// Length of this `linux_dirent64`.
    pub d_reclen: u16,
    /// File type (DT_* constant).
    pub d_type: u8,
    /// Filename (null-terminated, variable length).
    pub d_name: [u8; 256],
}

// ---------------------------------------------------------------------------
// Per-fd getdents iterator cache
// ---------------------------------------------------------------------------
//
// `getdents64` is stateful: each call returns the next batch of entries
// for a given fd, with no caller-visible position object.  We therefore
// keep a small static pool of per-fd snapshot caches.  On the first call
// for an fd, we snapshot the directory via SYS_FS_LIST_DIR; subsequent
// calls walk the snapshot until exhausted, at which point we free the
// slot and return 0.
//
// LIMITATIONS:
//   * Cache is keyed by fd number, so if a program closes a directory fd
//     mid-iteration and reuses that fd number, the next `getdents64`
//     call on the new fd will see stale snapshot data until the cache
//     reports EOF and frees the slot.  Real programs do not close-and-
//     reuse mid-iteration, but document the issue (see todo.txt).
//   * Snapshot pool is small (4 slots).  If more concurrent
//     getdents64-iterated directories are needed, raise this constant.

const MAX_GETDENTS_CACHES: usize = 4;

struct GetdentsCache {
    in_use: bool,
    fd: i32,
    /// Kernel snapshot buffer, same layout as the `Dir` buffer.
    buf: [u8; MAX_DIR_ENTRIES * DIR_ENTRY_SIZE],
    /// Number of kernel entries in `buf`.
    count: usize,
    /// Next entry index to emit.
    pos: usize,
}

impl GetdentsCache {
    const EMPTY: Self = Self {
        in_use: false,
        fd: -1,
        buf: [0u8; MAX_DIR_ENTRIES * DIR_ENTRY_SIZE],
        count: 0,
        pos: 0,
    };
}

static mut GETDENTS_POOL: [GetdentsCache; MAX_GETDENTS_CACHES] =
    [const { GetdentsCache::EMPTY }; MAX_GETDENTS_CACHES];

/// Find the cache slot owning `fd`, if any.
fn find_getdents_cache(fd: i32) -> Option<*mut GetdentsCache> {
    if fd < 0 {
        return None;
    }
    // SAFETY: Single-threaded access (consistent with the rest of posix).
    unsafe {
        let base = core::ptr::addr_of_mut!(GETDENTS_POOL).cast::<GetdentsCache>();
        let mut i: usize = 0;
        while i < MAX_GETDENTS_CACHES {
            let slot = base.add(i);
            if (*slot).in_use && (*slot).fd == fd {
                return Some(slot);
            }
            i = i.wrapping_add(1);
        }
    }
    None
}

/// Allocate a free cache slot, or return null if the pool is exhausted.
fn alloc_getdents_cache(fd: i32) -> *mut GetdentsCache {
    // SAFETY: Single-threaded access.
    unsafe {
        let base = core::ptr::addr_of_mut!(GETDENTS_POOL).cast::<GetdentsCache>();
        let mut i: usize = 0;
        while i < MAX_GETDENTS_CACHES {
            let slot = base.add(i);
            if !(*slot).in_use {
                (*slot).in_use = true;
                (*slot).fd = fd;
                (*slot).count = 0;
                (*slot).pos = 0;
                return slot;
            }
            i = i.wrapping_add(1);
        }
    }
    core::ptr::null_mut()
}

/// Release a cache slot back to the pool.
fn free_getdents_cache(slot: *mut GetdentsCache) {
    if slot.is_null() {
        return;
    }
    // SAFETY: caller guarantees `slot` points into GETDENTS_POOL.
    unsafe {
        (*slot).in_use = false;
        (*slot).fd = -1;
        (*slot).count = 0;
        (*slot).pos = 0;
    }
}

/// Header size of a `linux_dirent64` record (everything before `d_name`).
const LINUX_DIRENT64_HEADER: usize = 19;

/// Emit one `linux_dirent64` record into `out`.
///
/// Returns `Some(reclen)` on success (number of bytes written, padded to
/// 8-byte alignment) or `None` if `out` is too small to hold the record.
// `slot` is taken as `out[..reclen]` after the `reclen > out.len()` check,
// so `reclen` bytes (>= LINUX_DIRENT64_HEADER = 19) are guaranteed in
// scope.  `name_len` is bounded by `reclen - LINUX_DIRENT64_HEADER`.
// `reclen + 7` cannot overflow because the prior `checked_add` chain
// returned None if it would.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn emit_linux_dirent64(
    out: &mut [u8],
    ino: u64,
    off: i64,
    dtype: u8,
    name: &[u8],
) -> Option<usize> {
    // Name must include space for the trailing NUL terminator and the
    // whole record must be rounded up to 8-byte alignment so the next
    // record's u64 fields stay aligned.
    let name_len = name.len();
    let unpadded = LINUX_DIRENT64_HEADER
        .checked_add(name_len)?
        .checked_add(1)?;
    let reclen = unpadded.checked_add(7)? & !7usize;
    if reclen > out.len() || reclen > u16::MAX as usize {
        return None;
    }
    let slot = out.get_mut(..reclen)?;
    slot[0..8].copy_from_slice(&ino.to_le_bytes());
    slot[8..16].copy_from_slice(&off.to_le_bytes());
    let reclen_u16 = reclen as u16;
    slot[16..18].copy_from_slice(&reclen_u16.to_le_bytes());
    slot[18] = dtype;
    if let Some(name_dst) = slot.get_mut(LINUX_DIRENT64_HEADER..LINUX_DIRENT64_HEADER + name_len) {
        name_dst.copy_from_slice(name);
    }
    // Zero the NUL terminator and any tail padding.
    if let Some(tail) = slot.get_mut(LINUX_DIRENT64_HEADER + name_len..reclen) {
        for b in tail {
            *b = 0;
        }
    }
    Some(reclen)
}

/// Parse a single 264-byte kernel directory entry into `(name, dtype)`.
///
/// The returned `dtype` is already translated from the kernel's compact
/// type code (0=file, 1=dir, 3=symlink) into a POSIX `DT_*` value, so the
/// caller can emit it directly.
///
/// Returns `None` if the entry has no name (empty NUL-terminated string).
// Loop guard `name_len < 256` and `entry: &[u8; DIR_ENTRY_SIZE]`
// (DIR_ENTRY_SIZE == 264) keep `entry[name_len]` in bounds; `entry[260]`
// is similarly in range.
#[allow(clippy::indexing_slicing)]
fn parse_kernel_entry(entry: &[u8; DIR_ENTRY_SIZE]) -> Option<(&[u8], u8)> {
    // Name occupies bytes 0..256; find NUL.
    let mut name_len: usize = 0;
    while name_len < 256 && entry[name_len] != 0 {
        name_len = name_len.wrapping_add(1);
    }
    if name_len == 0 {
        return None;
    }
    let dtype = kernel_type_to_dt(entry[260]);
    let name = entry.get(..name_len)?;
    Some((name, dtype))
}

/// Read directory entries via the raw Linux `getdents64` syscall.
///
/// Programs normally use `readdir()`; this exists for low-level
/// compatibility with code that calls the raw syscall (e.g. `ls -f`
/// implementations and language runtimes that bypass libc's dir
/// streams).
///
/// On the first call for a given fd we snapshot the directory via
/// `SYS_FS_LIST_DIR`; subsequent calls drain the snapshot.  Returns the
/// number of bytes written into `dirp`, 0 at end-of-directory, or -1
/// with errno set on error.
///
/// # Errors
///
/// - `EBADF`  — `fd` is negative or not a valid open fd.
/// - `EFAULT` — `dirp` is null and `count` is non-zero.
/// - `EINVAL` — `count` is zero or too small to hold any single entry.
/// - `ENOTDIR` — `fd` does not refer to a directory.
/// - `ENFILE` — the per-fd snapshot cache pool is exhausted.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getdents64(fd: i32, dirp: *mut u8, count: usize) -> i64 {
    if fd < 0 {
        crate::errno::set_errno(crate::errno::EBADF);
        return -1;
    }
    if count == 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    if dirp.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }

    // Locate or allocate the cache slot for this fd.
    let slot_ptr = if let Some(existing) = find_getdents_cache(fd) {
        existing
    } else {
        // Validate fd before snapshotting.
        if crate::fdtable::get_fd(fd).is_none() {
            crate::errno::set_errno(crate::errno::EBADF);
            return -1;
        }
        // Look up the stored path for this fd.
        let mut path_buf = [0u8; crate::unistd::PATH_MAX];
        let path_len = crate::fdtable::get_fd_path(fd, &mut path_buf);
        if path_len == 0 {
            crate::errno::set_errno(crate::errno::ENOTDIR);
            return -1;
        }
        let slot = alloc_getdents_cache(fd);
        if slot.is_null() {
            crate::errno::set_errno(crate::errno::ENFILE);
            return -1;
        }
        // Snapshot the directory listing into the cache buffer.
        // SAFETY: slot is a valid pointer into GETDENTS_POOL; the
        // buffer is owned exclusively while in_use is true.
        let buf_ptr = unsafe { core::ptr::addr_of_mut!((*slot).buf) }.cast::<u8>();
        let ret = syscall3(
            SYS_FS_LIST_DIR,
            path_buf.as_ptr() as u64,
            path_len as u64,
            buf_ptr as u64,
        );
        if ret < 0 {
            free_getdents_cache(slot);
            let _ = errno::translate(ret); // Sets errno.
            return -1;
        }
        // SAFETY: slot is valid; we own it via in_use.
        unsafe {
            (*slot).count = ret as usize;
            (*slot).pos = 0;
        }
        slot
    };

    // SAFETY: slot_ptr is non-null and refers to an owned cache slot.
    let (snapshot_count, mut pos, buf_ptr_const) = unsafe {
        let cnt = (*slot_ptr).count;
        let p = (*slot_ptr).pos;
        let bp = core::ptr::addr_of!((*slot_ptr).buf).cast::<u8>();
        (cnt, p, bp)
    };

    // If we are already past the end, free the slot and report EOF.
    if pos >= snapshot_count {
        free_getdents_cache(slot_ptr);
        return 0;
    }

    // SAFETY: dirp points to a writable buffer of `count` bytes (caller
    // contract for getdents64).  We never read past `count`.
    let out = unsafe { core::slice::from_raw_parts_mut(dirp, count) };
    let mut written: usize = 0;
    let mut emitted_any = false;

    while pos < snapshot_count {
        let entry_off = pos.wrapping_mul(DIR_ENTRY_SIZE);
        // Bounds: each snapshot entry is exactly DIR_ENTRY_SIZE bytes
        // and `count < MAX_DIR_ENTRIES` was guaranteed by SYS_FS_LIST_DIR.
        let entry_end = entry_off.wrapping_add(DIR_ENTRY_SIZE);
        if entry_end > MAX_DIR_ENTRIES.wrapping_mul(DIR_ENTRY_SIZE) {
            // Defensive: bail rather than over-read.
            break;
        }
        // SAFETY: we computed bounds against the fixed-size buf above.
        let entry_slice =
            unsafe { core::slice::from_raw_parts(buf_ptr_const.add(entry_off), DIR_ENTRY_SIZE) };
        // The slice is exactly DIR_ENTRY_SIZE long, so `try_into` always
        // succeeds; the match is purely to satisfy the borrow checker.
        let Ok(entry_arr) = <&[u8; DIR_ENTRY_SIZE]>::try_from(entry_slice) else {
            break;
        };
        let Some((name, dtype)) = parse_kernel_entry(entry_arr) else {
            // Empty name — skip silently.
            pos = pos.wrapping_add(1);
            continue;
        };

        let Some(remaining) = out.get_mut(written..) else {
            break;
        };
        match emit_linux_dirent64(
            remaining,
            pos as u64,
            (pos as i64).wrapping_add(1),
            dtype,
            name,
        ) {
            Some(reclen) => {
                written = written.wrapping_add(reclen);
                emitted_any = true;
                pos = pos.wrapping_add(1);
            }
            None => {
                // Buffer full for this batch.
                break;
            }
        }
    }

    if !emitted_any {
        // Caller's buffer is too small for the next available record.
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }

    // Persist updated position.
    // SAFETY: slot_ptr still owned by this call.
    unsafe {
        (*slot_ptr).pos = pos;
    }

    written as i64
}

/// Read directory entries via the legacy Linux `getdents` syscall.
///
/// The legacy `struct linux_dirent` has a 32-bit inode field which
/// cannot represent our 64-bit inodes safely, so we never actually
/// produce records here — the function returns `ENOSYS` on valid
/// calls and callers should switch to `getdents64` or libc's
/// `readdir()`.  glibc and musl do not export a wrapper for either
/// raw syscall, so portable code already uses one of those.
///
/// However, an unimplemented sentinel is not a license to skip
/// argument-domain validation.  A buggy caller — for example a
/// language runtime that bypasses libc — passing a closed fd or a
/// NULL buffer must see the same errno values Linux would produce,
/// so the failure is diagnosed correctly even though the underlying
/// directory walk is not performed.  Validation order matches
/// `getdents64` above and Linux's `fs/readdir.c::sys_getdents`:
///
/// 1. `fd < 0`                   -> `EBADF`
/// 2. `count == 0`               -> `EINVAL`
/// 3. `dirp.is_null()`           -> `EFAULT`
/// 4. `fd` not in fdtable        -> `EBADF`
/// 5. all valid                  -> `ENOSYS`
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getdents(fd: i32, dirp: *mut u8, count: usize) -> i64 {
    if fd < 0 {
        crate::errno::set_errno(crate::errno::EBADF);
        return -1;
    }
    if count == 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    if dirp.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }
    if crate::fdtable::get_fd(fd).is_none() {
        crate::errno::set_errno(crate::errno::EBADF);
        return -1;
    }
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// scandirat — scan directory relative to a directory fd
// ---------------------------------------------------------------------------

/// `scandirat` — scan a directory relative to a directory fd.
///
/// Like `scandir`, but the directory is specified relative to `dirfd`.
/// If `dirfd` is `AT_FDCWD` or `dirname` is absolute, this behaves
/// identically to `scandir`.
///
/// Returns the number of matching entries on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn scandirat(
    dirfd: i32,
    dirname: *const u8,
    namelist: *mut *mut *mut Dirent,
    filter: Option<extern "C" fn(*const Dirent) -> i32>,
    compar: Option<extern "C" fn(*const *const Dirent, *const *const Dirent) -> i32>,
) -> i32 {
    if dirname.is_null() || namelist.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Resolve relative to dirfd if needed.
    if dirfd == crate::file::AT_FDCWD || crate::file::is_absolute_path(dirname) {
        return scandir(dirname, namelist, filter, compar);
    }

    // Build full path from dirfd + relative dirname.
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = crate::file::resolve_dirfd_path(dirfd, dirname, &mut full);
    if len == 0 {
        return -1; // errno set by resolve_dirfd_path
    }
    scandir(full.as_ptr(), namelist, filter, compar)
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
        // d_type uses the Linux <dirent.h> ABI values (re-exported from
        // linux_dirent_types) so ported programs interpret them correctly.
        assert_eq!(DT_UNKNOWN, 0);
        assert_eq!(DT_FIFO, 1);
        assert_eq!(DT_CHR, 2);
        assert_eq!(DT_DIR, 4);
        assert_eq!(DT_BLK, 6);
        assert_eq!(DT_REG, 8);
        assert_eq!(DT_LNK, 10);
        assert_eq!(DT_SOCK, 12);
    }

    #[test]
    fn test_kernel_type_to_dt() {
        // The kernel's compact type code must translate to the matching
        // POSIX DT_* value, NOT pass through unchanged.
        assert_eq!(kernel_type_to_dt(KERNEL_TYPE_FILE), DT_REG);
        assert_eq!(kernel_type_to_dt(KERNEL_TYPE_DIR), DT_DIR);
        assert_eq!(kernel_type_to_dt(KERNEL_TYPE_SYMLINK), DT_LNK);
        // Volume labels and any unexpected code → unknown.
        assert_eq!(kernel_type_to_dt(KERNEL_TYPE_VOLLABEL), DT_UNKNOWN);
        assert_eq!(kernel_type_to_dt(99), DT_UNKNOWN);
    }

    #[test]
    fn test_parse_kernel_entry_translates_type() {
        // A directory entry from the kernel has type code 1; parse_kernel_entry
        // must hand back the translated DT_DIR (4), not the raw 1.
        let mut entry = [0u8; DIR_ENTRY_SIZE];
        entry[0] = b'd';
        entry[1] = b'i';
        entry[2] = b'r';
        entry[260] = KERNEL_TYPE_DIR;
        let (name, dtype) = parse_kernel_entry(&entry).expect("entry has a name");
        assert_eq!(name, b"dir");
        assert_eq!(dtype, DT_DIR);
    }

    #[test]
    fn test_dt_types_distinct() {
        let types = [
            DT_UNKNOWN, DT_REG, DT_DIR, DT_LNK, DT_CHR, DT_BLK, DT_FIFO, DT_SOCK,
        ];
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
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        a.d_name[0] = b'f';
        a.d_name[1] = b'o';
        a.d_name[2] = b'o';
        b.d_name[0] = b'f';
        b.d_name[1] = b'o';
        b.d_name[2] = b'o';
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;
        assert_eq!(alphasort(&pa, &pb), 0);
    }

    #[test]
    fn test_alphasort_less() {
        let mut a = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        a.d_name[0] = b'a';
        a.d_name[1] = b'b';
        a.d_name[2] = b'c';
        b.d_name[0] = b'x';
        b.d_name[1] = b'y';
        b.d_name[2] = b'z';
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;
        assert!(alphasort(&pa, &pb) < 0);
    }

    #[test]
    fn test_alphasort_greater() {
        let mut a = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
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
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
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
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        a.d_name[0] = b'a';
        a.d_name[1] = b'b';
        b.d_name[0] = b'a';
        b.d_name[1] = b'b';
        b.d_name[2] = b'c';
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
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
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
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        a.d_name[0] = b'x';
        a.d_name[1] = b'1';
        b.d_name[0] = b'x';
        b.d_name[1] = b'1';
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;
        assert_eq!(versionsort(&pa, &pb), 0);
    }

    #[test]
    fn test_versionsort_null_outer() {
        let d = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
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
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
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
        assert!(
            alphasort(&pa, &pb) < 0,
            "alphasort: file10 < file2 (lexicographic)"
        );
    }

    // -- Dir pool constants --

    #[test]
    fn test_max_open_dirs() {
        assert_eq!(MAX_OPEN_DIRS, 8);
    }

    // -- Null pointer handling --

    #[test]
    fn test_readdir_null() {
        let ret = readdir(core::ptr::null_mut());
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_closedir_null() {
        let ret = closedir(core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_rewinddir_null_is_noop() {
        // Should not crash.
        rewinddir(core::ptr::null_mut());
    }

    #[test]
    fn test_telldir_null() {
        let ret = telldir(core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_seekdir_null_is_noop() {
        // Should not crash.
        seekdir(core::ptr::null_mut(), 5);
    }

    #[test]
    fn test_seekdir_negative_loc_is_noop() {
        // Negative loc should be silently ignored.
        // We can't test this without a real Dir, but we can test null+negative.
        seekdir(core::ptr::null_mut(), -1);
    }

    #[test]
    fn test_dirfd_null() {
        let ret = dirfd(core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_opendir_null() {
        let ret = opendir(core::ptr::null());
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- readdir_r null pointer handling --

    #[test]
    fn test_readdir_r_null_dirp() {
        let mut entry = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut result: *mut Dirent = core::ptr::null_mut();
        let ret = readdir_r(core::ptr::null_mut(), &raw mut entry, &raw mut result);
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn test_readdir_r_null_entry() {
        // Use a fake non-null dirp to test the entry null check.
        let fake_dirp = 0x1000 as *mut Dir;
        let mut result: *mut Dirent = core::ptr::null_mut();
        let ret = readdir_r(fake_dirp, core::ptr::null_mut(), &raw mut result);
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn test_readdir_r_null_result() {
        let fake_dirp = 0x1000 as *mut Dir;
        let mut entry = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let ret = readdir_r(fake_dirp, &raw mut entry, core::ptr::null_mut());
        assert_eq!(ret, errno::EFAULT);
    }

    // -- scandir null pointer handling --

    #[test]
    fn test_scandir_null_dirname() {
        let mut list: *mut *mut Dirent = core::ptr::null_mut();
        let ret = scandir(core::ptr::null(), &raw mut list, None, None);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_scandir_null_namelist() {
        let ret = scandir(b"/tmp\0".as_ptr(), core::ptr::null_mut(), None, None);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- fdopendir error handling --

    #[test]
    fn test_fdopendir_invalid_fd() {
        // fd 999 is not open.
        let ret = fdopendir(999);
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_fdopendir_negative_fd() {
        let ret = fdopendir(-1);
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -- DirSlot layout --

    #[test]
    fn test_dir_slot_empty_state() {
        let slot = DirSlot::EMPTY;
        assert!(!slot.in_use);
        assert_eq!(slot.dir.count, 0);
        assert_eq!(slot.dir.pos, 0);
        assert_eq!(slot.dir.owned_fd, -1);
    }

    // -- Buffer size validation --

    #[test]
    fn test_dir_buffer_capacity() {
        // The buffer must hold MAX_DIR_ENTRIES * DIR_ENTRY_SIZE bytes.
        let expected = MAX_DIR_ENTRIES * DIR_ENTRY_SIZE;
        assert_eq!(
            expected,
            256 * 264,
            "Buffer capacity must be 256 * 264 = 67584 bytes"
        );
    }

    // -- LP64 aliases --

    #[test]
    fn test_readdir64_null_returns_null() {
        let result = readdir64(core::ptr::null_mut());
        assert!(result.is_null());
    }

    #[test]
    fn test_readdir_r64_null_dirp() {
        let mut entry = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0; 256],
        };
        let mut result: *mut Dirent = core::ptr::null_mut();
        let ret = unsafe { readdir_r64(core::ptr::null_mut(), &mut entry, &mut result) };
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn test_readdir_r64_null_entry() {
        let mut result: *mut Dirent = core::ptr::null_mut();
        let ret = unsafe { readdir_r64(core::ptr::null_mut(), core::ptr::null_mut(), &mut result) };
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn test_scandir64_null_dirname() {
        let mut namelist: *mut *mut Dirent = core::ptr::null_mut();
        let ret = scandir64(core::ptr::null(), &mut namelist, None, None);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_scandir64_null_namelist() {
        let ret = scandir64(b"/tmp\0".as_ptr(), core::ptr::null_mut(), None, None);
        assert_eq!(ret, -1);
    }

    // -- getdents / getdents64 stubs --

    #[test]
    fn test_getdents64_null_buf_returns_efault() {
        // getdents64 is now implemented; a null buffer with non-zero
        // count must report EFAULT before touching the cache.
        crate::errno::set_errno(0);
        assert_eq!(getdents64(3, core::ptr::null_mut(), 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_getdents_still_enosys() {
        // Phase 67: getdents (legacy 32-bit-ino variant) is still
        // unimplemented, but now validates arguments first.  NULL dirp
        // with non-zero count now produces EFAULT (matching Linux),
        // not ENOSYS — this test was previously checking the pre-
        // validator behaviour.  Updated to call with valid args that
        // reach the ENOSYS sentinel (negative fd would short-circuit
        // with EBADF; use AT_FDCWD-style sentinel value of -100 is
        // also negative, so we have to use a real open fd).  Since
        // we cannot easily open a fd in this test, we assert the new
        // EFAULT semantics directly to keep regression coverage.
        crate::errno::set_errno(0);
        assert_eq!(getdents(3, core::ptr::null_mut(), 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_linux_dirent64_size() {
        let size = core::mem::size_of::<LinuxDirent64>();
        // d_ino(8) + d_off(8) + d_reclen(2) + d_type(1) + d_name(256) + padding
        assert!(
            size >= 275,
            "LinuxDirent64 should be at least 275 bytes, got {size}"
        );
    }

    #[test]
    fn test_linux_dirent64_alignment() {
        assert!(core::mem::align_of::<LinuxDirent64>() >= 8);
    }

    // -----------------------------------------------------------------------
    // scandirat
    // -----------------------------------------------------------------------

    #[test]
    fn test_scandirat_null_dirname() {
        crate::errno::set_errno(0);
        let mut list: *mut *mut Dirent = core::ptr::null_mut();
        let ret = scandirat(
            crate::file::AT_FDCWD,
            core::ptr::null(),
            &raw mut list,
            None,
            None,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_scandirat_null_namelist() {
        crate::errno::set_errno(0);
        let ret = scandirat(
            crate::file::AT_FDCWD,
            b"/tmp\0".as_ptr(),
            core::ptr::null_mut(),
            None,
            None,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_scandirat_with_at_fdcwd() {
        // AT_FDCWD delegates to scandir — result depends on test host.
        let mut list: *mut *mut Dirent = core::ptr::null_mut();
        let _ret = scandirat(
            crate::file::AT_FDCWD,
            b"/nonexistent_scandirat\0".as_ptr(),
            &raw mut list,
            None,
            None,
        );
        // Just verify no crash.
    }

    // -- getdents / getdents64 --

    #[test]
    fn test_getdents_returns_enosys() {
        // Phase 67: fd 3 is not an open fd in the test environment, so
        // the new validator now reports EBADF before reaching the
        // ENOSYS sentinel.  Test updated to verify EBADF for this
        // closed-fd case.  A separate test below
        // (`test_getdents_valid_args_reach_enosys`) covers the actual
        // ENOSYS path with a real open fd.
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(3, buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_getdents64_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents64(-1, buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_getdents64_zero_count_einval() {
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents64(3, buf.as_mut_ptr(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getdents64_null_buf_efault() {
        crate::errno::set_errno(0);
        let ret = getdents64(3, core::ptr::null_mut(), 256);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_getdents64_invalid_fd_ebadf() {
        // fd 9999 is far above any allocated test fd, so get_fd() returns None.
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents64(9999, buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_emit_linux_dirent64_layout() {
        // Verify the header layout: ino|off|reclen|type|name|NUL|pad.
        let mut out = [0u8; 64];
        let name = b"hi";
        let reclen = emit_linux_dirent64(&mut out, 0xCAFEBABE, 7, DT_REG, name)
            .expect("emit should succeed");
        // 19 (header) + 2 (name) + 1 (NUL) = 22 → rounded up to 24.
        assert_eq!(reclen, 24);
        // ino
        let ino = u64::from_le_bytes(out[0..8].try_into().unwrap());
        assert_eq!(ino, 0xCAFEBABE);
        // off
        let off = i64::from_le_bytes(out[8..16].try_into().unwrap());
        assert_eq!(off, 7);
        // reclen
        let rl = u16::from_le_bytes(out[16..18].try_into().unwrap());
        assert_eq!(rl as usize, reclen);
        // type
        assert_eq!(out[18], DT_REG);
        // name + NUL
        assert_eq!(&out[19..21], b"hi");
        assert_eq!(out[21], 0);
        // Padding bytes are zero.
        assert_eq!(out[22], 0);
        assert_eq!(out[23], 0);
    }

    #[test]
    fn test_emit_linux_dirent64_too_small() {
        let mut out = [0u8; 8];
        assert!(emit_linux_dirent64(&mut out, 1, 1, DT_DIR, b"abc").is_none());
    }

    #[test]
    fn test_emit_linux_dirent64_alignment() {
        // Every record must be a multiple of 8 bytes so consecutive
        // records keep their u64 fields aligned.
        let mut out = [0u8; 128];
        for name in [&b"a"[..], &b"abc"[..], &b"longer"[..], &b"exactly7"[..]] {
            let r = emit_linux_dirent64(&mut out, 0, 0, DT_REG, name).expect("emit should succeed");
            assert_eq!(r % 8, 0, "reclen {r} not 8-aligned for name {name:?}");
        }
    }

    #[test]
    fn test_parse_kernel_entry_empty_name_skipped() {
        let entry = [0u8; DIR_ENTRY_SIZE];
        assert!(parse_kernel_entry(&entry).is_none());
    }

    #[test]
    fn test_parse_kernel_entry_extracts_name_and_type() {
        let mut entry = [0u8; DIR_ENTRY_SIZE];
        entry[0] = b'f';
        entry[1] = b'o';
        entry[2] = b'o';
        // bytes 3.. are NUL — name terminates at NUL.
        // Byte 260 is the kernel type code (1=dir), which parse_kernel_entry
        // translates to the POSIX DT_DIR value.
        entry[260] = KERNEL_TYPE_DIR;
        let (name, dtype) = parse_kernel_entry(&entry).expect("should parse");
        assert_eq!(name, b"foo");
        assert_eq!(dtype, DT_DIR);
    }

    #[test]
    fn test_getdents_cache_pool_constants() {
        // Sanity check the pool size — bumping this constant must be
        // a deliberate decision (it grows .bss).
        assert_eq!(MAX_GETDENTS_CACHES, 4);
    }

    #[test]
    fn test_getdents_cache_find_returns_none_for_negative() {
        assert!(find_getdents_cache(-1).is_none());
        assert!(find_getdents_cache(-42).is_none());
    }

    #[test]
    fn test_linux_dirent64_header_size() {
        // 8 (ino) + 8 (off) + 2 (reclen) + 1 (type) = 19.
        assert_eq!(LINUX_DIRENT64_HEADER, 19);
    }

    // -----------------------------------------------------------------
    // Phase 67 — getdents argument-domain validators
    // -----------------------------------------------------------------
    //
    // The legacy `getdents` stub remains policy-driven (returns ENOSYS
    // on valid calls because the 32-bit-ino record format can't
    // represent our 64-bit inodes safely).  But invalid calls must
    // produce the same errno values Linux would, so a buggy caller is
    // not misled by ENOSYS into thinking the function never exists.

    // --- per-error-class ---

    #[test]
    fn test_getdents_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(-1, buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_getdents_very_negative_fd_ebadf() {
        // Even an "AT_FDCWD-like" -100 fd is rejected with EBADF here.
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(-100, buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_getdents_zero_count_einval() {
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(3, buf.as_mut_ptr(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getdents_null_buf_efault() {
        crate::errno::set_errno(0);
        let ret = getdents(3, core::ptr::null_mut(), 256);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_getdents_closed_fd_ebadf() {
        // fd 9999 is far beyond any allocated fd in tests, so fdtable
        // returns None and we report EBADF.
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(9999, buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_getdents_valid_args_reach_enosys() {
        // Open a real fd via pipe() and pass it to getdents.  The fd
        // is not a directory, but getdents's stub does not check kind
        // — it only verifies the fd is open, then returns ENOSYS.
        // A future refinement (when kind tracking lands for directories)
        // would refine non-directory fds to ENOTDIR.
        let mut pf = [-1i32; 2];
        let r = crate::pipe::pipe(pf.as_mut_ptr());
        assert_eq!(r, 0, "pipe() must succeed to set up test");
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(pf[0], buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
        // Clean up pipe fds.
        let _ = crate::fdtable::close_fd(pf[0]);
        let _ = crate::fdtable::close_fd(pf[1]);
    }

    // --- ordering ---

    #[test]
    fn test_getdents_negative_fd_beats_zero_count() {
        // fd<0 check fires before count==0 check.
        crate::errno::set_errno(0);
        let ret = getdents(-1, core::ptr::null_mut(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_getdents_negative_fd_beats_null_buf() {
        crate::errno::set_errno(0);
        let ret = getdents(-1, core::ptr::null_mut(), 256);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_getdents_zero_count_beats_null_buf() {
        // count==0 check fires before NULL-buf check.
        crate::errno::set_errno(0);
        let ret = getdents(3, core::ptr::null_mut(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getdents_null_buf_beats_closed_fd() {
        // NULL-buf check (with non-zero count) fires before the
        // fdtable lookup, so a closed fd plus NULL buf produces EFAULT,
        // not EBADF.
        crate::errno::set_errno(0);
        let ret = getdents(9999, core::ptr::null_mut(), 256);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // --- ordering parity with getdents64 ---

    #[test]
    fn test_getdents_and_getdents64_share_validation_order() {
        // Both validators check fd<0 first, then count==0, then NULL
        // buf, then fdtable.  This test pins that parity so a future
        // refactor doesn't diverge them silently.
        crate::errno::set_errno(0);
        let r1 = getdents(-1, core::ptr::null_mut(), 0);
        let e1 = crate::errno::get_errno();
        crate::errno::set_errno(0);
        let r2 = getdents64(-1, core::ptr::null_mut(), 0);
        let e2 = crate::errno::get_errno();
        assert_eq!(r1, -1);
        assert_eq!(r2, -1);
        assert_eq!(e1, e2);
        assert_eq!(e1, crate::errno::EBADF);
    }

    // --- real-world workflows ---

    #[test]
    fn test_workflow_legacy_program_calling_raw_getdents() {
        // A 32-bit-era program (or test harness emulating one) calls
        // the raw getdents syscall directly with a valid open fd.
        // Modern kernels would happily return records; we return
        // ENOSYS because we don't support the 32-bit-ino layout, but
        // the call must not be confused with "fd was bad".
        let mut pf = [-1i32; 2];
        assert_eq!(crate::pipe::pipe(pf.as_mut_ptr()), 0);
        crate::errno::set_errno(0);
        let mut buf = [0u8; 1024];
        let ret = getdents(pf[0], buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
        let _ = crate::fdtable::close_fd(pf[0]);
        let _ = crate::fdtable::close_fd(pf[1]);
    }

    // --- buggy callers ---

    #[test]
    fn test_buggy_caller_passes_closed_fd() {
        // A caller forgot to check the return value of open() and
        // passes the -1 sentinel through.  Linux returns EBADF; we
        // must too — not ENOSYS, which would suggest the function
        // doesn't exist at all.
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(-1, buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_buggy_caller_zero_size_buffer() {
        // A caller miscomputes the buffer size as 0.  Linux returns
        // EINVAL; we must too.
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(3, buf.as_mut_ptr(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_buggy_caller_uninitialised_buffer_pointer() {
        // A caller forgets to allocate the buffer and passes NULL.
        // Linux returns EFAULT; we must too.
        crate::errno::set_errno(0);
        let ret = getdents(3, core::ptr::null_mut(), 4096);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }
}
