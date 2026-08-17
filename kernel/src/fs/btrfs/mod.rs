//! Read-only Btrfs driver.
//!
//! Btrfs is a copy-on-write filesystem in which *everything* — including the
//! map from logical to physical addresses — is stored in B-trees keyed by
//! `(objectid, type, offset)`. That uniformity is what makes the driver
//! tractable: one node parser and one key search serve the chunk tree, the
//! root tree and every filesystem tree.
//!
//! # The bootstrap, which is the part that is genuinely awkward
//!
//! Every tree block is addressed *logically*, and logical addresses are
//! resolved through the chunk tree — which is itself a tree, at a logical
//! address. Reading the chunk tree therefore requires the chunk tree.
//!
//! Btrfs breaks the cycle by copying the handful of chunk items that cover the
//! metadata region into the superblock itself, as `sys_chunk_array`. So the
//! order is fixed and each step unlocks the next:
//!
//! 1. Read the superblock from a fixed *physical* offset — the only thing in
//!    the filesystem that does not need the map.
//! 2. Parse `sys_chunk_array` into a partial map, enough to cover
//!    `chunk_root`.
//! 3. Walk the chunk tree through that partial map and learn every remaining
//!    chunk. The map is now complete.
//! 4. Read the root tree at `root`, which lists the roots of all other trees.
//! 5. Look up `FS_TREE_OBJECTID` (5) there to get the default subvolume, and
//!    walk it for inodes, directory entries and file extents.
//!
//! # Scope: read-only, on purpose
//!
//! Writing to a CoW filesystem means allocating extents, updating the extent
//! tree's reference counts, and committing a transaction across several trees
//! atomically — a bug in any of which corrupts a volume that was fine before
//! we touched it. Read support is separately useful (mounting an existing
//! Linux volume) and cannot lose data. See `design-decisions.md` for the
//! matching decision on NTFS.

// Most of the on-disk vocabulary — the objectid and key-type constants, the
// block-group profile flags — exists because the format defines it, not
// because this read path happens to reach it, and a constant that is only
// named when a volume in the wild uses that feature is still the right thing
// to have written down. The self-test drives most of the rest, but that is
// invisible to dead-code analysis. Matches `fs::ntfs`.
#![allow(dead_code)]

pub mod btree;
pub mod chunk;
pub mod items;
pub mod raw;
pub mod sb;
pub mod tests;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::blocksrc::{DeviceSource, SectorSource, read_bytes};
use crate::fs::path::{Path, PathBuf};
use crate::fs::vfs::{DirEntry, EntryType, FileMeta, FileSystem, FsInfo};
use crate::serial_println;

use btree::TreeReader;
use chunk::ChunkMap;
use items::{DirItem, FileExtent, InodeItem, RootItem, name_hash, parse_dir_items};
use raw::{
    CHUNK_ITEM_KEY, DIR_INDEX_KEY, DIR_ITEM_KEY, EXTENT_DATA_KEY, FIRST_CHUNK_TREE_OBJECTID,
    FS_TREE_OBJECTID, INODE_ITEM_KEY, Key, ROOT_ITEM_KEY,
};
use sb::Superblock;

/// Refuse to slurp a file larger than this into a single `Vec`.
///
/// Matches the NTFS driver's cap. `read_file` has no streaming form in the
/// VFS trait, so the only alternative to a cap is a kernel allocation the
/// size of whatever file a caller names.
const MAX_READ_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// A mounted, read-only Btrfs volume.
///
/// # Synchronisation
///
/// None internally: the VFS stores each filesystem behind its own mutex, so
/// only one caller is inside these methods at a time. The struct is otherwise
/// immutable after `open_source` — the chunk map and the FS-tree root are
/// resolved once at mount and never change, because a read-only mount never
/// sees a new transaction.
pub struct BtrfsFs {
    src: Box<dyn SectorSource>,
    sb: Superblock,
    map: ChunkMap,
    fs_root: RootItem,
    /// Device name, when mounted from a real device rather than memory.
    device: Option<String>,
    /// VFS operations served, for `debug_stats`.
    ops: u64,
}

