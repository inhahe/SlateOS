//! `sun.c`: the Sun (SunOS/Solaris VTOC) disk label driver.
//!
//! Like SGI's, the label is the first sector itself -- `struct
//! sun_disklabel`, big-endian -- and is changed in the context's in-memory
//! first sector. Partitions start on a cylinder boundary: the label stores
//! a start *cylinder* and a length in sectors, so the context's geometry
//! is part of every address. The driver keeps no state of its own.

use crate::context::Context;
use crate::errno::{EINVAL, ERANGE};
use crate::field::{FIELDFL_NUMBER, Field};
use crate::item::{ItemData, LabelItem};
use crate::label::{Label, LabelData, LabelOps};
use crate::partition::Partition;
use crate::parttype::PartType;
use crate::utils::{c_str, div, rem, u64_as_u32};
use crate::{
    Geometry, LABEL_FL_REQUIRE_GEOMETRY, LabelType, NLABELITEMS, PLURAL, SINGULAR, fieldtype,
};

/// `SUN_LABEL_MAGIC`.
const LABEL_MAGIC: u32 = 0xDABE;
/// `SUN_VTOC_SANITY`.
const VTOC_SANITY: u32 = 0x600D_DEEE;
/// `SUN_VTOC_VERSION`.
const VTOC_VERSION: u32 = 1;
/// `SUN_MAXPARTITIONS`.
const MAXPARTITIONS: usize = 8;

/// `SUN_TAG_*` used here.
const TAG_UNASSIGNED: u32 = 0x00;
const TAG_SWAP: u32 = 0x03;
const TAG_WHOLEDISK: u32 = 0x05;
const TAG_LINUX_SWAP: u32 = 0x82;
const TAG_LINUX_NATIVE: u32 = 0x83;

/// `SUN_FLAG_UNMNT`: an unmountable partition, for
/// [`Context::toggle_partition_flag`].
pub const FLAG_UNMNT: u64 = 0x01;
/// `SUN_FLAG_RONLY`: a read-only partition.
pub const FLAG_RONLY: u64 = 0x10;
/// The same flags as stored in `vtoc.infos[i].flags`.
const UNMNT_BIT: u32 = 0x01;
const RONLY_BIT: u32 = 0x10;

/// Field offsets in `struct sun_disklabel`.
mod off {
    pub const LABEL_ID: usize = 0;
    pub const VERSION: usize = 128;
    pub const VOLUME_ID: usize = 132;
    pub const NPARTS: usize = 140;
    /// `vtoc.infos[8]`: `id` and `flags`, 4 bytes each.
    pub const INFOS: usize = 142;
    pub const SANITY: usize = 188;
    pub const RPM: usize = 420;
    pub const PCYL: usize = 422;
    pub const APC: usize = 424;
    pub const INTRLV: usize = 430;
    pub const NCYL: usize = 432;
    pub const ACYL: usize = 434;
    pub const NHEAD: usize = 436;
    pub const NSECT: usize = 438;
    /// `partitions[8]`: `start_cylinder` and `num_sectors`, 8 bytes each.
    pub const PARTITIONS: usize = 444;
    pub const MAGIC: usize = 508;
    pub const CSUM: usize = 510;
    /// `sizeof (struct sun_disklabel)`.
    pub const SIZE: usize = 512;
}

/// `SUN_LABELITEM_*`.
mod item {
    use crate::NLABELITEMS;
    pub const LABELID: i32 = NLABELITEMS;
    pub const VTOCID: i32 = NLABELITEMS + 1;
    pub const RPM: i32 = NLABELITEMS + 2;
    pub const ACYL: i32 = NLABELITEMS + 3;
    pub const PCYL: i32 = NLABELITEMS + 4;
    pub const APC: i32 = NLABELITEMS + 5;
    pub const INTRLV: i32 = NLABELITEMS + 6;
}

/// `sun_parttypes`.
static PARTTYPES: [PartType; 16] = [
    PartType::code(0x00, b"Unassigned"),
    PartType::code(0x01, b"Boot"),
    PartType::code(0x02, b"SunOS root"),
    PartType::code(0x03, b"SunOS swap"),
    PartType::code(0x04, b"SunOS usr"),
    PartType::code(0x05, b"Whole disk"),
    PartType::code(0x06, b"SunOS stand"),
    PartType::code(0x07, b"SunOS var"),
    PartType::code(0x08, b"SunOS home"),
    PartType::code(0x09, b"SunOS alt sectors"),
    PartType::code(0x0a, b"SunOS cachefs"),
    PartType::code(0x0b, b"SunOS reserved"),
    PartType::code(0x82, b"Linux swap"),
    PartType::code(0x83, b"Linux native"),
    PartType::code(0x8e, b"Linux LVM"),
    PartType::code(0xfd, b"Linux raid autodetect"),
];

/// `sun_pt_checksum (label)`: the XOR of the label's 16-bit words, which
/// is zero for a valid label. The words are XORed as the host loads them,
/// so the sum is two bytes -- of the even and of the odd offsets -- in
/// the order they are stored.
fn pt_checksum(b: &[u8]) -> [u8; 2] {
    let mut sum = [0u8; 2];
    for (k, &c) in b.iter().take(off::SIZE).enumerate() {
        if let Some(s) = sum.get_mut(k & 1) {
            *s ^= c;
        }
    }
    sum
}

