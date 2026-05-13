//! POSIX file tree walk (`<ftw.h>`).
//!
//! Provides `ftw` and `nftw` for recursively traversing directory trees.
//! Each file/directory visited triggers a user-supplied callback.
//!
//! ## Implementation
//!
//! Uses our dirent module for directory reading.  Recursion depth is
//! limited by the `nopenfd` parameter (capped at 32 to prevent stack
//! overflow in deeply nested trees).
//!
//! ## Limitations
//!
//! - Maximum path length is 4096 bytes.
//! - `FTW_MOUNT` flag is not supported (cross-device traversal cannot
//!   be detected without statvfs comparison).
//! - Symbolic link detection depends on `lstat` (uses `stat` as
//!   fallback, treating symlinks as regular files).

use crate::errno;
use crate::fcntl::{S_IFDIR, S_IFMT};
use crate::stat::Stat;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Regular file.
pub const FTW_F: i32 = 0;
/// Directory.
pub const FTW_D: i32 = 1;
/// Unreadable directory.
pub const FTW_DNR: i32 = 2;
/// `stat` failed (not a symlink).
pub const FTW_NS: i32 = 3;
/// Symbolic link (nftw only, with FTW_PHYS).
pub const FTW_SL: i32 = 4;
/// Directory, all children processed (nftw with FTW_DEPTH).
pub const FTW_DP: i32 = 5;
/// Symbolic link pointing to nonexistent file (nftw with FTW_PHYS).
pub const FTW_SLN: i32 = 6;

/// `nftw` flag: do not follow symbolic links.
pub const FTW_PHYS: i32 = 1;
/// `nftw` flag: stay on the same filesystem.
pub const FTW_MOUNT: i32 = 2;
/// `nftw` flag: change to each directory before reading it.
pub const FTW_CHDIR: i32 = 4;
/// `nftw` flag: do a depth-first search (call callback after children).
pub const FTW_DEPTH: i32 = 8;

/// Extra info passed to the `nftw` callback.
#[repr(C)]
pub struct FTW {
    /// Offset of the filename in the pathname.
    pub base: i32,
    /// Depth of this entry relative to the starting path.
    pub level: i32,
}

/// Maximum path length.
const PATH_MAX: usize = 4096;

/// Maximum recursion depth.
const MAX_DEPTH: i32 = 32;

// ---------------------------------------------------------------------------
// ftw
// ---------------------------------------------------------------------------

/// Callback type for `ftw`.
///
/// Parameters: (pathname, stat_buf, typeflag).
/// Return 0 to continue, non-zero to stop.
pub type FtwFn = extern "C" fn(*const u8, *const Stat, i32) -> i32;

/// Walk a file tree, calling `callback` for each entry.
///
/// `nopenfd` limits the number of simultaneously open directory
/// handles (used to limit recursion depth).
///
/// Returns 0 on success, -1 on error, or the non-zero value
/// returned by `callback`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ftw(
    dirpath: *const u8,
    callback: FtwFn,
    nopenfd: i32,
) -> i32 {
    if dirpath.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let depth_limit = nopenfd.min(MAX_DEPTH);
    ftw_recurse(dirpath, callback, depth_limit, 0)
}

/// Internal recursive tree walk for `ftw`.
fn ftw_recurse(
    path: *const u8,
    callback: FtwFn,
    depth_limit: i32,
    current_depth: i32,
) -> i32 {
    // Stat the path.
    let mut sb: Stat = unsafe { core::mem::zeroed() };
    let stat_result = crate::file::stat(path, &raw mut sb);

    if stat_result < 0 {
        // stat failed — call with FTW_NS.
        return callback(path, &raw const sb, FTW_NS);
    }

    // Check if it's a directory.
    let is_dir = (sb.st_mode & S_IFMT) == S_IFDIR;

    if !is_dir {
        // Regular file (or special file).
        return callback(path, &raw const sb, FTW_F);
    }

    // It's a directory — call callback first (pre-order).
    let ret = callback(path, &raw const sb, FTW_D);
    if ret != 0 {
        return ret;
    }

    // Don't recurse if we've hit the depth limit.
    if current_depth >= depth_limit {
        return 0;
    }

    // Open and iterate the directory.
    walk_directory(path, callback, depth_limit, current_depth)
}

