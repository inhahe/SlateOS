//! Read-only F2FS (Flash-Friendly File System) driver.
//!
//! F2FS is a log-structured filesystem designed for devices with a flash
//! translation layer: it never overwrites a block in place, so every update
//! appends and the map from "which block holds this node" to "where that block
//! is right now" has to be a separate, explicitly-versioned structure. That
//! structure — the NAT — is the reason an F2FS reader looks different from an
//! ext4 or Btrfs reader, and it is where most of this driver's care goes.
//!
//! # The four things a reader must get right
//!
//! 1. **The superblock** is at a fixed *byte* offset (1024), duplicated in the
//!    next block. It names the five areas — checkpoint, SIT, NAT, SSA, main —
//!    by starting block. A read-only driver needs three of the five: the SIT
//!    (which segments are in use) and the SSA (which node owns each block) are
//!    write-side and recovery-side bookkeeping.
//!
//! 2. **The checkpoint** exists as *two* packs, at `cp_blkaddr` and one segment
//!    later, written alternately. The higher version wins — but only if it is
//!    complete, and completeness is decided by the version being written into
//!    both the first *and* last block of the pack. A pack whose head and tail
//!    disagree was interrupted mid-write, and its higher version is precisely
//!    what makes it dangerous: taking it would mount a checkpoint that was
//!    never committed. See [`cp::read_checkpoint`].
//!
//! 3. **The NAT** maps a node id to the block holding that node. Each NAT
//!    entry has two copies on disk, and a bitmap in the checkpoint says which
//!    copy is current. Worse, the checkpoint carries a *journal* of up to 38
//!    NAT overrides that were not yet folded into the NAT area — and those
//!    override the area unconditionally. A reader that consults the area
//!    without the journal reads stale block addresses for exactly the files
//!    that were written most recently. See [`node::Nat::lookup`].
//!
//! 4. **The node block** it lands on is checked by nothing but its own footer,
//!    which repeats the nid it is supposed to be. So the footer check is not a
//!    nicety — it is the only thing standing between a mis-derived NAT address
//!    and a block of unrelated bytes parsed as an inode.
//!
//! # Scope: read-only, on purpose
//!
//! Writing to a log-structured filesystem means allocating from segments,
//! maintaining the SIT's valid-block bitmaps, running garbage collection to
//! reclaim partially-valid segments, and committing checkpoints atomically —
//! a bug in any of which corrupts a volume that was fine before we touched it.
//! Read support is separately useful (mounting an existing Linux volume) and
//! cannot lose data. Matches the NTFS and Btrfs drivers; see
//! `design-decisions.md`.

// Most of the on-disk vocabulary — the feature flags, the checkpoint flags,
// the file-type bytes — exists because the format defines it, not because this
// read path happens to reach it, and a constant that is only named when a
// volume in the wild uses that feature is still the right thing to have
// written down. The self-test drives most of the rest, but that is invisible
// to dead-code analysis. Matches `fs::ntfs` and `fs::btrfs`.
#![allow(dead_code)]

pub mod cp;
pub mod dir;
pub mod node;
pub mod raw;
pub mod sb;
pub mod tests;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::blocksrc::{DeviceSource, SectorSource};
use crate::fs::path::{Path, PathBuf};
use crate::fs::vfs::{DirEntry, EntryType, FileMeta, FileSystem, FsInfo};
use crate::serial_println;

use cp::Checkpoint;
use node::{Inode, Nat};
use raw::{BLOCK_SIZE, F2FS_FT_DIR, F2FS_FT_SYMLINK};
use sb::SuperBlock;

