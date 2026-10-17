//! `dos.c`: the DOS (MBR) label driver.
//!
//! Four primary entries in the first sector; one of them may be an
//! extended partition, holding a chain of EBRs (extended boot records),
//! each with a logical partition and a link to the next. Partitions 1-4 are
//! the primaries, 5 on the logicals in chain order.
//!
//! Upstream keeps a `struct pte` per partition, with pointers into sector
//! buffers: `pt_entry` (the partition's entry) and `ex_entry` (the EBR's
//! link). Here a [`Pte`] holds its sector buffer -- the first sector, shared,
//! for a primary; the EBR it read, for a logical -- and the slots (0-3) of
//! the two entries in it. Entries are read out as a [`DosPart`], changed,
//! and written back.

use std::cell::RefCell;
use std::rc::Rc;

use crate::context::Context;
use crate::errno::{EINVAL, ENOMEM, ENOSPC, ERANGE};
use crate::field::{FIELDFL_DETAIL, FIELDFL_EYECANDY, FIELDFL_NUMBER, Field};
use crate::item::{ItemData, LabelItem};
use crate::label::{Label, LabelData, LabelOps, Located};
use crate::partition::{Partition, UNDEF_BOOT};
use crate::parttype::{PartType, Shortcut};
use crate::utils::{div, rem, strtoul, u64_as_u32};
use crate::{
    DOS_FLAG_ACTIVE, Geometry, LABEL_FL_REQUIRE_GEOMETRY, LabelType, Sector, fieldtype, is_undef,
};

/// `MAXIMUM_PARTS`: the most partitions, primaries and logicals.
const MAXIMUM_PARTS: usize = 60;
/// `ACTIVE_FLAG`: the boot indicator.
const ACTIVE_FLAG: u8 = 0x80;
/// `MBR_PT_OFFSET`: where the four entries start.
const MBR_PT_OFFSET: usize = 0x1be;
/// `MBR_PT_BOOTBITS_SIZE`: the boot code before the disk identifier.
pub const MBR_PT_BOOTBITS_SIZE: u32 = 440;
/// `MBR_DOS_EXTENDED_PARTITION`.
pub const MBR_DOS_EXTENDED_PARTITION: u8 = 0x05;
/// `MBR_W95_EXTENDED_PARTITION`.
pub const MBR_W95_EXTENDED_PARTITION: u8 = 0x0f;
/// `MBR_LINUX_EXTENDED_PARTITION`.
pub const MBR_LINUX_EXTENDED_PARTITION: u8 = 0x85;
/// `MBR_LINUX_DATA_PARTITION`.
pub const MBR_LINUX_DATA_PARTITION: u32 = 0x83;
/// `MBR_GPT_PARTITION`: a protective MBR's type.
pub const MBR_GPT_PARTITION: u8 = 0xee;
/// `UINT_MAX`.
const UINT_MAX: u64 = 0xffff_ffff;

/// `IS_EXTENDED (i)`.
#[must_use]
pub fn is_extended(t: u32) -> bool {
    t == u32::from(MBR_DOS_EXTENDED_PARTITION)
        || t == u32::from(MBR_W95_EXTENDED_PARTITION)
        || t == u32::from(MBR_LINUX_EXTENDED_PARTITION)
}

/// `struct dos_partition`: one 16-byte entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DosPart {
    /// 0x80 when active.
    pub boot_ind: u8,
    /// Begin head.
    pub bh: u8,
    /// Begin sector, and the cylinder's two high bits.
    pub bs: u8,
    /// Begin cylinder's low eight bits.
    pub bc: u8,
    /// The type.
    pub sys_ind: u8,
    /// End head.
    pub eh: u8,
    /// End sector, and the cylinder's two high bits.
    pub es: u8,
    /// End cylinder's low eight bits.
    pub ec: u8,
    /// `start_sect`.
    pub start: u32,
    /// `nr_sects`.
    pub size: u32,
}

impl DosPart {
    /// `mbr_get_partition (buf, slot)` read out.
    #[must_use]
    pub fn read(buf: &[u8], slot: usize) -> Self {
        let off = MBR_PT_OFFSET.wrapping_add(slot.wrapping_mul(16));
        let b = |i: usize| buf.get(off.wrapping_add(i)).copied().unwrap_or(0);
        let le = |i: usize| {
            u32::from_le_bytes([
                b(i),
                b(i.wrapping_add(1)),
                b(i.wrapping_add(2)),
                b(i.wrapping_add(3)),
            ])
        };
        Self {
            boot_ind: b(0),
            bh: b(1),
            bs: b(2),
            bc: b(3),
            sys_ind: b(4),
            eh: b(5),
            es: b(6),
            ec: b(7),
            start: le(8),
            size: le(12),
        }
    }

    /// Written back to `slot` of `buf`.
    pub fn write(&self, buf: &mut [u8], slot: usize) {
        let off = MBR_PT_OFFSET.wrapping_add(slot.wrapping_mul(16));
        let s = self.start.to_le_bytes();
        let z = self.size.to_le_bytes();
        let bytes = [
            self.boot_ind,
            self.bh,
            self.bs,
            self.bc,
            self.sys_ind,
            self.eh,
            self.es,
            self.ec,
            s[0],
            s[1],
            s[2],
            s[3],
            z[0],
            z[1],
            z[2],
            z[3],
        ];
        if let Some(dst) = buf.get_mut(off..off.wrapping_add(16)) {
            dst.copy_from_slice(&bytes);
        }
    }

    /// `dos_partition_sync_chs (p, part_offset, geom_sectors, geom_heads)`:
    /// the C/H/S fields from the LBA ones, capped at cylinder 1023.
    pub fn sync_chs(&mut self, part_offset: u64, geom_sectors: u32, geom_heads: u32) {
        let mut start = part_offset.wrapping_add(u64::from(self.start));
        let mut stop = start.wrapping_add(u64::from(self.size)).wrapping_sub(1);
        let spc = u64::from(geom_heads.wrapping_mul(geom_sectors));
        let gs = u64::from(geom_sectors);
        let gh = u64::from(geom_heads);
        if div(start, spc) > 1023 {
            start = spc.wrapping_mul(1024).wrapping_sub(1);
        }
        if div(stop, spc) > 1023 {
            stop = spc.wrapping_mul(1024).wrapping_sub(1);
        }
        let low = |v: u64| v.to_le_bytes()[0];
        self.bc = low(div(start, spc) & 0xff);
        self.bh = low(rem(div(start, gs), gh));
        self.bs = low((rem(start, gs).wrapping_add(1) & 0x3f) | ((div(start, spc) >> 2) & 0xc0));
        self.ec = low(div(stop, spc) & 0xff);
        self.eh = low(rem(div(stop, gs), gh));
        self.es = low((rem(stop, gs).wrapping_add(1) & 0x3f) | ((div(stop, spc) >> 2) & 0xc0));
    }

    /// `is_used_partition (p)`: a size. (The kernel cares about nothing
    /// else.)
    #[must_use]
    pub fn is_used(&self) -> bool {
        self.size != 0
    }

    /// `is_cleared_partition (p)` for an entry that exists.
    #[must_use]
    pub fn is_cleared(&self) -> bool {
        *self == Self::default()
    }
}

/// `sector (s)`.
fn sector(s: u8) -> u32 {
    u32::from(s & 0x3f)
}

/// `cylinder (s, c)`.
fn cylinder(s: u8, c: u8) -> u32 {
    u32::from(c) | ((u32::from(s) & 0xc0) << 2)
}

/// `mbr_is_valid_magic`.
fn mbr_is_valid_magic(b: &[u8]) -> bool {
    b.get(510) == Some(&0x55) && b.get(511) == Some(&0xaa)
}

/// `mbr_set_magic`.
fn mbr_set_magic(b: &mut [u8]) {
    if let Some(m) = b.get_mut(510..512) {
        m.copy_from_slice(&[0x55, 0xaa]);
    }
}

/// `mbr_get_id`.
fn mbr_get_id(b: &[u8]) -> u32 {
    let g = |i: usize| b.get(i).copied().unwrap_or(0);
    u32::from_le_bytes([g(440), g(441), g(442), g(443)])
}

/// `mbr_set_id`.
fn mbr_set_id(b: &mut [u8], id: u32) {
    if let Some(m) = b.get_mut(440..444) {
        m.copy_from_slice(&id.to_le_bytes());
    }
}

/// A sector buffer: the first sector (shared with the context), or an EBR.
type SectorBuf = Rc<RefCell<Vec<u8>>>;

/// `struct pte`: one partition's place on the disk.
#[derive(Clone, Debug, Default)]
pub struct Pte {
    /// The slot of the partition's entry in the buffer.
    pt_entry: Option<usize>,
    /// The slot of the EBR's link in the buffer.
    ex_entry: Option<usize>,
    /// The sector the buffer is from.
    offset: Sector,
    /// The sector's contents.
    sectorbuffer: Option<SectorBuf>,
    /// To be written.
    changed: bool,
    /// The buffer is the pte's own (an EBR), not the first sector.
    private_sectorbuffer: bool,
}

impl Pte {
    /// The entry in `slot`, if the pte has a buffer.
    fn entry(&self, slot: Option<usize>) -> Option<DosPart> {
        let slot = slot?;
        let buf = self.sectorbuffer.as_ref()?;
        Some(DosPart::read(&buf.borrow(), slot))
    }

    /// The partition's entry (`pe->pt_entry`).
    fn pt(&self) -> Option<DosPart> {
        self.entry(self.pt_entry)
    }

    /// The EBR's link (`pe->ex_entry`).
    fn ex(&self) -> Option<DosPart> {
        self.entry(self.ex_entry)
    }

    /// Writes `e` to `slot`.
    fn put(&self, slot: Option<usize>, e: &DosPart) {
        if let (Some(slot), Some(buf)) = (slot, self.sectorbuffer.as_ref()) {
            e.write(&mut buf.borrow_mut(), slot);
        }
    }

    /// Changes the partition's entry.
    fn update_pt(&self, f: impl FnOnce(&mut DosPart)) {
        if let Some(mut e) = self.pt() {
            f(&mut e);
            self.put(self.pt_entry, &e);
        }
    }

    /// `get_abs_partition_start (pe)`.
    fn abs_start(&self) -> Sector {
        self.offset
            .wrapping_add(u64::from(self.pt().map_or(0, |p| p.start)))
    }

    /// `get_abs_partition_end (pe)`.
    fn abs_end(&self) -> Sector {
        let size = u64::from(self.pt().map_or(0, |p| p.size));
        self.abs_start()
            .wrapping_add(size)
            .wrapping_sub(u64::from(size != 0))
    }

    /// `is_cleared_partition (pe->pt_entry)`: false for a missing entry, as
    /// upstream's test of a NULL pointer is.
    fn pt_cleared(&self) -> bool {
        self.pt().is_some_and(|p| p.is_cleared())
    }
}

/// `struct fdisk_dos_label`'s own part.
#[derive(Debug)]
pub struct DosLabel {
    ptes: Vec<Pte>,
    /// Where the extended partition starts.
    ext_offset: Sector,
    /// Which primary it is, when `ext_offset` is set.
    ext_index: usize,
    /// DOS-compatible mode.
    compatible: bool,
    /// The MBR changed outside the table (the identifier).
    non_pt_changed: bool,
}

impl Default for DosLabel {
    fn default() -> Self {
        Self {
            ptes: vec![Pte::default(); MAXIMUM_PARTS],
            ext_offset: 0,
            ext_index: 0,
            compatible: false,
            non_pt_changed: false,
        }
    }
}

/// The current label's DOS data (`self_label`).
fn l(cxt: &Context) -> Option<&DosLabel> {
    match &cxt.label_ref()?.data {
        LabelData::Dos(d) => Some(d),
        _ => None,
    }
}

/// The current label's DOS data, for changing.
fn lm(cxt: &mut Context) -> Option<&mut DosLabel> {
    match &mut cxt.label_mut()?.data {
        LabelData::Dos(d) => Some(d),
        _ => None,
    }
}

/// `self_pte (cxt, i)`, copied out (it shares its buffer).
fn pte(cxt: &Context, i: usize) -> Option<Pte> {
    l(cxt)?.ptes.get(i).cloned()
}

/// Changes pte `i`.
fn pte_mut(cxt: &mut Context, i: usize, f: impl FnOnce(&mut Pte)) {
    if let Some(pe) = lm(cxt).and_then(|l| l.ptes.get_mut(i)) {
        f(pe);
    }
}

/// `self_partition (cxt, i)`.
fn partition(cxt: &Context, i: usize) -> Option<DosPart> {
    pte(cxt, i)?.pt()
}

