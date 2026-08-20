//! Read-only ZFS driver.
//!
//! ZFS is not laid out like the filesystems next to it in this tree. There is
//! no superblock at a fixed offset naming an inode table, and no inode number
//! that indexes anything directly. Instead there are three stacked layers,
//! and each one has to be climbed before the next makes sense:
//!
//! 1. **The SPA** — the storage pool. Four 256 KiB *vdev labels* live at the
//!    front and back of the device; each holds a configuration nvlist and an
//!    array of *uberblocks*. The uberblock with the highest transaction group
//!    that still checksums is the pool's current root.
//! 2. **The DMU** — objects. The uberblock's block pointer names an *objset*,
//!    whose meta-dnode describes an array of `dnode_phys_t`. An object number
//!    is an index into that array. Every other structure in the pool — the
//!    directory of datasets, a ZAP, a file's data — is an object read this
//!    way. Block pointers form a tree whose depth the dnode records.
//! 3. **The ZPL** — the POSIX layer. One dataset's objset holds a master node
//!    naming the root directory object; directories are ZAPs mapping names to
//!    `(type, object)`; a file's metadata is a System Attribute set in its
//!    dnode's bonus buffer.
//!
//! The route from a mount to a file is therefore:
//!
//! ```text
//! label -> uberblock -> MOS objset
//!       -> object 1 (object directory ZAP) -> "root_dataset"
//!       -> dsl_dir_phys_t.dd_head_dataset_obj
//!       -> dsl_dataset_phys_t.ds_bp -> filesystem objset
//!       -> object 1 (master node ZAP) -> "ROOT"
//!       -> the root directory object
//! ```
//!
//! Each of those arrows is a place a pool can be unreadable for a reason worth
//! distinguishing, which is why the errors below are specific rather than a
//! single "bad filesystem".
//!
//! # Scope, and what is refused rather than guessed
//!
//! This driver reads a **single-device, little-endian, unencrypted** pool. Each
//! of the following is detected and reported by name, never misread:
//!
//! | Not supported | Where it is caught |
//! |---|---|
//! | mirror / raidz (more than one vdev child) | `label::parse_config` |
//! | big-endian pool | `label::parse_uberblock`, `dmu::Reader::read_block` |
//! | encryption | `dmu::Reader::read_block` |
//! | Zstd compression | `zio::decompress` |
//! | gang blocks | `dmu::Reader::read_block` |
//! | external ZAP pointer tables | `zap::fat_lookup` |
//!
//! Read-only is a deliberate stopping point, for the same reason as Btrfs and
//! NTFS: writing to a copy-on-write pool means allocating from space maps,
//! updating a dataset's accounting, and committing a new uberblock — a bug in
//! any of which destroys a pool that was intact before we touched it. Reading
//! an existing pool is separately useful and cannot lose data.

// The on-disk vocabulary — object types, DSL field offsets, ZPL attribute
// names — exists because the format defines it, not because this read path
// reaches every member of it. Matches `fs::btrfs` and `fs::ntfs`.
#![allow(dead_code)]

pub mod dmu;
pub mod label;
pub mod nvlist;
pub mod raw;
pub mod sa;
pub mod tests;
pub mod zap;
pub mod zio;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::blocksrc::{DeviceSource, SectorSource, read_bytes};
use crate::fs::path::{Path, PathBuf};
use crate::fs::vfs::{DirEntry, EntryType, FileMeta, FileSystem, FsInfo};
use crate::serial_println;

use dmu::{Dnode, Reader, parse_dnode};
use label::{PoolConfig, Uberblock};
use raw::{
    BLKPTR_LEN, DMU_OST_ZFS, DMU_OT_SA, DNODE_LEN, UBERBLOCK_MAGIC, UBERBLOCK_MAGIC_SWAPPED,
    VDEV_LABEL_UBERBLOCK_OFFSET, VDEV_LABEL_UBERBLOCK_SIZE, bf64_get, parse_blkptr, read_u64,
};
use sa::{SaObject, SaRegistry};

