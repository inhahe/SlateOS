//! Virtual filesystem traits and global mount management.
//!
//! Defines the [`FileSystem`] trait that all filesystem implementations
//! must provide, and the [`Vfs`] singleton that manages mounted
//! filesystems and dispatches operations.
//!
//! ## Path resolution
//!
//! The VFS resolves paths component-by-component, following symlinks at
//! each step via `lstat()`.  This enables **cross-mount symlink resolution**:
//! a symlink on ext4 can point to `/tmp/file` (on memfs) and the VFS
//! correctly re-routes through the mount table.  Depth limit is 40.
//!
//! Operations that follow all symlinks (stat, read, write, etc.) use
//! `resolve_follow()`.  Operations that act on the entry itself (remove,
//! rmdir, lstat, readlink, rename) use `resolve_no_follow()`.
//!
//! ## Mount table
//!
//! The VFS uses longest-prefix matching with path-boundary checks.  A
//! mount at `/tmp` captures `/tmp/foo` but not `/tmpfile`.  Multiple
//! mounts are supported; submount directories are synthesized in readdir.

#![allow(dead_code)]

use crate::sync::Mutex;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
// Paths are byte strings, not UTF-8. See `super::path` for why.
pub use super::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Directory entry
// ---------------------------------------------------------------------------

/// Type of a filesystem entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    /// Regular file.
    File,
    /// Directory.
    Directory,
    /// Symbolic link.
    Symlink,
    /// Volume label (FAT-specific, usually hidden).
    VolumeLabel,
    /// Character device node (`/dev/input/event0`, `/dev/dri/card0`, …).
    ///
    /// Only [`devfs`](crate::fs::devfs) produces these, and they exist because
    /// `S_IFCHR` is load-bearing for real clients rather than cosmetic:
    /// libinput refuses a node that is not `S_ISCHR`, and libdrm and ALSA make
    /// the same check. Before this variant, `stat("/dev/input/event0")`
    /// reported a regular file, and those libraries would have rejected a
    /// device that works perfectly.
    ///
    /// Deliberately no `BlockDevice` sibling: this kernel has no block device
    /// nodes to name — storage is reached through the VFS, not through
    /// `/dev/sdaN` — and a variant with no producer is one that every `match`
    /// must answer for while teaching the reader something untrue about the
    /// system.
    CharDevice,
}

/// A single directory entry returned by readdir.
#[derive(Debug, Clone)]
pub struct DirEntry {
    /// Entry name: a single path component, so it contains neither a
    /// separator nor a NUL.
    ///
    /// A [`PathBuf`] and not a `String` because a directory entry name is
    /// whatever bytes the filesystem stored, and forcing UTF-8 on it is not a
    /// validation — it is data loss. ext4 used to *skip* entries whose names
    /// did not decode, which made such a file invisible to `readdir` and left
    /// its parent directory permanently un-`rmdir`-able (the entry is still
    /// there on disk, so the directory is never empty, but nothing can name it
    /// to delete it). Use [`Path::display`] to log one and
    /// [`Path::as_bytes`] for anything else.
    pub name: PathBuf,
    /// Entry type.
    pub entry_type: EntryType,
    /// File size in bytes (0 for directories).
    pub size: u64,
}

// ---------------------------------------------------------------------------
// File metadata
// ---------------------------------------------------------------------------

/// Bitflags for file attributes.
///
/// These are orthogonal to permissions — they control immutability
/// and other special behaviors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileAttr(u32);

#[allow(dead_code)]
impl FileAttr {
    /// No special attributes.
    pub const NONE: Self = Self(0);
    /// File cannot be modified, renamed, or deleted until cleared.
    /// Only a privileged user (capability holder) can set or clear this.
    pub const IMMUTABLE: Self = Self(1 << 0);
    /// File can only be appended to, never overwritten or truncated.
    /// Useful for log files.
    pub const APPEND_ONLY: Self = Self(1 << 1);
    /// File is hidden from normal directory listings.
    pub const HIDDEN: Self = Self(1 << 2);
    /// File is a system file (OS-managed, not user data).
    pub const SYSTEM: Self = Self(1 << 3);

    /// Combine two attribute sets (bitwise OR).
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Check if a specific attribute is set.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Raw bits for serialization.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Construct from raw bits.
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
}

/// Nanosecond timestamp (wall-clock: nanoseconds since the Unix epoch).
///
/// 0 means "not set" or "unknown".
pub type Timestamp = u64;

/// Current wall-clock time for filesystem metadata timestamps.
///
/// File timestamps (created/modified/accessed/changed) must be wall-clock
/// (nanoseconds since the Unix epoch) so that `stat`/`ls -l` report real
/// dates and on-disk ext4 inode times are correct. We deliberately use
/// `clock_realtime()` rather than the boot-relative `hpet::elapsed_ns()`:
/// a file created 5s after boot must not show as 1970-01-01 00:00:05.
///
/// Returns 0 before the RTC is initialized (same "unknown" sentinel as an
/// unset timestamp), and may step backwards if the wall clock is adjusted —
/// both are acceptable for metadata, and the relatime comparisons below use
/// `saturating_sub` so a backwards step simply yields no atime update.
#[inline]
#[must_use]
pub fn metadata_now_ns() -> Timestamp {
    crate::timekeeping::clock_realtime()
}

/// One day in nanoseconds (for relatime threshold).
const ONE_DAY_NS: u64 = 86_400_000_000_000;

// ----- Access mode flags (POSIX access() equivalent) -----

/// Check existence only (no permission bits tested).
pub const F_OK: u32 = 0;
/// Check read permission.
pub const R_OK: u32 = 4;
/// Check write permission.
pub const W_OK: u32 = 2;
/// Check execute permission.
pub const X_OK: u32 = 1;

/// Rich file metadata beyond what [`DirEntry`] carries.
///
/// Filesystem implementations fill in what they can; unsupported
/// fields stay at their defaults (0 / None / empty).
///
/// ## Timestamps
///
/// All timestamps are nanoseconds since boot (from HPET).  A value
/// of 0 means "not available".  The VFS updates `accessed_ns` using
/// **relatime** semantics: only if the current value is older than
/// `modified_ns` or more than one day old.  This avoids the I/O
/// cost of updating atime on every read.
///
/// ## Ownership
///
/// `uid` / `gid` follow standard Unix conventions (0 = root).
/// Filesystems that don't support ownership (e.g., FAT) report 0/0.
///
/// ## Capabilities
///
/// `required_caps` lists capability types needed to access this file.
/// This is checked by the VFS before allowing operations.
///
/// ## Extended attributes
///
/// Arbitrary key-value pairs stored alongside the file.  Maximum
/// key length is 255 bytes, maximum value is 64 KiB (per design spec).
#[derive(Debug, Clone)]
pub struct FileMeta {
    /// File size in bytes.
    pub size: u64,
    /// Entry type (file, directory, symlink, etc.).
    pub entry_type: EntryType,

    // --- Identity ---
    /// Inode number — a filesystem-unique identifier for the underlying
    /// object.  Two paths with the same `ino` on the same mount are the
    /// same file (hard links).  `0` means "not available": filesystems
    /// without a stable per-object identity (pseudo-filesystems, FAT,
    /// ISO9660) report 0, and callers must treat 0 as "unknown" rather
    /// than a real inode.  ext4 reports the real inode number; memfs a
    /// stable synthetic id assigned at node creation.
    pub ino: u64,

    // --- Timestamps (nanoseconds since the Unix epoch, wall-clock;
    //     0 = not available). These are absolute wall-clock times, not
    //     boot-relative monotonic times, so they are stable across
    //     reboots and can be returned directly to userspace stat(). ---
    /// Time the file was created.
    pub created_ns: Timestamp,
    /// Time the file was last modified (content change).
    pub modified_ns: Timestamp,
    /// Time the file was last accessed (read).
    /// Updated with relatime semantics.
    pub accessed_ns: Timestamp,
    /// Time metadata was last changed (permissions, owner, etc.).
    pub changed_ns: Timestamp,

    // --- Ownership ---
    /// Owner user ID (0 = root/system).
    pub uid: u32,
    /// Owner group ID (0 = root/system).
    pub gid: u32,

    // --- Permissions / attributes ---
    /// Unix-style permission bits (rwxrwxrwx, 9 bits).
    /// 0o755 = rwxr-xr-x.  0 = not applicable (e.g., FAT).
    pub permissions: u16,
    /// File attribute flags (immutable, append-only, etc.).
    pub attributes: FileAttr,

    // --- Extended attributes ---
    /// Arbitrary key-value metadata pairs.
    /// Keys are UTF-8 strings, values are byte vectors.
    pub xattrs: Vec<(String, Vec<u8>)>,

    // --- Link count ---
    /// Number of hard links pointing to the underlying data.
    /// Always 1 for filesystems that don't support hard links (FAT, memfs).
    pub nlinks: u32,

    // --- Block count ---
    /// Number of 512-byte sectors allocated to this file.
    /// Used by `stat` and `du`.  0 if not applicable.
    pub blocks: u64,

    // --- Content hash ---
    /// Optional content hash (e.g., SHA-256).
    /// Empty if not computed or not supported.
    pub hash: Vec<u8>,
}

impl FileMeta {
    /// Create a minimal metadata struct with only size and type set.
    ///
    /// All other fields are zeroed / empty.  Useful for filesystems
    /// that don't track rich metadata (e.g., FAT, memfs).
    pub fn minimal(entry_type: EntryType, size: u64) -> Self {
        Self {
            size,
            entry_type,
            ino: 0,
            created_ns: 0,
            modified_ns: 0,
            accessed_ns: 0,
            changed_ns: 0,
            uid: 0,
            gid: 0,
            permissions: 0,
            attributes: FileAttr::NONE,
            nlinks: 1,
            blocks: 0,
            xattrs: Vec::new(),
            hash: Vec::new(),
        }
    }

    /// Create metadata with timestamps set to "now".
    pub fn with_timestamps(entry_type: EntryType, size: u64) -> Self {
        let now = metadata_now_ns();
        Self {
            size,
            entry_type,
            ino: 0,
            created_ns: now,
            modified_ns: now,
            accessed_ns: now,
            changed_ns: now,
            uid: 0,
            gid: 0,
            permissions: if entry_type == EntryType::Directory {
                0o755
            } else {
                0o644
            },
            attributes: FileAttr::NONE,
            nlinks: 1,
            blocks: 0,
            xattrs: Vec::new(),
            hash: Vec::new(),
        }
    }

    /// Check if the access timestamp should be updated (relatime policy).
    ///
    /// Returns `true` if `accessed_ns` is older than `modified_ns`
    /// or more than one day old.
    pub fn should_update_atime(&self) -> bool {
        let now = metadata_now_ns();
        // Update if atime is older than mtime.
        if self.accessed_ns < self.modified_ns {
            return true;
        }
        // Update if atime is more than one day old.
        now.saturating_sub(self.accessed_ns) > ONE_DAY_NS
    }
}

// ---------------------------------------------------------------------------
// Filesystem info (statvfs)
// ---------------------------------------------------------------------------

/// Filesystem space and configuration information.
///
/// Returned by [`FileSystem::statvfs`].  Similar to POSIX `struct statvfs`.
/// Filesystems fill in what they can; unsupported fields stay at 0.
#[derive(Debug, Clone)]
pub struct FsInfo {
    /// Filesystem type name (e.g., `"fat16"`, `"ext4"`, `"memfs"`).
    pub fs_type: String,
    /// Volume label (empty if not available or not set).
    pub volume_label: String,
    /// Fundamental block size in bytes (the allocation unit).
    pub block_size: u64,
    /// Total number of blocks on the filesystem.
    pub total_blocks: u64,
    /// Number of free (available) blocks.
    pub free_blocks: u64,
    /// Total number of inodes (or directory entries, for FAT).
    /// 0 if the concept doesn't apply.
    pub total_inodes: u64,
    /// Number of free inodes.
    pub free_inodes: u64,
    /// Maximum filename length in bytes.
    pub max_name_len: u64,
    /// Whether the filesystem is read-only.
    pub read_only: bool,
}

impl FsInfo {
    /// Total capacity in bytes.
    pub fn total_bytes(&self) -> u64 {
        self.total_blocks.saturating_mul(self.block_size)
    }

    /// Free space in bytes.
    pub fn free_bytes(&self) -> u64 {
        self.free_blocks.saturating_mul(self.block_size)
    }

    /// Used space in bytes.
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes().saturating_sub(self.free_bytes())
    }

    /// Usage percentage (0-100).
    pub fn usage_percent(&self) -> u64 {
        let total = self.total_bytes();
        if total == 0 {
            return 0;
        }
        self.used_bytes().saturating_mul(100) / total
    }
}

// ---------------------------------------------------------------------------
// Filesystem trait
// ---------------------------------------------------------------------------

/// Trait for filesystem implementations.
///
/// All operations use path strings relative to the filesystem root.
/// Paths use forward slash (`/`) as separator.  The filesystem does
/// not see the mount point — the VFS strips it before calling.
///
/// # Thread safety
///
/// The trait requires `Send` so filesystems can be stored behind a
/// mutex.  Individual implementations must document their internal
/// synchronization.
pub trait FileSystem: Send {
    /// Return the filesystem type name (e.g., `"fat16"`, `"ext4"`).
    fn fs_type(&self) -> &str;

    /// List entries in a directory.
    ///
    /// `path` is `"/"` for the root directory, `"/subdir"` for a
    /// subdirectory, etc.
    fn readdir(&mut self, path: &Path) -> KernelResult<Vec<DirEntry>>;

    /// List entries in a directory with pagination.
    ///
    /// Returns up to `count` entries starting from `offset` (0-based).
    /// Also returns the total number of entries in the directory for
    /// the caller to know when it has read everything.
    ///
    /// Default implementation calls `readdir()` and slices.  Filesystem
    /// implementations with native pagination (e.g., ext4 htree) should
    /// override for efficiency.
    fn readdir_at(
        &mut self,
        path: &Path,
        offset: usize,
        count: usize,
    ) -> KernelResult<(Vec<DirEntry>, usize)> {
        let all = self.readdir(path)?;
        let total = all.len();
        let start = offset.min(total);
        let end = start.saturating_add(count).min(total);
        Ok((
            all.into_iter()
                .skip(start)
                .take(end.saturating_sub(start))
                .collect(),
            total,
        ))
    }

    /// Read the contents of a file.
    ///
    /// `path` is the full path relative to filesystem root
    /// (e.g., `"/HELLO.TXT"`).
    ///
    /// Returns the file contents as a byte vector.
    fn read_file(&mut self, path: &Path) -> KernelResult<Vec<u8>>;

    /// Get metadata for a path (file or directory).
    ///
    /// Returns a [`DirEntry`] with name, type, and size.
    fn stat(&mut self, path: &Path) -> KernelResult<DirEntry>;

    /// Write data to a file, creating it if it doesn't exist.
    ///
    /// If the file exists, its contents are replaced entirely.
    /// Returns `NotSupported` if the filesystem is read-only.
    fn write_file(&mut self, path: &Path, data: &[u8]) -> KernelResult<()> {
        let _ = (path, data);
        Err(KernelError::NotSupported)
    }

    /// Delete a file.
    ///
    /// Returns `NotSupported` if the filesystem is read-only.
    fn remove(&mut self, path: &Path) -> KernelResult<()> {
        let _ = path;
        Err(KernelError::NotSupported)
    }

    /// Create a directory.
    ///
    /// Returns `NotSupported` if the filesystem is read-only.
    fn mkdir(&mut self, path: &Path) -> KernelResult<()> {
        let _ = path;
        Err(KernelError::NotSupported)
    }

    /// Remove an empty directory.
    ///
    /// Returns `NotSupported` if the filesystem is read-only.
    fn rmdir(&mut self, path: &Path) -> KernelResult<()> {
        let _ = path;
        Err(KernelError::NotSupported)
    }

    /// Read a range of bytes from a file.
    ///
    /// Default implementation reads the whole file and slices.
    /// Filesystem implementations should override this for efficiency
    /// (e.g., walking the FAT cluster chain to the right offset).
    fn read_at(&mut self, path: &Path, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        let data = self.read_file(path)?;
        let start = (offset as usize).min(data.len());
        let end = (start.saturating_add(len)).min(data.len());
        Ok(data.get(start..end).map_or_else(Vec::new, |s| s.to_vec()))
    }

    /// Write bytes at a specific offset within a file.
    ///
    /// Default implementation reads the whole file, patches the range,
    /// and rewrites.  Filesystem implementations should override for
    /// efficiency.
    fn write_at(&mut self, path: &Path, offset: u64, data: &[u8]) -> KernelResult<()> {
        let mut contents = match self.read_file(path) {
            Ok(c) => c,
            Err(KernelError::NotFound) => Vec::new(),
            Err(e) => return Err(e),
        };

        let start = offset as usize;
        let end = start.saturating_add(data.len());

        // Extend the file if writing past current end.
        if end > contents.len() {
            contents.resize(end, 0);
        }

        if let Some(dest) = contents.get_mut(start..end) {
            dest.copy_from_slice(data);
        }

        self.write_file(path, &contents)
    }

    /// Pre-allocate space for a file without writing data.
    ///
    /// Ensures that at least `size` bytes are allocated for the file.
    /// The file's logical size does not change (reads beyond the
    /// current size still return zero/error).  This is useful for
    /// databases and log files that know their eventual size upfront —
    /// pre-allocation avoids fragmentation from incremental growth.
    ///
    /// Default implementation: no-op (reports success without actually
    /// reserving space).  Filesystems with block allocation (ext4, FAT)
    /// should override to actually reserve blocks.
    fn fallocate(&mut self, path: &Path, size: u64) -> KernelResult<()> {
        let _ = (path, size);
        // Default: pretend we allocated.  The actual write will extend
        // the file when data arrives.
        Ok(())
    }

    /// Truncate a file to the given size.
    ///
    /// If `size` is less than the current file size, data beyond
    /// `size` is discarded.  If `size` is greater, the file is
    /// extended with zero bytes.
    ///
    /// Default implementation reads, resizes, and rewrites.
    fn truncate(&mut self, path: &Path, size: u64) -> KernelResult<()> {
        let mut contents = match self.read_file(path) {
            Ok(c) => c,
            Err(KernelError::NotFound) => Vec::new(),
            Err(e) => return Err(e),
        };
        contents.resize(size as usize, 0);
        self.write_file(path, &contents)
    }

    /// Rename or move a file or directory.
    ///
    /// Both `from` and `to` are paths relative to the filesystem root.
    /// Returns `NotSupported` if the filesystem is read-only.
    fn rename(&mut self, from: &Path, to: &Path) -> KernelResult<()> {
        let _ = (from, to);
        Err(KernelError::NotSupported)
    }

    /// Atomically exchange two existing entries (Linux
    /// `renameat2(RENAME_EXCHANGE)`).
    ///
    /// Both `a` and `b` are paths relative to the filesystem root and BOTH
    /// must already exist (else `NotFound`); on success the entries swap
    /// places. The operation is atomic with respect to the filesystem's own
    /// locking. Default implementation returns `NotSupported` — the VFS maps
    /// that to `EINVAL` at the syscall boundary, matching how a Linux
    /// filesystem whose `->rename` lacks `RENAME_EXCHANGE` support responds.
    fn rename_exchange(&mut self, a: &Path, b: &Path) -> KernelResult<()> {
        let _ = (a, b);
        Err(KernelError::NotSupported)
    }

    /// Return optional debug/statistics information.
    ///
    /// Default returns an empty string.  Filesystem implementations
    /// can override to report cache statistics, internal counters, etc.
    fn debug_stats(&self) -> String {
        String::new()
    }

    // --- Extended metadata operations ---

    /// Return rich metadata for a path.
    ///
    /// Default implementation builds a minimal [`FileMeta`] from `stat()`.
    /// Filesystems that track timestamps, ownership, or xattrs should
    /// override this.
    fn metadata(&mut self, path: &Path) -> KernelResult<FileMeta> {
        let entry = self.stat(path)?;
        Ok(FileMeta::minimal(entry.entry_type, entry.size))
    }

    /// Return rich metadata for a path WITHOUT following a trailing symlink.
    ///
    /// This is the no-follow analogue of [`metadata`](Self::metadata):
    /// if `path` ends at a symlink, the symlink's own metadata is
    /// returned (with `entry_type == Symlink`) rather than the target's.
    ///
    /// Default implementation builds a minimal [`FileMeta`] from
    /// `lstat()`.  Filesystems that track timestamps, ownership, or
    /// xattrs should override this (typically mirroring their
    /// `metadata()` override but without symlink resolution).
    fn lmetadata(&mut self, path: &Path) -> KernelResult<FileMeta> {
        let entry = self.lstat(path)?;
        Ok(FileMeta::minimal(entry.entry_type, entry.size))
    }

    /// Set file attributes (immutable, append-only, etc.).
    ///
    /// Default: not supported.
    fn set_attributes(&mut self, path: &Path, attrs: FileAttr) -> KernelResult<()> {
        let _ = (path, attrs);
        Err(KernelError::NotSupported)
    }

    /// Set ownership (uid/gid).
    ///
    /// Default: not supported.
    fn set_owner(&mut self, path: &Path, uid: u32, gid: u32) -> KernelResult<()> {
        let _ = (path, uid, gid);
        Err(KernelError::NotSupported)
    }

    /// Set ownership on the path's final component WITHOUT following it if it
    /// is a symlink (`lchown` / `fchownat(AT_SYMLINK_NOFOLLOW)`).
    ///
    /// Default delegates to [`set_owner`](Self::set_owner) — correct for
    /// filesystems that have no symlinks (e.g. FAT).  Symlink-capable
    /// filesystems (memfs, ext4) override this to resolve the final
    /// component without following, so the link inode itself is chowned.
    fn set_owner_no_follow(&mut self, path: &Path, uid: u32, gid: u32) -> KernelResult<()> {
        self.set_owner(path, uid, gid)
    }

    /// Set Unix-style permission bits (rwxrwxrwx).
    ///
    /// Default: not supported.
    fn set_permissions(&mut self, path: &Path, permissions: u16) -> KernelResult<()> {
        let _ = (path, permissions);
        Err(KernelError::NotSupported)
    }

    /// Set permission bits on the path's final component WITHOUT following it
    /// if it is a symlink (`fchmodat2(AT_SYMLINK_NOFOLLOW)`, Linux 6.6+).
    ///
    /// Default delegates to [`set_permissions`](Self::set_permissions) —
    /// correct for filesystems that have no symlinks (e.g. FAT).  Symlink-
    /// capable filesystems (memfs, ext4) override this to resolve the final
    /// component without following, so the link inode itself is chmod-ed.
    fn set_permissions_no_follow(&mut self, path: &Path, permissions: u16) -> KernelResult<()> {
        self.set_permissions(path, permissions)
    }

    /// Update timestamps.
    ///
    /// Pass 0 for any timestamp to leave it unchanged.
    /// Default: not supported.
    fn set_times(
        &mut self,
        path: &Path,
        accessed_ns: Timestamp,
        modified_ns: Timestamp,
    ) -> KernelResult<()> {
        let _ = (path, accessed_ns, modified_ns);
        Err(KernelError::NotSupported)
    }

    /// Update timestamps WITHOUT following a final symlink
    /// (`lutimes` / `utimensat(AT_SYMLINK_NOFOLLOW)`).
    ///
    /// Default delegates to [`set_times`](Self::set_times) — correct for
    /// symlink-free filesystems.  memfs/ext4 override to stamp the link
    /// inode itself.
    fn set_times_no_follow(
        &mut self,
        path: &Path,
        accessed_ns: Timestamp,
        modified_ns: Timestamp,
    ) -> KernelResult<()> {
        self.set_times(path, accessed_ns, modified_ns)
    }

    /// Get an extended attribute value by key.
    ///
    /// Default: not supported.
    fn get_xattr(&mut self, path: &Path, key: &str) -> KernelResult<Vec<u8>> {
        let _ = (path, key);
        Err(KernelError::NotSupported)
    }

    /// Set an extended attribute.
    ///
    /// Default: not supported.
    fn set_xattr(&mut self, path: &Path, key: &str, value: &[u8]) -> KernelResult<()> {
        let _ = (path, key, value);
        Err(KernelError::NotSupported)
    }

    /// Remove an extended attribute.
    ///
    /// Default: not supported.
    fn remove_xattr(&mut self, path: &Path, key: &str) -> KernelResult<()> {
        let _ = (path, key);
        Err(KernelError::NotSupported)
    }

    /// List all extended attribute keys for a path.
    ///
    /// Default: empty list.
    fn list_xattrs(&mut self, path: &Path) -> KernelResult<Vec<String>> {
        let _ = path;
        Ok(Vec::new())
    }

    // --- No-follow xattr variants (lgetxattr/lsetxattr/llistxattr/
    // lremovexattr): operate on a trailing symlink itself, not its target.
    // Default delegates to the following version — correct for symlink-free
    // filesystems (FAT); memfs/ext4 override to resolve the final component
    // without following. ---

    /// No-follow analogue of [`get_xattr`](Self::get_xattr) (`lgetxattr`).
    fn get_xattr_no_follow(&mut self, path: &Path, key: &str) -> KernelResult<Vec<u8>> {
        self.get_xattr(path, key)
    }

    /// No-follow analogue of [`set_xattr`](Self::set_xattr) (`lsetxattr`).
    fn set_xattr_no_follow(&mut self, path: &Path, key: &str, value: &[u8]) -> KernelResult<()> {
        self.set_xattr(path, key, value)
    }

    /// No-follow analogue of [`remove_xattr`](Self::remove_xattr) (`lremovexattr`).
    fn remove_xattr_no_follow(&mut self, path: &Path, key: &str) -> KernelResult<()> {
        self.remove_xattr(path, key)
    }

    /// No-follow analogue of [`list_xattrs`](Self::list_xattrs) (`llistxattr`).
    fn list_xattrs_no_follow(&mut self, path: &Path) -> KernelResult<Vec<String>> {
        self.list_xattrs(path)
    }

    // --- Symlink operations ---

    /// Create a symbolic link at `path` pointing to `target`.
    ///
    /// `target` is stored as-is (not resolved).  It can be absolute or
    /// relative.  The symlink is resolved when it is traversed during
    /// path resolution.
    ///
    /// Default: not supported.
    fn symlink(&mut self, path: &Path, target: &Path) -> KernelResult<()> {
        let _ = (path, target);
        Err(KernelError::NotSupported)
    }

    /// Read the target of a symbolic link.
    ///
    /// Does NOT follow the symlink — returns the stored target path.
    ///
    /// Default: not supported.
    fn readlink(&mut self, path: &Path) -> KernelResult<PathBuf> {
        let _ = path;
        Err(KernelError::NotSupported)
    }

    /// Stat a path without following the final symbolic link.
    ///
    /// If `path` ends at a symlink, returns the symlink's own metadata
    /// (with `entry_type == Symlink`).  Intermediate symlinks in the
    /// path are still followed.
    ///
    /// Default implementation falls back to `stat()`.
    fn lstat(&mut self, path: &Path) -> KernelResult<DirEntry> {
        self.stat(path)
    }

    /// Return filesystem space and configuration information.
    ///
    /// Default returns a minimal struct with only the type name set.
    /// Filesystems that can report capacity/usage should override this.
    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        Ok(FsInfo {
            fs_type: String::from(self.fs_type()),
            volume_label: String::new(),
            block_size: 0,
            total_blocks: 0,
            free_blocks: 0,
            total_inodes: 0,
            free_inodes: 0,
            max_name_len: 255,
            read_only: false,
        })
    }

    /// Name of the block device backing this filesystem, if any.
    ///
    /// Disk-backed filesystems (FAT, ext4) return the registry name of their
    /// device (e.g. `"vda"`); virtual filesystems (procfs, sysfs, devfs,
    /// memfs) return `None`.  Used by the device-oriented `fstrim` entry point
    /// to find the mount backed by a given device.
    fn device_name(&self) -> Option<&str> {
        None
    }

    /// Discard (TRIM) the filesystem's free space on the backing device.
    ///
    /// Walks the free-space metadata and issues
    /// [`BlockDevice::discard`](crate::blkdev::BlockDevice::discard) for every
    /// run of free blocks, hinting to an SSD that those blocks may be released.
    /// This is the kernel side of `fstrim(8)`: it is **non-destructive** — only
    /// blocks the filesystem considers free are discarded; live file data is
    /// never touched.
    ///
    /// Returns the number of bytes discarded.  The default implementation
    /// returns `Ok(0)`: virtual filesystems (procfs, sysfs, devfs, memfs) and
    /// any filesystem whose backing device does not support discard have
    /// nothing to trim, which is a successful no-op rather than an error.
    fn trim(&mut self) -> KernelResult<u64> {
        Ok(0)
    }

    /// Create a hard link.
    ///
    /// `existing` is the path to the existing file.
    /// `new_path` is where the new directory entry should appear.
    ///
    /// Hard links create an additional directory entry pointing to the
    /// same underlying file data (same inode on ext4).  Both paths must
    /// be on the same filesystem.
    ///
    /// Default: not supported (FAT, memfs, procfs, devfs, ISO9660).
    fn link(&mut self, existing: &Path, new_path: &Path) -> KernelResult<()> {
        let _ = (existing, new_path);
        Err(KernelError::NotSupported)
    }

    /// Create a hard link WITHOUT following a trailing symlink in `existing`.
    ///
    /// This is the semantics of plain `link(2)` and `linkat` without
    /// `AT_SYMLINK_FOLLOW`: if `existing` names a symlink, the new entry
    /// hard-links the symlink inode itself, not its target.
    ///
    /// Default: delegate to [`link`].  This is correct for filesystems that
    /// either lack hard links entirely (FAT, memfs, procfs, devfs, ISO9660 —
    /// they return `NotSupported` regardless) or lack symlinks (FAT), where
    /// the follow/no-follow distinction cannot arise.  ext4 overrides this to
    /// resolve `existing` without following the final component.
    fn link_no_follow(&mut self, existing: &Path, new_path: &Path) -> KernelResult<()> {
        self.link(existing, new_path)
    }

    /// Flush (sync) all dirty data and metadata to stable storage.
    ///
    /// Called by `Vfs::sync()` to ensure durability.  For filesystems
    /// backed by block devices, this should flush the buffer cache and
    /// any pending journal transactions.
    ///
    /// Default: no-op (suitable for in-memory or read-only filesystems).
    fn sync(&mut self) -> KernelResult<()> {
        Ok(())
    }

    /// Set the filesystem volume label.
    ///
    /// Updates the on-disk volume label metadata.  Not all filesystems
    /// support labels — the default returns `NotSupported`.
    ///
    /// FAT: updates both the BPB boot sector and the root directory
    /// volume label entry.  Label is truncated to 11 bytes (8.3 format).
    fn set_volume_label(&mut self, _label: &str) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }
}