/// The label's `nparts_max`.
fn nmax(cxt: &Context) -> usize {
    cxt.label_ref().map_or(0, |l| l.nparts_max)
}

/// Sets the label's `nparts_max`.
fn set_nmax(cxt: &mut Context, n: usize) {
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_max = n;
    }
}

/// Marks the label changed.
fn set_label_changed(cxt: &mut Context) {
    if let Some(lb) = cxt.label_mut() {
        lb.changed = true;
    }
}

/// `ext_offset` and `ext_index`.
fn ext(cxt: &Context) -> (Sector, usize) {
    l(cxt).map_or((0, 0), |l| (l.ext_offset, l.ext_index))
}

/// `is_dos_compatible (cxt)`.
fn is_dos_compatible(cxt: &Context) -> bool {
    cxt.is_labeltype(LabelType::Dos) && l(cxt).is_some_and(|l| l.compatible)
}

/// `alignment_required (cxt)`.
fn alignment_required(cxt: &Context) -> bool {
    cxt.grain != cxt.sector_size
}

/// The geometry's sectors as the `unsigned int` the C/H/S code takes.
fn gsect(cxt: &Context) -> u32 {
    u64_as_u32(cxt.geom.sectors)
}

/// `fdisk_dos_get_partition (cxt, i)`: entry `i`, for the BSD driver.
#[must_use]
pub fn get_partition_entry(cxt: &Context, i: usize) -> Option<DosPart> {
    partition(cxt, i)
}

/// Writes entry `i` back, for the BSD driver.
pub fn set_partition_entry(cxt: &Context, i: usize, e: &DosPart) {
    if let Some(pe) = pte(cxt, i) {
        pe.put(pe.pt_entry, e);
    }
}

/// `dos_partition_parttype (cxt, p)`: the type, known or not.
fn dos_partition_parttype(cxt: &Context, p: &DosPart) -> PartType {
    cxt.label_ref()
        .and_then(|lb| lb.get_parttype_from_code(u32::from(p.sys_ind)))
        .cloned()
        .unwrap_or_else(|| PartType::new_unknown(u32::from(p.sys_ind), None))
}

/// `partition_set_changed (cxt, i, changed)`.
pub fn partition_set_changed(cxt: &mut Context, i: usize, changed: bool) {
    if i >= MAXIMUM_PARTS {
        return;
    }
    pte_mut(cxt, i, |pe| pe.changed = changed);
    if changed {
        set_label_changed(cxt);
    }
}

/// `get_partition_unused_primary (cxt, pa, &partno)`.
fn get_partition_unused_primary(cxt: &mut Context, pa: Option<&Partition>) -> Result<usize, i32> {
    let org = nmax(cxt);
    set_nmax(cxt, 4);
    let rc = cxt.partition_next_partno(pa);
    set_nmax(cxt, org);
    match rc {
        Ok(n) => Ok(n),
        Err(1) => {
            cxt.info(b"All primary partitions have been defined already.");
            Err(-1)
        }
        Err(rc) if rc == -ERANGE => {
            cxt.warnx(b"Primary partition not available.");
            Err(rc)
        }
        Err(rc) => Err(rc),
    }
}

/// `read_sector (cxt, secno, buf)`: 0, the negated errno, or -1 for a
/// short read; and the errno for a warning.
fn read_sector(cxt: &Context, secno: Sector, buf: &mut [u8]) -> (i32, i32) {
    use std::io::{Read, Seek, SeekFrom};
    let Some(dev) = cxt.dev.as_ref() else {
        return (-EINVAL, EINVAL);
    };
    let mut f = &**dev;
    if let Err(e) = f.seek(SeekFrom::Start(secno.wrapping_mul(cxt.sector_size))) {
        let n = crate::utils::errno_of(&e);
        return (n.wrapping_neg(), n);
    }
    match f.read(buf) {
        Ok(n) if n == buf.len() => (0, 0),
        Ok(_) => (-1, 0),
        Err(e) => {
            let n = crate::utils::errno_of(&e);
            (n.wrapping_neg(), n)
        }
    }
}

/// `read_pte (cxt, pno, offset)`: the EBR at `offset` into pte `pno`.
fn read_pte(cxt: &mut Context, pno: usize, offset: Sector) -> i32 {
    if pno >= MAXIMUM_PARTS {
        return -EINVAL;
    }
    let ss = usize::try_from(cxt.sector_size).unwrap_or(0);
    let mut buf = Vec::new();
    if buf.try_reserve_exact(ss).is_err() {
        return -ENOMEM;
    }
    buf.resize(ss, 0);
    let (rc, errnum) = read_sector(cxt, offset, &mut buf);
    let sb = Rc::new(RefCell::new(buf));
    pte_mut(cxt, pno, |pe| {
        pe.offset = offset;
        pe.sectorbuffer = Some(sb);
        pe.private_sectorbuffer = true;
    });
    if rc != 0 {
        cxt.warn(
            errnum,
            format!("Failed to read extended partition table (offset={offset})").as_bytes(),
        );
        return rc;
    }
    pte_mut(cxt, pno, |pe| {
        pe.changed = false;
        pe.pt_entry = None;
        pe.ex_entry = None;
    });
    0
}

/// `clear_partition (p)`.
fn clear(pe: &Pte, slot: Option<usize>) {
    pe.put(slot, &DosPart::default());
}

/// `dos_init (cxt)`: the four primaries from the first sector, and the
/// warnings a new label deserves.
fn dos_init(cxt: &mut Context) {
    set_nmax(cxt, 4);
    let fs = cxt.firstsector.clone();
    if let Some(l) = lm(cxt) {
        l.ext_index = 0;
        l.ext_offset = 0;
        l.non_pt_changed = false;
        l.ptes = vec![Pte::default(); MAXIMUM_PARTS];
        for (i, pe) in l.ptes.iter_mut().take(4).enumerate() {
            pe.pt_entry = Some(i);
            pe.sectorbuffer.clone_from(&fs);
        }
    }
    if cxt.is_listonly() {
        return;
    }
    if cxt.missing_geometry() {
        cxt.warnx(b"You can set geometry from the extra functions menu.");
    }
    if is_dos_compatible(cxt) {
        cxt.warnx(b"DOS-compatible mode is deprecated.");
        if cxt.sector_size != cxt.phy_sector_size {
            cxt.info(
                b"The device presents a logical sector size that is smaller than the physical sector size. \
                  Aligning to a physical sector (or optimal I/O) size boundary is recommended, or performance \
                  may be impacted.",
            );
        }
    }
    if cxt.use_cylinders() {
        cxt.warnx(b"Cylinders as display units are deprecated.");
    }
    if cxt.total_sectors > UINT_MAX {
        let bytes = cxt.total_sectors.wrapping_mul(cxt.sector_size);
        let szstr = ulstrutils::size_to_human_string(
            ulstrutils::SIZE_SUFFIX_SPACE | ulstrutils::SIZE_SUFFIX_3LETTER,
            bytes,
        );
        cxt.warnx(
            format!(
                "The size of this disk is {szstr} ({bytes} bytes). DOS partition table format cannot be used on \
                 drives for volumes larger than {} bytes for {}-byte sectors. Use GUID partition table format \
                 (GPT).",
                UINT_MAX.wrapping_mul(cxt.sector_size),
                cxt.sector_size
            )
            .as_bytes(),
        );
    }
}

/// `dos_deinit (lb)`.
fn dos_deinit(lb: &mut Label) {
    if let LabelData::Dos(d) = &mut lb.data {
        d.ptes = vec![Pte::default(); MAXIMUM_PARTS];
    }
}

/// `reset_pte (pe)`.
fn reset_pte(cxt: &mut Context, i: usize) {
    pte_mut(cxt, i, |pe| *pe = Pte::default());
}

/// `delete_partition (cxt, partnum)`.
fn delete_partition(cxt: &mut Context, partnum: usize) -> i32 {
    let Some(pe) = pte(cxt, partnum) else {
        return -EINVAL;
    };
    let (ext_offset, ext_index) = ext(cxt);
    let mut partnum = partnum;

    if partnum < 4 {
        if pe.pt().is_some_and(|p| is_extended(u32::from(p.sys_ind))) && partnum == ext_index {
            for i in 4..nmax(cxt) {
                reset_pte(cxt, i);
            }
            set_nmax(cxt, 4);
            pte_mut(cxt, ext_index, |p| p.ex_entry = None);
            if let Some(l) = lm(cxt) {
                l.ext_offset = 0;
                l.ext_index = 0;
            }
        }
        partition_set_changed(cxt, partnum, true);
        clear(&pe, pe.pt_entry);
    } else if pe.ex().is_some_and(|q| q.sys_ind == 0) && partnum > 4 {
        // The last in the chain.
        reset_pte(cxt, partnum);
        set_nmax(cxt, nmax(cxt).wrapping_sub(1));
        partnum = partnum.wrapping_sub(1);
        // The link to the deleted one, cleared.
        if let Some(prev) = pte(cxt, partnum) {
            clear(&prev, prev.ex_entry);
        }
        partition_set_changed(cxt, partnum, true);
    } else {
        // Move the rest down.
        if partnum > 4 {
            if let (Some(prev), Some(q)) = (pte(cxt, partnum.wrapping_sub(1)), pe.ex()) {
                let mut p = q;
                p.sync_chs(pe.offset, gsect(cxt), cxt.geom.heads);
                prev.put(prev.ex_entry, &p);
            }
            partition_set_changed(cxt, partnum.wrapping_sub(1), true);
        } else if nmax(cxt) > 5 {
            // The first logical's link.
            if let Some(second) = pte(cxt, 5) {
                let abs = second.abs_start();
                second.update_pt(|p| p.start = u64_as_u32(abs.wrapping_sub(ext_offset)));
            }
            pte_mut(cxt, 5, |p| p.offset = ext_offset);
            // Upstream syncs the deleted partition's own entry here, which is
            // then reset with it.
            pe.update_pt(|p| p.sync_chs(ext_offset, gsect(cxt), cxt.geom.heads));
            partition_set_changed(cxt, 5, true);
        }

        if nmax(cxt) > 5 {
            let n = nmax(cxt).wrapping_sub(1);
            set_nmax(cxt, n);
            reset_pte(cxt, partnum);
            if let Some(l) = lm(cxt) {
                while partnum < n {
                    if let Some(next) = l.ptes.get(partnum.wrapping_add(1)).cloned() {
                        if let Some(p) = l.ptes.get_mut(partnum) {
                            *p = next;
                        }
                    }
                    partnum = partnum.wrapping_add(1);
                }
                if let Some(p) = l.ptes.get_mut(partnum) {
                    *p = Pte::default();
                }
            }
        } else {
            // The only logical: cleared, not removed.
            if let Some(p) = pte(cxt, partnum) {
                clear(&p, p.pt_entry);
            }
            set_nmax(cxt, nmax(cxt).wrapping_sub(1));
            if partnum == 4 {
                reset_pte(cxt, partnum);
                partition_set_changed(cxt, ext_index, true);
            }
        }
    }
    set_label_changed(cxt);
    0
}

/// `dos_delete_partition (cxt, partnum)`.
fn dos_delete_partition(cxt: &mut Context, partnum: usize) -> i32 {
    match pte(cxt, partnum) {
        Some(pe) if pe.pt().is_some_and(|p| p.is_used()) => delete_partition(cxt, partnum),
        _ => -EINVAL,
    }
}