/// Refuse to slurp a file larger than this into a single `Vec`.
///
/// Matches the Btrfs and NTFS drivers. `read_file` has no streaming form in
/// the VFS trait, so the only alternative to a cap is a kernel allocation the
/// size of whatever file a caller names.
const MAX_READ_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// The object directory, and the master node: object 1 of the MOS and of a
/// filesystem objset respectively. Both are fixed by the format.
const OBJECT_DIRECTORY_OBJ: u64 = 1;
/// Same number, different objset — named separately because they are different
/// things and conflating them is the classic ZFS reading mistake.
const MASTER_NODE_OBJ: u64 = 1;

/// Key in the MOS object directory naming the pool's root DSL directory.
const ROOT_DATASET_KEY: &[u8] = b"root_dataset";
/// Key in a filesystem's master node naming its root directory object.
const ZPL_ROOT_KEY: &[u8] = b"ROOT";
/// Key in a filesystem's master node holding its ZPL version.
const ZPL_VERSION_KEY: &[u8] = b"VERSION";

/// `os_type` lives after the meta dnode and the ZIL header.
///
/// `pub(super)` only so [`tests`] can build an objset the mount path accepts.
/// A test that wrote `704` here instead would still pass if this moved, and
/// would then be asserting against a layout the driver no longer uses.
pub(super) const OBJSET_TYPE_OFFSET: usize = DNODE_LEN + 192;

/// `dsl_dir_phys_t.dd_head_dataset_obj`.
pub(super) const DD_HEAD_DATASET_OBJ: usize = 8;
/// `dsl_dataset_phys_t.ds_bp`.
pub(super) const DS_BP_OFFSET: usize = 128;

/// `ZFS_DIRENT_TYPE` / `ZFS_DIRENT_OBJ`: a directory ZAP's value packs the
/// entry's type into the top bits of the object number.
const DIRENT_TYPE_LOW: u32 = 60;
const DIRENT_TYPE_LEN: u32 = 4;
const DIRENT_OBJ_LEN: u32 = 48;

/// `IFTODT` values as they appear in a directory entry.
const DT_DIR: u64 = 4;
const DT_LNK: u64 = 10;

/// POSIX mode bits, which ZFS stores verbatim.
const S_IFMT: u64 = 0o170_000;
const S_IFDIR: u64 = 0o040_000;
const S_IFLNK: u64 = 0o120_000;

/// A directory with more entries than this is refused rather than listed. A
/// ZAP is a hash table with no declared entry count, so the bound has to come
/// from somewhere; this is far above any real directory and well below a
/// kernel allocation worth worrying about.
const MAX_DIR_ENTRIES: usize = 1 << 20;

/// Symlink resolution depth while walking a path, matching the VFS limit.
const MAX_SYMLINK_DEPTH: u32 = 40;

/// The attribute numbers this driver reads, resolved once from the registry.
///
/// Numbers, not names, because a layout lists numbers — and the numbers are
/// per-dataset, assigned in registration order, so they cannot be constants.
#[derive(Debug, Clone, Default)]
struct ZplAttrs {
    mode: Option<u16>,
    size: Option<u16>,
    generation: Option<u16>,
    uid: Option<u16>,
    gid: Option<u16>,
    parent: Option<u16>,
    links: Option<u16>,
    atime: Option<u16>,
    mtime: Option<u16>,
    ctime: Option<u16>,
    crtime: Option<u16>,
    flags: Option<u16>,
    rdev: Option<u16>,
    symlink: Option<u16>,
}

impl ZplAttrs {
    fn resolve(reg: &SaRegistry) -> Self {
        Self {
            mode: reg.num_of(b"ZPL_MODE"),
            size: reg.num_of(b"ZPL_SIZE"),
            generation: reg.num_of(b"ZPL_GEN"),
            uid: reg.num_of(b"ZPL_UID"),
            gid: reg.num_of(b"ZPL_GID"),
            parent: reg.num_of(b"ZPL_PARENT"),
            links: reg.num_of(b"ZPL_LINKS"),
            atime: reg.num_of(b"ZPL_ATIME"),
            mtime: reg.num_of(b"ZPL_MTIME"),
            ctime: reg.num_of(b"ZPL_CTIME"),
            crtime: reg.num_of(b"ZPL_CRTIME"),
            flags: reg.num_of(b"ZPL_FLAGS"),
            rdev: reg.num_of(b"ZPL_RDEV"),
            symlink: reg.num_of(b"ZPL_SYMLINK"),
        }
    }
}

