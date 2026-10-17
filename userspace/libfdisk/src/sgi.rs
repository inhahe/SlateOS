//! `sgi.c`: the SGI (IRIX) disk label driver.
//!
//! The label is the first sector itself -- `struct sgi_disklabel`,
//! big-endian -- so every change is made in the context's in-memory first
//! sector, as upstream makes it through a pointer into that buffer. The
//! driver's own state is the list of free extents `verify_disklabel`
//! rebuilds.

use crate::context::Context;
use crate::errno::{EINVAL, ERANGE};
use crate::field::{FIELDFL_EYECANDY, FIELDFL_NUMBER, Field};
use crate::item::{ItemData, LabelItem};
use crate::label::{Label, LabelData, LabelOps};
use crate::partition::Partition;
use crate::parttype::PartType;
use crate::utils::{be_get as be, be_put as set_be, c_str, u64_as_u32};
use crate::{
    Geometry, LABEL_FL_REQUIRE_GEOMETRY, LabelType, NLABELITEMS, PLURAL, SGI_FLAG_BOOT,
    SGI_FLAG_SWAP, SINGULAR, fieldtype,
};

/// `SGI_LABEL_MAGIC`.
const LABEL_MAGIC: u32 = 0x0be5_a941;
/// `SGI_MAXPARTITIONS`.
const MAXPARTITIONS: usize = 16;
/// `SGI_INFO_MAGIC`.
const INFO_MAGIC: u32 = 0x0007_2959;
/// `SGI_TYPE_*` used here.
const TYPE_VOLHDR: u32 = 0x00;
const TYPE_SWAP: u32 = 0x03;
const TYPE_ENTIRE_DISK: u32 = 0x06;
const TYPE_XFS: u32 = 0x0a;
/// `MBR_LINUX_SWAP_PARTITION`.
const LINUX_SWAP: u32 = 0x82;
/// `SGI_DEVPARAM_*` set on a new label.
const DEVPARAM_TRACK_FWD: u32 = 0x04;
const DEVPARAM_IGNORE_ERRORS: u32 = 0x10;
const DEVPARAM_RESEEK: u32 = 0x20;

/// Field offsets in `struct sgi_disklabel`.
mod off {
    pub const MAGIC: usize = 0;
    pub const ROOT_PART_NUM: usize = 4;
    pub const SWAP_PART_NUM: usize = 6;
    pub const BOOT_FILE: usize = 8;
    pub const DEVPARAM: usize = 24;
    pub const SPARECYL: usize = 27;
    pub const PCYLCOUNT: usize = 28;
    pub const NTRKS: usize = 32;
    pub const NSECT: usize = 38;
    pub const BYTES: usize = 40;
    pub const ILFACT: usize = 42;
    pub const FLAGS: usize = 44;
    pub const RETRIES: usize = 52;
    pub const VOLUME: usize = 72;
    pub const PARTITIONS: usize = 312;
    pub const CSUM: usize = 504;
    /// `sizeof (struct sgi_disklabel)`.
    pub const SIZE: usize = 512;
}

/// `sgi_parttypes`.
static PARTTYPES: [PartType; 18] = [
    PartType::code(0x00, b"SGI volhdr"),
    PartType::code(0x01, b"SGI trkrepl"),
    PartType::code(0x02, b"SGI secrepl"),
    PartType::code(0x03, b"SGI raw"),
    PartType::code(0x04, b"SGI bsd"),
    PartType::code(0x05, b"SGI sysv"),
    PartType::code(0x06, b"SGI volume"),
    PartType::code(0x07, b"SGI efs"),
    PartType::code(0x08, b"SGI lvol"),
    PartType::code(0x09, b"SGI rlvol"),
    PartType::code(0x0a, b"SGI xfs"),
    PartType::code(0x0b, b"SGI xfslog"),
    PartType::code(0x0c, b"SGI xlv"),
    PartType::code(0x0d, b"SGI xvm"),
    PartType::code(0x82, b"Linux swap"),
    PartType::code(0x83, b"Linux native"),
    PartType::code(0x8e, b"Linux LVM"),
    PartType::code(0xfd, b"Linux RAID"),
];

/// `struct fdisk_sgi_label`'s own part: `freelist[SGI_MAXPARTITIONS + 1]`.
#[derive(Clone, Debug, Default)]
pub struct SgiLabel {
    freelist: [(u32, u32); MAXPARTITIONS + 1],
}

/// `sgi_pt_checksum (label)`: 0 for a valid label.
fn pt_checksum(b: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let count = off::SIZE / 4;
    for k in (0..count).rev() {
        sum = sum.wrapping_sub(be(b, k.wrapping_mul(4), 4));
    }
    sum
}

fn sl(cxt: &Context) -> Option<&SgiLabel> {
    match &cxt.label_ref()?.data {
        LabelData::Sgi(s) => Some(s),
        _ => None,
    }
}

fn slm(cxt: &mut Context) -> Option<&mut SgiLabel> {
    match &mut cxt.label_mut()?.data {
        LabelData::Sgi(s) => Some(s),
        _ => None,
    }
}