/// Open a directory and walk its children.
fn walk_directory(
    path: *const u8,
    callback: FtwFn,
    depth_limit: i32,
    current_depth: i32,
) -> i32 {
    let dir = crate::dirent::opendir(path);
    if dir.is_null() {
        return 0; // Can't open — skip (already called FTW_D above).
    }

    loop {
        let entry = crate::dirent::readdir(dir);
        if entry.is_null() {
            break;
        }

        // Get entry name.
        // SAFETY: readdir returned a valid entry.
        let d_name_ptr = unsafe { core::ptr::addr_of!((*entry).d_name).cast::<u8>() };

        // Skip "." and "..".
        if is_dot_or_dotdot(d_name_ptr) {
            continue;
        }

        // Build child path: path + "/" + d_name.
        let mut child_path = [0u8; PATH_MAX];
        let child_len = build_child_path(path, d_name_ptr, &mut child_path);
        if child_len == 0 {
            continue; // Path too long — skip.
        }

        let ret = ftw_recurse(
            child_path.as_ptr(),
            callback,
            depth_limit,
            current_depth.wrapping_add(1),
        );
        if ret != 0 {
            crate::dirent::closedir(dir);
            return ret;
        }
    }

    crate::dirent::closedir(dir);
    0
}

// ---------------------------------------------------------------------------
// nftw
// ---------------------------------------------------------------------------

/// Callback type for `nftw`.
///
/// Parameters: (pathname, stat_buf, typeflag, ftwbuf).
pub type NftwFn = extern "C" fn(*const u8, *const Stat, i32, *const FTW) -> i32;

/// Walk a file tree with extended options.
///
/// Like `ftw` but supports flags (`FTW_DEPTH`, `FTW_PHYS`, etc.)
/// and provides an `FTW` struct with base offset and depth level.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nftw(
    dirpath: *const u8,
    callback: NftwFn,
    nopenfd: i32,
    flags: i32,
) -> i32 {
    if dirpath.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let depth_limit = nopenfd.min(MAX_DEPTH);
    let depth_first = flags & FTW_DEPTH != 0;

    nftw_recurse(dirpath, callback, depth_limit, 0, depth_first)
}

/// Internal recursive tree walk for `nftw`.
fn nftw_recurse(
    path: *const u8,
    callback: NftwFn,
    depth_limit: i32,
    current_depth: i32,
    depth_first: bool,
) -> i32 {
    let mut sb: Stat = unsafe { core::mem::zeroed() };
    let stat_result = crate::file::stat(path, &raw mut sb);

    let base_offset = find_basename_offset(path);
    let ftw_info = FTW {
        base: base_offset,
        level: current_depth,
    };

    if stat_result < 0 {
        return callback(path, &raw const sb, FTW_NS, &raw const ftw_info);
    }

    let is_dir = (sb.st_mode & S_IFMT) == S_IFDIR;

    if !is_dir {
        return callback(path, &raw const sb, FTW_F, &raw const ftw_info);
    }

    // Pre-order: call before children.
    if !depth_first {
        let ret = callback(path, &raw const sb, FTW_D, &raw const ftw_info);
        if ret != 0 {
            return ret;
        }
    }

    // Recurse into children (if within depth limit).
    if current_depth < depth_limit {
        let ret = nftw_walk_directory(
            path, callback, depth_limit, current_depth, depth_first,
        );
        if ret != 0 {
            return ret;
        }
    }

    // Post-order: call after children.
    if depth_first {
        let dp_info = FTW {
            base: base_offset,
            level: current_depth,
        };
        return callback(path, &raw const sb, FTW_DP, &raw const dp_info);
    }

    0
}

