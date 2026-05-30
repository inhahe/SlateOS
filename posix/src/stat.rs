//! POSIX stat structure.
//!
//! The stat struct returned by `stat()`, `fstat()`, and `lstat()`.
//! Layout matches Linux x86_64 `struct stat` for binary compatibility.

use crate::types::*;

/// Timespec — seconds + nanoseconds.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Timespec {
    /// Seconds since epoch.
    pub tv_sec: TimeT,
    /// Nanoseconds (0..999_999_999).
    pub tv_nsec: i64,
}

/// File status structure.
///
/// Returned by `stat()`, `fstat()`, `lstat()`.
/// Layout matches Linux x86_64 `struct stat`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Stat {
    /// Device ID of device containing file.
    pub st_dev: DevT,
    /// Inode number.
    pub st_ino: InoT,
    /// Number of hard links.
    pub st_nlink: NlinkT,
    /// File mode (permissions + type).
    pub st_mode: ModeT,
    /// User ID of owner.
    pub st_uid: UidT,
    /// Group ID of owner.
    pub st_gid: GidT,
    /// Padding.
    _pad0: i32,
    /// Device ID (if special file).
    pub st_rdev: DevT,
    /// Total size in bytes.
    pub st_size: OffT,
    /// Block size for filesystem I/O.
    pub st_blksize: BlksizeT,
    /// Number of 512-byte blocks allocated.
    pub st_blocks: BlkcntT,
    /// Time of last access.
    pub st_atim: Timespec,
    /// Time of last modification.
    pub st_mtim: Timespec,
    /// Time of last status change.
    pub st_ctim: Timespec,
    /// Reserved.
    _reserved: [i64; 3],
}

impl Default for Stat {
    fn default() -> Self {
        // SAFETY: Stat is a C-compatible struct, zero-init is valid.
        unsafe { core::mem::zeroed() }
    }
}

impl Stat {
    /// Create a zeroed stat structure.
    #[must_use]
    pub fn zeroed() -> Self {
        Self::default()
    }

    /// Check if this is a regular file.
    #[must_use]
    pub fn is_file(&self) -> bool {
        (self.st_mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFREG
    }

    /// Check if this is a directory.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        (self.st_mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFDIR
    }

    /// Check if this is a symbolic link.
    #[must_use]
    pub fn is_link(&self) -> bool {
        (self.st_mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFLNK
    }

    /// Check if this is a character device.
    #[must_use]
    pub fn is_chr(&self) -> bool {
        (self.st_mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFCHR
    }

    /// Check if this is a block device.
    #[must_use]
    pub fn is_blk(&self) -> bool {
        (self.st_mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFBLK
    }

    /// Check if this is a FIFO (named pipe).
    #[must_use]
    pub fn is_fifo(&self) -> bool {
        (self.st_mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFIFO
    }

    /// Check if this is a socket.
    #[must_use]
    pub fn is_sock(&self) -> bool {
        (self.st_mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFSOCK
    }
}

// ---------------------------------------------------------------------------
// Kernel `FsStatResult` translation
// ---------------------------------------------------------------------------
//
// `SYS_FS_STAT`, `SYS_FS_LSTAT`, and `SYS_FS_FSTAT` do NOT write a POSIX
// `struct stat`.  They write a compact, kernel-defined 72-byte
// `FsStatResult` whose layout bears no resemblance to `struct stat`.
// The low 16 bytes are ABI-stable (unchanged from the original 16-byte
// form); the kernel widened the buffer to carry the rest of the metadata
// it already tracks:
//
//   bytes [0..8]    file size                  (u64, little-endian)
//   byte  [8]       entry type                 (0=file, 1=dir, 2=volume label, 3=symlink)
//   bytes [9..12]   reserved (zero)
//   bytes [12..16]  hard link count            (u32, little-endian; 0 if not provided)
//   bytes [16..20]  permission bits            (u32; 0 = unknown, synthesize by type)
//   bytes [20..24]  uid                        (u32)
//   bytes [24..28]  gid                        (u32)
//   bytes [28..32]  file attribute flags       (u32, kernel FileAttr bits; unused here)
//   bytes [32..40]  512-byte block count        (u64; 0 = derive from size)
//   bytes [40..48]  modified time (ns since epoch, u64; 0 = unknown)
//   bytes [48..56]  accessed time (ns since epoch, u64; 0 = unknown)
//   bytes [56..64]  changed  time (ns since epoch, u64; 0 = unknown)
//   bytes [64..72]  created  time (ns since epoch, u64; 0 = unknown)
//
// Passing a `struct stat` pointer straight to those syscalls leaves
// `st_size` (offset 48) and `st_mode` (offset 24) untouched — i.e. zero —
// so every consumer that inspects the struct gets garbage.  All callers
// MUST funnel the raw bytes through [`fill_from_fsstat`] to obtain a
// correctly populated `struct stat`.

/// Number of bytes the kernel writes for an `FsStatResult`.
pub(crate) const KERNEL_STAT_LEN: usize = 72;

/// Split a nanoseconds-since-epoch value into a POSIX [`Timespec`].
///
/// A value of 0 means "not available"; we still produce a zeroed
/// `Timespec` (epoch), which is the conventional Linux behaviour for
/// filesystems that do not record the timestamp.
fn ns_to_timespec(ns: u64) -> Timespec {
    Timespec {
        tv_sec: i64::try_from(ns / 1_000_000_000).unwrap_or(i64::MAX),
        tv_nsec: i64::try_from(ns % 1_000_000_000).unwrap_or(0),
    }
}

/// Read a little-endian `u64` from `raw` at `off`, defaulting to 0 if the
/// slice is too short (defensive — the buffer is fixed-size in practice).
fn le_u64(raw: &[u8; KERNEL_STAT_LEN], off: usize) -> u64 {
    raw.get(off..off.wrapping_add(8))
        .and_then(|s| <[u8; 8]>::try_from(s).ok())
        .map_or(0, u64::from_le_bytes)
}

/// Read a little-endian `u32` from `raw` at `off`, defaulting to 0.
fn le_u32(raw: &[u8; KERNEL_STAT_LEN], off: usize) -> u32 {
    raw.get(off..off.wrapping_add(4))
        .and_then(|s| <[u8; 4]>::try_from(s).ok())
        .map_or(0, u32::from_le_bytes)
}

/// Translate the kernel's 72-byte `FsStatResult` into a POSIX `struct stat`.
///
/// The kernel now conveys size, entry type, link count, permission bits,
/// ownership, block count, and the access/modify/change timestamps.  When
/// the filesystem does not record real permission bits (`0`), they are
/// synthesized by type the way Linux does for metadata-less filesystems
/// (files `0644`, directories `0755`, symlinks `0777`).  `st_ino` is left
/// zero — [`crate::fs::FileMeta`] carries no inode number yet (see
/// `todo.txt`).  The creation time is carried in the buffer but `struct
/// stat` has no birth-time field; it is surfaced via `statx`'s
/// `STATX_BTIME` once that path is wired (see `todo.txt`).
pub(crate) fn fill_from_fsstat(buf: &mut Stat, raw: &[u8; KERNEL_STAT_LEN]) {
    *buf = Stat::zeroed();

    let size = le_u64(raw, 0);
    let entry_type = raw[8];
    let nlinks = le_u32(raw, 12);
    let permissions = le_u32(raw, 16);
    let uid = le_u32(raw, 20);
    let gid = le_u32(raw, 24);
    let blocks = le_u64(raw, 32);
    let modified_ns = le_u64(raw, 40);
    let accessed_ns = le_u64(raw, 48);
    let changed_ns = le_u64(raw, 56);

    // Map the kernel entry type to a POSIX file-type bit.  Volume labels (2)
    // and any unknown value fall back to a regular file so callers branching
    // on type get a sane answer.
    let type_bits = match entry_type {
        1 => crate::fcntl::S_IFDIR,
        3 => crate::fcntl::S_IFLNK,
        _ => crate::fcntl::S_IFREG,
    };
    // Use the filesystem's real permission bits when present; otherwise
    // synthesize the conventional defaults for the file type.
    let perm_bits = if permissions != 0 {
        permissions & 0o7777
    } else {
        match entry_type {
            1 => 0o755,
            3 => 0o777,
            _ => 0o644,
        }
    };
    buf.st_mode = type_bits | perm_bits;
    buf.st_size = i64::try_from(size).unwrap_or(i64::MAX);
    buf.st_nlink = if nlinks == 0 { 1 } else { u64::from(nlinks) };
    buf.st_uid = uid;
    buf.st_gid = gid;
    buf.st_blksize = 4096;
    // Prefer the filesystem's reported 512-byte block count; fall back to a
    // size-derived estimate (rounded up) — matches `stat`/`du` conventions.
    let block_count = if blocks != 0 { blocks } else { size.div_ceil(512) };
    buf.st_blocks = i64::try_from(block_count).unwrap_or(i64::MAX);
    buf.st_atim = ns_to_timespec(accessed_ns);
    buf.st_mtim = ns_to_timespec(modified_ns);
    buf.st_ctim = ns_to_timespec(changed_ns);
}

// ---------------------------------------------------------------------------
// S_IS* macros as C-callable functions
// ---------------------------------------------------------------------------
//
// C programs use S_ISREG(m), S_ISDIR(m), etc. as macros that expand to
// bitwise tests on the mode.  These are typically preprocessor macros, but
// some build systems or languages need them as linkable symbols.

/// `S_ISREG(mode)` — test for regular file.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn S_ISREG(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFREG)
}

/// `S_ISDIR(mode)` — test for directory.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn S_ISDIR(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFDIR)
}

/// `S_ISLNK(mode)` — test for symbolic link.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn S_ISLNK(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFLNK)
}

/// `S_ISCHR(mode)` — test for character device.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn S_ISCHR(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFCHR)
}

/// `S_ISBLK(mode)` — test for block device.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn S_ISBLK(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFBLK)
}