/// `read_extended (cxt, ext)`: the chain of EBRs from primary `ext`.
fn read_extended(cxt: &mut Context, ext_i: usize) {
    if let Some(l) = lm(cxt) {
        l.ext_index = ext_i;
    }
    let Some(pex) = pte(cxt, ext_i) else { return };
    pte_mut(cxt, ext_i, |p| p.ex_entry = p.pt_entry);

    // `p`: the link being followed -- (pte, slot).
    let mut p = (ext_i, pex.pt_entry);
    if pex.pt().is_none_or(|e| e.start == 0) {
        cxt.warnx(b"Bad offset in primary extended partition.");
        return;
    }
    let entry_of = |cxt: &Context, (i, slot): (usize, Option<usize>)| {
        pte(cxt, i).and_then(|pe| pe.entry(slot))
    };

    while entry_of(cxt, p).is_some_and(|e| is_extended(u32::from(e.sys_ind))) {
        let n = nmax(cxt);
        if n >= MAXIMUM_PARTS {
            // Not a Linux restriction: this program's arrays are that size.
            cxt.warnx(
                format!("Omitting partitions after #{n}. They will be deleted if you save this partition table.")
                    .as_bytes(),
            );
            if let Some(pre) = pte(cxt, n.wrapping_sub(1)) {
                clear(&pre, pre.ex_entry);
                partition_set_changed(cxt, n.wrapping_sub(1), true);
            }
            return;
        }
        let pstart = entry_of(cxt, p).map_or(0, |e| e.start);
        let (ext_offset, _) = ext(cxt);
        if read_pte(cxt, n, ext_offset.wrapping_add(u64::from(pstart))) != 0 {
            return;
        }
        if ext_offset == 0 {
            if let Some(l) = lm(cxt) {
                l.ext_offset = u64::from(pstart);
            }
        }
        let Some(pe) = pte(cxt, n) else { return };
        let mut pt_entry = None;
        let mut ex_entry = None;
        for slot in 0..4 {
            let Some(e) = pe.entry(Some(slot)) else {
                continue;
            };
            if e.size == 0 {
                continue;
            }
            if is_extended(u32::from(e.sys_ind)) {
                if ex_entry.is_some() {
                    cxt.warnx(
                        format!(
                            "Extra link pointer in partition table {}.",
                            n.wrapping_add(1)
                        )
                        .as_bytes(),
                    );
                } else {
                    ex_entry = Some(slot);
                }
            } else if e.sys_ind != 0 {
                if pt_entry.is_some() {
                    cxt.warnx(
                        format!(
                            "Ignoring extra data in partition table {}.",
                            n.wrapping_add(1)
                        )
                        .as_bytes(),
                    );
                } else {
                    pt_entry = Some(slot);
                }
            }
        }
        // "very strange code here..."
        if pt_entry.is_none() {
            pt_entry = Some(if ex_entry == Some(0) { 1 } else { 0 });
        }
        if ex_entry.is_none() {
            ex_entry = Some(if pt_entry == Some(0) { 1 } else { 0 });
        }
        pte_mut(cxt, n, |pe| {
            pe.pt_entry = pt_entry;
            pe.ex_entry = ex_entry;
        });
        p = (n, ex_entry);
        let n1 = n.wrapping_add(1);
        set_nmax(cxt, n1);
        if let Some(lb) = cxt.label_mut() {
            lb.nparts_cur = n1;
        }
    }

    // The last EBR, if empty, removed.
    let n = nmax(cxt);
    if let Some(pe) = pte(cxt, n.wrapping_sub(1)) {
        let ex_cleared = pe.ex().is_some_and(|e| e.is_cleared());
        if ex_cleared && pe.pt_cleared() {
            reset_pte(cxt, n.wrapping_sub(1));
            set_nmax(cxt, n.wrapping_sub(1));
            if let Some(lb) = cxt.label_mut() {
                lb.nparts_cur = lb.nparts_cur.wrapping_sub(1);
            }
        }
    }

    // Empty links removed.
    'remove: loop {
        let q = partition(cxt, 4);
        for i in 4..nmax(cxt) {
            let p = partition(cxt, i);
            if p.is_some_and(|p| p.size == 0)
                && (nmax(cxt) > 5 || q.is_some_and(|q| q.sys_ind != 0))
            {
                cxt.info(format!("omitting empty partition ({})", i.wrapping_add(1)).as_bytes());
                delete_partition(cxt, i);
                // The numbering changed.
                continue 'remove;
            }
        }
        break;
    }
}

/// `ul_random_get_bytes (&id, sizeof (id))`.
fn random_u32() -> u32 {
    let u = crate::uuid::generate_random();
    u32::from_le_bytes([u[0], u[1], u[2], u[3]])
}

/// `dos_create_disklabel (cxt)`.
fn dos_create_disklabel(cxt: &mut Context) -> i32 {
    let mut id: u32 = 0;
    let mut has_id = false;
    if let Some(script) = cxt.script.clone() {
        if let Some(s) = script.borrow().get_header(b"label-id") {
            let (v, end, err) = strtoul(s, 16);
            if err == 0 && end > 0 {
                has_id = true;
                id = u64_as_u32(v);
            }
        }
    }
    if !has_id {
        id = random_u32();
    }
    let rc = if cxt.has_protected_bootbits() {
        cxt.init_firstsector_buffer(0, MBR_PT_BOOTBITS_SIZE)
    } else {
        cxt.init_firstsector_buffer(0, 0)
    };
    if rc != 0 {
        return rc;
    }
    dos_init(cxt);
    cxt.with_firstsector(|b| mbr_set_id(b, id));
    if let Some(l) = lm(cxt) {
        l.non_pt_changed = true;
    }
    set_label_changed(cxt);
    cxt.with_firstsector(|b| mbr_set_magic(b));
    cxt.info(
        format!("Created a new DOS (MBR) disklabel with disk identifier 0x{id:08x}.").as_bytes(),
    );
    0
}

/// `dos_set_disklabel_id (cxt, str)`.
fn dos_set_disklabel_id(cxt: &mut Context, s: Option<&[u8]>) -> i32 {
    let old = mbr_get_id(&cxt.firstsector());
    let owned;
    let s = match s {
        Some(s) => s,
        None => match cxt.ask_string(b"Enter the new disk identifier") {
            Ok(r) => {
                owned = r.unwrap_or_default();
                &owned
            }
            Err(_) => return -EINVAL,
        },
    };
    let (v, end, err) = strtoul(s, 0);
    if err != 0 || end == 0 || end != s.len() {
        cxt.warnx(b"Incorrect value.");
        return -EINVAL;
    }
    let id = u64_as_u32(v);
    cxt.with_firstsector(|b| mbr_set_id(b, id));
    if let Some(l) = lm(cxt) {
        l.non_pt_changed = true;
    }
    set_label_changed(cxt);
    cxt.info(format!("Disk identifier changed from 0x{old:08x} to 0x{id:08x}.").as_bytes());
    0
}

/// `chs_div_minus (a1, a2, b1, b2)`.
fn chs_div_minus(a1: u32, a2: u32, b1: u32, b2: u32) -> u32 {
    let (a, b) = if a1 > a2 && b1 > b2 {
        (a1.wrapping_sub(a2), b1.wrapping_sub(b2))
    } else if a2 > a1 && b2 > b1 {
        (a2.wrapping_sub(a1), b2.wrapping_sub(b1))
    } else {
        return 0;
    };
    if a.checked_rem(b).unwrap_or(1) != 0 {
        return 0;
    }
    a.checked_div(b).unwrap_or(0)
}

/// `chs_overflowed (c, h, s)`: 1023/254/63 or 1023/255/63.
fn chs_overflowed(c: u32, h: u32, s: u32) -> bool {
    c == 1023 && (h == 254 || h == 255) && s == 63
}

/// `lba_overflowed (start, sects)`.
fn lba_overflowed(start: Sector, sects: Sector) -> bool {
    start.wrapping_add(sects.wrapping_sub(1)) > UINT_MAX
}

/// `get_partition_table_geometry (cxt, &h, &s)`: the heads and sectors the
/// table's C/H/S values were computed with, from pairs of (C/H/S, LBA)
/// tuples; `None` when they do not say.
fn get_partition_table_geometry(cxt: &Context) -> Option<(u32, u32)> {
    #[derive(Clone, Copy, Default)]
    struct T {
        c: u32,
        h: u32,
        o: u32,
        v: bool,
    }
    // Upstream leaves the tuples of unused entries uninitialized; here
    // they are invalid, which is what they are meant to be.
    let mut t = [T::default(); 8];
    let set = |c: u32, h: u32, s: u32, l: u32| T {
        c,
        h,
        o: l.wrapping_sub(s.wrapping_sub(1)),
        v: !chs_overflowed(c, h, s) && s != 0 && s.wrapping_sub(1) <= l,
    };
    let fs = cxt.firstsector();
    for i in 0..4 {
        let p = DosPart::read(&fs, i);
        if p.sys_ind == 0 {
            continue;
        }
        let mut l = p.start;
        if let Some(x) = t.get_mut(i.wrapping_mul(2)) {
            *x = set(cylinder(p.bs, p.bc), u32::from(p.bh), sector(p.bs), l);
        }
        let sects = p.size;
        if sects == 0 || lba_overflowed(u64::from(l), u64::from(sects)) {
            continue;
        }
        l = l.wrapping_add(sects.wrapping_sub(1));
        if let Some(x) = t.get_mut(i.wrapping_mul(2).wrapping_add(1)) {
            *x = set(cylinder(p.es, p.ec), u32::from(p.eh), sector(p.es), l);
        }
    }
    let (mut hh, mut ss) = (0u32, 0u32);
    'outer: for dif in 1..8usize {
        for i in 0..8usize.saturating_sub(dif) {
            let j = i.wrapping_add(dif);
            let (Some(ti), Some(tj)) = (t.get(i).copied(), t.get(j).copied()) else {
                continue;
            };
            if !ti.v || !tj.v {
                continue;
            }
            let n1 = ti.o.wrapping_mul(tj.h);
            let n2 = tj.o.wrapping_mul(ti.h);
            let n3 = tj.o.wrapping_mul(ti.c);
            let n4 = ti.o.wrapping_mul(tj.c);
            let n5 = ti.c.wrapping_mul(tj.h);
            let n6 = tj.c.wrapping_mul(ti.h);
            if hh == 0 && n1 != n2 && n3 != n4 {
                let h = chs_div_minus(n1, n2, n3, n4);
                if h > 0 && h <= 256 {
                    hh = h;
                }
            }
            if ss == 0 && n3 != n4 && n5 != n6 {
                let s = chs_div_minus(n3, n4, n5, n6);
                if s > 0 && s <= 63 {
                    ss = s;
                }
            }
            if hh != 0 && ss != 0 {
                break 'outer;
            }
        }
    }
    (hh != 0 && ss != 0).then_some((hh, ss))
}

/// `dos_reset_alignment (cxt)`: the deprecated DOS-compatible first LBA
/// (a track) and grain (a sector).
fn dos_reset_alignment(cxt: &mut Context) -> i32 {
    if is_dos_compatible(cxt) {
        if cxt.geom.sectors != 0 {
            cxt.first_lba = cxt.geom.sectors;
        }
        cxt.grain = cxt.sector_size;
    }
    0
}

/// `AIX_MAGIC_STRING`.
const AIX_MAGIC: [u8; 4] = [0xc9, 0xc2, 0xd4, 0xc1];

/// `dos_probe_label (cxt)`.
fn dos_probe_label(cxt: &mut Context) -> i32 {
    let fs = cxt.firstsector();
    // Disks with the AIX magic are not ours.
    if fs.get(..4) == Some(AIX_MAGIC.as_slice()) {
        return 0;
    }
    if !mbr_is_valid_magic(&fs) {
        return 0;
    }
    dos_init(cxt);
    if let Some((h, s)) = get_partition_table_geometry(cxt) {
        cxt.geom.heads = h;
        cxt.geom.sectors = u64::from(s);
        cxt.geom.cylinders = div(cxt.total_sectors, u64::from(h).wrapping_mul(u64::from(s)));
        if cxt.has_user_device_geometry() {
            cxt.apply_user_device_properties();
        }
    }
    for i in 0..4 {
        let Some(p) = partition(cxt, i) else { continue };
        if p.is_used() {
            if let Some(lb) = cxt.label_mut() {
                lb.nparts_cur = lb.nparts_cur.wrapping_add(1);
            }
        }
        if is_extended(u32::from(p.sys_ind)) {
            if nmax(cxt) != 4 {
                cxt.warnx(
                    format!("Ignoring extra extended partition {}", i.wrapping_add(1)).as_bytes(),
                );
            } else {
                read_extended(cxt, i);
            }
        }
    }
    for i in 3..nmax(cxt) {
        let Some(pe) = pte(cxt, i) else { continue };
        let buf = pe
            .sectorbuffer
            .as_ref()
            .map(|b| b.borrow().clone())
            .unwrap_or_default();
        if !mbr_is_valid_magic(&buf) {
            let b510 = buf.get(510).copied().unwrap_or(0);
            let b511 = buf.get(511).copied().unwrap_or(0);
            cxt.info(
                format!(
                    "Invalid flag 0x{b510:02x}{b511:02x} of EBR (for partition {}) will be corrected by w(rite).",
                    i.wrapping_add(1)
                )
                .as_bytes(),
            );
            partition_set_changed(cxt, i, true);
            // The extended too, to update the first EBR when there are no
            // logicals at all.
            let (_, ext_index) = ext(cxt);
            partition_set_changed(cxt, ext_index, true);
        }
    }
    1
}

