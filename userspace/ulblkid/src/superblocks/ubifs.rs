//! `ubifs.c`: UBIFS, its superblock node's CRC verified.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `sizeof(struct ubifs_sb_node)`.
const SB_NODE: u64 = 4096;

/// `probe_ubifs`.
fn probe_ubifs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(sb) = pr.get_sb_buffer(mag, SB_NODE) else {
        return pr.none_or_err();
    };
    // `ubifs_verify_csum`: CRC-32 from the sequence number to the end, with
    // no final inversion.
    let crc = crc32::crc32_raw(!0, sb.span(8, 4096 - 8));
    if !pr.verify_csum(u64::from(crc), u64::from(sb.le32(4))) {
        return 1;
    }
    pr.set_uuid(sb.span(108, 16));
    let w = i32::from_ne_bytes(sb.le32(80).to_ne_bytes());
    let r = i32::from_ne_bytes(sb.le32(124).to_ne_bytes());
    pr.sprintf_version(format!("w{w}r{r}"));
    pr.set_fssize(u64::from(sb.le32(36)).wrapping_mul(u64::from(sb.le32(40))));
    0
}

/// `ubifs_idinfo`.
pub static UBIFS: IdInfo = IdInfo {
    name: "ubifs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_ubifs),
    magics: &[IdMag::new(b"\x31\x18\x10\x06", 0, 0)],
};
