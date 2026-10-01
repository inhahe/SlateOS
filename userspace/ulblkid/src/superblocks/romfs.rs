//! `romfs.c`: romfs, its first-512-byte checksum verified.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `romfs_verify_csum`: the big-endian words of the first 512 bytes (or of
/// the whole image, if smaller) sum to zero.
fn verify_csum(pr: &mut Probe, mag: Option<&'static IdMag>, ros: &[u8]) -> bool {
    let csummed_size = ros.be32(8).min(512);
    if !csummed_size.is_multiple_of(4) {
        return false;
    }
    let Some(csummed) = pr.get_sb_buffer(mag, u64::from(csummed_size)) else {
        return false;
    };
    let csum = csummed
        .chunks(4)
        .fold(0u32, |c, w| c.wrapping_add(w.be32(0)));
    pr.verify_csum(u64::from(csum), 0)
}

/// `probe_romfs`.
fn probe_romfs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct romfs_super_block: magic[8], full_size, checksum, volume[16].
    let Some(ros) = pr.get_sb_buffer(mag, 32) else {
        return pr.none_or_err();
    };
    if !verify_csum(pr, mag, &ros) {
        return 1;
    }
    if ros.u8_at(16) != 0 {
        pr.set_label(ros.span(16, 16));
    }
    pr.set_fsblocksize(1024);
    pr.set_fssize(u64::from(ros.be32(8)));
    pr.set_block_size(1024);
    0
}

/// `romfs_idinfo`.
pub static ROMFS: IdInfo = IdInfo {
    name: "romfs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_romfs),
    magics: &[IdMag::new(b"-rom1fs-", 0, 0)],
};