/// `set_partition (cxt, i, doext, start, stop, sysid, boot)`: entry `i`
/// (or, with `doext`, its EBR's link) set from absolute sectors.
fn set_partition(
    cxt: &mut Context,
    i: usize,
    doext: bool,
    start: Sector,
    stop: Sector,
    sysid: u32,
    boot: bool,
) {
    let Some(pe) = pte(cxt, i) else { return };
    let (ext_offset, _) = ext(cxt);
    let (slot, offset) = if doext {
        (pe.ex_entry, ext_offset)
    } else {
        (pe.pt_entry, pe.offset)
    };
    let Some(mut p) = pe.entry(slot) else { return };
    p.boot_ind = if boot { ACTIVE_FLAG } else { 0 };
    p.sys_ind = sysid.to_le_bytes()[0];
    p.start = u64_as_u32(start.wrapping_sub(offset));
    p.size = u64_as_u32(stop.wrapping_sub(start).wrapping_add(1));
    p.sync_chs(offset, gsect(cxt), cxt.geom.heads);
    pe.put(slot, &p);
    partition_set_changed(cxt, i, true);
}

/// `get_start_from_user (cxt, &start, low, dflt, limit, pa)`.
fn get_start_from_user(
    cxt: &mut Context,
    low: Sector,
    dflt: Sector,
    limit: Sector,
    pa: Option<&Partition>,
) -> Result<Sector, i32> {
    if let Some(pa) = pa {
        if pa.start_follow_default {
            return Ok(dflt);
        }
        if pa.has_start() {
            let start = pa.start;
            if start < low || start > limit {
                cxt.warnx(format!("Start sector {start} out of range.").as_bytes());
                return Err(-ERANGE);
            }
            return Ok(start);
        }
    }
    let mut ask = crate::Ask::new(crate::AskType::Number);
    ask.set_query(if cxt.use_cylinders() {
        b"First cylinder".as_slice()
    } else {
        b"First sector"
    });
    ask.number_set_low(cxt.cround(low));
    ask.number_set_default(cxt.cround(dflt));
    ask.number_set_high(cxt.cround(limit));
    let rc = cxt.do_ask(&mut ask);
    let mut start = ask.number_get_result();
    if rc != 0 {
        return Err(rc);
    }
    if cxt.use_cylinders() {
        start = start
            .wrapping_sub(1)
            .wrapping_mul(u64::from(cxt.get_units_per_sector()));
        if start < low {
            start = low;
        }
    }
    Ok(start)
}

/// The used ptes from 0 (or, for a logical, 4) with their absolute
/// extents, an EBR counted as used from a first LBA before its partition.
fn used_extents(cxt: &Context, logical: bool) -> Vec<(Sector, Sector)> {
    let from = if logical { 4 } else { 0 };
    (from..nmax(cxt))
        .filter_map(|i| pte(cxt, i))
        .filter(|pe| !pe.pt_cleared())
        .map(|pe| {
            let mut s = pe.abs_start();
            if pe.offset != 0 {
                s = s.wrapping_sub(cxt.first_lba);
            }
            (s, pe.abs_end())
        })
        .collect()
}

/// `find_last_free (cxt, logical, begin, stop, &result)`: the last sector
/// of the free space at `begin`.
fn find_last_free(
    cxt: &Context,
    logical: bool,
    begin: Sector,
    stop: Sector,
) -> Result<Sector, i32> {
    let mut last = stop;
    let from = if logical { 4 } else { 0 };
    for i in from..nmax(cxt) {
        let Some(pe) = pte(cxt, i) else { continue };
        let mut p_start = pe.abs_start();
        let p_end = pe.abs_end();
        if pe.pt_cleared() {
            continue;
        }
        if pe.offset != 0 {
            p_start = p_start.wrapping_sub(cxt.first_lba);
        }
        if (p_start >= begin && p_start <= last) || (p_end >= begin && p_end <= last) {
            last = p_start.wrapping_sub(1);
        }
        if last < begin {
            return Err(-ENOSPC);
        }
    }
    if last == begin {
        last = stop;
    }
    Ok(last)
}

/// `find_last_free_sector_in_range (cxt, logical, begin, end, &result)`.
fn find_last_free_sector_in_range(
    cxt: &Context,
    logical: bool,
    begin: Sector,
    end: Sector,
) -> Result<Sector, i32> {
    let mut last = end;
    loop {
        let mut moved = false;
        for (p_start, p_end) in used_extents(cxt, logical) {
            if last >= p_start && last <= p_end {
                last = p_start.wrapping_sub(1);
                moved = true;
                if last < begin {
                    return Err(-ENOSPC);
                }
            }
        }
        if !moved {
            return Ok(last);
        }
    }
}

/// `find_first_free_sector_in_range (cxt, logical, begin, end, &result)`.
fn find_first_free_sector_in_range(
    cxt: &Context,
    logical: bool,
    begin: Sector,
    end: Sector,
) -> Result<Sector, i32> {
    let mut first = begin;
    loop {
        let mut moved = false;
        for (p_start, p_end) in used_extents(cxt, logical) {
            if first < p_start {
                continue;
            }
            if first <= p_end {
                first = p_end
                    .wrapping_add(1)
                    .wrapping_add(if logical { cxt.first_lba } else { 0 });
                moved = true;
                if first > end {
                    return Err(-ENOSPC);
                }
            }
        }
        if !moved {
            return Ok(first);
        }
    }
}

/// `get_disk_ranges (cxt, logical, &first, &last)`.
fn get_disk_ranges(cxt: &Context, logical: bool) -> Result<(Sector, Sector), i32> {
    if logical {
        let (ext_offset, ext_index) = ext(cxt);
        if ext_offset == 0 {
            return Err(-EINVAL);
        }
        let ext_pe = pte(cxt, ext_index).ok_or(-EINVAL)?;
        return Ok((ext_offset.wrapping_add(cxt.first_lba), ext_pe.abs_end()));
    }
    let mut last = if cxt.use_cylinders() || cxt.total_sectors == 0 {
        u64::from(cxt.geom.heads)
            .wrapping_mul(cxt.geom.sectors)
            .wrapping_mul(cxt.geom.cylinders)
            .wrapping_sub(1)
    } else {
        cxt.total_sectors.wrapping_sub(1)
    };
    if last > UINT_MAX {
        last = UINT_MAX;
    }
    Ok((cxt.first_lba, last))
}

/// `find_first_free_sector (cxt, logical, start, &result)`.
fn find_first_free_sector(cxt: &Context, logical: bool, start: Sector) -> Result<Sector, i32> {
    let (_, last) = get_disk_ranges(cxt, logical)?;
    find_first_free_sector_in_range(cxt, logical, start, last)
}

/// Whether the parent context is a GPT (a hybrid MBR being edited).
fn parent_is_gpt(cxt: &Context) -> bool {
    cxt.get_parent()
        .is_some_and(|p| p.is_labeltype(LabelType::Gpt))
}

/// `add_partition (cxt, n, pa)`: partition `n` created, asking for the
/// first and last sector unless `pa` says.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's one function, kept whole to compare"
)]
fn add_partition(cxt: &mut Context, n: usize, pa: Option<&Partition>) -> i32 {
    let sys = pa
        .and_then(|p| p.ty.as_ref())
        .map_or(MBR_LINUX_DATA_PARTITION, |t| t.code);
    let is_logical = n >= 4;
    let has_script = cxt.script.is_some();

    if partition(cxt, n).is_some_and(|p| p.is_used()) {
        cxt.warnx(
            format!(
                "Partition {} is already defined.  Delete it before re-adding it.",
                n.wrapping_add(1)
            )
            .as_bytes(),
        );
        return -EINVAL;
    }
    let (mut start, mut stop) = match get_disk_ranges(cxt, is_logical) {
        Ok(r) => r,
        Err(rc) => return rc,
    };
    if !is_logical && parent_is_gpt(cxt) {
        // A hybrid MBR.
        start = 1;
    }
    let mut limit = match find_last_free_sector_in_range(cxt, is_logical, start, stop) {
        Ok(l) => l,
        Err(rc) => {
            if rc == -ENOSPC {
                cxt.warnx(b"No free sectors available.");
            }
            return rc;
        }
    };
    let (ext_offset, _) = ext(cxt);
    if (is_logical || !parent_is_gpt(cxt))
        && has_script
        && pa.is_some_and(|p| {
            p.has_start() && p.start >= (if is_logical { ext_offset } else { 1 }) && p.start < start
        })
    {
        cxt.set_first_lba(1);
        match get_disk_ranges(cxt, is_logical) {
            Ok((s, e)) => {
                start = s;
                stop = e;
            }
            Err(rc) => return rc,
        }
    }
    let _ = stop;

    // The first sector.
    let mut read = false;
    let mut temp;
    loop {
        temp = start;
        let mut dflt = match find_first_free_sector(cxt, is_logical, start) {
            Ok(d) => d,
            Err(rc) => {
                if rc == -ENOSPC {
                    cxt.warnx(b"No free sectors available.");
                }
                return rc;
            }
        };
        start = dflt;
        if n >= 4
            && pa.is_some_and(Partition::has_start)
            && has_script
            && cxt.first_lba > 1
            && temp == start.wrapping_sub(cxt.first_lba)
        {
            cxt.set_first_lba(1);
            start = pa.map_or(start, |p| p.start);
        }
        // The default should be aligned and unused.
        loop {
            let aligned = cxt.align_lba_in_range(dflt, dflt, limit);
            if let Ok(d) = find_first_free_sector(cxt, is_logical, aligned) {
                dflt = d;
            }
            if !(dflt != aligned && dflt > aligned && dflt < limit) {
                break;
            }
        }
        if dflt >= limit {
            dflt = start;
        }
        if start > limit {
            break;
        }
        if start >= temp.wrapping_add(u64::from(cxt.get_units_per_sector())) && read {
            if pa.is_none_or(|p| !p.start_follow_default) {
                cxt.info(format!("Sector {temp} is already allocated.").as_bytes());
            }
            temp = start;
            read = false;
            if pa.is_some_and(Partition::has_start) {
                break;
            }
        }
        if !read && start == temp {
            match get_start_from_user(cxt, temp, dflt, limit, pa) {
                Ok(s) => start = s,
                Err(rc) => return rc,
            }
            read = true;
        }
        if let Some(pa) = pa.filter(|p| p.has_size()) {
            let mut rc = 0;
            match find_last_free(cxt, is_logical, start, limit) {
                Ok(last) => {
                    if last.wrapping_sub(start).wrapping_add(1) < pa.size {
                        if pa.has_start() && pa.start <= last {
                            rc = -ENOSPC;
                        } else {
                            start = last.wrapping_add(1);
                            continue;
                        }
                    }
                }
                Err(e) => rc = e,
            }
            if rc == -ENOSPC {
                cxt.warnx(b"No free sectors available.");
                return rc;
            }
        }
        if start == temp && read {
            break;
        }
    }

    if n == 4 {
        // The first EBR is at the start of the extended partition.
        pte_mut(cxt, n, |pe| pe.offset = ext_offset);
    } else if n > 4 {
        // The second and further EBRs.
        let first_lba = cxt.first_lba;
        let mut off = start.wrapping_sub(first_lba);
        if off == ext_offset {
            // Must be corrected.
            off = off.wrapping_add(1);
            if first_lba == 1 {
                start = start.wrapping_add(1);
            }
        }
        pte_mut(cxt, n, |pe| pe.offset = off);
    }

    match find_last_free(cxt, is_logical, start, limit) {
        Ok(s) => stop = s,
        Err(rc) => {
            if rc == -ENOSPC {
                cxt.warnx(b"No free sectors available.");
            }
            return rc;
        }
    }
    limit = stop;

    // The last sector.
    let mut isrel = false;
    if cxt.cround(start) == cxt.cround(limit) || pa.is_some_and(|p| p.end_follow_default) {
        stop = limit;
    } else if let Some(pa) = pa.filter(|p| p.has_size()) {
        stop = start.wrapping_add(pa.size);
        isrel = !pa.size_explicit;
        if (!isrel || !alignment_required(cxt)) && stop > start {
            stop = stop.wrapping_sub(1);
        }
    } else {
        loop {
            let mut ask = crate::Ask::new(crate::AskType::Offset);
            if cxt.use_cylinders() {
                ask.set_query(b"Last cylinder, +/-cylinders or +/-size{K,M,G,T,P}");
                ask.number_set_unit(
                    cxt.sector_size
                        .wrapping_mul(u64::from(cxt.get_units_per_sector())),
                );
            } else {
                ask.set_query(b"Last sector, +/-sectors or +/-size{K,M,G,T,P}");
                ask.number_set_unit(cxt.sector_size);
            }
            ask.number_set_low(cxt.cround(start));
            ask.number_set_default(cxt.cround(limit));
            ask.number_set_high(cxt.cround(limit));
            ask.number_set_base(cxt.cround(start));
            ask.number_set_wrap_negative(true);
            let rc = cxt.do_ask(&mut ask);
            if rc != 0 {
                return rc;
            }
            stop = ask.number_get_result();
            isrel = ask.number_is_relative();
            if cxt.use_cylinders() {
                stop = stop
                    .wrapping_mul(u64::from(cxt.get_units_per_sector()))
                    .wrapping_sub(1);
                if stop > limit {
                    stop = limit;
                }
            }
            if stop >= start && stop <= limit {
                break;
            }
            cxt.warnx(b"Value out of range.");
        }
    }
    if stop > limit {
        stop = limit;
    }
    if isrel && stop.wrapping_sub(start) < div(cxt.grain, cxt.sector_size) {
        // No smartness on a tiny partition: its end is not aligned.
        isrel = false;
    }
    if stop < limit && isrel && alignment_required(cxt) {
        // A +size end: aligned, so that the next partition starts on a
        // physical boundary.
        stop = cxt.align_lba_in_range(stop, start, limit);
        if stop > start {
            stop = stop.wrapping_sub(1);
        }
        if stop > limit {
            stop = limit;
        }
    }

    let boot = pa.is_some_and(Partition::is_bootable);
    set_partition(cxt, n, false, start, stop, sys, boot);
    if n > 4 {
        let off = pte(cxt, n).map_or(0, |p| p.offset);
        set_partition(
            cxt,
            n.wrapping_sub(1),
            true,
            off,
            stop,
            u32::from(MBR_DOS_EXTENDED_PARTITION),
            false,
        );
    }

    let t = cxt
        .label_ref()
        .and_then(|lb| lb.get_parttype_from_code(sys))
        .cloned();
    let num = i64::try_from(n.wrapping_add(1)).unwrap_or(i64::MAX);
    cxt.info_new_partition(num, start, stop, t.as_ref());

    if is_extended(sys) {
        if let Some(l) = lm(cxt) {
            l.ext_index = n;
            l.ext_offset = start;
        }
        pte_mut(cxt, n, |pe| pe.ex_entry = pe.pt_entry);
    }
    set_label_changed(cxt);
    0
}

