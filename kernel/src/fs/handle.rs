//! File handle management — open/read/write/seek/close model.
//!
//! Provides a global open-file table that maps integer handle IDs to
//! open file state (path, cursor position, mode flags).  Syscalls use
//! handle IDs instead of paths for file I/O after opening.
//!
//! ## Design
//!
//! File handles are kernel-managed integers.  Currently they live in a
//! global table; once the capability system matures, each handle will
//! be a capability entry in the per-process capability table (handles
//! ARE capabilities per the design spec).
//!
//! ## Thread Safety
//!
//! The open-file table is behind a `crate::sync::Mutex`.  Individual file
//! positions are mutated under this lock — this is acceptable for
//! early development but should move to per-handle locks or lock-free
//! structures on the hot path.

use crate::sync::Mutex;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Open flags
// ---------------------------------------------------------------------------

/// Flags passed to `open()` controlling the access mode.
///
/// These are a bitfield.  Multiple flags can be OR'd together.
#[derive(Debug, Clone, Copy)]
pub struct OpenFlags(u32);

#[allow(dead_code)]
impl OpenFlags {
    /// No flags (invalid — at least READ or WRITE must be set).
    pub const NONE: Self = Self(0);
    /// Open for reading.
    pub const READ: Self = Self(1 << 0);
    /// Open for writing.
    pub const WRITE: Self = Self(1 << 1);
    /// Create the file if it doesn't exist (requires WRITE).
    pub const CREATE: Self = Self(1 << 2);
    /// Truncate the file to zero length on open (requires WRITE).
    pub const TRUNCATE: Self = Self(1 << 3);
    /// All writes go to the end of the file regardless of seek position.
    pub const APPEND: Self = Self(1 << 4);
    /// Require the path to refer to a directory.
    ///
    /// When set, `open()` will allocate a directory handle (suitable for
    /// `getdents64(2)`) if the path resolves to a directory, or return
    /// `NotADirectory` if it resolves to anything else.  When not set,
    /// directories are rejected with `IsADirectory` as before.
    pub const DIRECTORY: Self = Self(1 << 5);
    /// Exclusive create (POSIX `O_EXCL`).  Only meaningful with
    /// [`CREATE`](Self::CREATE): if the path already exists, `open()` fails
    /// with [`AlreadyExists`](crate::error::KernelError::AlreadyExists)
    /// (→ `EEXIST`) instead of opening it.  Without `CREATE` it is ignored,
    /// matching Linux's regular-file behaviour.
    pub const EXCL: Self = Self(1 << 6);
    /// Do not follow a final symlink component (POSIX `O_NOFOLLOW`).  If the
    /// path's last component is a symbolic link, `open()` fails with
    /// [`TooManyLinks`](crate::error::KernelError::TooManyLinks) (→ `ELOOP`)
    /// instead of resolving through it.  Symlinks in *parent* components are
    /// still followed, matching POSIX.  Only the final component is guarded.
    pub const NOFOLLOW: Self = Self(1 << 7);
    /// Refuse to traverse *any* symlink in the path (`openat2`'s
    /// `RESOLVE_NO_SYMLINKS`).  Strictly stronger than [`NOFOLLOW`](Self::NOFOLLOW):
    /// a symbolic link in *any* component — parent or final — makes `open()`
    /// fail with [`TooManyLinks`](crate::error::KernelError::TooManyLinks)
    /// (→ `ELOOP`).  This is not a user-facing `O_*` flag; it is injected by
    /// the `openat2` handler when its `open_how.resolve` requests it, so a
    /// program can only opt *into* stricter resolution (never weaker),
    /// making it safe even though it is honoured on any open path.
    pub const NO_SYMLINKS: Self = Self(1 << 8);
    /// Open a directory if the path names one, a regular file otherwise:
    /// what a Linux `open` without `O_DIRECTORY` does for a read-only open
    /// (`open(".", O_RDONLY)`, a directory opened to `fsync` it). A
    /// directory is still refused anything that would write it. With
    /// neither this nor [`DIRECTORY`](Self::DIRECTORY), a directory is
    /// `IsADirectory`.
    pub const DIRECTORY_ALLOWED: Self = Self(1 << 9);

    /// Create from raw bits (for syscall argument parsing).
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Get the raw bits.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Check if a flag is set.
    #[must_use]
    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0 && flag.0 != 0
    }

    /// Combine flags.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Check if readable.
    #[must_use]
    pub const fn is_readable(self) -> bool {
        self.contains(Self::READ)
    }

    /// Check if writable.
    #[must_use]
    pub const fn is_writable(self) -> bool {
        self.contains(Self::WRITE)
    }
}

// ---------------------------------------------------------------------------
// Seek origin
// ---------------------------------------------------------------------------

/// Origin for seek operations.
#[derive(Debug, Clone, Copy)]
pub enum SeekFrom {
    /// Seek to an absolute byte position.
    Start(u64),
    /// Seek relative to the current position (can be negative).
    Current(i64),
    /// Seek relative to the end of the file (can be negative).
    End(i64),
    /// Seek to the next data region at or after the given offset.
    /// Returns the offset of the next data byte.  If the file has
    /// no holes (common case), this returns the given offset if it's
    /// within the file, or an error if past EOF.
    Data(u64),
    /// Seek to the next hole at or after the given offset.
    /// A "hole" is an unallocated region (reads as zeros).
    /// If the filesystem doesn't track holes, returns EOF as the
    /// first hole (the conceptual hole after all data).
    Hole(u64),
}

// ---------------------------------------------------------------------------
// Open file entry
// ---------------------------------------------------------------------------

/// An open file tracked by the kernel.
struct OpenFile {
    /// Absolute VFS path to the file.
    ///
    /// Stored as a byte [`PathBuf`], not a `String`: a path component may
    /// contain any byte except `/` and NUL, so a handle on a file whose
    /// name is not valid UTF-8 must still round-trip through
    /// [`handle_path`] and through the close-time `funlock`/`emit_closed`
    /// calls that re-derive the file from this field.
    path: PathBuf,
    /// Current read/write cursor position.
    offset: u64,
    /// The file's size as this handle last saw it -- at open, or after its
    /// own write or truncate. **For display only** (`/proc/fdinfo`, `lsof`):
    /// another writer's growth is not here. Every I/O decision asks the file
    /// itself. Until 2026-10-01 reads stopped at this number, so a reader
    /// never saw what another writer appended (`tail -f`; two SQLite
    /// processes; known-issues A-AN-OPEN-FILE-FOLLOWS-ITS-NAME).
    seen_size: u64,
    /// The file itself, held as its filesystem and inode, where the
    /// filesystem allows (`Vfs::open_object`): I/O goes through it, so a
    /// rename or an unlink of `path` leaves this handle on its file. `None`
    /// for a directory, and for a file on a filesystem without stable inodes
    /// (FAT, the pseudo filesystems), which still go by `path`.
    ///
    /// Shared: a `dup` of the description shares it, and each call in
    /// progress through the handle takes a reference for its length, so the
    /// hold goes back with the last of them -- never under a call that a close
    /// raced (`FileHold`).
    object: Option<Arc<crate::fs::vfs::FileHold>>,
    /// Flags this file was opened with.
    flags: OpenFlags,
    /// Number of owners sharing this open file description.
    ///
    /// One open file description (this `OpenFile` entry, with its shared
    /// cursor) can be referenced by several handle owners after `fork()`
    /// or a shared `dup`/`dup2`.  Each owner contributes one reference;
    /// `close()` decrements and only removes the entry (and releases the
    /// advisory lock) when the last reference goes away.  A freshly
    /// opened handle starts at `1`.  This mirrors POSIX semantics where a
    /// forked child shares the parent's open file description (and its
    /// offset), not an independent copy.
    refcount: u32,
    /// `true` if this is a directory handle (opened with `OpenFlags::DIRECTORY`).
    ///
    /// Directory handles support `read_dir_at` / `set_dir_cursor` but
    /// reject byte-oriented operations (read/write/seek/read_at/write_at)
    /// with `IsADirectory`.  The `offset` field doubles as the directory
    /// cursor (entry index) for these handles.
    is_directory: bool,
    /// For a directory handle: the identity of the directory *at open time*.
    ///
    /// `path` records what the handle was opened **as**; this records what it
    /// opened **onto**.  Without it, an fd-relative operation has only a name
    /// to work from, and a name can be re-pointed at another object between
    /// the open and the use — which is the whole of lane B's report in
    /// `requests/b-a-the-at-family-resolves-by-path-so-no-toctou-fix-is-\
    /// possible.md`.  With it, [`crate::fs::Vfs::unlink_at_pinned`] and its
    /// siblings can walk the name and then check where they landed.
    ///
    /// `None` for file handles (nothing resolves relative to them), and for
    /// directories on a filesystem with no stable inode numbers, where the
    /// question cannot be answered at all.  The two cases are distinguished
    /// by `is_directory`, and neither is ever read as "verified".
    dir_pin: Option<crate::fs::FileId>,
    /// The opener's view of the file was in a read-only volume
    /// (`ipc::namespace::check_writable`), taken at open: a change made
    /// through the handle later -- `fchmod`, `fchown`, `futimens` -- meets the
    /// restriction of the volume it was opened through, as Linux's
    /// `mnt_want_write_file` checks the mount a file was opened on
    /// (`HandleFile`). A handle opened for writing never has it: such an open
    /// is refused there.
    ro_volume: bool,
}

// ---------------------------------------------------------------------------
// Global open-file table
// ---------------------------------------------------------------------------

/// Counter for generating unique handle IDs.
///
/// Starts at 1 so that 0 is never a valid handle (useful as a
/// sentinel / error indicator in userspace).
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

/// The global open-file table.
///
/// Maps handle IDs → open file state.  Protected by a spin mutex.
///
/// TODO: migrate to per-process tables once the capability system
/// tracks file handles as capabilities.  For now, a single global
/// table is correct because the kernel's few userspace processes
/// don't share handle namespaces.
static OPEN_FILES: Mutex<BTreeMap<u64, OpenFile>> = Mutex::new(BTreeMap::new());

/// Maximum number of simultaneously open files (system-wide).
///
/// Prevents runaway handle allocation from exhausting kernel heap.
const MAX_OPEN_FILES: usize = 1024;

// ---------------------------------------------------------------------------
// Capability tag enforcement
// ---------------------------------------------------------------------------

/// Check capability tags and POSIX ACLs for an open of `path` under `flags`.
///
/// This module used to carry its own transcription of the VFS's tag check —
/// the same six lines, separately maintained — which is why the ACL half
/// could have been added to the VFS and silently not applied to `open`, the
/// one entry point through which essentially all userspace file access
/// arrives. It now defers to the single gate in `vfs`.
///
/// Called at open time only; subsequent reads and writes on the handle are
/// allowed without re-checking, because the handle is proof of access (Unix
/// file-descriptor semantics — an fd survives a later `chmod`/`setfacl`).
fn check_open_access(path: &Path, flags: OpenFlags) -> KernelResult<()> {
    use crate::fs::vfs::PathAccess;

    // A create, truncate or append modifies the file even when the caller did
    // not set the write access bit, so the write intent is taken from the
    // effect rather than from the access-mode bits alone.
    let writes = flags.contains(OpenFlags::WRITE)
        || flags.contains(OpenFlags::CREATE)
        || flags.contains(OpenFlags::TRUNCATE)
        || flags.contains(OpenFlags::APPEND);

    if flags.contains(OpenFlags::READ) {
        crate::fs::vfs::check_path_access(path, PathAccess::Read)?;
    }
    if writes {
        crate::fs::vfs::check_path_access(path, PathAccess::Write)?;
    }
    // An open asking for neither still has to clear capability tags, which are
    // not access-mode-specific; `Metadata` requests no ACL permission.
    if !flags.contains(OpenFlags::READ) && !writes {
        crate::fs::vfs::check_path_access(path, PathAccess::Metadata)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Open a file and return a handle ID.
///
/// Default permission bits for a freshly-created regular file when the
/// caller does not specify a mode.  Historically the VFS stamped every
/// new file 0o644; callers that go through the bare [`open`] wrapper keep
/// exactly that behaviour.  Userspace `open(O_CREAT)`/`creat` supply their
/// own (already umask-masked) mode via [`open_with_mode`].
pub const DEFAULT_CREATE_MODE: u16 = 0o644;

/// Validates that the file exists (or creates it if `CREATE` is set),
/// caches the file size, and optionally truncates.  The handle starts
/// with offset 0 (or at end-of-file if `APPEND` is set).
///
/// This is the mode-less convenience wrapper: on `CREATE` the new file is
/// stamped [`DEFAULT_CREATE_MODE`] (0o644), preserving the historical
/// behaviour for every in-kernel caller.  Userspace file creation goes
/// through [`open_with_mode`] so a caller-supplied, umask-masked mode is
/// honoured.
pub fn open(path: impl AsRef<Path>, flags: OpenFlags) -> KernelResult<u64> {
    open_with_mode(path, flags, DEFAULT_CREATE_MODE)
}

/// Like [`open`], but when the `CREATE` flag causes a new file to be
/// created the file is stamped `create_mode` (masked to the low 12
/// permission bits — `0o7777`, so setuid/setgid/sticky survive) instead
/// of the 0o644 default.  `create_mode` is ignored when no file is
/// created (existing file, or `CREATE` absent).
///
/// The mode is expected to be **already umask-masked by the caller**: our
/// umask lives in the userspace POSIX layer (per design-decision on the
/// thin-kernel-primitive model), so the kernel treats `create_mode` as the
/// final on-disk permission bits and does not apply any mask of its own.
pub fn open_with_mode(
    path: impl AsRef<Path>,
    flags: OpenFlags,
    create_mode: u16,
) -> KernelResult<u64> {
    open_impl(path.as_ref(), flags, create_mode, None)
}

/// Set an open description's status flags, as Linux's `fcntl(F_SETFL)` does
/// (`SYS_FS_SET_STATUS_FLAGS`): `APPEND` is taken from `flags`, and the
/// access mode and the creation bits are ignored, as `F_SETFL` ignores them.
///
/// It acts on the description, so every descriptor sharing the handle sees
/// it. With `APPEND` set, each write lands at the file's end as it is when the
/// write lands, as one opened with `O_APPEND` does.
///
/// An append-only file's (`chattr +a`) description keeps `APPEND` as it was
/// opened, as Linux's `setfl` keeps `O_APPEND`
/// (`fs::attr_policy::may_change_appending`): otherwise the rule that it is
/// opened for writing only to append would last until the first `F_SETFL`.
///
/// # Errors
///
/// `InvalidHandle` for a handle that is not open; `IsADirectory` for a
/// directory handle, which has no byte position to append at;
/// `NotPermitted` for a change to `APPEND` on an append-only file.
pub fn set_status_flags(handle: u64, flags: OpenFlags) -> KernelResult<()> {
    let append = OpenFlags::APPEND.bits();
    let (is_directory, appending) = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        (file.is_directory, file.flags.bits() & append)
    };
    if is_directory {
        return Err(KernelError::IsADirectory);
    }
    // With the table lock released: the VFS is not called with it held (see
    // `advance_offset`). A file whose metadata cannot be read has no
    // attributes to keep the flag for, and the change goes ahead.
    if appending != flags.bits() & append
        && let Ok(meta) = fstat(handle)
    {
        crate::fs::attr_policy::may_change_appending(meta.attributes)?;
    }
    let mut table = OPEN_FILES.lock();
    let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;
    file.flags = OpenFlags::from_bits((file.flags.bits() & !append) | (flags.bits() & append));
    Ok(())
}

/// Open a regular file with no name in directory `dir`: Linux's
/// `O_TMPFILE`, the native `SYS_FS_TMPFILE`.
///
/// The file exists only through the handle and its duplicates: no one can
/// open it by a name, nothing of it is left behind by a crash or a holder
/// that never cleans up, and it goes at the last close -- unless
/// [`link_handle`] names it first. That is allowed unless `flags` has
/// `EXCL`, as `O_EXCL` forbids it on Linux. It is shown under `dir/#ino`
/// (`handle_name`), as Linux shows one.
///
/// `flags`: the access mode, which must allow writing, as Linux requires;
/// `APPEND`; `EXCL`; `NO_SYMLINKS` for the walk to `dir`. `mode` is the
/// file's permission bits, already less the caller's umask.
///
/// # Errors
///
/// - `InvalidArgument`: `flags` does not allow writing.
/// - `NotADirectory`: `dir` is not a directory.
/// - `NotSupported`: the filesystem cannot keep a file with no name (FAT,
///   the pseudo filesystems: no inode numbers), as Linux answers
///   `EOPNOTSUPP`.
/// - The walk's and the create's own.
pub fn open_tmpfile(dir: impl AsRef<Path>, flags: OpenFlags, mode: u16) -> KernelResult<u64> {
    open_tmpfile_impl(dir.as_ref(), flags, mode, None)
}

/// [`open_tmpfile`] in a directory reached under `openat2`'s
/// `RESOLVE_BENEATH`: the walk to it may not leave `b.base`.
///
/// # Errors
///
/// As [`open_tmpfile`]; `CrossDevice` for a walk that would leave.
pub fn open_tmpfile_beneath(b: Beneath<'_>, flags: OpenFlags, mode: u16) -> KernelResult<u64> {
    open_tmpfile_impl(b.rel, flags, mode, Some(b))
}

fn open_tmpfile_impl(
    dir: &Path,
    flags: OpenFlags,
    mode: u16,
    beneath: Option<Beneath<'_>>,
) -> KernelResult<u64> {
    if !flags.is_writable() {
        return Err(KernelError::InvalidArgument);
    }
    let no_symlinks = flags.contains(OpenFlags::NO_SYMLINKS);
    // Resolved as a write open's path is, and in the same order (`open_impl`).
    let resolved = match beneath {
        Some(b) => {
            let resolved = crate::fs::Vfs::resolve_beneath(b.base, b.rel, true, no_symlinks)?;
            crate::ipc::namespace::check_writable(&resolved)?;
            resolved
        }
        None => {
            crate::ipc::namespace::check_writable(dir)?;
            if no_symlinks {
                crate::fs::Vfs::resolve_no_symlinks(dir)?
            } else {
                crate::fs::Vfs::resolve_path(dir)?
            }
        }
    };
    if crate::fs::Vfs::stat_resolved(&resolved)?.entry_type != crate::fs::EntryType::Directory {
        return Err(KernelError::NotADirectory);
    }
    let linkable = !flags.contains(OpenFlags::EXCL);
    let hold = crate::fs::Vfs::create_unnamed_object(&resolved, mode & 0o7777, linkable)?;
    let shown_as = resolved.join(alloc::format!("#{}", hold.id().ino));
    let file_flags = OpenFlags::from_bits(
        flags.bits()
            & (OpenFlags::READ.bits() | OpenFlags::WRITE.bits() | OpenFlags::APPEND.bits()),
    );
    allocate_handle_holding(shown_as, 0, 0, file_flags, Some(Arc::new(hold)))
}

/// Give the file `handle` holds the name `new_path`: `linkat` of a
/// descriptor (`AT_EMPTY_PATH`, or `/proc/self/fd/N` followed), the native
/// `SYS_FS_LINK_HANDLE`. See [`crate::fs::Vfs::link_object`] for which files
/// may be named: one made by [`open_tmpfile`] without `EXCL` gets its first
/// name, one with a name another.
///
/// # Errors
///
/// `InvalidHandle` for a handle that is not open; `IsADirectory` for a
/// directory, which takes no second name; `NotSupported` for a file held by
/// name (a filesystem without inode numbers, which have no hard links
/// either); `link_object`'s own.
pub fn link_handle(handle: u64, new_path: impl AsRef<Path>) -> KernelResult<()> {
    let object = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        if file.is_directory {
            return Err(KernelError::IsADirectory);
        }
        file.object.clone()
    };
    match &object {
        Some(held) => crate::fs::Vfs::link_object(held, new_path),
        None => Err(KernelError::NotSupported),
    }
}