/// The System Attribute machinery for one dataset.
struct SaContext {
    registry: SaRegistry,
    layouts: Dnode,
    attrs: ZplAttrs,
}

/// One ZPL object's metadata, however it happened to be stored.
///
/// The two on-disk encodings — a `znode_phys_t` struct up to ZPL 4, a System
/// Attribute set from ZPL 5 — are collapsed here so that nothing above this
/// point has to know which one a dataset uses.
#[derive(Debug, Clone)]
struct Znode {
    obj: u64,
    dn: Dnode,
    mode: u64,
    size: u64,
    links: u64,
    uid: u64,
    gid: u64,
    atime_s: u64,
    mtime_s: u64,
    ctime_s: u64,
    crtime_s: u64,
    /// A symlink target stored inline in the attribute set, when it was.
    symlink: Option<Vec<u8>>,
}

impl Znode {
    const fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }
    const fn is_symlink(&self) -> bool {
        self.mode & S_IFMT == S_IFLNK
    }
    const fn entry_type(&self) -> EntryType {
        if self.mode & S_IFMT == S_IFDIR {
            EntryType::Directory
        } else if self.mode & S_IFMT == S_IFLNK {
            EntryType::Symlink
        } else {
            EntryType::File
        }
    }
}

/// A mounted, read-only ZFS pool.
///
/// # Synchronisation
///
/// None internally: the VFS stores each filesystem behind its own mutex, so
/// only one caller is inside these methods at a time. Everything resolved at
/// mount — the uberblock, both meta dnodes, the SA registry — is immutable
/// afterwards, because a read-only mount never sees a new transaction group.
pub struct ZfsFs {
    src: Box<dyn SectorSource>,
    config: PoolConfig,
    ub: Uberblock,
    /// The MOS objset's meta dnode: the array every pool-wide object is in.
    mos_meta: Dnode,
    /// The filesystem objset's meta dnode: the array every file is in.
    fs_meta: Dnode,
    /// The root directory's object number.
    root_obj: u64,
    /// The dataset's ZPL version; 5 and above use System Attributes.
    zpl_version: u64,
    /// `None` on a legacy (ZPL <= 4) dataset, which stores `znode_phys_t`.
    sa: Option<SaContext>,
    device: Option<String>,
    ops: u64,
}