/// `add_logical (cxt, pa, &partno)`.
fn add_logical(cxt: &mut Context, pa: Option<&Partition>) -> Result<usize, i32> {
    let n = nmax(cxt);
    if n >= MAXIMUM_PARTS {
        return Err(-EINVAL);
    }
    let ss = usize::try_from(cxt.sector_size).unwrap_or(0);
    pte_mut(cxt, n, |pe| {
        if pe.sectorbuffer.is_none() {
            pe.sectorbuffer = Some(Rc::new(RefCell::new(vec![0u8; ss])));
            pe.private_sectorbuffer = true;
        }
        pe.pt_entry = Some(0);
        pe.ex_entry = Some(1);
        pe.offset = 0;
    });
    partition_set_changed(cxt, n, true);
    set_nmax(cxt, n.wrapping_add(1));

    // Only for the primary/extended/logical dialog, which scripts skip.
    if cxt.script.is_none() {
        cxt.info(format!("Adding logical partition {}", n.wrapping_add(1)).as_bytes());
    }
    let rc = add_partition(cxt, n, pa);
    if rc != 0 {
        set_nmax(cxt, n);
        pte_mut(cxt, n, |pe| {
            pe.pt_entry = None;
            pe.ex_entry = None;
            pe.offset = 0;
            pe.changed = false;
        });
        return Err(rc);
    }
    Ok(n)
}

/// `check (cxt, n, h, s, c, lba_sector)`: one end of a partition, against
/// the geometry (DOS-compatible mode only).
fn check(cxt: &mut Context, n: usize, h: u32, s: u32, c: u32, lba_sector: u32) -> i32 {
    if !is_dos_compatible(cxt) {
        return 0;
    }
    let s8 = s.to_le_bytes()[0];
    let c8 = c.to_le_bytes()[0];
    let real_s = sector(s8).wrapping_sub(1);
    let real_c = cylinder(s8, c8);
    let chs_sector = u64_as_u32(
        u64::from(real_c.wrapping_mul(cxt.geom.heads).wrapping_add(h))
            .wrapping_mul(cxt.geom.sectors)
            .wrapping_add(u64::from(real_s)),
    );
    let mut nerrors: i32 = 0;
    if chs_sector == 0 {
        cxt.warnx(format!("Partition {n}: contains sector 0").as_bytes());
        nerrors = nerrors.wrapping_add(1);
    }
    if h >= cxt.geom.heads {
        cxt.warnx(
            format!(
                "Partition {n}: head {} greater than maximum {}",
                h.wrapping_add(1) as i32,
                cxt.geom.heads as i32
            )
            .as_bytes(),
        );
        nerrors = nerrors.wrapping_add(1);
    }
    if u64::from(real_s) >= cxt.geom.sectors {
        cxt.warnx(
            format!(
                "Partition {n}: sector {} greater than maximum {}",
                real_s.wrapping_add(1) as i32,
                cxt.geom.sectors
            )
            .as_bytes(),
        );
        nerrors = nerrors.wrapping_add(1);
    }
    if u64::from(real_c) >= cxt.geom.cylinders {
        cxt.warnx(
            format!(
                "Partition {n}: cylinder {} greater than maximum {}",
                real_c.wrapping_add(1) as i32,
                cxt.geom.cylinders
            )
            .as_bytes(),
        );
        nerrors = nerrors.wrapping_add(1);
    }
    let spc = u64::from(cxt.geom.heads).wrapping_mul(cxt.geom.sectors);
    if div(u64::from(lba_sector), spc) < 1024 && lba_sector != chs_sector {
        cxt.warnx(
            format!("Partition {n}: LBA sector {lba_sector} disagrees with C/H/S calculated sector {chs_sector}")
                .as_bytes(),
        );
        nerrors = nerrors.wrapping_add(1);
    }
    nerrors
}

/// `long2chs (cxt, ls, &c, &h, &s)`.
fn long2chs(cxt: &Context, ls: u64) -> (u32, u32, u32) {
    // `int spc = heads * sectors`, sign-extended back for the division.
    let spc = crate::utils::u64_as_i32(u64::from(cxt.geom.heads).wrapping_mul(cxt.geom.sectors));
    let spc = u64::from_ne_bytes(i64::from(spc).to_ne_bytes());
    let c = u64_as_u32(div(ls, spc));
    let ls = rem(ls, spc);
    let h = u64_as_u32(div(ls, cxt.geom.sectors));
    let s = u64_as_u32(rem(ls, cxt.geom.sectors).wrapping_add(1));
    (c, h, s)
}

/// `check_consistency (cxt, p, partition)`: the C/H/S fields against the
/// LBA ones (DOS-compatible mode, primaries only).
fn check_consistency(cxt: &mut Context, p: &DosPart, partition: usize) -> i32 {
    if !is_dos_compatible(cxt) {
        return 0;
    }
    if cxt.geom.heads == 0 || cxt.geom.sectors == 0 || partition >= 4 {
        return 0;
    }
    let (pbc, pbh, pbs) = (cylinder(p.bs, p.bc), u32::from(p.bh), sector(p.bs));
    let (pec, peh, pes) = (cylinder(p.es, p.ec), u32::from(p.eh), sector(p.es));
    let (lbc, lbh, lbs) = long2chs(cxt, u64::from(p.start));
    let (lec, leh, les) = long2chs(cxt, u64::from(p.start.wrapping_add(p.size).wrapping_sub(1)));
    let mut nerrors: i32 = 0;
    let pn = partition.wrapping_add(1);
    if lbc < 1024 && (pbc != lbc || pbh != lbh || pbs != lbs) {
        cxt.warnx(
            format!(
                "Partition {pn}: different physical/logical beginnings (non-Linux?): phys=({}, {}, {}), \
                 logical=({}, {}, {})",
                pbc as i32, pbh as i32, pbs as i32, lbc as i32, lbh as i32, lbs as i32
            )
            .as_bytes(),
        );
        nerrors = nerrors.wrapping_add(1);
    }
    if lec < 1024 && (pec != lec || peh != leh || pes != les) {
        cxt.warnx(
            format!(
                "Partition {pn}: different physical/logical endings: phys=({}, {}, {}), logical=({}, {}, {})",
                pec as i32, peh as i32, pes as i32, lec as i32, leh as i32, les as i32
            )
            .as_bytes(),
        );
        nerrors = nerrors.wrapping_add(1);
    }
    if peh != cxt.geom.heads.wrapping_sub(1) || u64::from(pes) != cxt.geom.sectors {
        cxt.warnx(format!("Partition {pn}: does not end on cylinder boundary.").as_bytes());
        nerrors = nerrors.wrapping_add(1);
    }
    nerrors
}

/// `fill_bounds (cxt, first, last)`.
fn fill_bounds(cxt: &Context) -> (Vec<Sector>, Vec<Sector>) {
    let n = nmax(cxt);
    let mut first = vec![0; n];
    let mut last = vec![0; n];
    for i in 0..n {
        let Some(pe) = pte(cxt, i) else { continue };
        let p = pe.pt();
        let (f, l) = if p.is_none_or(|p| p.is_cleared() || is_extended(u32::from(p.sys_ind))) {
            (u64::MAX, 0)
        } else {
            (pe.abs_start(), pe.abs_end())
        };
        if let Some(x) = first.get_mut(i) {
            *x = f;
        }
        if let Some(x) = last.get_mut(i) {
            *x = l;
        }
    }
    (first, last)
}

/// `dos_verify_disklabel (cxt)`.
fn dos_verify_disklabel(cxt: &mut Context) -> i32 {
    let mut total: Sector = 1;
    let n_sectors = cxt.total_sectors;
    let (first, last) = fill_bounds(cxt);
    let fi = |i: usize| first.get(i).copied().unwrap_or(0);
    let la = |i: usize| last.get(i).copied().unwrap_or(0);
    let mut nerrors: i32 = 0;

    for i in 0..nmax(cxt) {
        let Some(pe) = pte(cxt, i) else { continue };
        let Some(p) = pe.pt() else { continue };
        if !p.is_used() || is_extended(u32::from(p.sys_ind)) {
            continue;
        }
        nerrors = nerrors.wrapping_add(check_consistency(cxt, &p, i));
        if pe.abs_start() < fi(i) {
            cxt.warnx(format!("Partition {}: bad start-of-data.", i.wrapping_add(1)).as_bytes());
            nerrors = nerrors.wrapping_add(1);
        }
        let pn = i.wrapping_add(1);
        nerrors = nerrors.wrapping_add(check(
            cxt,
            pn,
            u32::from(p.bh),
            u32::from(p.bs),
            u32::from(p.bc),
            u64_as_u32(fi(i)),
        ));
        nerrors = nerrors.wrapping_add(check(
            cxt,
            pn,
            u32::from(p.eh),
            u32::from(p.es),
            u32::from(p.ec),
            u64_as_u32(la(i)),
        ));
        total = total.wrapping_add(la(i).wrapping_add(1).wrapping_sub(fi(i)));
        if i == 0 {
            total = total.wrapping_add(pe.abs_start().wrapping_sub(1));
        }
        for j in 0..i {
            if (fi(i) >= fi(j) && fi(i) <= la(j)) || (la(i) <= la(j) && la(i) >= fi(j)) {
                cxt.warnx(
                    format!(
                        "Partition {}: overlaps partition {}.",
                        j.wrapping_add(1),
                        i.wrapping_add(1)
                    )
                    .as_bytes(),
                );
                nerrors = nerrors.wrapping_add(1);
                total = total.wrapping_add(if fi(i) >= fi(j) { fi(i) } else { fi(j) });
                total = total.wrapping_sub(if la(i) <= la(j) { la(i) } else { la(j) });
            }
        }
    }

    let (ext_offset, ext_index) = ext(cxt);
    if ext_offset != 0 {
        let e_last = pte(cxt, ext_index).map_or(0, |pe| pe.abs_end());
        for i in 4..nmax(cxt) {
            total = total.wrapping_add(1);
            let Some(p) = partition(cxt, i) else { continue };
            if p.sys_ind == 0 {
                if i != 4 || i.wrapping_add(1) < nmax(cxt) {
                    cxt.warnx(format!("Partition {}: empty.", i.wrapping_add(1)).as_bytes());
                    nerrors = nerrors.wrapping_add(1);
                }
            } else if fi(i) < ext_offset || la(i) > e_last {
                cxt.warnx(
                    format!(
                        "Logical partition {}: not entirely in partition {}.",
                        i.wrapping_add(1),
                        ext_index.wrapping_add(1)
                    )
                    .as_bytes(),
                );
                nerrors = nerrors.wrapping_add(1);
            }
        }
    }

    if nerrors == 0 {
        cxt.info(b"No errors detected.");
        if total > n_sectors {
            cxt.info(
                format!("Total allocated sectors {total} greater than the maximum {n_sectors}.")
                    .as_bytes(),
            );
        } else if total < n_sectors {
            cxt.info(
                format!(
                    "Remaining {} unallocated {}-byte sectors.",
                    n_sectors.wrapping_sub(total),
                    cxt.sector_size
                )
                .as_bytes(),
            );
        }
    } else {
        cxt.warnx(
            if nerrors == 1 {
                format!("{nerrors} error detected.")
            } else {
                format!("{nerrors} errors detected.")
            }
            .as_bytes(),
        );
    }
    nerrors
}

