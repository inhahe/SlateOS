//! `partition.c`: the label-independent partition, and the context
//! operations on partitions.
//!
//! A [`Partition`] is what the label driver reads out of its table, a
//! template for [`Context::add_partition`], or a line of an sfdisk script.
//! It is never the table's own data: changing one changes nothing until it
//! is handed to the context.

use crate::context::Context;
use crate::errno::{EINVAL, ENOSYS, ERANGE};
use crate::parttype::PartType;
use crate::table::Table;
use crate::utils::{div, partname};
use crate::{LABEL_FL_INCHARS_PARTNO, MOVE_DOWN, RESIZE_ENLARGE, RESIZE_REDUCE, SIZEUNIT_BYTES};
use crate::{SIZEUNIT_HUMAN, Sector, UNDEF, fieldtype, is_undef};

/// `FDISK_INIT_UNDEF` for a `size_t`.
pub const UNDEF_PARTNO: usize = usize::MAX;
/// `FDISK_INIT_UNDEF` for the `unsigned int` boot flag.
pub const UNDEF_BOOT: u32 = u32::MAX;

/// `struct fdisk_partition`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Partition {
    /// Its number, from 0.
    pub partno: usize,
    /// The extended partition holding it (MBR logicals).
    pub parent_partno: usize,
    /// Its first sector.
    pub start: Sector,
    /// Its size in sectors.
    pub size: Sector,
    /// `FDISK_MOVE_*` (scripts only).
    pub movestart: i32,
    /// `FDISK_RESIZE_*` (scripts only).
    pub resize: i32,
    /// Its name (GPT).
    pub name: Option<Vec<u8>>,
    /// Its UUID (GPT).
    pub uuid: Option<Vec<u8>>,
    /// Its attributes, as text.
    pub attrs: Option<Vec<u8>>,
    /// Its type.
    pub ty: Option<PartType>,
    /// The filesystem on it.
    pub fstype: Option<Vec<u8>>,
    /// That filesystem's UUID.
    pub fsuuid: Option<Vec<u8>>,
    /// That filesystem's label.
    pub fslabel: Option<Vec<u8>>,
    /// Printed after the start (`+`, `-`, `*`), or 0.
    pub start_post: u8,
    /// Printed after the end, or 0.
    pub end_post: u8,
    /// Printed after the size, or 0.
    pub size_post: u8,
    /// BSD: fragment size.
    pub fsize: u64,
    /// BSD: block size.
    pub bsize: u64,
    /// BSD: cylinders per group.
    pub cpg: u64,
    /// MBR: the start as C/H/S text.
    pub start_chs: Option<Vec<u8>>,
    /// MBR: the end as C/H/S text.
    pub end_chs: Option<Vec<u8>>,
    /// MBR: the boot flag.
    pub boot: u32,
    /// An extended partition.
    pub container: bool,
    /// Use the default end.
    pub end_follow_default: bool,
    /// Free space rather than a partition.
    pub freespace: bool,
    /// Use the default number.
    pub partno_follow_default: bool,
    /// Do not align the size.
    pub size_explicit: bool,
    /// Use the default start.
    pub start_follow_default: bool,
    /// blkid has looked at it.
    pub fs_probed: bool,
    /// It is in use.
    pub used: bool,
    /// A whole-disk system partition (Sun, SGI).
    pub wholedisk: bool,
}

impl Default for Partition {
    fn default() -> Self {
        Self::new()
    }
}

impl Partition {
    /// `fdisk_new_partition ()`: everything unset.
    #[must_use]
    pub fn new() -> Self {
        Self {
            partno: UNDEF_PARTNO,
            parent_partno: UNDEF_PARTNO,
            start: UNDEF,
            size: UNDEF,
            movestart: 0,
            resize: 0,
            name: None,
            uuid: None,
            attrs: None,
            ty: None,
            fstype: None,
            fsuuid: None,
            fslabel: None,
            start_post: 0,
            end_post: 0,
            size_post: 0,
            fsize: 0,
            bsize: 0,
            cpg: 0,
            start_chs: None,
            end_chs: None,
            boot: UNDEF_BOOT,
            container: false,
            end_follow_default: false,
            freespace: false,
            partno_follow_default: false,
            size_explicit: false,
            start_follow_default: false,
            fs_probed: false,
            used: false,
            wholedisk: false,
        }
    }

