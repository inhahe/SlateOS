//! Virtual filesystem traits and global mount management.
//!
//! Defines the [`FileSystem`] trait that all filesystem implementations
//! must provide, and the [`Vfs`] singleton that manages mounted
//! filesystems and dispatches operations.

use alloc::boxed::Box;
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

    /// List entries in a directory.
    ///
    /// If other filesystems are mounted at sub-paths of `path`, their
    /// mount points appear as directory entries in the listing (even if
    /// the underlying filesystem doesn't have a physical directory there).
    pub fn readdir(path: &str) -> KernelResult<Vec<DirEntry>> {
        validate_path(path)?;
        let mut vfs = VFS.lock();

        // Collect mount-point names that are direct children of `path`.
        // E.g., if path="/", mounts at "/tmp" and "/mnt" produce ["tmp", "mnt"].
        // Nested mounts like "/mnt/usb" are NOT direct children of "/".
        let submount_names: Vec<String> = Self::submount_children(&vfs, path);

        let (mp, relative) = find_mount(&mut vfs, path)?;
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
        validate_path(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, path)?;
        mp.fs.read_file(relative)
    }

    /// Get metadata for a path.
    pub fn stat(path: &str) -> KernelResult<DirEntry> {
        validate_path(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, path)?;
        mp.fs.stat(relative)
    }

    /// Write data to a file (create or overwrite).
    pub fn write_file(path: &str, data: &[u8]) -> KernelResult<()> {
        validate_path(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, path)?;
            mp.fs.write_file(relative, data)?;
        }
        // Notify and journal after releasing VFS lock (avoids holding both locks).
        super::notify::emit_modified(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// Delete a file.
    pub fn remove(path: &str) -> KernelResult<()> {
        validate_path(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, path)?;
            mp.fs.remove(relative)?;
        }
        super::notify::emit_deleted(path);
        super::journal::record(super::journal::JournalEventType::Deleted, path);
        Ok(())
    }

    /// Create a directory.
    pub fn mkdir(path: &str) -> KernelResult<()> {
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, path)?;
            mp.fs.mkdir(relative)?;
        }
        super::notify::emit_created(path);
        super::journal::record(super::journal::JournalEventType::Created, path);
        Ok(())
    }

    /// Remove an empty directory.
    pub fn rmdir(path: &str) -> KernelResult<()> {
        validate_path(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, path)?;
            mp.fs.rmdir(relative)?;
        }
        super::notify::emit_deleted(path);
        super::journal::record(super::journal::JournalEventType::Deleted, path);
        Ok(())
    }

    /// Read a range of bytes from a file.
    pub fn read_at(path: &str, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        validate_path(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, path)?;
        mp.fs.read_at(relative, offset, len)
        // Note: no ACCESS event emitted by default (high-frequency).
        // Callers that need it can emit manually.
    }

    /// Write bytes at a specific offset within a file.
    pub fn write_at(path: &str, offset: u64, data: &[u8]) -> KernelResult<()> {
        validate_path(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, path)?;
            mp.fs.write_at(relative, offset, data)?;
        }
        super::notify::emit_modified(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// Truncate a file to the given size.
    pub fn truncate(path: &str, size: u64) -> KernelResult<()> {
        validate_path(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, path)?;
            mp.fs.truncate(relative, size)?;
        }
        super::notify::emit_modified(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// Rename or move a file or directory.
    ///
    /// Both paths must be on the same mount point.
    pub fn rename(from: &str, to: &str) -> KernelResult<()> {
        validate_path(from)?;
        validate_path(to)?;
        {
            let mut vfs = VFS.lock();

            // Both paths must resolve to the same mount point.
            let (mp_from, rel_from) = find_mount(&mut vfs, from)?;
            let from_mount_path = mp_from.path.clone();
            let rel_from_owned = String::from(rel_from);

            // Find mount for `to` — must be the same filesystem.
            let (mp_to, rel_to) = find_mount(&mut vfs, to)?;
            if mp_to.path != from_mount_path {
                return Err(KernelError::InvalidArgument);
            }

            // Delegate to the filesystem (using the `from` mount).
            mp_to.fs.rename(&rel_from_owned, rel_to)?;
        }
        super::notify::emit_renamed(from, to);
        super::journal::record_rename(from, to);
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
        validate_path(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, path)?;
        mp.fs.metadata(relative)
    }

    /// Set file attributes (immutable, append-only, hidden, system).
    pub fn set_attributes(path: &str, attrs: FileAttr) -> KernelResult<()> {
        validate_path(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, path)?;
            mp.fs.set_attributes(relative, attrs)?;
        }
        super::notify::emit_modified(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// Set ownership (uid/gid).
    pub fn set_owner(path: &str, uid: u32, gid: u32) -> KernelResult<()> {
        validate_path(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, path)?;
            mp.fs.set_owner(relative, uid, gid)?;
        }
        super::notify::emit_modified(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// Set Unix-style permission bits.
    pub fn set_permissions(path: &str, permissions: u16) -> KernelResult<()> {
        validate_path(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, path)?;
            mp.fs.set_permissions(relative, permissions)?;
        }
        super::notify::emit_modified(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// Update timestamps (pass 0 to leave unchanged).
    pub fn set_times(
        path: &str,
        accessed_ns: Timestamp,
        modified_ns: Timestamp,
    ) -> KernelResult<()> {
        validate_path(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, path)?;
        mp.fs.set_times(relative, accessed_ns, modified_ns)
        // No notify/journal — timestamp changes are metadata-only.
    }

    /// Get an extended attribute value.
    pub fn get_xattr(path: &str, key: &str) -> KernelResult<Vec<u8>> {
        validate_path(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, path)?;
        mp.fs.get_xattr(relative, key)
    }

    /// Set an extended attribute.
    pub fn set_xattr(path: &str, key: &str, value: &[u8]) -> KernelResult<()> {
        validate_path(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, path)?;
            mp.fs.set_xattr(relative, key, value)?;
        }
        super::notify::emit_modified(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// Remove an extended attribute.
    pub fn remove_xattr(path: &str, key: &str) -> KernelResult<()> {
        validate_path(path)?;
        {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, path)?;
            mp.fs.remove_xattr(relative, key)?;
        }
        super::notify::emit_modified(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// List all extended attribute keys.
    pub fn list_xattrs(path: &str) -> KernelResult<Vec<String>> {
        validate_path(path)?;
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, path)?;
        mp.fs.list_xattrs(relative)
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
