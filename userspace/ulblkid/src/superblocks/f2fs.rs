//! `f2fs.c`: F2FS, its superblock checksum verified where it has one.

use crate::encode::ENCODE_UTF16LE;
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `sizeof(struct f2fs_super_block)` as libblkid declares it (to the end of
/// the volume name).
const F2FS_SB: u64 = 0x7C + 1024;

/// `f2fs_validate_checksum(pr, sb_off, sb)`: CRC-32 from F2FS's own seed
/// over the superblock up to the checksum, where the superblock says it
/// keeps one.
fn validate_checksum(pr: &mut Probe, sb_off: u64, sb: &[u8]) -> bool {
    let csum_off = sb.le32(0x20);
    if csum_off == 0 {
        return true;
    }
    if !csum_off.is_multiple_of(4) || u64::from(csum_off).wrapping_add(4) > 4096 {
        return false;
    }
    let Some(csum_data) = pr.get_buffer(sb_off.wrapping_add(u64::from(csum_off)), 4) else {
        return false;
    };
    let expected = csum_data.le32(0);
    let Some(csummed) = pr.get_buffer(sb_off, u64::from(csum_off)) else {
        return false;
    };
    let csum = crc32::crc32_raw(0xF2F5_2010, &csummed);
    pr.verify_csum(u64::from(csum), u64::from(expected))
}

/// `probe_f2fs`.
fn probe_f2fs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(sb) = pr.get_sb_buffer(mag, F2FS_SB) else {
        return pr.none_or_err();
    };
    let (vermaj, vermin) = (sb.le16(4), sb.le16(6));
    // Version 1.0's superblock layout cannot be known: the magic is all.
    if vermaj == 1 && vermin == 0 {
        return 0;
    }
    let sb_off = mag.map_or(0, |m| u64::try_from(m.kboff << 10).unwrap_or(0));
    if !validate_checksum(pr, sb_off, &sb) {
        return 1;
    }
    if sb.u8_at(0x7C) != 0 {
        pr.set_utf8label(sb.span(0x7C, 1024), ENCODE_UTF16LE);
    }
    pr.set_uuid(sb.span(0x6C, 16));
    pr.sprintf_version(format!("{vermaj}.{vermin}"));
    let log = sb.le32(0x10);
    if log < 32 {
        let blocksize = 1u32 << log;
        pr.set_fsblocksize(blocksize);
        pr.set_block_size(blocksize);
        pr.set_fssize(sb.le64(0x24).wrapping_mul(u64::from(blocksize)));
    }
    0
}

/// `f2fs_idinfo`.
pub static F2FS: IdInfo = IdInfo {
    name: "f2fs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_f2fs),
    magics: &[IdMag::new(b"\x10\x20\xF5\xF2", 1, 0)],
};
