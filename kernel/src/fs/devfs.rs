//! Device pseudo-filesystem (`/dev`).
//!
//! Provides virtual device files that are essential for standard programs
//! and shell usage.  All content is generated/consumed dynamically.
//!
//! ## Layout
//!
//! ```text
//! /dev/
//! ├── null       Discards all writes, reads return EOF (empty)
//! ├── zero       Reads return zero bytes, writes succeed
//! ├── full       Reads return zero bytes, writes fail with DiskFull
//! ├── random     Reads return CSPRNG bytes (ChaCha20, kernel `rng`)
//! ├── urandom    Same as random (no entropy blocking distinction)
//! ├── console    Reads/writes to the kernel console
//! ├── tty        Controlling terminal (aliases console, single-console)
//! ├── stdin/stdout/stderr, kmsg, uptime
//! ├── input/     event0 (keyboard), event1 (mouse)
//! ├── dri/       card0, renderD128
//! ├── snd/       controlC0, pcmC0D0p, pcmC0D0c
//! └── <disk>     One per registered block device: vda, nvme0n1, …
//! ```
//!
//! ## Design
//!
//! This is a minimal devfs for kernel-mode use.  In our microkernel
//! architecture, hardware devices are managed by userspace drivers via
//! IPC.  It provides the standard "utility" device files that programs expect.
//!
//! ## Block devices, and why they are the exception
//!
//! Everything above this line is a fixed table.  Block device nodes are not:
//! they are enumerated from [`crate::blkdev`] on every lookup, so the namespace
//! reflects what is registered rather than what was true when this file was
//! written.
//!
//! This module used to say, in this paragraph, that "the devfs does NOT expose
//! block devices."  That was a real position and not an oversight — storage is
//! reached through a mounted filesystem, and a program that wants a file should
//! open a file.  What changed is that two programs turned up whose subject is
//! the device itself: a disk imager writing an `.iso` to a USB stick, and a
//! partition editor.  Neither is asking for a shortcut to a file; for both, the
//! raw device *is* the object, and there is no filesystem on it to go through
//! — that is the point of the program.  Routing them through IPC to a userspace
//! driver would not change what they do, only how many hops it takes to do it,
//! because the driver is in this kernel already.
//!
//! Three properties make this safe to serve from here rather than merely
//! convenient:
//!
//! - **The bytes are not cached.**  The VFS page cache routes only
//!   `EntryType::File` with a stable inode ([`crate::fs::vfs`]
//!   `read_at_routed`), so a block node bypasses it by construction.  That is
//!   load-bearing for the imager's verify pass, which writes a device and then
//!   reads it back: a cached read would compare the image against itself and
//!   pass on a stick that was never written.
//! - **The offset means something.**  Unlike every other node here, these are
//!   seekable, and `write_at` honours the offset instead of forwarding to
//!   `write_file`.
//! - **Moving bytes needs authority, not just a mode bit.**  Overwriting a
//!   whole disk is the most destructive thing a userspace program on this
//!   system can do, and reading one sees every file the caller could not open
//!   by name, so each direction demands a
//!   [`ResourceType::BlockDevice`](crate::cap::ResourceType::BlockDevice)
//!   capability with the matching right — checked in [`require_block_cap`],
//!   here rather than in the syscall layer, for the reason given there.
//!   Enumerating is not gated: `readdir` and `stat` name devices without
//!   yielding a byte of them, listing which disks exist is not destructive, and
//!   a program that had to hold the right to erase every disk in the machine
//!   before it could draw its sidebar is one that must be launched
//!   over-privileged.
//!
//! Reads are byte-granular over a sector-granular device and may come back
//! short; writes are all-or-nothing (see [`block_write_at`] for why the return
//! type forces that).  `read_file` on a block node is refused rather than
//! served — a whole-device read into one `Vec` is not a smaller version of the
//! right thing.
//!
//! ## The subdirectories are a namespace, not an implementation
//!
//! The nodes under `input/`, `dri/` and `snd/` are **not served by this
//! filesystem**.  `open` of one is intercepted in the syscall layer
//! (`syscall/linux.rs`), which mints a device handle rather than a VFS file;
//! reading one through the VFS gets `NotSupported`.  They are listed here
//! because existing-but-invisible is not a state a real client can cope with:
//! libinput *scans* `/dev/input/` to discover devices and checks `S_ISCHR` on
//! what it finds, libdrm and ALSA do the same for their directories, and until
//! this table existed all three would have concluded the machine had no
//! hardware while the nodes sat there working for anyone who knew the exact
//! path.  Keeping the namespace here — rather than teaching each client our
//! paths — is what makes those libraries usable unmodified.
//!
//! `/dev/random` and `/dev/urandom` both delegate to the kernel
//! CSPRNG (`crate::rng::fill`, ChaCha20-based, seeded at boot from
//! RDSEED/RDRAND/HPET/TSC).  Output is cryptographically secure; the
//! random/urandom split exists purely for API compatibility — we
//! never block waiting for entropy.

#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::fs::vfs::{DirEntry, EntryType, FileAttr, FileMeta, FileSystem, FsInfo};

// ---------------------------------------------------------------------------
// Random bytes — delegates to kernel CSPRNG (rng module)
// ---------------------------------------------------------------------------

/// Fill a buffer with cryptographically-secure random bytes.
///
/// Delegates to the kernel CSPRNG (ChaCha20-based, seeded from hardware
/// entropy sources).  This replaces the previous weak xorshift64 PRNG.
fn fill_random(buf: &mut [u8]) {
    crate::rng::fill(buf);
}

// ---------------------------------------------------------------------------
// DevFs implementation
// ---------------------------------------------------------------------------

/// Virtual filesystem exposing standard device files.
pub struct DevFs;

impl DevFs {
    /// Create a new DevFs instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// One node in the devfs namespace.
///
/// `path` is relative to the devfs mount point and may contain a `/`, because
/// devfs has subdirectories now. They are spelled out flat here rather than
/// built as a tree: the namespace is a fixed handful of entries the kernel
/// knows at compile time, so a tree would be a data structure with no
/// variation to justify it. A devfs that grew nodes at run time would want the
/// tree; ours does not.
struct DevNode {
    /// Path relative to the devfs root — `"null"`, `"input/event0"`.
    path: &'static str,
    /// What `stat` reports. [`EntryType::CharDevice`] for real device nodes.
    entry_type: EntryType,
    /// Unix permission bits.
    mode: u16,
}

impl DevNode {
    /// A utility file this filesystem serves itself.
    const fn file(path: &'static str, mode: u16) -> Self {
        Self {
            path,
            entry_type: EntryType::File,
            mode,
        }
    }