impl BtrfsFs {
    /// Open a volume from any sector source, performing the whole bootstrap.
    ///
    /// Every step here can fail, and each failure means something different to
    /// whoever is looking at the error, so none of them is folded into a
    /// generic "bad filesystem": a bad superblock is `InvalidArgument`, an
    /// unsupported feature or profile is `NotSupported`, and a block that does
    /// not checksum or is not the block we asked for is `IoError`.
    pub fn open_source(src: Box<dyn SectorSource>) -> KernelResult<Self> {
        let sb = sb::read_superblock(src.as_ref())?;

        // One `SectorSource` is one device. A chunk with stripes on a second
        // device is unreadable here no matter how correct the parser is, so
        // say so now rather than during a tree descent.
        if sb.num_devices != 1 {
            return Err(KernelError::NotSupported);
        }

        let nodesize = sb.nodesize_usize()?;

        // Step 2: the bootstrap map, from the superblock's embedded chunks.
        let mut map = ChunkMap::new();
        map.load_sys_array(&sb.sys_chunk_array)?;

        // Step 3: the complete map, from the chunk tree the bootstrap reached.
        //
        // The entries are collected before being inserted because the reader
        // borrows the map immutably for as long as the walk runs — and that
        // borrow is not an inconvenience to work around, it is the compiler
        // stating the actual hazard: mutating the map mid-walk would change
        // the translation of the very blocks the walk is still reading.
        let found = {
            let reader = TreeReader::new(src.as_ref(), &map, nodesize);
            let start = Key::new(FIRST_CHUNK_TREE_OBJECTID, CHUNK_ITEM_KEY, 0);
            let mut path = reader.search(sb.chunk_root, Some(sb.chunk_root_generation), &start)?;
            let mut found = Vec::new();

            while path.current().is_some() {
                let key = path.current_key()?;
                // The chunk tree also holds `DEV_ITEM`s under objectid 1,
                // which sort before the chunks and are skipped by starting the
                // search where we did. Anything past the chunk items ends it.
                if key.objectid != FIRST_CHUNK_TREE_OBJECTID || key.item_type != CHUNK_ITEM_KEY {
                    break;
                }
                let (entry, _) = chunk::parse_chunk_item(&key, path.current_data()?)?;
                found.push(entry);
                if !reader.next(&mut path)? {
                    break;
                }
            }
            found
        };

        for entry in found {
            map.insert(entry)?;
        }

        // Step 4-5: the root tree, then the default subvolume's tree.
        let fs_root = {
            let reader = TreeReader::new(src.as_ref(), &map, nodesize);
            let key = Key::new(FS_TREE_OBJECTID, ROOT_ITEM_KEY, 0);
            let path = reader.search(sb.root, Some(sb.generation), &key)?;
            let found = path.current_key().ok().filter(|k| {
                k.objectid == FS_TREE_OBJECTID && k.item_type == ROOT_ITEM_KEY
            });
            if found.is_none() {
                return Err(KernelError::NotFound);
            }
            RootItem::parse(path.current_data()?)?
        };

        Ok(Self {
            src,
            sb,
            map,
            fs_root,
            device: None,
            ops: 0,
        })
    }

    /// Open the Btrfs volume on a named block device.
    pub fn open(device: &str) -> KernelResult<Self> {
        let mut fs = Self::open_source(Box::new(DeviceSource::new(device)))?;
        fs.device = Some(String::from(device));
        Ok(fs)
    }

