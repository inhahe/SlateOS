//! `apfs.c`: Apple APFS containers.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, PROBE_NONE, PROBE_OK, USAGE_FILESYSTEM};

/// `APFS_STANDARD_BLOCK_SIZE`.
const APFS_STANDARD_BLOCK_SIZE: u32 = 4096;

/// `probe_apfs`: a container superblock object, with the standard block
/// size -- a strict test, against false positives.
fn probe_apfs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct apfs_super_block: 88 bytes.
    let Some(sb) = pr.get_sb_buffer(mag, 88) else {
        return pr.none_or_err();
    };
    if sb.le16(24) != 1 || sb.le16(28) != 0 || sb.le16(30) != 0 {
        return PROBE_NONE;
    }
    if sb.le32(36) != APFS_STANDARD_BLOCK_SIZE {
        return PROBE_NONE;
    }
    if pr.set_uuid(sb.span(72, 16)) < 0 {
        return PROBE_NONE;
    }
    pr.set_fsblocksize(sb.le32(36));
    pr.set_block_size(sb.le32(36));
    PROBE_OK
}

/// `apfs_idinfo`.
pub static APFS: IdInfo = IdInfo {
    name: "apfs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_apfs),
    magics: &[IdMag::new(b"NXSB", 0, 32)],
};