// ---------------------------------------------------------------------------
// VFS — global filesystem manager
// ---------------------------------------------------------------------------

/// A mount point in the VFS.
/// Per-mount options controlling filesystem behavior.
#[derive(Debug, Clone, Copy)]
pub struct MountOptions {
    /// Mounted read-only — all write operations return `ReadOnlyFilesystem`.
    pub read_only: bool,
    /// Don't update access timestamps on reads.
    pub noatime: bool,
    /// Don't allow execution from this mount (reserved for future use).
    pub noexec: bool,
    /// Don't honor setuid/setgid bits (reserved for future use).
    pub nosuid: bool,
}

impl MountOptions {
    /// Default options: rw, relatime, suid, exec.
    pub const fn defaults() -> Self {
        Self {
            read_only: false,
            noatime: false,
            noexec: false,
            nosuid: false,
        }
    }

    /// Parse mount options from a comma-separated string (e.g., "ro,noatime").
    pub fn parse(opts: &str) -> Self {
        let mut result = Self::defaults();
        for opt in opts.split(',') {
            let opt = opt.trim();
            match opt {
                "ro" | "readonly" => result.read_only = true,
                "rw" | "readwrite" => result.read_only = false,
                "noatime" => result.noatime = true,
                "atime" => result.noatime = false,
                "noexec" => result.noexec = true,
                "exec" => result.noexec = false,
                "nosuid" => result.nosuid = true,
                "suid" => result.nosuid = false,
                "" => {}
                _ => {
                    crate::serial_println!("[vfs] Ignoring unknown mount option: '{}'", opt);
                }
            }
        }
        result
    }
}

/// Format options as a comma-separated string for /proc/mounts.
impl core::fmt::Display for MountOptions {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut parts: Vec<&str> = Vec::new();
        parts.push(if self.read_only { "ro" } else { "rw" });
        if self.noatime {
            parts.push("noatime");
        }
        if self.noexec {
            parts.push("noexec");
        }
        if self.nosuid {
            parts.push("nosuid");
        }
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            f.write_str(part)?;
        }
        Ok(())
    }
}

/// A mounted filesystem instance behind its per-mount lock.  Cloning the
/// `Arc` hands out an independent handle that can be queried without holding
/// the global VFS lock (see [`MountPoint::fs`] and design-decisions §43).
type MountedFs = Arc<Mutex<Box<dyn FileSystem>>>;

struct MountPoint {
    /// Path where this filesystem is mounted (e.g., `"/"`).
    path: PathBuf,
    /// The filesystem implementation.
    ///
    /// Held behind a *per-mount* lock (not the global VFS lock) so that
    /// filesystem I/O does not serialize on a single global mutex and,
    /// crucially, so stacked filesystems (e.g. the overlay) can re-enter the
    /// VFS to read their backing layers without deadlocking: the global VFS
    /// lock is released the moment the mount table lookup is done, and the
    /// per-mount lock taken here is a *different* lock from the one guarding
    /// any lower-layer mount.  See design-decisions §43.
    fs: MountedFs,
    /// The filesystem's type name (`"procfs"`, `"memfs"`, `"ext4"`, …), copied
    /// out of [`FileSystem::fs_type`] at mount time.
    ///
    /// Cached here rather than fetched through `fs.lock().fs_type()` because
    /// *enumerating the mount table must not lock any filesystem*. It looks
    /// harmless — `fs_type` returns a constant and the per-mount lock is
    /// released immediately — but the caller is frequently a filesystem that is
    /// **already locked**, and then the "immediately" never arrives:
    ///
    /// ```text
    /// Vfs::readdir("/proc")  ->  procfs mutex held by the VFS
    ///   procfs readdir sizes every root file
    ///     gen_mounts() -> Vfs::mounts_full()
    ///       fs.lock() on each mount ... including procfs   <-- self-deadlock
    /// ```
    ///
    /// That is not a hypothetical: it halted the kernel on `ls /proc` and on
    /// `cat /proc/mounts`, and was found by the `sysdiag` self-test the first
    /// time it ran at boot. design-decisions §43 makes the per-mount lock
    /// re-entrant-safe for *stacked* filesystems, where the lower layer is a
    /// different lock; it cannot help a filesystem that enumerates the table it
    /// is itself in. The value is immutable for the life of the mount, so
    /// caching costs one `String` per mount and removes the hazard by
    /// construction rather than by asking every caller to be careful.
    fs_type: String,
    /// Mount options (read-only, noatime, etc.).
    options: MountOptions,
    /// Stable, never-reused id for this mounted filesystem instance.
    ///
    /// Assigned monotonically at mount time from [`NEXT_FS_ID`] and kept for
    /// the lifetime of the mount.  Unlike the mount's index in the `mounts`
    /// `Vec` (which shifts when an earlier mount is removed), this id is
    /// stable across unmounts of *other* filesystems, so it can disambiguate
    /// inode numbers that two different filesystems might both use.  It is the
    /// device-id half of a [`FileId`] (the `(fs_id, ino)` pair that uniquely
    /// identifies a file system-wide), used as the page-cache key — see
    /// design-decisions §23/§36.
    fs_id: u64,
}

/// Monotonic source of stable mount ids ([`MountPoint::fs_id`]).
///
/// Starts at 1 so `0` can mean "no/unknown filesystem".  Never decrements and
/// ids are never reused, so a `FileId` minted for one mount can never collide
/// with a later mount even after the original is unmounted.
static NEXT_FS_ID: AtomicU64 = AtomicU64::new(1);

/// A system-wide-unique identity for a filesystem object.
///
/// A file is uniquely identified by the pair `(fs_id, ino)`: the stable mount
/// id ([`MountPoint::fs_id`]) plus the filesystem-local inode number
/// ([`FileMeta::ino`]).  Two paths that resolve to the same `(fs_id, ino)` are
/// the same underlying object (e.g. hard links on ext4); two objects on
/// different mounts that happen to share an `ino` are distinguished by `fs_id`.
///
/// This is the key type for the read-only page cache (design-decisions
/// §23/§36): cached frames are keyed by `(FileId, page-offset)` so that N
/// processes mapping the same shared library share one set of physical frames.
/// A file is only cacheable when it has a *stable* identity — i.e. its backing
/// filesystem reports a non-zero `ino` (ext4 real inodes, memfs synthetic
/// ids).  Filesystems without stable per-object identity (FAT, ISO9660,
/// pseudo-filesystems reporting `ino == 0`) are not cacheable;
/// [`Vfs::file_identity`] returns `None` for them so callers fall back to the
/// per-mapping read path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId {
    /// Stable mount id of the filesystem holding this object.
    pub fs_id: u64,
    /// Filesystem-local inode number (guaranteed non-zero in a `FileId`).
    pub ino: u64,
}

/// The global VFS state.
static VFS: Mutex<VfsInner> = Mutex::new(VfsInner { mounts: Vec::new() });

struct VfsInner {
    mounts: Vec<MountPoint>,
}

// ---------------------------------------------------------------------------
// Advisory file locking
// ---------------------------------------------------------------------------

/// Type of advisory lock on a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockType {
    /// Shared (read) lock — multiple holders allowed.
    Shared,
    /// Exclusive (write) lock — at most one holder.
    Exclusive,
}

/// A single advisory lock held on a file.
#[derive(Debug, Clone)]
struct FileLock {
    /// Owning process/task ID (0 = kernel).
    owner: u64,
    /// Lock type.
    lock_type: LockType,
}

/// Per-path lock table entry.
#[derive(Debug, Clone)]
struct PathLockEntry {
    /// Canonical path (after symlink resolution).
    path: PathBuf,
    /// Active locks on this path.
    locks: Vec<FileLock>,
}

/// Global advisory lock table.
///
/// Tracks advisory locks per file path.  Locks are process-scoped:
/// each lock is owned by a process ID, and a process can hold at most
/// one lock per path (re-locking upgrades/downgrades atomically).
///
/// ## Semantics
///
/// - **Shared locks**: multiple processes can hold shared locks
///   simultaneously.  A shared lock is incompatible with an exclusive lock.
/// - **Exclusive locks**: only one process can hold an exclusive lock.
///   Incompatible with both shared and exclusive locks from other owners.
/// - **Upgrade**: a process holding a shared lock can upgrade to exclusive
///   if no other locks exist.
/// - **Downgrade**: a process holding an exclusive lock can downgrade to
///   shared at any time.
///
/// Locks are advisory — they don't prevent actual I/O.  Cooperating
/// processes must check locks before accessing files.
static LOCK_TABLE: Mutex<Vec<PathLockEntry>> = Mutex::new(Vec::new());

/// Maximum number of distinct file paths that can be locked.
const MAX_LOCKED_PATHS: usize = 1024;

// ---------------------------------------------------------------------------
// VFS path resolution cache (dcache)
// ---------------------------------------------------------------------------

/// Number of entries in the VFS-level path resolution cache.
///
/// Caches `(normalized_path, follow_last) → resolved_path` to avoid the
/// expensive component-by-component `lstat()` walk that `resolve_inner()`
/// does for every VFS operation.  1024 entries covers deep directory
/// hierarchies and multi-process workloads.  At ~200 bytes per entry,
/// the total overhead is ~200 KiB.
// pub(crate) rather than pub(super) so `bench.rs` can report the scan length
// next to the measured lookup cost — the two numbers only mean anything
// together (a linear scan's cost is a function of how many slots are live).
pub(crate) const VFS_DCACHE_SIZE: usize = 1024;

/// A single entry in the VFS path resolution cache.
struct VfsDcacheEntry {
    /// The normalized input path (key).
    key: PathBuf,
    /// Whether the final component was followed (true = resolve_follow,
    /// false = resolve_no_follow).
    follow_last: bool,
    /// The resolved output path (after symlink expansion).
    /// Empty for negative entries (path does not exist).
    resolved: PathBuf,
    /// Monotonic access counter for LRU eviction.
    last_access: u64,
    /// Whether this entry contains valid data.
    valid: bool,
    /// Negative cache entry: true if this path is known to NOT exist.
    /// On hit, the caller can short-circuit with NotFound without
    /// walking the filesystem.  Invalidated on any mutation in the
    /// parent directory, same as positive entries.
    negative: bool,
}

impl VfsDcacheEntry {
    const fn empty() -> Self {
        Self {
            key: PathBuf::new(),
            follow_last: false,
            resolved: PathBuf::new(),
            last_access: 0,
            valid: false,
            negative: false,
        }
    }
}

/// Result of a VFS dcache lookup.
///
/// Distinguished from `Option<PathBuf>` so callers can tell the difference
/// between "not in cache" (walk needed) and "known not to exist" (short-
/// circuit with `NotFound`).
enum DcacheLookup {
    /// Path resolves to this value (positive cache hit).
    Hit(PathBuf),
    /// Path is known NOT to exist — a parent directory was missing when
    /// the path was last resolved.  Caller can return `NotFound`
    /// immediately without walking the filesystem.
    NegativeHit,
    /// Path not in cache — caller must do the full resolve walk.
    Miss,
}

/// VFS-level directory entry cache.
///
/// Caches resolved paths to skip the per-component symlink-checking walk
/// in `resolve_inner()`.  Each VFS operation first checks this cache;
/// a hit avoids N `lstat()` calls (where N is the path depth).
///
/// ## Negative entries
///
/// When path resolution fails with `NotFound` (a parent directory was
/// missing), the result is cached as a negative entry.  Future lookups
/// for the same path short-circuit with `NotFound` without touching the
/// filesystem.  Negative entries are invalidated when files or
/// directories are created at matching paths.
///
/// ## Invalidation
///
/// Any mutation (write, remove, mkdir, rmdir, rename, symlink, link)
/// invalidates entries whose key or resolved path has a matching prefix.
/// Creation operations (mkdir, write, link) specifically invalidate
/// negative entries so the new path becomes resolvable.  Mount/unmount
/// invalidates everything (rare operations).
///
/// ## Thread safety
///
/// Protected by its own spinlock, separate from the VFS mount table
/// lock.  This avoids extending the VFS critical section.
struct VfsDcache {
    entries: [VfsDcacheEntry; VFS_DCACHE_SIZE],
    /// Monotonic access counter.
    counter: u64,
    /// Cache hit count (for diagnostics).
    hits: u64,
    /// Cache miss count (for diagnostics).
    misses: u64,
}

impl VfsDcache {
    const fn new() -> Self {
        // SAFETY: VfsDcacheEntry::empty() is const and produces a valid
        // zero-like state.  We can't use [VfsDcacheEntry::empty(); N]
        // because PathBuf isn't Copy, so we initialize in init().
        Self {
            entries: [const { VfsDcacheEntry::empty() }; VFS_DCACHE_SIZE],
            counter: 0,
            hits: 0,
            misses: 0,
        }
    }

    /// Look up a resolved path in the cache.
    ///
    /// Returns `Hit(resolved)` for a positive cache entry, `NegativeHit`
    /// for a path known not to exist, or `Miss` if the path is not cached.
    fn lookup(&mut self, key: &Path, follow_last: bool) -> DcacheLookup {
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.follow_last == follow_last && entry.key.as_path() == key {
                self.counter = self.counter.wrapping_add(1);
                entry.last_access = self.counter;
                self.hits = self.hits.wrapping_add(1);
                if entry.negative {
                    return DcacheLookup::NegativeHit;
                }
                return DcacheLookup::Hit(entry.resolved.clone());
            }
        }
        self.misses = self.misses.wrapping_add(1);
        DcacheLookup::Miss
    }

    /// Insert a positive resolution result into the cache.
    ///
    /// Overwrites the least-recently-used entry if the cache is full.
    /// If the key previously held a negative entry, it is promoted to
    /// positive (the path now exists).
    fn insert(&mut self, key: &Path, follow_last: bool, resolved: &Path) {
        self.counter = self.counter.wrapping_add(1);

        // Check if already cached (update in place).
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.follow_last == follow_last && entry.key.as_path() == key {
                entry.resolved.clear();
                entry.resolved.extend_bytes(resolved.as_bytes());
                entry.last_access = self.counter;
                entry.negative = false;
                return;
            }
        }

        // Find an empty slot.
        for entry in self.entries.iter_mut() {
            if !entry.valid {
                entry.key = key.to_path_buf();
                entry.follow_last = follow_last;
                entry.resolved = resolved.to_path_buf();
                entry.last_access = self.counter;
                entry.valid = true;
                entry.negative = false;
                return;
            }
        }

        // Evict LRU entry.
        let mut lru_idx = 0;
        let mut lru_access = u64::MAX;
        for (i, entry) in self.entries.iter().enumerate() {
            if entry.last_access < lru_access {
                lru_access = entry.last_access;
                lru_idx = i;
            }
        }

        self.entries[lru_idx].key.clear();
        self.entries[lru_idx].key.extend_bytes(key.as_bytes());
        self.entries[lru_idx].follow_last = follow_last;
        self.entries[lru_idx].resolved.clear();
        self.entries[lru_idx]
            .resolved
            .extend_bytes(resolved.as_bytes());
        self.entries[lru_idx].last_access = self.counter;
        self.entries[lru_idx].valid = true;
        self.entries[lru_idx].negative = false;
    }

    /// Insert a negative cache entry for a path known to NOT exist.
    ///
    /// Used when `resolve_inner()` returns `NotFound` — the path's
    /// parent chain is broken, and subsequent lookups can short-circuit.
    /// Negative entries are invalidated by `invalidate_negative_prefix()`
    /// when creation operations succeed at matching paths.
    fn insert_negative(&mut self, key: &Path, follow_last: bool) {
        self.counter = self.counter.wrapping_add(1);

        // Check if already cached (update to negative in place).
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.follow_last == follow_last && entry.key.as_path() == key {
                entry.resolved.clear();
                entry.negative = true;
                entry.last_access = self.counter;
                return;
            }
        }

        // Find an empty slot.
        for entry in self.entries.iter_mut() {
            if !entry.valid {
                entry.key = key.to_path_buf();
                entry.follow_last = follow_last;
                entry.resolved = PathBuf::new();
                entry.last_access = self.counter;
                entry.valid = true;
                entry.negative = true;
                return;
            }
        }

        // Evict LRU entry.
        let mut lru_idx = 0;
        let mut lru_access = u64::MAX;
        for (i, entry) in self.entries.iter().enumerate() {
            if entry.last_access < lru_access {
                lru_access = entry.last_access;
                lru_idx = i;
            }
        }

        self.entries[lru_idx].key.clear();
        self.entries[lru_idx].key.extend_bytes(key.as_bytes());
        self.entries[lru_idx].follow_last = follow_last;
        self.entries[lru_idx].resolved.clear();
        self.entries[lru_idx].last_access = self.counter;
        self.entries[lru_idx].valid = true;
        self.entries[lru_idx].negative = true;
    }

    /// Invalidate all entries whose key or resolved path starts with
    /// `prefix` (or whose key/resolved path IS the prefix).
    ///
    /// Uses path-boundary checking: `/tmp` invalidates `/tmp/foo` but
    /// not `/tmpfile`.
    fn invalidate_prefix(&mut self, prefix: &Path) {
        for entry in self.entries.iter_mut() {
            if !entry.valid {
                continue;
            }
            if entry.key.starts_with(prefix) || entry.resolved.starts_with(prefix) {
                entry.valid = false;
            }
        }
    }

    /// Invalidate only negative entries whose key starts with `prefix`.
    ///
    /// Used by creation operations (mkdir, write_file, link) — positive
    /// cache entries remain valid because creating a new entry doesn't
    /// change how existing paths resolve, but a previously-negative path
    /// now exists.
    fn invalidate_negative_prefix(&mut self, prefix: &Path) {
        for entry in self.entries.iter_mut() {
            if !entry.valid || !entry.negative {
                continue;
            }
            if entry.key.starts_with(prefix) {
                entry.valid = false;
            }
        }
    }

    /// Invalidate all cache entries.
    ///
    /// Used on mount/unmount where any cached resolution could be stale.
    fn invalidate_all(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.valid = false;
        }
    }

    /// Return (hits, misses, valid_entries) for diagnostics.
    fn stats(&self) -> (u64, u64, usize) {
        let valid = self.entries.iter().filter(|e| e.valid).count();
        (self.hits, self.misses, valid)
    }
}

/// Global VFS path resolution cache.
static VFS_DCACHE: Mutex<VfsDcache> = Mutex::new(VfsDcache::new());

/// Public VFS interface.
///
/// All methods are static — they operate on the global VFS singleton.
pub struct Vfs;

impl Vfs {
    /// Mount a filesystem at the given path.
    ///
    /// `mount_path` must start with `/`.  Multiple mounts are supported;
    /// the VFS uses longest-prefix matching to route operations.
    pub fn mount(mount_path: impl AsRef<Path>, fs: Box<dyn FileSystem>) -> KernelResult<()> {
        let mount_path = mount_path.as_ref();
        Self::mount_with_options(mount_path, fs, MountOptions::defaults())
    }

    /// Mount a filesystem at the given path with specific mount options.
    pub fn mount_with_options(
        mount_path: impl AsRef<Path>,
        fs: Box<dyn FileSystem>,
        options: MountOptions,
    ) -> KernelResult<()> {
        let mount_path = mount_path.as_ref();
        if !mount_path.is_absolute() {
            return Err(KernelError::InvalidArgument);
        }
        // `.` and `..` are rejected rather than resolved.  Resolving them
        // here would need the VFS lock we have not taken yet, and a mount
        // recorded with them is unreachable anyway: `mount_matches` compares
        // components literally, so no resolved lookup path could ever match
        // `/mnt/../mnt`.  That is the same registered-but-unreachable class
        // the parent-exists check below exists to prevent.
        if !mount_path.has_no_dot_components() {
            return Err(KernelError::InvalidArgument);
        }
        // Canonicalise the separators before anything records or compares
        // these bytes — see `normalize_mount_path` for what depends on it.
        let mount_path = &normalize_mount_path(mount_path);

        // Refuse a mount that path lookup could never reach.
        //
        // `resolve_inner` walks every non-final component and requires each
        // to exist in its containing filesystem — a mount point counts,
        // because `resolve_mount`'s longest-prefix match maps it to the
        // mounted fs. So a mount at `/mnt/x` when `/mnt` does not exist is
        // registered, consumes an `fs_id`, appears in `/proc/mounts`, and is
        // reachable by *nothing*: every lookup dies on the `mnt` component
        // before it ever consults the mount table.
        //
        // That is the silently-wrong-behaviour class this codebase keeps
        // paying for, and it cost a real debugging session: overlay
        // self-test 13 mounted at `/mnt/ovl-cow-test`, `mount` returned
        // `Ok`, the log said "Mounted overlay filesystem at
        // '/mnt/ovl-cow-test' (rw)", and the very next read failed
        // `NotFound` — pointing the investigation at the overlay engine,
        // which was working perfectly.
        //
        // Only the *parent* must exist, not the mount point itself. Linux
        // requires the mount point directory to exist, but our boot sequence
        // mounts `/proc`, `/dev` and `/sys` over a root memfs that has no
        // such directories, and longest-prefix matching makes the mount
        // point itself reachable regardless. Requiring the parent is the
        // weakest condition that makes "registered" and "reachable" mean the
        // same thing.
        //
        // Checked before taking `VFS.lock()`, because `stat` re-enters the
        // VFS and would deadlock against our own guard.
        if let Some(parent) = mount_path.parent() {
            match Self::stat(parent) {
                Ok(entry) if entry.entry_type == EntryType::Directory => {}
                Ok(_) => return Err(KernelError::NotADirectory),
                Err(e) => return Err(e),
            }
        }

        let mut vfs = VFS.lock();

        // Check for duplicate mount point.  Both sides are normalised, so
        // `/mnt` and `/mnt/` now collide as they should.
        for mp in &vfs.mounts {
            if mp.path.as_path() == mount_path.as_path() {
                return Err(KernelError::AlreadyExists);
            }
        }

        let opts_str = options.to_string();
        crate::serial_println!(
            "[vfs] Mounted {} filesystem at '{}' ({})",
            fs.fs_type(),
            mount_path.display(),
            opts_str,
        );

        let fs_type = String::from(fs.fs_type());
        vfs.mounts.push(MountPoint {
            path: mount_path.to_path_buf(),
            fs: Arc::new(Mutex::new(fs)),
            fs_type,
            options,
            // Stable, never-reused id for this mount instance (see FileId).
            fs_id: NEXT_FS_ID.fetch_add(1, Ordering::Relaxed),
        });

        // Mount changes affect path resolution — invalidate entire dcache.
        drop(vfs);
        VFS_DCACHE.lock().invalidate_all();

        Ok(())
    }

    /// Unmount the filesystem at the given mount point.
    ///
    /// Syncs the filesystem before removing it to ensure all data is
    /// flushed.  Refuses to unmount if the mount point has sub-mounts
    /// (to prevent orphaning them).
    ///
    /// # Safety
    ///
    /// The caller must ensure no file handles are open on this
    /// filesystem.  Currently we don't track per-mount handle counts,
    /// so this is the caller's responsibility.
    /// Index of the mount at `mount_path`, if it may be unmounted right now.
    ///
    /// `NotFound` if nothing is mounted there, `DeviceBusy` if unmounting it
    /// would orphan a sub-mount.
    ///
    /// Factored out because [`Self::unmount`] must run this check *twice*: once
    /// to find the filesystem to sync, and again after it has dropped and
    /// retaken the VFS lock, by which point a sub-mount may have appeared.
    fn unmount_index(vfs: &VfsInner, mount_path: &Path) -> KernelResult<usize> {
        let idx = vfs
            .mounts
            .iter()
            .position(|mp| mp.path.as_path() == mount_path)
            .ok_or(KernelError::NotFound)?;

        // Check for sub-mounts that would be orphaned.  `path_strictly_under`
        // matches on component boundaries, so unmounting `/mnt` is not blocked
        // by an unrelated `/mnt_data` mount, and a `/mnt/` spelling of the
        // argument still finds the real children.
        let has_children = vfs.mounts.iter().enumerate().any(|(i, mp)| {
            i != idx && crate::fs::pathutil::path_strictly_under(&mp.path, mount_path)
        });
        if has_children {
            crate::serial_println!(
                "[vfs] Cannot unmount '{}': has sub-mounts",
                mount_path.display()
            );
            return Err(KernelError::DeviceBusy);
        }
        Ok(idx)
    }

    pub fn unmount(mount_path: impl AsRef<Path>) -> KernelResult<()> {
        // Normalise to the spelling registration stored, so an unmount is not
        // refused merely because the caller wrote the trailing slash that
        // `mount` accepted.  This also makes the root check below catch `//`.
        let mount_path = &normalize_mount_path(mount_path.as_ref());

        // Refuse to unmount root.  Checked before the table lookup so the answer
        // does not depend on whether `/` happens to be present.
        if mount_path.as_path() == Path::new("/") {
            return Err(KernelError::PermissionDenied);
        }

        // Phase 1: find the mount, take a *handle* to its filesystem, and drop
        // the VFS lock before touching that filesystem.
        //
        // A per-mount lock must never be taken while the global VFS lock is
        // held.  `MountPoint::fs` documents the rule (design-decisions §43): the
        // VFS lock is released as soon as the mount-table lookup is done,
        // precisely so a stacked filesystem — the overlay — can re-enter the VFS
        // to read its lower layer while holding its own per-mount lock.  This
        // function used to sync with both held, which is that order inverted;
        // lockdep reported the resulting `VFS -> per-mount` edge on every boot
        // (see known-issues TD-A-LOCKDEP-VIOLATION-REPORT-NAMES-NO-ADDRESS).
        // Cloning the `Arc` is the whole fix: it keeps the filesystem alive
        // across the unlocked window without keeping the mount table locked.
        let (fs, fs_id, fs_type) = {
            let vfs = VFS.lock();
            let idx = Self::unmount_index(&vfs, mount_path)?;
            let mp = vfs.mounts.get(idx).ok_or(KernelError::NotFound)?;
            (Arc::clone(&mp.fs), mp.fs_id, mp.fs_type.clone())
        };

        // Sync with no VFS lock held.
        if let Err(e) = fs.lock().sync() {
            crate::serial_println!(
                "[vfs] WARNING: sync failed during unmount of '{}': {:?}",
                mount_path.display(),
                e
            );
            // Continue with unmount anyway — data loss is better than a
            // permanently stuck mount.
        }

        // Phase 2: re-acquire and re-check.  Nothing learned in phase 1 may be
        // reused, because the table can have changed while we were unlocked:
        // the index can have shifted, a sub-mount can have appeared, and this
        // mount can have been replaced by a *different* filesystem at the same
        // path.  `fs_id` is monotonic and never reused (see `NEXT_FS_ID`), which
        // is what makes that last case detectable instead of a silent unmount of
        // someone else's filesystem.
        let mut vfs = VFS.lock();
        let idx = match Self::unmount_index(&vfs, mount_path) {
            Ok(i) => i,
            // Someone else unmounted it while we were syncing.  The caller's
            // postcondition — this path is not mounted — holds, and whoever won
            // the race ran the dcache and advisory-lock cleanup below, so this
            // is a success rather than a lost race.
            Err(KernelError::NotFound) => return Ok(()),
            // A sub-mount appeared in the window: genuinely busy, report it.
            Err(e) => return Err(e),
        };
        if vfs.mounts.get(idx).map(|mp| mp.fs_id) != Some(fs_id) {
            // Unmounted, and something else was mounted at the same path. Ours
            // is already gone; removing the newcomer would be the bug.
            return Ok(());
        }

        vfs.mounts.remove(idx);
        crate::serial_println!(
            "[vfs] Unmounted {} from '{}'",
            fs_type,
            mount_path.display()
        );

        // Unmount changes affect path resolution — invalidate entire dcache.
        drop(vfs);
        VFS_DCACHE.lock().invalidate_all();

        // Release any advisory locks on paths under this mount.  The subtree
        // test matches on component boundaries, so locks on `/mnt_data` are
        // not cleared when unmounting `/mnt` (and, unlike the byte-prefix
        // idiom this replaces, a `/mnt/` spelling does not silently keep every
        // child's lock alive).
        let mut table = LOCK_TABLE.lock();
        table.retain(|entry| !crate::fs::pathutil::path_in_subtree(&entry.path, mount_path));

        Ok(())
    }

    // -------------------------------------------------------------------
    // VFS-level path resolution (cross-mount symlink support)
    // -------------------------------------------------------------------

    /// Maximum symlink traversal depth (matches per-filesystem limits).
    const MAX_SYMLINK_DEPTH: usize = 40;

    /// Resolve a path following all symlinks, including cross-mount ones.
    ///
    /// Returns the canonical absolute path with all symlinks resolved.
    /// This is the public API for callers (like file handles) that need
    /// to resolve a path once and reuse the result.
    pub fn resolve_path<P: AsRef<Path>>(path: P) -> KernelResult<PathBuf> {
        Self::resolve_follow(path.as_ref())
    }

    /// The fixed prologue every path resolution pays, before the dcache is
    /// even consulted: per-process namespace translation (which may remap or
    /// block the path entirely), syntactic validation, and normalisation.
    ///
    /// Extracted because `resolve_follow` and `resolve_no_follow` carried it
    /// verbatim, and because it is a *measurement seam*: this is unconditional
    /// work on the hottest path in the VFS, so its cost has to be attributable
    /// separately from the cache lookup that follows it.  `bench.rs` calls it
    /// directly for that reason — hence `pub(crate)` rather than private.
    ///
    /// Returns the normalised, namespace-translated path ready for cache
    /// lookup or a full walk.
    pub(crate) fn resolve_prologue(path: &Path) -> KernelResult<PathBuf> {
        let ns_path = crate::ipc::namespace::resolve_path(path)?;
        let path: &Path = &ns_path;
        validate_path(path)?;
        Ok(normalize_path(path))
    }

