//! `context.c`: the library handle -- the device, what is known about it,
//! the label drivers, and the program's dialog callback.
//!
//! Three kinds of partitioning data, as upstream distinguishes them:
//!
//! * on-disk label data, read by the drivers when a device is assigned or
//!   a label is switched to, and changed only by
//!   [`Context::write_disklabel`];
//! * in-memory label data, in each [`Label`], which every partitioning
//!   operation works on;
//! * [`Partition`]s and [`Table`]s, the label-independent view, which never
//!   affect either of the others.

use std::cell::RefCell;
use std::fs::File;
use std::rc::Rc;

use crate::ask::AskCallback;
use crate::errno::{EBUSY, EINVAL, ERANGE};
use crate::label::{Label, deinit_label};
use crate::partition::Partition;
use crate::table::{Table, diff_tables};
use crate::utils::errno_of;
use crate::{Geometry, LabelType, SIZEUNIT_HUMAN, Sector, diff};

/// What `fstat` said about the device (`dev_st`), as far as libfdisk looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DevStat {
    /// `S_ISREG`.
    pub is_reg: bool,
    /// `S_ISBLK`.
    pub is_blk: bool,
    /// `st_size`.
    pub size: u64,
    /// `st_rdev`.
    pub rdev: u64,
}

impl DevStat {
    fn of(m: &std::fs::Metadata) -> Self {
        Self {
            is_reg: m.file_type().is_file(),
            is_blk: ulblkid::blkdev::is_block(m),
            size: m.len(),
            rdev: ulblkid::blkdev::rdev(m),
        }
    }
}

/// `struct fdisk_context`.
pub struct Context {
    /// The device (`dev_fd`).
    pub(crate) dev: Option<Rc<File>>,
    /// Its path.
    pub(crate) dev_path: Option<Vec<u8>>,
    /// `/sys/block/<name>/device/model`, once looked for.
    pub(crate) dev_model: Option<Vec<u8>>,
    /// What `fstat` said.
    pub(crate) dev_st: DevStat,
    /// The in-memory first sector, shared with a nested context's parent.
    pub(crate) firstsector: Option<Rc<RefCell<Vec<u8>>>>,
    /// The I/O size fdisk uses.
    pub(crate) io_size: u64,
    /// The device's optimal I/O size.
    pub(crate) optimal_io_size: u64,
    /// Its minimal I/O size.
    pub(crate) min_io_size: u64,
    /// Its physical sector size.
    pub(crate) phy_sector_size: u64,
    /// Its logical sector size.
    pub(crate) sector_size: u64,
    /// Its alignment offset.
    pub(crate) alignment_offset: u64,
    /// Do not write to the device.
    pub(crate) readonly: bool,
    /// Show positions in cylinders (Sun, SGI).
    pub(crate) display_in_cyl_units: bool,
    /// Expert display mode.
    pub(crate) display_details: bool,
    /// Do not zero the boot code when creating a label.
    pub(crate) protect_bootbits: bool,
    /// The collision is another partition table.
    pub(crate) pt_collision: bool,
    /// Dialog-driven partitioning disabled (`no_disalogs`).
    pub(crate) no_dialogs: bool,
    /// The model has been looked for.
    pub(crate) dev_model_probed: bool,
    /// libfdisk opened the device itself.
    pub(crate) is_priv: bool,
    /// Opened with `O_EXCL`.
    pub(crate) is_excl: bool,
    /// Listing only.
    pub(crate) listonly: bool,
    /// The filesystem, RAID or table libblkid found.
    pub(crate) collision: Option<Vec<u8>>,
    /// The areas to wipe before writing: (start, size) in sectors.
    pub(crate) wipes: Vec<(u64, u64)>,
    /// `FDISK_SIZEUNIT_*`.
    pub(crate) sizeunit: i32,
    /// The alignment unit, in bytes.
    pub(crate) grain: u64,
    /// Where the first partition should begin.
    pub(crate) first_lba: Sector,
    /// Where the last should end.
    pub(crate) last_lba: Sector,
    /// The size in logical sectors.
    pub(crate) total_sectors: Sector,
    /// The geometry.
    pub(crate) geom: Geometry,
    /// The user's geometry, over the device's.
    pub(crate) user_geom: Geometry,
    /// The user's physical sector size (`user_pyh_sector`).
    pub(crate) user_pyh_sector: u64,
    /// The user's logical sector size.
    pub(crate) user_log_sector: u64,
    /// The user's grain.
    pub(crate) user_grain: u64,
    /// The current label: a position in `labels`.
    pub(crate) label: Option<usize>,
    /// The label drivers.
    pub(crate) labels: Vec<Label>,
    /// The dialog callback.
    pub(crate) ask_cb: Option<AskCallback>,
    /// The parent of a nested context, which this one owns.
    pub(crate) parent: Option<Box<Context>>,
    /// The script being followed, shared with a nested context.
    pub(crate) script: Option<Rc<RefCell<crate::script::Script>>>,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("dev_path", &self.dev_path)
            .field("label", &self.label)
            .field("labels", &self.labels)
            .finish_non_exhaustive()
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

/// `sync ()`.
pub(crate) fn sync() {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn sync();
        }
        // SAFETY: sync(2) takes nothing and cannot fail.
        unsafe { sync() };
    }
}