/// `dos_add_partition (cxt, pa, &partno)`: primary or logical, by the
/// template's start and number (rules A-D upstream documents in
/// `fdisk_add_partition`), else by asking.
fn dos_add_partition(
    cxt: &mut Context,
    pa: Option<&mut Partition>,
    partno: Option<&mut usize>,
) -> i32 {
    let mut pa = pa;
    if nmax(cxt) >= MAXIMUM_PARTS {
        cxt.warnx(b"The maximum number of partitions has been created.");
        return -EINVAL;
    }
    let (ext_offset, ext_index) = ext(cxt);
    let ext_pe = if ext_offset != 0 {
        pte(cxt, ext_index)
    } else {
        None
    };
    let ext_end = ext_pe.as_ref().map_or(0, Pte::abs_end);
    let pr = pa.as_deref();
    let ext_type = pr
        .and_then(|p| p.ty.as_ref())
        .is_some_and(|t| is_extended(t.code));

    let result: Result<usize, i32> = if let Some(p) = pr.filter(|p| {
        p.has_start() && ext_pe.is_some() && p.start >= ext_offset && p.start <= ext_end
    }) {
        // A) a start inside the extended partition: a logical.
        if p.has_partno() && p.partno < 4 {
            return -EINVAL;
        }
        add_logical(cxt, pa.as_deref())
    } else if pr.is_some_and(Partition::has_start) && ext_pe.is_some() {
        // B) a start outside it: a primary.
        if pr.is_some_and(|p| p.has_partno() && p.partno >= 4) {
            return -EINVAL;
        }
        if ext_type {
            cxt.warnx(b"Extended partition already exists.");
            return -EINVAL;
        }
        add_primary(cxt, pa.as_deref())
    } else if pr
        .is_some_and(|p| (p.start_is_default() || p.has_start()) && p.has_partno() && p.partno < 4)
    {
        // C) a start (or the default) and a number below 4: a primary.
        if ext_pe.is_some() && ext_type {
            cxt.warnx(b"Extended partition already exists.");
            return -EINVAL;
        }
        add_primary(cxt, pa.as_deref())
    } else if pr
        .is_some_and(|p| (p.start_is_default() || p.has_start()) && p.has_partno() && p.partno >= 4)
    {
        // D) a start (or the default) and a number from 4: a logical.
        if ext_pe.is_none() {
            cxt.warnx(b"Extended partition does not exists. Failed to add logical partition.");
            return -EINVAL;
        }
        if pr.is_some_and(|p| p.has_start() && p.start < ext_offset && p.start > ext_end) {
            return -EINVAL;
        }
        add_logical(cxt, pa.as_deref())
    } else {
        // By dialog; `pa` may still give the type and the like.
        match dialog_add(cxt, &mut pa) {
            Ok(r) => r,
            Err(rc) => return rc,
        }
    };

    match result {
        Ok(res) => {
            if let Some(lb) = cxt.label_mut() {
                lb.nparts_cur = lb.nparts_cur.wrapping_add(1);
            }
            if let Some(partno) = partno {
                *partno = res;
            }
            0
        }
        Err(rc) => rc,
    }
}

/// `get_partition_unused_primary` and `add_partition` on it.
fn add_primary(cxt: &mut Context, pa: Option<&Partition>) -> Result<usize, i32> {
    let res = get_partition_unused_primary(cxt, pa)?;
    match add_partition(cxt, res, pa) {
        0 => Ok(res),
        rc => Err(rc),
    }
}

/// The dialog half of `dos_add_partition`: `Err` returns at once (without
/// counting), `Ok` carries the outcome to count.
fn dialog_add(
    cxt: &mut Context,
    pa: &mut Option<&mut Partition>,
) -> Result<Result<usize, i32>, i32> {
    let (ext_offset, _) = ext(cxt);
    let mut grain = if cxt.grain > cxt.sector_size {
        div(cxt.grain, cxt.sector_size)
    } else {
        1
    };
    let mut first = cxt.first_lba;
    if parent_is_gpt(cxt) {
        // A hybrid MBR throws out the rules.
        grain = 1;
        first = 1;
    }
    let mut free_primary: u8 = 0;
    let mut free_sectors = false;
    for i in 0..4 {
        match partition(cxt, i) {
            Some(p) if p.is_used() => {
                let start = u64::from(p.start);
                if first.wrapping_add(grain) <= start {
                    free_sectors = true;
                }
                first = start.wrapping_add(u64::from(p.size));
            }
            _ => free_primary = free_primary.wrapping_add(1),
        }
    }
    if first.wrapping_add(grain).wrapping_sub(1) <= cxt.total_sectors.wrapping_sub(1) {
        free_sectors = true;
    }

    if free_primary == 0 || !free_sectors {
        if ext_offset != 0 {
            if pa.as_deref().is_none_or(Partition::has_start) {
                // Case A) above, but the start outside the extended.
                let msg: &[u8] = if free_primary == 0 {
                    b"All primary partitions are in use."
                } else {
                    b"All space for primary partitions is in use."
                };
                if pa.as_deref().is_some_and(Partition::has_start) {
                    cxt.warnx(msg);
                    return Err(-EINVAL);
                }
                cxt.info(msg);
            }
            return Ok(add_logical(cxt, pa.as_deref()));
        }
        if free_primary != 0 {
            cxt.info(b"All space for primary partitions is in use.");
        } else {
            cxt.info(
                b"To create more partitions, first replace a primary with an extended partition.",
            );
        }
        return Err(-EINVAL);
    }

    // Scripts create primaries by default.
    if cxt.script.is_some() || !cxt.has_dialogs() {
        return Ok(add_primary(cxt, pa.as_deref()));
    }

    let mut ask = crate::Ask::new(crate::AskType::Menu);
    ask.set_query(b"Partition type");
    ask.menu_set_default(if free_primary == 1 && ext_offset == 0 {
        i32::from(b'e')
    } else {
        i32::from(b'p')
    });
    let used_primary = 4u32
        .wrapping_sub(u32::from(ext_offset != 0))
        .wrapping_sub(u32::from(free_primary));
    let hint = format!(
        "{used_primary} primary, {} extended, {free_primary} free",
        i32::from(ext_offset != 0)
    );
    ask.menu_add_item(i32::from(b'p'), "primary", hint);
    if ext_offset == 0 {
        ask.menu_add_item(
            i32::from(b'e'),
            "extended",
            "container for logical partitions",
        );
    } else {
        ask.menu_add_item(i32::from(b'l'), "logical", "numbered from 5");
    }
    let rc = cxt.do_ask(&mut ask);
    if rc != 0 {
        return Err(rc);
    }
    let c = ask.menu_get_result();
    if c == i32::from(b'p') {
        Ok(add_primary(cxt, pa.as_deref()))
    } else if c == i32::from(b'l') && ext_offset != 0 {
        Ok(add_logical(cxt, pa.as_deref()))
    } else if c == i32::from(b'e') && ext_offset == 0 {
        let res = match get_partition_unused_primary(cxt, pa.as_deref()) {
            Ok(r) => r,
            Err(rc) => return Ok(Err(rc)),
        };
        let t = cxt
            .label_ref()
            .and_then(|lb| lb.get_parttype_from_code(u32::from(MBR_DOS_EXTENDED_PARTITION)))
            .cloned();
        let rc = match pa.as_deref_mut() {
            Some(p) => {
                // Upstream sets the caller's template's type.
                p.set_type(t);
                add_partition(cxt, res, Some(p))
            }
            None => {
                let mut x = Partition::new();
                x.set_type(t);
                add_partition(cxt, res, Some(&x))
            }
        };
        Ok(if rc == 0 { Ok(res) } else { Err(rc) })
    } else {
        let ch = c.to_le_bytes()[0];
        let mut m = b"Invalid partition type `".to_vec();
        m.push(ch);
        m.extend_from_slice(b"'.");
        cxt.warnx(&m);
        // Upstream goes on to `done` with the dialog's 0: counted as added,
        // as partition 1.
        Ok(Ok(0))
    }
}

/// `write_sector (cxt, secno, buf)`.
fn write_sector(cxt: &mut Context, secno: Sector, buf: &[u8]) -> i32 {
    use std::io::{Seek, SeekFrom, Write};
    let Some(dev) = cxt.dev.clone() else {
        return -EINVAL;
    };
    let mut f = &*dev;
    if let Err(e) = f.seek(SeekFrom::Start(secno.wrapping_mul(cxt.sector_size))) {
        let n = crate::utils::errno_of(&e);
        cxt.warn(
            n,
            format!("Cannot write sector {secno}: seek failed").as_bytes(),
        );
        return n.wrapping_neg();
    }
    let len = usize::try_from(cxt.sector_size).unwrap_or(0).min(buf.len());
    match f.write(buf.get(..len).unwrap_or(buf)) {
        Ok(n) if u64::try_from(n).ok() == Some(cxt.sector_size) => 0,
        // A short write: upstream returns -errno, which a write that
        // succeeded in part left alone.
        Ok(_) => 0,
        Err(e) => crate::utils::neg_errno(&e),
    }
}

/// `dos_write_disklabel (cxt)`: the MBR if it changed, then each changed
/// EBR.
fn dos_write_disklabel(cxt: &mut Context) -> i32 {
    let mut mbr_changed = l(cxt).is_some_and(|l| l.non_pt_changed);
    if !mbr_changed {
        mbr_changed = (0..4).any(|i| pte(cxt, i).is_some_and(|pe| pe.changed));
    }
    if mbr_changed {
        cxt.with_firstsector(|b| mbr_set_magic(b));
        let fs = cxt.firstsector();
        let rc = write_sector(cxt, 0, &fs);
        if rc != 0 {
            return rc;
        }
    }
    let (ext_offset, ext_index) = ext(cxt);
    if nmax(cxt) <= 4 && ext_offset != 0 {
        // An empty extended partition: clear any EBR left in it.
        if let Some(pe) = pte(cxt, ext_index) {
            let off = pe.abs_start();
            if off != 0 && pe.changed {
                let mut empty = vec![0u8; usize::try_from(cxt.sector_size).unwrap_or(512).max(512)];
                mbr_set_magic(&mut empty);
                let _ = write_sector(cxt, off, &empty);
            }
        }
    }
    for i in 4..nmax(cxt) {
        let Some(pe) = pte(cxt, i) else { continue };
        let Some(buf) = pe.sectorbuffer.as_ref() else {
            continue;
        };
        if !pe.changed || pe.offset == 0 {
            continue;
        }
        mbr_set_magic(&mut buf.borrow_mut());
        let data = buf.borrow().clone();
        let rc = write_sector(cxt, pe.offset, &data);
        if rc != 0 {
            return rc;
        }
    }
    0
}

/// `dos_locate_disklabel (cxt, n, ...)`.
fn dos_locate_disklabel(cxt: &mut Context, n: i32) -> Result<Located, i32> {
    if n == 0 {
        return Ok(("MBR", 0, 512));
    }
    let i = usize::try_from(n)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .map(|n| n.wrapping_add(4));
    match i {
        Some(i) if i < nmax(cxt) => {
            let pe = pte(cxt, i).ok_or(-EINVAL)?;
            Ok(("EBR", pe.offset.wrapping_mul(cxt.sector_size), 512))
        }
        _ => Err(1),
    }
}

/// `wrong_p_order (cxt, &prev)`: the first partition (and the one it
/// should precede) out of start order, primaries and logicals apart; 0
/// when in order.
fn wrong_p_order(cxt: &Context) -> (usize, usize) {
    let mut last_start = 0;
    let mut last_i = 0;
    for i in 0..nmax(cxt) {
        let Some(pe) = pte(cxt, i) else { continue };
        if i == 4 {
            last_i = 4;
            last_start = 0;
        }
        if pe.pt().is_some_and(|p| p.is_used()) {
            let s = pe.abs_start();
            if last_start > s {
                return (i, last_i);
            }
            last_start = s;
            last_i = i;
        }
    }
    (0, 0)
}