impl ZfsFs {
    /// Open a pool from any sector source, performing the whole bootstrap.
    ///
    /// # Errors
    ///
    /// - [`KernelError::InvalidArgument`] if the device holds no readable vdev
    ///   label or no valid uberblock.
    /// - [`KernelError::NotSupported`] for a pool this driver refuses by name
    ///   (see the module documentation's table).
    /// - [`KernelError::CorruptedData`] if a structure the format guarantees
    ///   is absent or malformed.
    pub fn open_source(src: Box<dyn SectorSource>) -> KernelResult<Self> {
        let config = label::read_config(src.as_ref())?;
        let ub = label::find_uberblock(src.as_ref(), config.ashift)?;

        let reader = Reader::new(src.as_ref());

        // The MOS: the objset every pool-wide object lives in.
        let mos = reader.read_block(&ub.rootbp)?;
        let mos_meta = parse_dnode(&mos, 0)?;
        if !mos_meta.is_allocated() {
            return Err(KernelError::CorruptedData);
        }

        // The object directory names the pool's root DSL directory.
        let objdir = read_object(&reader, &mos_meta, OBJECT_DIRECTORY_OBJ)?;
        let root_dsl_obj = zap::lookup(&reader, &objdir, ROOT_DATASET_KEY)?;

        // A DSL directory's bonus points at the dataset holding its live data.
        let root_dsl = read_object(&reader, &mos_meta, root_dsl_obj)?;
        let head_ds_obj = read_u64(&root_dsl.bonus, DD_HEAD_DATASET_OBJ)?;
        if head_ds_obj == 0 {
            // A DSL directory with no head dataset is a container (the pool's
            // top level always has one), so this is the wrong object, not an
            // empty filesystem.
            return Err(KernelError::CorruptedData);
        }

        // The dataset's bonus carries the block pointer to its objset.
        let head_ds = read_object(&reader, &mos_meta, head_ds_obj)?;
        let ds_bp = parse_blkptr(&head_ds.bonus, DS_BP_OFFSET)?;
        if ds_bp.is_hole() {
            return Err(KernelError::CorruptedData);
        }

        let fs_objset = reader.read_block(&ds_bp)?;
        let os_type = read_u64(&fs_objset, OBJSET_TYPE_OFFSET)?;
        if os_type != DMU_OST_ZFS {
            // A ZVOL's objset is a block device, not a directory tree; mounting
            // it as one would list garbage.
            serial_println!("[zfs] Root dataset is objset type {}, not ZFS.", os_type);
            return Err(KernelError::NotSupported);
        }
        let fs_meta = parse_dnode(&fs_objset, 0)?;
        if !fs_meta.is_allocated() {
            return Err(KernelError::CorruptedData);
        }

        // The master node: the ZPL's entry point into its own objset.
        let master = read_object(&reader, &fs_meta, MASTER_NODE_OBJ)?;
        let root_obj = zap::lookup(&reader, &master, ZPL_ROOT_KEY)?;
        let zpl_version = zap::lookup(&reader, &master, ZPL_VERSION_KEY).unwrap_or(1);

        let sa = Self::open_sa(&reader, &fs_meta, &master);
        if zpl_version >= 5 && sa.is_none() {
            // Without the registry no attribute offset can be resolved, and
            // guessing the default layout is exactly the silent-wrong-answer
            // failure this driver exists to avoid. See `sa`'s module doc.
            serial_println!(
                "[zfs] ZPL v{} dataset has no readable SA registry.",
                zpl_version
            );
            return Err(KernelError::CorruptedData);
        }

        let fs = Self {
            src,
            config,
            ub,
            mos_meta,
            fs_meta,
            root_obj,
            zpl_version,
            sa,
            device: None,
            ops: 0,
        };

        // Nothing above proves the root *directory* is readable — every step
        // so far concerned the pool and the dataset, not the ZPL tree. A mount
        // is a claim that the volume is usable, so it is checked here rather
        // than left to fail on the first path lookup. See
        // `design-decisions.md` §216 Decision 5, which found the same gap in
        // F2FS.
        let root = fs.znode(fs.root_obj)?;
        if !root.is_dir() {
            serial_println!(
                "[zfs] Root object {} is not a directory (mode {:#o}).",
                fs.root_obj,
                root.mode
            );
            return Err(KernelError::CorruptedData);
        }
        Ok(fs)
    }

    /// Open the ZFS pool on a named block device.
    ///
    /// # Errors
    ///
    /// As [`Self::open_source`].
    pub fn open(device: &str) -> KernelResult<Self> {
        let mut fs = Self::open_source(Box::new(DeviceSource::new(device)))?;
        fs.device = Some(String::from(device));
        Ok(fs)
    }

    /// Resolve the System Attribute registry and layout table, if present.
    ///
    /// Returns `None` rather than an error: a ZPL 4 dataset legitimately has
    /// no `SA_ATTRS`, and the caller decides whether its absence is fatal.
    fn open_sa(reader: &Reader<'_>, fs_meta: &Dnode, master: &Dnode) -> Option<SaContext> {
        let sa_master_obj = zap::lookup(reader, master, sa::SA_ATTRS_KEY).ok()?;
        let sa_master = read_object(reader, fs_meta, sa_master_obj).ok()?;
        let reg_obj = zap::lookup(reader, &sa_master, sa::SA_REGISTRY_KEY).ok()?;
        let layouts_obj = zap::lookup(reader, &sa_master, sa::SA_LAYOUTS_KEY).ok()?;
        let reg_dn = read_object(reader, fs_meta, reg_obj).ok()?;
        let layouts = read_object(reader, fs_meta, layouts_obj).ok()?;
        let registry = SaRegistry::read(reader, &reg_dn).ok()?;
        if registry.is_empty() {
            return None;
        }
        let attrs = ZplAttrs::resolve(&registry);
        Some(SaContext {
            registry,
            layouts,
            attrs,
        })
    }