    /// A directory. Mode 0o755, as on Linux.
    const fn dir(path: &'static str) -> Self {
        Self {
            path,
            entry_type: EntryType::Directory,
            mode: 0o755,
        }
    }

    /// A character device node, served by the syscall layer, not by devfs.
    ///
    /// Mode 0o660 for all of them: on Linux each is `crw-rw----` owned by a
    /// per-class group (`input`, `video`, `audio`). We have no groups yet, so
    /// the bits are the honest part and the ownership is not.
    const fn chr(path: &'static str) -> Self {
        Self {
            path,
            entry_type: EntryType::CharDevice,
            mode: 0o660,
        }
    }

    /// The final component of [`path`](Self::path) — what `readdir` reports.
    fn name(&self) -> &'static str {
        match self.path.rsplit_once('/') {
            Some((_, base)) => base,
            None => self.path,
        }
    }

    /// The directory this node lives in, `""` for the root.
    fn parent(&self) -> &'static str {
        match self.path.rsplit_once('/') {
            Some((dir, _)) => dir,
            None => "",
        }
    }
}

/// The whole devfs namespace.
///
/// The character devices in the subdirectories are **not readable through this
/// filesystem** — `open` of one is intercepted in the syscall layer
/// (`syscall/linux.rs`) and mints a device handle instead of a VFS file. They
/// are listed here anyway, and that is the point of this table: a client that
/// *scans* `/dev/input/` to find input devices — which is exactly what libinput
/// does — has to be able to see them, and `stat` has to report `S_IFCHR` or
/// libinput will reject a device that works. Before this table those nodes were
/// openable by exact path and invisible to everything else, which is the kind
/// of half-existence that makes a port fail for no discoverable reason.
const DEV_NODES: &[DevNode] = &[
    // Utility files, served by this filesystem's own read/write.
    DevNode::file("null", 0o666),
    DevNode::file("zero", 0o666),
    DevNode::file("full", 0o666),
    DevNode::file("random", 0o666),
    DevNode::file("urandom", 0o666),
    DevNode::file("console", 0o600),
    DevNode::file("tty", 0o666),
    DevNode::file("stdin", 0o666),
    DevNode::file("stdout", 0o666),
    DevNode::file("stderr", 0o666),
    DevNode::file("kmsg", 0o666),
    // Read-only: `write_file` refuses it with NotSupported, so 0o444 is what
    // the mode bits should have said all along.
    DevNode::file("uptime", 0o444),
    // Device-node directories.
    DevNode::dir("input"),
    DevNode::dir("dri"),
    DevNode::dir("snd"),
    // Input devices — `evdev_ioctl` / `evdev_read` in the syscall layer.
    DevNode::chr("input/event0"),
    DevNode::chr("input/event1"),
    // DRM card and render node.
    DevNode::chr("dri/card0"),
    DevNode::chr("dri/renderD128"),
    // ALSA control and PCM substreams.
    DevNode::chr("snd/controlC0"),
    DevNode::chr("snd/pcmC0D0p"),
    DevNode::chr("snd/pcmC0D0c"),
];

/// Look up a node by its devfs-relative path.
fn find_node(rel: &str) -> Option<&'static DevNode> {
    DEV_NODES.iter().find(|n| n.path == rel)
}

// ---------------------------------------------------------------------------
// Block devices — the one part of this namespace that is not a fixed table
// ---------------------------------------------------------------------------

/// The block device named by a devfs-relative path, if there is one.
///
/// Unlike everything in [`DEV_NODES`], these names are not known at compile
/// time: they come from whatever [`crate::blkdev::register`] was given, which
/// depends on what the machine has. So the lookup is a query rather than a
/// table scan, and a name that was valid a moment ago can stop being valid when
/// a device is unregistered — callers must treat `None` as "gone", not as
/// "impossible".
///
/// Only the devfs root can hold one: `rel` containing a `/` is rejected before
/// the query, so `input/vda` cannot alias `vda`.
fn find_block(rel: &str) -> Option<crate::blkdev::BlockDeviceInfo> {
    if rel.is_empty() || rel.contains('/') {
        return None;
    }
    // A name that collides with a fixed node loses to it. That collision
    // should never happen -- no block driver names a device `null` -- but if
    // one ever did, silently shadowing `/dev/null` with a disk is the single
    // worst outcome available, so the static table wins by construction.
    if find_node(rel).is_some() {
        return None;
    }
    crate::blkdev::info(rel)
}

/// Total byte capacity of a block device, saturating rather than wrapping.
///
/// `sector_count * sector_size` is a `u64 * u32` that cannot overflow for any
/// real disk, but "cannot" here rests on a driver reporting an honest geometry,
/// and this number is handed to userspace as a file size. Saturating keeps a
/// nonsense geometry to a nonsense *size* instead of letting it wrap to a small
/// one, which is the version that would let a bounds check pass.
fn block_size_bytes(info: &crate::blkdev::BlockDeviceInfo) -> u64 {
    info.sector_count
        .saturating_mul(u64::from(info.sector_size))
}

/// This device's sector size as a divisor that is known not to be zero.
///
/// A driver reporting a zero sector size would make every `/ sector_size` below
/// a divide fault. Nothing in-tree does, and this is what keeps "nothing does"
/// from being load-bearing.
///
/// It returns a [`NonZeroU64`](core::num::NonZeroU64) rather than checking and
/// returning a plain `u64` so that the guard is carried in the *type* to every
/// division instead of being re-established beside each one. That is not
/// stylistic: with a plain `u64` the connection between the check at the top of
/// a function and the division forty lines down exists only in the reader's
/// head, and a later edit that moves one past the other compiles. Here the
/// divisions cannot be written at all without a value that has already been
/// checked -- and, incidentally, `clippy::arithmetic_side_effects` knows the
/// same thing, so the divisions stop being warnings that must be individually
/// excused.
fn sector_size_of(info: &crate::blkdev::BlockDeviceInfo) -> KernelResult<core::num::NonZeroU64> {
    core::num::NonZeroU64::new(u64::from(info.sector_size)).ok_or(KernelError::InvalidArgument)
}

/// Largest single read this filesystem will allocate for a block device.
///
/// A read is allowed to come back short (POSIX says so, and every caller that
/// streams a device already loops), so a cap costs the caller one extra
/// iteration and costs the kernel a bounded allocation. Without one, `read(fd,
/// buf, 4 GiB)` on `/dev/nvme0n1` is a 4 GiB kernel allocation requested by
/// userspace. 8 MiB is comfortably above the 1 MiB chunk the disk imager
/// streams with, so the cap is never reached in the case it was written for.
///
/// **Writes are not capped**, because [`FileSystem::write_at`] returns no byte
/// count and so has no way to say "I wrote less than you asked". A capped write
/// would have to either lie or fail; instead the write loop below chunks
/// internally, keeping the allocation bounded without ever writing part of a
/// request and reporting success.
const MAX_BLOCK_READ: usize = 8 * 1024 * 1024;

