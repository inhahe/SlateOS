//! `sgi.c`: the SGI (IRIX) disk label -- one 512-byte sector, sixteen
//! partitions, a checksum that makes the sector's 32-bit words sum to 0.

use super::need_typeonly;
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, PROBE_NONE, PROBE_OK};

/// `SGI_MAXPARTITIONS`.
const SGI_MAXPARTITIONS: usize = 16;
/// `offsetof(struct sgi_disklabel, partitions)`.
const PARTITIONS: usize = 312;
/// `sizeof(struct sgi_partition)`.
const PARTITION_SIZE: usize = 12;
/// `sizeof(struct sgi_disklabel)`.
const LABEL_SIZE: usize = 512;

/// `sgi_pt_checksum(label)`: 0 minus every big-endian 32-bit word of the
/// label -- 0 for a valid one.
fn sgi_pt_checksum(l: &[u8]) -> u32 {
    let mut sum = 0u32;
    for k in (0..LABEL_SIZE).step_by(4).rev() {
        sum = sum.wrapping_sub(l.be32(k));
    }
    sum
}

/// `probe_sgi_pt`.
fn probe_sgi_pt(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(l) = pr.get_sector(0) else {
        return pr.none_or_err();
    };
    if !pr.verify_csum(u64::from(sgi_pt_checksum(&l)), 0) {
        // A corrupted label.
        return PROBE_NONE;
    }
    if need_typeonly(pr) {
        return PROBE_OK;
    }
    let Some(ls) = pr.partlist_mut() else {
        return PROBE_NONE;
    };
    let tab = ls.new_parttable("sgi", 0);
    for i in 0..SGI_MAXPARTITIONS {
        let p = PARTITIONS.saturating_add(i.saturating_mul(PARTITION_SIZE));
        let size = l.be32(p);
        let start = l.be32(p.saturating_add(4));
        // An unsigned type in an int, as C converts it.
        let ty = l.be32(p.saturating_add(8)).cast_signed();
        if size == 0 {
            ls.increment_partno();
            continue;
        }
        let par = ls.add_partition(tab, u64::from(start), u64::from(size));
        if let Some(pp) = ls.part_mut(par) {
            pp.set_type(ty);
        }
    }
    PROBE_OK
}

/// `sgi_pt_idinfo`.
pub static SGI: IdInfo = IdInfo {
    name: "sgi",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_sgi_pt),
    magics: &[IdMag::new(b"\x0B\xE5\xA9\x41", 0, 0)],
};
