//! `erofs.c`: EROFS, its superblock checksum verified where the feature
//! says there is one.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, PROBE_NONE, PROBE_OK, USAGE_FILESYSTEM};

/// `EROFS_SUPER_OFFSET`.
const EROFS_SUPER_OFFSET: u32 = 1024;
/// `EROFS_FEATURE_SB_CSUM`.
const EROFS_FEATURE_SB_CSUM: u32 = 1 << 0;

/// `erofs_verify_checksum`: CRC-32C of the rest of the first block, the
/// checksum field read as zeros, no final inversion.
fn verify_checksum(pr: &mut Probe, mag: Option<&'static IdMag>, sb: &[u8]) -> bool {
    if sb.le32(8) & EROFS_FEATURE_SB_CSUM == 0 {
        return true;
    }
    let expected = sb.le32(4);
    let csummed_size = (1u32 << sb.u8_at(12)).wrapping_sub(EROFS_SUPER_OFFSET);
    let Some(csummed) = pr.get_sb_buffer(mag, u64::from(csummed_size)) else {
        return false;
    };
    let csum = crc32c::crc32c_raw_exclude(!0, &csummed, 4, 4);
    pr.verify_csum(u64::from(csum), u64::from(expected))
}

/// `probe_erofs`.
fn probe_erofs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct erofs_super_block: 128 bytes.
    let Some(sb) = pr.get_sb_buffer(mag, 128) else {
        return pr.none_or_err();
    };
    let bits = u32::from(sb.u8_at(12));
    // EROFS blocks are at most 4 KiB.
    if bits > 31 || (1u32 << bits) > 4096 {
        return PROBE_NONE;
    }
    if !verify_checksum(pr, mag, &sb) {
        return PROBE_NONE;
    }
    if sb.u8_at(64) != 0 {
        pr.set_label(sb.span(64, 16));
    }
    pr.set_uuid(sb.span(48, 16));
    pr.set_fsblocksize(1u32 << bits);
    pr.set_block_size(1u32 << bits);
    pr.set_fssize(u64::from(1u32 << bits).wrapping_mul(u64::from(sb.le32(36))));
    PROBE_OK
}

/// `erofs_idinfo`.
pub static EROFS: IdInfo = IdInfo {
    name: "erofs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_erofs),
    magics: &[IdMag::new(b"\xe2\xe1\xf5\xe0", 1, 0)],
};