    fn reader(&self) -> Reader<'_> {
        Reader::new(self.src.as_ref())
    }

    /// Read one object of the filesystem objset.
    fn object(&self, obj: u64) -> KernelResult<Dnode> {
        read_object(&self.reader(), &self.fs_meta, obj)
    }

    /// Read one object's ZPL metadata, from whichever encoding it uses.
    fn znode(&self, obj: u64) -> KernelResult<Znode> {
        let dn = self.object(obj)?;
        if !dn.is_allocated() {
            return Err(KernelError::NotFound);
        }
        if dn.bonustype == DMU_OT_SA || self.zpl_version >= 5 {
            if let Some(ctx) = self.sa.as_ref() {
                return self.znode_from_sa(obj, dn, ctx);
            }
        }
        Self::znode_from_legacy(obj, dn)
    }

    fn znode_from_sa(&self, obj: u64, dn: Dnode, ctx: &SaContext) -> KernelResult<Znode> {
        let reader = self.reader();
        let set = SaObject::read(&reader, &dn, &ctx.layouts, &ctx.registry)?;
        let a = &ctx.attrs;
        let get = |n: Option<u16>| n.and_then(|n| set.uint(n)).unwrap_or(0);
        let time = |n: Option<u16>| n.and_then(|n| set.time_secs(n)).unwrap_or(0);
        let symlink = a
            .symlink
            .and_then(|n| set.bytes(n))
            .filter(|b| !b.is_empty())
            .map(<[u8]>::to_vec);
        Ok(Znode {
            obj,
            dn,
            mode: get(a.mode),
            size: get(a.size),
            links: get(a.links),
            uid: get(a.uid),
            gid: get(a.gid),
            atime_s: time(a.atime),
            mtime_s: time(a.mtime),
            ctime_s: time(a.ctime),
            crtime_s: time(a.crtime),
            symlink,
        })
    }

    fn znode_from_legacy(obj: u64, dn: Dnode) -> KernelResult<Znode> {
        use sa::znode as z;
        let b = &dn.bonus;
        Ok(Znode {
            obj,
            mode: read_u64(b, z::MODE)?,
            size: read_u64(b, z::SIZE)?,
            links: read_u64(b, z::LINKS)?,
            uid: read_u64(b, z::UID).unwrap_or(0),
            gid: read_u64(b, z::GID).unwrap_or(0),
            atime_s: read_u64(b, z::ATIME).unwrap_or(0),
            mtime_s: read_u64(b, z::MTIME).unwrap_or(0),
            ctime_s: read_u64(b, z::CTIME).unwrap_or(0),
            crtime_s: read_u64(b, z::CRTIME).unwrap_or(0),
            symlink: None,
            dn,
        })
    }

    /// Look up one name in a directory object.
    fn lookup_child(&self, dir: &Znode, name: &[u8]) -> KernelResult<(u64, u64)> {
        if !dir.is_dir() {
            return Err(KernelError::NotADirectory);
        }
        let value = zap::lookup(&self.reader(), &dir.dn, name)?;
        Ok(split_dirent(value))
    }

    /// Every entry of a directory object.
    fn list_dir(&self, dir: &Znode) -> KernelResult<Vec<(Vec<u8>, u64, u64)>> {
        let ents = zap::entries(&self.reader(), &dir.dn)?;
        if ents.len() > MAX_DIR_ENTRIES {
            return Err(KernelError::CorruptedData);
        }
        Ok(ents
            .into_iter()
            .map(|e| {
                let (ty, obj) = split_dirent(e.value);
                (e.name, ty, obj)
            })
            .collect())
    }

    /// A symlink's target, from wherever the dataset put it.
    ///
    /// ZFS stores a short target inline in the attribute set and a long one in
    /// the object's data blocks. Both are normal, and which applies is a
    /// property of the individual file, not of the pool.
    fn link_target(&self, zn: &Znode) -> KernelResult<Vec<u8>> {
        if let Some(inline) = zn.symlink.as_ref() {
            return Ok(inline.clone());
        }
        let len = usize::try_from(zn.size).map_err(|_| KernelError::FileTooLarge)?;
        self.reader().read_object_range(&zn.dn, 0, len, zn.size)
    }

    /// Resolve a VFS path to its object, following intermediate symlinks.
    ///
    /// A trailing symlink is *not* followed, which is what makes `lstat` and
    /// `readlink` correct; callers that want it followed are already routed
    /// through the VFS's own resolution.
    fn resolve(&self, path: &Path) -> KernelResult<Znode> {
        self.resolve_inner(path, 0)
    }

    fn resolve_inner(&self, path: &Path, depth: u32) -> KernelResult<Znode> {
        if depth > MAX_SYMLINK_DEPTH {
            return Err(KernelError::TooManyLinks);
        }
        // `..` unwinds the path the caller used rather than a stored parent
        // pointer: `ZPL_PARENT` records one parent, but the path is what the
        // caller meant. Matches the Btrfs driver.
        let mut stack: Vec<u64> = Vec::new();
        let mut current = self.znode(self.root_obj)?;

        let comps: Vec<&Path> = path.components().collect();
        for (i, comp) in comps.iter().enumerate() {
            let name = comp.as_bytes();
            if name == b"." {
                continue;
            }
            if name == b".." {
                let up = stack.pop().unwrap_or(self.root_obj);
                current = self.znode(up)?;
                continue;
            }
            let (_, obj) = self.lookup_child(&current, name)?;
            let next = self.znode(obj)?;
            let last = i.saturating_add(1) == comps.len();
            if next.is_symlink() && !last {
                // An intermediate symlink is resolved here so that the walk can
                // continue through it. Only relative targets can be followed
                // inside one filesystem; an absolute one is the VFS's business
                // because it may cross a mount.
                let target = self.link_target(&next)?;
                let target = Path::new(target.as_slice());
                if target.is_absolute() {
                    return Err(KernelError::NotSupported);
                }
                // Rebuild from the components already walked, then the target,
                // then whatever is left of the original path.
                let mut joined = PathBuf::new();
                for c in comps.iter().take(i) {
                    joined.push(c);
                }
                joined.push(target);
                for c in comps.iter().skip(i.saturating_add(1)) {
                    joined.push(c);
                }
                return self.resolve_inner(joined.as_path(), depth.saturating_add(1));
            }
            stack.push(current.obj);
            current = next;
        }
        Ok(current)
    }

    /// The pool name as a `String`, lossily, for display only.
    fn label_string(&self) -> String {
        String::from_utf8_lossy(&self.config.name).into_owned()
    }

    /// The pool's parsed configuration.
    #[must_use]
    pub const fn pool_config(&self) -> &PoolConfig {
        &self.config
    }

    /// The uberblock this mount is reading through.
    #[must_use]
    pub const fn uberblock(&self) -> &Uberblock {
        &self.ub
    }
}

