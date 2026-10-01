//! `mpool.c`: mpool devices.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `offsetof(struct omf_sb_descriptor, osb_cksum1)`.
const CKSUM1: usize = 62;

/// `probe_mpool`: CRC-32C of the descriptor before its checksum.
fn probe_mpool(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct omf_sb_descriptor: 66 bytes.
    let Some(osd) = pr.get_sb_buffer(mag, 66) else {
        return pr.none_or_err();
    };
    let crc = crc32c::crc32c_raw(!0, osd.span(0, CKSUM1)) ^ !0;
    if !pr.verify_csum(u64::from(crc), u64::from(osd.le32(CKSUM1))) {
        return 1;
    }
    pr.set_label(osd.span(8, 32));
    pr.set_uuid(osd.span(40, 16));
    0
}

/// `mpool_idinfo`.
pub static MPOOL: IdInfo = IdInfo {
    name: "mpool",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_mpool),
    magics: &[IdMag::new(b"\x6D\x70\x6f\x6f\x6c\x44\x65\x76", 0, 0)],
};
