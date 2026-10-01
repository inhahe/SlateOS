//! `vxfs.c`: Veritas VxFS, either byte order.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, Endianness, USAGE_FILESYSTEM};

/// `probe_vxfs`.
fn probe_vxfs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct vxfs_super_block: 44 bytes.
    let Some(vxs) = pr.get_sb_buffer(mag, 44) else {
        return pr.none_or_err();
    };
    if vxs.le32(0) == 0xa501_fcf5 {
        pr.sprintf_version(vxs.le32(4).to_string());
        pr.set_fsblocksize(vxs.le32(32));
        pr.set_block_size(vxs.le32(32));
        pr.set_fsendianness(Endianness::Little);
    } else if vxs.be32(0) == 0xa501_fcf5 {
        pr.sprintf_version(vxs.be32(4).to_string());
        pr.set_fsblocksize(vxs.be32(32));
        pr.set_block_size(vxs.be32(32));
        pr.set_fsendianness(Endianness::Big);
    }
    0
}

/// `vxfs_idinfo`.
pub static VXFS: IdInfo = IdInfo {
    name: "vxfs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_vxfs),
    magics: &[
        IdMag::new(b"\xf5\xfc\x01\xa5", 1, 0),
        IdMag::new(b"\xa5\x01\xfc\xf5", 8, 0),
    ],
};