fn set_changed(cxt: &mut Context) {
    if let Some(lb) = cxt.label_mut() {
        lb.changed = true;
    }
}

fn nmax(cxt: &Context) -> usize {
    cxt.label_ref().map_or(0, |l| l.nparts_max)
}

/// `set_freelist (cxt, i, f, l)`.
fn set_freelist(cxt: &mut Context, i: usize, f: u32, l: u32) {
    if let Some(e) = slm(cxt).and_then(|s| s.freelist.get_mut(i)) {
        *e = (f, l);
    }
}

/// `add_to_freelist (cxt, f, l)`: in the first empty slot.
fn add_to_freelist(cxt: &mut Context, f: u32, l: u32) {
    let i = sl(cxt).map_or(0, |s| {
        s.freelist
            .iter()
            .position(|e| e.1 == 0)
            .unwrap_or(s.freelist.len())
    });
    set_freelist(cxt, i, f, l);
}

/// `clear_freelist (cxt)`.
fn clear_freelist(cxt: &mut Context) {
    if let Some(s) = slm(cxt) {
        s.freelist = [(0, 0); MAXPARTITIONS + 1];
    }
}

/// `is_in_freelist (cxt, b)`: the end of the free extent holding `b`, or 0.
fn is_in_freelist(cxt: &Context, b: u32) -> u32 {
    sl(cxt)
        .and_then(|s| s.freelist.iter().find(|e| e.0 <= b && e.1 >= b))
        .map_or(0, |e| e.1)
}

fn get_nsect(cxt: &Context) -> u32 {
    cxt.firstsector_be(off::NSECT, 2)
}

fn get_ntrks(cxt: &Context) -> u32 {
    cxt.firstsector_be(off::NTRKS, 2)
}

/// `sgi_get_start_sector (cxt, i)`.
fn start_sector(cxt: &Context, i: usize) -> u32 {
    cxt.firstsector_be(
        off::PARTITIONS
            .wrapping_add(i.wrapping_mul(12))
            .wrapping_add(4),
        4,
    )
}

/// `sgi_get_num_sectors (cxt, i)`.
fn num_sectors(cxt: &Context, i: usize) -> u32 {
    cxt.firstsector_be(off::PARTITIONS.wrapping_add(i.wrapping_mul(12)), 4)
}

/// `sgi_get_sysid (cxt, i)`.
fn sysid(cxt: &Context, i: usize) -> u32 {
    cxt.firstsector_be(
        off::PARTITIONS
            .wrapping_add(i.wrapping_mul(12))
            .wrapping_add(8),
        4,
    )
}

/// `sgi_get_bootpartition`.
fn bootpartition(cxt: &Context) -> i32 {
    i32::try_from(cxt.firstsector_be(off::ROOT_PART_NUM, 2)).unwrap_or(0)
}

/// `sgi_get_swappartition`.
fn swappartition(cxt: &Context) -> i32 {
    i32::try_from(cxt.firstsector_be(off::SWAP_PART_NUM, 2)).unwrap_or(0)
}

/// `sgi_get_lastblock (cxt)`: heads x sectors x cylinders, as an
/// `unsigned int`.
fn lastblock(cxt: &Context) -> u32 {
    u64_as_u32(
        u64::from(cxt.geom.heads)
            .wrapping_mul(cxt.geom.sectors)
            .wrapping_mul(cxt.geom.cylinders),
    )
}

/// `count_used_partitions (cxt)`.
fn count_used_partitions(cxt: &Context) -> usize {
    (0..nmax(cxt)).filter(|&i| num_sectors(cxt, i) > 0).count()
}

/// The boot file, as the C string upstream prints: a 16-byte field with
/// no room reserved for the NUL, so a full one runs on into the device
/// parameters, as `printf("%s")` and `strdup` read it.
fn boot_file(cxt: &Context) -> Vec<u8> {
    let fs = cxt.firstsector();
    c_str(fs.get(off::BOOT_FILE..).unwrap_or(&[])).to_vec()
}

/// An `unsigned int` printed with `%d`.
fn as_int(n: u32) -> i32 {
    i32::from_ne_bytes(n.to_ne_bytes())
}

/// `fdisk_sgi_create_info (cxt)`: the "sgilabel" volume in the second
/// sector.
pub fn create_info(cxt: &mut Context) -> i32 {
    cxt.set_firstsector_be(off::VOLUME + 8, 4, 2);
    cxt.set_firstsector_be(off::VOLUME + 12, 4, 512);
    cxt.with_firstsector(|b| {
        if let Some(n) = b.get_mut(off::VOLUME..off::VOLUME + 8) {
            n.copy_from_slice(b"sgilabel");
        }
    });
    cxt.info(b"SGI info created on second sector.");
    0
}