    /// `fdisk_reset_partition`.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// `fdisk_partition_set_start`: -ERANGE for the undefined value.
    pub fn set_start(&mut self, off: Sector) -> i32 {
        if is_undef(off) {
            return -ERANGE;
        }
        self.start = off;
        self.fs_probed = false;
        0
    }

    /// `fdisk_partition_unset_start`.
    pub fn unset_start(&mut self) {
        self.start = UNDEF;
        self.fs_probed = false;
    }

    /// `fdisk_partition_get_start`: meaningless unless [`Self::has_start`].
    #[must_use]
    pub fn get_start(&self) -> Sector {
        self.start
    }

    /// `fdisk_partition_has_start`.
    #[must_use]
    pub fn has_start(&self) -> bool {
        !is_undef(self.start)
    }

    /// `fdisk_partition_start_follow_default`.
    pub fn set_start_follow_default(&mut self, enable: bool) {
        self.start_follow_default = enable;
    }

    /// `fdisk_partition_start_is_default`.
    #[must_use]
    pub fn start_is_default(&self) -> bool {
        self.start_follow_default
    }

    /// `fdisk_partition_set_size`: -ERANGE for the undefined value.
    pub fn set_size(&mut self, sz: Sector) -> i32 {
        if is_undef(sz) {
            return -ERANGE;
        }
        self.size = sz;
        self.fs_probed = false;
        0
    }

    /// `fdisk_partition_unset_size`.
    pub fn unset_size(&mut self) {
        self.size = UNDEF;
        self.fs_probed = false;
    }

    /// `fdisk_partition_get_size`: meaningless unless [`Self::has_size`].
    #[must_use]
    pub fn get_size(&self) -> Sector {
        self.size
    }

    /// `fdisk_partition_has_size`.
    #[must_use]
    pub fn has_size(&self) -> bool {
        !is_undef(self.size)
    }

    /// `fdisk_partition_size_explicit`: do not align the size.
    pub fn set_size_explicit(&mut self, enable: bool) {
        self.size_explicit = enable;
    }

    /// `fdisk_partition_set_partno`: -ERANGE for the undefined value.
    pub fn set_partno(&mut self, num: usize) -> i32 {
        if num == UNDEF_PARTNO {
            return -ERANGE;
        }
        self.partno = num;
        0
    }

    /// `fdisk_partition_unset_partno`.
    pub fn unset_partno(&mut self) {
        self.partno = UNDEF_PARTNO;
    }

    /// `fdisk_partition_get_partno`.
    #[must_use]
    pub fn get_partno(&self) -> usize {
        self.partno
    }

    /// `fdisk_partition_has_partno`.
    #[must_use]
    pub fn has_partno(&self) -> bool {
        self.partno != UNDEF_PARTNO
    }

    /// `fdisk_partition_partno_follow_default`.
    pub fn set_partno_follow_default(&mut self, enable: bool) {
        self.partno_follow_default = enable;
    }

    /// `fdisk_partition_set_type`.
    pub fn set_type(&mut self, ty: Option<PartType>) {
        self.ty = ty;
    }

    /// `fdisk_partition_get_type`.
    #[must_use]
    pub fn get_type(&self) -> Option<&PartType> {
        self.ty.as_ref()
    }

    /// `fdisk_partition_set_name`.
    pub fn set_name(&mut self, name: Option<&[u8]>) {
        self.name = name.map(<[u8]>::to_vec);
    }

    /// `fdisk_partition_get_name`.
    #[must_use]
    pub fn get_name(&self) -> Option<&[u8]> {
        self.name.as_deref()
    }

    /// `fdisk_partition_set_uuid`.
    pub fn set_uuid(&mut self, uuid: Option<&[u8]>) {
        self.uuid = uuid.map(<[u8]>::to_vec);
    }

    /// `fdisk_partition_get_uuid`.
    #[must_use]
    pub fn get_uuid(&self) -> Option<&[u8]> {
        self.uuid.as_deref()
    }