/// Sectors per read-modify-write chunk in [`block_write_at`].
///
/// Bounds the staging buffer at `BLOCK_WRITE_CHUNK * SECTOR_SIZE` (512 KiB)
/// regardless of how much the caller passed.
const BLOCK_WRITE_CHUNK: u64 = 1024;

/// Demand a [`ResourceType::BlockDevice`](crate::cap::ResourceType::BlockDevice)
/// capability carrying `rights` before raw sectors move either way.
///
/// # Why the check is here and not in the syscall layer
///
/// Every other capability check in this kernel sits in `syscall/handlers.rs`,
/// and this one deliberately does not. Raw device bytes are reachable through
/// `SYS_FS_READ`, `SYS_FS_WRITE`, Linux `read`/`write`/`pread`/`pwrite`, and any
/// future path that resolves a name and asks the VFS for bytes — and *none* of
/// those sites can tell a block node from a regular file without asking this
/// module. A gate that each caller must remember to invoke, and that requires a
/// lookup here to even evaluate, is a gate with as many holes as it has callers.
/// Putting it at the point where the node type is already known makes "was this
/// checked?" answerable by reading one function instead of auditing every
/// syscall that can produce bytes.
///
/// Kernel-internal callers (the shell, self-tests) have no owning process;
/// [`require_cap_type`](crate::syscall::handlers::require_cap_type) returns `Ok`
/// for those, so this constrains userspace without making the kernel ask
/// permission of itself.
///
/// # Why reads are gated too
///
/// Reading a raw device is weaker than writing one but is not weak: the sectors
/// contain every file the caller could not open through the ordinary path, plus
/// every file that was deleted and not yet overwritten. Handing that out for
/// free would make the filesystem's permission bits advisory. Note that this
/// gates *bytes*, not *names* — `readdir`, `stat` and `/sys`'s block listing ask
/// nothing, which is what lets a program show the user which disks exist without
/// holding the authority to read or erase them.
fn require_block_cap(rights: crate::cap::Rights) -> KernelResult<()> {
    crate::syscall::handlers::require_cap_type(crate::cap::ResourceType::BlockDevice, rights)
}

/// Read `len` bytes at byte `offset` from a block device.
///
/// Requires a `BlockDevice` capability with `READ` — see [`require_block_cap`].
///
/// Byte-granular over a sector-granular device: the covering sector range is
/// read and then sliced. Callers do not have to align, and the disk imager in
/// particular does not — it streams an image whose length is whatever the image
/// is.
///
/// Returns a short buffer at the end of the device and an empty one past it,
/// which is EOF as every streaming caller expects.
fn block_read_at(
    info: &crate::blkdev::BlockDeviceInfo,
    offset: u64,
    len: usize,
) -> KernelResult<Vec<u8>> {
    // Before the EOF short-circuit below, not after: a caller without the
    // capability must not be able to learn the device's size by probing which
    // offsets return empty rather than denied.
    require_block_cap(crate::cap::Rights::READ)?;
    let total = block_size_bytes(info);
    if offset >= total || len == 0 {
        return Ok(Vec::new());
    }
    let sector_size = sector_size_of(info)?;

    let avail = total.saturating_sub(offset);
    let want = (len as u64).min(avail).min(MAX_BLOCK_READ as u64);
    let first_lba = offset / sector_size;
    let skip = (offset % sector_size) as usize;
    // `want >= 1` here, so `offset + want - 1` is the last byte requested.
    let last_lba = offset.saturating_add(want).saturating_sub(1) / sector_size;
    let count = last_lba
        .checked_sub(first_lba)
        .and_then(|n| n.checked_add(1))
        .ok_or(KernelError::InvalidArgument)?;
    let count32 = u32::try_from(count).map_err(|_| KernelError::InvalidArgument)?;
    let staging_len = count
        .checked_mul(sector_size.get())
        .and_then(|n| usize::try_from(n).ok())
        .ok_or(KernelError::InvalidArgument)?;

    let mut staging = vec![0u8; staging_len];
    crate::blkdev::with_device(&info.name, |dev| {
        dev.read_sectors(first_lba, count32, &mut staging)
    })
    .ok_or(KernelError::NotFound)??;

    let want_usize = usize::try_from(want).map_err(|_| KernelError::InvalidArgument)?;
    let end = skip
        .checked_add(want_usize)
        .ok_or(KernelError::InvalidArgument)?;
    let slice = staging.get(skip..end).ok_or(KernelError::InvalidArgument)?;
    Ok(slice.to_vec())
}

/// Write `data` at byte `offset` to a block device, all of it or none of it.
///
/// Read-modify-write on the partial sectors at each end, because a caller
/// writing bytes 10..20 must not lose bytes 0..10 of that sector. The middle is
/// written whole-sector without a preceding read.
///
/// A write that would run off the end of the device fails with
/// [`KernelError::DiskFull`] **before writing anything**. Truncating it instead
/// would be a silent short write that the return type cannot report, and for a
/// caller streaming a disk image that is the difference between a bootable
/// stick and one that fails somewhere in the middle with no error to point at.
fn block_write_at(
    info: &crate::blkdev::BlockDeviceInfo,
    offset: u64,
    data: &[u8],
) -> KernelResult<()> {
    // First, and ahead of the empty-write short-circuit: a zero-length write is
    // the cheapest possible probe for "am I allowed to write this disk", and it
    // should answer honestly rather than succeed vacuously.
    require_block_cap(crate::cap::Rights::WRITE)?;
    if info.read_only {
        return Err(KernelError::ReadOnlyFilesystem);
    }
    if data.is_empty() {
        return Ok(());
    }
    let sector_size = sector_size_of(info)?;
    let total = block_size_bytes(info);
    let end = offset
        .checked_add(data.len() as u64)
        .ok_or(KernelError::InvalidArgument)?;
    if end > total {
        return Err(KernelError::DiskFull);
    }

    let mut written: usize = 0;
    let mut pos = offset;
    while written < data.len() {
        let first_lba = pos / sector_size;
        let skip = (pos % sector_size) as usize;
        let remaining = data.len().saturating_sub(written);
        // How many sectors this chunk spans, capped so the staging buffer is
        // bounded no matter how large `data` is.
        let span_bytes = (skip as u64)
            .checked_add(remaining as u64)
            .ok_or(KernelError::InvalidArgument)?;
        let mut count = span_bytes
            .div_ceil(sector_size.get())
            .min(BLOCK_WRITE_CHUNK);
        if count == 0 {
            count = 1;
        }
        let count32 = u32::try_from(count).map_err(|_| KernelError::InvalidArgument)?;
        let staging_len = count
            .checked_mul(sector_size.get())
            .and_then(|n| usize::try_from(n).ok())
            .ok_or(KernelError::InvalidArgument)?;

        // How many of the caller's bytes land in this chunk.
        let take = remaining.min(staging_len.saturating_sub(skip));
        let tail = staging_len.saturating_sub(skip.saturating_add(take));

        let mut staging = vec![0u8; staging_len];
        // Read first only when the chunk has bytes this write does not
        // replace -- a leading partial sector, a trailing one, or both.
        // A chunk that covers whole sectors end to end needs no read, which
        // is the common case in the middle of a stream.
        if skip != 0 || tail != 0 {
            crate::blkdev::with_device(&info.name, |dev| {
                dev.read_sectors(first_lba, count32, &mut staging)
            })
            .ok_or(KernelError::NotFound)??;
        }
        let src = data
            .get(written..written.saturating_add(take))
            .ok_or(KernelError::InvalidArgument)?;
        let dst = staging
            .get_mut(skip..skip.saturating_add(take))
            .ok_or(KernelError::InvalidArgument)?;
        dst.copy_from_slice(src);

        crate::blkdev::with_device(&info.name, |dev| {
            dev.write_sectors(first_lba, count32, &staging)
        })
        .ok_or(KernelError::NotFound)??;

        written = written.saturating_add(take);
        pos = pos.saturating_add(take as u64);
        if take == 0 {
            // Cannot happen -- `staging_len > skip` always, since `skip` is a
            // remainder of `sector_size` and `staging_len` is a multiple of it
            // and non-zero. Bailing rather than spinning is what makes that
            // reasoning safe to be wrong about.
            return Err(KernelError::InvalidArgument);
        }
    }
    Ok(())
}

