//! `<fts.h>` — file tree traversal.
//!
//! Provides the `fts_open`, `fts_read`, `fts_children`, `fts_set`,
//! and `fts_close` functions for traversing file hierarchies.
//!
//! This is a stub implementation that records calls but does not
//! perform real filesystem traversal (the kernel filesystem is not
//! yet available).  All functions return appropriate error indicators.

use crate::errno;

// ---------------------------------------------------------------------------
// fts_open options
// ---------------------------------------------------------------------------

/// Follow symbolic links (compar to stat, not lstat).
pub const FTS_COMFOLLOW: i32 = 0x0001;

/// Logical traversal: follow all symlinks.
pub const FTS_LOGICAL: i32 = 0x0002;

/// Do not chdir during traversal.
pub const FTS_NOCHDIR: i32 = 0x0004;

/// Do not stat files; `fts_statp` is undefined.
pub const FTS_NOSTAT: i32 = 0x0008;

/// Physical traversal: do not follow symlinks.
pub const FTS_PHYSICAL: i32 = 0x0010;

/// Return dot-files.
pub const FTS_SEEDOT: i32 = 0x0020;

/// Do not cross mount points.
pub const FTS_XDEV: i32 = 0x0040;

// ---------------------------------------------------------------------------
// fts_info values (FTSENT::fts_info)
// ---------------------------------------------------------------------------

/// Preorder directory.
pub const FTS_D: i32 = 1;

/// Directory that causes a cycle.
pub const FTS_DC: i32 = 2;

/// Default (unknown type).
pub const FTS_DEFAULT: i32 = 3;

/// Unreadable directory.
pub const FTS_DNR: i32 = 4;

/// Dot file (`.` or `..`).
pub const FTS_DOT: i32 = 5;

/// Post-order directory.
pub const FTS_DP: i32 = 6;

/// Error (errno set).
pub const FTS_ERR: i32 = 7;

/// Regular file.
pub const FTS_F: i32 = 8;

/// Stale stat info (re-stat needed).
pub const FTS_INIT: i32 = 9;

/// No stat info requested.
pub const FTS_NS: i32 = 10;

/// No stat info available.
pub const FTS_NSOK: i32 = 11;

/// Symbolic link.
pub const FTS_SL: i32 = 12;

/// Symbolic link pointing to a nonexistent target.
pub const FTS_SLNONE: i32 = 13;

/// Name too long.
pub const FTS_W: i32 = 14;

// ---------------------------------------------------------------------------
// fts_set instructions
// ---------------------------------------------------------------------------

/// Follow this symbolic link.
pub const FTS_FOLLOW: i32 = 1;

/// Read this entry again.
pub const FTS_AGAIN: i32 = 2;

/// Skip this entry.
pub const FTS_SKIP: i32 = 3;

/// Do not descend into this directory.
pub const FTS_NOINSTR: i32 = 4;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// File tree entry returned by `fts_read`.
#[repr(C)]
pub struct FtsEnt {
    /// Info flags (FTS_D, FTS_F, etc.).
    pub fts_info: i32,

    /// Depth in the tree (root = 0).
    pub fts_level: i32,

    /// Length of `fts_path`.
    pub fts_pathlen: usize,

    /// Length of `fts_name`.
    pub fts_namelen: usize,

    /// Numeric link count.
    pub fts_nlink: u64,

    /// Error number (if FTS_ERR or FTS_DNR).
    pub fts_errno: i32,

    /// Instruction for fts_set (FTS_FOLLOW, FTS_SKIP, etc.).
    pub fts_instr: i32,

    /// Stat buffer pointer.
    pub fts_statp: *const crate::stat::Stat,

    /// File name (component).
    pub fts_name: *const u8,

    /// Full path.
    pub fts_path: *const u8,

    /// Pointer to parent entry.
    pub fts_parent: *mut FtsEnt,

    /// Linked list of children (via fts_children).
    pub fts_link: *mut FtsEnt,

    /// User-settable number for sorting.
    pub fts_number: i64,

    /// User-settable pointer.
    pub fts_pointer: *mut u8,
}

/// Opaque handle for an open FTS stream.
///
/// Stub implementation: contains only the options passed to `fts_open`.
#[repr(C)]
pub struct Fts {
    /// Options passed at open time.
    pub fts_options: i32,
}

// ---------------------------------------------------------------------------
// Functions (stubs)
// ---------------------------------------------------------------------------

/// Open a file hierarchy for traversal.
///
/// Returns null (not yet implemented). Sets errno to `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fts_open(
    _path_argv: *const *const u8,
    options: i32,
    _compar: Option<unsafe extern "C" fn(*const *const FtsEnt, *const *const FtsEnt) -> i32>,
) -> *mut Fts {
    let _ = options;
    errno::set_errno(errno::ENOSYS);
    core::ptr::null_mut()
}

/// Read the next entry from an FTS stream.
///
/// Returns null (not yet implemented). Sets errno to `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fts_read(_ftsp: *mut Fts) -> *mut FtsEnt {
    errno::set_errno(errno::ENOSYS);
    core::ptr::null_mut()
}

