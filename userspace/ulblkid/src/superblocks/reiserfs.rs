//! `reiserfs.c`: ReiserFS (3.5, 3.6, with an external journal) and Reiser4.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `probe_reiser`.
fn probe_reiser(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(mag) = mag else {
        return 1;
    };
    // struct reiserfs_super_block: 116 bytes.
    let Some(rs) = pr.get_sb_buffer(Some(mag), 116) else {
        return pr.none_or_err();
    };
    let blocksize = u32::from(rs.le16(44));
    // At least 512 bytes.
    if blocksize >> 9 == 0 {
        return 1;
    }
    // A superblock inside the journal is the wrong one.
    // blocksize >> 9 is not 0: checked above.
    if mag
        .kboff
        .checked_div(i64::from(blocksize >> 9))
        .unwrap_or(0)
        > i64::from(rs.le32(12) / 2)
    {
        return 1;
    }
    let v = mag.magic.u8_at(6);
    // LABEL and UUID exist only in later 3.6 filesystems.
    if v == b'2' || v == b'3' {
        if rs.u8_at(100) != 0 {
            pr.set_label(rs.span(100, 16));
        }
        pr.set_uuid(rs.span(84, 16));
    }
    let version: &[u8] = match v {
        b'3' => b"JR",
        b'2' => b"3.6",
        _ => b"3.5",
    };
    pr.set_version(version);
    pr.set_fsblocksize(blocksize);
    pr.set_block_size(blocksize);
    0
}

/// `probe_reiser4`.
fn probe_reiser4(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct reiser4_super_block: 60 bytes.
    let Some(rs4) = pr.get_sb_buffer(mag, 60) else {
        return pr.none_or_err();
    };
    let blocksize = u32::from(rs4.u8_at(19)) * 256;
    if rs4.u8_at(36) != 0 {
        pr.set_label(rs4.span(36, 16));
    }
    pr.set_uuid(rs4.span(20, 16));
    pr.set_version(b"4");
    pr.set_fsblocksize(blocksize);
    pr.set_block_size(blocksize);
    0
}

/// `reiser_idinfo`.
pub static REISER: IdInfo = IdInfo {
    name: "reiserfs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 128 * 1024,
    probefunc: Some(probe_reiser),
    magics: &[
        IdMag::new(b"ReIsErFs", 8, 0x34),
        IdMag::new(b"ReIsEr2Fs", 64, 0x34),
        IdMag::new(b"ReIsEr3Fs", 64, 0x34),
        IdMag::new(b"ReIsErFs", 64, 0x34),
        IdMag::new(b"ReIsErFs", 8, 20),
    ],
};

/// `reiser4_idinfo`.
pub static REISER4: IdInfo = IdInfo {
    name: "reiser4",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 128 * 1024,
    probefunc: Some(probe_reiser4),
    magics: &[IdMag::new(b"ReIsEr4", 64, 0)],
};
