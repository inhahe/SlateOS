//! `bsd.c`: the BSD disklabel driver -- on its own at the start of a disk,
//! or nested in a DOS partition of type FreeBSD, NetBSD or OpenBSD, which
//! `fdisk` edits through a nested context.
//!
//! The label is `struct bsd_disklabel`, 404 packed bytes, read from the
//! second 512-byte sector of an 8 KiB boot area. Both are kept as bytes,
//! as upstream keeps them in memory one after the other -- which matters
//! once, to the checksum, which runs over as many partition slots as the
//! label claims, sixteen or not.

use crate::context::Context;
use crate::errno::{EINVAL, ERANGE};
use crate::field::{FIELDFL_NUMBER, Field};
use crate::item::{ItemData, LabelItem};
use crate::label::{Label, LabelData, LabelOps};
use crate::partition::Partition;
use crate::parttype::PartType;
use crate::utils::{partname, u64_as_u32};
use crate::{
    Geometry, LABEL_FL_INCHARS_PARTNO, LABEL_FL_REQUIRE_GEOMETRY, LabelType, NLABELITEMS, fieldtype,
};

/// `BSD_MAXPARTITIONS`.
const MAXPARTITIONS: usize = 16;
/// `BSD_DISKMAGIC`.
const DISKMAGIC: u32 = 0x8256_4557;
/// `BSD_LINUX_BOOTDIR`.
const LINUX_BOOTDIR: &str = "/usr/ucb/mdec";
/// `BSD_LABELSECTOR` (on x86).
const LABELSECTOR: usize = 1;
/// `BSD_LABELOFFSET` (on x86).
const LABELOFFSET: usize = 0;
/// `BSD_BBSIZE`: the boot area, label included.
const BBSIZE: usize = 8192;
/// `BSD_SBSIZE`.
const SBSIZE: u32 = 8192;
/// `sizeof (struct bsd_disklabel)`.
const LABEL_SIZE: usize = 404;
/// `DEFAULT_SECTOR_SIZE`.
const SECTOR: usize = 512;
/// Where the label sits in the boot area.
const LABEL_AT: usize = LABELSECTOR * SECTOR + LABELOFFSET;

/// `BSD_DTYPE_SCSI`.
const DTYPE_SCSI: i16 = 4;
/// `BSD_DTYPE_ST506`.
const DTYPE_ST506: i16 = 6;
/// `BSD_D_*` flags.
const D_REMOVABLE: u32 = 0x01;
const D_ECC: u32 = 0x02;
const D_BADSECT: u32 = 0x04;
const D_DOSPART: u32 = 0x20;
/// `BSD_FS_*` used here.
const FS_UNUSED: u8 = 0;
const FS_BSDFFS: u8 = 7;
const FS_MSDOS: u8 = 8;
const FS_OTHER: u8 = 10;
const FS_HPFS: u8 = 11;

/// `bsd_dktypenames`.
const DKTYPENAMES: [&str; 11] = [
    "unknown", "SMD", "MSCP", "old DEC", "SCSI", "ESDI", "ST506", "HP-IB", "HP-FL", "type 9",
    "floppy",
];

/// `bsd_fstypes` (the x86 list: 8 is MS-DOS, not ext2).
static FSTYPES: [PartType; 17] = [
    PartType::code(0, b"unused"),
    PartType::code(1, b"swap"),
    PartType::code(2, b"Version 6"),
    PartType::code(3, b"Version 7"),
    PartType::code(4, b"System V"),
    PartType::code(5, b"4.1BSD"),
    PartType::code(6, b"Eighth Edition"),
    PartType::code(7, b"4.2BSD"),
    PartType::code(8, b"MS-DOS"),
    PartType::code(9, b"4.4LFS"),
    PartType::code(10, b"unknown"),
    PartType::code(11, b"HPFS"),
    PartType::code(12, b"ISO-9660"),
    PartType::code(13, b"boot"),
    PartType::code(14, b"ADOS"),
    PartType::code(15, b"HFS"),
    PartType::code(16, b"AdvFS"),
];

/// Field offsets in `struct bsd_disklabel`.
mod off {
    pub const MAGIC: usize = 0;
    pub const TYPE: usize = 4;
    pub const TYPENAME: usize = 8;
    pub const PACKNAME: usize = 24;
    pub const SECSIZE: usize = 40;
    pub const NSECTORS: usize = 44;
    pub const NTRACKS: usize = 48;
    pub const NCYLINDERS: usize = 52;
    pub const SECPERCYL: usize = 56;
    pub const SECPERUNIT: usize = 60;
    pub const RPM: usize = 72;
    pub const INTERLEAVE: usize = 74;
    pub const TRACKSKEW: usize = 76;
    pub const CYLSKEW: usize = 78;
    pub const HEADSWITCH: usize = 80;
    pub const TRKSEEK: usize = 84;
    pub const FLAGS: usize = 88;
    pub const MAGIC2: usize = 132;
    pub const CHECKSUM: usize = 136;
    pub const NPARTITIONS: usize = 138;
    pub const BBSIZE: usize = 140;
    pub const SBSIZE: usize = 144;
    pub const PARTITIONS: usize = 148;
}

/// `struct bsd_partition` field offsets, from the slot's start.
mod poff {
    pub const SIZE: usize = 0;
    pub const OFFSET: usize = 4;
    pub const FSIZE: usize = 8;
    pub const FSTYPE: usize = 12;
    pub const FRAG: usize = 13;
    pub const CPG: usize = 14;
}

/// A little-endian field.
fn get(b: &[u8], at: usize, n: usize) -> u64 {
    let mut v = [0u8; 8];
    if let (Some(s), Some(d)) = (b.get(at..at.wrapping_add(n)), v.get_mut(..n)) {
        d.copy_from_slice(s);
    }
    u64::from_le_bytes(v)
}