/// `sgi_new_info ()`: `struct sgi_info`, 512 bytes with its padding.
fn new_info() -> Vec<u8> {
    let mut b = vec![0u8; 512];
    set_be(&mut b, 0, 4, INFO_MAGIC);
    set_be(&mut b, 16, 4, 0xffff_ffff);
    set_be(&mut b, 20, 2, 0xffff);
    set_be(&mut b, 22, 2, 1);
    let put = |b: &mut Vec<u8>, at: usize, s: &[u8]| {
        if let Some(d) = b.get_mut(at..at.wrapping_add(s.len())) {
            d.copy_from_slice(s);
        }
    };
    put(&mut b, 94, b"IBM OEM 0662S12         3 30");
    put(&mut b, 144, b"0000");
    set_be(&mut b, 282, 2, 18 * 256 + 16);
    put(&mut b, 284, b"Sfx version 5.3, Oct 18, 1994");
    b
}

/// `sgi_probe_label (cxt)`.
fn sgi_probe_label(cxt: &mut Context) -> i32 {
    let fs = cxt.firstsector();
    if be(&fs, off::MAGIC, 4) != LABEL_MAGIC {
        return 0;
    }
    if pt_checksum(&fs) != 0 {
        cxt.warnx(b"Detected an SGI disklabel with wrong checksum.");
    }
    clear_freelist(cxt);
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_max = MAXPARTITIONS;
    }
    let n = count_used_partitions(cxt);
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_cur = n;
    }
    1
}

/// `SGI_LABELITEM_*`.
mod item {
    use crate::NLABELITEMS;
    pub const PCYLCOUNT: i32 = NLABELITEMS;
    pub const SPARECYL: i32 = NLABELITEMS + 1;
    pub const ILFACT: i32 = NLABELITEMS + 2;
    pub const BOOTFILE: i32 = NLABELITEMS + 3;
}

/// `sgi_get_disklabel_item (cxt, item)`.
fn sgi_get_disklabel_item(cxt: &mut Context, it: &mut LabelItem) -> i32 {
    let fs = cxt.firstsector();
    let (name, data) = match it.id {
        item::PCYLCOUNT => (
            "Physical cylinders",
            ItemData::U64(u64::from(be(&fs, off::PCYLCOUNT, 2))),
        ),
        item::SPARECYL => (
            "Extra sects/cyl",
            ItemData::U64(u64::from(fs.get(off::SPARECYL).copied().unwrap_or(0))),
        ),
        item::ILFACT => (
            "Interleave",
            ItemData::U64(u64::from(be(&fs, off::ILFACT, 2))),
        ),
        item::BOOTFILE => {
            let f = boot_file(cxt);
            (
                "Bootfile",
                if f.is_empty() {
                    ItemData::None
                } else {
                    ItemData::Str(f)
                },
            )
        }
        id => return if id < NLABELITEMS { 1 } else { 2 },
    };
    it.name = Some(name);
    it.data = data;
    0
}

/// `sgi_get_parttype (cxt, n)`.
fn get_parttype(cxt: &Context, n: usize) -> Option<PartType> {
    if n >= nmax(cxt) {
        return None;
    }
    let id = sysid(cxt, n);
    Some(
        cxt.label_ref()
            .and_then(|lb| lb.get_parttype_from_code(id))
            .cloned()
            .unwrap_or_else(|| PartType::new_unknown(id, None)),
    )
}

/// `sgi_get_partition (cxt, n, pa)`.
fn sgi_get_partition(cxt: &mut Context, n: usize, pa: &mut Partition) -> i32 {
    pa.used = num_sectors(cxt, n) > 0;
    if !pa.used {
        return 0;
    }
    pa.start = u64::from(start_sector(cxt, n));
    pa.size = u64::from(num_sectors(cxt, n));
    pa.ty = get_parttype(cxt, n);
    if pa.ty.as_ref().is_some_and(|t| t.code == TYPE_ENTIRE_DISK) {
        pa.wholedisk = true;
    }
    let n = i32::try_from(n).unwrap_or(-1);
    pa.attrs = if swappartition(cxt) == n {
        Some(b"swap".to_vec())
    } else if bootpartition(cxt) == n {
        Some(b"boot".to_vec())
    } else {
        None
    };
    0
}

/// `sgi_check_bootfile (cxt, name)`: 0 when `name` is good and new, 1 when
/// it is the current one, -EINVAL when it is no good.
fn check_bootfile(cxt: &mut Context, name: &[u8]) -> i32 {
    let sz = name.len();
    if sz < 3 {
        // "/a\n" is the shortest.
        cxt.warnx(
            b"Invalid bootfile!  The bootfile must be an absolute non-zero pathname, e.g. \"/unix\" or \
              \"/unix.save\".",
        );
        return -EINVAL;
    }
    if sz > 16 {
        cxt.warnx(b"Name of bootfile is too long: 16 bytes maximum.");
        return -EINVAL;
    }
    if name.first() != Some(&b'/') {
        cxt.warnx(b"Bootfile must have a fully qualified pathname.");
        return -EINVAL;
    }
    let fs = cxt.firstsector();
    let cur = fs.get(off::BOOT_FILE..off::BOOT_FILE + 16).unwrap_or(&[]);
    // strncmp over 16 bytes: equal up to the first NUL of either.
    let same = c_str(cur) == name;
    if !same {
        cxt.warnx(
            b"Be aware that the bootfile is not checked for existence.  SGI's default is \"/unix\", and for \
              backup \"/unix.save\".",
        );
        return 0;
    }
    1
}

