//! `lsi_raid.c`: LSI MegaRAID members, a signature in the last sector.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_RAID};

/// `LSI_SIGNATURE`.
const LSI_SIGNATURE: &[u8] = b"$XIDE$";

/// `probe_lsiraid`.
fn probe_lsiraid(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    if pr.size < 0x10000 {
        return 1;
    }
    if !pr.is_reg() && !pr.is_wholedisk() {
        return 1;
    }
    let off = (pr.size / 0x200).wrapping_sub(1).wrapping_mul(0x200);
    let Some(lsi) = pr.get_buffer(off, 6) else {
        return pr.none_or_err();
    };
    if lsi.span(0, 6) != LSI_SIGNATURE {
        return 1;
    }
    if pr.set_magic(off, lsi.span(0, 6)) != 0 {
        return 1;
    }
    0
}

/// `lsiraid_idinfo`.
pub static LSIRAID: IdInfo = IdInfo {
    name: "lsi_mega_raid_member",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_lsiraid),
    magics: &[],
};