/// `sunlabel->csum = 0; sunlabel->csum = sun_pt_checksum (sunlabel)`.
fn update_checksum(cxt: &Context) {
    cxt.with_firstsector(|b| {
        if let Some(c) = b.get_mut(off::CSUM..off::SIZE) {
            c.fill(0);
        }
        let sum = pt_checksum(b);
        if let Some(c) = b.get_mut(off::CSUM..off::SIZE) {
            c.copy_from_slice(&sum);
        }
    });
}

fn set_changed(cxt: &mut Context) {
    if let Some(lb) = cxt.label_mut() {
        lb.changed = true;
    }
}

fn nmax(cxt: &Context) -> usize {
    cxt.label_ref().map_or(0, |l| l.nparts_max)
}

/// `vtoc.infos[i].id`.
fn info_id_at(i: usize) -> usize {
    off::INFOS.wrapping_add(i.wrapping_mul(4))
}

/// `vtoc.infos[i].flags`.
fn info_flags_at(i: usize) -> usize {
    info_id_at(i).wrapping_add(2)
}

/// `partitions[i].start_cylinder`.
fn start_cyl_at(i: usize) -> usize {
    off::PARTITIONS.wrapping_add(i.wrapping_mul(8))
}

/// `partitions[i].num_sectors`.
fn nsect_at(i: usize) -> usize {
    start_cyl_at(i).wrapping_add(4)
}

/// `cxt->geom.heads * cxt->geom.sectors`: sectors per cylinder.
fn cylsize(cxt: &Context) -> u64 {
    u64::from(cxt.geom.heads).wrapping_mul(cxt.geom.sectors)
}

/// `cylinders * heads * sectors`, as the `uint32_t` it is stored in.
fn disk_sectors(cxt: &Context) -> u32 {
    u64_as_u32(cxt.geom.cylinders.wrapping_mul(cylsize(cxt)))
}

/// An `unsigned int` printed with `%d`.
fn as_int(n: u32) -> i32 {
    i32::from_ne_bytes(n.to_ne_bytes())
}

/// `set_partition (cxt, i, start, stop, sysid)`: `stop` is one past the
/// end, and `sysid` a `uint16_t`, so only its low 16 bits are kept.
fn set_partition(cxt: &mut Context, i: usize, start: u64, stop: u64, sysid: u32) {
    let sysid = sysid & 0xffff;
    let t = cxt
        .label_ref()
        .and_then(|lb| lb.get_parttype_from_code(sysid))
        .cloned();
    let cyl = div(start, cylsize(cxt));
    if cyl > u64::from(u32::MAX) {
        cxt.warnx(
            format!(
                "#{}: start cylinder overflows Sun label limits",
                i.wrapping_add(1)
            )
            .as_bytes(),
        );
    }
    if stop.wrapping_sub(start) > u64::from(u32::MAX) {
        cxt.warnx(
            format!(
                "#{}: number of sectors overflow Sun label limits",
                i.wrapping_add(1)
            )
            .as_bytes(),
        );
    }
    cxt.set_firstsector_be(info_id_at(i), 2, sysid);
    cxt.set_firstsector_be(info_flags_at(i), 2, 0);
    cxt.set_firstsector_be(start_cyl_at(i), 4, u64_as_u32(cyl));
    cxt.set_firstsector_be(nsect_at(i), 4, u64_as_u32(stop.wrapping_sub(start)));
    set_changed(cxt);
    let num = i64::try_from(i.wrapping_add(1)).unwrap_or(0);
    cxt.info_new_partition(num, start, stop, t.as_ref());
}

/// `count_used_partitions (cxt)`.
fn count_used_partitions(cxt: &Context) -> usize {
    (0..nmax(cxt))
        .filter(|&i| cxt.firstsector_be(nsect_at(i), 4) != 0)
        .count()
}

/// `sun_probe_label (cxt)`.
fn sun_probe_label(cxt: &mut Context) -> i32 {
    if cxt.firstsector_be(off::MAGIC, 2) != LABEL_MAGIC {
        return 0;
    }
    if cxt
        .with_firstsector(|b| pt_checksum(b) != [0, 0])
        .unwrap_or(false)
    {
        cxt.warnx(
            b"Detected sun disklabel with wrong checksum. Probably you'll have to set all the values, e.g. heads, \
              sectors, cylinders and partitions or force a fresh label (s command in main menu)",
        );
        return 1;
    }
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_max = MAXPARTITIONS;
    }
    cxt.geom.heads = cxt.firstsector_be(off::NHEAD, 2);
    cxt.geom.cylinders = u64::from(cxt.firstsector_be(off::NCYL, 2));
    cxt.geom.sectors = u64::from(cxt.firstsector_be(off::NSECT, 2));

    // The label has a geometry, but the user's wins.
    if cxt.has_user_device_geometry() {
        cxt.apply_user_device_properties();
    }

    let mut need_fixing = false;
    let version = cxt.firstsector_be(off::VERSION, 4);
    if version != VTOC_VERSION {
        cxt.warnx(
            format!(
                "Detected sun disklabel with wrong version [{}].",
                as_int(version)
            )
            .as_bytes(),
        );
        need_fixing = true;
    }
    let sanity = cxt.firstsector_be(off::SANITY, 4);
    if sanity != VTOC_SANITY {
        cxt.warnx(
            format!("Detected sun disklabel with wrong vtoc.sanity [0x{sanity:08x}].").as_bytes(),
        );
        need_fixing = true;
    }
    let nparts = cxt.firstsector_be(off::NPARTS, 2);
    if usize::try_from(nparts).ok() != Some(MAXPARTITIONS) {
        cxt.warnx(format!("Detected sun disklabel with wrong vtoc.nparts [{nparts}].").as_bytes());
        need_fixing = true;
    }
    if need_fixing {
        cxt.warnx(b"Warning: Wrong values need to be fixed up and will be corrected by w(rite)");
        cxt.set_firstsector_be(off::VERSION, 4, VTOC_VERSION);
        cxt.set_firstsector_be(off::SANITY, 4, VTOC_SANITY);
        cxt.set_firstsector_be(off::NPARTS, 2, 8);
        update_checksum(cxt);
        set_changed(cxt);
    }
    let n = count_used_partitions(cxt);
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_cur = n;
    }
    1
}