    /// `fdisk_partition_set_attrs`.
    pub fn set_attrs(&mut self, attrs: Option<&[u8]>) {
        self.attrs = attrs.map(<[u8]>::to_vec);
    }

    /// `fdisk_partition_get_attrs`.
    #[must_use]
    pub fn get_attrs(&self) -> Option<&[u8]> {
        self.attrs.as_deref()
    }

    /// `fdisk_partition_has_end`.
    #[must_use]
    pub fn has_end(&self) -> bool {
        self.has_start() && self.has_size()
    }

    /// `fdisk_partition_get_end`: the last sector; meaningless unless
    /// [`Self::has_end`].
    #[must_use]
    pub fn get_end(&self) -> Sector {
        self.start
            .wrapping_add(self.size)
            .wrapping_sub(u64::from(self.size != 0))
    }

    /// `fdisk_partition_end_follow_default`.
    pub fn set_end_follow_default(&mut self, enable: bool) {
        self.end_follow_default = enable;
    }

    /// `fdisk_partition_end_is_default`.
    #[must_use]
    pub fn end_is_default(&self) -> bool {
        self.end_follow_default
    }

    /// `fdisk_partition_is_nested`: an MBR logical partition.
    #[must_use]
    pub fn is_nested(&self) -> bool {
        self.parent_partno != UNDEF_PARTNO
    }

    /// `fdisk_partition_is_container`: an MBR extended partition.
    #[must_use]
    pub fn is_container(&self) -> bool {
        self.container
    }

    /// `fdisk_partition_get_parent`.
    #[must_use]
    pub fn get_parent(&self) -> usize {
        self.parent_partno
    }

    /// `fdisk_partition_is_used`.
    #[must_use]
    pub fn is_used(&self) -> bool {
        self.used
    }

    /// `fdisk_partition_is_bootable`.
    #[must_use]
    pub fn is_bootable(&self) -> bool {
        self.boot == 1
    }

    /// `fdisk_partition_is_freespace`.
    #[must_use]
    pub fn is_freespace(&self) -> bool {
        self.freespace
    }

    /// `fdisk_partition_is_wholedisk`.
    #[must_use]
    pub fn is_wholedisk(&self) -> bool {
        self.wholedisk
    }

    /// `fdisk_partition_cmp_start`: undefined starts first.
    #[must_use]
    pub fn cmp_start(a: &Self, b: &Self) -> i32 {
        match (a.has_start(), b.has_start()) {
            (false, false) => 0,
            (false, true) => -1,
            (true, false) => 1,
            (true, true) => match a.start.cmp(&b.start) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            },
        }
    }

    /// `fdisk_partition_cmp_partno`: `a->partno - b->partno`, as the `int`
    /// C returns it.
    #[must_use]
    pub fn cmp_partno(a: &Self, b: &Self) -> i32 {
        let d = a.partno.wrapping_sub(b.partno);
        crate::utils::trunc_i32(i64::from_ne_bytes(d.to_ne_bytes()))
    }
}

impl Context {
    /// `fdisk_partition_next_partno (pa, cxt, &n)`: the number a new
    /// partition gets -- the first free one when `pa` follows the default,
    /// `pa`'s own when it has one, else asked for. `Err` holds upstream's
    /// status: 1 when nothing is free, or a negative errno.
    pub fn partition_next_partno(&mut self, pa: Option<&Partition>) -> Result<usize, i32> {
        let nmax = self.label_ref().map_or(0, |l| l.nparts_max);
        if let Some(pa) = pa {
            if pa.partno_follow_default {
                for i in 0..nmax {
                    if self.is_partition_used(i) == 0 {
                        return Ok(i);
                    }
                }
                return Err(-ERANGE);
            }
            if pa.has_partno() {
                if pa.partno >= nmax || self.is_partition_used(pa.partno) != 0 {
                    return Err(-ERANGE);
                }
                return Ok(pa.partno);
            }
        }
        if self.has_dialogs() {
            return self.ask_partnum(true);
        }
        Err(-EINVAL)
    }

