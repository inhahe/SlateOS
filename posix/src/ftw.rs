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
pub type NftwFn = extern "C" fn(*const u8, *const Stat, i32, *mut FTW) -> i32;

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
    let mut ftw_info = FTW {
        base: base_offset,
        level: current_depth,
    };

    if stat_result < 0 {
        return callback(path, &raw const sb, FTW_NS, &raw mut ftw_info);
    }

    let is_dir = (sb.st_mode & S_IFMT) == S_IFDIR;

    if !is_dir {
        return callback(path, &raw const sb, FTW_F, &raw mut ftw_info);
    }

    // Pre-order: call before children.
    if !depth_first {
        let ret = callback(path, &raw const sb, FTW_D, &raw mut ftw_info);
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
        let mut dp_info = FTW {
            base: base_offset,
            level: current_depth,
        };
        return callback(path, &raw const sb, FTW_DP, &raw mut dp_info);
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
    // Use checked_add to prevent usize overflow on adversarially long paths.
    let Some(total) = parent_len.checked_add(sep_len).and_then(|s| s.checked_add(name_len)) else {
        return 0;
    };

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- FTW type flag constants --

    #[test]
    fn test_ftw_type_flags() {
        assert_eq!(FTW_F, 0);
        assert_eq!(FTW_D, 1);
        assert_eq!(FTW_DNR, 2);
        assert_eq!(FTW_NS, 3);
        assert_eq!(FTW_SL, 4);
        assert_eq!(FTW_DP, 5);
        assert_eq!(FTW_SLN, 6);
    }

    #[test]
    fn test_ftw_flag_constants() {
        assert_eq!(FTW_PHYS, 1);
        assert_eq!(FTW_MOUNT, 2);
        assert_eq!(FTW_CHDIR, 4);
        assert_eq!(FTW_DEPTH, 8);
    }

    #[test]
    fn test_ftw_flags_are_distinct_bits() {
        // Each flag should be a distinct power of 2.
        let all = FTW_PHYS | FTW_MOUNT | FTW_CHDIR | FTW_DEPTH;
        assert_eq!(all, 15);
    }

    // -- is_dot_or_dotdot --

    #[test]
    fn test_is_dot() {
        assert!(is_dot_or_dotdot(b".\0".as_ptr()));
    }

    #[test]
    fn test_is_dotdot() {
        assert!(is_dot_or_dotdot(b"..\0".as_ptr()));
    }

    #[test]
    fn test_not_dot_regular_name() {
        assert!(!is_dot_or_dotdot(b"hello\0".as_ptr()));
    }

    #[test]
    fn test_not_dot_dotfile() {
        // ".bashrc" starts with '.' but is not "." or "..".
        assert!(!is_dot_or_dotdot(b".bashrc\0".as_ptr()));
    }

    #[test]
    fn test_not_dot_triple_dot() {
        // "..." is not "." or "..".
        assert!(!is_dot_or_dotdot(b"...\0".as_ptr()));
    }

    #[test]
    fn test_is_dot_or_dotdot_null() {
        assert!(!is_dot_or_dotdot(core::ptr::null()));
    }

    // -- find_basename_offset --

    #[test]
    fn test_basename_offset_no_slash() {
        assert_eq!(find_basename_offset(b"file.txt\0".as_ptr()), 0);
    }

    #[test]
    fn test_basename_offset_simple() {
        assert_eq!(find_basename_offset(b"/foo/bar\0".as_ptr()), 5);
    }

    #[test]
    fn test_basename_offset_root() {
        assert_eq!(find_basename_offset(b"/file\0".as_ptr()), 1);
    }

    #[test]
    fn test_basename_offset_nested() {
        assert_eq!(find_basename_offset(b"/a/b/c/d\0".as_ptr()), 7);
    }

    #[test]
    fn test_basename_offset_trailing_slash() {
        // "/foo/" → basename offset is 5 (empty basename after last /).
        assert_eq!(find_basename_offset(b"/foo/\0".as_ptr()), 5);
    }

    #[test]
    fn test_basename_offset_empty() {
        assert_eq!(find_basename_offset(b"\0".as_ptr()), 0);
    }

    #[test]
    fn test_basename_offset_null() {
        assert_eq!(find_basename_offset(core::ptr::null()), 0);
    }

    // -- build_child_path --

    #[test]
    fn test_build_child_path_simple() {
        let mut buf = [0u8; PATH_MAX];
        let len = build_child_path(
            b"/foo\0".as_ptr(),
            b"bar\0".as_ptr(),
            &mut buf,
        );
        assert_eq!(len, 8); // "/foo/bar"
        assert_eq!(&buf[..8], b"/foo/bar");
        assert_eq!(buf[8], 0);
    }

    #[test]
    fn test_build_child_path_trailing_slash() {
        let mut buf = [0u8; PATH_MAX];
        let len = build_child_path(
            b"/foo/\0".as_ptr(),
            b"bar\0".as_ptr(),
            &mut buf,
        );
        // Parent already ends with '/', so no extra separator.
        assert_eq!(len, 8); // "/foo/bar"
        assert_eq!(&buf[..8], b"/foo/bar");
    }

    #[test]
    fn test_build_child_path_root() {
        let mut buf = [0u8; PATH_MAX];
        let len = build_child_path(
            b"/\0".as_ptr(),
            b"etc\0".as_ptr(),
            &mut buf,
        );
        assert_eq!(len, 4); // "/etc"
        assert_eq!(&buf[..4], b"/etc");
    }

    #[test]
    fn test_build_child_path_empty_parent() {
        let mut buf = [0u8; PATH_MAX];
        let len = build_child_path(
            b"\0".as_ptr(),
            b"file\0".as_ptr(),
            &mut buf,
        );
        // Empty parent, no separator needed (parent_len == 0).
        assert_eq!(len, 4);
        assert_eq!(&buf[..4], b"file");
    }

    // -- FTW struct layout --

    #[test]
    fn test_ftw_struct_size() {
        // FTW has two i32 fields = 8 bytes.
        assert_eq!(core::mem::size_of::<FTW>(), 8);
    }

    #[test]
    fn test_ftw_struct_fields() {
        let f = FTW { base: 5, level: 3 };
        assert_eq!(f.base, 5);
        assert_eq!(f.level, 3);
    }

    // -- PATH_MAX and MAX_DEPTH constants --

    #[test]
    fn test_path_max() {
        assert_eq!(PATH_MAX, 4096);
    }

    #[test]
    fn test_max_depth() {
        assert_eq!(MAX_DEPTH, 32);
    }

    // -- build_child_path overflow --

    #[test]
    fn test_build_child_path_near_limit() {
        // A parent near PATH_MAX-2 with a 1-byte name should work.
        let mut parent = [b'a'; PATH_MAX];
        parent[PATH_MAX - 3] = 0; // 4093 bytes of 'a', null at 4093
        let name = b"x\0";
        let mut buf = [0u8; PATH_MAX];
        let len = build_child_path(parent.as_ptr(), name.as_ptr(), &mut buf);
        // 4093 + "/" + "x" = 4095 bytes, which fits in PATH_MAX (4096).
        assert!(len > 0, "should fit within PATH_MAX");
    }

    #[test]
    fn test_build_child_path_at_limit() {
        // Exactly at PATH_MAX: parent(4093) + "/" + name(1) + NUL = 4096
        // But total must be < PATH_MAX, not <=, so this is at the edge.
        let mut parent = [b'a'; PATH_MAX];
        parent[PATH_MAX - 3] = 0; // length = 4093
        // name = "xy" (2 bytes) → total = 4093+1+2 = 4096 → == PATH_MAX → returns 0
        let name = b"xy\0";
        let mut buf = [0u8; PATH_MAX];
        let len = build_child_path(parent.as_ptr(), name.as_ptr(), &mut buf);
        assert_eq!(len, 0, "should fail when result hits PATH_MAX");
    }

    // -- find_basename_offset more cases --

    #[test]
    fn test_basename_offset_only_slash() {
        // "/" → basename offset is 1.
        assert_eq!(find_basename_offset(b"/\0".as_ptr()), 1);
    }

    #[test]
    fn test_basename_offset_double_slash() {
        // "//" → last slash at position 1, offset = 2.
        assert_eq!(find_basename_offset(b"//\0".as_ptr()), 2);
    }

    #[test]
    fn test_basename_offset_relative() {
        // "foo/bar" → last slash at 3, offset = 4.
        assert_eq!(find_basename_offset(b"foo/bar\0".as_ptr()), 4);
    }

    // -- is_dot_or_dotdot empty string --

    #[test]
    fn test_is_dot_or_dotdot_empty() {
        // Empty string ('\0') starts with '\0', not '.'.
        assert!(!is_dot_or_dotdot(b"\0".as_ptr()));
    }

    // -- FTW type flags are distinct --

    #[test]
    fn test_ftw_type_flags_distinct() {
        let types = [FTW_F, FTW_D, FTW_DNR, FTW_NS, FTW_SL, FTW_DP, FTW_SLN];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j],
                    "FTW types at indices {i} and {j} must be distinct");
            }
        }
    }
}
