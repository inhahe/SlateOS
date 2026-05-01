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
