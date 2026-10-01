//! `nilfs.c`: NILFS2 -- the primary superblock at 1 KiB and the backup near
//! the end, whichever is valid and newer.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Buf, Bytes, USAGE_FILESYSTEM};

/// `NILFS_SB_MAGIC`.
const NILFS_SB_MAGIC: u16 = 0x3434;
/// `NILFS_SB_OFFSET`.
const NILFS_SB_OFFSET: u64 = 0x400;
/// `sizeof(struct nilfs_super_block)`.
const NILFS_SB: u64 = 1024;
/// `offsetof(struct nilfs_super_block, s_sum)`.
const SUMOFF: usize = 16;

/// `NILFS_SBB_OFFSET(size)`: the backup superblock's place.
fn sbb_offset(size: u64) -> u64 {
    (size / 0x200).wrapping_sub(8).wrapping_mul(0x200)
}

/// `nilfs_valid_sb(pr, sb, is_bak)`: the magic, a backup's device size on a
/// whole disk, and the CRC over `s_bytes` with the checksum field read as
/// zeros.
fn valid_sb(pr: &mut Probe, sb: &[u8], is_bak: bool) -> bool {
    if sb.le16(6) != NILFS_SB_MAGIC {
        return false;
    }
    if is_bak && pr.is_wholedisk() && sb.le64(32) != pr.size {
        return false;
    }
    let bytes = usize::from(sb.le16(8));
    let crc_start = SUMOFF + 4;
    if bytes < crc_start || bytes > 1024 {
        return false;
    }
    let mut crc = crc32::crc32_raw(sb.le32(12), sb.span(0, SUMOFF));
    crc = crc32::crc32_raw(crc, &[0; 4]);
    crc = crc32::crc32_raw(crc, sb.span(crc_start, bytes.wrapping_sub(crc_start)));
    pr.verify_csum(u64::from(crc), u64::from(sb.le32(SUMOFF)))
}

/// `probe_nilfs2`.
fn probe_nilfs2(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(sbp) = pr.get_buffer(NILFS_SB_OFFSET, NILFS_SB) else {
        return pr.none_or_err();
    };
    let valid0 = valid_sb(pr, &sbp, false);
    let backup_at = sbb_offset(pr.size);
    let sbb: Option<Buf> = pr.get_buffer(backup_at, NILFS_SB);
    let valid1 = match &sbb {
        None => {
            // A valid primary carries on past an unreadable backup: this is
            // probably a CD, where errors at the end are normal.
            if !valid0 {
                return pr.none_or_err();
            }
            false
        }
        Some(b) => valid_sb(pr, b, true),
    };
    if !valid0 && !valid1 {
        return 1;
    }
    let swp = valid1 && (!valid0 || sbb.as_ref().is_some_and(|b| sbp.le64(56) > b.le64(56)));
    let sb = match (&sbb, swp) {
        (Some(b), true) => b.clone(),
        _ => sbp.clone(),
    };
    if sb.u8_at(168) != 0 {
        pr.set_label(sb.span(168, 80));
    }
    pr.set_uuid(sb.span(152, 16));
    pr.sprintf_version(sb.le32(0).to_string());
    let magoff = if swp { backup_at } else { NILFS_SB_OFFSET }.wrapping_add(6);
    if pr.set_magic(magoff, sb.span(6, 2)) != 0 {
        return 1;
    }
    let log = sb.le32(20);
    if log < 32 {
        pr.set_fsblocksize(1024u32.wrapping_shl(log));
        pr.set_block_size(1024u32.wrapping_shl(log));
    }
    0
}

/// `nilfs2_idinfo`.
pub static NILFS2: IdInfo = IdInfo {
    name: "nilfs2",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 1024 * 1024,
    probefunc: Some(probe_nilfs2),
    magics: &[],
};