    /// `probe_partition_content (cxt, pa)`: the filesystem type, UUID and
    /// label on the partition, by blkid. 0, or 1 when nothing could be
    /// probed.
    fn probe_partition_content(&mut self, pa: &mut Partition) -> i32 {
        pa.fstype = None;
        pa.fsuuid = None;
        pa.fslabel = None;
        if !pa.has_start() || !pa.has_size() {
            return 1;
        }
        let mut rc = 1;
        let mut pr = ulblkid::Probe::new();
        pr.enable_superblocks(true);
        pr.set_superblocks_flags(
            ulblkid::SUBLKS_MAGIC
                | ulblkid::SUBLKS_TYPE
                | ulblkid::SUBLKS_LABEL
                | ulblkid::SUBLKS_UUID
                | ulblkid::SUBLKS_BADCSUM,
        );
        let start = pa.start.wrapping_mul(self.sector_size);
        let size = pa.size.wrapping_mul(self.sector_size);
        if pr.set_device(
            self.dev.clone(),
            i64::from_ne_bytes(start.to_ne_bytes()),
            i64::from_ne_bytes(size.to_ne_bytes()),
        ) == 0
            && pr.do_fullprobe() == 0
        {
            rc = 0;
            let get = |name: &str| pr.lookup_value(name).map(|v| v.as_c_str().to_vec());
            pa.fstype = get("TYPE");
            pa.fslabel = get("LABEL");
            pa.fsuuid = get("UUID");
        }
        pa.fs_probed = true;
        rc
    }