/// Writes a little-endian field.
fn put(b: &mut [u8], at: usize, n: usize, val: u64) {
    let v = val.to_le_bytes();
    if let (Some(d), Some(s)) = (b.get_mut(at..at.wrapping_add(n)), v.get(..n)) {
        d.copy_from_slice(s);
    }
}

/// `struct fdisk_bsd_label`'s own part.
#[derive(Clone, Debug)]
pub struct BsdLabel {
    /// The parent DOS partition holding the label (`dos_part`), by number.
    dos_part: Option<usize>,
    /// `struct bsd_disklabel`.
    bsd: Vec<u8>,
    /// The boot area the label was read from and is written back in.
    bsdbuffer: Vec<u8>,
}

impl Default for BsdLabel {
    fn default() -> Self {
        Self {
            dos_part: None,
            bsd: vec![0u8; LABEL_SIZE],
            bsdbuffer: vec![0u8; BBSIZE],
        }
    }
}

impl BsdLabel {
    fn u32_at(&self, at: usize) -> u32 {
        u64_as_u32(get(&self.bsd, at, 4))
    }
    fn u16_at(&self, at: usize) -> u16 {
        u16::try_from(get(&self.bsd, at, 2)).unwrap_or(0)
    }
    fn set32(&mut self, at: usize, v: u32) {
        put(&mut self.bsd, at, 4, u64::from(v));
    }
    fn set16(&mut self, at: usize, v: u16) {
        put(&mut self.bsd, at, 2, u64::from(v));
    }
    fn d_type(&self) -> i16 {
        i16::from_le_bytes(self.u16_at(off::TYPE).to_le_bytes())
    }
    fn npartitions(&self) -> u16 {
        self.u16_at(off::NPARTITIONS)
    }
    fn set_npartitions(&mut self, n: u16) {
        self.set16(off::NPARTITIONS, n);
    }
    fn secpercyl(&self) -> u32 {
        self.u32_at(off::SECPERCYL)
    }
    fn p_at(i: usize) -> usize {
        off::PARTITIONS.wrapping_add(i.wrapping_mul(16))
    }
    fn p_size(&self, i: usize) -> u32 {
        self.u32_at(Self::p_at(i).wrapping_add(poff::SIZE))
    }
    fn p_offset(&self, i: usize) -> u32 {
        self.u32_at(Self::p_at(i).wrapping_add(poff::OFFSET))
    }
    fn p_fsize(&self, i: usize) -> u32 {
        self.u32_at(Self::p_at(i).wrapping_add(poff::FSIZE))
    }
    fn p_fstype(&self, i: usize) -> u8 {
        self.bsd
            .get(Self::p_at(i).wrapping_add(poff::FSTYPE))
            .copied()
            .unwrap_or(0)
    }
    fn p_frag(&self, i: usize) -> u8 {
        self.bsd
            .get(Self::p_at(i).wrapping_add(poff::FRAG))
            .copied()
            .unwrap_or(0)
    }
    fn p_cpg(&self, i: usize) -> u16 {
        self.u16_at(Self::p_at(i).wrapping_add(poff::CPG))
    }
    fn set_p(&mut self, i: usize, size: u32, offset: u32, fstype: u8) {
        let at = Self::p_at(i);
        self.set32(at.wrapping_add(poff::SIZE), size);
        self.set32(at.wrapping_add(poff::OFFSET), offset);
        if let Some(b) = self.bsd.get_mut(at.wrapping_add(poff::FSTYPE)) {
            *b = fstype;
        }
    }
    fn set_p_fstype(&mut self, i: usize, fstype: u8) {
        if let Some(b) = self.bsd.get_mut(Self::p_at(i).wrapping_add(poff::FSTYPE)) {
            *b = fstype;
        }
    }
}

/// The current label's BSD data.
fn bl(cxt: &Context) -> Option<&BsdLabel> {
    match &cxt.label_ref()?.data {
        LabelData::Bsd(b) => Some(b),
        _ => None,
    }
}

/// The current label's BSD data, for changing.
fn blm(cxt: &mut Context) -> Option<&mut BsdLabel> {
    match &mut cxt.label_mut()?.data {
        LabelData::Bsd(b) => Some(b),
        _ => None,
    }
}

/// Sets the label's `nparts_cur` (and `nparts_max`), and marks it changed
/// when asked.
fn set_counts(cxt: &mut Context, cur: usize, max: Option<usize>, changed: bool) {
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_cur = cur;
        if let Some(m) = max {
            lb.nparts_max = m;
        }
        if changed {
            lb.changed = true;
        }
    }
}

/// The parent's DOS entry `i`.
fn parent_entry(cxt: &Context, i: usize) -> Option<crate::dos::DosPart> {
    let parent = cxt.get_parent()?;
    if !parent.is_labeltype(LabelType::Dos) {
        return None;
    }
    crate::dos::get_partition_entry(parent, i)
}

/// The parent DOS partition the label is in (`l->dos_part`).
fn dos_part(cxt: &Context) -> Option<crate::dos::DosPart> {
    parent_entry(cxt, bl(cxt)?.dos_part?)
}

/// `bsd_partition_parttype (cxt, p)`.
fn partition_parttype(cxt: &Context, fstype: u8) -> PartType {
    cxt.label_ref()
        .and_then(|lb| lb.get_parttype_from_code(u32::from(fstype)))
        .cloned()
        .unwrap_or_else(|| PartType::new_unknown(u32::from(fstype), None))
}

/// `HIDDEN_MASK`.
const HIDDEN_MASK: u8 = 0x10;

