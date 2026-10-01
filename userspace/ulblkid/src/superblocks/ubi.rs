//! `ubi.c`: UBI (unsorted block images) devices, by their erase-counter
//! header.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_RAID};

/// `probe_ubi`. A header that cannot be read, or whose CRC is wrong, is an
/// *error* (-1), not "nothing" -- as upstream returns it, ending the chain.
fn probe_ubi(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct ubi_ec_hdr: 64 bytes, hdr_crc last.
    let Some(hdr) = pr.get_sb_buffer(mag, 64) else {
        return -1;
    };
    let crc = crc32::crc32_raw(!0, hdr.span(0, 60));
    if !pr.verify_csum(u64::from(crc), u64::from(hdr.be32(60))) {
        return -1;
    }
    pr.sprintf_version(hdr.u8_at(4).to_string());
    pr.sprintf_uuid(hdr.span(24, 4), hdr.be32(24).to_string());
    0
}

/// `ubi_idinfo`.
pub static UBI: IdInfo = IdInfo {
    name: "ubi",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_ubi),
    magics: &[IdMag::new(b"UBI#", 0, 0)],
};