/// `fdisk_sgi_set_bootfile (cxt)`: asks for a new boot file.
pub fn set_bootfile(cxt: &mut Context) -> i32 {
    let mut m = b"The current boot file is: ".to_vec();
    m.extend_from_slice(&boot_file(cxt));
    cxt.info(&m);
    let name = match cxt.ask_string(b"Enter of the new boot file") {
        Ok(n) => n.unwrap_or_default(),
        Err(rc) => return rc,
    };
    let rc = check_bootfile(cxt, &name);
    if rc != 0 {
        if rc == 1 {
            cxt.info(b"Boot file is unchanged.");
        }
        return rc;
    }
    cxt.with_firstsector(|b| {
        if let Some(f) = b.get_mut(off::BOOT_FILE..off::BOOT_FILE + 16) {
            f.fill(0);
            if let Some(d) = f.get_mut(..name.len()) {
                d.copy_from_slice(&name);
            }
        }
    });
    let mut m = b"Bootfile has been changed to \"".to_vec();
    m.extend_from_slice(&name);
    m.extend_from_slice(b"\".");
    cxt.info(&m);
    0
}

/// `sgi_write_disklabel (cxt)`: the label, and the info block when the
/// first volume is "sgilabel".
fn sgi_write_disklabel(cxt: &mut Context) -> i32 {
    cxt.set_firstsector_be(off::CSUM, 4, 0);
    let sum = pt_checksum(&cxt.firstsector());
    cxt.set_firstsector_be(off::CSUM, 4, sum);
    let fs = cxt.firstsector();
    let rc = cxt.write_to_device(fs.get(..512).unwrap_or(&fs), 0);
    if rc != 0 {
        return rc;
    }
    if fs.get(off::VOLUME..off::VOLUME + 8) == Some(b"sgilabel".as_slice()) {
        // Upstream keeps "this habit of first writing the sgilabel" from
        // the original fdisk. The block number is an `int`: a negative
        // offset is lseek's EINVAL.
        let block = as_int(be(&fs, off::VOLUME + 8, 4));
        let Ok(at) = u64::try_from(i64::from(block).wrapping_mul(512)) else {
            return -EINVAL;
        };
        let rc = cxt.write_to_device(&new_info(), at);
        if rc != 0 {
            return rc;
        }
    }
    0
}

/// `compare_start (cxt, x, y)`: by start, the larger first at the same
/// start (entry zero is the entire disk).
fn compare_start(cxt: &Context, i: usize, j: usize) -> i32 {
    let (a, b) = (start_sector(cxt, i), start_sector(cxt, j));
    let (c, d) = (num_sectors(cxt, i), num_sectors(cxt, j));
    if a == b {
        return match d.cmp(&c) {
            std::cmp::Ordering::Greater => 1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Less => -1,
        };
    }
    if a > b { 1 } else { -1 }
}

/// `sort (base, num, size, cxt, cmp)`: upstream's heap sort (from the
/// kernel's), kept because it is not stable and the order of equal entries
/// shows in the warnings.
fn heap_sort(cxt: &Context, v: &mut [usize]) {
    let num = v.len();
    let cmp = |v: &[usize], a: usize, b: usize| -> i32 {
        match (v.get(a), v.get(b)) {
            (Some(&x), Some(&y)) => compare_start(cxt, x, y),
            _ => 0,
        }
    };
    if num >= 2 {
        let mut i = (num / 2).wrapping_sub(1);
        loop {
            let mut r = i;
            while r.wrapping_mul(2).wrapping_add(1) < num {
                let mut c = r.wrapping_mul(2).wrapping_add(1);
                if c < num.wrapping_sub(1) && cmp(v, c, c.wrapping_add(1)) < 0 {
                    c = c.wrapping_add(1);
                }
                if cmp(v, r, c) >= 0 {
                    break;
                }
                v.swap(r, c);
                r = c;
            }
            if i == 0 {
                break;
            }
            i = i.wrapping_sub(1);
        }
    }
    let mut i = num.wrapping_sub(1);
    while num > 0 && i > 0 {
        v.swap(0, i);
        let mut r = 0usize;
        while r.wrapping_mul(2).wrapping_add(1) < i {
            let mut c = r.wrapping_mul(2).wrapping_add(1);
            if c < i.wrapping_sub(1) && cmp(v, c, c.wrapping_add(1)) < 0 {
                c = c.wrapping_add(1);
            }
            if cmp(v, r, c) >= 0 {
                break;
            }
            v.swap(r, c);
            r = c;
        }
        i = i.wrapping_sub(1);
    }
}

/// `P_(singular, plural, n)`.
fn plural(n: u32, one: String, many: String) -> String {
    if n == 1 { one } else { many }
}

