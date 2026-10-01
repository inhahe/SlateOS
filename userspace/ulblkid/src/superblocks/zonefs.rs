//! `zonefs.c`: zonefs, on zoned block devices.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `ZONEFS_BLOCK_SIZE`.
const ZONEFS_BLOCK_SIZE: u32 = 4096;

/// `probe_zonefs`: CRC-32 of the 4 KiB superblock with its checksum field
/// read as zeros (`ul_crc32_exclude_offset`), no final inversion.
fn probe_zonefs(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(sb) = pr.get_buffer(0, 4096) else {
        return pr.none_or_err();
    };
    let expected = sb.le32(4);
    let mut crc = crc32::crc32_raw(!0, sb.span(0, 4));
    crc = crc32::crc32_raw(crc, &[0; 4]);
    crc = crc32::crc32_raw(crc, sb.span(8, 4096 - 8));
    if !pr.verify_csum(u64::from(crc), u64::from(expected)) {
        return 1;
    }
    if sb.u8_at(8) != 0 {
        pr.set_label(sb.span(8, 32));
    }
    pr.set_uuid(sb.span(40, 16));
    pr.set_fsblocksize(ZONEFS_BLOCK_SIZE);
    pr.set_block_size(ZONEFS_BLOCK_SIZE);
    0
}

/// `zonefs_idinfo`.
pub static ZONEFS: IdInfo = IdInfo {
    name: "zonefs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_zonefs),
    magics: &[IdMag::new(b"SFOZ", 0, 0)],
};
