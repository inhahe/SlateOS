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

use super::attr_policy;
use super::xattr_policy;
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
    /// Volume label — FAT-specific, and **never present in a listing**.
    ///
    /// FAT stores the label in a root-directory slot; that is an encoding
    /// detail, not a claim that the volume contains a file by that name.
    /// [`Vfs::drop_volume_labels`] removes it on every route out of the VFS,
    /// so this variant reaches no caller of `readdir`, `readdir_at` or
    /// `readdir_pinned` and no syscall's entry-type byte. The label is
    /// available from [`Vfs::statvfs`]'s `volume_label` instead.
    ///
    /// The variant is kept rather than deleted because `FatFs::metadata`
    /// still classifies a raw on-disk slot with it internally, and because
    /// deleting it would silently renumber the syscall entry-type bytes
    /// above it. Byte `2` stays reserved.
    VolumeLabel,
    /// Character device node (`/dev/input/event0`, `/dev/dri/card0`, …).
    ///
    /// Only [`devfs`](crate::fs::devfs) produces these, and they exist because
    /// `S_IFCHR` is load-bearing for real clients rather than cosmetic:
    /// libinput refuses a node that is not `S_ISCHR`, and libdrm and ALSA make
    /// the same check. Before this variant, `stat("/dev/input/event0")`
    /// reported a regular file, and those libraries would have rejected a
    /// device that works perfectly.
    CharDevice,

    /// Block device node (`/dev/vda`, `/dev/nvme0n1`, …).
    ///
    /// This variant did not exist until there was something to name. The
    /// comment it replaces said so plainly — "this kernel has no block device
    /// nodes to name" — and that was true: storage was reached through the VFS
    /// and nothing else. It stopped being true when
    /// [`devfs`](crate::fs::devfs) began publishing one node per registered
    /// [`crate::blkdev`] device, which is what a disk imager or a partition
    /// editor needs: those programs' subject *is* the raw device, so reaching
    /// it through a mounted filesystem is not a smaller version of the job, it
    /// is a different job.
    ///
    /// `S_IFBLK` is load-bearing for the same reason `S_IFCHR` is above. A
    /// program that is about to overwrite a whole disk checks what it is
    /// pointed at first, and a raw device reported as a regular file is one
    /// that such a check waves through — the failure mode being writing a disk
    /// image over somebody's file rather than over their USB stick.
    BlockDevice,

    /// A Unix-domain socket's name (`S_IFSOCK`): the node `bind` creates at a
    /// path such as `/dev/log`, and the one `connect` looks for there.
    ///
    /// The node holds nothing. What a `connect` reaches is the socket bound to
    /// it, found by the node's identity ([`FileId`]) in
    /// [`crate::ipc::unix_socket`]'s table -- so a renamed node still leads to
    /// its socket, and an unlinked one leads nowhere, as on Linux. A node
    /// whose socket has closed stays, and `connect` to it is refused.
    /// Opening one is `ENXIO`: it has no contents to read.
    Socket,
}

impl EntryType {
    /// The entry-type byte the native ABI carries for this type, in every
    /// directory record and stat result.
    ///
    /// These bytes are ABI, so a new type is appended and never inserted: a
    /// program that does not know 6 still reads 0..=5 as it always did. They
    /// were five copies of one `match` in `syscall/handlers.rs` until
    /// 2026-10-02, when the sixth type would have meant five more edits that
    /// had to agree.
    #[must_use]
    pub const fn type_byte(self) -> u8 {
        match self {
            Self::File => 0,
            Self::Directory => 1,
            Self::VolumeLabel => 2,
            Self::Symlink => 3,
            Self::CharDevice => 4,
            Self::BlockDevice => 5,
            Self::Socket => 6,
        }
    }
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
    /// Inode number of the object this name refers to, or `0` when the
    /// filesystem has no stable per-object identity to report.
    ///
    /// This is the **same number** [`FileMeta::ino`] carries for the same
    /// object, and that is the whole point of the field: `readdir`'s `d_ino`
    /// and `stat`'s `st_ino` are cross-checked by real programs, and two
    /// answers that disagree are worse than one answer that is missing.
    /// Every backend has the inode bound at the moment it builds a
    /// `DirEntry` — the ext4 arms take it as a closure parameter and use it
    /// on the line above — so filling it in costs nothing and makes the two
    /// agree *by construction* rather than by two implementations
    /// independently getting it right.
    ///
    /// Before this existed the wire record had no such field and both
    /// consumers invented one. `posix`'s `readdir` used the entry's index in
    /// the listing, which gives the first entry of every directory
    /// `d_ino == 0` — the value the ABI reserves for "not available" — and
    /// makes `/a/foo` and `/b/bar` the same inode, so `du` and `tar`
    /// coalesce them. The Linux-ABI `getdents64` used an FNV hash of
    /// `path + "/" + name`, which is stable and collision-resistant but can
    /// never equal the `st_ino` its own `stat` reports for the same file, so
    /// `find -inum`, `ls -i` compared against `stat`, and `rsync`'s and
    /// `tar`'s hard-link detection all cross-check two numbers guaranteed to
    /// differ. A client cannot manufacture this value; only the filesystem
    /// knows it.
    ///
    /// `0` is honest rather than absent: FAT, ISO9660 and the
    /// pseudo-filesystems have no inode to report and their [`FileMeta`]
    /// says `0` as well, so the two still agree. See
    /// `requests/b-a-664s-record-has-no-inode-and-647-turns-out-to-have-no-callers-either.md`.
    pub ino: u64,
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

/// "Now", as a time requested of [`Vfs::set_times`] and its variants: Linux's
/// `UTIME_NOW`. The VFS makes it the current wall-clock time before the
/// filesystem sees it.
///
/// A request rather than a time because the two are refused differently: an
/// append-only file may have its times set to now -- `touch` -- but not to
/// given values, which could backdate a log (`fs::attr_policy::may_touch`).
/// A caller that computed "now" itself would be setting a given value.
/// `u64::MAX` is the year 2554, which no file has.
pub const TIME_NOW: Timestamp = u64::MAX;

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
    /// The device the file is on: its filesystem's device number, which
    /// `stat` reports as `st_dev`'s minor under major 0 ([`dev_of`]).
    ///
    /// Filled by the VFS from the mount the file was found on, never by a
    /// filesystem, which leaves it 0; 0 means unknown. One `(dev, ino)` pair
    /// is one file -- how `tar`, `cp -a`, `du` and `find -samefile` tell two
    /// names of one file from two files. Until 2026-10-01 every file reported
    /// 0, so files on two filesystems that shared an inode number were one.
    pub dev: u32,
    /// The device a character or block device node names (`st_rdev`), in
    /// Linux's numbering ([`crate::fs::devnum`]); [`DevNum::NONE`] for
    /// anything that is not a device node.
    pub rdev: crate::fs::devnum::DevNum,

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
    /// Unix-style permission bits — **twelve**: `setuid setgid sticky
    /// rwxrwxrwx`, i.e. `0o7777`.  0o755 = rwxr-xr-x, 0o4755 = the same
    /// with setuid, 0o1777 = `/tmp`.  0 = not applicable (e.g. FAT, which
    /// has no Unix mode and returns `NotSupported` from `set_permissions`).
    ///
    /// This said "9 bits" until 2026-08-30, and it was wrong the whole
    /// time — ext4's `vfs_impl` has always written `type_bits | (mode &
    /// 0o7777)` and read `i_mode & 0o7777` back, and memfs stores the
    /// `u16` unmasked.  The nine-bit claim was a doc comment describing a
    /// narrowing that lived in two syscall handlers rather than here, and
    /// it is why `SYS_FS_OPEN_MODE` masked away setuid for so long without
    /// anyone noticing the storage layer disagreed.  See
    /// `design-decisions.md` §639.
    pub permissions: u16,
    /// File attribute flags (immutable, append-only, etc.).
    pub attributes: FileAttr,

    // --- Extended attributes ---
    /// Arbitrary key-value metadata pairs.
    ///
    /// Both halves are byte vectors: a name is an opaque NUL-terminated byte
    /// string, exactly as a path component is, and typing it as a `String`
    /// meant a filesystem could hold a name this struct was unable to report.
    /// See `design-decisions.md` §660.
    pub xattrs: Vec<(Vec<u8>, Vec<u8>)>,

    // --- Link count ---
    /// Number of hard links pointing to the underlying data.
    /// Always 1 for filesystems that don't support hard links (FAT).
    ///
    /// For a *directory* this is the Unix convention — `2 + immediate
    /// subdirectory count`, counting `.` and each child's `..` — not the
    /// number of names the directory has, which is always 1.  `find(1)`'s
    /// leaf optimisation reads it and stops descending once it has seen
    /// `nlinks - 2` subdirectories, so reporting the name count instead
    /// would make `find` skip real directories.
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
            dev: 0,
            rdev: crate::fs::devnum::DevNum::NONE,
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
            dev: 0,
            rdev: crate::fs::devnum::DevNum::NONE,
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

    /// Whether a path resolution through this filesystem may be kept in
    /// the VFS path cache (`VFS_DCACHE`): whether the same path names the
    /// same thing for every caller, and goes on doing so until a VFS
    /// operation changes it -- which is when the cache is invalidated.
    ///
    /// `false` for procfs. Its `self` link names the caller, its process
    /// directories appear and go as processes do, and its `cwd`, `root`,
    /// `exe` and `fd/<n>` links follow the process -- Linux revalidates
    /// procfs dentries on every lookup for the same reasons.
    fn dcache_safe(&self) -> bool {
        true
    }

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

    // ----- An open regular file, by inode -----
    //
    // `fs::handle` holds an open regular file as its inode on a filesystem
    // that offers these (`Vfs::open_object`; known-issues
    // A-AN-OPEN-FILE-FOLLOWS-ITS-NAME), so a rename or an unlink of its name
    // cannot take the file from under the handle. The defaults answer
    // `NotSupported`, and such a filesystem's handles go by path.

    /// Hold inode `ino` open. Its last name may then go without the file:
    /// it stays, unnamed, until the matching [`unpin_ino`](Self::unpin_ino).
    /// `NotSupported` for an inode the filesystem does not hold this way.
    fn pin_ino(&mut self, ino: u64) -> KernelResult<()> {
        let _ = ino;
        Err(KernelError::NotSupported)
    }

    /// Give back one [`pin_ino`](Self::pin_ino). A file with no name left
    /// goes with its last pin.
    fn unpin_ino(&mut self, ino: u64) {
        let _ = ino;
    }

    /// [`read_at`](Self::read_at) for a held inode.
    fn read_ino(&mut self, ino: u64, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        let _ = (ino, offset, len);
        Err(KernelError::NotSupported)
    }

    /// [`write_at`](Self::write_at) for a held inode. Never creates a file.
    fn write_ino(&mut self, ino: u64, offset: u64, data: &[u8]) -> KernelResult<()> {
        let _ = (ino, offset, data);
        Err(KernelError::NotSupported)
    }

    /// Write `data` at a held inode's end, the end found and written in one
    /// call so two appenders cannot land on one offset (`O_APPEND`). Returns
    /// the offset it landed at.
    fn append_ino(&mut self, ino: u64, data: &[u8]) -> KernelResult<u64> {
        let _ = (ino, data);
        Err(KernelError::NotSupported)
    }

    /// [`truncate`](Self::truncate) for a held inode.
    fn truncate_ino(&mut self, ino: u64, size: u64) -> KernelResult<()> {
        let _ = (ino, size);
        Err(KernelError::NotSupported)
    }

    /// [`metadata`](Self::metadata) for a held inode: `nlinks` 0 once its
    /// last name has gone, as Linux's `fstat` reports it.
    fn metadata_ino(&mut self, ino: u64) -> KernelResult<FileMeta> {
        let _ = ino;
        Err(KernelError::NotSupported)
    }

    /// [`set_permissions`](Self::set_permissions) for a held inode:
    /// `fchmod`, whatever the file's name now.
    fn chmod_ino(&mut self, ino: u64, permissions: u16) -> KernelResult<()> {
        let _ = (ino, permissions);
        Err(KernelError::NotSupported)
    }

    /// [`set_attributes`](Self::set_attributes) for a held inode:
    /// `FS_IOC_SETFLAGS` through a descriptor, whatever the file's name now.
    fn set_attributes_ino(&mut self, ino: u64, attrs: FileAttr) -> KernelResult<()> {
        let _ = (ino, attrs);
        Err(KernelError::NotSupported)
    }

    /// [`set_owner`](Self::set_owner) for a held inode: `fchown`. The ids are
    /// concrete; the VFS resolves "leave unchanged" first.
    fn chown_ino(&mut self, ino: u64, uid: u32, gid: u32) -> KernelResult<()> {
        let _ = (ino, uid, gid);
        Err(KernelError::NotSupported)
    }

    /// [`set_times`](Self::set_times) for a held inode: `futimens`. A time of
    /// 0 is left as it is.
    fn utimes_ino(
        &mut self,
        ino: u64,
        accessed_ns: Timestamp,
        modified_ns: Timestamp,
    ) -> KernelResult<()> {
        let _ = (ino, accessed_ns, modified_ns);
        Err(KernelError::NotSupported)
    }

    /// [`fallocate`](Self::fallocate) for a held inode. The default, as
    /// `fallocate`'s, reserves nothing and reports success: the writes that
    /// come allocate.
    fn fallocate_ino(&mut self, ino: u64, size: u64) -> KernelResult<()> {
        let _ = (ino, size);
        Ok(())
    }

    /// Make a regular file with no name in directory `dir`, held once as
    /// [`pin_ino`](Self::pin_ino) holds one, and return its inode: Linux's
    /// `->tmpfile`, behind `O_TMPFILE`. It goes at the matching
    /// [`unpin_ino`](Self::unpin_ino) unless
    /// [`link_held_ino`](Self::link_held_ino) names it first. `mode` is its
    /// permission bits. `NotSupported` where a file cannot be held unnamed.
    fn create_unnamed(&mut self, dir: &Path, mode: u16) -> KernelResult<u64> {
        let _ = (dir, mode);
        Err(KernelError::NotSupported)
    }

    /// Give held inode `ino` the name `new_path`, as [`link`](Self::link)
    /// gives an existing file another: a file
    /// [`create_unnamed`](Self::create_unnamed) made is named by it, and no
    /// longer goes at its last unpin. `AlreadyExists` if the name is taken.
    fn link_held_ino(&mut self, ino: u64, new_path: &Path) -> KernelResult<()> {
        let _ = (ino, new_path);
        Err(KernelError::NotSupported)
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
    /// Default implementation: for anything but a symlink,
    /// [`metadata`](Self::metadata), since the two differ only at a final
    /// symlink; for a symlink, a minimal [`FileMeta`] from `lstat()`.
    /// Filesystems with symlinks that track timestamps, ownership, or xattrs
    /// should override this (typically mirroring their `metadata()` override
    /// but without symlink resolution).
    ///
    /// The default built the minimal [`FileMeta`] for everything until
    /// 2026-10-02, so a filesystem with a rich `metadata` and no override --
    /// FAT, which has no symlinks to override it for, procfs, devfs, the
    /// overlay -- reported a regular file's `lstat` with no times, owner,
    /// mode or attributes: `ls -l`, which lists by `lstat`, dated every FAT
    /// file 1970, and the VFS's attribute rules (`fs::attr_policy`), which
    /// look at a name without following it, saw no read-only bit.
    fn lmetadata(&mut self, path: &Path) -> KernelResult<FileMeta> {
        let entry = self.lstat(path)?;
        if entry.entry_type == EntryType::Symlink {
            Ok(FileMeta::minimal(entry.entry_type, entry.size))
        } else {
            self.metadata(path)
        }
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

    // --- Extended attributes ---
    //
    // A key is `&[u8]`, not `&str`, for the same reason a `Path` is: the name
    // is an opaque NUL-terminated byte string that the kernel does not get to
    // interpret, and a filesystem written by Linux may well carry one that is
    // not UTF-8.  Typing it as a `str` did not merely reject such a name — the
    // `from_utf8` sat inside the loop that reads *every* attribute on the
    // inode, so one bad name failed the whole inode and took the ordinary
    // attributes down with it.  See `design-decisions.md` §660.

    /// Whether this filesystem keeps extended attributes at all: Linux's
    /// `IOP_XATTR`. On one that does not, every name is `NotSupported`
    /// (`EOPNOTSUPP`) before it is looked at -- a bare prefix too, which a
    /// filesystem that keeps them refuses as `InvalidArgument` -- as Linux's
    /// `xattr_resolve_name` answers. A listing is empty.
    ///
    /// Default: no.
    fn xattrs_supported(&self) -> bool {
        false
    }

    /// Get an extended attribute value by key.
    ///
    /// Returns [`KernelError::NoAttribute`] when the object exists but carries
    /// no attribute by that name — never `NotFound`, which is reserved for the
    /// *path* not resolving.  A caller has to tell those apart.
    ///
    /// Default: not supported.
    fn get_xattr(&mut self, path: &Path, key: &[u8]) -> KernelResult<Vec<u8>> {
        let _ = (path, key);
        Err(KernelError::NotSupported)
    }

    /// Set an extended attribute.
    ///
    /// Default: not supported.
    fn set_xattr(&mut self, path: &Path, key: &[u8], value: &[u8]) -> KernelResult<()> {
        let _ = (path, key, value);
        Err(KernelError::NotSupported)
    }

    /// Remove an extended attribute.
    ///
    /// Returns [`KernelError::NoAttribute`] when the attribute was already
    /// absent, which is what lets a "remove if present" idiom be written
    /// without a racy pre-flight probe.
    ///
    /// Default: not supported.
    fn remove_xattr(&mut self, path: &Path, key: &[u8]) -> KernelResult<()> {
        let _ = (path, key);
        Err(KernelError::NotSupported)
    }

    /// List all extended attribute keys for a path.
    ///
    /// Default: empty list.
    fn list_xattrs(&mut self, path: &Path) -> KernelResult<Vec<Vec<u8>>> {
        let _ = path;
        Ok(Vec::new())
    }

    // --- No-follow xattr variants (lgetxattr/lsetxattr/llistxattr/
    // lremovexattr): operate on a trailing symlink itself, not its target.
    // Default delegates to the following version — correct for symlink-free
    // filesystems (FAT); memfs/ext4 override to resolve the final component
    // without following. ---

    /// No-follow analogue of [`get_xattr`](Self::get_xattr) (`lgetxattr`).
    fn get_xattr_no_follow(&mut self, path: &Path, key: &[u8]) -> KernelResult<Vec<u8>> {
        self.get_xattr(path, key)
    }

    /// No-follow analogue of [`set_xattr`](Self::set_xattr) (`lsetxattr`).
    fn set_xattr_no_follow(&mut self, path: &Path, key: &[u8], value: &[u8]) -> KernelResult<()> {
        self.set_xattr(path, key, value)
    }

    /// No-follow analogue of [`remove_xattr`](Self::remove_xattr) (`lremovexattr`).
    fn remove_xattr_no_follow(&mut self, path: &Path, key: &[u8]) -> KernelResult<()> {
        self.remove_xattr(path, key)
    }

    /// No-follow analogue of [`list_xattrs`](Self::list_xattrs) (`llistxattr`).
    fn list_xattrs_no_follow(&mut self, path: &Path) -> KernelResult<Vec<Vec<u8>>> {
        self.list_xattrs(path)
    }

    // --- By inode number: a file held open, whatever its names now
    // (`fgetxattr` and the rest, through `fs::handle::HandleFile`). ---

    /// [`get_xattr`](Self::get_xattr) of the file with inode number `ino`.
    ///
    /// Default: not supported.
    fn get_xattr_ino(&mut self, ino: u64, key: &[u8]) -> KernelResult<Vec<u8>> {
        let _ = (ino, key);
        Err(KernelError::NotSupported)
    }

    /// [`set_xattr`](Self::set_xattr) of the file with inode number `ino`.
    ///
    /// Default: not supported.
    fn set_xattr_ino(&mut self, ino: u64, key: &[u8], value: &[u8]) -> KernelResult<()> {
        let _ = (ino, key, value);
        Err(KernelError::NotSupported)
    }

    /// [`remove_xattr`](Self::remove_xattr) of the file with inode number
    /// `ino`.
    ///
    /// Default: not supported.
    fn remove_xattr_ino(&mut self, ino: u64, key: &[u8]) -> KernelResult<()> {
        let _ = (ino, key);
        Err(KernelError::NotSupported)
    }

    /// [`list_xattrs`](Self::list_xattrs) of the file with inode number `ino`.
    ///
    /// Default: empty list.
    fn list_xattrs_ino(&mut self, ino: u64) -> KernelResult<Vec<Vec<u8>>> {
        let _ = ino;
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
    fn symlink(&mut self, path: &Path, target: &Path) -> KernelResult<()> {
        let _ = (path, target);
        Err(KernelError::NotSupported)
    }

    /// Create a Unix-domain socket's node ([`EntryType::Socket`]) at `path`,
    /// with permission bits `mode`, and return its inode number.
    ///
    /// `path` must not exist (`AlreadyExists`). The node has no contents; what
    /// it names is kept by [`crate::ipc::unix_socket`], keyed by the node's
    /// identity, which is why the inode number is returned and must be
    /// non-zero and stable for as long as the node exists.
    ///
    /// Default: not supported -- a filesystem that cannot hold one makes
    /// `bind` there fail as Linux's does on, say, FAT (`EPERM`).
    fn mknod_socket(&mut self, path: &Path, mode: u16) -> KernelResult<u64> {
        let _ = (path, mode);
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

    /// The type of the entry at `path`, without following a final
    /// symlink: all that path resolution needs from each component it
    /// walks (`Vfs::resolve_inner`).
    ///
    /// Defaults to [`lstat`](Self::lstat)'s. A filesystem whose `lstat`
    /// does more work than that answer needs overrides it: procfs makes a
    /// file's contents to report its size, and a procfs walk is never
    /// cached ([`dcache_safe`](Self::dcache_safe)), so resolution would
    /// make every file it opens twice.
    fn entry_type(&mut self, path: &Path) -> KernelResult<EntryType> {
        self.lstat(path).map(|e| e.entry_type)
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

    /// The volume's own identity, which travels with it from machine to
    /// machine and outlives any mount of it: ext4's superblock UUID
    /// (`s_uuid`), as `blkid` prints it.
    ///
    /// `None` for a filesystem without one (memfs, procfs, FAT here) and for an
    /// all-zero UUID, which names no volume in particular. The deferred-operation
    /// queue (`fs::deferred_ops`) files a volume's entries under it.
    fn volume_uuid(&self) -> Option<[u8; 16]> {
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
    /// Default: not supported (FAT, procfs, devfs, ISO9660).  ext4 and memfs
    /// override it.
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
    /// either lack hard links entirely (FAT, procfs, devfs, ISO9660 — they
    /// return `NotSupported` regardless) or lack symlinks (FAT), where the
    /// follow/no-follow distinction cannot arise.  ext4 and memfs override
    /// this to resolve `existing` without following the final component.
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
    /// Open files held on this mount as objects ([`Vfs::open_object`]). An
    /// unmount refuses while any is, as Linux's `umount` answers `EBUSY`:
    /// I/O through a held file must never reach a filesystem that has gone.
    /// Counted under the mount table's lock, which is what keeps an open
    /// racing an unmount from slipping between the check and the removal.
    objects: usize,
}

/// An open regular file held as its filesystem and inode, not its name
/// (known-issues A-AN-OPEN-FILE-FOLLOWS-ITS-NAME).
///
/// While one exists its inode is pinned (`FileSystem::pin_ino`): a rename
/// leaves the holder on the file, and unlinking its last name removes the
/// name while the file lives on until the hold goes. It also holds its
/// mount, which cannot be unmounted while it does.
///
/// Only ever inside a [`FileHold`], which gives the hold back when it is
/// dropped. Not `Clone`, so no copy can give it back a second time.
pub struct FileObject {
    fs: MountedFs,
    fs_id: u64,
    ino: u64,
}

impl FileObject {
    /// The file's system-wide identity: the key of the page cache and of the
    /// lock tables.
    #[must_use]
    pub fn id(&self) -> FileId {
        FileId {
            fs_id: self.fs_id,
            ino: self.ino,
        }
    }
}

impl core::fmt::Debug for FileObject {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FileObject")
            .field("fs_id", &self.fs_id)
            .field("ino", &self.ino)
            .finish_non_exhaustive()
    }
}

/// One hold on an open regular file ([`FileObject`]): the file stays, and
/// its mount cannot be unmounted, until the hold is dropped.
///
/// `fs::handle` keeps each in an `Arc`, shared by the open file descriptions
/// of the file it opened and by every call in progress through one of them.
/// So a close on one thread, racing a read or a write on another, cannot
/// give the file back under the call: the last reference to go gives it
/// back, as Linux's `fdget` keeps a `struct file` alive across a syscall.
/// Until 2026-10-01 the final close gave it back at once, and a call already
/// in the filesystem went on with an inode that might be freed: an unlinked
/// file's blocks back in the free pool, still being written.
///
/// Never to be dropped with `fs::handle`'s table lock held: giving the hold
/// back takes the filesystem's lock and the mount table's.
#[derive(Debug)]
pub struct FileHold(FileObject);

impl core::ops::Deref for FileHold {
    type Target = FileObject;

    fn deref(&self) -> &FileObject {
        &self.0
    }
}

impl Drop for FileHold {
    fn drop(&mut self) {
        release_hold(&self.0);
    }
}

/// Give back one hold ([`FileHold`]'s drop). A file whose last name went
/// while it was held goes with its last hold; the mount may be unmounted
/// once none is left.
fn release_hold(obj: &FileObject) {
    obj.fs.lock().unpin_ino(obj.ino);
    if let Some(name) = note_release(obj.id()) {
        // The last hold on a file whose last name went while it was held:
        // the file is gone now. Its cached pages go, since its inode number
        // may be given to another file, and so does the state kept about it,
        // deferred at the unlink (`defer_forget_if_held`).
        crate::mm::page_cache::invalidate_identity(obj.fs_id, obj.ino);
        super::perfile::object_unlinked(
            super::perfile::Unlinked {
                id: Some(obj.id()),
                last_name: true,
            },
            &name,
        );
    }
    release_mount_hold(obj.fs_id);
}

/// Give back one hold on mount `fs_id` (`MountPoint::objects`).
fn release_mount_hold(fs_id: u64) {
    let mut vfs = VFS.lock();
    if let Some(mp) = vfs.mounts.iter_mut().find(|m| m.fs_id == fs_id) {
        mp.objects = mp.objects.saturating_sub(1);
    }
}

/// `ReadOnlyFilesystem` if mount `fs_id` is read-only now, `NotFound` if it
/// has gone: what can change under a file opened for writing.
fn check_writable_fs(fs_id: u64) -> KernelResult<()> {
    let vfs = VFS.lock();
    let mp = vfs
        .mounts
        .iter()
        .find(|m| m.fs_id == fs_id)
        .ok_or(KernelError::NotFound)?;
    if mp.options.read_only {
        Err(KernelError::ReadOnlyFilesystem)
    } else {
        Ok(())
    }
}

/// The files held open, by identity: how many holds each has, and -- once
/// its last name has gone while held -- the name its per-file state was kept
/// under (`super::perfile`).
///
/// That state (an ACL, flags, seals, attributes) ends with the file, which
/// for a held file is its last release, not the unlink: a write-sealed file
/// unlinked while open must stay sealed to the handle still writing it.
/// `perfile::object_unlinked` asks [`defer_forget_if_held`]; the last
/// [`FileHold`] to go then ends it, and drops the file's cached pages, since
/// its inode number may be given to another file. A leaf lock.
struct Held {
    holds: usize,
    unlinked_as: Option<PathBuf>,
    /// Made with no name to be given one (`O_TMPFILE` without `O_EXCL`):
    /// Linux's `I_LINKABLE`. Cleared when it is named.
    linkable: bool,
}

static HELD: Mutex<alloc::collections::BTreeMap<FileId, Held>> =
    Mutex::new(alloc::collections::BTreeMap::new());

/// One more hold on `id`.
fn note_hold(id: FileId) {
    let mut held = HELD.lock();
    let entry = held.entry(id).or_insert(Held {
        holds: 0,
        unlinked_as: None,
        linkable: false,
    });
    entry.holds = entry.holds.saturating_add(1);
}

/// One hold on `id` gone. When it was the last, and the file's last name had
/// gone while it was held, the name its state was kept under: the file is
/// gone now, and its state goes with it.
fn note_release(id: FileId) -> Option<PathBuf> {
    let mut held = HELD.lock();
    let entry = held.get_mut(&id)?;
    entry.holds = entry.holds.saturating_sub(1);
    if entry.holds == 0 {
        held.remove(&id).and_then(|h| h.unlinked_as)
    } else {
        None
    }
}

/// A file made with no name (`Vfs::create_unnamed_object`): its one hold,
/// the name it is shown under, and whether it may be given a real one.
fn note_unnamed(id: FileId, shown_as: PathBuf, linkable: bool) {
    HELD.lock().insert(
        id,
        Held {
            holds: 1,
            unlinked_as: Some(shown_as),
            linkable,
        },
    );
}

/// Held file `id` has been given a name (`Vfs::link_object`): it no longer
/// goes with its last hold.
fn note_named(id: FileId) {
    if let Some(entry) = HELD.lock().get_mut(&id) {
        entry.unlinked_as = None;
        entry.linkable = false;
    }
}

/// Whether held file `id` may be given a name: it has one already (and gets
/// another, as `link` gives one), or it was made with none to be given one.
/// A file deleted while open may not, as on Linux.
fn may_link(id: FileId) -> bool {
    HELD.lock()
        .get(&id)
        .is_some_and(|h| h.unlinked_as.is_none() || h.linkable)
}

/// `id`'s last name, `path`, has gone. If the file is held open it lives
/// on, unnamed: record that, and answer `true`, so its per-file state is
/// kept until the last release instead of ended now.
pub(crate) fn defer_forget_if_held(id: FileId, path: &Path) -> bool {
    let mut held = HELD.lock();
    match held.get_mut(&id) {
        Some(entry) => {
            entry.unlinked_as = Some(path.to_path_buf());
            true
        }
        None => false,
    }
}

/// Monotonic source of stable mount ids ([`MountPoint::fs_id`]).
///
/// Starts at 1 so `0` can mean "no/unknown filesystem".  Never decrements and
/// ids are never reused, so a `FileId` minted for one mount can never collide
/// with a later mount even after the original is unmounted.
static NEXT_FS_ID: AtomicU64 = AtomicU64::new(1);

/// Device numbers of the mounted filesystems: `fs_id` to the number `stat`
/// reports as `st_dev`'s minor, under major 0, as Linux numbers its
/// anonymous filesystems.
///
/// Not `fs_id` itself. That is never reused, because it keys the page cache
/// and the lock tables, and a reused one would alias a dead mount's entries;
/// so it only grows. A device number is reused once its mount is gone, as
/// Linux reuses an anonymous device's minor, so it stays as small as the
/// number of filesystems mounted at once and fits the native stat record's
/// 24 bits.
///
/// A leaf lock: nothing is taken under it, so `/proc` may read it under its
/// own filesystem lock.
static MOUNT_DEVS: Mutex<alloc::collections::BTreeMap<u64, u32>> =
    Mutex::new(alloc::collections::BTreeMap::new());

/// Give mount `fs_id` the smallest device number no mounted filesystem has,
/// from 1.
fn assign_dev(fs_id: u64) {
    let mut devs = MOUNT_DEVS.lock();
    let mut used: Vec<u32> = devs.values().copied().collect();
    used.sort_unstable();
    let mut dev: u32 = 1;
    for u in used {
        if u == dev {
            dev = dev.saturating_add(1);
        } else if u > dev {
            break;
        }
    }
    devs.insert(fs_id, dev);
}

/// Mount `fs_id` is gone: its device number is free again.
fn release_dev(fs_id: u64) {
    MOUNT_DEVS.lock().remove(&fs_id);
}

/// The device number (`st_dev`'s minor) of mounted filesystem `fs_id`, or 0
/// for one that is not mounted.
#[must_use]
pub fn dev_of(fs_id: u64) -> u32 {
    MOUNT_DEVS.lock().get(&fs_id).copied().unwrap_or(0)
}

/// Linux's `dev_t` for device number `dev` under major 0, as glibc's
/// `makedev(0, dev)` builds it: the minor's low byte in bits 0-7 and the rest
/// from bit 20. `major()` and `minor()` take it apart again.
#[must_use]
pub fn linux_dev_t(dev: u32) -> u64 {
    u64::from(dev & 0xff) | (u64::from(dev & 0xffff_ff00) << 12)
}

/// A system-wide-unique identity for a filesystem object.
///
/// A file is uniquely identified by the pair `(fs_id, ino)`: the stable mount
/// id ([`MountPoint::fs_id`]) plus the filesystem-local inode number
/// ([`FileMeta::ino`]).  Two paths that resolve to the same `(fs_id, ino)` are
/// the same underlying object (e.g. hard links on ext4); two objects on
/// different mounts that happen to share an `ino` are distinguished by `fs_id`.
///
/// **Unique among live objects only.** `fs_id` is never reused, but an inode
/// number is, once its object is gone: ext4 hands a freed inode to the next
/// file it creates, so a `FileId` read before a deletion can name a different
/// file after it. (memfs counts up and never reuses one, which is why this
/// went unnoticed on `/tmp`.) State keyed on a `FileId` that can outlive its
/// file must therefore end with the file: the page cache is invalidated on
/// every removal (`invalidate_identity`), and the tables of per-file state --
/// ACLs, flags, seals, indexed attributes -- are told through
/// [`super::perfile`], where any new such table must be registered.
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

/// A mounted filesystem, as [`Vfs::volume_of`] reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeInfo {
    /// Where it is mounted.
    pub mount: PathBuf,
    /// Its type name (`"ext4"`, `"memfs"`, ...).
    pub fs_type: String,
    /// The mount's never-reused id: the `fs_id` half of every [`FileId`] on it.
    pub fs_id: u64,
    /// Whether it is mounted read-only.
    pub read_only: bool,
    /// The volume's own UUID ([`FileSystem::volume_uuid`]), if it has one.
    pub uuid: Option<[u8; 16]>,
}

/// What a deferred delete or rename would act on ([`Vfs::deferral_target`]).
#[derive(Debug, Clone)]
pub struct DeferralTarget {
    /// The name, as the host spells it (no namespace left to apply).
    pub path: PathBuf,
    /// The directory holding the name.
    pub parent: PathBuf,
    /// The file the name leads to now, the final component not followed.
    pub id: FileId,
    /// What kind of file it is.
    pub entry_type: EntryType,
    /// Its `chattr` marks.
    pub attributes: FileAttr,
    /// Its directory's `chattr` marks.
    pub parent_attributes: FileAttr,
    /// The filesystem it is on.
    pub volume: VolumeInfo,
}

/// Which of the three renames a caller is asking for.
///
/// The VFS owns this rather than the syscall layer because it names a
/// *filesystem semantic*, not an encoding: the path route
/// ([`Vfs::rename`], [`Vfs::rename_noreplace`], [`Vfs::rename_exchange`])
/// and the handle route ([`Vfs::rename_at_pinned`]) offer the same three,
/// and one type for them is what stops the two routes drifting into
/// offering different sets.
///
/// The *decode* from a flags word lives with the ABI, in
/// `crate::syscall::handlers` — the bit values are Linux's and are a
/// statement about the syscall interface, not about the filesystem.  The
/// split is deliberate: the semantics belong here, the encoding belongs
/// there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameMode {
    /// Replace the destination if it exists.
    Replace,
    /// `RENAME_NOREPLACE` — refuse if the destination exists.
    NoReplace,
    /// `RENAME_EXCHANGE` — swap the two entries atomically.
    Exchange,
}

/// A directory handle's identity, captured when the handle was opened.
///
/// `path` is what the handle was opened **as**; `id` is what it opened
/// **onto**.  Carrying both is the entire point.  A path alone can be
/// re-pointed at a different object between the open and the use — that is
/// the defect lane B reported in `requests/b-a-the-at-family-resolves-by-\
/// path-so-no-toctou-fix-is-possible.md`, where every `*at` syscall recovers
/// the dirfd's *name* and concatenates.  An id alone is useless here, because
/// every method on [`FileSystem`] takes a name and none takes an inode.
///
/// Holding the pair lets an fd-relative operation walk the name and then
/// *check* that the walk arrived where the handle was opened, refusing with
/// [`KernelError::StaleHandle`] when it did not.
#[derive(Debug, Clone)]
pub struct PinnedDir {
    /// Absolute, already-resolved VFS path the handle was opened under.
    pub path: PathBuf,
    /// Identity of the directory at open time, or `None` when the filesystem
    /// has no stable per-object identity to pin (`ino == 0` — FAT, ISO9660,
    /// and the synthetic trees).
    ///
    /// `None` means *unverifiable*, and is never quietly read as *verified*.
    /// Callers that need the guarantee ask
    /// [`Vfs::pinned_dir_is_verifiable`] and refuse; callers that only want
    /// the containment (a single component, no `..`, no `/`) proceed knowing
    /// what they did not get.
    pub id: Option<FileId>,
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
    /// Who holds it: a process ([`flock_process_owner`]) or an open file
    /// description ([`flock_description_owner`]).
    owner: u64,
    /// Lock type.
    lock_type: LockType,
    /// The process that took it, for `/proc/locks` (0 for a kernel task).
    /// For a description's lock this is who asked; the description holds it.
    pid: u64,
}

/// Per-path lock table entry.
#[derive(Debug, Clone)]
struct PathLockEntry {
    /// Canonical path (after symlink resolution).
    ///
    /// Retained for `/proc` display and as the fallback key on filesystems
    /// with no stable inode. It is no longer the primary key: two names for
    /// one file used to get two entries and two exclusive locks.
    path: PathBuf,
    /// Filesystem identity, when the filesystem provides one.
    ///
    /// This is the real key. `None` means the filesystem reports `ino == 0`
    /// -- devfs, procfs, sysfs -- and those cannot have two names for one
    /// object, so falling back to the path there is exact rather than
    /// approximate. See known-issues 2026-09-21 for the per-filesystem
    /// measurement.
    id: Option<FileId>,
    /// Active locks on this path.
    locks: Vec<FileLock>,
}

/// Does this entry describe the same file as `(path, id)`?
///
/// Identity wins when both sides have one; otherwise the resolved path is
/// the key. A free function rather than three inline comparisons because
/// three sites comparing by hand is how one of them ends up different.
fn lock_entry_matches(e: &PathLockEntry, path: &Path, id: Option<FileId>) -> bool {
    match (e.id, id) {
        (Some(a), Some(b)) => a == b,
        _ => e.path.as_path() == path,
    }
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

/// Tag bit marking a `flock` owner that is a **process**, as against an open
/// file description.
///
/// Locks taken through a handle -- Linux `flock(2)`, the native
/// `SYS_FS_FLOCK_HANDLE` -- belong to the open file description, as BSD and
/// Linux `flock` locks do. The native path-based `SYS_FS_FLOCK` takes them
/// for the calling process. Pids and handles are both small counters from 1,
/// and until 2026-10-01 they were one owner space: pid 57's lock and handle
/// 57's never conflicted, and process 57's exit released handle 57's locks.
const FLOCK_PROCESS_TAG: u64 = 1 << 63;

/// The `flock` owner for a process: the native path-based `SYS_FS_FLOCK`.
/// Released when the process exits.
#[must_use]
pub fn flock_process_owner(pid: u64) -> u64 {
    pid | FLOCK_PROCESS_TAG
}

/// The `flock` owner for an open file description, by handle: Linux
/// `flock(2)` and the native `SYS_FS_FLOCK_HANDLE`. Released at the
/// description's final close (`fs::handle::close`).
#[must_use]
pub fn flock_description_owner(handle: u64) -> u64 {
    handle & !FLOCK_PROCESS_TAG
}

/// A task parked in [`Vfs::flock_wait_resolved`], and the file it waits on.
struct FlockWaiter {
    task: crate::sched::task::TaskId,
    path: PathBuf,
    id: Option<FileId>,
}

/// Tasks waiting for a `flock`. A list of its own, never held with
/// `LOCK_TABLE`: a waiter registers here before each attempt (see
/// `flock_wait_resolved`), which keeps a release between its attempt and its
/// park from being lost.
static FLOCK_WAITERS: Mutex<Vec<FlockWaiter>> = Mutex::new(Vec::new());

/// Tasks that may wait for a `flock` at once. Each is a parked task; the
/// bound is on heap use.
const MAX_FLOCK_WAITERS: usize = 1024;

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

/// What Linux's `inode_permission` asks of the file for an xattr access, as
/// the gate's [`PathAccess`]: to read the value, or to change it.
fn path_access_for(access: xattr_policy::Access) -> PathAccess {
    match access {
        xattr_policy::Access::Read => PathAccess::Read,
        xattr_policy::Access::Write => PathAccess::Write,
    }
}

/// The metadata `fs::xattr_policy` decides an xattr call on: the file a
/// trailing link names when `follow`, the link itself when not.
fn xattr_meta(fs: &mut dyn FileSystem, relative: &Path, follow: bool) -> KernelResult<FileMeta> {
    if follow {
        fs.metadata(relative)
    } else {
        fs.lmetadata(relative)
    }
}

// ---------------------------------------------------------------------------
// The ACL door: `system.posix_acl_access` (`fs::acl`)
// ---------------------------------------------------------------------------
//
// Linux keeps a file's ACL in that extended attribute, and `getfacl`/`setfacl`
// reach it there. Here the ACL the kernel enforces is `fs::acl`'s table, so the
// VFS answers the name from the table, not the filesystem: what is set through
// it is what refuses. Under the filesystem's lock, by the file's identity, as
// every other per-file rule is. A directory's default ACL
// (`system.posix_acl_default`) is answered the same way, from `fs::acl`'s
// table of them, which the VFS reads when it makes a file (`new_node_mode`).

/// What the ACL door knows of a file under the filesystem's lock: its
/// metadata, and the identity and name the ACL table keys it by.
struct AclDoorFile<'a> {
    meta: FileMeta,
    id: Option<FileId>,
    name: &'a Path,
}

impl<'a> AclDoorFile<'a> {
    fn of(meta: FileMeta, fs_id: u64, name: &'a Path) -> Self {
        let id = (meta.ino != 0).then_some(FileId {
            fs_id,
            ino: meta.ino,
        });
        Self { meta, id, name }
    }

    fn held(meta: FileMeta, obj: &FileObject, name: &'a Path) -> Self {
        Self {
            meta,
            id: Some(obj.id()),
            name,
        }
    }
}

/// `getxattr`: the ACL in Linux's layout; `NoAttribute` (`ENODATA`) for a file
/// with none, as Linux answers; `NotSupported` for a symlink, which has none.
fn acl_door_get(subject: &AclDoorFile<'_>) -> KernelResult<Vec<u8>> {
    if subject.meta.entry_type == EntryType::Symlink {
        return Err(KernelError::NotSupported);
    }
    super::acl::get_acl_for(subject.id, subject.name)
        .map(|acl| super::acl::encode_xattr(&acl))
        .ok_or(KernelError::NoAttribute)
}

/// Who may change a file's ACL: no one, on an immutable or append-only file
/// (`EPERM`); otherwise its owner or root -- Linux's `inode_owner_or_capable`.
fn acl_door_may_change(
    subject: &AclDoorFile<'_>,
    caller: xattr_policy::Caller,
) -> KernelResult<()> {
    if subject.meta.entry_type == EntryType::Symlink {
        return Err(KernelError::NotSupported);
    }
    xattr_policy::namespace_rules(
        xattr_policy::Namespace::System,
        xattr_policy::Access::Write,
        &subject.meta,
        caller,
    )?;
    if caller.privileged || caller.uid == Some(subject.meta.uid) {
        Ok(())
    } else {
        Err(KernelError::NotPermitted)
    }
}

/// `setxattr`, in Linux's order (`do_set_acl`): the value parsed
/// (`InvalidArgument`); who may ([`acl_door_may_change`]); the mode
/// (`XATTR_CREATE`, `XATTR_REPLACE`). The file's mode becomes the one the ACL
/// means (`posix_acl_update_mode`), through `chmod`; and an ACL the mode says
/// all of is not kept, only the mode, as Linux keeps none then.
fn acl_door_set(
    subject: &AclDoorFile<'_>,
    value: &[u8],
    mode: XattrSetMode,
    caller: xattr_policy::Caller,
    chmod: impl FnOnce(u16) -> KernelResult<()>,
) -> KernelResult<()> {
    let acl = super::acl::decode_xattr(value)?;
    acl_door_may_change(subject, caller)?;
    mode.check(acl_door_get(subject))?;
    let new_mode = super::acl::mode_of(&acl, subject.meta.permissions);
    if new_mode != subject.meta.permissions {
        chmod(new_mode)?;
    }
    if super::acl::is_minimal(&acl) {
        super::acl::remove_acl_for(subject.id, subject.name);
        Ok(())
    } else {
        super::acl::set_acl_for(subject.id, subject.name, acl)
    }
}

/// `removexattr`: who may set an ACL may remove it; `NoAttribute` for a file
/// with none. The mode stays as it is, as Linux leaves it.
fn acl_door_remove(subject: &AclDoorFile<'_>, caller: xattr_policy::Caller) -> KernelResult<()> {
    acl_door_may_change(subject, caller)?;
    if super::acl::remove_acl_for(subject.id, subject.name) {
        Ok(())
    } else {
        Err(KernelError::NoAttribute)
    }
}

/// `listxattr`'s part: the ACL's name, when the file has one, and a
/// directory's default ACL's, when it has one.
fn acl_door_listed(subject: &AclDoorFile<'_>, names: &mut Vec<Vec<u8>>) {
    if super::acl::get_acl_for(subject.id, subject.name).is_some() {
        names.push(super::acl::XATTR_ACCESS.to_vec());
    }
    if subject.meta.entry_type == EntryType::Directory
        && super::acl::get_default_for(subject.id, subject.name).is_some()
    {
        names.push(super::acl::XATTR_DEFAULT.to_vec());
    }
}

/// `getxattr` of `system.posix_acl_default`: a directory's default ACL in
/// Linux's layout. `NoAttribute` (`ENODATA`) for a directory with none and for
/// anything that is not a directory -- only a directory has one, and Linux
/// answers so; `NotSupported` for a symlink, as for the access ACL.
fn acl_default_get(subject: &AclDoorFile<'_>) -> KernelResult<Vec<u8>> {
    if subject.meta.entry_type == EntryType::Symlink {
        return Err(KernelError::NotSupported);
    }
    if subject.meta.entry_type != EntryType::Directory {
        return Err(KernelError::NoAttribute);
    }
    super::acl::get_default_for(subject.id, subject.name)
        .map(|acl| super::acl::encode_xattr(&acl))
        .ok_or(KernelError::NoAttribute)
}

/// `setxattr` of `system.posix_acl_default`, in Linux's order: the value
/// parsed (`InvalidArgument`); who may ([`acl_door_may_change`]); only a
/// directory has one -- anything else `PermissionDenied` (`EACCES`), Linux's
/// answer; the mode (`XATTR_CREATE`, `XATTR_REPLACE`). Kept as given: a
/// default ACL the mode bits could say is still a default ACL, and the
/// directory's own mode is not touched.
fn acl_default_set(
    subject: &AclDoorFile<'_>,
    value: &[u8],
    mode: XattrSetMode,
    caller: xattr_policy::Caller,
) -> KernelResult<()> {
    let acl = super::acl::decode_xattr(value)?;
    acl_door_may_change(subject, caller)?;
    if subject.meta.entry_type != EntryType::Directory {
        return Err(KernelError::PermissionDenied);
    }
    mode.check(acl_default_get(subject))?;
    super::acl::set_default_for(subject.id, subject.name, acl)
}

/// `removexattr` of `system.posix_acl_default`: who may set one may remove
/// it. On anything but a directory there is none to remove, and Linux answers
/// success; on a directory with none, `NoAttribute`.
fn acl_default_remove(subject: &AclDoorFile<'_>, caller: xattr_policy::Caller) -> KernelResult<()> {
    acl_door_may_change(subject, caller)?;
    if subject.meta.entry_type != EntryType::Directory {
        return Ok(());
    }
    if super::acl::remove_default_for(subject.id, subject.name) {
        Ok(())
    } else {
        Err(KernelError::NoAttribute)
    }
}

/// After a `chmod` of a file with an ACL: the mode written into the ACL, as
/// Linux's `posix_acl_chmod` writes it, so the permission check and `getfacl`
/// both see the new mode. Nothing is looked up while no file anywhere has an
/// ACL, which is almost always.
fn acl_follow_chmod(id: Option<FileId>, name: &Path, permissions: u16) {
    if super::acl::count() == 0 {
        return;
    }
    if let Some(acl) = super::acl::get_acl_for(id, name) {
        // Discarded deliberately: `with_mode` changes no entry's tag, so an
        // ACL that was valid when stored is valid now, and `set_acl_for` can
        // refuse only an invalid one.
        let _ = super::acl::set_acl_for(id, name, super::acl::with_mode(&acl, permissions));
    }
}

/// [`acl_follow_chmod`] for what `relative` names on `fs`.
fn acl_follow_chmod_at(
    fs: &mut dyn FileSystem,
    fs_id: u64,
    relative: &Path,
    name: &Path,
    follow: bool,
    permissions: u16,
) {
    if super::acl::count() == 0 {
        return;
    }
    // Discarded deliberately: the chmod has just succeeded on this name under
    // this lock, so it resolves; a filesystem that cannot answer has no
    // identity to key an ACL by, and the name fallback below still applies.
    let ino = xattr_meta(fs, relative, follow).map_or(0, |m| m.ino);
    let id = (ino != 0).then_some(FileId { fs_id, ino });
    acl_follow_chmod(id, name, permissions);
}

/// What [`Vfs::set_xattr_with`] does when the attribute already exists.
///
/// The kernel takes this rather than leaving userspace to probe first
/// because the probe cannot be made atomic from outside: see
/// [`Vfs::set_xattr_with`] and `design-decisions.md` §661.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XattrSetMode {
    /// Create the attribute or overwrite it — plain `setxattr`.
    Any,
    /// Fail with [`KernelError::AlreadyExists`] (`EEXIST`) if the attribute
    /// is already present — `XATTR_CREATE`.
    Create,
    /// Fail with [`KernelError::NoAttribute`] (`ENODATA`) if the attribute
    /// is not already present — `XATTR_REPLACE`.
    Replace,
    /// Both `XATTR_CREATE` and `XATTR_REPLACE`, which Linux's filesystems
    /// refuse either way: `AlreadyExists` when the attribute is present,
    /// `NoAttribute` when it is not. Only the Linux layer asks it; the native
    /// ABI refuses the two bits together as `InvalidArgument`.
    Neither,
}

/// The file an xattr call names, found and checked as far as Linux goes
/// before it reads the attribute's name ([`Vfs::xattr_target`]): the path;
/// for a change, the mount's writability; the capability tags.
///
/// The Linux layer reads the name between the two steps, so its refusals come
/// in Linux's order: `getxattr`, `listxattr` and `removexattr` look up the path
/// before they read the name, `setxattr` after.
#[derive(Debug, Clone)]
pub struct XattrTarget {
    /// The resolved host path.
    path: PathBuf,
    /// Whether a trailing link was followed; when not, the target is the
    /// link.
    follow: bool,
    /// What it was found for: a change has been checked against the mount.
    access: xattr_policy::Access,
}

impl XattrSetMode {
    /// Decide whether a set may proceed, given the result of probing for
    /// the attribute.
    ///
    /// Only [`KernelError::NoAttribute`] is read as absence.  Any other
    /// error means the filesystem could not *answer* — most importantly
    /// `NotFound`, which is about the path and not the attribute — and is
    /// propagated rather than quietly treated as "not there", which is
    /// exactly the mistake the userspace probe made: it read every
    /// negative return as absence, so `XATTR_REPLACE` on a nonexistent
    /// file reported `ENODATA` instead of `ENOENT`.
    fn check(self, probe: KernelResult<Vec<u8>>) -> KernelResult<()> {
        let present = match probe {
            Ok(_) => true,
            Err(KernelError::NoAttribute) => false,
            Err(e) => return Err(e),
        };
        match (self, present) {
            (Self::Any, _) | (Self::Create, false) | (Self::Replace, true) => Ok(()),
            (Self::Create | Self::Neither, true) => Err(KernelError::AlreadyExists),
            (Self::Replace | Self::Neither, false) => Err(KernelError::NoAttribute),
        }
    }
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
        // Stable, never-reused id for this mount instance (see FileId), and
        // the device number `stat` reports for its files: assigned before
        // the mount is visible, so no `stat` of it can see 0.
        let fs_id = NEXT_FS_ID.fetch_add(1, Ordering::Relaxed);
        assign_dev(fs_id);
        vfs.mounts.push(MountPoint {
            path: mount_path.to_path_buf(),
            fs: Arc::new(Mutex::new(fs)),
            fs_type,
            options,
            fs_id,
            objects: 0,
        });

        // Mount changes affect path resolution — invalidate entire dcache.
        let mounted_path = mount_path.to_path_buf();
        drop(vfs);
        VFS_DCACHE.lock().invalidate_all();

        // Replay any deferred filesystem operations that were queued while
        // this volume was absent, busy, or read-only.  Best-effort: errors
        // are logged but do not fail the mount.
        super::deferred_ops::replay_on_mount(&mounted_path);

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
        // A file held open on it keeps it, as Linux's `umount` answers EBUSY.
        // Checked under the lock the holds are counted under, so no open can
        // slip in between this and the removal.
        if vfs.mounts.get(idx).is_some_and(|mp| mp.objects > 0) {
            return Err(KernelError::DeviceBusy);
        }

        vfs.mounts.remove(idx);
        release_dev(fs_id);
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
        LOCK_TABLE
            .lock()
            .retain(|entry| !crate::fs::pathutil::path_in_subtree(&entry.path, mount_path));
        // Whoever waited for one of those locks tries again, and finds the
        // file gone or free. After the table lock: the two are never held
        // together.
        wake_all_flock_waiters();

        // And the state kept about its files outside it: no identity on this
        // filesystem can match again (`super::perfile`). After the lock above
        // is released, so that it orders against nothing.
        super::perfile::filesystem_unmounted(fs_id);

        Ok(())
    }

    /// Make the filesystem mounted at `new_root` the root, and put the one
    /// that was the root at `put_old` -- Linux's `pivot_root(2)`, for the
    /// boot's switch to the system image (design-decisions §1513).
    ///
    /// - The mount at `new_root` becomes `/`, and every mount beneath it moves
    ///   with it: `new_root/x` becomes `/x`.
    /// - The old root's mount moves to `put_old`, which must be a direct child
    ///   of `/` (so it is reachable whatever the new root holds) and not
    ///   already a mount point.
    /// - Every other mount -- `/tmp`, `/proc`, `/dev`, `/sys` -- keeps its
    ///   path, and from now on sits over the new root.
    /// - Advisory locks move with their files: a lock taken by path on
    ///   `new_root/f` is a lock on `/f`.
    ///
    /// A mount keeps its identity (`fs_id`), so a file held open on either
    /// filesystem stays open and the same file. Records kept elsewhere by
    /// *path* -- a process's working directory, a watch -- are not
    /// rewritten: this is for the boot, before any process that keeps one
    /// runs. The caller unmounts `put_old` afterwards if it has no further
    /// use for it; that refuses while a file there is held, as any unmount
    /// does.
    ///
    /// # Errors
    ///
    /// Nothing changes on an error.
    /// - `InvalidArgument`: a path that is not absolute or has `.`/`..`;
    ///   `new_root` is `/`; `put_old` is not a direct child of `/`, or is
    ///   `new_root`.
    /// - `NotFound`: nothing is mounted at `new_root` (or at `/`).
    /// - `AlreadyExists`: something is mounted at `put_old`, or a mount moving
    ///   with `new_root` would land on a path another mount has.
    pub fn pivot_root(new_root: impl AsRef<Path>, put_old: impl AsRef<Path>) -> KernelResult<()> {
        Self::pivot_mounts(Path::new("/"), new_root.as_ref(), put_old.as_ref())
    }

    /// [`Self::pivot_root`] about the mount at `root` rather than `/`: the
    /// mount at `new_root`, strictly under `root`, takes `root`'s place with
    /// everything beneath it, and the mount that was at `root` moves to
    /// `put_old`, a direct child of `root`. Every rule is the root's with
    /// `root` for `/`; separate only so the self-test can pivot a tree that
    /// is not the one the system is running on.
    fn pivot_mounts(root: &Path, new_root: &Path, put_old: &Path) -> KernelResult<()> {
        for p in [root, new_root, put_old] {
            if !p.is_absolute() || !p.has_no_dot_components() {
                return Err(KernelError::InvalidArgument);
            }
        }
        let root = normalize_mount_path(root);
        let new_root = normalize_mount_path(new_root);
        let put_old = normalize_mount_path(put_old);
        if !crate::fs::pathutil::path_strictly_under(&new_root, &root)
            || put_old.as_path().parent() != Some(root.as_path())
            || put_old == new_root
        {
            return Err(KernelError::InvalidArgument);
        }

        // Every path is planned and checked before any is changed, under one
        // hold of the table, so a refusal leaves the table as it was and no
        // lookup ever sees half a pivot.
        let (moves, before) = {
            let mut vfs = VFS.lock();
            let old_idx = vfs
                .mounts
                .iter()
                .position(|m| m.path == root)
                .ok_or(KernelError::NotFound)?;
            let new_idx = vfs
                .mounts
                .iter()
                .position(|m| m.path == new_root)
                .ok_or(KernelError::NotFound)?;
            if vfs.mounts.iter().any(|m| m.path == put_old) {
                return Err(KernelError::AlreadyExists);
            }
            let mut plan: Vec<(usize, PathBuf)> = Vec::new();
            plan.push((old_idx, put_old.clone()));
            for (i, m) in vfs.mounts.iter().enumerate() {
                if i == new_idx || crate::fs::pathutil::path_strictly_under(&m.path, &new_root) {
                    let to = rebase_under(&m.path, &new_root, &root)
                        .ok_or(KernelError::InvalidArgument)?;
                    plan.push((i, to));
                }
            }
            let planned = |j: usize| plan.iter().any(|&(i, _)| i == j);
            for (_, to) in &plan {
                let clash = vfs
                    .mounts
                    .iter()
                    .enumerate()
                    .any(|(j, m)| !planned(j) && m.path == *to);
                if clash {
                    return Err(KernelError::AlreadyExists);
                }
            }
            let before: Vec<PathBuf> = vfs.mounts.iter().map(|m| m.path.clone()).collect();
            let mut moves: Vec<(PathBuf, PathBuf)> = Vec::new();
            for (i, to) in plan {
                if let Some(m) = vfs.mounts.get_mut(i) {
                    moves.push((core::mem::replace(&mut m.path, to.clone()), to));
                }
            }
            (moves, before)
        };
        // Paths now resolve differently: drop every cached lookup.
        VFS_DCACHE.lock().invalidate_all();

        // Advisory locks taken by path move with their files. A lock belongs
        // to the mount that answered its path before the pivot -- the longest
        // mount path containing it -- and moves only if that mount did.
        // After the table lock: the two are never held together.
        {
            let mut locks = LOCK_TABLE.lock();
            for entry in locks.iter_mut() {
                let owner = before
                    .iter()
                    .filter(|m| crate::fs::pathutil::path_in_subtree(&entry.path, m))
                    .max_by_key(|m| m.as_path().components().count());
                let moved = owner.and_then(|o| moves.iter().find(|(from, _)| from == o));
                if let Some((from, to)) = moved {
                    if let Some(new_path) = rebase_under(&entry.path, from, to) {
                        entry.path = new_path;
                    }
                }
            }
        }

        for (from, to) in &moves {
            crate::serial_println!("[vfs] Pivot: '{}' -> '{}'", from.display(), to.display());
        }
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

        // Check VFS dcache first -- avoids component-by-component lstat walk.
        {
            let mut dcache = VFS_DCACHE.lock();
            match dcache.lookup(&norm, true) {
                DcacheLookup::Hit(resolved) => return Ok(resolved),
                DcacheLookup::NegativeHit => return Err(KernelError::NotFound),
                DcacheLookup::Miss => {}
            }
        }

        // A walk through a filesystem whose answers depend on the caller or
        // on time (`FileSystem::dcache_safe`) is not kept, either way: until
        // 2026-10-03 `/etc/mtab`, a link to `/proc/self/mounts`, was cached
        // as whichever caller resolved it first had it, and every later
        // caller read that answer (rq42 and rq43's `/etc/mtab` self-test).
        let mut uncacheable = false;
        match Self::resolve_inner(&norm, true, 0, false, None, &mut uncacheable) {
            Ok(resolved) => {
                // Cache the positive result for future lookups.
                if !uncacheable {
                    let mut dcache = VFS_DCACHE.lock();
                    dcache.insert(&norm, true, &resolved);
                }
                Ok(resolved)
            }
            Err(KernelError::NotFound) => {
                // Cache the negative result -- this path's parent chain is
                // broken (a non-final component doesn't exist) -- so future
                // lookups can short-circuit without walking the filesystem.
                if !uncacheable {
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

        // Check VFS dcache first -- avoids component-by-component lstat walk.
        {
            let mut dcache = VFS_DCACHE.lock();
            match dcache.lookup(&norm, false) {
                DcacheLookup::Hit(resolved) => return Ok(resolved),
                DcacheLookup::NegativeHit => return Err(KernelError::NotFound),
                DcacheLookup::Miss => {}
            }
        }

        // As `resolve_follow`: a walk through procfs is not kept.
        let mut uncacheable = false;
        match Self::resolve_inner(&norm, false, 0, false, None, &mut uncacheable) {
            Ok(resolved) => {
                // Cache the positive result for future lookups.
                if !uncacheable {
                    let mut dcache = VFS_DCACHE.lock();
                    dcache.insert(&norm, false, &resolved);
                }
                Ok(resolved)
            }
            Err(KernelError::NotFound) => {
                // Cache the negative result -- this path's parent chain is
                // broken (a non-final component doesn't exist) -- so future
                // lookups can short-circuit without walking the filesystem.
                if !uncacheable {
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
        // Not cached, so whether the walk could have been does not matter.
        Self::resolve_inner(&norm, true, 0, true, None, &mut false)
    }

    /// Resolve `rel` relative to `base`, refusing any escape from `base`.
    ///
    /// Implements `openat2`'s `RESOLVE_BENEATH`: the walk may not leave the
    /// directory `base` names, whether by a `..` in `rel`, by an absolute
    /// `rel`, or by a symlink met along the way whose target points out (or
    /// is absolute, or leaves and returns).  Every such attempt fails with
    /// [`KernelError::CrossDevice`] (→ `EXDEV`), which is the error Linux
    /// documents for a walk that would cross a forbidden boundary.
    ///
    /// `base` is resolved normally first — the containment is on the walk
    /// `rel` performs, not on how the caller reached `base`.  That matches
    /// Linux, where `dirfd` is an already-open directory and no restriction
    /// is retroactively applied to the path that opened it.
    ///
    /// `rel` must be relative; an absolute `rel` is refused rather than
    /// silently reinterpreted, because a caller who asked for containment
    /// and passed an absolute path has contradicted itself, and guessing
    /// which half it meant is precisely the mistake this flag exists to
    /// prevent.  (`RESOLVE_IN_ROOT`, which re-roots absolute paths onto the
    /// base instead of refusing them, is deliberately not implemented: lane
    /// B has no consumer for it and an unused ABI is a commitment.)
    ///
    /// `follow_last` and `no_symlinks` behave as they do elsewhere, so
    /// `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS` composes without special
    /// handling.
    pub fn resolve_beneath(
        base: impl AsRef<Path>,
        rel: impl AsRef<Path>,
        follow_last: bool,
        no_symlinks: bool,
    ) -> KernelResult<PathBuf> {
        let rel = rel.as_ref();

        // Step 1: the caller-supplied fragment, checked before anything
        // normalizes it.  This is the only place `rel`'s own `..` are
        // visible -- `normalize_path` below collapses them, and after that
        // `../base/sub` is indistinguishable from `sub`.
        Self::beneath_fragment_ok(rel)?;

        // Step 2: canonicalise the base.  Symlinks *in the base* are
        // followed normally (the caller reached it before asking for
        // containment), and the canonical form is what the per-hop checks
        // measure depth against, so it must be settled before the walk.
        let base = Self::resolve_follow(base.as_ref())?;

        // Step 3: walk base + rel with the base threaded through, so each
        // symlink target met on the way is judged where it is read.
        let mut full = base.clone();
        full.push(rel);
        let norm = normalize_path(&full);
        validate_path(&norm)?;

        // Step 1 guarantees this, but assert it rather than assume it: the
        // whole value of the flag is that the answer is inside the base, and
        // a future edit to the joining above must not be able to break that
        // silently.  Cheap, and the only check whose failure would mean the
        // reasoning above is wrong rather than the caller.
        if !norm.starts_with(&base) {
            return Err(KernelError::CrossDevice);
        }

        // Not cached, so whether the walk could have been does not matter.
        Self::resolve_inner(&norm, follow_last, 0, no_symlinks, Some(&base), &mut false)
    }

    /// The half of `RESOLVE_BENEATH` that is decidable without a base.
    ///
    /// A fragment that is absolute, or that begins by stepping above wherever
    /// it starts, is an escape whatever directory it is measured against —
    /// the rule is syntactic, so no base is needed to say no. This is exposed
    /// separately so callers that hold a *descriptor* for the base can refuse
    /// such a request **before** they translate it: the answer is already
    /// determined, and answering it first means the reply cannot be used to
    /// probe whether the descriptor was valid. `sys_openat_beneath` relies on
    /// exactly that, and [`Self::resolve_beneath`] performs the same check as
    /// its own first step, so the two cannot drift.
    ///
    /// Passing a fragment this accepts does not mean the walk will succeed —
    /// a symlink met along the way may still escape, and only the full walk
    /// can see that.
    pub fn beneath_fragment_ok(rel: &Path) -> KernelResult<()> {
        Self::beneath_step(0, rel).map(|_| ())
    }

    /// One step of the `RESOLVE_BENEATH` containment rule.
    ///
    /// `depth` is how many components the walk currently sits below the
    /// containment base.  `fragment` is a path about to be walked from
    /// there — either the caller-supplied relative path at the start of
    /// resolution, or the target of a symlink met part-way through.
    /// Returns the depth after walking it, or [`KernelError::CrossDevice`]
    /// (→ `EXDEV`) if the walk would step *above* the base at any point.
    ///
    /// # The rule is per-hop and syntactic, not "canonicalise and compare"
    ///
    /// This is the part that is counter-intuitive, and the natural
    /// implementation is the wrong one.  The obvious approach — resolve the
    /// whole path, then check the result still has the base as a prefix —
    /// is **more permissive than Linux in exactly the cases an attacker
    /// picks**.  Lane B measured GNU tar (which emulates these same
    /// semantics) against ten cases while building the userspace
    /// substitute in `userspace/coreutils/src/bin/tar.rs`; a prefix check
    /// disagrees on three of them:
    ///
    /// | ancestor symlink points at | `RESOLVE_BENEATH` | a prefix check |
    /// |---|---|---|
    /// | `sub` (relative, inside) | allow | allow |
    /// | `deep/../sub` (`..` that never leaves) | allow | allow |
    /// | `deep/er/../..` (`..` back to the base itself) | allow | allow |
    /// | `$PWD/sub` (absolute, and *inside* the base) | **refuse** | allow ✗ |
    /// | `$PWD` (absolute, the base itself) | **refuse** | allow ✗ |
    /// | `../d/sub` (up and straight back in) | **refuse** | allow ✗ |
    /// | `../out`, `/tmp` (escapes) | refuse | refuse |
    ///
    /// So: an **absolute** target is refused outright, without ever being
    /// compared to the base — being inside it does not save it.  And a
    /// `..` is refused at the moment the walk *would step above* the base,
    /// not judged by where it eventually lands: `deep/er/../..` is allowed
    /// because it never rises above the base, while `../d/sub` is refused
    /// because it does, even though it comes straight back in.
    ///
    /// Tracking a depth counter rather than comparing paths is what makes
    /// those two questions distinguishable at all.  A resolved path has
    /// forgotten how it got there.
    fn beneath_step(depth: usize, fragment: &Path) -> KernelResult<usize> {
        // An absolute target is refused before any comparison: under
        // RESOLVE_BENEATH the base is the whole world, and a target that
        // names a path from `/` has left it by construction.  (This is the
        // row where "but it points inside the base!" is wrong — the caller
        // asked for a walk that cannot address anything outside, and an
        // absolute target is such an address whatever it happens to name.)
        if fragment.is_absolute() {
            return Err(KernelError::CrossDevice);
        }
        let mut depth = depth;
        for comp in fragment.components() {
            match comp.as_bytes() {
                b"." => {}
                b".." => {
                    // The whole rule, in one line: stepping above the base
                    // is refused *here*, at the hop, and not forgiven by a
                    // later component that steps back down.
                    depth = depth.checked_sub(1).ok_or(KernelError::CrossDevice)?;
                }
                _ => depth = depth.saturating_add(1),
            }
        }
        Ok(depth)
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
    ///
    /// When `beneath` is `Some(base)`, every symlink met along the way has
    /// its target checked with [`Self::beneath_step`] before it is followed,
    /// so the walk cannot leave `base` — `openat2`'s `RESOLVE_BENEATH`.  The
    /// check must happen here, per hop, and not once on the final answer:
    /// see that function's doc comment for the three measured cases where
    /// checking the answer instead of the hops is wrong.  `base` must be
    /// normalized and must already be a prefix of `path`; the caller
    /// establishes that (see [`Self::resolve_beneath`]).
    fn resolve_inner(
        path: &Path,
        follow_last: bool,
        depth: usize,
        no_symlinks: bool,
        beneath: Option<&Path>,
        uncacheable: &mut bool,
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
                        Ok((fs, _id, _opts, relative)) => {
                            let mut fs = fs.lock();
                            // Set before the lookup, so a failure inside such
                            // a filesystem is not cached either.
                            if !fs.dcache_safe() {
                                *uncacheable = true;
                            }
                            match fs.entry_type(&relative) {
                                Ok(t) => Some(t),
                                // Last component may not exist yet (creating a
                                // new file/dir/symlink).
                                Err(KernelError::NotFound) if is_last => None,
                                Err(e) => return Err(e),
                            }
                        }
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

                    // RESOLVE_BENEATH: judge the target *here*, where the
                    // walk still knows where it stands, and before the
                    // `normalize_path` below erases the `..` that decide it.
                    // Checking the normalized result instead would allow the
                    // three cases in `beneath_step`'s table -- including an
                    // absolute target and a `..` that leaves and returns.
                    if let Some(base) = beneath {
                        // The target is walked from the symlink's *parent*
                        // directory, so that is the depth it starts at.
                        // `resolved` is at/below `base` by induction: the
                        // entry call established it and every recursion is
                        // guarded by this same check.
                        let below = resolved
                            .components()
                            .count()
                            .saturating_sub(1)
                            .checked_sub(base.components().count())
                            .ok_or(KernelError::CrossDevice)?;
                        Self::beneath_step(below, &target)?;
                    }

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
                        beneath,
                        uncacheable,
                    );
                }
            }
        }

        Ok(resolved)
    }

    // -------------------------------------------------------------------
    // VFS operations
    // -------------------------------------------------------------------

    /// Drop [`EntryType::VolumeLabel`] entries from a listing.
    ///
    /// **A volume label is filesystem metadata, not a directory entry.** FAT
    /// stores it in a root-directory slot, which is an encoding detail of
    /// FAT and not a statement that the volume has a file named `MYDISK`.
    /// The label is reachable through [`Vfs::statvfs`]'s `volume_label` and
    /// settable through [`Vfs::set_volume_label`], which is where a caller
    /// that wants it should look; nothing needs it to also appear in `ls`.
    ///
    /// **Why this is here and not left to the drivers.** It *was* left to the
    /// drivers, and it worked: `FatFs::resolve_path`, `FatFs::readdir` and
    /// `FatFs::readdir_at` each filter labels independently, so no VFS
    /// listing has ever contained one. Lane B asked (2026-08-31) whether
    /// `SYS_FS_LIST_DIR` (603), which drops labels, disagreed with
    /// `SYS_FS_READDIR_AT` (647) and `SYS_FS_GETDENTS_PINNED` (664), which
    /// pass them through — a `cp -r` of a FAT volume acquiring a spurious
    /// entry named after the label. It does not, but only because the one
    /// producer of the variant filters at every route it has.
    ///
    /// That is agreement by luck, and the luck is the kind that expires: an
    /// exFAT driver *does* have a label in its root directory, and whoever
    /// adds one has no reason to know that three syscalls above depend on
    /// their filtering it. The guarantee belongs to the layer that states
    /// it, so this states it — once, before pagination, for every route.
    /// Drivers may keep filtering (FAT's is cheaper there, and it keeps its
    /// name-collision checks honest); it is simply no longer load-bearing.
    ///
    /// Consequence for the ABI: entry-type byte `2` is **reserved and never
    /// emitted** on 603/647/664. The match arms that produce it stay, because
    /// the enum is exhaustive and an unreachable arm is cheaper than an
    /// `unreachable!()` in a syscall path, but no decoder will see one.
    fn drop_volume_labels(entries: &mut Vec<DirEntry>) {
        entries.retain(|e| e.entry_type != EntryType::VolumeLabel);
    }

    /// Turn a backing filesystem's raw listing of `path` into the listing the
    /// VFS promises: no volume labels, plus the mount points that are direct
    /// children of `path`.
    ///
    /// **Every route out of the VFS must go through this**, and it exists
    /// because for a while one of them did not. `readdir` and
    /// `readdir_at_resolved` each open-coded both steps; `readdir_pinned`
    /// open-coded neither, so `SYS_FS_GETDENTS_PINNED` (664) omitted every
    /// mount point that `SYS_FS_READDIR_AT` (647) and `SYS_FS_LIST_DIR` (603)
    /// showed — listing `/` by path gave `tmp`, `proc`, `sys`, `dev`; listing
    /// the same directory through a pinned handle gave only what the root
    /// filesystem physically contained.
    ///
    /// That is worse than a cosmetic difference, because 664 exists to be the
    /// *race-free substitute* for a listing taken by path: a program that
    /// swaps routes to stop a rename racing its walk would silently have
    /// stopped descending into mounted filesystems. Two copies of a rule and
    /// one place that forgot it is the same shape as the four disagreeing
    /// `mkdir` masks in §663; the fix is the same shape too — one
    /// implementation, called from everywhere, rather than three that agree
    /// by inspection.
    ///
    /// **Lock order:** takes `VFS` and must therefore be called with no
    /// backing-filesystem lock held. `readdir_pinned` drops its `fs` guard
    /// before calling this for exactly that reason.
    fn finish_listing(path: &Path, mut entries: Vec<DirEntry>) -> Vec<DirEntry> {
        Self::drop_volume_labels(&mut entries);

        // Mount-point names that are direct children of `path`, each paired
        // with the filesystem mounted there.  E.g. with path="/", mounts at
        // "/tmp" and "/mnt" produce ["tmp", "mnt"]; a nested mount like
        // "/mnt/usb" is not a direct child of "/".
        //
        // The handle is cloned here, under the lock we are already holding to
        // find the name, because the alternative is to reconstruct a path and
        // look the same mount up again from scratch — which is what this used
        // to do, and it was the single most expensive thing about listing a
        // directory that has mounts under it.  See `submount_root_ino`.
        let submounts: Vec<(PathBuf, MountedFs)> = {
            let vfs = VFS.lock();
            Self::submount_children(&vfs, path)
        };

        // Inject the ones the underlying filesystem doesn't know about.
        //
        // The VFS lock is released above, before the loop takes any per-mount
        // lock — the §43 ordering that `submount_root_ino` documents and that
        // the `MountPoint::fs_type` comment explains the cost of getting wrong.
        for (name, fs) in submounts {
            if !entries.iter().any(|e| e.name == name) {
                let ino = Self::submount_root_ino(&fs);
                entries.push(DirEntry {
                    name,
                    entry_type: EntryType::Directory,
                    size: 0,
                    ino,
                });
            }
        }

        entries
    }

    /// List entries in a directory.
    ///
    /// If other filesystems are mounted at sub-paths of `path`, their
    /// mount points appear as directory entries in the listing (even if
    /// the underlying filesystem doesn't have a physical directory there).
    ///
    /// Volume labels are never listed — see [`drop_volume_labels`](Self::drop_volume_labels).
    pub fn readdir(path: impl AsRef<Path>) -> KernelResult<Vec<DirEntry>> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        check_path_access(&path, PathAccess::Read)?;

        let raw = {
            let (fs, _id, _opts, relative) = resolve_mount(&path)?;
            fs.lock().readdir(&relative)?
        };
        Ok(Self::finish_listing(&path, raw))
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

        let raw = {
            let (fs, _id, _opts, relative) = resolve_mount(path)?;
            fs.lock().readdir(&relative)?
        };
        // Both steps run *before* `total` is taken and before the page is
        // sliced.  A label dropped or a mount injected after the slice would
        // make `entries_written` disagree with how far the offset actually
        // advanced, so the caller's next page would step over a real
        // neighbour: filtering belongs where the offset is computed, not
        // where the record is packed.
        let entries = Self::finish_listing(path, raw);

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
        // `alloc::vec![0u8; out_len]` aborts the process on allocation
        // failure, which in a kernel is a panic. On 2026-09-18 a
        // 22,526,200-byte read did exactly that with 2.7 GB free: the
        // buddy allocator rounds a request up to a power-of-two frame
        // count (heap.rs:883), so 1,376 frames became 2,048 and a 22.5 MB
        // file demanded a 32 MiB CONTIGUOUS block. A 20.5 MB file is the
        // same order and reads fine, so this is fragmentation rather than
        // a size limit -- i.e. nondeterministic, which is the worst kind of
        // panic to leave in a read path.
        //
        // `try_reserve_exact` returns instead of aborting. The `resize`
        // after it cannot fail, because the capacity is already reserved.
        let mut buf: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
        buf.try_reserve_exact(out_len)
            .map_err(|_| crate::error::KernelError::OutOfMemory)?;
        buf.resize(out_len, 0u8);
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
        Self::write_file_inner(path.as_ref(), data, Creating::AsNeeded)
    }

    /// Make an empty regular file at the already-resolved host `path`, which
    /// must not exist yet -- `open(O_CREAT)`'s creation -- with the mode `mode`
    /// asks for, less the caller's umask or as its directory's default ACL
    /// gives it, and its owner and ACL, all under the one hold of the
    /// filesystem's lock that makes it: nothing sees the file without them.
    ///
    /// # Errors
    ///
    /// `AlreadyExists` when something has the name already; otherwise as
    /// [`write_file_resolved`](Self::write_file_resolved).
    pub fn create_file_resolved(path: &Path, mode: u16) -> KernelResult<()> {
        Self::write_file_inner(path, &[], Creating::New(mode))
    }

    /// [`write_file_resolved`](Self::write_file_resolved) and
    /// [`create_file_resolved`](Self::create_file_resolved).
    fn write_file_inner(path: &Path, data: &[u8], creating: Creating) -> KernelResult<()> {
        check_path_access(path, PathAccess::Write)?;
        check_writable(path)?;
        // Intercept: let pre-operation handlers approve/deny before proceeding.
        // Called before VFS lock to avoid deadlock (interceptors must not call VFS).
        super::intercept::pre_write(path)?;
        // Quota: check whether this write would exceed the user's quota.
        // uid 0 is the default until per-process identity is wired up.
        enforce_quota_write(path, data.len() as u64)?;
        let creator = creator_ids();
        let umask = creator_umask();
        let cache_inval = {
            let (fs, fs_id, _opts, relative) = resolve_mount(path)?;
            let mut guard = fs.lock();
            // A write that makes the file gives it its owner, mode and ACL;
            // one that replaces an existing file's contents leaves them be.
            let made = guard.lstat(&relative).is_err();
            if !made && matches!(creating, Creating::New(_)) {
                return Err(KernelError::AlreadyExists);
            }
            // The new file's mode and ACLs, when this write makes it.
            let new_node = if made {
                // A new name in its directory (`fs::attr_policy`). Replacing
                // an existing file's contents is the filesystem's to refuse,
                // on the inode it writes.
                attr_policy::may_create(dir_attrs(&mut **guard, &relative))?;
                let requested = match creating {
                    Creating::AsNeeded => Self::DEFAULT_FILE_MODE,
                    Creating::New(mode) => mode & 0o7777,
                };
                Some(new_node_mode(
                    &mut **guard,
                    fs_id,
                    parent_of(&relative),
                    parent_of(path),
                    requested,
                    umask,
                    false,
                ))
            } else {
                if let Some((seals, meta)) = seals_at(&mut **guard, fs_id, &relative, path) {
                    // Every byte rewritten, and the size taken from the old to
                    // the new (`fs::sealing`).
                    let len = data.len() as u64;
                    super::sealing::may_write(seals, 0, len, meta.size)?;
                    super::sealing::may_resize(seals, meta.size, len)?;
                }
                None
            };
            guard.write_file(&relative, data)?;
            if let Some((perm, acls)) = new_node {
                stamp_new_mode(&mut **guard, &relative, perm, Self::DEFAULT_FILE_MODE)?;
                init_new_owner(&mut **guard, &relative, creator, false)?;
                acls.store(&mut **guard, fs_id, None, Some(&relative), path)?;
            }
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
        // Version history (design-decisions §936, §971): queue this save's
        // result to be recorded after we return, instead of reading back and
        // checksumming the old content before the write, which was about
        // half the cost of saving a small file.  A no-op for a path not
        // enrolled for history.
        super::history::record_after_save(path);
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
        let (cache_inval, unlinked) = {
            let (fs, fs_id, _opts, relative) = resolve_mount(&path)?;
            let mut guard = fs.lock();
            // Capture identity *before* removal — the inode (and its number)
            // is gone afterward, and that number may be reused by a future
            // file.  Dropping the cached pages now prevents a later file that
            // reuses this inode from being served the removed file's bytes.
            let id = cache_identity(&mut guard, fs_id, &relative);
            // The same hazard for the state kept about files outside the
            // filesystem -- an ACL, flags, seals, attributes -- which a file
            // reusing the number would otherwise inherit (`super::perfile`).
            let unlinked = unlinked_object(&mut guard, fs_id, &relative);
            guard_delete(&mut **guard, &relative)?;
            guard.remove(&relative)?;
            (id, unlinked)
        };
        if let Some((fs_id, ino)) = cache_inval {
            crate::mm::page_cache::invalidate_identity(fs_id, ino);
        }
        super::perfile::name_removed(unlinked, &path);
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

    /// The mode memfs and ext4 give a regular file `write_file` makes, and so
    /// the one a creation does not need to stamp again.
    pub const DEFAULT_FILE_MODE: u16 = 0o644;

    pub fn mkdir(path: impl AsRef<Path>) -> KernelResult<()> {
        let path = path.as_ref();
        Self::mkdir_mode(path, Self::DEFAULT_DIR_MODE)
    }

    /// Create a directory, stamping it with `mode` (masked to `0o1777` — the
    /// nine permission bits plus sticky) instead of the 0o755 default.
    ///
    /// `mode` is expected to be **already umask-masked by the caller** — the
    /// umask lives in the userspace POSIX layer, so the kernel treats `mode`
    /// as the final on-disk permission bits (same thin-primitive model as the
    /// file-create path in [`crate::fs::handle::open_with_mode`]).
    ///
    /// **Ten bits, not nine and not twelve**, matching Linux's `vfs_mkdir`
    /// (`mode &= (S_IRWXUGO|S_ISVTX)`) exactly. This is deliberately *narrower*
    /// than the file-create path's `0o7777`, and Linux draws the same line in
    /// the same place (`vfs_create` keeps `S_IALLUGO`): setgid on a directory
    /// is *inherited from the parent*, not requested by the creator, so a mode
    /// word is the wrong channel for it. See `design-decisions.md` §663.
    ///
    /// It is also wider than it was. This masked to `0o777` until 2026-09-01,
    /// which meant §639's widening of `sys_fs_mkdir_mode` to `0o7777` never
    /// reached the filesystem — the handler stopped dropping sticky and this
    /// line dropped it one layer down, so `mkdir(path, 0o1777)` produced a
    /// `0o777` directory exactly as before and nothing said otherwise.
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
        let creator = creator_ids();
        let umask = creator_umask();
        {
            let (fs, fs_id, _opts, relative) = resolve_mount(&path)?;
            let mut guard = fs.lock();
            guard_create(&mut **guard, &relative)?;
            // The mode asked for (twelve bits but setuid and setgid, §663),
            // less the umask or as the parent's default ACL gives it, stamped
            // under the same lock as the mkdir -- which stamps 0o755 itself --
            // and before the owner, whose set-group-ID inheritance adds a bit
            // this would otherwise overwrite.
            let (perm, acls) = new_node_mode(
                &mut **guard,
                fs_id,
                parent_of(&relative),
                parent_of(&path),
                mode & 0o1777,
                umask,
                true,
            );
            guard.mkdir(&relative)?;
            stamp_new_mode(&mut **guard, &relative, perm, Self::DEFAULT_DIR_MODE)?;
            init_new_owner(&mut **guard, &relative, creator, true)?;
            acls.store(&mut **guard, fs_id, None, Some(&relative), &path)?;
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
    /// Limited to [`MAX_MKDIR_ALL_COMPONENTS`] path components to prevent abuse.
    pub fn mkdir_all(path: impl AsRef<Path>) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        validate_path(path)?;
        let norm = normalize_path(path);

        let components: Vec<&Path> = norm.components().collect();

        if components.len() > MAX_MKDIR_ALL_COMPONENTS {
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
        let unlinked = {
            let (fs, fs_id, _opts, relative) = resolve_mount(&path)?;
            let mut guard = fs.lock();
            // A directory's state ends with it, as a file's does; see `remove`.
            let unlinked = unlinked_object(&mut guard, fs_id, &relative);
            guard_delete(&mut **guard, &relative)?;
            guard.rmdir(&relative)?;
            unlinked
        };
        super::perfile::name_removed(unlinked, &path);
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

        // `alloc::vec![0u8; out_len]` aborts the process on allocation
        // failure, which in a kernel is a panic. On 2026-09-18 a
        // 22,526,200-byte read did exactly that with 2.7 GB free: the
        // buddy allocator rounds a request up to a power-of-two frame
        // count (heap.rs:883), so 1,376 frames became 2,048 and a 22.5 MB
        // file demanded a 32 MiB CONTIGUOUS block. A 20.5 MB file is the
        // same order and reads fine, so this is fragmentation rather than
        // a size limit -- i.e. nondeterministic, which is the worst kind of
        // panic to leave in a read path.
        //
        // `try_reserve_exact` returns instead of aborting. The `resize`
        // after it cannot fail, because the capacity is already reserved.
        let mut buf: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
        buf.try_reserve_exact(out_len)
            .map_err(|_| crate::error::KernelError::OutOfMemory)?;
        buf.resize(out_len, 0u8);
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
            if let Some((seals, meta)) = seals_at(&mut **guard, fs_id, &relative, path) {
                super::sealing::may_write(seals, offset, data.len() as u64, meta.size)?;
            }
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

    /// Append data to the end of a file, creating it if it doesn't exist.
    ///
    /// The end is found and written in **one hold of the filesystem's lock**
    /// ([`append_resolved`](Self::append_resolved)), so two appenders never
    /// land on the same offset and two creators never replace each other.
    /// Until 2026-09-26 this was `stat` and then `write_at`, two separate
    /// holds: concurrent appenders could both read the same size, and the
    /// second silently overwrote the first's record (known-issues.md
    /// `A-VFS-APPEND-RACES`).
    pub fn append(path: impl AsRef<Path>, data: &[u8]) -> KernelResult<()> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_follow(path)?;
        Self::append_resolved(&path, data).map(|_| ())
    }

    /// Like [`append`](Self::append) but on an **already-resolved** host path
    /// (see [`read_at_resolved`](Self::read_at_resolved)).
    ///
    /// The checks and bookkeeping are [`write_file_resolved`]'s, less the
    /// version-history snapshot, which records content about to be
    /// overwritten and an append overwrites nothing.
    ///
    /// [`write_file_resolved`]: Self::write_file_resolved
    ///
    /// Returns the offset the data landed at: the end it found.
    pub fn append_resolved(path: impl AsRef<Path>, data: &[u8]) -> KernelResult<u64> {
        let path = path.as_ref();
        check_path_access(path, PathAccess::Write)?;
        check_writable(path)?;
        // Before the VFS lock, as for every write: interceptors must not
        // call back into the VFS while it is held.
        super::intercept::pre_write(path)?;
        enforce_quota_write(path, data.len() as u64)?;
        let cache_inval = {
            let (fs, fs_id, _opts, relative) = resolve_mount(path)?;
            let mut guard = fs.lock();
            // The end, and the write at it, under the same hold: nothing can
            // move the end in between. A missing file is created under the
            // same hold, so a second creator finds it and appends.
            let at = match guard.stat(&relative) {
                Ok(entry) => {
                    if let Some((seals, _)) = seals_at(&mut **guard, fs_id, &relative, path) {
                        let len = data.len() as u64;
                        super::sealing::may_write(seals, entry.size, len, entry.size)?;
                    }
                    guard.write_at(&relative, entry.size, data)?;
                    entry.size
                }
                Err(KernelError::NotFound) => {
                    attr_policy::may_create(dir_attrs(&mut **guard, &relative))?;
                    guard.write_file(&relative, data)?;
                    0
                }
                Err(e) => return Err(e),
            };
            (at, cache_identity(&mut guard, fs_id, &relative))
        };
        let (at, cache_inval) = cache_inval;
        if let Some((fs_id, ino)) = cache_inval {
            crate::mm::page_cache::invalidate_identity(fs_id, ino);
        }
        super::quota::charge_bytes(0, 0, data.len() as u64);
        // The append may have created the file: drop negative entries.
        VFS_DCACHE.lock().invalidate_negative_prefix(path);
        super::notify::emit_modified(path);
        super::index::on_file_changed(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        super::audit::log_ok(super::audit::AuditOp::Write, 0, path);
        Ok(at)
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
            if let Some((seals, meta)) = seals_at(&mut **guard, fs_id, &relative, path) {
                super::sealing::may_resize(seals, meta.size, size)?;
            }
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

    // ----- An open regular file, held as its object -----
    //
    // `fs::handle` holds an open regular file as its filesystem and inode
    // where the filesystem allows (known-issues
    // A-AN-OPEN-FILE-FOLLOWS-ITS-NAME): a rename or an unlink of the name it
    // was opened under leaves the handle on the file, and its sizes are the
    // file's own at each call. These are those calls. Everything else about a
    // handle -- events, quota's audit trail, `/proc` -- keeps the open-time
    // name.

    /// Hold the regular file at already-resolved `path` as an object.
    ///
    /// `Ok(None)` where it cannot be held: not a regular file, no stable
    /// inode, or a filesystem without inode-addressed I/O (FAT, the pseudo
    /// filesystems). The caller then goes by path, as every handle did
    /// before.
    ///
    /// The hold is counted on the mount first, under the mount table's lock,
    /// so an unmount racing this open either sees the count or removes the
    /// mount before it is found; it never leaves an object on a filesystem it
    /// has taken away.
    ///
    /// # Errors
    ///
    /// The filesystem's own, from looking the file up or pinning it.
    pub fn open_object(path: impl AsRef<Path>) -> KernelResult<Option<FileHold>> {
        let path = path.as_ref();
        let (fs, fs_id, relative) = {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, path)?;
            mp.objects = mp.objects.saturating_add(1);
            (Arc::clone(&mp.fs), mp.fs_id, relative.to_path_buf())
        };
        let pinned = {
            let mut guard = fs.lock();
            match guard.metadata(&relative) {
                Ok(meta) if meta.entry_type == EntryType::File && meta.ino != 0 => {
                    guard.pin_ino(meta.ino).map(|()| Some(meta.ino))
                }
                Ok(_) => Ok(None),
                Err(e) => Err(e),
            }
        };
        match pinned {
            Ok(Some(ino)) => {
                note_hold(FileId { fs_id, ino });
                Ok(Some(FileHold(FileObject { fs, fs_id, ino })))
            }
            Ok(None) | Err(KernelError::NotSupported) => {
                release_mount_hold(fs_id);
                Ok(None)
            }
            Err(e) => {
                release_mount_hold(fs_id);
                Err(e)
            }
        }
    }

    /// Make a regular file with no name in already-resolved directory `dir`,
    /// held: Linux's `O_TMPFILE` (`fs::handle::open_tmpfile`).
    ///
    /// The file exists only through the hold: no one can open it by a name,
    /// and it goes when the last reference to the hold does -- unless
    /// [`link_object`](Self::link_object) names it first, which `linkable`
    /// allows (`O_TMPFILE` without `O_EXCL`). It is shown under `dir/#ino`,
    /// as Linux shows one.
    ///
    /// Asked as a create in `dir` is: write access to it, a writable mount,
    /// an interceptor, quota. No event and no journal entry: there is no name
    /// to report, as Linux reports none.
    ///
    /// # Errors
    ///
    /// `NotSupported` on a filesystem that cannot hold a file with no name
    /// (FAT, the pseudo filesystems); `NotADirectory`; the checks' and the
    /// filesystem's own.
    pub fn create_unnamed_object(dir: &Path, mode: u16, linkable: bool) -> KernelResult<FileHold> {
        check_path_access(dir, PathAccess::Write)?;
        check_writable(dir)?;
        super::intercept::pre_check(super::intercept::FsOp::Write, dir, None)?;
        enforce_quota_create(dir)?;
        // The hold is counted on the mount first, as `open_object` counts
        // one, so an unmount racing this create cannot take the filesystem
        // away under it.
        let (fs, fs_id, relative) = {
            let mut vfs = VFS.lock();
            let (mp, relative) = find_mount(&mut vfs, dir)?;
            mp.objects = mp.objects.saturating_add(1);
            (Arc::clone(&mp.fs), mp.fs_id, relative.to_path_buf())
        };
        let creator = creator_ids();
        let umask = creator_umask();
        let made = {
            let mut guard = fs.lock();
            // Nothing is made in an immutable directory, named or not.
            let allowed = guard_create_in(&mut **guard, &relative);
            // The directory it is made in is `relative` itself.
            let (perm, acls) = new_node_mode(
                &mut **guard,
                fs_id,
                &relative,
                dir,
                mode & 0o7777,
                umask,
                false,
            );
            allowed
                .and_then(|()| guard.create_unnamed(&relative, perm))
                .and_then(|ino| {
                    // Its owner, as a named file's (`init_new_owner`), by inode:
                    // it has no name to give it one by.
                    let parent = guard.metadata(&relative).ok();
                    let gid = match parent {
                        Some(p) if p.permissions & S_ISGID != 0 => p.gid,
                        _ => creator.1,
                    };
                    if (creator.0, gid) != (0, 0) {
                        match guard.chown_ino(ino, creator.0, gid) {
                            Ok(()) | Err(KernelError::NotSupported) => {}
                            Err(e) => {
                                // Let go of the hold the creation gave it, so the
                                // nameless file does not outlive the failure.
                                guard.unpin_ino(ino);
                                return Err(e);
                            }
                        }
                    }
                    // Its ACL from the directory's default, by inode, as its
                    // owner: it has no name to key it by.
                    let named = dir.join(alloc::format!("#{ino}"));
                    if let Err(e) = acls.store(&mut **guard, fs_id, Some(ino), None, &named) {
                        guard.unpin_ino(ino);
                        return Err(e);
                    }
                    Ok(ino)
                })
        };
        let ino = match made {
            Ok(ino) => ino,
            Err(e) => {
                release_mount_hold(fs_id);
                return Err(e);
            }
        };
        note_unnamed(
            FileId { fs_id, ino },
            dir.join(alloc::format!("#{ino}")),
            linkable,
        );
        Ok(FileHold(FileObject { fs, fs_id, ino }))
    }

    /// Give held file `obj` the name `new_path` (not yet resolved): `linkat`
    /// of a descriptor, by `AT_EMPTY_PATH` or a followed `/proc/self/fd/N`,
    /// and the native `SYS_FS_LINK_HANDLE`.
    ///
    /// A file made with no name is named if it was made to be (`O_TMPFILE`
    /// without `O_EXCL`), as Linux's `I_LINKABLE` allows, and then no longer
    /// goes with its last hold. A file with a name gets another, as `link`
    /// gives one. A file deleted while open, or made with `O_EXCL`, is
    /// `NotFound`, as Linux answers `ENOENT`. Asked as `link` asks the new
    /// name: write access, a writable mount, an interceptor, quota, the same
    /// mount (`CrossDevice`).
    ///
    /// # Errors
    ///
    /// As above; `AlreadyExists` for a name taken; the filesystem's own.
    pub fn link_object(obj: &FileObject, new_path: impl AsRef<Path>) -> KernelResult<()> {
        let new_path = new_path.as_ref();
        crate::ipc::namespace::check_writable(new_path)?;
        let new_path = Self::resolve_no_follow(new_path)?;
        check_path_access(&new_path, PathAccess::Write)?;
        check_writable(&new_path)?;
        super::intercept::pre_check(super::intercept::FsOp::Link, &new_path, None)?;
        enforce_quota_create(&new_path)?;
        if !may_link(obj.id()) {
            return Err(KernelError::NotFound);
        }
        {
            let (_fs, fs_id, _opts, rel_new) = resolve_mount(&new_path)?;
            if fs_id != obj.fs_id {
                return Err(KernelError::CrossDevice);
            }
            let mut guard = obj.fs.lock();
            let source = ino_attrs(&mut **guard, obj.ino);
            guard_link_attrs(&mut **guard, source, &rel_new)?;
            guard.link_held_ino(obj.ino, &rel_new)?;
        }
        note_named(obj.id());
        // A name is counted as `link_inner` counts one.
        super::quota::charge_inode(0, 0);
        VFS_DCACHE.lock().invalidate_negative_prefix(&new_path);
        super::notify::emit_created(&new_path);
        super::index::on_file_changed(&new_path);
        super::journal::record(super::journal::JournalEventType::Created, &new_path);
        super::audit::log_ok(super::audit::AuditOp::Link, 0, &new_path);
        Ok(())
    }

    /// The file's metadata now, its device included. `nlinks` is 0 once its
    /// last name has gone.
    ///
    /// # Errors
    ///
    /// The filesystem's own.
    pub fn object_metadata(obj: &FileObject) -> KernelResult<FileMeta> {
        let mut meta = obj.fs.lock().metadata_ino(obj.ino)?;
        meta.dev = dev_of(obj.fs_id);
        Ok(meta)
    }

    /// Read through an object: through the page cache, as
    /// [`read_at_resolved`](Self::read_at_resolved) is for a path, and
    /// clamped to the file's size now rather than when it was opened, so a
    /// reader sees what another writer has added. `path` names it for the
    /// access event.
    ///
    /// # Errors
    ///
    /// `OutOfMemory` for a buffer that cannot be had; the filesystem's own.
    pub fn object_read(
        obj: &FileObject,
        path: &Path,
        offset: u64,
        len: usize,
    ) -> KernelResult<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let size = obj.fs.lock().metadata_ino(obj.ino)?.size;
        if offset >= size {
            return Ok(Vec::new());
        }
        let avail = size.saturating_sub(offset);
        let out_len = usize::try_from(avail).map_or(len, |a| a.min(len));
        // As in `read_at_routed`: a failed allocation is an error, not an
        // abort.
        let mut buf: Vec<u8> = Vec::new();
        buf.try_reserve_exact(out_len)
            .map_err(|_| KernelError::OutOfMemory)?;
        buf.resize(out_len, 0u8);
        crate::mm::page_cache::read_through(obj.id(), offset, &mut buf, |page_off, page_buf| {
            let data = obj.fs.lock().read_ino(obj.ino, page_off, page_buf.len())?;
            let n = data.len().min(page_buf.len());
            if let (Some(dst), Some(src)) = (page_buf.get_mut(..n), data.get(..n)) {
                dst.copy_from_slice(src);
            }
            Ok(())
        })?;
        if super::notify::interest_includes(super::notify::FsEventMask::ACCESS) {
            super::notify::emit(super::notify::FsEventType::Accessed, path, None);
        }
        Ok(buf)
    }

    /// [`object_read`](Self::object_read) straight from the filesystem, past
    /// the page cache: the cache's own fill, for an `mmap` fault.
    ///
    /// # Errors
    ///
    /// The filesystem's own.
    pub fn object_read_uncached(
        obj: &FileObject,
        offset: u64,
        len: usize,
    ) -> KernelResult<Vec<u8>> {
        obj.fs.lock().read_ino(obj.ino, offset, len)
    }

    /// Write through an object at `offset`.
    ///
    /// The open was the access check, as POSIX has it: a `chmod` after the
    /// open does not take a descriptor's write away. What is checked here is
    /// what can change under an open file: the mount turned read-only, an
    /// interceptor, quota. Never creates a file: a held file whose name is
    /// gone is written where it is, unnamed. `path` names it for events.
    ///
    /// # Errors
    ///
    /// `ReadOnlyFilesystem`, `DiskFull` (quota), an interceptor's refusal,
    /// the filesystem's own.
    pub fn object_write(
        obj: &FileObject,
        path: &Path,
        offset: u64,
        data: &[u8],
    ) -> KernelResult<()> {
        Self::object_write_checks(obj, path, data.len())?;
        {
            let mut guard = obj.fs.lock();
            if let Some((seals, meta)) = seals_held(&mut **guard, obj) {
                super::sealing::may_write(seals, offset, data.len() as u64, meta.size)?;
            }
            guard.write_ino(obj.ino, offset, data)?;
        }
        Self::object_written(obj, path, data.len());
        Ok(())
    }

    /// Write through an object at the file's end, found and written in one
    /// call so two appenders cannot land on one offset: `O_APPEND`. Returns
    /// where it landed. Checks as [`object_write`](Self::object_write).
    ///
    /// # Errors
    ///
    /// As [`object_write`](Self::object_write).
    pub fn object_append(obj: &FileObject, path: &Path, data: &[u8]) -> KernelResult<u64> {
        Self::object_write_checks(obj, path, data.len())?;
        let at = {
            let mut guard = obj.fs.lock();
            if let Some((seals, meta)) = seals_held(&mut **guard, obj) {
                super::sealing::may_write(seals, meta.size, data.len() as u64, meta.size)?;
            }
            guard.append_ino(obj.ino, data)?
        };
        Self::object_written(obj, path, data.len());
        Ok(at)
    }

    /// Cut or extend a held file to `size`. Checks as
    /// [`object_write`](Self::object_write), less quota.
    ///
    /// # Errors
    ///
    /// `ReadOnlyFilesystem`, the filesystem's own.
    pub fn object_truncate(obj: &FileObject, path: &Path, size: u64) -> KernelResult<()> {
        check_writable_fs(obj.fs_id)?;
        {
            let mut guard = obj.fs.lock();
            if let Some((seals, meta)) = seals_held(&mut **guard, obj) {
                super::sealing::may_resize(seals, meta.size, size)?;
            }
            guard.truncate_ino(obj.ino, size)?;
        }
        crate::mm::page_cache::invalidate_identity(obj.fs_id, obj.ino);
        super::notify::emit_modified(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// `fchmod` through a held file, whatever its name now. The open was the
    /// access check, as for every call through a handle; what can refuse it
    /// now is the mount turned read-only, or the file made immutable or
    /// append-only (`fs::attr_policy`). `path` names it for events.
    ///
    /// # Errors
    ///
    /// `ReadOnlyFilesystem`; `NotPermitted`; the filesystem's own.
    pub fn object_set_permissions(
        obj: &FileObject,
        path: &Path,
        permissions: u16,
    ) -> KernelResult<()> {
        check_writable_fs(obj.fs_id)?;
        {
            let mut guard = obj.fs.lock();
            attr_policy::may_change_metadata(ino_attrs(&mut **guard, obj.ino))?;
            if let Some((seals, meta)) = seals_held(&mut **guard, obj) {
                super::sealing::may_change_mode(seals, meta.permissions, permissions)?;
            }
            guard.chmod_ino(obj.ino, permissions)?;
            acl_follow_chmod(Some(obj.id()), path, permissions);
        }
        super::notify::emit_metadata(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// `fchown` through a held file, as
    /// [`object_set_permissions`](Self::object_set_permissions). `u32::MAX`
    /// leaves an id as it is, as for [`set_owner`](Self::set_owner).
    ///
    /// # Errors
    ///
    /// `ReadOnlyFilesystem`; `NotPermitted`; the filesystem's own.
    pub fn object_set_owner(obj: &FileObject, path: &Path, uid: u32, gid: u32) -> KernelResult<()> {
        check_writable_fs(obj.fs_id)?;
        {
            let mut fs = obj.fs.lock();
            if changes_owner(uid, gid) {
                attr_policy::may_change_metadata(ino_attrs(&mut **fs, obj.ino))?;
            }
            let (uid, gid) = if uid == u32::MAX || gid == u32::MAX {
                let meta = fs.metadata_ino(obj.ino)?;
                (
                    if uid == u32::MAX { meta.uid } else { uid },
                    if gid == u32::MAX { meta.gid } else { gid },
                )
            } else {
                (uid, gid)
            };
            fs.chown_ino(obj.ino, uid, gid)?;
        }
        super::notify::emit_metadata(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// `futimens` through a held file, as
    /// [`object_set_permissions`](Self::object_set_permissions). A time of 0
    /// is left as it is; [`TIME_NOW`] is now.
    ///
    /// # Errors
    ///
    /// `ReadOnlyFilesystem`; `NotPermitted`; the filesystem's own.
    pub fn object_set_times(
        obj: &FileObject,
        accessed_ns: Timestamp,
        modified_ns: Timestamp,
    ) -> KernelResult<()> {
        check_writable_fs(obj.fs_id)?;
        let mut guard = obj.fs.lock();
        times_rule(ino_attrs(&mut **guard, obj.ino), accessed_ns, modified_ns)?;
        let now = metadata_now_ns();
        guard.utimes_ino(
            obj.ino,
            resolve_time(accessed_ns, now),
            resolve_time(modified_ns, now),
        )
        // No notify/journal — timestamp changes are metadata-only.
    }

    /// `fgetxattr` through a held file, whatever its names now: Linux's rules
    /// (`fs::xattr_policy`) on the file it is, for the calling task.
    ///
    /// # Errors
    ///
    /// `fs::xattr_policy`'s; the filesystem's (`NoAttribute`).
    pub fn object_get_xattr(obj: &FileObject, name: &[u8]) -> KernelResult<Vec<u8>> {
        if name == super::acl::XATTR_ACCESS {
            let mut guard = obj.fs.lock();
            let meta = guard.metadata_ino(obj.ino)?;
            return acl_door_get(&AclDoorFile::held(meta, obj, Path::new("")));
        }
        if name == super::acl::XATTR_DEFAULT {
            let mut guard = obj.fs.lock();
            let meta = guard.metadata_ino(obj.ino)?;
            return acl_default_get(&AclDoorFile::held(meta, obj, Path::new("")));
        }
        let caller = xattr_policy::Caller::current();
        Self::object_xattr_on(obj, name, xattr_policy::Access::Read, caller, |fs, ino| {
            fs.get_xattr_ino(ino, name)
        })
    }

    /// `fsetxattr` through a held file, as
    /// [`object_get_xattr`](Self::object_get_xattr). `path` names it for
    /// events.
    ///
    /// # Errors
    ///
    /// `ReadOnlyFilesystem`; `fs::xattr_policy`'s; the mode's; the
    /// filesystem's.
    pub fn object_set_xattr(
        obj: &FileObject,
        path: &Path,
        name: &[u8],
        value: &[u8],
        mode: XattrSetMode,
    ) -> KernelResult<()> {
        check_writable_fs(obj.fs_id)?;
        let caller = xattr_policy::Caller::current();
        if name == super::acl::XATTR_ACCESS {
            let mut guard = obj.fs.lock();
            let meta = guard.metadata_ino(obj.ino)?;
            let subject = AclDoorFile::held(meta, obj, path);
            acl_door_set(&subject, value, mode, caller, |new_mode| {
                if let Some((seals, meta)) = seals_held(&mut **guard, obj) {
                    super::sealing::may_change_mode(seals, meta.permissions, new_mode)?;
                }
                guard.chmod_ino(obj.ino, new_mode)
            })?;
        } else if name == super::acl::XATTR_DEFAULT {
            let mut guard = obj.fs.lock();
            let meta = guard.metadata_ino(obj.ino)?;
            acl_default_set(&AclDoorFile::held(meta, obj, path), value, mode, caller)?;
        } else {
            Self::object_xattr_on(obj, name, xattr_policy::Access::Write, caller, |fs, ino| {
                mode.check(fs.get_xattr_ino(ino, name))?;
                fs.set_xattr_ino(ino, name, value)
            })?;
        }
        super::notify::emit_metadata(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// `fremovexattr` through a held file, as
    /// [`object_set_xattr`](Self::object_set_xattr).
    ///
    /// # Errors
    ///
    /// `ReadOnlyFilesystem`; `fs::xattr_policy`'s; `NoAttribute`.
    pub fn object_remove_xattr(obj: &FileObject, path: &Path, name: &[u8]) -> KernelResult<()> {
        check_writable_fs(obj.fs_id)?;
        let caller = xattr_policy::Caller::current();
        if name == super::acl::XATTR_ACCESS {
            let mut guard = obj.fs.lock();
            let meta = guard.metadata_ino(obj.ino)?;
            acl_door_remove(&AclDoorFile::held(meta, obj, path), caller)?;
        } else if name == super::acl::XATTR_DEFAULT {
            let mut guard = obj.fs.lock();
            let meta = guard.metadata_ino(obj.ino)?;
            acl_default_remove(&AclDoorFile::held(meta, obj, path), caller)?;
        } else {
            Self::object_xattr_on(obj, name, xattr_policy::Access::Write, caller, |fs, ino| {
                fs.remove_xattr_ino(ino, name)
            })?;
        }
        super::notify::emit_metadata(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// `flistxattr` through a held file: the names the calling task may see
    /// ([`xattr_policy::listed`]), checked against nothing else, as a path's
    /// listing is.
    ///
    /// # Errors
    ///
    /// The filesystem's.
    pub fn object_list_xattrs(obj: &FileObject) -> KernelResult<Vec<Vec<u8>>> {
        let privileged = xattr_policy::Caller::current().privileged;
        let (mut names, meta) = {
            let mut guard = obj.fs.lock();
            // Discarded deliberately, as `xattr_list_as`'s.
            (
                guard.list_xattrs_ino(obj.ino)?,
                guard.metadata_ino(obj.ino).ok(),
            )
        };
        names.retain(|name| xattr_policy::listed(name, privileged));
        if let Some(meta) = meta {
            acl_door_listed(&AclDoorFile::held(meta, obj, Path::new("")), &mut names);
        }
        Ok(names)
    }

    /// Whether a held file's mount takes a change now: Linux's
    /// `mnt_want_write_file`, which `fremovexattr` meets before it reads the
    /// name.
    ///
    /// # Errors
    ///
    /// `ReadOnlyFilesystem`.
    pub fn object_check_writable(obj: &FileObject) -> KernelResult<()> {
        check_writable_fs(obj.fs_id)
    }

    /// [`xattr_on`](Self::xattr_on) for a held file, by its inode number. The
    /// ACL is the file's own, by its identity ([`check_object_access`]); the
    /// capability tags were the open's to check.
    fn object_xattr_on<T>(
        obj: &FileObject,
        name: &[u8],
        access: xattr_policy::Access,
        caller: xattr_policy::Caller,
        op: impl FnOnce(&mut dyn FileSystem, u64) -> KernelResult<T>,
    ) -> KernelResult<T> {
        let namespace = xattr_policy::Namespace::of(name);
        if namespace.checks_permission() && super::acl::count() != 0 {
            let meta = obj.fs.lock().metadata_ino(obj.ino)?;
            xattr_policy::namespace_rules(namespace, access, &meta, caller)?;
            check_object_access(obj, &meta, path_access_for(access))?;
        }
        let mut guard = obj.fs.lock();
        let meta = guard.metadata_ino(obj.ino)?;
        xattr_policy::namespace_rules(namespace, access, &meta, caller)?;
        xattr_policy::after_permission(name, access, caller, guard.xattrs_supported())?;
        op(&mut **guard, obj.ino)
    }

    /// `fallocate(KEEP_SIZE)` through a held file: reserve space for its
    /// first `size` bytes without changing its size.
    ///
    /// # Errors
    ///
    /// `ReadOnlyFilesystem`; the filesystem's own.
    pub fn object_fallocate(obj: &FileObject, size: u64) -> KernelResult<()> {
        check_writable_fs(obj.fs_id)?;
        let mut guard = obj.fs.lock();
        attr_policy::may_allocate(ino_attrs(&mut **guard, obj.ino))?;
        if let Some((seals, meta)) = seals_held(&mut **guard, obj) {
            super::sealing::may_resize(seals, meta.size, meta.size.max(size))?;
        }
        guard.fallocate_ino(obj.ino, size)
    }

    /// `fstatfs` through a held file: the filesystem it is on, read-only if
    /// the mount is, as [`statvfs_resolved`](Self::statvfs_resolved) reports.
    ///
    /// # Errors
    ///
    /// `NotFound` if the mount has gone; the filesystem's own.
    pub fn object_statvfs(obj: &FileObject) -> KernelResult<FsInfo> {
        let read_only = {
            let vfs = VFS.lock();
            vfs.mounts
                .iter()
                .find(|m| m.fs_id == obj.fs_id)
                .map(|m| m.options.read_only)
                .ok_or(KernelError::NotFound)?
        };
        let mut info = obj.fs.lock().statvfs()?;
        info.read_only |= read_only;
        Ok(info)
    }

    /// What may refuse a write to a held file: the mount turned read-only,
    /// an interceptor, quota.
    fn object_write_checks(obj: &FileObject, path: &Path, len: usize) -> KernelResult<()> {
        check_writable_fs(obj.fs_id)?;
        // Before the filesystem lock, as for every write: interceptors must
        // not call back into the VFS while it is held.
        super::intercept::pre_write(path)?;
        enforce_quota_write(path, len as u64)
    }

    /// After a write to a held file: cached pages are stale, and the write is
    /// counted and announced.
    fn object_written(obj: &FileObject, path: &Path, len: usize) {
        crate::mm::page_cache::invalidate_identity(obj.fs_id, obj.ino);
        super::quota::charge_bytes(0, 0, len as u64);
        super::notify::emit_modified(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
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
        Self::fallocate_resolved(&path, size)
    }

    /// [`fallocate`](Self::fallocate) on an already-resolved host path: an
    /// open handle's (`fs::handle::HandleFile`).
    ///
    /// # Errors
    ///
    /// As [`fallocate`](Self::fallocate).
    pub fn fallocate_resolved(path: &Path, size: u64) -> KernelResult<()> {
        check_writable(path)?;
        check_path_access(path, PathAccess::Write)?;
        let (fs, fs_id, _opts, relative) = resolve_mount(path)?;
        let mut guard = fs.lock();
        if let Some(meta) = attr_meta(&mut **guard, &relative, true) {
            attr_policy::may_allocate(meta.attributes)?;
        }
        // Space past the end is growth, as Linux's `shmem_fallocate` counts
        // it under `F_SEAL_GROW`, whether or not the size moves.
        if let Some((seals, meta)) = seals_at(&mut **guard, fs_id, &relative, path) {
            super::sealing::may_resize(seals, meta.size, meta.size.max(size))?;
        }
        guard.fallocate(&relative, size)
    }

    /// Rename or move a file or directory.
    ///
    /// Both paths must be on the same mount point.
    ///
    /// See [`RenameMode`] for the three semantics this family offers and
    /// [`rename_at_pinned`](Self::rename_at_pinned) for the handle-resolved
    /// form.
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
    /// file into `to` between the check and the rename.  The guarantee is now
    /// unconditional: a cross-mount rename is refused with
    /// [`KernelError::CrossDevice`] before anything is examined, so there is no
    /// second path with a weaker promise.  (There used to be — a copy+delete
    /// convenience that could not be atomic and kept a best-effort pre-check.)
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
            let (dest_inval, displaced) = {
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
                // After the no-replace check: `EEXIST` before `EPERM`, as
                // Linux's lookup precedes `vfs_rename`.
                guard_rename(&mut **guard, &rel_from, &rel_to, false)?;
                // A replacing rename unlinks the destination's existing inode
                // (whose number may later be reused); capture its identity
                // before the rename so we can drop its cached pages.  The
                // source's identity is unchanged (same inode, new name), so its
                // cached pages stay valid.
                let id = cache_identity(&mut guard, fs_id_to, &rel_to);
                // The same removal, for the state kept about files outside the
                // filesystem (`super::perfile`).
                let displaced = displaced_object(&mut guard, fs_id_to, &rel_from, &rel_to);
                guard.rename(&rel_from, &rel_to)?;
                (id, displaced)
            };
            if let Some((fs_id, ino)) = dest_inval {
                crate::mm::page_cache::invalidate_identity(fs_id, ino);
            }
            // The replaced file first, so that its state is gone before the
            // moved file's names arrive where its were.
            if let Some(unlinked) = displaced {
                super::perfile::object_unlinked(unlinked, &to);
            }
            super::perfile::names_moved(&from, &to);
        } else {
            // Cross-mount: refuse with `CrossDevice` (-> `EXDEV`), which is
            // what POSIX defines and every other Unix returns.
            //
            // This branch used to copy the bytes, delete the original and
            // report success. That was a deliberate convenience, and it was
            // wrong in a way that got worse the more of userland existed:
            //
            // * It made `mv`'s cross-filesystem path dead code on the target.
            //   Several hundred lines of `userspace/coreutils` exist precisely
            //   to handle this case and are certified against GNU coreutils on
            //   every harness run, but on SlateOS itself they never ran -- the
            //   kernel had already done something `mv` never saw.
            // * It could not be atomic. The sequence is a `stat`, a `copy`, a
            //   `set_permissions`, a `set_owner` and a `remove`, each taking
            //   and releasing its own lock, so `rename_noreplace`'s guarantee
            //   degraded silently to best-effort here while holding on the
            //   same-mount path.
            // * It was already refused by the neighbours. `rename_exchange`
            //   returns `CrossDevice`, `link` returns `CrossDevice`, and
            //   `SYS_FS_RENAMEAT_PINNED` returns `CrossDevice` for exactly
            //   this reason (`design-decisions.md` sec 666: a pin cannot span
            //   a copy). The stricter call was the honest one and the ordinary
            //   call was the one quietly doing something else.
            //
            // Requested by lane B in
            // `requests/b-a-rename-across-a-mount-copies-instead-of-answering-exdev.md`,
            // which was filed blocked and unblocked first: making the kernel
            // correct while `mv` still refused hard-linked sets and directory
            // trees would have turned working moves into failures. Both gaps
            // closed 2026-09-01 and 2026-09-03, so `mv` now handles every
            // shape across a boundary that it handles within one.
            //
            // Note this also *widens* what succeeds, via `mv`: a cross-mount
            // directory move used to be `NotSupported` outright, because the
            // kernel would have had to recurse. `mv` recurses.
            return Err(KernelError::CrossDevice);
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
            // Each name leaves its directory for the other's: both are
            // `may_delete`'s, as Linux asks of an exchange.
            let mut guard = fs_b.lock();
            guard_rename(&mut **guard, &rel_a, &rel_b, true)?;
            guard.rename_exchange(&rel_a, &rel_b)?;
        }
        // Both files keep their identities and swap names (`super::perfile`).
        super::perfile::names_exchanged(&a, &b);

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

    /// Every mount point with its device number (`st_dev`'s minor), for
    /// `/proc/<pid>/mountinfo`, whose `major:minor` field is the `st_dev` of
    /// the files under it.
    ///
    /// Locks no filesystem, as [`Self::mounts`].
    pub fn mounts_with_dev() -> Vec<(PathBuf, String, MountOptions, u32)> {
        let vfs = VFS.lock();
        vfs.mounts
            .iter()
            .map(|mp| {
                (
                    mp.path.clone(),
                    mp.fs_type.clone(),
                    mp.options,
                    dev_of(mp.fs_id),
                )
            })
            .collect()
    }

    /// Get mount options for the filesystem containing `path`.
    pub fn mount_options(path: impl AsRef<Path>) -> KernelResult<MountOptions> {
        let path = path.as_ref();
        let mut vfs = VFS.lock();
        let (mp, _) = find_mount(&mut vfs, path)?;
        Ok(mp.options)
    }

    /// The mounted filesystem an already-resolved host `path` is on: where it
    /// is mounted, its type, its `fs_id`, whether it is read-only, and the
    /// volume's own UUID ([`FileSystem::volume_uuid`]).
    ///
    /// The mount table's lock is released before the filesystem's is taken
    /// for the UUID, as everywhere else ([`MountPoint::fs_type`]).
    ///
    /// # Errors
    ///
    /// `NotFound` when nothing is mounted over `path` (only before the root
    /// is).
    pub fn volume_of(path: impl AsRef<Path>) -> KernelResult<VolumeInfo> {
        let path = path.as_ref();
        let (fs, info) = {
            let vfs = VFS.lock();
            // The longest mount path over `path`, the later of two equal ones
            // -- the one that shadows -- as `check_writable` chooses.
            let mut best: Option<&MountPoint> = None;
            for mp in &vfs.mounts {
                if mount_matches(&mp.path, path)
                    && best.is_none_or(|b| mp.path.len() >= b.path.len())
                {
                    best = Some(mp);
                }
            }
            let mp = best.ok_or(KernelError::NotFound)?;
            (
                Arc::clone(&mp.fs),
                VolumeInfo {
                    mount: mp.path.clone(),
                    fs_type: mp.fs_type.clone(),
                    fs_id: mp.fs_id,
                    read_only: mp.options.read_only,
                    uuid: None,
                },
            )
        };
        let uuid = fs.lock().volume_uuid();
        Ok(VolumeInfo { uuid, ..info })
    }

    /// The mounted filesystem `path` is on, with `path` resolved as the caller
    /// resolves it (its namespace; every symlink followed) and the caller's
    /// permission gate asked whether it may look (`Metadata`): for a request
    /// that names a volume by any path on it, as `statfs` does.
    ///
    /// # Errors
    ///
    /// As resolution and the gate fail.
    pub fn volume_named_by(path: impl AsRef<Path>) -> KernelResult<VolumeInfo> {
        let host = Self::resolve_follow(path.as_ref())?;
        check_path_access(&host, PathAccess::Metadata)?;
        Self::volume_of(&host)
    }

    /// What a deferred delete or rename of `path` would act on
    /// (`fs::deferred_ops`): the name as the host spells it, with the final
    /// component not followed -- a deferred delete removes the name it was
    /// given, as `unlink` does -- the file's identity, kind and `chattr`
    /// marks, its directory and that directory's marks, and the volume.
    ///
    /// In the caller's context: its namespace resolves the path, and its
    /// permission gate decides whether it may look (`Metadata`).
    ///
    /// # Errors
    ///
    /// `NotFound` for a missing name; `DeviceBusy` for a mount point, which no
    /// delete or rename could act on while it is one; `NotSupported` on a
    /// filesystem without stable inode numbers, where nothing could say later
    /// that the file is still the same one; the gate's error.
    pub fn deferral_target(path: impl AsRef<Path>) -> KernelResult<DeferralTarget> {
        let host = Self::resolve_no_follow(path.as_ref())?;
        check_path_access(&host, PathAccess::Metadata)?;
        let volume = Self::volume_of(&host)?;
        let (fs, fs_id, _opts, relative) = resolve_mount(&host)?;
        if relative.as_bytes() == b"/" {
            return Err(KernelError::DeviceBusy);
        }
        let (meta, parent_attributes) = {
            let mut guard = fs.lock();
            let meta = guard.lmetadata(&relative)?;
            let rel_parent = relative.parent().unwrap_or(Path::new("/"));
            // A directory whose marks cannot be read has none to refuse by;
            // the operation itself is checked again when it runs.
            let parent_attributes = guard
                .metadata(rel_parent)
                .map_or(FileAttr::NONE, |m| m.attributes);
            (meta, parent_attributes)
        };
        if meta.ino == 0 {
            return Err(KernelError::NotSupported);
        }
        let parent = host.parent().unwrap_or(Path::new("/")).to_path_buf();
        Ok(DeferralTarget {
            path: host,
            parent,
            id: FileId {
                fs_id,
                ino: meta.ino,
            },
            entry_type: meta.entry_type,
            attributes: meta.attributes,
            parent_attributes,
            volume,
        })
    }

    /// The name a deferred rename would move its file to, as the host spells
    /// it (the caller's namespace applied, the final component not followed
    /// and not required to exist), and the `fs_id` of the filesystem it is on.
    ///
    /// The caller's gate is asked whether it may look (`Metadata`), as for
    /// the target: a name under a directory its capability tags refuse it is
    /// not one it may learn anything about.
    ///
    /// # Errors
    ///
    /// `NotFound` when a directory on the way is missing; `DeviceBusy` for a
    /// mount point; the gate's error.
    pub fn deferral_destination(path: impl AsRef<Path>) -> KernelResult<(PathBuf, u64)> {
        let host = Self::resolve_no_follow(path.as_ref())?;
        check_path_access(&host, PathAccess::Metadata)?;
        let (_fs, fs_id, _opts, relative) = resolve_mount(&host)?;
        if relative.as_bytes() == b"/" {
            return Err(KernelError::DeviceBusy);
        }
        Ok((host, fs_id))
    }

    /// Re-mount a filesystem with new options (e.g., `remount,ro`).
    pub fn remount(mount_path: impl AsRef<Path>, options: MountOptions) -> KernelResult<()> {
        // Same normalisation as `mount`/`unmount`: identify the mount by its
        // canonical spelling, not by the caller's.
        let mount_path = &normalize_mount_path(mount_path.as_ref());
        let became_writable = {
            let mut vfs = VFS.lock();
            let Some(mp) = vfs
                .mounts
                .iter_mut()
                .find(|mp| mp.path.as_path() == mount_path.as_path())
            else {
                return Err(KernelError::NotFound);
            };
            crate::serial_println!(
                "[vfs] Remounted '{}' with options: {}",
                mount_path.display(),
                options.to_string(),
            );
            let became_writable = mp.options.read_only && !options.read_only;
            mp.options = options;
            became_writable
        };
        // A volume that was read-only is the commonest reason an operation
        // was deferred, and what clears it is this, not a mount: replay its
        // queue now, as a mount does (`fs::deferred_ops`). Best-effort, as
        // there: the remount has happened whatever the replay finds.
        if became_writable {
            super::deferred_ops::replay_on_mount(mount_path);
        }
        Ok(())
    }

    /// Find mount-point names that are direct children of `dir_path`, each
    /// paired with a handle to the filesystem mounted there.
    ///
    /// For example, if `dir_path` is `"/"` and there are mounts at
    /// `"/tmp"` and `"/mnt/usb"`, this returns `["tmp"]` — only the
    /// immediate child, not nested mounts.
    ///
    /// The handle is cloned out of the mount table rather than re-resolved
    /// later by path, because this scan is already looking at the very entry
    /// a later `resolve_mount` would find: [`Vfs::mount`] refuses a duplicate
    /// mount path, so each path here maps to exactly one entry and the two
    /// are equivalent.  Doing it here turns a per-submount longest-prefix
    /// scan of the whole mount table into an `Arc` clone — see the
    /// `vfs_readdir_mp_*` benchmarks for what that was costing.
    fn submount_children(vfs: &VfsInner, dir_path: &Path) -> Vec<(PathBuf, MountedFs)> {
        let mut names = Vec::new();

        for mp in &vfs.mounts {
            // `strip_prefix` is component-aligned, so a mount at `/tmpfile`
            // is not treated as living under `/tmp`.  A tail of exactly one
            // component is a direct child; an empty tail is the mount that
            // *is* this directory (skipped), and a longer tail is a nested
            // mount that some intermediate directory owns, not this one.
            if let Some(tail) = mp.path.strip_prefix(dir_path) {
                if tail.components().count() == 1 {
                    names.push((tail.to_path_buf(), Arc::clone(&mp.fs)));
                }
            }
        }

        names
    }

    /// The inode a synthesised submount directory entry should report.
    ///
    /// A mount point that the underlying filesystem has no directory for is
    /// still a real object to a caller: `stat`ting it resolves *through* the
    /// mount and answers with the mounted filesystem's root inode.  A
    /// listing that reported 0 for the same name would therefore disagree
    /// with `stat` on an entry that `stat` can answer for — which is exactly
    /// the `d_ino` vs `st_ino` mismatch this field exists to prevent.
    ///
    /// Returns 0 when the mounted filesystem has no stable identity to
    /// report, or when its root cannot be statted at all; 0 is the field's
    /// documented "not available", and it is what `stat` would report in the
    /// first of those cases anyway.  Errors are deliberately swallowed
    /// rather than propagated: a filesystem that cannot answer for its own
    /// root must not make listing the directory *above* it fail.
    ///
    /// `fs` is the handle [`submount_children`] cloned out of the mount
    /// table, so this is `metadata_resolved`'s body with the path resolution
    /// already done: for a path that *is* a mount point, `find_mount` yields
    /// the relative path `/`, which is what is statted here.
    ///
    /// Callers must not hold the VFS lock — this takes the mounted
    /// filesystem's own lock, and the §43 ordering is VFS-lock-first, so
    /// taking them in that order from a caller that already holds the VFS
    /// lock would invert it.  (The concrete deadlock this avoids is spelled
    /// out on [`MountPoint::fs_type`]: `readdir("/proc")` holds the procfs
    /// mutex while `gen_mounts` asks the VFS for the mount table.)
    fn submount_root_ino(fs: &MountedFs) -> u64 {
        fs.lock().metadata(Path::new("/")).map_or(0, |m| m.ino)
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
        let (fs, fs_id, _opts, relative) = resolve_mount(path)?;
        let mut meta = fs.lock().metadata(&relative)?;
        meta.dev = dev_of(fs_id);
        Ok(meta)
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

    // -------------------------------------------------------------------
    // Fd-relative primitives that verify a pinned directory identity
    //
    // These exist because the `*at` family cannot be made correct on top of
    // the path-based entry points above.  `openat`/`unlinkat`/`fstatat` and
    // the rest currently recover the *text* of the dirfd and concatenate, so
    // whatever the name leads to at the moment of the call is what gets
    // operated on -- a renamed or symlink-swapped directory redirects the
    // operation silently.  See design-decisions.md §647.
    // -------------------------------------------------------------------

    /// Capture the identity of an already-resolved directory path, for
    /// storing in a handle.
    ///
    /// Returns a [`PinnedDir`] whose `id` is `None` when the filesystem has
    /// no stable per-object identity (`ino == 0`).  That is reported rather
    /// than faked, so a later operation can tell "this directory is still the
    /// one you opened" apart from "this filesystem cannot answer that".
    pub fn pin_dir(path: impl AsRef<Path>) -> KernelResult<PinnedDir> {
        let path = path.as_ref();
        // `Metadata`, because that is exactly what this reads. The caller is
        // usually `allocate_dir_handle`, which has already gated the open --
        // but "my caller checked" is the assumption the gate exists to refuse,
        // and `pin_dir` is `pub`, so a future caller need not have gated
        // anything. (Until §648 that caller already existed: `pinned_dir_arg`
        // pinned a cwd that was never opened through this path at all. That
        // branch is gone, which removes the instance and not the reason.)
        // Without this check, the entry-type and inode of any path are readable
        // by a caller who may not stat it, which is a disclosure however small.
        check_path_access(path, PathAccess::Metadata)?;
        let (fs, fs_id, _opts, relative) = resolve_mount(path)?;
        let meta = fs.lock().lmetadata(&relative)?;
        if meta.entry_type != EntryType::Directory {
            return Err(KernelError::NotADirectory);
        }
        let id = if meta.ino == 0 {
            None
        } else {
            Some(FileId {
                fs_id,
                ino: meta.ino,
            })
        };
        Ok(PinnedDir {
            path: path.to_path_buf(),
            id,
        })
    }

    /// Whether `dir` carries an identity that can actually be checked.
    ///
    /// `false` means the filesystem behind the handle has no stable inode
    /// numbers, so the anti-TOCTOU guarantee is unavailable there — not that
    /// it failed.  A caller that requires the guarantee must ask this and
    /// refuse; one that does not may proceed with the single-component
    /// containment alone.
    #[must_use]
    pub fn pinned_dir_is_verifiable(dir: &PinnedDir) -> bool {
        dir.id.is_some()
    }

    /// Remove `name` from the directory `dir` denotes, refusing if `dir` no
    /// longer denotes the directory it was opened on.
    ///
    /// `remove_dir` selects `rmdir` semantics (`AT_REMOVEDIR`) over `unlink`.
    /// `name` must be a single component: see [`check_at_name`].
    ///
    /// The identity check and the removal happen under **one** hold of the
    /// filesystem lock, so no rename can slip between them.  A prior check
    /// runs before the policy checks as well, so that a handle already stale
    /// on entry cannot induce a side effect (notably the auto-version
    /// snapshot, which reads file content) on a file the caller never named.
    pub fn unlink_at_pinned(dir: &PinnedDir, name: &[u8], remove_dir: bool) -> KernelResult<()> {
        check_at_name(name)?;

        // Pass 1: refuse an already-stale handle *before* anything with a
        // side effect runs.  Pass 2 below is what makes the removal atomic;
        // this one is what keeps the steps in between honest.
        {
            let (fs, fs_id, _opts, dir_rel) = resolve_mount(&dir.path)?;
            let mut guard = fs.lock();
            verify_pinned(&mut guard, fs_id, &dir_rel, dir)?;
        }

        // From here the path is known to have denoted the pinned directory a
        // moment ago, and `name` is known to be a single component, so the
        // join names the object the caller meant.  The policy checks below
        // are the same ones `remove`/`rmdir` run, in the same order.
        let child = dir.path.join(name);
        crate::ipc::namespace::check_writable(&child)?;
        check_path_access(&child, PathAccess::Write)?;
        check_writable(&child)?;
        super::intercept::pre_delete(&child)?;

        let file_size = if remove_dir {
            0
        } else {
            Self::stat(&child).map(|s| s.size).unwrap_or(0)
        };
        if !remove_dir {
            super::history::try_auto_record(&child);
        }

        let (cache_inval, unlinked) = {
            let (fs, fs_id, _opts, dir_rel) = resolve_mount(&dir.path)?;
            let mut guard = fs.lock();
            // Pass 2, under the same guard as the removal itself.  Anything
            // that moved the directory since pass 1 is caught here, and
            // nothing can move it between here and the `remove` below.
            verify_pinned(&mut guard, fs_id, &dir_rel, dir)?;
            let child_rel = dir_rel.join(name);
            // Before the directory check below: Linux's `may_delete` refuses
            // an immutable name before it looks at what kind of name it is.
            guard_delete(&mut **guard, &child_rel)?;
            if remove_dir {
                // See `remove` for why this is read before the removal.
                let unlinked = unlinked_object(&mut guard, fs_id, &child_rel);
                guard.rmdir(&child_rel)?;
                (None, unlinked)
            } else {
                // `unlink` never follows a trailing symlink, and must not
                // silently swallow a directory: without this, `unlinkat`
                // without `AT_REMOVEDIR` would delegate the decision to
                // whatever each filesystem's `remove` happens to do.
                if guard.lstat(&child_rel)?.entry_type == EntryType::Directory {
                    return Err(KernelError::IsADirectory);
                }
                let id = cache_identity(&mut guard, fs_id, &child_rel);
                let unlinked = unlinked_object(&mut guard, fs_id, &child_rel);
                guard.remove(&child_rel)?;
                (id, unlinked)
            }
        };

        if let Some((fs_id, ino)) = cache_inval {
            crate::mm::page_cache::invalidate_identity(fs_id, ino);
        }
        super::perfile::name_removed(unlinked, &child);
        if file_size > 0 {
            super::quota::release_bytes(0, 0, file_size);
        }
        super::quota::release_inode(0, 0);
        VFS_DCACHE.lock().invalidate_prefix(&child);
        if remove_dir {
            super::notify::emit_deleted_dir(&child);
        } else {
            super::notify::emit_deleted(&child);
        }
        super::index::on_file_deleted(&child);
        super::journal::record(super::journal::JournalEventType::Deleted, &child);
        super::audit::log_ok(
            if remove_dir {
                super::audit::AuditOp::Rmdir
            } else {
                super::audit::AuditOp::Delete
            },
            0,
            &child,
        );
        Ok(())
    }

    /// Stat `name` within the directory `dir` denotes, refusing if `dir` no
    /// longer denotes the directory it was opened on.
    ///
    /// `no_follow` selects `lstat` semantics (`AT_SYMLINK_NOFOLLOW`).
    /// As with [`unlink_at_pinned`](Self::unlink_at_pinned), the check and
    /// the read share one hold of the filesystem lock.
    pub fn metadata_at_pinned(
        dir: &PinnedDir,
        name: &[u8],
        no_follow: bool,
    ) -> KernelResult<FileMeta> {
        check_at_name(name)?;
        let child = dir.path.join(name);
        check_path_access(&child, PathAccess::Metadata)?;
        let (fs, fs_id, _opts, dir_rel) = resolve_mount(&dir.path)?;
        let mut guard = fs.lock();
        verify_pinned(&mut guard, fs_id, &dir_rel, dir)?;
        let child_rel = dir_rel.join(name);
        let mut meta = if no_follow {
            guard.lmetadata(&child_rel)?
        } else {
            guard.metadata(&child_rel)?
        };
        drop(guard);
        meta.dev = dev_of(fs_id);
        Ok(meta)
    }

    /// Change the permission bits of `name` within the directory a handle was
    /// opened on, refusing if the handle no longer denotes that directory.
    ///
    /// This is the entry point `fchmodat` needs, and it is the highest-value
    /// member of the pinned family after `unlink`. `chmod -R` walking a tree it
    /// does not control is the classic privilege-escalation shape: an attacker
    /// who can swap a directory for a symlink mid-walk gets the mode applied to
    /// whatever the link names. A path-based `fchmodat` re-derives the directory
    /// by name on every entry and so cannot tell that it happened; this one
    /// verifies the handle still denotes the directory it was opened on and
    /// fails with `StaleHandle` if it does not.
    ///
    /// `no_follow` selects `AT_SYMLINK_NOFOLLOW`: the mode lands on the link
    /// inode itself rather than on its target. Following is the POSIX default
    /// and is what `chmod` without `-h` wants; the pin protects the *directory*
    /// either way, which is where the race lives.
    ///
    /// What the pin does *not* protect is a followed symlink's target: asking
    /// to follow is asking to leave the directory. So the sandbox and
    /// read-only checks are run against the resolved object rather than the
    /// name, which is what stops a link inside the pinned directory from
    /// carrying a chmod to somewhere policy forbids.
    ///
    /// Verified twice, for the reason
    /// [`unlink_at_pinned`](Self::unlink_at_pinned) is: pass 1 refuses an
    /// already-stale handle before any policy check runs, and pass 2 happens
    /// under the same lock as the change itself, so nothing can move the
    /// directory between the check and the write.
    pub fn set_permissions_at_pinned(
        dir: &PinnedDir,
        name: &[u8],
        permissions: u16,
        no_follow: bool,
    ) -> KernelResult<()> {
        check_at_name(name)?;

        // Pass 1: an already-stale handle is refused before anything with a
        // side effect, and before the policy checks below can report on a
        // directory the caller no longer holds.
        {
            let (fs, fs_id, _opts, dir_rel) = resolve_mount(&dir.path)?;
            let mut guard = fs.lock();
            verify_pinned(&mut guard, fs_id, &dir_rel, dir)?;
        }

        // `name` is known to be a single component, so this join names the
        // object the caller meant.  The namespace check applies to the name as
        // given, before any symlink is followed, so a link cannot be used to
        // reach out of the caller's namespace.
        let child = dir.path.join(name);
        crate::ipc::namespace::check_writable(&child)?;

        // The remaining checks must describe the object that actually changes,
        // not the name used to reach it.  Without `no_follow`, chmod follows a
        // final symlink; checking `child` would evaluate the sandbox policy and
        // the read-only flag where the *link* lives while the mode landed on
        // the target.  `Vfs::set_permissions` resolves before checking for
        // exactly this reason, and resolving the same way here is what keeps
        // the pinned and path-based routes from enforcing different policies.
        // chmod is a metadata write, so the gate is `PathAccess::Metadata` and
        // a writable mount, not `PathAccess::Write`.
        let target = if no_follow {
            Self::resolve_no_follow(&child)?
        } else {
            Self::resolve_follow(&child)?
        };
        check_writable(&target)?;
        check_path_access(&target, PathAccess::Metadata)?;

        if target == child {
            // The ordinary case: `name` is not a symlink, so the object being
            // changed is inside the directory the pin verified.  Do it under
            // the pin's own lock, verified a second time, so nothing can move
            // the directory between the check and the write.
            let (fs, fs_id, _opts, dir_rel) = resolve_mount(&dir.path)?;
            let mut guard = fs.lock();
            verify_pinned(&mut guard, fs_id, &dir_rel, dir)?;
            let child_rel = dir_rel.join(name);
            // `no_follow` unconditionally, even when the caller asked to
            // follow.  The resolution above established that `name` is not a
            // symlink; if it *became* one in the window since, a following
            // write would land the mode on a target that was never checked.
            // For a non-symlink the two calls are the same operation, so this
            // costs nothing and closes that window.
            guard_metadata(&mut **guard, &child_rel, false)?;
            seal_mode_guard(&mut **guard, fs_id, &child_rel, &child, permissions)?;
            guard.set_permissions_no_follow(&child_rel, permissions)?;
            acl_follow_chmod_at(&mut **guard, fs_id, &child_rel, &child, false, permissions);
        } else {
            // `name` is a symlink and the caller asked to follow it, so the
            // object being chmod-ed is outside the pinned directory and
            // possibly on another mount.  Pinning cannot cover this: following
            // is an explicit request to leave, and the residual race on the
            // link's own contents is the one plain `chmod` has too.  Operating
            // through the target's own mount also avoids holding two
            // filesystem locks at once.
            let (fs, fs_id, _opts, relative) = resolve_mount(&target)?;
            let mut guard = fs.lock();
            guard_metadata(&mut **guard, &relative, !no_follow)?;
            seal_mode_guard(&mut **guard, fs_id, &relative, &target, permissions)?;
            if no_follow {
                guard.set_permissions_no_follow(&relative, permissions)?;
            } else {
                guard.set_permissions(&relative, permissions)?;
            }
            acl_follow_chmod_at(
                &mut **guard,
                fs_id,
                &relative,
                &target,
                !no_follow,
                permissions,
            );
        }

        super::notify::emit_metadata(&target);
        super::journal::record(super::journal::JournalEventType::Modified, &target);
        Ok(())
    }

    /// Create a directory named `name` inside the directory `dir` denotes,
    /// refusing if `dir` no longer denotes the directory it was opened on.
    ///
    /// This is the entry point `mkdirat` needs, and the first of the four the
    /// `cp -r` shape requires. A recursive copy re-derives its *destination*
    /// directory from that directory's text on every entry it writes, so a
    /// swap performed partway through the walk silently redirects the whole
    /// remainder of the tree — and unlike the source side, the destination is
    /// where new objects get created, so the redirect is a write primitive.
    /// Pinning the destination once and verifying it per operation is what
    /// removes the re-derivation, and with it the race and the repeated
    /// full-path walk.
    ///
    /// `mode` is treated exactly as [`mkdir_mode`](Self::mkdir_mode) treats
    /// it: **already umask-masked by the caller**, since the umask lives in
    /// the userspace POSIX layer, so the kernel stamps it as the final
    /// on-disk permission bits.
    ///
    /// The parent is *not* re-resolved by name here, deliberately. Re-resolving
    /// is the bug this replaces; the parent is established by identity instead,
    /// and `mkdir` never follows the final component, so `dir.path.join(name)`
    /// names the object the caller meant.
    ///
    /// Verified twice, for the reason
    /// [`unlink_at_pinned`](Self::unlink_at_pinned) is: pass 1 refuses an
    /// already-stale handle before any policy check or side effect runs, and
    /// pass 2 happens under the same filesystem lock as the creation itself.
    ///
    /// The create and the permission stamp share that one lock, which is a
    /// small improvement on the path-based route rather than a copy of it:
    /// `mkdir_mode` chmods in a second, separate acquisition, leaving a window
    /// in which the new directory is already visible carrying the filesystem's
    /// 0o755 default before the caller's (possibly much narrower) mode lands.
    /// Doing both under one hold closes it.
    pub fn mkdir_at_pinned(dir: &PinnedDir, name: &[u8], mode: u16) -> KernelResult<()> {
        check_at_name(name)?;

        // Pass 1: refuse an already-stale handle before anything with a side
        // effect runs, and before the policy checks below can report on a
        // directory the caller no longer holds.
        {
            let (fs, fs_id, _opts, dir_rel) = resolve_mount(&dir.path)?;
            let mut guard = fs.lock();
            verify_pinned(&mut guard, fs_id, &dir_rel, dir)?;
        }

        // Same checks the path-based `mkdir_mode` runs, in the same order.
        let child = dir.path.join(name);
        crate::ipc::namespace::check_writable(&child)?;
        check_path_access(&child, PathAccess::Write)?;
        check_writable(&child)?;
        super::intercept::pre_mkdir(&child)?;
        enforce_quota_create(&child)?;

        // `0o1777`, the same mask `mkdir_mode` applies — see its doc and §663.
        // One operation with two masks depending on which route ran is worse
        // than either mask, which is the argument that decided `link`'s error
        // code the same week.
        let requested = mode & 0o1777;
        let creator = creator_ids();
        let umask = creator_umask();
        {
            let (fs, fs_id, _opts, dir_rel) = resolve_mount(&dir.path)?;
            let mut guard = fs.lock();
            // Pass 2, under the same guard as the creation itself. Anything
            // that moved the directory since pass 1 is caught here, and
            // nothing can move it between here and the `mkdir` below.
            verify_pinned(&mut guard, fs_id, &dir_rel, dir)?;
            let child_rel = dir_rel.join(name);
            guard_create(&mut **guard, &child_rel)?;
            // As `mkdir_mode`: the umask or the directory's default ACL. The
            // stamp is no-follow (`stamp_new_mode`), though what was just
            // created is a directory: under this lock the two forms are one
            // operation, and the no-follow one asserts more.
            let (perm, acls) = new_node_mode(
                &mut **guard,
                fs_id,
                &dir_rel,
                &dir.path,
                requested,
                umask,
                true,
            );
            guard.mkdir(&child_rel)?;
            stamp_new_mode(&mut **guard, &child_rel, perm, Self::DEFAULT_DIR_MODE)?;
            // Its creator's, after the mode as `mkdir_mode` orders them: a
            // set-group-ID parent's inheritance adds a bit the stamp above
            // would otherwise overwrite. Root's, whoever made it, until
            // 2026-10-02 (`A-PINNED-CREATES-ARE-OWNED-BY-ROOT`).
            init_new_owner(&mut **guard, &child_rel, creator, true)?;
            acls.store(&mut **guard, fs_id, None, Some(&child_rel), &child)?;
        }

        super::quota::charge_inode(0, 0);
        // A new directory invalidates negative cache entries that claimed this
        // path (or children below it) did not exist. Positive entries are
        // unaffected — existing resolutions remain valid.
        VFS_DCACHE.lock().invalidate_negative_prefix(&child);
        super::notify::emit_created_dir(&child);
        super::index::on_file_changed(&child);
        super::journal::record(super::journal::JournalEventType::Created, &child);
        super::audit::log_ok(super::audit::AuditOp::Mkdir, 0, &child);
        Ok(())
    }

    /// Create a symbolic link named `name` inside the directory `dir` denotes,
    /// refusing if `dir` no longer denotes the directory it was opened on.
    ///
    /// This is the entry point `symlinkat` needs. `name` is the new link;
    /// `target` is the text it will contain.
    ///
    /// **Only `name` is a single component — `target` is not, and must not be.**
    /// [`check_at_name`] applies to the name being created inside the pinned
    /// directory, because that is what the pin's containment guarantee covers.
    /// A symlink target is arbitrary text that is stored verbatim and resolved
    /// only when something later traverses the link; it may be relative, may
    /// be absolute, may contain `..`, and may name something that does not
    /// exist. Refusing those would not make anything safer — it would only
    /// make `symlinkat` unable to reproduce the links `cp -r` is copying —
    /// and it would be a check applied at the wrong time in any case, since
    /// what the target resolves to is decided at traversal, not here. The
    /// traversal-time checks are what govern the target, exactly as they do
    /// for the path-based [`symlink`](Self::symlink).
    ///
    /// Verified twice, for the reason
    /// [`unlink_at_pinned`](Self::unlink_at_pinned) is.
    pub fn symlink_at_pinned(
        dir: &PinnedDir,
        name: &[u8],
        target: impl AsRef<Path>,
    ) -> KernelResult<()> {
        let target = target.as_ref();
        check_at_name(name)?;

        // Pass 1: an already-stale handle is refused before the intercept
        // hook — which is caller-supplied code that can observe being called —
        // runs on a directory the caller no longer holds.
        {
            let (fs, fs_id, _opts, dir_rel) = resolve_mount(&dir.path)?;
            let mut guard = fs.lock();
            verify_pinned(&mut guard, fs_id, &dir_rel, dir)?;
        }

        // Same checks the path-based `symlink` runs, in the same order.
        let child = dir.path.join(name);
        crate::ipc::namespace::check_writable(&child)?;
        check_writable(&child)?;
        check_path_access(&child, PathAccess::Write)?;
        super::intercept::pre_check(super::intercept::FsOp::Symlink, &child, Some(target))?;
        enforce_quota_create(&child)?;
        let creator = creator_ids();

        {
            let (fs, fs_id, _opts, dir_rel) = resolve_mount(&dir.path)?;
            let mut guard = fs.lock();
            // Pass 2, under the same guard as the creation.
            verify_pinned(&mut guard, fs_id, &dir_rel, dir)?;
            let child_rel = dir_rel.join(name);
            guard_create(&mut **guard, &child_rel)?;
            guard.symlink(&child_rel, target)?;
            // Its creator's, as the path-based `symlink` makes one. It was
            // root's, whoever made it, until 2026-10-02
            // (`A-PINNED-CREATES-ARE-OWNED-BY-ROOT`).
            init_new_owner(&mut **guard, &child_rel, creator, false)?;
        }

        super::quota::charge_inode(0, 0);
        // A new symlink can change how any path *through* it resolves, so the
        // whole parent prefix goes, not just the negative entries. The parent
        // is the pinned directory itself — no `parent()` fallback is needed
        // here, because a single-component name always has one.
        VFS_DCACHE.lock().invalidate_prefix(&dir.path);
        super::notify::emit_created(&child);
        super::index::on_file_changed(&child);
        super::journal::record(super::journal::JournalEventType::Created, &child);
        super::audit::log_ok(super::audit::AuditOp::Symlink, 0, &child);
        Ok(())
    }

    /// Set the timestamps of `name` within the directory `dir` denotes,
    /// refusing if `dir` no longer denotes the directory it was opened on.
    ///
    /// This is the entry point `utimensat` needs — the last of the four the
    /// `cp -r` shape requires, and the one that runs on *every* copied entry
    /// rather than once per directory, since preserving mtime is what `cp -p`
    /// and every archive extractor do last on each file.
    ///
    /// A zero leaves that timestamp unchanged, matching
    /// [`set_times`](Self::set_times). `no_follow` selects
    /// `AT_SYMLINK_NOFOLLOW`, stamping the link inode itself.
    ///
    /// As in [`set_permissions_at_pinned`](Self::set_permissions_at_pinned),
    /// the pin protects the *directory*, and asking to follow is asking to
    /// leave it: when `name` turns out to be a symlink and the caller did not
    /// pass `no_follow`, the object being stamped is outside the pinned
    /// directory and possibly on another mount, so the policy checks are run
    /// against the resolved target and the write goes through the target's own
    /// mount. That residual race on the link's own contents is the one plain
    /// `utimensat` has too; what the pin removes is the far larger race on the
    /// directory.
    pub fn set_times_at_pinned(
        dir: &PinnedDir,
        name: &[u8],
        accessed_ns: Timestamp,
        modified_ns: Timestamp,
        no_follow: bool,
    ) -> KernelResult<()> {
        check_at_name(name)?;

        // Pass 1: refuse an already-stale handle before the policy checks can
        // report on a directory the caller no longer holds.
        {
            let (fs, fs_id, _opts, dir_rel) = resolve_mount(&dir.path)?;
            let mut guard = fs.lock();
            verify_pinned(&mut guard, fs_id, &dir_rel, dir)?;
        }

        // The namespace check applies to the name as given, before any symlink
        // is followed, so a link cannot be used to reach out of the caller's
        // namespace.
        let child = dir.path.join(name);
        crate::ipc::namespace::check_writable(&child)?;

        // The remaining checks must describe the object that actually changes,
        // not the name used to reach it — same reasoning as
        // `set_permissions_at_pinned`. Stamping times is a metadata write, so
        // the gate is `PathAccess::Metadata` on a writable mount.
        let target = if no_follow {
            Self::resolve_no_follow(&child)?
        } else {
            Self::resolve_follow(&child)?
        };
        check_writable(&target)?;
        check_path_access(&target, PathAccess::Metadata)?;
        // What the filesystem stores: `TIME_NOW` made now. The rule is
        // decided on the request (`guard_times`).
        let now = metadata_now_ns();
        let (accessed, modified) = (
            resolve_time(accessed_ns, now),
            resolve_time(modified_ns, now),
        );

        if target == child {
            // The ordinary case: `name` is not a symlink, so the object being
            // stamped is inside the directory the pin verified.
            let (fs, fs_id, _opts, dir_rel) = resolve_mount(&dir.path)?;
            let mut guard = fs.lock();
            verify_pinned(&mut guard, fs_id, &dir_rel, dir)?;
            let child_rel = dir_rel.join(name);
            // `no_follow` unconditionally: the resolution above established
            // that `name` is not a symlink, and if it *became* one in the
            // window since, a following write would stamp a target that was
            // never checked. For a non-symlink the two are the same call.
            guard_times(&mut **guard, &child_rel, false, accessed_ns, modified_ns)?;
            guard.set_times_no_follow(&child_rel, accessed, modified)?;
        } else {
            // `name` is a symlink and the caller asked to follow it. Operating
            // through the target's own mount also avoids holding two
            // filesystem locks at once.
            let (fs, _id, _opts, relative) = resolve_mount(&target)?;
            let mut guard = fs.lock();
            guard_times(
                &mut **guard,
                &relative,
                !no_follow,
                accessed_ns,
                modified_ns,
            )?;
            if no_follow {
                guard.set_times_no_follow(&relative, accessed, modified)?;
            } else {
                guard.set_times(&relative, accessed, modified)?;
            }
        }
        // No notify/journal — timestamp changes are metadata-only, matching
        // the path-based `set_times`.
        Ok(())
    }

    /// Hard-link `old_name` in the directory `old_dir` denotes to `new_name`
    /// in the directory `new_dir` denotes, refusing if either handle no longer
    /// denotes the directory it was opened on.
    ///
    /// This is the entry point `linkat` needs, and the first member of the
    /// family that pins **two** directories rather than one. `follow` selects
    /// `AT_SYMLINK_FOLLOW`: with it, a symlink at `old_name` is dereferenced
    /// and the link is made to the underlying object; without it — which is
    /// plain `link(2)`'s behaviour and `linkat`'s default — the symlink inode
    /// itself gains a name.
    ///
    /// Hard links cannot cross mounts, and that is enforced here as it is on
    /// the path route: two paths share a mount iff [`resolve_mount`] hands
    /// back the same per-mount handle. That rule has a useful consequence for
    /// this primitive — in every case where the link can succeed at all, both
    /// directories are behind *one* filesystem lock, so both pins can be
    /// verified and the link performed under a single hold. There is no lock
    /// ordering to get wrong, because there are never two locks.
    ///
    /// The exception is a followed symlink, where the source object may be on
    /// a different mount from `old_dir`. Following is a request to leave the
    /// pinned directory, so `old_dir`'s pin is then checked only by pass 1 —
    /// the same honest limit [`set_permissions_at_pinned`](Self::set_permissions_at_pinned)
    /// documents. `new_dir`, where the entry is actually created, is verified
    /// under the write's own lock either way.
    pub fn link_at_pinned(
        old_dir: &PinnedDir,
        old_name: &[u8],
        new_dir: &PinnedDir,
        new_name: &[u8],
        follow: bool,
    ) -> KernelResult<()> {
        check_at_name(old_name)?;
        check_at_name(new_name)?;

        // Pass 1, on both handles: a stale handle is refused before the
        // intercept hook runs and before quota is charged. Each guard is
        // taken and dropped in turn, so this cannot deadlock even when the
        // two directories are on the same filesystem.
        {
            let (fs, fs_id, _opts, dir_rel) = resolve_mount(&old_dir.path)?;
            let mut guard = fs.lock();
            verify_pinned(&mut guard, fs_id, &dir_rel, old_dir)?;
        }
        {
            let (fs, fs_id, _opts, dir_rel) = resolve_mount(&new_dir.path)?;
            let mut guard = fs.lock();
            verify_pinned(&mut guard, fs_id, &dir_rel, new_dir)?;
        }

        let old_child = old_dir.path.join(old_name);
        let new_child = new_dir.path.join(new_name);
        crate::ipc::namespace::check_writable(&new_child)?;

        // The source is resolved per `follow` so the intercept hook and the
        // same-mount rule below both see the object that will actually gain a
        // name, not the text used to reach it.
        let source = if follow {
            Self::resolve_follow(&old_child)?
        } else {
            Self::resolve_no_follow(&old_child)?
        };
        // `Read` on the source, `Write` on the destination — the asymmetry
        // `link_inner` explains, and the same checks it runs, so the two routes
        // enforce one policy rather than two.
        check_path_access(&source, PathAccess::Read)?;
        check_path_access(&new_child, PathAccess::Write)?;
        check_writable(&new_child)?;
        super::intercept::pre_check(super::intercept::FsOp::Link, &new_child, Some(&source))?;
        enforce_quota_create(&new_child)?;

        {
            // Every mount-table lookup happens before any filesystem guard is
            // taken, so no ordering between the VFS lock and a filesystem lock
            // can arise.
            let (fs_old, old_fs_id, _opts, old_dir_rel) = resolve_mount(&old_dir.path)?;
            let (fs_src, _src_id, _opts, src_rel) = resolve_mount(&source)?;
            let (fs_new, new_fs_id, _opts, new_dir_rel) = resolve_mount(&new_dir.path)?;
            if !Arc::ptr_eq(&fs_src, &fs_new) {
                // POSIX names this exact case: `link()` gives `[EXDEV]` for
                // "the link named by path2 and the file named by path1 are on
                // different file systems and the implementation does not
                // support links between file systems".  This returned
                // `InvalidArgument` → `EINVAL` until 2026-08-31, which is not
                // a near-miss but a different statement — `ln` printed
                // "Invalid argument" where GNU prints "Invalid cross-device
                // link", and nothing branched on it, which is why nothing
                // caught it.  Both this route and the path-based `link` were
                // changed together: one operation with two error codes
                // depending on which route ran is worse than either code.
                return Err(KernelError::CrossDevice);
            }
            let new_rel = new_dir_rel.join(new_name);

            let mut guard = fs_new.lock();
            // Pass 2 on the destination, under the same guard as the creation.
            verify_pinned(&mut guard, new_fs_id, &new_dir_rel, new_dir)?;
            // And on the source directory too, whenever it is reachable
            // through this same guard. It always is when the source is still
            // inside it; only a followed symlink can put it elsewhere.
            if Arc::ptr_eq(&fs_old, &fs_new) {
                verify_pinned(&mut guard, old_fs_id, &old_dir_rel, old_dir)?;
            }

            if source == old_child {
                // `old_name` is not a symlink, so the source is inside the
                // verified directory. `link_no_follow` unconditionally, even
                // when the caller asked to follow: the resolution above
                // established there is nothing to follow, and if the name
                // *became* a symlink in the window since, a following link
                // would name an object that was never checked. For a
                // non-symlink the two are the same operation.
                let old_rel = old_dir_rel.join(old_name);
                guard_link(&mut **guard, &old_rel, false, &new_rel)?;
                guard.link_no_follow(&old_rel, &new_rel)?;
            } else {
                // A symlink was followed out of the pinned directory; the
                // source is wherever it resolved to, on this same mount.
                guard_link(&mut **guard, &src_rel, true, &new_rel)?;
                guard.link(&src_rel, &new_rel)?;
            }
        }

        super::quota::charge_inode(0, 0);
        VFS_DCACHE.lock().invalidate_negative_prefix(&new_child);
        super::notify::emit_created(&new_child);
        super::index::on_file_changed(&new_child);
        super::journal::record(super::journal::JournalEventType::Created, &new_child);
        super::audit::log_ok(super::audit::AuditOp::Link, 0, &new_child);
        Ok(())
    }

    /// Rename `old_name` in the directory `old_dir` denotes to `new_name` in
    /// the directory `new_dir` denotes, refusing if either handle no longer
    /// denotes the directory it was opened on.
    ///
    /// This is the entry point `renameat`/`renameat2` needs, and the last
    /// member of the pinned family.  Lane B asked for it by naming the race it
    /// closes: "the destination race was the one that could create a file
    /// somewhere I never named"
    /// (`requests/b-a-666-669-are-wired-two-answers-and-one-bug-that-was-mine.md`).
    /// A `mv` that resolves its destination directory by path can have that
    /// directory swapped under it between the resolve and the rename, and then
    /// writes the entry into whatever now answers to the name.
    ///
    /// ## No symlink following, on either side
    ///
    /// Rename operates on *names*, never on what a final component resolves
    /// to: `mv link other` moves the link, it does not move the link's target.
    /// So unlike [`link_at_pinned`](Self::link_at_pinned) there is no `follow`
    /// argument and no branch where the object leaves the pinned directory.
    /// Both pins are therefore verified under the *write's own lock*, not
    /// merely by a pre-pass — this is the strongest guarantee any member of the
    /// family offers, and it comes for free from what rename already is.
    ///
    /// ## Cross-mount is refused, as it now is on the path route too
    ///
    /// [`rename_inner`](Self::rename_inner) falls back to copy+delete when the
    /// two paths are on different mounts.  This does not, and returns
    /// [`KernelError::CrossDevice`] (→ `EXDEV`, which is what Linux returns for
    /// the whole case).
    ///
    /// That is a deliberate divergence rather than an omission.  A copy+delete
    /// is a *sequence* of independent lock acquisitions — `stat`, `copy`,
    /// `set_permissions`, `set_owner`, `remove` — and a pin cannot span it: the
    /// directory could be swapped out between any two of those steps, which is
    /// the exact condition this call exists to refuse.  Offering it would hand
    /// the caller a handle-verified operation whose verification quietly lapses
    /// halfway through, which is worse than not offering it, because the caller
    /// has no way to tell the two apart.  A caller that genuinely wants the
    /// convenience copy wants the path route, where the pin was buying it
    /// nothing anyway.
    ///
    /// The refusal also buys the property [`link_at_pinned`](Self::link_at_pinned)
    /// relies on: in every case that can succeed at all, both directories are
    /// behind *one* filesystem lock, so there is no lock ordering to get wrong
    /// because there are never two locks.
    ///
    /// ## Returns
    ///
    /// `StaleHandle` if either handle no longer denotes its directory;
    /// `CrossDevice` if the two are on different mounts; `AlreadyExists` for
    /// [`RenameMode::NoReplace`] onto a taken name; `NotSupported` if the
    /// filesystem cannot exchange; otherwise whatever the rename itself
    /// reports.
    pub fn rename_at_pinned(
        old_dir: &PinnedDir,
        old_name: &[u8],
        new_dir: &PinnedDir,
        new_name: &[u8],
        mode: RenameMode,
    ) -> KernelResult<()> {
        check_at_name(old_name)?;
        check_at_name(new_name)?;

        // Pass 1, on both handles: a stale handle is refused before the
        // intercept hook runs. Each guard is taken and dropped in turn, so
        // this cannot deadlock even when both directories are on one
        // filesystem.
        {
            let (fs, fs_id, _opts, dir_rel) = resolve_mount(&old_dir.path)?;
            let mut guard = fs.lock();
            verify_pinned(&mut guard, fs_id, &dir_rel, old_dir)?;
        }
        {
            let (fs, fs_id, _opts, dir_rel) = resolve_mount(&new_dir.path)?;
            let mut guard = fs.lock();
            verify_pinned(&mut guard, fs_id, &dir_rel, new_dir)?;
        }

        let old_child = old_dir.path.join(old_name);
        let new_child = new_dir.path.join(new_name);

        // Both sides are namespace-checked. A rename *removes* the source name
        // as surely as it creates the destination, so checking only the
        // destination would let a caller unlink out of a namespace it cannot
        // write to by renaming within it.
        crate::ipc::namespace::check_writable(&old_child)?;
        crate::ipc::namespace::check_writable(&new_child)?;
        check_path_access(&old_child, PathAccess::Write)?;
        check_path_access(&new_child, PathAccess::Write)?;
        check_writable(&old_child)?;
        check_writable(&new_child)?;
        super::intercept::pre_rename(&old_child, &new_child)?;

        let (dest_inval, displaced) = {
            // Every mount-table lookup happens before any filesystem guard is
            // taken, so no ordering between the VFS lock and a filesystem lock
            // can arise.
            let (fs_old, old_fs_id, _opts, old_dir_rel) = resolve_mount(&old_dir.path)?;
            let (fs_new, new_fs_id, _opts, new_dir_rel) = resolve_mount(&new_dir.path)?;
            if !Arc::ptr_eq(&fs_old, &fs_new) {
                return Err(KernelError::CrossDevice);
            }
            let old_rel = old_dir_rel.join(old_name);
            let new_rel = new_dir_rel.join(new_name);

            let mut guard = fs_new.lock();
            // Pass 2, on both handles, under the guard that performs the
            // rename. `Arc::ptr_eq` above established that one guard reaches
            // both directories, so unlike `link_at_pinned` there is no case
            // where only one of the two can be re-verified here.
            verify_pinned(&mut guard, new_fs_id, &new_dir_rel, new_dir)?;
            verify_pinned(&mut guard, old_fs_id, &old_dir_rel, old_dir)?;

            match mode {
                RenameMode::Exchange => {
                    guard_rename(&mut **guard, &old_rel, &new_rel, true)?;
                    guard.rename_exchange(&old_rel, &new_rel)?;
                    // Both names still exist afterwards, so nothing is
                    // unlinked and no page cache identity dies.
                    (None, None)
                }
                RenameMode::NoReplace => {
                    // The existence check and the rename run under the *same*
                    // hold, which is the whole content of the guarantee:
                    // a separate pre-check would leave exactly the window
                    // `RENAME_NOREPLACE` exists to close.
                    match guard.stat(&new_rel) {
                        Ok(_) => return Err(KernelError::AlreadyExists),
                        Err(KernelError::NotFound) => {}
                        Err(e) => return Err(e),
                    }
                    // After that check: `EEXIST` before `EPERM`, as in
                    // `rename_inner`.
                    guard_rename(&mut **guard, &old_rel, &new_rel, false)?;
                    guard.rename(&old_rel, &new_rel)?;
                    // Nothing was displaced -- the check above proved it.
                    (None, None)
                }
                RenameMode::Replace => {
                    guard_rename(&mut **guard, &old_rel, &new_rel, false)?;
                    // A replacing rename unlinks whatever held the destination
                    // name, and that inode's number may be reused later, so
                    // its cached pages must go. Captured before the rename,
                    // while the name still reaches it.
                    let id = cache_identity(&mut guard, new_fs_id, &new_rel);
                    // And the state kept about it outside the filesystem
                    // (`super::perfile`).
                    let displaced = displaced_object(&mut guard, new_fs_id, &old_rel, &new_rel);
                    guard.rename(&old_rel, &new_rel)?;
                    (id, displaced)
                }
            }
        };
        if let Some((fs_id, ino)) = dest_inval {
            crate::mm::page_cache::invalidate_identity(fs_id, ino);
        }

        {
            let mut dcache = VFS_DCACHE.lock();
            dcache.invalidate_prefix(&old_child);
            dcache.invalidate_prefix(&new_child);
        }
        // An exchange leaves BOTH names present with swapped contents, so the
        // indexer must be told they *changed* rather than that one moved --
        // `on_file_renamed` would drop a path it still needs to track. This is
        // the same split `rename_exchange` makes on the path route.
        match mode {
            RenameMode::Exchange => {
                super::notify::emit_renamed(&old_child, &new_child);
                super::notify::emit_renamed(&new_child, &old_child);
                super::index::on_file_changed(&old_child);
                super::index::on_file_changed(&new_child);
                super::perfile::names_exchanged(&old_child, &new_child);
            }
            RenameMode::Replace | RenameMode::NoReplace => {
                super::notify::emit_renamed(&old_child, &new_child);
                super::index::on_file_renamed(&old_child, &new_child);
                // The replaced file first; see `rename_inner`.
                if let Some(unlinked) = displaced {
                    super::perfile::object_unlinked(unlinked, &new_child);
                }
                super::perfile::names_moved(&old_child, &new_child);
            }
        }
        super::journal::record_rename(&old_child, &new_child);
        super::audit::log_ok(super::audit::AuditOp::Rename, 0, &old_child);
        Ok(())
    }

    /// List the directory `dir` denotes, refusing if it no longer denotes the
    /// directory it was opened on.
    ///
    /// This is the entry point `getdents64` needs: it resolves *the handle*,
    /// so a directory renamed out from under an open descriptor is reported
    /// as stale rather than listed from whatever now answers to the old name.
    ///
    /// The listing is the same one the path routes give — no volume labels,
    /// submounts injected; see [`finish_listing`](Self::finish_listing).  That
    /// matters more here than anywhere else: 664 exists to be the *race-free
    /// substitute* for a listing taken by path, so swapping routes to close a
    /// race must not also change what the directory is said to contain.
    pub fn readdir_pinned(dir: &PinnedDir) -> KernelResult<Vec<DirEntry>> {
        check_path_access(&dir.path, PathAccess::Read)?;
        // The `fs` guard is scoped so it is released before `finish_listing`,
        // which takes `VFS`.  Holding both would invert the lock order every
        // other listing route establishes.
        let raw = {
            let (fs, fs_id, _opts, dir_rel) = resolve_mount(&dir.path)?;
            let mut guard = fs.lock();
            verify_pinned(&mut guard, fs_id, &dir_rel, dir)?;
            guard.readdir(&dir_rel)?
        };
        Ok(Self::finish_listing(&dir.path, raw))
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
    ///
    /// As Linux's `FS_IOC_SETFLAGS` allows it: the file's owner may change
    /// its attributes, but only root may change `IMMUTABLE` or `APPEND_ONLY`
    /// -- Linux's `CAP_LINUX_IMMUTABLE` -- since an owner who could clear
    /// them could undo a protection root placed ([`attribute_change_verdict`]).
    /// Kernel tasks pass.
    ///
    /// # Errors
    ///
    /// `NotPermitted` (`EPERM`) as above; the filesystem's own.
    pub fn set_attributes(path: impl AsRef<Path>, attrs: FileAttr) -> KernelResult<()> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        Self::set_attributes_resolved(&path, attrs)
    }

    /// [`set_attributes`](Self::set_attributes) on an already-resolved host
    /// path: a descriptor's whose file has no inode to hold it by
    /// (`fs::handle::HandleFile`).
    ///
    /// The privilege is decided on the file as it stands under its
    /// filesystem's lock, the one the change is made under.
    ///
    /// # Errors
    ///
    /// As [`set_attributes`](Self::set_attributes).
    pub fn set_attributes_resolved(path: &Path, attrs: FileAttr) -> KernelResult<()> {
        check_writable(path)?;
        check_path_access(path, PathAccess::Metadata)?;
        {
            let (fs, _id, _opts, relative) = resolve_mount(path)?;
            let mut guard = fs.lock();
            if let Some((uid, _)) = caller_uid_gid() {
                let meta = guard.metadata(&relative)?;
                attribute_change_verdict(uid, meta.uid, meta.attributes, attrs)?;
            }
            guard.set_attributes(&relative, attrs)?;
        }
        super::notify::emit_metadata(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
        Ok(())
    }

    /// `FS_IOC_SETFLAGS` through a held file, whatever its name now, as
    /// [`set_attributes`](Self::set_attributes) decides it. `path` names it
    /// for events.
    ///
    /// # Errors
    ///
    /// `ReadOnlyFilesystem`; `NotPermitted`; the filesystem's own.
    pub fn object_set_attributes(
        obj: &FileObject,
        path: &Path,
        attrs: FileAttr,
    ) -> KernelResult<()> {
        check_writable_fs(obj.fs_id)?;
        {
            let mut guard = obj.fs.lock();
            if let Some((uid, _)) = caller_uid_gid() {
                let meta = guard.metadata_ino(obj.ino)?;
                attribute_change_verdict(uid, meta.uid, meta.attributes, attrs)?;
            }
            guard.set_attributes_ino(obj.ino, attrs)?;
        }
        super::notify::emit_metadata(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
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
        Self::set_owner_resolved(&path, uid, gid)
    }

    /// [`set_owner`](Self::set_owner) on an already-resolved host path: an
    /// open handle's, whose namespace was applied at its open
    /// (`fs::handle::HandleFile`).
    ///
    /// # Errors
    ///
    /// As [`set_owner`](Self::set_owner).
    pub fn set_owner_resolved(path: &Path, uid: u32, gid: u32) -> KernelResult<()> {
        check_writable(path)?;
        check_path_access(path, PathAccess::Metadata)?;
        let requested = (uid, gid);
        // Resolve "leave unchanged" sentinels before taking the VFS lock
        // (metadata_resolved() takes the lock itself).
        let (uid, gid) = if uid == u32::MAX || gid == u32::MAX {
            let meta = Self::metadata_resolved(path)?;
            (
                if uid == u32::MAX { meta.uid } else { uid },
                if gid == u32::MAX { meta.gid } else { gid },
            )
        } else {
            (uid, gid)
        };
        {
            let (fs, _id, _opts, relative) = resolve_mount(path)?;
            let mut guard = fs.lock();
            if changes_owner(requested.0, requested.1) {
                guard_metadata(&mut **guard, &relative, true)?;
            }
            guard.set_owner(&relative, uid, gid)?;
        }
        super::notify::emit_metadata(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
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
        let requested = (uid, gid);
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
            let mut guard = fs.lock();
            if changes_owner(requested.0, requested.1) {
                guard_metadata(&mut **guard, &relative, false)?;
            }
            guard.set_owner_no_follow(&relative, uid, gid)?;
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
        Self::set_permissions_resolved(&path, permissions)
    }

    /// [`set_permissions`](Self::set_permissions) on an already-resolved
    /// host path: a create stamping the mode it was asked for, or an open
    /// handle's path (`fs::handle::HandleFile`).
    ///
    /// The creates called `set_permissions` with the path they had resolved
    /// until 2026-10-01, so a jailed process's jail was applied to it a
    /// second time: `open(O_CREAT)` and `mkdir` with any mode but the
    /// default made the file and then failed, `NotFound`, leaving it.
    ///
    /// # Errors
    ///
    /// As [`set_permissions`](Self::set_permissions).
    pub fn set_permissions_resolved(path: &Path, permissions: u16) -> KernelResult<()> {
        check_writable(path)?;
        check_path_access(path, PathAccess::Metadata)?;
        {
            let (fs, fs_id, _opts, relative) = resolve_mount(path)?;
            let mut guard = fs.lock();
            guard_metadata(&mut **guard, &relative, true)?;
            seal_mode_guard(&mut **guard, fs_id, &relative, path, permissions)?;
            guard.set_permissions(&relative, permissions)?;
            acl_follow_chmod_at(&mut **guard, fs_id, &relative, path, true, permissions);
        }
        super::notify::emit_metadata(path);
        super::journal::record(super::journal::JournalEventType::Modified, path);
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
            let (fs, fs_id, _opts, relative) = resolve_mount(&path)?;
            let mut guard = fs.lock();
            guard_metadata(&mut **guard, &relative, false)?;
            seal_mode_guard(&mut **guard, fs_id, &relative, &path, permissions)?;
            guard.set_permissions_no_follow(&relative, permissions)?;
            acl_follow_chmod_at(&mut **guard, fs_id, &relative, &path, false, permissions);
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
        Self::set_times_resolved(&path, accessed_ns, modified_ns)
    }

    /// [`set_times`](Self::set_times) on an already-resolved host path: an
    /// open handle's (`fs::handle::HandleFile`).
    ///
    /// # Errors
    ///
    /// As [`set_times`](Self::set_times).
    pub fn set_times_resolved(
        path: &Path,
        accessed_ns: Timestamp,
        modified_ns: Timestamp,
    ) -> KernelResult<()> {
        check_writable(path)?;
        check_path_access(path, PathAccess::Metadata)?;
        let (fs, _id, _opts, relative) = resolve_mount(path)?;
        let mut guard = fs.lock();
        guard_times(&mut **guard, &relative, true, accessed_ns, modified_ns)?;
        let now = metadata_now_ns();
        guard.set_times(
            &relative,
            resolve_time(accessed_ns, now),
            resolve_time(modified_ns, now),
        )
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
        let mut guard = fs.lock();
        guard_times(&mut **guard, &relative, false, accessed_ns, modified_ns)?;
        let now = metadata_now_ns();
        guard.set_times_no_follow(
            &relative,
            resolve_time(accessed_ns, now),
            resolve_time(modified_ns, now),
        )
        // No notify/journal — timestamp changes are metadata-only.
    }

    // --- Extended attributes ---
    //
    // Every call is Linux 6.6's, in its order (`fs::xattr_policy`): the path;
    // for a change, the mount's writability; the capability tags; then, on
    // the file as it stands under its filesystem's lock, the namespace's
    // rules, the ACL where Linux checks the file's own permission, and the
    // name. A follow call acts on the file a trailing link names, a no-follow
    // one (`lgetxattr` and the rest) on the link.
    //
    // A call is two steps: `xattr_target` finds the file and checks what
    // Linux checks before it reads the attribute's name; `xattr_get` and the
    // rest decide the rest. The Linux layer reads the name between them. The
    // path calls below take both at once.

    /// Get an extended attribute value.
    ///
    /// # Errors
    ///
    /// The path's (`NotFound`, ...), then `fs::xattr_policy`'s, then the
    /// filesystem's: `NoAttribute` when the file has no such attribute.
    pub fn get_xattr(path: impl AsRef<Path>, key: &[u8]) -> KernelResult<Vec<u8>> {
        let target = Self::xattr_target(path, true, xattr_policy::Access::Read)?;
        Self::xattr_get(&target, key)
    }

    /// Set an extended attribute, creating it or overwriting it.
    ///
    /// # Errors
    ///
    /// As [`set_xattr_with`](Self::set_xattr_with).
    pub fn set_xattr(path: impl AsRef<Path>, key: &[u8], value: &[u8]) -> KernelResult<()> {
        Self::set_xattr_with(path, key, value, XattrSetMode::Any)
    }

    /// Set an extended attribute, subject to an [`XattrSetMode`].
    ///
    /// The existence check and the write happen under a **single** hold of
    /// the filesystem lock, which is the whole reason the mode belongs in
    /// the kernel: userspace used to spell `XATTR_CREATE` as `getxattr`
    /// followed by `setxattr`, and two syscalls are two lock acquisitions
    /// with a window between them.  A second writer landing in that window
    /// turned "create, or fail" into a silent overwrite and "replace, or
    /// fail" into a create — the two outcomes the flags exist to forbid.
    /// See `design-decisions.md` §661.
    ///
    /// # Errors
    ///
    /// The path's; `ReadOnlyFilesystem`; `fs::xattr_policy`'s; the mode's
    /// (`AlreadyExists`, `NoAttribute`); the filesystem's.
    pub fn set_xattr_with(
        path: impl AsRef<Path>,
        key: &[u8],
        value: &[u8],
        mode: XattrSetMode,
    ) -> KernelResult<()> {
        let target = Self::xattr_target(path, true, xattr_policy::Access::Write)?;
        Self::xattr_set(&target, key, value, mode)
    }

    /// Remove an extended attribute.
    ///
    /// # Errors
    ///
    /// The path's; `ReadOnlyFilesystem`; `fs::xattr_policy`'s; `NoAttribute`
    /// when the file has no such attribute.
    pub fn remove_xattr(path: impl AsRef<Path>, key: &[u8]) -> KernelResult<()> {
        let target = Self::xattr_target(path, true, xattr_policy::Access::Write)?;
        Self::xattr_remove(&target, key)
    }

    /// List the extended attributes' names a caller may see
    /// ([`xattr_policy::listed`]).
    ///
    /// # Errors
    ///
    /// The path's; the filesystem's.
    pub fn list_xattrs(path: impl AsRef<Path>) -> KernelResult<Vec<Vec<u8>>> {
        Self::xattr_list(&Self::xattr_target(path, true, xattr_policy::Access::Read)?)
    }

    /// Get an xattr WITHOUT following a trailing symlink (`lgetxattr`).
    ///
    /// # Errors
    ///
    /// As [`get_xattr`](Self::get_xattr).
    pub fn get_xattr_no_follow(path: impl AsRef<Path>, key: &[u8]) -> KernelResult<Vec<u8>> {
        let target = Self::xattr_target(path, false, xattr_policy::Access::Read)?;
        Self::xattr_get(&target, key)
    }

    /// Set an xattr WITHOUT following a trailing symlink (`lsetxattr`).
    ///
    /// # Errors
    ///
    /// As [`set_xattr_with`](Self::set_xattr_with).
    pub fn set_xattr_no_follow(
        path: impl AsRef<Path>,
        key: &[u8],
        value: &[u8],
    ) -> KernelResult<()> {
        Self::set_xattr_no_follow_with(path, key, value, XattrSetMode::Any)
    }

    /// No-follow analogue of [`set_xattr_with`](Self::set_xattr_with).
    ///
    /// # Errors
    ///
    /// As [`set_xattr_with`](Self::set_xattr_with).
    pub fn set_xattr_no_follow_with(
        path: impl AsRef<Path>,
        key: &[u8],
        value: &[u8],
        mode: XattrSetMode,
    ) -> KernelResult<()> {
        let target = Self::xattr_target(path, false, xattr_policy::Access::Write)?;
        Self::xattr_set(&target, key, value, mode)
    }

    /// Remove an xattr WITHOUT following a trailing symlink (`lremovexattr`).
    ///
    /// # Errors
    ///
    /// As [`remove_xattr`](Self::remove_xattr).
    pub fn remove_xattr_no_follow(path: impl AsRef<Path>, key: &[u8]) -> KernelResult<()> {
        let target = Self::xattr_target(path, false, xattr_policy::Access::Write)?;
        Self::xattr_remove(&target, key)
    }

    /// List xattr names WITHOUT following a trailing symlink (`llistxattr`).
    ///
    /// # Errors
    ///
    /// As [`list_xattrs`](Self::list_xattrs).
    pub fn list_xattrs_no_follow(path: impl AsRef<Path>) -> KernelResult<Vec<Vec<u8>>> {
        Self::xattr_list(&Self::xattr_target(
            path,
            false,
            xattr_policy::Access::Read,
        )?)
    }

    /// Find the file an xattr call names, and check what Linux checks before
    /// it reads the attribute's name, in Linux's order: that the caller may
    /// reach it -- the capability tags and ACLs, which deny reaching the object
    /// at all, as a path walk's search permission does; that it exists
    /// (`user_path_at`); for a change, the namespace's and the mount's
    /// writability (`mnt_want_write`). `follow`: a trailing link is followed,
    /// or is the target itself.
    ///
    /// # Errors
    ///
    /// The path's; a capability tag's or an ACL's; `NotFound` for a file that
    /// is not there; then `ReadOnlyFilesystem` for a change.
    pub fn xattr_target(
        path: impl AsRef<Path>,
        follow: bool,
        access: xattr_policy::Access,
    ) -> KernelResult<XattrTarget> {
        let path = path.as_ref();
        let resolved = if follow {
            Self::resolve_follow(path)?
        } else {
            Self::resolve_no_follow(path)?
        };
        // Reaching it at all comes first, so a caller kept from it learns
        // nothing -- not even whether it is there.
        check_path_access(&resolved, PathAccess::Metadata)?;
        // Then the file is looked up, before anything about the change or the
        // attribute is decided, as `user_path_at` does. Until 2026-10-02
        // nothing here looked it up -- a missing name resolves to itself -- so
        // a missing file was found only when the attribute was read, after its
        // name: `getxattr` of an empty name on a missing path answered ERANGE,
        // where Linux answers ENOENT (rq42).
        {
            let (fs, _id, _opts, relative) = resolve_mount(&resolved)?;
            let mut guard = fs.lock();
            if follow {
                guard.stat(&relative)?;
            } else {
                guard.lstat(&relative)?;
            }
        }
        if access == xattr_policy::Access::Write {
            crate::ipc::namespace::check_writable(path)?;
            check_writable(&resolved)?;
        }
        Ok(XattrTarget {
            path: resolved,
            follow,
            access,
        })
    }

    /// [`xattr_target`](Self::xattr_target) for a host path already
    /// resolved: an open handle's (`fs::handle::HandleFile`), to which the
    /// caller's namespace was applied when it was opened. The target is the
    /// file the path names.
    ///
    /// The file is open, so it exists and was reached when it was opened: what
    /// is left is the mount's writability for a change (Linux's
    /// `mnt_want_write_file`), then the capability tags and ACLs.
    ///
    /// # Errors
    ///
    /// `ReadOnlyFilesystem` for a change; a capability tag's or an ACL's.
    pub fn xattr_target_resolved(
        path: &Path,
        access: xattr_policy::Access,
    ) -> KernelResult<XattrTarget> {
        if access == xattr_policy::Access::Write {
            check_writable(path)?;
        }
        check_path_access(path, PathAccess::Metadata)?;
        Ok(XattrTarget {
            path: path.to_path_buf(),
            follow: true,
            access,
        })
    }

    /// `getxattr` of `name` on a target, for the calling task.
    ///
    /// # Errors
    ///
    /// `fs::xattr_policy`'s; the filesystem's (`NoAttribute`).
    pub fn xattr_get(target: &XattrTarget, name: &[u8]) -> KernelResult<Vec<u8>> {
        Self::xattr_get_as(target, name, xattr_policy::Caller::current())
    }

    /// `setxattr` of `name` on a target found for a change, for the calling
    /// task.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` for a target found for reading; `fs::xattr_policy`'s;
    /// the mode's; the filesystem's.
    pub fn xattr_set(
        target: &XattrTarget,
        name: &[u8],
        value: &[u8],
        mode: XattrSetMode,
    ) -> KernelResult<()> {
        Self::xattr_set_as(target, name, value, mode, xattr_policy::Caller::current())
    }

    /// `removexattr` of `name` on a target found for a change, for the
    /// calling task.
    ///
    /// # Errors
    ///
    /// As [`xattr_set`](Self::xattr_set), and `NoAttribute`.
    pub fn xattr_remove(target: &XattrTarget, name: &[u8]) -> KernelResult<()> {
        Self::xattr_remove_as(target, name, xattr_policy::Caller::current())
    }

    /// `listxattr` on a target: the names the calling task may see
    /// ([`xattr_policy::listed`]).
    ///
    /// # Errors
    ///
    /// The filesystem's.
    pub fn xattr_list(target: &XattrTarget) -> KernelResult<Vec<Vec<u8>>> {
        Self::xattr_list_as(target, xattr_policy::Caller::current())
    }

    /// [`xattr_get`](Self::xattr_get) for `caller`. Taken as an argument
    /// rather than looked up so the self-test can be someone other than the
    /// kernel task it runs as (`self_test_xattr_rules`).
    fn xattr_get_as(
        target: &XattrTarget,
        name: &[u8],
        caller: xattr_policy::Caller,
    ) -> KernelResult<Vec<u8>> {
        if name == super::acl::XATTR_ACCESS {
            let (fs, fs_id, _opts, relative) = resolve_mount(&target.path)?;
            let mut guard = fs.lock();
            let meta = xattr_meta(&mut **guard, &relative, target.follow)?;
            return acl_door_get(&AclDoorFile::of(meta, fs_id, &target.path));
        }
        if name == super::acl::XATTR_DEFAULT {
            let (fs, fs_id, _opts, relative) = resolve_mount(&target.path)?;
            let mut guard = fs.lock();
            let meta = xattr_meta(&mut **guard, &relative, target.follow)?;
            return acl_default_get(&AclDoorFile::of(meta, fs_id, &target.path));
        }
        let follow = target.follow;
        Self::xattr_on(
            target,
            name,
            xattr_policy::Access::Read,
            caller,
            |fs, rel| {
                if follow {
                    fs.get_xattr(rel, name)
                } else {
                    fs.get_xattr_no_follow(rel, name)
                }
            },
        )
    }

    /// [`xattr_set`](Self::xattr_set) for `caller` (see
    /// [`xattr_get_as`](Self::xattr_get_as)).
    fn xattr_set_as(
        target: &XattrTarget,
        name: &[u8],
        value: &[u8],
        mode: XattrSetMode,
        caller: xattr_policy::Caller,
    ) -> KernelResult<()> {
        if name == super::acl::XATTR_ACCESS {
            if target.access != xattr_policy::Access::Write {
                return Err(KernelError::InvalidArgument);
            }
            let (fs, fs_id, _opts, relative) = resolve_mount(&target.path)?;
            {
                let mut guard = fs.lock();
                let meta = xattr_meta(&mut **guard, &relative, target.follow)?;
                let subject = AclDoorFile::of(meta, fs_id, &target.path);
                acl_door_set(&subject, value, mode, caller, |new_mode| {
                    seal_mode_guard(&mut **guard, fs_id, &relative, &target.path, new_mode)?;
                    guard.set_permissions_no_follow(&relative, new_mode)
                })?;
            }
            super::notify::emit_metadata(&target.path);
            super::journal::record(super::journal::JournalEventType::Modified, &target.path);
            return Ok(());
        }
        if name == super::acl::XATTR_DEFAULT {
            if target.access != xattr_policy::Access::Write {
                return Err(KernelError::InvalidArgument);
            }
            let (fs, fs_id, _opts, relative) = resolve_mount(&target.path)?;
            {
                let mut guard = fs.lock();
                let meta = xattr_meta(&mut **guard, &relative, target.follow)?;
                let subject = AclDoorFile::of(meta, fs_id, &target.path);
                acl_default_set(&subject, value, mode, caller)?;
            }
            super::notify::emit_metadata(&target.path);
            super::journal::record(super::journal::JournalEventType::Modified, &target.path);
            return Ok(());
        }
        let follow = target.follow;
        Self::xattr_on(
            target,
            name,
            xattr_policy::Access::Write,
            caller,
            |fs, rel| {
                if follow {
                    mode.check(fs.get_xattr(rel, name))?;
                    fs.set_xattr(rel, name, value)
                } else {
                    mode.check(fs.get_xattr_no_follow(rel, name))?;
                    fs.set_xattr_no_follow(rel, name, value)
                }
            },
        )?;
        super::notify::emit_metadata(&target.path);
        super::journal::record(super::journal::JournalEventType::Modified, &target.path);
        Ok(())
    }

    /// [`xattr_remove`](Self::xattr_remove) for `caller` (see
    /// [`xattr_get_as`](Self::xattr_get_as)).
    fn xattr_remove_as(
        target: &XattrTarget,
        name: &[u8],
        caller: xattr_policy::Caller,
    ) -> KernelResult<()> {
        if name == super::acl::XATTR_ACCESS {
            if target.access != xattr_policy::Access::Write {
                return Err(KernelError::InvalidArgument);
            }
            let (fs, fs_id, _opts, relative) = resolve_mount(&target.path)?;
            {
                let mut guard = fs.lock();
                let meta = xattr_meta(&mut **guard, &relative, target.follow)?;
                acl_door_remove(&AclDoorFile::of(meta, fs_id, &target.path), caller)?;
            }
            super::notify::emit_metadata(&target.path);
            super::journal::record(super::journal::JournalEventType::Modified, &target.path);
            return Ok(());
        }
        if name == super::acl::XATTR_DEFAULT {
            if target.access != xattr_policy::Access::Write {
                return Err(KernelError::InvalidArgument);
            }
            let (fs, fs_id, _opts, relative) = resolve_mount(&target.path)?;
            {
                let mut guard = fs.lock();
                let meta = xattr_meta(&mut **guard, &relative, target.follow)?;
                acl_default_remove(&AclDoorFile::of(meta, fs_id, &target.path), caller)?;
            }
            super::notify::emit_metadata(&target.path);
            super::journal::record(super::journal::JournalEventType::Modified, &target.path);
            return Ok(());
        }
        let follow = target.follow;
        Self::xattr_on(
            target,
            name,
            xattr_policy::Access::Write,
            caller,
            |fs, rel| {
                if follow {
                    fs.remove_xattr(rel, name)
                } else {
                    fs.remove_xattr_no_follow(rel, name)
                }
            },
        )?;
        super::notify::emit_metadata(&target.path);
        super::journal::record(super::journal::JournalEventType::Modified, &target.path);
        Ok(())
    }

    /// [`xattr_list`](Self::xattr_list) for `caller` (see
    /// [`xattr_get_as`](Self::xattr_get_as)). Checked against the capability
    /// tags, when the target was found, and nothing else, as Linux's
    /// `listxattr` is checked against nothing: a listing shows names, and a
    /// value's own rules still stand between a caller and the value.
    fn xattr_list_as(
        target: &XattrTarget,
        caller: xattr_policy::Caller,
    ) -> KernelResult<Vec<Vec<u8>>> {
        let (fs, fs_id, _opts, relative) = resolve_mount(&target.path)?;
        let (mut names, meta) = {
            let mut guard = fs.lock();
            let names = if target.follow {
                guard.list_xattrs(&relative)?
            } else {
                guard.list_xattrs_no_follow(&relative)?
            };
            // Discarded deliberately: the listing above has just found the
            // file; one whose metadata cannot be read has no ACL to list.
            (
                names,
                xattr_meta(&mut **guard, &relative, target.follow).ok(),
            )
        };
        names.retain(|name| xattr_policy::listed(name, caller.privileged));
        if let Some(meta) = meta {
            acl_door_listed(&AclDoorFile::of(meta, fs_id, &target.path), &mut names);
        }
        Ok(names)
    }

    /// Run `op` on a target's file if `fs::xattr_policy` lets `caller` have
    /// `access` to the attribute `name`, deciding in Linux 6.6's order.
    ///
    /// The namespace's rules and the name are decided under the same hold of
    /// the filesystem's lock as `op` runs under, so the file cannot become
    /// immutable, or a sticky directory change hands, between the decision
    /// and the change -- Linux holds the inode's lock across both. The ACL,
    /// which Linux checks between the two, cannot be read under that lock
    /// (`acl::check_access` finds the file by its name, through the VFS), so
    /// when an ACL could refuse, the rules are also applied to the file once
    /// before it, for a refusal of theirs to come first as on Linux.
    fn xattr_on<T>(
        target: &XattrTarget,
        name: &[u8],
        access: xattr_policy::Access,
        caller: xattr_policy::Caller,
        op: impl FnOnce(&mut dyn FileSystem, &Path) -> KernelResult<T>,
    ) -> KernelResult<T> {
        if access == xattr_policy::Access::Write && target.access != xattr_policy::Access::Write {
            // Found for reading: its mount was not checked for a change.
            return Err(KernelError::InvalidArgument);
        }
        let namespace = xattr_policy::Namespace::of(name);
        let (fs, _id, _opts, relative) = resolve_mount(&target.path)?;
        if namespace.checks_permission() && super::acl::count() != 0 {
            let meta = xattr_meta(&mut **fs.lock(), &relative, target.follow)?;
            xattr_policy::namespace_rules(namespace, access, &meta, caller)?;
            check_path_access(&target.path, path_access_for(access))?;
        }
        let mut guard = fs.lock();
        let meta = xattr_meta(&mut **guard, &relative, target.follow)?;
        xattr_policy::namespace_rules(namespace, access, &meta, caller)?;
        xattr_policy::after_permission(name, access, caller, guard.xattrs_supported())?;
        op(&mut **guard, &relative)
    }

    // --- Socket node ---

    /// Create a Unix-domain socket's node at `path` (`bind`'s half that is
    /// the filesystem's), with permission bits `mode` -- already
    /// umask-masked by the caller, as for [`Vfs::mkdir_mode`] -- and return
    /// the node's identity, which [`crate::ipc::unix_socket`] binds the
    /// socket to.
    ///
    /// The final component is not followed: a symlink there is
    /// `AlreadyExists`, as Linux's `bind` answers `EADDRINUSE` for any
    /// existing name. Everything a creation goes through -- the namespace,
    /// a read-only mount, write permission on the directory, the intercept
    /// hooks, the inode quota -- is gone through here.
    ///
    /// # Errors
    ///
    /// `AlreadyExists` if the name exists; `NotSupported` if the filesystem
    /// cannot hold a socket node, or reports no stable inode for it;
    /// otherwise what path resolution and the checks above report.
    pub fn mknod_socket(path: impl AsRef<Path>, mode: u16) -> KernelResult<FileId> {
        let path = path.as_ref();
        crate::ipc::namespace::check_writable(path)?;
        let path = Self::resolve_no_follow(path)?;
        check_writable(&path)?;
        check_path_access(&path, PathAccess::Write)?;
        // The intercept hooks know creation by writing; a socket node is a
        // name created in its directory, as a file is.
        super::intercept::pre_check(super::intercept::FsOp::Write, &path, None)?;
        enforce_quota_create(&path)?;
        let creator = creator_ids();
        let umask = creator_umask();
        let (fs_id, ino) = {
            let (fs, fs_id, _opts, relative) = resolve_mount(&path)?;
            let mut guard = fs.lock();
            guard_create(&mut **guard, &relative)?;
            let (perm, acls) = new_node_mode(
                &mut **guard,
                fs_id,
                parent_of(&relative),
                parent_of(&path),
                mode & 0o7777,
                umask,
                false,
            );
            let ino = guard.mknod_socket(&relative, perm)?;
            init_new_owner(&mut **guard, &relative, creator, false)?;
            acls.store(
                &mut **guard,
                fs_id,
                (ino != 0).then_some(ino),
                Some(&relative),
                &path,
            )?;
            (fs_id, ino)
        };
        if ino == 0 {
            // A node the table could not find again. The filesystem promised
            // a stable identity by implementing this; take the node back out
            // rather than leave a name nothing can ever be bound to.
            let (fs, _id, _opts, relative) = resolve_mount(&path)?;
            // Best effort: the creation is being refused either way, and a
            // failure to remove leaves only an inert node behind.
            let _ = fs.lock().remove(&relative);
            return Err(KernelError::NotSupported);
        }
        super::quota::charge_inode(0, 0);
        VFS_DCACHE.lock().invalidate_negative_prefix(&path);
        super::notify::emit_created(&path);
        super::index::on_file_changed(&path);
        super::journal::record(super::journal::JournalEventType::Created, &path);
        // Audited as a write to the directory, as a file's creation is.
        super::audit::log_ok(super::audit::AuditOp::Write, 0, &path);
        Ok(FileId { fs_id, ino })
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
        let creator = creator_ids();
        {
            let (fs, _id, _opts, relative) = resolve_mount(&path)?;
            let mut guard = fs.lock();
            guard_create(&mut **guard, &relative)?;
            guard.symlink(&relative, target)?;
            init_new_owner(&mut **guard, &relative, creator, false)?;
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
    ///
    /// # Why the source is gated for *read* and the destination for *write*
    ///
    /// A hard link is the one mutation whose two paths want different
    /// permissions.  `rename` takes `Write` on both, because a rename removes
    /// the source name; a link does not touch the source name at all, only its
    /// link count.  Demanding `Write` on the source would therefore forbid
    /// hard-linking a read-only file — which is the *main* legitimate use of
    /// hard links (content-addressed stores, `cp -l`, dedup-based backup), and
    /// this tree has such a store in `pkg/`.
    ///
    /// `Read` on the source is nonetheless required, and is the check that
    /// matters for containment: after the link exists, the caller reaches the
    /// object through a name *they* chose, under whatever policy that name
    /// carries.  Without this gate a sandbox that denies `/etc/shadow` is
    /// defeated by linking it into a directory the sandbox does allow, and
    /// nothing in the policy would notice — which is exactly the shape
    /// `check-vfs-permission-gate.py` exists to catch.
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
        // Both checks run against the *resolved* paths, so a symlink cannot be
        // used to have the gate judge one object while the link is made to
        // another.
        check_path_access(&existing, PathAccess::Read)?;
        check_path_access(&new_path, PathAccess::Write)?;
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
                // `EXDEV`, per `link()`'s own POSIX text — see the identical
                // check in `link_at_pinned` for why this is not `EINVAL`.
                return Err(KernelError::CrossDevice);
            }
            // The Vfs layer already resolved `existing` per `follow`, but the
            // final on-disk lookup happens inside the FS driver — so route to
            // the matching driver method to keep the no-follow contract when a
            // symlink is the final component.
            let mut guard = fs_existing.lock();
            guard_link(&mut **guard, &rel_existing, follow, &rel_new)?;
            if follow {
                guard.link(&rel_existing, &rel_new)?;
            } else {
                guard.link_no_follow(&rel_existing, &rel_new)?;
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
        let (fs, fs_id, _opts, relative) = resolve_mount(&path)?;
        let mut meta = fs.lock().lmetadata(&relative)?;
        meta.dev = dev_of(fs_id);
        Ok(meta)
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
        Self::statvfs_resolved(&path)
    }

    /// [`statvfs`](Self::statvfs) on an already-resolved host path: an open
    /// handle's (`fs::handle::HandleFile`).
    ///
    /// `read_only` is the mount's as well as the filesystem's: a writable
    /// filesystem mounted read-only is read-only to its callers, and
    /// `statvfs`'s `ST_RDONLY` says so, as Linux's does. It reported only the
    /// filesystem's until 2026-10-01.
    ///
    /// # Errors
    ///
    /// `NotFound` for a path under no mount; the filesystem's own.
    pub fn statvfs_resolved(path: &Path) -> KernelResult<FsInfo> {
        let (fs, _id, opts, _relative) = resolve_mount(path)?;
        let mut info = fs.lock().statvfs()?;
        info.read_only |= opts.read_only;
        Ok(info)
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
    ///
    /// **Returns `false` for every failure, not just absence.** `stat` goes through
    /// [`check_path_access`] and `resolve_mount`, so it can answer
    /// `PermissionDenied`, `InvalidArgument`, `InternalError` or a symlink-loop
    /// error, and all of them arrive here as "not there".
    ///
    /// That is fine for the common use -- *is there a config file to read?* -- and
    /// wrong for anything that DECIDES on the answer. Use
    /// [`exists_or_err`](Self::exists_or_err) when a false answer would let the
    /// caller proceed, because "I could not tell" and "it is not there" must not
    /// be the same value at a point like that.
    pub fn exists(path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        Self::stat(path).is_ok()
    }

    /// Whether a path exists, distinguishing absence from failure.
    ///
    /// `Ok(false)` only for [`KernelError::NotFound`]; every other error is
    /// returned. This is the form to use where the answer guards a decision --
    /// which layer of an overlay serves a file, whether it is safe to create
    /// something, whether a resource is still in use.
    ///
    /// Lane B found the same shape in `userspace/mkfs`: `is_mounted` returned
    /// `false` when it could not read `/proc/mounts`, so one unreadable line made
    /// every device look free and `mkfs` would format a live filesystem. The
    /// general rule is theirs: for any check guarding a destructive or privileged
    /// action, the error path must not answer in the permissive direction.
    pub fn exists_or_err(path: impl AsRef<Path>) -> KernelResult<bool> {
        let path = path.as_ref();
        match Self::stat(path) {
            Ok(_) => Ok(true),
            Err(KernelError::NotFound) => Ok(false),
            Err(e) => Err(e),
        }
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
    /// Also checks the immutable attribute: `NotPermitted` (`EPERM`), as
    /// [`access`](Self::access) answers it.
    pub fn is_writable(path: impl AsRef<Path>) -> KernelResult<()> {
        let path = path.as_ref();
        let meta = Self::metadata(path)?;
        if meta.attributes.contains(FileAttr::IMMUTABLE) {
            return Err(KernelError::NotPermitted);
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
    /// `NotFound` / `PermissionDenied` on failure -- and `NotPermitted`
    /// (`EPERM`) for `W_OK` on an immutable file, which `access(2)` lists:
    /// "Write permission was requested to a file that has the immutable flag
    /// set" (Linux's `inode_permission`).
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
            return Err(KernelError::NotPermitted);
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

    /// What the Linux `access(2)` owes a caller: whether the gates an open or
    /// an exec of `path` would actually meet let it in, for each of `mode`'s
    /// [`R_OK`], [`W_OK`] and [`X_OK`] -- and nothing else. `follow`: whether
    /// a final symlink is followed (`faccessat2` without
    /// `AT_SYMLINK_NOFOLLOW`).
    ///
    /// The gates, in Linux's order (`inode_permission`, then the mount):
    ///
    /// - for `W_OK`, an immutable file (`NotPermitted`, `EPERM`, which
    ///   `access(2)` lists);
    /// - the ACLs and capability file-tags ([`check_path_access`]), for each
    ///   access asked;
    /// - for `W_OK`, a read-only mount (`ReadOnlyFilesystem`, `EROFS`), as
    ///   Linux reports one for a file, a directory or a symlink -- not for a
    ///   device, a socket or a FIFO, whose writes do not go to the mount.
    ///
    /// Not the mode bits, unlike [`access`](Self::access): nothing enforces
    /// them, so a mode-`444` file is writable, and answering from them would
    /// refuse what a write gets
    /// (`TD-B-ACCESS-CANNOT-SEE-THE-ONE-PERMISSION-MECHANISM-THAT-IS-ENFORCED`).
    /// Until 2026-10-02 the Linux `access` consulted no gate at all, and
    /// answered every existing file writable.
    ///
    /// # Errors
    ///
    /// The path's (`NotFound`, ...); then as above.
    pub fn access_gates(path: impl AsRef<Path>, mode: u32, follow: bool) -> KernelResult<()> {
        let path = path.as_ref();
        // `lmetadata` resolves its path itself, so it is given the caller's:
        // given the resolved one, a jailed caller's jail would be applied
        // twice.
        let (resolved, meta) = if follow {
            let resolved = Self::resolve_follow(path)?;
            let meta = Self::metadata_resolved(&resolved)?;
            (resolved, meta)
        } else {
            (Self::resolve_no_follow(path)?, Self::lmetadata(path)?)
        };
        if mode & W_OK != 0 && meta.attributes.contains(FileAttr::IMMUTABLE) {
            return Err(KernelError::NotPermitted);
        }
        for (bit, want) in [
            (R_OK, PathAccess::Read),
            (W_OK, PathAccess::Write),
            (X_OK, PathAccess::Execute),
        ] {
            if mode & bit != 0 {
                check_path_access(&resolved, want)?;
            }
        }
        let on_the_mount = matches!(
            meta.entry_type,
            EntryType::File | EntryType::Directory | EntryType::Symlink
        );
        if mode & W_OK != 0 && on_the_mount && Self::mount_options(&resolved)?.read_only {
            return Err(KernelError::ReadOnlyFilesystem);
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

    /// Take a `flock` lock on a file, without waiting.
    ///
    /// `path` is resolved (symlinks followed) before locking. `owner` is the
    /// holder: a process ([`flock_process_owner`]) or an open file
    /// description ([`flock_description_owner`]).
    ///
    /// ## Semantics (Linux's `flock_lock_inode`)
    ///
    /// - **Shared lock**: compatible with other shared locks, incompatible
    ///   with exclusive locks from other owners.
    /// - **Exclusive lock**: incompatible with any lock from another owner.
    /// - **Conversion is not atomic.** An owner asking for the other type
    ///   first loses the lock it holds, then asks as anyone would. So two
    ///   holders of a shared lock that both ask for exclusive do not wait on
    ///   each other forever: one gets it. A refused conversion leaves the
    ///   owner with no lock, as on Linux.
    ///
    /// `WouldBlock` while another owner's lock is in the way;
    /// [`flock_wait_resolved`](Self::flock_wait_resolved) waits instead.
    /// `ResourceExhausted` when the table is full (`ENOLCK`).
    pub fn flock(path: impl AsRef<Path>, owner: u64, lock_type: LockType) -> KernelResult<()> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        Self::flock_resolved(&path, owner, lock_type)
    }

    /// [`flock`](Self::flock) on an already-resolved host path.
    ///
    /// Handle-backed callers already hold a resolved host path (captured at
    /// `open`), so they must NOT re-run namespace translation -- doing so would
    /// re-apply the chroot jail prefix a second time (double-jail) and key the
    /// lock on the wrong path. This worker operates directly on `path`.
    pub fn flock_resolved(
        path: impl AsRef<Path>,
        owner: u64,
        lock_type: LockType,
    ) -> KernelResult<()> {
        let path = path.as_ref();
        // Identity of the file this path names, resolved per call. Not
        // cached: if the name is repointed between operations the identity
        // should differ, which is the whole reason for keying on it.
        //
        // Resolved BEFORE `LOCK_TABLE` is taken, never under it: resolving
        // locks the mounted filesystem, and procfs's `/proc/locks` takes
        // `LOCK_TABLE` while its own filesystem lock is held -- so resolving
        // under the table was the reverse order, an AB/BA deadlock lockdep
        // reported on the 2026-09-26 integration boot (a `/proc/locks` read
        // against an advisory lock on any procfs file). The table lock never
        // kept a rename out anyway, so nothing is lost by looking first.
        let id = Self::file_identity_resolved(path).unwrap_or(None);
        flock_attempt(path, id, owner, lock_type)
    }

    /// The `flock` calls for an open file, keyed on the file it holds --
    /// `fs::handle::lock_key` -- rather than on what its name names now. A
    /// file renamed while open keeps its locks, and a new file under its old
    /// name is not locked by them; they keyed on the open-time name until
    /// 2026-10-01. `path` is the name a lock is shown under (`/proc/locks`),
    /// and the key for a file with no identity.
    ///
    /// # Errors
    ///
    /// As [`flock_resolved`](Self::flock_resolved).
    pub fn flock_key(
        path: &Path,
        id: Option<FileId>,
        owner: u64,
        lock_type: LockType,
    ) -> KernelResult<()> {
        flock_attempt(path, id, owner, lock_type)
    }

    /// [`flock_resolved`](Self::flock_resolved), waiting while another
    /// owner's lock is in the way: `flock` without `LOCK_NB`.
    ///
    /// Woken by every change that can let it in: an unlock, a release at a
    /// close or an exit, a conversion. It then tries again, and parks again
    /// if it lost the race.
    ///
    /// # Errors
    ///
    /// - `Interrupted` when a deliverable signal arrives during the wait. The
    ///   syscall layers restart it under `SA_RESTART`, as Linux restarts
    ///   `flock`, and answer `EINTR` otherwise. Kernel tasks have no signals
    ///   and wait uninterruptibly.
    /// - `ResourceExhausted` when the table, or the list of waiters, is full.
    pub fn flock_wait_resolved(
        path: impl AsRef<Path>,
        owner: u64,
        lock_type: LockType,
    ) -> KernelResult<()> {
        let path = path.as_ref();
        // Resolved once, before any lock, as in `flock_resolved`.
        let id = Self::file_identity_resolved(path).unwrap_or(None);
        Self::flock_wait_key(path, id, owner, lock_type)
    }

    /// [`flock_key`](Self::flock_key), waiting while another owner's lock is
    /// in the way, as [`flock_wait_resolved`](Self::flock_wait_resolved)
    /// waits.
    ///
    /// # Errors
    ///
    /// As [`flock_wait_resolved`](Self::flock_wait_resolved).
    pub fn flock_wait_key(
        path: &Path,
        id: Option<FileId>,
        owner: u64,
        lock_type: LockType,
    ) -> KernelResult<()> {
        use crate::ipc::waiters;

        // The uncontended case takes no lock on the waiter list at all.
        match flock_attempt(path, id, owner, lock_type) {
            Err(KernelError::WouldBlock) => {}
            other => return other,
        }
        let task = crate::sched::current_task_id();
        let pid = waiters::current_user_pid();
        loop {
            // Registered BEFORE each attempt: a release that follows the
            // attempt finds this task to wake, and one that precedes it is
            // seen by the attempt. Never with `LOCK_TABLE` held.
            {
                let mut list = FLOCK_WAITERS.lock();
                if list.len() >= MAX_FLOCK_WAITERS {
                    return Err(KernelError::ResourceExhausted);
                }
                list.push(FlockWaiter {
                    task,
                    path: path.to_path_buf(),
                    id,
                });
            }
            let attempt = flock_attempt(path, id, owner, lock_type);
            if attempt != Err(KernelError::WouldBlock) {
                remove_flock_waiter(task);
                return attempt;
            }
            if waiters::deliverable_signal_pending(pid) {
                remove_flock_waiter(task);
                return Err(KernelError::Interrupted);
            }
            waiters::park_interruptible(
                pid,
                task,
                crate::wchan::Wait::on(crate::wchan::WaitChannel::FileLock),
            );
            remove_flock_waiter(task);
        }
    }

    /// Release an advisory lock on a file.
    ///
    /// If the owner doesn't hold a lock on the path, this is a no-op.
    pub fn funlock(path: impl AsRef<Path>, owner: u64) -> KernelResult<()> {
        let path = path.as_ref();
        let path = Self::resolve_follow(path)?;
        Self::funlock_resolved(&path, owner)
    }

    /// Release an advisory lock on an already-resolved host path, waking
    /// whoever was waiting for it.
    ///
    /// Worker for [`funlock`](Self::funlock); handle-backed callers pass the
    /// resolved host path directly to avoid double-jailing (see
    /// [`flock_resolved`](Self::flock_resolved)).
    pub fn funlock_resolved(path: impl AsRef<Path>, owner: u64) -> KernelResult<()> {
        let path = path.as_ref();
        // Resolved before `LOCK_TABLE` is taken, never under it: see
        // `flock_resolved`.
        let id = Self::file_identity_resolved(path).unwrap_or(None);
        Self::funlock_key(path, id, owner);
        Ok(())
    }

    /// [`flock_key`](Self::flock_key)'s release, waking whoever was waiting.
    /// Nothing for an owner holding no lock on the file.
    pub fn funlock_key(path: &Path, id: Option<FileId>, owner: u64) {
        let released = {
            let mut table = LOCK_TABLE.lock();
            let mut released = false;
            if let Some(idx) = table.iter().position(|e| lock_entry_matches(e, path, id))
                && let Some(entry) = table.get_mut(idx)
            {
                let before = entry.locks.len();
                entry.locks.retain(|l| l.owner != owner);
                released = entry.locks.len() != before;
                // Clean up empty entries to prevent unbounded growth.
                if entry.locks.is_empty() {
                    table.swap_remove(idx);
                }
            }
            released
        };
        if released {
            wake_flock_waiters(path, id);
        }
    }

    /// Release every advisory lock one owner holds: a process's at its
    /// exit. Wakes the waiters of every file it held.
    pub fn funlock_all(owner: u64) {
        let freed: Vec<(PathBuf, Option<FileId>)> = {
            let mut table = LOCK_TABLE.lock();
            let mut freed = Vec::new();
            // Remove this owner from every entry, then clean up empties.
            table.retain_mut(|entry| {
                let before = entry.locks.len();
                entry.locks.retain(|l| l.owner != owner);
                if entry.locks.len() != before {
                    freed.push((entry.path.clone(), entry.id));
                }
                !entry.locks.is_empty()
            });
            freed
        };
        for (path, id) in &freed {
            wake_flock_waiters(path, *id);
        }
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
        // Resolved before `LOCK_TABLE` is taken, never under it: see
        // `flock_resolved`.
        let id = Self::file_identity_resolved(path).unwrap_or(None);
        let table = LOCK_TABLE.lock();
        if let Some(entry) = table.iter().find(|e| lock_entry_matches(e, path, id)) {
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

/// One `flock` attempt, with the file's identity already looked up: the
/// table half of [`Vfs::flock_resolved`] and [`Vfs::flock_wait_resolved`].
///
/// A conversion removes the owner's old lock before anything else (see
/// [`Vfs::flock`]), and that removal can let a waiter in, so it wakes the
/// file's waiters -- after the table lock is dropped, whatever the attempt's
/// own answer.
fn flock_attempt(
    path: &Path,
    id: Option<FileId>,
    owner: u64,
    lock_type: LockType,
) -> KernelResult<()> {
    // Which process took it, for `/proc/locks`. Asked before the table lock:
    // it takes the thread table's.
    let pid = crate::ipc::waiters::current_user_pid();
    let (result, converted) = {
        let mut table = LOCK_TABLE.lock();
        flock_in_table(&mut table, path, id, owner, lock_type, pid)
    };
    if converted {
        wake_flock_waiters(path, id);
    }
    result
}

/// The decision itself, under the table lock: Linux's `flock_lock_inode`.
///
/// - The owner's own lock of the asked type: nothing to do.
/// - Its lock of the other type: removed first, and the request goes on as
///   a new one. The second value says this happened.
/// - A new lock: refused with `WouldBlock` while another owner's lock
///   conflicts, else added.
fn flock_in_table(
    table: &mut Vec<PathLockEntry>,
    path: &Path,
    id: Option<FileId>,
    owner: u64,
    lock_type: LockType,
    pid: u64,
) -> (KernelResult<()>, bool) {
    let Some(idx) = table.iter().position(|e| lock_entry_matches(e, path, id)) else {
        if table.len() >= MAX_LOCKED_PATHS {
            return (Err(KernelError::ResourceExhausted), false);
        }
        table.push(PathLockEntry {
            // Stored so a later lookup by a DIFFERENT name for the same
            // file finds this entry rather than creating a second one.
            id,
            path: path.to_path_buf(),
            locks: alloc::vec![FileLock {
                owner,
                lock_type,
                pid,
            }],
        });
        return (Ok(()), false);
    };
    let Some(entry) = table.get_mut(idx) else {
        return (Err(KernelError::InternalError), false);
    };
    let mut converted = false;
    if let Some(pos) = entry.locks.iter().position(|l| l.owner == owner) {
        if entry
            .locks
            .get(pos)
            .is_some_and(|l| l.lock_type == lock_type)
        {
            return (Ok(()), false);
        }
        entry.locks.remove(pos);
        converted = true;
    }
    let conflict = match lock_type {
        LockType::Shared => entry
            .locks
            .iter()
            .any(|l| l.lock_type == LockType::Exclusive),
        LockType::Exclusive => !entry.locks.is_empty(),
    };
    if conflict {
        // Not empty: something conflicts. The converted owner's lock is gone.
        return (Err(KernelError::WouldBlock), converted);
    }
    entry.locks.push(FileLock {
        owner,
        lock_type,
        pid,
    });
    (Ok(()), converted)
}

/// Wake the tasks waiting for a `flock` on one file, taking them off the
/// list: each tries again, and re-registers if it loses.
fn wake_flock_waiters(path: &Path, id: Option<FileId>) {
    let woken: Vec<crate::sched::task::TaskId> = {
        let mut list = FLOCK_WAITERS.lock();
        let mut woken = Vec::new();
        list.retain(|w| {
            let same = match (w.id, id) {
                (Some(a), Some(b)) => a == b,
                _ => w.path.as_path() == path,
            };
            if same {
                woken.push(w.task);
            }
            !same
        });
        woken
    };
    crate::ipc::waiters::wake_all(woken);
}

/// Wake every task waiting for a `flock`: the files under an unmounted
/// filesystem lose their locks all at once.
fn wake_all_flock_waiters() {
    let woken: Vec<crate::sched::task::TaskId> =
        FLOCK_WAITERS.lock().drain(..).map(|w| w.task).collect();
    crate::ipc::waiters::wake_all(woken);
}

/// Take `task` off the waiter list, on every way out of a wait.
fn remove_flock_waiter(task: crate::sched::task::TaskId) {
    FLOCK_WAITERS.lock().retain(|w| w.task != task);
}

// ---------------------------------------------------------------------------
// Lock table dump (for procfs)
// ---------------------------------------------------------------------------

/// Files report their filesystem's device number (`st_dev`), and a device
/// number is reused once its mount is gone.
///
/// Until 2026-10-01 every file reported 0, so two files on two filesystems
/// that shared an inode number were one file to `tar`, `cp -a` and `du`.
fn device_numbers_self_test() -> KernelResult<()> {
    const MNT: &str = "/tmp/vfsdev";
    const FILE: &str = "/tmp/vfsdev/probe";
    crate::serial_println!("[vfs]   Testing device numbers...");

    // The encoding first: glibc's `minor()` must take back what was put in.
    let decode = |d: u64| (d & 0xff) | ((d >> 12) & 0xffff_ff00);
    for dev in [1u32, 0xff, 0x100, 0x12_3456] {
        let encoded = linux_dev_t(dev);
        if decode(encoded) != u64::from(dev) || encoded & 0xffff_f000_000f_ff00 != 0 {
            crate::serial_println!(
                "[vfs]   FAIL: device {:#x} encodes as {:#x}, which decodes to minor {:#x}, major bits {:#x}",
                dev,
                encoded,
                decode(encoded),
                encoded & 0xffff_f000_000f_ff00
            );
            return Err(KernelError::InternalError);
        }
    }

    crate::fs::memfs::mount(MNT)?;
    let outcome = (|| -> KernelResult<(u32, u32, u32, u32)> {
        Vfs::write_file(FILE, b"dev")?;
        let tmp = Vfs::metadata("/tmp")?.dev;
        let file = Vfs::metadata(FILE)?.dev;
        let unfollowed = Vfs::lmetadata(FILE)?.dev;
        let root = Vfs::metadata(MNT)?.dev;
        Ok((tmp, file, unfollowed, root))
    })();
    // Best effort: the file goes with the scratch mount.
    let _ = Vfs::remove(FILE);
    Vfs::unmount(MNT)?;
    let (tmp, file, unfollowed, root) = outcome?;
    // The mount's own root is on it, and so is the file; /tmp is not.
    if tmp == 0 || file == 0 || file == tmp || unfollowed != file || root != file {
        crate::serial_println!(
            "[vfs]   FAIL: device numbers: /tmp {}, a file on a mount under it {} (lstat {}), the mount's root {}",
            tmp,
            file,
            unfollowed,
            root
        );
        return Err(KernelError::InternalError);
    }

    // The number is free again: the next mount takes the smallest free one,
    // which is it, unless another mount came in between.
    crate::fs::memfs::mount(MNT)?;
    let again = Vfs::metadata(MNT).map(|m| m.dev);
    Vfs::unmount(MNT)?;
    match again {
        Ok(dev) if dev == file => {}
        Ok(dev) if dev != 0 => crate::serial_println!(
            "[vfs]     note: the remount took device {} rather than {} (another mount in between)",
            dev,
            file
        ),
        other => {
            crate::serial_println!("[vfs]   FAIL: a remount reported device {:?}", other);
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!(
        "[vfs]   device numbers: per mount, reused after unmount, Linux's dev_t: OK"
    );
    Ok(())
}

/// One `flock` lock, as `/proc/locks` reports it.
#[derive(Debug, Clone)]
pub struct FlockInfo {
    /// The resolved path it was taken on.
    pub path: PathBuf,
    /// The file's identity, where its filesystem has one.
    pub id: Option<FileId>,
    pub lock_type: LockType,
    /// The process that took it (0 for a kernel task).
    pub pid: u64,
}

/// Every `flock` lock held, for `/proc/locks`.
pub fn lock_table_dump() -> Vec<FlockInfo> {
    let table = LOCK_TABLE.lock();
    let mut result = Vec::new();
    for entry in table.iter() {
        for lock in &entry.locks {
            result.push(FlockInfo {
                path: entry.path.clone(),
                id: entry.id,
                lock_type: lock.lock_type,
                pid: lock.pid,
            });
        }
    }
    result
}

/// What [`flock_wait_task`] returned: 0 while still waiting, 1 for `Ok`,
/// `0x100 | -code` for an error.
static FLOCK_WAIT_RESULT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// The file [`self_test_flock_wait`] locks: a real one, so it has an identity.
const FLOCK_WAIT_TEST_PATH: &str = "/tmp/_vfs_flock_wait_test";

/// [`Vfs::flock_wait_resolved`] on the rungs' file, in a task of its own:
/// `arg` is the owner, with bit 32 set for an exclusive lock.
extern "C" fn flock_wait_task(arg: u64) {
    let lock_type = if arg & (1 << 32) != 0 {
        LockType::Exclusive
    } else {
        LockType::Shared
    };
    let got = match Vfs::flock_wait_resolved(FLOCK_WAIT_TEST_PATH, arg & 0xFFFF_FFFF, lock_type) {
        Ok(()) => 1,
        Err(e) => 0x100 | u64::from(e.code().unsigned_abs()),
    };
    FLOCK_WAIT_RESULT.store(got, core::sync::atomic::Ordering::SeqCst);
}

/// Start [`flock_wait_task`] and give it time to park.
fn spawn_flock_waiter(arg: u64) -> KernelResult<()> {
    FLOCK_WAIT_RESULT.store(0, core::sync::atomic::Ordering::SeqCst);
    crate::sched::spawn(b"flock-wait", 16, flock_wait_task, arg, 0)?;
    crate::sched::sleep_ns_interruptible(20_000_000);
    Ok(())
}

/// Whether the waiter is parked: registered, and not yet returned.
fn flock_waiter_parked() -> bool {
    let registered = FLOCK_WAITERS
        .lock()
        .iter()
        .filter(|w| w.path.as_path() == Path::new(FLOCK_WAIT_TEST_PATH))
        .count();
    registered == 1 && FLOCK_WAIT_RESULT.load(core::sync::atomic::Ordering::SeqCst) == 0
}

/// [`FLOCK_WAIT_RESULT`] once it is set, or 0 after about a second without.
fn await_flock_wait_result() -> u64 {
    let deadline = crate::hrtimer::now_ns().saturating_add(1_000_000_000);
    loop {
        let got = FLOCK_WAIT_RESULT.load(core::sync::atomic::Ordering::SeqCst);
        if got != 0 || crate::hrtimer::now_ns() >= deadline {
            return got;
        }
        crate::sched::yield_now();
    }
}

/// `flock` without `LOCK_NB` waits, and is woken.
///
/// Run once interrupts are on (`main.rs`, beside `fs::reclock`'s waiting
/// rungs): a waiter that cannot park is not tested.
///
/// 1. A waiter for an exclusive lock parks behind another owner's, and the
///    unlock wakes it into the lock.
/// 2. Two holders of a shared lock both upgrading. The one that waits gives
///    its shared lock up first, so the other's upgrade is granted, and that
///    one's unlock lets the waiter in. With an atomic upgrade, as this table
///    had before 2026-10-01, each would wait on the other forever.
///
/// # Errors
///
/// `InternalError` when a rung answers wrongly; the setup's own otherwise.
pub fn self_test_flock_wait() -> KernelResult<()> {
    const A: u64 = 9301;
    const B: u64 = 9302;
    // Best effort: a leftover from an earlier boot's failure.
    let _ = Vfs::remove(FLOCK_WAIT_TEST_PATH);
    Vfs::write_file(FLOCK_WAIT_TEST_PATH, b"flock wait")?;
    let outcome = flock_wait_rungs(A, B);
    // A waiter a failing rung left parked is woken by these releases, and
    // takes its lock; the second release of B returns it.
    Vfs::funlock_all(A);
    Vfs::funlock_all(B);
    let _ = await_flock_wait_result();
    Vfs::funlock_all(B);
    // Best effort: the file is this test's own scratch.
    let _ = Vfs::remove(FLOCK_WAIT_TEST_PATH);
    outcome?;
    crate::serial_println!("[vfs] flock waiting: PASSED");
    Ok(())
}

/// [`self_test_flock_wait`]'s rungs, as owners `a` and `b`.
fn flock_wait_rungs(a: u64, b: u64) -> KernelResult<()> {
    const EXCLUSIVE: u64 = 1 << 32;
    let path = FLOCK_WAIT_TEST_PATH;

    // 1. Parked behind an exclusive lock; the unlock lets it in.
    Vfs::flock_resolved(path, a, LockType::Exclusive)?;
    spawn_flock_waiter(b | EXCLUSIVE)?;
    let parked = flock_waiter_parked();
    Vfs::funlock_resolved(path, a)?;
    let woke = await_flock_wait_result();
    let held = Vfs::lock_query_resolved(path)?;
    Vfs::funlock_resolved(path, b)?;
    if !parked || woke != 1 || !matches!(held, Some((LockType::Exclusive, 1))) {
        crate::serial_println!(
            "[vfs]   FAIL: flock wait: parked {}, returned {:#x}, then {:?}",
            parked,
            woke,
            held
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[vfs]     a waiter parks, and the unlock wakes it into the lock OK");

    // 2. Two sharers upgrading: B waits, A's upgrade is granted.
    Vfs::flock_resolved(path, a, LockType::Shared)?;
    Vfs::flock_resolved(path, b, LockType::Shared)?;
    spawn_flock_waiter(b | EXCLUSIVE)?;
    let parked = flock_waiter_parked();
    let upgraded = Vfs::flock_resolved(path, a, LockType::Exclusive);
    Vfs::funlock_resolved(path, a)?;
    let woke = await_flock_wait_result();
    Vfs::funlock_resolved(path, b)?;
    if !parked || upgraded.is_err() || woke != 1 {
        crate::serial_println!(
            "[vfs]   FAIL: two upgrading sharers: B parked {}, A's upgrade {:?}, B returned {:#x}",
            parked,
            upgraded,
            woke
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[vfs]     two sharers upgrading do not wait on each other OK");
    Ok(())
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

/// Maximum number of path components [`Vfs::mkdir_all`] will create in one call.
///
/// A denial-of-service bound: `mkdir_all` walks the path creating each missing
/// component, so an arbitrarily deep path is an arbitrarily long operation.
///
/// Named rather than inlined because it is a ceiling that callers have to
/// respect and cannot discover by reading their own code. `kshell`'s
/// `WALK_DEPTH_CAP` is bounded by it — the shell's recursive walkers cap their
/// *descent*, and the self-test fixtures that prove the cap bites must build a
/// tree deeper than it, which is a `mkdir_all` deep enough to hit this. That
/// collision cost a boot test before either limit was named.
pub(crate) const MAX_MKDIR_ALL_COMPONENTS: usize = 64;

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

/// Where `path` lands when the subtree at `from` moves to `to`: `from` itself
/// becomes `to`, `from/x` becomes `to/x`, and `None` means `path` is not in
/// that subtree. [`crate::fs::pathutil::rebase`], except that `from` may be
/// `/`: a pivot moves the root, which no rename does.
fn rebase_under(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    if from.components().next().is_none() {
        // Everything is under `/`: `/x` becomes `to/x`.
        let mut out = to.to_path_buf();
        for c in path.components() {
            out.push(c);
        }
        return Some(out);
    }
    crate::fs::pathutil::rebase(path, from, to)
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

/// What removing the name `relative` ends, for the tables that keep state
/// about files outside the filesystem ([`super::perfile`]). Read under the
/// guard that performs the removal, while the name still resolves.
///
/// `lmetadata`, not `metadata`: removing a symlink removes the link, and
/// following it would end the state of the file it points to. `None` if the
/// name cannot be read; [`super::perfile::name_removed`] reports that case
/// when the removal then succeeds regardless.
fn unlinked_object(
    fs: &mut Box<dyn FileSystem>,
    fs_id: u64,
    relative: &Path,
) -> Option<super::perfile::Unlinked> {
    fs.lmetadata(relative)
        .ok()
        .map(|meta| super::perfile::Unlinked::from_meta(fs_id, &meta))
}

/// What a rename of `rel_from` onto `rel_to` removes from `rel_to`: the object
/// that name held, or `None` when it held nothing -- or held the moving object
/// itself, which POSIX makes a rename that does nothing. That case needs the
/// check: a directory, or a file on a filesystem without inode numbers, would
/// otherwise read as losing its last name to a rename that removed nothing.
fn displaced_object(
    fs: &mut Box<dyn FileSystem>,
    fs_id: u64,
    rel_from: &Path,
    rel_to: &Path,
) -> Option<super::perfile::Unlinked> {
    if rel_from == rel_to {
        return None;
    }
    let to = fs.lmetadata(rel_to).ok()?;
    if to.ino != 0 && fs.lmetadata(rel_from).is_ok_and(|from| from.ino == to.ino) {
        return None;
    }
    Some(super::perfile::Unlinked::from_meta(fs_id, &to))
}

/// Longest single path component an fd-relative operation will accept.
///
/// Matches the `NAME_MAX` every filesystem we mount agrees on; the check
/// exists so a caller cannot use the `name` argument to smuggle in a whole
/// path's worth of bytes and blow a fixed buffer deeper in a driver.
const AT_NAME_MAX: usize = 255;

/// Validate the single trailing component of an fd-relative operation.
///
/// The containment guarantee of the `*at` primitives rests entirely on this:
/// `name` must be exactly one component, so that joining it to a directory
/// whose identity has been verified cannot reach outside that directory.  A
/// `/` would make the argument a path (and re-open the multi-hop resolution
/// the primitives exist to avoid); `..` would climb out of the verified
/// directory, making the verification meaningless; `.` would denote the
/// directory itself, which is not a thing you can unlink.
fn check_at_name(name: &[u8]) -> KernelResult<()> {
    if name.is_empty() || name.len() > AT_NAME_MAX {
        return Err(KernelError::InvalidArgument);
    }
    if name.contains(&b'/') || name.contains(&0) {
        return Err(KernelError::InvalidArgument);
    }
    if name == b"." || name == b".." {
        return Err(KernelError::InvalidArgument);
    }
    Ok(())
}

/// Check that `dir` still denotes the object it was pinned to, using a
/// filesystem guard the caller already holds.
///
/// Taking the guard rather than a path is what makes the check worth
/// anything: every operation on a given filesystem serialises on that one
/// mutex (see [`resolve_mount`], which clones the `Arc` and drops the VFS
/// lock), so a verify and an act performed under a *single* hold cannot be
/// interleaved by a rename.  Verifying through a `Vfs::` entry point instead
/// would take and release the lock, leaving exactly the check-then-use window
/// the whole design is here to close.
fn verify_pinned(
    fs: &mut Box<dyn FileSystem>,
    fs_id: u64,
    relative: &Path,
    dir: &PinnedDir,
) -> KernelResult<()> {
    let Some(want) = dir.id else {
        // Nothing to check against.  Not an error here: whether an
        // unverifiable handle is acceptable was decided by the caller before
        // it got this far, and answering "verified" for a filesystem that
        // cannot be verified would be the invented value this whole family
        // of primitives exists to stop producing.
        return Ok(());
    };
    if fs_id != want.fs_id {
        return Err(KernelError::StaleHandle);
    }
    // `lmetadata`, not `metadata`: if the name now leads to a *symlink* where
    // a directory used to be, following it would report the target's inode --
    // and an attacker who can plant the symlink can point it at a directory
    // whose inode is whatever they like, including the pinned one.  The link's
    // own inode cannot be forged into a match.
    let now = fs.lmetadata(relative)?.ino;
    if now == 0 || now != want.ino {
        return Err(KernelError::StaleHandle);
    }
    Ok(())
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

/// The set-group-ID bit of a mode.
pub const S_ISGID: u16 = 0o2000;

/// Who makes a node now: the calling process's uid and gid, or root's in
/// kernel context.
///
/// Read before a filesystem's lock is taken -- as [`check_path_access`]
/// reads credentials -- so the process table is never locked inside a
/// filesystem's lock.
fn creator_ids() -> (u32, u32) {
    let task = crate::sched::current_task_id();
    match crate::proc::thread::acting_process(task) {
        Some(pid) if pid != 0 => crate::proc::pcb::process_uid_gid(pid).unwrap_or((0, 0)),
        _ => (0, 0),
    }
}

/// Give the node just made at `relative` its owner, under the same hold of
/// the filesystem's lock that made it -- so no other operation sees it
/// owned by anyone else -- as Linux's `inode_init_owner` does at creation:
///
/// - the owner is the creator (`creator`, from [`creator_ids`]);
/// - the group is the creator's, unless the directory it was made in is
///   set-group-ID, when it is the directory's (a shared project directory
///   keeps its group), and a directory made there is set-group-ID too.
///
/// A filesystem with no owners (FAT) answers `NotSupported`, which is not a
/// failure of the creation. Until 2026-10-02 nothing set an owner at all, so
/// every node was root's whoever made it
/// (`A-NEW-FILES-ARE-OWNED-BY-UID-0-WHOEVER-CREATES-THEM`).
fn init_new_owner(
    fs: &mut dyn FileSystem,
    relative: &Path,
    creator: (u32, u32),
    is_dir: bool,
) -> KernelResult<()> {
    let parent = relative.parent().unwrap_or(Path::new("/"));
    let (parent_mode, parent_gid) = match fs.metadata(parent) {
        Ok(m) => (m.permissions, m.gid),
        // No metadata for the directory: the creator's group, as an ordinary
        // directory would give.
        Err(_) => (0, creator.1),
    };
    let setgid_dir = parent_mode & S_ISGID != 0;
    let gid = if setgid_dir { parent_gid } else { creator.1 };
    if (creator.0, gid) != (0, 0) {
        match fs.set_owner_no_follow(relative, creator.0, gid) {
            Ok(()) | Err(KernelError::NotSupported) => {}
            Err(e) => return Err(e),
        }
    }
    if is_dir && setgid_dir {
        let mode = fs.lmetadata(relative).map_or(0, |m| m.permissions);
        match fs.set_permissions_no_follow(relative, mode | S_ISGID) {
            Ok(()) | Err(KernelError::NotSupported) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// The calling process's umask, or none (0) for the kernel -- read, as
/// [`creator_ids`] is, before a filesystem's lock is taken.
///
/// The kernel applies it, as Linux's VFS applies `current_umask()`: a
/// directory with a default ACL gives its new files no umask
/// ([`new_node_mode`]), so the bits it removes must still be there when the
/// creation is decided. Until 2026-10-02 each ABI applied it before calling in
/// -- the Linux shim, and lane D's libc for native programs -- and a default
/// ACL could not have restored what was already gone. Applying a umask twice is
/// applying it once, so a libc that still does is harmless meanwhile.
fn creator_umask() -> u16 {
    let task = crate::sched::current_task_id();
    match crate::proc::thread::acting_process(task) {
        Some(pid) if pid != 0 => crate::proc::pcb::get_umask(pid).unwrap_or(0),
        _ => 0,
    }
}

/// What `Vfs::write_file_inner` does with a name that has no file yet, and
/// with one that has.
#[derive(Clone, Copy)]
enum Creating {
    /// Make the file if it is missing, at the default mode; replace an
    /// existing one's contents -- `write_file`.
    AsNeeded,
    /// Make it, asked for this mode; refuse an existing one (`AlreadyExists`)
    /// -- `open(O_CREAT)`.
    New(u16),
}

/// The ACLs a new node takes from its directory's default ACL, stored once the
/// node exists ([`NewAcls::store`]).
#[derive(Default)]
struct NewAcls {
    /// Its access ACL: `None` when the mode says all of it.
    access: Option<super::acl::Acl>,
    /// A new directory's own default ACL: its parent's, unchanged.
    default: Option<super::acl::Acl>,
}

impl NewAcls {
    /// Keep them for the node just made, by its identity -- `ino` when the
    /// filesystem handed it back, else what `relative` names now -- under the
    /// lock that made it, so no operation sees the node without them.
    fn store(
        self,
        fs: &mut dyn FileSystem,
        fs_id: u64,
        ino: Option<u64>,
        relative: Option<&Path>,
        path: &Path,
    ) -> KernelResult<()> {
        if self.access.is_none() && self.default.is_none() {
            return Ok(());
        }
        let ino = match (ino, relative) {
            (Some(ino), _) => ino,
            (None, Some(rel)) => fs.lmetadata(rel).map_or(0, |m| m.ino),
            (None, None) => 0,
        };
        let id = (ino != 0).then_some(FileId { fs_id, ino });
        if let Some(acl) = self.access {
            super::acl::set_acl_for(id, path, acl)?;
        }
        if let Some(acl) = self.default {
            super::acl::set_default_for(id, path, acl)?;
        }
        Ok(())
    }
}

/// The mode a node about to be made in the directory `parent_rel` (on `fs`;
/// `parent` on the host) begins with, and the ACLs it takes: `requested` less
/// the creator's `umask`; or, when the directory has a default ACL, what that
/// ACL grants of `requested` -- with no umask -- and the access ACL that goes
/// with it, plus the default again for a new directory: Linux's
/// `posix_acl_create`. Under the filesystem's lock that will make the node.
fn new_node_mode(
    fs: &mut dyn FileSystem,
    fs_id: u64,
    parent_rel: &Path,
    parent: &Path,
    requested: u16,
    umask: u16,
    is_dir: bool,
) -> (u16, NewAcls) {
    let default = if super::acl::default_count() == 0 {
        None
    } else {
        // A directory whose identity cannot be read is looked up by its name,
        // which is what the table keys it by on such a filesystem.
        let parent_id = fs
            .metadata(parent_rel)
            .ok()
            .filter(|m| m.ino != 0)
            .map(|m| FileId { fs_id, ino: m.ino });
        super::acl::get_default_for(parent_id, parent)
    };
    match default {
        Some(default) => {
            let (mode, access) = super::acl::inherit(&default, requested);
            let default = is_dir.then_some(default);
            (mode, NewAcls { access, default })
        }
        None => (requested & !umask, NewAcls::default()),
    }
}

/// Give the node just made at `relative` the mode `perm`, unless it is the
/// filesystem's own default for such a node (`default`), which it already
/// has.
///
/// Twelve bits, not nine. The open path once masked to `0o777` "until
/// setuid/setgid/sticky are plumbed through the create path" -- but they
/// were: ext4 stores `permissions & 0o7777` and memfs the `u16` whole, so
/// the mask was the only thing dropping a bit the caller asked for, which is
/// what lanes A and B agreed to rule out (design-decisions.md §639). A setuid
/// bit `exec` does not yet honour is stored safely: unenforced, it grants
/// nothing, so the error is less privilege than the metadata claims.
///
/// `NotSupported` is not a failure of the creation: FAT stores no mode, and
/// reporting a failure for a file that was made and left behind is the worst
/// of both answers; Linux's vfat likewise ignores the mode and lets the
/// mount's umask govern. (The boot test never mounts a FAT root, which is how
/// that arm once survived unseen.) Any other error is a failure: a filesystem
/// that can store a mode and does not is discarding what the caller asked for.
fn stamp_new_mode(
    fs: &mut dyn FileSystem,
    relative: &Path,
    perm: u16,
    default: u16,
) -> KernelResult<()> {
    if perm == default {
        return Ok(());
    }
    match fs.set_permissions_no_follow(relative, perm) {
        Ok(()) | Err(KernelError::NotSupported) => Ok(()),
        Err(e) => Err(e),
    }
}

/// The directory a path is in; the root is its own.
fn parent_of(path: &Path) -> &Path {
    path.parent().unwrap_or(Path::new("/"))
}

/// The calling process's uid and gid, or `None` for a kernel task (or a
/// process being torn down), which the permission checks let pass.
fn caller_uid_gid() -> Option<(u32, u32)> {
    let task_id = crate::sched::current_task_id();
    let pid = match crate::proc::thread::acting_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => return None,
    };
    crate::proc::pcb::get_credentials(pid).map(|c| (c.uid, c.gid))
}

/// Whether `uid` may change a file owned by `owner` from attributes `old` to
/// `new`, as Linux's `FS_IOC_SETFLAGS` decides: root may change anything;
/// the owner anything but `IMMUTABLE` and `APPEND_ONLY`, which need
/// `CAP_LINUX_IMMUTABLE` (root here); anyone else nothing.
///
/// # Errors
///
/// `NotPermitted` (`EPERM`).
pub(crate) fn attribute_change_verdict(
    uid: u32,
    owner: u32,
    old: FileAttr,
    new: FileAttr,
) -> KernelResult<()> {
    if uid == 0 {
        return Ok(());
    }
    if uid != owner {
        return Err(KernelError::NotPermitted);
    }
    let guarded = FileAttr::IMMUTABLE.union(FileAttr::APPEND_ONLY).bits();
    if (old.bits() ^ new.bits()) & guarded != 0 {
        return Err(KernelError::NotPermitted);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The immutable and append-only attributes, as the VFS applies them
// ---------------------------------------------------------------------------
//
// `fs::attr_policy` holds the rules; these helpers apply them to what a
// mount-relative path names. Each takes the filesystem under its lock and is
// called immediately before the filesystem call it guards, under the same
// guard, so no attribute can be set between the check and the change -- and
// so every filesystem that reports the attributes is held to them, whether or
// not it checks them itself.
//
// A lookup that fails decides nothing: the operation runs and reports its own
// error. The lookup and the operation see the same tree under the same lock,
// so a name that cannot be looked up cannot be operated on either, and the
// operation's error is the one its callers expect -- `ENOENT` and `EEXIST`
// before `EPERM`, as Linux orders them, its lookups preceding `may_delete` and
// `may_create`.

/// The metadata of what `relative` names on `fs`, or `None` when the lookup
/// fails (see above). `follow`: whether a final symlink names the object
/// (`true`) or is the object (`false`, as `unlink`, `rename` and `lchown` see
/// it).
fn attr_meta(fs: &mut dyn FileSystem, relative: &Path, follow: bool) -> Option<FileMeta> {
    let meta = if follow {
        fs.metadata(relative)
    } else {
        fs.lmetadata(relative)
    };
    // Discarded deliberately: a failed lookup is the operation's to report,
    // under the same lock (see above).
    meta.ok()
}

/// The attributes of the directory `relative` is named in; `NONE` when it
/// cannot be looked up, as [`attr_meta`].
fn dir_attrs(fs: &mut dyn FileSystem, relative: &Path) -> FileAttr {
    let parent = relative.parent().unwrap_or(Path::new("/"));
    attr_meta(fs, parent, true).map_or(FileAttr::NONE, |m| m.attributes)
}

/// The attributes of the held inode `ino`; `NONE` when it cannot be looked
/// up, as [`attr_meta`] -- the operation reports why.
fn ino_attrs(fs: &mut dyn FileSystem, ino: u64) -> FileAttr {
    fs.metadata_ino(ino)
        .map_or(FileAttr::NONE, |m| m.attributes)
}

/// [`attr_policy::may_delete`] for the name `relative`: `unlink` and
/// `rmdir`. Nothing to decide when it names nothing.
fn guard_delete(fs: &mut dyn FileSystem, relative: &Path) -> KernelResult<()> {
    let Some(victim) = attr_meta(fs, relative, false) else {
        return Ok(());
    };
    attr_policy::may_delete(dir_attrs(fs, relative), victim.attributes)
}

/// [`attr_policy::may_create`] for the new name `relative`. Nothing to
/// decide when the name is taken: the operation refuses that itself, or --
/// a whole-file write -- replaces the contents, which the filesystem checks.
fn guard_create(fs: &mut dyn FileSystem, relative: &Path) -> KernelResult<()> {
    if attr_meta(fs, relative, false).is_some() {
        return Ok(());
    }
    attr_policy::may_create(dir_attrs(fs, relative))
}

/// [`attr_policy::may_create`] for an unnamed file made in the directory
/// `dir` (`O_TMPFILE`).
fn guard_create_in(fs: &mut dyn FileSystem, dir: &Path) -> KernelResult<()> {
    let attrs = attr_meta(fs, dir, true).map_or(FileAttr::NONE, |m| m.attributes);
    attr_policy::may_create(attrs)
}

/// Giving the object with attributes `source` the new name `new`:
/// [`attr_policy::may_create`] for `new`'s directory, then
/// [`attr_policy::may_link`] for the object, in Linux's `vfs_link` order.
fn guard_link_attrs(fs: &mut dyn FileSystem, source: FileAttr, new: &Path) -> KernelResult<()> {
    guard_create(fs, new)?;
    attr_policy::may_link(source)
}

/// [`guard_link_attrs`] for the object `source` names. `follow`: whether a
/// final symlink in `source` names the object (`link`) or is it
/// (`link_no_follow`, `linkat` without `AT_SYMLINK_FOLLOW`). Nothing to
/// decide when `source` names nothing.
fn guard_link(
    fs: &mut dyn FileSystem,
    source: &Path,
    follow: bool,
    new: &Path,
) -> KernelResult<()> {
    let Some(src) = attr_meta(fs, source, follow) else {
        return Ok(());
    };
    guard_link_attrs(fs, src.attributes, new)
}

/// The rules for renaming `from` to `to` on one filesystem, or for
/// exchanging them (`exchange`, `RENAME_EXCHANGE`).
///
/// Linux's `vfs_rename`: the source's name leaves its directory
/// ([`attr_policy::may_delete`]); the destination's, when it has one, leaves
/// its own -- it is replaced, or exchanged -- and otherwise the destination's
/// directory gains a name ([`attr_policy::may_create`]). Two names for one
/// object are left alone before any of that, as Linux returns before
/// `may_delete` when `source == target`: such a rename does nothing. An
/// exchange with a missing name is the exchange's to refuse (`ENOENT`).
fn guard_rename(
    fs: &mut dyn FileSystem,
    from: &Path,
    to: &Path,
    exchange: bool,
) -> KernelResult<()> {
    let Some(source) = attr_meta(fs, from, false) else {
        return Ok(());
    };
    let target = attr_meta(fs, to, false);
    if let Some(target) = &target
        && (from == to || (source.ino != 0 && source.ino == target.ino))
    {
        return Ok(());
    }
    match target {
        Some(target) => {
            attr_policy::may_delete(dir_attrs(fs, from), source.attributes)?;
            attr_policy::may_delete(dir_attrs(fs, to), target.attributes)
        }
        None if exchange => Ok(()),
        None => {
            attr_policy::may_delete(dir_attrs(fs, from), source.attributes)?;
            attr_policy::may_create(dir_attrs(fs, to))
        }
    }
}

/// [`attr_policy::may_change_metadata`] for what `relative` names: `chmod`,
/// `chown`, and times set to given values. Nothing to decide when it names
/// nothing.
fn guard_metadata(fs: &mut dyn FileSystem, relative: &Path, follow: bool) -> KernelResult<()> {
    match attr_meta(fs, relative, follow) {
        Some(meta) => attr_policy::may_change_metadata(meta.attributes),
        None => Ok(()),
    }
}

/// The rule for setting the times of what `relative` names to `accessed` and
/// `modified` (0 leaves one as it is, [`TIME_NOW`] makes it now):
/// [`attr_policy::may_touch`] when both are [`TIME_NOW`] -- what `touch`
/// asks -- and [`attr_policy::may_change_metadata`] otherwise, as Linux
/// tells `ATTR_TOUCH` from `ATTR_TIMES_SET`.
fn guard_times(
    fs: &mut dyn FileSystem,
    relative: &Path,
    follow: bool,
    accessed: Timestamp,
    modified: Timestamp,
) -> KernelResult<()> {
    match attr_meta(fs, relative, follow) {
        Some(meta) => times_rule(meta.attributes, accessed, modified),
        None => Ok(()),
    }
}

/// [`guard_times`]'s decision, given the attributes. Nothing to decide when
/// both times are left as they are (0): nothing changes, as Linux's
/// `utimensat` returns before it looks when both are `UTIME_OMIT`.
fn times_rule(attrs: FileAttr, accessed: Timestamp, modified: Timestamp) -> KernelResult<()> {
    if accessed == 0 && modified == 0 {
        Ok(())
    } else if accessed == TIME_NOW && modified == TIME_NOW {
        attr_policy::may_touch(attrs)
    } else {
        attr_policy::may_change_metadata(attrs)
    }
}

/// Whether a `chown` of `uid` and `gid` changes either: `u32::MAX` leaves one
/// as it is. Linux's `chown_common` asks `may_setattr` about neither when
/// both are -1, so such a call is not refused on an immutable file.
fn changes_owner(uid: u32, gid: u32) -> bool {
    uid != u32::MAX || gid != u32::MAX
}

/// A requested time with [`TIME_NOW`] made the current wall-clock time, for
/// the filesystem, which stores times rather than requests.
fn resolve_time(requested: Timestamp, now: Timestamp) -> Timestamp {
    if requested == TIME_NOW {
        now
    } else {
        requested
    }
}

// ---------------------------------------------------------------------------
// Seals (`fs::sealing`), as the VFS applies them
// ---------------------------------------------------------------------------
//
// Asked on every change of a file's contents or size and on every chmod, under
// the filesystem's lock, as the attribute rules are -- filesystem lock, then
// module state, is the kernel's order, and the seal table calls nothing back.
// By the file's identity, so a seal holds whichever name or descriptor the
// change comes through. The fast path is one relaxed load: on a system that
// has sealed nothing, which is almost every system, nothing is looked up.

/// The seals on what `relative` names on `fs`, with its metadata: `None` when
/// no file anywhere is sealed, or when it cannot be looked up -- the
/// operation then reports why, as with [`attr_meta`]. `host` is the name a
/// seal is keyed by on a filesystem without identities.
fn seals_at(
    fs: &mut dyn FileSystem,
    fs_id: u64,
    relative: &Path,
    host: &Path,
) -> Option<(super::sealing::SealFlags, FileMeta)> {
    if !super::sealing::any_sealed() {
        return None;
    }
    // Discarded deliberately, as `attr_meta`'s.
    let meta = fs.metadata(relative).ok()?;
    let id = (meta.ino != 0).then_some(FileId {
        fs_id,
        ino: meta.ino,
    });
    Some((super::sealing::seals_of(id, host), meta))
}

/// [`seals_at`] for a held file, by its inode. A held file always has an
/// identity, so no name is needed to key a seal by.
fn seals_held(
    fs: &mut dyn FileSystem,
    obj: &FileObject,
) -> Option<(super::sealing::SealFlags, FileMeta)> {
    if !super::sealing::any_sealed() {
        return None;
    }
    // Discarded deliberately, as `attr_meta`'s.
    let meta = fs.metadata_ino(obj.ino).ok()?;
    Some((
        super::sealing::seals_of(Some(obj.id()), Path::new("")),
        meta,
    ))
}

/// The seal rules for a chmod of what `relative` names to `permissions`
/// ([`super::sealing::may_change_mode`]).
fn seal_mode_guard(
    fs: &mut dyn FileSystem,
    fs_id: u64,
    relative: &Path,
    host: &Path,
    permissions: u16,
) -> KernelResult<()> {
    match seals_at(fs, fs_id, relative, host) {
        Some((seals, meta)) => {
            super::sealing::may_change_mode(seals, meta.permissions, permissions)
        }
        None => Ok(()),
    }
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

    // Get the calling process's PID: none for a kernel task, or a task
    // acting with the kernel's authority (`proc::thread::as_kernel`).
    let task_id = crate::sched::current_task_id();
    let pid = match crate::proc::thread::acting_process(task_id) {
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
        check_acl(AclSubject::Path(path), uid, gid, want)?;
    }
    Ok(())
}

/// [`check_path_access`]'s ACL half for a file held open (`FileObject`), by
/// the file's own identity rather than a name it may no longer have: for the
/// calls through a descriptor that Linux still checks against the file's own
/// permission -- `fsetxattr` of a `user.` name wants write permission,
/// whatever the descriptor was opened for. The capability tags were the
/// open's to check. `meta` is the file's.
///
/// Kernel tasks and user id 0 pass, as through [`check_path_access`].
///
/// # Errors
///
/// `PermissionDenied` (`EACCES`) when the file's ACL refuses `want`.
pub(crate) fn check_object_access(
    obj: &FileObject,
    meta: &FileMeta,
    want: PathAccess,
) -> KernelResult<()> {
    if super::acl::count() == 0 {
        return Ok(());
    }
    let task_id = crate::sched::current_task_id();
    let pid = match crate::proc::thread::acting_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => return Ok(()), // Kernel task or PID 0 -- bypass.
    };
    let creds = match crate::proc::pcb::get_credentials(pid) {
        Some(c) => c,
        None => return Ok(()), // No credentials -- process being torn down.
    };
    check_acl(AclSubject::Held(obj.id(), meta), creds.uid, creds.gid, want)
}

/// What [`check_acl`] is asked about: the file a path names now, or a file
/// held open, with its metadata in hand.
#[derive(Clone, Copy)]
enum AclSubject<'a> {
    /// The file the path names now.
    Path(&'a Path),
    /// A held file, by its identity, and its metadata.
    Held(FileId, &'a FileMeta),
}

/// Evaluate the POSIX ACL of the file `subject` names, if it has one, for
/// `want`.
///
/// Split out from [`check_path_access`] so it can be exercised directly with
/// synthetic credentials: the kernel's own self-tests run as a kernel task,
/// which the gate above bypasses before reaching any ACL, so a test that went
/// through the gate could only ever observe "allowed" and would pass against
/// an ACL layer that had been removed entirely.
fn check_acl(subject: AclSubject<'_>, uid: u32, gid: u32, want: PathAccess) -> KernelResult<()> {
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
    // A held file's comes with it.
    let fetched;
    let (file, meta) = match subject {
        AclSubject::Path(path) => match Vfs::metadata_resolved(path) {
            Ok(m) => {
                fetched = m;
                (super::acl::AclFile::Path(path), &fetched)
            }
            // The object is gone or the filesystem cannot report ownership.
            // Defer: the operation itself is about to fail with a better error
            // than PermissionDenied, and denying here would turn a missing file
            // into a permissions puzzle.
            Err(_) => return Ok(()),
        },
        AclSubject::Held(id, meta) => (super::acl::AclFile::Held(id), meta),
    };

    super::acl::check_access(file, uid, gid, meta.uid, meta.gid, request)
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

    // Who may change a file's attributes (`set_attributes`, FS_IOC_SETFLAGS):
    // the decision alone, since this test runs as a kernel task, which passes.
    {
        let none = FileAttr::NONE;
        let imm = FileAttr::IMMUTABLE;
        let app = FileAttr::APPEND_ONLY;
        let hidden = FileAttr::HIDDEN;
        let verdicts = [
            (
                0,
                1000,
                none,
                imm,
                true,
                "root sets IMMUTABLE on anyone's file",
            ),
            (1000, 1000, imm, none, false, "an owner clears IMMUTABLE"),
            (1000, 1000, none, app, false, "an owner sets APPEND_ONLY"),
            (1000, 1000, none, hidden, true, "an owner sets HIDDEN"),
            (
                1000,
                1000,
                imm,
                imm.union(hidden),
                true,
                "an owner sets HIDDEN, IMMUTABLE kept",
            ),
            (1001, 1000, none, hidden, false, "a stranger sets HIDDEN"),
        ];
        for (uid, owner, old, new, allowed, what) in verdicts {
            let verdict = attribute_change_verdict(uid, owner, old, new);
            let ok = if allowed {
                verdict.is_ok()
            } else {
                verdict == Err(KernelError::NotPermitted)
            };
            if !ok {
                serial_println!("[vfs]   FAIL: attribute change: {} -> {:?}", what, verdict);
                return Err(KernelError::InternalError);
            }
        }
        serial_println!(
            "[vfs]   attribute changes: root any, owner all but IMMUTABLE/APPEND_ONLY: OK"
        );
    }

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

    // Fourteen of the sections below are gated on `has_tmp`. The gate itself is
    // honest — it is a mount-table fact, not a swallowed error — but without
    // this record the last line would read `Self-test PASSED` after a run that
    // skipped most of the suite, and a reader who scrolls to the bottom would
    // have no way to tell that from a full one.
    let mut skips = crate::fs::selftest::Skips::new();
    if !has_tmp {
        // Three of the fourteen record themselves separately further down, so
        // this one covers the other eleven. The split is not arbitrary: a
        // section a reader would want named when it is missing — the two pinned
        // families and `RESOLVE_BENEATH`, whose whole subject is a security
        // property — says so itself, while the long tail is summarised.
        skips.record(
            "symlink resolution, xattrs, ACLs, quotas, mount normalisation and 6 more",
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

        // A process and a description are different owners even when their
        // numbers match: process 300's exclusive request is refused by the
        // shared lock 300 -- a handle number -- holds. Until 2026-10-01 they
        // were one owner, so it was a conversion and was granted.
        let as_process = Vfs::flock(test_path, flock_process_owner(300), LockType::Exclusive);
        if as_process != Err(KernelError::WouldBlock) {
            serial_println!(
                "[vfs]   FAIL: process 300 and handle 300 were one flock owner: {:?}",
                as_process
            );
            Vfs::funlock_all(flock_process_owner(300));
            Vfs::funlock_all(300);
            let _ = Vfs::remove(test_path);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     a process and a handle with one number are two owners OK");

        // A conversion is not atomic, as Linux's is not: asking for the other
        // type first gives up the lock held. So a refused upgrade leaves its
        // owner with nothing -- the price of two sharers that both upgrade
        // never waiting on each other forever (`self_test_flock_wait`).
        Vfs::flock(test_path, 400, LockType::Shared)?;
        let refused = Vfs::flock(test_path, 400, LockType::Exclusive);
        let after = Vfs::lock_query(test_path)?;
        Vfs::funlock_all(400);
        if refused != Err(KernelError::WouldBlock) || !matches!(after, Some((LockType::Shared, 1)))
        {
            serial_println!(
                "[vfs]   FAIL: a refused upgrade answered {:?} and left {:?} (want WouldBlock, then 300's Shared alone)",
                refused,
                after
            );
            Vfs::funlock_all(300);
            let _ = Vfs::remove(test_path);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     a refused upgrade gives up the lock held OK");

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

    // --- Cross-mount rename is refused, not performed ---
    // `/tmp` (memfs) and `/` (ext4/fat) are different mounts, so this is the
    // cross-device case POSIX defines `EXDEV` for.
    //
    // This test used to assert the opposite: that the bytes arrived at the
    // destination and the source was gone. The kernel copied across the
    // boundary and reported success, which made `mv`'s cross-filesystem path
    // -- the several hundred lines that exist precisely for this -- dead code
    // on the target. Inverted with the fix, in the same commit, because a
    // behaviour change that leaves its own test asserting the old behaviour
    // fails two hours later in a boot test rather than at the edit.
    //
    // The two assertions after the error code are the ones that matter: a
    // refusal must leave the source where it was and must not leave a partial
    // destination behind. A refusal that consumed either would be worse than
    // the copy it replaced.
    if has_tmp {
        serial_println!("[vfs]   Testing cross-mount rename refusal...");

        let src_path = "/tmp/_vfs_xmv_src.txt";
        let dst_path = "/_vfs_xmv_dst.txt";
        Vfs::write_file(src_path, b"cross mount data")?;

        match Vfs::rename(src_path, dst_path) {
            Err(KernelError::CrossDevice) => {
                serial_println!("[vfs]     cross-mount rename: refused with CrossDevice");
            }
            Ok(()) => {
                serial_println!(
                    "[vfs]   FAIL: cross-mount rename succeeded; it must answer CrossDevice"
                );
                let _ = Vfs::remove(src_path);
                let _ = Vfs::remove(dst_path);
                return Err(KernelError::InternalError);
            }
            Err(e) => {
                serial_println!(
                    "[vfs]   FAIL: cross-mount rename -> {:?}, expected CrossDevice",
                    e
                );
                let _ = Vfs::remove(src_path);
                return Err(KernelError::InternalError);
            }
        }

        // The source must survive a refusal, with its contents intact.
        match Vfs::read_file(src_path) {
            Ok(data) if data == b"cross mount data" => {
                serial_println!("[vfs]     cross-mount rename: source left intact");
            }
            Ok(data) => {
                serial_println!(
                    "[vfs]   FAIL: source damaged by a refused rename ({} bytes)",
                    data.len()
                );
                let _ = Vfs::remove(src_path);
                return Err(KernelError::InternalError);
            }
            Err(e) => {
                serial_println!("[vfs]   FAIL: source gone after a refused rename: {:?}", e);
                return Err(KernelError::InternalError);
            }
        }

        // And nothing may have been created at the destination.
        match Vfs::stat(dst_path) {
            Err(KernelError::NotFound) => {
                serial_println!("[vfs]     cross-mount rename: no partial destination");
            }
            _ => {
                serial_println!("[vfs]   FAIL: refused rename still created {}", dst_path);
                let _ = Vfs::remove(src_path);
                let _ = Vfs::remove(dst_path);
                return Err(KernelError::InternalError);
            }
        }

        let _ = Vfs::remove(src_path);
        serial_println!("[vfs]     cross-mount rename refusal test PASSED");
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

        // Every listed inode must equal the one `metadata` reports for the
        // same name.  This is the invariant `DirEntry::ino` exists to hold:
        // userspace cross-checks `d_ino` against `st_ino` in `find -inum`,
        // in `ls -i` versus `stat`, and in the hard-link detection of `tar`,
        // `rsync` and `du`, and a mismatch is silent in all of them — no
        // error code is produced, the wrong answer is simply believed.  A
        // listing that reported a *synthesised* number would pass every
        // other assertion in this test.
        for entry in &all {
            let child = format!("{}/{}", pg_dir, entry.name.display());
            let meta = Vfs::metadata(&child)?;
            if entry.ino != meta.ino {
                serial_println!(
                    "[vfs]   FAIL: {} d_ino={} but st_ino={}",
                    child,
                    entry.ino,
                    meta.ino
                );
                let _ = Vfs::remove_recursive(pg_dir);
                return Err(KernelError::InternalError);
            }
        }
        // memfs assigns every node a distinct number at creation, so a zero
        // here would mean the listing lost it rather than that the backing
        // filesystem had none to give.
        if all.iter().any(|e| e.ino == 0) {
            serial_println!("[vfs]   FAIL: memfs listing reported ino=0");
            let _ = Vfs::remove_recursive(pg_dir);
            return Err(KernelError::InternalError);
        }
        serial_println!("[vfs]     readdir_at d_ino == st_ino for all 10 entries OK");

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

    // --- Device numbers (st_dev) ---
    if has_tmp {
        device_numbers_self_test()?;
    }

    // --- Permission gate: POSIX ACL enforcement ---
    if has_tmp {
        acl_gate_self_test()?;
    }

    // --- RESOLVE_BENEATH containment, per hop ---
    //
    // Not gated on `has_tmp`: `beneath_step` is a pure function over a path
    // fragment, so it needs no mount and can never be skipped.  The cases are
    // lane B's measured table (see the doc comment on `beneath_step`), and the
    // three marked ✗ there are the ones a "canonicalise and compare the
    // prefix" implementation gets *wrong by allowing* — which is why they are
    // asserted here rather than left to read as obvious.
    {
        // `depth` is where the walk already sits below the base; 0 is the base
        // itself, which is what a fresh caller-supplied relative path starts at.
        let allow = |depth: usize, frag: &str, want: usize| -> KernelResult<()> {
            match Vfs::beneath_step(depth, Path::new(frag)) {
                Ok(d) if d == want => Ok(()),
                Ok(d) => {
                    serial_println!(
                        "[vfs]   FAIL: beneath_step({}, {:?}) = {}, want {}",
                        depth,
                        frag,
                        d,
                        want
                    );
                    Err(KernelError::InvalidArgument)
                }
                Err(e) => {
                    serial_println!(
                        "[vfs]   FAIL: beneath_step({}, {:?}) refused ({:?}), want {}",
                        depth,
                        frag,
                        e,
                        want
                    );
                    Err(e)
                }
            }
        };
        let refuse = |depth: usize, frag: &str| -> KernelResult<()> {
            match Vfs::beneath_step(depth, Path::new(frag)) {
                Err(KernelError::CrossDevice) => Ok(()),
                other => {
                    serial_println!(
                        "[vfs]   FAIL: beneath_step({}, {:?}) = {:?}, want CrossDevice",
                        depth,
                        frag,
                        other
                    );
                    Err(KernelError::InvalidArgument)
                }
            }
        };

        // Rows that stay inside, and where a prefix check agrees.
        allow(0, "sub", 1)?;
        allow(0, "deep/../sub", 1)?;
        // `..` all the way back to the base is allowed: it never rises above.
        allow(0, "deep/er/../..", 0)?;
        allow(0, "./sub/./deeper", 2)?;

        // The three a prefix check would wrongly allow.  The first two are
        // absolute targets that happen to name something inside the base --
        // refused without comparison, because the caller asked for a walk that
        // cannot address anything from the root.
        refuse(0, "/tmp/base/sub")?;
        refuse(0, "/tmp/base")?;
        // And the one that steps above the base and comes straight back in.
        // A resolved path cannot tell this from `sub`; a depth counter can.
        refuse(0, "../base/sub")?;

        // Plain escapes, which every implementation gets right.
        refuse(0, "../out")?;
        refuse(0, "/tmp")?;
        refuse(1, "../../out")?;

        // Depth carries across hops, which is what makes a symlink chain
        // decidable: two hops that are each individually fine.
        let d = Vfs::beneath_step(0, Path::new("a/b"))?;
        allow(d, "../c", 2)?;
        // ...and a second hop that escapes from where the first one left off.
        refuse(d, "../../..")?;

        serial_println!(
            "[vfs]   RESOLVE_BENEATH containment is per-hop and syntactic: OK \
             (absolute target refused even inside the base, `..` refused where \
             it steps above rather than where it lands)"
        );
    }

    // --- RESOLVE_BENEATH end-to-end, through real symlinks on disk ---
    //
    // The section above tests the rule; this one tests that the resolver
    // actually consults it, which is the half a pure-function test cannot
    // reach.  Every refusal below is a symlink the walk really reads and
    // really declines to follow.
    if has_tmp {
        let base = "/tmp/_beneath/base";
        let cleanup = || {
            for p in [
                "/tmp/_beneath/base/escape",
                "/tmp/_beneath/base/updown",
                "/tmp/_beneath/base/abs_inside",
                "/tmp/_beneath/base/updown_in",
                "/tmp/_beneath/base/rel_in",
                "/tmp/_beneath/base/sub/f",
                "/tmp/_beneath/out/f",
            ] {
                let _ = Vfs::remove(p);
            }
            for d in [
                "/tmp/_beneath/base/sub",
                "/tmp/_beneath/base",
                "/tmp/_beneath/out",
                "/tmp/_beneath",
            ] {
                let _ = Vfs::rmdir(d);
            }
        };
        cleanup();

        let run = || -> KernelResult<()> {
            Vfs::mkdir("/tmp/_beneath")?;
            Vfs::mkdir("/tmp/_beneath/base")?;
            Vfs::mkdir("/tmp/_beneath/base/sub")?;
            Vfs::mkdir("/tmp/_beneath/out")?;
            Vfs::write_file("/tmp/_beneath/base/sub/f", b"inside")?;
            Vfs::write_file("/tmp/_beneath/out/f", b"outside")?;

            // Two that must be followed, and three that must not.  The
            // three are the interesting ones: each points at a path that
            // exists and is readable, so nothing but the containment rule
            // stops them.
            Vfs::symlink("/tmp/_beneath/base/rel_in", "sub")?;
            Vfs::symlink("/tmp/_beneath/base/updown_in", "deep/../sub")?;
            Vfs::symlink("/tmp/_beneath/base/abs_inside", "/tmp/_beneath/base/sub")?;
            Vfs::symlink("/tmp/_beneath/base/updown", "../base/sub")?;
            Vfs::symlink("/tmp/_beneath/base/escape", "../out")?;

            let allow = |rel: &str| -> KernelResult<()> {
                match Vfs::resolve_beneath(base, rel, true, false) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        serial_println!("[vfs]   FAIL: beneath {:?} refused ({:?})", rel, e);
                        Err(e)
                    }
                }
            };
            let refuse = |rel: &str| -> KernelResult<()> {
                match Vfs::resolve_beneath(base, rel, true, false) {
                    Err(KernelError::CrossDevice) => Ok(()),
                    other => {
                        serial_println!(
                            "[vfs]   FAIL: beneath {:?} = {:?}, want CrossDevice",
                            rel,
                            other
                        );
                        Err(KernelError::InvalidArgument)
                    }
                }
            };

            // Plain walks, no symlink involved.
            allow("sub/f")?;
            refuse("../out/f")?;
            // An absolute `rel` is refused, not reinterpreted -- even though
            // this exact path is the one `sub/f` resolves to.
            refuse("/tmp/_beneath/base/sub/f")?;

            // Through symlinks that stay inside.
            allow("rel_in/f")?;
            allow("updown_in/f")?;

            // Through symlinks that do not.  `abs_inside` and `updown` both
            // name a file *inside* the base -- they are refused for how they
            // say it, which is the behaviour a prefix check cannot produce.
            refuse("abs_inside/f")?;
            refuse("updown/f")?;
            refuse("escape/f")?;

            // Containment does not disturb the ordinary resolvers: the same
            // escaping link is still followed when nobody asked for a base.
            let unconfined = Vfs::resolve_follow(Path::new("/tmp/_beneath/base/escape/f"))?;
            if unconfined != PathBuf::from("/tmp/_beneath/out/f") {
                serial_println!(
                    "[vfs]   FAIL: unconfined resolve changed: {}",
                    unconfined.display()
                );
                return Err(KernelError::InvalidArgument);
            }
            Ok(())
        };

        let result = run();
        cleanup();
        result?;

        serial_println!(
            "[vfs]   RESOLVE_BENEATH through real symlinks: OK (a link naming a \
             file inside the base is still refused when it says so absolutely \
             or by leaving and returning; unconfined resolution unchanged)"
        );
    } else {
        skips.record(
            "RESOLVE_BENEATH end-to-end symlink walk",
            "/tmp not mounted",
        );
    }

    // --- Fd-relative primitives verify a pinned directory identity ---
    //
    // The case that matters is the one a path-based `unlinkat` gets wrong and
    // cannot be made to get right: the directory the handle was opened on is
    // moved aside and a *different* directory takes its name.  A primitive
    // that re-resolves the name deletes out of the impostor and reports
    // success.  This section builds exactly that situation and requires the
    // removal to be refused, then requires the impostor's file to still be
    // there -- because "refused" and "deleted the wrong file but returned an
    // error" are indistinguishable from the return value alone.
    if has_tmp {
        let cleanup = || {
            for p in ["/tmp/_pin/real/victim", "/tmp/_pin/decoy/victim"] {
                let _ = Vfs::remove(p);
            }
            for d in ["/tmp/_pin/real", "/tmp/_pin/decoy", "/tmp/_pin"] {
                let _ = Vfs::rmdir(d);
            }
        };
        cleanup();

        let run = || -> KernelResult<()> {
            Vfs::mkdir("/tmp/_pin")?;
            Vfs::mkdir("/tmp/_pin/real")?;
            Vfs::write_file("/tmp/_pin/real/victim", b"original")?;

            let pin = Vfs::pin_dir("/tmp/_pin/real")?;
            if !Vfs::pinned_dir_is_verifiable(&pin) {
                // memfs assigns synthetic inodes, so /tmp must be pinnable.
                // If this ever fires, every assertion below is vacuous and
                // saying so is more useful than passing.
                serial_println!(
                    "[vfs]   FAIL: /tmp/_pin/real has no pinnable identity — the \
                     stale-handle assertions below would all pass vacuously"
                );
                return Err(KernelError::InvalidArgument);
            }

            // The two read primitives answer *correctly* through a live pin.
            // Without this pair, an implementation that returned StaleHandle
            // unconditionally would satisfy every assertion further down --
            // the swap assertions only prove a refusal happens, not that it
            // happens for the right reason.
            let pinned_meta = match Vfs::metadata_at_pinned(&pin, b"victim", false) {
                Ok(m) if m.size == 8 => m,
                other => {
                    serial_println!(
                        "[vfs]   FAIL: metadata_at_pinned through a live pin = {:?}, want size 8",
                        other.map(|m| m.size)
                    );
                    return Err(KernelError::InvalidArgument);
                }
            };

            // The pinned lookup must carry the *identity* fields, not just the
            // size.  `SYS_FS_FSTATAT_PINNED` (663) exists to back a race-free
            // `fstatat`, which fills a `struct stat`; if this path returned a
            // `FileMeta` whose `ino` were zero, widening 663's record to the
            // 80-byte stat layout (§653) would have bought nothing, and the
            // failure would be silent -- a zero inode is a plausible value, so
            // `cp` refuses legitimate copies and `find -samefile` matches
            // everything, with no error anywhere.  Compared against the
            // path-based answer rather than merely checked non-zero, because
            // the two routes reaching *different* inodes for one file is the
            // same bug wearing a different mask.
            let path_meta = match Vfs::metadata("/tmp/_pin/real/victim") {
                Ok(m) => m,
                Err(e) => {
                    serial_println!(
                        "[vfs]   FAIL: path-based metadata on the file the pin just described = \
                         {e:?} -- the comparison below has nothing to compare against"
                    );
                    return Err(e);
                }
            };
            if pinned_meta.ino == 0 {
                serial_println!(
                    "[vfs]   FAIL: metadata_at_pinned returned ino 0 on a filesystem that \
                     assigns inodes -- 663's stat record would report st_ino == 0"
                );
                return Err(KernelError::InvalidArgument);
            }
            if pinned_meta.ino != path_meta.ino {
                serial_println!(
                    "[vfs]   FAIL: metadata_at_pinned ino {} != path-based stat ino {} for one file",
                    pinned_meta.ino,
                    path_meta.ino
                );
                return Err(KernelError::InvalidArgument);
            }
            if pinned_meta.nlinks != path_meta.nlinks {
                serial_println!(
                    "[vfs]   FAIL: metadata_at_pinned nlinks {} != path-based stat nlinks {}",
                    pinned_meta.nlinks,
                    path_meta.nlinks
                );
                return Err(KernelError::InvalidArgument);
            }
            match Vfs::readdir_pinned(&pin) {
                Ok(v) if v.iter().any(|e| e.name.as_bytes() == b"victim") => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: readdir_pinned through a live pin did not list `victim` \
                         ({:?} entries)",
                        other.map(|v| v.len())
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }

            // chmod through a live pin, and the twelve-bit question. Masking a
            // mode to nine bits drops setuid with no error (§639), which is the
            // worst way for a permission request to fail; the syscall masks to
            // `0o7777`, but that is only worth anything if the filesystem
            // underneath actually stores the high three. So this asserts the
            // bit survives the round trip rather than assuming it does.
            Vfs::set_permissions_at_pinned(&pin, b"victim", 0o4751, false)?;
            match Vfs::metadata_at_pinned(&pin, b"victim", false) {
                Ok(m) if m.permissions == 0o4751 => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: set_permissions_at_pinned(0o4751) then read back = {:?}, \
                         want 0o4751 -- setuid must survive, not be silently masked off",
                        other.map(|m| m.permissions)
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }

            // The same one-component containment `unlink` has. `..` is the
            // load-bearing one: a chmod that climbed out of the verified
            // directory would be the exact privilege escalation the pin exists
            // to stop.
            for bad in [&b".."[..], b".", b"a/b", b"", b"/etc"] {
                match Vfs::set_permissions_at_pinned(&pin, bad, 0o777, false) {
                    Err(KernelError::InvalidArgument) => {}
                    other => {
                        serial_println!(
                            "[vfs]   FAIL: set_permissions_at_pinned(.., {:?}) = {:?}, \
                             want InvalidArgument",
                            core::str::from_utf8(bad).unwrap_or("<non-utf8>"),
                            other
                        );
                        return Err(KernelError::InvalidArgument);
                    }
                }
            }

            // A single component in the directory it was opened on: works.
            Vfs::unlink_at_pinned(&pin, b"victim", false)?;
            if Vfs::stat("/tmp/_pin/real/victim").is_ok() {
                serial_println!("[vfs]   FAIL: unlink_at_pinned did not remove the file");
                return Err(KernelError::InvalidArgument);
            }

            // Anything that is not one component is refused, because the
            // containment rests on it. `..` is the load-bearing one: it would
            // climb out of the directory whose identity was just verified.
            for bad in [&b".."[..], b".", b"a/b", b"", b"/etc"] {
                match Vfs::unlink_at_pinned(&pin, bad, false) {
                    Err(KernelError::InvalidArgument) => {}
                    other => {
                        serial_println!(
                            "[vfs]   FAIL: unlink_at_pinned(.., {:?}) = {:?}, want InvalidArgument",
                            core::str::from_utf8(bad).unwrap_or("<non-utf8>"),
                            other
                        );
                        return Err(KernelError::InvalidArgument);
                    }
                }
            }

            // Now the swap. The pinned directory moves aside; a fresh, empty
            // directory takes its name and is given a file of the same name.
            // `pin.path` is unchanged and still resolves -- to the impostor.
            Vfs::rename("/tmp/_pin/real", "/tmp/_pin/decoy")?;
            Vfs::mkdir("/tmp/_pin/real")?;
            Vfs::write_file("/tmp/_pin/real/victim", b"must survive")?;

            match Vfs::unlink_at_pinned(&pin, b"victim", false) {
                Err(KernelError::StaleHandle) => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: unlink_at_pinned through a swapped directory = {:?}, \
                         want StaleHandle",
                        other
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }
            // The refusal has to mean nothing happened, not merely that
            // something was reported.
            match Vfs::read_file("/tmp/_pin/real/victim") {
                Ok(b) if b == b"must survive" => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: the stale unlink still touched the impostor's file ({:?})",
                        other.map(|b| b.len())
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }

            // The other two primitives answer the same way, so a caller
            // cannot read through a handle it may not delete through.
            match Vfs::metadata_at_pinned(&pin, b"victim", false) {
                Err(KernelError::StaleHandle) => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: metadata_at_pinned through a swapped directory = {:?}, \
                         want StaleHandle",
                        other.map(|m| m.size)
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }
            match Vfs::readdir_pinned(&pin) {
                Err(KernelError::StaleHandle) => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: readdir_pinned through a swapped directory = {:?}, \
                         want StaleHandle",
                        other.map(|v| v.len())
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }

            // chmod is the one where a wrong answer is a privilege escalation
            // rather than an information leak, so it gets the stronger form of
            // the assertion: refused, *and* the impostor's file is provably
            // untouched. A `chmod -R` that reported an error but had already
            // set the setuid bit would have failed in the way that matters.
            let before = match Vfs::metadata("/tmp/_pin/real/victim") {
                Ok(m) => m.permissions,
                Err(e) => {
                    serial_println!(
                        "[vfs]   FAIL: cannot read the impostor's mode before the stale chmod \
                         ({e:?}) -- the comparison below would have nothing to compare against"
                    );
                    return Err(e);
                }
            };
            match Vfs::set_permissions_at_pinned(&pin, b"victim", 0o4777, false) {
                Err(KernelError::StaleHandle) => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: set_permissions_at_pinned through a swapped directory = \
                         {other:?}, want StaleHandle"
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }
            let after = match Vfs::metadata("/tmp/_pin/real/victim") {
                Ok(m) => m.permissions,
                Err(e) => {
                    serial_println!(
                        "[vfs]   FAIL: the impostor's file is unreadable after the stale chmod \
                         ({e:?}) -- a refusal must leave it exactly as it was"
                    );
                    return Err(e);
                }
            };
            if after != before {
                serial_println!(
                    "[vfs]   FAIL: the stale chmod was reported as refused but still changed the \
                     impostor's mode ({before:#o} -> {after:#o})"
                );
                return Err(KernelError::InvalidArgument);
            }

            // A pin taken on the impostor is live again, and unlink without
            // AT_REMOVEDIR still refuses a directory rather than leaving the
            // choice to whatever the filesystem's `remove` happens to do.
            let fresh = Vfs::pin_dir("/tmp/_pin/real")?;
            Vfs::mkdir("/tmp/_pin/real/sub")?;
            match Vfs::unlink_at_pinned(&fresh, b"sub", false) {
                Err(KernelError::IsADirectory) => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: unlink_at_pinned on a directory without AT_REMOVEDIR = \
                         {:?}, want IsADirectory",
                        other
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }
            Vfs::unlink_at_pinned(&fresh, b"sub", true)?;
            if Vfs::stat("/tmp/_pin/real/sub").is_ok() {
                serial_println!("[vfs]   FAIL: unlink_at_pinned(AT_REMOVEDIR) did not rmdir");
                return Err(KernelError::InvalidArgument);
            }

            // And a pin on a plain file is not a directory pin.
            Vfs::write_file("/tmp/_pin/real/plain", b"x")?;
            match Vfs::pin_dir("/tmp/_pin/real/plain") {
                Err(KernelError::NotADirectory) => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: pin_dir on a file = {:?}, want NotADirectory",
                        other.map(|p| p.id.is_some())
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }
            let _ = Vfs::remove("/tmp/_pin/real/plain");
            Ok(())
        };

        let result = run();
        let _ = Vfs::rmdir("/tmp/_pin/real/sub");
        cleanup();
        result?;

        serial_println!(
            "[vfs]   pinned fd-relative primitives: OK (a directory swapped out \
             from under the pin is refused with StaleHandle and the impostor's \
             file and mode are untouched; `..`, `.`, `a/b` and an empty name \
             are refused; a live pin reports a matching inode and preserves \
             setuid through chmod)"
        );
    } else {
        skips.record("pinned fd-relative primitives", "/tmp not mounted");
    }

    // --- The pinned set a recursive copy needs (mkdir/symlink/link/utimens) ---
    //
    // Separate from the section above because the failure it guards against is
    // a different one.  For `unlink` and `chmod` a stale handle means the wrong
    // *existing* object is modified.  For these four it means a new object is
    // **created** somewhere the caller never named -- so "refused" is only
    // half the assertion, and every stale case below also requires the impostor
    // directory to be provably empty afterwards.  A primitive that reported
    // StaleHandle after already creating the entry would satisfy the return
    // value and fail at the only thing that matters.
    if has_tmp {
        let cleanup = || {
            for p in [
                "/tmp/_pin2/dst/hard",
                "/tmp/_pin2/dst/link",
                "/tmp/_pin2/dst/stamped",
                "/tmp/_pin2/dst/moved",
                "/tmp/_pin2/src/f",
                "/tmp/_pin2/src/mv_me",
                "/tmp/_pin2/src/mv_me2",
            ] {
                let _ = Vfs::remove(p);
            }
            for d in [
                "/tmp/_pin2/dst/made",
                "/tmp/_pin2/dst",
                "/tmp/_pin2/aside",
                "/tmp/_pin2/src",
                "/tmp/_pin2",
            ] {
                let _ = Vfs::rmdir(d);
            }
        };
        cleanup();

        let run = || -> KernelResult<()> {
            Vfs::mkdir("/tmp/_pin2")?;
            Vfs::mkdir("/tmp/_pin2/src")?;
            Vfs::mkdir("/tmp/_pin2/dst")?;
            Vfs::write_file("/tmp/_pin2/src/f", b"payload")?;

            let src = Vfs::pin_dir("/tmp/_pin2/src")?;
            let dst = Vfs::pin_dir("/tmp/_pin2/dst")?;
            if !Vfs::pinned_dir_is_verifiable(&src) || !Vfs::pinned_dir_is_verifiable(&dst) {
                serial_println!(
                    "[vfs]   FAIL: /tmp/_pin2 has no pinnable identity — every stale-handle \
                     assertion below would pass vacuously"
                );
                return Err(KernelError::InvalidArgument);
            }

            // mkdirat through a live pin, with a mode that is *not* the 0o755
            // default.  Asserting the mode rather than only the existence is
            // what proves the create-then-stamp pair happened at all: the
            // filesystem's own `mkdir` stamps 0o755, so a primitive that forgot
            // the stamp would still create the directory and still "pass" an
            // existence check.
            Vfs::mkdir_at_pinned(&dst, b"made", 0o700)?;
            match Vfs::metadata_at_pinned(&dst, b"made", false) {
                Ok(m) if m.entry_type == EntryType::Directory && m.permissions == 0o700 => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: mkdir_at_pinned(0o700) then read back = {:?}, want a \
                         directory with mode 0o700",
                        other.map(|m| (m.entry_type, m.permissions))
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }

            // Sticky survives the create, on *both* routes, and the two routes
            // agree.  This is asserted because the opposite was true and
            // nothing noticed: §639 widened `sys_fs_mkdir_mode`'s mask on
            // 2026-08-30, but `mkdir_mode` and `mkdir_at_pinned` both went on
            // masking to `0o777` one layer below, so the widening reached
            // nothing.  A test that only checks `0o700` (as the one above does)
            // cannot see that, because every bit it asserts is inside `0o777`.
            //
            // The `!= DEFAULT_DIR_MODE` guard on the stamp is the other reason
            // to use a mode whose low nine bits are `0o755`: `0o1755` must
            // still be stamped, and a guard comparing the *masked* value
            // against the default would skip it and leave a plain 0o755
            // directory behind.
            Vfs::mkdir_at_pinned(&dst, b"sticky", 0o1755)?;
            match Vfs::metadata_at_pinned(&dst, b"sticky", false) {
                Ok(m) if m.permissions == 0o1755 => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: mkdir_at_pinned(0o1755) read back = {:?}, want mode \
                         0o1755 — the sticky bit was dropped between the handler and the disk",
                        other.map(|m| m.permissions)
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }
            Vfs::mkdir_mode("/tmp/_pin2/dst/sticky_path", 0o1755)?;
            match Vfs::metadata("/tmp/_pin2/dst/sticky_path") {
                Ok(m) if m.permissions == 0o1755 => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: mkdir_mode(0o1755) read back = {:?}, want mode 0o1755 — \
                         the path route drops sticky where the pinned route keeps it",
                        other.map(|m| m.permissions)
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }

            // setgid is refused by both routes, which is the half of §663 that
            // is a *narrowing*.  `mkdir(2)` takes a new directory's setgid bit
            // from its parent, never from the mode word; accepting it here
            // would offer an authority Linux does not, in the bit that decides
            // who owns files created in the directory later.  Asserted rather
            // than assumed because the mask that drops it is one character
            // different from the mask that keeps it.
            Vfs::mkdir_at_pinned(&dst, b"nosgid", 0o2755)?;
            match Vfs::metadata_at_pinned(&dst, b"nosgid", false) {
                Ok(m) if m.permissions == 0o755 => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: mkdir_at_pinned(0o2755) read back = {:?}, want mode \
                         0o755 — setgid must not be settable from a directory create mode",
                        other.map(|m| m.permissions)
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }

            // symlinkat: the *name* is one component, the *target* is not
            // constrained at all.  This is the asymmetry the design turns on,
            // and it is asserted in both directions -- a target containing `..`
            // and `/` is stored verbatim, while the same bytes as a *name* are
            // refused.  An implementation that ran `check_at_name` over the
            // target would pass every other test here and quietly make
            // `symlinkat` unable to reproduce the relative links a recursive
            // copy is copying.
            Vfs::symlink_at_pinned(&dst, b"link", "../src/f")?;
            match Vfs::readlink("/tmp/_pin2/dst/link") {
                Ok(t) if t.as_path() == Path::new("../src/f") => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: symlink_at_pinned stored target = {:?}, want `../src/f` \
                         verbatim",
                        other.map(|t| t.len())
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }

            // linkat: the new name is a second name for the same inode, which
            // is checked by inode equality and link count rather than by
            // content -- two files that merely happen to hold the same bytes
            // would pass a content check and be a completely different (and
            // wrong) result.
            //
            // This used to accept `NotSupported` as a skip: memfs stored its
            // tree as owned nodes -- a directory held its children by value --
            // so two names could not denote one node. memfs now keeps a flat
            // inode table with directories holding name -> ino, so the skip is
            // gone and the assertion runs. It is stated unconditionally on
            // purpose: an assertion that tolerates a filesystem declining to
            // do the thing is an assertion that passes when the thing stops
            // working.
            Vfs::link_at_pinned(&src, b"f", &dst, b"hard", false)?;
            let src_meta = Vfs::metadata("/tmp/_pin2/src/f")?;
            let hard_meta = Vfs::metadata("/tmp/_pin2/dst/hard")?;
            if src_meta.ino == 0 || src_meta.ino != hard_meta.ino {
                serial_println!(
                    "[vfs]   FAIL: link_at_pinned produced ino {} for a link to ino {} — a hard \
                     link must be the same inode, not a copy",
                    hard_meta.ino,
                    src_meta.ino
                );
                return Err(KernelError::InvalidArgument);
            }
            if hard_meta.nlinks < 2 {
                serial_println!(
                    "[vfs]   FAIL: link_at_pinned left nlinks = {}, want at least 2 — `rm` uses \
                     this to decide whether removing a name destroys the data",
                    hard_meta.nlinks
                );
                return Err(KernelError::InvalidArgument);
            }

            // utimensat, including the zero-means-unchanged convention: the
            // modification time is set and the access time is left alone in the
            // same call, so a primitive that stamped both from one argument
            // fails here rather than in a user's backup years later.
            //
            // Stamped on a file created for the purpose rather than on the hard
            // link above, so that this assertion does not silently vanish on a
            // filesystem that cannot make the link.
            Vfs::write_file("/tmp/_pin2/dst/stamped", b"t")?;
            let stamp_meta = Vfs::metadata("/tmp/_pin2/dst/stamped")?;
            let want_mtime: Timestamp = 1_000_000_000;
            let before_atime = stamp_meta.accessed_ns;
            Vfs::set_times_at_pinned(&dst, b"stamped", 0, want_mtime, false)?;
            match Vfs::metadata_at_pinned(&dst, b"stamped", false) {
                Ok(m) if m.modified_ns == want_mtime && m.accessed_ns == before_atime => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: set_times_at_pinned(0, {want_mtime}) gave {:?}, want \
                         mtime {want_mtime} with atime left at {before_atime}",
                        other.map(|m| (m.accessed_ns, m.modified_ns))
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }

            // renameat, all three modes.  Unlike the four above, this one
            // *removes* a name as well as creating one, so every assertion
            // below checks both ends: a rename that created the destination
            // without unlinking the source would leave a copy, and one that
            // unlinked without creating would lose the file outright.
            Vfs::write_file("/tmp/_pin2/src/mv_me", b"cargo")?;
            Vfs::rename_at_pinned(&src, b"mv_me", &dst, b"moved", RenameMode::Replace)?;
            match (
                Vfs::metadata("/tmp/_pin2/src/mv_me"),
                Vfs::read_file("/tmp/_pin2/dst/moved"),
            ) {
                (Err(KernelError::NotFound), Ok(v)) if v == b"cargo" => {}
                (from, to) => {
                    serial_println!(
                        "[vfs]   FAIL: rename_at_pinned left source {:?} and destination {:?}, \
                         want the source gone and the destination holding the payload",
                        from.map(|_| "present"),
                        to.map(|v| v.len())
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }

            // NoReplace onto a name that is taken.  Both names must survive
            // untouched: the failure mode worth catching is a primitive that
            // unlinks the source first and only then discovers the destination
            // is occupied, which loses the file and reports an error for it.
            Vfs::write_file("/tmp/_pin2/src/mv_me", b"second")?;
            match Vfs::rename_at_pinned(&src, b"mv_me", &dst, b"moved", RenameMode::NoReplace) {
                Err(KernelError::AlreadyExists) => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: rename_at_pinned(NoReplace) onto a taken name = {other:?}, \
                         want AlreadyExists"
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }
            match (
                Vfs::read_file("/tmp/_pin2/src/mv_me"),
                Vfs::read_file("/tmp/_pin2/dst/moved"),
            ) {
                (Ok(a), Ok(b)) if a == b"second" && b == b"cargo" => {}
                (a, b) => {
                    serial_println!(
                        "[vfs]   FAIL: a refused NoReplace rename disturbed the tree — source \
                         {:?}, destination {:?}, want both exactly as they were",
                        a.map(|v| v.len()),
                        b.map(|v| v.len())
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }

            // Exchange: both names stay, their contents swap.  Checked by
            // content in *both* directions, since an implementation that moved
            // one way and dropped the other would satisfy a one-sided check.
            Vfs::rename_at_pinned(&src, b"mv_me", &dst, b"moved", RenameMode::Exchange)?;
            match (
                Vfs::read_file("/tmp/_pin2/src/mv_me"),
                Vfs::read_file("/tmp/_pin2/dst/moved"),
            ) {
                (Ok(a), Ok(b)) if a == b"cargo" && b == b"second" => {}
                (a, b) => {
                    serial_println!(
                        "[vfs]   FAIL: rename_at_pinned(Exchange) gave source {:?} destination \
                         {:?}, want the two payloads swapped with both names still present",
                        a.map(|v| v.len()),
                        b.map(|v| v.len())
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }

            // The same handle on both sides -- a rename within one directory,
            // which is what `mv a b` in a single directory compiles to and the
            // case where a naive two-lock implementation would deadlock on
            // itself.
            Vfs::rename_at_pinned(&src, b"mv_me", &src, b"mv_me2", RenameMode::Replace)?;
            match (
                Vfs::metadata("/tmp/_pin2/src/mv_me"),
                Vfs::read_file("/tmp/_pin2/src/mv_me2"),
            ) {
                (Err(KernelError::NotFound), Ok(v)) if v == b"cargo" => {}
                (from, to) => {
                    serial_println!(
                        "[vfs]   FAIL: rename_at_pinned within one pinned directory left source \
                         {:?} destination {:?}",
                        from.map(|_| "present"),
                        to.map(|v| v.len())
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }

            // The one-component containment, on all four.  `..` is the
            // load-bearing entry: a create that climbed out of the verified
            // directory is the whole class of bug the pin exists to stop, and
            // for these four it would be a *write* outside the destination
            // rather than a read.
            for bad in [&b".."[..], b".", b"a/b", b"", b"/etc"] {
                let shown = core::str::from_utf8(bad).unwrap_or("<non-utf8>");
                match Vfs::mkdir_at_pinned(&dst, bad, 0o755) {
                    Err(KernelError::InvalidArgument) => {}
                    other => {
                        serial_println!(
                            "[vfs]   FAIL: mkdir_at_pinned(.., {shown:?}) = {other:?}, want \
                             InvalidArgument"
                        );
                        return Err(KernelError::InvalidArgument);
                    }
                }
                match Vfs::symlink_at_pinned(&dst, bad, "/tmp") {
                    Err(KernelError::InvalidArgument) => {}
                    other => {
                        serial_println!(
                            "[vfs]   FAIL: symlink_at_pinned(.., {shown:?}) = {other:?}, want \
                             InvalidArgument"
                        );
                        return Err(KernelError::InvalidArgument);
                    }
                }
                match Vfs::set_times_at_pinned(&dst, bad, 1, 1, false) {
                    Err(KernelError::InvalidArgument) => {}
                    other => {
                        serial_println!(
                            "[vfs]   FAIL: set_times_at_pinned(.., {shown:?}) = {other:?}, want \
                             InvalidArgument"
                        );
                        return Err(KernelError::InvalidArgument);
                    }
                }
                // Both of `linkat`'s names are checked, not just one: a
                // primitive that validated the source and forwarded the
                // destination unchecked would create the new name outside the
                // pinned directory, which is the more dangerous half.
                match Vfs::link_at_pinned(&src, bad, &dst, b"escaped", false) {
                    Err(KernelError::InvalidArgument) => {}
                    other => {
                        serial_println!(
                            "[vfs]   FAIL: link_at_pinned(src {shown:?}) = {other:?}, want \
                             InvalidArgument"
                        );
                        return Err(KernelError::InvalidArgument);
                    }
                }
                match Vfs::link_at_pinned(&src, b"f", &dst, bad, false) {
                    Err(KernelError::InvalidArgument) => {}
                    other => {
                        serial_println!(
                            "[vfs]   FAIL: link_at_pinned(dst {shown:?}) = {other:?}, want \
                             InvalidArgument"
                        );
                        return Err(KernelError::InvalidArgument);
                    }
                }
                // Both of rename's names too, and the source side is the one
                // that would be the worse bug: an uncontained source name is a
                // way to *unlink* something outside the pinned directory,
                // where an uncontained destination merely creates.
                match Vfs::rename_at_pinned(&src, bad, &dst, b"escaped", RenameMode::Replace) {
                    Err(KernelError::InvalidArgument) => {}
                    other => {
                        serial_println!(
                            "[vfs]   FAIL: rename_at_pinned(src {shown:?}) = {other:?}, want \
                             InvalidArgument"
                        );
                        return Err(KernelError::InvalidArgument);
                    }
                }
                match Vfs::rename_at_pinned(&src, b"f", &dst, bad, RenameMode::Replace) {
                    Err(KernelError::InvalidArgument) => {}
                    other => {
                        serial_println!(
                            "[vfs]   FAIL: rename_at_pinned(dst {shown:?}) = {other:?}, want \
                             InvalidArgument"
                        );
                        return Err(KernelError::InvalidArgument);
                    }
                }
            }
            // The containment loop above asked for 10 renames of `src/f` and
            // every one had to be refused, so the file is still there. Stated
            // as an assertion rather than assumed: if any of them had gone
            // through, the refusals loop would still have passed on the *next*
            // iteration's `InvalidArgument`, and the missing file would only
            // surface much later.
            if Vfs::metadata("/tmp/_pin2/src/f").is_err() {
                serial_println!(
                    "[vfs]   FAIL: src/f is gone after ten refused renames — a refusal that still \
                     unlinked the source is the same bug as one that still created"
                );
                return Err(KernelError::InvalidArgument);
            }

            // Now the swap, on the destination -- the pin that matters for a
            // recursive copy, because it is where new objects land.  `dst.path`
            // still resolves, to an impostor that must end up empty.
            Vfs::rename("/tmp/_pin2/dst", "/tmp/_pin2/aside")?;
            Vfs::mkdir("/tmp/_pin2/dst")?;

            match Vfs::mkdir_at_pinned(&dst, b"intruder", 0o755) {
                Err(KernelError::StaleHandle) => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: mkdir_at_pinned through a swapped directory = {other:?}, \
                         want StaleHandle"
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }
            match Vfs::symlink_at_pinned(&dst, b"intruder", "/etc/passwd") {
                Err(KernelError::StaleHandle) => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: symlink_at_pinned through a swapped directory = {other:?}, \
                         want StaleHandle"
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }
            match Vfs::link_at_pinned(&src, b"f", &dst, b"intruder", false) {
                Err(KernelError::StaleHandle) => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: link_at_pinned into a swapped destination = {other:?}, \
                         want StaleHandle"
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }
            match Vfs::set_times_at_pinned(&dst, b"intruder", 1, 1, false) {
                Err(KernelError::StaleHandle) => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: set_times_at_pinned through a swapped directory = \
                         {other:?}, want StaleHandle"
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }

            match Vfs::rename_at_pinned(&src, b"f", &dst, b"intruder", RenameMode::Replace) {
                Err(KernelError::StaleHandle) => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: rename_at_pinned into a swapped destination = {other:?}, \
                         want StaleHandle"
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }
            // Rename is the only member of the family whose refusal has to be
            // checked at the *source* as well: the other four cannot destroy
            // anything, so for them "refused" and "left the source alone" are
            // the same statement. Here they are not.
            if Vfs::metadata("/tmp/_pin2/src/f").is_err() {
                serial_println!(
                    "[vfs]   FAIL: a rename refused for a stale destination still unlinked the \
                     source — the file is now in neither place"
                );
                return Err(KernelError::InvalidArgument);
            }

            // The half that the return value cannot tell you.  Four of the
            // five calls above would have *created* `intruder` had they
            // re-resolved the name, so the impostor being empty is the real
            // assertion and the StaleHandle results are only corroboration.
            match Vfs::readdir("/tmp/_pin2/dst") {
                Ok(v) if v.is_empty() => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: the impostor directory is not empty after four refused \
                         pinned creates ({:?} entries) — a refusal that still wrote is the bug \
                         this whole family exists to prevent",
                        other.map(|v| v.len())
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }

            // And a stale *source* is refused too, not merely a stale
            // destination: the source pin is what says which file gets a second
            // name, and linking the wrong file is how a hard link becomes a way
            // to keep a deleted secret alive.
            let live_dst = Vfs::pin_dir("/tmp/_pin2/dst")?;
            let stale_src = {
                Vfs::mkdir("/tmp/_pin2/src/gone")?;
                let p = Vfs::pin_dir("/tmp/_pin2/src/gone")?;
                Vfs::rmdir("/tmp/_pin2/src/gone")?;
                Vfs::mkdir("/tmp/_pin2/src/gone")?;
                Vfs::write_file("/tmp/_pin2/src/gone/f", b"impostor")?;
                p
            };
            let stale_src_result = Vfs::link_at_pinned(&stale_src, b"f", &live_dst, b"x", false);
            let _ = Vfs::remove("/tmp/_pin2/src/gone/f");
            let _ = Vfs::rmdir("/tmp/_pin2/src/gone");
            match stale_src_result {
                // `NotFound` is also correct here on a filesystem that reuses
                // an inode number for the replacement directory: the pin then
                // genuinely still matches, and the removed directory's `f` is
                // genuinely absent. Both answers are refusals; what would be
                // wrong is `Ok`.
                Err(KernelError::StaleHandle | KernelError::NotFound) => {}
                other => {
                    serial_println!(
                        "[vfs]   FAIL: link_at_pinned from a stale *source* pin = {other:?}, \
                         want StaleHandle (or NotFound)"
                    );
                    return Err(KernelError::InvalidArgument);
                }
            }
            let _ = Vfs::remove("/tmp/_pin2/dst/x");
            Ok(())
        };

        let result = run();
        for p in ["/tmp/_pin2/dst/x", "/tmp/_pin2/src/gone/f"] {
            let _ = Vfs::remove(p);
        }
        let _ = Vfs::rmdir("/tmp/_pin2/src/gone");
        // The swap above leaves the original directory under `aside`, so its
        // contents have to go before either name can be removed.
        for p in [
            "/tmp/_pin2/aside/hard",
            "/tmp/_pin2/aside/link",
            "/tmp/_pin2/aside/stamped",
            "/tmp/_pin2/aside/moved",
            "/tmp/_pin2/dst/intruder",
        ] {
            let _ = Vfs::remove(p);
        }
        let _ = Vfs::rmdir("/tmp/_pin2/aside/made");
        cleanup();
        result?;

        serial_println!(
            "[vfs]   pinned mkdir/symlink/link/utimens/rename: OK (a swapped destination refuses \
             all five with StaleHandle and leaves the impostor directory empty; both of linkat's \
             and both of renameat's names are contained; a symlink target keeps `..` and `/` \
             while a link *name* with them is refused; mkdirat's mode is stamped, utimensat \
             leaves a zero argument alone, linkat shares an inode; renameat moves, refuses \
             NoReplace onto a taken name without disturbing either end, swaps under Exchange, \
             and never unlinks the source on a path it refuses)"
        );
    } else {
        skips.record(
            "pinned mkdir/symlink/link/utimens/rename (the set `cp -r` and `mv` need)",
            "/tmp not mounted",
        );
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

    // 6. The asymmetry `Vfs::link_inner` relies on: a third party who may read
    //    the file may *not* write it. That pair is what makes `Read` the right
    //    gate for a hard link's source and `Write` the wrong one — under a
    //    Write-based gate this user could not hard-link a file they are
    //    plainly allowed to read, which would break every read-only-source use
    //    of hard links (content-addressed stores, `cp -l`, dedup backups).
    //    Step 4 already established the read half; this is the write half, and
    //    the two together are the decision.
    match path_access_verdict(path, 4242, 4242, &[], PathAccess::Write) {
        Err(KernelError::PermissionDenied) => {}
        other => {
            serial_println!(
                "[vfs]     FAIL: ACL_OTHER (r--) got {:?} on write, expected PermissionDenied — \
                 a read/write split is what `link_inner`'s source gate depends on",
                other
            );
            cleanup();
            return Err(KernelError::InternalError);
        }
    }

    // 7. Removing the ACL restores unrestricted access, and drops the count
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

    // 8. A path with no ACL is unaffected — the gate fails open, deferring to
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
    serial_println!(
        "[vfs]     POSIX ACL enforcement: OK (deny, metadata-exempt, root bypass, read/write split)"
    );
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
/// Two names for one file must share one lock entry.
///
/// **This is the only test that exercises identity keying at all.** The other
/// lock self-tests use synthetic paths like `/test/a` which do not exist, so
/// `Vfs::file_identity` returns `NotFound` and `flock_resolved` falls back to
/// keying by path -- exactly the pre-2026-09-21 behaviour. Those tests pass
/// identically before and after the change, which makes them worthless as
/// evidence for it.
///
/// This one uses `/tmp` (memfs, which has real inodes) and a hard link, so
/// the two paths resolve to one `FileId`. It fails on the old path-keyed code
/// -- where the second `flock` would succeed, granting two exclusive locks on
/// one file -- and passes on the new.
fn test_flock_shares_one_entry_across_hard_links() -> KernelResult<()> {
    use crate::serial_println;
    const A: &[u8] = b"/tmp/flock-id-a";
    const B: &[u8] = b"/tmp/flock-id-b";

    // Best-effort cleanup from an earlier run; absence is fine.
    let _ = Vfs::remove(Path::new(A));
    let _ = Vfs::remove(Path::new(B));

    Vfs::write_file(Path::new(A), b"x")?;
    match crate::fs::selftest::classify(Vfs::link(Path::new(A), Path::new(B))) {
        crate::fs::selftest::Setup::Ready => {}
        // Only NotSupported/ReadOnlyFilesystem/NoSuchDevice reach here.
        crate::fs::selftest::Setup::Unsupported(e) => {
            serial_println!(
                "[vfs]   identity rung SKIPPED -- link() unsupported here: {:?}",
                e
            );
            let _ = Vfs::remove(Path::new(A));
            return Ok(());
        }
        // The system was ASKED and REFUSED. Reporting that as 'no hard
        // links here' would announce a cause never established.
        crate::fs::selftest::Setup::Failed(e) => {
            serial_println!("[vfs]   FAIL: link() refused with {:?}, which is not", e);
            serial_println!("[vfs]         'this system cannot'");
            let _ = Vfs::remove(Path::new(A));
            return Err(e);
        }
    }

    // Both names must resolve to the same identity, or the test below proves
    // nothing about identity keying.
    let ida = Vfs::file_identity(Path::new(A))?;
    let idb = Vfs::file_identity(Path::new(B))?;
    if ida.is_none() || ida != idb {
        serial_println!(
            "[vfs]   identity rung SKIPPED -- {:?} and {:?} differ or are absent",
            ida,
            idb
        );
        let _ = Vfs::remove(Path::new(A));
        let _ = Vfs::remove(Path::new(B));
        return Ok(());
    }

    // A third file, NOT a link to A, for the negative control below.
    const C: &[u8] = b"/tmp/flock-id-c";
    let _ = Vfs::remove(Path::new(C));
    Vfs::write_file(Path::new(C), b"x")?;

    Vfs::flock(Path::new(A), 1, LockType::Exclusive)?;
    let second = Vfs::flock(Path::new(B), 2, LockType::Exclusive);
    let unrelated = Vfs::flock(Path::new(C), 3, LockType::Exclusive);
    let _ = Vfs::funlock(Path::new(C), 3);
    let _ = Vfs::funlock(Path::new(A), 1);
    let _ = Vfs::remove(Path::new(C));
    let _ = Vfs::remove(Path::new(B));
    let _ = Vfs::remove(Path::new(A));

    // NEGATIVE CONTROL, checked first. An implementation in which every
    // second flock fails -- or one whose entry matcher matches anything --
    // refuses B for reasons having nothing to do with identity, and would
    // pass the assertion below while proving nothing (dd-954).
    if unrelated.is_err() {
        serial_println!("[vfs]   ERROR: control failed -- flock refused an UNRELATED");
        serial_println!("[vfs]          file, so refusing B is not evidence of identity");
        return Err(KernelError::InternalError);
    }

    if second.is_ok() {
        serial_println!("[vfs]   FAIL: flock on a second name for the same file succeeded --");
        serial_println!("[vfs]         two exclusive locks on one file");
        return Err(KernelError::InternalError);
    }
    serial_println!("[vfs]   identity rung OK -- flock keys on identity, not name (hard link)");
    Ok(())
}

pub fn mount_self_test() -> KernelResult<()> {
    use crate::serial_println;

    serial_println!("[vfs] Running mount/unmount self-test...");

    // The identity-keying rung. Wired here as a separate edit because the
    // applier that defined it had no assertion that it was CALLED -- the three
    // sibling appliers asserted `count == 2` (definition plus one call) and
    // this one did not, so it silently produced exactly the defect it exists
    // to close: a correct, tested function nothing invokes.
    test_flock_shares_one_entry_across_hard_links()?;

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

/// Unix-domain socket nodes ([`EntryType::Socket`]) on memfs and devfs:
/// [`Vfs::mknod_socket`] makes one with a stable identity, `stat` and a
/// listing say what it is, an existing name is refused, `open` and a whole
/// read are `ENXIO`, a rename keeps the identity (which is how a renamed
/// node still leads to its socket), `chmod` takes, and `remove` takes it away.
///
/// ext4 is not exercised here: no ext4 volume is writable this early in the
/// boot. Its `mknod_socket` is the same shape as its `symlink`, and the
/// socket rungs later in the boot bind on the root filesystem.
///
/// # Errors
///
/// `InternalError` naming the first check that failed.
pub fn socket_node_self_test() -> KernelResult<()> {
    use crate::serial_println;

    serial_println!("[vfs] Running socket-node self-test...");

    /// The checks, on the node `a` -- renamed to `b` -- in directory `dir`.
    fn exercise(fs: &str, dir: &str, a: &str, b: &str) -> Result<(), &'static str> {
        let id = Vfs::mknod_socket(a, 0o755).map_err(|_| "mknod_socket refused a free name")?;
        if id.ino == 0 {
            return Err("the node has no inode number");
        }
        let st = Vfs::stat(a).map_err(|_| "stat of the node failed")?;
        let meta = Vfs::metadata(a).map_err(|_| "metadata of the node failed")?;
        if st.entry_type != EntryType::Socket || meta.entry_type != EntryType::Socket {
            return Err("stat does not call the node a socket");
        }
        if meta.ino != id.ino || meta.permissions != 0o755 {
            return Err("the node's inode or mode is not what mknod_socket made");
        }
        if !matches!(Vfs::file_identity(a), Ok(Some(found)) if found == id) {
            return Err("the node's identity is not the one mknod_socket returned");
        }
        let listed = Vfs::readdir(dir).map_err(|_| "the directory could not be listed")?;
        let name = Path::new(a).file_name().unwrap_or(Path::new(""));
        if !listed
            .iter()
            .any(|e| e.name.as_path() == name && e.entry_type == EntryType::Socket)
        {
            return Err("the listing does not show the node as a socket");
        }
        if Vfs::mknod_socket(a, 0o755) != Err(KernelError::AlreadyExists) {
            return Err("an existing name was not refused");
        }
        match crate::fs::handle::open(a, crate::fs::handle::OpenFlags::READ) {
            Err(KernelError::NoSuchDeviceOrAddress) => {}
            Ok(h) => {
                // Opened by mistake: closing it is all that is left to do.
                let _ = crate::fs::handle::close(h);
                return Err("open of the node succeeded");
            }
            Err(_) => return Err("open of the node failed, but not with ENXIO"),
        }
        if Vfs::read_file(a) != Err(KernelError::NoSuchDeviceOrAddress) {
            return Err("a whole read of the node was not ENXIO");
        }
        Vfs::set_permissions(a, 0o666).map_err(|_| "chmod of the node failed")?;
        if Vfs::metadata(a).map(|m| m.permissions) != Ok(0o666) {
            return Err("chmod of the node did not take");
        }
        if fs == "memfs" {
            // devfs has no rename: its names are the kernel's, bar these.
            Vfs::rename(a, b).map_err(|_| "rename of the node failed")?;
            if !matches!(Vfs::file_identity(b), Ok(Some(found)) if found == id) {
                return Err("a renamed node lost its identity");
            }
            Vfs::remove(b).map_err(|_| "remove of the renamed node failed")?;
            if Vfs::stat(b).is_ok() {
                return Err("the removed node is still there");
            }
        } else {
            Vfs::remove(a).map_err(|_| "remove of the node failed")?;
            if Vfs::stat(a).is_ok() {
                return Err("the removed node is still there");
            }
        }
        Ok(())
    }

    let mp = "/_socknode_selftest";
    if Vfs::mounts()
        .iter()
        .any(|(p, _)| p.as_path() == Path::new(mp))
    {
        // A previous boot's leftover; the test needs the name.
        let _ = Vfs::unmount(mp);
    }
    crate::fs::memfs::mount(mp)?;
    let on_memfs = exercise(
        "memfs",
        mp,
        "/_socknode_selftest/a.sock",
        "/_socknode_selftest/b.sock",
    );
    // Teardown whatever happened; each may already be gone.
    let _ = Vfs::remove("/_socknode_selftest/a.sock");
    let _ = Vfs::remove("/_socknode_selftest/b.sock");
    let _ = Vfs::unmount(mp);
    if let Err(why) = on_memfs {
        serial_println!("[vfs]   FAIL: socket node on memfs: {}", why);
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[vfs]   socket node on memfs: made, seen, ENXIO to open, renamed, removed: OK"
    );

    let on_devfs = exercise(
        "devfs",
        "/dev",
        "/dev/_selftest.sock",
        "/dev/_selftest.sock",
    );
    // Gone already unless a check failed first.
    let _ = Vfs::remove("/dev/_selftest.sock");
    if let Err(why) = on_devfs {
        serial_println!("[vfs]   FAIL: socket node on devfs: {}", why);
        return Err(KernelError::InternalError);
    }
    if Vfs::mknod_socket("/dev/input/_selftest.sock", 0o755) != Err(KernelError::NotSupported) {
        let _ = Vfs::remove("/dev/input/_selftest.sock");
        serial_println!("[vfs]   FAIL: devfs made a socket node in a subdirectory");
        return Err(KernelError::InternalError);
    }
    if Vfs::mknod_socket("/dev/null", 0o755) != Err(KernelError::AlreadyExists) {
        serial_println!("[vfs]   FAIL: devfs made a socket node over /dev/null");
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[vfs]   socket node on devfs: made at the root, refused in a subdirectory and over a \
         device: OK"
    );

    serial_println!("[vfs] Socket-node self-test PASSED");
    Ok(())
}

/// Who owns a new node (`init_new_owner`): a file, directory, symlink and
/// socket node made by a uid-1000 process are its, and so are a directory
/// and a symlink it makes through a held directory; a file it overwrites
/// keeps its owner; in a set-group-ID directory the group is the
/// directory's and a new directory is set-group-ID too; kernel context
/// makes root's.
///
/// # Errors
///
/// `InternalError` naming the first check that failed.
pub fn owner_self_test() -> KernelResult<()> {
    use crate::proc::pcb;
    use crate::proc::thread::self_test_as_process;
    use crate::serial_println;

    serial_println!("[vfs] Running new-node owner self-test...");
    let dir = "/_owner_selftest";
    if Vfs::mounts()
        .iter()
        .any(|(p, _)| p.as_path() == Path::new(dir))
    {
        // A previous boot's leftover; the test needs the name.
        let _ = Vfs::unmount(dir);
    }
    crate::fs::memfs::mount(dir)?;
    let pid = pcb::create("owner-selftest", 0);
    let result = (|| -> Result<(), &'static str> {
        pcb::set_credentials(pid, pcb::ProcessCredentials::new(1000, 1000))
            .map_err(|_| "set_credentials failed")?;
        let owner = |p: &str| Vfs::lmetadata(p).map(|m| (m.uid, m.gid, m.permissions));
        // Made by the kernel: root's.
        Vfs::write_file("/_owner_selftest/rootfile", b"r").map_err(|_| "kernel write")?;
        if owner("/_owner_selftest/rootfile").map(|o| (o.0, o.1)) != Ok((0, 0)) {
            return Err("a file the kernel made is not root's");
        }
        // A set-group-ID directory of group 50.
        Vfs::mkdir_mode("/_owner_selftest/shared", 0o775).map_err(|_| "mkdir shared")?;
        Vfs::set_owner("/_owner_selftest/shared", 0, 50).map_err(|_| "chown shared")?;
        Vfs::set_permissions("/_owner_selftest/shared", 0o2775).map_err(|_| "chmod shared")?;
        let made = self_test_as_process(pid, || -> KernelResult<()> {
            Vfs::write_file("/_owner_selftest/f", b"x")?;
            Vfs::mkdir_mode("/_owner_selftest/d", 0o755)?;
            Vfs::symlink("/_owner_selftest/l", "f")?;
            Vfs::mknod_socket("/_owner_selftest/s", 0o755)?;
            Vfs::write_file("/_owner_selftest/rootfile", b"overwritten")?;
            Vfs::write_file("/_owner_selftest/shared/g", b"y")?;
            Vfs::mkdir_mode("/_owner_selftest/shared/sub", 0o755)?;
            // Through held directories, as `mkdirat` and `symlinkat` reach
            // the kernel natively.
            let top = Vfs::pin_dir("/_owner_selftest")?;
            Vfs::mkdir_at_pinned(&top, b"pd", 0o755)?;
            Vfs::symlink_at_pinned(&top, b"pl", "f")?;
            let shared = Vfs::pin_dir("/_owner_selftest/shared")?;
            Vfs::mkdir_at_pinned(&shared, b"psub", 0o755)?;
            Ok(())
        });
        made.map_err(|_| "a creation as the uid-1000 process failed")?;
        for p in [
            "/_owner_selftest/f",
            "/_owner_selftest/d",
            "/_owner_selftest/l",
            "/_owner_selftest/s",
            "/_owner_selftest/pd",
            "/_owner_selftest/pl",
        ] {
            if owner(p).map(|o| (o.0, o.1)) != Ok((1000, 1000)) {
                serial_println!("[vfs]   {} is {:?}", p, owner(p));
                return Err("a node a uid-1000 process made is not its");
            }
        }
        if owner("/_owner_selftest/rootfile").map(|o| (o.0, o.1)) != Ok((0, 0)) {
            return Err("overwriting a file changed its owner");
        }
        if owner("/_owner_selftest/shared/g").map(|o| (o.0, o.1)) != Ok((1000, 50)) {
            return Err("a file in a set-group-ID directory did not take the directory's group");
        }
        for sub in [
            "/_owner_selftest/shared/sub",
            "/_owner_selftest/shared/psub",
        ] {
            match owner(sub) {
                Ok((1000, 50, mode)) if mode & S_ISGID != 0 && mode & 0o777 == 0o755 => {}
                other => {
                    serial_println!("[vfs]   {} is {:?}", sub, other);
                    return Err(
                        "a directory in a set-group-ID directory is not set-group-ID with its group",
                    );
                }
            }
        }
        Ok(())
    })();
    pcb::destroy(pid);
    let _ = Vfs::unmount(dir);
    if let Err(why) = result {
        serial_println!("[vfs]   FAIL: new-node owner: {}", why);
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[vfs]   new nodes are their creator's, by path and through a held directory; an \
         overwrite keeps the owner; a set-group-ID directory gives its group: OK"
    );
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

/// The extended-attribute rules as the VFS applies them (`fs::xattr_policy`):
/// on a memfs file, a link to it, a sticky directory and an immutable file in
/// `/tmp`, for three callers -- the kernel's own privilege, the files' owner
/// and another user -- given to the calls' `*_as` forms, this test being a
/// kernel task itself. `xattr_policy::self_test` has the rules alone; this
/// proves every call reaches them, in Linux's order, and that a refused name
/// is never stored.
///
/// # Errors
///
/// `InternalError` naming the case that failed; the setup's own.
pub fn self_test_xattr_rules() -> KernelResult<()> {
    use crate::serial_println;
    use xattr_policy::Caller;

    const FILE: &str = "/tmp/_xr_file";
    const LINK: &str = "/tmp/_xr_link";
    const STICKY: &str = "/tmp/_xr_sticky";
    const FROZEN: &str = "/tmp/_xr_frozen";

    let root = Caller {
        privileged: true,
        uid: Some(0),
    };
    let owner = Caller {
        privileged: false,
        uid: Some(1000),
    };
    let stranger = Caller {
        privileged: false,
        uid: Some(2000),
    };

    // Best effort, before and after: a file left by an earlier run must not
    // fail this one, and what is not there to remove is not an error.
    let cleanup = || {
        let _ = Vfs::set_attributes(FROZEN, FileAttr::NONE);
        let _ = Vfs::remove(FROZEN);
        let _ = Vfs::remove(LINK);
        let _ = Vfs::remove(FILE);
        let _ = Vfs::rmdir(STICKY);
    };
    cleanup();

    let (read, write) = (xattr_policy::Access::Read, xattr_policy::Access::Write);
    let get = |path: &str, follow: bool, name: &[u8], who: Caller| {
        Vfs::xattr_target(path, follow, read)
            .and_then(|target| Vfs::xattr_get_as(&target, name, who))
            .map(|_| ())
    };
    let set = |path: &str, follow: bool, name: &[u8], who: Caller| {
        Vfs::xattr_target(path, follow, write)
            .and_then(|target| Vfs::xattr_set_as(&target, name, b"v", XattrSetMode::Any, who))
    };
    let remove = |path: &str, name: &[u8], who: Caller| {
        Vfs::xattr_target(path, true, write)
            .and_then(|target| Vfs::xattr_remove_as(&target, name, who))
    };

    let run = || -> KernelResult<()> {
        Vfs::write_file(FILE, b"x")?;
        Vfs::set_owner(FILE, 1000, 1000)?;
        Vfs::symlink(LINK, FILE)?;
        Vfs::mkdir(STICKY)?;
        Vfs::set_owner(STICKY, 1000, 1000)?;
        Vfs::set_permissions(STICKY, 0o1777)?;
        Vfs::write_file(FROZEN, b"x")?;
        Vfs::set_attributes(FROZEN, FileAttr::IMMUTABLE)?;

        let denied = Err(KernelError::NotPermitted);
        let absent = Err(KernelError::NoAttribute);
        let unsupported = Err(KernelError::NotSupported);
        // In order: each case may stand on what an earlier one stored.
        let cases: [(&str, KernelResult<()>, KernelResult<()>); 25] = [
            // Names no handler takes are refused, and nothing is stored.
            (
                "a name in no namespace",
                set(FILE, true, b"foo", root),
                unsupported,
            ),
            (
                "a bare prefix",
                set(FILE, true, b"user.", root),
                Err(KernelError::InvalidArgument),
            ),
            // The ACL's name reaches the ACL table (`fs::acl`), which parses
            // the value: one byte is no ACL (`EINVAL`, as Linux answers).
            (
                "an ACL's name with no ACL in it",
                set(FILE, true, b"system.posix_acl_access", root),
                Err(KernelError::InvalidArgument),
            ),
            // The default ACL's name reaches the default-ACL table, which
            // parses the value first, as for the access ACL.
            (
                "the default ACL's name with no ACL in it",
                set(FILE, true, b"system.posix_acl_default", root),
                Err(KernelError::InvalidArgument),
            ),
            (
                "no namespace, read",
                get(FILE, true, b"foo", root),
                unsupported,
            ),
            // user.
            (
                "user. by the owner",
                set(FILE, true, b"user.a", owner),
                Ok(()),
            ),
            (
                "user. read by another",
                get(FILE, true, b"user.a", stranger),
                Ok(()),
            ),
            (
                "user. through a link",
                get(LINK, true, b"user.a", stranger),
                Ok(()),
            ),
            ("user. on a link", set(LINK, false, b"user.l", root), denied),
            (
                "user. read on a link",
                get(LINK, false, b"user.l", root),
                absent,
            ),
            (
                "user. on another's sticky dir",
                set(STICKY, true, b"user.d", stranger),
                denied,
            ),
            (
                "user. on one's own sticky dir",
                set(STICKY, true, b"user.d", owner),
                Ok(()),
            ),
            (
                "user. on a sticky dir, privileged",
                set(STICKY, true, b"user.e", root),
                Ok(()),
            ),
            // trusted.
            (
                "trusted. by the kernel",
                set(FILE, true, b"trusted.t", root),
                Ok(()),
            ),
            (
                "trusted. read, unprivileged",
                get(FILE, true, b"trusted.t", owner),
                absent,
            ),
            (
                "trusted. written, unprivileged",
                set(FILE, true, b"trusted.t", owner),
                denied,
            ),
            (
                "trusted. removed, unprivileged",
                remove(FILE, b"trusted.t", owner),
                denied,
            ),
            (
                "trusted. on a link, privileged",
                set(LINK, false, b"trusted.l", root),
                Ok(()),
            ),
            // security.
            (
                "security. by the owner",
                set(FILE, true, b"security.s", owner),
                denied,
            ),
            (
                "security. by the kernel",
                set(FILE, true, b"security.s", root),
                Ok(()),
            ),
            (
                "security. read by another",
                get(FILE, true, b"security.s", stranger),
                Ok(()),
            ),
            // An immutable file takes no change, from anyone.
            (
                "a change to an immutable file",
                set(FROZEN, true, b"user.i", root),
                denied,
            ),
            (
                "a removal from an immutable file",
                remove(FROZEN, b"user.i", root),
                denied,
            ),
            (
                "a read of an immutable file",
                get(FROZEN, true, b"user.i", owner),
                absent,
            ),
            // The path is answered before the name is looked at.
            (
                "a missing file",
                get("/tmp/_xr_absent", true, b"foo", root),
                Err(KernelError::NotFound),
            ),
        ];
        for (what, got, want) in cases {
            if got != want {
                serial_println!("[vfs]   FAIL: xattr {}: {:?}, want {:?}", what, got, want);
                return Err(KernelError::InternalError);
            }
        }

        // A listing: `trusted.` to the privileged only, and none of the
        // refused names, which were never stored.
        let names = |path: &str, follow: bool, who: Caller| -> KernelResult<Vec<Vec<u8>>> {
            let target = Vfs::xattr_target(path, follow, read)?;
            let mut names = Vfs::xattr_list_as(&target, who)?;
            names.sort();
            Ok(names)
        };
        let want = |list: &[&[u8]]| list.iter().map(|n| n.to_vec()).collect::<Vec<_>>();
        let listings = [
            (
                "the owner's",
                names(FILE, true, owner)?,
                want(&[b"security.s", b"user.a"]),
            ),
            (
                "the kernel's",
                names(FILE, true, root)?,
                want(&[b"security.s", b"trusted.t", b"user.a"]),
            ),
            (
                "the link's",
                names(LINK, false, root)?,
                want(&[b"trusted.l"]),
            ),
            (
                "the link's, unprivileged",
                names(LINK, false, owner)?,
                Vec::new(),
            ),
        ];
        for (what, got, want) in listings {
            if got != want {
                serial_println!(
                    "[vfs]   FAIL: xattr listing, {}: {:?}, want {:?}",
                    what,
                    got,
                    want
                );
                return Err(KernelError::InternalError);
            }
        }

        // The public calls find their caller themselves (`Caller::current`):
        // a process with user id 1000 is unprivileged and the files' owner,
        // and the kernel task this runs as is privileged.
        let pid = crate::proc::pcb::create("xattr-rules", 0);
        let found = crate::proc::pcb::set_credentials(
            pid,
            crate::proc::pcb::ProcessCredentials::new(1000, 1000),
        )
        .map(|()| {
            crate::proc::thread::self_test_as_process(pid, || {
                (
                    Caller::current(),
                    Vfs::get_xattr(FILE, b"trusted.t").map(|_| ()),
                    Vfs::set_xattr(FILE, b"user.p", b"v"),
                    Vfs::list_xattrs(FILE).map(|mut names| {
                        names.sort();
                        names
                    }),
                )
            })
        });
        crate::proc::pcb::destroy(pid);
        let (who, trusted, user, listed) = found?;
        if who != owner
            || trusted != absent
            || user != Ok(())
            || listed != Ok(want(&[b"security.s", b"user.a", b"user.p"]))
            || Caller::current()
                != (Caller {
                    privileged: true,
                    uid: None,
                })
        {
            serial_println!(
                "[vfs]   FAIL: xattr as a process: {:?}, {:?}, {:?}, {:?}",
                who,
                trusted,
                user,
                listed
            );
            return Err(KernelError::InternalError);
        }
        Ok(())
    };
    let result = run();
    cleanup();
    result?;
    serial_println!(
        "[vfs]   xattr rules: names no handler takes refused and never stored; \
         user. on files and directories only, a sticky one's by its owner; \
         trusted. privileged and absent to others; security. read by all, \
         written by the privileged; no change to an immutable file; the path \
         before the name; trusted. listed to the privileged only; the caller \
         a process is: OK"
    );
    Ok(())
}

/// The immutable and append-only attributes through every VFS operation that
/// changes a name, the metadata or the contents, on `/tmp` (memfs):
/// `fs::attr_policy`'s rules as the VFS, the handle layer and the filesystem
/// apply them. Run as a kernel task, which the rules bind as they bind root.
///
/// Each refusal must be `NotPermitted` -- `EPERM`, not the `EACCES` these
/// answered until 2026-10-02, and not some other error that would refuse
/// for the wrong reason -- and what was refused must be unchanged after. The
/// controls at the end are what make the refusals mean something: cleared,
/// the same objects take every change.
///
/// # Errors
///
/// `InternalError` naming the first case that answered wrongly; the setup's.
pub fn self_test_attr_rules() -> KernelResult<()> {
    use super::handle::{self, OpenFlags};
    use crate::serial_println;

    const DIR: &str = "/tmp/_ar";
    // An immutable file, an append-only file, a plain one.
    const FROZEN: &str = "/tmp/_ar/frozen";
    const LOG: &str = "/tmp/_ar/log";
    const OTHER: &str = "/tmp/_ar/other";
    // An immutable directory and an append-only one, each with a file.
    const SEALED: &str = "/tmp/_ar/sealed";
    const INSIDE: &str = "/tmp/_ar/sealed/inside";
    const LOGS: &str = "/tmp/_ar/logs";
    const KEPT: &str = "/tmp/_ar/logs/kept";
    // Names a refused operation would have made.
    const MOVED: &str = "/tmp/_ar/moved";
    const SECOND: &str = "/tmp/_ar/second";

    // Best effort, before and after: a tree left by an earlier run must not
    // fail this one, and what is not there to remove is not an error.
    let cleanup = || {
        for path in [FROZEN, LOG, SEALED, LOGS] {
            let _ = Vfs::set_attributes(path, FileAttr::NONE);
        }
        for path in [
            FROZEN,
            LOG,
            OTHER,
            INSIDE,
            KEPT,
            MOVED,
            SECOND,
            "/tmp/_ar/sealed/new",
            "/tmp/_ar/sealed/created",
            "/tmp/_ar/sealed/sym",
            "/tmp/_ar/sealed/other",
            "/tmp/_ar/logs/new",
            "/tmp/_ar/logs/renamed",
            "/tmp/_ar/logs/other",
        ] {
            let _ = Vfs::remove(path);
        }
        for path in ["/tmp/_ar/sealed/newdir", SEALED, LOGS, DIR] {
            let _ = Vfs::rmdir(path);
        }
    };
    cleanup();

    // An open that succeeds is closed at once: these ask only whether it may.
    let open = |path: &str, flags: OpenFlags| -> KernelResult<()> {
        let h = handle::open(path, flags)?;
        handle::close(h)
    };
    let (read, write) = (OpenFlags::READ, OpenFlags::WRITE);
    let append = write.union(OpenFlags::APPEND);

    let run = || -> KernelResult<()> {
        Vfs::mkdir(DIR)?;
        Vfs::write_file(FROZEN, b"frozen")?;
        Vfs::write_file(LOG, b"log")?;
        Vfs::write_file(OTHER, b"other")?;
        Vfs::mkdir(SEALED)?;
        Vfs::write_file(INSIDE, b"inside")?;
        Vfs::mkdir(LOGS)?;
        Vfs::write_file(KEPT, b"kept")?;
        Vfs::set_attributes(FROZEN, FileAttr::IMMUTABLE)?;
        Vfs::set_attributes(LOG, FileAttr::APPEND_ONLY)?;
        Vfs::set_attributes(SEALED, FileAttr::IMMUTABLE)?;
        Vfs::set_attributes(LOGS, FileAttr::APPEND_ONLY)?;
        let before_touch = metadata_now_ns();

        // `fcntl(F_SETFL)` on an append-only file's descriptor: `O_APPEND`
        // stays as it was opened.
        let setfl = {
            let h = handle::open(LOG, append)?;
            let cleared = handle::set_status_flags(h, write);
            handle::close(h)?;
            cleared
        };

        let denied = Err(KernelError::NotPermitted);
        let ok = Ok(());
        // In order: each case may stand on what an earlier one did.
        let cases: [(&str, KernelResult<()>, KernelResult<()>); 47] = [
            // An immutable file: nothing changes, and no name comes or goes.
            (
                "overwrite an immutable file",
                Vfs::write_file(FROZEN, b"x"),
                denied,
            ),
            (
                "write into an immutable file",
                Vfs::write_at(FROZEN, 0, b"x"),
                denied,
            ),
            (
                "append to an immutable file",
                Vfs::append(FROZEN, b"x"),
                denied,
            ),
            (
                "truncate an immutable file",
                Vfs::truncate(FROZEN, 0),
                denied,
            ),
            (
                "allocate to an immutable file",
                Vfs::fallocate(FROZEN, 4096),
                denied,
            ),
            (
                "chmod an immutable file",
                Vfs::set_permissions(FROZEN, 0o600),
                denied,
            ),
            (
                "chown an immutable file",
                Vfs::set_owner(FROZEN, 1000, 1000),
                denied,
            ),
            // Linux asks `may_setattr` nothing for a chown of (-1, -1).
            (
                "chown an immutable file to -1, -1",
                Vfs::set_owner(FROZEN, u32::MAX, u32::MAX),
                ok,
            ),
            (
                "set an immutable file's times",
                Vfs::set_times(FROZEN, 1, 1),
                denied,
            ),
            (
                "touch an immutable file",
                Vfs::set_times(FROZEN, TIME_NOW, TIME_NOW),
                denied,
            ),
            ("unlink an immutable file", Vfs::remove(FROZEN), denied),
            (
                "rename an immutable file",
                Vfs::rename(FROZEN, MOVED),
                denied,
            ),
            (
                "rename onto an immutable file",
                Vfs::rename(OTHER, FROZEN),
                denied,
            ),
            // One object under one name: Linux returns before `may_delete`.
            (
                "rename an immutable file onto itself",
                Vfs::rename(FROZEN, FROZEN),
                ok,
            ),
            ("link an immutable file", Vfs::link(FROZEN, SECOND), denied),
            (
                "open an immutable file to write",
                open(FROZEN, write),
                denied,
            ),
            (
                "open an immutable file to truncate",
                open(FROZEN, write.union(OpenFlags::TRUNCATE)),
                denied,
            ),
            ("open an immutable file to read", open(FROZEN, read), ok),
            // An append-only file: it grows at its end and nowhere else.
            ("append to an append-only file", Vfs::append(LOG, b"+1"), ok),
            (
                "write at an append-only file's end",
                Vfs::write_at(LOG, 5, b"+2"),
                ok,
            ),
            (
                "write into an append-only file",
                Vfs::write_at(LOG, 0, b"x"),
                denied,
            ),
            (
                "overwrite an append-only file",
                Vfs::write_file(LOG, b"x"),
                denied,
            ),
            (
                "truncate an append-only file",
                Vfs::truncate(LOG, 0),
                denied,
            ),
            (
                "allocate to an append-only file",
                Vfs::fallocate(LOG, 4096),
                ok,
            ),
            (
                "chmod an append-only file",
                Vfs::set_permissions(LOG, 0o600),
                denied,
            ),
            (
                "set an append-only file's times",
                Vfs::set_times(LOG, 1, 1),
                denied,
            ),
            (
                "set one of its times to now",
                Vfs::set_times(LOG, TIME_NOW, 0),
                denied,
            ),
            (
                "touch an append-only file",
                Vfs::set_times(LOG, TIME_NOW, TIME_NOW),
                ok,
            ),
            ("unlink an append-only file", Vfs::remove(LOG), denied),
            ("link an append-only file", Vfs::link(LOG, SECOND), denied),
            (
                "open an append-only file to write",
                open(LOG, write),
                denied,
            ),
            (
                "open it to append and truncate",
                open(LOG, append.union(OpenFlags::TRUNCATE)),
                denied,
            ),
            ("open an append-only file to append", open(LOG, append), ok),
            ("F_SETFL without O_APPEND on it", setfl, denied),
            // An immutable directory: no name comes or goes; its files'
            // contents are theirs.
            (
                "create in an immutable directory",
                Vfs::write_file("/tmp/_ar/sealed/new", b"x"),
                denied,
            ),
            (
                "O_CREAT in an immutable directory",
                open("/tmp/_ar/sealed/created", write.union(OpenFlags::CREATE)),
                denied,
            ),
            (
                "mkdir in an immutable directory",
                Vfs::mkdir("/tmp/_ar/sealed/newdir"),
                denied,
            ),
            (
                "symlink in an immutable directory",
                Vfs::symlink("/tmp/_ar/sealed/sym", "inside"),
                denied,
            ),
            (
                "unlink from an immutable directory",
                Vfs::remove(INSIDE),
                denied,
            ),
            (
                "rename out of an immutable directory",
                Vfs::rename(INSIDE, MOVED),
                denied,
            ),
            (
                "rename into an immutable directory",
                Vfs::rename(OTHER, "/tmp/_ar/sealed/other"),
                denied,
            ),
            (
                "link into an immutable directory",
                Vfs::link(OTHER, "/tmp/_ar/sealed/other"),
                denied,
            ),
            ("rmdir an immutable directory", Vfs::rmdir(SEALED), denied),
            (
                "write a file in an immutable directory",
                Vfs::write_file(INSIDE, b"changed"),
                ok,
            ),
            // The name's errors first, as Linux's lookups precede the rules.
            (
                "unlink a missing name there",
                Vfs::remove("/tmp/_ar/sealed/none"),
                Err(KernelError::NotFound),
            ),
            (
                "mkdir a taken name there",
                Vfs::mkdir(INSIDE),
                Err(KernelError::AlreadyExists),
            ),
            // An append-only directory: names come and do not go.
            (
                "create in an append-only directory",
                Vfs::write_file("/tmp/_ar/logs/new", b"x"),
                ok,
            ),
        ];
        for (what, got, want) in cases {
            if got != want {
                serial_println!("[vfs]   FAIL: {}: got {:?}, want {:?}", what, got, want);
                return Err(KernelError::InternalError);
            }
        }
        let more: [(&str, KernelResult<()>, KernelResult<()>); 3] = [
            (
                "unlink from an append-only directory",
                Vfs::remove(KEPT),
                denied,
            ),
            (
                "rename within an append-only directory",
                Vfs::rename(KEPT, "/tmp/_ar/logs/renamed"),
                denied,
            ),
            (
                "rename into an append-only directory",
                Vfs::rename(OTHER, "/tmp/_ar/logs/other"),
                ok,
            ),
        ];
        for (what, got, want) in more {
            if got != want {
                serial_println!("[vfs]   FAIL: {}: got {:?}, want {:?}", what, got, want);
                return Err(KernelError::InternalError);
            }
        }

        // What was refused is as it was.
        let kept: [(&str, &[u8]); 4] = [
            (FROZEN, b"frozen"),
            (LOG, b"log+1+2"),
            (INSIDE, b"changed"),
            (KEPT, b"kept"),
        ];
        for (path, want) in kept {
            let got = Vfs::read_file(path)?;
            if got.as_slice() != want {
                serial_println!(
                    "[vfs]   FAIL: {} holds {:?} after the refusals, want {:?}",
                    path,
                    got.as_slice(),
                    want
                );
                return Err(KernelError::InternalError);
            }
        }
        for path in [
            MOVED,
            SECOND,
            "/tmp/_ar/sealed/other",
            "/tmp/_ar/logs/renamed",
        ] {
            if Vfs::lstat(path).is_ok() {
                serial_println!("[vfs]   FAIL: {} exists: a refused operation made it", path);
                return Err(KernelError::InternalError);
            }
        }
        // `TIME_NOW` reached the file as the time, not as the request.
        let touched = Vfs::metadata(LOG)?.modified_ns;
        if touched == TIME_NOW || touched < before_touch {
            serial_println!(
                "[vfs]   FAIL: touching the log stamped {}, want the time (>= {})",
                touched,
                before_touch
            );
            return Err(KernelError::InternalError);
        }

        // The controls: cleared, the same objects take every change.
        for path in [FROZEN, LOG, SEALED, LOGS] {
            Vfs::set_attributes(path, FileAttr::NONE)?;
        }
        Vfs::write_file(FROZEN, b"thawed")?;
        Vfs::set_permissions(FROZEN, 0o600)?;
        Vfs::set_times(FROZEN, 1, 1)?;
        Vfs::link(FROZEN, SECOND)?;
        Vfs::rename(FROZEN, MOVED)?;
        Vfs::truncate(LOG, 0)?;
        Vfs::write_file("/tmp/_ar/sealed/new", b"x")?;
        Vfs::remove(INSIDE)?;
        Vfs::remove(KEPT)?;
        Ok(())
    };
    let result = run();
    cleanup();
    result?;
    serial_println!(
        "[vfs]   immutable and append-only: 50 operations refused or allowed as on \
         Linux (EPERM), the refused left unchanged, and each allowed once cleared: OK"
    );
    Ok(())
}

/// Seals (`fs::sealing`) through every VFS route that changes a file's
/// contents, size or mode, on `/tmp` (memfs): by the name the seal was placed
/// under, by a second name for the same file, and through an open handle --
/// each refusal `NotPermitted`, each refused file unchanged after, and the
/// unsealed neighbour, a control, taking every change. A file's seals end
/// with it: removed, the table is empty again.
///
/// # Errors
///
/// `InternalError` naming the first case that answered wrongly; the setup's.
pub fn self_test_seal_rules() -> KernelResult<()> {
    use super::handle::{self, OpenFlags};
    use super::sealing::{self, SealFlags};
    use crate::serial_println;

    const WRITE: &str = "/tmp/_seal_write";
    const ALIAS: &str = "/tmp/_seal_write_alias";
    const GROW: &str = "/tmp/_seal_grow";
    const SHRINK: &str = "/tmp/_seal_shrink";
    const EXEC: &str = "/tmp/_seal_exec";
    const PLAIN: &str = "/tmp/_seal_plain";
    let all = [WRITE, ALIAS, GROW, SHRINK, EXEC, PLAIN];

    // Best effort, before and after: seals end with their files, so removing
    // the files is the whole of the cleanup.
    let cleanup = || {
        for path in all {
            let _ = Vfs::remove(path);
        }
    };
    cleanup();

    let run = || -> KernelResult<()> {
        for path in [WRITE, GROW, SHRINK, EXEC, PLAIN] {
            Vfs::write_file(path, b"0123456789")?;
        }
        Vfs::set_permissions(EXEC, 0o644)?;
        Vfs::link(WRITE, ALIAS)?;
        sealing::add_seals(WRITE, SealFlags::WRITE)?;
        sealing::add_seals(GROW, SealFlags::GROW)?;
        sealing::add_seals(SHRINK, SealFlags::SHRINK)?;
        sealing::add_seals(EXEC, SealFlags::EXEC.union(SealFlags::SEAL))?;
        let held = handle::open(WRITE, OpenFlags::READ.union(OpenFlags::WRITE))?;
        let through_handle = handle::write(held, b"x").map(|_| ());
        handle::close(held)?;

        let no = Err(KernelError::NotPermitted);
        let ok = Ok(());
        // In order: each case may stand on what an earlier one did.
        let cases: [(&str, KernelResult<()>, KernelResult<()>); 22] = [
            // WRITE: nothing about the contents or the size changes.
            (
                "write into a write-sealed file",
                Vfs::write_at(WRITE, 0, b"x"),
                no,
            ),
            (
                "overwrite it whole",
                Vfs::write_file(WRITE, b"0123456789"),
                no,
            ),
            ("append to it", Vfs::append(WRITE, b"x"), no),
            ("truncate it", Vfs::truncate(WRITE, 4), no),
            (
                "write it by another name",
                Vfs::write_at(ALIAS, 0, b"x"),
                no,
            ),
            ("write it through an open handle", through_handle, no),
            (
                "chmod it (no EXEC seal)",
                Vfs::set_permissions(WRITE, 0o600),
                ok,
            ),
            // GROW: inside the file, yes; past its end, no.
            (
                "write inside a grow-sealed file",
                Vfs::write_at(GROW, 2, b"ab"),
                ok,
            ),
            ("write past its end", Vfs::write_at(GROW, 8, b"abcd"), no),
            ("append to it", Vfs::append(GROW, b"x"), no),
            ("truncate it larger", Vfs::truncate(GROW, 20), no),
            ("allocate past its end", Vfs::fallocate(GROW, 4096), no),
            ("truncate it smaller", Vfs::truncate(GROW, 8), ok),
            // SHRINK: it may grow, and may not shrink.
            (
                "truncate a shrink-sealed file smaller",
                Vfs::truncate(SHRINK, 4),
                no,
            ),
            (
                "overwrite it shorter",
                Vfs::write_file(SHRINK, b"short"),
                no,
            ),
            ("append to it", Vfs::append(SHRINK, b"+"), ok),
            // EXEC (and SEAL): the execute bits are fixed; no seal is added.
            (
                "set an execute bit on an exec-sealed file",
                Vfs::set_permissions(EXEC, 0o755),
                no,
            ),
            (
                "change no execute bit",
                Vfs::set_permissions(EXEC, 0o600),
                ok,
            ),
            (
                "add a seal after SEAL",
                sealing::add_seals(EXEC, SealFlags::GROW).map(|_| ()),
                no,
            ),
            // The control: the same changes to an unsealed file.
            (
                "write the unsealed file",
                Vfs::write_at(PLAIN, 8, b"abcd"),
                ok,
            ),
            ("truncate it", Vfs::truncate(PLAIN, 2), ok),
            (
                "chmod it executable",
                Vfs::set_permissions(PLAIN, 0o755),
                ok,
            ),
        ];
        for (what, got, want) in cases {
            if got != want {
                serial_println!(
                    "[vfs]   FAIL: seals: {}: got {:?}, want {:?}",
                    what,
                    got,
                    want
                );
                return Err(KernelError::InternalError);
            }
        }
        // What was refused is as it was.
        let kept: [(&str, &[u8]); 3] = [
            (WRITE, b"0123456789"),
            (GROW, b"01ab4567"),
            (SHRINK, b"0123456789+"),
        ];
        for (path, want) in kept {
            let got = Vfs::read_file(path)?;
            if got.as_slice() != want {
                serial_println!(
                    "[vfs]   FAIL: seals: {} holds {:?}, want {:?}",
                    path,
                    got.as_slice(),
                    want
                );
                return Err(KernelError::InternalError);
            }
        }
        if Vfs::metadata(EXEC)?.permissions & 0o777 != 0o600 {
            serial_println!("[vfs]   FAIL: seals: the exec-sealed file's mode moved");
            return Err(KernelError::InternalError);
        }
        Ok(())
    };
    let result = run();
    cleanup();
    result?;
    // Seals end with their files: removed, nothing of them is left.
    for path in all {
        if !sealing::seals_of(None, Path::new(path)).is_empty() {
            serial_println!("[vfs]   FAIL: seals: {} kept its seal after removal", path);
            return Err(KernelError::InternalError);
        }
    }
    serial_println!(
        "[vfs]   seals: write, grow, shrink, exec and seal refused on every route (by name, \
         by a second name, through a handle) with EPERM, the refused unchanged, the unsealed \
         control free, and the seals gone with the files: OK"
    );
    Ok(())
}

/// How a new node's mode and ACL are decided, by the VFS for every route that
/// makes one (design-decisions §1531), on `/tmp` (memfs):
///
/// - a process's umask applies -- a directory or socket asked for 0777 and a
///   file asked for 0666 (or `write_file`'s 0644) come out 0750 and 0640
///   under umask 027, through all six routes: `mkdir_mode`,
///   `mkdir_at_pinned`, `mknod_socket`, `create_file_resolved`
///   (`open(O_CREAT)`'s), `write_file` and `create_unnamed_object`
///   (`O_TMPFILE`'s); the kernel has none;
/// - `create_file_resolved` over an existing file refuses (`AlreadyExists`)
///   and leaves its contents, which is what lets `open(O_CREAT)` that lost a
///   race open the file instead of emptying it;
/// - in a directory with a default ACL the umask does not apply: a new file's
///   mode and access ACL are what the default grants of the mode asked for
///   (an unnamed file's too, kept by its identity), a new directory takes the
///   default as its own, and a file in that directory inherits again;
/// - a default ACL the mode bits say all of gives a new file its mode and no
///   ACL;
/// - the door: a default ACL read back and listed as `system.posix_acl_default`,
///   removed, and `ENODATA` after; removing one from a file succeeds, as on
///   Linux.
///
/// # Errors
///
/// `InternalError` naming the first step that answered wrongly; the setup's.
pub fn self_test_create_modes() -> KernelResult<()> {
    use super::acl::{self, AclPerm, AclTag};
    use crate::serial_println;
    use xattr_policy::Caller;

    const DIR: &str = "/tmp/_cmodes";
    const SHARED: &str = "/tmp/_cmodes/shared";
    const PLAIN: &str = "/tmp/_cmodes/plain";
    let root = Caller {
        privileged: true,
        uid: Some(0),
    };
    let fail = |what: &str| -> KernelResult<()> {
        serial_println!("[vfs]   FAIL: create modes: {}", what);
        Err(KernelError::InternalError)
    };
    let mode_of = |p: &str| Vfs::lmetadata(p).map(|m| m.permissions & 0o7777);
    let set_default = |path: &str, value: &[u8]| {
        Vfs::xattr_target(path, true, xattr_policy::Access::Write)
            .and_then(|t| Vfs::xattr_set_as(&t, acl::XATTR_DEFAULT, value, XattrSetMode::Any, root))
    };
    let get_default = |path: &str| {
        Vfs::xattr_target(path, true, xattr_policy::Access::Read)
            .and_then(|t| Vfs::xattr_get_as(&t, acl::XATTR_DEFAULT, root))
    };
    let remove_default = |path: &str| {
        Vfs::xattr_target(path, true, xattr_policy::Access::Write)
            .and_then(|t| Vfs::xattr_remove_as(&t, acl::XATTR_DEFAULT, root))
    };
    let access_acl = |path: &str| {
        let id = Vfs::file_identity(path).unwrap_or(None);
        acl::get_acl_for(id, Path::new(path))
    };
    let perm =
        |a: &acl::Acl, tag: AclTag| a.entries.iter().find(|e| e.tag == tag).map(|e| e.perm.0);
    // Best effort, both ends: this test's own scratch tree, whose ACLs and
    // default ACLs end with its files (`fs::perfile`).
    let cleanup = || {
        let _ = Vfs::remove_recursive(DIR);
    };
    cleanup();

    let run = || -> KernelResult<()> {
        Vfs::mkdir(DIR)?;
        Vfs::mkdir(SHARED)?;
        Vfs::mkdir(PLAIN)?;
        // user::rwx user:2000:rw- group::r-x mask::rwx other::r--
        let shared = acl::build_acl(
            AclPerm::ALL,
            AclPerm(5),
            AclPerm(4),
            &[(2000, AclPerm(6))],
            &[],
        );
        set_default(SHARED, &acl::encode_xattr(&shared))?;
        set_default(PLAIN, &acl::encode_xattr(&acl::from_mode(0o750)))?;
        let pinned = Vfs::pin_dir(DIR)?;

        // Everything a process makes, as one with umask 027.
        let pid = crate::proc::pcb::create("create-modes", 0);
        // Discarded: the process was made just above, so it is there.
        let _ = crate::proc::pcb::set_umask(pid, 0o027);
        // An unnamed file (`O_TMPFILE`) made in `dir`, asked for 0666: its mode
        // and whether it took an access ACL, read while it is held -- it goes
        // with the hold, and its ACL with it.
        let unnamed = |dir: &str| -> KernelResult<(u16, bool)> {
            let hold = Vfs::create_unnamed_object(Path::new(dir), 0o666, false)?;
            let mode = hold
                .fs
                .lock()
                .metadata_ino(hold.ino)
                .map(|m| m.permissions & 0o7777)?;
            Ok((
                mode,
                acl::get_acl_for(Some(hold.id()), Path::new(dir)).is_some(),
            ))
        };
        let made = crate::proc::thread::self_test_as_process(pid, || {
            Vfs::mkdir_mode("/tmp/_cmodes/d", 0o777)?;
            Vfs::create_file_resolved(Path::new("/tmp/_cmodes/f"), 0o666)?;
            Vfs::write_file("/tmp/_cmodes/w", b"w")?;
            Vfs::mknod_socket("/tmp/_cmodes/s", 0o777)?;
            Vfs::mkdir_at_pinned(&pinned, b"p", 0o777)?;
            let plain_unnamed = unnamed(DIR)?;
            Vfs::create_file_resolved(Path::new("/tmp/_cmodes/shared/f"), 0o666)?;
            Vfs::mkdir_mode("/tmp/_cmodes/shared/sub", 0o777)?;
            Vfs::create_file_resolved(Path::new("/tmp/_cmodes/shared/sub/g"), 0o600)?;
            Vfs::create_file_resolved(Path::new("/tmp/_cmodes/plain/f"), 0o666)?;
            let shared_unnamed = unnamed(SHARED)?;
            Ok((plain_unnamed, shared_unnamed))
        });
        crate::proc::pcb::destroy(pid);
        let (plain_unnamed, shared_unnamed) = made?;
        // And one the kernel makes, which has no umask.
        Vfs::mkdir_mode("/tmp/_cmodes/k", 0o777)?;

        // `O_TMPFILE`'s route: the umask, or the default ACL and the access
        // ACL it grants.
        if plain_unnamed != (0o640, false) || shared_unnamed != (0o664, true) {
            serial_println!(
                "[vfs]     unnamed files: {:?} in a plain directory, {:?} under a default ACL",
                plain_unnamed,
                shared_unnamed
            );
            return fail("an unnamed file's mode or ACL is wrong");
        }

        // A create that finds the name taken -- `open(O_CREAT)` losing a race
        // to another's create -- refuses, and leaves what is there alone: the
        // open goes back to open that file, whose contents a create that
        // wrote the empty file over it would have lost.
        Vfs::write_file("/tmp/_cmodes/kept", b"kept")?;
        let again = Vfs::create_file_resolved(Path::new("/tmp/_cmodes/kept"), 0o600);
        let kept = Vfs::read_file("/tmp/_cmodes/kept");
        if again != Err(KernelError::AlreadyExists) || kept.as_deref() != Ok(&b"kept"[..]) {
            serial_println!("[vfs]     create over a file: {:?}, then {:?}", again, kept);
            return fail("a create over an existing file did not refuse, or changed it");
        }

        let modes: [(&str, &str, u16); 10] = [
            ("mkdir, umask 027", "/tmp/_cmodes/d", 0o750),
            ("open(O_CREAT), umask 027", "/tmp/_cmodes/f", 0o640),
            (
                "write_file making a file, umask 027",
                "/tmp/_cmodes/w",
                0o640,
            ),
            ("a socket node, umask 027", "/tmp/_cmodes/s", 0o750),
            (
                "mkdir through a pinned directory, umask 027",
                "/tmp/_cmodes/p",
                0o750,
            ),
            ("the kernel's mkdir, no umask", "/tmp/_cmodes/k", 0o777),
            (
                "a file under a default ACL: no umask",
                "/tmp/_cmodes/shared/f",
                0o664,
            ),
            ("a directory under it", "/tmp/_cmodes/shared/sub", 0o774),
            (
                "a file asked for 0600 in that directory",
                "/tmp/_cmodes/shared/sub/g",
                0o600,
            ),
            (
                "a file under a minimal default",
                "/tmp/_cmodes/plain/f",
                0o640,
            ),
        ];
        for (what, path, want) in modes {
            let got = mode_of(path);
            if got != Ok(want) {
                serial_println!("[vfs]     {}: {:?}, want {:o}", what, got, want);
                return fail("a new node's mode is wrong");
            }
        }

        // The ACLs they took.
        let file = access_acl("/tmp/_cmodes/shared/f");
        let deep = access_acl("/tmp/_cmodes/shared/sub/g");
        let file_ok = file.as_ref().is_some_and(|a| {
            perm(a, AclTag::User(2000)) == Some(6) && perm(a, AclTag::Mask) == Some(6)
        });
        let deep_ok = deep.as_ref().is_some_and(|a| {
            perm(a, AclTag::User(2000)) == Some(6) && perm(a, AclTag::Mask) == Some(0)
        });
        if !file_ok || !deep_ok {
            serial_println!("[vfs]     file {:?}, deeper {:?}", file, deep);
            return fail("a file under a default ACL did not take the access ACL it grants");
        }
        let inherited = get_default("/tmp/_cmodes/shared/sub")
            .and_then(|b| acl::decode_xattr(&b))
            .map(|a| a.entries == shared.entries);
        if inherited != Ok(true) || access_acl("/tmp/_cmodes/plain/f").is_some() {
            serial_println!("[vfs]     inherited default {:?}", inherited);
            return fail(
                "a new directory did not take its parent's default, or a minimal default gave an ACL",
            );
        }
        if access_acl("/tmp/_cmodes/f").is_some() || get_default("/tmp/_cmodes/d").is_ok() {
            return fail("a node made where there is no default ACL took one");
        }

        // The door: listed, removed, gone; a file's removal succeeds.
        let listed = Vfs::list_xattrs(SHARED)?
            .iter()
            .any(|n| n.as_slice() == acl::XATTR_DEFAULT);
        let removed = remove_default(SHARED);
        let after = get_default(SHARED);
        let again = remove_default(SHARED);
        let on_file = remove_default("/tmp/_cmodes/f");
        if !listed
            || removed.is_err()
            || after != Err(KernelError::NoAttribute)
            || again != Err(KernelError::NoAttribute)
            || on_file.is_err()
        {
            serial_println!(
                "[vfs]     listed {}, removed {:?}, then read {:?}, removed again {:?}, a file's {:?}",
                listed,
                removed,
                after,
                again,
                on_file
            );
            return fail("the default ACL's door answered wrongly");
        }
        Ok(())
    };
    let result = run();
    cleanup();
    result?;
    serial_println!(
        "[vfs]   create modes: a process's umask on six routes and none for the kernel; a create \
         over a file refused and the file kept; a default ACL in the umask's place, unnamed \
         files' included, inherited by a new directory and again below it; a minimal default; \
         the door: OK"
    );
    Ok(())
}

/// The ACL door (`system.posix_acl_access`) through the VFS, on `/tmp`
/// (memfs): what `setfacl` and `getfacl` do, by path and through an open
/// handle, as the file's owner, as root and as a stranger.
///
/// Each step is checked against the ACL table the permission check reads
/// (`fs::acl`), not only against what the door answers, so a door that
/// stored the bytes and enforced nothing would fail here.
///
/// # Errors
///
/// `InternalError` naming the first step that answered wrongly; the setup's.
pub fn self_test_acl_door() -> KernelResult<()> {
    use super::acl::{self, AclPerm, AclTag};
    use super::handle::{self, OpenFlags};
    use crate::serial_println;
    use xattr_policy::Caller;

    const FILE: &str = "/tmp/_acl_door";
    const FROZEN: &str = "/tmp/_acl_door_frozen";
    let name = acl::XATTR_ACCESS;
    let root = Caller {
        privileged: true,
        uid: Some(0),
    };
    let owner = Caller {
        privileged: false,
        uid: Some(1000),
    };
    let stranger = Caller {
        privileged: false,
        uid: Some(3000),
    };
    let set = |path: &str, value: &[u8], mode: XattrSetMode, who: Caller| {
        Vfs::xattr_target(path, true, xattr_policy::Access::Write)
            .and_then(|t| Vfs::xattr_set_as(&t, acl::XATTR_ACCESS, value, mode, who))
    };
    let get = |path: &str| {
        Vfs::xattr_target(path, true, xattr_policy::Access::Read)
            .and_then(|t| Vfs::xattr_get_as(&t, acl::XATTR_ACCESS, root))
    };
    let remove = |path: &str, who: Caller| {
        Vfs::xattr_target(path, true, xattr_policy::Access::Write)
            .and_then(|t| Vfs::xattr_remove_as(&t, acl::XATTR_ACCESS, who))
    };
    let mode_of = |path: &str| Vfs::metadata(path).map(|m| m.permissions & 0o7777);
    let fail = |what: &str| -> KernelResult<()> {
        serial_println!("[vfs]   FAIL: ACL door: {}", what);
        Err(KernelError::InternalError)
    };
    let cleanup = || {
        let _ = Vfs::set_attributes(FROZEN, FileAttr::NONE);
        let _ = Vfs::remove(FROZEN);
        let _ = Vfs::remove(FILE);
    };
    cleanup();

    let run = || -> KernelResult<()> {
        Vfs::write_file(FILE, b"x")?;
        Vfs::set_owner(FILE, 1000, 1000)?;
        Vfs::set_permissions(FILE, 0o640)?;
        // owner rwx, group r-x, other r--, and user 2000 rw-; the mask is the
        // union of the group's and the named entry's: rwx.
        let named = acl::build_acl(
            AclPerm::ALL,
            AclPerm(5),
            AclPerm(4),
            &[(2000, AclPerm(6))],
            &[],
        );
        let bytes = acl::encode_xattr(&named);

        // Root sets it: stored where the check reads it, and the mode it means.
        set(FILE, &bytes, XattrSetMode::Any, root)?;
        if get(FILE)? != bytes || mode_of(FILE)? != 0o774 {
            return fail("root's ACL did not read back, or did not set the mode it means");
        }
        let listed = Vfs::list_xattrs(FILE)?;
        if !listed.iter().any(|n| n.as_slice() == name) {
            return fail("a file with an ACL does not list its name");
        }
        // Through the permission gate's own decision, with the caller's
        // identity given (`path_access_verdict`): a self-test runs as a kernel
        // task, which `check_path_access` lets through before asking.
        let check =
            |uid: u32, want: PathAccess| path_access_verdict(Path::new(FILE), uid, uid, &[], want);
        if check(2000, PathAccess::Write).is_err() || check(3000, PathAccess::Write).is_ok() {
            return fail("the stored ACL does not decide as it says");
        }

        // A chmod is written into the ACL: the mask narrows the named entry.
        Vfs::set_permissions(FILE, 0o750)?;
        let after = acl::decode_xattr(&get(FILE)?)?;
        let mask = after
            .entries
            .iter()
            .find(|e| e.tag == AclTag::Mask)
            .map(|e| e.perm);
        let user = after
            .entries
            .iter()
            .find(|e| e.tag == AclTag::User(2000))
            .map(|e| e.perm);
        if mask != Some(AclPerm(5)) || user != Some(AclPerm(6)) {
            return fail("a chmod did not reach the ACL's mask, or moved a named entry");
        }
        if check(2000, PathAccess::Write).is_ok() || check(2000, PathAccess::Read).is_err() {
            return fail("after the chmod, the narrower mask is not what decides");
        }

        // Who may: the owner and root; a stranger not.
        let refusals: [(&str, KernelResult<()>, KernelResult<()>); 6] = [
            (
                "a stranger sets one",
                set(FILE, &bytes, XattrSetMode::Any, stranger),
                Err(KernelError::NotPermitted),
            ),
            (
                "a stranger removes it",
                remove(FILE, stranger),
                Err(KernelError::NotPermitted),
            ),
            (
                "XATTR_CREATE over one",
                set(FILE, &bytes, XattrSetMode::Create, owner),
                Err(KernelError::AlreadyExists),
            ),
            (
                "a malformed one",
                set(
                    FILE,
                    bytes.get(..bytes.len().saturating_sub(1)).unwrap_or(&[]),
                    XattrSetMode::Any,
                    owner,
                ),
                Err(KernelError::InvalidArgument),
            ),
            ("the owner removes it", remove(FILE, owner), Ok(())),
            (
                "XATTR_REPLACE of none",
                set(FILE, &bytes, XattrSetMode::Replace, owner),
                Err(KernelError::NoAttribute),
            ),
        ];
        for (what, got, want) in refusals {
            if got != want {
                serial_println!(
                    "[vfs]   FAIL: ACL door: {}: got {:?}, want {:?}",
                    what,
                    got,
                    want
                );
                return Err(KernelError::InternalError);
            }
        }
        if get(FILE) != Err(KernelError::NoAttribute) || check(3000, PathAccess::Write).is_err() {
            return fail("a removed ACL still reads back, or still decides");
        }

        // A minimal ACL is the mode alone: kept as the mode, not as an ACL.
        set(
            FILE,
            &acl::encode_xattr(&acl::from_mode(0o600)),
            XattrSetMode::Any,
            owner,
        )?;
        if get(FILE) != Err(KernelError::NoAttribute) || mode_of(FILE)? != 0o600 {
            return fail("a minimal ACL was kept as an ACL, or did not set the mode");
        }

        // Through an open handle, as `fsetfacl` does: the same table.
        let h = handle::open(FILE, OpenFlags::READ)?;
        let through = handle::HandleFile::of(h)
            .and_then(|f| f.set_xattr(acl::XATTR_ACCESS, &bytes, XattrSetMode::Any));
        let read_back = handle::HandleFile::of(h).and_then(|f| f.get_xattr(acl::XATTR_ACCESS));
        handle::close(h)?;
        if through.is_err() || read_back.as_deref() != Ok(bytes.as_slice()) || get(FILE)? != bytes {
            serial_println!(
                "[vfs]   ACL door through a handle: set {:?}, read {:?}",
                through,
                read_back.map(|b| b.len())
            );
            return fail("an ACL set through a handle is not the file's");
        }

        // No change to an immutable file's ACL, whoever asks; and no default
        // ACL on a file -- only a directory has one (EACCES, as on Linux).
        Vfs::write_file(FROZEN, b"x")?;
        Vfs::set_attributes(FROZEN, FileAttr::IMMUTABLE)?;
        let frozen = set(FROZEN, &bytes, XattrSetMode::Any, root);
        let default = Vfs::xattr_target(FILE, true, xattr_policy::Access::Write).and_then(|t| {
            Vfs::xattr_set_as(&t, acl::XATTR_DEFAULT, &bytes, XattrSetMode::Any, root)
        });
        if frozen != Err(KernelError::NotPermitted) || default != Err(KernelError::PermissionDenied)
        {
            serial_println!(
                "[vfs]   ACL door: immutable {:?}, default on a file {:?}",
                frozen,
                default
            );
            return fail("an immutable file took an ACL, or a file took a default ACL");
        }
        Ok(())
    };
    let result = run();
    cleanup();
    result?;
    if acl::get_acl(FILE).is_some() {
        return fail("the file's ACL outlived it");
    }
    serial_println!(
        "[vfs]   ACL door: set by root and read back, listed, the mode it means, a chmod written into it, enforced as \\
         stored; refused to a stranger, malformed, over XATTR_CREATE and REPLACE; a minimal one kept as the mode; \\
         the same through a handle; not on an immutable file; no default ACLs: OK"
    );
    Ok(())
}

/// Records each [`append_race_worker`] appends to the shared file.
const APPEND_RACE_RECORDS: usize = 200;
/// The shared file both workers append to.
const APPEND_RACE_PATH: &str = "/tmp/.vfs-append-race";
/// Workers finished (each adds one on exit).
static APPEND_RACE_DONE: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// `i`'s last three decimal digits, as ASCII.
fn append_race_digits(i: usize) -> [u8; 3] {
    let digit = |d: usize| b'0'.saturating_add(u8::try_from(d % 10).unwrap_or(0));
    [digit(i / 100), digit(i / 10), digit(i)]
}

/// One appender for [`self_test_append_is_atomic`]: `arg` is its tag byte.
/// Each record is `<tag><3 digits>\n`, five bytes, so a record that was
/// overwritten shows up as a missing number.
extern "C" fn append_race_worker(arg: u64) {
    let tag = u8::try_from(arg).unwrap_or(b'?');
    for i in 0..APPEND_RACE_RECORDS {
        let [d0, d1, d2] = append_race_digits(i);
        let rec = [tag, d0, d1, d2, b'\n'];
        // Ignored on purpose: the main task judges the file, and a failed
        // append shows there as a missing record.
        let _ = Vfs::append(APPEND_RACE_PATH, &rec);
        if i % 16 == 0 {
            crate::sched::yield_now();
        }
    }
    APPEND_RACE_DONE.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
}

/// Stress self-test: two tasks appending to one file lose no record.
///
/// Probabilistic by nature -- the old race needed the two appenders
/// interleaved between a `stat` and a `write_at`, which two tasks
/// yielding every sixteen appends make likely but cannot force -- so it can
/// catch a regression, not prove its absence. The absence is by
/// construction: [`Vfs::append_resolved`] reads the end and writes at it
/// under one hold of the filesystem lock.
///
/// # Errors
///
/// `InternalError` when the file does not hold exactly the 400 records --
/// every one of `A000..A199` and `B000..B199` once, whole.
pub fn self_test_append_is_atomic() -> KernelResult<()> {
    // A stale file from an earlier run would add records.
    match Vfs::remove(APPEND_RACE_PATH) {
        Ok(()) | Err(KernelError::NotFound) => {}
        Err(e) => return Err(e),
    }
    APPEND_RACE_DONE.store(0, core::sync::atomic::Ordering::SeqCst);
    crate::sched::spawn(b"append-race-a", 16, append_race_worker, u64::from(b'A'), 0)?;
    crate::sched::spawn(b"append-race-b", 16, append_race_worker, u64::from(b'B'), 0)?;
    let deadline = crate::hrtimer::now_ns().saturating_add(20_000_000_000); // 20 s
    while APPEND_RACE_DONE.load(core::sync::atomic::Ordering::SeqCst) < 2 {
        if crate::hrtimer::now_ns() >= deadline {
            crate::serial_println!("[vfs]   FAIL: append race: workers did not finish in 20 s");
            return Err(KernelError::InternalError);
        }
        crate::sched::yield_now();
    }
    let data = Vfs::read_file(APPEND_RACE_PATH)?;
    let _ = Vfs::remove(APPEND_RACE_PATH); // best-effort tidy-up; judged below
    let mut seen = [[false; APPEND_RACE_RECORDS]; 2];
    let mut whole = 0usize;
    for rec in data.chunks(5) {
        let (tag, digits, nl) = match rec {
            [t, a, b, c, n] => (*t, [*a, *b, *c], *n),
            _ => break,
        };
        let which = match tag {
            b'A' => 0,
            b'B' => 1,
            _ => break,
        };
        if nl != b'\n' {
            break;
        }
        let Some(i) = core::str::from_utf8(&digits)
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
        else {
            break;
        };
        match seen.get_mut(which).and_then(|s| s.get_mut(i)) {
            Some(slot) if !*slot => *slot = true,
            _ => break,
        }
        whole = whole.saturating_add(1);
    }
    let want = APPEND_RACE_RECORDS.saturating_mul(2);
    if data.len() != want.saturating_mul(5) || whole != want {
        crate::serial_println!(
            "[vfs]   FAIL: append race: {} bytes, {} whole records (want {} and {}) -- \
             concurrent appends overwrote each other",
            data.len(),
            whole,
            want.saturating_mul(5),
            want
        );
        return Err(KernelError::InternalError);
    }
    Ok(())
}

/// Where [`self_test_pivot_mounts`] builds its tree: under `/tmp`, so the
/// pivot it makes is never the system's own.
const PIVOT_TEST_ROOT: &str = "/tmp/pivot-test";

/// [`Vfs::pivot_root`], through [`Vfs::pivot_mounts`] about a tree under
/// `/tmp` -- the same code about a mount that is not the system's root:
///
/// - the mount at `new_root` takes the root's place and its sub-mount moves
///   with it; the old root lands at `put_old`; a mount beside them (as `/proc`
///   is beside the image) stays where it was;
/// - each refusal -- `put_old` not a child of the root, already a mount, or
///   the new root itself; no mount at `new_root`; a moving mount landing on
///   another's path -- changes nothing;
/// - a path-taken advisory lock moves with its file.
pub fn self_test_pivot_mounts() -> KernelResult<()> {
    crate::serial_println!("[vfs] Running pivot_root self-test...");
    let root = PIVOT_TEST_ROOT;
    let at = |rel: &str| alloc::format!("{root}{rel}");
    let result = pivot_test_body(root, &at);
    // Children before parents, whichever stage the body reached; an unmount
    // of what is not mounted answers NotFound, which is what it means here.
    for rel in ["/img/sys", "/img/sub", "/sub", "/sys", "/old", "/img", ""] {
        let _ = Vfs::unmount(at(rel));
    }
    match result {
        Ok(()) => {
            crate::serial_println!(
                "[vfs]   pivot_root: the new root and its sub-mount move, the old root to put_old, \
                 a lock with its file; refusals change nothing: OK"
            );
            Ok(())
        }
        Err(what) => {
            crate::serial_println!("[vfs]   FAIL: pivot_root: {}", what);
            Err(KernelError::InternalError)
        }
    }
}

/// [`self_test_pivot_mounts`]'s body, for a caller that unmounts after it.
fn pivot_test_body(root: &str, at: &dyn Fn(&str) -> String) -> Result<(), &'static str> {
    let mount = |rel: &str| crate::fs::memfs::mount(at(rel).as_str()).map_err(|_| "a memfs mount");
    let mark =
        |rel: &str, text: &[u8]| Vfs::write_file(at(rel), text).map_err(|_| "writing a marker");
    let read = |rel: &str| Vfs::read_file(at(rel)).ok();
    // The old root, the image with a sub-mount, and a mount beside them.
    mount("")?;
    mark("/marker", b"old")?;
    mount("/img")?;
    mark("/img/marker", b"img")?;
    mark("/img/locked", b"")?;
    mount("/img/sub")?;
    mark("/img/sub/marker", b"sub")?;
    mount("/sys")?;
    mark("/sys/marker", b"sys")?;

    let as_before = || {
        read("/marker").as_deref() == Some(b"old".as_slice())
            && read("/img/marker").as_deref() == Some(b"img".as_slice())
            && read("/img/sub/marker").as_deref() == Some(b"sub".as_slice())
    };
    let pivot = |new_root: &str, put_old: &str| {
        Vfs::pivot_mounts(
            Path::new(root),
            Path::new(&at(new_root)),
            Path::new(put_old),
        )
    };
    let old = at("/old");
    let refusals: [(&str, KernelResult<()>, KernelError); 4] = [
        (
            "put_old outside the root",
            pivot("/img", "/tmp/pivot-test-elsewhere"),
            KernelError::InvalidArgument,
        ),
        (
            "put_old a mount point",
            pivot("/img", &at("/sys")),
            KernelError::AlreadyExists,
        ),
        (
            "put_old the new root",
            pivot("/img", &at("/img")),
            KernelError::InvalidArgument,
        ),
        (
            "no mount at new_root",
            pivot("/nothing", &old),
            KernelError::NotFound,
        ),
    ];
    for (why, got, want) in refusals {
        if got != Err(want) || !as_before() {
            crate::serial_println!("[vfs]   pivot refusal '{}' answered {:?}", why, got);
            return Err("a refusal answered wrongly, or changed the tree");
        }
    }
    // A sub-mount of the new root that would land on the mount beside it.
    mount("/img/sys")?;
    let clash = pivot("/img", &old);
    let _ = Vfs::unmount(at("/img/sys"));
    if clash != Err(KernelError::AlreadyExists) || !as_before() {
        return Err("a moving mount landing on another's path was not refused");
    }

    // A lock taken by path on the image's file.
    let holder = flock_process_owner(0xFFF1);
    let other = flock_process_owner(0xFFF2);
    Vfs::flock(at("/img/locked"), holder, LockType::Exclusive).map_err(|_| "taking the lock")?;

    pivot("/img", &old).map_err(|_| "the pivot itself was refused")?;
    if read("/marker").as_deref() != Some(b"img".as_slice())
        || read("/sub/marker").as_deref() != Some(b"sub".as_slice())
        || read("/old/marker").as_deref() != Some(b"old".as_slice())
        || read("/sys/marker").as_deref() != Some(b"sys".as_slice())
        || read("/img/marker").is_some()
    {
        return Err("after the pivot a mount is not where it should be");
    }
    // The lock moved with the file: another owner is refused at its new path.
    let blocked = Vfs::flock(at("/locked"), other, LockType::Exclusive);
    let released = Vfs::funlock(at("/locked"), holder);
    let taken = Vfs::flock(at("/locked"), other, LockType::Exclusive);
    let _ = Vfs::funlock(at("/locked"), other);
    if blocked != Err(KernelError::WouldBlock) || released.is_err() || taken.is_err() {
        return Err("the lock did not move with its file");
    }
    Ok(())
}