/// Walk directory children for `nftw`.
fn nftw_walk_directory(
    path: *const u8,
    callback: NftwFn,
    depth_limit: i32,
    current_depth: i32,
    depth_first: bool,
) -> i32 {
    let dir = crate::dirent::opendir(path);
    if dir.is_null() {
        return 0;
    }

    loop {
        let entry = crate::dirent::readdir(dir);
        if entry.is_null() {
            break;
        }

        let d_name_ptr = unsafe { core::ptr::addr_of!((*entry).d_name).cast::<u8>() };

        if is_dot_or_dotdot(d_name_ptr) {
            continue;
        }

        let mut child_path = [0u8; PATH_MAX];
        let child_len = build_child_path(path, d_name_ptr, &mut child_path);
        if child_len == 0 {
            continue;
        }

        let ret = nftw_recurse(
            child_path.as_ptr(),
            callback,
            depth_limit,
            current_depth.wrapping_add(1),
            depth_first,
        );
        if ret != 0 {
            crate::dirent::closedir(dir);
            return ret;
        }
    }

    crate::dirent::closedir(dir);
    0
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Check if a name is "." or "..".
fn is_dot_or_dotdot(name: *const u8) -> bool {
    if name.is_null() {
        return false;
    }
    // SAFETY: name is a valid C string from readdir.
    let c0 = unsafe { *name };
    if c0 != b'.' {
        return false;
    }
    let c1 = unsafe { *name.add(1) };
    if c1 == 0 {
        return true; // "."
    }
    if c1 == b'.' {
        let c2 = unsafe { *name.add(2) };
        return c2 == 0; // ".."
    }
    false
}

/// Build a child path: parent + "/" + name.
///
/// Returns the length of the resulting path, or 0 if it doesn't fit.
fn build_child_path(parent: *const u8, name: *const u8, buf: &mut [u8; PATH_MAX]) -> usize {
    let parent_len = unsafe { crate::string::strlen(parent) };
    let name_len = unsafe { crate::string::strlen(name) };

    // parent + "/" + name + NUL must fit.
    let needs_sep = parent_len > 0 && unsafe { *parent.add(parent_len.wrapping_sub(1)) } != b'/';
    let sep_len: usize = usize::from(needs_sep);
    let total = parent_len.wrapping_add(sep_len).wrapping_add(name_len);

    if total >= PATH_MAX {
        return 0;
    }

    // Copy parent.
    let mut i: usize = 0;
    while i < parent_len {
        if let Some(slot) = buf.get_mut(i) {
            *slot = unsafe { *parent.add(i) };
        }
        i = i.wrapping_add(1);
    }

    // Add separator if needed.
    if needs_sep {
        if let Some(slot) = buf.get_mut(i) {
            *slot = b'/';
        }
        i = i.wrapping_add(1);
    }

    // Copy name.
    let mut j: usize = 0;
    while j < name_len {
        if let Some(slot) = buf.get_mut(i) {
            *slot = unsafe { *name.add(j) };
        }
        i = i.wrapping_add(1);
        j = j.wrapping_add(1);
    }

    // NUL terminate.
    if let Some(slot) = buf.get_mut(i) {
        *slot = 0;
    }

    i
}

/// Find the offset of the basename component in a path.
fn find_basename_offset(path: *const u8) -> i32 {
    if path.is_null() {
        return 0;
    }

    let len = unsafe { crate::string::strlen(path) };
    if len == 0 {
        return 0;
    }

    // Walk backwards to find the last '/'.
    let mut i = len;
    loop {
        if i == 0 {
            return 0; // No '/' found — basename is at offset 0.
        }
        i = i.wrapping_sub(1);
        if unsafe { *path.add(i) } == b'/' {
            return i.wrapping_add(1) as i32;
        }
    }
}

// ---------------------------------------------------------------------------
// LFS64 aliases — our off_t is already 64-bit
// ---------------------------------------------------------------------------

/// `ftw64` — Large File Support alias for `ftw`.
///
/// On our OS, `off_t` is always 64-bit (LP64 data model), so
/// `struct stat` and `ftw` already handle large files.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ftw64(
    path: *const u8,
    callback: FtwFn,
    maxfds: i32,
) -> i32 {
    ftw(path, callback, maxfds)
}

/// `nftw64` — Large File Support alias for `nftw`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nftw64(
    path: *const u8,
    callback: NftwFn,
    maxfds: i32,
    flags: i32,
) -> i32 {
    nftw(path, callback, maxfds, flags)
}