/// `ask_geom (cxt)`: heads, sectors and cylinders, within the label's
/// limits; an answer not given leaves the value as it was.
fn ask_geom(cxt: &mut Context) {
    let (min, max) = cxt
        .label_ref()
        .map_or((Geometry::default(), Geometry::default()), |l| {
            (l.geom_min, l.geom_max)
        });
    if let Ok(r) = cxt.ask_number(u64::from(min.heads), 1, u64::from(max.heads), b"Heads") {
        cxt.geom.heads = u64_as_u32(r);
    }
    if let Ok(r) = cxt.ask_number(min.sectors, 1, max.sectors, b"Sectors/track") {
        cxt.geom.sectors = r;
    }
    if let Ok(r) = cxt.ask_number(min.cylinders, 1, max.cylinders, b"Cylinders") {
        cxt.geom.cylinders = r;
    }
}

/// `sun_create_disklabel (cxt)`: a new label, with -- unless a script is
/// being followed -- upstream's default layout: Linux native, 50 MiB of
/// swap (or a third of the disk on a small one), and the whole disk as
/// partition 3.
fn sun_create_disklabel(cxt: &mut Context) -> i32 {
    let rc = cxt.init_firstsector_buffer(0, 0);
    if rc != 0 {
        return rc;
    }
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_max = MAXPARTITIONS;
    }
    cxt.set_firstsector_be(off::MAGIC, 2, LABEL_MAGIC);
    cxt.set_firstsector_be(off::VERSION, 4, VTOC_VERSION);
    cxt.set_firstsector_be(off::SANITY, 4, VTOC_SANITY);
    cxt.set_firstsector_be(off::NPARTS, 2, 8);

    if cxt.geom.heads != 0 && cxt.geom.sectors != 0 {
        match cxt.dev.as_ref().map(|d| ulblkid::blkdev::get_sectors(d)) {
            Some(Ok(llsectors)) => {
                let sec_fac = cxt.sector_size / 512;
                cxt.geom.cylinders = div(llsectors, cylsize(cxt).wrapping_mul(sec_fac));
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
    } else {
        ask_geom(cxt);
    }

    let g = cxt.geom;
    cxt.set_firstsector_be(off::ACYL, 2, 0);
    cxt.set_firstsector_be(off::PCYL, 2, u64_as_u32(g.cylinders));
    cxt.set_firstsector_be(off::RPM, 2, 5400);
    cxt.set_firstsector_be(off::INTRLV, 2, 1);
    cxt.set_firstsector_be(off::APC, 2, 0);
    cxt.set_firstsector_be(off::NHEAD, 2, g.heads);
    cxt.set_firstsector_be(off::NSECT, 2, u64_as_u32(g.sectors));
    cxt.set_firstsector_be(off::NCYL, 2, u64_as_u32(g.cylinders));

    // snprintf into the 128-byte field: at most 127 bytes and a NUL.
    let id = format!(
        "Linux cyl {} alt {} hd {} sec {}",
        g.cylinders, 0, g.heads, g.sectors
    );
    cxt.with_firstsector(|b| {
        if let Some(f) = b.get_mut(off::LABEL_ID..off::VERSION) {
            f.fill(0);
            let n = id.len().min(127);
            if let (Some(d), Some(s)) = (f.get_mut(..n), id.as_bytes().get(..n)) {
                d.copy_from_slice(s);
            }
        }
    });

    let hs = cylsize(cxt);
    let ndiv: u32 = if g.cylinders.wrapping_mul(hs) >= 150 * 2048 {
        // 50M of swap.
        u64_as_u32(g.cylinders.wrapping_sub(div(50 * 2048, hs)))
    } else {
        u64_as_u32(g.cylinders.wrapping_mul(2) / 3)
    };

    // The default layout only when no script is followed.
    if cxt.script.is_none() {
        let split = u64::from(ndiv).wrapping_mul(hs);
        let whole = g.cylinders.wrapping_mul(hs);
        set_partition(cxt, 0, 0, split, TAG_LINUX_NATIVE);
        set_partition(cxt, 1, split, whole, TAG_LINUX_SWAP);
        let flags = cxt.firstsector_be(info_flags_at(1), 2);
        cxt.set_firstsector_be(info_flags_at(1), 2, flags | UNMNT_BIT);
        set_partition(cxt, 2, 0, whole, TAG_WHOLEDISK);
    }

    update_checksum(cxt);
    set_changed(cxt);
    let n = count_used_partitions(cxt);
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_cur = n;
    }
    cxt.info(b"Created a new Sun disklabel.");
    0
}