/// The entries directly inside `dir` (`""` for the devfs root).
fn children_of(dir: &str) -> Vec<DirEntry> {
    let mut entries: Vec<DirEntry> = DEV_NODES
        .iter()
        .filter(|n| n.parent() == dir)
        .map(|n| DirEntry {
            name: PathBuf::from(n.name()),
            entry_type: n.entry_type,
            // Special files have no meaningful static size.
            size: 0,
        })
        .collect();

    // Block devices live in the root only, and are appended rather than
    // merged in sorted order: the fixed nodes are the ones a caller is most
    // likely to be scanning for, and readdir order is not a promise anywhere
    // in this tree.
    if dir.is_empty() {
        for info in crate::blkdev::list_devices() {
            if find_node(&info.name).is_some() {
                continue; // shadowed by a fixed node; see `find_block`
            }
            entries.push(DirEntry {
                name: PathBuf::from(info.name.as_str()),
                entry_type: EntryType::BlockDevice,
                // Unlike every other node here, this size is real, and it is
                // what `ls -l /dev` and a progress bar both read.
                size: block_size_bytes(&info),
            });
        }
    }
    entries
}

// ---------------------------------------------------------------------------
// FileSystem trait implementation
// ---------------------------------------------------------------------------

/// Strip the leading "/" to get the path relative to the devfs root, and
/// decode.
///
/// This is where devfs stops caring about bytes. Paths are byte strings in
/// general (see [`super::path`]), but devfs's namespace is built entirely by
/// the kernel out of fixed ASCII names, so a path that is not UTF-8 cannot
/// name anything that exists and `NotFound` is the honest answer. A lossy
/// conversion would be strictly worse: it would let a garbage byte alias a
/// real entry. This is the only decode in the module.
fn strip_root(path: &Path) -> KernelResult<&str> {
    let s = path.to_str().ok_or(KernelError::NotFound)?;
    Ok(s.strip_prefix('/').unwrap_or(s))
}

/// The answer for a path that this filesystem does not itself serve.
///
/// A character device node exists — `stat` says so, and `readdir` lists it —
/// but its contents do not come from here: `open` of one is intercepted in the
/// syscall layer, which mints a device handle. Reaching it through the VFS's
/// own read/write is therefore not "no such file" but "not that way", and
/// saying `NotFound` about a path `stat` just confirmed would send whoever hits
/// it looking for a missing entry that is right there.
fn unserved(rel: &str) -> KernelError {
    match find_node(rel) {
        Some(n) if n.entry_type == EntryType::Directory => KernelError::IsADirectory,
        Some(_) => KernelError::NotSupported,
        // A block device *is* served here, just not by the whole-file path
        // that reached this function. Distinguishing it from a missing name
        // matters: `NotFound` for a device `stat` just described would send
        // the caller hunting for an absent node instead of using `read_at`.
        None if find_block(rel).is_some() => KernelError::NotSupported,
        None => KernelError::NotFound,
    }
}

