//! `gfs.c`: GFS and GFS2 cluster filesystems.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `sizeof(struct gfs2_sb)`.
const GFS2_SB: u64 = 272;

/// The label (the lock table) and the UUID both formats report.
fn set_ids(pr: &mut Probe, sbd: &[u8]) {
    if sbd.u8_at(160) != 0 {
        pr.set_label(sbd.span(160, 64));
    }
    pr.set_uuid(sbd.span(256, 16));
}

/// `probe_gfs`: GFS's own format numbers.
fn probe_gfs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(sbd) = pr.get_sb_buffer(mag, GFS2_SB) else {
        return pr.none_or_err();
    };
    if sbd.be32(24) == 1309 && sbd.be32(28) == 1401 {
        set_ids(pr, &sbd);
        return 0;
    }
    1
}

/// `probe_gfs2`: format 18xx, multihost format 19xx.
fn probe_gfs2(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(sbd) = pr.get_sb_buffer(mag, GFS2_SB) else {
        return pr.none_or_err();
    };
    if (1800..1900).contains(&sbd.be32(24)) && (1900..2000).contains(&sbd.be32(28)) {
        set_ids(pr, &sbd);
        pr.set_version(b"1");
        pr.set_fsblocksize(sbd.be32(36));
        pr.set_block_size(sbd.be32(36));
        return 0;
    }
    1
}

/// `gfs_idinfo`.
pub static GFS: IdInfo = IdInfo {
    name: "gfs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 32 * 1024 * 1024,
    probefunc: Some(probe_gfs),
    magics: &[IdMag::new(b"\x01\x16\x19\x70", 64, 0)],
};

/// `gfs2_idinfo`.
pub static GFS2: IdInfo = IdInfo {
    name: "gfs2",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 32 * 1024 * 1024,
    probefunc: Some(probe_gfs2),
    magics: &[IdMag::new(b"\x01\x16\x19\x70", 64, 0)],
};