/// `is_bsd_partition_type (type)`: FreeBSD, NetBSD, OpenBSD, or one of them
/// hidden.
fn is_bsd_partition_type(t: u8) -> bool {
    /// `MBR_FREEBSD_PARTITION`, `MBR_NETBSD_PARTITION`, `MBR_OPENBSD_PARTITION`.
    const BSD: [u8; 3] = [0xa5, 0xa9, 0xa6];
    BSD.iter().any(|&b| t == b || t == b ^ HIDDEN_MASK)
}

/// `bsd_assign_dos_partition (cxt)`: the first DOS primary of a BSD type.
/// 0, 1 when there is none, -1 for one starting at sector 0.
fn assign_dos_partition(cxt: &mut Context) -> i32 {
    for i in 0..4 {
        if let Some(l) = blm(cxt) {
            l.dos_part = Some(i);
        }
        let Some(p) = parent_entry(cxt, i) else {
            continue;
        };
        if !is_bsd_partition_type(p.sys_ind) {
            continue;
        }
        if p.start == 0 {
            cxt.warnx(
                format!(
                    "Partition {}: has invalid starting sector 0.",
                    i.wrapping_add(1)
                )
                .as_bytes(),
            );
            return -1;
        }
        if let Some(pp) = cxt.get_parent().and_then(|p| p.dev_path.clone()) {
            cxt.dev_path = partname(Some(&pp), i.wrapping_add(1));
        }
        return 0;
    }
    let mut m = b"There is no *BSD partition on ".to_vec();
    m.extend_from_slice(
        cxt.get_parent()
            .and_then(|p| p.dev_path.as_deref())
            .unwrap_or(b"(null)"),
    );
    m.push(b'.');
    cxt.warnx(&m);
    cxt.dev_path = None;
    if let Some(l) = blm(cxt) {
        l.dos_part = None;
    }
    1
}

/// `bsd_probe_label (cxt)`.
fn bsd_probe_label(cxt: &mut Context) -> i32 {
    let mut rc = 0;
    if cxt.parent.is_some() {
        rc = assign_dos_partition(cxt);
    }
    if rc == 0 {
        rc = readlabel(cxt);
    }
    i32::from(rc == 0)
}

/// `set_parttype (cxt, partnum, t)`.
fn set_parttype(cxt: &mut Context, partnum: usize, t: &PartType) -> i32 {
    let Some(l) = blm(cxt) else { return -EINVAL };
    if partnum >= usize::from(l.npartitions()) || t.code > u32::from(u8::MAX) {
        return -EINVAL;
    }
    let code = t.code.to_le_bytes()[0];
    if u32::from(l.p_fstype(partnum)) == t.code {
        return 0;
    }
    l.set_p_fstype(partnum, code);
    if let Some(lb) = cxt.label_mut() {
        lb.changed = true;
    }
    0
}

/// `bsd_add_partition (cxt, pa, &partno)`.
fn bsd_add_partition(
    cxt: &mut Context,
    pa: Option<&mut Partition>,
    partno: Option<&mut usize>,
) -> i32 {
    let pa = pa.map(|p| &*p);
    let i = match cxt.partition_next_partno(pa) {
        Ok(i) => i,
        Err(rc) => return rc,
    };
    if i >= MAXPARTITIONS {
        return -ERANGE;
    }
    let Some(l) = bl(cxt) else { return -EINVAL };
    let secperunit = l.u32_at(off::SECPERUNIT);
    let secpercyl = l.secpercyl();
    let (mut begin, mut end): (u32, u32) = match dos_part(cxt) {
        Some(d) => (d.start, d.start.wrapping_add(d.size).wrapping_sub(1)),
        None => (0, secperunit.wrapping_sub(1)),
    };

    // The first sector.
    if pa.is_some_and(|p| p.start_follow_default) {
    } else if let Some(p) = pa.filter(|p| p.has_start()) {
        if p.start < u64::from(begin) || p.start > u64::from(end) {
            return -ERANGE;
        }
        begin = u64_as_u32(p.start);
    } else {
        let mut ask = crate::Ask::new(crate::AskType::Number);
        ask.set_query(if cxt.use_cylinders() {
            b"First cylinder".as_slice()
        } else {
            b"First sector"
        });
        ask.number_set_low(cxt.cround(u64::from(begin)));
        ask.number_set_default(cxt.cround(u64::from(begin)));
        ask.number_set_high(cxt.cround(u64::from(end)));
        let rc = cxt.do_ask(&mut ask);
        begin = u64_as_u32(ask.number_get_result());
        if rc != 0 {
            return rc;
        }
        if cxt.use_cylinders() {
            begin = begin.wrapping_sub(1).wrapping_mul(secpercyl);
        }
    }

    // The last sector.
    if pa.is_some_and(|p| p.end_follow_default) {
    } else if let Some(p) = pa.filter(|p| p.has_size()) {
        if u64::from(begin).wrapping_add(p.size) > u64::from(end) {
            return -ERANGE;
        }
        end = u64_as_u32(u64::from(begin).wrapping_add(p.size).wrapping_sub(1));
    } else {
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
        ask.number_set_low(cxt.cround(u64::from(begin)));
        ask.number_set_default(cxt.cround(u64::from(end)));
        ask.number_set_high(cxt.cround(u64::from(end)));
        ask.number_set_base(cxt.cround(u64::from(begin)));
        ask.number_set_wrap_negative(true);
        let rc = cxt.do_ask(&mut ask);
        end = u64_as_u32(ask.number_get_result());
        if rc != 0 {
            return rc;
        }
        if cxt.use_cylinders() {
            end = end.wrapping_mul(secpercyl).wrapping_sub(1);
        }
    }

    let Some(l) = blm(cxt) else { return -EINVAL };
    l.set_p(i, end.wrapping_sub(begin).wrapping_add(1), begin, FS_UNUSED);
    if i >= usize::from(l.npartitions()) {
        l.set_npartitions(u16::try_from(i.wrapping_add(1)).unwrap_or(u16::MAX));
    }
    let n = usize::from(l.npartitions());
    set_counts(cxt, n, None, false);
    if let Some(t) = pa.and_then(|p| p.ty.clone()) {
        let _ = set_parttype(cxt, i, &t);
    }
    if let Some(lb) = cxt.label_mut() {
        lb.changed = true;
    }
    if let Some(partno) = partno {
        *partno = i;
    }
    0
}

