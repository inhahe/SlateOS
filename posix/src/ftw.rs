//! POSIX file tree walk (`<ftw.h>`).
//!
//! Provides `ftw` and `nftw` for recursively traversing directory trees.
//! Each file/directory visited triggers a user-supplied callback.
//!
//! ## Implementation
//!
//! Both entry points drive one walker ([`Walker`]) over our dirent
//! module, differing only in how an entry is delivered.  They used to be
//! two near-identical recursions, which is exactly how they came to
//! disagree: `nftw` parsed [`FTW_PHYS`] and then walked as if it had not
//! been given.
//!
//! One directory stream is held open per level, and `nopenfd` is
//! therefore both the descriptor budget and the depth at which the walk
//! stops (capped at [`MAX_DEPTH`]).  The path is a single buffer in the
//! walker, mutated in place as it descends and ascends, so recursion
//! carries a few words per level rather than 4 KiB.
//!
//! ## Nothing is skipped in silence
//!
//! Every entry the walk cannot handle is *reported*, because a traversal
//! that quietly omits files is worse than one that fails: the caller
//! believes it saw everything.  All three of these used to be a bare
//! `return 0` or `continue`:
//!
//! - A directory `opendir` refuses (typically `EACCES`) is reported
//!   [`FTW_DNR`], which is what POSIX has always said and what this
//!   module never once produced.
//! - A directory below the descriptor budget is reported [`FTW_DNR`]
//!   with `errno` = `ENOMEM` — a resource failure, not a property of the
//!   directory.
//! - A child whose path would exceed [`PATH_MAX`] ends the walk with -1
//!   and `ENAMETOOLONG`, since POSIX has no type flag that could report
//!   it per-entry.
//!
//! ## Limitations
//!
//! - Maximum path length is 4096 bytes.
//! - [`FTW_MOUNT`] and [`FTW_CHDIR`] are **rejected** with `EINVAL`
//!   rather than accepted and ignored — see [`UNSUPPORTED_NFTW_FLAGS`].
//! - No cycle detection: without [`FTW_PHYS`], a symlink loop walks
//!   until it hits [`MAX_DEPTH`] and stops there with [`FTW_DNR`].

use crate::errno;
use crate::fcntl::{S_IFDIR, S_IFLNK, S_IFMT};
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
// The walk
// ---------------------------------------------------------------------------

/// What one walk was asked for, carried down the tree unchanged.
#[derive(Clone, Copy)]
struct Walk {
    /// The deepest level at which a directory will be opened.  A
    /// directory at this level is reported [`FTW_DNR`] instead of being
    /// descended into — see [`Walker::entry`].
    depth_limit: i32,
    /// Report a directory *after* its children ([`FTW_DEPTH`]).
    depth_first: bool,
    /// Do not follow symbolic links ([`FTW_PHYS`]).
    physical: bool,
}

/// One traversal in progress.
///
/// The path buffer lives here, **once**, and is mutated in place as the
/// walk descends and ascends — the same discipline [`crate::fts`] uses.
/// It used to be a `[u8; PATH_MAX]` local inside the recursive function,
/// which meant 4 KiB of stack per level and 128 KiB at the depth cap; the
/// module's own header cited stack overflow as the reason for the cap,
/// while the code was what made the stack expensive.
struct Walker<F> {
    /// NUL-terminated path of the entry currently being considered.
    path: [u8; PATH_MAX],
    opts: Walk,
    /// Where an entry is delivered.  Takes `(path, stat, typeflag,
    /// level)`; `nftw` builds its [`FTW`] from the level and the path,
    /// and `ftw` throws both away.
    emit: F,
}