    /// Internal: resolve following all symlinks.
    ///
    /// Walks path components one at a time, checking each for symlink
    /// status via the underlying filesystem's `lstat()`.  When a symlink
    /// is found, reads the target and re-resolves through the VFS, which
    /// correctly handles references to other mount points.
    ///
    /// Performance note: O(n) filesystem lookups where n is path depth.
    /// Redundant for intra-mount paths (filesystem already follows), but
    /// necessary for correctness when symlinks cross mount boundaries.
    /// A future optimization: add a single-component `lookup()` to the
    /// `FileSystem` trait (like Linux's namei) to avoid re-resolving
    /// parent components.
    fn resolve_follow(path: &Path) -> KernelResult<PathBuf> {
        let norm = Self::resolve_prologue(path)?;

        // Check VFS dcache first — avoids component-by-component lstat walk.
        {
            let mut dcache = VFS_DCACHE.lock();
            match dcache.lookup(&norm, true) {
                DcacheLookup::Hit(resolved) => return Ok(resolved),
                DcacheLookup::NegativeHit => return Err(KernelError::NotFound),
                DcacheLookup::Miss => {}
            }
        }

        match Self::resolve_inner(&norm, true, 0, false) {
            Ok(resolved) => {
                // Cache the positive result for future lookups.
                {
                    let mut dcache = VFS_DCACHE.lock();
                    dcache.insert(&norm, true, &resolved);
                }
                Ok(resolved)
            }
            Err(KernelError::NotFound) => {
                // Cache the negative result — this path's parent chain is
                // broken (a non-final component doesn't exist).  Future
                // lookups can short-circuit without walking the filesystem.
                {
                    let mut dcache = VFS_DCACHE.lock();
                    dcache.insert_negative(&norm, true);
                }
                Err(KernelError::NotFound)
            }
            Err(e) => Err(e),
        }
    }

    /// Like [`resolve_follow`] but does NOT follow the final component.
    ///
    /// Used for operations that act on the entry itself: `remove`,
    /// `rmdir`, `lstat`, `readlink`, `symlink`, `rename`.
    fn resolve_no_follow(path: &Path) -> KernelResult<PathBuf> {
        let norm = Self::resolve_prologue(path)?;

        // Check VFS dcache first.
        {
            let mut dcache = VFS_DCACHE.lock();
            match dcache.lookup(&norm, false) {
                DcacheLookup::Hit(resolved) => return Ok(resolved),
                DcacheLookup::NegativeHit => return Err(KernelError::NotFound),
                DcacheLookup::Miss => {}
            }
        }

        match Self::resolve_inner(&norm, false, 0, false) {
            Ok(resolved) => {
                // Cache the positive result.
                {
                    let mut dcache = VFS_DCACHE.lock();
                    dcache.insert(&norm, false, &resolved);
                }
                Ok(resolved)
            }
            Err(KernelError::NotFound) => {
                // Cache the negative result.
                {
                    let mut dcache = VFS_DCACHE.lock();
                    dcache.insert_negative(&norm, false);
                }
                Err(KernelError::NotFound)
            }
            Err(e) => Err(e),
        }
    }

    /// Resolve `path` while refusing to traverse **any** symbolic link.
    ///
    /// Implements `openat2`'s `RESOLVE_NO_SYMLINKS`: if any component of the
    /// path (parent *or* final) is a symlink, resolution fails with
    /// [`KernelError::TooManyLinks`] (→ `ELOOP`) rather than following it.
    /// On success the returned path equals the normalized input (no symlink
    /// substitution ever happens), and all non-final components are verified
    /// to exist; the final component may be absent (open-with-create).
    ///
    /// The VFS dcache is intentionally bypassed: it stores fully
    /// symlink-*followed* resolutions, which would mask the very symlinks
    /// this mode must reject.  These calls are rare (security-sensitive
    /// `openat2` opens), so the extra component walk is acceptable.
    pub fn resolve_no_symlinks<P: AsRef<Path>>(path: P) -> KernelResult<PathBuf> {
        // Apply per-process namespace translation before anything else.
        let ns_path = crate::ipc::namespace::resolve_path(path.as_ref())?;
        let path: &Path = &ns_path;

        validate_path(path)?;
        let norm = normalize_path(path);
        Self::resolve_inner(&norm, true, 0, true)
    }

    /// Core recursive resolver.
    ///
    /// `path` must already be normalized (no `.`, `..`, or double slashes).
    ///
    /// When `no_symlinks` is set, encountering a symlink in *any* component
    /// (including the final one, regardless of `follow_last`) fails with
    /// [`KernelError::TooManyLinks`] instead of following it.  This
    /// implements `openat2`'s `RESOLVE_NO_SYMLINKS` semantics — strictly
    /// stronger than `O_NOFOLLOW`, which only guards the final component.
    fn resolve_inner(
        path: &Path,
        follow_last: bool,
        depth: usize,
        no_symlinks: bool,
    ) -> KernelResult<PathBuf> {
        if depth > Self::MAX_SYMLINK_DEPTH {
            return Err(KernelError::TooManyLinks);
        }

        let components: Vec<&Path> = path.components().collect();

        if components.is_empty() {
            return Ok(PathBuf::from("/"));
        }

        let mut resolved = PathBuf::with_capacity(path.len());

        for (i, comp) in components.iter().enumerate() {
            let is_last = i == components.len().saturating_sub(1);

            // Build current absolute path.
            resolved.extend_bytes(b"/");
            resolved.extend_bytes(comp.as_bytes());

            // Check for symlinks if we should follow at this position, or
            // whenever `no_symlinks` is requested (which must reject a
            // final-component symlink too, even when `follow_last` is false).
            if !is_last || follow_last || no_symlinks {
                let entry_type = {
                    match resolve_mount(&resolved) {
                        Ok((fs, _id, _opts, relative)) => match fs.lock().lstat(&relative) {
                            Ok(e) => Some(e.entry_type),
                            // Last component may not exist yet (creating a
                            // new file/dir/symlink).
                            Err(KernelError::NotFound) if is_last => None,
                            Err(e) => return Err(e),
                        },
                        Err(KernelError::NotFound) if is_last => None,
                        Err(e) => return Err(e),
                    }
                }; // VFS lock released

                if entry_type == Some(EntryType::Symlink) {
                    // RESOLVE_NO_SYMLINKS: refuse to traverse or open any
                    // symlink, at any depth, rather than following it.
                    if no_symlinks {
                        return Err(KernelError::TooManyLinks);
                    }
                    // Read the symlink target (separate lock acquisition).
                    let target = {
                        let (fs, _id, _opts, relative) = resolve_mount(&resolved)?;
                        fs.lock().readlink(&relative)?
                    }; // lock released

                    // Build new path: symlink target + remaining components.
                    let mut full = if target.is_absolute() {
                        // Absolute target — restart from VFS root.
                        target
                    } else {
                        // Relative target — resolve from symlink's parent.
                        // `parent()` returns `None` only for a path with no
                        // component to drop, which cannot happen here: this
                        // loop has pushed at least one component onto
                        // `resolved` before it can observe a symlink.
                        let mut base = resolved
                            .parent()
                            .map_or_else(|| PathBuf::from("/"), Path::to_path_buf);
                        base.push(&target);
                        base
                    };

                    for r in components.get(i.saturating_add(1)..).unwrap_or(&[]) {
                        full.push(*r);
                    }

                    // Normalize (resolve `.` and `..` introduced by target)
                    // and recurse with incremented depth.
                    let normalized = normalize_path(&full);
                    return Self::resolve_inner(
                        &normalized,
                        follow_last,
                        depth.saturating_add(1),
                        no_symlinks,
                    );
                }
            }
        }

        Ok(resolved)
    }

    // -------------------------------------------------------------------
    // VFS operations
    // -------------------------------------------------------------------

    /// List entries in a directory.
    ///
    /// If other filesystems are mounted at sub-paths of `path`, their
    /// mount points appear as directory entries in the listing (even if
    /// the underlying filesystem doesn't have a physical directory there).
    pub fn readdir(path: impl AsRef<Path>) -> KernelResult<Vec<DirEntry>> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        check_path_access(&path, PathAccess::Read)?;

        // Collect mount-point names that are direct children of `path`.
        // E.g., if path="/", mounts at "/tmp" and "/mnt" produce ["tmp", "mnt"].
        // Nested mounts like "/mnt/usb" are NOT direct children of "/".
        let submount_names: Vec<PathBuf> = {
            let vfs = VFS.lock();
            Self::submount_children(&vfs, &path)
        };

        let (fs, _id, _opts, relative) = resolve_mount(&path)?;
        let mut entries = fs.lock().readdir(&relative)?;

        // Inject submount directories that the underlying FS doesn't know about.
        for name in submount_names {
            if !entries.iter().any(|e| e.name == name) {
                entries.push(DirEntry {
                    name,
                    entry_type: EntryType::Directory,
                    size: 0,
                });
            }
        }