/// `bsd_set_partition (cxt, n, pa)`.
fn bsd_set_partition(cxt: &mut Context, n: usize, pa: &Partition) -> i32 {
    let Some(l) = bl(cxt) else { return -EINVAL };
    if n >= usize::from(l.npartitions()) {
        return -EINVAL;
    }
    let (p_offset, p_size) = (l.p_offset(n), l.p_size(n));
    // The partition has to stay within the parent DOS partition.
    if let Some(d) = dos_part(cxt).filter(|_| pa.has_start() || pa.has_size()) {
        let dosbegin = u64::from(d.start);
        let dosend = dosbegin.wrapping_add(u64::from(d.size)).wrapping_sub(1);
        let begin = if pa.has_start() {
            pa.start
        } else {
            u64::from(p_offset)
        };
        let end = begin
            .wrapping_add(if pa.has_size() {
                pa.size
            } else {
                u64::from(p_size)
            })
            .wrapping_sub(1);
        if begin < dosbegin || begin > dosend || end < dosbegin || end > dosend {
            return -ERANGE;
        }
    }
    if let Some(t) = pa.ty.as_ref() {
        let rc = set_parttype(cxt, n, t);
        if rc != 0 {
            return rc;
        }
    }
    let Some(l) = blm(cxt) else { return -EINVAL };
    let at = BsdLabel::p_at(n);
    if pa.has_start() {
        l.set32(at.wrapping_add(poff::OFFSET), u64_as_u32(pa.start));
    }
    if pa.has_size() {
        l.set32(at.wrapping_add(poff::SIZE), u64_as_u32(pa.size));
    }
    if let Some(lb) = cxt.label_mut() {
        lb.changed = true;
    }
    0
}

/// `bsd_create_disklabel (cxt)`: asks first. 1 when the answer is no.
fn bsd_create_disklabel(cxt: &mut Context) -> i32 {
    let mut m = b"The device ".to_vec();
    m.extend_from_slice(cxt.dev_path.as_deref().unwrap_or(b"(null)"));
    m.extend_from_slice(b" does not contain BSD disklabel.");
    cxt.info(&m);
    let yes = match cxt.ask_yesno(b"Do you want to create a BSD disklabel?") {
        Ok(y) => y,
        Err(rc) => return rc,
    };
    if !yes {
        return 1;
    }
    if cxt.parent.is_some() {
        let rc = assign_dos_partition(cxt);
        if rc == 1 {
            // No DOS partition fit for a BSD label.
            return -EINVAL;
        }
        if rc != 0 {
            return rc;
        }
    }
    let rc = initlabel(cxt);
    if rc == 0 {
        let n = bl(cxt).map_or(0, |l| usize::from(l.npartitions()));
        set_counts(cxt, n, Some(MAXPARTITIONS), false);
    }
    rc
}

/// `bsd_delete_part (cxt, partnum)`.
fn bsd_delete_part(cxt: &mut Context, partnum: usize) -> i32 {
    let Some(l) = blm(cxt) else { return -EINVAL };
    l.set_p(partnum, 0, 0, FS_UNUSED);
    if usize::from(l.npartitions()) == partnum.wrapping_add(1) {
        // Down to the last used slot. (Upstream reads before the first
        // when none is.)
        while l.npartitions() > 0 && l.p_size(usize::from(l.npartitions()).wrapping_sub(1)) == 0 {
            let n = l.npartitions().wrapping_sub(1);
            l.set_npartitions(n);
        }
    }
    let n = usize::from(l.npartitions());
    set_counts(cxt, n, None, true);
    0
}

/// `BSD_LABELITEM_*`.
mod item {
    use crate::NLABELITEMS;
    pub const TYPE: i32 = NLABELITEMS;
    pub const DISK: i32 = NLABELITEMS + 1;
    pub const PACKNAME: i32 = NLABELITEMS + 2;
    pub const FLAGS: i32 = NLABELITEMS + 3;
    pub const SECSIZE: i32 = NLABELITEMS + 4;
    pub const NTRACKS: i32 = NLABELITEMS + 5;
    pub const SECPERCYL: i32 = NLABELITEMS + 6;
    pub const CYLINDERS: i32 = NLABELITEMS + 7;
    pub const RPM: i32 = NLABELITEMS + 8;
    pub const INTERLEAVE: i32 = NLABELITEMS + 9;
    pub const TRACKSKEW: i32 = NLABELITEMS + 10;
    pub const CYLINDERSKEW: i32 = NLABELITEMS + 11;
    pub const HEADSWITCH: i32 = NLABELITEMS + 12;
    pub const TRKSEEK: i32 = NLABELITEMS + 13;
}

/// `strndup (s, n)`: up to the first NUL of `n` bytes.
fn strndup(b: &[u8]) -> Vec<u8> {
    crate::utils::c_str(b).to_vec()
}

