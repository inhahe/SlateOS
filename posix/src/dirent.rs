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
//! - `dirfd` returns -1 (entire directory is buffered at opendir time).

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
#[unsafe(no_mangle)]
pub extern "C" fn opendir(name: *const u8) -> *mut Dir {
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }

    // Resolve relative paths against CWD.
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = (unsafe { crate::unistd::resolve_path(name, &mut resolved) }) else {
        errno::set_errno(errno::ENAMETOOLONG);
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn closedir(dirp: *mut Dir) -> i32 {
    if dirp.is_null() {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    free_dir(dirp);
    0
}

/// Reset a directory stream to the beginning.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn telldir(dirp: *mut Dir) -> i64 {
    if dirp.is_null() {
        return -1;
    }
    // SAFETY: dirp is valid.
    unsafe { (*dirp).pos as i64 }
}

/// Set the position of the directory stream.
#[unsafe(no_mangle)]
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
/// Stub: returns -1 since our Dir doesn't keep an open fd (the entire
/// listing is buffered at opendir time).
#[unsafe(no_mangle)]
pub extern "C" fn dirfd(_dirp: *mut Dir) -> i32 {
    // Our implementation reads the full directory at open time and
    // doesn't hold an open fd.  Return -1 (invalid fd).
    -1
}

/// Compare two directory entries alphabetically by name.
///
/// Suitable as a comparator for `scandir`.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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

    // First pass: count matching entries.
    let dir = unsafe { &mut *dirp };
    let total = dir.count;
    let mut count: usize = 0;

    // We'll do two passes: first count, then collect.  This avoids
    // over-allocating when a filter rejects many entries.
    dir.pos = 0;
    for _ in 0..total {
        let entry = readdir(dirp);
        if entry.is_null() {
            break;
        }
        let include = match filter {
            Some(f) => f(entry) != 0,
            None => true,
        };
        if include {
            count = count.wrapping_add(1);
        }
    }

    if count == 0 {
        closedir(dirp);
        // Allocate an empty array (POSIX allows returning 0 with a valid
        // but empty namelist).
        let arr = crate::malloc::malloc(core::mem::size_of::<*mut Dirent>());
        if arr.is_null() {
            errno::set_errno(errno::ENOMEM);
            return -1;
        }
        unsafe { *namelist = arr.cast::<*mut Dirent>(); }
        return 0;
    }

    // Allocate the array of pointers.
    let arr_size = count.wrapping_mul(core::mem::size_of::<*mut Dirent>());
    let arr = crate::malloc::malloc(arr_size);
    if arr.is_null() {
        closedir(dirp);
        errno::set_errno(errno::ENOMEM);
        return -1;
    }
    let arr_typed = arr.cast::<*mut Dirent>();

    // Second pass: collect entries.
    dir.pos = 0;
    let mut idx: usize = 0;
    for _ in 0..total {
        let entry = readdir(dirp);
        if entry.is_null() {
            break;
        }
        let include = match filter {
            Some(f) => f(entry) != 0,
            None => true,
        };
        if include && idx < count {
            // Allocate a copy of the Dirent.
            let dup = crate::malloc::malloc(core::mem::size_of::<Dirent>());
            if dup.is_null() {
                // Free everything allocated so far.
                let mut j: usize = 0;
                while j < idx {
                    // SAFETY: we wrote valid pointers at indices < idx.
                    unsafe { crate::malloc::free((*arr_typed.add(j)).cast::<u8>()); }
                    j = j.wrapping_add(1);
                }
                // SAFETY: arr was allocated by malloc above.
                unsafe { crate::malloc::free(arr); }
                closedir(dirp);
                errno::set_errno(errno::ENOMEM);
                return -1;
            }
            // SAFETY: entry points to dir.current which is valid; dup has
            // enough space for a Dirent.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    entry.cast::<u8>(),
                    dup,
                    core::mem::size_of::<Dirent>(),
                );
                *arr_typed.add(idx) = dup.cast::<Dirent>();
            }
            idx = idx.wrapping_add(1);
        }
    }

    closedir(dirp);

    let final_count = idx;

    // Sort if comparator provided.
    if let Some(cmp) = compar {
        // Simple insertion sort — directories are typically small.
        let mut i: usize = 1;
        while i < final_count {
            let mut j = i;
            while j > 0 {
                // SAFETY: j and j-1 are valid indices.
                let a = unsafe { arr_typed.add(j.wrapping_sub(1)) };
                let b = unsafe { arr_typed.add(j) };
                let a_ref = a.cast::<*const Dirent>();
                let b_ref = b.cast::<*const Dirent>();
                if cmp(a_ref, b_ref) > 0 {
                    // Swap.
                    unsafe {
                        let tmp = *a.cast::<*mut Dirent>();
                        *a.cast::<*mut Dirent>() = *b.cast::<*mut Dirent>();
                        *b.cast::<*mut Dirent>() = tmp;
                    }
                    j = j.wrapping_sub(1);
                } else {
                    break;
                }
            }
            i = i.wrapping_add(1);
        }
    }

    unsafe { *namelist = arr_typed; }
    final_count as i32
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