/// `sun_toggle_partition_flag (cxt, i, flag)`: [`FLAG_UNMNT`] or
/// [`FLAG_RONLY`].
fn sun_toggle_partition_flag(cxt: &mut Context, i: usize, flag: u64) -> i32 {
    if i >= nmax(cxt) {
        return -EINVAL;
    }
    let bit = match flag {
        FLAG_UNMNT => UNMNT_BIT,
        FLAG_RONLY => RONLY_BIT,
        _ => return 1,
    };
    let flags = cxt.firstsector_be(info_flags_at(i), 2);
    cxt.set_firstsector_be(info_flags_at(i), 2, flags ^ bit);
    set_changed(cxt);
    0
}

/// What `fetch_sun` finds: each partition's start and length in sectors
/// (zero for an unused, unassigned or whole-disk one), and the free range
/// left after the partitions packed from the start of the disk.
struct Fetched {
    starts: [u32; MAXPARTITIONS],
    lens: [u32; MAXPARTITIONS],
    start: u32,
    stop: u32,
}

/// `fetch_sun (cxt, starts, lens, &start, &stop)`.
fn fetch_sun(cxt: &Context) -> Fetched {
    // An `int` in C; every use below is `unsigned`.
    let spc = u64_as_u32(cylsize(cxt));
    let mut f = Fetched {
        starts: [0; MAXPARTITIONS],
        lens: [0; MAXPARTITIONS],
        start: 0,
        stop: u64_as_u32(cxt.geom.cylinders).wrapping_mul(spc),
    };
    let mut continuous = true;
    for i in 0..nmax(cxt).min(MAXPARTITIONS) {
        let nsec = cxt.firstsector_be(nsect_at(i), 4);
        let id = cxt.firstsector_be(info_id_at(i), 2);
        let (s, l) = if nsec != 0 && id != TAG_UNASSIGNED && id != TAG_WHOLEDISK {
            (
                cxt.firstsector_be(start_cyl_at(i), 4).wrapping_mul(spc),
                nsec,
            )
        } else {
            (0, 0)
        };
        if let (Some(fs), Some(fl)) = (f.starts.get_mut(i), f.lens.get_mut(i)) {
            *fs = s;
            *fl = l;
        }
        if l == 0 || !continuous {
            continue;
        }
        if s == f.start {
            f.start = f.start.wrapping_add(l);
            let remained = u64_as_u32(rem(u64::from(f.start), u64::from(spc)));
            if remained != 0 {
                f.start = f.start.wrapping_add(spc.wrapping_sub(remained));
            }
        } else if s.wrapping_add(l) >= f.stop {
            f.stop = s;
        } else {
            // There are probably more gaps than one: checked afterwards.
            continuous = false;
        }
    }
    f
}

/// glibc's `qsort_r`, which `sun_verify_disklabel` sorts with: in glibc
/// 2.39 a top-down merge sort (`msort_with_tmp`) that takes from the left
/// run unless the comparison is positive. Upstream's comparison is not a
/// consistent order -- it never returns 0 -- so the result depends on
/// exactly this algorithm.
fn glibc_qsort_r(v: &mut [i32], cmp: &dyn Fn(i32, i32) -> i32) {
    let n = v.len();
    if n <= 1 {
        return;
    }
    let n1 = n / 2;
    {
        let (b1, b2) = v.split_at_mut(n1);
        glibc_qsort_r(b1, cmp);
        glibc_qsort_r(b2, cmp);
    }
    let mut tmp = Vec::with_capacity(n);
    let (mut i, mut j) = (0, n1);
    while let (true, Some(&a), Some(&b)) = (i < n1, v.get(i), v.get(j)) {
        if cmp(a, b) <= 0 {
            tmp.push(a);
            i = i.wrapping_add(1);
        } else {
            tmp.push(b);
            j = j.wrapping_add(1);
        }
    }
    tmp.extend_from_slice(v.get(i..n1).unwrap_or(&[]));
    if let Some(d) = v.get_mut(..tmp.len()) {
        d.copy_from_slice(&tmp);
    }
}