impl<F> Walker<F>
where
    F: FnMut(*const u8, *const Stat, i32, i32) -> i32,
{
    /// Deliver one entry to the caller's callback.
    fn call(&mut self, sb: *const Stat, typeflag: i32, level: i32) -> i32 {
        // `as_ptr` ends the borrow of `self.path` before `self.emit` is
        // borrowed mutably, which is what lets both live on `self`.
        let p = self.path.as_ptr();
        (self.emit)(p, sb, typeflag, level)
    }

    /// Consider the entry currently named by `self.path[..len]`.
    ///
    /// Returns 0 when the walk should continue, the callback's non-zero
    /// value when the caller asked to stop, or -1 with `errno` set on a
    /// failure that ends the walk.
    fn entry(&mut self, len: usize, level: i32) -> i32 {
        let mut sb: Stat = unsafe { core::mem::zeroed() };
        let p = self.path.as_ptr();

        // FTW_PHYS is the difference between describing the link and
        // describing what it points at — and, below, between walking
        // into a linked directory and not.  It used to be parsed and
        // then dropped on the floor, so `nftw(path, cb, n, FTW_PHYS)`
        // followed every symlink it was explicitly told not to follow.
        let rc = if self.opts.physical {
            crate::file::lstat(p, &raw mut sb)
        } else {
            crate::file::stat(p, &raw mut sb)
        };

        if rc < 0 {
            // Without FTW_PHYS the stat followed the link, so a failure
            // here may be a link with nothing on the far end — which
            // POSIX gives its own flag, because "this name is a dangling
            // symlink" and "I could not find out what this name is" are
            // different answers.
            if !self.opts.physical {
                let mut lb: Stat = unsafe { core::mem::zeroed() };
                if crate::file::lstat(p, &raw mut lb) >= 0 && (lb.st_mode & S_IFMT) == S_IFLNK {
                    return self.call(&raw const lb, FTW_SLN, level);
                }
            }
            return self.call(&raw const sb, FTW_NS, level);
        }

        if self.opts.physical && (sb.st_mode & S_IFMT) == S_IFLNK {
            return self.call(&raw const sb, FTW_SL, level);
        }

        if (sb.st_mode & S_IFMT) != S_IFDIR {
            return self.call(&raw const sb, FTW_F, level);
        }

        // A directory.  Open it *before* announcing it: POSIX says an
        // unreadable directory is reported as FTW_DNR, and FTW_DNR
        // replaces FTW_D rather than following it.  Opening afterwards
        // is what left `FTW_DNR` a constant this module defined, tested
        // the numeric value of, and never once produced — an EACCES
        // directory was skipped in silence, and a `du` built on this
        // under-reported without a word.
        let dir = if level >= self.opts.depth_limit {
            // Out of descriptor budget.  Nothing is wrong with the
            // directory, so this is a resource failure: ENOMEM, the same
            // answer `fts` gives when it runs out of traversal stack.
            // It used to `return 0` — the subtree vanished, and with
            // FTW_DEPTH the FTW_DP still fired, so a caller doing a
            // recursive delete removed the directory it had never
            // emptied.
            errno::set_errno(errno::ENOMEM);
            core::ptr::null_mut()
        } else {
            crate::dirent::opendir(p)
        };
        if dir.is_null() {
            return self.call(&raw const sb, FTW_DNR, level);
        }

        if !self.opts.depth_first {
            let ret = self.call(&raw const sb, FTW_D, level);
            if ret != 0 {
                crate::dirent::closedir(dir);
                return ret;
            }
        }

        let ret = self.children(dir, len, level);
        crate::dirent::closedir(dir);
        if ret != 0 {
            return ret;
        }

        if self.opts.depth_first {
            return self.call(&raw const sb, FTW_DP, level);
        }
        0
    }

    /// Walk the children of the directory `dir`, whose path occupies
    /// `self.path[..parent_len]`.
    fn children(&mut self, dir: *mut crate::dirent::Dir, parent_len: usize, level: i32) -> i32 {
        loop {
            let ent = crate::dirent::readdir(dir);
            if ent.is_null() {
                // End of listing.  `readdir` reports errors the same
                // way, but a `Dir` here is a snapshot taken at
                // `opendir`, so there is no read left to fail.
                return 0;
            }
            // SAFETY: readdir returned a valid entry pointer.
            let name = unsafe { core::ptr::addr_of!((*ent).d_name).cast::<u8>() };
            if is_dot_or_dotdot(name) {
                continue;
            }

            let child_len = append_component(&mut self.path, parent_len, name);
            if child_len == 0 {
                // POSIX has no type flag for "the name is too long to
                // build", so there is no way to report this entry and
                // keep going.  glibc fails the whole walk; skipping it
                // silently — which is what this did — turns a truncated
                // traversal into a successful one.
                errno::set_errno(errno::ENAMETOOLONG);
                return -1;
            }

            let ret = self.entry(child_len, level.wrapping_add(1));
            // Cut the path back to the parent before the next sibling.
            // The recursion below us extended it; nothing else does.
            if let Some(slot) = self.path.get_mut(parent_len) {
                *slot = 0;
            }
            if ret != 0 {
                return ret;
            }
        }
    }
}

