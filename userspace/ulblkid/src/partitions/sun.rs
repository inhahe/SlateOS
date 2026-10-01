//! `sun.c`: the Sun disk label -- one 512-byte sector with eight
//! partitions, whose starts are in cylinders, and (where its VTOC is valid)
//! each partition's tag and flags.

use super::need_typeonly;
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, PROBE_NONE, PROBE_OK};

/// `SUN_VTOC_SANITY`.
const SUN_VTOC_SANITY: u32 = 0x600D_DEEE;
/// `SUN_VTOC_VERSION`.
const SUN_VTOC_VERSION: u32 = 1;
/// `SUN_MAXPARTITIONS`.
const SUN_MAXPARTITIONS: u16 = 8;
/// `SUN_TAG_WHOLEDISK`: the slice covering the whole disk.
const SUN_TAG_WHOLEDISK: u16 = 0x05;

/// `offsetof(struct sun_disklabel, vtoc.version)`.
const VTOC_VERSION: usize = 128;
/// `offsetof(struct sun_disklabel, vtoc.nparts)`.
const VTOC_NPARTS: usize = 140;
/// `offsetof(struct sun_disklabel, vtoc.infos)`.
const VTOC_INFOS: usize = 142;
/// `offsetof(struct sun_disklabel, vtoc.sanity)`.
const VTOC_SANITY: usize = 188;
/// `offsetof(struct sun_disklabel, nhead)`.
const NHEAD: usize = 436;
/// `offsetof(struct sun_disklabel, nsect)`.
const NSECT: usize = 438;
/// `offsetof(struct sun_disklabel, partitions)`.
const PARTITIONS: usize = 444;
/// `offsetof(struct sun_disklabel, magic)`.
const MAGIC: u32 = 508;

/// `sun_pt_checksum(label)`: the XOR of all the label's 16-bit words -- 0
/// for a valid label, whose checksum field makes it so.
fn sun_pt_checksum(l: &[u8]) -> u16 {
    let mut sum = 0u16;
    for k in (0..512).step_by(2) {
        sum ^= l.le16(k);
    }
    sum
}

/// `probe_sun_pt`.
fn probe_sun_pt(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(l) = pr.get_sector(0) else {
        return pr.none_or_err();
    };
    if !pr.verify_csum(u64::from(sun_pt_checksum(&l)), 0) {
        // A corrupted label.
        return PROBE_NONE;
    }
    if need_typeonly(pr) {
        return PROBE_OK;
    }
    let Some(ls) = pr.partlist_mut() else {
        return PROBE_NONE;
    };
    let tab = ls.new_parttable("sun", 0);
    // Sectors per cylinder: starts are in cylinders.
    let spc = u64::from(l.be16(NHEAD)).wrapping_mul(u64::from(l.be16(NSECT)));
    let sanity = l.be32(VTOC_SANITY);
    let version = l.be32(VTOC_VERSION);
    let vtoc_nparts = l.be16(VTOC_NPARTS);
    let valid_vtoc = sanity == SUN_VTOC_SANITY
        && version == SUN_VTOC_VERSION
        && vtoc_nparts <= SUN_MAXPARTITIONS;
    // Eight entries unless a valid VTOC says otherwise.
    let nparts = if valid_vtoc {
        vtoc_nparts
    } else {
        SUN_MAXPARTITIONS
    };
    // Old Linux-made labels have an all-zero VTOC; their tags are read too.
    let use_vtoc = valid_vtoc || (sanity == 0 && version == 0 && vtoc_nparts == 0);
    for i in 0..usize::from(nparts) {
        let p = PARTITIONS.saturating_add(i.saturating_mul(8));
        let start = u64::from(l.be32(p)).wrapping_mul(spc);
        let size = u64::from(l.be32(p.saturating_add(4)));
        let (ty, flags) = if use_vtoc {
            let info = VTOC_INFOS.saturating_add(i.saturating_mul(4));
            (l.be16(info), l.be16(info.saturating_add(2)))
        } else {
            (0, 0)
        };
        if ty == SUN_TAG_WHOLEDISK || size == 0 {
            ls.increment_partno();
            continue;
        }
        let par = ls.add_partition(tab, start, size);
        if let Some(pp) = ls.part_mut(par) {
            if ty != 0 {
                pp.set_type(i32::from(ty));
            }
            if flags != 0 {
                pp.set_flags(u64::from(flags));
            }
        }
    }
    PROBE_OK
}

/// `sun_pt_idinfo`: the big-endian magic 0xDABE near the end of the label.
pub static SUN: IdInfo = IdInfo {
    name: "sun",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_sun_pt),
    magics: &[IdMag::new(b"\xDA\xBE", 0, MAGIC)],
};