/// `sun_verify_disklabel (cxt)`: warns of partitions that overlap or do
/// not end on a cylinder boundary, and of the gaps between them.
fn sun_verify_disklabel(cxt: &mut Context) -> i32 {
    let Fetched {
        mut starts,
        mut lens,
        ..
    } = fetch_sun(cxt);
    let hs = cylsize(cxt);
    let s = |a: &[u32; MAXPARTITIONS], i: usize| a.get(i).copied().unwrap_or(0);

    for k in 0..7 {
        for i in 0..MAXPARTITIONS {
            let num = i.wrapping_add(1);
            if k != 0 && rem(u64::from(s(&lens, i)), hs) != 0 {
                cxt.warnx(format!("Partition {num} doesn't end on cylinder boundary.").as_bytes());
            }
            if s(&lens, i) == 0 {
                continue;
            }
            for j in 0..i {
                let (si, li, sj, lj) = (s(&starts, i), s(&lens, i), s(&starts, j), s(&lens, j));
                if lj == 0 {
                    continue;
                }
                if sj == si.wrapping_add(li) {
                    if let (Some(a), Some(b)) = (starts.get_mut(j), lens.get_mut(j)) {
                        *a = si;
                        *b = lj.wrapping_add(li);
                    }
                    if let Some(b) = lens.get_mut(i) {
                        *b = 0;
                    }
                } else if si == sj.wrapping_add(lj) {
                    if let Some(b) = lens.get_mut(j) {
                        *b = lj.wrapping_add(li);
                    }
                    if let Some(b) = lens.get_mut(i) {
                        *b = 0;
                    }
                } else if k == 0 && si < sj.wrapping_add(lj) && sj < si.wrapping_add(li) {
                    let starto = si.max(sj);
                    let endo = si.wrapping_add(li).min(sj.wrapping_add(lj));
                    cxt.warnx(
                        format!("Partition {num} overlaps with others in sectors {starto}-{endo}.")
                            .as_bytes(),
                    );
                }
            }
        }
    }

    let mut array = [-1i32; MAXPARTITIONS];
    for (i, a) in array.iter_mut().enumerate() {
        if s(&lens, i) != 0 {
            *a = i32::try_from(i).unwrap_or(-1);
        }
    }
    let at = |x: i32| usize::try_from(x).unwrap_or(usize::MAX);
    glibc_qsort_r(&mut array, &|a, b| {
        if a == -1 {
            1
        } else if b == -1 || s(&starts, at(a)) <= s(&starts, at(b)) {
            -1
        } else {
            1
        }
    });

    let first = array.first().copied().unwrap_or(-1);
    if first == -1 {
        cxt.info(b"No partitions defined.");
        return 0;
    }
    let stop = disk_sectors(cxt);
    let s0 = s(&starts, at(first));
    if s0 != 0 {
        cxt.warnx(format!("Unused gap - sectors 0-{s0}.").as_bytes());
    }
    let ent = |k: usize| array.get(k).copied().unwrap_or(-1);
    let end_of = |x: i32| s(&starts, at(x)).wrapping_add(s(&lens, at(x)));
    let mut i: usize = 0;
    while i < 7 && ent(i.wrapping_add(1)) != -1 {
        let (from, to) = (end_of(ent(i)), s(&starts, at(ent(i.wrapping_add(1)))));
        cxt.warnx(format!("Unused gap - sectors {from}-{to}.").as_bytes());
        i = i.wrapping_add(1);
    }
    let start = end_of(ent(i));
    if start < stop {
        cxt.warnx(format!("Unused gap - sectors {start}-{stop}.").as_bytes());
    }
    0
}

/// `is_free_sector (cxt, s, starts, lens)`.
fn is_free_sector(cxt: &Context, s: u64, f: &Fetched) -> bool {
    !(0..nmax(cxt)).any(|i| {
        let (st, l) = (
            f.starts.get(i).copied().unwrap_or(0),
            f.lens.get(i).copied().unwrap_or(0),
        );
        l != 0 && u64::from(st) <= s && u64::from(st.wrapping_add(l)) > s
    })
}

/// The number query `sun_add_partition` puts for the first sector.
fn ask_first(cxt: &mut Context, low: u64, dflt: u64, high: u64) -> Result<u32, i32> {
    let mut ask = crate::Ask::new(crate::AskType::Number);
    ask.set_query(format!("First {}", cxt.get_unit(SINGULAR)).as_bytes());
    ask.number_set_low(low);
    ask.number_set_default(dflt);
    ask.number_set_high(high);
    let rc = cxt.do_ask(&mut ask);
    if rc != 0 {
        return Err(rc);
    }
    Ok(u64_as_u32(ask.number_get_result()))
}