/// Read object `obj` out of the dnode array described by `meta`.
///
/// An object number is an index of 512-byte dnode slots into one object, so
/// this is a division and a block read — there is no inode table to consult.
fn read_object(reader: &Reader<'_>, meta: &Dnode, obj: u64) -> KernelResult<Dnode> {
    let byte = obj
        .checked_mul(DNODE_LEN as u64)
        .ok_or(KernelError::InvalidArgument)?;
    if meta.datablksz == 0 {
        return Err(KernelError::CorruptedData);
    }
    let blkid = byte
        .checked_div(meta.datablksz)
        .ok_or(KernelError::CorruptedData)?;
    let within = usize::try_from(byte.checked_rem(meta.datablksz).unwrap_or(0))
        .map_err(|_| KernelError::InvalidArgument)?;
    let block = reader.read_object_block(meta, blkid)?;
    parse_dnode(&block, within)
}

/// Split a directory ZAP value into its entry type and object number.
const fn split_dirent(value: u64) -> (u64, u64) {
    (
        bf64_get(value, DIRENT_TYPE_LOW, DIRENT_TYPE_LEN),
        bf64_get(value, 0, DIRENT_OBJ_LEN),
    )
}

/// Map a directory entry's stored type to a VFS entry type.
const fn dirent_entry_type(ty: u64) -> EntryType {
    match ty {
        DT_DIR => EntryType::Directory,
        DT_LNK => EntryType::Symlink,
        _ => EntryType::File,
    }
}

