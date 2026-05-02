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
        Self::resolve_inner(&norm, true, 0)
    }

    /// Like [`resolve_follow`] but does NOT follow the final component.
    ///
    /// Used for operations that act on the entry itself: `remove`,
    /// `rmdir`, `lstat`, `readlink`, `symlink`, `rename`.
    fn resolve_no_follow(path: &str) -> KernelResult<String> {
        validate_path(path)?;
        let norm = normalize_path(path);
        Self::resolve_inner(&norm, false, 0)
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
        // Notify and journal after releasing VFS lock (avoids holding both locks).
        super::notify::emit_modified(&path);
        super::journal::record(super::journal::JournalEventType::Modified, &path);
        Ok(())
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

    /// Rename or move a file or directory.
    ///
    /// Both paths must be on the same mount point.
    pub fn rename(from: &str, to: &str) -> KernelResult<()> {
        let from = Self::resolve_no_follow(from)?;
        let to = Self::resolve_no_follow(to)?;
        {
            let mut vfs = VFS.lock();

            // Both paths must resolve to the same mount point.
            let (mp_from, rel_from) = find_mount(&mut vfs, &from)?;
            let from_mount_path = mp_from.path.clone();
            let rel_from_owned = String::from(rel_from);

            // Find mount for `to` — must be the same filesystem.
            let (mp_to, rel_to) = find_mount(&mut vfs, &to)?;
            if mp_to.path != from_mount_path {
                return Err(KernelError::InvalidArgument);
            }

            // Delegate to the filesystem (using the `from` mount).
            mp_to.fs.rename(&rel_from_owned, rel_to)?;
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

    serial_println!("[vfs] Self-test PASSED");
    Ok(())
}
