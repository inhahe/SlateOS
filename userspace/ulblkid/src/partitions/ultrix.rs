//! `ultrix.c`: the ULTRIX disk label -- eight partitions in the last 72
//! bytes of sector 31, the label's end at 16 KiB. It has no magic string
//! for the search, so the prober always runs.

use super::need_typeonly;
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, ENOMEM, PROBE_NONE, PROBE_OK};

/// `ULTRIX_MAXPARTITIONS`.
const ULTRIX_MAXPARTITIONS: usize = 8;
/// `ULTRIX_MAGIC`.
const ULTRIX_MAGIC: i32 = 0x0003_2957;
/// `ULTRIX_MAGIC_STR`: the magic as upstream reports it, big-endian (the
/// label itself is little-endian).
const ULTRIX_MAGIC_STR: &[u8] = b"\x02\x29\x57";
/// `sizeof(struct ultrix_disklabel)`.
const LABEL_SIZE: usize = 72;
/// `ULTRIX_SECTOR`: the sector with the label, `(16384 - 72) >> 9`.
const ULTRIX_SECTOR: u32 = 31;
/// `ULTRIX_OFFSET`: where in that sector, `512 - 72`.
const ULTRIX_OFFSET: usize = 440;
/// `(ULTRIX_SECTOR << 9) + ULTRIX_OFFSET`: where the magic is.
const MAGIC_OFFSET: u64 = (31 << 9) + 440;

/// `probe_ultrix_pt`.
fn probe_ultrix_pt(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(data) = pr.get_sector(ULTRIX_SECTOR) else {
        return pr.none_or_err();
    };
    let l = data.span(ULTRIX_OFFSET, LABEL_SIZE);
    // pt_magic and pt_valid are signed, in the machine's order.
    if l.le32(0).cast_signed() != ULTRIX_MAGIC || l.le32(4).cast_signed() != 1 {
        return PROBE_NONE;
    }
    if pr.set_magic(MAGIC_OFFSET, ULTRIX_MAGIC_STR) != 0 {
        return -ENOMEM;
    }
    if need_typeonly(pr) {
        return PROBE_OK;
    }
    let Some(ls) = pr.partlist_mut() else {
        return PROBE_NONE;
    };
    let tab = ls.new_parttable("ultrix", 0);
    for i in 0..ULTRIX_MAXPARTITIONS {
        let p = i.saturating_mul(8).saturating_add(8);
        let nblocks = l.le32(p).cast_signed();
        let blkoff = l.le32(p.saturating_add(4));
        if nblocks == 0 {
            ls.increment_partno();
        } else {
            // A negative block count, widened as C widens an int32_t to a
            // uint64_t: sign-extended.
            ls.add_partition(tab, u64::from(blkoff), i64::from(nblocks).cast_unsigned());
        }
    }
    PROBE_OK
}

/// `ultrix_pt_idinfo`.
pub static ULTRIX: IdInfo = IdInfo {
    name: "ultrix",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_ultrix_pt),
    magics: &[],
};