/// `dos_get_disklabel_item (cxt, item)`.
fn dos_get_disklabel_item(cxt: &mut Context, item: &mut LabelItem) -> i32 {
    if item.id == crate::LABELITEM_ID {
        let num = mbr_get_id(&cxt.firstsector());
        item.name = Some("Disk identifier");
        item.data = ItemData::Str(format!("0x{num:08x}").into_bytes());
        0
    } else if item.id < crate::NLABELITEMS {
        1
    } else {
        2
    }
}

/// `dos_get_partition (cxt, n, pa)`.
fn dos_get_partition(cxt: &mut Context, n: usize, pa: &mut Partition) -> i32 {
    let Some(pe) = pte(cxt, n) else {
        return -EINVAL;
    };
    let Some(p) = pe.pt() else {
        // Upstream reads through the NULL entry here; no entry is unused.
        pa.used = false;
        return 0;
    };
    pa.used = !p.is_cleared();
    if !pa.used {
        return 0;
    }
    let (ext_offset, ext_index) = ext(cxt);
    pa.ty = Some(dos_partition_parttype(cxt, &p));
    pa.boot = u32::from(p.boot_ind == ACTIVE_FLAG);
    pa.start = pe.abs_start();
    pa.size = u64::from(p.size);
    pa.container = ext_offset != 0 && n == ext_index;
    if n >= 4 {
        pa.parent_partno = ext_index;
    }
    if p.boot_ind != 0 {
        pa.attrs = Some(format!("{:02x}", p.boot_ind).into_bytes());
    }
    pa.start_chs = Some(format!("{}/{}/{}", cylinder(p.bs, p.bc), p.bh, sector(p.bs)).into_bytes());
    pa.end_chs = Some(format!("{}/{}/{}", cylinder(p.es, p.ec), p.eh, sector(p.es)).into_bytes());
    0
}

/// `has_logical (cxt)`.
fn has_logical(cxt: &Context) -> bool {
    (4..nmax(cxt)).any(|i| pte(cxt, i).is_some_and(|pe| pe.pt_entry.is_some()))
}

/// `dos_set_partition (cxt, n, pa)`.
fn dos_set_partition(cxt: &mut Context, n: usize, pa: &Partition) -> i32 {
    if n >= nmax(cxt) {
        return -EINVAL;
    }
    let Some(pe) = pte(cxt, n) else {
        return -EINVAL;
    };
    let Some(p) = pe.pt() else { return -EINVAL };
    let (ext_offset, ext_index) = ext(cxt);
    let orgtype = u32::from(p.sys_ind);

    if let Some(t) = pa.ty.as_ref() {
        if is_extended(t.code) && ext_offset != 0 && ext_index != n {
            cxt.warnx(b"Extended partition already exists.");
            return -EINVAL;
        }
        if t.code == 0 {
            cxt.warnx(b"Type 0 means free space to many systems. Having partitions of type 0 is probably unwise.");
        }
        if is_extended(orgtype) && !is_extended(t.code) && has_logical(cxt) {
            cxt.warnx(
                b"Cannot change type of the extended partition which is already used by logical partitions. \
                  Delete logical partitions first.",
            );
            return -EINVAL;
        }
    }

    let start = if pa.has_start() {
        pa.start
    } else {
        crate::UNDEF
    };
    let size = if pa.has_size() { pa.size } else { crate::UNDEF };
    if !is_undef(start) || !is_undef(size) {
        let start = if is_undef(start) {
            pe.abs_start()
        } else {
            start
        };
        let size = if is_undef(size) {
            u64::from(p.size)
        } else {
            size
        };
        let boot = if pa.boot == UNDEF_BOOT {
            p.boot_ind == ACTIVE_FLAG
        } else {
            pa.is_bootable()
        };
        let sysid = pa.ty.as_ref().map_or(orgtype, |t| t.code);
        set_partition(
            cxt,
            n,
            false,
            start,
            start.wrapping_add(size).wrapping_sub(1),
            sysid,
            boot,
        );
    } else {
        pe.update_pt(|e| {
            if let Some(t) = pa.ty.as_ref() {
                e.sys_ind = t.code.to_le_bytes()[0];
            }
            if pa.boot != UNDEF_BOOT {
                e.boot_ind = if pa.is_bootable() { ACTIVE_FLAG } else { 0 };
            }
        });
    }

    if let Some(t) = pa.ty.as_ref() {
        if is_extended(t.code) && !is_extended(orgtype) {
            // A new extended partition: referenced.
            let start = pte(cxt, n)
                .and_then(|pe| pe.pt())
                .map_or(0, |e| u64::from(e.start));
            if let Some(l) = lm(cxt) {
                l.ext_index = n;
                l.ext_offset = start;
            }
            pte_mut(cxt, n, |pe| pe.ex_entry = pe.pt_entry);
        } else if is_extended(orgtype) {
            // The extended partition removed.
            set_nmax(cxt, 4);
            pte_mut(cxt, ext_index, |pe| pe.ex_entry = None);
            if let Some(l) = lm(cxt) {
                l.ext_offset = 0;
                l.ext_index = 0;
            }
        }
    }
    partition_set_changed(cxt, n, true);
    0
}

/// `cmp_ebr_offsets`: by offset, unset offsets last.
fn cmp_ebr_offsets(a: &Pte, b: &Pte) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a.offset == 0, b.offset == 0) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => a.offset.cmp(&b.offset),
    }
}

/// `fix_chain_of_logicals (cxt)`: the EBRs sorted by offset, the data
/// partitions by start among them, and the links redone.
fn fix_chain_of_logicals(cxt: &mut Context) {
    let n = nmax(cxt);
    if let Some(l) = lm(cxt) {
        if let Some(chain) = l.ptes.get_mut(4..n) {
            chain.sort_by(cmp_ebr_offsets);
        }
    }
    // The data partitions by start.
    'again: loop {
        for i in 4..n.saturating_sub(1) {
            let (Some(cur), Some(nxt)) = (pte(cxt, i), pte(cxt, i.wrapping_add(1))) else {
                continue;
            };
            if cur.abs_start() > nxt.abs_start() {
                let (Some(cp), Some(np)) = (cur.pt(), nxt.pt()) else {
                    continue;
                };
                let cur_start = cur.abs_start();
                let nxt_start = nxt.abs_start();
                // The entries swapped, each start recounted against its own
                // EBR so that the absolute address stays.
                let mut a = np;
                a.start = u64_as_u32(nxt_start.wrapping_sub(cur.offset));
                a.sync_chs(cur.offset, gsect(cxt), cxt.geom.heads);
                cur.put(cur.pt_entry, &a);
                let mut b = cp;
                b.start = u64_as_u32(cur_start.wrapping_sub(nxt.offset));
                b.sync_chs(nxt.offset, gsect(cxt), cxt.geom.heads);
                nxt.put(nxt.pt_entry, &b);
                partition_set_changed(cxt, i, true);
                partition_set_changed(cxt, i.wrapping_add(1), true);
                continue 'again;
            }
        }
        break;
    }
    // The links.
    let (ext_offset, _) = ext(cxt);
    for i in 4..n.saturating_sub(1) {
        let (Some(cur), Some(nxt)) = (pte(cxt, i), pte(cxt, i.wrapping_add(1))) else {
            continue;
        };
        let noff = nxt.offset.wrapping_sub(ext_offset);
        let ooff = u64::from(cur.ex().map_or(0, |e| e.start));
        if noff == ooff {
            continue;
        }
        set_partition(
            cxt,
            i,
            true,
            nxt.offset,
            nxt.abs_end(),
            u32::from(MBR_DOS_EXTENDED_PARTITION),
            false,
        );
    }
    // The chain always terminated.
    if let Some(last) = pte(cxt, n.wrapping_sub(1)) {
        clear(&last, last.ex_entry);
        partition_set_changed(cxt, n.wrapping_sub(1), true);
    }
}

/// `dos_reorder (cxt)`: 1 when already in order.
fn dos_reorder(cxt: &mut Context) -> i32 {
    if wrong_p_order(cxt).0 == 0 {
        return 1;
    }
    let mut i;
    loop {
        let (wi, k) = wrong_p_order(cxt);
        i = wi;
        if i == 0 || i >= 4 {
            break;
        }
        // Partition i belongs earlier: the MBR entries move.
        let (Some(pei), Some(pek)) = (pte(cxt, i), pte(cxt, k)) else {
            break;
        };
        let (Some(pi), Some(pk)) = (pei.pt(), pek.pt()) else {
            break;
        };
        pte_mut(cxt, i, |p| p.ex_entry = pek.ex_entry);
        pte_mut(cxt, k, |p| p.ex_entry = pei.ex_entry);
        pei.put(pei.pt_entry, &pk);
        pek.put(pek.pt_entry, &pi);
        partition_set_changed(cxt, i, true);
        partition_set_changed(cxt, k, true);
    }
    if i != 0 {
        fix_chain_of_logicals(cxt);
    }
    0
}

/// `fdisk_dos_fix_chs (cxt)`: every partition's C/H/S recomputed from its
/// LBA; the number changed.
pub fn fix_chs(cxt: &mut Context) -> i32 {
    let mut changed: i32 = 0;
    for i in 0..nmax(cxt) {
        let Some(pe) = pte(cxt, i) else { continue };
        let Some(p) = pe.pt().filter(DosPart::is_used) else {
            continue;
        };
        let (obc, obh, obs) = (cylinder(p.bs, p.bc), u32::from(p.bh), sector(p.bs));
        let (oec, oeh, oes) = (cylinder(p.es, p.ec), u32::from(p.eh), sector(p.es));
        let l = pe.abs_start();
        let (mut nbc, mut nbh, mut nbs) = long2chs(cxt, l);
        if l > UINT_MAX || nbc >= 1024 {
            nbc = 1023;
            nbh = cxt.geom.heads.wrapping_sub(1);
            nbs = gsect(cxt);
        }
        let sects = u64::from(p.size);
        let (mut nec, mut neh, mut nes) = long2chs(cxt, l.wrapping_add(sects).wrapping_sub(1));
        if lba_overflowed(l, sects) || nec >= 1024 {
            nec = 1023;
            neh = cxt.geom.heads.wrapping_sub(1);
            nes = gsect(cxt);
        }
        if obc != nbc || obh != nbh || obs != nbs || oec != nec || oeh != neh || oes != nes {
            let low = |v: u32| v.to_le_bytes()[0];
            pe.update_pt(|e| {
                e.bc = low(nbc & 0xff);
                e.bh = low(nbh);
                e.bs = low(nbs | ((nbc >> 2) & 0xc0));
                e.ec = low(nec & 0xff);
                e.eh = low(neh);
                e.es = low(nes | ((nec >> 2) & 0xc0));
            });
            partition_set_changed(cxt, i, true);
            changed = changed.wrapping_add(1);
        }
    }
    changed
}

/// `fdisk_dos_move_begin (cxt, i)`: asks for a new first data sector for
/// partition `i`, keeping its end.
pub fn move_begin(cxt: &mut Context, i: usize) -> i32 {
    let Some(pe) = pte(cxt, i) else {
        return -EINVAL;
    };
    let Some(p) = pe.pt() else { return -EINVAL };
    if !p.is_used() || is_extended(u32::from(p.sys_ind)) {
        cxt.warnx(format!("Partition {}: no data area.", i.wrapping_add(1)).as_bytes());
        return 0;
    }
    // The safe start is the second sector, but some need the MBR inside the
    // first partition: so the disk's first sector, or the extended's
    // second.
    let mut free_start = if pe.offset != 0 {
        u64_as_u32(pe.offset).wrapping_add(1)
    } else {
        0
    };
    let curr_start = u64_as_u32(pe.abs_start());
    for x in 0..nmax(cxt) {
        let Some(prev) = pte(cxt, x) else { continue };
        let Some(pp) = prev.pt() else { continue };
        let end = u64_as_u32(prev.abs_start()).wrapping_add(pp.size);
        if pp.is_used() && end > free_start && end <= curr_start {
            free_start = end;
        }
    }
    let last = u64_as_u32(pe.abs_end());
    let res = match cxt.ask_number(
        u64::from(free_start),
        u64::from(curr_start),
        u64::from(last),
        b"New beginning of data",
    ) {
        Ok(r) => r,
        Err(rc) => return rc,
    };
    let new = u64_as_u32(res.wrapping_sub(pe.offset));
    // Upstream compares the new start with the size.
    if new != p.size {
        let sects = p.size.wrapping_add(p.start).wrapping_sub(new);
        pe.update_pt(|e| {
            e.size = sects;
            e.start = new;
            e.sync_chs(pe.offset, gsect(cxt), cxt.geom.heads);
        });
        partition_set_changed(cxt, i, true);
        if new == 0 {
            cxt.info(
                b"The new beginning of the partition overlaps the disk label area. Be very careful when using \
                  the partition. You can lose all your partitions on the disk.",
            );
        }
    }
    0
}

