//! `vdo.c`: VDO (Virtual Data Optimizer) volumes.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_OTHER};

/// `probe_vdo`.
fn probe_vdo(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct vdo_super_block: magic[8], unused[32], sb_uuid[16].
    let Some(vsb) = pr.get_sb_buffer(mag, 56) else {
        return pr.none_or_err();
    };
    pr.set_uuid(vsb.span(40, 16));
    0
}

/// `vdo_idinfo`.
pub static VDO: IdInfo = IdInfo {
    name: "vdo",
    usage: USAGE_OTHER,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_vdo),
    magics: &[IdMag::new(b"dmvdo001", 0, 0)],
};