    /// A reader over the filesystem tree.
    fn reader(&self) -> KernelResult<TreeReader<'_>> {
        Ok(TreeReader::new(
            self.src.as_ref(),
            &self.map,
            self.sb.nodesize_usize()?,
        ))
    }

    /// The FS tree's root address and generation, as `search` wants them.
    fn fs_tree(&self) -> (u64, Option<u64>) {
        (self.fs_root.bytenr, Some(self.fs_root.generation))
    }

    /// Read one inode's metadata.
    fn lookup_inode(&self, ino: u64) -> KernelResult<InodeItem> {
        let reader = self.reader()?;
        let (root, generation) = self.fs_tree();
        let key = Key::new(ino, INODE_ITEM_KEY, 0);
        let path = reader
            .find(root, generation, &key)?
            .ok_or(KernelError::NotFound)?;
        InodeItem::parse(path.current_data()?)
    }

    /// Find a named child of a directory.
    ///
    /// The key is `(dir, DIR_ITEM, hash(name))`, so this is a single tree
    /// descent rather than a scan — but the name still has to be compared
    /// after the key matches, because the key's offset is a *hash* and two
    /// names in one directory may collide into the same item.
    fn lookup_child(&self, dir: u64, name: &[u8]) -> KernelResult<DirItem> {
        let reader = self.reader()?;
        let (root, generation) = self.fs_tree();
        let key = Key::new(dir, DIR_ITEM_KEY, name_hash(name));
        let Some(path) = reader.find(root, generation, &key)? else {
            return Err(KernelError::NotFound);
        };
        parse_dir_items(path.current_data()?)?
            .into_iter()
            .find(|d| d.name == name)
            .ok_or(KernelError::NotFound)
    }

    /// Every entry in a directory, in readdir order.
    ///
    /// `DIR_INDEX` rather than `DIR_ITEM`: the index key's offset is a
    /// monotonically increasing sequence number, so iterating it yields
    /// entries in creation order, whereas iterating `DIR_ITEM` would yield
    /// them in *hash* order — which is stable but arbitrary, and would make a
    /// directory listing look randomly shuffled.
    fn list_dir(&self, dir: u64) -> KernelResult<Vec<DirItem>> {
        let reader = self.reader()?;
        let (root, generation) = self.fs_tree();
        let start = Key::new(dir, DIR_INDEX_KEY, 0);
        let mut path = reader.search(root, generation, &start)?;
        let mut out = Vec::new();

        while path.current().is_some() {
            let key = path.current_key()?;
            if key.objectid != dir || key.item_type != DIR_INDEX_KEY {
                break;
            }
            for item in parse_dir_items(path.current_data()?)? {
                // Extended attributes are stored as dir items but are not
                // directory entries; listing them would invent files.
                if item.ftype != items::FT_XATTR {
                    out.push(item);
                }
            }
            if !reader.next(&mut path)? {
                break;
            }
        }

        Ok(out)
    }

    /// Resolve a VFS path to an inode number and its metadata.
    ///
    /// `..` is handled with a stack of the inodes walked through rather than
    /// by consulting `INODE_REF` backrefs. That is not a shortcut — it is the
    /// correct answer: a hard-linked directory is impossible, but the *path*
    /// the caller used is what `..` must unwind, and the backref records only
    /// one parent.
    fn resolve(&self, path: &Path) -> KernelResult<(u64, InodeItem)> {
        let root_ino = self.fs_root.root_dirid;
        let mut stack: Vec<u64> = Vec::new();
        let mut ino = root_ino;
        let mut inode = self.lookup_inode(ino)?;

        for comp in path.components() {
            let name = comp.as_bytes();
            if name == b"." {
                continue;
            }
            if name == b".." {
                ino = stack.pop().unwrap_or(root_ino);
                inode = self.lookup_inode(ino)?;
                continue;
            }
            if !inode.is_dir() {
                return Err(KernelError::NotADirectory);
            }

            let child = self.lookup_child(ino, name)?;
            // A nested subvolume's entry points at a ROOT_ITEM, not an inode
            // in this tree. Crossing into it means opening a second tree,
            // which this driver does not do yet — and following the objectid
            // as if it were an inode number would land on an unrelated file.
            if child.location.item_type != INODE_ITEM_KEY {
                return Err(KernelError::NotSupported);
            }
            stack.push(ino);
            ino = child.location.objectid;
            inode = self.lookup_inode(ino)?;
        }

        Ok((ino, inode))
    }

    /// Read `len` bytes of a file starting at `offset`.
    ///
    /// The output buffer is zero-filled up front, which is not laziness but
    /// the definition of the format: a file range with no `EXTENT_DATA` item
    /// is a hole, and a `PREALLOC` extent is space that was reserved and never
    /// written. Both must read as zeroes, and starting from zeroes means
    /// neither needs a special case — only extents that actually hold data
    /// write into the buffer.
    fn read_range(&self, ino: u64, inode: &InodeItem, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        if offset >= inode.size || len == 0 {
            return Ok(Vec::new());
        }
        let avail = inode.size.saturating_sub(offset);
        let want = u64::try_from(len)
            .map_err(|_| KernelError::InvalidArgument)?
            .min(avail);
        let want_usize = usize::try_from(want).map_err(|_| KernelError::FileTooLarge)?;
        let mut out = vec![0u8; want_usize];
        let end = offset.checked_add(want).ok_or(KernelError::InvalidArgument)?;

        let reader = self.reader()?;
        let (root, generation) = self.fs_tree();
        let mut path = reader.search(root, generation, &Key::new(ino, EXTENT_DATA_KEY, offset))?;

        // A forward search lands on the first extent starting at or after
        // `offset`, which is the wrong one whenever `offset` falls inside an
        // extent. Step back once and keep the earlier item only if it really
        // is one of this file's extents.
        let landed_exact = matches!(
            path.current_key(),
            Ok(k) if k.objectid == ino && k.item_type == EXTENT_DATA_KEY && k.offset == offset
        );
        if !landed_exact && reader.prev(&mut path)? {
            let usable = matches!(
                path.current_key(),
                Ok(k) if k.objectid == ino && k.item_type == EXTENT_DATA_KEY && k.offset <= offset
            );
            if !usable {
                reader.next(&mut path)?;
            }
        }

        while path.current().is_some() {
            let key = path.current_key()?;
            if key.objectid != ino || key.item_type != EXTENT_DATA_KEY {
                // Items for this inode that sort before its extents (the
                // inode item, its refs) are stepped over; anything sorting
                // after them ends the scan.
                let before = (key.objectid, key.item_type) < (ino, EXTENT_DATA_KEY);
                if before && reader.next(&mut path)? {
                    continue;
                }
                break;
            }
            if key.offset >= end {
                break;
            }

            let extent = items::parse_file_extent(path.current_data()?)?;
            self.apply_extent(&extent, key.offset, offset, &mut out, &reader)?;

            if !reader.next(&mut path)? {
                break;
            }
        }

        Ok(out)
    }

    /// Copy one extent's overlap with the requested window into `out`.
    fn apply_extent(
        &self,
        extent: &FileExtent,
        file_off: u64,
        window_start: u64,
        out: &mut [u8],
        reader: &TreeReader<'_>,
    ) -> KernelResult<()> {
        let (ext_len, inline): (u64, Option<&[u8]>) = match extent {
            FileExtent::Inline { ram_bytes, data } => (*ram_bytes, Some(data)),
            FileExtent::Regular { num_bytes, .. } => (*num_bytes, None),
        };
        if ext_len == 0 {
            return Ok(());
        }

        let window_end = window_start
            .checked_add(u64::try_from(out.len()).map_err(|_| KernelError::InvalidArgument)?)
            .ok_or(KernelError::InvalidArgument)?;
        let ext_end = file_off
            .checked_add(ext_len)
            .ok_or(KernelError::InvalidArgument)?;

        let copy_start = file_off.max(window_start);
        let copy_end = ext_end.min(window_end);
        if copy_end <= copy_start {
            return Ok(());
        }

        let n = usize::try_from(copy_end.saturating_sub(copy_start))
            .map_err(|_| KernelError::InvalidArgument)?;
        let dst_off = usize::try_from(copy_start.saturating_sub(window_start))
            .map_err(|_| KernelError::InvalidArgument)?;
        let skip = copy_start.saturating_sub(file_off);
        let dst_end = dst_off.checked_add(n).ok_or(KernelError::InvalidArgument)?;
        let dst = out.get_mut(dst_off..dst_end).ok_or(KernelError::InternalError)?;

        match extent {
            FileExtent::Inline { .. } => {
                let data = inline.ok_or(KernelError::InternalError)?;
                let skip_usize = usize::try_from(skip).map_err(|_| KernelError::InvalidArgument)?;
                let src_end = skip_usize.checked_add(n).ok_or(KernelError::InvalidArgument)?;
                // An inline extent's stored bytes may be shorter than
                // `ram_bytes` only if the item is truncated, which is
                // corruption rather than an implicit tail of zeroes.
                let src = data.get(skip_usize..src_end).ok_or(KernelError::InvalidArgument)?;
                dst.copy_from_slice(src);
            }
            FileExtent::Regular {
                disk_bytenr,
                offset,
                prealloc,
                ..
            } => {
                // A hole (`disk_bytenr == 0`) and preallocated space both read
                // as the zeroes already in `out`.
                if *disk_bytenr == 0 || *prealloc {
                    return Ok(());
                }
                let logical = disk_bytenr
                    .checked_add(*offset)
                    .and_then(|v| v.checked_add(skip))
                    .ok_or(KernelError::InvalidArgument)?;
                let bytes = reader.read_logical(logical, n)?;
                if bytes.len() != n {
                    return Err(KernelError::IoError);
                }
                dst.copy_from_slice(&bytes);
            }
        }
        Ok(())
    }

    /// Map a Btrfs inode mode to a VFS entry type.
    fn entry_type_of(inode: &InodeItem) -> EntryType {
        if inode.is_dir() {
            EntryType::Directory
        } else if inode.is_symlink() {
            EntryType::Symlink
        } else {
            EntryType::File
        }
    }

    /// The volume label as a `String`, lossily, for display only.
    fn label_string(&self) -> String {
        String::from_utf8_lossy(&self.sb.label).into_owned()
    }

    /// Number of chunks in the assembled map.
    pub fn chunk_count(&self) -> usize {
        self.map.len()
    }

    /// The parsed superblock.
    pub fn superblock(&self) -> &Superblock {
        &self.sb
    }
}