/// Seconds to nanoseconds, saturating rather than wrapping — a corrupt
/// timestamp should read as "very far in the future", not as a small number.
const fn secs_to_ns(secs: u64) -> u64 {
    secs.saturating_mul(1_000_000_000)
}

impl FileSystem for ZfsFs {
    // The trait fixes the signature as `&self -> &str`; narrowing this impl to
    // `&'static str` would no longer implement it.
    #[allow(clippy::unnecessary_literal_bound)]
    fn fs_type(&self) -> &str {
        "zfs"
    }

    fn readdir(&mut self, path: &Path) -> KernelResult<Vec<DirEntry>> {
        self.ops = self.ops.saturating_add(1);
        let dir = self.resolve(path)?;
        if !dir.is_dir() {
            return Err(KernelError::NotADirectory);
        }
        let items = self.list_dir(&dir)?;
        let mut out = Vec::with_capacity(items.len());
        for (name, ty, obj) in items {
            let entry_type = dirent_entry_type(ty);
            // A directory's size is meaningless to a caller and costs a dnode
            // read to obtain, so only files pay for one.
            let size = if entry_type == EntryType::Directory {
                0
            } else {
                self.znode(obj).map(|z| z.size).unwrap_or(0)
            };
            out.push(DirEntry {
                name: PathBuf::from(name.as_slice()),
                entry_type,
                size,
            });
        }
        Ok(out)
    }

    fn read_file(&mut self, path: &Path) -> KernelResult<Vec<u8>> {
        self.ops = self.ops.saturating_add(1);
        let zn = self.resolve(path)?;
        if zn.is_dir() {
            return Err(KernelError::IsADirectory);
        }
        if zn.size > MAX_READ_FILE_BYTES {
            return Err(KernelError::FileTooLarge);
        }
        let len = usize::try_from(zn.size).map_err(|_| KernelError::FileTooLarge)?;
        self.reader().read_object_range(&zn.dn, 0, len, zn.size)
    }

    fn read_at(&mut self, path: &Path, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        self.ops = self.ops.saturating_add(1);
        let zn = self.resolve(path)?;
        if zn.is_dir() {
            return Err(KernelError::IsADirectory);
        }
        self.reader()
            .read_object_range(&zn.dn, offset, len, zn.size)
    }