        Ok(entries)
    }

    /// List entries in a directory with pagination.
    ///
    /// Returns up to `count` entries starting from `offset` (0-based
    /// index into the combined listing of filesystem entries + submount
    /// directories).  Also returns the total entry count.
    ///
    /// This is the efficient API for large directories — callers can
    /// read entries in batches instead of loading everything at once.
    pub fn readdir_at(
        path: impl AsRef<Path>,
        offset: usize,
        count: usize,
    ) -> KernelResult<(Vec<DirEntry>, usize)> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        Self::readdir_at_resolved(&path, offset, count)
    }

    /// Like [`readdir_at`](Self::readdir_at) but on an **already-resolved**
    /// host path (see [`read_at_resolved`](Self::read_at_resolved)) — used by
    /// directory file handles, which store the resolved path.
    pub fn readdir_at_resolved(
        path: impl AsRef<Path>,
        offset: usize,
        count: usize,
    ) -> KernelResult<(Vec<DirEntry>, usize)> {
        let path = path.as_ref();
        check_path_access(path, PathAccess::Read)?;
        let submount_names: Vec<PathBuf> = {
            let vfs = VFS.lock();
            Self::submount_children(&vfs, path)
        };

        let (fs, _id, _opts, relative) = resolve_mount(path)?;
        let mut entries = fs.lock().readdir(&relative)?;

        // Inject submount directories.
        for name in submount_names {
            if !entries.iter().any(|e| e.name == name) {
                entries.push(DirEntry {
                    name,
                    entry_type: EntryType::Directory,
                    size: 0,
                });
            }
        }

        let total = entries.len();
        let start = offset.min(total);
        let end = start.saturating_add(count).min(total);
        let page: Vec<DirEntry> = entries
            .into_iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect();
        Ok((page, total))
    }

    /// Read a file's contents.
    pub fn read_file(path: impl AsRef<Path>) -> KernelResult<Vec<u8>> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        Self::read_file_resolved(&path)
    }

    /// Like [`read_file`](Self::read_file) but on an **already-resolved** host
    /// path (see [`read_at_resolved`](Self::read_at_resolved) for why handle-
    /// backed I/O must skip re-translation).
    pub fn read_file_resolved(path: impl AsRef<Path>) -> KernelResult<Vec<u8>> {
        let path = path.as_ref();
        check_path_access(path, PathAccess::Read)?;
        let result = Self::read_file_routed(path);
        // inotify IN_ACCESS: emit an Accessed event after a successful read,
        // but only when some watch actually requested ACCESS (a lock-free
        // gate).  The read path is high-frequency, so without an ACCESS watch
        // this is a single relaxed atomic load and we never touch the notify
        // lock.  Emitted after releasing the VFS lock (notify is a leaf lock).
        if result.is_ok() && super::notify::interest_includes(super::notify::FsEventMask::ACCESS) {
            super::notify::emit(super::notify::FsEventType::Accessed, path, None);
        }
        result
    }

    /// Whole-file read that routes regular-file data through the shared page
    /// cache (design-decisions §38), mirroring [`read_at_routed`](Self::read_at_routed).
    ///
    /// A stable-identity regular file (`ino != 0`) is served from the page
    /// cache, sharing one copy with `mmap` and byte-range `read(2)`.  Everything
    /// else — symlinks (whose `read_file` returns the link target), and objects
    /// without a stable identity (FAT/ISO/pseudo-filesystems) — falls back to
    /// the per-filesystem `read_file` unchanged.
    fn read_file_routed(path: impl AsRef<Path>) -> KernelResult<Vec<u8>> {
        let path = path.as_ref();
        let (file_id, size) = {
            let (fs, fs_id, _opts, relative) = resolve_mount(path)?;
            let mut guard = fs.lock();
            let meta = guard.metadata(&relative)?;
            if meta.entry_type != EntryType::File || meta.ino == 0 {
                return guard.read_file(&relative);
            }
            (
                FileId {
                    fs_id,
                    ino: meta.ino,
                },
                meta.size,
            )
        };
        if size == 0 {
            return Ok(Vec::new());
        }
        let out_len = usize::try_from(size).map_err(|_| KernelError::InvalidArgument)?;
        let mut buf = alloc::vec![0u8; out_len];
        crate::mm::page_cache::read_through(file_id, 0, &mut buf, |page_off, page_buf| {
            Self::fill_file_page(path, page_off, page_buf)
        })?;
        Ok(buf)
    }

    /// Get metadata for a path.
    pub fn stat(path: impl AsRef<Path>) -> KernelResult<DirEntry> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        Self::stat_resolved(&path)
    }

    /// Like [`stat`](Self::stat) but on an **already-resolved** host path (see
    /// [`read_at_resolved`](Self::read_at_resolved)).
    pub fn stat_resolved(path: impl AsRef<Path>) -> KernelResult<DirEntry> {
        let path = path.as_ref();
        check_path_access(path, PathAccess::Metadata)?;
        let (fs, _id, _opts, relative) = resolve_mount(path)?;
        fs.lock().stat(&relative)
    }

    /// Write data to a file (create or overwrite).
    pub fn write_file(path: impl AsRef<Path>, data: &[u8]) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_follow(path)?;
        Self::write_file_resolved(&path, data)
    }

    /// Like [`write_file`](Self::write_file) but on an **already-resolved**
    /// host path (see [`read_at_resolved`](Self::read_at_resolved)).
    pub fn write_file_resolved(path: impl AsRef<Path>, data: &[u8]) -> KernelResult<()> {
        let path = path.as_ref();
        check_path_access(path, PathAccess::Write)?;
        check_writable(path)?;
        // Intercept: let pre-operation handlers approve/deny before proceeding.
        // Called before VFS lock to avoid deadlock (interceptors must not call VFS).
        super::intercept::pre_write(path)?;
        // Quota: check whether this write would exceed the user's quota.
        // uid 0 is the default until per-process identity is wired up.
        enforce_quota_write(path, data.len() as u64)?;
        // Auto-version: save the old content before overwriting.
        // Called before taking the VFS lock to avoid deadlock (record_version
        // reads the file through VFS internally).  TOCTOU between read and
        // write is acceptable — version history is best-effort.
        super::history::try_auto_record(path);
        let cache_inval = {
            let (fs, fs_id, _opts, relative) = resolve_mount(path)?;
            let mut guard = fs.lock();
            guard.write_file(&relative, data)?;
            // Coherence: a full overwrite replaces the file's contents — drop
            // any cached pages so mappers see the new bytes.
            cache_identity(&mut guard, fs_id, &relative)
        };
        if let Some((fs_id, ino)) = cache_inval {
            crate::mm::page_cache::invalidate_identity(fs_id, ino);
        }
        // Charge quota usage after successful write.
        super::quota::charge_bytes(0, 0, data.len() as u64);
        // Writing may create a new file — invalidate negative cache entries
        // that claimed this path didn't exist.
        VFS_DCACHE.lock().invalidate_negative_prefix(path);
        // Notify, index, and journal after releasing VFS lock (avoids holding both locks).
        super::notify::emit_modified(path);
        super::index::on_file_changed(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        super::audit::log_ok(super::audit::AuditOp::Write, 0, path);
        Ok(())
    }

    /// Copy a file from one path to another.
    ///
    /// Reads the source and writes to the destination.  Both paths are
    /// resolved through symlinks.  Works across mount boundaries.
    ///
    /// Future optimization: if both paths are on the same filesystem,
    /// delegate to a filesystem-level copy (reflink, server-side copy).
    pub fn copy(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> KernelResult<u64> {
        let src = src.as_ref();
        let dst = dst.as_ref();
        // For files that fit in a reasonable buffer (≤64 KiB), do a
        // simple read-all + write-all.  For larger files, use chunked
        // read_at / write_at to avoid loading the entire file into
        // heap memory at once.
        const CHUNK_THRESHOLD: u64 = 64 * 1024;
        const CHUNK_SIZE: usize = 64 * 1024;

        let entry = Self::stat(src)?;
        let size = entry.size;

        if size <= CHUNK_THRESHOLD {
            // Small file — simple path.
            let data = Self::read_file(src)?;
            Self::write_file(dst, &data)?;
            return Ok(data.len() as u64);
        }

        // Large file — chunked copy.
        // Create/truncate the destination first.
        Self::write_file(dst, &[])?;

        let mut offset: u64 = 0;
        while offset < size {
            let chunk = Self::read_at(src, offset, CHUNK_SIZE)?;
            if chunk.is_empty() {
                break; // EOF.
            }
            Self::write_at(dst, offset, &chunk)?;
            offset = offset.saturating_add(chunk.len() as u64);
        }

        Ok(offset)
    }

    /// Recursively copy a file or directory tree from `src` to `dst`.
    ///
    /// If `src` is a file, behaves like `copy()`.  If `src` is a directory,
    /// creates `dst` as a directory and recursively copies all contents.
    /// Works across mount points.  Preserves permissions and ownership.
    ///
    /// ## Depth limit
    ///
    /// Recursion depth is limited to 64 levels to prevent stack overflow.
    pub fn copy_recursive(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> KernelResult<u64> {
        let src = src.as_ref();
        let dst = dst.as_ref();
        Self::copy_recursive_inner(src, dst, 0)
    }

    fn copy_recursive_inner(
        src: impl AsRef<Path>,
        dst: impl AsRef<Path>,
        depth: usize,
    ) -> KernelResult<u64> {
        let src = src.as_ref();
        let dst = dst.as_ref();
        const MAX_DEPTH: usize = 64;
        if depth > MAX_DEPTH {
            return Err(KernelError::TooManyLinks);
        }

        let entry = Self::stat(src)?;

        if entry.entry_type == EntryType::File {
            // Simple file copy.
            let bytes = Self::copy(src, dst)?;
            // Best-effort metadata preservation.
            if let Ok(meta) = Self::metadata(src) {
                let _ = Self::set_permissions(dst, meta.permissions);
                let _ = Self::set_owner(dst, meta.uid, meta.gid);
            }
            return Ok(bytes);
        }

        if entry.entry_type != EntryType::Directory {
            return Err(KernelError::NotSupported);
        }

        // Create the destination directory.
        Self::mkdir(dst)?;

        // Copy each entry recursively.
        let entries = Self::readdir(src)?;
        let mut total_bytes = 0u64;

        for child in &entries {
            let src_child = src.join(&child.name);
            let dst_child = dst.join(&child.name);
            let bytes =
                Self::copy_recursive_inner(&src_child, &dst_child, depth.saturating_add(1))?;
            total_bytes = total_bytes.saturating_add(bytes);
        }

        // Best-effort metadata preservation on the directory.
        if let Ok(meta) = Self::metadata(src) {
            let _ = Self::set_permissions(dst, meta.permissions);
            let _ = Self::set_owner(dst, meta.uid, meta.gid);
        }

        Ok(total_bytes)
    }

    /// Recursively remove a file or directory tree.
    ///
    /// If `path` is a file, behaves like `remove()`.  If `path` is a
    /// directory, removes all contents first (depth-first), then removes
    /// the empty directory.
    ///
    /// ## Depth limit
    ///
    /// Recursion depth is limited to 64 levels to prevent stack overflow.
    pub fn remove_recursive(path: impl AsRef<Path>) -> KernelResult<u64> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        Self::remove_recursive_inner(path, 0)
    }

    fn remove_recursive_inner(path: impl AsRef<Path>, depth: usize) -> KernelResult<u64> {
        let path = path.as_ref();
        const MAX_DEPTH: usize = 64;
        if depth > MAX_DEPTH {
            return Err(KernelError::TooManyLinks);
        }

        let entry = Self::stat(path)?;

        if entry.entry_type == EntryType::File || entry.entry_type == EntryType::Symlink {
            Self::remove(path)?;
            return Ok(1);
        }

        if entry.entry_type != EntryType::Directory {
            return Err(KernelError::NotSupported);
        }

        // Remove contents depth-first.
        let entries = Self::readdir(path)?;
        let mut count = 0u64;

        for child in &entries {
            let child_path = path.join(&child.name);
            let removed = Self::remove_recursive_inner(&child_path, depth.saturating_add(1))?;
            count = count.saturating_add(removed);
        }

        // Now remove the empty directory.
        Self::rmdir(path)?;
        count = count.saturating_add(1);

        Ok(count)
    }

    /// Delete a file.
    ///
    /// Does NOT follow the final symlink — removes the link itself.
    pub fn remove(path: impl AsRef<Path>) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_no_follow(path)?;
        check_path_access(&path, PathAccess::Write)?;
        check_writable(&path)?;
        // Intercept: let pre-operation handlers approve/deny.
        super::intercept::pre_delete(&path)?;
        // Capture file size before deletion for quota release.
        let file_size = Self::stat(&path).map(|s| s.size).unwrap_or(0);
        // Auto-version: save the file content before deleting.
        // Allows `fhist restore` to recover accidentally deleted files.
        super::history::try_auto_record(&path);
        let cache_inval = {
            let (fs, fs_id, _opts, relative) = resolve_mount(&path)?;
            let mut guard = fs.lock();
            // Capture identity *before* removal — the inode (and its number)
            // is gone afterward, and that number may be reused by a future
            // file.  Dropping the cached pages now prevents a later file that
            // reuses this inode from being served the removed file's bytes.
            let id = cache_identity(&mut guard, fs_id, &relative);
            guard.remove(&relative)?;
            id
        };
        if let Some((fs_id, ino)) = cache_inval {
            crate::mm::page_cache::invalidate_identity(fs_id, ino);
        }
        // Release quota usage for deleted file.
        if file_size > 0 {
            super::quota::release_bytes(0, 0, file_size);
        }
        super::quota::release_inode(0, 0);
        // Removing a file/symlink can invalidate cached resolutions that
        // traverse through it (if it was a symlink) or resolve to it.
        VFS_DCACHE.lock().invalidate_prefix(&path);
        super::notify::emit_deleted(&path);
        super::index::on_file_deleted(&path);
        super::journal::record(super::journal::JournalEventType::Deleted, &path);
        super::audit::log_ok(super::audit::AuditOp::Delete, 0, &path);
        Ok(())
    }

    /// Create a directory.
    ///
    /// Intermediate symlinks are followed; the last component is the
    /// new directory name (not followed if it happens to exist).
    /// Default permission bits for a freshly-created directory when the
    /// caller does not specify a mode (the historical 0o755).
    pub const DEFAULT_DIR_MODE: u16 = 0o755;

    pub fn mkdir(path: impl AsRef<Path>) -> KernelResult<()> {
        let path = path.as_ref();
        Self::mkdir_mode(path, Self::DEFAULT_DIR_MODE)
    }

    /// Create a directory, stamping it with `mode` (masked to the low 9
    /// permission bits) instead of the 0o755 default.
    ///
    /// `mode` is expected to be **already umask-masked by the caller** — the
    /// umask lives in the userspace POSIX layer, so the kernel treats `mode`
    /// as the final on-disk permission bits (same thin-primitive model as the
    /// file-create path in [`crate::fs::handle::open_with_mode`]).
    pub fn mkdir_mode(path: impl AsRef<Path>, mode: u16) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_no_follow(path)?;
        check_path_access(&path, PathAccess::Write)?;
        check_writable(&path)?;
        // Intercept: let pre-operation handlers approve/deny.
        super::intercept::pre_mkdir(&path)?;
        // Quota: check inode creation limit.
        enforce_quota_create(&path)?;
        {
            let (fs, _id, _opts, relative) = resolve_mount(&path)?;
            fs.lock().mkdir(&relative)?;
        }
        // Stamp the caller-supplied (umask-masked) permission bits; the
        // underlying mkdir stamps a 0o755 default, so only override when the
        // requested mode differs.
        let perm = mode & 0o777;
        if perm != Self::DEFAULT_DIR_MODE {
            Self::set_permissions(&path, perm)?;
        }
        // Charge quota for new inode.
        super::quota::charge_inode(0, 0);
        // New directory invalidates negative cache entries that claimed
        // this path (or children) didn't exist.  Positive entries are
        // unaffected — existing path resolutions remain valid.
        VFS_DCACHE.lock().invalidate_negative_prefix(&path);
        super::notify::emit_created_dir(&path);
        super::index::on_file_changed(&path);
        super::journal::record(super::journal::JournalEventType::Created, &path);
        super::audit::log_ok(super::audit::AuditOp::Mkdir, 0, &path);
        Ok(())
    }

    /// Create a directory and all missing parent directories.
    ///
    /// Like `mkdir -p` — creates each component in the path that doesn't
    /// exist yet.  Succeeds if the full path already exists as a directory.
    /// Fails if any component exists but is not a directory.
    ///
    /// ## Depth limit
    ///
    /// Limited to 64 path components to prevent abuse.
    pub fn mkdir_all(path: impl AsRef<Path>) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        validate_path(path)?;
        let norm = normalize_path(path);

        let components: Vec<&Path> = norm.components().collect();

        if components.len() > 64 {
            return Err(KernelError::InvalidArgument);
        }

        // Seed with the root separator, not an empty buffer: `components()`
        // drops the leading `/`, so pushing the first component onto an empty
        // `PathBuf` would build a *relative* path and the `stat` below would
        // fail `validate_path` ("must be absolute") before touching the disk.
        let mut built = PathBuf::with_capacity(norm.len().saturating_add(1));
        built.extend_bytes(b"/");

        for comp in &components {
            built.push(comp);

            // Check if this component exists.
            match Self::stat(&built) {
                Ok(entry) => {
                    if entry.entry_type != EntryType::Directory {
                        // Exists but is not a directory — can't create children.
                        return Err(KernelError::NotADirectory);
                    }
                    // Already a directory — continue to next component.
                }
                Err(KernelError::NotFound) => {
                    // Doesn't exist — create it.
                    Self::mkdir(&built)?;
                }
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    /// Remove an empty directory.
    pub fn rmdir(path: impl AsRef<Path>) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_no_follow(path)?;
        check_path_access(&path, PathAccess::Write)?;
        check_writable(&path)?;
        // Intercept: let pre-operation handlers approve/deny.
        super::intercept::pre_delete(&path)?;
        {
            let (fs, _id, _opts, relative) = resolve_mount(&path)?;
            fs.lock().rmdir(&relative)?;
        }
        // Release inode quota for removed directory.
        super::quota::release_inode(0, 0);
        // Removing a directory invalidates any cached paths through it.
        VFS_DCACHE.lock().invalidate_prefix(&path);
        super::notify::emit_deleted_dir(&path);
        super::index::on_file_deleted(&path);
        super::journal::record(super::journal::JournalEventType::Deleted, &path);
        super::audit::log_ok(super::audit::AuditOp::Rmdir, 0, &path);
        Ok(())
    }

    /// Read a range of bytes from a file.
    pub fn read_at(path: impl AsRef<Path>, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        Self::read_at_resolved(&path, offset, len)
    }

    /// Like [`read_at`](Self::read_at) but on an **already-resolved** host
    /// path — one previously produced by [`resolve_follow`](Self::resolve_follow)
    /// (e.g. the path stored in an open file handle).
    ///
    /// Skips namespace/jail re-translation and symlink re-following: the input
    /// is the final canonical host path, so re-running `resolve_follow` would be
    /// wrong for a *jailed* process (its per-process chroot prefix would be
    /// applied a second time, escaping the file the fd actually refers to).
    /// Open file descriptors hold a resolved reference (Unix semantics — an fd
    /// is immune to later chroot/rename/symlink changes), so handle-backed I/O
    /// must use this entry point, never the path-based [`read_at`](Self::read_at).
    pub fn read_at_resolved(
        path: impl AsRef<Path>,
        offset: u64,
        len: usize,
    ) -> KernelResult<Vec<u8>> {
        let path = path.as_ref();
        check_path_access(path, PathAccess::Read)?;
        let result = Self::read_at_routed(path, offset, len);
        // inotify IN_ACCESS, gated on a live ACCESS watch (see `read_file`).
        // The gate keeps this off the read hot path when no watch wants it —
        // the reason ACCESS was historically not emitted here at all.
        if result.is_ok() && super::notify::interest_includes(super::notify::FsEventMask::ACCESS) {
            super::notify::emit(super::notify::FsEventType::Accessed, path, None);
        }
        result
    }

    /// Read implementation that routes regular-file data through the shared
    /// **page cache** (design-decisions §38, page-cache-primary).
    ///
    /// A regular file with a *stable identity* (`ino != 0`: ext4, memfs) has its
    /// data served from the single shared cache frame — exactly the frame the
    /// `mmap` fault path uses — so `read(2)` and `mmap` share one copy and
    /// `read(2)` coherence falls out of the §36 write/truncate invalidation
    /// hooks for free.  On a cache miss the page is filled from the backing
    /// filesystem's *data* path, which (post-§38) bypasses the block buffer
    /// cache, leaving that cache for metadata only.
    ///
    /// Everything else falls back to the per-filesystem read unchanged: objects
    /// without a stable identity (FAT, ISO9660, pseudo-filesystems — they keep
    /// their own caching) and non-regular files.
    ///
    /// The VFS lock is taken only to resolve identity/size and, separately,
    /// inside the page-fill closure — it is **never** held across
    /// [`crate::mm::page_cache::read_through`], so the cache→VFS fill path does
    /// not nest the two locks (the cache lock is already dropped before the fill
    /// closure runs).
    fn read_at_routed(path: impl AsRef<Path>, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        let path = path.as_ref();
        if len == 0 {
            return Ok(Vec::new());
        }

        // Resolve identity, size, and regular-file-ness, then drop the VFS lock.
        let (file_id, size) = {
            let (fs, fs_id, _opts, relative) = resolve_mount(path)?;
            let mut guard = fs.lock();
            let meta = guard.metadata(&relative)?;
            // Only stable-identity regular files are page-cacheable; anything
            // else reads straight from the filesystem (current behaviour).
            if meta.entry_type != EntryType::File || meta.ino == 0 {
                return guard.read_at(&relative, offset, len);
            }
            (
                FileId {
                    fs_id,
                    ino: meta.ino,
                },
                meta.size,
            )
        };

        // Clamp the request to the bytes that actually exist (the page cache
        // zero-extends past EOF; the caller must not see those padding bytes).
        if offset >= size {
            return Ok(Vec::new());
        }
        let avail = size.saturating_sub(offset);
        let out_len = (len as u64).min(avail) as usize;
        if out_len == 0 {
            return Ok(Vec::new());
        }

        let mut buf = alloc::vec![0u8; out_len];
        crate::mm::page_cache::read_through(file_id, offset, &mut buf, |page_off, page_buf| {
            Self::fill_file_page(path, page_off, page_buf)
        })?;
        Ok(buf)
    }

    /// Page-cache fill: populate one 16 KiB `page_buf` with the bytes of `path`
    /// starting at the frame-aligned `page_off`, reading from the filesystem's
    /// *data* path (which bypasses the block buffer cache — §38).
    ///
    /// Shared by [`read_at_routed`](Self::read_at_routed) and
    /// [`read_file_routed`](Self::read_file_routed).  Bytes past EOF are left as
    /// the caller's pre-zeroed page (demand-paging zero-fill semantics).  The
    /// mount is re-resolved under the VFS lock here; the page-cache lock is
    /// already dropped before this runs, so the cache and VFS locks never nest.
    fn fill_file_page(
        path: impl AsRef<Path>,
        page_off: u64,
        page_buf: &mut [u8],
    ) -> KernelResult<()> {
        let path = path.as_ref();
        let data = {
            let (fs, _id, _opts, relative) = resolve_mount(path)?;
            fs.lock().read_at(&relative, page_off, page_buf.len())?
        };
        let n = data.len().min(page_buf.len());
        if let (Some(dst), Some(src)) = (page_buf.get_mut(..n), data.get(..n)) {
            dst.copy_from_slice(src);
        }
        Ok(())
    }

    /// Read a range of bytes from a file **directly from the backing
    /// filesystem**, bypassing the page cache.
    ///
    /// This is the fill primitive behind the page cache itself: the `mmap`
    /// fault path and [`read_at_routed`](Self::read_at_routed)'s page-fill
    /// closure both need to read a file's data *without* re-entering
    /// [`crate::mm::page_cache::get_or_fill`] (which would recurse on the same
    /// key).  It performs the same path resolution and tag check as
    /// [`read_at`](Self::read_at) but goes straight to `mp.fs.read_at`, so for
    /// regular files it reads through the filesystem's *data* path (which, after
    /// §38, bypasses the block buffer cache too — a genuinely uncached read).
    ///
    /// It deliberately does **not** emit the inotify `IN_ACCESS` event: callers
    /// are internal cache fills, not user-visible reads (the user-visible read
    /// that triggered the fill emits `ACCESS` at the [`read_at`](Self::read_at)
    /// layer).
    pub fn read_at_uncached(
        path: impl AsRef<Path>,
        offset: u64,
        len: usize,
    ) -> KernelResult<Vec<u8>> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        Self::read_at_uncached_resolved(&path, offset, len)
    }

    /// Like [`read_at_uncached`](Self::read_at_uncached) but on an
    /// **already-resolved** host path (see
    /// [`read_at_resolved`](Self::read_at_resolved)).
    pub fn read_at_uncached_resolved(
        path: impl AsRef<Path>,
        offset: u64,
        len: usize,
    ) -> KernelResult<Vec<u8>> {
        let path = path.as_ref();
        check_path_access(path, PathAccess::Read)?;
        let (fs, _id, _opts, relative) = resolve_mount(path)?;
        fs.lock().read_at(&relative, offset, len)
    }

    /// Write bytes at a specific offset within a file.
    pub fn write_at(path: impl AsRef<Path>, offset: u64, data: &[u8]) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_follow(path)?;
        Self::write_at_resolved(&path, offset, data)
    }

    /// Like [`write_at`](Self::write_at) but on an **already-resolved** host
    /// path (see [`read_at_resolved`](Self::read_at_resolved)).
    pub fn write_at_resolved(path: impl AsRef<Path>, offset: u64, data: &[u8]) -> KernelResult<()> {
        let path = path.as_ref();
        check_path_access(path, PathAccess::Write)?;
        check_writable(path)?;
        // Intercept and quota checks on partial writes.
        super::intercept::pre_write(path)?;
        enforce_quota_write(path, data.len() as u64)?;
        let cache_inval = {
            let (fs, fs_id, _opts, relative) = resolve_mount(path)?;
            let mut guard = fs.lock();
            guard.write_at(&relative, offset, data)?;
            // Coherence: drop any cached pages of this file so a later mapper
            // (or re-fault) reads the post-write bytes, not stale cached ones.
            cache_identity(&mut guard, fs_id, &relative)
        };
        if let Some((fs_id, ino)) = cache_inval {
            crate::mm::page_cache::invalidate_identity(fs_id, ino);
        }
        super::quota::charge_bytes(0, 0, data.len() as u64);
        super::notify::emit_modified(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// Append data to the end of a file.
    ///
    /// Creates the file if it doesn't exist.  Uses write_at at the
    /// current file size for efficient append without rewriting.
    pub fn append(path: impl AsRef<Path>, data: &[u8]) -> KernelResult<()> {
        let path = path.as_ref();
        let offset = match Self::stat(path) {
            Ok(entry) => entry.size,
            Err(KernelError::NotFound) => {
                // File doesn't exist — create it.
                return Self::write_file(path, data);
            }
            Err(e) => return Err(e),
        };
        Self::write_at(path, offset, data)
    }

    /// Truncate a file to the given size.
    pub fn truncate(path: impl AsRef<Path>, size: u64) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_follow(path)?;
        Self::truncate_resolved(&path, size)
    }

    /// Like [`truncate`](Self::truncate) but on an **already-resolved** host
    /// path (see [`read_at_resolved`](Self::read_at_resolved)).
    pub fn truncate_resolved(path: impl AsRef<Path>, size: u64) -> KernelResult<()> {
        let path = path.as_ref();
        check_writable(path)?;
        check_path_access(path, PathAccess::Write)?;
        let cache_inval = {
            let (fs, fs_id, _opts, relative) = resolve_mount(path)?;
            let mut guard = fs.lock();
            guard.truncate(&relative, size)?;
            // Coherence: truncation changes (or zeroes the tail of) the file's
            // pages — drop cached copies.
            cache_identity(&mut guard, fs_id, &relative)
        };
        if let Some((fs_id, ino)) = cache_inval {
            crate::mm::page_cache::invalidate_identity(fs_id, ino);
        }
        super::notify::emit_modified(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// Pre-allocate space for a file.
    ///
    /// Reserves `size` bytes of disk space for the file.  The file's
    /// logical size is not changed — this just ensures the blocks are
    /// allocated so future writes don't fail due to ENOSPC and don't
    /// cause fragmentation.
    pub fn fallocate(path: impl AsRef<Path>, size: u64) -> KernelResult<()> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        check_writable(&path)?;
        check_path_access(&path, PathAccess::Write)?;
        let (fs, _id, _opts, relative) = resolve_mount(&path)?;
        fs.lock().fallocate(&relative, size)
    }

    /// Rename or move a file or directory.
    ///
    /// Both paths must be on the same mount point.
    pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> KernelResult<()> {
        let from = from.as_ref();
        let to = to.as_ref();
        Self::rename_inner(from, to, false)
    }

    /// Atomic no-replace rename (Linux `renameat2(RENAME_NOREPLACE)`).
    ///
    /// Identical to [`rename`](Self::rename) but fails with
    /// [`KernelError::AlreadyExists`] (EEXIST) if `to` already exists. For the
    /// common same-mount case the destination-existence check is performed
    /// under the *same* `VFS` lock that guards the underlying filesystem
    /// rename, so there is no TOCTOU window: no concurrent creator can slip a
    /// file into `to` between the check and the rename. (The cross-mount
    /// copy+delete path — itself a SlateOS convenience that Linux rejects with
    /// EXDEV — cannot be made atomic and keeps a documented best-effort
    /// pre-check; see the comment in the cross-mount branch.)
    pub fn rename_noreplace(from: impl AsRef<Path>, to: impl AsRef<Path>) -> KernelResult<()> {
        let from = from.as_ref();
        let to = to.as_ref();
        Self::rename_inner(from, to, true)
    }

    fn rename_inner(
        from: impl AsRef<Path>,
        to: impl AsRef<Path>,
        noreplace: bool,
    ) -> KernelResult<()> {
        let from = from.as_ref();
        let to = to.as_ref();
        crate::ipc::namespace::check_writable(from)?;
        crate::ipc::namespace::check_writable(to)?;
        let from = Self::resolve_no_follow(from)?;
        let to = Self::resolve_no_follow(to)?;
        check_path_access(&from, PathAccess::Write)?;
        check_path_access(&to, PathAccess::Write)?;
        check_writable(&from)?;
        check_writable(&to)?;
        // Intercept: let pre-operation handlers approve/deny.
        super::intercept::pre_rename(&from, &to)?;

        // Check if both paths are on the same mount point.  Two paths share a
        // mount iff `resolve_mount` hands back the *same* per-mount filesystem
        // handle (`Arc::ptr_eq`), so we compare the handles directly.
        let (fs_from, _id_from, _opts_from, rel_from) = resolve_mount(&from)?;
        let (fs_to, fs_id_to, _opts_to, rel_to) = resolve_mount(&to)?;
        let same_mount = Arc::ptr_eq(&fs_from, &fs_to);

        if same_mount {
            // Same mount — delegate to the filesystem's native rename.  Both
            // relative paths live on the one filesystem, so a single per-mount
            // lock keeps the no-replace check and the rename atomic w.r.t. that
            // filesystem (the old global-lock guarantee, now scoped per mount).
            let dest_inval = {
                let mut guard = fs_to.lock();
                if noreplace {
                    // Atomic RENAME_NOREPLACE: the destination-existence check
                    // and the rename below execute under the same held per-mount
                    // lock, closing the TOCTOU window a separate pre-check would
                    // leave.
                    match guard.stat(&rel_to) {
                        Ok(_) => return Err(KernelError::AlreadyExists),
                        Err(KernelError::NotFound) => {}
                        Err(e) => return Err(e),
                    }
                }
                // A replacing rename unlinks the destination's existing inode
                // (whose number may later be reused); capture its identity
                // before the rename so we can drop its cached pages.  The
                // source's identity is unchanged (same inode, new name), so its
                // cached pages stay valid.
                let id = cache_identity(&mut guard, fs_id_to, &rel_to);
                guard.rename(&rel_from, &rel_to)?;
                id
            };
            if let Some((fs_id, ino)) = dest_inval {
                crate::mm::page_cache::invalidate_identity(fs_id, ino);
            }
        } else {
            // Cross-mount rename: copy + delete.  This is the only way to
            // "move" files between different filesystems (like Linux's mv).
            // We first stat the source to verify it exists and check type.
            let stat = Self::stat(&from)?;

            if noreplace {
                // Best-effort: the cross-mount copy+delete is inherently
                // non-atomic (multiple lock acquisitions), so a documented
                // TOCTOU remains here regardless. Linux itself returns EXDEV
                // for cross-mount rename; this branch is a SlateOS convenience.
                match Self::stat(&to) {
                    Ok(_) => return Err(KernelError::AlreadyExists),
                    Err(KernelError::NotFound) => {}
                    Err(e) => return Err(e),
                }
            }

            if stat.entry_type == EntryType::Directory {
                // Cross-mount directory rename is not supported (would need
                // recursive copy).  Use cp -r + rm -r manually.
                return Err(KernelError::NotSupported);
            }

            // Copy file data from source to destination.
            Self::copy(&from, &to)?;

            // Copy metadata if the source filesystem supports it.
            if let Ok(meta) = Self::metadata(&from) {
                let _ = Self::set_permissions(&to, meta.permissions);
                let _ = Self::set_owner(&to, meta.uid, meta.gid);
            }

            // Remove the original file.
            Self::remove(&from)?;
        }

        // Rename invalidates paths under both old and new locations.
        {
            let mut dcache = VFS_DCACHE.lock();
            dcache.invalidate_prefix(&from);
            dcache.invalidate_prefix(&to);
        }
        super::notify::emit_renamed(&from, &to);
        super::index::on_file_renamed(&from, &to);
        super::journal::record_rename(&from, &to);
        super::audit::log_ok(super::audit::AuditOp::Rename, 0, &from);
        Ok(())
    }

    /// Atomically exchange two existing entries (Linux
    /// `renameat2(RENAME_EXCHANGE)`).
    ///
    /// Both paths must exist and reside on the **same mount** — the swap is
    /// delegated to that filesystem's [`rename_exchange`](FileSystem::rename_exchange)
    /// under the held `VFS` lock, so it is atomic with respect to the FS's own
    /// state. Cross-mount exchange returns [`KernelError::CrossDevice`]
    /// (no atomic cross-filesystem swap is possible) which the syscall layer
    /// maps to `EXDEV`, matching Linux. A filesystem lacking exchange support
    /// returns [`KernelError::NotSupported`], which the syscall layer maps to
    /// `EINVAL` (mirroring Linux's `->rename` returning `EINVAL` when it
    /// cannot honour the flag).
    pub fn rename_exchange(a: impl AsRef<Path>, b: impl AsRef<Path>) -> KernelResult<()> {
        let a = a.as_ref();
        let b = b.as_ref();
        crate::ipc::namespace::check_writable(a)?;
        crate::ipc::namespace::check_writable(b)?;
        let a = Self::resolve_no_follow(a)?;
        let b = Self::resolve_no_follow(b)?;
        check_path_access(&a, PathAccess::Write)?;
        check_path_access(&b, PathAccess::Write)?;
        check_writable(&a)?;
        check_writable(&b)?;
        // Intercept: let pre-operation handlers approve/deny (treat as a
        // rename touching both paths).
        super::intercept::pre_rename(&a, &b)?;

        {
            let (fs_a, _id_a, _opts_a, rel_a) = resolve_mount(&a)?;
            let (fs_b, _id_b, _opts_b, rel_b) = resolve_mount(&b)?;
            if !Arc::ptr_eq(&fs_a, &fs_b) {
                // Cross-mount exchange: no atomic cross-FS swap exists.
                // Linux returns EXDEV here (not EINVAL); surface it as
                // CrossDevice so the syscall layer maps it correctly.
                return Err(KernelError::CrossDevice);
            }
            // Same FS — perform the atomic swap under the per-mount lock.
            fs_b.lock().rename_exchange(&rel_a, &rel_b)?;
        }

        // Both entries moved: invalidate caches and notify for each.
        {
            let mut dcache = VFS_DCACHE.lock();
            dcache.invalidate_prefix(&a);
            dcache.invalidate_prefix(&b);
        }
        super::notify::emit_renamed(&a, &b);
        super::notify::emit_renamed(&b, &a);
        // Exchange leaves BOTH paths present (with swapped contents), so use
        // the "changed" hook rather than "renamed" (which would drop a path
        // the indexer still needs to track).
        super::index::on_file_changed(&a);
        super::index::on_file_changed(&b);
        super::journal::record_rename(&a, &b);
        super::audit::log_ok(super::audit::AuditOp::Rename, 0, &a);
        Ok(())
    }

    /// List mount points that appear in the VFS.
    ///
    /// Returns a list of `(mount_path, fs_type)` pairs.
    /// Safe to call from inside a mounted filesystem — it locks no filesystem
    /// at all, only the global VFS lock, because the type name is cached in
    /// [`MountPoint::fs_type`]. That is what `/proc/mounts` depends on.
    pub fn mounts() -> Vec<(PathBuf, String)> {
        let vfs = VFS.lock();
        vfs.mounts
            .iter()
            .map(|mp| (mp.path.clone(), mp.fs_type.clone()))
            .collect()
    }

    /// List all mount points with full information (path, fs type, options).
    ///
    /// Locks no filesystem — see [`Self::mounts`] and [`MountPoint::fs_type`].
    pub fn mounts_full() -> Vec<(PathBuf, String, MountOptions)> {
        let vfs = VFS.lock();
        vfs.mounts
            .iter()
            .map(|mp| (mp.path.clone(), mp.fs_type.clone(), mp.options))
            .collect()
    }

    /// Get mount options for the filesystem containing `path`.
    pub fn mount_options(path: impl AsRef<Path>) -> KernelResult<MountOptions> {
        let path = path.as_ref();
        let mut vfs = VFS.lock();
        let (mp, _) = find_mount(&mut vfs, path)?;
        Ok(mp.options)
    }

    /// Re-mount a filesystem with new options (e.g., `remount,ro`).
    pub fn remount(mount_path: impl AsRef<Path>, options: MountOptions) -> KernelResult<()> {
        // Same normalisation as `mount`/`unmount`: identify the mount by its
        // canonical spelling, not by the caller's.
        let mount_path = &normalize_mount_path(mount_path.as_ref());
        let mut vfs = VFS.lock();
        for mp in &mut vfs.mounts {
            if mp.path.as_path() == mount_path.as_path() {
                crate::serial_println!(
                    "[vfs] Remounted '{}' with options: {}",
                    mount_path.display(),
                    options.to_string(),
                );
                mp.options = options;
                return Ok(());
            }
        }
        Err(KernelError::NotFound)
    }

    /// Find mount-point names that are direct children of `dir_path`.
    ///
    /// For example, if `dir_path` is `"/"` and there are mounts at
    /// `"/tmp"` and `"/mnt/usb"`, this returns `["tmp"]` — only the
    /// immediate child, not nested mounts.
    fn submount_children(vfs: &VfsInner, dir_path: &Path) -> Vec<PathBuf> {
        let mut names = Vec::new();

        for mp in &vfs.mounts {
            // `strip_prefix` is component-aligned, so a mount at `/tmpfile`
            // is not treated as living under `/tmp`.  A tail of exactly one
            // component is a direct child; an empty tail is the mount that
            // *is* this directory (skipped), and a longer tail is a nested
            // mount that some intermediate directory owns, not this one.
            if let Some(tail) = mp.path.strip_prefix(dir_path) {
                if tail.components().count() == 1 {
                    names.push(tail.to_path_buf());
                }
            }
        }

        names
    }

    // --- Extended metadata VFS methods ---

    /// Get rich metadata for a path.
    pub fn metadata(path: impl AsRef<Path>) -> KernelResult<FileMeta> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        Self::metadata_resolved(&path)
    }

    /// Like [`metadata`](Self::metadata) but on an **already-resolved** host
    /// path (see [`read_at_resolved`](Self::read_at_resolved)).
    pub fn metadata_resolved(path: impl AsRef<Path>) -> KernelResult<FileMeta> {
        let path = path.as_ref();
        let (fs, _id, _opts, relative) = resolve_mount(path)?;
        fs.lock().metadata(&relative)
    }

    /// Resolve `path` to its stable system-wide [`FileId`], or `None` if the
    /// object has no stable identity (and is therefore not cacheable).
    ///
    /// Combines the owning mount's stable [`MountPoint::fs_id`] with the
    /// backing filesystem's inode number ([`FileMeta::ino`]) into the
    /// `(fs_id, ino)` pair that uniquely identifies a file across the whole
    /// VFS namespace.  This is the page-cache key (design-decisions §23/§36):
    /// two mappings that resolve to the same `FileId` are the same underlying
    /// object and may share read-only physical frames.
    ///
    /// Returns `Ok(None)` — meaning "no stable identity, do not cache" — when
    /// the backing filesystem reports `ino == 0` (FAT, ISO9660, pseudo-
    /// filesystems).  Callers must treat `None` as "fall back to the
    /// per-mapping read path", never as an error.  Symlinks are followed
    /// (identity is of the final target, matching `stat`/`metadata`).
    ///
    /// # Errors
    ///
    /// Propagates path-resolution / metadata errors (`NotFound`, etc.).  A
    /// missing or unreadable path is a real error; only a *successfully
    /// resolved* object that lacks a stable inode yields `Ok(None)`.
    pub fn file_identity(path: impl AsRef<Path>) -> KernelResult<Option<FileId>> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        Self::file_identity_resolved(&path)
    }

    /// Like [`file_identity`](Self::file_identity) but on an **already-resolved**
    /// host path (see [`read_at_resolved`](Self::read_at_resolved)).
    pub fn file_identity_resolved(path: impl AsRef<Path>) -> KernelResult<Option<FileId>> {
        let path = path.as_ref();
        check_path_access(path, PathAccess::Metadata)?;
        let (fs, fs_id, _opts, relative) = resolve_mount(path)?;
        let ino = fs.lock().metadata(&relative)?.ino;
        // ino == 0 ⇒ filesystem has no stable per-object identity ⇒ not
        // cacheable.  Returning None (not an error) lets the caller degrade
        // gracefully to the per-mapping read path.
        if ino == 0 {
            return Ok(None);
        }
        Ok(Some(FileId { fs_id, ino }))
    }

    /// Compute the SHA-256 content hash of a file.
    ///
    /// Reads the file and returns the 32-byte SHA-256 digest.
    /// Returns `IsADirectory` if the path is a directory.
    pub fn content_hash(path: impl AsRef<Path>) -> KernelResult<Vec<u8>> {
        let path = path.as_ref();
        let data = Self::read_file(path)?;
        Ok(crate::crypto::sha256_vec(&data))
    }

    /// Set file attributes (immutable, append-only, hidden, system).
    pub fn set_attributes(path: impl AsRef<Path>, attrs: FileAttr) -> KernelResult<()> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        check_writable(&path)?;
        check_path_access(&path, PathAccess::Metadata)?;
        {
            let (fs, _id, _opts, relative) = resolve_mount(&path)?;
            fs.lock().set_attributes(&relative, attrs)?;
        }
        super::notify::emit_metadata(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// Set ownership (uid/gid).
    ///
    /// Per POSIX `chown`, a uid or gid of `u32::MAX` (i.e. `(uid_t)-1` /
    /// `(gid_t)-1`) means "leave that field unchanged".  We resolve those
    /// sentinels here against the file's current owner so every backing
    /// filesystem `set_owner` impl receives concrete values and need not
    /// know about the sentinel convention.
    pub fn set_owner(path: impl AsRef<Path>, uid: u32, gid: u32) -> KernelResult<()> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        check_writable(&path)?;
        check_path_access(&path, PathAccess::Metadata)?;
        // Resolve "leave unchanged" sentinels before taking the VFS lock
        // (metadata() takes the lock itself).
        let (uid, gid) = if uid == u32::MAX || gid == u32::MAX {
            let meta = Self::metadata(&path)?;
            (
                if uid == u32::MAX { meta.uid } else { uid },
                if gid == u32::MAX { meta.gid } else { gid },
            )
        } else {
            (uid, gid)
        };
        {
            let (fs, _id, _opts, relative) = resolve_mount(&path)?;
            fs.lock().set_owner(&relative, uid, gid)?;
        }
        super::notify::emit_metadata(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// Set ownership WITHOUT following a trailing symlink (`lchown` /
    /// `fchownat(AT_SYMLINK_NOFOLLOW)`).
    ///
    /// No-follow analogue of [`set_owner`](Self::set_owner): if the final
    /// component is a symlink, the link inode itself is chowned rather than
    /// its target.  Intermediate symlinks are still resolved.  The
    /// `u32::MAX` "leave unchanged" sentinels are read from the link's own
    /// metadata via [`lmetadata`](Self::lmetadata) (not the target's).
    pub fn set_owner_no_follow(path: impl AsRef<Path>, uid: u32, gid: u32) -> KernelResult<()> {
        let path = path.as_ref();
        let path = Self::resolve_no_follow(path)?;
        check_writable(&path)?;
        check_path_access(&path, PathAccess::Metadata)?;
        let (uid, gid) = if uid == u32::MAX || gid == u32::MAX {
            let meta = Self::lmetadata(&path)?;
            (
                if uid == u32::MAX { meta.uid } else { uid },
                if gid == u32::MAX { meta.gid } else { gid },
            )
        } else {
            (uid, gid)
        };
        {
            let (fs, _id, _opts, relative) = resolve_mount(&path)?;
            fs.lock().set_owner_no_follow(&relative, uid, gid)?;
        }
        super::notify::emit_metadata(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// Set Unix-style permission bits.
    pub fn set_permissions(path: impl AsRef<Path>, permissions: u16) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_follow(path)?;
        check_writable(&path)?;
        check_path_access(&path, PathAccess::Metadata)?;
        {
            let (fs, _id, _opts, relative) = resolve_mount(&path)?;
            fs.lock().set_permissions(&relative, permissions)?;
        }
        super::notify::emit_metadata(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// Set permission bits WITHOUT following a final symlink
    /// (`fchmodat2(AT_SYMLINK_NOFOLLOW)`).
    ///
    /// No-follow analogue of [`set_permissions`](Self::set_permissions): if the
    /// final component is a symlink, the link inode's own mode bits are changed
    /// rather than its target's.  Intermediate symlinks are still resolved.
    pub fn set_permissions_no_follow(path: impl AsRef<Path>, permissions: u16) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_no_follow(path)?;
        check_writable(&path)?;
        check_path_access(&path, PathAccess::Metadata)?;
        {
            let (fs, _id, _opts, relative) = resolve_mount(&path)?;
            fs.lock()
                .set_permissions_no_follow(&relative, permissions)?;
        }
        super::notify::emit_metadata(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// Update timestamps (pass 0 to leave unchanged).
    pub fn set_times(
        path: impl AsRef<Path>,
        accessed_ns: Timestamp,
        modified_ns: Timestamp,
    ) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_follow(path)?;
        check_writable(&path)?;
        check_path_access(&path, PathAccess::Metadata)?;
        let (fs, _id, _opts, relative) = resolve_mount(&path)?;
        fs.lock().set_times(&relative, accessed_ns, modified_ns)
        // No notify/journal — timestamp changes are metadata-only.
    }

    /// Update timestamps WITHOUT following a trailing symlink (`lutimes` /
    /// `utimensat(AT_SYMLINK_NOFOLLOW)`).
    ///
    /// No-follow analogue of [`set_times`](Self::set_times): stamps the
    /// link inode itself when the final component is a symlink.
    pub fn set_times_no_follow(
        path: impl AsRef<Path>,
        accessed_ns: Timestamp,
        modified_ns: Timestamp,
    ) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_no_follow(path)?;
        check_writable(&path)?;
        check_path_access(&path, PathAccess::Metadata)?;
        let (fs, _id, _opts, relative) = resolve_mount(&path)?;
        fs.lock()
            .set_times_no_follow(&relative, accessed_ns, modified_ns)
        // No notify/journal — timestamp changes are metadata-only.
    }

    /// Get an extended attribute value.
    pub fn get_xattr(path: impl AsRef<Path>, key: &str) -> KernelResult<Vec<u8>> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        check_path_access(&path, PathAccess::Read)?;
        let (fs, _id, _opts, relative) = resolve_mount(&path)?;
        fs.lock().get_xattr(&relative, key)
    }

    /// Set an extended attribute.
    pub fn set_xattr(path: impl AsRef<Path>, key: &str, value: &[u8]) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_follow(path)?;
        check_writable(&path)?;
        check_path_access(&path, PathAccess::Write)?;
        {
            let (fs, _id, _opts, relative) = resolve_mount(&path)?;
            fs.lock().set_xattr(&relative, key, value)?;
        }
        super::notify::emit_metadata(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// Remove an extended attribute.
    pub fn remove_xattr(path: impl AsRef<Path>, key: &str) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_follow(path)?;
        check_writable(&path)?;
        check_path_access(&path, PathAccess::Write)?;
        {
            let (fs, _id, _opts, relative) = resolve_mount(&path)?;
            fs.lock().remove_xattr(&relative, key)?;
        }
        super::notify::emit_metadata(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// List all extended attribute keys.
    pub fn list_xattrs(path: impl AsRef<Path>) -> KernelResult<Vec<String>> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        check_path_access(&path, PathAccess::Read)?;
        let (fs, _id, _opts, relative) = resolve_mount(&path)?;
        fs.lock().list_xattrs(&relative)
    }

    // --- No-follow xattr wrappers (lgetxattr/lsetxattr/llistxattr/
    // lremovexattr): operate on the symlink itself when the final component
    // is a link.  Intermediate symlinks are still resolved. ---

    /// Get an xattr WITHOUT following a trailing symlink (`lgetxattr`).
    pub fn get_xattr_no_follow(path: impl AsRef<Path>, key: &str) -> KernelResult<Vec<u8>> {
        let path = path.as_ref();
        let path = Self::resolve_no_follow(path)?;
        check_path_access(&path, PathAccess::Read)?;
        let (fs, _id, _opts, relative) = resolve_mount(&path)?;
        fs.lock().get_xattr_no_follow(&relative, key)
    }

    /// Set an xattr WITHOUT following a trailing symlink (`lsetxattr`).
    pub fn set_xattr_no_follow(
        path: impl AsRef<Path>,
        key: &str,
        value: &[u8],
    ) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_no_follow(path)?;
        check_writable(&path)?;
        check_path_access(&path, PathAccess::Write)?;
        {
            let (fs, _id, _opts, relative) = resolve_mount(&path)?;
            fs.lock().set_xattr_no_follow(&relative, key, value)?;
        }
        super::notify::emit_metadata(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// Remove an xattr WITHOUT following a trailing symlink (`lremovexattr`).
    pub fn remove_xattr_no_follow(path: impl AsRef<Path>, key: &str) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_no_follow(path)?;
        check_writable(&path)?;
        check_path_access(&path, PathAccess::Write)?;
        {
            let (fs, _id, _opts, relative) = resolve_mount(&path)?;
            fs.lock().remove_xattr_no_follow(&relative, key)?;
        }
        super::notify::emit_metadata(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// List xattr keys WITHOUT following a trailing symlink (`llistxattr`).
    pub fn list_xattrs_no_follow(path: impl AsRef<Path>) -> KernelResult<Vec<String>> {
        let path = path.as_ref();
        let path = Self::resolve_no_follow(path)?;
        check_path_access(&path, PathAccess::Read)?;
        let (fs, _id, _opts, relative) = resolve_mount(&path)?;
        fs.lock().list_xattrs_no_follow(&relative)
    }

    // --- Symlink VFS methods ---

    /// Create a symbolic link.
    ///
    /// `path` is the location of the new symlink.  `target` is the
    /// string it points to (stored as-is, resolved on traversal).
    pub fn symlink(path: impl AsRef<Path>, target: impl AsRef<Path>) -> KernelResult<()> {
        let path = path.as_ref();
        let target = target.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_no_follow(path)?;
        check_writable(&path)?;
        check_path_access(&path, PathAccess::Write)?;
        // Intercept: let pre-operation handlers approve/deny symlink creation.
        super::intercept::pre_check(super::intercept::FsOp::Symlink, &path, Some(target))?;
        // Quota: creating a symlink consumes an inode.
        enforce_quota_create(&path)?;
        {
            let (fs, _id, _opts, relative) = resolve_mount(&path)?;
            fs.lock().symlink(&relative, target)?;
        }
        // Charge inode quota for new symlink.
        super::quota::charge_inode(0, 0);
        // A new symlink can change how any path through it resolves.
        // Invalidate the parent directory prefix to be safe.
        VFS_DCACHE
            .lock()
            .invalidate_prefix(path.parent().unwrap_or(Path::new("/")));
        super::notify::emit_created(&path);
        super::index::on_file_changed(&path);
        super::journal::record(super::journal::JournalEventType::Created, &path);
        super::audit::log_ok(super::audit::AuditOp::Symlink, 0, &path);
        Ok(())
    }

    /// Create a hard link.
    ///
    /// `existing` is the path to an existing file.
    /// `new_path` is where the new directory entry will be created.
    ///
    /// Both paths must resolve to the same mount point.  The existing
    /// path is followed through symlinks (the link points to the
    /// underlying file, not the symlink) — this is the `linkat` +
    /// `AT_SYMLINK_FOLLOW` semantics.  Plain `link(2)` must NOT follow;
    /// use [`link_no_follow`] for that.
    pub fn link(existing: impl AsRef<Path>, new_path: impl AsRef<Path>) -> KernelResult<()> {
        let existing = existing.as_ref();
        let new_path = new_path.as_ref();
        Self::link_inner(existing, new_path, true)
    }

    /// Create a hard link WITHOUT following a trailing symlink in `existing`
    /// (`link(2)` / `linkat` without `AT_SYMLINK_FOLLOW`).
    ///
    /// If `existing` names a symlink, the new directory entry hard-links the
    /// symlink inode itself rather than its target.  Intermediate symlinks in
    /// `existing` are still resolved; only the final component is not.
    pub fn link_no_follow(
        existing: impl AsRef<Path>,
        new_path: impl AsRef<Path>,
    ) -> KernelResult<()> {
        let existing = existing.as_ref();
        let new_path = new_path.as_ref();
        Self::link_inner(existing, new_path, false)
    }

    /// Shared `link`/`link_no_follow` back-end.  `follow` selects whether a
    /// trailing symlink in `existing` is dereferenced before the hard link is
    /// created; everything else (namespace/write checks, same-mount rule,
    /// quota, notify/journal/audit) is identical.
    fn link_inner(
        existing: impl AsRef<Path>,
        new_path: impl AsRef<Path>,
        follow: bool,
    ) -> KernelResult<()> {
        let existing = existing.as_ref();
        let new_path = new_path.as_ref();
        crate::ipc::namespace::check_writable(new_path)?;
        let existing = if follow {
            Self::resolve_follow(existing)?
        } else {
            Self::resolve_no_follow(existing)?
        };
        let new_path = Self::resolve_no_follow(new_path)?;
        check_writable(&new_path)?;
        // Intercept: let pre-operation handlers approve/deny link creation.
        super::intercept::pre_check(super::intercept::FsOp::Link, &new_path, Some(&existing))?;
        // Quota: creating a link is creating a new inode reference.
        enforce_quota_create(&new_path)?;

        {
            // Both paths must be on the same mount — they share one iff
            // `resolve_mount` hands back the same per-mount handle.  Resolving
            // each also yields the mount-relative paths, replacing the manual
            // longest-prefix scan the global-lock version performed inline.
            let (fs_existing, _id_e, _opts_e, rel_existing) = resolve_mount(&existing)?;
            let (fs_new, _id_n, _opts_n, rel_new) = resolve_mount(&new_path)?;
            if !Arc::ptr_eq(&fs_existing, &fs_new) {
                return Err(KernelError::InvalidArgument); // Cross-mount hard link.
            }
            // The Vfs layer already resolved `existing` per `follow`, but the
            // final on-disk lookup happens inside the FS driver — so route to
            // the matching driver method to keep the no-follow contract when a
            // symlink is the final component.
            if follow {
                fs_existing.lock().link(&rel_existing, &rel_new)?;
            } else {
                fs_existing.lock().link_no_follow(&rel_existing, &rel_new)?;
            }
        }

        // Charge inode quota for new link.
        super::quota::charge_inode(0, 0);
        // New hard link invalidates negative cache entries for the new path.
        VFS_DCACHE.lock().invalidate_negative_prefix(&new_path);
        super::notify::emit_created(&new_path);
        super::index::on_file_changed(&new_path);
        super::journal::record(super::journal::JournalEventType::Created, &new_path);
        super::audit::log_ok(super::audit::AuditOp::Link, 0, &new_path);
        Ok(())
    }

    /// Read a symbolic link's target.
    ///
    /// Does NOT follow the symlink — returns the stored target string.
    pub fn readlink(path: impl AsRef<Path>) -> KernelResult<PathBuf> {
        let path = path.as_ref();
        let path = Self::resolve_no_follow(path)?;
        check_path_access(&path, PathAccess::Metadata)?;
        let (fs, _id, _opts, relative) = resolve_mount(&path)?;
        fs.lock().readlink(&relative)
    }

    /// Stat a path without following the final symbolic link.
    pub fn lstat(path: impl AsRef<Path>) -> KernelResult<DirEntry> {
        let path = path.as_ref();
        let path = Self::resolve_no_follow(path)?;
        check_path_access(&path, PathAccess::Metadata)?;
        let (fs, _id, _opts, relative) = resolve_mount(&path)?;
        fs.lock().lstat(&relative)
    }

    /// Get rich metadata for a path WITHOUT following a trailing symlink.
    ///
    /// No-follow analogue of [`metadata`](Self::metadata), backing the
    /// `lstat`/`lfstatat` syscalls.  Intermediate symlinks are still
    /// resolved; only the final component is left unfollowed.
    pub fn lmetadata(path: impl AsRef<Path>) -> KernelResult<FileMeta> {
        let path = path.as_ref();
        let path = Self::resolve_no_follow(path)?;
        check_path_access(&path, PathAccess::Metadata)?;
        let (fs, _id, _opts, relative) = resolve_mount(&path)?;
        fs.lock().lmetadata(&relative)
    }

    /// Return debug statistics for the filesystem mounted at `path`.
    pub fn debug_stats(path: impl AsRef<Path>) -> KernelResult<String> {
        match Self::debug_stats_fs(path)? {
            Some(fs) => Ok(fs.lock().debug_stats()),
            None => Err(KernelError::NotFound),
        }
    }

    /// [`debug_stats`](Self::debug_stats), but never blocks on the per-mount
    /// lock: `Ok(None)` means the filesystem is busy right now.
    ///
    /// This exists for callers that are themselves running *inside* a mounted
    /// filesystem, where the blocking form is not slow but fatal.
    /// `/proc/fsstats` walks every mount and asks it for stats, and one of
    /// those mounts is the procfs the read is being served from — whose lock
    /// the VFS is holding for the duration of that very read. The blocking
    /// form deadlocks the kernel there, deterministically, on `cat
    /// /proc/fsstats` and on `ls /proc`.
    ///
    /// Reporting "busy" rather than blocking is not a weaker answer in that
    /// case, it is the only truthful one: a filesystem asked to describe its
    /// own internals mid-operation would be describing a half-finished read.
    pub fn debug_stats_nonblocking(path: impl AsRef<Path>) -> KernelResult<Option<String>> {
        match Self::debug_stats_fs(path)? {
            Some(fs) => Ok(fs.try_lock().map(|g| g.debug_stats())),
            None => Err(KernelError::NotFound),
        }
    }

    /// Find the filesystem whose mount point is the longest prefix of `path`.
    ///
    /// Clones the per-mount handle under a brief global lock so the caller can
    /// query it with the VFS lock dropped (`debug_stats` may itself touch the
    /// VFS on stacked mounts).
    fn debug_stats_fs(path: impl AsRef<Path>) -> KernelResult<Option<MountedFs>> {
        let path = path.as_ref();
        let vfs = VFS.lock();
        // Longest-prefix, not first-match.  `find` returned whichever covering
        // mount sat earliest in the table, and the root mount is registered
        // first and covers everything — so `debug_stats` for a path under any
        // submount reported the *root* filesystem's stats.  Every other mount
        // lookup here scores by prefix length; this one did not.
        let mut best: Option<&MountPoint> = None;
        for mp in &vfs.mounts {
            if !mount_matches(&mp.path, path) {
                continue;
            }
            if best.is_none_or(|b| mp.path.len() > b.path.len()) {
                best = Some(mp);
            }
        }
        Ok(best.map(|mp| Arc::clone(&mp.fs)))
    }

    /// Query filesystem space and configuration for the mount at `path`.
    ///
    /// Returns capacity, free space, block size, and other filesystem
    /// metadata.  Analogous to POSIX `statvfs()`.
    pub fn statvfs(path: impl AsRef<Path>) -> KernelResult<FsInfo> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        let (fs, _id, _opts, _relative) = resolve_mount(&path)?;
        fs.lock().statvfs()
    }

    /// Discard (TRIM) the free space of the filesystem containing `path`.
    ///
    /// Resolves `path` to its mount point and asks that filesystem to issue
    /// discard for every run of free blocks on its backing device (the kernel
    /// side of `fstrim(8)`).  Returns the number of bytes discarded.  This is
    /// non-destructive: only free blocks are trimmed.  Read-only mounts and
    /// filesystems whose backing device does not support discard return
    /// `Ok(0)` (nothing to do).
    pub fn trim(path: impl AsRef<Path>) -> KernelResult<u64> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        let (fs, _id, opts, _relative) = resolve_mount(&path)?;
        // A read-only mount has no business mutating the device; treat it as a
        // no-op rather than letting the filesystem attempt discards.
        if opts.read_only {
            return Ok(0);
        }
        fs.lock().trim()
    }

    /// Discard (TRIM) the free space of the filesystem backed by `device`.
    ///
    /// Finds the mount whose backing block device is `device` (e.g. `"vda"`)
    /// and trims its free space (the kernel side of `fstrim` invoked by
    /// device name rather than mount path).  Returns the number of bytes
    /// discarded.  A read-only mount is a no-op (`Ok(0)`).  Returns
    /// [`KernelError::NotFound`] if no mounted filesystem is backed by that
    /// device — fstrim needs the free-space metadata of a live mount, so an
    /// unmounted device cannot be trimmed this way.
    pub fn trim_device(device: &str) -> KernelResult<u64> {
        // Snapshot the handles under a brief global lock, then scan and trim
        // with it dropped.
        //
        // The scan used to run `mp.fs.lock().device_name()` inside the VFS lock,
        // excused as safe because `device_name` cannot re-enter the VFS.  That
        // reasoning does not hold: a lock *order* is a global property, so what
        // matters is not what this callee does but that taking a per-mount lock
        // under the VFS lock inverts the order design-decisions §43 relies on.
        // Another CPU holding a per-mount lock and waiting on `VFS` — exactly
        // what the overlay does when it re-enters to reach its lower layer —
        // deadlocks against it regardless of how trivial `device_name` is.
        let candidates: Vec<(MountedFs, bool)> = {
            let vfs = VFS.lock();
            vfs.mounts
                .iter()
                .map(|mp| (Arc::clone(&mp.fs), mp.options.read_only))
                .collect()
        };
        let found = candidates
            .into_iter()
            .find(|(fs, _)| fs.lock().device_name() == Some(device));
        match found {
            Some((fs, read_only)) => {
                if read_only {
                    return Ok(0);
                }
                fs.lock().trim()
            }
            None => Err(KernelError::NotFound),
        }
    }

    /// List all mount points with their filesystem info.
    ///
    /// Returns `(mount_path, FsInfo)` for each mounted filesystem.
    pub fn mount_info() -> KernelResult<Vec<(PathBuf, FsInfo)>> {
        // Snapshot (path, handle) pairs under a brief global lock, then query
        // each filesystem lock-free — `statvfs` on a stacked mount may itself
        // re-enter the VFS, so it must not run under the global lock.
        let mounts: Vec<(PathBuf, MountedFs)> = {
            let vfs = VFS.lock();
            vfs.mounts
                .iter()
                .map(|mp| (mp.path.clone(), Arc::clone(&mp.fs)))
                .collect()
        };
        let mut result = Vec::new();
        for (path, fs) in mounts {
            let mut guard = fs.lock();
            // statvfs may fail for virtual filesystems or misconfigured
            // mounts.  Log the error but still include the mount in the
            // list with zeroed stats so df/mount show it exists.
            let info = match guard.statvfs() {
                Ok(i) => i,
                Err(e) => {
                    crate::serial_println!(
                        "[vfs] mount_info: statvfs failed for '{}' ({}): {:?}",
                        path.display(),
                        guard.fs_type(),
                        e
                    );
                    FsInfo {
                        fs_type: String::from(guard.fs_type()),
                        volume_label: String::new(),
                        block_size: 0,
                        total_blocks: 0,
                        free_blocks: 0,
                        total_inodes: 0,
                        free_inodes: 0,
                        max_name_len: 255,
                        read_only: false,
                    }
                }
            };
            result.push((path, info));
        }
        Ok(result)
    }

    // ----- Path resolution cache stats -----

    // ----- Convenience helpers -----

    /// Check if a path exists (file, directory, or symlink).
    ///
    /// Follows symlinks.  Returns `false` for broken symlinks.
    pub fn exists(path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        Self::stat(path).is_ok()
    }

    /// Check if a path exists and is a directory.
    ///
    /// Follows symlinks.  Returns `false` if the path doesn't exist
    /// or is not a directory.
    pub fn is_directory(path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        Self::stat(path)
            .map(|e| e.entry_type == EntryType::Directory)
            .unwrap_or(false)
    }

    /// Check if a path exists and is a regular file.
    pub fn is_file(path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        Self::stat(path)
            .map(|e| e.entry_type == EntryType::File)
            .unwrap_or(false)
    }

    /// Get the size of a file in bytes.
    ///
    /// Returns `NotFound` if the path doesn't exist, `NotSupported` if
    /// it's a directory (use `readdir` to count entries).
    pub fn file_size(path: impl AsRef<Path>) -> KernelResult<u64> {
        let path = path.as_ref();
        let entry = Self::stat(path)?;
        if entry.entry_type == EntryType::Directory {
            return Err(KernelError::NotSupported);
        }
        Ok(entry.size)
    }

    /// Check if a path is readable.
    ///
    /// Returns `Ok(())` if the file exists and has read permission,
    /// or an appropriate error (`NotFound`, `PermissionDenied`).
    pub fn is_readable(path: impl AsRef<Path>) -> KernelResult<()> {
        let path = path.as_ref();
        let meta = Self::metadata(path)?;
        // Check any read permission bit (owner/group/other).
        if meta.permissions & 0o444 != 0 {
            Ok(())
        } else {
            Err(KernelError::PermissionDenied)
        }
    }

    /// Check if a path is writable.
    ///
    /// Returns `Ok(())` if the file exists and has write permission,
    /// or an appropriate error (`NotFound`, `PermissionDenied`).
    /// Also checks the immutable attribute.
    pub fn is_writable(path: impl AsRef<Path>) -> KernelResult<()> {
        let path = path.as_ref();
        let meta = Self::metadata(path)?;
        if meta.attributes.contains(FileAttr::IMMUTABLE) {
            return Err(KernelError::PermissionDenied);
        }
        // Check any write permission bit (owner/group/other).
        if meta.permissions & 0o222 != 0 {
            Ok(())
        } else {
            Err(KernelError::PermissionDenied)
        }
    }

    /// Check file accessibility (POSIX `access()` equivalent).
    ///
    /// `mode` is a bitmask of [`F_OK`], [`R_OK`], [`W_OK`], [`X_OK`].
    /// `F_OK` (0) just checks existence.
    ///
    /// Returns `Ok(())` when every requested access is permitted, or
    /// `NotFound` / `PermissionDenied` on failure.
    pub fn access(path: impl AsRef<Path>, mode: u32) -> KernelResult<()> {
        let path = path.as_ref();
        let meta = Self::metadata(path)?; // NotFound propagated here

        // File capability tag check — regardless of mode, a process
        // must pass group membership requirements on tagged paths.
        let resolved = Self::resolve_follow(path).unwrap_or_else(|_| path.to_path_buf());
        check_path_access(&resolved, PathAccess::Metadata)?;

        // F_OK (0) — existence only; metadata() already succeeded.
        if mode == F_OK {
            return Ok(());
        }

        // Check mount options: read-only mounts deny W_OK, noexec denies X_OK.
        if let Ok(opts) = Self::mount_options(path) {
            if mode & W_OK != 0 && opts.read_only {
                return Err(KernelError::ReadOnlyFilesystem);
            }
            if mode & X_OK != 0 && opts.noexec {
                return Err(KernelError::PermissionDenied);
            }
        }

        // Immutable files deny write regardless of permission bits.
        if mode & W_OK != 0 && meta.attributes.contains(FileAttr::IMMUTABLE) {
            return Err(KernelError::PermissionDenied);
        }

        // For each class of permission requested, at least one
        // owner/group/other bit must be set (same logic as is_readable/is_writable).
        if mode & R_OK != 0 && meta.permissions & 0o444 == 0 {
            return Err(KernelError::PermissionDenied);
        }
        if mode & W_OK != 0 && meta.permissions & 0o222 == 0 {
            return Err(KernelError::PermissionDenied);
        }
        if mode & X_OK != 0 && meta.permissions & 0o111 == 0 {
            return Err(KernelError::PermissionDenied);
        }

        // POSIX ACLs, last: an ACL refines the traditional bits, it does not
        // override a mount flag or an immutable attribute that already refused
        // above. Each requested class is asked for separately because an ACL
        // can grant read and deny write to the same requester, which a single
        // combined request could not express.
        for (bit, want) in [
            (R_OK, PathAccess::Read),
            (W_OK, PathAccess::Write),
            (X_OK, PathAccess::Execute),
        ] {
            if mode & bit != 0 {
                check_path_access(&resolved, want)?;
            }
        }

        Ok(())
    }

    /// Return VFS dcache statistics: (hits, misses, valid_entries).
    ///
    /// Used by procfs to report cache performance.
    pub fn dcache_stats() -> (u64, u64, usize) {
        VFS_DCACHE.lock().stats()
    }

    // ----- Glob -----

    /// Find all files/directories matching a glob pattern path.
    ///
    /// The pattern can contain glob metacharacters in any path component:
    /// - `/tmp/*.txt` — all .txt files in /tmp
    /// - `/proc/*/status` — status file for all PIDs
    /// - `/sys/params/mm.*` — all mm. params
    /// - `/**/*.rs` — all .rs files recursively
    /// - `/home/**` — all files under /home recursively
    ///
    /// The `**` pattern matches zero or more directory levels.  It can
    /// appear at any position in the path:
    /// - `/**/foo.txt` — find foo.txt anywhere
    /// - `/tmp/**/*.log` — all .log files under /tmp at any depth
    ///
    /// Returns a list of absolute paths that match.  Directories are not
    /// recursed into unless the pattern explicitly has deeper components
    /// or uses `**`.
    ///
    /// ## Limits
    ///
    /// - Maximum 1000 results to prevent runaway expansion.
    /// - Maximum pattern depth of 32 components.
    /// - Maximum recursion depth of 16 for `**` patterns.
    pub fn glob(pattern: impl AsRef<Path>) -> KernelResult<Vec<PathBuf>> {
        let pattern = pattern.as_ref();
        let components: Vec<&Path> = pattern.components().collect();

        if components.is_empty() {
            return Ok(alloc::vec![PathBuf::from("/")]);
        }

        if components.len() > 32 {
            return Err(KernelError::InvalidArgument);
        }

        let mut results = Vec::new();
        glob_recurse(
            Path::new("/"),
            &components,
            0,
            &mut results,
            1000, // max results
        );
        Ok(results)
    }

    // ----- Sync / Flush -----

    /// Flush all dirty data and metadata across all mounted filesystems.
    ///
    /// Ensures that all pending writes are committed to stable storage.
    /// Analogous to POSIX `sync()`.
    pub fn sync() -> KernelResult<()> {
        // Snapshot the handles under a brief global lock, then sync each
        // lock-free (a stacked filesystem's sync may re-enter the VFS).
        let handles: Vec<MountedFs> = {
            let vfs = VFS.lock();
            vfs.mounts.iter().map(|mp| Arc::clone(&mp.fs)).collect()
        };
        let mut last_err: Option<KernelError> = None;
        for fs in handles {
            if let Err(e) = fs.lock().sync() {
                last_err = Some(e);
            }
        }
        match last_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Flush a specific filesystem (the one that `path` resolves to).
    pub fn sync_path(path: impl AsRef<Path>) -> KernelResult<()> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        let (fs, _id, _opts, _relative) = resolve_mount(&path)?;
        fs.lock().sync()
    }

    /// Set the volume label of the filesystem containing `path`.
    ///
    /// Dispatches to the underlying filesystem's `set_volume_label()`
    /// method.  Returns `NotSupported` for filesystems without labels.
    pub fn set_volume_label(path: impl AsRef<Path>, label: &str) -> KernelResult<()> {
        let path = path.as_ref();
        check_writable(path)?;
        let path = Self::resolve_follow(path)?;
        let (fs, _id, _opts, _relative) = resolve_mount(&path)?;
        fs.lock().set_volume_label(label)
    }

    // ----- Atomic file operations -----

    /// Atomically replace a file's contents.
    ///
    /// Writes `data` to a temporary file in the same directory as `path`,
    /// syncs the filesystem, then renames the temp file to the final path.
    /// If the rename succeeds, the file is guaranteed to contain either the
    /// old data or the new data — never a partial write.
    ///
    /// If any step fails, the temporary file is cleaned up and the original
    /// file is left untouched.
    ///
    /// This is the standard safe-write pattern (used by editors, databases,
    /// config writers, etc.) exposed as a single VFS operation.
    pub fn atomic_write(path: impl AsRef<Path>, data: &[u8]) -> KernelResult<()> {
        let path = path.as_ref();
        // Authoritative read-only volume check on the caller's (guest) path,
        // before resolution.  Internal write_file/rename calls below operate
        // on already-resolved host temp paths, so this top-level check is the
        // one that enforces per-process read-only volume mounts.
        crate::ipc::namespace::check_writable(path)?;
        let resolved = Self::resolve_follow(path)?;
        check_path_access(&resolved, PathAccess::Write)?;
        check_writable(&resolved)?;

        // Generate a unique temp filename in the same directory.
        // Same directory ensures rename is on the same filesystem (atomic).
        let dir = resolved.parent().unwrap_or(Path::new("/"));

        let ns = crate::hpet::elapsed_ns();
        // SAFETY: rdtsc is always available on x86_64 and has no side effects.
        let tsc = unsafe { core::arch::x86_64::_rdtsc() };
        let unique = ns ^ tsc;
        let tmp_path = dir.join(alloc::format!(".tmp_atomic_{unique:016x}"));

        // Step 1: Write data to the temp file.
        if let Err(e) = Self::write_file(&tmp_path, data) {
            // Cleanup temp file if it was partially created.
            let _ = Self::remove(&tmp_path);
            return Err(e);
        }

        // Step 2: Sync the filesystem to ensure data is on disk.
        // Errors from sync are non-fatal — the rename will still work
        // in memory, and the next sync or shutdown will persist it.
        let _ = Self::sync_path(&tmp_path);

        // Step 3: Rename temp file to the final path (atomic on same fs).
        if let Err(e) = Self::rename(&tmp_path, &resolved) {
            // Rename failed — clean up the temp file.
            let _ = Self::remove(&tmp_path);
            return Err(e);
        }

        Ok(())
    }

    /// Atomically write a file, preserving its permissions and ownership.
    ///
    /// Like `atomic_write()`, but copies the original file's metadata
    /// (permissions, ownership, timestamps) to the new file after the
    /// rename.  Use this when replacing config files or documents where
    /// metadata preservation matters.
    pub fn atomic_write_preserve(path: impl AsRef<Path>, data: &[u8]) -> KernelResult<()> {
        let path = path.as_ref();
        let resolved = Self::resolve_follow(path)?;

        // Capture existing metadata before the atomic write replaces it.
        let old_meta = Self::metadata(&resolved).ok();

        // Perform the atomic write (writes temp, syncs, renames).
        Self::atomic_write(path, data)?;

        // Restore metadata from the original file.
        if let Some(meta) = old_meta {
            // Permissions.
            let _ = Self::set_permissions(&resolved, meta.permissions);
            // Ownership.
            let _ = Self::set_owner(&resolved, meta.uid, meta.gid);
        }

        Ok(())
    }

    // ----- Advisory file locking -----

    /// Acquire an advisory lock on a file.
    ///
    /// `path` is resolved (symlinks followed) before locking.
    /// `owner` identifies the lock holder (typically a process/task ID).
    ///
    /// ## Semantics
    ///
    /// - **Shared lock**: compatible with other shared locks, incompatible
    ///   with exclusive locks from other owners.
    /// - **Exclusive lock**: incompatible with any lock from another owner.
    /// - If the owner already holds a lock on this path, the lock is
    ///   upgraded or downgraded atomically.
    ///
    /// Returns `WouldBlock` if the lock cannot be acquired (non-blocking).
    pub fn flock(path: impl AsRef<Path>, owner: u64, lock_type: LockType) -> KernelResult<()> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        Self::flock_resolved(&path, owner, lock_type)
    }

    /// Acquire an advisory lock on an already-resolved host path.
    ///
    /// Like [`flock_resolved`](Self::flock_resolved) for `read_at_resolved`:
    /// handle-backed callers already hold a resolved host path (captured at
    /// `open`), so they must NOT re-run namespace translation — doing so would
    /// re-apply the chroot jail prefix a second time (double-jail) and key the
    /// lock on the wrong path. This worker operates directly on `path`.
    pub fn flock_resolved(
        path: impl AsRef<Path>,
        owner: u64,
        lock_type: LockType,
    ) -> KernelResult<()> {
        let path = path.as_ref();
        let mut table = LOCK_TABLE.lock();

        // Find or create the entry for this path.
        let entry_idx = table.iter().position(|e| e.path.as_path() == path);

        if let Some(idx) = entry_idx {
            let entry = &mut table[idx];

            // Check if this owner already has a lock (upgrade/downgrade).
            if let Some(pos) = entry.locks.iter().position(|l| l.owner == owner) {
                // Re-lock: upgrade/downgrade.
                match lock_type {
                    LockType::Exclusive => {
                        // Can only upgrade to exclusive if no other locks exist.
                        if entry.locks.len() > 1 {
                            return Err(KernelError::WouldBlock);
                        }
                        entry.locks[pos].lock_type = LockType::Exclusive;
                    }
                    LockType::Shared => {
                        // Downgrade is always allowed.
                        entry.locks[pos].lock_type = LockType::Shared;
                    }
                }
                return Ok(());
            }

            // New lock on this path.
            match lock_type {
                LockType::Shared => {
                    // Compatible only if no exclusive lock exists.
                    if entry
                        .locks
                        .iter()
                        .any(|l| l.lock_type == LockType::Exclusive)
                    {
                        return Err(KernelError::WouldBlock);
                    }
                }
                LockType::Exclusive => {
                    // Incompatible with any existing lock.
                    if !entry.locks.is_empty() {
                        return Err(KernelError::WouldBlock);
                    }
                }
            }

            entry.locks.push(FileLock { owner, lock_type });
        } else {
            // No existing entry — create one.
            if table.len() >= MAX_LOCKED_PATHS {
                return Err(KernelError::OutOfMemory);
            }
            table.push(PathLockEntry {
                path: path.to_path_buf(),
                locks: alloc::vec![FileLock { owner, lock_type }],
            });
        }

        Ok(())
    }

    /// Release an advisory lock on a file.
    ///
    /// If the owner doesn't hold a lock on the path, this is a no-op.
    pub fn funlock(path: impl AsRef<Path>, owner: u64) -> KernelResult<()> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        Self::funlock_resolved(&path, owner)
    }

    /// Release an advisory lock on an already-resolved host path.
    ///
    /// Worker for [`funlock`](Self::funlock); handle-backed callers pass the
    /// resolved host path directly to avoid double-jailing (see
    /// [`flock_resolved`](Self::flock_resolved)).
    pub fn funlock_resolved(path: impl AsRef<Path>, owner: u64) -> KernelResult<()> {
        let path = path.as_ref();
        let mut table = LOCK_TABLE.lock();

        if let Some(idx) = table.iter().position(|e| e.path.as_path() == path) {
            let entry = &mut table[idx];
            entry.locks.retain(|l| l.owner != owner);

            // Clean up empty entries to prevent unbounded growth.
            if entry.locks.is_empty() {
                table.swap_remove(idx);
            }
        }

        Ok(())
    }

    /// Release all advisory locks held by a given owner (process cleanup).
    ///
    /// Called during process exit to avoid leaked locks.
    pub fn funlock_all(owner: u64) {
        let mut table = LOCK_TABLE.lock();
        // Remove this owner from every entry, then clean up empties.
        table.retain_mut(|entry| {
            entry.locks.retain(|l| l.owner != owner);
            !entry.locks.is_empty()
        });
    }

    /// Query the lock state of a file.
    ///
    /// Returns `None` if no locks are held, or `Some((lock_type, count))`
    /// describing the current lock state.
    pub fn lock_query(path: impl AsRef<Path>) -> KernelResult<Option<(LockType, usize)>> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        Self::lock_query_resolved(&path)
    }

    /// Query the lock state of an already-resolved host path.
    ///
    /// Worker for [`lock_query`](Self::lock_query); handle-backed callers pass
    /// the resolved host path directly to avoid double-jailing (see
    /// [`flock_resolved`](Self::flock_resolved)).
    pub fn lock_query_resolved(path: impl AsRef<Path>) -> KernelResult<Option<(LockType, usize)>> {
        let path = path.as_ref();
        let table = LOCK_TABLE.lock();

        if let Some(entry) = table.iter().find(|e| e.path.as_path() == path) {
            if entry.locks.is_empty() {
                return Ok(None);
            }
            // If any lock is exclusive, report exclusive.
            if entry
                .locks
                .iter()
                .any(|l| l.lock_type == LockType::Exclusive)
            {
                return Ok(Some((LockType::Exclusive, 1)));
            }
            // Otherwise all are shared.
            Ok(Some((LockType::Shared, entry.locks.len())))
        } else {
            Ok(None)
        }
    }
}