/// A request to open under `openat2`'s `RESOLVE_BENEATH`.
///
/// The two halves travel together in one type on purpose: a base without
/// the caller's own fragment cannot be checked (the fragment's `..` are
/// only visible before the two are joined and normalized), and a fragment
/// without a base has nothing to be contained by.  Passing them as two
/// `Option`s that must agree is the shape that lets one arrive without the
/// other.
#[derive(Clone, Copy)]
pub struct Beneath<'a> {
    /// The directory the walk may not leave — `openat2`'s `dirfd`.
    pub base: &'a Path,
    /// The caller's path, relative to `base`.
    pub rel: &'a Path,
}

/// Like [`open_with_mode`], but the resolution may not escape `b.base`.
///
/// Implements `openat2(dirfd, path, { .resolve = RESOLVE_BENEATH })`.  Any
/// escape — a `..` in `b.rel`, an absolute `b.rel`, or a symlink met on the
/// way whose target leaves (or is absolute, or leaves and returns) — fails
/// with [`KernelError::CrossDevice`] (→ `EXDEV`).  See
/// [`crate::fs::Vfs::beneath_step`] for why the rule is per-hop rather than
/// a check on the final answer.
pub fn open_beneath(b: Beneath<'_>, flags: OpenFlags) -> KernelResult<u64> {
    open_beneath_with_mode(b, flags, DEFAULT_CREATE_MODE)
}

/// Like [`open_beneath`], but a file created by `CREATE` is stamped
/// `create_mode` instead of [`DEFAULT_CREATE_MODE`].
///
/// This is what `SYS_FS_OPENAT2` calls, and it exists for the same reason
/// [`open_with_mode`] does: `openat2(2)` carries a mode, and a containment
/// wrapper that quietly substituted 0o644 would create files with
/// permissions the caller did not ask for and cannot see it did not get.
/// `open_beneath` is now defined in terms of this one rather than beside
/// it, so the two can never drift on anything but the mode.
///
/// `create_mode` is the final on-disk permission bits, already
/// umask-masked by the caller, exactly as for [`open_with_mode`].
pub fn open_beneath_with_mode(
    b: Beneath<'_>,
    flags: OpenFlags,
    create_mode: u16,
) -> KernelResult<u64> {
    open_impl(b.rel, flags, create_mode, Some(b))
}

fn open_impl(
    path: &Path,
    flags: OpenFlags,
    create_mode: u16,
    beneath: Option<Beneath<'_>>,
) -> KernelResult<u64> {
    // Must have at least READ or WRITE.
    if !flags.is_readable() && !flags.is_writable() {
        return Err(KernelError::InvalidArgument);
    }

    // The opener's view of the file in a read-only volume, kept with the
    // handle (`OpenFile::ro_volume`); a write open there is refused below.
    let ro_volume = matches!(
        match beneath {
            Some(b) => crate::ipc::namespace::check_writable(b.base.join(b.rel)),
            None => crate::ipc::namespace::check_writable(path),
        },
        Err(KernelError::ReadOnlyFilesystem)
    );

    // RESOLVE_BENEATH takes its own route to `norm`, because the *order* of
    // the checks below is part of the guarantee.  A caller that asked for
    // containment must not be able to learn anything about a path outside
    // the base -- not even whether it exists -- so the containment-aware
    // resolution runs first and every later probe is made against its
    // result.  Running the ordinary `check_writable` / NOFOLLOW-`lstat` on
    // the naively joined path would probe `base/../out/f` before anything
    // had refused it, which is an existence oracle for the tree the flag
    // was invoked to hide.
    if let Some(b) = beneath {
        let no_symlinks = flags.contains(OpenFlags::NO_SYMLINKS);

        // Resolve without following the final component.  This validates
        // the caller's fragment and every parent hop under the containment
        // rule, so from here on the path is known to be inside the base.
        let parent_resolved = crate::fs::Vfs::resolve_beneath(b.base, b.rel, false, no_symlinks)?;

        // O_NOFOLLOW, applied to the contained path rather than the raw
        // one.  Same meaning as the general case below: a final component
        // that is itself a symlink is ELOOP.
        if flags.contains(OpenFlags::NOFOLLOW) {
            match crate::fs::Vfs::lstat(&parent_resolved) {
                Ok(entry) if entry.entry_type == crate::fs::EntryType::Symlink => {
                    return Err(KernelError::TooManyLinks);
                }
                Ok(_) | Err(KernelError::NotFound) => {}
                Err(e) => return Err(e),
            }
        }

        if flags.is_writable()
            || flags.contains(OpenFlags::CREATE)
            || flags.contains(OpenFlags::TRUNCATE)
            || flags.contains(OpenFlags::APPEND)
        {
            crate::ipc::namespace::check_writable(&parent_resolved)?;
        }

        // Follow the final component unless NOFOLLOW asked us not to; the
        // walk is contained either way.
        let norm = if flags.contains(OpenFlags::NOFOLLOW) {
            parent_resolved
        } else {
            crate::fs::Vfs::resolve_beneath(b.base, b.rel, true, no_symlinks)?
        };
        return open_resolved(norm, flags, create_mode).map(|h| marked(h, ro_volume));
    }

    // Read-only volume enforcement: if this open would mutate the file
    // (write, create, truncate, or append), and the calling process has a
    // read-only volume mount covering this path, reject with EROFS before
    // touching the filesystem.  This is a cheap no-op for any process
    // without read-only volumes (the common case).
    if flags.is_writable()
        || flags.contains(OpenFlags::CREATE)
        || flags.contains(OpenFlags::TRUNCATE)
        || flags.contains(OpenFlags::APPEND)
    {
        crate::ipc::namespace::check_writable(path)?;
    }

    // O_NOFOLLOW: refuse to open through a final symlink component.  The
    // resolve_path below follows every symlink (including the last), so the
    // guard must run first — lstat does NOT follow the final component, so it
    // reports the link itself.  A symlink final component → ELOOP; anything
    // else (regular/dir/missing) falls through to the normal open/create path
    // (parent-component symlinks are still followed, matching POSIX).  Only
    // the final component is guarded, so a missing target is not our concern
    // here (CREATE handles it below).
    if flags.contains(OpenFlags::NOFOLLOW) {
        match crate::fs::Vfs::lstat(path) {
            Ok(entry) if entry.entry_type == crate::fs::EntryType::Symlink => {
                return Err(KernelError::TooManyLinks);
            }
            // Not a symlink, or the path/target doesn't exist yet — let the
            // regular resolve/create path decide (ENOENT vs. create).
            Ok(_) | Err(KernelError::NotFound) => {}
            Err(e) => return Err(e),
        }
    }

    // Resolve symlinks at open time so the handle refers to the
    // underlying file, not the symlink.  This matches Unix semantics:
    // if the symlink is later changed, existing handles still point
    // to the original target.
    //
    // NO_SYMLINKS (openat2 RESOLVE_NO_SYMLINKS) uses the stricter resolver
    // that refuses to traverse a symlink in *any* component — parent or
    // final — returning ELOOP.  It subsumes the NOFOLLOW final-component
    // guard above, so we never reach here having followed a link.
    let norm = if flags.contains(OpenFlags::NO_SYMLINKS) {
        crate::fs::Vfs::resolve_no_symlinks(path)?
    } else {
        crate::fs::Vfs::resolve_path(path)?
    };

    open_resolved(norm, flags, create_mode).map(|h| marked(h, ro_volume))
}

/// `handle`, marked as opened in a read-only volume when `ro_volume`
/// (`OpenFile::ro_volume`). Before the handle is returned, so nothing can
/// use it unmarked.
fn marked(handle: u64, ro_volume: bool) -> u64 {
    if ro_volume && let Some(file) = OPEN_FILES.lock().get_mut(&handle) {
        file.ro_volume = true;
    }
    handle
}

/// Everything an open does once the path has been resolved.
///
/// Split out so `RESOLVE_BENEATH` can reach it: that mode resolves by a
/// different route (containment-checked, and in a different order — see
/// [`open_impl`]) but must then do exactly what every other open does.
/// Sharing the tail is what keeps the two from drifting, which is the same
/// failure mode lane B found between the kernel's `openat2` and libc's.
fn open_resolved(norm: PathBuf, flags: OpenFlags, create_mode: u16) -> KernelResult<u64> {
    open_resolved_within(norm, flags, create_mode, CREATE_RACE_RETRIES)
}

/// How many times an `O_CREAT` open whose create found the name taken -- made
/// by another between its lookup and its create -- goes back to open what was
/// made, before it gives up with `WouldBlock` (`EAGAIN`). One is what a real
/// race needs; the rest are for a name made and removed repeatedly.
const CREATE_RACE_RETRIES: u32 = 4;

/// [`open_resolved`], with `create_races_left` more passes allowed after a
/// create that found the name taken.
fn open_resolved_within(
    norm: PathBuf,
    flags: OpenFlags,
    create_mode: u16,
    create_races_left: u32,
) -> KernelResult<u64> {
    // Check capability tags and POSIX ACLs — the process must be a member of
    // all groups required for this path (or any ancestor with tags), and the
    // path's ACL, if it has one, must grant the access this open asks for.
    // Checked at open time; subsequent reads/writes on the handle are allowed
    // without re-checking (the handle is proof of access, like a file
    // descriptor).
    check_open_access(&norm, flags)?;

    // Check if the file exists.
    // Note: Vfs::stat() runs the same gate internally, but our explicit check
    // above handles the CREATE case (file doesn't exist yet, so stat is never
    // called — we still need the check).
    let stat_result = crate::fs::Vfs::stat_resolved(&norm);

    match stat_result {
        Ok(entry) => {
            // File exists.
            // O_CREAT | O_EXCL demands that *this* open create the file, so an
            // existing path (of any type) must fail with EEXIST (POSIX; Linux
            // do_last()).  Checked before the dir/regular split so exclusive
            // create over an existing directory also fails with EEXIST.  EXCL
            // is only honoured together with CREATE (matching Linux's
            // regular-file semantics); EXCL alone is ignored.
            if flags.contains(OpenFlags::CREATE) && flags.contains(OpenFlags::EXCL) {
                return Err(KernelError::AlreadyExists);
            }
            if entry.entry_type == crate::fs::EntryType::Directory {
                // Directory: only allowed if the caller asked for one, or
                // for whatever the path names.
                if !flags.contains(OpenFlags::DIRECTORY)
                    && !flags.contains(OpenFlags::DIRECTORY_ALLOWED)
                {
                    return Err(KernelError::IsADirectory);
                }
                // O_DIRECTORY combined with anything that would mutate a
                // file makes no sense — TRUNCATE / CREATE / APPEND only
                // apply to regular files.  Reject early so we don't
                // silently ignore the flag.
                if flags.contains(OpenFlags::TRUNCATE)
                    || flags.contains(OpenFlags::CREATE)
                    || flags.contains(OpenFlags::APPEND)
                    || flags.is_writable()
                {
                    return Err(KernelError::IsADirectory);
                }
                return allocate_dir_handle(norm.clone(), flags);
            }

            // Regular file.
            if flags.contains(OpenFlags::DIRECTORY) {
                // Caller demanded a directory but found something else.
                return Err(KernelError::NotADirectory);
            }

            // A Unix-domain socket's name has nothing behind it to read or
            // write: what it names is reached by `connect`. `ENXIO`, as on
            // Linux -- `ntpdate` opening `/dev/log` as a file is the case
            // that meets this.
            if entry.entry_type == crate::fs::EntryType::Socket {
                return Err(KernelError::NoSuchDeviceOrAddress);
            }

            // `chattr +i` and `+a`: refused at the open, as Linux's
            // `may_open` refuses them, rather than at the first write
            // (`fs::attr_policy::may_open`) -- `echo x > file` fails at its
            // redirection. A file whose metadata cannot be read has no
            // attributes to refuse by; the writes are checked again by the
            // filesystem regardless, on the inode they change.
            if (flags.is_writable() || flags.contains(OpenFlags::TRUNCATE))
                && let Ok(meta) = crate::fs::Vfs::metadata_resolved(&norm)
            {
                crate::fs::attr_policy::may_open(
                    meta.attributes,
                    flags.is_writable(),
                    flags.contains(OpenFlags::APPEND),
                    flags.contains(OpenFlags::TRUNCATE),
                )?;
            }

            let mut size = entry.size;

            // Handle TRUNCATE flag.
            if flags.contains(OpenFlags::TRUNCATE) {
                if !flags.is_writable() {
                    return Err(KernelError::InvalidArgument);
                }
                crate::fs::Vfs::truncate_resolved(&norm, 0)?;
                size = 0;
            }

            let offset = if flags.contains(OpenFlags::APPEND) {
                size
            } else {
                0
            };

            // inotify IN_OPEN: emit only after the handle is installed so a
            // failed allocation never produces a spurious open event.  The
            // emit is gated lock-free on the OPEN interest count, so opens
            // pay nothing when no watch is watching for opens.
            let handle = allocate_handle(norm.clone(), offset, size, flags)?;
            crate::fs::notify::emit_opened(&norm);
            Ok(handle)
        }
        Err(KernelError::NotFound) => {
            // File doesn't exist — create if CREATE is set.
            // O_DIRECTORY|O_CREAT is nonsensical: directories are created
            // by mkdir, not open.  Mirror Linux which simply fails the
            // lookup with ENOENT in that combination.
            if flags.contains(OpenFlags::DIRECTORY) {
                return Err(KernelError::NotFound);
            }
            if !flags.contains(OpenFlags::CREATE) {
                return Err(KernelError::NotFound);
            }
            if !flags.is_writable() {
                return Err(KernelError::InvalidArgument);
            }

            // Create an empty file -- with its mode, `create_mode` less the
            // caller's umask or as the directory's default ACL gives it, its
            // owner and its ACL, under one hold of the filesystem's lock
            // (`Vfs::create_file_resolved`). Twelve bits, not nine: the VFS
            // stores the setuid, setgid and sticky bits a caller asks for
            // (design-decisions.md §639; `vfs::stamp_new_mode` has the
            // history). The create emits IN_CREATE; the IN_OPEN below follows
            // it, matching Linux's O_CREAT open order.
            match crate::fs::Vfs::create_file_resolved(&norm, create_mode & 0o7777) {
                Ok(()) => {}
                // Made by someone else between the lookup above and this
                // create: open what they made, as Linux's O_CREAT would have
                // (it holds the directory's lock across the lookup and the
                // create, so it never meets this). O_EXCL is refused by the
                // arm below, `AlreadyExists` being its answer anyway.
                Err(KernelError::AlreadyExists) if !flags.contains(OpenFlags::EXCL) => {
                    return match create_races_left.checked_sub(1) {
                        Some(left) => open_resolved_within(norm, flags, create_mode, left),
                        // A name made and removed again on every pass: not
                        // chased further, so the recursion an adversary paces
                        // stays bounded. Try again, which is what EAGAIN says.
                        None => Err(KernelError::WouldBlock),
                    };
                }
                Err(e) => return Err(e),
            }

            let handle = allocate_handle(norm.clone(), 0, 0, flags)?;
            crate::fs::notify::emit_opened(&norm);
            Ok(handle)
        }
        Err(e) => Err(e),
    }
}

/// The file's size now: from the file held, or by name for one held by
/// name.
fn current_size(object: Option<&crate::fs::vfs::FileHold>, path: &Path) -> KernelResult<u64> {
    match object {
        Some(obj) => crate::fs::Vfs::object_metadata(obj).map(|m| m.size),
        None => crate::fs::Vfs::metadata_resolved(path).map(|m| m.size),
    }
}

/// Close an open file handle.
///
/// Frees the handle ID and releases any advisory locks held with
/// this handle ID as owner.  Further operations on this handle
/// will return `InvalidHandle`.
pub fn close(handle: u64) -> KernelResult<()> {
    // Decrement the open file description's refcount.  Only the final
    // close (refcount → 0) removes the entry and releases advisory
    // locks — earlier closes by other owners (forked siblings, shared
    // dups) just drop their reference, leaving the shared cursor intact
    // for the remaining owners.
    let closed = {
        let mut table = OPEN_FILES.lock();
        let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;
        file.refcount = file.refcount.saturating_sub(1);
        if file.refcount > 0 {
            // Other owners still hold this description; nothing to tear
            // down yet.
            return Ok(());
        }
        // Last reference — remove the entry and capture the path, write-mode,
        // and directory flag so we can release any advisory lock and emit an
        // inotify close event below (both after dropping this lock, to keep
        // the OPEN_FILES → WATCHES lock order one-directional).
        table.remove(&handle).map(|file| {
            (
                file.path,
                file.flags.is_writable(),
                file.is_directory,
                file.object,
            )
        })
    };

    if let Some((ref p, writable, is_dir, object)) = closed {
        // Release the `flock` lock this description holds, keyed on its file
        // as it was taken (`lock_key`): the file held, else what its name --
        // the resolved host path captured at open, so no namespace is
        // applied twice -- names now. Best effort: a release has nothing to
        // report.
        let id = match &object {
            Some(held) => Some(held.id()),
            None => crate::fs::Vfs::file_identity_resolved(p).unwrap_or(None),
        };
        crate::fs::Vfs::funlock_key(p, id, crate::fs::vfs::flock_description_owner(handle));
        // And the fcntl OFD record locks. An OFD lock belongs to the open
        // file description, so the final close of that description is
        // exactly when it ends -- that is what distinguishes it from a
        // POSIX lock, which ends when the process does and is released in
        // `pcb.rs` instead. Both guards above are already dropped, so this
        // takes `reclock`'s table without holding any filesystem lock.
        crate::fs::reclock::release_ofd(handle);

        // inotify IN_CLOSE_WRITE / IN_CLOSE_NOWRITE on the final close of the
        // open file description.  Directory handles report the close too and
        // are tagged `is_dir` so the inotify adapter ORs in IN_ISDIR.  Gated
        // lock-free on the matching CLOSE interest count.
        crate::fs::notify::emit_closed(p, writable, is_dir);

        // Last: this description's reference to the file's hold. A file
        // whose last name went while it was open goes with the last
        // reference -- here, unless another description or a call still in
        // progress through this one has one.
        drop(object);
    }

    Ok(())
}