impl FileSystem for DevFs {
    fn fs_type(&self) -> &'static str {
        "devfs"
    }

    fn readdir(&mut self, path: &Path) -> KernelResult<Vec<DirEntry>> {
        let rel = strip_root(path)?;

        if rel.is_empty() {
            return Ok(children_of(""));
        }
        match find_node(rel) {
            Some(node) if node.entry_type == EntryType::Directory => Ok(children_of(rel)),
            Some(_) => Err(KernelError::NotADirectory),
            None if find_block(rel).is_some() => Err(KernelError::NotADirectory),
            None => Err(KernelError::NotFound),
        }
    }

    fn read_file(&mut self, path: &Path) -> KernelResult<Vec<u8>> {
        let rel = strip_root(path)?;

        match rel {
            "" => Err(KernelError::IsADirectory),
            "null" => {
                // /dev/null: read returns empty (EOF).
                Ok(Vec::new())
            }
            "zero" | "full" => {
                // /dev/zero and /dev/full: return a page of zero bytes.
                // Real /dev/zero is infinite; we return a bounded chunk.
                Ok(vec![0u8; 4096])
            }
            "random" | "urandom" => {
                // /dev/random, /dev/urandom: return pseudo-random bytes.
                // No distinction between the two — our PRNG never blocks.
                let mut buf = vec![0u8; 256];
                fill_random(&mut buf);
                Ok(buf)
            }
            "console" | "tty" => {
                // /dev/console, /dev/tty: reading returns whatever is in the
                // keyboard buffer; for now, return empty (non-blocking).
                // /dev/tty is the controlling terminal of the calling process;
                // since we're single-console, it aliases /dev/console.
                Ok(Vec::new())
            }
            _ => Err(unserved(rel)),
        }
    }

    fn read_at(&mut self, path: &Path, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        let rel = strip_root(path)?;

        // For streaming devices, offset is ignored — they always produce
        // fresh data.  This is important for file-handle reads that advance
        // a cursor: reading /dev/zero at offset 8192 should still produce
        // zeros, not EOF.
        match rel {
            "" => Err(KernelError::IsADirectory),
            "null" => Ok(Vec::new()),
            "zero" | "full" => Ok(vec![0u8; len.min(65536)]),
            "random" | "urandom" => {
                let actual = len.min(65536);
                let mut buf = vec![0u8; actual];
                fill_random(&mut buf);
                Ok(buf)
            }
            "console" | "tty" | "stdin" => {
                // Reading from console/tty/stdin returns empty (no interactive input).
                let _ = offset;
                Ok(Vec::new())
            }
            "stdout" | "stderr" => {
                // Reading from stdout/stderr returns empty.
                let _ = offset;
                Ok(Vec::new())
            }
            "kmsg" => {
                // /dev/kmsg: kernel log ring buffer (JSON-lines format).
                // Reads all entries from the klog ring buffer.
                let mut buf = alloc::vec![0u8; 64 * 1024];
                let (written, _last_seq) = crate::klog::read_logs(u64::MAX, &mut buf);
                buf.truncate(written);
                Ok(buf)
            }
            "uptime" => {
                // /dev/uptime: system uptime as a simple decimal string.
                let ns = crate::hpet::elapsed_ns();
                let secs = ns / 1_000_000_000;
                let frac = ns % 1_000_000_000;
                let text = alloc::format!("{secs}.{frac:09}\n");
                Ok(text.into_bytes())
            }
            _ => match find_block(rel) {
                Some(info) => block_read_at(&info, offset, len),
                None => Err(unserved(rel)),
            },
        }
    }

    fn write_file(&mut self, path: &Path, data: &[u8]) -> KernelResult<()> {
        let rel = strip_root(path)?;

        match rel {
            "" => Err(KernelError::IsADirectory),
            "null" => {
                // /dev/null: discard all data silently.
                let _ = data;
                Ok(())
            }
            "zero" => {
                // /dev/zero: writes succeed but data is discarded.
                let _ = data;
                Ok(())
            }
            "full" => {
                // /dev/full: writes always fail with DiskFull.
                // Useful for testing error handling in programs.
                let _ = data;
                Err(KernelError::DiskFull)
            }
            "random" | "urandom" => {
                // /dev/random: writes contribute to entropy pool.
                // Mix user-supplied data into the kernel CSPRNG.
                if !data.is_empty() {
                    let mut hash: u64 = 0;
                    for chunk in data.chunks(8) {
                        let mut buf = [0u8; 8];
                        let len = chunk.len().min(8);
                        if let Some(dest) = buf.get_mut(..len) {
                            if let Some(src) = chunk.get(..len) {
                                dest.copy_from_slice(src);
                            }
                        }
                        hash ^= u64::from_le_bytes(buf);
                    }
                    crate::rng::add_interrupt_entropy(hash);
                }
                Ok(())
            }
            "console" | "tty" | "stdout" | "stderr" => {
                // /dev/console, /dev/tty, stdout, stderr: write to kernel console output.
                if let Ok(text) = core::str::from_utf8(data) {
                    crate::console_print!("{}", text);
                } else {
                    // Binary data — print hex summary.
                    crate::console_print!("[binary: {} bytes]", data.len());
                }
                Ok(())
            }
            "stdin" => {
                // Writing to stdin is a no-op (no input buffer to push into).
                let _ = data;
                Ok(())
            }
            "kmsg" => {
                // Writing to kmsg logs a message (print to serial for now).
                if let Ok(text) = core::str::from_utf8(data) {
                    crate::serial_println!("[kmsg] {}", text.trim_end());
                }
                Ok(())
            }
            "uptime" => {
                // /dev/uptime is read-only.
                Err(KernelError::NotSupported)
            }
            _ => Err(unserved(rel)),
        }
    }

    fn write_at(&mut self, path: &Path, offset: u64, data: &[u8]) -> KernelResult<()> {
        let rel = strip_root(path)?;

        // A block device is the one node here for which the offset means
        // something. Everything else is a stream whose position is not a
        // place, so those keep forwarding to `write_file`.
        if let Some(info) = find_block(rel) {
            return block_write_at(&info, offset, data);
        }
        self.write_file(path, data)
    }

    fn stat(&mut self, path: &Path) -> KernelResult<DirEntry> {
        let rel = strip_root(path)?;

        if rel.is_empty() {
            return Ok(DirEntry {
                name: PathBuf::from("/"),
                entry_type: EntryType::Directory,
                size: 0,
            });
        }

        if let Some(info) = find_block(rel) {
            return Ok(DirEntry {
                name: PathBuf::from(info.name.as_str()),
                entry_type: EntryType::BlockDevice,
                size: block_size_bytes(&info),
            });
        }

        let node = find_node(rel).ok_or(KernelError::NotFound)?;
        Ok(DirEntry {
            name: PathBuf::from(node.name()),
            entry_type: node.entry_type,
            size: 0,
        })
    }

    fn metadata(&mut self, path: &Path) -> KernelResult<FileMeta> {
        let rel = strip_root(path)?;

        if rel.is_empty() {
            return Ok(FileMeta {
                size: 0,
                entry_type: EntryType::Directory,
                permissions: 0o755,
                nlinks: 1,
                blocks: 0,
                ..FileMeta::minimal(EntryType::Directory, 0)
            });
        }

        if let Some(info) = find_block(rel) {
            let size = block_size_bytes(&info);
            return Ok(FileMeta {
                size,
                entry_type: EntryType::BlockDevice,
                // 0o660 for the same reason the character nodes use it: on
                // Linux this is `brw-rw----` owned by group `disk`. We have no
                // groups, so the bits are the honest part. A read-only device
                // drops the write bits, which is not the authority check --
                // that is `CAP_BLOCK_WRITE` in the syscall layer -- but it is
                // what `ls -l` shows and what a program's own pre-flight check
                // reads.
                permissions: if info.read_only { 0o440 } else { 0o660 },
                attributes: FileAttr::NONE,
                nlinks: 1,
                // In 512-byte units, matching `st_blocks` everywhere else.
                blocks: size / 512,
                ..FileMeta::minimal(EntryType::BlockDevice, size)
            });
        }

        let node = find_node(rel).ok_or(KernelError::NotFound)?;
        Ok(FileMeta {
            size: 0,
            entry_type: node.entry_type,
            permissions: node.mode,
            attributes: FileAttr::NONE,
            // A directory here has no `.`/`..` on disk to count, and every
            // other node is a single unlinked-from-nowhere device: one link.
            nlinks: 1,
            blocks: 0,
            ..FileMeta::minimal(node.entry_type, 0)
        })
    }

    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        Ok(FsInfo {
            fs_type: String::from("devfs"),
            volume_label: String::new(),
            block_size: 0,
            total_blocks: 0,
            free_blocks: 0,
            total_inodes: DEV_NODES.len() as u64,
            free_inodes: 0,
            max_name_len: 255,
            read_only: false,
        })
    }

    fn debug_stats(&self) -> String {
        format!("devfs: {} nodes", DEV_NODES.len())
    }
}

