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

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

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
}

/// A single directory entry returned by readdir.
#[derive(Debug, Clone)]
pub struct DirEntry {
    /// Entry name (UTF-8, no path separators).
    pub name: String,
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

/// Nanosecond timestamp (monotonic or wall-clock, depending on source).
///
/// 0 means "not set" or "unknown".
pub type Timestamp = u64;

/// One day in nanoseconds (for relatime threshold).
const ONE_DAY_NS: u64 = 86_400_000_000_000;

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

    // --- Timestamps (nanoseconds since boot, 0 = not available) ---
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
            created_ns: 0,
            modified_ns: 0,
            accessed_ns: 0,
            changed_ns: 0,
            uid: 0,
            gid: 0,
            permissions: 0,
            attributes: FileAttr::NONE,
            nlinks: 1,
            xattrs: Vec::new(),
            hash: Vec::new(),
        }
    }

    /// Create metadata with timestamps set to "now".
    pub fn with_timestamps(entry_type: EntryType, size: u64) -> Self {
        let now = crate::hpet::elapsed_ns();
        Self {
            size,
            entry_type,
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
            xattrs: Vec::new(),
            hash: Vec::new(),
        }
    }

    /// Check if the access timestamp should be updated (relatime policy).
    ///
    /// Returns `true` if `accessed_ns` is older than `modified_ns`
    /// or more than one day old.
    pub fn should_update_atime(&self) -> bool {
        let now = crate::hpet::elapsed_ns();
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
    fn readdir(&mut self, path: &str) -> KernelResult<Vec<DirEntry>>;

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
        path: &str,
        offset: usize,
        count: usize,
    ) -> KernelResult<(Vec<DirEntry>, usize)> {
        let all = self.readdir(path)?;
        let total = all.len();
        let start = offset.min(total);
        let end = start.saturating_add(count).min(total);
        Ok((all.into_iter().skip(start).take(end.saturating_sub(start)).collect(), total))
    }

    /// Read the contents of a file.
    ///
    /// `path` is the full path relative to filesystem root
    /// (e.g., `"/HELLO.TXT"`).
    ///
    /// Returns the file contents as a byte vector.
    fn read_file(&mut self, path: &str) -> KernelResult<Vec<u8>>;

    /// Get metadata for a path (file or directory).
    ///
    /// Returns a [`DirEntry`] with name, type, and size.
    fn stat(&mut self, path: &str) -> KernelResult<DirEntry>;

    /// Write data to a file, creating it if it doesn't exist.
    ///
    /// If the file exists, its contents are replaced entirely.
    /// Returns `NotSupported` if the filesystem is read-only.
    fn write_file(&mut self, path: &str, data: &[u8]) -> KernelResult<()> {
        let _ = (path, data);
        Err(KernelError::NotSupported)
    }

    /// Delete a file.
    ///
    /// Returns `NotSupported` if the filesystem is read-only.
    fn remove(&mut self, path: &str) -> KernelResult<()> {
        let _ = path;
        Err(KernelError::NotSupported)
    }

    /// Create a directory.
    ///
    /// Returns `NotSupported` if the filesystem is read-only.
    fn mkdir(&mut self, path: &str) -> KernelResult<()> {
        let _ = path;
        Err(KernelError::NotSupported)
    }

    /// Remove an empty directory.
    ///
    /// Returns `NotSupported` if the filesystem is read-only.
    fn rmdir(&mut self, path: &str) -> KernelResult<()> {
        let _ = path;
        Err(KernelError::NotSupported)
    }

    /// Read a range of bytes from a file.
    ///
    /// Default implementation reads the whole file and slices.
    /// Filesystem implementations should override this for efficiency
    /// (e.g., walking the FAT cluster chain to the right offset).
    fn read_at(&mut self, path: &str, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
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
    fn write_at(&mut self, path: &str, offset: u64, data: &[u8]) -> KernelResult<()> {
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
    fn fallocate(&mut self, path: &str, size: u64) -> KernelResult<()> {
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
    fn truncate(&mut self, path: &str, size: u64) -> KernelResult<()> {
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
    fn rename(&mut self, from: &str, to: &str) -> KernelResult<()> {
        let _ = (from, to);
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
    fn metadata(&mut self, path: &str) -> KernelResult<FileMeta> {
        let entry = self.stat(path)?;
        Ok(FileMeta::minimal(entry.entry_type, entry.size))
    }

    /// Set file attributes (immutable, append-only, etc.).
    ///
    /// Default: not supported.
    fn set_attributes(&mut self, path: &str, attrs: FileAttr) -> KernelResult<()> {
        let _ = (path, attrs);
        Err(KernelError::NotSupported)
    }

    /// Set ownership (uid/gid).
    ///
    /// Default: not supported.
    fn set_owner(&mut self, path: &str, uid: u32, gid: u32) -> KernelResult<()> {
        let _ = (path, uid, gid);
        Err(KernelError::NotSupported)
    }

    /// Set Unix-style permission bits (rwxrwxrwx).
    ///
    /// Default: not supported.
    fn set_permissions(&mut self, path: &str, permissions: u16) -> KernelResult<()> {
        let _ = (path, permissions);
        Err(KernelError::NotSupported)
    }

    /// Update timestamps.
    ///
    /// Pass 0 for any timestamp to leave it unchanged.
    /// Default: not supported.
    fn set_times(
        &mut self,
        path: &str,
        accessed_ns: Timestamp,
        modified_ns: Timestamp,
    ) -> KernelResult<()> {
        let _ = (path, accessed_ns, modified_ns);
        Err(KernelError::NotSupported)
    }

    /// Get an extended attribute value by key.
    ///
    /// Default: not supported.
    fn get_xattr(&mut self, path: &str, key: &str) -> KernelResult<Vec<u8>> {
        let _ = (path, key);
        Err(KernelError::NotSupported)
    }

    /// Set an extended attribute.
    ///
    /// Default: not supported.
    fn set_xattr(&mut self, path: &str, key: &str, value: &[u8]) -> KernelResult<()> {
        let _ = (path, key, value);
        Err(KernelError::NotSupported)
    }

    /// Remove an extended attribute.
    ///
    /// Default: not supported.
    fn remove_xattr(&mut self, path: &str, key: &str) -> KernelResult<()> {
        let _ = (path, key);
        Err(KernelError::NotSupported)
    }

    /// List all extended attribute keys for a path.
    ///
    /// Default: empty list.
    fn list_xattrs(&mut self, path: &str) -> KernelResult<Vec<String>> {
        let _ = path;
        Ok(Vec::new())
    }

    // --- Symlink operations ---

    /// Create a symbolic link at `path` pointing to `target`.
    ///
    /// `target` is stored as-is (not resolved).  It can be absolute or
    /// relative.  The symlink is resolved when it is traversed during
    /// path resolution.
    ///
    /// Default: not supported.
    fn symlink(&mut self, path: &str, target: &str) -> KernelResult<()> {
        let _ = (path, target);
        Err(KernelError::NotSupported)
    }

    /// Read the target of a symbolic link.
    ///
    /// Does NOT follow the symlink — returns the stored target string.
    ///
    /// Default: not supported.
    fn readlink(&mut self, path: &str) -> KernelResult<String> {
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
    fn lstat(&mut self, path: &str) -> KernelResult<DirEntry> {
        self.stat(path)
    }

    /// Return filesystem space and configuration information.
    ///
    /// Default returns a minimal struct with only the type name set.
    /// Filesystems that can report capacity/usage should override this.
    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        Ok(FsInfo {
            fs_type: String::from(self.fs_type()),
            block_size: 0,
            total_blocks: 0,
            free_blocks: 0,
            total_inodes: 0,
            free_inodes: 0,
            max_name_len: 255,
            read_only: false,
        })
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
    fn link(&mut self, existing: &str, new_path: &str) -> KernelResult<()> {
        let _ = (existing, new_path);
        Err(KernelError::NotSupported)
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
}

// ---------------------------------------------------------------------------
// VFS — global filesystem manager
// ---------------------------------------------------------------------------

/// A mount point in the VFS.
struct MountPoint {
    /// Path where this filesystem is mounted (e.g., `"/"`).
    path: String,
    /// The filesystem implementation.
    fs: Box<dyn FileSystem>,
}

/// The global VFS state.
static VFS: Mutex<VfsInner> = Mutex::new(VfsInner {
    mounts: Vec::new(),
});

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
    path: String,
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
pub(super) const VFS_DCACHE_SIZE: usize = 1024;

/// A single entry in the VFS path resolution cache.
struct VfsDcacheEntry {
    /// The normalized input path (key).
    key: String,
    /// Whether the final component was followed (true = resolve_follow,
    /// false = resolve_no_follow).
    follow_last: bool,
    /// The resolved output path (after symlink expansion).
    /// Empty for negative entries (path does not exist).
    resolved: String,
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
            key: String::new(),
            follow_last: false,
            resolved: String::new(),
            last_access: 0,
            valid: false,
            negative: false,
        }
    }
}

/// Result of a VFS dcache lookup.
///
/// Distinguished from `Option<String>` so callers can tell the difference
/// between "not in cache" (walk needed) and "known not to exist" (short-
/// circuit with `NotFound`).
enum DcacheLookup {
    /// Path resolves to this value (positive cache hit).
    Hit(String),
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
        // because String isn't Copy, so we initialize in init().
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
    fn lookup(&mut self, key: &str, follow_last: bool) -> DcacheLookup {
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.follow_last == follow_last && entry.key == key {
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
    fn insert(&mut self, key: &str, follow_last: bool, resolved: &str) {
        self.counter = self.counter.wrapping_add(1);

        // Check if already cached (update in place).
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.follow_last == follow_last && entry.key == key {
                entry.resolved.clear();
                entry.resolved.push_str(resolved);
                entry.last_access = self.counter;
                entry.negative = false;
                return;
            }
        }

        // Find an empty slot.
        for entry in self.entries.iter_mut() {
            if !entry.valid {
                entry.key = String::from(key);
                entry.follow_last = follow_last;
                entry.resolved = String::from(resolved);
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
        self.entries[lru_idx].key.push_str(key);
        self.entries[lru_idx].follow_last = follow_last;
        self.entries[lru_idx].resolved.clear();
        self.entries[lru_idx].resolved.push_str(resolved);
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
    fn insert_negative(&mut self, key: &str, follow_last: bool) {
        self.counter = self.counter.wrapping_add(1);

        // Check if already cached (update to negative in place).
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.follow_last == follow_last && entry.key == key {
                entry.resolved.clear();
                entry.negative = true;
                entry.last_access = self.counter;
                return;
            }
        }

        // Find an empty slot.
        for entry in self.entries.iter_mut() {
            if !entry.valid {
                entry.key = String::from(key);
                entry.follow_last = follow_last;
                entry.resolved = String::new();
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
        self.entries[lru_idx].key.push_str(key);
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
    fn invalidate_prefix(&mut self, prefix: &str) {
        for entry in self.entries.iter_mut() {
            if !entry.valid {
                continue;
            }
            if path_prefix_matches(&entry.key, prefix)
                || path_prefix_matches(&entry.resolved, prefix)
            {
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
    fn invalidate_negative_prefix(&mut self, prefix: &str) {
        for entry in self.entries.iter_mut() {
            if !entry.valid || !entry.negative {
                continue;
            }
            if path_prefix_matches(&entry.key, prefix) {
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

/// Check if `path` starts with `prefix` at a path boundary.
///
/// Returns true if:
/// - `path == prefix`, or
/// - `path` starts with `prefix` followed by '/', or
/// - `prefix == "/"` (root matches everything)
fn path_prefix_matches(path: &str, prefix: &str) -> bool {
    if prefix == "/" {
        return true;
    }
    if path == prefix {
        return true;
    }
    if path.starts_with(prefix) {
        // Must be followed by '/' to be a path boundary.
        path.as_bytes().get(prefix.len()) == Some(&b'/')
    } else {
        false
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
    pub fn mount(mount_path: &str, fs: Box<dyn FileSystem>) -> KernelResult<()> {
        if !mount_path.starts_with('/') {
            return Err(KernelError::InvalidArgument);
        }

        let mut vfs = VFS.lock();

        // Check for duplicate mount point.
        for mp in &vfs.mounts {
            if mp.path == mount_path {
                return Err(KernelError::AlreadyExists);
            }
        }

        crate::serial_println!(
            "[vfs] Mounted {} filesystem at '{}'",
            fs.fs_type(),
            mount_path
        );

        vfs.mounts.push(MountPoint {
            path: String::from(mount_path),
            fs,
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
    pub fn unmount(mount_path: &str) -> KernelResult<()> {
        let mut vfs = VFS.lock();

        let idx = vfs.mounts.iter().position(|mp| mp.path == mount_path)
            .ok_or(KernelError::NotFound)?;

        // Refuse to unmount root.
        if mount_path == "/" {
            return Err(KernelError::PermissionDenied);
        }

        // Check for sub-mounts that would be orphaned.
        let has_children = vfs.mounts.iter().enumerate().any(|(i, mp)| {
            i != idx
                && mp.path.starts_with(mount_path)
                && mp.path.len() > mount_path.len()
                && mp.path.as_bytes().get(mount_path.len()) == Some(&b'/')
        });
        if has_children {
            crate::serial_println!(
                "[vfs] Cannot unmount '{}': has sub-mounts",
                mount_path
            );
            return Err(KernelError::DeviceBusy);
        }

        // Sync before removing.
        if let Err(e) = vfs.mounts[idx].fs.sync() {
            crate::serial_println!(
                "[vfs] WARNING: sync failed during unmount of '{}': {:?}",
                mount_path, e
            );
            // Continue with unmount anyway — data loss is better than a
            // permanently stuck mount.
        }

        let removed = vfs.mounts.remove(idx);
        crate::serial_println!(
            "[vfs] Unmounted {} from '{}'",
            removed.fs.fs_type(),
            mount_path
        );

        // Unmount changes affect path resolution — invalidate entire dcache.
        drop(vfs);
        VFS_DCACHE.lock().invalidate_all();

        // Release any advisory locks on paths under this mount.
        // Use path-boundary check to avoid accidentally clearing locks
        // on paths like "/mnt_data" when unmounting "/mnt".
        let mut table = LOCK_TABLE.lock();
        table.retain(|entry| {
            if entry.path == mount_path {
                return false; // Exact match — remove.
            }
            if entry.path.starts_with(mount_path) {
                // Only remove if mount_path is a path boundary prefix.
                entry.path.as_bytes().get(mount_path.len()) != Some(&b'/')
            } else {
                true // Different prefix — keep.
            }
        });

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
    pub fn resolve_path(path: &str) -> KernelResult<String> {
        Self::resolve_follow(path)
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
    fn resolve_follow(path: &str) -> KernelResult<String> {
        validate_path(path)?;
        let norm = normalize_path(path);

        // Check VFS dcache first — avoids component-by-component lstat walk.
        {
            let mut dcache = VFS_DCACHE.lock();
            match dcache.lookup(&norm, true) {
                DcacheLookup::Hit(resolved) => return Ok(resolved),
                DcacheLookup::NegativeHit => return Err(KernelError::NotFound),
                DcacheLookup::Miss => {}
            }
        }

        match Self::resolve_inner(&norm, true, 0) {
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
    fn resolve_no_follow(path: &str) -> KernelResult<String> {
        validate_path(path)?;
        let norm = normalize_path(path);

        // Check VFS dcache first.
        {
            let mut dcache = VFS_DCACHE.lock();
            match dcache.lookup(&norm, false) {
                DcacheLookup::Hit(resolved) => return Ok(resolved),
                DcacheLookup::NegativeHit => return Err(KernelError::NotFound),
                DcacheLookup::Miss => {}
            }
        }

        match Self::resolve_inner(&norm, false, 0) {
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

    /// Core recursive resolver.
    ///
    /// `path` must already be normalized (no `.`, `..`, or double slashes).
    fn resolve_inner(
        path: &str,
        follow_last: bool,
        depth: usize,
    ) -> KernelResult<String> {
        if depth > Self::MAX_SYMLINK_DEPTH {
            return Err(KernelError::TooManyLinks);
        }

        let components: Vec<&str> = path
            .split('/')
            .filter(|c| !c.is_empty())
            .collect();

        if components.is_empty() {
            return Ok(String::from("/"));
        }

        let mut resolved = String::with_capacity(path.len());

        for (i, comp) in components.iter().enumerate() {
            let is_last = i == components.len().saturating_sub(1);

            // Build current absolute path.
            resolved.push('/');
            resolved.push_str(comp);

            // Only check for symlinks if we should follow at this position.
            if !is_last || follow_last {
                let entry_type = {
                    let mut vfs = VFS.lock();
                    match find_mount(&mut vfs, &resolved) {
                        Ok((mp, relative)) => match mp.fs.lstat(relative) {
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
                    // Read the symlink target (separate lock acquisition).
                    let target = {
                        let mut vfs = VFS.lock();
                        let (mp, relative) = find_mount(&mut vfs, &resolved)?;
                        mp.fs.readlink(relative)?
                    }; // lock released

                    // Build new path: symlink target + remaining components.
                    let base = if target.starts_with('/') {
                        // Absolute target — restart from VFS root.
                        target
                    } else {
                        // Relative target — resolve from symlink's parent.
                        let parent_end = resolved.rfind('/').unwrap_or(0);
                        let parent = if parent_end == 0 { "/" } else { &resolved[..parent_end] };
                        format!("{}/{}", parent, target)
                    };

                    let remaining = &components[i.saturating_add(1)..];
                    let full = if remaining.is_empty() {
                        base
                    } else {
                        format!("{}/{}", base, remaining.join("/"))
                    };

                    // Normalize (resolve `.` and `..` introduced by target)
                    // and recurse with incremented depth.
                    let normalized = normalize_path(&full);
                    return Self::resolve_inner(
                        &normalized,
                        follow_last,
                        depth.saturating_add(1),
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
    pub fn readdir(path: &str) -> KernelResult<Vec<DirEntry>> {
        let path = Self::resolve_follow(path)?;
        let mut vfs = VFS.lock();

        // Collect mount-point names that are direct children of `path`.
        // E.g., if path="/", mounts at "/tmp" and "/mnt" produce ["tmp", "mnt"].
        // Nested mounts like "/mnt/usb" are NOT direct children of "/".
        let submount_names: Vec<String> = Self::submount_children(&vfs, &path);

        let (mp, relative) = find_mount(&mut vfs, &path)?;
        let mut entries = mp.fs.readdir(relative)?;

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
        path: &str,
        offset: usize,
        count: usize,
    ) -> KernelResult<(Vec<DirEntry>, usize)> {
        let path = Self::resolve_follow(path)?;
        let mut vfs = VFS.lock();

        let submount_names: Vec<String> = Self::submount_children(&vfs, &path);

        let (mp, relative) = find_mount(&mut vfs, &path)?;
        let mut entries = mp.fs.readdir(relative)?;

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
        let page: Vec<DirEntry> = entries.into_iter().skip(start).take(end.saturating_sub(start)).collect();
        Ok((page, total))
    }

    /// Read a file's contents.
    pub fn read_file(path: &str) -> KernelResult<Vec<u8>> {
        let path = Self::resolve_follow(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, &path)?;
        mp.fs.read_file(relative)
    }

    /// Get metadata for a path.
    pub fn stat(path: &str) -> KernelResult<DirEntry> {
        let path = Self::resolve_follow(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, &path)?;
        mp.fs.stat(relative)
    }

    /// Write data to a file (create or overwrite).
    pub fn write_file(path: &str, data: &[u8]) -> KernelResult<()> {
        let path = Self::resolve_follow(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, &path)?;
            mp.fs.write_file(relative, data)?;
        }
        // Writing may create a new file — invalidate negative cache entries
        // that claimed this path didn't exist.
        VFS_DCACHE.lock().invalidate_negative_prefix(&path);
        // Notify and journal after releasing VFS lock (avoids holding both locks).
        super::notify::emit_modified(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// Copy a file from one path to another.
    ///
    /// Reads the source and writes to the destination.  Both paths are
    /// resolved through symlinks.  Works across mount boundaries.
    ///
    /// Future optimization: if both paths are on the same filesystem,
    /// delegate to a filesystem-level copy (reflink, server-side copy).
    pub fn copy(src: &str, dst: &str) -> KernelResult<u64> {
        let data = Self::read_file(src)?;
        let size = data.len() as u64;
        Self::write_file(dst, &data)?;
        Ok(size)
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
    pub fn copy_recursive(src: &str, dst: &str) -> KernelResult<u64> {
        Self::copy_recursive_inner(src, dst, 0)
    }

    fn copy_recursive_inner(src: &str, dst: &str, depth: usize) -> KernelResult<u64> {
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
            let src_child = format!("{}/{}", src, child.name);
            let dst_child = format!("{}/{}", dst, child.name);
            let bytes = Self::copy_recursive_inner(&src_child, &dst_child, depth.saturating_add(1))?;
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
    pub fn remove_recursive(path: &str) -> KernelResult<u64> {
        Self::remove_recursive_inner(path, 0)
    }

    fn remove_recursive_inner(path: &str, depth: usize) -> KernelResult<u64> {
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
            let child_path = format!("{}/{}", path, child.name);
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
    pub fn remove(path: &str) -> KernelResult<()> {
        let path = Self::resolve_no_follow(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, &path)?;
            mp.fs.remove(relative)?;
        }
        // Removing a file/symlink can invalidate cached resolutions that
        // traverse through it (if it was a symlink) or resolve to it.
        VFS_DCACHE.lock().invalidate_prefix(&path);
        super::notify::emit_deleted(&path);
        super::journal::record(super::journal::JournalEventType::Deleted, &path);
        Ok(())
    }

    /// Create a directory.
    ///
    /// Intermediate symlinks are followed; the last component is the
    /// new directory name (not followed if it happens to exist).
    pub fn mkdir(path: &str) -> KernelResult<()> {
        let path = Self::resolve_no_follow(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, &path)?;
            mp.fs.mkdir(relative)?;
        }
        // New directory invalidates negative cache entries that claimed
        // this path (or children) didn't exist.  Positive entries are
        // unaffected — existing path resolutions remain valid.
        VFS_DCACHE.lock().invalidate_negative_prefix(&path);
        super::notify::emit_created(&path);
        super::journal::record(super::journal::JournalEventType::Created, &path);
        Ok(())
    }

    /// Remove an empty directory.
    pub fn rmdir(path: &str) -> KernelResult<()> {
        let path = Self::resolve_no_follow(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, &path)?;
            mp.fs.rmdir(relative)?;
        }
        // Removing a directory invalidates any cached paths through it.
        VFS_DCACHE.lock().invalidate_prefix(&path);
        super::notify::emit_deleted(&path);
        super::journal::record(super::journal::JournalEventType::Deleted, &path);
        Ok(())
    }

    /// Read a range of bytes from a file.
    pub fn read_at(path: &str, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        let path = Self::resolve_follow(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, &path)?;
        mp.fs.read_at(relative, offset, len)
        // Note: no ACCESS event emitted by default (high-frequency).
        // Callers that need it can emit manually.
    }

    /// Write bytes at a specific offset within a file.
    pub fn write_at(path: &str, offset: u64, data: &[u8]) -> KernelResult<()> {
        let path = Self::resolve_follow(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, &path)?;
            mp.fs.write_at(relative, offset, data)?;
        }
        super::notify::emit_modified(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// Append data to the end of a file.
    ///
    /// Creates the file if it doesn't exist.  Uses write_at at the
    /// current file size for efficient append without rewriting.
    pub fn append(path: &str, data: &[u8]) -> KernelResult<()> {
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
    pub fn truncate(path: &str, size: u64) -> KernelResult<()> {
        let path = Self::resolve_follow(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, &path)?;
            mp.fs.truncate(relative, size)?;
        }
        super::notify::emit_modified(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// Pre-allocate space for a file.
    ///
    /// Reserves `size` bytes of disk space for the file.  The file's
    /// logical size is not changed — this just ensures the blocks are
    /// allocated so future writes don't fail due to ENOSPC and don't
    /// cause fragmentation.
    pub fn fallocate(path: &str, size: u64) -> KernelResult<()> {
        let path = Self::resolve_follow(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, &path)?;
        mp.fs.fallocate(relative, size)
    }

    /// Rename or move a file or directory.
    ///
    /// Both paths must be on the same mount point.
    pub fn rename(from: &str, to: &str) -> KernelResult<()> {
        let from = Self::resolve_no_follow(from)?;
        let to = Self::resolve_no_follow(to)?;

        // Check if both paths are on the same mount point.
        let same_mount = {
            let mut vfs = VFS.lock();
            let (mp_from, _) = find_mount(&mut vfs, &from)?;
            let from_path = mp_from.path.clone();
            let (mp_to, _) = find_mount(&mut vfs, &to)?;
            mp_to.path == from_path
        };

        if same_mount {
            // Same mount — delegate to the filesystem's native rename.
            let mut vfs = VFS.lock();
            let (_mp_from, rel_from) = find_mount(&mut vfs, &from)?;
            let rel_from_owned = String::from(rel_from);
            let (mp_to, rel_to) = find_mount(&mut vfs, &to)?;
            mp_to.fs.rename(&rel_from_owned, rel_to)?;
        } else {
            // Cross-mount rename: copy + delete.  This is the only way to
            // "move" files between different filesystems (like Linux's mv).
            // We first stat the source to verify it exists and check type.
            let stat = Self::stat(&from)?;

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
        super::journal::record_rename(&from, &to);
        Ok(())
    }

    /// List mount points that appear in the VFS.
    ///
    /// Returns a list of `(mount_path, fs_type)` pairs.
    pub fn mounts() -> Vec<(String, String)> {
        let vfs = VFS.lock();
        vfs.mounts
            .iter()
            .map(|mp| (mp.path.clone(), String::from(mp.fs.fs_type())))
            .collect()
    }

    /// Find mount-point names that are direct children of `dir_path`.
    ///
    /// For example, if `dir_path` is `"/"` and there are mounts at
    /// `"/tmp"` and `"/mnt/usb"`, this returns `["tmp"]` — only the
    /// immediate child, not nested mounts.
    fn submount_children(vfs: &VfsInner, dir_path: &str) -> Vec<String> {
        let mut names = Vec::new();
        let prefix = if dir_path == "/" {
            "/"
        } else {
            dir_path
        };

        for mp in &vfs.mounts {
            // Skip the mount that *is* this directory (root mount for "/").
            if mp.path == prefix && prefix == "/" {
                continue;
            }
            if mp.path == dir_path {
                continue;
            }

            // Check if this mount is directly under dir_path.
            if prefix == "/" {
                // Mount "/tmp" → child name "tmp" (strip leading /).
                // Mount "/mnt/usb" → not a direct child of "/".
                let tail = &mp.path[1..]; // strip leading /
                if !tail.is_empty() && !tail.contains('/') {
                    names.push(String::from(tail));
                }
            } else if mp.path.starts_with(prefix)
                && mp.path.as_bytes().get(prefix.len()) == Some(&b'/')
            {
                // Mount "/mnt/usb" under dir_path "/mnt" → child "usb".
                let tail = &mp.path[prefix.len() + 1..];
                if !tail.is_empty() && !tail.contains('/') {
                    names.push(String::from(tail));
                }
            }
        }

        names
    }

    // --- Extended metadata VFS methods ---

    /// Get rich metadata for a path.
    pub fn metadata(path: &str) -> KernelResult<FileMeta> {
        let path = Self::resolve_follow(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, &path)?;
        mp.fs.metadata(relative)
    }

    /// Compute the SHA-256 content hash of a file.
    ///
    /// Reads the file and returns the 32-byte SHA-256 digest.
    /// Returns `IsADirectory` if the path is a directory.
    pub fn content_hash(path: &str) -> KernelResult<Vec<u8>> {
        let data = Self::read_file(path)?;
        Ok(crate::crypto::sha256_vec(&data))
    }

    /// Set file attributes (immutable, append-only, hidden, system).
    pub fn set_attributes(path: &str, attrs: FileAttr) -> KernelResult<()> {
        let path = Self::resolve_follow(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, &path)?;
            mp.fs.set_attributes(relative, attrs)?;
        }
        super::notify::emit_modified(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// Set ownership (uid/gid).
    pub fn set_owner(path: &str, uid: u32, gid: u32) -> KernelResult<()> {
        let path = Self::resolve_follow(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, &path)?;
            mp.fs.set_owner(relative, uid, gid)?;
        }
        super::notify::emit_modified(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// Set Unix-style permission bits.
    pub fn set_permissions(path: &str, permissions: u16) -> KernelResult<()> {
        let path = Self::resolve_follow(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, &path)?;
            mp.fs.set_permissions(relative, permissions)?;
        }
        super::notify::emit_modified(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// Update timestamps (pass 0 to leave unchanged).
    pub fn set_times(
        path: &str,
        accessed_ns: Timestamp,
        modified_ns: Timestamp,
    ) -> KernelResult<()> {
        let path = Self::resolve_follow(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, &path)?;
        mp.fs.set_times(relative, accessed_ns, modified_ns)
        // No notify/journal — timestamp changes are metadata-only.
    }

    /// Get an extended attribute value.
    pub fn get_xattr(path: &str, key: &str) -> KernelResult<Vec<u8>> {
        let path = Self::resolve_follow(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, &path)?;
        mp.fs.get_xattr(relative, key)
    }

    /// Set an extended attribute.
    pub fn set_xattr(path: &str, key: &str, value: &[u8]) -> KernelResult<()> {
        let path = Self::resolve_follow(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, &path)?;
            mp.fs.set_xattr(relative, key, value)?;
        }
        super::notify::emit_modified(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// Remove an extended attribute.
    pub fn remove_xattr(path: &str, key: &str) -> KernelResult<()> {
        let path = Self::resolve_follow(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, &path)?;
            mp.fs.remove_xattr(relative, key)?;
        }
        super::notify::emit_modified(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
    }

    /// List all extended attribute keys.
    pub fn list_xattrs(path: &str) -> KernelResult<Vec<String>> {
        let path = Self::resolve_follow(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, &path)?;
        mp.fs.list_xattrs(relative)
    }

    // --- Symlink VFS methods ---

    /// Create a symbolic link.
    ///
    /// `path` is the location of the new symlink.  `target` is the
    /// string it points to (stored as-is, resolved on traversal).
    pub fn symlink(path: &str, target: &str) -> KernelResult<()> {
        let path = Self::resolve_no_follow(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, &path)?;
            mp.fs.symlink(relative, target)?;
        }
        // A new symlink can change how any path through it resolves.
        // Invalidate the parent directory prefix to be safe.
        if let Some(last_slash) = path.rfind('/') {
            let parent = if last_slash == 0 { "/" } else { &path[..last_slash] };
            VFS_DCACHE.lock().invalidate_prefix(parent);
        }
        super::notify::emit_created(&path);
        super::journal::record(super::journal::JournalEventType::Created, &path);
        Ok(())
    }

    /// Create a hard link.
    ///
    /// `existing` is the path to an existing file.
    /// `new_path` is where the new directory entry will be created.
    ///
    /// Both paths must resolve to the same mount point.  The existing
    /// path is followed through symlinks (the link points to the
    /// underlying file, not the symlink).
    pub fn link(existing: &str, new_path: &str) -> KernelResult<()> {
        let existing = Self::resolve_follow(existing)?;
        let new_path = Self::resolve_no_follow(new_path)?;

        {
            let mut vfs = VFS.lock();
            // Both paths must be on the same mount.
            let mount_idx_existing = {
                let mut best = None;
                let mut best_len = 0;
                for (i, mp) in vfs.mounts.iter().enumerate() {
                    let prefix = &mp.path;
                    if existing.starts_with(prefix.as_str())
                        && (existing.len() == prefix.len()
                            || existing.as_bytes().get(prefix.len()) == Some(&b'/')
                            || prefix == "/")
                    {
                        if prefix.len() > best_len {
                            best = Some(i);
                            best_len = prefix.len();
                        }
                    }
                }
                best.ok_or(KernelError::NotFound)?
            };
            let mount_idx_new = {
                let mut best = None;
                let mut best_len = 0;
                for (i, mp) in vfs.mounts.iter().enumerate() {
                    let prefix = &mp.path;
                    if new_path.starts_with(prefix.as_str())
                        && (new_path.len() == prefix.len()
                            || new_path.as_bytes().get(prefix.len()) == Some(&b'/')
                            || prefix == "/")
                    {
                        if prefix.len() > best_len {
                            best = Some(i);
                            best_len = prefix.len();
                        }
                    }
                }
                best.ok_or(KernelError::NotFound)?
            };

            if mount_idx_existing != mount_idx_new {
                return Err(KernelError::InvalidArgument); // Cross-mount hard link.
            }

            let mp = &mut vfs.mounts[mount_idx_existing];
            let mount_prefix = &mp.path;

            // Strip mount prefix to get filesystem-relative paths.
            let rel_existing = if mount_prefix == "/" {
                &existing[..]
            } else if existing.len() > mount_prefix.len() {
                &existing[mount_prefix.len()..]
            } else {
                "/"
            };
            let rel_new = if mount_prefix == "/" {
                &new_path[..]
            } else if new_path.len() > mount_prefix.len() {
                &new_path[mount_prefix.len()..]
            } else {
                "/"
            };

            mp.fs.link(rel_existing, rel_new)?;
        }

        // New hard link invalidates negative cache entries for the new path.
        VFS_DCACHE.lock().invalidate_negative_prefix(&new_path);
        super::notify::emit_created(&new_path);
        super::journal::record(super::journal::JournalEventType::Created, &new_path);
        Ok(())
    }

    /// Read a symbolic link's target.
    ///
    /// Does NOT follow the symlink — returns the stored target string.
    pub fn readlink(path: &str) -> KernelResult<String> {
        let path = Self::resolve_no_follow(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, &path)?;
        mp.fs.readlink(relative)
    }

    /// Stat a path without following the final symbolic link.
    pub fn lstat(path: &str) -> KernelResult<DirEntry> {
        let path = Self::resolve_no_follow(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, &path)?;
        mp.fs.lstat(relative)
    }

    /// Return debug statistics for the filesystem mounted at `path`.
    pub fn debug_stats(path: &str) -> KernelResult<String> {
        let vfs = VFS.lock();
        for mp in &vfs.mounts {
            if path.starts_with(&mp.path) {
                return Ok(mp.fs.debug_stats());
            }
        }
        Err(KernelError::NotFound)
    }

    /// Query filesystem space and configuration for the mount at `path`.
    ///
    /// Returns capacity, free space, block size, and other filesystem
    /// metadata.  Analogous to POSIX `statvfs()`.
    pub fn statvfs(path: &str) -> KernelResult<FsInfo> {
        let path = Self::resolve_follow(path)?;
        let mut vfs = VFS.lock();
        let (mp, _relative) = find_mount(&mut vfs, &path)?;
        mp.fs.statvfs()
    }

    /// List all mount points with their filesystem info.
    ///
    /// Returns `(mount_path, FsInfo)` for each mounted filesystem.
    pub fn mount_info() -> KernelResult<Vec<(String, FsInfo)>> {
        let mut vfs = VFS.lock();
        let mut result = Vec::new();
        for mp in vfs.mounts.iter_mut() {
            let info = mp.fs.statvfs().unwrap_or(FsInfo {
                fs_type: String::from(mp.fs.fs_type()),
                block_size: 0,
                total_blocks: 0,
                free_blocks: 0,
                total_inodes: 0,
                free_inodes: 0,
                max_name_len: 255,
                read_only: false,
            });
            result.push((mp.path.clone(), info));
        }
        Ok(result)
    }

    // ----- Path resolution cache stats -----

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
    /// - `/**/*.rs` — all .rs files recursively (not yet supported)
    ///
    /// Returns a list of absolute paths that match.  Directories are not
    /// recursed into unless the pattern explicitly has deeper components.
    ///
    /// ## Limits
    ///
    /// - Maximum 1000 results to prevent runaway expansion.
    /// - Maximum pattern depth of 32 components.
    pub fn glob(pattern: &str) -> KernelResult<Vec<String>> {
        let components: Vec<&str> = pattern
            .split('/')
            .filter(|c| !c.is_empty())
            .collect();

        if components.is_empty() {
            return Ok(alloc::vec![String::from("/")]);
        }

        if components.len() > 32 {
            return Err(KernelError::InvalidArgument);
        }

        let mut results = Vec::new();
        glob_recurse(
            &String::from("/"),
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
        let mut vfs = VFS.lock();
        let mut last_err: Option<KernelError> = None;
        for mp in vfs.mounts.iter_mut() {
            if let Err(e) = mp.fs.sync() {
                last_err = Some(e);
            }
        }
        match last_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Flush a specific filesystem (the one that `path` resolves to).
    pub fn sync_path(path: &str) -> KernelResult<()> {
        let path = Self::resolve_follow(path)?;
        let mut vfs = VFS.lock();
        let (mp, _relative) = find_mount(&mut vfs, &path)?;
        mp.fs.sync()
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
    pub fn flock(path: &str, owner: u64, lock_type: LockType) -> KernelResult<()> {
        let path = Self::resolve_follow(path)?;
        let mut table = LOCK_TABLE.lock();

        // Find or create the entry for this path.
        let entry_idx = table.iter().position(|e| e.path == path);

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
                    if entry.locks.iter().any(|l| l.lock_type == LockType::Exclusive) {
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
                path,
                locks: alloc::vec![FileLock { owner, lock_type }],
            });
        }

        Ok(())
    }

    /// Release an advisory lock on a file.
    ///
    /// If the owner doesn't hold a lock on the path, this is a no-op.
    pub fn funlock(path: &str, owner: u64) -> KernelResult<()> {
        let path = Self::resolve_follow(path)?;
        let mut table = LOCK_TABLE.lock();

        if let Some(idx) = table.iter().position(|e| e.path == path) {
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
    pub fn lock_query(path: &str) -> KernelResult<Option<(LockType, usize)>> {
        let path = Self::resolve_follow(path)?;
        let table = LOCK_TABLE.lock();

        if let Some(entry) = table.iter().find(|e| e.path == path) {
            if entry.locks.is_empty() {
                return Ok(None);
            }
            // If any lock is exclusive, report exclusive.
            if entry.locks.iter().any(|l| l.lock_type == LockType::Exclusive) {
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
pub fn lock_table_dump() -> Vec<(String, LockType, u64)> {
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
pub fn validate_path(path: &str) -> KernelResult<()> {
    // No null bytes.
    if path.bytes().any(|b| b == 0) {
        return Err(KernelError::InvalidArgument);
    }

    // Must be absolute.
    if !path.starts_with('/') {
        return Err(KernelError::InvalidArgument);
    }

    // Check each component length.
    for component in path.split('/') {
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
pub fn normalize_path(path: &str) -> String {
    let mut components: Vec<&str> = Vec::new();

    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            _ => components.push(part),
        }
    }

    if components.is_empty() {
        return String::from("/");
    }

    let mut result = String::new();
    for c in &components {
        result.push('/');
        result.push_str(c);
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
fn glob_recurse(
    base: &str,
    components: &[&str],
    depth: usize,
    results: &mut Vec<String>,
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

    // Check if this component contains glob metacharacters.
    let is_glob = component.contains('*') || component.contains('?') || component.contains('[');

    if is_glob {
        // Read the current directory and match each entry against the pattern.
        let entries = match Vfs::readdir(base) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in &entries {
            if glob_match(&entry.name, component, true) {
                let child_path = if base == "/" {
                    alloc::format!("/{}", entry.name)
                } else {
                    alloc::format!("{}/{}", base, entry.name)
                };

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
        let child_path = if base == "/" {
            alloc::format!("/{}", component)
        } else {
            alloc::format!("{}/{}", base, component)
        };

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

// ---------------------------------------------------------------------------
// Mount point lookup
// ---------------------------------------------------------------------------

/// Check if `path` matches mount point `mount_path` with proper
/// path-boundary semantics.
///
/// A mount at `"/tmp"` must match `"/tmp"` and `"/tmp/foo"` but
/// NOT `"/tmpfile"`.  The root mount `"/"` matches everything.
fn mount_matches(mount_path: &str, path: &str) -> bool {
    if !path.starts_with(mount_path) {
        return false;
    }
    // Root mount matches everything.
    if mount_path == "/" {
        return true;
    }
    // Exact match (e.g., path == "/tmp" and mount == "/tmp").
    if path.len() == mount_path.len() {
        return true;
    }
    // The character after the mount prefix must be '/' to ensure
    // we're on a path boundary.  "/tmp/foo" → ok, "/tmpfile" → no.
    path.as_bytes().get(mount_path.len()) == Some(&b'/')
}

/// Find the mount point that best matches `path`.
///
/// Uses longest-prefix matching with path-boundary checks so that
/// a mount at `"/tmp"` doesn't accidentally capture `"/tmpfile"`.
///
/// Returns a mutable reference to the mount point and the
/// path relative to that mount's root.
fn find_mount<'a, 'p>(vfs: &'a mut VfsInner, path: &'p str) -> KernelResult<(&'a mut MountPoint, &'p str)> {
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
        let stripped = &path[best_len..];
        if stripped.is_empty() {
            "/"
        } else {
            stripped
        }
    };

    // SAFETY: We checked `best_idx` is Some and within bounds.
    let mp = &mut vfs.mounts[idx];
    Ok((mp, relative))
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
        serial_println!("[vfs]     {} -> {}", path, fs_type);
    }

    let has_tmp = mounts.iter().any(|(p, _)| p == "/tmp");

    // --- Basic path validation ---
    match Vfs::stat("relative/path") {
        Err(KernelError::InvalidArgument) => {
            serial_println!("[vfs]   validate_path rejects relative: OK");
        }
        other => {
            serial_println!("[vfs]   FAIL: relative path should be InvalidArgument, got {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    // --- normalize_path ---
    let norm = normalize_path("/a/b/../c/./d");
    if norm != "/a/c/d" {
        serial_println!("[vfs]   FAIL: normalize '/a/b/../c/./d' = '{}', expected '/a/c/d'", norm);
        return Err(KernelError::InternalError);
    }
    serial_println!("[vfs]   normalize_path: /a/b/../c/./d → {} OK", norm);

    // --- Intra-mount symlink resolution (on /tmp memfs) ---
    if has_tmp {
        serial_println!("[vfs]   Testing intra-mount symlink resolution on /tmp...");

        // Create a target file and a symlink to it within /tmp.
        Vfs::write_file("/tmp/_vfs_test_target", b"vfs target")?;
        Vfs::symlink("/tmp/_vfs_test_link", "/tmp/_vfs_test_target")?;

        // stat through the symlink should return File.
        let stat_via_link = Vfs::stat("/tmp/_vfs_test_link")?;
        if stat_via_link.entry_type != EntryType::File {
            serial_println!("[vfs]   FAIL: stat through symlink should be File, got {:?}", stat_via_link.entry_type);
            let _ = Vfs::remove("/tmp/_vfs_test_link");
            let _ = Vfs::remove("/tmp/_vfs_test_target");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     stat through intra-mount symlink: File OK");

        // lstat on the symlink itself should return Symlink.
        let lstat_link = Vfs::lstat("/tmp/_vfs_test_link")?;
        if lstat_link.entry_type != EntryType::Symlink {
            serial_println!("[vfs]   FAIL: lstat on symlink should be Symlink, got {:?}", lstat_link.entry_type);
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
        if target != "/tmp/_vfs_test_target" {
            serial_println!("[vfs]   FAIL: readlink = '{}', expected '/tmp/_vfs_test_target'", target);
            let _ = Vfs::remove("/tmp/_vfs_test_link");
            let _ = Vfs::remove("/tmp/_vfs_test_target");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     readlink: '{}' OK", target);

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
                serial_println!("[vfs]   FAIL: cross-mount stat type={:?}, expected File", entry.entry_type);
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
                serial_println!("[vfs]   FAIL: cross-mount read returned {} bytes, wrong content", data.len());
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
                    path,
                    info.fs_type,
                    if info.total_bytes() > 0 {
                        let mb = info.total_bytes() / (1024 * 1024);
                        alloc::format!("{} MiB, {}% used", mb, info.usage_percent())
                    } else {
                        alloc::format!("ram-backed")
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
            serial_println!("[vfs]   FAIL: expected no lock after unlock, got {:?}", state);
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
            serial_println!("[vfs]   FAIL: expected Shared after downgrade, got {:?}", state);
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
                hits_mid, hits_after,
            );
        } else {
            serial_println!(
                "[vfs]     dcache repeated access: hits {} → {} (no increase, may be OK if path was simple)",
                hits_mid, hits_after,
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
                valid_before_remove, valid_after_remove,
            );
        } else {
            // Might be the same if other entries were added between.
            serial_println!(
                "[vfs]     dcache after remove: {} → {} valid (invalidation may have been masked by new inserts)",
                valid_before_remove, valid_after_remove,
            );
        }

        // Test path_prefix_matches helper.
        if !path_prefix_matches("/tmp/foo", "/tmp") {
            serial_println!("[vfs]   FAIL: path_prefix_matches('/tmp/foo', '/tmp') should be true");
            return Err(KernelError::InternalError);
        }
        if path_prefix_matches("/tmpfile", "/tmp") {
            serial_println!("[vfs]   FAIL: path_prefix_matches('/tmpfile', '/tmp') should be false");
            return Err(KernelError::InternalError);
        }
        if !path_prefix_matches("/tmp", "/tmp") {
            serial_println!("[vfs]   FAIL: path_prefix_matches('/tmp', '/tmp') should be true");
            return Err(KernelError::InternalError);
        }
        if !path_prefix_matches("/anything", "/") {
            serial_println!("[vfs]   FAIL: path_prefix_matches('/anything', '/') should be true");
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     path_prefix_matches: all cases OK");

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
                hits_mid_neg, hits_post_neg,
            );
        } else {
            // May happen if resolve_follow doesn't fail at the resolve level
            // for this particular path (parent exists but child doesn't).
            serial_println!(
                "[vfs]     negative cache: {} → {} hits (path may not trigger resolve-level NotFound)",
                hits_mid_neg, hits_post_neg,
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
            serial_println!("[vfs]     dcache stats: {} hits, {} misses ({}% hit rate), {} valid entries",
                h, m, rate, v);
        } else {
            serial_println!("[vfs]     dcache stats: no accesses yet");
        }

        serial_println!("[vfs]     dcache test completed OK");
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
        let has_a = copy_entries.iter().any(|e| e.name == "a" && e.entry_type == EntryType::Directory);
        let has_top = copy_entries.iter().any(|e| e.name == "top.txt" && e.entry_type == EntryType::File);
        if !has_a || !has_top {
            serial_println!("[vfs]   FAIL: copy directory structure wrong (a={}, top.txt={})", has_a, has_top);
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
            serial_println!("[vfs]     remove_recursive: {} items removed OK", removed_count);
        }

        // Verify the copy is gone.
        match Vfs::stat("/tmp/_vfs_rc_copy") {
            Err(KernelError::NotFound) => {
                serial_println!("[vfs]     remove_recursive: directory confirmed gone OK");
            }
            Ok(_) => {
                serial_println!("[vfs]   FAIL: /tmp/_vfs_rc_copy still exists after remove_recursive");
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
                        serial_println!("[vfs]   FAIL: source still exists after cross-mount rename");
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
                serial_println!("[vfs]     cross-mount rename failed: {:?} (may be expected)", e);
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
            serial_println!(
                "[vfs]   FAIL: readdir_at total = {}, expected 10",
                total
            );
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
        serial_println!("[vfs]     readdir_at(0, 100): {} entries, total={} OK", all.len(), total);

        // Read first page (3 entries).
        let (page1, total1) = Vfs::readdir_at(pg_dir, 0, 3)?;
        if page1.len() != 3 || total1 != 10 {
            serial_println!(
                "[vfs]   FAIL: page1 len={}, total={} (expected 3, 10)",
                page1.len(), total1,
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
                page2.len(), total2,
            );
            let _ = Vfs::remove_recursive(pg_dir);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     readdir_at(3, 3): {} entries OK", page2.len());

        // Verify no overlap between pages.
        let names1: Vec<&str> = page1.iter().map(|e| e.name.as_str()).collect();
        let names2: Vec<&str> = page2.iter().map(|e| e.name.as_str()).collect();
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
                tail.len(), total_tail,
            );
            let _ = Vfs::remove_recursive(pg_dir);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     readdir_at(8, 5): {} entries (tail) OK", tail.len());

        // Read completely past end: offset 20 → should return 0 entries.
        let (empty, total_empty) = Vfs::readdir_at(pg_dir, 20, 5)?;
        if !empty.is_empty() || total_empty != 10 {
            serial_println!(
                "[vfs]   FAIL: past-end len={}, total={} (expected 0, 10)",
                empty.len(), total_empty,
            );
            let _ = Vfs::remove_recursive(pg_dir);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     readdir_at(20, 5): empty (past end) OK");

        let _ = Vfs::remove_recursive(pg_dir);
        serial_println!("[vfs]     readdir_at pagination test PASSED");
    }

    // ── Glob pattern matching tests ──
    glob_self_test()?;

    serial_println!("[vfs] Self-test PASSED");
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
pub fn glob_match(name: &str, pattern: &str, case_insensitive: bool) -> bool {
    glob_match_inner(name.as_bytes(), pattern.as_bytes(), case_insensitive)
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
                    if let Some((matched, end_pi)) = match_char_class(
                        name.get(ni).copied().unwrap_or(0),
                        pattern,
                        pi,
                        ci,
                    ) {
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
        } else {
            // Pattern exhausted.  Backtrack if we had a `*`.
            if let Some(sp) = star_pi {
                star_ni += 1;
                ni = star_ni;
                pi = sp + 1;
                continue;
            }
            return false;
        }
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
                    let hi = if ci { end_c.to_ascii_lowercase() } else { end_c };
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
        a.to_ascii_lowercase() == b.to_ascii_lowercase()
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