/// Read up to `buf_len` bytes from the file at the current offset.
///
/// Advances the offset by the number of bytes read.  Returns the
/// number of bytes actually read (may be less than `buf_len` if
/// near end-of-file; 0 means already at EOF).
pub fn read(handle: u64, buf: &mut [u8]) -> KernelResult<usize> {
    // Snapshot what the VFS call needs, then drop the table lock before
    // making it. See `advance_offset` for why holding it across the call is
    // a deadlock and not merely a bottleneck.
    let (object, path, start) = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;

        if file.is_directory {
            return Err(KernelError::IsADirectory);
        }

        if !file.flags.is_readable() {
            return Err(KernelError::PermissionDenied);
        }

        (file.object.clone(), file.path.clone(), file.offset)
    };

    // The end of file is the file's now, not as this handle last saw it:
    // both calls below clamp to the current size.
    let data = match &object {
        Some(obj) => crate::fs::Vfs::object_read(obj, &path, start, buf.len())?,
        None => crate::fs::Vfs::read_at_resolved(&path, start, buf.len())?,
    };
    let copy_len = data.len().min(buf.len());

    if let Some(dest) = buf.get_mut(..copy_len) {
        if let Some(src) = data.get(..copy_len) {
            dest.copy_from_slice(src);
        }
    }

    advance_offset(handle, start, copy_len as u64);

    Ok(copy_len)
}

/// Move an open handle's cursor to `start + delta`, but never backwards.
///
/// Called after the VFS I/O has completed and the table lock has been
/// released and retaken, so it must re-look-up the handle rather than hold a
/// reference across the call: the map may have been rebalanced, and the
/// handle may have been closed outright. A vanished handle is not an error —
/// the bytes really were transferred — so the bookkeeping is simply skipped.
///
/// The advance is `max`, not `+=`, for the case where another thread sharing
/// the handle completed its own transfer while this one was in the VFS: adding
/// would count that thread's progress twice. Two threads reading one handle
/// get an arbitrary interleaving either way — Linux guards only the position
/// update itself, with `f_pos_lock`, and offers no more — but a monotone
/// cursor never re-reads bytes it has already returned, which is the property
/// callers actually depend on.
///
/// ## Why the lock is dropped at all
///
/// Holding `OPEN_FILES` across a VFS call inverts the kernel's lock order.
/// `Vfs::readdir` takes the *filesystem's* lock and calls `readdir` under it;
/// for procfs that generates content, which reaches `list_handles`, which
/// takes `OPEN_FILES`. So one path runs FS → `OPEN_FILES` while the old
/// `read`/`write` ran `OPEN_FILES` → FS. Lockdep observed both orders in a
/// single boot (batch 32). Dropping the lock also stops every file read in
/// the system from serialising behind one global mutex, which the original
/// comment here acknowledged as "acceptable for early dev".
fn advance_offset(handle: u64, start: u64, delta: u64) {
    if let Some(file) = OPEN_FILES.lock().get_mut(&handle) {
        file.offset = file.offset.max(start.saturating_add(delta));
    }
}

/// Return the file offset at which the next byte written via
/// [`fn@write`] would land.
///
/// This is the file's size now for handles opened with
/// [`OpenFlags::APPEND`] (POSIX rule: append-mode writes always go to EOF,
/// ignoring the stored offset) and `file.offset` otherwise.
///
/// Used by the Linux ABI translation layer to enforce `RLIMIT_FSIZE`
/// against the current-offset write paths (`write(2)`, `writev(2)`)
/// — those syscalls don't carry an explicit offset, so the kernel
/// must peek at the open-file description to know where the write
/// will land before clipping or rejecting it.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] — `handle` is not in the table.
/// - [`KernelError::IsADirectory`] — `handle` is a directory handle
///   (which doesn't support byte-oriented writes).
/// - [`KernelError::PermissionDenied`] — handle was not opened for
///   writing.
pub fn peek_write_offset(handle: u64) -> KernelResult<u64> {
    let (object, path, offset, append) = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        if file.is_directory {
            return Err(KernelError::IsADirectory);
        }
        if !file.flags.is_writable() {
            return Err(KernelError::PermissionDenied);
        }
        (
            file.object.clone(),
            file.path.clone(),
            file.offset,
            file.flags.contains(OpenFlags::APPEND),
        )
    };
    // The size is looked up with the table lock released: see
    // `advance_offset`.
    if append {
        current_size(object.as_deref(), &path)
    } else {
        Ok(offset)
    }
}

/// Return the file's current offset (`f_pos`) **without** modifying it.
///
/// This is the raw open-file-description position — exactly the value
/// `/proc/<pid>/fdinfo/<n>` reports as `pos:`.  Unlike
/// [`peek_write_offset`], it never applies the `APPEND`→EOF adjustment:
/// `pos` is literally `f_pos`, the same as `lseek(fd, 0, SEEK_CUR)`.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] — `handle` is not in the table.
/// - [`KernelError::IsADirectory`] — `handle` is a directory handle
///   (directories have no byte offset).
pub fn current_offset(handle: u64) -> KernelResult<u64> {
    let table = OPEN_FILES.lock();
    let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
    if file.is_directory {
        return Err(KernelError::IsADirectory);
    }
    Ok(file.offset)
}

/// Write bytes to the file at the current offset (or at EOF if APPEND).
///
/// Advances the offset by the number of bytes written.  Grows the
/// file if writing past the current end.  Returns bytes written.
pub fn write(handle: u64, data: &[u8]) -> KernelResult<usize> {
    // Snapshot, then release: see `advance_offset` for why the table lock
    // must not span the VFS call.
    let (object, path, offset, append) = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;

        if file.is_directory {
            return Err(KernelError::IsADirectory);
        }

        if !file.flags.is_writable() {
            return Err(KernelError::PermissionDenied);
        }

        (
            file.object.clone(),
            file.path.clone(),
            file.offset,
            file.flags.contains(OpenFlags::APPEND),
        )
    };

    // An APPEND write lands at the file's end as it is when it lands: the
    // end is found and written in one hold of the filesystem's lock, so two
    // appenders -- through this handle or any other -- never land on one
    // offset. Until 2026-10-01 it went to the end as this handle had last
    // seen it, and two appenders through separate opens overwrote each other.
    let write_offset = match (&object, append) {
        (Some(obj), true) => crate::fs::Vfs::object_append(obj, &path, data)?,
        (Some(obj), false) => {
            crate::fs::Vfs::object_write(obj, &path, offset, data)?;
            offset
        }
        (None, true) => crate::fs::Vfs::append_resolved(&path, data)?,
        (None, false) => {
            crate::fs::Vfs::write_at_resolved(&path, offset, data)?;
            offset
        }
    };

    let written = data.len();

    // Both APPEND and non-APPEND leave the cursor at the end of what was just
    // written.
    let new_end = write_offset.saturating_add(written as u64);
    if let Some(file) = OPEN_FILES.lock().get_mut(&handle) {
        file.offset = file.offset.max(new_end);
        file.seen_size = file.seen_size.max(new_end);
    }

    Ok(written)
}

/// Read up to `buf.len()` bytes starting at an explicit `offset`,
/// **without** modifying the file's current offset.
///
/// This is the backbone of `pread64(2)` and `preadv(2)` — they're
/// defined to be atomic with respect to the current offset (no
/// observable change after the call returns).  We achieve that by
/// going straight to the VFS with the caller-supplied offset and
/// never touching `file.offset`.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] — `handle` is not in the table.
/// - [`KernelError::PermissionDenied`] — handle was not opened for
///   reading.
/// - VFS errors propagated unchanged.
pub fn read_at(handle: u64, offset: u64, buf: &mut [u8]) -> KernelResult<usize> {
    // Snapshot, then release: see `advance_offset` for why the table lock
    // must not span the VFS call. `pread` touches no cursor, so unlike
    // `read` there is nothing to write back afterwards.
    let (object, path) = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        if file.is_directory {
            return Err(KernelError::IsADirectory);
        }
        if !file.flags.is_readable() {
            return Err(KernelError::PermissionDenied);
        }
        if buf.is_empty() {
            return Ok(0);
        }
        (file.object.clone(), file.path.clone())
    };
    // Clamped to the file's size now, by the calls themselves.
    let data = match &object {
        Some(obj) => crate::fs::Vfs::object_read(obj, &path, offset, buf.len())?,
        None => crate::fs::Vfs::read_at_resolved(&path, offset, buf.len())?,
    };
    let copy_len = data.len().min(buf.len());
    if let Some(dest) = buf.get_mut(..copy_len) {
        if let Some(src) = data.get(..copy_len) {
            dest.copy_from_slice(src);
        }
    }
    Ok(copy_len)
}

/// Read up to `buf.len()` bytes at an explicit `offset` **directly from the
/// backing filesystem**, bypassing the page cache.
///
/// Identical to [`read_at`] except it routes through
/// [`crate::fs::Vfs::read_at_uncached`].  Used by the `mmap` fault path's
/// page-cache fill closure, which must read a file's data *without* re-entering
/// [`crate::mm::page_cache::get_or_fill`] — calling the cached [`read_at`] there
/// would recurse on the very page being filled (design-decisions §38).
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] — `handle` is not in the table.
/// - [`KernelError::IsADirectory`] — `handle` refers to a directory.
/// - [`KernelError::PermissionDenied`] — handle was not opened for reading.
/// - VFS errors propagated unchanged.
pub fn read_at_uncached(handle: u64, offset: u64, buf: &mut [u8]) -> KernelResult<usize> {
    let (object, path) = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        if file.is_directory {
            return Err(KernelError::IsADirectory);
        }
        if !file.flags.is_readable() {
            return Err(KernelError::PermissionDenied);
        }
        (file.object.clone(), file.path.clone())
    };
    if buf.is_empty() {
        return Ok(0);
    }
    // Past the end, the filesystem itself returns nothing.
    let data = match &object {
        Some(obj) => crate::fs::Vfs::object_read_uncached(obj, offset, buf.len())?,
        None => crate::fs::Vfs::read_at_uncached_resolved(&path, offset, buf.len())?,
    };
    let copy_len = data.len().min(buf.len());
    if let Some(dest) = buf.get_mut(..copy_len) {
        if let Some(src) = data.get(..copy_len) {
            dest.copy_from_slice(src);
        }
    }
    Ok(copy_len)
}

/// Write bytes at an explicit `offset`, **without** modifying the
/// file's current offset.
///
/// Backbone of `pwrite64(2)` and `pwritev(2)`.  Linux ignores the
/// `O_APPEND` flag for `pwrite` (POSIX: "the offset argument shall be
/// used and the file offset shall not be changed") — we follow that
/// rule.  Grows the cached size if the write extends past the
/// current end-of-file.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] — `handle` is not in the table.
/// - [`KernelError::PermissionDenied`] — handle was not opened for
///   writing.
/// - VFS errors propagated unchanged.
pub fn write_at(handle: u64, offset: u64, data: &[u8]) -> KernelResult<usize> {
    // Snapshot, then release: see `advance_offset` for why the table lock
    // must not span the VFS call.
    let (object, path) = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        if file.is_directory {
            return Err(KernelError::IsADirectory);
        }
        if !file.flags.is_writable() {
            return Err(KernelError::PermissionDenied);
        }
        if data.is_empty() {
            return Ok(0);
        }
        (file.object.clone(), file.path.clone())
    };
    match &object {
        Some(obj) => crate::fs::Vfs::object_write(obj, &path, offset, data)?,
        None => crate::fs::Vfs::write_at_resolved(&path, offset, data)?,
    }
    let written = data.len();
    let new_end = offset.saturating_add(written as u64);
    if let Some(file) = OPEN_FILES.lock().get_mut(&handle) {
        file.seen_size = file.seen_size.max(new_end);
    }
    Ok(written)
}

/// Read a page of directory entries starting at the handle's current
/// cursor, **without** advancing the cursor.
///
/// Returns `(start_cursor, entries)` where `start_cursor` is the cursor
/// value that was in effect when the read began (so the caller can
/// compute the next cursor by adding the number of entries consumed
/// and then invoke [`set_dir_cursor`]).
///
/// Directory handles store the cursor in the same `offset` field used
/// by file handles for byte position — interpreted here as an entry
/// index into the underlying VFS listing.  The split between "read
/// page" and "advance cursor" matches the `getdents64(2)` contract:
/// the caller may consume only a prefix of the returned entries when
/// the userspace buffer fills up mid-page, and must then advance the
/// cursor by exactly that prefix length, not by the full page.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] — `handle` is not in the table.
/// - [`KernelError::NotADirectory`] — `handle` refers to a file.
/// - VFS errors propagated unchanged.
pub fn read_dir_at(
    handle: u64,
    max_entries: usize,
) -> KernelResult<(u64, alloc::vec::Vec<crate::fs::DirEntry>)> {
    let (path, cursor) = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        if !file.is_directory {
            return Err(KernelError::NotADirectory);
        }
        (file.path.clone(), file.offset)
    };

    let start = usize::try_from(cursor).map_err(|_| KernelError::InvalidArgument)?;
    let (entries, _total) = crate::fs::Vfs::readdir_at_resolved(&path, start, max_entries)?;
    Ok((cursor, entries))
}

/// Advance (or rewind) a directory handle's cursor.
///
/// `cursor` is an entry index — typically the previous cursor plus the
/// number of entries actually consumed by userspace.  Passing 0 rewinds
/// the iteration to the start, mirroring `rewinddir(3)`.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] — `handle` is not in the table.
/// - [`KernelError::NotADirectory`] — `handle` refers to a file.
pub fn set_dir_cursor(handle: u64, cursor: u64) -> KernelResult<()> {
    let mut table = OPEN_FILES.lock();
    let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;
    if !file.is_directory {
        return Err(KernelError::NotADirectory);
    }
    file.offset = cursor;
    Ok(())
}

/// Seek to a new position in the file.
///
/// Returns the new absolute offset after seeking.
pub fn seek(handle: u64, from: SeekFrom) -> KernelResult<u64> {
    // The forms that need the end ask the file for it, before the table lock
    // is taken: a VFS call must not be made under it (`advance_offset`). They
    // used the size this handle had last seen until 2026-10-01, so
    // `SEEK_END` missed another writer's growth.
    let held = |handle: u64| -> KernelResult<(Option<Arc<crate::fs::vfs::FileHold>>, PathBuf)> {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        if file.is_directory {
            return Err(KernelError::IsADirectory);
        }
        Ok((file.object.clone(), file.path.clone()))
    };
    let size = match from {
        SeekFrom::End(_) => {
            let (object, path) = held(handle)?;
            current_size(object.as_deref(), &path)?
        }
        _ => 0,
    };
    // SEEK_DATA / SEEK_HOLE ask the filesystem where its holes are -- also
    // before the table lock. Until 2026-10-07 every file was taken to have
    // none, so `cp --sparse`, `tar -S` and `rsync -S` read every zero of a
    // sparse file, and the C library could not find a file's holes to
    // allocate them (lane D's report). A handle held by path, on a
    // filesystem that cannot hold a file by inode, keeps that answer.
    let found = match from {
        SeekFrom::Data(pos) | SeekFrom::Hole(pos) => {
            let want_data = matches!(from, SeekFrom::Data(_));
            let (object, path) = held(handle)?;
            match object.as_deref() {
                Some(obj) => crate::fs::Vfs::object_seek_data_hole(obj, pos, want_data)?,
                None => {
                    let size = current_size(None, &path)?;
                    (pos < size).then_some(if want_data { pos } else { size })
                }
            }
        }
        _ => None,
    };

    let mut table = OPEN_FILES.lock();
    let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;

    if file.is_directory {
        return Err(KernelError::IsADirectory);
    }

    let new_offset = match from {
        SeekFrom::Start(pos) => pos,
        SeekFrom::Current(delta) => {
            if delta >= 0 {
                #[allow(clippy::cast_sign_loss)]
                let d = delta as u64;
                file.offset
                    .checked_add(d)
                    .ok_or(KernelError::InvalidArgument)?
            } else {
                #[allow(clippy::cast_sign_loss)]
                let d = delta.unsigned_abs();
                file.offset
                    .checked_sub(d)
                    .ok_or(KernelError::InvalidArgument)?
            }
        }
        SeekFrom::End(delta) => {
            if delta >= 0 {
                #[allow(clippy::cast_sign_loss)]
                let d = delta as u64;
                size.checked_add(d).ok_or(KernelError::InvalidArgument)?
            } else {
                #[allow(clippy::cast_sign_loss)]
                let d = delta.unsigned_abs();
                size.checked_sub(d).ok_or(KernelError::InvalidArgument)?
            }
        }
        // No data at or after the offset, or the offset at or past the end:
        // Linux's ENXIO (it was EINVAL, and SEEK_HOLE at exactly the end
        // succeeded).
        SeekFrom::Data(_) | SeekFrom::Hole(_) => found.ok_or(KernelError::NoSuchDeviceOrAddress)?,
    };

    file.offset = new_offset;
    Ok(new_offset)
}

/// Stat an open file handle, returning the full file metadata.
///
/// Resolves the handle to its backing path and queries the VFS for
/// rich metadata (size, type, timestamps, ownership, permissions,
/// link count, block count).  Avoids a redundant user-side path
/// lookup; the kernel already holds the resolved path for the handle.
pub fn fstat(handle: u64) -> KernelResult<crate::fs::FileMeta> {
    // Snapshot, then release: see `advance_offset` for why the table lock
    // must not span the VFS call.
    let (object, path) = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        (file.object.clone(), file.path.clone())
    };

    // The file held, whatever its name now; `nlinks` 0 once the last name
    // has gone. A file held by name: metadata() follows symlinks, but an
    // open handle already refers to the resolved target.
    match &object {
        Some(obj) => crate::fs::Vfs::object_metadata(obj),
        None => crate::fs::Vfs::metadata_resolved(&path),
    }
}

/// The flags an open handle was opened with: whether it may read and write,
/// for a caller that must refuse an operation the handle's access mode does
/// not allow (a write lock through a read-only handle is `EBADF`, POSIX
/// `fcntl`).
///
/// # Errors
///
/// `InvalidHandle` for a handle that is not open.
pub fn open_flags(handle: u64) -> KernelResult<OpenFlags> {
    let table = OPEN_FILES.lock();
    table
        .get(&handle)
        .map(|file| file.flags)
        .ok_or(KernelError::InvalidHandle)
}

/// Returns whether an open handle refers to a directory.
///
/// Cheap table lookup of the cached `is_directory` flag (no VFS round-trip).
/// Used by the `fstat`/`statx` syscall translators to report `S_IFDIR` for a
/// directory fd — without this, an `open(O_DIRECTORY)` handle (which is a
/// `HandleKind::File` fd) would stat as a regular file, breaking glibc's
/// `opendir`, which `fstat`s the fd and bails with `ENOTDIR` unless it sees
/// `S_ISDIR`.  Returns `false` for an unknown handle (the caller then reports
/// the default regular-file type, matching the pre-existing behaviour).
#[must_use]
pub fn is_directory(handle: u64) -> bool {
    let table = OPEN_FILES.lock();
    table.get(&handle).is_some_and(|file| file.is_directory)
}