/// Return the children of the current directory.
///
/// Returns null (not yet implemented). Sets errno to `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fts_children(_ftsp: *mut Fts, _instr: i32) -> *mut FtsEnt {
    errno::set_errno(errno::ENOSYS);
    core::ptr::null_mut()
}

/// Set instructions for the current entry.
///
/// Returns -1 (not yet implemented). Sets errno to `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fts_set(
    _ftsp: *mut Fts,
    _f: *mut FtsEnt,
    _instr: i32,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Close an FTS stream.
///
/// Returns -1 (not yet implemented). Sets errno to `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fts_close(_ftsp: *mut Fts) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Option constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_option_constants() {
        assert_eq!(FTS_COMFOLLOW, 0x0001);
        assert_eq!(FTS_LOGICAL, 0x0002);
        assert_eq!(FTS_NOCHDIR, 0x0004);
        assert_eq!(FTS_NOSTAT, 0x0008);
        assert_eq!(FTS_PHYSICAL, 0x0010);
        assert_eq!(FTS_SEEDOT, 0x0020);
        assert_eq!(FTS_XDEV, 0x0040);
    }

    #[test]
    fn test_options_are_bitmask() {
        // Options should be combinable without collision.
        let all = FTS_COMFOLLOW | FTS_LOGICAL | FTS_NOCHDIR
                | FTS_NOSTAT | FTS_PHYSICAL | FTS_SEEDOT | FTS_XDEV;
        assert_eq!(all, 0x007F);
    }

    #[test]
    fn test_options_powers_of_two() {
        let opts = [
            FTS_COMFOLLOW, FTS_LOGICAL, FTS_NOCHDIR, FTS_NOSTAT,
            FTS_PHYSICAL, FTS_SEEDOT, FTS_XDEV,
        ];
        for &o in &opts {
            assert!(o > 0);
            assert_eq!(o & (o - 1), 0, "option 0x{o:X} is not a power of two");
        }
    }

    // -----------------------------------------------------------------------
    // Info constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_info_constants() {
        assert_eq!(FTS_D, 1);
        assert_eq!(FTS_DC, 2);
        assert_eq!(FTS_DEFAULT, 3);
        assert_eq!(FTS_DNR, 4);
        assert_eq!(FTS_DOT, 5);
        assert_eq!(FTS_DP, 6);
        assert_eq!(FTS_ERR, 7);
        assert_eq!(FTS_F, 8);
        assert_eq!(FTS_INIT, 9);
        assert_eq!(FTS_NS, 10);
        assert_eq!(FTS_NSOK, 11);
        assert_eq!(FTS_SL, 12);
        assert_eq!(FTS_SLNONE, 13);
        assert_eq!(FTS_W, 14);
    }

    #[test]
    fn test_info_constants_distinct() {
        let infos = [
            FTS_D, FTS_DC, FTS_DEFAULT, FTS_DNR, FTS_DOT, FTS_DP,
            FTS_ERR, FTS_F, FTS_INIT, FTS_NS, FTS_NSOK, FTS_SL,
            FTS_SLNONE, FTS_W,
        ];
        for i in 0..infos.len() {
            for j in (i + 1)..infos.len() {
                assert_ne!(
                    infos[i], infos[j],
                    "FTS info values must be distinct"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Instruction constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_instruction_constants() {
        assert_eq!(FTS_FOLLOW, 1);
        assert_eq!(FTS_AGAIN, 2);
        assert_eq!(FTS_SKIP, 3);
        assert_eq!(FTS_NOINSTR, 4);
    }

    #[test]
    fn test_instructions_distinct() {
        let instrs = [FTS_FOLLOW, FTS_AGAIN, FTS_SKIP, FTS_NOINSTR];
        for i in 0..instrs.len() {
            for j in (i + 1)..instrs.len() {
                assert_ne!(instrs[i], instrs[j]);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Struct sizes
    // -----------------------------------------------------------------------

    #[test]
    fn test_fts_struct_nonzero_size() {
        assert!(core::mem::size_of::<Fts>() > 0);
    }

    #[test]
    fn test_ftsent_struct_nonzero_size() {
        assert!(core::mem::size_of::<FtsEnt>() > 0);
    }

    // -----------------------------------------------------------------------
    // Function stubs
    // -----------------------------------------------------------------------

    #[test]
    fn test_fts_open_returns_null() {
        let result = fts_open(core::ptr::null(), FTS_PHYSICAL, None);
        assert!(result.is_null());
    }

    #[test]
    fn test_fts_read_returns_null() {
        let result = fts_read(core::ptr::null_mut());
        assert!(result.is_null());
    }

    #[test]
    fn test_fts_children_returns_null() {
        let result = fts_children(core::ptr::null_mut(), 0);
        assert!(result.is_null());
    }

    #[test]
    fn test_fts_set_returns_error() {
        let result = fts_set(
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            FTS_SKIP,
        );
        assert_eq!(result, -1);
    }

    #[test]
    fn test_fts_close_returns_error() {
        let result = fts_close(core::ptr::null_mut());
        assert_eq!(result, -1);
    }

    #[test]
    fn test_fts_open_sets_enosys() {
        crate::errno::set_errno(0);
        let _ = fts_open(core::ptr::null(), FTS_PHYSICAL, None);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }
}