/// Seed a walker with `root` and run it.
fn run<F>(root: *const u8, opts: Walk, emit: F) -> i32
where
    F: FnMut(*const u8, *const Stat, i32, i32) -> i32,
{
    let root_len = unsafe { crate::string::strlen(root) };
    if root_len >= PATH_MAX {
        errno::set_errno(errno::ENAMETOOLONG);
        return -1;
    }
    let mut w = Walker {
        path: [0u8; PATH_MAX],
        opts,
        emit,
    };
    let mut i: usize = 0;
    while i < root_len {
        if let Some(slot) = w.path.get_mut(i) {
            // SAFETY: i < strlen(root), so this byte is inside the string.
            *slot = unsafe { *root.add(i) };
        }
        i = i.wrapping_add(1);
    }
    w.entry(root_len, 0)
}

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
/// handles.  One is held per level, so it is also the depth at which the
/// walk stops — a directory below it is reported [`FTW_DNR`] with
/// `errno` = `ENOMEM` rather than silently skipped.  Capped at
/// [`MAX_DEPTH`].
///
/// Returns 0 on success, -1 with `errno` set on error, or the non-zero
/// value returned by `callback`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ftw(dirpath: *const u8, callback: FtwFn, nopenfd: i32) -> i32 {
    if dirpath.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if nopenfd < 1 {
        // A walk that may open no directory cannot walk.
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    run(
        dirpath,
        Walk {
            depth_limit: nopenfd.min(MAX_DEPTH),
            depth_first: false,
            physical: false,
        },
        // `ftw` has no FTW_SL/FTW_SLN/FTW_DP: those are `nftw`'s, and a
        // caller written against `ftw` will not have a case for them.
        // Only FTW_SLN can arise here (a dangling symlink, which `ftw`
        // reports as a failed stat), and folding it here rather than
        // suppressing it in the walker keeps the walker's answer true.
        |p, sb, flag, _level| {
            let flag = if flag == FTW_SLN { FTW_NS } else { flag };
            callback(p, sb, flag)
        },
    )
}

// ---------------------------------------------------------------------------
// nftw
// ---------------------------------------------------------------------------

/// Callback type for `nftw`.
///
/// Parameters: (pathname, stat_buf, typeflag, ftwbuf).
pub type NftwFn = extern "C" fn(*const u8, *const Stat, i32, *mut FTW) -> i32;

/// Flags `nftw` cannot honour, and therefore refuses.
///
/// Accepting a flag and ignoring it is the worst of the three options: a
/// caller that passes [`FTW_CHDIR`] uses `ftwbuf->base` as a *relative*
/// filename, so ignoring it silently points every callback at the wrong
/// file; a caller that passes [`FTW_MOUNT`] is asking not to cross into
/// another filesystem, and ignoring that is how a `--one-file-system`
/// delete walks into a network mount.  Refusing is loud, is trivially
/// reversible when the flags are implemented, and cannot corrupt
/// anything.  See design-decisions.md §761.
const UNSUPPORTED_NFTW_FLAGS: i32 = FTW_MOUNT | FTW_CHDIR;

/// Walk a file tree with extended options.
///
/// Supports [`FTW_PHYS`] and [`FTW_DEPTH`].  [`FTW_MOUNT`] and
/// [`FTW_CHDIR`] are rejected with `EINVAL` — see
/// [`UNSUPPORTED_NFTW_FLAGS`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nftw(dirpath: *const u8, callback: NftwFn, nopenfd: i32, flags: i32) -> i32 {
    if dirpath.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if nopenfd < 1 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if flags & UNSUPPORTED_NFTW_FLAGS != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    run(
        dirpath,
        Walk {
            depth_limit: nopenfd.min(MAX_DEPTH),
            depth_first: flags & FTW_DEPTH != 0,
            physical: flags & FTW_PHYS != 0,
        },
        |p, sb, flag, level| {
            let mut info = FTW {
                base: find_basename_offset(p),
                level,
            };
            callback(p, sb, flag, &raw mut info)
        },
    )
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