/// `verify_disklabel (cxt, verbose)`: the free list rebuilt; 1 when there
/// is free space, 0 when the disk is exactly covered, -1 on an overlap.
fn verify_disklabel(cxt: &mut Context, verbose: bool) -> i32 {
    let mut index: Vec<usize> = Vec::new();
    let mut entire: i32 = 0;
    let mut gap: i64 = 0;
    let mut last = lastblock(cxt);
    clear_freelist(cxt);
    for i in 0..MAXPARTITIONS {
        if num_sectors(cxt, i) != 0 {
            index.push(i);
            if sysid(cxt, i) == TYPE_ENTIRE_DISK {
                let was = entire;
                entire = entire.wrapping_add(1);
                if was == 1 && verbose {
                    cxt.info(b"More than one entire disk entry present.");
                }
            }
        }
    }
    if index.is_empty() {
        if verbose {
            cxt.info(b"No partitions defined.");
        }
        if last != 0 {
            add_to_freelist(cxt, 0, last);
        }
        return i32::from(last > 0);
    }
    heap_sort(cxt, &mut index);
    let at = |k: usize| index.get(k).copied().unwrap_or(0);

    if sysid(cxt, at(0)) == TYPE_ENTIRE_DISK {
        if verbose && at(0) != 10 {
            cxt.info(b"IRIX likes it when partition 11 covers the entire disk.");
        }
        if verbose && start_sector(cxt, at(0)) != 0 {
            cxt.info(
                format!(
                    "The entire disk partition should start at block 0, not at block {}.",
                    as_int(start_sector(cxt, at(0)))
                )
                .as_bytes(),
            );
        }
        last = num_sectors(cxt, at(0));
    } else if verbose {
        cxt.info(b"Partition 11 should cover the entire disk.");
    }

    let mut start: u32 = 0;
    for k in 1..index.len() {
        let cylsize = get_nsect(cxt).wrapping_mul(get_ntrks(cxt));
        let ik = at(k);
        // Several "entire disk" entries are not handled.
        if sysid(cxt, ik) == TYPE_ENTIRE_DISK {
            continue;
        }
        let s = start_sector(cxt, ik);
        if start > s {
            if verbose {
                let n = start.wrapping_sub(s);
                let (a, b) = (at(k.wrapping_sub(1)).wrapping_add(1), ik.wrapping_add(1));
                cxt.info(
                    plural(
                        n,
                        format!("Partitions {a} and {b} overlap by {} sector.", as_int(n)),
                        format!("Partitions {a} and {b} overlap by {} sectors.", as_int(n)),
                    )
                    .as_bytes(),
                );
            }
            if gap > 0 {
                gap = gap.wrapping_neg();
            }
            if gap == 0 {
                gap = -1;
            }
        }
        if start < s {
            let n = s.wrapping_sub(start);
            if verbose {
                cxt.info(
                    plural(
                        n,
                        format!("Unused gap of {n:8} sector: sector {start:8}"),
                        format!(
                            "Unused gap of {n:8} sectors: sectors {start:8}-{}",
                            s.wrapping_sub(1)
                        ),
                    )
                    .as_bytes(),
                );
            }
            gap = gap.wrapping_add(i64::from(n));
            add_to_freelist(cxt, start, s);
        }
        start = s.wrapping_add(num_sectors(cxt, ik));
        // Free space aligned to cylinders.
        if cylsize != 0 && !start.is_multiple_of(cylsize) {
            start =
                start.wrapping_add(cylsize.wrapping_sub(start.checked_rem(cylsize).unwrap_or(0)));
        }
    }
    if start < last {
        let n = last.wrapping_sub(start);
        if verbose {
            cxt.info(
                plural(
                    n,
                    format!("Unused gap of {n:8} sector: sector {start:8}"),
                    format!(
                        "Unused gap of {n:8} sectors: sectors {start:8}-{}",
                        last.wrapping_sub(1)
                    ),
                )
                .as_bytes(),
            );
        }
        gap = gap.wrapping_add(i64::from(n));
        add_to_freelist(cxt, start, last);
    }
    if verbose {
        let bp = bootpartition(cxt);
        if bp < 0 || num_sectors(cxt, usize::try_from(bp).unwrap_or(0)) == 0 {
            cxt.info(b"The boot partition does not exist.");
        }
        let sp = swappartition(cxt);
        let spu = usize::try_from(sp).unwrap_or(0);
        if sp < 0 || num_sectors(cxt, spu) == 0 {
            cxt.info(b"The swap partition does not exist.");
        } else if sysid(cxt, spu) != TYPE_SWAP && sysid(cxt, spu) != LINUX_SWAP {
            cxt.info(b"The swap partition has no swap type.");
        }
        if check_bootfile(cxt, b"/unix") != 0 {
            cxt.info(b"You have chosen an unusual bootfile name.");
        }
    }
    match gap.cmp(&0) {
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Less => -1,
    }
}

/// `sgi_verify_disklabel (cxt)`.
fn sgi_verify_disklabel(cxt: &mut Context) -> i32 {
    verify_disklabel(cxt, true)
}

/// `sgi_gaps (cxt)`.
fn gaps(cxt: &mut Context) -> i32 {
    verify_disklabel(cxt, false)
}

/// `sgi_entire (cxt)`: the first entire-disk entry.
fn entire(cxt: &Context) -> Option<usize> {
    (0..MAXPARTITIONS).find(|&i| sysid(cxt, i) == TYPE_ENTIRE_DISK)
}