/// Truncate a file to a given size by handle.
///
/// Requires the handle to be opened with WRITE permission. The offset is
/// not changed, as POSIX says: a write past the new end leaves a hole. It
/// was clamped to the new end until 2026-10-01.
pub fn ftruncate(handle: u64, size: u64) -> KernelResult<()> {
    // Snapshot, then release: see `advance_offset` for why the table lock
    // must not span the VFS call.
    let (object, path) = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;

        if file.is_directory {
            return Err(KernelError::IsADirectory);
        }

        if !file.flags.is_writable() {
            return Err(KernelError::PermissionDenied);
        }

        (file.object.clone(), file.path.clone())
    };

    match &object {
        Some(obj) => crate::fs::Vfs::object_truncate(obj, &path, size)?,
        None => crate::fs::Vfs::truncate_resolved(&path, size)?,
    }

    if let Some(file) = OPEN_FILES.lock().get_mut(&handle) {
        file.seen_size = size;
    }

    Ok(())
}

/// Duplicate a file handle, creating a new handle that refers to the
/// same file with the same flags and an independent cursor position.
///
/// The new handle starts at the same offset as the original.
pub fn dup(handle: u64) -> KernelResult<u64> {
    let table = OPEN_FILES.lock();
    let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;

    let path = file.path.clone();
    let offset = file.offset;
    let size = file.seen_size;
    let flags = file.flags;
    let is_directory = file.is_directory;
    let object = file.object.clone();
    let ro_volume = file.ro_volume;

    // Need to drop the lock before calling allocate_handle (it
    // acquires the same lock).
    drop(table);

    if is_directory {
        // Preserve the cursor on the duplicate so callers iterating a
        // directory through a dup'd handle see the same position.
        let id = allocate_dir_handle(path, flags)?;
        // set_dir_cursor takes the same lock; safe now that allocate
        // already released it.
        set_dir_cursor(id, offset)?;
        Ok(marked(id, ro_volume))
    } else {
        // A second description of the same file shares its hold: the file
        // stays until the last of them, and every call through them, is done.
        allocate_handle_holding(path, offset, size, flags, object).map(|h| marked(h, ro_volume))
    }
}

/// Open the file a handle holds again: a new open file description of the
/// same file -- the same inode, even if renamed or unlinked since -- at
/// offset 0, with `flags`. What `open("/proc/self/fd/N")` does on Linux,
/// behind the Linux ABI's `/dev/stdin`, `/dev/fd/N` and `/proc/self/fd/N`
/// (`syscall::linux::reopen_own_fd`).
///
/// `flags` may ask for no more access than the handle has: `PermissionDenied`
/// otherwise, since widening it needs the permission check an open by name
/// makes, which the caller then makes (and which needs the name to still be
/// this file). `TRUNCATE` truncates, with write access. A directory reopens
/// only for reading (`IsADirectory`), its listing from the start.
///
/// # Errors
///
/// `InvalidHandle`, `PermissionDenied`, `IsADirectory`, or the truncate's.
pub fn reopen(handle: u64, flags: OpenFlags) -> KernelResult<u64> {
    let (path, size, have, is_directory, object, ro_volume) = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        (
            file.path.clone(),
            file.seen_size,
            file.flags,
            file.is_directory,
            file.object.clone(),
            file.ro_volume,
        )
    };
    let wants = |f: OpenFlags| flags.contains(f) && !have.contains(f);
    if wants(OpenFlags::READ) || wants(OpenFlags::WRITE) {
        return Err(KernelError::PermissionDenied);
    }
    if is_directory {
        if flags.contains(OpenFlags::WRITE) {
            return Err(KernelError::IsADirectory);
        }
        return allocate_dir_handle(path, flags).map(|h| marked(h, ro_volume));
    }
    // A new description of the same file shares its hold, as `dup`'s does.
    let new =
        allocate_handle_holding(path, 0, size, flags, object).map(|h| marked(h, ro_volume))?;
    if flags.contains(OpenFlags::TRUNCATE) && flags.contains(OpenFlags::WRITE) {
        if let Err(e) = ftruncate(new, 0) {
            // The new description is this call's own; nobody else has it.
            let _ = close(new);
            return Err(e);
        }
    }
    Ok(new)
}

/// Duplicate a file handle by sharing the *same* open file description.
///
/// Unlike [`dup`], this does not allocate a new handle id or a fresh
/// independent cursor — it bumps the refcount on the existing open file
/// description and returns the **same** handle id.  Both owners then
/// share one cursor: a read or write through either id advances the
/// offset for both.
///
/// This is the operation `fork()` needs: the child's userspace fd table
/// is copy-on-write cloned and therefore references the same kernel
/// handle ids as the parent, so the kernel must bump the refcount on
/// those exact ids rather than mint new ones (which the child's table
/// would never see).  It also matches POSIX `fork()` / `dup()` semantics
/// where the descriptions — and offsets — are shared.
///
/// # Returns
///
/// - `Ok(handle)` — refcount incremented; the same handle id is returned.
/// - `Err(InvalidHandle)` — no such open file, or the refcount would
///   overflow `u32::MAX`.
pub fn dup_shared(handle: u64) -> KernelResult<u64> {
    let mut table = OPEN_FILES.lock();
    let file = table.get_mut(&handle).ok_or(KernelError::InvalidHandle)?;
    file.refcount = file
        .refcount
        .checked_add(1)
        .ok_or(KernelError::InvalidHandle)?;
    Ok(handle)
}

/// The name an open handle was opened under, **checked to name its file
/// still**: for a call that must act on the file by name (`fexecve`, a
/// directory handle as the base of a `*at` path).
///
/// `NotFound` when it no longer does -- the file renamed or deleted since the
/// open -- rather than the name, which may be another file's by now: a call
/// acting on it would act on that file. A handle with no file held (a
/// directory, or a file on a filesystem without inode numbers) answers its
/// name unchecked, there being nothing to check it against; directories have
/// [`pinned_dir`] for that.
///
/// Until 2026-10-01 this answered the name unchecked for every handle, so
/// the Linux `fchmod`, `fchown`, `ftruncate`, `fallocate` and `futimens` of
/// a file renamed while open acted on whatever had its old name.
///
/// [`handle_name`] is the name for display.
///
/// # Errors
///
/// `InvalidHandle` for a handle that is not open; `NotFound` as above; the
/// filesystem's own, from the check.
pub fn handle_path(handle: u64) -> KernelResult<PathBuf> {
    let (object, path) = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        (file.object.clone(), file.path.clone())
    };
    if let Some(held) = &object {
        match crate::fs::Vfs::file_identity_resolved(&path) {
            Ok(Some(id)) if id == held.id() => {}
            Ok(_) | Err(KernelError::NotFound) => return Err(KernelError::NotFound),
            Err(e) => return Err(e),
        }
    }
    Ok(path)
}

/// The name an open handle was opened under, for display: `/proc/<pid>/fd`,
/// `/proc/<pid>/maps`, `SYS_FS_HANDLE_PATH`, the name a lock is shown under.
///
/// Not checked: a held file may have been renamed or deleted since, and
/// another file may have its old name. To act on the file by name, use
/// [`handle_path`]; better, act through the handle.
///
/// # Errors
///
/// `InvalidHandle` for a handle that is not open.
pub fn handle_name(handle: u64) -> KernelResult<PathBuf> {
    let table = OPEN_FILES.lock();
    let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
    Ok(file.path.clone())
}

/// What an open handle's `flock` locks are keyed on (`Vfs::flock_key`): the
/// name it was opened under, to show, and the identity of the file it holds,
/// whatever its name now. A file held by name has the identity its name has
/// now, as before.
///
/// # Errors
///
/// `InvalidHandle` for a handle that is not open.
pub fn lock_key(handle: u64) -> KernelResult<(PathBuf, Option<crate::fs::vfs::FileId>)> {
    let path = handle_name(handle)?;
    // No identity keys by the name, as for a file on a filesystem without
    // inode numbers.
    let id = file_identity(handle).unwrap_or(None);
    Ok((path, id))
}

/// The file behind an open handle, taken for one call that acts on the file
/// itself rather than on bytes at the cursor: its metadata (`fchmod`,
/// `fchown`, `futimens`, `fstatfs`), or its contents in place (`fallocate`).
///
/// It is the file the handle holds, whatever its name now; or -- a file on a
/// filesystem without inode numbers, or a directory -- the handle's name,
/// the resolved host path captured at open, so no namespace is applied to it
/// a second time. Before a change made by a directory's name, the name is
/// checked to name what was opened still (`pinned_dir`). Holding one keeps a
/// held file, and its mount, for the length of the call (`FileHold`).
///
/// Until 2026-10-01 the Linux layer made these calls by the handle's name,
/// through the path calls: a jailed caller's jail was applied to the name
/// twice, and a file renamed or deleted while open was missed -- or another
/// file, under its old name, was changed instead.
pub struct HandleFile {
    object: Option<Arc<crate::fs::vfs::FileHold>>,
    path: PathBuf,
    /// For a directory, the identity it was opened onto
    /// (`OpenFile::dir_pin`): checked before a change made by its name.
    pin: Option<crate::fs::FileId>,
    /// Opened in a read-only volume (`OpenFile::ro_volume`).
    ro_volume: bool,
    /// Taken by [`writable`](Self::writable): the contents may be changed.
    writable: bool,
}

impl HandleFile {
    /// The file or directory behind `handle`, for a query or a change of
    /// its metadata. No access mode is asked of the handle: who may change a
    /// file's mode, owner or times is a matter of ownership, not of how the
    /// file was opened, as POSIX has it for `fchmod`.
    ///
    /// # Errors
    ///
    /// `InvalidHandle` for a handle that is not open.
    pub fn of(handle: u64) -> KernelResult<Self> {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        Ok(Self {
            object: file.object.clone(),
            path: file.path.clone(),
            pin: if file.is_directory {
                file.dir_pin
            } else {
                None
            },
            ro_volume: file.ro_volume,
            writable: false,
        })
    }

    /// The regular file behind `handle`, opened for writing, for a call that
    /// changes its contents in place (`fallocate`). The bytes such a call
    /// moves are read without the handle's read access, as Linux moves them
    /// under the inode.
    ///
    /// # Errors
    ///
    /// `InvalidHandle` for a handle that is not open; `IsADirectory`;
    /// `PermissionDenied` for a handle not opened for writing.
    pub fn writable(handle: u64) -> KernelResult<Self> {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        if file.is_directory {
            return Err(KernelError::IsADirectory);
        }
        if !file.flags.is_writable() {
            return Err(KernelError::PermissionDenied);
        }
        Ok(Self {
            object: file.object.clone(),
            path: file.path.clone(),
            pin: None,
            ro_volume: file.ro_volume,
            writable: true,
        })
    }

    /// What may refuse a change of metadata: a handle opened in a read-only
    /// volume, and a directory whose name names something else now
    /// (`NotFound`, rather than change that).
    fn check_change(&self) -> KernelResult<()> {
        if self.ro_volume {
            return Err(KernelError::ReadOnlyFilesystem);
        }
        self.check_pin()
    }

    /// A directory, reached by its name: `NotFound` when the name names
    /// something else now, rather than act on that.
    fn check_pin(&self) -> KernelResult<()> {
        if let Some(id) = self.pin
            && crate::fs::Vfs::file_identity_resolved(&self.path)? != Some(id)
        {
            return Err(KernelError::NotFound);
        }
        Ok(())
    }

    /// The file's metadata, whatever its name now: `fstat`'s answer, for a
    /// handle already in hand.
    ///
    /// # Errors
    ///
    /// As `check_pin`; the filesystem's.
    pub fn metadata(&self) -> KernelResult<crate::fs::FileMeta> {
        match &self.object {
            Some(held) => crate::fs::Vfs::object_metadata(held),
            None => {
                self.check_pin()?;
                crate::fs::Vfs::metadata_resolved(&self.path)
            }
        }
    }

    /// The file's identity -- `None` on a filesystem that gives none -- and
    /// the name it was opened under: what a seal is keyed and reported by
    /// (`fs::sealing`).
    ///
    /// # Errors
    ///
    /// As `check_pin`; the filesystem's.
    pub fn identity(&self) -> KernelResult<(Option<crate::fs::vfs::FileId>, PathBuf)> {
        let id = match &self.object {
            Some(held) => Some(held.id()),
            None => {
                self.check_pin()?;
                crate::fs::Vfs::file_identity_resolved(&self.path)?
            }
        };
        Ok((id, self.path.clone()))
    }

    /// `FS_IOC_SETFLAGS`: the file's attributes (`chattr`), as
    /// `Vfs::set_attributes` decides who may change them.
    ///
    /// # Errors
    ///
    /// As `check_change`; `ReadOnlyFilesystem`; `NotPermitted`; the
    /// filesystem's.
    pub fn set_attributes(&self, attrs: crate::fs::FileAttr) -> KernelResult<()> {
        self.check_change()?;
        match &self.object {
            Some(held) => crate::fs::Vfs::object_set_attributes(held, &self.path, attrs),
            None => crate::fs::Vfs::set_attributes_resolved(&self.path, attrs),
        }
    }

    /// `fgetxattr`: the attribute `name` of the file, for the calling task
    /// (`fs::xattr_policy`).
    ///
    /// # Errors
    ///
    /// As `check_pin`; the policy's; the filesystem's (`NoAttribute`).
    pub fn get_xattr(&self, name: &[u8]) -> KernelResult<alloc::vec::Vec<u8>> {
        match &self.object {
            Some(held) => crate::fs::Vfs::object_get_xattr(held, name),
            None => {
                self.check_pin()?;
                let target = crate::fs::Vfs::xattr_target_resolved(
                    &self.path,
                    crate::fs::xattr_policy::Access::Read,
                )?;
                crate::fs::Vfs::xattr_get(&target, name)
            }
        }
    }

    /// Whether the file takes a change through this handle now: Linux's
    /// `mnt_want_write_file`, which `fremovexattr` meets before it reads the
    /// attribute's name.
    ///
    /// # Errors
    ///
    /// As `check_change`; `ReadOnlyFilesystem` for a mount that is.
    pub fn may_change(&self) -> KernelResult<()> {
        self.check_change()?;
        match &self.object {
            Some(held) => crate::fs::Vfs::object_check_writable(held),
            None => crate::fs::Vfs::xattr_target_resolved(
                &self.path,
                crate::fs::xattr_policy::Access::Write,
            )
            .map(|_| ()),
        }
    }

    /// `fsetxattr`.
    ///
    /// # Errors
    ///
    /// As `may_change`; the policy's; the mode's; the filesystem's.
    pub fn set_xattr(
        &self,
        name: &[u8],
        value: &[u8],
        mode: crate::fs::XattrSetMode,
    ) -> KernelResult<()> {
        self.check_change()?;
        match &self.object {
            Some(held) => crate::fs::Vfs::object_set_xattr(held, &self.path, name, value, mode),
            None => {
                let target = crate::fs::Vfs::xattr_target_resolved(
                    &self.path,
                    crate::fs::xattr_policy::Access::Write,
                )?;
                crate::fs::Vfs::xattr_set(&target, name, value, mode)
            }
        }
    }

    /// `fremovexattr`.
    ///
    /// # Errors
    ///
    /// As `may_change`; the policy's; `NoAttribute`.
    pub fn remove_xattr(&self, name: &[u8]) -> KernelResult<()> {
        self.check_change()?;
        match &self.object {
            Some(held) => crate::fs::Vfs::object_remove_xattr(held, &self.path, name),
            None => {
                let target = crate::fs::Vfs::xattr_target_resolved(
                    &self.path,
                    crate::fs::xattr_policy::Access::Write,
                )?;
                crate::fs::Vfs::xattr_remove(&target, name)
            }
        }
    }

    /// `flistxattr`: the names the calling task may see.
    ///
    /// # Errors
    ///
    /// As `check_pin`; the filesystem's.
    pub fn list_xattrs(&self) -> KernelResult<alloc::vec::Vec<alloc::vec::Vec<u8>>> {
        match &self.object {
            Some(held) => crate::fs::Vfs::object_list_xattrs(held),
            None => {
                self.check_pin()?;
                let target = crate::fs::Vfs::xattr_target_resolved(
                    &self.path,
                    crate::fs::xattr_policy::Access::Read,
                )?;
                crate::fs::Vfs::xattr_list(&target)
            }
        }
    }

    /// A change of contents needs a handle taken by
    /// [`writable`](Self::writable).
    fn check_writable(&self) -> KernelResult<()> {
        if self.writable {
            Ok(())
        } else {
            Err(KernelError::PermissionDenied)
        }
    }

    /// `fchmod`.
    ///
    /// # Errors
    ///
    /// As `check_change`; the filesystem's own.
    pub fn set_permissions(&self, permissions: u16) -> KernelResult<()> {
        self.check_change()?;
        match &self.object {
            Some(held) => crate::fs::Vfs::object_set_permissions(held, &self.path, permissions),
            None => crate::fs::Vfs::set_permissions_resolved(&self.path, permissions),
        }
    }

    /// `fchown`. `u32::MAX` leaves an id as it is.
    ///
    /// # Errors
    ///
    /// As `check_change`; the filesystem's own.
    pub fn set_owner(&self, uid: u32, gid: u32) -> KernelResult<()> {
        self.check_change()?;
        match &self.object {
            Some(held) => crate::fs::Vfs::object_set_owner(held, &self.path, uid, gid),
            None => crate::fs::Vfs::set_owner_resolved(&self.path, uid, gid),
        }
    }

    /// `futimens`. A time of 0 is left as it is; `fs::vfs::TIME_NOW` is now.
    ///
    /// # Errors
    ///
    /// As `check_change`; `NotPermitted` for an immutable or append-only
    /// file (`fs::attr_policy`); the filesystem's own.
    pub fn set_times(
        &self,
        accessed_ns: crate::fs::vfs::Timestamp,
        modified_ns: crate::fs::vfs::Timestamp,
    ) -> KernelResult<()> {
        self.check_change()?;
        match &self.object {
            Some(held) => crate::fs::Vfs::object_set_times(held, accessed_ns, modified_ns),
            None => crate::fs::Vfs::set_times_resolved(&self.path, accessed_ns, modified_ns),
        }
    }

    /// `fstatfs`: the filesystem the file is on.
    ///
    /// # Errors
    ///
    /// The filesystem's own.
    pub fn statvfs(&self) -> KernelResult<crate::fs::vfs::FsInfo> {
        match &self.object {
            Some(held) => crate::fs::Vfs::object_statvfs(held),
            None => crate::fs::Vfs::statvfs_resolved(&self.path),
        }
    }

    /// The file's size now.
    ///
    /// # Errors
    ///
    /// The filesystem's own.
    pub fn size(&self) -> KernelResult<u64> {
        current_size(self.object.as_deref(), &self.path)
    }

    /// Up to `len` bytes at `offset`, fewer at the end of the file.
    ///
    /// # Errors
    ///
    /// `PermissionDenied` unless taken by [`writable`](Self::writable); the
    /// filesystem's own.
    pub fn read(&self, offset: u64, len: usize) -> KernelResult<alloc::vec::Vec<u8>> {
        self.check_writable()?;
        match &self.object {
            Some(held) => crate::fs::Vfs::object_read(held, &self.path, offset, len),
            None => crate::fs::Vfs::read_at_resolved(&self.path, offset, len),
        }
    }