/// `bsd_get_disklabel_item (cxt, item)`.
fn bsd_get_disklabel_item(cxt: &mut Context, it: &mut LabelItem) -> i32 {
    let Some(d) = bl(cxt) else { return -EINVAL };
    let num = |v: u64| ItemData::U64(v);
    let (name, data) = match it.id {
        item::TYPE => {
            let t = d.d_type();
            let s = usize::try_from(t)
                .ok()
                .and_then(|i| DKTYPENAMES.get(i))
                .map_or_else(|| t.to_string(), |s| (*s).to_string());
            ("Type", ItemData::Str(s.into_bytes()))
        }
        item::DISK => (
            "Disk",
            ItemData::Str(strndup(
                d.bsd.get(off::TYPENAME..off::TYPENAME + 16).unwrap_or(&[]),
            )),
        ),
        item::PACKNAME => (
            "Packname",
            ItemData::Str(strndup(
                d.bsd.get(off::PACKNAME..off::PACKNAME + 16).unwrap_or(&[]),
            )),
        ),
        item::FLAGS => {
            let f = d.u32_at(off::FLAGS);
            let s: &[u8] = if f & D_REMOVABLE != 0 {
                b" removable"
            } else if f & D_ECC != 0 {
                b" ecc"
            } else if f & D_BADSECT != 0 {
                b" badsect"
            } else {
                b""
            };
            ("Flags", ItemData::Str(s.to_vec()))
        }
        item::SECSIZE => ("Bytes/Sector", num(u64::from(d.u32_at(off::SECSIZE)))),
        item::NTRACKS => ("Tracks/Cylinder", num(u64::from(d.u32_at(off::NTRACKS)))),
        item::SECPERCYL => ("Sectors/Cylinder", num(u64::from(d.secpercyl()))),
        item::CYLINDERS => ("Cylinders", num(u64::from(d.u32_at(off::NCYLINDERS)))),
        item::RPM => ("Rpm", num(u64::from(d.u16_at(off::RPM)))),
        item::INTERLEAVE => ("Interleave", num(u64::from(d.u16_at(off::INTERLEAVE)))),
        item::TRACKSKEW => ("Trackskew", num(u64::from(d.u16_at(off::TRACKSKEW)))),
        item::CYLINDERSKEW => ("Cylinderskew", num(u64::from(d.u16_at(off::CYLSKEW)))),
        item::HEADSWITCH => ("Headswitch", num(u64::from(d.u32_at(off::HEADSWITCH)))),
        item::TRKSEEK => (
            "Track-to-track seek",
            num(u64::from(d.u32_at(off::TRKSEEK))),
        ),
        id => return if id < NLABELITEMS { 1 } else { 2 },
    };
    it.name = Some(name);
    it.data = data;
    0
}

/// `bsd_get_partition (cxt, n, pa)`.
fn bsd_get_partition(cxt: &mut Context, n: usize, pa: &mut Partition) -> i32 {
    let Some(d) = bl(cxt) else { return -EINVAL };
    if n >= usize::from(d.npartitions()) {
        return -EINVAL;
    }
    let (size, offset, fstype) = (d.p_size(n), d.p_offset(n), d.p_fstype(n));
    let (fsize, frag, cpg, spc) = (d.p_fsize(n), d.p_frag(n), d.p_cpg(n), d.secpercyl());
    pa.used = size != 0;
    if !pa.used {
        return 0;
    }
    if cxt.use_cylinders() && spc != 0 {
        pa.start_post = if offset.checked_rem(spc).unwrap_or(0) != 0 {
            b'*'
        } else {
            b' '
        };
        pa.end_post = if offset.wrapping_add(size).checked_rem(spc).unwrap_or(0) != 0 {
            b'*'
        } else {
            b' '
        };
    }
    pa.start = u64::from(offset);
    pa.size = u64::from(size);
    pa.ty = Some(partition_parttype(cxt, fstype));
    if fstype == FS_UNUSED || fstype == FS_BSDFFS {
        pa.fsize = u64::from(fsize);
        pa.bsize = u64::from(fsize).wrapping_mul(u64::from(frag));
    }
    if fstype == FS_BSDFFS {
        pa.cpg = u64::from(cpg);
    }
    0
}

/// `ask_uint32 (cxt, dflt, mesg)`.
fn ask_uint32(cxt: &mut Context, dflt: u32, mesg: &[u8]) -> u32 {
    match cxt.ask_number(
        u64::from(dflt.min(1)),
        u64::from(dflt),
        u64::from(u32::MAX),
        mesg,
    ) {
        Ok(r) => u64_as_u32(r),
        Err(_) => dflt,
    }
}

/// `ask_uint16 (cxt, dflt, mesg)`.
fn ask_uint16(cxt: &mut Context, dflt: u16, mesg: &[u8]) -> u16 {
    match cxt.ask_number(
        u64::from(dflt.min(1)),
        u64::from(dflt),
        u64::from(u16::MAX),
        mesg,
    ) {
        Ok(r) => u16::try_from(r & 0xffff).unwrap_or(0),
        Err(_) => dflt,
    }
}