impl Context {
    /// A context with nothing in it, as `calloc` leaves one: no drivers, no
    /// device. A program uses one as the placeholder it leaves while it
    /// moves its context into a nested one ([`Context::new_nested`]) or
    /// out of it ([`Context::into_parent`]).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            dev: None,
            dev_path: None,
            dev_model: None,
            dev_st: DevStat::default(),
            firstsector: None,
            io_size: 0,
            optimal_io_size: 0,
            min_io_size: 0,
            phy_sector_size: 0,
            sector_size: 0,
            alignment_offset: 0,
            readonly: false,
            display_in_cyl_units: false,
            display_details: false,
            protect_bootbits: false,
            pt_collision: false,
            no_dialogs: false,
            dev_model_probed: false,
            is_priv: false,
            is_excl: false,
            listonly: false,
            collision: None,
            wipes: Vec::new(),
            sizeunit: SIZEUNIT_HUMAN,
            grain: 0,
            first_lba: 0,
            last_lba: 0,
            total_sectors: 0,
            geom: Geometry::default(),
            user_geom: Geometry::default(),
            user_pyh_sector: 0,
            user_log_sector: 0,
            user_grain: 0,
            label: None,
            labels: Vec::new(),
            ask_cb: None,
            parent: None,
            script: None,
        }
    }

    /// `fdisk_new_context ()`: a context with every label driver, in the
    /// order they are probed.
    #[must_use]
    pub fn new() -> Self {
        let mut cxt = Self::empty();
        cxt.labels = crate::label::all_drivers();
        cxt
    }

    /// `init_nested_from_parent (cxt, isnew)`: the device, its properties
    /// and the callback from the parent; the display settings too for a new
    /// nested context.
    fn init_nested_from_parent(&mut self, isnew: bool) -> i32 {
        let Some(parent) = self.parent.as_deref() else {
            return -EINVAL;
        };
        self.wipes.clear();
        self.alignment_offset = parent.alignment_offset;
        self.ask_cb.clone_from(&parent.ask_cb);
        self.dev.clone_from(&parent.dev);
        self.first_lba = parent.first_lba;
        self.firstsector.clone_from(&parent.firstsector);
        self.geom = parent.geom;
        self.grain = parent.grain;
        self.io_size = parent.io_size;
        self.last_lba = parent.last_lba;
        self.min_io_size = parent.min_io_size;
        self.optimal_io_size = parent.optimal_io_size;
        self.phy_sector_size = parent.phy_sector_size;
        self.readonly = parent.readonly;
        self.script.clone_from(&parent.script);
        self.sector_size = parent.sector_size;
        self.total_sectors = parent.total_sectors;
        self.user_geom = parent.user_geom;
        self.user_log_sector = parent.user_log_sector;
        self.user_pyh_sector = parent.user_pyh_sector;
        if isnew {
            self.listonly = parent.listonly;
            self.display_details = parent.display_details;
            self.display_in_cyl_units = parent.display_in_cyl_units;
            self.protect_bootbits = parent.protect_bootbits;
        }
        self.dev_model = None;
        self.dev_model_probed = false;
        self.dev_path.clone_from(&parent.dev_path);
        0
    }

    /// `fdisk_new_nested_context (parent, name)`: a context for the label
    /// nested in `parent`'s ("bsd", or "dos"/"mbr"), which takes ownership
    /// of the parent and probes for the label when the parent has a device.
    #[must_use]
    pub fn new_nested(parent: Self, name: Option<&[u8]>) -> Self {
        let has_dev = parent.dev.is_some();
        let mut cxt = Self::empty();
        cxt.parent = Some(Box::new(parent));
        let _ = cxt.init_nested_from_parent(true);

        let lb = match name {
            Some(n) if crate::utils::strcasecmp(n, b"bsd") == 0 => {
                crate::label::nested_driver(LabelType::Bsd)
            }
            Some(n)
                if crate::utils::strcasecmp(n, b"dos") == 0
                    || crate::utils::strcasecmp(n, b"mbr") == 0 =>
            {
                crate::label::nested_driver(LabelType::Dos)
            }
            _ => None,
        };
        if let Some(lb) = lb {
            cxt.labels.push(lb);
            let i = cxt.labels.len().saturating_sub(1);
            if has_dev {
                cxt.label = Some(i);
                let probe = cxt.labels.get(i).and_then(|l| l.op.probe);
                if probe.is_some_and(|f| f(&mut cxt) == 1) {
                    cxt.switch_label(i);
                } else {
                    if let Some(l) = cxt.labels.get_mut(i) {
                        deinit_label(l);
                    }
                    cxt.label = None;
                }
            }
        }
        cxt
    }

    /// Gives back the parent of a nested context.
    #[must_use]
    pub fn into_parent(mut self) -> Option<Self> {
        self.parent.take().map(|b| *b)
    }

    /// `fdisk_get_parent (cxt)`.
    #[must_use]
    pub fn get_parent(&self) -> Option<&Self> {
        self.parent.as_deref()
    }

    /// The parent, for changing.
    pub fn get_parent_mut(&mut self) -> Option<&mut Self> {
        self.parent.as_deref_mut()
    }

    /// `fdisk_get_label (cxt, name)`: the driver called `name` ("mbr" is
    /// "dos"), or the current one; its position in the context.
    #[must_use]
    pub fn get_label(&self, name: Option<&[u8]>) -> Option<usize> {
        let Some(name) = name else { return self.label };
        let name: &[u8] = if crate::utils::strcasecmp(name, b"mbr") == 0 {
            b"dos"
        } else {
            name
        };
        self.labels
            .iter()
            .position(|l| crate::utils::strcasecmp(l.name.as_bytes(), name) == 0)
    }

    /// The driver at position `i`.
    #[must_use]
    pub fn label_at(&self, i: usize) -> Option<&Label> {
        self.labels.get(i)
    }

    /// The driver at position `i`, for changing.
    pub fn label_at_mut(&mut self, i: usize) -> Option<&mut Label> {
        self.labels.get_mut(i)
    }

    /// The driver called `name`.
    #[must_use]
    pub fn label_by_name(&self, name: &[u8]) -> Option<&Label> {
        self.get_label(Some(name)).and_then(|i| self.labels.get(i))
    }

    /// The driver called `name`, for changing.
    pub fn label_by_name_mut(&mut self, name: &[u8]) -> Option<&mut Label> {
        let i = self.get_label(Some(name))?;
        self.labels.get_mut(i)
    }

    /// `fdisk_next_label` as an iteration: every driver, in order.
    pub fn labels(&self) -> std::slice::Iter<'_, Label> {
        self.labels.iter()
    }

    /// `fdisk_get_nlabels`.
    #[must_use]
    pub fn get_nlabels(&self) -> usize {
        self.labels.len()
    }

    /// `__fdisk_switch_label (cxt, lb)`: makes driver `i` current and
    /// applies its alignment. -EINVAL for a disabled one.
    pub fn switch_label(&mut self, i: usize) -> i32 {
        match self.labels.get(i) {
            Some(lb) if !lb.disabled => {}
            _ => return -EINVAL,
        }
        self.label = Some(i);
        self.apply_label_device_properties();
        0
    }

    /// `fdisk_has_label`.
    #[must_use]
    pub fn has_label(&self) -> bool {
        self.label.is_some()
    }

    /// `fdisk_has_protected_bootbits`.
    #[must_use]
    pub fn has_protected_bootbits(&self) -> bool {
        self.protect_bootbits
    }

    /// `fdisk_enable_bootbits_protection`.
    pub fn enable_bootbits_protection(&mut self, enable: bool) {
        self.protect_bootbits = enable;
    }

    /// `fdisk_disable_dialogs`.
    pub fn disable_dialogs(&mut self, disable: bool) {
        self.no_dialogs = disable;
    }

    /// `fdisk_has_dialogs`.
    #[must_use]
    pub fn has_dialogs(&self) -> bool {
        !self.no_dialogs
    }

    /// `fdisk_enable_wipe`: wipe the whole device's signatures on write.
    pub fn enable_wipe(&mut self, enable: bool) {
        let n = self.total_sectors;
        let _ = self.set_wipe_area(0, n, enable);
    }

    /// `fdisk_has_wipe`.
    #[must_use]
    pub fn has_wipe(&self) -> bool {
        self.has_wipe_area(0, self.total_sectors)
    }

    /// `fdisk_get_collision`.
    #[must_use]
    pub fn get_collision(&self) -> Option<&[u8]> {
        self.collision.as_deref()
    }

    /// `fdisk_is_ptcollision`.
    #[must_use]
    pub fn is_ptcollision(&self) -> bool {
        self.pt_collision
    }

    /// `fdisk_get_npartitions`: the most partitions the current label can
    /// hold.
    #[must_use]
    pub fn get_npartitions(&self) -> usize {
        self.label_ref().map_or(0, |l| l.nparts_max)
    }

    /// `fdisk_label_set_changed (cxt->label, 1)`: the current label marked
    /// as changed in memory, to be written.
    pub(crate) fn set_label_changed(&mut self) {
        if let Some(lb) = self.label_mut() {
            lb.changed = true;
        }
    }

    /// `fdisk_is_labeltype (cxt, id)`.
    #[must_use]
    pub fn is_labeltype(&self, id: LabelType) -> bool {
        self.label_ref().is_some_and(|l| l.id == id)
    }

    /// `reset_context`: back to no device and no label; the drivers' data
    /// reset.
    fn reset_context(&mut self) {
        for lb in &mut self.labels {
            deinit_label(lb);
        }
        // The device closes with its last holder; a nested context's first
        // sector may be its own or its parent's, and dropping it is right
        // either way.
        self.dev = None;
        self.firstsector = None;
        self.dev_path = None;
        self.dev_model = None;
        self.dev_model_probed = false;
        self.collision = None;
        self.dev_st = DevStat::default();
        self.is_priv = false;
        self.is_excl = false;
        self.zeroize_device_properties();
        self.script = None;
        self.label = None;
        self.free_wipe_areas();
    }

    /// `fdisk_assign_fd`: the body of assigning a device. A nested context
    /// assigns its parent, and takes over what that found.
    fn assign_fd(
        &mut self,
        file: Rc<File>,
        fname: &[u8],
        readonly: bool,
        privdev: bool,
        excl: bool,
    ) -> i32 {
        if self.parent.is_some() {
            let listonly = self.listonly;
            let rc = match self.parent.as_deref_mut() {
                Some(parent) => {
                    let org = parent.listonly;
                    // assign is sensitive to "listonly": follow this
                    // context's, to avoid unwanted warnings.
                    parent.listonly = listonly;
                    let rc = parent.assign_fd(file, fname, readonly, privdev, excl);
                    parent.listonly = org;
                    rc
                }
                None => -EINVAL,
            };
            let mut rc = rc;
            if rc == 0 {
                rc = self.init_nested_from_parent(false);
            }
            if rc == 0 {
                self.probe_labels();
            }
            return rc;
        }

        self.reset_context();
        let meta = match file.metadata() {
            Ok(m) => m,
            Err(e) => return crate::utils::neg_errno(&e),
        };
        self.dev_st = DevStat::of(&meta);
        self.readonly = readonly;
        self.dev = Some(file);
        self.is_priv = privdev;
        self.is_excl = excl;
        self.dev_path = Some(fname.to_vec());

        self.discover_topology();
        self.discover_geometry();
        self.apply_user_device_properties();

        let rc = self.read_firstsector();
        if rc < 0 {
            self.dev = None;
            return rc;
        }
        // Warn about obsolete stuff on the device unless only listing.
        if !self.listonly {
            let rc = self.check_collisions();
            if rc < 0 {
                self.dev = None;
                return rc;
            }
        }
        self.probe_labels();
        self.apply_label_device_properties();

        // No collision when there is a valid table already: the boot bits
        // are wiped only when a new one is created.
        if self.pt_collision && self.has_label() {
            self.pt_collision = false;
            self.collision = None;
        }
        0
    }

    /// `fdisk_assign_device (cxt, fname, readonly)`: opens `fname` --
    /// read-write and exclusively unless `readonly`, without `O_EXCL` when
    /// that is refused as busy -- and assigns it.
    pub fn assign_device(&mut self, fname: &[u8], readonly: bool) -> i32 {
        let path = ulsysfs::path_of(fname);
        let open = |excl: bool| {
            let mut o = std::fs::OpenOptions::new();
            o.read(true).write(!readonly);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                /// `O_EXCL`.
                const O_EXCL: i32 = 0o200;
                if excl {
                    o.custom_flags(O_EXCL);
                }
            }
            #[cfg(not(unix))]
            let _ = excl;
            o.open(&path)
        };
        let mut excl = !readonly;
        let file = match open(excl) {
            Ok(f) => f,
            Err(e) if excl && e.raw_os_error() == Some(EBUSY) => {
                excl = false;
                match open(false) {
                    Ok(f) => f,
                    Err(e) => return crate::utils::neg_errno(&e),
                }
            }
            Err(e) => return crate::utils::neg_errno(&e),
        };
        self.assign_fd(Rc::new(file), fname, readonly, true, excl)
    }

    /// `fdisk_assign_device_by_fd (cxt, fd, fname, readonly)`: a device the
    /// caller opened (and keeps its own handle to).
    pub fn assign_device_by_file(&mut self, file: Rc<File>, fname: &[u8], readonly: bool) -> i32 {
        self.assign_fd(file, fname, readonly, false, false)
    }

    /// `fdisk_deassign_device (cxt, nosync)`: flushed and closed (and the
    /// system synced unless `nosync`); only closed when read-only.
    pub fn deassign_device(&mut self, nosync: bool) -> i32 {
        if let Some(parent) = self.parent.as_deref_mut() {
            let rc = parent.deassign_device(nosync);
            if rc != 0 {
                return rc;
            }
            return self.init_nested_from_parent(false);
        }
        if !(self.readonly && self.is_priv) {
            if let Some(dev) = self.dev.as_ref() {
                if let Err(e) = dev.sync_all() {
                    let errnum = errno_of(&e);
                    let mut m = self.dev_path.clone().unwrap_or_default();
                    m.extend_from_slice(b": fsync device failed");
                    self.warn(errnum, &m);
                    return errnum.wrapping_neg();
                }
            }
            if !nosync {
                self.info(b"Syncing disks.");
                sync();
            }
        }
        self.dev_path = None;
        self.dev = None;
        self.is_priv = false;
        self.is_excl = false;
        0
    }

    /// `fdisk_reassign_device (cxt)`: everything in memory dropped, the
    /// device assigned afresh.
    pub fn reassign_device(&mut self) -> i32 {
        let Some(devname) = self.dev_path.clone() else {
            return -EINVAL;
        };
        let rdonly = self.readonly;
        let privdev = self.is_priv;
        // Only a descriptor the program gave is kept: one of the library's
        // own is closed by `deassign_device`, as upstream's is, so that its
        // exclusive open of a block device is let go before the reopen asks
        // for it again.
        let file = if privdev { None } else { self.dev.clone() };
        let excl = self.is_excl;
        // What could fail there -- the fsync of a writable device -- it has
        // said; upstream does not look at the result either.
        let _ = self.deassign_device(true);
        if privdev {
            self.assign_device(&devname, rdonly)
        } else {
            match file {
                Some(f) => self.assign_fd(f, &devname, rdonly, privdev, excl),
                None => -EINVAL,
            }
        }
    }

    /// `fdisk_reread_partition_table (cxt)`: the kernel told to re-read the
    /// table (a block device only).
    pub fn reread_partition_table(&mut self) -> i32 {
        if !self.dev_st.is_blk {
            return 0;
        }
        sync();
        self.info(b"Calling ioctl() to re-read partition table.");
        let rc = match self.dev.as_ref() {
            Some(dev) => ulblkid::blkdev::ioctl_val(dev, ulblkid::blkdev::BLKRRPART, 0),
            None => Err(EINVAL),
        };
        if let Err(errnum) = rc {
            self.warn(errnum, b"Re-reading the partition table failed.");
            self.info(
                b"The kernel still uses the old table. The new table will be used at the next \
                  reboot or after you run partprobe(8) or partx(8).",
            );
            return errnum.wrapping_neg();
        }
        0
    }

    /// `fdisk_reread_changes (cxt, org)`: the kernel told about each
    /// partition that changed since `org`, by `BLKPG`, so that the rest may
    /// stay mounted.
    pub fn reread_changes(&mut self, org: &Table) -> i32 {
        let mut tb = Table::new();
        let _ = self.get_partitions(&mut tb);
        let mut rem: Vec<Partition> = Vec::new();
        let mut add: Vec<Partition> = Vec::new();
        let mut upd: Vec<Partition> = Vec::new();
        for (change, pa) in diff_tables(Some(org), Some(&tb)) {
            match change {
                diff::REMOVED => rem.push(pa),
                diff::ADDED => add.push(pa),
                diff::RESIZED => upd.push(pa),
                diff::MOVED => {
                    rem.push(pa.clone());
                    add.push(pa);
                }
                _ => {}
            }
        }
        // The sector-size factor: BLKPG counts 512-byte sectors.
        let ssf = self.sector_size / 512;
        let mut err: u32 = 0;
        for pa in &rem {
            if partx_del_partition(self.dev.as_deref(), pa.partno.wrapping_add(1)) != 0 {
                let errnum = last_errno();
                self.warn(
                    errnum,
                    format!(
                        "Failed to remove partition {} from system",
                        pa.partno.wrapping_add(1)
                    )
                    .as_bytes(),
                );
                err = err.wrapping_add(1);
            }
        }
        for pa in &upd {
            if partx_resize_partition(
                self.dev.as_deref(),
                pa.partno.wrapping_add(1),
                pa.start.wrapping_mul(ssf),
                pa.size.wrapping_mul(ssf),
            ) != 0
            {
                let errnum = last_errno();
                self.warn(
                    errnum,
                    format!(
                        "Failed to update system information about partition {}",
                        pa.partno.wrapping_add(1)
                    )
                    .as_bytes(),
                );
                err = err.wrapping_add(1);
            }
        }
        let is_dos = self.is_labeltype(LabelType::Dos);
        for pa in &add {
            let mut sz = pa.size.wrapping_mul(ssf);
            if is_dos && pa.is_container() {
                // As the kernel does: an extended partition is 1 or 2
                // sectors.
                sz = sz.min(2);
            }
            if partx_add_partition(
                self.dev.as_deref(),
                pa.partno.wrapping_add(1),
                pa.start.wrapping_mul(ssf),
                sz,
            ) != 0
            {
                let errnum = last_errno();
                self.warn(
                    errnum,
                    format!(
                        "Failed to add partition {} to system",
                        pa.partno.wrapping_add(1)
                    )
                    .as_bytes(),
                );
                err = err.wrapping_add(1);
            }
        }
        if err != 0 {
            self.info(b"The kernel still uses the old partitions. The new table will be used at the next reboot. ");
        }
        0
    }

    /// `fdisk_device_is_used (cxt)`: opened read-write without `O_EXCL` by
    /// libfdisk -- the kernel would not let it have the device exclusively.
    #[must_use]
    pub fn device_is_used(&self) -> bool {
        !self.readonly && !self.is_excl && self.is_priv
    }

    /// `fdisk_is_readonly`.
    #[must_use]
    pub fn is_readonly(&self) -> bool {
        self.readonly
    }

    /// `fdisk_is_regfile`.
    #[must_use]
    pub fn is_regfile(&self) -> bool {
        self.dev_st.is_reg
    }

    /// `fdisk_enable_details`.
    pub fn enable_details(&mut self, enable: bool) {
        self.display_details = enable;
    }

    /// `fdisk_is_details`.
    #[must_use]
    pub fn is_details(&self) -> bool {
        self.display_details
    }

    /// `fdisk_enable_listonly`.
    pub fn enable_listonly(&mut self, enable: bool) {
        self.listonly = enable;
    }

    /// `fdisk_is_listonly`.
    #[must_use]
    pub fn is_listonly(&self) -> bool {
        self.listonly
    }

    /// `fdisk_set_unit (cxt, str)`: "cylinder(s)" or "sector(s)"; anything
    /// else (or none) is sectors.
    pub fn set_unit(&mut self, s: Option<&[u8]>) {
        self.display_in_cyl_units = matches!(s, Some(b"cylinder" | b"cylinders"));
    }

    /// `fdisk_get_unit (cxt, n)`: the unit's name, `SINGULAR` or `PLURAL`.
    #[must_use]
    pub fn get_unit(&self, n: i32) -> &'static str {
        match (self.use_cylinders(), n == crate::SINGULAR) {
            (true, true) => "cylinder",
            (true, false) => "cylinders",
            (false, true) => "sector",
            (false, false) => "sectors",
        }
    }

    /// `fdisk_use_cylinders`.
    #[must_use]
    pub fn use_cylinders(&self) -> bool {
        self.display_in_cyl_units
    }

    /// `fdisk_get_units_per_sector`: sectors per display unit.
    #[must_use]
    pub fn get_units_per_sector(&self) -> u32 {
        if self.use_cylinders() {
            crate::utils::u64_as_u32(u64::from(self.geom.heads).wrapping_mul(self.geom.sectors))
        } else {
            1
        }
    }

    /// `fdisk_get_optimal_iosize`: never 0.
    #[must_use]
    pub fn get_optimal_iosize(&self) -> u64 {
        if self.optimal_io_size != 0 {
            self.optimal_io_size
        } else {
            self.io_size
        }
    }

    /// `fdisk_get_minimal_iosize`.
    #[must_use]
    pub fn get_minimal_iosize(&self) -> u64 {
        self.min_io_size
    }

    /// `fdisk_get_physector_size`.
    #[must_use]
    pub fn get_physector_size(&self) -> u64 {
        self.phy_sector_size
    }

    /// `fdisk_get_sector_size`.
    #[must_use]
    pub fn get_sector_size(&self) -> u64 {
        self.sector_size
    }

    /// `fdisk_get_alignment_offset`.
    #[must_use]
    pub fn get_alignment_offset(&self) -> u64 {
        self.alignment_offset
    }

    /// `fdisk_get_grain_size`.
    #[must_use]
    pub fn get_grain_size(&self) -> u64 {
        self.grain
    }

    /// `fdisk_get_first_lba`.
    #[must_use]
    pub fn get_first_lba(&self) -> Sector {
        self.first_lba
    }

    /// `fdisk_set_first_lba`.
    pub fn set_first_lba(&mut self, lba: Sector) {
        self.first_lba = lba;
    }

    /// `fdisk_get_last_lba`.
    #[must_use]
    pub fn get_last_lba(&self) -> Sector {
        self.last_lba
    }

    /// `fdisk_set_last_lba`: -ERANGE past the device or at 0.
    pub fn set_last_lba(&mut self, lba: Sector) -> i32 {
        if lba > self.total_sectors.wrapping_sub(1) || lba < 1 {
            return -ERANGE;
        }
        self.last_lba = lba;
        0
    }

    /// `fdisk_set_size_unit`.
    pub fn set_size_unit(&mut self, unit: i32) {
        self.sizeunit = unit;
    }

    /// `fdisk_get_size_unit`.
    #[must_use]
    pub fn get_size_unit(&self) -> i32 {
        self.sizeunit
    }

    /// `fdisk_get_nsectors`: the size in logical sectors.
    #[must_use]
    pub fn get_nsectors(&self) -> Sector {
        self.total_sectors
    }

    /// `fdisk_get_devname`.
    #[must_use]
    pub fn get_devname(&self) -> Option<&[u8]> {
        self.dev_path.as_deref()
    }

    /// `fdisk_get_devno`: 0 unless a block device.
    #[must_use]
    pub fn get_devno(&self) -> u64 {
        if self.dev_st.is_blk {
            self.dev_st.rdev
        } else {
            0
        }
    }

    /// `fdisk_get_devmodel`: `device/model` from sysfs, looked for once.
    pub fn get_devmodel(&mut self) -> Option<&[u8]> {
        if !self.dev_model_probed {
            let devno = self.get_devno();
            if devno != 0 {
                if let Some(pc) = ulsysfs::new_sysfs_path(devno, None, None) {
                    // A model that cannot be read is none: upstream's
                    // ul_path_read_string failing leaves it NULL, and the
                    // listing then has no `Disk model:` line.
                    self.dev_model = pc.read_string(b"device/model").ok().flatten();
                }
            }
            self.dev_model_probed = true;
        }
        self.dev_model.as_deref()
    }

    /// The device (`fdisk_get_devfd`).
    #[must_use]
    pub fn get_dev(&self) -> Option<&Rc<File>> {
        self.dev.as_ref()
    }

    /// `fdisk_get_geom_heads`.
    #[must_use]
    pub fn get_geom_heads(&self) -> u32 {
        self.geom.heads
    }

    /// `fdisk_get_geom_sectors`.
    #[must_use]
    pub fn get_geom_sectors(&self) -> Sector {
        self.geom.sectors
    }

    /// `fdisk_get_geom_cylinders`.
    #[must_use]
    pub fn get_geom_cylinders(&self) -> Sector {
        self.geom.cylinders
    }

    /// `fdisk_missing_geometry (cxt)`: the label needs C/H/S and some is 0
    /// -- said, unless only listing.
    pub fn missing_geometry(&mut self) -> bool {
        let Some(lb) = self.label_ref() else {
            return false;
        };
        let rc = lb.require_geometry()
            && (self.geom.heads == 0 || self.geom.sectors == 0 || self.geom.cylinders == 0);
        if rc && !self.listonly {
            self.warnx(b"Incomplete geometry setting.");
        }
        rc
    }
}