    /// Write `data` at `offset`.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read).
    pub fn write(&self, offset: u64, data: &[u8]) -> KernelResult<()> {
        self.check_writable()?;
        match &self.object {
            Some(held) => crate::fs::Vfs::object_write(held, &self.path, offset, data),
            None => crate::fs::Vfs::write_at_resolved(&self.path, offset, data),
        }
    }

    /// Cut or zero-extend the file to `size` bytes.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read).
    pub fn truncate(&self, size: u64) -> KernelResult<()> {
        self.check_writable()?;
        match &self.object {
            Some(held) => crate::fs::Vfs::object_truncate(held, &self.path, size),
            None => crate::fs::Vfs::truncate_resolved(&self.path, size),
        }
    }

    /// Reserve space for the first `size` bytes, the size unchanged:
    /// `fallocate(FALLOC_FL_KEEP_SIZE)`.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read).
    pub fn fallocate(&self, size: u64) -> KernelResult<()> {
        self.check_writable()?;
        match &self.object {
            Some(held) => crate::fs::Vfs::object_fallocate(held, size),
            None => crate::fs::Vfs::fallocate_resolved(&self.path, size),
        }
    }
}

/// Resolve the stable system-wide file identity of an open handle.
///
/// Returns the backing file's [`crate::fs::vfs::FileId`] (mount `fs_id`
/// plus inode number) by querying the VFS for the handle's cached
/// resolved path.  Used at mmap time to key the shared read-only page
/// cache.
///
/// - `Ok(Some(id))` — the backing filesystem exposes a stable inode.
/// - `Ok(None)` — no stable identity (`ino == 0`: FAT, ISO9660, pseudo
///   filesystems); the caller must fall back to the per-mapping read path.
/// - `Err(_)` — the handle is invalid or the path no longer resolves.
pub fn file_identity(handle: u64) -> KernelResult<Option<crate::fs::vfs::FileId>> {
    let (object, path) = {
        let table = OPEN_FILES.lock();
        let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
        (file.object.clone(), file.path.clone())
    };
    // The file held, whatever its name now; else by name.
    match object {
        Some(obj) => Ok(Some(obj.id())),
        None => crate::fs::Vfs::file_identity_resolved(&path),
    }
}

/// Recover a directory handle as a [`PinnedDir`](crate::fs::PinnedDir) — the
/// path it was opened under *together with* the identity it opened onto.
///
/// This is what every fd-relative operation should take instead of
/// [`handle_path`].  The difference is the whole of the fix: `handle_path`
/// hands back a name, and a name is a question the filesystem re-answers
/// every time it is asked, so an `unlinkat` built on it deletes whatever
/// answers *now*.  A `PinnedDir` carries what the answer was at open time, so
/// [`crate::fs::Vfs::unlink_at_pinned`] can refuse when it has changed.
///
/// Fails with [`KernelError::NotADirectory`] for a handle that is not a
/// directory: nothing resolves relative to a file, and returning a
/// never-verifiable `PinnedDir` for one would quietly hand the caller the
/// weaker guarantee under the stronger name.
pub fn pinned_dir(handle: u64) -> KernelResult<crate::fs::PinnedDir> {
    let table = OPEN_FILES.lock();
    let file = table.get(&handle).ok_or(KernelError::InvalidHandle)?;
    if !file.is_directory {
        return Err(KernelError::NotADirectory);
    }
    Ok(crate::fs::PinnedDir {
        path: file.path.clone(),
        id: file.dir_pin,
    })
}

/// Get the current number of open file handles (for diagnostics).
pub fn open_count() -> usize {
    OPEN_FILES.lock().len()
}

/// Snapshot of an open file handle's state (for /proc/fdinfo).
pub struct HandleInfo {
    /// Handle ID.
    pub id: u64,
    /// VFS path.
    pub path: PathBuf,
    /// Current offset.
    pub offset: u64,
    /// The file's size as the handle last saw it (see `OpenFile::seen_size`):
    /// a display value, not the file's size now.
    pub size: u64,
    /// Open flags (raw bits).
    pub flags: u32,
}

/// Enumerate all open file handles for diagnostics.
///
/// Returns a snapshot of every open handle.  Used by `/proc/fdinfo`.
pub fn list_handles() -> alloc::vec::Vec<HandleInfo> {
    let table = OPEN_FILES.lock();
    let mut result = alloc::vec::Vec::with_capacity(table.len());
    for (&id, file) in table.iter() {
        result.push(HandleInfo {
            id,
            path: file.path.clone(),
            offset: file.offset,
            size: file.seen_size,
            flags: file.flags.bits(),
        });
    }
    result
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Allocate a handle in the global table.
fn allocate_handle(path: PathBuf, offset: u64, size: u64, flags: OpenFlags) -> KernelResult<u64> {
    // The file itself, held before the handle exists, so no I/O through the
    // handle can happen by name. Taken without the table lock: it locks the
    // mount table and the filesystem.
    let object = crate::fs::Vfs::open_object(&path)?.map(Arc::new);
    allocate_handle_holding(path, offset, size, flags, object)
}

/// [`allocate_handle`] with the hold already taken (or none). A failure here
/// gives this reference to the hold back.
fn allocate_handle_holding(
    path: PathBuf,
    offset: u64,
    size: u64,
    flags: OpenFlags,
    object: Option<Arc<crate::fs::vfs::FileHold>>,
) -> KernelResult<u64> {
    let mut table = OPEN_FILES.lock();

    if table.len() >= MAX_OPEN_FILES {
        // The table lock first: dropping the last reference to a hold gives
        // it back, which must not happen under it (`FileHold`).
        drop(table);
        drop(object);
        return Err(KernelError::OutOfMemory);
    }

    let id = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);

    table.insert(
        id,
        OpenFile {
            path,
            offset,
            seen_size: size,
            flags,
            refcount: 1,
            is_directory: false,
            dir_pin: None,
            object,
            ro_volume: false,
        },
    );

    Ok(id)
}

/// Allocate a directory handle in the global table.
///
/// Directory handles use `offset` as an entry cursor and `size = 0`
/// (the underlying VFS doesn't track a stable entry count cheaply, so
/// we don't cache one — `read_dir_at` queries the VFS each time).
fn allocate_dir_handle(path: PathBuf, flags: OpenFlags) -> KernelResult<u64> {
    // Pin the directory's identity *before* taking the table lock: capturing
    // it needs the filesystem lock, and taking the two in this order here
    // while `close` takes only the table lock keeps the pair one-directional.
    //
    // Doing it at open rather than on first use is the point of the exercise.
    // An identity read later could only tell us what the name means *then*,
    // which is exactly the question that has no useful answer; read now, it
    // records what the caller actually opened, and every later operation has
    // something to be checked against.
    //
    // A failure to read it is not a failure to open: filesystems with no
    // inode numbers answer 0 here, and are recorded as unpinnable rather than
    // refused, so opening a directory on a FAT stick still works.
    let dir_pin = crate::fs::Vfs::pin_dir(&path).ok().and_then(|p| p.id);

    let mut table = OPEN_FILES.lock();

    if table.len() >= MAX_OPEN_FILES {
        return Err(KernelError::OutOfMemory);
    }

    let id = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);

    table.insert(
        id,
        OpenFile {
            path,
            offset: 0,
            seen_size: 0,
            flags,
            refcount: 1,
            is_directory: true,
            dir_pin,
            // Directories go by name, and check their pin.
            object: None,
            ro_volume: false,
        },
    );

    Ok(id)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// A handle holds its file, not its name, where the filesystem allows
/// (known-issues A-AN-OPEN-FILE-FOLLOWS-ITS-NAME). The rungs:
///
/// 1. The name unlinked while open: the handle still writes and reads the
///    file, `fstat` says 0 links, and the name stays gone. On memfs the
///    write used to re-create the file under its old name, zero-filled.
/// 2. The name renamed while open: the handle writes the renamed file.
/// 3. Another writer's growth is seen: a reader opened earlier reads the
///    bytes appended after its open, and `SEEK_END` lands past them.
/// 4. `O_APPEND` through two opens lands each write at the true end.
/// 5. `ftruncate` leaves the offset where it was, as POSIX says.
/// 6. A mount with a file held open on it cannot be unmounted.
///
/// On a root that holds files by name (FAT) the rungs do not apply, and are
/// reported as skipped rather than passed.
fn test_held_files(skips: &mut crate::fs::selftest::Skips) -> KernelResult<()> {
    use crate::fs::Vfs;
    const A: &str = "/held_a.txt";
    const B: &str = "/held_b.txt";
    const MNT: &str = "/tmp/held_mnt";

    fn fail(what: &str, got: &dyn core::fmt::Debug) -> KernelResult<()> {
        crate::serial_println!("[fs::handle]   FAIL: held files: {}: {:?}", what, got);
        Err(KernelError::InternalError)
    }
    let rw = OpenFlags::READ.union(OpenFlags::WRITE);
    // Best effort: leftovers from an earlier boot's failure.
    let _ = Vfs::remove(A);
    let _ = Vfs::remove(B);
    Vfs::write_file(A, b"hello")?;
    let h = open(A, rw)?;
    let held = OPEN_FILES
        .lock()
        .get(&h)
        .is_some_and(|f| f.object.is_some());
    if !held {
        let _ = close(h);
        let _ = Vfs::remove(A);
        skips.record("held files", "the root filesystem holds open files by name");
        return Ok(());
    }

    // 1. Unlinked while open.
    let mut buf = [0u8; 32];
    Vfs::remove(A)?;
    let wrote = write_at(h, 5, b" world");
    let read_back = read_at(h, 0, &mut buf);
    let meta = fstat(h);
    let resurrected = Vfs::stat(A).is_ok();
    let _ = close(h);
    let after_close = Vfs::stat(A).is_ok();
    match (wrote, read_back, meta) {
        (Ok(6), Ok(11), Ok(m)) if buf.get(..11) == Some(b"hello world") && m.nlinks == 0 => {}
        other => return fail("a file unlinked while open", &other),
    }
    if resurrected || after_close {
        return fail("the unlinked name came back", &(resurrected, after_close));
    }
    crate::serial_println!("[fs::handle]   held: unlinked while open, still the handle's: OK");

    // 2. Renamed while open.
    Vfs::write_file(A, b"abc")?;
    let h = open(A, rw)?;
    Vfs::rename(A, B)?;
    let wrote = write_at(h, 3, b"def");
    let _ = close(h);
    let at_b = Vfs::read_file(B);
    let at_a = Vfs::stat(A).is_ok();
    let _ = Vfs::remove(B);
    match (wrote, at_b) {
        (Ok(3), Ok(data)) if data == b"abcdef" && !at_a => {}
        other => return fail("a file renamed while open", &(other, at_a)),
    }
    crate::serial_println!(
        "[fs::handle]   held: renamed while open, the write follows the file: OK"
    );

    // 3. Another writer's growth, and SEEK_END.
    Vfs::write_file(A, b"0123")?;
    let reader = open(A, OpenFlags::READ)?;
    let writer = open(A, rw)?;
    let grew = write_at(writer, 4, b"4567");
    let n = read(reader, &mut buf);
    let end = seek(reader, SeekFrom::End(0));
    let _ = close(reader);
    let _ = close(writer);
    match (grew, n, end) {
        (Ok(4), Ok(8), Ok(8)) if buf.get(..8) == Some(b"01234567") => {}
        other => return fail("another writer's growth", &other),
    }
    crate::serial_println!("[fs::handle]   held: a reader sees another writer's growth: OK");

    // 4. O_APPEND through two opens.
    Vfs::write_file(A, b"")?;
    let appending = rw.union(OpenFlags::APPEND);
    let (h1, h2) = (open(A, appending)?, open(A, appending)?);
    let writes = [write(h1, b"a"), write(h2, b"b"), write(h1, b"c")];
    let _ = close(h1);
    let _ = close(h2);
    let content = Vfs::read_file(A);
    if writes.iter().any(|w| *w != Ok(1)) || content.as_deref() != Ok(b"abc".as_slice()) {
        return fail("O_APPEND through two opens", &(writes, content));
    }
    crate::serial_println!("[fs::handle]   held: O_APPEND lands at the true end: OK");

    // 5. ftruncate leaves the offset alone.
    let h = open(A, rw)?;
    let _ = seek(h, SeekFrom::Start(3));
    let cut = ftruncate(h, 1);
    let at = seek(h, SeekFrom::Current(0));
    let _ = close(h);
    let _ = Vfs::remove(A);
    if cut.is_err() || at != Ok(3) {
        return fail("ftruncate moved the offset", &(cut, at));
    }
    crate::serial_println!("[fs::handle]   held: ftruncate keeps the offset: OK");

    // 6. A held file keeps its mount.
    if Vfs::stat("/tmp").is_ok() {
        crate::fs::memfs::mount(MNT)?;
        let inner = "/tmp/held_mnt/f";
        Vfs::write_file(inner, b"x")?;
        let h = open(inner, OpenFlags::READ)?;
        let busy = Vfs::unmount(MNT);
        let _ = close(h);
        let freed = Vfs::unmount(MNT);
        if busy != Err(KernelError::DeviceBusy) || freed.is_err() {
            // Best effort, so a failure does not leave the scratch mount.
            let _ = Vfs::unmount(MNT);
            return fail("unmount with a file held open", &(busy, freed));
        }
        crate::serial_println!("[fs::handle]   held: a mount with a file open is busy: OK");
    } else {
        skips.record(
            "held files: busy mount",
            "no /tmp to mount a scratch filesystem under",
        );
    }

    // 7. A call in progress keeps its file, and the file's mount, through a
    // close that races it: the reference the call took is the last hold.
    if Vfs::stat("/tmp").is_ok() {
        crate::fs::memfs::mount(MNT)?;
        let inner = "/tmp/held_mnt/g";
        Vfs::write_file(inner, b"kept")?;
        let h = open(inner, OpenFlags::READ)?;
        Vfs::remove(inner)?;
        // What a read takes as it starts, before the close on "another
        // thread".
        let in_flight = OPEN_FILES.lock().get(&h).and_then(|f| f.object.clone());
        let _ = close(h);
        let busy = Vfs::unmount(MNT);
        let read_back = in_flight
            .as_deref()
            .map(|held| Vfs::object_read(held, Path::new(inner), 0, 16));
        drop(in_flight);
        let freed = Vfs::unmount(MNT);
        match (busy, read_back, freed) {
            (Err(KernelError::DeviceBusy), Some(Ok(data)), Ok(())) if data == b"kept" => {}
            other => {
                // Best effort, so a failure does not leave the scratch mount.
                let _ = Vfs::unmount(MNT);
                return fail("a close racing a call in progress", &other);
            }
        }
        crate::serial_println!(
            "[fs::handle]   held: a call in progress outlives a racing close: OK"
        );
    } else {
        skips.record(
            "held files: a racing close",
            "no /tmp to mount a scratch filesystem under",
        );
    }

    // 8. A name the file no longer has is not handed out as its own:
    // `handle_path` refuses it, `handle_name` still shows it.
    Vfs::write_file(A, b"one")?;
    let h = open(A, OpenFlags::READ)?;
    Vfs::rename(A, B)?;
    Vfs::write_file(A, b"two")?;
    let checked = handle_path(h);
    let shown = handle_name(h);
    let _ = close(h);
    let _ = Vfs::remove(A);
    let _ = Vfs::remove(B);
    match (checked, shown) {
        (Err(KernelError::NotFound), Ok(name)) if name.as_path() == Path::new(A) => {}
        other => return fail("the name of a file renamed while open", &other),
    }
    crate::serial_println!(
        "[fs::handle]   held: a name another file has now is not the handle's: OK"
    );

    // 9. `flock` keys on the file: a lock through a handle on a file since
    // renamed meets one through a handle opened under the new name, and not
    // one on the new file under the old name.
    Vfs::write_file(A, b"")?;
    let first = open(A, OpenFlags::READ)?;
    Vfs::rename(A, B)?;
    Vfs::write_file(A, b"")?;
    let second = open(B, OpenFlags::READ)?;
    let stranger = open(A, OpenFlags::READ)?;
    let take = |h: u64| -> KernelResult<()> {
        let (name, id) = lock_key(h)?;
        Vfs::flock_key(
            &name,
            id,
            crate::fs::vfs::flock_description_owner(h),
            crate::fs::LockType::Exclusive,
        )
    };
    let taken = take(first);
    let refused = take(second);
    let other_file = take(stranger);
    // The closes release the locks, keyed as they were taken.
    let _ = close(stranger);
    let _ = close(second);
    let _ = close(first);
    let _ = Vfs::remove(A);
    let _ = Vfs::remove(B);
    match (taken, refused, other_file) {
        (Ok(()), Err(KernelError::WouldBlock), Ok(())) => {}
        other => return fail("flock on a file renamed while open", &other),
    }
    crate::serial_println!("[fs::handle]   held: flock follows the file, not its name: OK");

    // 10. A change made through a handle reaches the file it holds, whatever
    // its name now (`HandleFile`): a mode, an owner and times through a
    // handle on a file since renamed, a new file under its old name left
    // alone; then its contents, once it has no name at all.
    Vfs::write_file(A, b"0123456789")?;
    let h = open(A, rw)?;
    Vfs::rename(A, B)?;
    Vfs::write_file(A, b"")?;
    let stranger_mode = Vfs::metadata(A).map(|m| m.permissions & 0o7777);
    let changes = HandleFile::of(h).map(|file| {
        (
            file.set_permissions(0o600),
            file.set_owner(1234, 5678),
            file.set_times(1_000_000_000_000_000_000, 2_000_000_000_000_000_000),
        )
    });
    let renamed = Vfs::metadata(B);
    let left_alone = Vfs::metadata(A).map(|m| m.permissions & 0o7777);
    Vfs::remove(B)?;
    let body = HandleFile::writable(h).and_then(|f| {
        f.truncate(4)?;
        f.write(4, b"!")?;
        f.fallocate(64)?;
        Ok((f.size()?, f.read(0, 16)?))
    });
    let unnamed = fstat(h);
    let _ = close(h);
    let _ = Vfs::remove(A);
    match (&changes, &renamed, &body, &unnamed) {
        (Ok((Ok(()), Ok(()), Ok(()))), Ok(m), Ok((5, data)), Ok(u))
            if m.permissions & 0o7777 == 0o600
                && (m.uid, m.gid) == (1234, 5678)
                && m.modified_ns / 1_000_000_000 == 2_000_000_000
                && data.as_slice() == b"0123!"
                && u.nlinks == 0
                && left_alone == stranger_mode => {}
        _ => {
            return fail(
                "changes through a handle on a renamed, then unlinked, file",
                &(
                    &changes,
                    &renamed,
                    &body,
                    &unnamed,
                    &stranger_mode,
                    &left_alone,
                ),
            );
        }
    }
    crate::serial_println!(
        "[fs::handle]   held: fchmod/fchown/futimens/ftruncate through the handle reach its file: OK"
    );

    // 11. A directory is changed by its name only while the name names it;
    // its filesystem is asked by the name either way.
    const D: &str = "/held_dir";
    const D2: &str = "/held_dir2";
    // Best effort: leftovers from an earlier boot's failure.
    let _ = Vfs::rmdir(D);
    let _ = Vfs::rmdir(D2);
    Vfs::mkdir(D)?;
    let hd = open(D, OpenFlags::READ.union(OpenFlags::DIRECTORY))?;
    Vfs::rename(D, D2)?;
    Vfs::mkdir(D)?;
    let stale = HandleFile::of(hd).and_then(|f| f.set_permissions(0o700));
    let asked = HandleFile::of(hd).and_then(|f| f.statvfs()).map(|_| ());
    let new_mode = Vfs::metadata(D).map(|m| m.permissions & 0o7777);
    let _ = close(hd);
    let _ = Vfs::rmdir(D);
    let _ = Vfs::rmdir(D2);
    match (stale, asked, new_mode) {
        (Err(KernelError::NotFound), Ok(()), Ok(0o755)) => {}
        other => return fail("a change by a renamed directory's name", &other),
    }
    crate::serial_println!(
        "[fs::handle]   held: a renamed directory's old name is not changed: OK"
    );

    // 12. `DIRECTORY_ALLOWED` opens whatever the path names, for reading.
    Vfs::write_file(A, b"x")?;
    let either = OpenFlags::READ.union(OpenFlags::DIRECTORY_ALLOWED);
    let as_dir = open("/", either);
    let as_file = open(A, either);
    let as_writer = open("/", either.union(OpenFlags::WRITE));
    let kinds = (
        as_dir.as_ref().map(|h| is_directory(*h)).ok(),
        as_file.as_ref().map(|h| is_directory(*h)).ok(),
        as_writer.as_ref().err().copied(),
    );
    for h in [as_dir, as_file, as_writer].into_iter().flatten() {
        // Best effort: this test's own handles.
        let _ = close(h);
    }
    let _ = Vfs::remove(A);
    if kinds != (Some(true), Some(false), Some(KernelError::IsADirectory)) {
        return fail("DIRECTORY_ALLOWED", &kinds);
    }
    crate::serial_println!(
        "[fs::handle]   DIRECTORY_ALLOWED: a directory or a file, for reading: OK"
    );

    // 13. A jailed process: a create's mode and a mkdir's land on the jailed
    // path (they applied the jail twice until 2026-10-01, and failed after
    // creating), and a change through a handle opened in a read-only volume
    // is refused.
    if Vfs::stat("/tmp").is_ok() {
        const JAIL: &str = "/tmp/held_jail";
        const RO: &str = "/tmp/held_jail_ro";
        // Best effort: leftovers from an earlier boot's failure.
        let _ = Vfs::remove_recursive(JAIL);
        let _ = Vfs::remove_recursive(RO);
        Vfs::mkdir(JAIL)?;
        Vfs::mkdir(RO)?;
        Vfs::write_file("/tmp/held_jail_ro/f", b"r")?;
        let pid = crate::proc::pcb::create("held-jail", 0);
        let outcome = crate::ipc::namespace::set_root(pid, JAIL)
            .and_then(|()| crate::ipc::namespace::add_volume(pid, "/ro", RO, true))
            .map(|()| {
                crate::proc::thread::self_test_as_process(pid, || {
                    let made =
                        open_with_mode("/made", OpenFlags::WRITE.union(OpenFlags::CREATE), 0o600)
                            .map(|h| {
                                // Best effort: this test's own handle.
                                let _ = close(h);
                            });
                    let dir = Vfs::mkdir_mode("/dir", 0o700);
                    let ro_change = open("/ro/f", OpenFlags::READ).and_then(|h| {
                        let changed = HandleFile::of(h).and_then(|f| f.set_permissions(0o600));
                        // Best effort: this test's own handle.
                        let _ = close(h);
                        changed
                    });
                    (made, dir, ro_change)
                })
            });
        // Ends the jail and the volume with the process.
        crate::proc::pcb::destroy(pid);
        let modes = (
            Vfs::metadata("/tmp/held_jail/made").map(|m| m.permissions & 0o7777),
            Vfs::metadata("/tmp/held_jail/dir").map(|m| m.permissions & 0o7777),
            Vfs::metadata("/tmp/held_jail_ro/f").map(|m| m.permissions & 0o7777),
        );
        let _ = Vfs::remove_recursive(JAIL);
        let _ = Vfs::remove_recursive(RO);
        match (outcome, modes) {
            (
                Ok((Ok(()), Ok(()), Err(KernelError::ReadOnlyFilesystem))),
                (Ok(0o600), Ok(0o700), Ok(0o644)),
            ) => {}
            other => {
                return fail(
                    "a jailed process's creates and its read-only volume",
                    &other,
                );
            }
        }
        crate::serial_println!(
            "[fs::handle]   jailed: a create's mode lands; a read-only volume's file keeps its own: OK"
        );
    } else {
        skips.record(
            "held files: a jailed process",
            "no /tmp to build a jail under",
        );
    }

    // 14. A file made with no name (`open_tmpfile`): read and written through
    // its handle, in no directory, shown as `dir/#ino`; named by
    // `link_handle` it stays, and may take a second name; one made with
    // `EXCL` may not be named; gone with the last close, its mount busy
    // until then.
    if Vfs::stat("/tmp").is_ok() {
        crate::fs::memfs::mount(MNT)?;
        let inodes = || Vfs::statvfs(MNT).map(|i| i.total_inodes);
        let before = inodes();
        let h = open_tmpfile(MNT, rw, 0o640)?;
        let wrote = write(h, b"unnamed");
        let read_back = read_at(h, 0, &mut buf);
        let listed = Vfs::readdir(MNT).map(|e| e.len());
        let shown = handle_name(h);
        let checked = handle_path(h);
        let meta = fstat(h).map(|m| (m.nlinks, m.permissions & 0o7777));
        let while_held = inodes();
        let busy = Vfs::unmount(MNT);
        let named = link_handle(h, "/tmp/held_mnt/named");
        let second = link_handle(h, "/tmp/held_mnt/second");
        let links = fstat(h).map(|m| m.nlinks);
        let _ = close(h);
        let kept = Vfs::read_file("/tmp/held_mnt/named");
        let _ = Vfs::remove("/tmp/held_mnt/named");
        let _ = Vfs::remove("/tmp/held_mnt/second");
        let h = open_tmpfile(MNT, rw.union(OpenFlags::EXCL), 0o600)?;
        let refused = link_handle(h, "/tmp/held_mnt/never");
        let _ = close(h);
        let after = inodes();
        let read_only = open_tmpfile(MNT, OpenFlags::READ, 0o600);
        Vfs::write_file("/tmp/held_mnt/plain", b"")?;
        let not_dir = open_tmpfile("/tmp/held_mnt/plain", rw, 0o600);
        let _ = Vfs::remove("/tmp/held_mnt/plain");
        let freed = Vfs::unmount(MNT);
        let shown_ok = shown
            .as_ref()
            .is_ok_and(|n| n.as_bytes().starts_with(b"/tmp/held_mnt/#"));
        let ok = wrote == Ok(7)
            && read_back == Ok(7)
            && buf.get(..7) == Some(b"unnamed".as_slice())
            && listed == Ok(0)
            && shown_ok
            && checked == Err(KernelError::NotFound)
            && meta == Ok((0, 0o640))
            && while_held.is_ok_and(|n| before == Ok(n.saturating_sub(1)))
            && busy == Err(KernelError::DeviceBusy)
            && named == Ok(())
            && second == Ok(())
            && links == Ok(2)
            && kept.as_deref() == Ok(b"unnamed".as_slice())
            && refused == Err(KernelError::NotFound)
            && after == before
            && read_only == Err(KernelError::InvalidArgument)
            && not_dir == Err(KernelError::NotADirectory)
            && freed == Ok(());
        if !ok {
            // Best effort, so a failure does not leave the scratch mount.
            let _ = Vfs::unmount(MNT);
            return fail(
                "a file made with no name",
                &(
                    (wrote, read_back, listed, shown, checked, meta),
                    (before, while_held, busy, named, second, links),
                    (kept, refused, after, read_only, not_dir, freed),
                ),
            );
        }
        crate::serial_println!(
            "[fs::handle]   unnamed (O_TMPFILE): in no directory, named on request, gone at its close: OK"
        );
    } else {
        skips.record(
            "held files: a file made with no name",
            "no /tmp to mount a scratch filesystem under",
        );
    }
    Ok(())
}