/// `fdisk_bsd_edit_disklabel (cxt)`: asks for the geometry fields.
pub fn edit_disklabel(cxt: &mut Context) -> i32 {
    let Some(d) = bl(cxt) else { return -EINVAL };
    let spc = u64::from(d.u32_at(off::NSECTORS)).wrapping_mul(u64::from(d.u32_at(off::NTRACKS)));
    if let Ok(res) = cxt.ask_number(1, spc, spc, b"sectors/cylinder") {
        if let Some(l) = blm(cxt) {
            l.set32(off::SECPERCYL, u64_as_u32(res));
        }
    }
    let get16 = |cxt: &Context, at: usize| bl(cxt).map_or(0, |l| l.u16_at(at));
    let get32 = |cxt: &Context, at: usize| bl(cxt).map_or(0, |l| l.u32_at(at));
    for (at, name) in [
        (off::RPM, b"rpm".as_slice()),
        (off::INTERLEAVE, b"interleave"),
        (off::TRACKSKEW, b"trackskew"),
        (off::CYLSKEW, b"cylinderskew"),
    ] {
        let v = ask_uint16(cxt, get16(cxt, at), name);
        if let Some(l) = blm(cxt) {
            l.set16(at, v);
        }
    }
    for (at, name) in [
        (off::HEADSWITCH, b"headswitch".as_slice()),
        (off::TRKSEEK, b"track-to-track seek"),
    ] {
        let v = ask_uint32(cxt, get32(cxt, at), name);
        if let Some(l) = blm(cxt) {
            l.set32(at, v);
        }
    }
    if let Some(l) = blm(cxt) {
        let unit = l.secpercyl().wrapping_mul(l.u32_at(off::NCYLINDERS));
        l.set32(off::SECPERUNIT, unit);
    }
    0
}

/// `msg` with `arg` spliced in at `{}`.
fn with_arg(msg: &str, arg: &[u8]) -> Vec<u8> {
    let (a, b) = msg.split_once("{}").unwrap_or((msg, ""));
    let mut m = a.as_bytes().to_vec();
    m.extend_from_slice(arg);
    m.extend_from_slice(b.as_bytes());
    m
}

/// `bsd_get_bootstrap (cxt, path, ptr, size)`: `size` bytes of `path`.
fn get_bootstrap(cxt: &mut Context, path: &[u8], size: usize) -> Result<Vec<u8>, i32> {
    use std::io::Read;
    let mut f = match std::fs::File::open(ulsysfs::path_of(path)) {
        Ok(f) => f,
        Err(e) => {
            let n = crate::utils::errno_of(&e);
            cxt.warn(n, &with_arg("cannot open {}", path));
            return Err(n.wrapping_neg());
        }
    };
    let mut buf = vec![0u8; size];
    if let Err(e) = f.read_exact(&mut buf) {
        let n = crate::utils::errno_of(&e);
        cxt.warn(n, &with_arg("cannot read {}", path));
        return Err(n.wrapping_neg());
    }
    cxt.info(&with_arg(
        "The bootstrap file {} successfully loaded.",
        path,
    ));
    Ok(buf)
}

/// `fdisk_bsd_write_bootstrap (cxt)`: a boot program installed in the
/// boot area, around the label.
pub fn write_bootstrap(cxt: &mut Context) -> i32 {
    let Some(d) = bl(cxt).cloned() else {
        return -EINVAL;
    };
    let dflt = if d.d_type() == DTYPE_SCSI { "sd" } else { "wd" };
    let res = match cxt
        .ask_string(format!("Bootstrap: {dflt}boot -> boot{dflt} (default {dflt})").as_bytes())
    {
        Ok(r) => r,
        Err(rc) => return rc,
    };
    let name: Vec<u8> = match res.as_deref().filter(|r| !r.is_empty()) {
        Some(r) => r.to_vec(),
        None => dflt.as_bytes().to_vec(),
    };
    let path = |pre: &[u8], post: &[u8]| -> Vec<u8> {
        let mut p = format!("{LINUX_BOOTDIR}/").into_bytes();
        p.extend_from_slice(pre);
        p.extend_from_slice(post);
        p
    };
    let secsize = usize::try_from(d.u32_at(off::SECSIZE)).unwrap_or(0);
    let first = match get_bootstrap(cxt, &path(&name, b"boot"), secsize) {
        Ok(b) => b,
        Err(rc) => return rc,
    };
    let mut buf = d.bsdbuffer.clone();
    if let Some(dst) = buf.get_mut(..first.len()) {
        dst.copy_from_slice(&first);
    }
    // The label, kept aside: the second stage covers it with zeros.
    let saved = buf
        .get(LABEL_AT..LABEL_AT + LABEL_SIZE)
        .map(<[u8]>::to_vec)
        .unwrap_or_default();
    if let Some(dp) = buf.get_mut(LABEL_AT..LABEL_AT + LABEL_SIZE) {
        dp.fill(0);
    }
    let bbsize = usize::try_from(d.u32_at(off::BBSIZE)).unwrap_or(0);
    let second = match get_bootstrap(cxt, &path(b"boot", &name), bbsize.wrapping_sub(secsize)) {
        Ok(b) => b,
        Err(rc) => return rc,
    };
    if let Some(dst) = buf.get_mut(secsize..secsize.wrapping_add(second.len())) {
        dst.copy_from_slice(&second);
    }
    if buf
        .get(LABEL_AT..LABEL_AT + LABEL_SIZE)
        .is_some_and(|dp| dp.iter().any(|&b| b != 0))
    {
        cxt.warnx(b"Bootstrap overlaps with disklabel!");
        return -EINVAL;
    }
    if let Some(dp) = buf.get_mut(LABEL_AT..LABEL_AT + LABEL_SIZE) {
        dp.copy_from_slice(&saved);
    }
    let sector = dos_part(cxt).map_or(0, |p| u64::from(p.start));
    let devp = cxt.dev_path.clone().unwrap_or_default();
    let rc = cxt.write_to_device(buf.get(..BBSIZE).unwrap_or(&buf), sector.wrapping_mul(512));
    if rc != 0 {
        cxt.warn(rc.wrapping_neg(), &with_arg("cannot write {}", &devp));
        return rc;
    }
    if let Some(l) = blm(cxt) {
        l.bsdbuffer = buf;
    }
    cxt.info(&with_arg("Bootstrap installed on {}.", &devp));
    sync_disks(cxt);
    0
}

