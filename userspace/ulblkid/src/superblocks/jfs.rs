//! `jfs.c`: IBM JFS.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `probe_jfs`: the block sizes agree with their logs.
fn probe_jfs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct jfs_super_block: 184 bytes (naturally aligned).
    let Some(js) = pr.get_sb_buffer(mag, 184) else {
        return pr.none_or_err();
    };
    let l2bsize = u32::from(js.le16(20));
    let l2pbsize = u32::from(js.le16(28));
    if l2bsize > 31 || l2pbsize > 31 {
        return 1;
    }
    if js.le32(16) != 1u32 << l2bsize {
        return 1;
    }
    if js.le32(24) != 1u32 << l2pbsize {
        return 1;
    }
    if i64::from(l2bsize).wrapping_sub(i64::from(l2pbsize)) != i64::from(js.le16(22)) {
        return 1;
    }
    if js.u8_at(152) != 0 {
        pr.set_label(js.span(152, 16));
    }
    pr.set_uuid(js.span(136, 16));
    pr.set_fsblocksize(js.le32(16));
    pr.set_block_size(js.le32(16));
    0
}

/// `jfs_idinfo`.
pub static JFS: IdInfo = IdInfo {
    name: "jfs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 16 * 1024 * 1024,
    probefunc: Some(probe_jfs),
    magics: &[IdMag::new(b"JFS1", 32, 0)],
};
