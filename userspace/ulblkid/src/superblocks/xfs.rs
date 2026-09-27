//! `xfs.c`: XFS filesystems, their sanity checks and v5 CRC, and XFS
//! external log devices.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM, USAGE_OTHER};

/// `sizeof(struct xfs_super_block)`.
const XFS_SB_SIZE: u64 = 272;
/// `offsetof(struct xfs_super_block, sb_crc)`.
const SB_CRC_OFF: usize = 224;
/// `XFS_MIN_AG_BLOCKS`.
const XFS_MIN_AG_BLOCKS: u64 = 64;
/// `XFS_SB_VERSION_MOREBITSBIT`.
const XFS_SB_VERSION_MOREBITSBIT: u16 = 0x8000;
/// `XFS_SB_VERSION2_CRCBIT`.
const XFS_SB_VERSION2_CRCBIT: u32 = 0x0000_0100;

/// The sanity checks `xfs_verify_sb` and `exfs_verify_sb` share: sector,
/// block and inode sizes and their logs, the realtime extent size, the
/// inode percentage, and the data block count against the allocation
/// groups.
pub(super) fn geometry_ok(sb: &[u8]) -> bool {
    let agcount = sb.be32(88);
    let agblocks = sb.be32(84);
    let sectsize = u32::from(sb.be16(102));
    let sectlog = u32::from(sb.u8_at(121));
    let blocksize = sb.be32(4);
    let blocklog = u32::from(sb.u8_at(120));
    let inodesize = u32::from(sb.be16(104));
    let inodelog = u32::from(sb.u8_at(122));
    let inopblog = i32::from(sb.u8_at(123));
    let rextsize = sb.be32(80);
    let imax_pct = sb.u8_at(127);
    let dblocks = sb.be64(8);
    let max_dblocks = u64::from(agcount).wrapping_mul(u64::from(agblocks));
    let min_dblocks = u64::from(agcount.wrapping_sub(1))
        .wrapping_mul(u64::from(agblocks))
        .wrapping_add(XFS_MIN_AG_BLOCKS);
    let rt = rextsize.wrapping_mul(blocksize);
    if agcount == 0
        || !(512..=32768).contains(&sectsize)
        || !(9..=15).contains(&sectlog)
        || sectsize != 1 << sectlog
        || !(512..=65536).contains(&blocksize)
        || !(9..=16).contains(&blocklog)
        || u64::from(blocksize) != 1u64 << blocklog
        || !(256..=2048).contains(&inodesize)
        || !(8..=11).contains(&inodelog)
        || inodesize != 1 << inodelog
        || i32::from(sb.u8_at(120)).wrapping_sub(i32::from(sb.u8_at(122))) != inopblog
        || rt > 1024 * 1024 * 1024
        || rt < 4 * 1024
        || imax_pct > 100
        || dblocks == 0
        || dblocks > max_dblocks
        || dblocks < min_dblocks
    {
        return false;
    }
    true
}

/// `xfs_verify_sb(ondisk, pr, mag)`: the geometry is self-consistent, and a
/// v5 superblock's CRC-32C holds.
fn verify_sb(sb: &[u8], pr: &mut Probe, mag: Option<&'static IdMag>) -> bool {
    if !geometry_ok(sb) {
        return false;
    }
    let sectsize = u32::from(sb.be16(102));
    let versionnum = sb.be16(100);
    if versionnum & 0x0f == 5 {
        if versionnum & XFS_SB_VERSION_MOREBITSBIT == 0 {
            return false;
        }
        if sb.be32(200) & XFS_SB_VERSION2_CRCBIT == 0 {
            return false;
        }
        let expected = sb.be32(SB_CRC_OFF);
        let Some(csummed) = pr.get_sb_buffer(mag, u64::from(sectsize)) else {
            return false;
        };
        let raw = crc32c::crc32c_raw_exclude(!0, &csummed, SB_CRC_OFF, 4);
        let crc = (raw ^ !0).swap_bytes();
        if !pr.verify_csum(u64::from(crc), u64::from(expected)) {
            return false;
        }
    }
    true
}

/// `probe_xfs`.
fn probe_xfs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(xs) = pr.get_sb_buffer(mag, XFS_SB_SIZE) else {
        return pr.none_or_err();
    };
    if !verify_sb(&xs, pr, mag) {
        return 1;
    }
    if xs.u8_at(108) != 0 {
        pr.set_label(xs.span(108, 12));
    }
    pr.set_uuid(xs.span(32, 16));
    // `xfs_fssize`: the data blocks less an internal log's.
    let lsize = if xs.span(48, 8).iter().any(|&b| b != 0) {
        xs.be32(96)
    } else {
        0
    };
    let avail = xs.be64(8).wrapping_sub(u64::from(lsize));
    pr.set_fssize(avail.wrapping_mul(u64::from(xs.be32(4))));
    pr.set_fslastblock(xs.be64(8));
    pr.set_fsblocksize(xs.be32(4));
    pr.set_block_size(u32::from(xs.be16(102)));
    0
}

/// `xlog_valid_rec_header`: a log record header -- magic, a known version,
/// a body length, a known format.
fn valid_rec_header(r: &[u8]) -> bool {
    if r.span(0, 4) != 0xFEED_BABE_u32.to_be_bytes() {
        return false;
    }
    let version = r.be32(8);
    if version == 0 || version & !3 != 0 {
        return false;
    }
    let hlen = r.be32(12);
    if hlen == 0 || hlen > 0x7fff_ffff {
        return false;
    }
    let fmt = r.be32(300);
    matches!(fmt, 1..=3)
}

/// `probe_xfs_log`: a log record header in one of the first 512 sectors --
/// and no XFS superblock among them, which would make this a filesystem.
fn probe_xfs_log(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(buf) = pr.get_buffer(0, 256 * 1024) else {
        return pr.none_or_err();
    };
    for i in 0..512usize {
        let sector = buf.span(i.saturating_mul(512), 512);
        if sector.span(0, 4) == b"XFSB" {
            return 1;
        }
        if valid_rec_header(sector) {
            pr.set_uuid_as(sector.span(304, 16), Some("LOGUUID"));
            if pr.set_magic(i.saturating_mul(512) as u64, sector.span(0, 4)) != 0 {
                return 1;
            }
            return 0;
        }
    }
    1
}

/// `xfs_idinfo`.
pub static XFS: IdInfo = IdInfo {
    name: "xfs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_xfs),
    magics: &[IdMag::new(b"XFSB", 0, 0)],
};

/// `xfs_log_idinfo`.
pub static XFS_LOG: IdInfo = IdInfo {
    name: "xfs_external_log",
    usage: USAGE_OTHER,
    flags: 0,
    minsz: 10 * 1024 * 1024,
    probefunc: Some(probe_xfs_log),
    magics: &[],
};