impl FileSystem for BtrfsFs {
    // The trait fixes the signature as `&self -> &str`; narrowing this impl to
    // `&'static str` would no longer implement it.
    #[allow(clippy::unnecessary_literal_bound)]
    fn fs_type(&self) -> &str {
        "btrfs"
    }

    fn readdir(&mut self, path: &Path) -> KernelResult<Vec<DirEntry>> {
        self.ops = self.ops.saturating_add(1);
        let (ino, inode) = self.resolve(path)?;
        if !inode.is_dir() {
            return Err(KernelError::NotADirectory);
        }

        let items = self.list_dir(ino)?;
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let entry_type = match item.ftype {
                items::FT_DIR => EntryType::Directory,
                items::FT_SYMLINK => EntryType::Symlink,
                _ => EntryType::File,
            };
            // A directory's size is meaningless to a caller and costs a tree
            // descent to obtain, so only files pay for one.
            let size = if entry_type == EntryType::File {
                self.lookup_inode(item.location.objectid)
                    .map(|i| i.size)
                    .unwrap_or(0)
            } else {
                0
            };
            out.push(DirEntry {
                name: PathBuf::from(item.name.as_slice()),
                entry_type,
                size,
            });
        }
        Ok(out)
    }

    fn read_file(&mut self, path: &Path) -> KernelResult<Vec<u8>> {
        self.ops = self.ops.saturating_add(1);
        let (ino, inode) = self.resolve(path)?;
        if inode.is_dir() {
            return Err(KernelError::IsADirectory);
        }
        if inode.size > MAX_READ_FILE_BYTES {
            return Err(KernelError::FileTooLarge);
        }
        let len = usize::try_from(inode.size).map_err(|_| KernelError::FileTooLarge)?;
        self.read_range(ino, &inode, 0, len)
    }

    fn read_at(&mut self, path: &Path, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        self.ops = self.ops.saturating_add(1);
        let (ino, inode) = self.resolve(path)?;
        if inode.is_dir() {
            return Err(KernelError::IsADirectory);
        }
        self.read_range(ino, &inode, offset, len)
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

        // Unlike NTFS, Btrfs stores a real POSIX mode, so it is reported
        // as-is — with the write bits masked off, because the mount refuses
        // every write and a mode that says otherwise is a lie userspace will
        // act on.
        let permissions = u16::try_from(inode.mode & 0o777).unwrap_or(0o444) & 0o555;

        Ok(FileMeta {
            size,
            entry_type,
            ino,
            created_ns: inode.otime_ns,
            modified_ns: inode.mtime_ns,
            accessed_ns: inode.atime_ns,
            changed_ns: inode.ctime_ns,
            permissions,
            nlinks: inode.nlink.max(1),
            blocks: inode.nbytes.checked_div(512).unwrap_or(0),
            ..FileMeta::minimal(entry_type, size)
        })
    }

    fn lmetadata(&mut self, path: &Path) -> KernelResult<FileMeta> {
        self.metadata(path)
    }

    fn readlink(&mut self, path: &Path) -> KernelResult<PathBuf> {
        self.ops = self.ops.saturating_add(1);
        let (ino, inode) = self.resolve(path)?;
        if !inode.is_symlink() {
            return Err(KernelError::InvalidArgument);
        }
        // Btrfs stores a symlink target exactly like file contents — almost
        // always as one inline extent — so this is a read, not a special case.
        let len = usize::try_from(inode.size).map_err(|_| KernelError::FileTooLarge)?;
        let target = self.read_range(ino, &inode, 0, len)?;
        Ok(PathBuf::from(target.as_slice()))
    }

    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        Ok(FsInfo {
            fs_type: String::from("btrfs"),
            volume_label: self.label_string(),
            block_size: u64::from(self.sb.sectorsize),
            total_blocks: self
                .sb
                .total_bytes
                .checked_div(u64::from(self.sb.sectorsize))
                .unwrap_or(0),
            // `bytes_used` counts *allocated* space, and free space on Btrfs
            // is not `total - used` in any useful sense — unallocated chunks,
            // metadata over-provisioning and shared extents all move the
            // number. Reporting 0 free is honest for a mount that refuses
            // every write, and is what the NTFS driver does for the same
            // reason.
            free_blocks: 0,
            total_inodes: 0,
            free_inodes: 0,
            max_name_len: 255,
            read_only: true,
        })
    }

    fn device_name(&self) -> Option<&str> {
        self.device.as_deref()
    }

    fn debug_stats(&self) -> String {
        alloc::format!(
            "Btrfs: label='{}', node={}B, sector={}B, gen={}, chunks={}, fs_root={:#x} level {}, ops={}",
            self.label_string(),
            self.sb.nodesize,
            self.sb.sectorsize,
            self.sb.generation,
            self.map.len(),
            self.fs_root.bytenr,
            self.fs_root.level,
            self.ops,
        )
    }
}