/// `S_ISFIFO(mode)` — test for FIFO.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn S_ISFIFO(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFIFO)
}

/// `S_ISSOCK(mode)` — test for socket.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn S_ISSOCK(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFSOCK)
}

// ---------------------------------------------------------------------------
// mknod / mkfifo — create special files
// ---------------------------------------------------------------------------

/// Return `true` if the `S_IFMT` field of `mode` names a file type that
/// `mknod(2)` accepts.  Linux's `fs/namei.c::do_mknodat` rejects any
/// other value with `-EINVAL`, including `mode & S_IFMT == 0`.
///
/// Accepted types: regular (`S_IFREG`), character device (`S_IFCHR`),
/// block device (`S_IFBLK`), FIFO (`S_IFIFO`), socket (`S_IFSOCK`).
/// Directory and symlink are excluded because `mkdir`/`symlink` create
/// them.
#[must_use]
pub fn mknod_type_valid(mode: u32) -> bool {
    let t = mode & crate::fcntl::S_IFMT;
    t == crate::fcntl::S_IFREG
        || t == crate::fcntl::S_IFCHR
        || t == crate::fcntl::S_IFBLK
        || t == crate::fcntl::S_IFIFO
        || t == crate::fcntl::S_IFSOCK
}

/// Create a special or ordinary file.
///
/// Returns -1 with `ENOSYS` after argument-domain validation.  Our
/// filesystem doesn't support device nodes or special files yet, but
/// invalid callers must still see Linux-matching errno values so
/// portable code (udev, mdev, tmpfiles.d processors) reports failures
/// correctly.
///
/// Validation order matches `fs/namei.c::do_mknodat` and
/// `fs/namei.c::vfs_mknod` in Linux:
/// 1. `pathname == NULL` → `EFAULT`.
/// 2. `pathname` is the empty string → `ENOENT`.
/// 3. `mode & S_IFMT` is not a valid file type → `EINVAL`.
///    Plain `0` (no type bits) is rejected — Linux treats that as
///    "create a regular file" in the BSD legacy interface but
///    `do_mknodat` is strict.  Our stub follows the strict path.
/// 4. (Phase 188) `mode & S_IFMT` is `S_IFCHR` or `S_IFBLK` and the
///    caller lacks `CAP_MKNOD` → `EPERM`.  Matches Linux's
///    `vfs_mknod`: `if (S_ISCHR(mode) || S_ISBLK(mode)) { if
///    (!capable(CAP_MKNOD)) return -EPERM; }`.  FIFO, socket, and
///    regular-file types do not require the cap.
/// 5. All validated → `ENOSYS`.
///
/// Things we cannot validate yet:
/// - `EEXIST`: pathname already exists.
/// - `ENOTDIR`/`ENOENT`: a path component is wrong.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mknod(pathname: *const u8, mode: u32, _dev: u64) -> i32 {
    if pathname.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }
    // SAFETY: pathname non-NULL; read one byte to detect empty string.
    if unsafe { *pathname } == 0 {
        crate::errno::set_errno(crate::errno::ENOENT);
        return -1;
    }
    if !mknod_type_valid(mode) {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    // Phase 188: CAP_MKNOD gate fires only for character and block
    // device types — matches Linux's `vfs_mknod` placement.  FIFO,
    // socket, and regular-file creations bypass this check.
    let t = mode & crate::fcntl::S_IFMT;
    if (t == crate::fcntl::S_IFCHR || t == crate::fcntl::S_IFBLK)
        && !crate::sys_capability::has_capability(
            crate::sys_capability::CAP_MKNOD,
        )
    {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Create a special file relative to a directory fd.
///
/// Returns -1 with `ENOSYS` after argument-domain validation, matching
/// `mknod` for path/mode and adding directory-fd checks.
///
/// Validation order:
/// 1. `pathname == NULL` → `EFAULT`.
/// 2. `pathname` empty → `ENOENT`.
/// 3. `mode & S_IFMT` invalid → `EINVAL`.
/// 4. `dirfd != AT_FDCWD` and `dirfd < 0` → `EBADF`.
/// 5. `dirfd != AT_FDCWD` and not an open fd → `EBADF`.
/// 6. (Phase 188) `mode & S_IFMT` is `S_IFCHR` or `S_IFBLK` and the
///    caller lacks `CAP_MKNOD` → `EPERM`.  Linux's `vfs_mknod` runs
///    after path resolution, so the dirfd checks beat the cap check.
/// 7. All validated → `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mknodat(dirfd: i32, pathname: *const u8, mode: u32, _dev: u64) -> i32 {
    if pathname.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }
    // SAFETY: pathname non-NULL.
    if unsafe { *pathname } == 0 {
        crate::errno::set_errno(crate::errno::ENOENT);
        return -1;
    }
    if !mknod_type_valid(mode) {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    if dirfd != crate::file::AT_FDCWD {
        if dirfd < 0 {
            crate::errno::set_errno(crate::errno::EBADF);
            return -1;
        }
        if crate::fdtable::get_fd(dirfd).is_none() {
            crate::errno::set_errno(crate::errno::EBADF);
            return -1;
        }
    }
    // Phase 188: CAP_MKNOD gate fires only for CHR/BLK device types,
    // and only after path resolution / dirfd validation — matching
    // Linux's `vfs_mknod` placement deep inside `do_mknodat`.
    let t = mode & crate::fcntl::S_IFMT;
    if (t == crate::fcntl::S_IFCHR || t == crate::fcntl::S_IFBLK)
        && !crate::sys_capability::has_capability(
            crate::sys_capability::CAP_MKNOD,
        )
    {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Create a FIFO (named pipe).
///
/// Returns -1 with `ENOSYS` after argument-domain validation.  Named
/// pipes require kernel support for special file types in the
/// filesystem, which we don't have yet.
///
/// Validation order (matches `fs/namei.c::do_mkfifoat` in Linux):
/// 1. `pathname == NULL` → `EFAULT`.
/// 2. `pathname` empty → `ENOENT`.
/// 3. All validated → `ENOSYS`.  Linux does not validate `mode` bits
///    here — the type field is implicit (S_IFIFO) and the permission
///    bits are silently masked against the umask.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mkfifo(pathname: *const u8, _mode: u32) -> i32 {
    if pathname.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }
    // SAFETY: pathname non-NULL.
    if unsafe { *pathname } == 0 {
        crate::errno::set_errno(crate::errno::ENOENT);
        return -1;
    }
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Create a FIFO relative to a directory fd.
///
/// Returns -1 with `ENOSYS` after argument-domain validation, matching
/// `mkfifo` plus directory-fd checks.
///
/// Validation order:
/// 1. `pathname == NULL` → `EFAULT`.
/// 2. `pathname` empty → `ENOENT`.
/// 3. `dirfd != AT_FDCWD` and `dirfd < 0` → `EBADF`.
/// 4. `dirfd != AT_FDCWD` and not an open fd → `EBADF`.
/// 5. All validated → `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mkfifoat(dirfd: i32, pathname: *const u8, _mode: u32) -> i32 {
    if pathname.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }
    // SAFETY: pathname non-NULL.
    if unsafe { *pathname } == 0 {
        crate::errno::set_errno(crate::errno::ENOENT);
        return -1;
    }
    if dirfd != crate::file::AT_FDCWD {
        if dirfd < 0 {
            crate::errno::set_errno(crate::errno::EBADF);
            return -1;
        }
        if crate::fdtable::get_fd(dirfd).is_none() {
            crate::errno::set_errno(crate::errno::EBADF);
            return -1;
        }
    }
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcntl::*;

    // -- FsStatResult translation --

    /// Build a raw `FsStatResult` populating only the low-16-byte
    /// ABI-stable fields (size, type, link count); the rest are zero.
    fn raw_fsstat(size: u64, entry_type: u8, nlinks: u32) -> [u8; KERNEL_STAT_LEN] {
        let mut raw = [0u8; KERNEL_STAT_LEN];
        raw[0..8].copy_from_slice(&size.to_le_bytes());
        raw[8] = entry_type;
        raw[12..16].copy_from_slice(&nlinks.to_le_bytes());
        raw
    }

    /// Build a fully-populated raw `FsStatResult` exercising every field
    /// of the widened 72-byte layout.
    #[allow(clippy::too_many_arguments)]
    fn raw_fsstat_full(
        size: u64,
        entry_type: u8,
        nlinks: u32,
        perms: u32,
        uid: u32,
        gid: u32,
        blocks: u64,
        modified_ns: u64,
        accessed_ns: u64,
        changed_ns: u64,
        created_ns: u64,
    ) -> [u8; KERNEL_STAT_LEN] {
        let mut raw = [0u8; KERNEL_STAT_LEN];
        raw[0..8].copy_from_slice(&size.to_le_bytes());
        raw[8] = entry_type;
        raw[12..16].copy_from_slice(&nlinks.to_le_bytes());
        raw[16..20].copy_from_slice(&perms.to_le_bytes());
        raw[20..24].copy_from_slice(&uid.to_le_bytes());
        raw[24..28].copy_from_slice(&gid.to_le_bytes());
        raw[32..40].copy_from_slice(&blocks.to_le_bytes());
        raw[40..48].copy_from_slice(&modified_ns.to_le_bytes());
        raw[48..56].copy_from_slice(&accessed_ns.to_le_bytes());
        raw[56..64].copy_from_slice(&changed_ns.to_le_bytes());
        raw[64..72].copy_from_slice(&created_ns.to_le_bytes());
        raw
    }

    #[test]
    fn test_fill_from_fsstat_regular_file() {
        let raw = raw_fsstat(4096, 0, 1);
        let mut st = Stat::zeroed();
        fill_from_fsstat(&mut st, &raw);
        assert_eq!(st.st_mode, S_IFREG | 0o644);
        assert!(st.is_file());
        assert_eq!(st.st_size, 4096);
        assert_eq!(st.st_nlink, 1);
        assert_eq!(st.st_blksize, 4096);
        // 4096 / 512 = 8 blocks.
        assert_eq!(st.st_blocks, 8);
    }

    #[test]
    fn test_fill_from_fsstat_directory() {
        let raw = raw_fsstat(0, 1, 2);
        let mut st = Stat::zeroed();
        fill_from_fsstat(&mut st, &raw);
        assert_eq!(st.st_mode, S_IFDIR | 0o755);
        assert!(st.is_dir());
        assert_eq!(st.st_nlink, 2);
    }

    #[test]
    fn test_fill_from_fsstat_symlink() {
        let raw = raw_fsstat(12, 3, 1);
        let mut st = Stat::zeroed();
        fill_from_fsstat(&mut st, &raw);
        assert_eq!(st.st_mode, S_IFLNK | 0o777);
        assert!(st.is_link());
        assert_eq!(st.st_size, 12);
    }

    #[test]
    fn test_fill_from_fsstat_volume_label_is_regular() {
        // Entry type 2 (volume label) and unknown types fall back to a
        // regular file so type-branching callers behave sensibly.
        let raw = raw_fsstat(0, 2, 0);
        let mut st = Stat::zeroed();
        fill_from_fsstat(&mut st, &raw);
        assert!(st.is_file());
    }

    #[test]
    fn test_fill_from_fsstat_zero_nlinks_defaults_to_one() {
        // The plain stat path doesn't populate nlinks; treat 0 as 1.
        let raw = raw_fsstat(100, 0, 0);
        let mut st = Stat::zeroed();
        fill_from_fsstat(&mut st, &raw);
        assert_eq!(st.st_nlink, 1);
    }

    #[test]
    fn test_fill_from_fsstat_block_count_rounds_up() {
        // 100 bytes still occupies one 512-byte block.
        let raw = raw_fsstat(100, 0, 1);
        let mut st = Stat::zeroed();
        fill_from_fsstat(&mut st, &raw);
        assert_eq!(st.st_blocks, 1);
        // 513 bytes spills into a second block.
        let raw = raw_fsstat(513, 0, 1);
        fill_from_fsstat(&mut st, &raw);
        assert_eq!(st.st_blocks, 2);
    }

    #[test]
    fn test_fill_from_fsstat_resets_stale_fields() {
        // A buffer reused across calls must be fully overwritten.
        let mut st = Stat::zeroed();
        st.st_size = 9999;
        st.st_mode = S_IFSOCK;
        let raw = raw_fsstat(8, 0, 1);
        fill_from_fsstat(&mut st, &raw);
        assert_eq!(st.st_size, 8);
        assert!(st.is_file());
    }

    #[test]
    fn test_fill_from_fsstat_real_permissions_used() {
        // When the filesystem reports real permission bits, they take
        // precedence over the synthesized type defaults.
        let raw = raw_fsstat_full(
            100, 0, 1, 0o600, 0, 0, 0, 0, 0, 0, 0,
        );
        let mut st = Stat::zeroed();
        fill_from_fsstat(&mut st, &raw);
        assert_eq!(st.st_mode, S_IFREG | 0o600);
    }

    #[test]
    fn test_fill_from_fsstat_setuid_bits_preserved() {
        // Permission field carries the full 12 low bits (setuid/setgid/sticky).
        let raw = raw_fsstat_full(
            0, 0, 1, 0o4755, 0, 0, 0, 0, 0, 0, 0,
        );
        let mut st = Stat::zeroed();
        fill_from_fsstat(&mut st, &raw);
        assert_eq!(st.st_mode, S_IFREG | 0o4755);
    }

    #[test]
    fn test_fill_from_fsstat_ownership() {
        let raw = raw_fsstat_full(
            0, 0, 1, 0, 1000, 1001, 0, 0, 0, 0, 0,
        );
        let mut st = Stat::zeroed();
        fill_from_fsstat(&mut st, &raw);
        assert_eq!(st.st_uid, 1000);
        assert_eq!(st.st_gid, 1001);
    }

    #[test]
    fn test_fill_from_fsstat_explicit_block_count() {
        // A non-zero block count from the filesystem is used verbatim,
        // not re-derived from size.
        let raw = raw_fsstat_full(
            100, 0, 1, 0, 0, 0, 16, 0, 0, 0, 0,
        );
        let mut st = Stat::zeroed();
        fill_from_fsstat(&mut st, &raw);
        assert_eq!(st.st_blocks, 16);
    }

    #[test]
    fn test_fill_from_fsstat_timestamps() {
        // ns since epoch split into sec/nsec across a/m/c times.
        let modified = 1_500_000_000_123_456_789u64;
        let accessed = 1_400_000_000_000_000_001u64;
        let changed = 1_600_000_000_999_999_999u64;
        let raw = raw_fsstat_full(
            0, 0, 1, 0, 0, 0, 0, modified, accessed, changed, 42,
        );
        let mut st = Stat::zeroed();
        fill_from_fsstat(&mut st, &raw);
        assert_eq!(st.st_mtim.tv_sec, 1_500_000_000);
        assert_eq!(st.st_mtim.tv_nsec, 123_456_789);
        assert_eq!(st.st_atim.tv_sec, 1_400_000_000);
        assert_eq!(st.st_atim.tv_nsec, 1);
        assert_eq!(st.st_ctim.tv_sec, 1_600_000_000);
        assert_eq!(st.st_ctim.tv_nsec, 999_999_999);
    }

    #[test]
    fn test_fill_from_fsstat_zero_timestamps_are_epoch() {
        // 0 ns ("not available") yields a zeroed Timespec (epoch).
        let raw = raw_fsstat(0, 0, 1);
        let mut st = Stat::zeroed();
        fill_from_fsstat(&mut st, &raw);
        assert_eq!(st.st_mtim.tv_sec, 0);
        assert_eq!(st.st_mtim.tv_nsec, 0);
        assert_eq!(st.st_atim.tv_sec, 0);
        assert_eq!(st.st_ctim.tv_sec, 0);
    }

    // -- S_IS* C-callable functions --

    #[test]
    fn test_s_isreg() {
        assert_eq!(S_ISREG(S_IFREG | 0o644), 1);
        assert_eq!(S_ISREG(S_IFDIR | 0o755), 0);
        assert_eq!(S_ISREG(0), 0);
    }

    #[test]
    fn test_s_isdir() {
        assert_eq!(S_ISDIR(S_IFDIR | 0o755), 1);
        assert_eq!(S_ISDIR(S_IFREG | 0o644), 0);
    }

    #[test]
    fn test_s_islnk() {
        assert_eq!(S_ISLNK(S_IFLNK | 0o777), 1);
        assert_eq!(S_ISLNK(S_IFREG | 0o644), 0);
    }

    #[test]
    fn test_s_ischr() {
        assert_eq!(S_ISCHR(S_IFCHR | 0o666), 1);
        assert_eq!(S_ISCHR(S_IFBLK | 0o660), 0);
    }

    #[test]
    fn test_s_isblk() {
        assert_eq!(S_ISBLK(S_IFBLK | 0o660), 1);
        assert_eq!(S_ISBLK(S_IFCHR | 0o666), 0);
    }

    #[test]
    fn test_s_isfifo() {
        assert_eq!(S_ISFIFO(S_IFIFO | 0o644), 1);
        assert_eq!(S_ISFIFO(S_IFREG | 0o644), 0);
    }

    #[test]
    fn test_s_issock() {
        assert_eq!(S_ISSOCK(S_IFSOCK | 0o755), 1);
        assert_eq!(S_ISSOCK(S_IFREG | 0o644), 0);
    }

    // -- Stat struct methods --

    #[test]
    fn test_stat_is_file() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFREG | 0o644;
        assert!(st.is_file());
        assert!(!st.is_dir());
        assert!(!st.is_link());
    }

    #[test]
    fn test_stat_is_dir() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFDIR | 0o755;
        assert!(st.is_dir());
        assert!(!st.is_file());
    }

    #[test]
    fn test_stat_is_link() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFLNK | 0o777;
        assert!(st.is_link());
        assert!(!st.is_file());
        assert!(!st.is_dir());
    }

    #[test]
    fn test_stat_is_chr() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFCHR | 0o666;
        assert!(st.is_chr());
    }

    #[test]
    fn test_stat_is_blk() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFBLK | 0o660;
        assert!(st.is_blk());
    }

    #[test]
    fn test_stat_is_fifo() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFIFO | 0o644;
        assert!(st.is_fifo());
    }

    #[test]
    fn test_stat_is_sock() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFSOCK;
        assert!(st.is_sock());
    }

    // -- Stat struct layout --

    #[test]
    fn test_stat_size() {
        // Linux x86_64 struct stat is 144 bytes.
        assert_eq!(core::mem::size_of::<Stat>(), 144);
    }

    #[test]
    fn test_stat_zeroed() {
        let st = Stat::zeroed();
        assert_eq!(st.st_dev, 0);
        assert_eq!(st.st_ino, 0);
        assert_eq!(st.st_mode, 0);
        assert_eq!(st.st_size, 0);
    }

    // -- Timespec layout --

    #[test]
    fn test_timespec_size() {
        assert_eq!(core::mem::size_of::<Timespec>(), 16);
    }

    #[test]
    fn test_timespec_default() {
        let ts = Timespec::default();
        assert_eq!(ts.tv_sec, 0);
        assert_eq!(ts.tv_nsec, 0);
    }

    // -- File mode constants match Linux --

    #[test]
    fn test_mode_constants() {
        assert_eq!(S_IFMT, 0o170_000);
        assert_eq!(S_IFREG, 0o100_000);
        assert_eq!(S_IFDIR, 0o040_000);
        assert_eq!(S_IFLNK, 0o120_000);
        assert_eq!(S_IFCHR, 0o020_000);
        assert_eq!(S_IFBLK, 0o060_000);
        assert_eq!(S_IFIFO, 0o010_000);
        assert_eq!(S_IFSOCK, 0o140_000);
    }

    #[test]
    fn test_permission_constants() {
        assert_eq!(S_IRUSR, 0o400);
        assert_eq!(S_IWUSR, 0o200);
        assert_eq!(S_IXUSR, 0o100);
        assert_eq!(S_IRGRP, 0o040);
        assert_eq!(S_IWGRP, 0o020);
        assert_eq!(S_IXGRP, 0o010);
        assert_eq!(S_IROTH, 0o004);
        assert_eq!(S_IWOTH, 0o002);
        assert_eq!(S_IXOTH, 0o001);
    }

    #[test]
    fn test_special_bits() {
        assert_eq!(S_ISUID, 0o4000);
        assert_eq!(S_ISGID, 0o2000);
        assert_eq!(S_ISVTX, 0o1000);
    }

    // -- All types are disjoint --

    #[test]
    fn test_file_types_disjoint() {
        let types = [S_IFREG, S_IFDIR, S_IFLNK, S_IFCHR, S_IFBLK, S_IFIFO, S_IFSOCK];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(
                    types[i], types[j],
                    "file types must be disjoint"
                );
            }
        }
    }

    // -- mknod/mkfifo stubs return ENOSYS --

    #[test]
    fn test_mknod_returns_enosys() {
        assert_eq!(mknod(b"/dev/null\0".as_ptr(), S_IFCHR | 0o666, 0), -1);
    }

    #[test]
    fn test_mknodat_returns_enosys() {
        // Phase 66: mknodat now rejects dirfd<0 (other than AT_FDCWD) with
        // EBADF before reaching ENOSYS.  Use AT_FDCWD so the call resolves
        // to the cwd path and reaches the ENOSYS sentinel.
        assert_eq!(
            mknodat(crate::file::AT_FDCWD, b"node\0".as_ptr(), S_IFCHR | 0o666, 0),
            -1,
        );
    }

    #[test]
    fn test_mkfifo_returns_enosys() {
        assert_eq!(mkfifo(b"/tmp/fifo\0".as_ptr(), 0o644), -1);
    }

    #[test]
    fn test_mkfifoat_returns_enosys() {
        // Phase 66: mkfifoat now rejects dirfd<0 (other than AT_FDCWD) with
        // EBADF.  Use AT_FDCWD so the call reaches ENOSYS.
        assert_eq!(mkfifoat(crate::file::AT_FDCWD, b"fifo\0".as_ptr(), 0o644), -1);
    }

    // -- S_IS* functions edge cases --

    #[test]
    fn test_s_isreg_with_permissions() {
        // Regular file with setuid bit — still a regular file.
        assert_eq!(S_ISREG(S_IFREG | S_ISUID | 0o755), 1);
    }

    #[test]
    fn test_s_isdir_with_sticky() {
        // Directory with sticky bit — still a directory.
        assert_eq!(S_ISDIR(S_IFDIR | S_ISVTX | 0o755), 1);
    }

    #[test]
    fn test_stat_methods_consistent_with_c_functions() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFREG | 0o644;
        assert_eq!(S_ISREG(st.st_mode) != 0, st.is_file());
        assert_eq!(S_ISDIR(st.st_mode) != 0, st.is_dir());
        assert_eq!(S_ISLNK(st.st_mode) != 0, st.is_link());
    }

    // -- S_IS* with mode=0 (no type set) --

    #[test]
    fn test_s_is_functions_mode_zero() {
        assert_eq!(S_ISREG(0), 0);
        assert_eq!(S_ISDIR(0), 0);
        assert_eq!(S_ISLNK(0), 0);
        assert_eq!(S_ISCHR(0), 0);
        assert_eq!(S_ISBLK(0), 0);
        assert_eq!(S_ISFIFO(0), 0);
        assert_eq!(S_ISSOCK(0), 0);
    }

    // -- S_IS* with only permission bits (no type) --

    #[test]
    fn test_s_isreg_only_perms() {
        assert_eq!(S_ISREG(0o777), 0);
    }

    // -- Stat::is_* all false for mode=0 --

    #[test]
    fn test_stat_is_methods_mode_zero() {
        let st = Stat::zeroed();
        assert!(!st.is_file());
        assert!(!st.is_dir());
        assert!(!st.is_link());
        assert!(!st.is_chr());
        assert!(!st.is_blk());
        assert!(!st.is_fifo());
        assert!(!st.is_sock());
    }

    // -- Each type is exclusively one type --

    #[test]
    fn test_s_is_exclusive_reg() {
        let mode = S_IFREG | 0o644;
        assert_eq!(S_ISREG(mode), 1);
        assert_eq!(S_ISDIR(mode), 0);
        assert_eq!(S_ISLNK(mode), 0);
        assert_eq!(S_ISCHR(mode), 0);
        assert_eq!(S_ISBLK(mode), 0);
        assert_eq!(S_ISFIFO(mode), 0);
        assert_eq!(S_ISSOCK(mode), 0);
    }

    #[test]
    fn test_s_is_exclusive_dir() {
        let mode = S_IFDIR | 0o755;
        assert_eq!(S_ISDIR(mode), 1);
        assert_eq!(S_ISREG(mode), 0);
        assert_eq!(S_ISLNK(mode), 0);
        assert_eq!(S_ISCHR(mode), 0);
    }

    #[test]
    fn test_s_is_exclusive_lnk() {
        let mode = S_IFLNK | 0o777;
        assert_eq!(S_ISLNK(mode), 1);
        assert_eq!(S_ISREG(mode), 0);
        assert_eq!(S_ISDIR(mode), 0);
    }

    #[test]
    fn test_s_is_exclusive_chr() {
        let mode = S_IFCHR | 0o666;
        assert_eq!(S_ISCHR(mode), 1);
        assert_eq!(S_ISREG(mode), 0);
        assert_eq!(S_ISBLK(mode), 0);
    }

    #[test]
    fn test_s_is_exclusive_sock() {
        let mode = S_IFSOCK | 0o755;
        assert_eq!(S_ISSOCK(mode), 1);
        assert_eq!(S_ISREG(mode), 0);
        assert_eq!(S_ISFIFO(mode), 0);
    }

    // -- S_IFMT mask extracts type correctly --

    #[test]
    fn test_s_ifmt_extraction() {
        let mode = S_IFREG | S_ISUID | S_ISGID | S_ISVTX | 0o777;
        assert_eq!(mode & S_IFMT, S_IFREG);
    }

    #[test]
    fn test_s_ifmt_strips_permissions() {
        let mode = S_IFDIR | 0o777;
        assert_eq!(mode & S_IFMT, S_IFDIR);
        assert_eq!(mode & !S_IFMT, 0o777);
    }

    // -- Timespec --

    #[test]
    fn test_timespec_alignment() {
        assert_eq!(core::mem::align_of::<Timespec>(), 8);
    }

    #[test]
    fn test_timespec_field_values() {
        let ts = Timespec { tv_sec: 1000, tv_nsec: 500_000_000 };
        assert_eq!(ts.tv_sec, 1000);
        assert_eq!(ts.tv_nsec, 500_000_000);
    }

    // -- Stat struct alignment and field offsets --

    #[test]
    fn test_stat_alignment() {
        assert_eq!(core::mem::align_of::<Stat>(), 8);
    }

    #[test]
    fn test_stat_field_values() {
        let mut st = Stat::zeroed();
        st.st_ino = 12345;
        st.st_mode = S_IFREG | 0o644;
        st.st_nlink = 1;
        st.st_uid = 1000;
        st.st_gid = 1000;
        st.st_size = 4096;
        assert_eq!(st.st_ino, 12345);
        assert_eq!(st.st_nlink, 1);
        assert_eq!(st.st_uid, 1000);
        assert_eq!(st.st_gid, 1000);
        assert_eq!(st.st_size, 4096);
    }

    // -- Permission bit combining --

    #[test]
    fn test_permission_bits_compose() {
        // Owner rwx = 0o700
        assert_eq!(S_IRUSR | S_IWUSR | S_IXUSR, 0o700);
        // Group rwx = 0o070
        assert_eq!(S_IRGRP | S_IWGRP | S_IXGRP, 0o070);
        // Other rwx = 0o007
        assert_eq!(S_IROTH | S_IWOTH | S_IXOTH, 0o007);
        // All rwx = 0o777
        assert_eq!(
            S_IRUSR | S_IWUSR | S_IXUSR |
            S_IRGRP | S_IWGRP | S_IXGRP |
            S_IROTH | S_IWOTH | S_IXOTH,
            0o777,
        );
    }

    // -- mknod/mkfifo set errno --

    #[test]
    fn test_mknod_sets_enosys() {
        // Phase 66: mode=0 (no type bits) is now rejected with EINVAL
        // before reaching ENOSYS.  Use a valid type to reach the sentinel.
        crate::errno::set_errno(0);
        mknod(b"/tmp/n\0".as_ptr(), S_IFREG | 0o644, 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mkfifo_sets_enosys() {
        crate::errno::set_errno(0);
        mkfifo(b"/tmp/f\0".as_ptr(), 0o644);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mknodat_sets_enosys() {
        // Phase 66: mode=0 → EINVAL, dirfd=0 (not AT_FDCWD, not open) → EBADF.
        // Use S_IFREG type and AT_FDCWD to reach ENOSYS.
        crate::errno::set_errno(0);
        mknodat(crate::file::AT_FDCWD, b"n\0".as_ptr(), S_IFREG | 0o644, 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mkfifoat_sets_enosys() {
        // Phase 66: dirfd=0 (not AT_FDCWD, not open) → EBADF.  Use AT_FDCWD.
        crate::errno::set_errno(0);
        mkfifoat(crate::file::AT_FDCWD, b"f\0".as_ptr(), 0o644);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -- Null pointer args don't crash --

    #[test]
    fn test_mknod_null_path() {
        assert_eq!(mknod(core::ptr::null(), 0, 0), -1);
    }

    #[test]
    fn test_mkfifo_null_path() {
        assert_eq!(mkfifo(core::ptr::null(), 0), -1);
    }

    // -----------------------------------------------------------------
    // Phase 66 — mknod / mknodat / mkfifo / mkfifoat full validators
    // -----------------------------------------------------------------

    // --- mknod_type_valid helper ---

    #[test]
    fn test_mknod_type_valid_accepts_reg() {
        assert!(mknod_type_valid(S_IFREG));
        assert!(mknod_type_valid(S_IFREG | 0o644));
    }

    #[test]
    fn test_mknod_type_valid_accepts_chr() {
        assert!(mknod_type_valid(S_IFCHR));
        assert!(mknod_type_valid(S_IFCHR | 0o666));
    }

    #[test]
    fn test_mknod_type_valid_accepts_blk() {
        assert!(mknod_type_valid(S_IFBLK));
        assert!(mknod_type_valid(S_IFBLK | 0o660));
    }

    #[test]
    fn test_mknod_type_valid_accepts_fifo() {
        assert!(mknod_type_valid(S_IFIFO));
        assert!(mknod_type_valid(S_IFIFO | 0o644));
    }

    #[test]
    fn test_mknod_type_valid_accepts_sock() {
        assert!(mknod_type_valid(S_IFSOCK));
        assert!(mknod_type_valid(S_IFSOCK | 0o755));
    }

    #[test]
    fn test_mknod_type_valid_rejects_dir() {
        // Directories are created via mkdir(2), not mknod.
        assert!(!mknod_type_valid(S_IFDIR | 0o755));
    }

    #[test]
    fn test_mknod_type_valid_rejects_symlink() {
        // Symlinks are created via symlink(2), not mknod.
        assert!(!mknod_type_valid(S_IFLNK | 0o777));
    }

    #[test]
    fn test_mknod_type_valid_rejects_zero() {
        // mode=0 has no type bits — Linux's do_mknodat rejects this.
        assert!(!mknod_type_valid(0));
        assert!(!mknod_type_valid(0o644));
    }

    // --- mknod: per-error-class ---

    #[test]
    fn test_mknod_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(mknod(core::ptr::null(), S_IFREG | 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mknod_empty_enoent() {
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"\0".as_ptr(), S_IFREG | 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_mknod_bad_type_einval() {
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"/tmp/x\0".as_ptr(), S_IFDIR | 0o755, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mknod_no_type_bits_einval() {
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"/tmp/x\0".as_ptr(), 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mknod_symlink_type_einval() {
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"/tmp/x\0".as_ptr(), S_IFLNK | 0o777, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mknod_valid_reaches_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"/tmp/x\0".as_ptr(), S_IFCHR | 0o666, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- mknod: ordering ---

    #[test]
    fn test_mknod_null_beats_bad_type() {
        // NULL path checked before mode validation.
        crate::errno::set_errno(0);
        assert_eq!(mknod(core::ptr::null(), S_IFDIR | 0o755, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mknod_empty_beats_bad_type() {
        // Empty path checked before mode validation.
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"\0".as_ptr(), S_IFDIR | 0o755, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    // --- mknodat: per-error-class ---

    #[test]
    fn test_mknodat_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(
            mknodat(crate::file::AT_FDCWD, core::ptr::null(), S_IFREG | 0o644, 0),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mknodat_empty_enoent() {
        crate::errno::set_errno(0);
        assert_eq!(
            mknodat(crate::file::AT_FDCWD, b"\0".as_ptr(), S_IFREG | 0o644, 0),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_mknodat_bad_type_einval() {
        crate::errno::set_errno(0);
        assert_eq!(
            mknodat(crate::file::AT_FDCWD, b"n\0".as_ptr(), S_IFDIR | 0o755, 0),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mknodat_negative_dirfd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(mknodat(-1, b"n\0".as_ptr(), S_IFREG | 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_mknodat_nonexistent_fd_ebadf() {
        crate::errno::set_errno(0);
        // fd 9999 is overwhelmingly unlikely to be open.
        assert_eq!(mknodat(9999, b"n\0".as_ptr(), S_IFREG | 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_mknodat_at_fdcwd_reaches_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(
            mknodat(crate::file::AT_FDCWD, b"n\0".as_ptr(), S_IFCHR | 0o666, 0),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- mknodat: ordering ---

    #[test]
    fn test_mknodat_null_beats_bad_type() {
        // NULL path is checked first, before mode and dirfd.
        crate::errno::set_errno(0);
        assert_eq!(mknodat(-1, core::ptr::null(), S_IFDIR | 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mknodat_empty_beats_bad_type() {
        crate::errno::set_errno(0);
        assert_eq!(mknodat(-1, b"\0".as_ptr(), S_IFDIR | 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_mknodat_bad_type_beats_bad_dirfd() {
        // Mode validation comes before dirfd validation.
        crate::errno::set_errno(0);
        assert_eq!(mknodat(-1, b"n\0".as_ptr(), S_IFDIR | 0o755, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- mkfifo: per-error-class ---

    #[test]
    fn test_mkfifo_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifo(core::ptr::null(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mkfifo_empty_enoent() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifo(b"\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_mkfifo_valid_reaches_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifo(b"/tmp/fifo\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mkfifo_any_mode_ok() {
        // Linux does not validate mode bits — only the type field matters
        // (implicit S_IFIFO) and even garbage mode bits are accepted at
        // this layer (perms are masked by umask later).
        crate::errno::set_errno(0);
        assert_eq!(mkfifo(b"/tmp/fifo\0".as_ptr(), 0xFFFF_FFFF), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- mkfifo: ordering ---

    #[test]
    fn test_mkfifo_null_beats_empty() {
        // Trivially: NULL path is checked before the empty-string check
        // because dereferencing a NULL would crash.
        crate::errno::set_errno(0);
        assert_eq!(mkfifo(core::ptr::null(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // --- mkfifoat: per-error-class ---

    #[test]
    fn test_mkfifoat_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(
            mkfifoat(crate::file::AT_FDCWD, core::ptr::null(), 0o644),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mkfifoat_empty_enoent() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifoat(crate::file::AT_FDCWD, b"\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_mkfifoat_negative_dirfd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifoat(-1, b"f\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_mkfifoat_nonexistent_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifoat(9999, b"f\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_mkfifoat_at_fdcwd_reaches_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifoat(crate::file::AT_FDCWD, b"f\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- mkfifoat: ordering ---

    #[test]
    fn test_mkfifoat_null_beats_bad_dirfd() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifoat(-1, core::ptr::null(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mkfifoat_empty_beats_bad_dirfd() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifoat(-1, b"\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    // --- Real-world workflows ---

    #[test]
    fn test_workflow_udev_creates_dev_null() {
        // udev creating /dev/null: major=1, minor=3, mode=S_IFCHR | 0o666.
        // Linux makes this dev with makedev(1,3) → ((1u64) << 8) | 3.
        crate::errno::set_errno(0);
        let dev = (1u64 << 8) | 3;
        assert_eq!(
            mknod(b"/dev/null\0".as_ptr(), S_IFCHR | 0o666, dev),
            -1,
        );
        // Properly-formed call reaches ENOSYS (we don't implement nodes).
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_workflow_systemd_tmpfiles_creates_pipe() {
        // tmpfiles.d entry like:  p /run/initctl 0644 root root - -
        // would be implemented as mkfifo("/run/initctl", 0644).
        crate::errno::set_errno(0);
        assert_eq!(mkfifo(b"/run/initctl\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_workflow_mdev_block_device() {
        // mdev creates block device for /dev/sda: S_IFBLK | 0o660.
        crate::errno::set_errno(0);
        assert_eq!(
            mknod(b"/dev/sda\0".as_ptr(), S_IFBLK | 0o660, (8u64 << 8) | 0),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_workflow_mkfifoat_relative_to_dir() {
        // A daemon mkfifo()s relative to its working dir.  We use
        // AT_FDCWD since we can't open arbitrary directories in tests.
        crate::errno::set_errno(0);
        assert_eq!(
            mkfifoat(crate::file::AT_FDCWD, b"control\0".as_ptr(), 0o600),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- Real-world buggy callers ---

    #[test]
    fn test_buggy_mknod_perms_only() {
        // Common bug: passing 0o644 to mknod expecting it to create a
        // regular file.  POSIX permits this (mode==0 → regular file in
        // some implementations) but Linux's do_mknodat is strict and
        // returns EINVAL.  We match Linux.
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"/tmp/x\0".as_ptr(), 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_buggy_mknod_directory_type() {
        // Buggy caller tries to create a directory via mknod.  Must use
        // mkdir(2).  Linux rejects with EINVAL.
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"/tmp/d\0".as_ptr(), S_IFDIR | 0o755, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_buggy_mknod_symlink_type() {
        // Buggy caller tries to create a symlink via mknod.  Must use
        // symlink(2).  Linux rejects with EINVAL.
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"/tmp/l\0".as_ptr(), S_IFLNK | 0o777, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_buggy_mknodat_unopened_dirfd() {
        // Caller uses an fd that was never opened (or was closed).
        // Should get EBADF, not silently succeed.
        crate::errno::set_errno(0);
        assert_eq!(mknodat(12345, b"n\0".as_ptr(), S_IFREG | 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_buggy_mkfifoat_unopened_dirfd() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifoat(12345, b"f\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_buggy_mknod_no_pathname() {
        // Caller passes NULL pathname after a failed string construction.
        crate::errno::set_errno(0);
        assert_eq!(mknod(core::ptr::null(), S_IFREG | 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // ----------------------------------------------------------------------
    // Phase 188: mknod / mknodat — CAP_MKNOD gate for CHR / BLK
    // ----------------------------------------------------------------------
    //
    // Linux's `fs/namei.c::vfs_mknod` opens with:
    //
    //     if (S_ISCHR(mode) || S_ISBLK(mode)) {
    //         if (!capable(CAP_MKNOD))
    //             return -EPERM;
    //     }
    //
    // The check runs *after* `do_mknodat` has resolved the path and
    // validated the file-type bits — so EFAULT (NULL path), ENOENT
    // (empty), EINVAL (bad type), and EBADF (bad dirfd) all beat the
    // EPERM.
    //
    // FIFO, socket, and regular-file creations bypass the cap check
    // entirely.  Mode bits outside the type field (e.g. perms) are
    // irrelevant — only `mode & S_IFMT` is examined.
    //
    // Host test build holds CAP_MKNOD by default (bit 27 ∈
    // DEFAULT_CAPS_LOW = u32::MAX), so all pre-existing mknod /
    // mknodat / mkfifo / mkfifoat tests continue to hit the same
    // ENOSYS / EINVAL / EBADF terminals.
    //
    // Must run with `--test-threads=1` because the tests manipulate
    // process-wide capability state.
    // ----------------------------------------------------------------------

    mod mknod_cap_phase188 {
        use super::*;

        /// Snapshot/restore-on-drop guard — same pattern as Phase
        /// 187 (`setgroups_cap_phase187`).
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_mknod() {
            use crate::sys_capability::CAP_MKNOD;
            let (lo, hi) =
                crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_MKNOD < 32 {
                (lo & !(1u32 << CAP_MKNOD), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_MKNOD - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0,
                "capset must succeed when dropping CAP_MKNOD");
            assert!(!crate::sys_capability::has_capability(CAP_MKNOD));
        }

        // -- Per-error-class --------------------------------------------------

        /// CHR device without CAP_MKNOD → EPERM.  Matches `vfs_mknod`
        /// when `S_ISCHR(mode) && !capable(CAP_MKNOD)`.
        #[test]
        fn test_mknod_phase188_chr_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_mknod();
            crate::errno::set_errno(0);
            assert_eq!(
                mknod(b"/dev/zero\0".as_ptr(), S_IFCHR | 0o666, 0),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
        }

        /// BLK device without CAP_MKNOD → EPERM.
        #[test]
        fn test_mknod_phase188_blk_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_mknod();
            crate::errno::set_errno(0);
            assert_eq!(
                mknod(b"/dev/loop0\0".as_ptr(), S_IFBLK | 0o660, 0),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
        }

        /// FIFO without CAP_MKNOD → still reaches ENOSYS.  Linux does
        /// not require CAP_MKNOD for named pipes — the cap gate is
        /// type-conditional.
        #[test]
        fn test_mknod_phase188_fifo_no_cap_reaches_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_mknod();
            crate::errno::set_errno(0);
            assert_eq!(
                mknod(b"/run/fifo\0".as_ptr(), S_IFIFO | 0o600, 0),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS,
                "FIFO must not require CAP_MKNOD");
        }

        /// Socket without CAP_MKNOD → still reaches ENOSYS.
        #[test]
        fn test_mknod_phase188_sock_no_cap_reaches_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_mknod();
            crate::errno::set_errno(0);
            assert_eq!(
                mknod(b"/run/s\0".as_ptr(), S_IFSOCK | 0o600, 0),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS,
                "Socket must not require CAP_MKNOD");
        }

        /// Regular file without CAP_MKNOD → still reaches ENOSYS.
        #[test]
        fn test_mknod_phase188_reg_no_cap_reaches_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_mknod();
            crate::errno::set_errno(0);
            assert_eq!(
                mknod(b"/tmp/r\0".as_ptr(), S_IFREG | 0o644, 0),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS,
                "Regular file must not require CAP_MKNOD");
        }

        // -- Ordering matrix --------------------------------------------------

        /// EFAULT (NULL path) beats EPERM — the path-validation step
        /// runs before any cap check in Linux's `do_mknodat`.
        #[test]
        fn test_mknod_phase188_efault_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_mknod();
            crate::errno::set_errno(0);
            assert_eq!(mknod(core::ptr::null(), S_IFCHR | 0o666, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        }

        /// ENOENT (empty path) beats EPERM.
        #[test]
        fn test_mknod_phase188_enoent_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_mknod();
            crate::errno::set_errno(0);
            assert_eq!(mknod(b"\0".as_ptr(), S_IFBLK | 0o660, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
        }

        /// EINVAL (bad type, e.g. S_IFDIR) beats EPERM.  Type
        /// validation runs before `vfs_mknod` is even called.
        #[test]
        fn test_mknod_phase188_einval_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_mknod();
            crate::errno::set_errno(0);
            assert_eq!(
                mknod(b"/tmp/d\0".as_ptr(), S_IFDIR | 0o755, 0),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL,
                "Bad-type EINVAL must beat CAP_MKNOD EPERM");
        }

        /// EPERM beats ENOSYS — the cap gate is the last validation
        /// before the stub returns ENOSYS, so missing-cap callers
        /// never see ENOSYS for CHR/BLK.
        #[test]
        fn test_mknod_phase188_eperm_beats_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_mknod();
            crate::errno::set_errno(0);
            assert_eq!(
                mknod(b"/dev/tty\0".as_ptr(), S_IFCHR | 0o620, 0),
                -1,
            );
            // Without the gate this would return ENOSYS.
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM,
                "Missing CAP_MKNOD must surface as EPERM, not ENOSYS");
        }

        // -- mknodat ordering --------------------------------------------------

        /// mknodat: EBADF (bad dirfd) beats EPERM.  Linux runs the
        /// fdget before path resolution; vfs_mknod is deeper still.
        #[test]
        fn test_mknodat_phase188_ebadf_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_mknod();
            crate::errno::set_errno(0);
            assert_eq!(
                mknodat(-1, b"d\0".as_ptr(), S_IFCHR | 0o666, 0),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::EBADF,
                "Bad dirfd EBADF must beat CAP_MKNOD EPERM");
        }

        /// mknodat with AT_FDCWD: CHR without cap → EPERM.
        #[test]
        fn test_mknodat_phase188_at_fdcwd_chr_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_mknod();
            crate::errno::set_errno(0);
            assert_eq!(
                mknodat(
                    crate::file::AT_FDCWD,
                    b"dev\0".as_ptr(),
                    S_IFCHR | 0o666,
                    0,
                ),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
        }

        /// mknodat with AT_FDCWD: FIFO bypasses cap → ENOSYS.
        #[test]
        fn test_mknodat_phase188_at_fdcwd_fifo_bypasses_cap() {
            let _g = CapGuard::snapshot();
            drop_cap_mknod();
            crate::errno::set_errno(0);
            assert_eq!(
                mknodat(
                    crate::file::AT_FDCWD,
                    b"f\0".as_ptr(),
                    S_IFIFO | 0o600,
                    0,
                ),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
        }

        // -- Workflow --------------------------------------------------------

        /// Privileged → unprivileged → privileged round-trip.
        /// Mirrors a setuid helper that drops CAP_MKNOD after device
        /// setup and a re-execed root daemon that gets it back.
        #[test]
        fn test_mknod_phase188_drop_then_restore_workflow() {
            let _g = CapGuard::snapshot();
            // 1. Cap held — CHR reaches ENOSYS (proper request, stub
            //    can't materialize it).
            crate::errno::set_errno(0);
            assert_eq!(
                mknod(b"/dev/null\0".as_ptr(), S_IFCHR | 0o666, 0),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
            // 2. Drop cap — CHR fails with EPERM.
            drop_cap_mknod();
            crate::errno::set_errno(0);
            assert_eq!(
                mknod(b"/dev/null\0".as_ptr(), S_IFCHR | 0o666, 0),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
            // 3. Restore cap (via capset to u32::MAX) — CHR reaches
            //    ENOSYS again.
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(
                crate::sys_capability::capset(&mut hdr, data.as_ptr()),
                0,
            );
            crate::errno::set_errno(0);
            assert_eq!(
                mknod(b"/dev/null\0".as_ptr(), S_IFCHR | 0o666, 0),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
        }

        // -- Buggy-caller ----------------------------------------------------

        /// A caller that didn't clear errno before mknod sees a fresh
        /// EPERM, not the stale value.
        #[test]
        fn test_mknod_phase188_buggy_caller_stale_errno_replaced() {
            let _g = CapGuard::snapshot();
            drop_cap_mknod();
            crate::errno::set_errno(crate::errno::ENOENT);
            assert_eq!(
                mknod(b"/dev/sda\0".as_ptr(), S_IFBLK | 0o660, 0),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM,
                "Stale ENOENT must be overwritten with EPERM");
        }

        // -- Recovery --------------------------------------------------------

        /// CapGuard drop restores the cap so a subsequent CHR call in
        /// the same test process reaches ENOSYS again.
        #[test]
        fn test_mknod_phase188_capguard_restore_clears_state() {
            {
                let _g = CapGuard::snapshot();
                drop_cap_mknod();
                crate::errno::set_errno(0);
                assert_eq!(
                    mknod(b"/dev/x\0".as_ptr(), S_IFCHR | 0o666, 0),
                    -1,
                );
                assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
            } // _g dropped here; cap restored.
            crate::errno::set_errno(0);
            assert_eq!(
                mknod(b"/dev/x\0".as_ptr(), S_IFCHR | 0o666, 0),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS,
                "CapGuard drop must restore cap; CHR reaches ENOSYS");
        }

        // -- Sentinel --------------------------------------------------------

        /// With CAP_MKNOD held, all existing terminals still fire:
        /// EFAULT, ENOENT, EINVAL, ENOSYS.  Confirms the gate is
        /// gated, not unconditional.
        #[test]
        fn test_mknod_phase188_with_cap_existing_terminals_unchanged() {
            let _g = CapGuard::snapshot();
            // Cap held by default — do not drop.
            // EFAULT.
            crate::errno::set_errno(0);
            assert_eq!(mknod(core::ptr::null(), S_IFCHR | 0o666, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
            // ENOENT.
            crate::errno::set_errno(0);
            assert_eq!(mknod(b"\0".as_ptr(), S_IFCHR | 0o666, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
            // EINVAL.
            crate::errno::set_errno(0);
            assert_eq!(
                mknod(b"/tmp/d\0".as_ptr(), S_IFDIR | 0o755, 0),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
            // ENOSYS for CHR with cap.
            crate::errno::set_errno(0);
            assert_eq!(
                mknod(b"/dev/n\0".as_ptr(), S_IFCHR | 0o666, 0),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
        }

        // -- Cross-check -----------------------------------------------------

        /// Dropping CAP_SETPCAP alone must NOT affect mknod — Linux
        /// gates vfs_mknod on CAP_MKNOD specifically.  Pins down the
        /// cross-cap invariant so a future refactor that probes the
        /// wrong cap is caught.
        #[test]
        fn test_mknod_phase188_setpcap_drop_does_not_affect_mknod() {
            use crate::sys_capability::CAP_SETPCAP;
            let _g = CapGuard::snapshot();
            // Drop only CAP_SETPCAP (bit 8), leave CAP_MKNOD (bit 27).
            let (lo, hi) =
                crate::sys_capability::current_caps_effective();
            let new_lo = lo & !(1u32 << CAP_SETPCAP);
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(
                crate::sys_capability::capset(&mut hdr, data.as_ptr()),
                0,
            );
            // mknod CHR still reaches ENOSYS.
            crate::errno::set_errno(0);
            assert_eq!(
                mknod(b"/dev/y\0".as_ptr(), S_IFCHR | 0o666, 0),
                -1,
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS,
                "CAP_SETPCAP drop must not affect mknod");
        }

        /// Phase 188 errno is EPERM (capable convention), matching
        /// Linux's `vfs_mknod` → `-EPERM`.  Distinct from the EACCES
        /// errno used by Phase 186 (seccomp) — a cross-phase invariant.
        #[test]
        fn test_mknod_phase188_errno_is_eperm_not_eacces() {
            let _g = CapGuard::snapshot();
            drop_cap_mknod();
            crate::errno::set_errno(0);
            assert_eq!(
                mknod(b"/dev/q\0".as_ptr(), S_IFBLK | 0o660, 0),
                -1,
            );
            let e = crate::errno::get_errno();
            assert_eq!(e, crate::errno::EPERM);
            assert_ne!(e, crate::errno::EACCES,
                "vfs_mknod uses EPERM (capable convention)");
        }

        /// mkfifo (always implicit S_IFIFO) is unaffected by cap drop
        /// — it does not pass through vfs_mknod's S_ISCHR/S_ISBLK gate.
        #[test]
        fn test_mknod_phase188_mkfifo_unaffected_by_cap_drop() {
            let _g = CapGuard::snapshot();
            drop_cap_mknod();
            crate::errno::set_errno(0);
            assert_eq!(mkfifo(b"/run/p\0".as_ptr(), 0o644), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS,
                "mkfifo does not pass through CAP_MKNOD gate");
        }
    }
}
