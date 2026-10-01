//! `exfs.c`: EXFS, a filesystem laid out as XFS is, under its own magic.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `sizeof(struct exfs_super_block)`.
const EXFS_SB_SIZE: u64 = 160;

/// `probe_exfs`.
fn probe_exfs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(xs) = pr.get_sb_buffer(mag, EXFS_SB_SIZE) else {
        return pr.none_or_err();
    };
    if !super::xfs::geometry_ok(&xs) {
        return 1;
    }
    if xs.u8_at(108) != 0 {
        pr.set_label(xs.span(108, 12));
    }
    pr.set_uuid(xs.span(32, 16));
    pr.set_fsblocksize(xs.be32(4));
    pr.set_block_size(xs.be32(4));
    0
}

/// `exfs_idinfo`.
pub static EXFS: IdInfo = IdInfo {
    name: "exfs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_exfs),
    magics: &[IdMag::new(b"EXFS", 0, 0)],
};