    fn stat(&mut self, path: &Path) -> KernelResult<DirEntry> {
        self.ops = self.ops.saturating_add(1);
        let zn = self.resolve(path)?;
        let entry_type = zn.entry_type();
        let name = path
            .file_name()
            .map_or_else(|| PathBuf::from("/"), Path::to_path_buf);
        Ok(DirEntry {
            name,
            entry_type,
            size: if entry_type == EntryType::Directory {
                0
            } else {
                zn.size
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
        let zn = self.resolve(path)?;
        let entry_type = zn.entry_type();
        let size = if entry_type == EntryType::Directory {
            0
        } else {
            zn.size
        };
        // ZFS stores a real POSIX mode, reported as-is with the write bits
        // masked off: the mount refuses every write, and a mode claiming
        // otherwise is a lie userspace will act on.
        let permissions = u16::try_from(zn.mode & 0o777).unwrap_or(0o444) & 0o555;
        Ok(FileMeta {
            size,
            entry_type,
            ino: zn.obj,
            created_ns: secs_to_ns(zn.crtime_s),
            modified_ns: secs_to_ns(zn.mtime_s),
            accessed_ns: secs_to_ns(zn.atime_s),
            changed_ns: secs_to_ns(zn.ctime_s),
            permissions,
            nlinks: u32::try_from(zn.links).unwrap_or(1).max(1),
            blocks: zn.dn.used.checked_div(512).unwrap_or(0),
            ..FileMeta::minimal(entry_type, size)
        })
    }

    fn lmetadata(&mut self, path: &Path) -> KernelResult<FileMeta> {
        self.metadata(path)
    }

    fn readlink(&mut self, path: &Path) -> KernelResult<PathBuf> {
        self.ops = self.ops.saturating_add(1);
        let zn = self.resolve(path)?;
        if !zn.is_symlink() {
            return Err(KernelError::InvalidArgument);
        }
        Ok(PathBuf::from(self.link_target(&zn)?.as_slice()))
    }

    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        Ok(FsInfo {
            fs_type: String::from("zfs"),
            volume_label: self.label_string(),
            // A pool's block size is per-dataset and variable up to 16 MiB;
            // the vdev's `ashift` is the one size that is a property of the
            // device and true for every dataset on it.
            block_size: 1u64 << self.config.ashift,
            // Free and total space on ZFS come from the space maps and the
            // dataset's accounting, neither of which this read path parses.
            // Reporting zeroes is honest for a mount that refuses every write;
            // inventing a number from the device size would not be.
            total_blocks: 0,
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
            "ZFS: pool='{}', guid={:#x}, ashift={}, txg={}, spa_version={}, zpl_version={}, sa={}, root_obj={}, ops={}",
            self.label_string(),
            self.config.guid,
            self.config.ashift,
            self.ub.txg,
            self.config.version,
            self.zpl_version,
            if self.sa.is_some() { "yes" } else { "legacy" },
            self.root_obj,
            self.ops,
        )
    }
}

// ---------------------------------------------------------------------------
// Mount / probe
// ---------------------------------------------------------------------------

/// Mount the ZFS pool on `device` at `mount_path`.
///
/// # Errors
///
/// Propagates [`ZfsFs::open`] and [`crate::fs::Vfs::mount`] failures.
pub fn mount(device: &str, mount_path: &str) -> KernelResult<()> {
    let fs = ZfsFs::open(device)?;
    crate::fs::Vfs::mount(mount_path, Box::new(fs))?;
    serial_println!("[zfs] Mounted {} at {} (read-only)", device, mount_path);
    Ok(())
}

/// Whether `device` holds a ZFS pool.
///
/// Scans label 0's uberblock array for the uberblock magic rather than reading
/// the configuration nvlist. The magic is a full 64 bits and appears at a
/// known stride, so it is both cheaper and far more specific than parsing XDR;
/// and the byte-swapped form is accepted here because a big-endian pool *is*
/// ZFS — refusing it belongs in the mount, where the message can say so.
#[must_use]
pub fn probe(device: &str) -> bool {
    let source = DeviceSource::new(device);
    let Ok(data) = read_bytes(
        &source,
        VDEV_LABEL_UBERBLOCK_OFFSET,
        usize::try_from(VDEV_LABEL_UBERBLOCK_SIZE).unwrap_or(0),
    ) else {
        return false;
    };
    // The slot stride is `1 << max(ashift, 10)`, always a multiple of 1024, so
    // stepping by 1024 finds every slot whatever the pool's ashift.
    data.chunks_exact(1 << 10).any(|slot| {
        matches!(
            read_u64(slot, 0),
            Ok(UBERBLOCK_MAGIC | UBERBLOCK_MAGIC_SWAPPED)
        )
    })
}

/// Run the ZFS self-tests.
///
/// # Errors
///
/// Propagates the first failing check.
pub fn self_test() -> KernelResult<()> {
    tests::self_test()
}

// Keep the constant referenced even when no path below happens to use it, so
// that a future reader sees it is the same 128 bytes the parser assumes.
const _: () = assert!(BLKPTR_LEN == 128);