/// `set_partition (cxt, i, start, length, sys)`.
fn set_partition(cxt: &mut Context, i: usize, start: u32, length: u32, sys: u32) -> i32 {
    let base = off::PARTITIONS.wrapping_add(i.wrapping_mul(12));
    cxt.set_firstsector_be(base.wrapping_add(8), 4, sys);
    cxt.set_firstsector_be(base, 4, length);
    cxt.set_firstsector_be(base.wrapping_add(4), 4, start);
    set_changed(cxt);
    // The free list rebuilt.
    if gaps(cxt) < 0 {
        cxt.warnx(b"Partition overlap on the disk.");
    }
    if length != 0 {
        let t = cxt
            .label_ref()
            .and_then(|lb| lb.get_parttype_from_code(sys))
            .cloned();
        let num = i64::try_from(i.wrapping_add(1)).unwrap_or(0);
        // Upstream passes one past the end, in `unsigned int`, so the size
        // the message reports is a sector too large; kept.
        let stop = u64::from(start.wrapping_add(length));
        cxt.info_new_partition(num, u64::from(start), stop, t.as_ref());
    }
    0
}

/// `sgi_set_entire (cxt)`: partition 11 the whole disk, if free.
fn set_entire(cxt: &mut Context) {
    for n in 10..nmax(cxt) {
        if num_sectors(cxt, n) == 0 {
            let lb = lastblock(cxt);
            set_partition(cxt, n, 0, lb, TYPE_ENTIRE_DISK);
            break;
        }
    }
}

/// `sgi_set_volhdr (cxt)`: partition 9 a 4096-sector volume header, if
/// free.
fn set_volhdr(cxt: &mut Context) {
    for n in 8..nmax(cxt) {
        if num_sectors(cxt, n) == 0 {
            // The size IRIX fx uses.
            if 4096 < lastblock(cxt) {
                set_partition(cxt, n, 0, 4096, TYPE_VOLHDR);
            }
            break;
        }
    }
}

/// `sgi_delete_partition (cxt, partnum)`.
fn sgi_delete_partition(cxt: &mut Context, partnum: usize) -> i32 {
    if partnum > nmax(cxt) {
        return -EINVAL;
    }
    let rc = set_partition(cxt, partnum, 0, 0, 0);
    let n = count_used_partitions(cxt);
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_cur = n;
    }
    rc
}

/// `sgi_add_partition (cxt, pa, &partno)`.
fn sgi_add_partition(
    cxt: &mut Context,
    pa: Option<&mut Partition>,
    partno: Option<&mut usize>,
) -> i32 {
    let pa = pa.map(|p| &*p);
    let mut sys = pa.and_then(|p| p.ty.as_ref()).map_or(TYPE_XFS, |t| t.code);
    let n = match cxt.partition_next_partno(pa) {
        Ok(n) => n,
        Err(rc) => return rc,
    };
    if n == 10 {
        sys = TYPE_ENTIRE_DISK;
    } else if n == 8 {
        sys = 0;
    }
    if num_sectors(cxt, n) != 0 {
        cxt.warnx(
            format!(
                "Partition {} is already defined.  Delete it before re-adding it.",
                n.wrapping_add(1)
            )
            .as_bytes(),
        );
        return -EINVAL;
    }
    if cxt.script.is_none() && entire(cxt).is_none() && sys != TYPE_ENTIRE_DISK {
        cxt.info(b"Attempting to generate entire disk entry automatically.");
        set_entire(cxt);
        set_volhdr(cxt);
    }
    if gaps(cxt) == 0 && sys != TYPE_ENTIRE_DISK {
        cxt.warnx(b"The entire disk is already covered with partitions.");
        return -EINVAL;
    }
    if gaps(cxt) < 0 {
        cxt.warnx(b"You got a partition overlap on the disk. Fix it first!");
        return -EINVAL;
    }
    let (mut first, mut last) = if sys == TYPE_ENTIRE_DISK {
        (0, lastblock(cxt))
    } else {
        sl(cxt)
            .and_then(|s| s.freelist.first().copied())
            .unwrap_or((0, 0))
    };

    // The first sector.
    if pa.is_some_and(|p| p.start_follow_default) {
        // The start of the first free extent, as found.
    } else if let Some(p) = pa.filter(|p| p.has_start()) {
        first = u64_as_u32(p.start);
        last = is_in_freelist(cxt, first);
        if sys != TYPE_ENTIRE_DISK && last == 0 {
            return -ERANGE;
        }
    } else {
        let mut ask = crate::Ask::new(crate::AskType::Number);
        ask.set_query(format!("First {}", cxt.get_unit(SINGULAR)).as_bytes());
        ask.number_set_low(cxt.scround(u64::from(first)));
        ask.number_set_default(cxt.scround(u64::from(first)));
        ask.number_set_high(cxt.scround(u64::from(last)).wrapping_sub(1));
        let rc = cxt.do_ask(&mut ask);
        first = u64_as_u32(ask.number_get_result());
        if rc != 0 {
            return rc;
        }
        if cxt.use_cylinders() {
            first = first.wrapping_mul(cxt.get_units_per_sector());
        }
    }
    if first != 0 && sys == TYPE_ENTIRE_DISK {
        cxt.info(
            b"It is highly recommended that the eleventh partition covers the entire disk and is of type \
              'SGI volume'.",
        );
    }
    if last == 0 {
        last = is_in_freelist(cxt, first);
    }

    // The last sector.
    if pa.is_some_and(|p| p.end_follow_default) {
        last = last.wrapping_sub(1);
    } else if let Some(p) = pa.filter(|p| p.has_size()) {
        let e = u64::from(first).wrapping_add(p.size).wrapping_sub(1);
        if e > u64::from(last) {
            return -ERANGE;
        }
        last = u64_as_u32(e);
    } else {
        let mut ask = crate::Ask::new(crate::AskType::Offset);
        ask.set_query(
            format!(
                "Last {} or +{} or +size{{K,M,G,T,P}}",
                cxt.get_unit(SINGULAR),
                cxt.get_unit(PLURAL)
            )
            .as_bytes(),
        );
        ask.number_set_low(cxt.scround(u64::from(first)));
        ask.number_set_default(cxt.scround(u64::from(last)).wrapping_sub(1));
        ask.number_set_high(cxt.scround(u64::from(last)).wrapping_sub(1));
        ask.number_set_base(cxt.scround(u64::from(first)));
        ask.number_set_wrap_negative(true);
        if cxt.use_cylinders() {
            ask.number_set_unit(
                cxt.sector_size
                    .wrapping_mul(u64::from(cxt.get_units_per_sector())),
            );
        } else {
            ask.number_set_unit(cxt.sector_size);
        }
        let rc = cxt.do_ask(&mut ask);
        last = u64_as_u32(ask.number_get_result().wrapping_add(1));
        if rc != 0 {
            return rc;
        }
        if cxt.use_cylinders() {
            last = last.wrapping_mul(cxt.get_units_per_sector());
        }
    }
    if sys == TYPE_ENTIRE_DISK && (first != 0 || last != lastblock(cxt)) {
        cxt.info(
            b"It is highly recommended that the eleventh partition covers the entire disk and is of type \
              'SGI volume'.",
        );
    }
    set_partition(cxt, n, first, last.wrapping_sub(first), sys);
    let used = count_used_partitions(cxt);
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_cur = used;
    }
    if let Some(partno) = partno {
        *partno = n;
    }
    0
}