/// `bsd_dkcksum (lp)`: the XOR of the label's 16-bit words up to the last
/// partition slot it claims -- read on into the boot area past the
/// sixteenth, as upstream's struct layout has it.
fn dkcksum(l: &BsdLabel) -> u16 {
    let end = off::PARTITIONS.wrapping_add(usize::from(l.npartitions()).wrapping_mul(16));
    let mut bytes = l.bsd.clone();
    bytes.extend_from_slice(&l.bsdbuffer);
    let mut sum: u16 = 0;
    let mut p = 0usize;
    while p < end {
        let lo = bytes.get(p).copied().unwrap_or(0);
        let hi = bytes.get(p.wrapping_add(1)).copied().unwrap_or(0);
        sum ^= u16::from_le_bytes([lo, hi]);
        p = p.wrapping_add(2);
    }
    sum
}

/// `bsd_initlabel (cxt)`: a new label from the geometry.
fn initlabel(cxt: &mut Context) -> i32 {
    let geom = cxt.geom;
    let scsi = cxt
        .dev_path
        .as_deref()
        .is_some_and(|p| p.starts_with(b"/dev/sd"));
    let dos = dos_part(cxt);
    let Some(l) = blm(cxt) else { return -EINVAL };
    l.bsd = vec![0u8; LABEL_SIZE];
    l.set32(off::MAGIC, DISKMAGIC);
    let ty = if scsi { DTYPE_SCSI } else { DTYPE_ST506 };
    l.set16(off::TYPE, u16::from_le_bytes(ty.to_le_bytes()));
    l.set32(off::FLAGS, D_DOSPART);
    l.set32(off::SECSIZE, 512);
    l.set32(off::NSECTORS, u64_as_u32(geom.sectors));
    l.set32(off::NTRACKS, geom.heads);
    l.set32(off::NCYLINDERS, u64_as_u32(geom.cylinders));
    let mut spc = u64_as_u32(geom.sectors.wrapping_mul(u64::from(geom.heads)));
    if spc == 0 {
        // Avoid a division by zero later.
        spc = 1;
    }
    l.set32(off::SECPERCYL, spc);
    let unit = spc.wrapping_mul(u64_as_u32(geom.cylinders));
    l.set32(off::SECPERUNIT, unit);
    l.set16(off::RPM, 3600);
    l.set16(off::INTERLEAVE, 1);
    l.set32(off::MAGIC2, DISKMAGIC);
    l.set32(off::BBSIZE, u32::try_from(BBSIZE).unwrap_or(0));
    l.set32(off::SBSIZE, SBSIZE);
    match dos {
        Some(p) => {
            l.set_npartitions(4);
            // C: the NetBSD partition; D: the whole disk.
            l.set_p(2, p.size, p.start, FS_UNUSED);
            l.set_p(3, unit, 0, FS_UNUSED);
        }
        None => {
            l.set_npartitions(3);
            // C: the whole disk.
            l.set_p(2, unit, 0, FS_UNUSED);
        }
    }
    0
}

/// `bsd_readlabel (cxt)`: the label at the start of the disk or of the
/// parent DOS partition. 0 when it has the magic.
fn readlabel(cxt: &mut Context) -> i32 {
    // Nested, the label is at the DOS partition's start, in the device's
    // own sectors.
    let offset = dos_part(cxt).map_or(0, |p| u64::from(p.start).wrapping_mul(cxt.sector_size));
    let mut buf = vec![0u8; BBSIZE];
    {
        use std::io::{Read, Seek, SeekFrom};
        let Some(dev) = cxt.dev.as_ref() else {
            return -1;
        };
        let mut f = &**dev;
        if f.seek(SeekFrom::Start(offset)).is_err() {
            return -1;
        }
        // `read_all`: as much as there is, short of an error.
        let mut got = 0;
        while got < BBSIZE {
            match f.read(buf.get_mut(got..).unwrap_or(&mut [])) {
                Ok(0) => break,
                Ok(n) => got = got.wrapping_add(n),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return crate::utils::neg_errno(&e),
            }
        }
    }
    let Some(l) = blm(cxt) else { return -1 };
    l.bsdbuffer = buf;
    l.bsd = l
        .bsdbuffer
        .get(LABEL_AT..LABEL_AT + LABEL_SIZE)
        .map(<[u8]>::to_vec)
        .unwrap_or_default();
    if l.u32_at(off::MAGIC) != DISKMAGIC || l.u32_at(off::MAGIC2) != DISKMAGIC {
        return -1;
    }
    let n = usize::from(l.npartitions());
    for t in n..MAXPARTITIONS {
        l.set_p(t, 0, 0, FS_UNUSED);
    }
    let (ns, nt, nc) = (
        l.u32_at(off::NSECTORS),
        l.u32_at(off::NTRACKS),
        l.u32_at(off::NCYLINDERS),
    );
    if n > MAXPARTITIONS {
        cxt.warnx(format!("Too many partitions ({n}, maximum is {MAXPARTITIONS}).").as_bytes());
    }
    // Follow the label's geometry.
    cxt.geom.sectors = u64::from(ns);
    cxt.geom.heads = nt;
    cxt.geom.cylinders = u64::from(nc);
    if cxt.has_user_device_geometry() {
        cxt.apply_user_device_properties();
    }
    set_counts(cxt, n, Some(MAXPARTITIONS), false);
    0
}