/// The errno the last failed call left.
fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// `struct blkpg_partition`.
#[repr(C)]
struct BlkpgPartition {
    start: i64,
    length: i64,
    pno: i32,
    devname: [u8; 64],
    volname: [u8; 64],
}

/// `struct blkpg_ioctl_arg`.
#[repr(C)]
struct BlkpgIoctlArg {
    op: i32,
    flags: i32,
    datalen: i32,
    data: *mut BlkpgPartition,
}

/// `BLKPG`.
const BLKPG: u64 = 0x1269;
/// `BLKPG_ADD_PARTITION`.
const BLKPG_ADD_PARTITION: i32 = 1;
/// `BLKPG_DEL_PARTITION`.
const BLKPG_DEL_PARTITION: i32 = 2;
/// `BLKPG_RESIZE_PARTITION`.
const BLKPG_RESIZE_PARTITION: i32 = 3;

/// `partx_*`: one `BLKPG` request, with start and length in 512-byte
/// sectors. 0, or -1 with errno set.
fn blkpg(dev: Option<&File>, op: i32, partno: usize, start: u64, size: u64) -> i32 {
    let Some(dev) = dev else { return -1 };
    let mut p = BlkpgPartition {
        start: i64::from_ne_bytes(start.wrapping_mul(512).to_ne_bytes()),
        length: i64::from_ne_bytes(size.wrapping_mul(512).to_ne_bytes()),
        pno: i32::try_from(partno).unwrap_or(i32::MAX),
        devname: [0; 64],
        volname: [0; 64],
    };
    let mut a = BlkpgIoctlArg {
        op,
        flags: 0,
        datalen: i32::try_from(std::mem::size_of::<BlkpgPartition>()).unwrap_or(0),
        data: &raw mut p,
    };
    // SAFETY: BLKPG reads one `struct blkpg_ioctl_arg`, whose `data` points
    // at `p`, a live `struct blkpg_partition` of `datalen` bytes.
    match unsafe { ulblkid::blkdev::ioctl_ptr(dev, BLKPG, &mut a) } {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

/// `partx_del_partition (fd, partno)`.
fn partx_del_partition(dev: Option<&File>, partno: usize) -> i32 {
    blkpg(dev, BLKPG_DEL_PARTITION, partno, 0, 0)
}

/// `partx_add_partition (fd, partno, start, size)`.
fn partx_add_partition(dev: Option<&File>, partno: usize, start: u64, size: u64) -> i32 {
    blkpg(dev, BLKPG_ADD_PARTITION, partno, start, size)
}

/// `partx_resize_partition (fd, partno, start, size)`.
fn partx_resize_partition(dev: Option<&File>, partno: usize, start: u64, size: u64) -> i32 {
    blkpg(dev, BLKPG_RESIZE_PARTITION, partno, start, size)
}