/// Append `/name` to the path already in `buf[..parent_len]`.
///
/// Returns the new length, or 0 if it doesn't fit.  Writes in place
/// rather than building into a second buffer, because the caller does
/// this once per level and a per-level `[u8; PATH_MAX]` is 4 KiB of
/// stack that the recursion has to carry all the way down.
fn append_component(buf: &mut [u8; PATH_MAX], parent_len: usize, name: *const u8) -> usize {
    // SAFETY: `name` is a NUL-terminated name from `readdir`.
    let name_len = unsafe { crate::string::strlen(name) };

    // parent + "/" + name + NUL must fit.
    let needs_sep = parent_len > 0
        && buf
            .get(parent_len.wrapping_sub(1))
            .is_some_and(|b| *b != b'/');
    let sep_len: usize = usize::from(needs_sep);
    // Use checked_add to prevent usize overflow on adversarially long paths.
    let Some(total) = parent_len
        .checked_add(sep_len)
        .and_then(|s| s.checked_add(name_len))
    else {
        return 0;
    };

    if total >= PATH_MAX {
        return 0;
    }

    let mut i = parent_len;

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
            // SAFETY: j < strlen(name), so this byte is inside the name.
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
pub extern "C" fn ftw64(path: *const u8, callback: FtwFn, maxfds: i32) -> i32 {
    ftw(path, callback, maxfds)
}

/// `nftw64` — Large File Support alias for `nftw`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nftw64(path: *const u8, callback: NftwFn, maxfds: i32, flags: i32) -> i32 {
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

    // -- append_component --
    //
    // The buffer is the walker's one path, seeded with the parent and
    // extended in place, so each case sets up the parent bytes first.

    fn seeded(parent: &[u8]) -> ([u8; PATH_MAX], usize) {
        let mut buf = [0u8; PATH_MAX];
        buf[..parent.len()].copy_from_slice(parent);
        (buf, parent.len())
    }

    #[test]
    fn test_append_component_simple() {
        let (mut buf, parent_len) = seeded(b"/foo");
        let len = append_component(&mut buf, parent_len, b"bar\0".as_ptr());
        assert_eq!(len, 8); // "/foo/bar"
        assert_eq!(&buf[..8], b"/foo/bar");
        assert_eq!(buf[8], 0);
    }

    #[test]
    fn test_append_component_trailing_slash() {
        let (mut buf, parent_len) = seeded(b"/foo/");
        let len = append_component(&mut buf, parent_len, b"bar\0".as_ptr());
        // Parent already ends with '/', so no extra separator.
        assert_eq!(len, 8); // "/foo/bar"
        assert_eq!(&buf[..8], b"/foo/bar");
    }

    #[test]
    fn test_append_component_root() {
        let (mut buf, parent_len) = seeded(b"/");
        let len = append_component(&mut buf, parent_len, b"etc\0".as_ptr());
        assert_eq!(len, 4); // "/etc"
        assert_eq!(&buf[..4], b"/etc");
    }

    #[test]
    fn test_append_component_empty_parent() {
        let (mut buf, parent_len) = seeded(b"");
        let len = append_component(&mut buf, parent_len, b"file\0".as_ptr());
        // Empty parent, no separator needed (parent_len == 0).
        assert_eq!(len, 4);
        assert_eq!(&buf[..4], b"file");
    }

    #[test]
    fn test_append_component_overwrites_the_previous_sibling() {
        // The whole point of writing in place: the buffer is reused for
        // every child of a directory, so a shorter name must not leave
        // the tail of a longer one behind it.
        let (mut buf, parent_len) = seeded(b"/d");
        let long = append_component(&mut buf, parent_len, b"aaaaaaaa\0".as_ptr());
        assert_eq!(&buf[..long], b"/d/aaaaaaaa");
        let short = append_component(&mut buf, parent_len, b"b\0".as_ptr());
        assert_eq!(&buf[..short], b"/d/b");
        assert_eq!(buf[short], 0, "the name must end where it says it ends");
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

    // -- append_component overflow --

    #[test]
    fn test_append_component_near_limit() {
        // A parent near PATH_MAX-2 with a 1-byte name should work.
        let mut buf = [b'a'; PATH_MAX];
        let parent_len = PATH_MAX - 3; // 4093 bytes of 'a', null at 4093
        buf[parent_len] = 0;
        let len = append_component(&mut buf, parent_len, b"x\0".as_ptr());
        // 4093 + "/" + "x" = 4095 bytes, which fits in PATH_MAX (4096).
        assert!(len > 0, "should fit within PATH_MAX");
    }

    #[test]
    fn test_append_component_at_limit() {
        // Exactly at PATH_MAX: parent(4093) + "/" + name(2) + NUL = 4097,
        // so the 4096-byte buffer cannot hold it and the append must refuse.
        let mut buf = [b'a'; PATH_MAX];
        let parent_len = PATH_MAX - 3; // length = 4093
        buf[parent_len] = 0;
        let len = append_component(&mut buf, parent_len, b"xy\0".as_ptr());
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
                assert_ne!(
                    types[i], types[j],
                    "FTW types at indices {i} and {j} must be distinct"
                );
            }
        }
    }

    // -- argument validation --
    //
    // These reject before any syscall is attempted, so they are the part
    // of the walk that is honestly testable on the host (where every raw
    // syscall returns ENOSYS).

    extern "C" fn never_called_ftw(_p: *const u8, _sb: *const Stat, _flag: i32) -> i32 {
        panic!("callback must not run when the arguments are rejected");
    }

    extern "C" fn never_called_nftw(
        _p: *const u8,
        _sb: *const Stat,
        _flag: i32,
        _fb: *mut FTW,
    ) -> i32 {
        panic!("callback must not run when the arguments are rejected");
    }

    #[test]
    fn test_ftw_null_path_efault() {
        assert_eq!(ftw(core::ptr::null(), never_called_ftw, 4), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_nftw_null_path_efault() {
        assert_eq!(nftw(core::ptr::null(), never_called_nftw, 4, 0), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_ftw_zero_nopenfd_einval() {
        // A walk allowed no open directory cannot descend at all, so the
        // only honest answer is a refusal — not a walk that reports the
        // root and calls the tree covered.
        assert_eq!(ftw(b"/tmp\0".as_ptr(), never_called_ftw, 0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_nftw_zero_nopenfd_einval() {
        assert_eq!(nftw(b"/tmp\0".as_ptr(), never_called_nftw, 0, 0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_nftw_rejects_ftw_mount() {
        // Accepted-and-ignored is how a --one-file-system delete walks
        // into a network mount.  See UNSUPPORTED_NFTW_FLAGS.
        assert_eq!(
            nftw(b"/tmp\0".as_ptr(), never_called_nftw, 4, FTW_MOUNT),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_nftw_rejects_ftw_chdir() {
        assert_eq!(
            nftw(b"/tmp\0".as_ptr(), never_called_nftw, 4, FTW_CHDIR),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_nftw_accepts_the_flags_it_implements() {
        // FTW_PHYS|FTW_DEPTH must *not* be refused: the rejection is a
        // deliberate list, and a test that only checks the refusals would
        // pass just as happily if the check rejected everything.
        let rc = nftw(b"\0".as_ptr(), passthrough_nftw, 4, FTW_PHYS | FTW_DEPTH);
        assert_ne!(
            errno::get_errno(),
            errno::EINVAL,
            "supported flags must not be refused (rc = {rc})"
        );
    }

    extern "C" fn passthrough_nftw(
        _p: *const u8,
        _sb: *const Stat,
        _flag: i32,
        _fb: *mut FTW,
    ) -> i32 {
        0
    }

    #[test]
    fn test_run_rejects_an_over_long_root() {
        // strlen(root) >= PATH_MAX is refused before anything is copied
        // into the walker's buffer — the copy loop's bound, not a check
        // inside it, is what keeps that buffer intact.
        let mut root = [b'a'; PATH_MAX + 9];
        root[PATH_MAX + 8] = 0;
        assert_eq!(ftw(root.as_ptr(), never_called_ftw, 4), -1);
        assert_eq!(errno::get_errno(), errno::ENAMETOOLONG);
    }
}