/// Refuse to slurp a file larger than this into a single `Vec`.
///
/// Matches the NTFS and Btrfs drivers' cap. `read_file` has no streaming form
/// in the VFS trait, so the only alternative to a cap is a kernel allocation
/// the size of whatever file a caller names.
const MAX_READ_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// A mounted, read-only F2FS volume.
///
/// # Synchronisation
///
/// None internally: the VFS stores each filesystem behind its own mutex, so
/// only one caller is inside these methods at a time. The struct is otherwise
/// immutable after `open_source` — the superblock and the winning checkpoint
/// are resolved once at mount and never change, because a read-only mount
/// never writes a new checkpoint and nothing else can move the one we chose.
pub struct F2fsFs {
    src: Box<dyn SectorSource>,
    sb: SuperBlock,
    cp: Checkpoint,
    /// Device name, when mounted from a real device rather than memory.
    device: Option<String>,
    /// VFS operations served, for `debug_stats`.
    ops: u64,
}

impl F2fsFs {
    /// Open a volume from any sector source.
    ///
    /// Three steps, in the only order they can happen: the superblock names
    /// where the checkpoint area is, the checkpoint says which half of the NAT
    /// is live, and the root inode is then reachable. Everything after mount is
    /// a lookup through those three.
    ///
    /// The root inode is read *here*, and the mount fails if it cannot be read
    /// or is not a directory, which is what Linux's `f2fs_fill_super` does.
    /// Deferring it to the first operation is cheaper and wrong for two
    /// reasons. A successful mount is a claim: it publishes a VFS entry,
    /// shadows whatever directory was at the mount point, and tells every
    /// caller the volume is usable — so returning success for a volume whose
    /// checkpoint selects an unreadable NAT half makes the kernel assert
    /// something it has not checked and cannot honour. And it destroys
    /// attribution: the fault then surfaces as an error from some later
    /// `open()` that has nothing obviously to do with the mount, instead of
    /// from the operation that actually had the evidence.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` for a superblock that is not F2FS or whose geometry
    /// does not hold together, `NotSupported` for a feature this driver cannot
    /// honour without silently returning wrong bytes, and `CorruptedData` for
    /// a checkpoint that neither pack could supply or a root inode that is
    /// unreachable or is not a directory.
    pub fn open_source(src: Box<dyn SectorSource>) -> KernelResult<Self> {
        let sb = sb::read_superblock(src.as_ref())?;
        let cp = cp::read_checkpoint(src.as_ref(), &sb)?;
        let fs = Self {
            src,
            sb,
            cp,
            device: None,
            ops: 0,
        };

        let root = fs.inode(fs.sb.root_ino)?;
        if !root.is_dir() {
            serial_println!(
                "[f2fs] Root inode {} is not a directory (mode {:#o}).",
                fs.sb.root_ino,
                root.mode
            );
            return Err(KernelError::CorruptedData);
        }
        Ok(fs)
    }

    /// Open the F2FS volume on a named block device.
    ///
    /// # Errors
    ///
    /// Propagates [`F2fsFs::open_source`].
    pub fn open(device: &str) -> KernelResult<Self> {
        let mut fs = Self::open_source(Box::new(DeviceSource::new(device)))?;
        fs.device = Some(String::from(device));
        Ok(fs)
    }