/// Test the file handle system end-to-end.
///
/// Requires a mounted filesystem (skips gracefully if none available).
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[fs::handle] Running self-test...");

    // Sections that could not run, reported next to the closing line rather
    // than only where they happen — see `selftest::Skips`.
    let mut skips = crate::fs::selftest::Skips::new();

    let test_path = "/handle_test.txt";
    let test_data = b"Hello from handle test!";

    // A suite may decline to run when its environment lacks a precondition,
    // but the precondition has to be something it looked up.  This was
    // `if write_file(..).is_err() { SKIPPED; return Ok(()) }`, which reads as
    // "no filesystem is mounted" and means "the write failed for any reason
    // whatsoever" — a permission gate refusing it, a full disk, a bug in the
    // write path.  Every one of those silently deleted all twenty-one
    // sections below and returned success.
    if crate::fs::Vfs::mounts().is_empty() {
        crate::serial_println!("[fs::handle] Self-test SKIPPED (no filesystem mounted)");
        return Ok(());
    }
    if let Err(e) = crate::fs::Vfs::write_file(test_path, test_data) {
        crate::serial_println!(
            "[fs::handle]   FAIL: a filesystem is mounted but writing {} failed: {:?}",
            test_path,
            e
        );
        return Err(KernelError::InternalError);
    }

    // 1. Open for reading.
    let h = open(test_path, OpenFlags::READ)?;
    crate::serial_println!("[fs::handle]   open(READ) → handle {}", h);

    // 2. Read and verify.
    let mut buf = [0u8; 64];
    let n = read(h, &mut buf)?;
    if n != test_data.len() {
        crate::serial_println!(
            "[fs::handle]   FAIL: read returned {}, expected {}",
            n,
            test_data.len()
        );
        close(h).ok();
        return Err(KernelError::InternalError);
    }
    if buf.get(..n) != Some(test_data.as_slice()) {
        crate::serial_println!("[fs::handle]   FAIL: read data mismatch");
        close(h).ok();
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   read {} bytes: OK", n);

    // 3. Read again — should be at EOF, return 0.
    let n2 = read(h, &mut buf)?;
    if n2 != 0 {
        crate::serial_println!("[fs::handle]   FAIL: expected 0 at EOF, got {}", n2);
        close(h).ok();
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   read at EOF: 0 bytes (correct)");

    // 4. Seek back to start.
    let pos = seek(h, SeekFrom::Start(0))?;
    if pos != 0 {
        crate::serial_println!("[fs::handle]   FAIL: seek Start(0) returned {}", pos);
        close(h).ok();
        return Err(KernelError::InternalError);
    }

    // 5. Seek forward from current.
    let pos2 = seek(h, SeekFrom::Current(5))?;
    if pos2 != 5 {
        crate::serial_println!("[fs::handle]   FAIL: seek Current(5) returned {}", pos2);
        close(h).ok();
        return Err(KernelError::InternalError);
    }

    // 6. Read from offset 5.
    let n3 = read(h, &mut buf)?;
    if let Some(expected) = test_data.get(5..) {
        if n3 != expected.len() || buf.get(..n3) != Some(expected) {
            crate::serial_println!("[fs::handle]   FAIL: read from offset 5 mismatch");
            close(h).ok();
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[fs::handle]   seek + read from offset 5: OK");

    // 7. Close.
    close(h)?;
    crate::serial_println!("[fs::handle]   close: OK");

    // 8. Verify closed handle is rejected.
    let result = read(h, &mut buf);
    if result != Err(KernelError::InvalidHandle) {
        crate::serial_println!(
            "[fs::handle]   FAIL: read on closed handle should return InvalidHandle"
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   read after close: InvalidHandle (correct)");

    // 9. Test write via handle.
    let hw = open(
        "/handle_write_test.txt",
        OpenFlags::WRITE
            .union(OpenFlags::CREATE)
            .union(OpenFlags::READ),
    )?;
    let write_data = b"Written via handle!";
    let nw = write(hw, write_data)?;
    if nw != write_data.len() {
        crate::serial_println!("[fs::handle]   FAIL: write returned {}", nw);
        close(hw).ok();
        return Err(KernelError::InternalError);
    }

    // Seek back and verify.
    seek(hw, SeekFrom::Start(0))?;
    let nr = read(hw, &mut buf)?;
    if nr != write_data.len() || buf.get(..nr) != Some(write_data.as_slice()) {
        crate::serial_println!("[fs::handle]   FAIL: write+read verification failed");
        close(hw).ok();
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   write + read-back: OK");

    // 10. fstat.
    let stat_result = fstat(hw)?;
    if stat_result.size != write_data.len() as u64
        || stat_result.entry_type != crate::fs::EntryType::File
    {
        crate::serial_println!(
            "[fs::handle]   FAIL: fstat size={} type={:?}, expected size={} type=File",
            stat_result.size,
            stat_result.entry_type,
            write_data.len()
        );
        close(hw).ok();
        return Err(KernelError::InternalError);
    }
    crate::serial_println!(
        "[fs::handle]   fstat: OK (size={}, type=file, nlinks={})",
        stat_result.size,
        stat_result.nlinks
    );

    // 11. ftruncate.
    let trunc_size = 7u64;
    ftruncate(hw, trunc_size)?;
    let trunc_stat = fstat(hw)?;
    if trunc_stat.size != trunc_size {
        crate::serial_println!(
            "[fs::handle]   FAIL: ftruncate to {} but fstat shows {}",
            trunc_size,
            trunc_stat.size
        );
        close(hw).ok();
        return Err(KernelError::InternalError);
    }
    // Read back the truncated content.
    seek(hw, SeekFrom::Start(0))?;
    let nt = read(hw, &mut buf)?;
    if nt != trunc_size as usize {
        crate::serial_println!(
            "[fs::handle]   FAIL: read after truncate returned {} bytes",
            nt
        );
        close(hw).ok();
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   ftruncate to {} bytes: OK", trunc_size);

    // 12. dup — duplicate handle, verify independent cursor.
    seek(hw, SeekFrom::Start(0))?;
    let hdup = dup(hw)?;
    // Read 3 bytes from original — advances original cursor.
    let n_orig = read(hw, &mut buf[..3])?;
    if n_orig != 3 {
        crate::serial_println!("[fs::handle]   FAIL: read 3 from original got {}", n_orig);
        close(hdup).ok();
        close(hw).ok();
        return Err(KernelError::InternalError);
    }
    // Dup'd handle was at offset 0 when dup'd — read should start there.
    let n_dup = read(hdup, &mut buf[..3])?;
    if n_dup != 3 {
        crate::serial_println!("[fs::handle]   FAIL: read 3 from dup got {}", n_dup);
        close(hdup).ok();
        close(hw).ok();
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   dup: independent cursor OK");

    // 13. handle_path.
    let path_check = handle_path(hw)?;
    if path_check.as_path() != Path::new("/handle_write_test.txt") {
        crate::serial_println!(
            "[fs::handle]   FAIL: handle_path = '{}'",
            path_check.display()
        );
        close(hdup).ok();
        close(hw).ok();
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   handle_path: '{}' OK", path_check.display());

    close(hdup)?;
    close(hw)?;

    // 13b. dup_shared — fork-style shared open file description.
    //
    // Both ids share one cursor (a read through either advances both)
    // and refcounted close keeps the description alive until the last
    // owner closes it.
    let hs = open("/handle_write_test.txt", OpenFlags::READ)?;
    seek(hs, SeekFrom::Start(0))?;
    let hs2 = dup_shared(hs)?;
    // dup_shared returns the SAME id (shared description).
    if hs2 != hs {
        crate::serial_println!(
            "[fs::handle]   FAIL: dup_shared returned new id {} (expected same {})",
            hs2,
            hs
        );
        close(hs).ok();
        return Err(KernelError::InternalError);
    }
    // Read 3 bytes through hs — advances the shared cursor.
    let s_a = read(hs, &mut buf[..3])?;
    // Read 3 bytes through hs2 — continues from the SAME (advanced)
    // offset, since the description is shared.  If cursors were
    // independent this would re-read the first 3 bytes.
    let off_after = seek(hs2, SeekFrom::Current(0))?;
    if s_a != 3 || off_after != 3 {
        crate::serial_println!(
            "[fs::handle]   FAIL: shared cursor: read {} bytes, offset now {} (expected 3/3)",
            s_a,
            off_after
        );
        close(hs).ok();
        return Err(KernelError::InternalError);
    }
    // First close drops one reference; the description must survive.
    close(hs2)?;
    let still_open = read(hs, &mut buf[..1]);
    if still_open.is_err() {
        crate::serial_println!(
            "[fs::handle]   FAIL: description freed after first of two closes: {:?}",
            still_open
        );
        close(hs).ok();
        return Err(KernelError::InternalError);
    }
    // Second close drops the last reference; now it must be gone.
    close(hs)?;
    if read(hs, &mut buf[..1]) != Err(KernelError::InvalidHandle) {
        crate::serial_println!("[fs::handle]   FAIL: description not freed after final close");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[fs::handle]   dup_shared: shared cursor + refcounted close OK");

    // 14. Lock-on-close: verify advisory locks are released when handle closes.
    let lock_path = "/handle_lock_test.txt";
    crate::fs::Vfs::write_file(lock_path, b"lock test data")?;

    let hlock = open(lock_path, OpenFlags::READ.union(OpenFlags::WRITE))?;

    // Acquire an exclusive lock using the handle ID as owner.
    crate::fs::Vfs::flock(lock_path, hlock, crate::fs::LockType::Exclusive)?;

    // Verify lock is held.
    match crate::fs::Vfs::lock_query(lock_path) {
        Ok(Some(_)) => {} // Lock is held — expected.
        other => {
            crate::serial_println!(
                "[fs::handle]   FAIL: lock not held after flock: {:?}",
                other
            );
            close(hlock).ok();
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[fs::handle]   flock(exclusive) on handle {}: OK", hlock);

    // Close the handle — this should auto-release the lock.
    close(hlock)?;

    // Verify lock was released.
    match crate::fs::Vfs::lock_query(lock_path) {
        Ok(None) => {} // Lock released — expected.
        other => {
            crate::serial_println!(
                "[fs::handle]   FAIL: lock still held after close: {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[fs::handle]   lock-on-close: auto-released OK");

    // 15. Directory-handle self-test.
    //
    // Create a small directory, populate it with two files, then exercise
    // open(DIRECTORY) / read_dir_at / set_dir_cursor and the rejection
    // paths (open without DIRECTORY → IsADirectory; open(file, DIRECTORY)
    // → NotADirectory; read/write on dir handle → IsADirectory).
    let dir_path = "/handle_dir_test";
    let file_a = "/handle_dir_test/a.txt";
    let file_b = "/handle_dir_test/b.txt";
    let file_outside = "/handle_outside.txt";

    // Cleanup any leftovers from a prior run.
    crate::fs::Vfs::remove(file_a).ok();
    crate::fs::Vfs::remove(file_b).ok();
    crate::fs::Vfs::remove(dir_path).ok();
    crate::fs::Vfs::remove(file_outside).ok();

    let dir_ready = crate::selftest_setup!(
        skips,
        "[fs::handle]",
        "directory handle ops",
        "no directory support",
        crate::fs::Vfs::mkdir(dir_path),
    );
    if dir_ready {
        crate::fs::Vfs::write_file(file_a, b"a")?;
        crate::fs::Vfs::write_file(file_b, b"b")?;
        crate::fs::Vfs::write_file(file_outside, b"o")?;

        // (a) open(dir, READ) without DIRECTORY → IsADirectory.
        match open(dir_path, OpenFlags::READ) {
            Err(KernelError::IsADirectory) => {}
            other => {
                crate::serial_println!(
                    "[fs::handle]   FAIL: open(dir, READ) not IsADirectory: {:?}",
                    other
                );
                return Err(KernelError::InternalError);
            }
        }

        // (b) open(file, READ|DIRECTORY) → NotADirectory.
        match open(file_outside, OpenFlags::READ.union(OpenFlags::DIRECTORY)) {
            Err(KernelError::NotADirectory) => {}
            other => {
                crate::serial_println!(
                    "[fs::handle]   FAIL: open(file, DIRECTORY) not NotADirectory: {:?}",
                    other
                );
                return Err(KernelError::InternalError);
            }
        }

        // (c) open(dir, READ|DIRECTORY) → ok.
        let hd = open(dir_path, OpenFlags::READ.union(OpenFlags::DIRECTORY))?;

        // (d) byte-oriented ops on dir handle → IsADirectory.
        if read(hd, &mut buf) != Err(KernelError::IsADirectory) {
            crate::serial_println!("[fs::handle]   FAIL: read(dir-handle) not IsADirectory");
            close(hd).ok();
            return Err(KernelError::InternalError);
        }
        if write(hd, b"x") != Err(KernelError::IsADirectory) {
            crate::serial_println!("[fs::handle]   FAIL: write(dir-handle) not IsADirectory");
            close(hd).ok();
            return Err(KernelError::InternalError);
        }
        if read_at(hd, 0, &mut buf) != Err(KernelError::IsADirectory) {
            crate::serial_println!("[fs::handle]   FAIL: read_at(dir-handle) not IsADirectory");
            close(hd).ok();
            return Err(KernelError::InternalError);
        }
        if seek(hd, SeekFrom::Start(0)) != Err(KernelError::IsADirectory) {
            crate::serial_println!("[fs::handle]   FAIL: seek(dir-handle) not IsADirectory");
            close(hd).ok();
            return Err(KernelError::InternalError);
        }

        // (e) read_dir_at returns the two files (order is FS-defined; just
        //     verify both names appear).
        let (start, entries) = read_dir_at(hd, 16)?;
        if start != 0 {
            crate::serial_println!("[fs::handle]   FAIL: initial cursor not 0: {}", start);
            close(hd).ok();
            return Err(KernelError::InternalError);
        }
        let mut saw_a = false;
        let mut saw_b = false;
        for e in &entries {
            if e.name.as_path() == Path::new("a.txt") {
                saw_a = true;
            }
            if e.name.as_path() == Path::new("b.txt") {
                saw_b = true;
            }
        }
        if !(saw_a && saw_b) {
            crate::serial_println!(
                "[fs::handle]   FAIL: dir listing missing entries (saw_a={}, saw_b={})",
                saw_a,
                saw_b
            );
            close(hd).ok();
            return Err(KernelError::InternalError);
        }

        // (f) advance cursor past the entries, next read_dir_at returns empty.
        set_dir_cursor(hd, entries.len() as u64)?;
        let (_, page2) = read_dir_at(hd, 16)?;
        if !page2.is_empty() {
            crate::serial_println!(
                "[fs::handle]   FAIL: dir listing after exhaustion not empty ({} entries)",
                page2.len()
            );
            close(hd).ok();
            return Err(KernelError::InternalError);
        }

        // (g) rewind via set_dir_cursor(0).
        set_dir_cursor(hd, 0)?;
        let (_, page3) = read_dir_at(hd, 16)?;
        if page3.len() != entries.len() {
            crate::serial_println!(
                "[fs::handle]   FAIL: rewind+read mismatch ({} vs {})",
                page3.len(),
                entries.len()
            );
            close(hd).ok();
            return Err(KernelError::InternalError);
        }

        // (h) read_dir_at on a file handle → NotADirectory.
        let hf = open(file_outside, OpenFlags::READ)?;
        if let Err(e) = read_dir_at(hf, 16) {
            if e != KernelError::NotADirectory {
                crate::serial_println!(
                    "[fs::handle]   FAIL: read_dir_at(file-handle) wrong err: {:?}",
                    e
                );
                close(hf).ok();
                close(hd).ok();
                return Err(KernelError::InternalError);
            }
        } else {
            crate::serial_println!("[fs::handle]   FAIL: read_dir_at(file-handle) not Err");
            close(hf).ok();
            close(hd).ok();
            return Err(KernelError::InternalError);
        }
        close(hf)?;
        close(hd)?;

        // Cleanup.
        crate::fs::Vfs::remove(file_a).ok();
        crate::fs::Vfs::remove(file_b).ok();
        crate::fs::Vfs::remove(dir_path).ok();
        crate::fs::Vfs::remove(file_outside).ok();

        crate::serial_println!("[fs::handle]   directory handle ops: OK");
    }

    // 16. O_EXCL — exclusive create semantics.
    //
    // Proves both halves so a regression can't silently pass:
    //   (a) CREATE|EXCL on an *existing* path → AlreadyExists (EEXIST), and
    //   (b) CREATE|EXCL on a *fresh* path → succeeds (creates the file), and
    //   (c) a second CREATE|EXCL over the just-created file → AlreadyExists,
    //   (d) EXCL *without* CREATE over an existing file is ignored (opens it).
    let excl_existing = "/handle_excl_existing.txt";
    let excl_fresh = "/handle_excl_fresh.txt";
    crate::fs::Vfs::remove(excl_existing).ok();
    crate::fs::Vfs::remove(excl_fresh).ok();

    // Stage an existing file for (a) and (d).
    crate::fs::Vfs::write_file(excl_existing, b"pre-existing")?;

    // (a) exclusive create over an existing file must fail with EEXIST.
    let excl_flags = OpenFlags::WRITE
        .union(OpenFlags::CREATE)
        .union(OpenFlags::EXCL);
    match open(excl_existing, excl_flags) {
        Err(KernelError::AlreadyExists) => {}
        other => {
            crate::serial_println!(
                "[fs::handle]   FAIL: O_CREAT|O_EXCL on existing file not AlreadyExists: {:?}",
                other
            );
            crate::fs::Vfs::remove(excl_existing).ok();
            return Err(KernelError::InternalError);
        }
    }

    // (d) EXCL without CREATE over an existing file is ignored → opens it.
    let hd_excl = open(excl_existing, OpenFlags::READ.union(OpenFlags::EXCL))?;
    close(hd_excl)?;

    // (b) exclusive create on a fresh path must succeed and create the file.
    let hf_excl = open(excl_fresh, excl_flags)?;
    close(hf_excl)?;
    if crate::fs::Vfs::stat(excl_fresh).is_err() {
        crate::serial_println!(
            "[fs::handle]   FAIL: O_CREAT|O_EXCL fresh path did not create file"
        );
        crate::fs::Vfs::remove(excl_existing).ok();
        crate::fs::Vfs::remove(excl_fresh).ok();
        return Err(KernelError::InternalError);
    }

    // (c) a second exclusive create over the now-existing file must fail.
    match open(excl_fresh, excl_flags) {
        Err(KernelError::AlreadyExists) => {}
        other => {
            crate::serial_println!(
                "[fs::handle]   FAIL: second O_CREAT|O_EXCL not AlreadyExists: {:?}",
                other
            );
            crate::fs::Vfs::remove(excl_existing).ok();
            crate::fs::Vfs::remove(excl_fresh).ok();
            return Err(KernelError::InternalError);
        }
    }

    crate::fs::Vfs::remove(excl_existing).ok();
    crate::fs::Vfs::remove(excl_fresh).ok();
    crate::serial_println!("[fs::handle]   O_EXCL exclusive-create semantics: OK");

    // 17. O_NOFOLLOW — refuse to open through a final symlink component.
    //
    // Stage a real target file and a symlink to it, then prove:
    //   (a) open(symlink, READ) without NOFOLLOW follows to the target (OK),
    //   (b) open(symlink, READ|NOFOLLOW) fails with TooManyLinks (-> ELOOP),
    //   (c) open(regular, READ|NOFOLLOW) succeeds (NOFOLLOW only guards the
    //       final component, and this one is not a symlink).
    // Skipped gracefully if the root FS does not support symlinks.
    let nf_target = "/handle_nofollow_target.txt";
    let nf_link = "/handle_nofollow_link";
    crate::fs::Vfs::remove(nf_link).ok();
    crate::fs::Vfs::remove(nf_target).ok();
    crate::fs::Vfs::write_file(nf_target, b"nofollow target")?;
    let nf_ready = crate::selftest_setup!(
        skips,
        "[fs::handle]",
        "O_NOFOLLOW final-symlink guard",
        "no symlink support",
        crate::fs::Vfs::symlink(nf_link, nf_target),
    );
    if nf_ready {
        // (a) without NOFOLLOW the symlink resolves to the target.
        let hnf = open(nf_link, OpenFlags::READ)?;
        close(hnf)?;

        // (b) with NOFOLLOW a final symlink component is refused with ELOOP.
        match open(nf_link, OpenFlags::READ.union(OpenFlags::NOFOLLOW)) {
            Err(KernelError::TooManyLinks) => {}
            other => {
                crate::serial_println!(
                    "[fs::handle]   FAIL: O_NOFOLLOW on symlink not TooManyLinks: {:?}",
                    other
                );
                crate::fs::Vfs::remove(nf_link).ok();
                crate::fs::Vfs::remove(nf_target).ok();
                return Err(KernelError::InternalError);
            }
        }

        // (c) NOFOLLOW on a non-symlink final component opens normally.
        let hnf2 = open(nf_target, OpenFlags::READ.union(OpenFlags::NOFOLLOW))?;
        close(hnf2)?;

        crate::fs::Vfs::remove(nf_link).ok();
        crate::fs::Vfs::remove(nf_target).ok();
        crate::serial_println!("[fs::handle]   O_NOFOLLOW final-symlink guard: OK");
    } else {
        crate::fs::Vfs::remove(nf_target).ok();
    }

    // 18. NO_SYMLINKS (openat2 RESOLVE_NO_SYMLINKS) — refuse a symlink in ANY
    // component, which O_NOFOLLOW (final-only) cannot catch.  Stage a real
    // directory with a file, a symlink to that directory, and a symlink to the
    // file, then prove:
    //   (a) open(dirlink/f, READ) without NO_SYMLINKS follows the parent
    //       symlink and opens the file (OK),
    //   (b) open(dirlink/f, READ|NO_SYMLINKS) fails ELOOP — a *parent*-component
    //       symlink is rejected (the case NOFOLLOW would happily follow),
    //   (c) open(filelink, READ|NO_SYMLINKS) fails ELOOP — a *final* symlink is
    //       rejected too,
    //   (d) open(dir/f, READ|NO_SYMLINKS) succeeds — no symlink anywhere.
    // Skipped gracefully if the root FS lacks mkdir/symlink support.
    let ns_dir = "/handle_nsym_dir";
    let ns_file = "/handle_nsym_dir/f.txt";
    let ns_dirlink = "/handle_nsym_dirlink";
    let ns_flink = "/handle_nsym_flink";
    let ns_via_dirlink = "/handle_nsym_dirlink/f.txt";
    // Best-effort cleanup of any stragglers from a prior boot.
    crate::fs::Vfs::remove(ns_flink).ok();
    crate::fs::Vfs::remove(ns_dirlink).ok();
    crate::fs::Vfs::remove(ns_file).ok();
    crate::fs::Vfs::rmdir(ns_dir).ok();
    let ns_ready = crate::selftest_setup!(
        skips,
        "[fs::handle]",
        "NO_SYMLINKS any-component guard",
        "no mkdir/symlink support",
        crate::fs::Vfs::mkdir(ns_dir),
        crate::fs::Vfs::write_file(ns_file, b"nsym"),
        crate::fs::Vfs::symlink(ns_dirlink, ns_dir),
        crate::fs::Vfs::symlink(ns_flink, ns_file),
    );
    if ns_ready {
        // Local cleanup helper for the early-return failure paths.
        let cleanup = || {
            crate::fs::Vfs::remove(ns_flink).ok();
            crate::fs::Vfs::remove(ns_dirlink).ok();
            crate::fs::Vfs::remove(ns_file).ok();
            crate::fs::Vfs::rmdir(ns_dir).ok();
        };

        // (a) without NO_SYMLINKS the parent symlink is followed.
        match open(ns_via_dirlink, OpenFlags::READ) {
            Ok(h) => {
                close(h)?;
            }
            other => {
                crate::serial_println!(
                    "[fs::handle]   FAIL: open via dir symlink (no flag) not OK: {:?}",
                    other
                );
                cleanup();
                return Err(KernelError::InternalError);
            }
        }

        // (b) with NO_SYMLINKS a PARENT-component symlink is refused (ELOOP).
        match open(
            ns_via_dirlink,
            OpenFlags::READ.union(OpenFlags::NO_SYMLINKS),
        ) {
            Err(KernelError::TooManyLinks) => {}
            other => {
                crate::serial_println!(
                    "[fs::handle]   FAIL: NO_SYMLINKS parent-symlink not TooManyLinks: {:?}",
                    other
                );
                cleanup();
                return Err(KernelError::InternalError);
            }
        }

        // (c) with NO_SYMLINKS a FINAL symlink is also refused (ELOOP).
        match open(ns_flink, OpenFlags::READ.union(OpenFlags::NO_SYMLINKS)) {
            Err(KernelError::TooManyLinks) => {}
            other => {
                crate::serial_println!(
                    "[fs::handle]   FAIL: NO_SYMLINKS final-symlink not TooManyLinks: {:?}",
                    other
                );
                cleanup();
                return Err(KernelError::InternalError);
            }
        }

        // (d) a symlink-free path opens normally even with NO_SYMLINKS.
        match open(ns_file, OpenFlags::READ.union(OpenFlags::NO_SYMLINKS)) {
            Ok(h) => {
                close(h)?;
            }
            other => {
                crate::serial_println!(
                    "[fs::handle]   FAIL: NO_SYMLINKS on symlink-free path not OK: {:?}",
                    other
                );
                cleanup();
                return Err(KernelError::InternalError);
            }
        }

        cleanup();
        crate::serial_println!("[fs::handle]   NO_SYMLINKS any-component guard: OK");
    } else {
        crate::fs::Vfs::remove(ns_flink).ok();
        crate::fs::Vfs::remove(ns_dirlink).ok();
        crate::fs::Vfs::remove(ns_file).ok();
        crate::fs::Vfs::rmdir(ns_dir).ok();
    }

    // -- §19: no-follow chown/times operate on the LINK inode, not target --
    // lchown / fchownat(AT_SYMLINK_NOFOLLOW) / lutimes / utimensat(NOFOLLOW)
    // must mutate the symlink's own inode.  Prove that:
    //   (a) set_owner_no_follow changes the LINK's uid/gid (lmetadata) but
    //       leaves the TARGET's owner untouched (metadata follows the link),
    //   (b) set_owner (following) changes the TARGET, not the link.
    // Skipped gracefully if the root FS lacks symlink/chown support.
    let nfo_target = "/handle_nfo_target.txt";
    let nfo_link = "/handle_nfo_link";
    crate::fs::Vfs::remove(nfo_link).ok();
    crate::fs::Vfs::remove(nfo_target).ok();
    // `set_owner` and `set_owner_no_follow` here are both entry points that
    // `vfs::check_path_access` gates.  Under the old `.is_ok()` chain, a gate
    // that started refusing them would have switched off precisely the test
    // that noticed, and the suite would still have printed PASSED.
    let nfo_ready = crate::selftest_setup!(
        skips,
        "[fs::handle]",
        "no-follow chown targets link inode",
        "no symlink/chown support",
        crate::fs::Vfs::write_file(nfo_target, b"nfo"),
        crate::fs::Vfs::symlink(nfo_link, nfo_target),
        // Seed distinct baseline owners so a mistaken follow is detectable.
        crate::fs::Vfs::set_owner(nfo_target, 1000, 1000),
        crate::fs::Vfs::set_owner_no_follow(nfo_link, 1000, 1000),
    );
    if nfo_ready {
        let nfo_cleanup = || {
            crate::fs::Vfs::remove(nfo_link).ok();
            crate::fs::Vfs::remove(nfo_target).ok();
        };
        // (a) chown the LINK itself (no-follow) to a distinct uid.
        if let Err(e) = crate::fs::Vfs::set_owner_no_follow(nfo_link, 4242, 4243) {
            crate::serial_println!("[fs::handle]   FAIL: set_owner_no_follow: {:?}", e);
            nfo_cleanup();
            return Err(KernelError::InternalError);
        }
        // The link's own inode (lmetadata = no-follow) must show the new uid.
        let link_meta = crate::fs::Vfs::lmetadata(nfo_link)?;
        // The target (metadata = follow) must be UNCHANGED at the baseline.
        let tgt_meta = crate::fs::Vfs::metadata(nfo_link)?;
        if link_meta.uid != 4242 || link_meta.gid != 4243 {
            crate::serial_println!(
                "[fs::handle]   FAIL: no-follow chown didn't stamp link (uid={} gid={})",
                link_meta.uid,
                link_meta.gid
            );
            nfo_cleanup();
            return Err(KernelError::InternalError);
        }
        if tgt_meta.uid != 1000 || tgt_meta.gid != 1000 {
            crate::serial_println!(
                "[fs::handle]   FAIL: no-follow chown leaked to target (uid={} gid={})",
                tgt_meta.uid,
                tgt_meta.gid
            );
            nfo_cleanup();
            return Err(KernelError::InternalError);
        }
        // (b) following chown must hit the TARGET, not the link.
        if let Err(e) = crate::fs::Vfs::set_owner(nfo_link, 7000, 7001) {
            crate::serial_println!("[fs::handle]   FAIL: set_owner (follow): {:?}", e);
            nfo_cleanup();
            return Err(KernelError::InternalError);
        }
        let link_after = crate::fs::Vfs::lmetadata(nfo_link)?;
        let tgt_after = crate::fs::Vfs::metadata(nfo_link)?;
        if tgt_after.uid != 7000 || link_after.uid != 4242 {
            crate::serial_println!(
                "[fs::handle]   FAIL: follow chown wrong target (link.uid={} tgt.uid={})",
                link_after.uid,
                tgt_after.uid
            );
            nfo_cleanup();
            return Err(KernelError::InternalError);
        }
        nfo_cleanup();
        crate::serial_println!("[fs::handle]   no-follow chown targets link inode: OK");
    } else {
        crate::fs::Vfs::remove(nfo_link).ok();
        crate::fs::Vfs::remove(nfo_target).ok();
    }

    // -- §20: no-follow xattr operate on the LINK inode, not target --
    // lsetxattr / lgetxattr / llistxattr / lremovexattr must mutate the
    // symlink's OWN inode, not the file it points to.  Prove that:
    //   (a) set_xattr_no_follow stores on the LINK (get_xattr_no_follow
    //       reads it back) while the TARGET (get_xattr = follow) has no
    //       such attribute — the write did not leak through the link,
    //   (b) set_xattr (following) stores on the TARGET, invisible to the
    //       link's own no-follow view,
    //   (c) remove_xattr_no_follow strips the link's attribute only.
    // Skipped gracefully if the root FS lacks symlink/xattr support.
    //
    // A `trusted.` name, which a link may carry: `user.` is refused on a link
    // (`fs::xattr_policy`, as on Linux), and the setup below would read that
    // refusal as "no xattr support" and skip the section. This test is a
    // kernel task, which is privileged.
    let nfx_target = "/handle_nfx_target.txt";
    let nfx_link = "/handle_nfx_link";
    let nfx_key: &[u8] = b"trusted.nfx";
    crate::fs::Vfs::remove(nfx_link).ok();
    crate::fs::Vfs::remove(nfx_target).ok();
    let nfx_ready = crate::selftest_setup!(
        skips,
        "[fs::handle]",
        "no-follow xattr targets link inode",
        "no symlink/xattr support",
        crate::fs::Vfs::write_file(nfx_target, b"nfx"),
        crate::fs::Vfs::symlink(nfx_link, nfx_target),
        // Probe xattr support on the link itself; skip if unsupported.
        crate::fs::Vfs::set_xattr_no_follow(nfx_link, nfx_key, b"link"),
    );
    if nfx_ready {
        let nfx_cleanup = || {
            crate::fs::Vfs::remove(nfx_link).ok();
            crate::fs::Vfs::remove(nfx_target).ok();
        };
        // (a) the LINK's own inode carries the attribute (no-follow read).
        match crate::fs::Vfs::get_xattr_no_follow(nfx_link, nfx_key) {
            Ok(v) if v == b"link" => {}
            other => {
                crate::serial_println!(
                    "[fs::handle]   FAIL: no-follow xattr not on link inode: {:?}",
                    other
                );
                nfx_cleanup();
                return Err(KernelError::InternalError);
            }
        }
        // ... and the TARGET (follow) must NOT see it — the write to the
        // link did not leak to the pointed-at file.
        //
        // The error has to be `NoAttribute`, not merely *some* error: the
        // target file plainly exists (we wrote it above), so a `NotFound` here
        // would be the kernel saying the file is gone when it means the
        // attribute is absent.  Accepting any `Err` is what let those two be
        // conflated for as long as they were.  See `design-decisions.md` §660.
        match crate::fs::Vfs::get_xattr(nfx_link, nfx_key) {
            Err(KernelError::NoAttribute) => {}
            other => {
                crate::serial_println!(
                    "[fs::handle]   FAIL: target should report NoAttribute, got {:?}",
                    other
                );
                nfx_cleanup();
                return Err(KernelError::InternalError);
            }
        }
        // The other half of the same distinction: a path that does not resolve
        // is still `NotFound`.  One code for both would make these two calls
        // indistinguishable, and a caller cannot branch on a difference it
        // cannot see.
        match crate::fs::Vfs::get_xattr("/handle_nfx_absent.txt", nfx_key) {
            Err(KernelError::NotFound) => {}
            other => {
                crate::serial_println!(
                    "[fs::handle]   FAIL: missing path should report NotFound, got {:?}",
                    other
                );
                nfx_cleanup();
                return Err(KernelError::InternalError);
            }
        }
        // (b) a following set must hit the TARGET, invisible to the link.
        if let Err(e) = crate::fs::Vfs::set_xattr(nfx_link, nfx_key, b"target") {
            crate::serial_println!("[fs::handle]   FAIL: set_xattr (follow): {:?}", e);
            nfx_cleanup();
            return Err(KernelError::InternalError);
        }
        match (
            crate::fs::Vfs::get_xattr(nfx_link, nfx_key),
            crate::fs::Vfs::get_xattr_no_follow(nfx_link, nfx_key),
        ) {
            (Ok(t), Ok(l)) if t == b"target" && l == b"link" => {}
            other => {
                crate::serial_println!(
                    "[fs::handle]   FAIL: follow/no-follow xattr views crossed: {:?}",
                    other
                );
                nfx_cleanup();
                return Err(KernelError::InternalError);
            }
        }
        // llistxattr must list the link's key (not necessarily the target's).
        match crate::fs::Vfs::list_xattrs_no_follow(nfx_link) {
            Ok(keys) if keys.iter().any(|k| k == nfx_key) => {}
            other => {
                crate::serial_println!(
                    "[fs::handle]   FAIL: no-follow list missing link key: {:?}",
                    other
                );
                nfx_cleanup();
                return Err(KernelError::InternalError);
            }
        }
        // (c) no-follow remove strips the LINK's attribute; the TARGET keeps
        // its own following-set value.
        if let Err(e) = crate::fs::Vfs::remove_xattr_no_follow(nfx_link, nfx_key) {
            crate::serial_println!("[fs::handle]   FAIL: remove_xattr_no_follow: {:?}", e);
            nfx_cleanup();
            return Err(KernelError::InternalError);
        }
        let link_gone = crate::fs::Vfs::get_xattr_no_follow(nfx_link, nfx_key).is_err();
        let tgt_kept = matches!(
            crate::fs::Vfs::get_xattr(nfx_link, nfx_key),
            Ok(ref v) if v == b"target"
        );
        if !link_gone || !tgt_kept {
            crate::serial_println!(
                "[fs::handle]   FAIL: no-follow remove wrong inode (link_gone={} tgt_kept={})",
                link_gone,
                tgt_kept
            );
            nfx_cleanup();
            return Err(KernelError::InternalError);
        }
        nfx_cleanup();
        crate::serial_println!("[fs::handle]   no-follow xattr targets link inode: OK");
    } else {
        crate::fs::Vfs::remove(nfx_link).ok();
        crate::fs::Vfs::remove(nfx_target).ok();
    }

    // -- §20c: XATTR_CREATE / XATTR_REPLACE are decided in the kernel --
    // The mode is checked under the same filesystem lock that performs the
    // write, so "create, or fail" cannot be turned into an overwrite by a
    // writer arriving between a probe and a set.  A single-threaded test
    // cannot observe the race, so what it pins is the part that made the
    // old userspace probe *wrong* rather than merely racy: which error each
    // refusal spends, and above all that REPLACE on a missing PATH is
    // `NotFound` and not `NoAttribute` — the probe read every negative
    // return as "no such attribute" and so answered ENODATA for a file that
    // did not exist.
    let xm_path = "/handle_xmode.txt";
    let xm_key: &[u8] = b"user.xmode";
    crate::fs::Vfs::remove(xm_path).ok();
    let xm_ready = crate::selftest_setup!(
        skips,
        "[fs::handle]",
        "xattr create/replace modes",
        "no xattr support",
        crate::fs::Vfs::write_file(xm_path, b"xm"),
        crate::fs::Vfs::set_xattr(xm_path, xm_key, b"seed"),
        crate::fs::Vfs::remove_xattr(xm_path, xm_key),
    );
    if xm_ready {
        let xm_cleanup = || {
            crate::fs::Vfs::remove(xm_path).ok();
        };
        let xm_fail = |what: &str| {
            crate::serial_println!("[fs::handle]   FAIL: xattr mode: {}", what);
            xm_cleanup();
            KernelError::InternalError
        };
        use crate::fs::XattrSetMode;

        // CREATE on an absent attribute succeeds and stores the value.
        if crate::fs::Vfs::set_xattr_with(xm_path, xm_key, b"first", XattrSetMode::Create).is_err()
        {
            return Err(xm_fail("CREATE on absent attribute was refused"));
        }
        // CREATE on a present attribute is EEXIST, and must not overwrite.
        if !matches!(
            crate::fs::Vfs::set_xattr_with(xm_path, xm_key, b"second", XattrSetMode::Create),
            Err(KernelError::AlreadyExists)
        ) {
            return Err(xm_fail("CREATE on present attribute was not AlreadyExists"));
        }
        if !matches!(crate::fs::Vfs::get_xattr(xm_path, xm_key), Ok(ref v) if v == b"first") {
            return Err(xm_fail("refused CREATE still overwrote the value"));
        }
        // REPLACE on a present attribute succeeds and does overwrite.
        if crate::fs::Vfs::set_xattr_with(xm_path, xm_key, b"third", XattrSetMode::Replace).is_err()
        {
            return Err(xm_fail("REPLACE on present attribute was refused"));
        }
        if !matches!(crate::fs::Vfs::get_xattr(xm_path, xm_key), Ok(ref v) if v == b"third") {
            return Err(xm_fail("REPLACE did not store the new value"));
        }
        // REPLACE on an absent attribute is ENODATA, and must not create.
        if crate::fs::Vfs::remove_xattr(xm_path, xm_key).is_err() {
            return Err(xm_fail("remove_xattr failed"));
        }
        if !matches!(
            crate::fs::Vfs::set_xattr_with(xm_path, xm_key, b"fourth", XattrSetMode::Replace),
            Err(KernelError::NoAttribute)
        ) {
            return Err(xm_fail("REPLACE on absent attribute was not NoAttribute"));
        }
        if !matches!(
            crate::fs::Vfs::get_xattr(xm_path, xm_key),
            Err(KernelError::NoAttribute)
        ) {
            return Err(xm_fail("refused REPLACE created the attribute anyway"));
        }
        // REPLACE against a path that does not exist is about the PATH.
        if !matches!(
            crate::fs::Vfs::set_xattr_with(
                "/handle_xmode_absent.txt",
                xm_key,
                b"x",
                XattrSetMode::Replace
            ),
            Err(KernelError::NotFound)
        ) {
            return Err(xm_fail("REPLACE on a missing path was not NotFound"));
        }
        xm_cleanup();
        crate::serial_println!("[fs::handle]   xattr create/replace modes: OK");
    } else {
        crate::fs::Vfs::remove(xm_path).ok();
    }

    // -- §21: no-follow chmod operates on the LINK inode, not target --
    // fchmodat2(AT_SYMLINK_NOFOLLOW) must set the symlink's own mode bits.
    // Prove that:
    //   (a) set_permissions_no_follow changes the LINK's mode (lmetadata) but
    //       leaves the TARGET's mode untouched (metadata follows the link),
    //   (b) set_permissions (following) changes the TARGET, not the link.
    // Skipped gracefully if the root FS lacks symlink/chmod support.
    let nfp_target = "/handle_nfp_target.txt";
    let nfp_link = "/handle_nfp_link";
    crate::fs::Vfs::remove(nfp_link).ok();
    crate::fs::Vfs::remove(nfp_target).ok();
    let nfp_ready = crate::selftest_setup!(
        skips,
        "[fs::handle]",
        "no-follow chmod targets link inode",
        "no symlink/chmod support",
        crate::fs::Vfs::write_file(nfp_target, b"nfp"),
        crate::fs::Vfs::symlink(nfp_link, nfp_target),
        // Seed distinct baseline modes so a mistaken follow is detectable.
        crate::fs::Vfs::set_permissions(nfp_target, 0o644),
        crate::fs::Vfs::set_permissions_no_follow(nfp_link, 0o644),
    );
    if nfp_ready {
        let nfp_cleanup = || {
            crate::fs::Vfs::remove(nfp_link).ok();
            crate::fs::Vfs::remove(nfp_target).ok();
        };
        // (a) chmod the LINK itself (no-follow) to a distinct mode.
        if let Err(e) = crate::fs::Vfs::set_permissions_no_follow(nfp_link, 0o600) {
            crate::serial_println!("[fs::handle]   FAIL: set_permissions_no_follow: {:?}", e);
            nfp_cleanup();
            return Err(KernelError::InternalError);
        }
        let link_meta = crate::fs::Vfs::lmetadata(nfp_link)?;
        let tgt_meta = crate::fs::Vfs::metadata(nfp_link)?;
        if link_meta.permissions & 0o777 != 0o600 {
            crate::serial_println!(
                "[fs::handle]   FAIL: no-follow chmod didn't stamp link (mode={:o})",
                link_meta.permissions
            );
            nfp_cleanup();
            return Err(KernelError::InternalError);
        }
        if tgt_meta.permissions & 0o777 != 0o644 {
            crate::serial_println!(
                "[fs::handle]   FAIL: no-follow chmod leaked to target (mode={:o})",
                tgt_meta.permissions
            );
            nfp_cleanup();
            return Err(KernelError::InternalError);
        }
        // (b) following chmod must hit the TARGET, not the link.
        if let Err(e) = crate::fs::Vfs::set_permissions(nfp_link, 0o755) {
            crate::serial_println!("[fs::handle]   FAIL: set_permissions (follow): {:?}", e);
            nfp_cleanup();
            return Err(KernelError::InternalError);
        }
        let link_after = crate::fs::Vfs::lmetadata(nfp_link)?;
        let tgt_after = crate::fs::Vfs::metadata(nfp_link)?;
        if tgt_after.permissions & 0o777 != 0o755 || link_after.permissions & 0o777 != 0o600 {
            crate::serial_println!(
                "[fs::handle]   FAIL: follow chmod wrong target (link={:o} tgt={:o})",
                link_after.permissions,
                tgt_after.permissions
            );
            nfp_cleanup();
            return Err(KernelError::InternalError);
        }
        nfp_cleanup();
        crate::serial_println!("[fs::handle]   no-follow chmod targets link inode: OK");
    } else {
        crate::fs::Vfs::remove(nfp_link).ok();
        crate::fs::Vfs::remove(nfp_target).ok();
    }

    // `open_beneath` — RESOLVE_BENEATH all the way to an installed handle.
    //
    // `fs::vfs`'s self-test already proves the containment *rule* against
    // real symlinks, and `syscall::linux`'s proves the two refusals the ABI
    // returns.  Neither reaches this function, and the thing neither can
    // show is the one that matters most here: that a contained open still
    // **succeeds**, and hands back a handle that reads the right bytes.  A
    // containment check that refused everything would pass both of those
    // suites and be useless, so the positive case is asserted first.
    let bn_base = "/handle_beneath/base";
    let bn_cleanup = || {
        crate::fs::Vfs::remove("/handle_beneath/base/escape").ok();
        crate::fs::Vfs::remove("/handle_beneath/base/sub/f").ok();
        crate::fs::Vfs::rmdir("/handle_beneath/base/sub").ok();
        crate::fs::Vfs::rmdir("/handle_beneath/base").ok();
        crate::fs::Vfs::remove("/handle_beneath/out/f").ok();
        crate::fs::Vfs::rmdir("/handle_beneath/out").ok();
        crate::fs::Vfs::rmdir("/handle_beneath").ok();
    };
    bn_cleanup();
    let bn_ready = crate::selftest_setup!(
        skips,
        "[fs::handle]",
        "open_beneath honours RESOLVE_BENEATH",
        "no mkdir/symlink support",
        crate::fs::Vfs::mkdir_all("/handle_beneath/base/sub"),
        crate::fs::Vfs::mkdir_all("/handle_beneath/out"),
        crate::fs::Vfs::write_file("/handle_beneath/base/sub/f", b"inside"),
        crate::fs::Vfs::write_file("/handle_beneath/out/f", b"outside"),
        // An ancestor symlink that leaves the base.  The file it reaches
        // exists and is readable, so nothing but containment can refuse it —
        // which is the whole point of testing with a live target rather than
        // a dangling one.
        crate::fs::Vfs::symlink("/handle_beneath/base/escape", "../out"),
    );
    if bn_ready {
        let ok = |rel: &str| -> KernelResult<()> {
            let h = open_beneath(
                Beneath {
                    base: Path::new(bn_base.as_bytes()),
                    rel: Path::new(rel.as_bytes()),
                },
                OpenFlags::READ,
            )?;
            let mut buf = [0u8; 16];
            let n = read(h, &mut buf)?;
            close(h)?;
            if buf.get(..n) != Some(b"inside".as_slice()) {
                crate::serial_println!(
                    "[fs::handle]   FAIL: open_beneath({}) read the wrong file",
                    rel
                );
                return Err(KernelError::InternalError);
            }
            Ok(())
        };
        // Allowed: plainly inside, and inside by a route whose `..` never
        // rises above the base.  The second is the row a "canonicalise the
        // final path" implementation gets right by accident and a "reject any
        // `..`" implementation gets wrong.
        for rel in ["sub/f", "sub/../sub/f"] {
            if let Err(e) = ok(rel) {
                crate::serial_println!("[fs::handle]   FAIL: open_beneath({}): {:?}", rel, e);
                bn_cleanup();
                return Err(e);
            }
        }
        // Refused: `..` out of the base; an absolute path *naming a file that
        // is inside the base* (refused for being absolute, not for where it
        // points); and a symlink whose target leaves.
        for rel in [
            "../out/f",
            "/handle_beneath/base/sub/f",
            "escape/f",
            "../handle_beneath/base/sub/f",
        ] {
            match open_beneath(
                Beneath {
                    base: Path::new(bn_base.as_bytes()),
                    rel: Path::new(rel.as_bytes()),
                },
                OpenFlags::READ,
            ) {
                Err(KernelError::CrossDevice) => {}
                other => {
                    // A leaked handle here would also be a leaked *capability*,
                    // so close it rather than only complaining.
                    if let Ok(h) = other {
                        close(h).ok();
                    }
                    crate::serial_println!(
                        "[fs::handle]   FAIL: open_beneath({}) was not refused with CrossDevice",
                        rel
                    );
                    bn_cleanup();
                    return Err(KernelError::InternalError);
                }
            }
        }
        // The escape route is real: unconfined, the same path opens the file
        // outside.  Without this the refusals above would also pass if the
        // symlink were simply broken.
        match open(Path::new(b"/handle_beneath/base/escape/f"), OpenFlags::READ) {
            Ok(h) => {
                let mut buf = [0u8; 16];
                let n = read(h, &mut buf).unwrap_or(0);
                close(h).ok();
                if buf.get(..n) != Some(b"outside".as_slice()) {
                    crate::serial_println!(
                        "[fs::handle]   FAIL: unconfined open through the escape symlink read \
                         the wrong file — the refusals above prove nothing"
                    );
                    bn_cleanup();
                    return Err(KernelError::InternalError);
                }
            }
            Err(e) => {
                crate::serial_println!(
                    "[fs::handle]   FAIL: the escape symlink does not actually escape ({:?}), \
                     so the CrossDevice refusals above are vacuous",
                    e
                );
                bn_cleanup();
                return Err(KernelError::InternalError);
            }
        }
        bn_cleanup();
        crate::serial_println!("[fs::handle]   open_beneath honours RESOLVE_BENEATH: OK");
    } else {
        bn_cleanup();
    }

    // Cleanup test files.
    crate::fs::Vfs::remove(lock_path).ok();
    crate::fs::Vfs::remove(test_path).ok();
    crate::fs::Vfs::remove("/handle_write_test.txt").ok();

    // Handles hold their files, not their names.
    test_held_files(&mut skips)?;

    skips.report("[fs::handle]");
    crate::serial_println!("[fs::handle] Self-test PASSED{}", skips.suffix());
    Ok(())
}