/// `sun_add_partition (cxt, pa, &partno)`.
#[allow(
    clippy::too_many_lines,
    reason = "one C function, ported as one to keep it comparable"
)]
fn sun_add_partition(
    cxt: &mut Context,
    pa: Option<&mut Partition>,
    partno: Option<&mut usize>,
) -> i32 {
    let pa = pa.map(|p| &*p);
    let mut sys = pa
        .and_then(|p| p.ty.as_ref())
        .map_or(TAG_LINUX_NATIVE, |t| t.code);
    let n = match cxt.partition_next_partno(pa) {
        Ok(n) => n,
        Err(rc) => return rc,
    };
    if cxt.firstsector_be(nsect_at(n), 4) != 0
        && cxt.firstsector_be(info_id_at(n), 2) != TAG_UNASSIGNED
    {
        cxt.info(
            format!(
                "Partition {} is already defined.  Delete it before re-adding it.",
                n.wrapping_add(1)
            )
            .as_bytes(),
        );
        return -EINVAL;
    }
    let f = fetch_sun(cxt);
    let mut whole_disk = pa
        .and_then(|p| p.ty.as_ref())
        .is_some_and(|t| t.code == TAG_WHOLEDISK);
    if f.stop <= f.start {
        if n == 2 {
            whole_disk = true;
        } else {
            cxt.info(b"Other partitions already cover the whole disk. Delete some/shrink them before retry.");
            return -EINVAL;
        }
    }

    let mut first: u32;
    if pa.is_some_and(|p| p.start_follow_default) {
        first = f.start;
    } else if let Some(p) = pa.filter(|p| p.has_start()) {
        first = u64_as_u32(p.start);
        if !whole_disk && !is_free_sector(cxt, u64::from(first), &f) {
            return -ERANGE;
        }
    } else {
        if n == 2 {
            cxt.info(
                b"It is highly recommended that the third partition covers the whole disk and is of type `Whole \
                  disk'",
            );
        }
        loop {
            let (low, dflt, high) = if whole_disk {
                (0, 0, 0)
            } else if n == 2 {
                (0, 0, cxt.scround(u64::from(f.stop)))
            } else {
                let s = cxt.scround(u64::from(f.start));
                (s, s, cxt.scround(u64::from(f.stop)))
            };
            first = match ask_first(cxt, low, dflt, high) {
                Ok(v) => v,
                Err(rc) => return rc,
            };
            if cxt.use_cylinders() {
                first = first.wrapping_mul(cxt.get_units_per_sector());
            } else {
                // The first sector aligned to a cylinder.
                let cs = u64_as_u32(cylsize(cxt));
                let x = u64_as_u32(rem(u64::from(first), u64::from(cs)));
                if x != 0 {
                    let to = first.wrapping_add(cs).wrapping_sub(x);
                    cxt.info(format!("Aligning the first sector from {first} to {to} to be on cylinder boundary.").as_bytes());
                    first = to;
                }
            }
            // Upstream keeps the original fdisk's notes here: a Sun disk
            // boots from a first partition at cylinder 0, which holds the
            // label and the boot block, so it must not be Linux swap; and
            // a partition at block 0 in an md array trashes the label.
            if is_free_sector(cxt, u64::from(first), &f) || whole_disk {
                break;
            }
            if n == 2 && first == 0 {
                whole_disk = true;
                break;
            }
            cxt.warnx(format!("Sector {} is already allocated", as_int(first)).as_bytes());
        }
    }

    // "ancient"
    let stop2 = disk_sectors(cxt);
    let mut stop = stop2;
    for i in 0..nmax(cxt) {
        let st = f.starts.get(i).copied().unwrap_or(0);
        if st > first && st < stop {
            stop = st;
        }
    }

    let mut last: u32;
    if pa.is_some_and(|p| p.end_follow_default) {
        last = if whole_disk || (n == 2 && first == 0) {
            stop2
        } else {
            stop
        };
    } else if let Some(p) = pa.filter(|p| p.has_size()) {
        last = u64_as_u32(u64::from(first).wrapping_add(p.size));
        if !whole_disk && last > stop {
            return -ERANGE;
        }
    } else {
        let mut ask = crate::Ask::new(crate::AskType::Offset);
        ask.set_query(
            format!(
                "Last {} or +/-{} or +/-size{{K,M,G,T,P}}",
                cxt.get_unit(SINGULAR),
                cxt.get_unit(PLURAL)
            )
            .as_bytes(),
        );
        let (sf, ss, ss2) = (
            cxt.scround(u64::from(first)),
            cxt.scround(u64::from(stop)),
            cxt.scround(u64::from(stop2)),
        );
        let (low, dflt, high, base) = if whole_disk {
            (ss2, ss2, ss2, 0)
        } else if n == 2 && first == 0 {
            (sf, ss2, ss2, sf)
        } else {
            (sf, ss, ss, sf)
        };
        ask.number_set_low(low);
        ask.number_set_default(dflt);
        ask.number_set_high(high);
        ask.number_set_base(base);
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
        last = u64_as_u32(ask.number_get_result());
        if rc != 0 {
            return rc;
        }
        if cxt.use_cylinders() {
            last = last.wrapping_mul(cxt.get_units_per_sector());
        }
    }

    if n == 2 && first == 0 {
        if last >= stop2 {
            whole_disk = true;
            last = stop2;
        } else if last > stop {
            let unit = cxt.get_unit(SINGULAR);
            let (l, s) = (cxt.scround(u64::from(last)), cxt.scround(u64::from(stop)));
            cxt.warnx(
                format!(
                    "You haven't covered the whole disk with the 3rd partition, but your value\n{l} {unit} covers \
                     some other partition. Your entry has been changed\nto {s} {unit}"
                )
                .as_bytes(),
            );
            last = stop;
        }
    } else if !whole_disk && last > stop {
        last = stop;
    }
    if whole_disk {
        sys = TAG_WHOLEDISK;
    }
    set_partition(cxt, n, u64::from(first), u64::from(last), sys);
    let used = count_used_partitions(cxt);
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_cur = used;
    }
    if let Some(partno) = partno {
        *partno = n;
    }
    0
}

/// `sun_delete_partition (cxt, partnum)`.
fn sun_delete_partition(cxt: &mut Context, partnum: usize) -> i32 {
    if partnum == 2
        && cxt.firstsector_be(info_id_at(partnum), 2) == TAG_WHOLEDISK
        && cxt.firstsector_be(start_cyl_at(partnum), 4) == 0
    {
        let nsec = cxt.firstsector_be(nsect_at(partnum), 4);
        if u64::from(nsec) == cylsize(cxt).wrapping_mul(cxt.geom.cylinders) {
            cxt.info(
                format!(
                    "If you want to maintain SunOS/Solaris compatibility, consider leaving this partition as Whole \
                     disk (5), starting at 0, with {nsec} sectors"
                )
                .as_bytes(),
            );
        }
    }
    cxt.set_firstsector_be(info_id_at(partnum), 2, TAG_UNASSIGNED);
    cxt.set_firstsector_be(nsect_at(partnum), 4, 0);
    let n = count_used_partitions(cxt);
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_cur = n;
    }
    set_changed(cxt);
    0
}

/// `strndup (field, sizeof field)`, or `None` for an empty field.
fn field_str(cxt: &Context, at: usize, len: usize) -> ItemData {
    let s = cxt
        .with_firstsector(|b| c_str(b.get(at..at.wrapping_add(len)).unwrap_or(&[])).to_vec())
        .unwrap_or_default();
    if s.is_empty() {
        ItemData::None
    } else {
        ItemData::Str(s)
    }
}