// ---------------------------------------------------------------------------
// Mount / probe
// ---------------------------------------------------------------------------

/// Mount the Btrfs volume on `device` at `mount_path`.
///
/// # Errors
///
/// Propagates [`BtrfsFs::open`] and [`crate::fs::Vfs::mount`] failures.
pub fn mount(device: &str, mount_path: &str) -> KernelResult<()> {
    let fs = BtrfsFs::open(device)?;
    crate::fs::Vfs::mount(mount_path, Box::new(fs))?;
    serial_println!("[btrfs] Mounted {} at {} (read-only)", device, mount_path);
    Ok(())
}

/// Whether `device` holds a Btrfs volume.
///
/// Reads only the primary superblock's magic. A probe is allowed to be
/// cheaper and more permissive than a mount — it answers "should this driver
/// be asked?", and the mount is where every other validation happens.
pub fn probe(device: &str) -> bool {
    let source = DeviceSource::new(device);
    match read_bytes(&source, raw::SUPERBLOCK_OFFSET, sb::SUPER_INFO_SIZE) {
        Ok(data) => raw::read_u64(&data, 0x40) == Ok(raw::MAGIC),
        Err(_) => false,
    }
}

/// Run the Btrfs self-tests.
///
/// # Errors
///
/// Propagates the first failing check.
pub fn self_test() -> KernelResult<()> {
    tests::self_test()
}