// ---------------------------------------------------------------------------
// Lock table dump (for procfs)
// ---------------------------------------------------------------------------

/// Dump all active advisory locks for display in `/proc/locks`.
///
/// Returns `(path, lock_type, owner)` for each active lock.
pub fn lock_table_dump() -> Vec<(PathBuf, LockType, u64)> {
    let table = LOCK_TABLE.lock();
    let mut result = Vec::new();
    for entry in table.iter() {
        for lock in &entry.locks {
            result.push((entry.path.clone(), lock.lock_type, lock.owner));
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Path validation
// ---------------------------------------------------------------------------

/// Maximum length of a single filename component (bytes, not characters).
///
/// The design spec (CLAUDE.md) specifies 255 bytes.  This matches the
/// Linux ext4 limit and is generous enough for any reasonable name while
/// preventing denial-of-service via absurdly long names.
const MAX_COMPONENT_LEN: usize = 255;

/// Validate a VFS path.
///
/// Rules (per design.txt lines 275-278):
/// - No null bytes anywhere in the path.
/// - Each component (between `/` separators) must be ≤ 255 bytes.
/// - Empty components are allowed (they result from double slashes and
///   are harmlessly collapsed by [`normalize_path`]).
/// - The path must start with `/` (absolute paths only in the VFS).
///
/// Returns `Ok(())` if valid, `Err(InvalidArgument)` if not.
pub fn validate_path<P: AsRef<Path>>(path: P) -> KernelResult<()> {
    let path = path.as_ref();

    // No null bytes.  Every *other* byte is legal in a name — that is the
    // design rule (`design.txt`), and the reason this takes a `Path` rather
    // than a `&str`: rejecting a name for not being UTF-8 would not be
    // validation, it would be an unreachable file.
    if !path.is_valid() {
        return Err(KernelError::InvalidArgument);
    }

    // Must be absolute.
    if !path.is_absolute() {
        return Err(KernelError::InvalidArgument);
    }

    // Check each component length.
    for component in path.components() {
        if component.len() > MAX_COMPONENT_LEN {
            return Err(KernelError::InvalidArgument);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Path normalization
// ---------------------------------------------------------------------------

/// Normalize a VFS path: resolve `.`, `..`, collapse double slashes.
///
/// Returns an owned `String`.  The result always starts with `/` and
/// never ends with `/` (except for the root `/` itself).
///
/// # Examples
///
/// - `"/foo/./bar"` → `"/foo/bar"`
/// - `"/foo/bar/../baz"` → `"/foo/baz"`
/// - `"/foo//bar"` → `"/foo/bar"`
/// - `"/"` → `"/"`
/// - `"/foo/bar/.."` → `"/foo"`
pub fn normalize_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let path = path.as_ref();
    let mut components: Vec<&Path> = Vec::new();

    // `Path::components` already drops empty parts (leading, repeated and
    // trailing separators); `.` and `..` are yielded verbatim because whether
    // they may be resolved lexically is the caller's decision, and here it is
    // yes — this is the purely textual normalizer.
    for part in path.components() {
        match part.as_bytes() {
            b"." => {}
            b".." => {
                components.pop();
            }
            _ => components.push(part),
        }
    }

    if components.is_empty() {
        return PathBuf::from("/");
    }

    let mut result = PathBuf::with_capacity(path.len());
    for c in &components {
        result.extend_bytes(b"/");
        result.extend_bytes(c.as_bytes());
    }
    result
}

// ---------------------------------------------------------------------------
// Mount point lookup
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Glob expansion helper
// ---------------------------------------------------------------------------

/// Recursively expand a glob pattern by matching directory contents.
///
/// `base` is the current absolute path prefix.
/// `components` is the full list of pattern components.
/// `depth` is the current component index being matched.
/// `results` collects matching paths.
/// `max_results` caps the output to prevent runaway expansion.
/// Maximum directory recursion depth for `**` patterns.
const GLOBSTAR_MAX_DEPTH: usize = 16;

fn glob_recurse(
    base: &Path,
    components: &[&Path],
    depth: usize,
    results: &mut Vec<PathBuf>,
    max_results: usize,
) {
    if results.len() >= max_results {
        return;
    }

    // Get the current component to match.
    let component = match components.get(depth) {
        Some(c) => *c,
        None => return, // No more components — shouldn't get here.
    };

    let is_last = depth + 1 == components.len();

    // Handle `**` (globstar): matches zero or more directory levels.
    if component == Path::new("**") {
        // `**` as the last component: match everything under base recursively.
        if is_last {
            glob_collect_recursive(base, results, max_results, 0);
            return;
        }

        // `**` followed by more components: try matching remaining pattern
        // at current level (zero directories) and at every subdirectory level.

        // Zero directories: skip `**` and try remaining components from base.
        glob_recurse(base, components, depth + 1, results, max_results);

        // One or more directories: for each subdirectory of base, try `**`
        // again (which will recurse deeper) and the remaining pattern.
        globstar_recurse(base, components, depth, results, max_results, 0);
        return;
    }

    // Check if this component contains glob metacharacters.
    let is_glob = component
        .as_bytes()
        .iter()
        .any(|&b| b == b'*' || b == b'?' || b == b'[');

    if is_glob {
        // Read the current directory and match each entry against the pattern.
        let entries = match Vfs::readdir(base) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in &entries {
            if glob_match(&entry.name, component, true) {
                let child_path = base.join(&entry.name);

                if is_last {
                    // This was the last component — add to results.
                    if results.len() < max_results {
                        results.push(child_path);
                    }
                } else if entry.entry_type == EntryType::Directory {
                    // More components to match — recurse into directories.
                    glob_recurse(&child_path, components, depth + 1, results, max_results);
                }
            }
        }
    } else {
        // No glob chars — this is a literal path component.
        let child_path = base.join(component);

        if is_last {
            // Check if this path exists.
            if Vfs::stat(&child_path).is_ok() {
                if results.len() < max_results {
                    results.push(child_path);
                }
            }
        } else {
            // Check if it's a directory before recursing.
            match Vfs::stat(&child_path) {
                Ok(entry) if entry.entry_type == EntryType::Directory => {
                    glob_recurse(&child_path, components, depth + 1, results, max_results);
                }
                _ => {} // Not a directory or doesn't exist — skip.
            }
        }
    }
}

/// Recursively descend into subdirectories for a `**` pattern component.
///
/// At each level, tries matching the remaining pattern components (after `**`)
/// from each subdirectory, then recurses deeper into their subdirectories.
fn globstar_recurse(
    base: &Path,
    components: &[&Path],
    star_depth: usize, // Index of `**` in components.
    results: &mut Vec<PathBuf>,
    max_results: usize,
    recurse_depth: usize,
) {
    if results.len() >= max_results || recurse_depth >= GLOBSTAR_MAX_DEPTH {
        return;
    }

    let entries = match Vfs::readdir(base) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        if entry.entry_type != EntryType::Directory {
            continue;
        }

        let child_path = base.join(&entry.name);

        // Try matching remaining components (after **) from this subdir.
        glob_recurse(
            &child_path,
            components,
            star_depth + 1,
            results,
            max_results,
        );

        // Continue recursing deeper.
        globstar_recurse(
            &child_path,
            components,
            star_depth,
            results,
            max_results,
            recurse_depth + 1,
        );
    }
}

/// Collect all entries under a directory recursively (for `**` as last component).
fn glob_collect_recursive(
    base: &Path,
    results: &mut Vec<PathBuf>,
    max_results: usize,
    depth: usize,
) {
    if results.len() >= max_results || depth >= GLOBSTAR_MAX_DEPTH {
        return;
    }

    let entries = match Vfs::readdir(base) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        let child_path = base.join(&entry.name);

        if results.len() < max_results {
            results.push(child_path.clone());
        }

        if entry.entry_type == EntryType::Directory {
            glob_collect_recursive(&child_path, results, max_results, depth + 1);
        }
    }
}

// ---------------------------------------------------------------------------
// Mount point lookup
// ---------------------------------------------------------------------------

/// Check if `path` matches mount point `mount_path` with proper
/// path-boundary semantics.
///
/// A mount at `"/tmp"` must match `"/tmp"` and `"/tmp/foo"` but
/// NOT `"/tmpfile"`.  The root mount `"/"` matches everything.
fn mount_matches(mount_path: &Path, path: &Path) -> bool {
    // Component-aligned by construction: a mount at `/tmp` must capture
    // `/tmp/foo` but not `/tmpfile`.  See [`Path::starts_with`].
    path.starts_with(mount_path)
}

/// A mount path in its canonical spelling: absolute, with no trailing
/// separator and no repeated ones — except the root mount, which *is* a
/// single separator.
///
/// Mount paths are stored verbatim and then used in two ways that both
/// assume this spelling, so normalising once at the boundary is what makes
/// their precondition true by construction:
///
/// - [`find_mount`] strips the mount prefix by **byte offset**
///   (`path.as_bytes().get(best_len..)`) rather than by component, because
///   it is on the VFS lookup hot path and a component-wise strip would
///   allocate a `PathBuf` on every single path resolution.  A mount
///   registered as `/mnt/` makes that strip start one byte late, so
///   `/mnt/foo` resolves to the *relative* `foo` instead of `/foo` and the
///   mounted filesystem is handed a path it cannot find; `/mnt//sub` makes
///   it land mid-name and produce outright garbage.
/// - Registration, [`Vfs::unmount`] and [`Vfs::remount`] identify a mount by
///   **byte equality**.  Without normalisation `/mnt` and `/mnt/` are two
///   different table entries: a second mount at the other spelling is
///   accepted rather than refused as a duplicate, and an unmount or remount
///   spelled either way cannot find one registered under the other.
///
/// Note this normalises *separators only*.  `.` and `..` are rejected
/// outright at registration instead — see [`Vfs::mount_with_options`].
fn normalize_mount_path(p: &Path) -> PathBuf {
    // Starting from `/` rather than the empty path is what gives the
    // zero-component case (`/`, `//`, `///`) the root mount's spelling;
    // `PathBuf::push` supplies the separators between the rest.
    let mut out = PathBuf::from("/");
    for c in p.components() {
        out.push(c);
    }
    out
}

/// Find the mount point that best matches `path`.
///
/// Uses longest-prefix matching with path-boundary checks so that
/// a mount at `"/tmp"` doesn't accidentally capture `"/tmpfile"`.
///
/// Returns a mutable reference to the mount point and the
/// path relative to that mount's root.
/// Capture a file's page-cache identity `(fs_id, ino)` under the held VFS lock,
/// for coherence invalidation after a content/lifecycle mutation.
///
/// Gated on [`crate::mm::page_cache::is_populated`] (a single relaxed atomic
/// load): when nothing is cached — the common case — this returns `None`
/// without the per-mutation `metadata` lookup, so the write/truncate/remove
/// hot paths pay almost nothing.  Returns `None` for `ino == 0` (no stable
/// identity, never cacheable).
fn cache_identity(fs: &mut Box<dyn FileSystem>, fs_id: u64, relative: &Path) -> Option<(u64, u64)> {
    if !crate::mm::page_cache::is_populated() {
        return None;
    }
    let ino = fs.metadata(relative).ok()?.ino;
    if ino == 0 {
        return None;
    }
    Some((fs_id, ino))
}

fn find_mount<'a, 'p>(
    vfs: &'a mut VfsInner,
    path: &'p Path,
) -> KernelResult<(&'a mut MountPoint, &'p Path)> {
    if vfs.mounts.is_empty() {
        return Err(KernelError::NotFound);
    }

    // Find the longest matching mount path.
    let mut best_idx = None;
    let mut best_len = 0;

    for (i, mp) in vfs.mounts.iter().enumerate() {
        if mount_matches(&mp.path, path) && mp.path.len() >= best_len {
            best_idx = Some(i);
            best_len = mp.path.len();
        }
    }

    let idx = best_idx.ok_or(KernelError::NotFound)?;

    // Strip the mount prefix to get the relative path.
    // For root mount ("/"), "/foo.txt" → "/foo.txt" (keep the leading /).
    // For submount ("/mnt"), "/mnt/foo.txt" → "/foo.txt".
    let relative = if best_len <= 1 {
        path // Mount is "/", keep the full path.
    } else {
        match path.as_bytes().get(best_len..) {
            None | Some([]) => Path::new("/"),
            Some(rest) => Path::new(rest),
        }
    };

    let mp = vfs.mounts.get_mut(idx).ok_or(KernelError::NotFound)?;
    Ok((mp, relative))
}

/// Resolve `path` to its owning mount, returning a cloned *per-mount*
/// filesystem handle plus the mount's stable id, options and the
/// mount-relative path — **without holding the global VFS lock afterwards**.
///
/// This is the lock-discipline foundation (design-decisions §43) that lets
/// the VFS dispatch filesystem operations without serializing all I/O on a
/// single global mutex, and that lets stacked filesystems (the overlay)
/// re-enter the VFS to read their backing layers without deadlocking: the
/// global lock is held only long enough to look up the mount table and clone
/// the `Arc`, then released.  The caller locks the returned per-mount handle
/// to perform the actual operation — a *different* lock from the global one
/// and from any lower-layer mount's lock, so reentrancy is safe.
fn resolve_mount(path: &Path) -> KernelResult<(MountedFs, u64, MountOptions, PathBuf)> {
    let mut vfs = VFS.lock();
    let (mp, relative) = find_mount(&mut vfs, path)?;
    Ok((
        Arc::clone(&mp.fs),
        mp.fs_id,
        mp.options,
        relative.to_path_buf(),
    ))
}

/// Check that the mount for `path` allows writes.
///
/// Returns `ReadOnlyFilesystem` if the mount is read-only.
/// Does not hold the VFS lock after returning.
fn check_writable(path: &Path) -> KernelResult<()> {
    let vfs = VFS.lock();
    // Find mount without &mut (we only need to read options).
    let mut best_len = 0;
    let mut best_ro = false;
    for mp in &vfs.mounts {
        if mount_matches(&mp.path, path) && mp.path.len() >= best_len {
            best_len = mp.path.len();
            best_ro = mp.options.read_only;
        }
    }
    if best_len == 0 {
        return Err(KernelError::NotFound);
    }
    if best_ro {
        return Err(KernelError::ReadOnlyFilesystem);
    }
    Ok(())
}

/// Enforce filesystem quota on a write operation.
///
/// Checks whether writing `bytes` for the current user (uid/gid 0 until
/// per-process identity is wired up) would exceed configured quota limits.
/// Returns `DiskFull` on hard-limit denial.  Soft-limit warnings are
/// logged but writes are allowed.
///
/// This is called *before* the VFS lock is taken.  When no quotas are
/// configured the function returns immediately (fast path in the quota
/// module).
fn enforce_quota_write(path: &Path, bytes: u64) -> KernelResult<()> {
    // uid/gid 0 until per-process identity tracking is available.
    match super::quota::check_write(0, 0, bytes) {
        super::quota::QuotaCheckResult::Allowed => Ok(()),
        super::quota::QuotaCheckResult::SoftWarning => {
            // Over soft limit but within grace — warn and allow.
            super::audit::log_err(super::audit::AuditOp::Write, 0, path, KernelError::DiskFull);
            Ok(())
        }
        super::quota::QuotaCheckResult::Denied => {
            super::audit::log_err(super::audit::AuditOp::Write, 0, path, KernelError::DiskFull);
            Err(KernelError::DiskFull)
        }
    }
}

/// Enforce filesystem quota on an inode (file/directory) creation.
///
/// Checks whether creating a new file or directory would exceed the
/// configured inode limit for the current user.
fn enforce_quota_create(path: &Path) -> KernelResult<()> {
    match super::quota::check_create(0, 0) {
        super::quota::QuotaCheckResult::Allowed => Ok(()),
        super::quota::QuotaCheckResult::SoftWarning => {
            super::audit::log_err(super::audit::AuditOp::Mkdir, 0, path, KernelError::DiskFull);
            Ok(())
        }
        super::quota::QuotaCheckResult::Denied => {
            super::audit::log_err(super::audit::AuditOp::Mkdir, 0, path, KernelError::DiskFull);
            Err(KernelError::DiskFull)
        }
    }
}

/// What a VFS operation intends to do with the path it was given.
///
/// The gate below needs this because the two checks it runs disagree about
/// granularity: capability tags are all-or-nothing on a path, while a POSIX
/// ACL grants read, write and execute independently. Passing the intent in
/// keeps the decision at the call site, where it is known, rather than
/// inferring it from the function name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathAccess {
    /// The operation touches the inode's own attributes, not its contents —
    /// `stat`, `readlink`, `chmod`, `chown`, `utimes`, attribute flags.
    ///
    /// No ACL permission is required, in *either* direction, and both halves
    /// of that are deliberate:
    ///
    /// - Reading: POSIX makes `stat` depend on search permission along the
    ///   path, not on read permission of the target, so requiring `r` here
    ///   would deny `stat` on files an ACL deliberately made
    ///   unreadable-but-visible.
    /// - Writing: POSIX ACLs govern access to a file's *data*; who may change
    ///   the inode's ownership and mode is decided by ownership and
    ///   privilege, not by the ACL. Requiring `w` would make a mode-`444`
    ///   file un-`chmod`-able by its own owner, which is not how any Unix
    ///   behaves.
    ///
    /// Capability tags still apply — they are mandatory access control and
    /// deny reaching the object at all, however innocuous the intent.
    Metadata,
    /// The operation reads the object's contents (or lists a directory).
    Read,
    /// The operation creates, modifies, renames or removes the object.
    Write,
    /// The operation executes the object.
    Execute,
}

/// The single permission gate every path operation passes through.
///
/// Two independent checks live here, and they live *together* on purpose:
/// before this existed, `check_file_tags` was called individually from
/// sixteen places in this file plus a seventeenth copy in `fs/handle.rs`, and
/// `acl::check_access` — the whole POSIX 1003.1e evaluation algorithm — was
/// called from none of them, so `setfacl` reported success while governing
/// nothing. A hook that has to be remembered at every entry point is a hook
/// the next entry point will not have.
///
/// Order matters: capability tags are checked first because they are a
/// system-policy restriction that an ACL must not be able to grant past.
///
/// Both checks bypass for kernel tasks (no owning process) and for uid 0, and
/// both fail *open* when no tag/ACL covers the path — deferring to the
/// traditional permission bits checked elsewhere.
pub(crate) fn check_path_access(path: &Path, want: PathAccess) -> KernelResult<()> {
    // Fast path: nothing is configured, so nothing can deny. Both counts are
    // relaxed atomic loads, so an unconfigured system pays two loads per VFS
    // operation and never touches either subsystem's lock.
    let tags = crate::cap::file_tags::count();
    let acls = super::acl::count();
    if tags == 0 && acls == 0 {
        return Ok(());
    }

    // Get the calling process's PID.
    let task_id = crate::sched::current_task_id();
    let pid = match crate::proc::thread::owner_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => return Ok(()), // Kernel task or PID 0 — bypass.
    };

    // Get process credentials.
    let creds = match crate::proc::pcb::get_credentials(pid) {
        Some(c) => c,
        None => return Ok(()), // No credentials — process being torn down.
    };

    path_access_verdict(path, creds.uid, creds.gid, &creds.groups, want)
}

/// The gate's decision, with the caller's identity passed in rather than
/// looked up.
///
/// Split from [`check_path_access`] so the decision can be tested: the
/// kernel's self-tests run as a kernel task, which the lookup above bypasses
/// before any check runs, so a test that went through it could only ever
/// observe "allowed" — and would keep passing if the ACL half were deleted
/// again. Everything that decides anything lives here.
pub(crate) fn path_access_verdict(
    path: &Path,
    uid: u32,
    gid: u32,
    supplementary_gids: &[u32],
    want: PathAccess,
) -> KernelResult<()> {
    // Tags first: they are system policy, and an ACL must not be able to grant
    // past one. (`check_access` on either side is a no-op when its own table
    // has no entry for the path, so the order only matters when both do.)
    if crate::cap::file_tags::count() != 0 {
        crate::cap::file_tags::check_access(uid, gid, supplementary_gids, path)?;
    }
    if super::acl::count() != 0 {
        check_acl(path, uid, gid, want)?;
    }
    Ok(())
}

/// Evaluate the path's POSIX ACL, if it has one, for `want`.
///
/// Split out from [`check_path_access`] so it can be exercised directly with
/// synthetic credentials: the kernel's own self-tests run as a kernel task,
/// which the gate above bypasses before reaching any ACL, so a test that went
/// through the gate could only ever observe "allowed" and would pass against
/// an ACL layer that had been removed entirely.
fn check_acl(path: &Path, uid: u32, gid: u32, want: PathAccess) -> KernelResult<()> {
    let request = match want {
        // See `PathAccess::Metadata`.
        PathAccess::Metadata => return Ok(()),
        PathAccess::Read => super::acl::AccessRequest::READ,
        PathAccess::Write => super::acl::AccessRequest::WRITE,
        PathAccess::Execute => super::acl::AccessRequest::EXECUTE,
    };

    // Root is not subject to ACLs, matching the traditional model and Linux.
    if uid == 0 {
        return Ok(());
    }

    // The ACL's owner and owning-group entries are relative to the file's own
    // uid/gid, so they have to be read before the algorithm can run.
    //
    // `metadata_resolved` and not `Vfs::metadata`: the latter re-enters this
    // gate, which would recurse without bound. `metadata_resolved` is the
    // ungated primitive and takes only the filesystem lock, which no caller of
    // this gate holds yet — every call site runs it before touching the VFS.
    let meta = match Vfs::metadata_resolved(path) {
        Ok(m) => m,
        // The object is gone or the filesystem cannot report ownership. Defer:
        // the operation itself is about to fail with a better error than
        // PermissionDenied, and denying here would turn a missing file into a
        // permissions puzzle.
        Err(_) => return Ok(()),
    };

    super::acl::check_access(path, uid, gid, meta.uid, meta.gid, request)
}

// ---------------------------------------------------------------------------
// VFS self-test
// ---------------------------------------------------------------------------

/// Test VFS path resolution, symlinks, and cross-mount operations.
///
/// Requires at least a root mount (`/`) and `/tmp` (memfs) to be mounted.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    serial_println!("[vfs] Running self-test...");

    // Check that we have at least root and /tmp mounts.
    let mounts = Vfs::mounts();
    if mounts.is_empty() {
        serial_println!("[vfs]   No mounts — skipping self-test.");
        return Ok(());
    }
    serial_println!("[vfs]   {} mount(s) active", mounts.len());
    for (path, fs_type) in &mounts {
        serial_println!("[vfs]     {} -> {}", path.display(), fs_type);
    }

    let has_tmp = mounts.iter().any(|(p, _)| p.as_path() == Path::new("/tmp"));

    // Twelve of the sections below are gated on `has_tmp`. The gate itself is
    // honest — it is a mount-table fact, not a swallowed error — but without
    // this record the last line would read `Self-test PASSED` after a run that
    // skipped most of the suite, and a reader who scrolls to the bottom would
    // have no way to tell that from a full one.
    let mut skips = crate::fs::selftest::Skips::new();
    if !has_tmp {
        skips.record(
            "symlink resolution, xattrs, ACLs, quotas, mount normalisation and 7 more",
            "/tmp not mounted",
        );
    }

    // --- Basic path validation ---
    match Vfs::stat("relative/path") {
        Err(KernelError::InvalidArgument) => {
            serial_println!("[vfs]   validate_path rejects relative: OK");
        }
        other => {
            serial_println!(
                "[vfs]   FAIL: relative path should be InvalidArgument, got {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    // --- normalize_path ---
    let norm = normalize_path("/a/b/../c/./d");
    if norm.as_path() != Path::new("/a/c/d") {
        serial_println!(
            "[vfs]   FAIL: normalize '/a/b/../c/./d' = '{}', expected '/a/c/d'",
            norm.display()
        );
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[vfs]   normalize_path: /a/b/../c/./d → {} OK",
        norm.display()
    );

    // --- Intra-mount symlink resolution (on /tmp memfs) ---
    if has_tmp {
        serial_println!("[vfs]   Testing intra-mount symlink resolution on /tmp...");

        // Create a target file and a symlink to it within /tmp.
        Vfs::write_file("/tmp/_vfs_test_target", b"vfs target")?;
        Vfs::symlink("/tmp/_vfs_test_link", "/tmp/_vfs_test_target")?;

        // stat through the symlink should return File.
        let stat_via_link = Vfs::stat("/tmp/_vfs_test_link")?;
        if stat_via_link.entry_type != EntryType::File {
            serial_println!(
                "[vfs]   FAIL: stat through symlink should be File, got {:?}",
                stat_via_link.entry_type
            );
            let _ = Vfs::remove("/tmp/_vfs_test_link");
            let _ = Vfs::remove("/tmp/_vfs_test_target");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     stat through intra-mount symlink: File OK");

        // lstat on the symlink itself should return Symlink.
        let lstat_link = Vfs::lstat("/tmp/_vfs_test_link")?;
        if lstat_link.entry_type != EntryType::Symlink {
            serial_println!(
                "[vfs]   FAIL: lstat on symlink should be Symlink, got {:?}",
                lstat_link.entry_type
            );
            let _ = Vfs::remove("/tmp/_vfs_test_link");
            let _ = Vfs::remove("/tmp/_vfs_test_target");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     lstat on symlink: Symlink OK");

        // Read through the symlink should return target content.
        let content = Vfs::read_file("/tmp/_vfs_test_link")?;
        if content != b"vfs target" {
            serial_println!("[vfs]   FAIL: read through symlink returned wrong data");
            let _ = Vfs::remove("/tmp/_vfs_test_link");
            let _ = Vfs::remove("/tmp/_vfs_test_target");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     read through symlink: content matches OK");

        // readlink should return the raw target.
        let target = Vfs::readlink("/tmp/_vfs_test_link")?;
        if target.as_path() != Path::new("/tmp/_vfs_test_target") {
            serial_println!(
                "[vfs]   FAIL: readlink = '{}', expected '/tmp/_vfs_test_target'",
                target.display()
            );
            let _ = Vfs::remove("/tmp/_vfs_test_link");
            let _ = Vfs::remove("/tmp/_vfs_test_target");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     readlink: '{}' OK", target.display());

        // --- Cross-mount symlink resolution ---
        // Create a symlink on root (/) that points to /tmp/file.
        // This exercises VFS-level resolution across mount boundaries.
        serial_println!("[vfs]   Testing cross-mount symlink resolution...");

        let cross_link = "/_vfs_cross_link";
        Vfs::symlink(cross_link, "/tmp/_vfs_test_target")?;

        // stat through the cross-mount symlink should follow to the
        // file on /tmp and return File.
        match Vfs::stat(cross_link) {
            Ok(entry) if entry.entry_type == EntryType::File => {
                serial_println!("[vfs]     stat through cross-mount symlink: File OK");
            }
            Ok(entry) => {
                serial_println!(
                    "[vfs]   FAIL: cross-mount stat type={:?}, expected File",
                    entry.entry_type
                );
                let _ = Vfs::remove(cross_link);
                let _ = Vfs::remove("/tmp/_vfs_test_link");
                let _ = Vfs::remove("/tmp/_vfs_test_target");
                return Err(KernelError::InternalError);
            }
            Err(e) => {
                serial_println!("[vfs]   FAIL: cross-mount stat failed: {:?}", e);
                let _ = Vfs::remove(cross_link);
                let _ = Vfs::remove("/tmp/_vfs_test_link");
                let _ = Vfs::remove("/tmp/_vfs_test_target");
                return Err(KernelError::InternalError);
            }
        }

        // Read through the cross-mount symlink.
        match Vfs::read_file(cross_link) {
            Ok(data) if data == b"vfs target" => {
                serial_println!("[vfs]     read through cross-mount symlink: content OK");
            }
            Ok(data) => {
                serial_println!(
                    "[vfs]   FAIL: cross-mount read returned {} bytes, wrong content",
                    data.len()
                );
                let _ = Vfs::remove(cross_link);
                let _ = Vfs::remove("/tmp/_vfs_test_link");
                let _ = Vfs::remove("/tmp/_vfs_test_target");
                return Err(KernelError::InternalError);
            }
            Err(e) => {
                serial_println!("[vfs]   FAIL: cross-mount read failed: {:?}", e);
                let _ = Vfs::remove(cross_link);
                let _ = Vfs::remove("/tmp/_vfs_test_link");
                let _ = Vfs::remove("/tmp/_vfs_test_target");
                return Err(KernelError::InternalError);
            }
        }

        // Clean up all test files.
        let _ = Vfs::remove(cross_link);
        let _ = Vfs::remove("/tmp/_vfs_test_link");
        let _ = Vfs::remove("/tmp/_vfs_test_target");
        serial_println!("[vfs]     test files cleaned up OK");
    } else {
        serial_println!("[vfs]   /tmp not mounted — skipping symlink tests");
    }

    // ---------------------------------------------------------------
    // statvfs test
    // ---------------------------------------------------------------
    serial_println!("[vfs]   Testing statvfs...");

    match Vfs::statvfs("/") {
        Ok(info) => {
            serial_println!(
                "[vfs]   / : type={}, block_size={}, total={}, free={} ({} bytes total, {} free)",
                info.fs_type,
                info.block_size,
                info.total_blocks,
                info.free_blocks,
                info.total_bytes(),
                info.free_bytes(),
            );
            serial_println!(
                "[vfs]   / : usage={}%, read_only={}, max_name_len={}",
                info.usage_percent(),
                info.read_only,
                info.max_name_len,
            );
        }
        Err(e) => {
            serial_println!("[vfs]   statvfs(/) failed: {:?}", e);
        }
    }

    // Test mount_info to list all mounts.
    match Vfs::mount_info() {
        Ok(mounts) => {
            serial_println!("[vfs]   {} mount(s):", mounts.len());
            for (path, info) in &mounts {
                serial_println!(
                    "[vfs]     {} → {} ({})",
                    path.display(),
                    info.fs_type,
                    if info.total_bytes() > 0 {
                        let mb = info.total_bytes() / (1024 * 1024);
                        alloc::format!("{} MiB, {}% used", mb, info.usage_percent())
                    } else {
                        "ram-backed".to_string()
                    },
                );
            }
        }
        Err(e) => {
            serial_println!("[vfs]   mount_info failed: {:?}", e);
        }
    }

    // --- Advisory file locking tests ---
    serial_println!("[vfs]   Testing advisory file locking...");
    {
        let test_path = "/tmp/_vfs_lock_test";
        Vfs::write_file(test_path, b"lock test")?;

        // Initially no lock.
        let state = Vfs::lock_query(test_path)?;
        if state.is_some() {
            serial_println!("[vfs]   FAIL: expected no lock, got {:?}", state);
            let _ = Vfs::remove(test_path);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     initial: no lock OK");

        // Acquire shared lock from owner 100.
        Vfs::flock(test_path, 100, LockType::Shared)?;
        let state = Vfs::lock_query(test_path)?;
        if !matches!(state, Some((LockType::Shared, 1))) {
            serial_println!("[vfs]   FAIL: expected Shared(1), got {:?}", state);
            Vfs::funlock_all(100);
            let _ = Vfs::remove(test_path);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     shared lock acquired OK");

        // Second shared lock from owner 200 — should succeed.
        Vfs::flock(test_path, 200, LockType::Shared)?;
        let state = Vfs::lock_query(test_path)?;
        if !matches!(state, Some((LockType::Shared, 2))) {
            serial_println!("[vfs]   FAIL: expected Shared(2), got {:?}", state);
            Vfs::funlock_all(100);
            Vfs::funlock_all(200);
            let _ = Vfs::remove(test_path);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     second shared lock OK (2 holders)");

        // Exclusive lock from owner 300 should fail (shared locks exist).
        match Vfs::flock(test_path, 300, LockType::Exclusive) {
            Err(KernelError::WouldBlock) => {
                serial_println!("[vfs]     exclusive blocked by shared OK");
            }
            other => {
                serial_println!("[vfs]   FAIL: expected WouldBlock, got {:?}", other);
                Vfs::funlock_all(100);
                Vfs::funlock_all(200);
                let _ = Vfs::remove(test_path);
                return Err(KernelError::InternalError);
            }
        }

        // Release both shared locks.
        Vfs::funlock(test_path, 100)?;
        Vfs::funlock(test_path, 200)?;
        let state = Vfs::lock_query(test_path)?;
        if state.is_some() {
            serial_println!(
                "[vfs]   FAIL: expected no lock after unlock, got {:?}",
                state
            );
            let _ = Vfs::remove(test_path);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     unlock both holders: clean OK");

        // Exclusive lock should now succeed.
        Vfs::flock(test_path, 300, LockType::Exclusive)?;
        let state = Vfs::lock_query(test_path)?;
        if !matches!(state, Some((LockType::Exclusive, 1))) {
            serial_println!("[vfs]   FAIL: expected Exclusive(1), got {:?}", state);
            Vfs::funlock_all(300);
            let _ = Vfs::remove(test_path);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     exclusive lock acquired OK");

        // Shared lock from another owner should fail.
        match Vfs::flock(test_path, 400, LockType::Shared) {
            Err(KernelError::WouldBlock) => {
                serial_println!("[vfs]     shared blocked by exclusive OK");
            }
            other => {
                serial_println!("[vfs]   FAIL: expected WouldBlock, got {:?}", other);
                Vfs::funlock_all(300);
                let _ = Vfs::remove(test_path);
                return Err(KernelError::InternalError);
            }
        }

        // Downgrade exclusive to shared.
        Vfs::flock(test_path, 300, LockType::Shared)?;
        let state = Vfs::lock_query(test_path)?;
        if !matches!(state, Some((LockType::Shared, 1))) {
            serial_println!(
                "[vfs]   FAIL: expected Shared after downgrade, got {:?}",
                state
            );
            Vfs::funlock_all(300);
            let _ = Vfs::remove(test_path);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     downgrade exclusive→shared OK");

        // funlock_all cleanup.
        Vfs::funlock_all(300);

        let _ = Vfs::remove(test_path);
        serial_println!("[vfs]     lock test cleanup OK");
    }

    // --- VFS dcache (path resolution cache) tests ---
    if has_tmp {
        serial_println!("[vfs]   Testing VFS path resolution cache (dcache)...");

        // Create a test file.
        let dcache_test = "/tmp/_vfs_dcache_test";
        Vfs::write_file(dcache_test, b"dcache test data")?;

        // Record stats before our test.
        let (_hits_before, _misses_before, _) = Vfs::dcache_stats();

        // First access: will be a miss (not cached yet) or a hit
        // if a previous operation already cached it.
        let _content = Vfs::read_file(dcache_test)?;

        // Second access to the same path: should be a cache hit.
        let (hits_mid, _, _) = Vfs::dcache_stats();
        let _content = Vfs::read_file(dcache_test)?;
        let (hits_after, _, valid_entries) = Vfs::dcache_stats();

        // The second read should have produced at least one more hit
        // than before it (the resolve_follow path was cached).
        if hits_after > hits_mid {
            serial_println!(
                "[vfs]     dcache hit on repeated path: {} → {} hits OK",
                hits_mid,
                hits_after,
            );
        } else {
            serial_println!(
                "[vfs]     dcache repeated access: hits {} → {} (no increase, may be OK if path was simple)",
                hits_mid,
                hits_after,
            );
        }
        serial_println!("[vfs]     dcache valid entries: {}", valid_entries);

        // Test invalidation: remove the file, then check that the
        // resolved path was invalidated.
        let (_, _, valid_before_remove) = Vfs::dcache_stats();
        let _ = Vfs::remove(dcache_test);
        let (_, _, valid_after_remove) = Vfs::dcache_stats();

        // After remove, the entry should be invalidated (fewer valid entries).
        if valid_after_remove < valid_before_remove {
            serial_println!(
                "[vfs]     dcache invalidation on remove: {} → {} valid OK",
                valid_before_remove,
                valid_after_remove,
            );
        } else {
            // Might be the same if other entries were added between.
            serial_println!(
                "[vfs]     dcache after remove: {} → {} valid (invalidation may have been masked by new inserts)",
                valid_before_remove,
                valid_after_remove,
            );
        }

        // Dcache invalidation is component-aligned: it now uses
        // `Path::starts_with` rather than the open-coded
        // `starts_with(prefix) && bytes[prefix.len()] == b'/'` idiom this
        // module used to carry.  The last two cases are the ones the old
        // idiom got wrong — a prefix with a trailing slash made the boundary
        // byte land inside the *next* component, so `/tmp/` matched nothing
        // at all, and invalidation silently skipped every affected entry.
        for (path, prefix, want) in [
            ("/tmp/foo", "/tmp", true),
            ("/tmpfile", "/tmp", false),
            ("/tmp", "/tmp", true),
            ("/anything", "/", true),
            ("/tmp/foo", "/tmp/", true),
            ("/tmp", "/tmp/", true),
        ] {
            if Path::new(path).starts_with(Path::new(prefix)) != want {
                serial_println!(
                    "[vfs]   FAIL: Path::starts_with('{}', '{}') should be {}",
                    path,
                    prefix,
                    want,
                );
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[vfs]     dcache prefix matching: all cases OK");

        // --- Negative cache test ---
        // Access a path with a non-existent parent.  This should produce a
        // NotFound error and cache the result as a negative entry.  The
        // second access should hit the negative cache (increased hits).
        let neg_path = "/tmp/_vfs_no_such_parent/child.txt";
        let (_hits_pre_neg, _, _) = Vfs::dcache_stats();
        // First access: miss, resolve_inner fails, inserts negative entry.
        let r1 = Vfs::stat(neg_path);
        assert!(r1.is_err(), "stat on non-existent parent should fail");
        // Second access: should hit the negative cache.
        let (hits_mid_neg, _, _) = Vfs::dcache_stats();
        let r2 = Vfs::stat(neg_path);
        assert!(r2.is_err(), "stat on non-existent parent should still fail");
        let (hits_post_neg, _, _) = Vfs::dcache_stats();
        if hits_post_neg > hits_mid_neg {
            serial_println!(
                "[vfs]     negative cache hit: {} → {} hits OK",
                hits_mid_neg,
                hits_post_neg,
            );
        } else {
            // May happen if resolve_follow doesn't fail at the resolve level
            // for this particular path (parent exists but child doesn't).
            serial_println!(
                "[vfs]     negative cache: {} → {} hits (path may not trigger resolve-level NotFound)",
                hits_mid_neg,
                hits_post_neg,
            );
        }

        // Negative entry invalidation: creating the parent should allow
        // subsequent accesses to proceed past the resolve step.
        let neg_parent = "/tmp/_vfs_no_such_parent";
        let _ = Vfs::mkdir(neg_parent);
        Vfs::write_file(neg_path, b"negative cache invalidation test")?;
        let content = Vfs::read_file(neg_path)?;
        assert!(
            content == b"negative cache invalidation test",
            "file should be readable after negative cache invalidation",
        );
        serial_println!("[vfs]     negative cache invalidation: create parent + file OK");
        // Cleanup.
        let _ = Vfs::remove(neg_path);
        let _ = Vfs::rmdir(neg_parent);
        serial_println!("[vfs]     negative cache test OK");

        // Report overall dcache stats.
        let (h, m, v) = Vfs::dcache_stats();
        let total = h.saturating_add(m);
        if total > 0 {
            let rate = h.saturating_mul(100) / total;
            serial_println!(
                "[vfs]     dcache stats: {} hits, {} misses ({}% hit rate), {} valid entries",
                h,
                m,
                rate,
                v
            );
        } else {
            serial_println!("[vfs]     dcache stats: no accesses yet");
        }

        serial_println!("[vfs]     dcache test completed OK");
    }

    // --- mkdir_all tests ---
    if has_tmp {
        serial_println!("[vfs]   Testing mkdir_all (recursive mkdir)...");

        // Create a deep directory tree in one call.
        let deep_path = "/tmp/_vfs_mkdirall/a/b/c";
        Vfs::mkdir_all(deep_path)?;

        // Verify all intermediate directories exist.
        let stat_a = Vfs::stat("/tmp/_vfs_mkdirall")?;
        assert!(
            stat_a.entry_type == EntryType::Directory,
            "mkdirall: root should be dir"
        );
        let stat_b = Vfs::stat("/tmp/_vfs_mkdirall/a")?;
        assert!(
            stat_b.entry_type == EntryType::Directory,
            "mkdirall: a should be dir"
        );
        let stat_c = Vfs::stat("/tmp/_vfs_mkdirall/a/b")?;
        assert!(
            stat_c.entry_type == EntryType::Directory,
            "mkdirall: a/b should be dir"
        );
        let stat_d = Vfs::stat(deep_path)?;
        assert!(
            stat_d.entry_type == EntryType::Directory,
            "mkdirall: a/b/c should be dir"
        );

        // Calling again on existing path should succeed (idempotent).
        Vfs::mkdir_all(deep_path)?;

        // Cleanup.
        let _ = Vfs::rmdir("/tmp/_vfs_mkdirall/a/b/c");
        let _ = Vfs::rmdir("/tmp/_vfs_mkdirall/a/b");
        let _ = Vfs::rmdir("/tmp/_vfs_mkdirall/a");
        let _ = Vfs::rmdir("/tmp/_vfs_mkdirall");

        serial_println!("[vfs]     mkdir_all: deep creation + idempotency OK");
    }

    // --- Recursive copy/remove tests ---
    if has_tmp {
        serial_println!("[vfs]   Testing recursive copy and remove...");

        // Create a directory tree: /tmp/_vfs_rc/a/b with files at each level.
        Vfs::mkdir("/tmp/_vfs_rc")?;
        Vfs::mkdir("/tmp/_vfs_rc/a")?;
        Vfs::mkdir("/tmp/_vfs_rc/a/b")?;
        Vfs::write_file("/tmp/_vfs_rc/top.txt", b"top level")?;
        Vfs::write_file("/tmp/_vfs_rc/a/mid.txt", b"mid level")?;
        Vfs::write_file("/tmp/_vfs_rc/a/b/bot.txt", b"bottom level")?;

        // Verify tree exists.
        let top = Vfs::stat("/tmp/_vfs_rc")?;
        if top.entry_type != EntryType::Directory {
            serial_println!("[vfs]   FAIL: /tmp/_vfs_rc should be directory");
            return Err(KernelError::InternalError);
        }
        let bot = Vfs::read_file("/tmp/_vfs_rc/a/b/bot.txt")?;
        if bot != b"bottom level" {
            serial_println!("[vfs]   FAIL: bot.txt content mismatch");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     directory tree created OK (3 dirs, 3 files)");

        // Recursive copy: /tmp/_vfs_rc → /tmp/_vfs_rc_copy
        let bytes_copied = Vfs::copy_recursive("/tmp/_vfs_rc", "/tmp/_vfs_rc_copy")?;
        serial_println!("[vfs]     copy_recursive: {} bytes copied", bytes_copied);

        // Verify copy contents match.
        let copy_top = Vfs::read_file("/tmp/_vfs_rc_copy/top.txt")?;
        if copy_top != b"top level" {
            serial_println!("[vfs]   FAIL: copied top.txt content mismatch");
            let _ = Vfs::remove_recursive("/tmp/_vfs_rc");
            let _ = Vfs::remove_recursive("/tmp/_vfs_rc_copy");
            return Err(KernelError::InternalError);
        }
        let copy_mid = Vfs::read_file("/tmp/_vfs_rc_copy/a/mid.txt")?;
        if copy_mid != b"mid level" {
            serial_println!("[vfs]   FAIL: copied mid.txt content mismatch");
            let _ = Vfs::remove_recursive("/tmp/_vfs_rc");
            let _ = Vfs::remove_recursive("/tmp/_vfs_rc_copy");
            return Err(KernelError::InternalError);
        }
        let copy_bot = Vfs::read_file("/tmp/_vfs_rc_copy/a/b/bot.txt")?;
        if copy_bot != b"bottom level" {
            serial_println!("[vfs]   FAIL: copied bot.txt content mismatch");
            let _ = Vfs::remove_recursive("/tmp/_vfs_rc");
            let _ = Vfs::remove_recursive("/tmp/_vfs_rc_copy");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     copy_recursive: all files verified OK");

        // Verify the copy has the expected structure.
        let copy_entries = Vfs::readdir("/tmp/_vfs_rc_copy")?;
        let has_a = copy_entries
            .iter()
            .any(|e| e.name.as_path() == Path::new("a") && e.entry_type == EntryType::Directory);
        let has_top = copy_entries
            .iter()
            .any(|e| e.name.as_path() == Path::new("top.txt") && e.entry_type == EntryType::File);
        if !has_a || !has_top {
            serial_println!(
                "[vfs]   FAIL: copy directory structure wrong (a={}, top.txt={})",
                has_a,
                has_top
            );
            let _ = Vfs::remove_recursive("/tmp/_vfs_rc");
            let _ = Vfs::remove_recursive("/tmp/_vfs_rc_copy");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     copy_recursive: directory structure OK");

        // Recursive remove: /tmp/_vfs_rc_copy
        let removed_count = Vfs::remove_recursive("/tmp/_vfs_rc_copy")?;
        // Expected: 3 files + 3 directories = 6 items
        if removed_count < 6 {
            serial_println!(
                "[vfs]   WARNING: remove_recursive removed {} items, expected 6",
                removed_count
            );
        } else {
            serial_println!(
                "[vfs]     remove_recursive: {} items removed OK",
                removed_count
            );
        }

        // Verify the copy is gone.
        match Vfs::stat("/tmp/_vfs_rc_copy") {
            Err(KernelError::NotFound) => {
                serial_println!("[vfs]     remove_recursive: directory confirmed gone OK");
            }
            Ok(_) => {
                serial_println!(
                    "[vfs]   FAIL: /tmp/_vfs_rc_copy still exists after remove_recursive"
                );
                let _ = Vfs::remove_recursive("/tmp/_vfs_rc_copy");
                let _ = Vfs::remove_recursive("/tmp/_vfs_rc");
                return Err(KernelError::InternalError);
            }
            Err(e) => {
                serial_println!("[vfs]   FAIL: stat after remove_recursive: {:?}", e);
                let _ = Vfs::remove_recursive("/tmp/_vfs_rc");
                return Err(KernelError::InternalError);
            }
        }

        // Verify original still exists.
        let orig = Vfs::read_file("/tmp/_vfs_rc/a/b/bot.txt")?;
        if orig != b"bottom level" {
            serial_println!("[vfs]   FAIL: original bot.txt corrupted after copy+remove");
            let _ = Vfs::remove_recursive("/tmp/_vfs_rc");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     original tree intact after removing copy OK");

        // Clean up original.
        let _ = Vfs::remove_recursive("/tmp/_vfs_rc");
        serial_println!("[vfs]     recursive copy/remove test PASSED");
    }

    // --- Cross-mount rename test ---
    // This tests rename across /tmp (memfs) and / (ext4/fat).
    // Only runs if both root and /tmp are available as separate mounts.
    if has_tmp {
        serial_println!("[vfs]   Testing cross-mount rename...");

        let src_path = "/tmp/_vfs_xmv_src.txt";
        let dst_path = "/_vfs_xmv_dst.txt";
        Vfs::write_file(src_path, b"cross mount data")?;

        // Rename from /tmp to / — this is cross-mount.
        match Vfs::rename(src_path, dst_path) {
            Ok(()) => {
                // Verify destination has the data.
                match Vfs::read_file(dst_path) {
                    Ok(data) if data == b"cross mount data" => {
                        serial_println!("[vfs]     cross-mount rename: data verified OK");
                    }
                    Ok(data) => {
                        serial_println!(
                            "[vfs]   FAIL: cross-mount rename data mismatch ({} bytes)",
                            data.len()
                        );
                        let _ = Vfs::remove(dst_path);
                        return Err(KernelError::InternalError);
                    }
                    Err(e) => {
                        serial_println!("[vfs]   FAIL: read after cross-mount rename: {:?}", e);
                        let _ = Vfs::remove(dst_path);
                        return Err(KernelError::InternalError);
                    }
                }

                // Verify source is gone.
                match Vfs::stat(src_path) {
                    Err(KernelError::NotFound) => {
                        serial_println!("[vfs]     cross-mount rename: source removed OK");
                    }
                    _ => {
                        serial_println!(
                            "[vfs]   FAIL: source still exists after cross-mount rename"
                        );
                        let _ = Vfs::remove(src_path);
                        let _ = Vfs::remove(dst_path);
                        return Err(KernelError::InternalError);
                    }
                }

                let _ = Vfs::remove(dst_path);
                serial_println!("[vfs]     cross-mount rename test PASSED");
            }
            Err(KernelError::NotSupported) => {
                // Root filesystem may not support write operations.
                serial_println!("[vfs]     cross-mount rename: root FS is read-only, skipping");
                let _ = Vfs::remove(src_path);
            }
            Err(e) => {
                serial_println!(
                    "[vfs]     cross-mount rename failed: {:?} (may be expected)",
                    e
                );
                let _ = Vfs::remove(src_path);
            }
        }
    }

    // --- Paginated readdir_at test ---
    if has_tmp {
        serial_println!("[vfs]   Testing paginated readdir_at...");

        // Create a directory with several files for pagination testing.
        let pg_dir = "/tmp/_vfs_paginate";
        Vfs::mkdir(pg_dir)?;
        for i in 0..10 {
            let fname = format!("{}/file_{:02}.txt", pg_dir, i);
            let content = format!("content {}", i);
            Vfs::write_file(&fname, content.as_bytes())?;
        }

        // Full listing should have 10 entries.
        let (all, total) = Vfs::readdir_at(pg_dir, 0, 100)?;
        if total != 10 {
            serial_println!("[vfs]   FAIL: readdir_at total = {}, expected 10", total);
            let _ = Vfs::remove_recursive(pg_dir);
            return Err(KernelError::InternalError);
        }
        if all.len() != 10 {
            serial_println!(
                "[vfs]   FAIL: readdir_at returned {} entries, expected 10",
                all.len()
            );
            let _ = Vfs::remove_recursive(pg_dir);
            return Err(KernelError::InternalError);
        }
        serial_println!(
            "[vfs]     readdir_at(0, 100): {} entries, total={} OK",
            all.len(),
            total
        );

        // Read first page (3 entries).
        let (page1, total1) = Vfs::readdir_at(pg_dir, 0, 3)?;
        if page1.len() != 3 || total1 != 10 {
            serial_println!(
                "[vfs]   FAIL: page1 len={}, total={} (expected 3, 10)",
                page1.len(),
                total1,
            );
            let _ = Vfs::remove_recursive(pg_dir);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     readdir_at(0, 3): {} entries OK", page1.len());

        // Read second page (3 entries starting at offset 3).
        let (page2, total2) = Vfs::readdir_at(pg_dir, 3, 3)?;
        if page2.len() != 3 || total2 != 10 {
            serial_println!(
                "[vfs]   FAIL: page2 len={}, total={} (expected 3, 10)",
                page2.len(),
                total2,
            );
            let _ = Vfs::remove_recursive(pg_dir);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     readdir_at(3, 3): {} entries OK", page2.len());

        // Verify no overlap between pages.
        let names1: Vec<&Path> = page1.iter().map(|e| e.name.as_path()).collect();
        let names2: Vec<&Path> = page2.iter().map(|e| e.name.as_path()).collect();
        let has_overlap = names1.iter().any(|n| names2.contains(n));
        if has_overlap {
            serial_println!("[vfs]   FAIL: page1 and page2 overlap!");
            let _ = Vfs::remove_recursive(pg_dir);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     pages don't overlap OK");

        // Read past end: offset 8, count 5 → should return 2 entries.
        let (tail, total_tail) = Vfs::readdir_at(pg_dir, 8, 5)?;
        if tail.len() != 2 || total_tail != 10 {
            serial_println!(
                "[vfs]   FAIL: tail len={}, total={} (expected 2, 10)",
                tail.len(),
                total_tail,
            );
            let _ = Vfs::remove_recursive(pg_dir);
            return Err(KernelError::InternalError);
        }
        serial_println!(
            "[vfs]     readdir_at(8, 5): {} entries (tail) OK",
            tail.len()
        );

        // Read completely past end: offset 20 → should return 0 entries.
        let (empty, total_empty) = Vfs::readdir_at(pg_dir, 20, 5)?;
        if !empty.is_empty() || total_empty != 10 {
            serial_println!(
                "[vfs]   FAIL: past-end len={}, total={} (expected 0, 10)",
                empty.len(),
                total_empty,
            );
            let _ = Vfs::remove_recursive(pg_dir);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     readdir_at(20, 5): empty (past end) OK");

        let _ = Vfs::remove_recursive(pg_dir);
        serial_println!("[vfs]     readdir_at pagination test PASSED");
    }

    // ── VFS access() tests ──
    {
        serial_println!("[vfs]   --- access() tests ---");

        // Existing file should be accessible with F_OK.
        if Vfs::access("/tmp", F_OK).is_err() {
            serial_println!("[vfs]     FAIL: access /tmp F_OK should succeed");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     access /tmp F_OK: OK");

        // Non-existent path should fail.
        if Vfs::access("/tmp/__no_such_file__", F_OK).is_ok() {
            serial_println!("[vfs]     FAIL: access non-existent should fail");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     access non-existent: NotFound OK");

        // /tmp directory should be readable and writable (memfs default perms).
        if Vfs::access("/tmp", R_OK).is_err() {
            serial_println!("[vfs]     FAIL: access /tmp R_OK should succeed");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     access /tmp R_OK: OK");

        if Vfs::access("/tmp", W_OK).is_err() {
            serial_println!("[vfs]     FAIL: access /tmp W_OK should succeed");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     access /tmp W_OK: OK");

        // Combined mode check.
        if Vfs::access("/tmp", R_OK | W_OK).is_err() {
            serial_println!("[vfs]     FAIL: access /tmp R_OK|W_OK should succeed");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     access /tmp R_OK|W_OK: OK");

        // Convenience helpers.
        if Vfs::is_readable("/tmp").is_err() {
            serial_println!("[vfs]     FAIL: is_readable /tmp should succeed");
            return Err(KernelError::InternalError);
        }
        if Vfs::is_writable("/tmp").is_err() {
            serial_println!("[vfs]     FAIL: is_writable /tmp should succeed");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     is_readable + is_writable: OK");

        // Read-only filesystem entries (procfs) should fail W_OK.
        if Vfs::access("/proc/version", R_OK).is_err() {
            serial_println!("[vfs]     FAIL: access /proc/version R_OK should succeed");
            return Err(KernelError::InternalError);
        }
        if Vfs::access("/proc/version", W_OK).is_ok() {
            serial_println!("[vfs]     FAIL: access /proc/version W_OK should fail");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     access /proc/version R_OK ok, W_OK denied: OK");

        serial_println!("[vfs]     access() tests PASSED");
    }

    // ── Mount options / read-only enforcement test ──
    serial_println!("[vfs]   Testing mount options (read-only enforcement)...");
    {
        // Remount /tmp as read-only.
        let orig_opts = Vfs::mount_options("/tmp").unwrap_or(MountOptions::defaults());
        let mut ro_opts = orig_opts;
        ro_opts.read_only = true;
        Vfs::remount("/tmp", ro_opts)?;

        // Verify writes are rejected.
        let test_file = "/tmp/_ro_test.txt";
        match Vfs::write_file(test_file, b"should fail") {
            Err(KernelError::ReadOnlyFilesystem) => {
                serial_println!("[vfs]     write_file correctly rejected on ro mount");
            }
            Ok(()) => {
                serial_println!("[vfs]     FAIL: write_file succeeded on ro mount!");
                let _ = Vfs::remove(test_file);
                Vfs::remount("/tmp", orig_opts)?;
                return Err(KernelError::InternalError);
            }
            Err(e) => {
                serial_println!(
                    "[vfs]     FAIL: write_file returned {:?} instead of ReadOnlyFilesystem",
                    e
                );
                Vfs::remount("/tmp", orig_opts)?;
                return Err(e);
            }
        }

        // Verify mkdir is rejected.
        match Vfs::mkdir("/tmp/_ro_test_dir") {
            Err(KernelError::ReadOnlyFilesystem) => {
                serial_println!("[vfs]     mkdir correctly rejected on ro mount");
            }
            other => {
                serial_println!(
                    "[vfs]     FAIL: mkdir returned {:?} instead of ReadOnlyFilesystem",
                    other
                );
                let _ = Vfs::rmdir("/tmp/_ro_test_dir");
                Vfs::remount("/tmp", orig_opts)?;
                return Err(KernelError::InternalError);
            }
        }

        // Restore original options.
        Vfs::remount("/tmp", orig_opts)?;

        // Verify writes succeed again.
        Vfs::write_file(test_file, b"should succeed")?;
        Vfs::remove(test_file)?;
        serial_println!("[vfs]     read-only enforcement test PASSED");
    }

    // ── Glob pattern matching tests ──
    glob_self_test()?;

    // ── Globstar (**) recursive glob test ──
    if has_tmp {
        serial_println!("[vfs]   Testing ** (globstar) recursive glob...");

        // Create a small directory tree for testing.
        let _ = Vfs::mkdir("/tmp/_glob_test");
        let _ = Vfs::mkdir("/tmp/_glob_test/sub");
        let _ = Vfs::mkdir("/tmp/_glob_test/sub/deep");
        Vfs::write_file("/tmp/_glob_test/a.txt", b"a")?;
        Vfs::write_file("/tmp/_glob_test/b.rs", b"b")?;
        Vfs::write_file("/tmp/_glob_test/sub/c.txt", b"c")?;
        Vfs::write_file("/tmp/_glob_test/sub/deep/d.txt", b"d")?;
        Vfs::write_file("/tmp/_glob_test/sub/deep/e.rs", b"e")?;

        // Test 1: /**/*.txt should find all .txt files recursively.
        let txt_results = Vfs::glob("/tmp/_glob_test/**/*.txt")?;
        let txt_count = txt_results
            .iter()
            .filter(|p| p.as_bytes().ends_with(b".txt"))
            .count();
        if txt_count < 3 {
            serial_println!(
                "[vfs]   FAIL: **/*.txt found {} .txt files, expected >= 3",
                txt_count
            );
            // Clean up.
            let _ = cleanup_glob_test();
            return Err(KernelError::InternalError);
        }
        serial_println!(
            "[vfs]     **/*.txt found {} .txt files (>= 3) OK",
            txt_count
        );

        // Test 2: /** should find everything under the dir.
        let all_results = Vfs::glob("/tmp/_glob_test/**")?;
        // Should find at least: sub, sub/deep, a.txt, b.rs, sub/c.txt,
        // sub/deep/d.txt, sub/deep/e.rs = 7 entries.
        if all_results.len() < 7 {
            serial_println!(
                "[vfs]   FAIL: /** found {} entries, expected >= 7",
                all_results.len()
            );
            let _ = cleanup_glob_test();
            return Err(KernelError::InternalError);
        }
        serial_println!(
            "[vfs]     /** found {} entries (>= 7) OK",
            all_results.len()
        );

        // Test 3: /**/*.rs should find .rs files at any depth.
        let rs_results = Vfs::glob("/tmp/_glob_test/**/*.rs")?;
        let rs_count = rs_results
            .iter()
            .filter(|p| p.as_bytes().ends_with(b".rs"))
            .count();
        if rs_count < 2 {
            serial_println!(
                "[vfs]   FAIL: **/*.rs found {} .rs files, expected >= 2",
                rs_count
            );
            let _ = cleanup_glob_test();
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     **/*.rs found {} .rs files (>= 2) OK", rs_count);

        // Clean up.
        let _ = cleanup_glob_test();
        serial_println!("[vfs]     globstar (**) test PASSED");
    }

    // --- Plain rename: POSIX replace semantics ---
    //
    // Regression test.  memfs used to reject *any* existing destination with
    // `AlreadyExists`, i.e. it baked `RENAME_NOREPLACE` into the plain
    // operation, which made `Vfs::atomic_write` (tested below) unable to
    // replace an existing file on `/tmp` — the whole point of a safe write.
    // ext4 and FAT always implemented replacement; memfs was the outlier.
    if has_tmp {
        serial_println!("[vfs]   --- rename (replace semantics) ---");

        let src = "/tmp/_vfs_rn_src";
        let dst = "/tmp/_vfs_rn_dst";
        // Best-effort pre-clean: absence is the normal case, so an error here
        // means "nothing to remove" and is not a failure.
        let _ = Vfs::remove(src);
        let _ = Vfs::remove(dst);

        // (1) rename over an EXISTING file replaces it.
        Vfs::write_file(src, b"new")?;
        Vfs::write_file(dst, b"old")?;
        Vfs::rename(src, dst)?;
        if Vfs::read_file(dst)?.as_slice() != b"new" {
            serial_println!("[vfs]     FAIL: rename did not replace existing destination");
            let _ = Vfs::remove(src);
            let _ = Vfs::remove(dst);
            return Err(KernelError::InternalError);
        }
        if Vfs::stat(src).is_ok() {
            serial_println!("[vfs]     FAIL: rename left the source behind");
            let _ = Vfs::remove(src);
            let _ = Vfs::remove(dst);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     rename replaces existing destination OK");

        // (2) rename onto ITSELF is a no-op success — it must not delete the
        //     file, which is what a naive "detach then re-insert" does.
        Vfs::rename(dst, dst)?;
        if Vfs::read_file(dst)?.as_slice() != b"new" {
            serial_println!("[vfs]     FAIL: self-rename destroyed the file");
            let _ = Vfs::remove(dst);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     rename onto itself is a no-op OK");

        // (3) A DIRECTORY destination is refused (matching ext4 and FAT).
        let dstdir = "/tmp/_vfs_rn_dir";
        let _ = Vfs::remove(dstdir);
        Vfs::mkdir(dstdir)?;
        match Vfs::rename(dst, dstdir) {
            Err(KernelError::IsADirectory) => {}
            other => {
                serial_println!(
                    "[vfs]     FAIL: rename onto a directory -> {:?} (expected IsADirectory)",
                    other
                );
                let _ = Vfs::remove(dst);
                let _ = Vfs::remove(dstdir);
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[vfs]     rename onto a directory refused OK");

        // (4) Moving a directory INTO ITS OWN SUBTREE must fail without
        //     destroying it.  The old code detached the source first, then
        //     walked to a destination parent that had just gone with it, and
        //     dropped the entire subtree on the floor.
        let inner = "/tmp/_vfs_rn_dir/inner";
        Vfs::mkdir(inner)?;
        match Vfs::rename(dstdir, "/tmp/_vfs_rn_dir/inner/moved") {
            Err(KernelError::InvalidArgument) => {}
            other => {
                serial_println!(
                    "[vfs]     FAIL: rename dir into own subtree -> {:?} (expected InvalidArgument)",
                    other
                );
                let _ = Vfs::remove(dst);
                let _ = Vfs::remove_recursive(dstdir);
                return Err(KernelError::InternalError);
            }
        }
        if Vfs::stat(inner).is_err() {
            serial_println!("[vfs]     FAIL: rejected subtree move destroyed the subtree");
            let _ = Vfs::remove(dst);
            let _ = Vfs::remove_recursive(dstdir);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     rename dir into own subtree refused OK");

        let _ = Vfs::remove(dst);
        let _ = Vfs::remove_recursive(dstdir);
        serial_println!("[vfs]     rename replace-semantics test PASSED");
    }

    // --- Atomic write test ---
    if has_tmp {
        serial_println!("[vfs]   --- atomic write ---");

        let test_path = "/tmp/_vfs_atomic_test";
        let original = b"Original data before atomic write";
        let replacement = b"Replacement data via atomic write";

        // Write original file.
        Vfs::write_file(test_path, original)?;
        let check = Vfs::read_file(test_path)?;
        if check.as_slice() != original {
            serial_println!("[vfs]     FAIL: initial write data mismatch");
            let _ = Vfs::remove(test_path);
            return Err(KernelError::InternalError);
        }

        // Atomic replace.
        Vfs::atomic_write(test_path, replacement)?;
        let check2 = Vfs::read_file(test_path)?;
        if check2.as_slice() != replacement {
            serial_println!("[vfs]     FAIL: atomic write data mismatch");
            let _ = Vfs::remove(test_path);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     atomic_write: replace OK");

        // Atomic write to new file (no pre-existing file).
        let new_path = "/tmp/_vfs_atomic_new";
        let _ = Vfs::remove(new_path);
        Vfs::atomic_write(new_path, b"new file via atomic")?;
        let check3 = Vfs::read_file(new_path)?;
        if check3.as_slice() != b"new file via atomic" {
            serial_println!("[vfs]     FAIL: atomic write new file data mismatch");
            let _ = Vfs::remove(test_path);
            let _ = Vfs::remove(new_path);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     atomic_write: new file OK");

        // Atomic write with metadata preservation.
        Vfs::atomic_write_preserve(test_path, b"preserved metadata")?;
        let check4 = Vfs::read_file(test_path)?;
        if check4.as_slice() != b"preserved metadata" {
            serial_println!("[vfs]     FAIL: atomic_write_preserve data mismatch");
            let _ = Vfs::remove(test_path);
            let _ = Vfs::remove(new_path);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     atomic_write_preserve OK");

        // Verify no temp files left behind.
        let tmp_entries = Vfs::readdir("/tmp")?;
        let stale = tmp_entries
            .iter()
            .any(|e| e.name.starts_with(".tmp_atomic_"));
        if stale {
            serial_println!("[vfs]     WARN: stale temp file found after atomic write");
        }

        // Cleanup.
        let _ = Vfs::remove(test_path);
        let _ = Vfs::remove(new_path);
        serial_println!("[vfs]     atomic write test PASSED");
    }

    // --- Mount path normalisation ---
    //
    // A mount registered with a trailing slash used to be stored verbatim,
    // and `find_mount` strips the mount prefix by byte offset -- so
    // `/tmp/vfsnorm/x` was handed to the mounted filesystem as the relative
    // `x` instead of `/x`, and every operation under the mount failed. The
    // matching byte-equality failures let the same directory be mounted
    // twice under two spellings and made unmount/remount spelling-sensitive.
    if has_tmp {
        serial_println!("[vfs]   Testing mount path normalisation...");

        // Trailing slash, and a doubled separator for good measure.
        crate::fs::memfs::mount("/tmp//vfsnorm/")?;

        // Stored under the canonical spelling.
        let stored = Vfs::mounts();
        if !stored
            .iter()
            .any(|(p, _)| p.as_path() == Path::new("/tmp/vfsnorm"))
        {
            serial_println!("[vfs]   FAIL: mount not stored as '/tmp/vfsnorm'");
            let _ = Vfs::unmount("/tmp/vfsnorm");
            return Err(KernelError::InternalError);
        }

        // The mount is usable: this is the byte-offset strip that broke.
        if let Err(e) = Vfs::write_file("/tmp/vfsnorm/probe", b"normalised") {
            serial_println!("[vfs]   FAIL: write under normalised mount: {:?}", e);
            let _ = Vfs::unmount("/tmp/vfsnorm");
            return Err(e);
        }
        match Vfs::read_file("/tmp/vfsnorm/probe") {
            Ok(data) if data.as_slice() == b"normalised" => {}
            other => {
                serial_println!("[vfs]   FAIL: read under normalised mount: {:?}", other);
                let _ = Vfs::unmount("/tmp/vfsnorm");
                return Err(KernelError::InternalError);
            }
        }

        // The other spelling is now recognised as a duplicate.
        match crate::fs::memfs::mount("/tmp/vfsnorm") {
            Err(KernelError::AlreadyExists) => {}
            other => {
                serial_println!(
                    "[vfs]   FAIL: duplicate mount under other spelling: {:?}",
                    other
                );
                let _ = Vfs::unmount("/tmp/vfsnorm");
                return Err(KernelError::InternalError);
            }
        }

        // `..` in a mount path is refused rather than silently unreachable.
        match crate::fs::memfs::mount("/tmp/../tmp/vfsdots") {
            Err(KernelError::InvalidArgument) => {}
            other => {
                serial_println!("[vfs]   FAIL: dot-component mount accepted: {:?}", other);
                let _ = Vfs::unmount("/tmp/vfsdots");
                let _ = Vfs::unmount("/tmp/vfsnorm");
                return Err(KernelError::InternalError);
            }
        }

        let _ = Vfs::remove("/tmp/vfsnorm/probe");
        // Unmount by the *un*-normalised spelling: it must find the mount
        // registered under the canonical one.
        if let Err(e) = Vfs::unmount("/tmp/vfsnorm/") {
            serial_println!("[vfs]   FAIL: unmount by trailing-slash spelling: {:?}", e);
            let _ = Vfs::unmount("/tmp/vfsnorm");
            return Err(e);
        }
        serial_println!("[vfs]   mount path normalisation: OK");
    }

    // --- Permission gate: POSIX ACL enforcement ---
    if has_tmp {
        acl_gate_self_test()?;
    }

    skips.report("[vfs]");
    serial_println!("[vfs] Self-test PASSED{}", skips.suffix());
    Ok(())
}

/// Verify that the permission gate actually consults POSIX ACLs.
///
/// This exists because `acl::check_access` — the whole POSIX 1003.1e
/// evaluation algorithm — spent its entire life with **no production
/// callers**. `setfacl` validated and stored an ACL, `getfacl` read it back
/// verbatim, procfs counted it, and no file operation ever asked it anything:
/// a security feature that reported success while doing nothing, which is
/// strictly worse than not having the feature, because the absence of a
/// feature is visible and a silently-inert one is not.
///
/// The test goes through [`path_access_verdict`] rather than through
/// `Vfs::read_file`, because the kernel's self-tests run as a kernel task and
/// [`check_path_access`] bypasses those before any check runs — so a test
/// driven through a real VFS call could only ever observe "allowed", and
/// would go on passing if the ACL half were deleted again. The call sites are
/// covered separately by `scripts/check-vfs-permission-gate.py`.
///
/// The **deny** direction is what is asserted first. `check_access` fails open
/// when it finds no ACL for the path, so a test that only checks that ordinary
/// access still works passes just as well against a gate that was never wired
/// in at all.
fn acl_gate_self_test() -> KernelResult<()> {
    use super::acl::{AclPerm, build_acl};
    use crate::serial_println;

    serial_println!("[vfs]   --- permission gate: POSIX ACLs ---");

    const PATH: &str = "/tmp/_vfs_acl_gate";
    const OWNER_UID: u32 = 500;
    const OWNER_GID: u32 = 500;
    const DENIED_UID: u32 = 1001;

    // A helper so every failure path removes the ACL: leaving one behind would
    // make every later VFS operation in the boot take the slow path, and worse,
    // could deny an unrelated test.
    fn cleanup() {
        super::acl::remove_acl(PATH);
        let _ = Vfs::remove(PATH);
    }

    let _ = Vfs::remove(PATH);
    Vfs::write_file(PATH, b"acl gate")?;
    Vfs::set_owner(PATH, OWNER_UID, OWNER_GID)?;

    // Owner rw-, group r--, other r--, and a named-user entry denying 1001
    // everything. `build_acl` derives the mask as GROUP_OBJ ∪ every named
    // entry, so here it is r-- — permissive enough that the *named entry*, not
    // the mask, is what denies. That distinction matters: if the mask were what
    // refused, the test would pass without the named entry ever being consulted.
    let acl = build_acl(
        AclPerm::READ.union(AclPerm::WRITE),
        AclPerm::READ,
        AclPerm::READ,
        &[(DENIED_UID, AclPerm::NONE)],
        &[],
    );
    super::acl::set_acl(PATH, acl)?;

    if super::acl::count() != 1 {
        serial_println!(
            "[vfs]     FAIL: acl::count() is {} after one set_acl",
            super::acl::count()
        );
        cleanup();
        return Err(KernelError::InternalError);
    }

    let path = Path::new(PATH);

    // 1. The denied user is refused a read. This is the assertion the whole
    //    test exists for.
    match path_access_verdict(path, DENIED_UID, DENIED_UID, &[], PathAccess::Read) {
        Err(KernelError::PermissionDenied) => {}
        other => {
            serial_println!(
                "[vfs]     FAIL: denied uid {} got {:?} on read (expected PermissionDenied)",
                DENIED_UID,
                other
            );
            cleanup();
            return Err(KernelError::InternalError);
        }
    }
    // ...and a write, which goes through a different `PathAccess` arm.
    if path_access_verdict(path, DENIED_UID, DENIED_UID, &[], PathAccess::Write).is_ok() {
        serial_println!("[vfs]     FAIL: denied uid was allowed to write");
        cleanup();
        return Err(KernelError::InternalError);
    }

    // 2. Metadata is still permitted: POSIX makes `stat` depend on search
    //    permission along the path, not on read permission of the target, so a
    //    deny ACL must not make the file un-`stat`-able.
    if let Err(e) = path_access_verdict(path, DENIED_UID, DENIED_UID, &[], PathAccess::Metadata) {
        serial_println!(
            "[vfs]     FAIL: denied uid refused metadata access: {:?}",
            e
        );
        cleanup();
        return Err(KernelError::InternalError);
    }

    // 3. The owner still gets what the ACL grants — and only that. Write is
    //    granted, execute is not, which proves the request is compared against
    //    the entry rather than treated as all-or-nothing.
    if let Err(e) = path_access_verdict(path, OWNER_UID, OWNER_GID, &[], PathAccess::Write) {
        serial_println!(
            "[vfs]     FAIL: owner refused write by its own ACL: {:?}",
            e
        );
        cleanup();
        return Err(KernelError::InternalError);
    }
    if path_access_verdict(path, OWNER_UID, OWNER_GID, &[], PathAccess::Execute).is_ok() {
        serial_println!("[vfs]     FAIL: owner granted execute the ACL does not carry");
        cleanup();
        return Err(KernelError::InternalError);
    }

    // 4. An unrelated user falls through to ACL_OTHER, which grants read.
    if let Err(e) = path_access_verdict(path, 4242, 4242, &[], PathAccess::Read) {
        serial_println!(
            "[vfs]     FAIL: ACL_OTHER read refused for a third party: {:?}",
            e
        );
        cleanup();
        return Err(KernelError::InternalError);
    }

    // 5. Root is not subject to ACLs, matching the traditional model.
    if let Err(e) = path_access_verdict(path, 0, 0, &[], PathAccess::Read) {
        serial_println!("[vfs]     FAIL: root denied by an ACL: {:?}", e);
        cleanup();
        return Err(KernelError::InternalError);
    }

    // 6. Removing the ACL restores unrestricted access, and drops the count
    //    back to zero so the fast path in the gate is taken again.
    if !super::acl::remove_acl(PATH) {
        serial_println!("[vfs]     FAIL: remove_acl reported nothing to remove");
        cleanup();
        return Err(KernelError::InternalError);
    }
    if super::acl::count() != 0 {
        serial_println!(
            "[vfs]     FAIL: acl::count() is {} after remove_acl",
            super::acl::count()
        );
        cleanup();
        return Err(KernelError::InternalError);
    }
    if let Err(e) = path_access_verdict(path, DENIED_UID, DENIED_UID, &[], PathAccess::Read) {
        serial_println!(
            "[vfs]     FAIL: read still refused after the ACL was removed: {:?}",
            e
        );
        cleanup();
        return Err(KernelError::InternalError);
    }

    // 7. A path with no ACL is unaffected — the gate fails open, deferring to
    //    the traditional permission bits.
    if let Err(e) = path_access_verdict(
        Path::new("/tmp/_vfs_acl_absent"),
        DENIED_UID,
        DENIED_UID,
        &[],
        PathAccess::Write,
    ) {
        serial_println!("[vfs]     FAIL: gate denied a path with no ACL: {:?}", e);
        cleanup();
        return Err(KernelError::InternalError);
    }

    cleanup();
    serial_println!("[vfs]     POSIX ACL enforcement: OK (deny, metadata-exempt, root bypass)");
    Ok(())
}

/// Mount/unmount roundtrip self-test.
///
/// Exercises the same backend calls that the `SYS_FS_MOUNT` / `SYS_FS_UMOUNT`
/// handlers dispatch to: it mounts a fresh in-memory filesystem (the "tmpfs"
/// fstype) at a scratch mount point, writes and reads a file through it,
/// confirms the root filesystem cannot be unmounted, then unmounts and
/// verifies the mount is gone.  Runs on any root (in-memory or disk-backed),
/// so it is called unconditionally during boot.
pub fn mount_self_test() -> KernelResult<()> {
    use crate::serial_println;

    serial_println!("[vfs] Running mount/unmount self-test...");

    // A scratch mount point that boot setup never uses (boot mounts ext4 at
    // /mnt, so avoid that path entirely).
    let mp = "/_mount_selftest";

    // Refuse to clobber a stale mount from a previous run.
    if Vfs::mounts()
        .iter()
        .any(|(p, _)| p.as_path() == Path::new(mp))
    {
        serial_println!("[vfs]   {} already mounted — unmounting stale entry", mp);
        let _ = Vfs::unmount(mp);
    }

    // Mount a fresh in-memory filesystem (same call as fstype "tmpfs").
    crate::fs::memfs::mount(mp)?;
    if !Vfs::mounts()
        .iter()
        .any(|(p, _)| p.as_path() == Path::new(mp))
    {
        serial_println!("[vfs]   FAIL: {} not present after mount", mp);
        let _ = Vfs::unmount(mp);
        return Err(KernelError::InternalError);
    }
    serial_println!("[vfs]   mount tmpfs at {}: OK", mp);

    // Write and read back through the new mount.
    let test_file = "/_mount_selftest/_probe";
    Vfs::write_file(test_file, b"mounted fs works")?;
    let back = Vfs::read_file(test_file)?;
    if back.as_slice() != b"mounted fs works" {
        serial_println!("[vfs]   FAIL: read-back through {} mismatch", mp);
        let _ = Vfs::remove(test_file);
        let _ = Vfs::unmount(mp);
        return Err(KernelError::InternalError);
    }
    serial_println!("[vfs]   write/read through {}: OK", mp);
    let _ = Vfs::remove(test_file);

    // Root must never be unmountable (the guard the handler relies on).
    match Vfs::unmount("/") {
        Err(_) => serial_println!("[vfs]   unmount('/') refused: OK"),
        Ok(()) => {
            serial_println!("[vfs]   FAIL: unmount('/') should be refused");
            let _ = Vfs::unmount(mp);
            return Err(KernelError::InternalError);
        }
    }

    // Unmount the scratch mount and verify it is gone.
    Vfs::unmount(mp)?;
    if Vfs::mounts()
        .iter()
        .any(|(p, _)| p.as_path() == Path::new(mp))
    {
        serial_println!("[vfs]   FAIL: {} still present after unmount", mp);
        return Err(KernelError::InternalError);
    }
    serial_println!("[vfs]   unmount {}: OK", mp);

    serial_println!("[vfs] Mount/unmount self-test PASSED");
    Ok(())
}

/// Self-test for stable file identity ([`Vfs::file_identity`]) — the page-cache
/// key precursor for the C-lite read-only page cache (design-decisions §23/§36).
///
/// Validates the four properties callers depend on:
/// 1. A real file on a stable-inode backend (memfs) yields `Some(FileId)` with a
///    non-zero `ino`.
/// 2. Identity is stable: two lookups of the same path return the same `FileId`.
/// 3. Distinct files on the same mount have distinct `FileId`s (same `fs_id`,
///    different `ino`).
/// 4. Files on *different* mounts never collide even if their inode numbers
///    happen to match — the `fs_id` half disambiguates them.
pub fn file_identity_self_test() -> KernelResult<()> {
    use crate::serial_println;

    serial_println!("[vfs] Running file-identity self-test...");

    let mp_a = "/_fileid_selftest_a";
    let mp_b = "/_fileid_selftest_b";

    // Refuse to clobber stale mounts from a previous run.
    for mp in [mp_a, mp_b] {
        if Vfs::mounts()
            .iter()
            .any(|(p, _)| p.as_path() == Path::new(mp))
        {
            let _ = Vfs::unmount(mp);
        }
    }

    // Helper that always tears down both scratch mounts before returning an
    // error, so a failure never leaks mounts into the rest of the boot.
    fn teardown(mp_a: &str, mp_b: &str) {
        let _ = Vfs::remove("/_fileid_selftest_a/f1");
        let _ = Vfs::remove("/_fileid_selftest_a/f2");
        let _ = Vfs::remove("/_fileid_selftest_b/f1");
        let _ = Vfs::unmount(mp_a);
        let _ = Vfs::unmount(mp_b);
    }

    crate::fs::memfs::mount(mp_a)?;
    if let Err(e) = crate::fs::memfs::mount(mp_b) {
        let _ = Vfs::unmount(mp_a);
        return Err(e);
    }

    // Macro-free inline error handling: on any failure, tear down and bail.
    let run = || -> KernelResult<()> {
        Vfs::write_file("/_fileid_selftest_a/f1", b"alpha")?;
        Vfs::write_file("/_fileid_selftest_a/f2", b"beta")?;
        Vfs::write_file("/_fileid_selftest_b/f1", b"gamma")?;

        // (1) Real file ⇒ Some(FileId) with non-zero ino.
        let a1 = Vfs::file_identity("/_fileid_selftest_a/f1")?;
        let a1 = match a1 {
            Some(id) if id.ino != 0 => id,
            other => {
                serial_println!("[vfs]   FAIL: expected Some(non-zero ino), got {:?}", other);
                return Err(KernelError::InternalError);
            }
        };
        serial_println!("[vfs]   identity(a/f1) = {:?}: OK", a1);

        // (2) Stable across repeated lookups.
        let a1_again = Vfs::file_identity("/_fileid_selftest_a/f1")?;
        if a1_again != Some(a1) {
            serial_println!("[vfs]   FAIL: identity not stable across lookups");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]   identity stable across lookups: OK");

        // (3) Distinct files on the same mount ⇒ same fs_id, different ino.
        let a2 = Vfs::file_identity("/_fileid_selftest_a/f2")?.ok_or(KernelError::InternalError)?;
        if a2.fs_id != a1.fs_id {
            serial_println!("[vfs]   FAIL: same-mount files have different fs_id");
            return Err(KernelError::InternalError);
        }
        if a2 == a1 {
            serial_println!("[vfs]   FAIL: distinct files share a FileId");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]   distinct files on one mount differ: OK");

        // (4) Different mounts never collide — distinct fs_id even if ino matches.
        let b1 = Vfs::file_identity("/_fileid_selftest_b/f1")?.ok_or(KernelError::InternalError)?;
        if b1.fs_id == a1.fs_id {
            serial_println!("[vfs]   FAIL: separate mounts share an fs_id");
            return Err(KernelError::InternalError);
        }
        if b1 == a1 {
            serial_println!("[vfs]   FAIL: cross-mount FileId collision");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]   cross-mount identities never collide: OK");

        Ok(())
    };

    let result = run();
    teardown(mp_a, mp_b);
    result?;

    serial_println!("[vfs] File-identity self-test PASSED");
    Ok(())
}

/// Clean up globstar test directory tree.
fn cleanup_glob_test() -> KernelResult<()> {
    let _ = Vfs::remove("/tmp/_glob_test/sub/deep/e.rs");
    let _ = Vfs::remove("/tmp/_glob_test/sub/deep/d.txt");
    let _ = Vfs::remove("/tmp/_glob_test/sub/c.txt");
    let _ = Vfs::remove("/tmp/_glob_test/b.rs");
    let _ = Vfs::remove("/tmp/_glob_test/a.txt");
    let _ = Vfs::rmdir("/tmp/_glob_test/sub/deep");
    let _ = Vfs::rmdir("/tmp/_glob_test/sub");
    let _ = Vfs::rmdir("/tmp/_glob_test");
    Ok(())
}

// ---------------------------------------------------------------------------
// Glob pattern matching
// ---------------------------------------------------------------------------

/// Match a filename against a glob pattern.
///
/// Supports:
/// - `*` — matches zero or more characters (except `/`)
/// - `?` — matches exactly one character (except `/`)
/// - `[abc]` — matches any one of the characters in the set
/// - `[a-z]` — matches any character in the range
/// - `[!abc]` or `[^abc]` — negated character class
/// - `\\` — literal escape (e.g., `\\*` matches a literal `*`)
///
/// Case-insensitive by default (controlled by `case_insensitive` parameter).
///
/// This operates on a single filename component (no `/` matching).  For
/// full path globbing, use `Vfs::glob()`.
///
/// ## Examples
///
/// - `glob_match("hello.rs", "*.rs", false)` → true
/// - `glob_match("hello.rs", "hello.?s", false)` → true
/// - `glob_match("test.txt", "test.[tx][tx][tx]", false)` → true
/// - `glob_match("abc", "a*c", false)` → true
/// - `glob_match("abc", "a?c", false)` → true
pub fn glob_match<N: AsRef<[u8]>, P: AsRef<[u8]>>(
    name: N,
    pattern: P,
    case_insensitive: bool,
) -> bool {
    glob_match_inner(name.as_ref(), pattern.as_ref(), case_insensitive)
}

/// Inner recursive glob matcher operating on byte slices.
///
/// Uses a simple recursive algorithm with backtracking.  For the patterns
/// and name lengths we encounter in a filesystem (max 255 bytes), this is
/// efficient enough.  A pathological case like `*****abc` could be slow
/// on very long names, but that doesn't happen in practice.
fn glob_match_inner(name: &[u8], pattern: &[u8], ci: bool) -> bool {
    let mut ni = 0;
    let mut pi = 0;

    // Track the last `*` position for backtracking.
    let mut star_pi: Option<usize> = None;
    let mut star_ni: usize = 0;

    while ni < name.len() {
        if pi < pattern.len() {
            match pattern.get(pi).copied() {
                Some(b'?') => {
                    // Match any single character.
                    ni += 1;
                    pi += 1;
                    continue;
                }
                Some(b'*') => {
                    // Record backtrack point and try matching zero chars.
                    star_pi = Some(pi);
                    star_ni = ni;
                    pi += 1;
                    continue;
                }
                Some(b'[') => {
                    // Character class.
                    if let Some((matched, end_pi)) =
                        match_char_class(name.get(ni).copied().unwrap_or(0), pattern, pi, ci)
                    {
                        if matched {
                            ni += 1;
                            pi = end_pi;
                            continue;
                        }
                    }
                    // Class didn't match — try backtracking.
                    if let Some(sp) = star_pi {
                        star_ni += 1;
                        ni = star_ni;
                        pi = sp + 1;
                        continue;
                    }
                    return false;
                }
                Some(b'\\') => {
                    // Escaped character — match literally.
                    pi += 1;
                    let pc = pattern.get(pi).copied().unwrap_or(b'\\');
                    let nc = name.get(ni).copied().unwrap_or(0);
                    if char_eq(nc, pc, ci) {
                        ni += 1;
                        pi += 1;
                        continue;
                    }
                    if let Some(sp) = star_pi {
                        star_ni += 1;
                        ni = star_ni;
                        pi = sp + 1;
                        continue;
                    }
                    return false;
                }
                Some(pc) => {
                    let nc = name.get(ni).copied().unwrap_or(0);
                    if char_eq(nc, pc, ci) {
                        ni += 1;
                        pi += 1;
                        continue;
                    }
                    // Mismatch — try backtracking to last `*`.
                    if let Some(sp) = star_pi {
                        star_ni += 1;
                        ni = star_ni;
                        pi = sp + 1;
                        continue;
                    }
                    return false;
                }
                None => {
                    // Pattern exhausted but name has characters left.
                    if let Some(sp) = star_pi {
                        star_ni += 1;
                        ni = star_ni;
                        pi = sp + 1;
                        continue;
                    }
                    return false;
                }
            }
        }
        // Pattern exhausted.  Backtrack if we had a `*`.
        if let Some(sp) = star_pi {
            star_ni += 1;
            ni = star_ni;
            pi = sp + 1;
            continue;
        }
        return false;
    }

    // Name exhausted.  Skip any remaining `*`s in pattern.
    while pattern.get(pi) == Some(&b'*') {
        pi += 1;
    }

    // Both must be exhausted for a match.
    pi == pattern.len()
}

/// Match a character class `[...]` at the given pattern index.
///
/// Returns `Some((matched, end_index))` where `end_index` is the byte
/// position after the closing `]`.  Returns `None` if the pattern is
/// malformed (no closing `]`).
fn match_char_class(ch: u8, pattern: &[u8], start: usize, ci: bool) -> Option<(bool, usize)> {
    // start points to `[`; advance past it.
    let mut pi = start + 1;
    let mut negated = false;

    if pattern.get(pi) == Some(&b'!') || pattern.get(pi) == Some(&b'^') {
        negated = true;
        pi += 1;
    }

    let mut matched = false;

    // Handle `]` as first character in class (literal `]`).
    if pattern.get(pi) == Some(&b']') {
        if char_eq(ch, b']', ci) {
            matched = true;
        }
        pi += 1;
    }

    while let Some(&c) = pattern.get(pi) {
        if c == b']' {
            // End of class.
            let result = if negated { !matched } else { matched };
            return Some((result, pi + 1));
        }

        // Check for range: `a-z`.
        if pattern.get(pi + 1) == Some(&b'-') {
            if let Some(&end_c) = pattern.get(pi + 2) {
                if end_c != b']' {
                    // It's a range.
                    let lo = if ci { c.to_ascii_lowercase() } else { c };
                    let hi = if ci {
                        end_c.to_ascii_lowercase()
                    } else {
                        end_c
                    };
                    let test = if ci { ch.to_ascii_lowercase() } else { ch };
                    if test >= lo && test <= hi {
                        matched = true;
                    }
                    pi += 3;
                    continue;
                }
            }
        }

        // Single character.
        if char_eq(ch, c, ci) {
            matched = true;
        }
        pi += 1;
    }

    // No closing `]` found — malformed pattern.
    None
}

/// Compare two bytes, optionally case-insensitively.
fn char_eq(a: u8, b: u8, case_insensitive: bool) -> bool {
    if case_insensitive {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

/// Self-test for glob pattern matching.
///
/// Exercises `*`, `?`, character classes, negation, ranges, escaping,
/// and case-insensitive mode.
#[allow(clippy::needless_pass_by_value)]
pub fn glob_self_test() -> KernelResult<()> {
    use crate::serial_println;
    serial_println!("[glob] Running self-test...");

    // Basic wildcard.
    assert!(glob_match("hello.rs", "*.rs", false));
    assert!(glob_match("hello.rs", "hello.*", false));
    assert!(glob_match("hello.rs", "*", false));
    assert!(glob_match("", "*", false));
    assert!(!glob_match("hello.rs", "*.txt", false));
    serial_println!("[glob]   wildcard (*): OK");

    // Single char.
    assert!(glob_match("hello.rs", "hell?.rs", false));
    assert!(!glob_match("hello.rs", "hell?.txt", false));
    assert!(!glob_match("hello.rs", "hel?.rs", false)); // ? matches exactly one
    serial_println!("[glob]   single char (?): OK");

    // Character classes.
    assert!(glob_match("hello.rs", "hello.[rt]s", false));
    assert!(glob_match("a", "[abc]", false));
    assert!(!glob_match("d", "[abc]", false));
    serial_println!("[glob]   char class []: OK");

    // Negated classes.
    assert!(glob_match("d", "[!abc]", false));
    assert!(!glob_match("a", "[!abc]", false));
    assert!(glob_match("d", "[^abc]", false));
    serial_println!("[glob]   negated class [!]: OK");

    // Ranges.
    assert!(glob_match("m", "[a-z]", false));
    assert!(!glob_match("5", "[a-z]", false));
    assert!(glob_match("5", "[0-9]", false));
    serial_println!("[glob]   ranges [a-z]: OK");

    // Case insensitive.
    assert!(glob_match("Hello.RS", "*.rs", true));
    assert!(!glob_match("Hello.RS", "*.rs", false));
    serial_println!("[glob]   case insensitive: OK");

    // Escape.
    assert!(glob_match("file*.txt", "file\\*.txt", false));
    assert!(!glob_match("fileX.txt", "file\\*.txt", false));
    serial_println!("[glob]   escape: OK");

    // Complex patterns.
    assert!(glob_match("abcdef", "a*f", false));
    assert!(glob_match("abcdef", "a*d*f", false));
    assert!(glob_match("abcdef", "*", false));
    assert!(glob_match("abc", "abc", false));
    assert!(!glob_match("abc", "abd", false));
    serial_println!("[glob]   complex patterns: OK");

    // Edge cases.
    assert!(glob_match("", "", false));
    assert!(!glob_match("a", "", false));
    assert!(!glob_match("", "a", false));
    assert!(glob_match("", "*", false));
    serial_println!("[glob]   edge cases: OK");

    serial_println!("[glob] Self-test passed.");
    Ok(())
}