/// `dos_partition_is_used (cxt, i)`.
fn dos_partition_is_used(cxt: &mut Context, i: usize) -> i32 {
    if i >= nmax(cxt) {
        return 0;
    }
    i32::from(partition(cxt, i).is_some_and(|p| !p.is_cleared()))
}

/// `dos_toggle_partition_flag (cxt, i, flag)`.
fn dos_toggle_partition_flag(cxt: &mut Context, i: usize, flag: u64) -> i32 {
    if i >= nmax(cxt) {
        return -EINVAL;
    }
    let Some(pe) = pte(cxt, i) else {
        return -EINVAL;
    };
    let Some(p) = pe.pt() else { return -EINVAL };
    if flag != DOS_FLAG_ACTIVE {
        return 1;
    }
    if is_extended(u32::from(p.sys_ind)) && p.boot_ind == 0 {
        cxt.warnx(format!("Partition {}: is an extended partition.", i.wrapping_add(1)).as_bytes());
    }
    let on = p.boot_ind == 0;
    pe.update_pt(|e| e.boot_ind = if on { ACTIVE_FLAG } else { 0 });
    partition_set_changed(cxt, i, true);
    cxt.info(
        if on {
            format!(
                "The bootable flag on partition {} is enabled now.",
                i.wrapping_add(1)
            )
        } else {
            format!(
                "The bootable flag on partition {} is disabled now.",
                i.wrapping_add(1)
            )
        }
        .as_bytes(),
    );
    0
}

/// `dos_fields`.
static DOS_FIELDS: [Field; 12] = [
    Field::new(fieldtype::DEVICE, "Device", 10.0, 0),
    Field::new(fieldtype::BOOT, "Boot", 1.0, 0),
    Field::new(fieldtype::START, "Start", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::END, "End", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::SECTORS, "Sectors", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::CYLINDERS, "Cylinders", 5.0, FIELDFL_NUMBER),
    Field::new(
        fieldtype::SIZE,
        "Size",
        5.0,
        FIELDFL_NUMBER | FIELDFL_EYECANDY,
    ),
    Field::new(fieldtype::TYPEID, "Id", 2.0, FIELDFL_NUMBER),
    Field::new(fieldtype::TYPE, "Type", 0.1, 0),
    Field::new(
        fieldtype::SADDR,
        "Start-C/H/S",
        1.0,
        FIELDFL_NUMBER | FIELDFL_DETAIL,
    ),
    Field::new(
        fieldtype::EADDR,
        "End-C/H/S",
        1.0,
        FIELDFL_NUMBER | FIELDFL_DETAIL,
    ),
    Field::new(
        fieldtype::ATTR,
        "Attrs",
        2.0,
        FIELDFL_NUMBER | FIELDFL_DETAIL,
    ),
];

/// `dos_operations`.
static DOS_OPS: LabelOps = LabelOps {
    probe: Some(dos_probe_label),
    write: Some(dos_write_disklabel),
    verify: Some(dos_verify_disklabel),
    create: Some(dos_create_disklabel),
    locate: Some(dos_locate_disklabel),
    reorder: Some(dos_reorder),
    get_item: Some(dos_get_disklabel_item),
    set_id: Some(dos_set_disklabel_id),
    add_part: Some(dos_add_partition),
    del_part: Some(dos_delete_partition),
    get_part: Some(dos_get_partition),
    set_part: Some(dos_set_partition),
    part_is_used: Some(dos_partition_is_used),
    part_toggle_flag: Some(dos_toggle_partition_flag),
    reset_alignment: Some(dos_reset_alignment),
    deinit: Some(dos_deinit),
};

/// `dos_parttype_cuts`.
static DOS_PARTTYPE_CUTS: [Shortcut; 8] = [
    Shortcut {
        shortcut: Some("L"),
        alias: Some("linux"),
        data: "83",
        deprecated: false,
    },
    Shortcut {
        shortcut: Some("S"),
        alias: Some("swap"),
        data: "82",
        deprecated: false,
    },
    // Collides with the 0x0e type.
    Shortcut {
        shortcut: Some("E"),
        alias: Some("extended"),
        data: "05",
        deprecated: true,
    },
    Shortcut {
        shortcut: Some("Ex"),
        alias: Some("extended"),
        data: "05",
        deprecated: false,
    },
    Shortcut {
        shortcut: Some("U"),
        alias: Some("uefi"),
        data: "EF",
        deprecated: false,
    },
    Shortcut {
        shortcut: Some("R"),
        alias: Some("raid"),
        data: "FD",
        deprecated: false,
    },
    Shortcut {
        shortcut: Some("V"),
        alias: Some("lvm"),
        data: "8E",
        deprecated: false,
    },
    Shortcut {
        shortcut: Some("X"),
        alias: Some("linuxex"),
        data: "85",
        deprecated: false,
    },
];

/// `fdisk_new_dos_label (cxt)`.
#[must_use]
pub fn new_label() -> Label {
    Label {
        name: "dos",
        id: LabelType::Dos,
        parttypes: &DOS_PARTTYPES,
        parttype_cuts: &DOS_PARTTYPE_CUTS,
        nparts_max: 0,
        nparts_cur: 0,
        flags: 0,
        geom_min: Geometry {
            heads: 1,
            sectors: 1,
            cylinders: 1,
        },
        geom_max: Geometry {
            heads: 255,
            sectors: 63,
            cylinders: 1_048_576,
        },
        changed: false,
        disabled: false,
        fields: &DOS_FIELDS,
        op: &DOS_OPS,
        data: LabelData::Dos(Box::default()),
    }
}

/// `fdisk_dos_enable_compatible (lb, enable)`.
pub fn enable_compatible(lb: &mut Label, enable: bool) -> i32 {
    let LabelData::Dos(d) = &mut lb.data else {
        return -EINVAL;
    };
    d.compatible = enable;
    if enable {
        lb.flags |= LABEL_FL_REQUIRE_GEOMETRY;
    }
    0
}

/// `fdisk_dos_is_compatible (lb)`.
#[must_use]
pub fn is_compatible(lb: &Label) -> bool {
    matches!(&lb.data, LabelData::Dos(d) if d.compatible)
}

/// `dos_parttypes`: `include/pt-mbr-partnames.h`.
static DOS_PARTTYPES: [PartType; 101] = [
    PartType::code(0x00, b"Empty"),
    PartType::code(0x01, b"FAT12"),
    PartType::code(0x02, b"XENIX root"),
    PartType::code(0x03, b"XENIX usr"),
    PartType::code(0x04, b"FAT16 <32M"),
    PartType::code(0x05, b"Extended"),
    PartType::code(0x06, b"FAT16"),
    PartType::code(0x07, b"HPFS/NTFS/exFAT"),
    PartType::code(0x08, b"AIX"),
    PartType::code(0x09, b"AIX bootable"),
    PartType::code(0x0a, b"OS/2 Boot Manager"),
    PartType::code(0x0b, b"W95 FAT32"),
    PartType::code(0x0c, b"W95 FAT32 (LBA)"),
    PartType::code(0x0e, b"W95 FAT16 (LBA)"),
    PartType::code(0x0f, b"W95 Ext'd (LBA)"),
    PartType::code(0x10, b"OPUS"),
    PartType::code(0x11, b"Hidden FAT12"),
    PartType::code(0x12, b"Compaq diagnostics"),
    PartType::code(0x14, b"Hidden FAT16 <32M"),
    PartType::code(0x16, b"Hidden FAT16"),
    PartType::code(0x17, b"Hidden HPFS/NTFS"),
    PartType::code(0x18, b"AST SmartSleep"),
    PartType::code(0x1b, b"Hidden W95 FAT32"),
    PartType::code(0x1c, b"Hidden W95 FAT32 (LBA)"),
    PartType::code(0x1e, b"Hidden W95 FAT16 (LBA)"),
    PartType::code(0x24, b"NEC DOS"),
    PartType::code(0x27, b"Hidden NTFS WinRE"),
    PartType::code(0x39, b"Plan 9"),
    PartType::code(0x3c, b"PartitionMagic recovery"),
    PartType::code(0x40, b"Venix 80286"),
    PartType::code(0x41, b"PPC PReP Boot"),
    PartType::code(0x42, b"SFS"),
    PartType::code(0x4d, b"QNX4.x"),
    PartType::code(0x4e, b"QNX4.x 2nd part"),
    PartType::code(0x4f, b"QNX4.x 3rd part"),
    PartType::code(0x50, b"OnTrack DM"),
    PartType::code(0x51, b"OnTrack DM6 Aux1"),
    PartType::code(0x52, b"CP/M"),
    PartType::code(0x53, b"OnTrack DM6 Aux3"),
    PartType::code(0x54, b"OnTrackDM6"),
    PartType::code(0x55, b"EZ-Drive"),
    PartType::code(0x56, b"Golden Bow"),
    PartType::code(0x5c, b"Priam Edisk"),
    PartType::code(0x61, b"SpeedStor"),
    PartType::code(0x63, b"GNU HURD or SysV"),
    PartType::code(0x64, b"Novell Netware 286"),
    PartType::code(0x65, b"Novell Netware 386"),
    PartType::code(0x70, b"DiskSecure Multi-Boot"),
    PartType::code(0x75, b"PC/IX"),
    PartType::code(0x80, b"Old Minix"),
    PartType::code(0x81, b"Minix / old Linux"),
    PartType::code(0x82, b"Linux swap / Solaris"),
    PartType::code(0x83, b"Linux"),
    PartType::code(0x84, b"OS/2 hidden or Intel hibernation"),
    PartType::code(0x85, b"Linux extended"),
    PartType::code(0x86, b"NTFS volume set"),
    PartType::code(0x87, b"NTFS volume set"),
    PartType::code(0x88, b"Linux plaintext"),
    PartType::code(0x8e, b"Linux LVM"),
    PartType::code(0x93, b"Amoeba"),
    PartType::code(0x94, b"Amoeba BBT"),
    PartType::code(0x9f, b"BSD/OS"),
    PartType::code(0xa0, b"IBM Thinkpad hibernation"),
    PartType::code(0xa5, b"FreeBSD"),
    PartType::code(0xa6, b"OpenBSD"),
    PartType::code(0xa7, b"NeXTSTEP"),
    PartType::code(0xa8, b"Darwin UFS"),
    PartType::code(0xa9, b"NetBSD"),
    PartType::code(0xab, b"Darwin boot"),
    PartType::code(0xaf, b"HFS / HFS+"),
    PartType::code(0xb7, b"BSDI fs"),
    PartType::code(0xb8, b"BSDI swap"),
    PartType::code(0xbb, b"Boot Wizard hidden"),
    PartType::code(0xbc, b"Acronis FAT32 LBA"),
    PartType::code(0xbe, b"Solaris boot"),
    PartType::code(0xbf, b"Solaris"),
    PartType::code(0xc1, b"DRDOS/sec (FAT-12)"),
    PartType::code(0xc4, b"DRDOS/sec (FAT-16 < 32M)"),
    PartType::code(0xc6, b"DRDOS/sec (FAT-16)"),
    PartType::code(0xc7, b"Syrinx"),
    PartType::code(0xda, b"Non-FS data"),
    PartType::code(0xdb, b"CP/M / CTOS / ..."),
    PartType::code(0xde, b"Dell Utility"),
    PartType::code(0xdf, b"BootIt"),
    PartType::code(0xe1, b"DOS access"),
    PartType::code(0xe3, b"DOS R/O"),
    PartType::code(0xe4, b"SpeedStor"),
    PartType::code(0xea, b"Linux extended boot"),
    PartType::code(0xeb, b"BeOS fs"),
    PartType::code(0xee, b"GPT"),
    PartType::code(0xef, b"EFI (FAT-12/16/32)"),
    PartType::code(0xf0, b"Linux/PA-RISC boot"),
    PartType::code(0xf1, b"SpeedStor"),
    PartType::code(0xf4, b"SpeedStor"),
    PartType::code(0xf2, b"DOS secondary"),
    PartType::code(0xf8, b"EBBR protective"),
    PartType::code(0xfb, b"VMware VMFS"),
    PartType::code(0xfc, b"VMware VMKCORE"),
    PartType::code(0xfd, b"Linux raid autodetect"),
    PartType::code(0xfe, b"LANstep"),
    PartType::code(0xff, b"BBT"),
];