    /// `fdisk_partition_to_string (pa, cxt, id, &data)`: one column of a
    /// listing, `None` where the column is empty for this partition.
    pub fn partition_to_string(
        &mut self,
        pa: &mut Partition,
        id: i32,
    ) -> Result<Option<Vec<u8>>, i32> {
        let num = |x: u64, post: u8| -> Vec<u8> {
            let mut v = x.to_string().into_bytes();
            if post != 0 {
                v.push(post);
            }
            v
        };
        let nonempty = |s: &Option<Vec<u8>>| s.as_ref().filter(|s| !s.is_empty()).cloned();
        let p = match id {
            fieldtype::DEVICE => {
                if pa.freespace {
                    Some(b"Free space".to_vec())
                } else if pa.has_partno() && self.dev_path.is_some() {
                    let inchars = self
                        .label_ref()
                        .is_some_and(|l| l.flags & LABEL_FL_INCHARS_PARTNO != 0);
                    if inchars {
                        let c = pa.partno.wrapping_add(usize::from(b'a'));
                        Some(vec![c.to_le_bytes().first().copied().unwrap_or(b'?')])
                    } else {
                        partname(self.dev_path.as_deref(), pa.partno.wrapping_add(1))
                    }
                } else {
                    None
                }
            }
            fieldtype::BOOT => pa.is_bootable().then(|| b"*".to_vec()),
            fieldtype::START => pa
                .has_start()
                .then(|| num(self.cround(pa.start), pa.start_post)),
            fieldtype::END => pa
                .has_end()
                .then(|| num(self.cround(pa.get_end()), pa.end_post)),
            fieldtype::SIZE => {
                if pa.has_size() {
                    let sz = pa.size.wrapping_mul(self.sector_size);
                    match self.sizeunit {
                        SIZEUNIT_BYTES => Some(sz.to_string().into_bytes()),
                        SIZEUNIT_HUMAN => {
                            if self.is_details() {
                                Some(num(sz, pa.size_post))
                            } else {
                                Some(
                                    ulstrutils::size_to_human_string(
                                        ulstrutils::SIZE_SUFFIX_1LETTER,
                                        sz,
                                    )
                                    .into_bytes(),
                                )
                            }
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            }
            fieldtype::CYLINDERS => {
                let sz = if pa.has_size() { pa.size } else { 0 };
                (sz != 0).then(|| {
                    let hs = u64::from(self.geom.heads).wrapping_mul(self.geom.sectors);
                    div(sz, hs).wrapping_add(1).to_string().into_bytes()
                })
            }
            fieldtype::SECTORS => Some(
                (if pa.has_size() { pa.size } else { 0 })
                    .to_string()
                    .into_bytes(),
            ),
            fieldtype::BSIZE => Some(pa.bsize.to_string().into_bytes()),
            fieldtype::FSIZE => Some(pa.fsize.to_string().into_bytes()),
            fieldtype::CPG => Some(pa.cpg.to_string().into_bytes()),
            fieldtype::TYPE => pa
                .ty
                .as_ref()
                .and_then(|t| t.name.as_deref())
                .map(<[u8]>::to_vec),
            fieldtype::TYPEID => pa.ty.as_ref().map(|t| match t.get_string() {
                Some(s) => s.to_vec(),
                None => format!("{:x}", t.get_code()).into_bytes(),
            }),
            fieldtype::UUID => nonempty(&pa.uuid),
            fieldtype::NAME => nonempty(&pa.name),
            fieldtype::ATTR => nonempty(&pa.attrs),
            fieldtype::SADDR => nonempty(&pa.start_chs),
            fieldtype::EADDR => nonempty(&pa.end_chs),
            fieldtype::FSUUID | fieldtype::FSLABEL | fieldtype::FSTYPE => {
                if pa.fs_probed || self.probe_partition_content(pa) == 0 {
                    nonempty(match id {
                        fieldtype::FSUUID => &pa.fsuuid,
                        fieldtype::FSLABEL => &pa.fslabel,
                        _ => &pa.fstype,
                    })
                } else {
                    None
                }
            }
            _ => return Err(-EINVAL),
        };
        Ok(p)
    }

    /// `fdisk_get_partition (cxt, partno, &pa)`: partition `partno` as the
    /// label has it. -EINVAL for an unused one.
    pub fn get_partition(&mut self, partno: usize) -> Result<Partition, i32> {
        let get_part = self
            .label_ref()
            .ok_or(-EINVAL)?
            .op
            .get_part
            .ok_or(-ENOSYS)?;
        if self.is_partition_used(partno) == 0 {
            return Err(-EINVAL);
        }
        let mut pa = Partition::new();
        pa.partno = partno;
        match get_part(self, partno, &mut pa) {
            0 => {
                pa.size_explicit = true;
                Ok(pa)
            }
            rc => Err(rc),
        }
    }

    /// `fdisk_set_partition (cxt, partno, pa)`: partition `partno` changed
    /// as `pa` says -- a new one when it is unused; its start and size
    /// recounted when `pa` moves or resizes it.
    pub fn set_partition(&mut self, partno: usize, pa: &mut Partition) -> i32 {
        let Some(lb) = self.label_ref() else {
            return -EINVAL;
        };
        let Some(set_part) = lb.op.set_part else {
            return -ENOSYS;
        };
        pa.fs_probed = false;

        if self.is_partition_used(partno) == 0 {
            pa.partno = partno;
            return self.add_partition(Some(pa), None);
        }

        let mut copy = None;
        if pa.resize != 0 || pa.movestart != 0 || pa.has_start() || pa.has_size() {
            let mut xpa = pa.clone();
            xpa.movestart = 0;
            xpa.resize = 0;
            xpa.size = UNDEF;
            xpa.start = UNDEF;
            let rc = self.recount_resize(partno, &mut xpa, pa);
            if rc != 0 {
                return rc;
            }
            copy = Some(xpa);
        }
        let xpa = copy.as_ref().unwrap_or(pa);

        // Disable the wipe of the old area.
        let mut wipe = 0;
        if let Ok(tmp) = self.get_partition(partno) {
            wipe = self.set_wipe_area(tmp.start, tmp.size, false);
        }
        let rc = set_part(self, partno, xpa);
        // ...and enable it for the new one.
        if rc == 0 && wipe != 0 {
            let _ = self.wipe_partition(partno, true);
        }
        rc
    }

    /// `recount_resize (cxt, partno, res, tpl)`: the new start and size of
    /// partition `partno` into `res`, from the relative or absolute ones in
    /// `tpl`, checked against the free space around it.
    fn recount_resize(&mut self, partno: usize, res: &mut Partition, tpl: &Partition) -> i32 {
        let mut start = UNDEF;
        let mut size = UNDEF;
        let mut tb = Table::new();
        let mut rc = self.get_partitions(&mut tb);
        if rc == 0 {
            // For resize free space is found at sector granularity, not
            // grain.
            let org = self.grain;
            self.grain = self.sector_size;
            rc = self.get_freespaces(&mut tb);
            self.grain = org;
        }
        if rc != 0 {
            return rc;
        }
        tb.sort_partitions(Partition::cmp_start);

        let Some(cur) = tb.get_partition_by_partno(partno) else {
            return -EINVAL;
        };
        let Some(cur_pa) = tb.get_partition(cur).cloned() else {
            return -EINVAL;
        };

        let erange = |cxt: &mut Self| -> i32 {
            cxt.warnx(
                format!("Failed to resize partition #{}.", partno.wrapping_add(1)).as_bytes(),
            );
            -ERANGE
        };

        // 1a) a new start relative to the current one;
        if tpl.movestart != 0 && tpl.has_start() {
            start = cur_pa.start;
            if tpl.movestart == MOVE_DOWN {
                if tpl.start > start {
                    return erange(self);
                }
                start = start.wrapping_sub(tpl.start);
            } else {
                start = start.wrapping_add(tpl.start);
            }
        // 1b) the first free sector before it;
        } else if tpl.movestart == MOVE_DOWN {
            match resize_get_first_possible(&tb, cur) {
                Some(s) => start = s,
                None => return erange(self),
            }
        // 1c) an absolute start.
        } else if tpl.has_start() {
            start = tpl.start;
        }

        // 2) The start has to be in the partition or in free space.
        if is_undef(start) {
            start = cur_pa.start;
        } else {
            let area = area_by_offset(&tb, &cur_pa, start);
            let grain_sectors = div(self.grain, self.sector_size);
            let ok = area == Some(cur)
                || area.is_some_and(|a| tb.get_partition(a).is_some_and(Partition::is_freespace))
                || (area.is_none()
                    && start >= self.first_lba
                    && start < self.first_lba.wrapping_add(grain_sectors));
            if !ok {
                return erange(self);
            }
        }

        // 3a) reduce;
        if tpl.resize == RESIZE_REDUCE && tpl.has_size() {
            size = cur_pa.size;
            if tpl.size > size {
                return erange(self);
            }
            size = size.wrapping_sub(tpl.size);
        // 3b) enlarge;
        } else if tpl.resize == RESIZE_ENLARGE && tpl.has_size() {
            size = cur_pa.size.wrapping_add(tpl.size);
        // 3c) enlarge to all the free space;
        } else if tpl.resize == RESIZE_ENLARGE {
            match resize_get_last_possible(&tb, cur, start) {
                Some(s) => size = s,
                None => return erange(self),
            }
        // 3d) an absolute size.
        } else if tpl.has_size() {
            size = tpl.size;
        }

        // 4) The size has to fit the partition and the free space after it.
        let xsize = if is_undef(size) { cur_pa.size } else { size };
        if cur_pa.has_size() {
            match resize_get_last_possible(&tb, cur, start) {
                Some(maxsz) if xsize <= maxsz => {}
                _ => return erange(self),
            }
        }

        res.start = start;
        res.size = size;
        0
    }

    /// `fdisk_wipe_partition (cxt, partno, enable)`: wipe (or not) the
    /// signatures in the partition's area when the table is written.
    pub fn wipe_partition(&mut self, partno: usize, enable: bool) -> i32 {
        match self.get_partition(partno) {
            Ok(pa) => {
                let rc = self.set_wipe_area(pa.start, pa.size, enable);
                if rc < 0 { rc } else { 0 }
            }
            Err(rc) => rc,
        }
    }

    /// `fdisk_partition_has_wipe (cxt, pa)`.
    #[must_use]
    pub fn partition_has_wipe(&self, pa: &Partition) -> bool {
        self.has_wipe_area(pa.start, pa.size)
    }

    /// `fdisk_add_partition (cxt, pa, &partno)`: a new partition after the
    /// template `pa` (or entirely by asking); `partno` receives its number.
    pub fn add_partition(&mut self, pa: Option<&mut Partition>, partno: Option<&mut usize>) -> i32 {
        let Some(lb) = self.label_ref() else {
            return -EINVAL;
        };
        let Some(add_part) = lb.op.add_part else {
            return -ENOSYS;
        };
        if self.missing_geometry() {
            return -EINVAL;
        }
        let mut pa = pa;
        if let Some(p) = pa.as_deref_mut() {
            p.fs_probed = false;
        }
        add_part(self, pa, partno)
    }

    /// `fdisk_delete_partition (cxt, partno)`.
    pub fn delete_partition(&mut self, partno: usize) -> i32 {
        let Some(lb) = self.label_ref() else {
            return -EINVAL;
        };
        let Some(del_part) = lb.op.del_part else {
            return -ENOSYS;
        };
        let _ = self.wipe_partition(partno, false);
        del_part(self, partno)
    }

    /// `fdisk_delete_all_partitions (cxt)`.
    pub fn delete_all_partitions(&mut self) -> i32 {
        let Some(lb) = self.label_ref() else {
            return -EINVAL;
        };
        let nmax = lb.nparts_max;
        for i in 0..nmax {
            if self.is_partition_used(i) == 0 {
                continue;
            }
            let rc = self.delete_partition(i);
            if rc != 0 {
                return rc;
            }
        }
        0
    }

    /// `fdisk_is_partition_used (cxt, n)`: 1 or 0 -- or a negative errno,
    /// which upstream's callers, testing for zero, take as "used".
    pub fn is_partition_used(&mut self, n: usize) -> i32 {
        let Some(lb) = self.label_ref() else {
            return -EINVAL;
        };
        let Some(f) = lb.op.part_is_used else {
            return -ENOSYS;
        };
        f(self, n)
    }
}

/// `area_by_offset (tb, cur, off)`: the entry of `tb` holding sector `off`,
/// among those in `cur`'s container when `cur` is nested.
fn area_by_offset(tb: &Table, cur: &Partition, off: Sector) -> Option<usize> {
    tb.iter().position(|pa| {
        pa.has_start()
            && pa.has_size()
            && !(cur.is_nested() && pa.parent_partno != cur.parent_partno)
            && off >= pa.start
            && off < pa.start.wrapping_add(pa.size)
    })
}

/// `resize_get_first_possible (tb, cur, &start)`: the start of the free
/// space just before entry `cur`, when there is some.
fn resize_get_first_possible(tb: &Table, cur: usize) -> Option<Sector> {
    let cur_pa = tb.get_partition(cur)?;
    let mut first: Option<&Partition> = None;
    for (i, pa) in tb.iter().enumerate() {
        if pa.start > cur_pa.start || i == cur {
            break;
        }
        if !pa.is_freespace()
            || !pa.has_start()
            || !pa.has_size()
            || (cur_pa.is_nested() && pa.parent_partno != cur_pa.parent_partno)
        {
            first = None;
            continue;
        }
        if pa.start.wrapping_add(pa.size) <= cur_pa.start {
            first = Some(pa);
        }
    }
    first.map(|p| p.start)
}

/// `resize_get_last_possible (tb, cur, start, &maxsz)`: how far a
/// partition at `start` can reach -- through entry `cur` and the free
/// space after it, up to the next partition.
fn resize_get_last_possible(tb: &Table, cur: usize, start: Sector) -> Option<Sector> {
    let cur_pa = tb.get_partition(cur)?;
    let mut last = false;
    let mut maxsz = 0;
    for (i, pa) in tb.iter().enumerate() {
        if !pa.has_start() || !pa.has_size() || (pa.is_container() && i != cur) {
            continue;
        }
        if pa.is_nested() && cur_pa.is_container() && pa.parent_partno == cur_pa.partno {
            continue;
        }
        if cur_pa.is_nested() && pa.parent_partno != cur_pa.parent_partno {
            continue;
        }
        if !last {
            if start >= pa.start && start < pa.start.wrapping_add(pa.size) {
                if pa.is_freespace() || i == cur {
                    last = true;
                } else {
                    break;
                }
                maxsz = pa.size.wrapping_sub(start.wrapping_sub(pa.start));
            }
        } else if !pa.is_freespace() && i != cur {
            break;
        } else {
            maxsz = pa.size.wrapping_sub(start.wrapping_sub(pa.start));
        }
    }
    last.then_some(maxsz)
}