    /// A NAT bound to this volume's source, superblock and checkpoint.
    fn nat(&self) -> Nat<'_> {
        Nat::new(self.src.as_ref(), &self.sb, &self.cp)
    }

    /// Read one inode by node id.
    fn inode(&self, ino: u32) -> KernelResult<Inode> {
        node::read_inode(&self.nat(), &self.sb, ino)
    }

    /// Resolve a VFS path to an inode number and its metadata.
    ///
    /// `..` unwinds a stack of the inodes actually walked through rather than
    /// consulting each inode's `pino`. That is not a shortcut — it is the
    /// correct answer: `pino` records one parent, and for a hard-linked file
    /// it records whichever directory happened to be last, whereas what `..`
    /// must undo is the path the caller wrote.
    ///
    /// A trailing symlink is deliberately *not* followed, which is what lets
    /// `lstat` delegate to `stat` and lets `readlink` see the link itself.
    fn resolve(&self, path: &Path) -> KernelResult<(u32, Inode)> {
        let root = self.sb.root_ino;
        let mut stack: Vec<u32> = Vec::new();
        let mut ino = root;
        let mut inode = self.inode(ino)?;

        for comp in path.components() {
            let name = comp.as_bytes();
            if name == b"." {
                continue;
            }
            if name == b".." {
                ino = stack.pop().unwrap_or(root);
                inode = self.inode(ino)?;
                continue;
            }
            if !inode.is_dir() {
                return Err(KernelError::NotADirectory);
            }

            let entry =
                dir::lookup(&self.nat(), &self.sb, &inode, name)?.ok_or(KernelError::NotFound)?;
            stack.push(ino);
            ino = entry.ino;
            inode = self.inode(ino)?;
        }

        Ok((ino, inode))
    }

    /// Read `len` bytes of a file starting at `offset`.
    ///
    /// Clamped to the inode's size, because the last block of a file is a
    /// whole block on disk and the bytes past `i_size` in it are whatever the
    /// allocator left there. Holes and preallocated-but-unwritten blocks read
    /// as zeroes, which [`node::read_file_block`] already guarantees, so
    /// nothing here needs a special case for them.
    fn read_range(&self, inode: &Inode, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        if offset >= inode.size || len == 0 {
            return Ok(Vec::new());
        }
        let avail = inode.size.saturating_sub(offset);
        let want = u64::try_from(len)
            .map_err(|_| KernelError::InvalidArgument)?
            .min(avail);
        let want_usize = usize::try_from(want).map_err(|_| KernelError::FileTooLarge)?;

        // An inline file lives entirely inside its own inode block; there is
        // no block pointer to resolve and the size is bounded by the inline
        // area, so it is served straight out of the inode.
        if inode.has_inline_data() {
            let area = inode.inline_area()?;
            let start = usize::try_from(offset).map_err(|_| KernelError::FileTooLarge)?;
            let mut out = vec![0u8; want_usize];
            if let Some(src) = area.get(start..) {
                let n = src.len().min(want_usize);
                out.get_mut(..n)
                    .ok_or(KernelError::InternalError)?
                    .copy_from_slice(src.get(..n).ok_or(KernelError::InternalError)?);
            }
            return Ok(out);
        }

        let block_size = u64::try_from(BLOCK_SIZE).map_err(|_| KernelError::InternalError)?;
        let mut out = Vec::with_capacity(want_usize);
        let mut pos = offset;
        let end = offset
            .checked_add(want)
            .ok_or(KernelError::InvalidArgument)?;

        while pos < end {
            let index = pos
                .checked_div(block_size)
                .ok_or(KernelError::InternalError)?;
            let within = usize::try_from(pos.checked_rem(block_size).unwrap_or(0))
                .map_err(|_| KernelError::InternalError)?;
            let take = usize::try_from(end.saturating_sub(pos))
                .unwrap_or(BLOCK_SIZE)
                .min(BLOCK_SIZE.saturating_sub(within));

            let block = node::read_file_block(&self.nat(), &self.sb, inode, index)?;
            let slice = block
                .get(within..within.checked_add(take).ok_or(KernelError::InternalError)?)
                .ok_or(KernelError::CorruptedData)?;
            out.extend_from_slice(slice);

            pos = pos
                .checked_add(u64::try_from(take).map_err(|_| KernelError::InternalError)?)
                .ok_or(KernelError::InternalError)?;
        }

        Ok(out)
    }

    /// Map an inode's mode to a VFS entry type.
    fn entry_type_of(inode: &Inode) -> EntryType {
        if inode.is_dir() {
            EntryType::Directory
        } else if inode.is_symlink() {
            EntryType::Symlink
        } else {
            EntryType::File
        }
    }

    /// The parsed superblock.
    pub const fn superblock(&self) -> &SuperBlock {
        &self.sb
    }

    /// The checkpoint this mount is reading against.
    pub const fn checkpoint(&self) -> &Checkpoint {
        &self.cp
    }
}

