//! `squashfs.c`: SquashFS 4, and the older (3.x and before) format of
//! either byte order.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, Endianness, USAGE_FILESYSTEM};

/// `sizeof(struct sqsh_super_block)`.
const SQSH_SB: u64 = 96;

/// `probe_squashfs`: version 4 and later, little-endian.
fn probe_squashfs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(sq) = pr.get_sb_buffer(mag, SQSH_SB) else {
        return pr.none_or_err();
    };
    let (vermaj, vermin) = (sq.le16(28), sq.le16(30));
    if vermaj < 4 {
        return 1;
    }
    pr.sprintf_version(format!("{vermaj}.{vermin}"));
    pr.set_fsblocksize(sq.le32(12));
    pr.set_block_size(sq.le32(12));
    pr.set_fssize(sq.le64(40));
    0
}

/// `probe_squashfs3`: version 3 and before, "sqsh" big-endian.
fn probe_squashfs3(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(sq) = pr.get_sb_buffer(mag, SQSH_SB) else {
        return pr.none_or_err();
    };
    let big = mag.is_some_and(|m| m.magic == b"sqsh");
    let (vermaj, vermin, e) = if big {
        (sq.be16(28), sq.be16(30), Endianness::Big)
    } else {
        (sq.le16(28), sq.le16(30), Endianness::Little)
    };
    if vermaj > 3 {
        return 1;
    }
    pr.sprintf_version(format!("{vermaj}.{vermin}"));
    pr.set_fsblocksize(1024);
    pr.set_block_size(1024);
    pr.set_fsendianness(e);
    0
}

/// `squashfs_idinfo`.
pub static SQUASHFS: IdInfo = IdInfo {
    name: "squashfs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_squashfs),
    magics: &[IdMag::new(b"hsqs", 0, 0)],
};

/// `squashfs3_idinfo`.
pub static SQUASHFS3: IdInfo = IdInfo {
    name: "squashfs3",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_squashfs3),
    magics: &[IdMag::new(b"sqsh", 0, 0), IdMag::new(b"hsqs", 0, 0)],
};
