//! `aix.c`: an AIX disk. The prober only recognises it -- upstream reads no
//! partitions -- and the DOS prober keeps away from it, since the AIX boot
//! record ends in 55AA too.

use super::need_typeonly;
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{PROBE_NONE, PROBE_OK};

/// `BLKID_AIX_MAGIC_STRING`: "IBMA" in EBCDIC.
pub(crate) const AIX_MAGIC: &[u8] = b"\xC9\xC2\xD4\xC1";

/// `probe_aix_pt`: an empty table.
fn probe_aix_pt(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    if need_typeonly(pr) {
        return PROBE_OK;
    }
    let Some(ls) = pr.partlist_mut() else {
        return PROBE_NONE;
    };
    ls.new_parttable("aix", 0);
    PROBE_OK
}

/// `aix_pt_idinfo`.
pub static AIX: IdInfo = IdInfo {
    name: "aix",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_aix_pt),
    magics: &[IdMag::new(AIX_MAGIC, 0, 0)],
};