/// `bsd_write_disklabel (cxt)`.
fn bsd_write_disklabel(cxt: &mut Context) -> i32 {
    let offset = dos_part(cxt).map_or(0, |p| u64::from(p.start).wrapping_mul(cxt.sector_size));
    let Some(l) = blm(cxt) else { return -EINVAL };
    put(&mut l.bsd, off::CHECKSUM, 2, 0);
    let sum = dkcksum(l);
    put(&mut l.bsd, off::CHECKSUM, 2, u64::from(sum));
    // The label updated within the boot area.
    let label = l.bsd.clone();
    if let Some(dst) = l.bsdbuffer.get_mut(LABEL_AT..LABEL_AT + LABEL_SIZE) {
        dst.copy_from_slice(&label);
    }
    let buf = l.bsdbuffer.clone();
    let shown = cxt.dev_path.clone().unwrap_or_default();
    let rc = cxt.write_to_device(&buf, offset);
    if rc != 0 {
        let mut m = b"cannot write ".to_vec();
        m.extend_from_slice(&shown);
        cxt.warn(rc.wrapping_neg(), &m);
        return rc;
    }
    sync_disks(cxt);
    let parent_changed = cxt
        .get_parent()
        .and_then(Context::label_ref)
        .is_some_and(|l| l.changed);
    let mut m = b"Disklabel written to ".to_vec();
    m.extend_from_slice(&shown);
    if parent_changed {
        m.extend_from_slice(b".  (Don't forget to write the ");
        m.extend_from_slice(
            cxt.get_parent()
                .and_then(|p| p.dev_path.as_deref())
                .unwrap_or(b""),
        );
        m.extend_from_slice(b" disklabel too.)");
    } else {
        m.push(b'.');
    }
    cxt.info(&m);
    0
}

/// `sync_disks (cxt)`.
fn sync_disks(cxt: &mut Context) {
    cxt.info(b"Syncing disks.");
    crate::context::sync();
}

/// `bsd_translate_fstype (linux_type)`.
fn translate_fstype(t: u8) -> u8 {
    match t {
        0x01 | 0x04 | 0x06 | 0xe1 | 0xe3 | 0xf2 => FS_MSDOS,
        0x07 => FS_HPFS,
        _ => FS_OTHER,
    }
}

/// `fdisk_bsd_link_partition (cxt)`: a DOS partition of the parent made a
/// BSD partition, both asked for.
pub fn link_partition(cxt: &mut Context) -> i32 {
    if !cxt
        .get_parent()
        .is_some_and(|p| p.is_labeltype(LabelType::Dos))
    {
        cxt.warnx(b"BSD label is not nested within a DOS partition.");
        return -EINVAL;
    }
    let k = match cxt.get_parent_mut().map(|p| p.ask_partnum(false)) {
        Some(Ok(k)) => k,
        Some(Err(rc)) => return rc,
        None => return -EINVAL,
    };
    let i = match cxt.ask_partnum(true) {
        Ok(i) => i,
        Err(rc) => return rc,
    };
    if i >= MAXPARTITIONS {
        return -EINVAL;
    }
    let Some(p) = parent_entry(cxt, k) else {
        return -EINVAL;
    };
    let Some(l) = blm(cxt) else { return -EINVAL };
    l.set_p(i, p.size, p.start, translate_fstype(p.sys_ind));
    if i >= usize::from(l.npartitions()) {
        l.set_npartitions(u16::try_from(i.wrapping_add(1)).unwrap_or(u16::MAX));
    }
    let n = usize::from(l.npartitions());
    set_counts(cxt, n, None, true);
    let c = char::from(b'a'.wrapping_add(u8::try_from(i).unwrap_or(0)));
    cxt.info(
        format!(
            "BSD partition '{c}' linked to DOS partition {}.",
            k.wrapping_add(1)
        )
        .as_bytes(),
    );
    0
}

/// `bsd_partition_is_used (cxt, partnum)`.
fn bsd_partition_is_used(cxt: &mut Context, partnum: usize) -> i32 {
    if partnum >= MAXPARTITIONS {
        return 0;
    }
    i32::from(bl(cxt).is_some_and(|l| l.p_size(partnum) != 0))
}

/// `bsd_operations`.
static BSD_OPS: LabelOps = LabelOps {
    probe: Some(bsd_probe_label),
    write: Some(bsd_write_disklabel),
    verify: None,
    create: Some(bsd_create_disklabel),
    locate: None,
    reorder: None,
    get_item: Some(bsd_get_disklabel_item),
    set_id: None,
    add_part: Some(bsd_add_partition),
    del_part: Some(bsd_delete_part),
    get_part: Some(bsd_get_partition),
    set_part: Some(bsd_set_partition),
    part_is_used: Some(bsd_partition_is_used),
    part_toggle_flag: None,
    reset_alignment: None,
    deinit: None,
};

/// `bsd_fields`.
static BSD_FIELDS: [Field; 10] = [
    Field::new(fieldtype::DEVICE, "Slice", 1.0, 0),
    Field::new(fieldtype::START, "Start", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::END, "End", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::SECTORS, "Sectors", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::CYLINDERS, "Cylinders", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::SIZE, "Size", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::TYPE, "Type", 8.0, 0),
    Field::new(fieldtype::FSIZE, "Fsize", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::BSIZE, "Bsize", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::CPG, "Cpg", 5.0, FIELDFL_NUMBER),
];

/// `fdisk_new_bsd_label (cxt)`.
#[must_use]
pub fn new_label() -> Label {
    Label {
        name: "bsd",
        id: LabelType::Bsd,
        parttypes: &FSTYPES,
        parttype_cuts: &[],
        nparts_max: 0,
        nparts_cur: 0,
        flags: LABEL_FL_INCHARS_PARTNO | LABEL_FL_REQUIRE_GEOMETRY,
        geom_min: Geometry::default(),
        geom_max: Geometry::default(),
        changed: false,
        disabled: false,
        fields: &BSD_FIELDS,
        op: &BSD_OPS,
        data: LabelData::Bsd(Box::default()),
    }
}
