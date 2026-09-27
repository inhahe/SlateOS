//! `minix.c`: Minix subpartitions -- an MBR inside a DOS partition of type
//! 0x81. The parent's type is the only thing telling it from a DOS table,
//! so the prober answers only as a nested one.

use super::mbr::{self, MBR_MINIX_PARTITION, MBR_PT_OFFSET};
use super::{Partition, is_nested_dimension, need_typeonly};
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{PROBE_NONE, PROBE_OK};

/// `probe_minix_pt`.
fn probe_minix_pt(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(data) = pr.get_sector(0) else {
        return pr.none_or_err();
    };
    let typeonly = need_typeonly(pr);
    let Some(ls) = pr.partlist_mut() else {
        return PROBE_NONE;
    };
    // The parent is required.
    let parent: Option<Partition> = ls.parent().and_then(|id| ls.partition(id.0).cloned());
    let Some(parent) = parent else {
        return PROBE_NONE;
    };
    if parent.ty != i32::from(MBR_MINIX_PARTITION) {
        return PROBE_NONE;
    }
    if typeonly {
        return PROBE_OK;
    }
    let tab = ls.new_parttable("minix", MBR_PT_OFFSET);
    for p in &mbr::partitions(&data) {
        if p.sys_ind != MBR_MINIX_PARTITION {
            continue;
        }
        if !is_nested_dimension(&parent, u64::from(p.start), u64::from(p.size)) {
            // Reaches outside its parent: ignored.
            continue;
        }
        let id = ls.add_partition(tab, u64::from(p.start), u64::from(p.size));
        if let Some(pp) = ls.part_mut(id) {
            pp.set_type(i32::from(p.sys_ind));
            pp.set_flags(u64::from(p.boot_ind));
        }
    }
    PROBE_OK
}

/// `minix_pt_idinfo`: the same magic as DOS.
pub static MINIX: IdInfo = IdInfo {
    name: "minix",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_minix_pt),
    magics: &[IdMag::new(b"\x55\xAA", 0, 510)],
};