// ---------------------------------------------------------------------------
// Mount helper
// ---------------------------------------------------------------------------

/// Mount devfs at the given path (typically `/dev`).
///
/// Takes a path rather than a `&str` (design-decisions.md 261): a mount
/// point is an ordinary directory, whose name may contain any byte but `/`
/// and NUL.
pub fn mount(mount_path: impl AsRef<Path>) -> KernelResult<()> {
    let fs = DevFs::new();
    crate::fs::Vfs::mount(mount_path, alloc::boxed::Box::new(fs))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Test the devfs implementation.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    serial_println!("[devfs] Running self-test...");
    let mut skips = crate::fs::selftest::Skips::new();

    let mut fs = DevFs::new();

    // Test root readdir.  The root holds every node whose path has no slash,
    // plus one per registered block device.
    //
    // Asserted as a *composition* rather than as a total, which is the lesson
    // this assertion taught the moment block devices landed: it compared
    // against the fixed-node count alone, so the first QEMU boot with four real
    // block devices registered failed with "19 entries, expected 15" -- a
    // correct complaint about a listing that was entirely correct.  A total is
    // only as stable as the least stable thing it sums, and the number of disks
    // attached to a machine is not stable at all.  Counting the two populations
    // separately says what is actually meant, and cannot be falsified by
    // plugging in a drive.
    let expect_fixed = DEV_NODES.iter().filter(|n| n.parent().is_empty()).count();
    let entries = fs.readdir(Path::new("/"))?;
    let (blocks, fixed): (Vec<_>, Vec<_>) = entries
        .iter()
        .partition(|e| e.entry_type == EntryType::BlockDevice);
    if fixed.len() != expect_fixed {
        serial_println!(
            "[devfs]   FAIL: readdir returned {} non-block entries, expected {}",
            fixed.len(),
            expect_fixed
        );
        return Err(KernelError::InternalError);
    }
    // Every block entry must name a device that is really registered.  This is
    // the direction a count cannot check: a stale node, or one invented by a
    // bug in `children_of`, keeps the total right while naming nothing.
    for e in &blocks {
        let listed = e.name.as_path().as_bytes();
        let found = core::str::from_utf8(listed)
            .ok()
            .and_then(crate::blkdev::info)
            .is_some();
        if !found {
            serial_println!(
                "[devfs]   FAIL: /{} is listed but no such block device exists",
                e.name.as_path().display()
            );
            return Err(KernelError::InternalError);
        }
    }
    serial_println!(
        "[devfs]   readdir /: {} fixed + {} block device(s) OK",
        fixed.len(),
        blocks.len()
    );

    // Test stat on root.
    let root_stat = fs.stat(Path::new("/"))?;
    if root_stat.entry_type != EntryType::Directory {
        serial_println!("[devfs]   FAIL: stat / not a directory");
        return Err(KernelError::InternalError);
    }

    // Test /dev/null: read returns empty.
    let null_data = fs.read_file(Path::new("/null"))?;
    if !null_data.is_empty() {
        serial_println!("[devfs]   FAIL: /dev/null read should be empty");
        return Err(KernelError::InternalError);
    }
    // Write to null should succeed.
    fs.write_file(Path::new("/null"), b"discarded")?;
    serial_println!("[devfs]   null: read=empty, write=discard OK");

    // Test /dev/zero: read returns zeros.
    let zero_data = fs.read_file(Path::new("/zero"))?;
    if zero_data.is_empty() {
        serial_println!("[devfs]   FAIL: /dev/zero read should not be empty");
        return Err(KernelError::InternalError);
    }
    if zero_data.iter().any(|&b| b != 0) {
        serial_println!("[devfs]   FAIL: /dev/zero data contains non-zero bytes");
        return Err(KernelError::InternalError);
    }
    serial_println!("[devfs]   zero: {} zero bytes OK", zero_data.len());

    // Test /dev/full: read returns zeros, write fails with DiskFull.
    let full_data = fs.read_file(Path::new("/full"))?;
    if full_data.is_empty() {
        serial_println!("[devfs]   FAIL: /dev/full read should not be empty");
        return Err(KernelError::InternalError);
    }
    if full_data.iter().any(|&b| b != 0) {
        serial_println!("[devfs]   FAIL: /dev/full data contains non-zero bytes");
        return Err(KernelError::InternalError);
    }
    match fs.write_file(Path::new("/full"), b"should fail") {
        Err(KernelError::DiskFull) => {}
        other => {
            serial_println!(
                "[devfs]   FAIL: /dev/full write should return DiskFull, got {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[devfs]   full: read=zeros, write=DiskFull OK");

    // Test /dev/random: read returns data, two reads differ.
    let rand1 = fs.read_file(Path::new("/random"))?;
    let rand2 = fs.read_file(Path::new("/random"))?;
    if rand1.is_empty() || rand2.is_empty() {
        serial_println!("[devfs]   FAIL: /dev/random read should not be empty");
        return Err(KernelError::InternalError);
    }
    if rand1 == rand2 {
        serial_println!("[devfs]   FAIL: two /dev/random reads should differ");
        return Err(KernelError::InternalError);
    }
    // Write to random (entropy contribution) should succeed.
    fs.write_file(Path::new("/random"), b"entropy seed")?;
    serial_println!(
        "[devfs]   random: {} random bytes, entropy write OK",
        rand1.len()
    );

    // Test /dev/urandom: same behavior as /dev/random.
    let urand = fs.read_file(Path::new("/urandom"))?;
    if urand.is_empty() {
        serial_println!("[devfs]   FAIL: /dev/urandom read should not be empty");
        return Err(KernelError::InternalError);
    }
    fs.write_file(Path::new("/urandom"), b"more entropy")?;
    serial_println!("[devfs]   urandom: {} random bytes OK", urand.len());

    // Test read_at on /dev/zero — should always return zeros regardless of offset.
    let zero_at = fs.read_at(Path::new("/zero"), 99999, 64)?;
    if zero_at.len() != 64 || zero_at.iter().any(|&b| b != 0) {
        serial_println!("[devfs]   FAIL: /dev/zero read_at should return 64 zero bytes");
        return Err(KernelError::InternalError);
    }
    serial_println!("[devfs]   read_at /dev/zero: offset-independent OK");

    // Test /dev/console: write should succeed (outputs to console).
    fs.write_file(Path::new("/console"), b"[devfs]   console write test\n")?;
    serial_println!("[devfs]   console: write OK");

    // Test nonexistent device.
    if fs.stat(Path::new("/nonexistent")).is_ok() {
        serial_println!("[devfs]   FAIL: stat /nonexistent should fail");
        return Err(KernelError::InternalError);
    }
    serial_println!("[devfs]   stat /nonexistent: NotFound OK");

    // ------------------------------------------------------------------
    // Device-node subdirectories.
    //
    // This is the whole reason the node table exists, so it is tested as a
    // client would use it: scan the directory, then stat what you found and
    // check it is a character device.  A regression here does not break a
    // kernel test -- it makes libinput report no input devices on a machine
    // whose keyboard works, which is a very long way from the cause.
    // ------------------------------------------------------------------
    for (dir, want) in [("/input", 2usize), ("/dri", 2), ("/snd", 3)] {
        let listing = fs.readdir(Path::new(dir))?;
        if listing.len() != want {
            serial_println!(
                "[devfs]   FAIL: readdir {} returned {} entries, expected {}",
                dir,
                listing.len(),
                want
            );
            return Err(KernelError::InternalError);
        }
        for ent in &listing {
            if ent.entry_type != EntryType::CharDevice {
                serial_println!(
                    "[devfs]   FAIL: {}/{} is not a character device",
                    dir,
                    ent.name.display()
                );
                return Err(KernelError::InternalError);
            }
            // The name readdir reports must be a bare component, and the
            // path built from it must stat back to the same thing -- that
            // round trip is exactly what a scanning client performs.
            let mut full = String::from(dir);
            full.push('/');
            full.push_str(&alloc::format!("{}", ent.name.display()));
            let st = fs.stat(Path::new(&full))?;
            if st.entry_type != EntryType::CharDevice {
                serial_println!("[devfs]   FAIL: stat {} is not a char device", full);
                return Err(KernelError::InternalError);
            }
            let meta = fs.metadata(Path::new(&full))?;
            if meta.entry_type != EntryType::CharDevice || meta.permissions != 0o660 {
                serial_println!(
                    "[devfs]   FAIL: metadata {} is {:?}/{:o}, expected CharDevice/660",
                    full,
                    meta.entry_type,
                    meta.permissions
                );
                return Err(KernelError::InternalError);
            }
            // Reading one through the VFS must say "not that way", never
            // "no such file" -- `stat` just said it is there.
            match fs.read_file(Path::new(&full)) {
                Err(KernelError::NotSupported) => {}
                other => {
                    serial_println!(
                        "[devfs]   FAIL: read {} should be NotSupported, got {:?}",
                        full,
                        other.map(|v| v.len())
                    );
                    return Err(KernelError::InternalError);
                }
            }
        }
        // The directory itself stats as a directory, and reading it as a file
        // is IsADirectory rather than NotFound.
        if fs.stat(Path::new(dir))?.entry_type != EntryType::Directory {
            serial_println!("[devfs]   FAIL: stat {} is not a directory", dir);
            return Err(KernelError::InternalError);
        }
        match fs.read_file(Path::new(dir)) {
            Err(KernelError::IsADirectory) => {}
            _ => {
                serial_println!("[devfs]   FAIL: read {} should be IsADirectory", dir);
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[devfs]   {}: {} char devices OK", dir, want);
    }

    // A plain file is not a directory, and a missing directory is not found:
    // the two failure modes must stay distinguishable, because "readdir said
    // NotADirectory" is how a client learns to stop descending.
    match fs.readdir(Path::new("/null")) {
        Err(KernelError::NotADirectory) => {}
        _ => {
            serial_println!("[devfs]   FAIL: readdir /null should be NotADirectory");
            return Err(KernelError::InternalError);
        }
    }
    match fs.readdir(Path::new("/nosuchdir")) {
        Err(KernelError::NotFound) => {}
        _ => {
            serial_println!("[devfs]   FAIL: readdir /nosuchdir should be NotFound");
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[devfs]   readdir /null=NotADirectory, /nosuchdir=NotFound OK");

    // Through the mount, not through a detached instance.
    //
    // Same reasoning as the sysfs and procfs blocks: everything above drives a
    // bare `DevFs`, so the VFS never holds this filesystem's per-mount lock
    // while one of its handlers runs. devfs has no handler that re-enters the
    // VFS today, and this block is how that stays true -- a future `/dev` node
    // backed by a VFS query (a `/dev/disk/by-uuid` symlink farm is the obvious
    // one) would deadlock here, in a suite, rather than in whatever first ran
    // `ls /dev`.
    // Ask the mount table, not `stat("/dev").is_ok()`. The latter reads as "is
    // /dev mounted" but means "did stat fail for any reason at all" -- so a
    // permission gate wrongly denying `/dev`, or a lookup bug, would have
    // switched off precisely the block that exists to notice it.
    let dev_mounted = crate::fs::Vfs::mounts()
        .iter()
        .any(|(p, _)| p.as_path() == Path::new("/dev"));
    if dev_mounted {
        let entries = crate::fs::Vfs::readdir("/dev")?;
        // /dev/null reads as EOF. Going through the VFS exercises the mount
        // lookup and the per-mount lock, which the direct calls above do not.
        let nul = crate::fs::Vfs::read_file("/dev/null")?;
        if !nul.is_empty() {
            serial_println!("[devfs]   FAIL: /dev/null read {} bytes, want 0", nul.len());
            return Err(KernelError::InternalError);
        }
        serial_println!(
            "[devfs]   through-the-mount: readdir {} entries, /dev/null EOF, no self-deadlock OK",
            entries.len()
        );
    } else {
        skips.record("through-the-mount readdir", "/dev is not mounted");
        serial_println!("[devfs]   through-the-mount: /dev not mounted, skipped");
    }

    // ---- Block devices -------------------------------------------------
    //
    // Over a scratch `RamBlockDevice`, never a real disk: every assertion below
    // writes, and the whole point of this node type is that a write to it is
    // unrecoverable.  The names are registered and unregistered inside this
    // block so a failed assertion cannot leave `/dev` with a device in it.
    //
    // The capability gate is transparent here -- a self-test runs as a bare
    // kernel task with no owning process, and `require_cap_type` passes those
    // through.  That is deliberate and is not a hole in the coverage: the gate's
    // own plumbing (that `BlockDevice` round-trips the wire ABI, and that
    // `admin` grants it) is checked by `cap::groups`' two tests, which walk
    // `1..=ResourceType::LAST` and so covered this type the moment it existed.
    // What this block checks is the part those cannot: the I/O underneath.
    {
        const SCRATCH: &str = "selftestblk0";
        const SECTORS: u64 = 64;
        const CAP: u64 = SECTORS.saturating_mul(512);

        crate::blkdev::register(
            SCRATCH,
            Box::new(crate::blkdev::RamBlockDevice::new(SECTORS)),
        );
        let result = self_test_block(&mut fs, SCRATCH, CAP);
        crate::blkdev::unregister(SCRATCH);
        result?;

        // A device may not shadow a fixed node.  Registering one called `null`
        // is the worst case available: `/dev/null` silently becoming a disk
        // means every program that discards output starts overwriting sectors.
        //
        // Asserted as "the disk did not win" rather than as a specific type,
        // because the fixed node's own type belongs to DEV_NODES and not to
        // this test.  The first version demanded `CharDevice` and failed on a
        // correct tree: `null` is declared with `DevNode::file`, so it stats as
        // `File`.  Pinning the neighbouring value would have made this rung
        // fail again the day `null` is retyped -- which it arguably should be,
        // see A-DEVFS-NULL-AND-ZERO-STAT-AS-REGULAR-FILES.
        crate::blkdev::register("null", Box::new(crate::blkdev::RamBlockDevice::new(8)));
        let shadowed = fs.stat(Path::new("/null")).map(|m| m.entry_type);
        let nul_len = fs.read_file(Path::new("/null")).map(|v| v.len());
        crate::blkdev::unregister("null");
        let fixed_won = matches!(shadowed, Ok(t) if t != EntryType::BlockDevice);
        if !fixed_won || nul_len != Ok(0) {
            serial_println!(
                "[devfs]   FAIL: a block device named `null` shadowed /dev/null \
                 (type={shadowed:?}, read={nul_len:?})"
            );
            return Err(KernelError::InternalError);
        }

        // Read-only devices refuse writes before touching anything.
        crate::blkdev::register(
            SCRATCH,
            Box::new(crate::blkdev::RamBlockDevice::new_read_only(SECTORS)),
        );
        let ro = fs.write_at(Path::new("/selftestblk0"), 0, &[1u8; 4]);
        crate::blkdev::unregister(SCRATCH);
        if ro != Err(KernelError::ReadOnlyFilesystem) {
            serial_println!("[devfs]   FAIL: write to a read-only device gave {ro:?}");
            return Err(KernelError::InternalError);
        }

        serial_println!(
            "[devfs]   block devices: stat/unaligned rw/RMW/EOF/DiskFull, no shadowing, \
             read-only refused OK"
        );
    }

    skips.report("[devfs]");
    serial_println!("[devfs] Self-test PASSED{}", skips.suffix());
    Ok(())
}

/// The writable half of the block-device rung, split out so its caller can
/// [`crate::blkdev::unregister`] the scratch device on every exit path.
///
/// Inlining this would mean an early `return Err` on a failed assertion leaves
/// `selftestblk0` registered, and the next test to list `/dev` would find a
/// device that no longer has an owner -- turning one failure into a confusing
/// second one somewhere else.
fn self_test_block(fs: &mut DevFs, name: &str, cap: u64) -> KernelResult<()> {
    use crate::serial_println;

    let path_buf = alloc::format!("/{name}");
    let path = Path::new(path_buf.as_str());

    // Named, sized, and typed.
    let meta = fs.stat(path)?;
    if meta.entry_type != EntryType::BlockDevice || meta.size != cap {
        serial_println!(
            "[devfs]   FAIL: stat /{name} gave {:?} size {}, want BlockDevice size {cap}",
            meta.entry_type,
            meta.size
        );
        return Err(KernelError::InternalError);
    }
    if !fs
        .readdir(Path::new("/"))?
        .iter()
        .any(|e| e.name.as_path() == Path::new(name) && e.entry_type == EntryType::BlockDevice)
    {
        serial_println!("[devfs]   FAIL: /{name} is missing from the root listing");
        return Err(KernelError::InternalError);
    }

    // Unaligned round-trip straddling the 512-byte sector boundary, which is
    // the case byte-granular access over a sector-granular device exists for.
    // 500..510 lies in sector 0; 505..515 crosses into sector 1.
    let pattern: [u8; 15] = [
        0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD, 0xAE,
    ];
    fs.write_at(path, 505, &pattern)?;
    let got = fs.read_at(path, 505, pattern.len())?;
    if got != pattern {
        serial_println!("[devfs]   FAIL: unaligned cross-sector round-trip gave {got:?}");
        return Err(KernelError::InternalError);
    }

    // Read-modify-write must preserve the bytes it did not write.  Byte 504 is
    // in the same sector as the write above and one byte before it; it was
    // never written, so it must still be zero.  A write path that read the
    // sector, ignored it, and wrote the whole thing back would pass every
    // assertion above and fail this one.
    let neighbour = fs.read_at(path, 504, 1)?;
    if neighbour != [0u8] {
        serial_println!("[devfs]   FAIL: RMW clobbered byte 504: {neighbour:?}");
        return Err(KernelError::InternalError);
    }

    // Short at the end, empty past it -- EOF as a streaming caller expects.
    let tail = fs.read_at(path, cap.saturating_sub(4), 64)?;
    if tail.len() != 4 {
        serial_println!(
            "[devfs]   FAIL: read at EOF-4 gave {} bytes, want 4",
            tail.len()
        );
        return Err(KernelError::InternalError);
    }
    if !fs.read_at(path, cap, 16)?.is_empty() {
        serial_println!("[devfs]   FAIL: read past the end returned bytes");
        return Err(KernelError::InternalError);
    }

    // A write that would run off the end fails whole, and does not write the
    // part that fits.  Checking byte `cap - 4` afterwards is what distinguishes
    // "refused" from "wrote four bytes and then reported an error".
    let over = fs.write_at(path, cap.saturating_sub(4), &[0xFFu8; 8]);
    if over != Err(KernelError::DiskFull) {
        serial_println!("[devfs]   FAIL: write past the end gave {over:?}, want DiskFull");
        return Err(KernelError::InternalError);
    }
    if fs.read_at(path, cap.saturating_sub(4), 4)? != [0u8; 4] {
        serial_println!("[devfs]   FAIL: a refused write still wrote its first bytes");
        return Err(KernelError::InternalError);
    }

    // Whole-device operations are refused rather than served.
    match fs.read_file(path) {
        Err(KernelError::NotSupported) => {}
        other => {
            serial_println!(
                "[devfs]   FAIL: read_file on a block node gave {:?}, want NotSupported",
                other.map(|v| v.len())
            );
            return Err(KernelError::InternalError);
        }
    }
    match fs.readdir(path) {
        Err(KernelError::NotADirectory) => {}
        other => {
            serial_println!(
                "[devfs]   FAIL: readdir on a block node gave {:?}, want NotADirectory",
                other.map(|v| v.len())
            );
            return Err(KernelError::InternalError);
        }
    }

    Ok(())
}