impl FileSystem for F2fsFs {
    // The trait fixes the signature as `&self -> &str`; narrowing this impl to
    // `&'static str` would no longer implement it.
    #[allow(clippy::unnecessary_literal_bound)]
    fn fs_type(&self) -> &str {
        "f2fs"
    }

    fn readdir(&mut self, path: &Path) -> KernelResult<Vec<DirEntry>> {
        self.ops = self.ops.saturating_add(1);
        let (_, inode) = self.resolve(path)?;
        if !inode.is_dir() {
            return Err(KernelError::NotADirectory);
        }

        let entries = dir::read_dir(&self.nat(), &self.sb, &inode)?;
        let mut out = Vec::with_capacity(entries.len());
        for entry in entries {
            // `.` and `..` are the VFS's business, not a filesystem's; every
            // other driver here omits them and callers rely on that.
            if entry.name == b"." || entry.name == b".." {
                continue;
            }
            let entry_type = match entry.file_type {
                F2FS_FT_DIR => EntryType::Directory,
                F2FS_FT_SYMLINK => EntryType::Symlink,
                _ => EntryType::File,
            };
            // A directory's size is meaningless to a caller and costs a NAT
            // lookup plus a block read to obtain, so only files pay for one.
            let size = if entry_type == EntryType::File {
                self.inode(entry.ino).map(|i| i.size).unwrap_or(0)
            } else {
                0
            };
            out.push(DirEntry {
                name: PathBuf::from(entry.name.as_slice()),
                entry_type,
                size,
            });
        }
        Ok(out)
    }

    fn read_file(&mut self, path: &Path) -> KernelResult<Vec<u8>> {
        self.ops = self.ops.saturating_add(1);
        let (_, inode) = self.resolve(path)?;
        if inode.is_dir() {
            return Err(KernelError::IsADirectory);
        }
        if inode.size > MAX_READ_FILE_BYTES {
            return Err(KernelError::FileTooLarge);
        }
        let len = usize::try_from(inode.size).map_err(|_| KernelError::FileTooLarge)?;
        self.read_range(&inode, 0, len)
    }

    fn read_at(&mut self, path: &Path, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        self.ops = self.ops.saturating_add(1);
        let (_, inode) = self.resolve(path)?;
        if inode.is_dir() {
            return Err(KernelError::IsADirectory);
        }
        self.read_range(&inode, offset, len)
    }

    fn stat(&mut self, path: &Path) -> KernelResult<DirEntry> {
        self.ops = self.ops.saturating_add(1);
        let (_, inode) = self.resolve(path)?;
        let entry_type = Self::entry_type_of(&inode);
        let name = path
            .file_name()
            .map_or_else(|| PathBuf::from("/"), Path::to_path_buf);
        Ok(DirEntry {
            name,
            entry_type,
            size: if entry_type == EntryType::Directory {
                0
            } else {
                inode.size
            },
        })
    }

    fn lstat(&mut self, path: &Path) -> KernelResult<DirEntry> {
        // `resolve` does not follow a trailing symlink, so `stat` already
        // reports the link itself and the two agree.
        self.stat(path)
    }

    fn metadata(&mut self, path: &Path) -> KernelResult<FileMeta> {
        self.ops = self.ops.saturating_add(1);
        let (ino, inode) = self.resolve(path)?;
        let entry_type = Self::entry_type_of(&inode);
        let size = if entry_type == EntryType::Directory {
            0
        } else {
            inode.size
        };

        // F2FS stores a real POSIX mode, so it is reported as-is — with the
        // write bits masked off, because the mount refuses every write and a
        // mode that says otherwise is a lie userspace will act on.
        let permissions = (inode.mode & 0o777) & 0o555;
        let ns = |t: (u64, u32)| {
            t.0.saturating_mul(1_000_000_000)
                .saturating_add(u64::from(t.1))
        };

        Ok(FileMeta {
            size,
            entry_type,
            ino: u64::from(ino),
            created_ns: ns(inode.ctime),
            modified_ns: ns(inode.mtime),
            accessed_ns: ns(inode.atime),
            changed_ns: ns(inode.ctime),
            permissions,
            nlinks: inode.links.max(1),
            blocks: inode.blocks,
            ..FileMeta::minimal(entry_type, size)
        })
    }