/// `sun_get_disklabel_item (cxt, item)`.
fn sun_get_disklabel_item(cxt: &mut Context, it: &mut LabelItem) -> i32 {
    let num = |at: usize| ItemData::U64(u64::from(cxt.firstsector_be(at, 2)));
    let (name, data) = match it.id {
        item::LABELID => ("Label ID", field_str(cxt, off::LABEL_ID, 128)),
        item::VTOCID => ("Volume ID", field_str(cxt, off::VOLUME_ID, 8)),
        item::RPM => ("Rpm", num(off::RPM)),
        item::ACYL => ("Alternate cylinders", num(off::ACYL)),
        item::PCYL => ("Physical cylinders", num(off::PCYL)),
        item::APC => ("Extra sects/cyl", num(off::APC)),
        item::INTRLV => ("Interleave", num(off::INTRLV)),
        id => return if id < NLABELITEMS { 1 } else { 2 },
    };
    it.name = Some(name);
    it.data = data;
    0
}

/// `sun_get_parttype (cxt, n)`.
fn get_parttype(cxt: &Context, n: usize) -> Option<PartType> {
    if n >= nmax(cxt) {
        return None;
    }
    let id = cxt.firstsector_be(info_id_at(n), 2);
    Some(
        cxt.label_ref()
            .and_then(|lb| lb.get_parttype_from_code(id))
            .cloned()
            .unwrap_or_else(|| PartType::new_unknown(id, None)),
    )
}

/// `sun_get_partition (cxt, n, pa)`.
fn sun_get_partition(cxt: &mut Context, n: usize, pa: &mut Partition) -> i32 {
    if n >= nmax(cxt) {
        return -EINVAL;
    }
    let nsec = cxt.firstsector_be(nsect_at(n), 4);
    pa.used = nsec != 0;
    if !pa.used {
        return 0;
    }
    let flags = u64::from(cxt.firstsector_be(info_flags_at(n), 2));
    let start = u64::from(cxt.firstsector_be(start_cyl_at(n), 4)).wrapping_mul(cylsize(cxt));
    pa.ty = get_parttype(cxt, n);
    if pa.ty.as_ref().is_some_and(|t| t.code == TAG_WHOLEDISK) {
        pa.wholedisk = true;
    }
    if flags & (FLAG_UNMNT | FLAG_RONLY) != 0 {
        pa.attrs = Some(vec![
            if flags & FLAG_UNMNT != 0 { b'u' } else { b' ' },
            if flags & FLAG_RONLY != 0 { b'r' } else { b' ' },
        ]);
    }
    pa.start = start;
    pa.size = u64::from(nsec);
    0
}

/// A label field set from a number question, as the `fdisk_sun_set_*`
/// functions set it.
fn ask_field(cxt: &mut Context, at: usize, low: u64, high: u64, query: &[u8]) -> i32 {
    let dflt = u64::from(cxt.firstsector_be(at, 2));
    match cxt.ask_number(low, dflt, high, query) {
        Ok(res) => {
            cxt.set_firstsector_be(at, 2, u64_as_u32(res));
            0
        }
        Err(rc) => rc,
    }
}

/// `fdisk_sun_set_alt_cyl (cxt)`: asks for the number of alternate
/// cylinders.
pub fn set_alt_cyl(cxt: &mut Context) -> i32 {
    ask_field(cxt, off::ACYL, 0, 65535, b"Number of alternate cylinders")
}

/// `fdisk_sun_set_xcyl (cxt)`: asks for the extra sectors per cylinder.
pub fn set_xcyl(cxt: &mut Context) -> i32 {
    let high = cxt.geom.sectors;
    ask_field(cxt, off::APC, 0, high, b"Extra sectors per cylinder")
}

/// `fdisk_sun_set_ilfact (cxt)`: asks for the interleave factor.
pub fn set_ilfact(cxt: &mut Context) -> i32 {
    ask_field(cxt, off::INTRLV, 1, 32, b"Interleave factor")
}

/// `fdisk_sun_set_rspeed (cxt)`: asks for the rotation speed.
pub fn set_rspeed(cxt: &mut Context) -> i32 {
    ask_field(cxt, off::RPM, 1, 65535, b"Rotation speed (rpm)")
}

/// `fdisk_sun_set_pcylcount (cxt)`: asks for the number of physical
/// cylinders -- and, as upstream does, never stores the answer: its test
/// of the question's result is inverted, so an answer returns at once,
/// and a failed question stores an uninitialised variable, which this
/// port does not reproduce. Either way the label is unchanged and the
/// result is 0.
pub fn set_pcylcount(cxt: &mut Context) -> i32 {
    let dflt = u64::from(cxt.firstsector_be(off::PCYL, 2));
    // Upstream discards the result in both branches (above).
    let _ = cxt.ask_number(0, dflt, 65535, b"Number of physical cylinders");
    0
}

