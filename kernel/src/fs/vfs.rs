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
    /// `mount_path` must start with `/`.  Currently only `"/"` is
    /// supported (single root mount).
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
    pub fn readdir(path: &str) -> KernelResult<Vec<DirEntry>> {
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, path)?;
        mp.fs.readdir(relative)
    }

    /// Read a file's contents.
    pub fn read_file(path: &str) -> KernelResult<Vec<u8>> {
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, path)?;
        mp.fs.read_file(relative)
    }

    /// Get metadata for a path.
    pub fn stat(path: &str) -> KernelResult<DirEntry> {
        let mut vfs = VFS.lock();
        let (mp, relative) = find_mount(&mut vfs, path)?;
        mp.fs.stat(relative)
    }
}

/// Find the mount point that best matches `path`.
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
        if path.starts_with(&mp.path) && mp.path.len() >= best_len {
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