/// `sgi_create_disklabel (cxt)`.
fn sgi_create_disklabel(cxt: &mut Context) -> i32 {
    if cxt.geom.heads != 0 && cxt.geom.sectors != 0 {
        match cxt.dev.as_ref().map(|d| ulblkid::blkdev::get_sectors(d)) {
            Some(Ok(llsectors)) => {
                // The size ioctl worked.
                let sec_fac = cxt.sector_size / 512;
                let llcyls = crate::utils::div(
                    llsectors,
                    u64::from(cxt.geom.heads)
                        .wrapping_mul(cxt.geom.sectors)
                        .wrapping_mul(sec_fac),
                );
                cxt.geom.cylinders = llcyls;
            }
            _ => {
                let mut m = b"BLKGETSIZE ioctl failed on ".to_vec();
                m.extend_from_slice(cxt.dev_path.as_deref().unwrap_or(b"(null)"));
                m.extend_from_slice(
                    format!(
                        ". Using geometry cylinder value of {}. This value may be truncated for devices > 33.8 GB.",
                        cxt.geom.cylinders
                    )
                    .as_bytes(),
                );
                cxt.warnx(&m);
            }
        }
    }
    let rc = cxt.init_firstsector_buffer(0, 0);
    if rc != 0 {
        return rc;
    }
    let g = cxt.geom;
    let ss = cxt.sector_size;
    cxt.with_firstsector(|b| {
        set_be(b, off::MAGIC, 4, LABEL_MAGIC);
        set_be(b, off::ROOT_PART_NUM, 2, 0);
        set_be(b, off::SWAP_PART_NUM, 2, 1);
        if let Some(f) = b.get_mut(off::BOOT_FILE..off::BOOT_FILE + 16) {
            f.fill(0);
            if let Some(d) = f.get_mut(..5) {
                d.copy_from_slice(b"/unix");
            }
        }
        // The device parameters: zeros but for these.
        if let Some(d) = b.get_mut(off::DEVPARAM..off::VOLUME) {
            d.fill(0);
        }
        set_be(b, off::PCYLCOUNT, 2, u64_as_u32(g.cylinders) & 0xffff);
        set_be(b, off::NTRKS, 2, g.heads & 0xffff);
        set_be(b, off::NSECT, 2, u64_as_u32(g.sectors) & 0xffff);
        set_be(b, off::BYTES, 2, u64_as_u32(ss) & 0xffff);
        set_be(b, off::ILFACT, 2, 1);
        set_be(
            b,
            off::FLAGS,
            4,
            DEVPARAM_TRACK_FWD | DEVPARAM_IGNORE_ERRORS | DEVPARAM_RESEEK,
        );
        set_be(b, off::RETRIES, 4, 1);
        if let Some(v) = b.get_mut(off::VOLUME..off::CSUM) {
            v.fill(0);
        }
    });
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_max = MAXPARTITIONS;
    }
    // No default layout when following a script.
    if cxt.script.is_none() {
        set_entire(cxt);
        set_volhdr(cxt);
    }
    let n = count_used_partitions(cxt);
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_cur = n;
    }
    cxt.info(b"Created a new SGI disklabel.");
    0
}