/// `sun_write_disklabel (cxt)`.
fn sun_write_disklabel(cxt: &mut Context) -> i32 {
    // The geometry may have changed.
    let g = cxt.geom;
    cxt.set_firstsector_be(off::NHEAD, 2, g.heads);
    cxt.set_firstsector_be(off::NSECT, 2, u64_as_u32(g.sectors));
    if g.cylinders != u64::from(cxt.firstsector_be(off::NCYL, 2)) {
        // Upstream subtracts the native alternate-cylinder count from the
        // byte-swapped cylinder count and stores the difference without
        // swapping it back: right only while there are no alternate
        // cylinders. Reproduced as a little-endian host computes it.
        let a = i32::from(
            u16::try_from(g.cylinders & 0xffff)
                .unwrap_or(0)
                .swap_bytes(),
        );
        let b = i32::try_from(cxt.firstsector_be(off::ACYL, 2)).unwrap_or(0);
        let raw = u16::try_from(a.wrapping_sub(b) & 0xffff).unwrap_or(0);
        cxt.with_firstsector(|buf| {
            if let Some(d) = buf.get_mut(off::NCYL..off::NCYL + 2) {
                d.copy_from_slice(&raw.to_le_bytes());
            }
        });
    }
    update_checksum(cxt);
    let fs = cxt.firstsector();
    cxt.write_to_device(fs.get(..off::SIZE).unwrap_or(&fs), 0)
}

/// `sun_set_partition (cxt, i, pa)`.
fn sun_set_partition(cxt: &mut Context, i: usize, pa: &Partition) -> i32 {
    if i >= nmax(cxt) {
        return -EINVAL;
    }
    if let Some(t) = pa.ty.as_ref() {
        if t.code > u32::from(u16::MAX) {
            return -EINVAL;
        }
        if i == 2 && t.code != TAG_WHOLEDISK {
            cxt.info(b"Consider leaving partition 3 as Whole disk (5),\nas SunOS/Solaris expects it and even Linux likes it.\n");
        }
        if cxt.script.is_none()
            && t.code == TAG_LINUX_SWAP
            && cxt.firstsector_be(start_cyl_at(i), 4) == 0
        {
            match cxt.ask_yesno(
                b"It is highly recommended that the partition at offset 0\nis UFS, EXT2FS filesystem or SunOS swap. \
                  Putting Linux swap\nthere may destroy your partition table and bootblock.\nAre you sure you want \
                  to tag the partition as Linux swap?",
            ) {
                Err(rc) => return rc,
                Ok(false) => return 1,
                Ok(true) => {}
            }
        }
        let flags = cxt.firstsector_be(info_flags_at(i), 2);
        // Swap is not mountable by default; anything else is assumed to
        // be, and the user can change it.
        let flags = if t.code == TAG_SWAP || t.code == TAG_LINUX_SWAP {
            flags | UNMNT_BIT
        } else {
            flags & !UNMNT_BIT
        };
        cxt.set_firstsector_be(info_flags_at(i), 2, flags);
        cxt.set_firstsector_be(info_id_at(i), 2, t.code);
    }
    if pa.has_start() {
        cxt.set_firstsector_be(start_cyl_at(i), 4, u64_as_u32(div(pa.start, cylsize(cxt))));
    }
    if pa.has_size() {
        cxt.set_firstsector_be(nsect_at(i), 4, u64_as_u32(pa.size));
    }
    set_changed(cxt);
    0
}

/// `sun_reset_alignment (cxt)`: partitions may start at sector 0.
fn sun_reset_alignment(cxt: &mut Context) -> i32 {
    cxt.set_first_lba(0);
    0
}

/// `sun_partition_is_used (cxt, i)`.
fn sun_partition_is_used(cxt: &mut Context, i: usize) -> i32 {
    if i >= nmax(cxt) {
        return 0;
    }
    i32::from(cxt.firstsector_be(nsect_at(i), 4) != 0)
}

/// `sun_fields`.
static SUN_FIELDS: [Field; 9] = [
    Field::new(fieldtype::DEVICE, "Device", 10.0, 0),
    Field::new(fieldtype::START, "Start", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::END, "End", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::SECTORS, "Sectors", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::CYLINDERS, "Cylinders", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::SIZE, "Size", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::TYPEID, "Id", 2.0, FIELDFL_NUMBER),
    Field::new(fieldtype::TYPE, "Type", 0.1, 0),
    Field::new(fieldtype::ATTR, "Flags", 0.0, FIELDFL_NUMBER),
];

/// `sun_operations`.
static SUN_OPS: LabelOps = LabelOps {
    probe: Some(sun_probe_label),
    write: Some(sun_write_disklabel),
    verify: Some(sun_verify_disklabel),
    create: Some(sun_create_disklabel),
    locate: None,
    reorder: None,
    get_item: Some(sun_get_disklabel_item),
    set_id: None,
    add_part: Some(sun_add_partition),
    del_part: Some(sun_delete_partition),
    get_part: Some(sun_get_partition),
    set_part: Some(sun_set_partition),
    part_is_used: Some(sun_partition_is_used),
    part_toggle_flag: Some(sun_toggle_partition_flag),
    reset_alignment: Some(sun_reset_alignment),
    deinit: None,
};

/// `fdisk_new_sun_label (cxt)`.
#[must_use]
pub fn new_label() -> Label {
    Label {
        name: "sun",
        id: LabelType::Sun,
        parttypes: &PARTTYPES,
        parttype_cuts: &[],
        nparts_max: 0,
        nparts_cur: 0,
        flags: LABEL_FL_REQUIRE_GEOMETRY,
        geom_min: Geometry {
            heads: 1,
            sectors: 1,
            cylinders: 1,
        },
        geom_max: Geometry {
            heads: 1024,
            sectors: 1024,
            cylinders: 65535,
        },
        changed: false,
        disabled: false,
        fields: &SUN_FIELDS,
        op: &SUN_OPS,
        data: LabelData::None,
    }
}