    fn lmetadata(&mut self, path: &Path) -> KernelResult<FileMeta> {
        self.metadata(path)
    }

    fn readlink(&mut self, path: &Path) -> KernelResult<PathBuf> {
        self.ops = self.ops.saturating_add(1);
        let (_, inode) = self.resolve(path)?;
        if !inode.is_symlink() {
            return Err(KernelError::InvalidArgument);
        }
        // F2FS stores a symlink target exactly like file contents — a short
        // one inline in the inode block, a long one in data blocks — so this
        // is a read, not a special case.
        let len = usize::try_from(inode.size).map_err(|_| KernelError::FileTooLarge)?;
        let target = self.read_range(&inode, 0, len)?;
        Ok(PathBuf::from(target.as_slice()))
    }

    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        Ok(FsInfo {
            fs_type: String::from("f2fs"),
            volume_label: self.sb.label.clone(),
            block_size: u64::try_from(BLOCK_SIZE).unwrap_or(4096),
            total_blocks: self.cp.user_block_count,
            // The checkpoint knows how many blocks are valid, so free space is
            // computable here — unlike on Btrfs. It is still reported as 0,
            // because a mount that refuses every write has no free space to
            // offer, and a non-zero figure invites a caller to try.
            free_blocks: 0,
            total_inodes: 0,
            free_inodes: u64::from(self.cp.valid_inode_count),
            max_name_len: u64::try_from(raw::F2FS_NAME_LEN).unwrap_or(255),
            read_only: true,
        })
    }

    fn device_name(&self) -> Option<&str> {
        self.device.as_deref()
    }

    fn debug_stats(&self) -> String {
        alloc::format!(
            "F2FS: label='{}', block=4096B, seg={}blk, cp_ver={}, main={:#x}, nat={:#x}, \
             nodes={}, inodes={}, journal={}, ops={}",
            self.sb.label,
            self.sb.blocks_per_seg,
            self.cp.version,
            self.sb.main_blkaddr,
            self.sb.nat_blkaddr,
            self.cp.valid_node_count,
            self.cp.valid_inode_count,
            self.cp.nat_journal.len(),
            self.ops,
        )
    }
}

// ---------------------------------------------------------------------------
// Mount / probe
// ---------------------------------------------------------------------------

/// Mount the F2FS volume on `device` at `mount_path`.
///
/// # Errors
///
/// Propagates [`F2fsFs::open`] and [`crate::fs::Vfs::mount`] failures.
pub fn mount(device: &str, mount_path: impl AsRef<crate::fs::path::Path>) -> KernelResult<()> {
    let mount_path = mount_path.as_ref();
    let fs = F2fsFs::open(device)?;
    crate::fs::Vfs::mount(mount_path, Box::new(fs))?;
    serial_println!(
        "[f2fs] Mounted {} at {} (read-only)",
        device,
        mount_path.display()
    );
    Ok(())
}

/// Whether `device` holds an F2FS volume.
///
/// Reads only the magic, in both superblock copies. A probe is allowed to be
/// cheaper and more permissive than a mount — it answers "should this driver
/// be asked?", and the mount is where every other validation happens.
pub fn probe(device: &str) -> bool {
    sb::probe(&DeviceSource::new(device))
}

/// Run the F2FS self-tests.
///
/// # Errors
///
/// Propagates the first failing check.
pub fn self_test() -> KernelResult<()> {
    tests::self_test()
}