/// `sgi_set_partition (cxt, i, pa)`.
fn sgi_set_partition(cxt: &mut Context, i: usize, pa: &Partition) -> i32 {
    if i >= nmax(cxt) {
        return -EINVAL;
    }
    let base = off::PARTITIONS.wrapping_add(i.wrapping_mul(12));
    if let Some(t) = pa.ty.as_ref() {
        if num_sectors(cxt, i) == 0 {
            cxt.warnx(b"Sorry, only for non-empty partitions you can change the tag.");
            return -EINVAL;
        }
        if (i == 10 && t.code != TYPE_ENTIRE_DISK) || (i == 8 && t.code != 0) {
            cxt.info(
                b"Consider leaving partition 9 as volume header (0), and partition 11 as entire volume (6), as \
                  IRIX expects it.",
            );
        }
        if cxt.script.is_none()
            && t.code != TYPE_ENTIRE_DISK
            && t.code != TYPE_VOLHDR
            && start_sector(cxt, i) < 1
        {
            let yes = cxt
                .ask_yesno(
                    b"It is highly recommended that the partition at offset 0 is of type \"SGI volhdr\", the IRIX \
                      system will rely on it to retrieve from its directory standalone tools like sash and fx. Only \
                      the \"SGI volume\" entire disk section may violate this. Are you sure about tagging this \
                      partition differently?",
                )
                .unwrap_or(false);
            if !yes {
                return 1;
            }
        }
        cxt.set_firstsector_be(base.wrapping_add(8), 4, t.code);
    }
    if pa.has_start() {
        cxt.set_firstsector_be(base.wrapping_add(4), 4, u64_as_u32(pa.start));
    }
    if pa.has_size() {
        cxt.set_firstsector_be(base, 4, u64_as_u32(pa.size));
    }
    set_changed(cxt);
    0
}

/// `sgi_partition_is_used (cxt, i)`.
fn sgi_partition_is_used(cxt: &mut Context, i: usize) -> i32 {
    if i >= nmax(cxt) {
        return 0;
    }
    i32::from(num_sectors(cxt, i) != 0)
}

/// `sgi_toggle_partition_flag (cxt, i, flag)`: the boot or swap partition
/// set to `i`, or back to 0 when it is `i`.
fn sgi_toggle_partition_flag(cxt: &mut Context, i: usize, flag: u64) -> i32 {
    if i >= nmax(cxt) {
        return -EINVAL;
    }
    let at = match flag {
        SGI_FLAG_BOOT => off::ROOT_PART_NUM,
        SGI_FLAG_SWAP => off::SWAP_PART_NUM,
        _ => return 1,
    };
    let cur = cxt.firstsector_be(at, 2);
    let iu = u32::try_from(i).unwrap_or(u32::MAX);
    cxt.set_firstsector_be(at, 2, if cur == iu { 0 } else { iu & 0xffff });
    set_changed(cxt);
    0
}

/// `sgi_fields`.
static SGI_FIELDS: [Field; 9] = [
    Field::new(fieldtype::DEVICE, "Device", 10.0, 0),
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
    Field::new(fieldtype::TYPE, "Type", 0.1, FIELDFL_EYECANDY),
    Field::new(fieldtype::ATTR, "Attrs", 0.0, FIELDFL_NUMBER),
];

/// `sgi_operations`.
static SGI_OPS: LabelOps = LabelOps {
    probe: Some(sgi_probe_label),
    write: Some(sgi_write_disklabel),
    verify: Some(sgi_verify_disklabel),
    create: Some(sgi_create_disklabel),
    locate: None,
    reorder: None,
    get_item: Some(sgi_get_disklabel_item),
    set_id: None,
    add_part: Some(sgi_add_partition),
    del_part: Some(sgi_delete_partition),
    get_part: Some(sgi_get_partition),
    set_part: Some(sgi_set_partition),
    part_is_used: Some(sgi_partition_is_used),
    part_toggle_flag: Some(sgi_toggle_partition_flag),
    reset_alignment: None,
    deinit: None,
};

/// `fdisk_new_sgi_label (cxt)`.
#[must_use]
pub fn new_label() -> Label {
    Label {
        name: "sgi",
        id: LabelType::Sgi,
        parttypes: &PARTTYPES,
        parttype_cuts: &[],
        nparts_max: 0,
        nparts_cur: 0,
        flags: LABEL_FL_REQUIRE_GEOMETRY,
        geom_min: Geometry::default(),
        geom_max: Geometry::default(),
        changed: false,
        disabled: false,
        fields: &SGI_FIELDS,
        op: &SGI_OPS,
        data: LabelData::Sgi(Box::default()),
    }
}
