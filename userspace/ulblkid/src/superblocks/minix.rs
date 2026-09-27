//! `minix.c`: the Minix filesystem, versions 1, 2 and 3, either byte order.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, Endianness, USAGE_FILESYSTEM};

/// `MINIX_BLOCK_SIZE`.
const MINIX_BLOCK_SIZE: u64 = 1024;
/// `MINIX_VALID_FS`.
const MINIX_VALID_FS: u16 = 0x0001;
/// `MINIX_ERROR_FS`.
const MINIX_ERROR_FS: u16 = 0x0002;
/// `MINIX3_SUPER_MAGIC`.
const MINIX3_SUPER_MAGIC: u16 = 0x4d5a;

/// `get_minix_version(data, &other_endian)`: 1, 2 or 3, and whether the
/// superblock is in the other byte order. A byte-swapped version 3 is never
/// found: upstream's second pass tests the unswapped v3 magic again.
fn get_version(data: &[u8]) -> Option<(u32, bool)> {
    let v12 = |m: u16| match m {
        0x137F | 0x138F => Some(1),
        0x2468 | 0x2478 => Some(2),
        _ => None,
    };
    let magic = data.le16(16);
    let v3 = data.le16(24) == MINIX3_SUPER_MAGIC;
    if let Some(v) = v12(magic).or(if v3 { Some(3) } else { None }) {
        return Some((v, false));
    }
    v12(magic.swap_bytes())
        .or(if v3 { Some(3) } else { None })
        .map(|v| (v, true))
}

/// `probe_minix`.
fn probe_minix(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let err = |pr: &Probe| pr.none_or_err();
    // max(sizeof(struct minix_super_block), sizeof(struct minix3_super_block)).
    let Some(data) = pr.get_buffer(1024, 32) else {
        return err(pr);
    };
    let Some((version, swab)) = get_version(&data) else {
        return 1;
    };
    let s16 = |at: usize| {
        let v = data.le16(at);
        if swab { v.swap_bytes() } else { v }
    };
    let s32 = |at: usize| {
        let v = data.le32(at);
        if swab { v.swap_bytes() } else { v }
    };
    let (zones, ninodes, imaps, zmaps, firstz, zone_size, block_size): (
        u64,
        u64,
        u64,
        u64,
        i64,
        u16,
        u32,
    );
    if version == 3 {
        zones = u64::from(s32(20));
        ninodes = u64::from(s32(0));
        imaps = u64::from(s16(6));
        zmaps = u64::from(s16(8));
        firstz = i64::from(s16(10));
        zone_size = data.le16(12);
        block_size = u32::from(s16(28));
    } else {
        let state = s16(18);
        if state & (MINIX_VALID_FS | MINIX_ERROR_FS) != state {
            return 1;
        }
        zones = if version == 2 {
            u64::from(s32(20))
        } else {
            u64::from(s16(2))
        };
        ninodes = u64::from(s16(0));
        imaps = u64::from(s16(4));
        zmaps = u64::from(s16(6));
        firstz = i64::from(s16(8));
        zone_size = data.le16(10);
        block_size = 1024;
    }
    // fsck.minix's read_superblock checks.
    if zone_size != 0 || ninodes == 0 || ninodes == u64::from(u32::MAX) {
        return 1;
    }
    if imaps.wrapping_mul(MINIX_BLOCK_SIZE).wrapping_mul(8) < ninodes.wrapping_add(1) {
        return 1;
    }
    #[allow(clippy::cast_possible_wrap, reason = "C's (off_t) cast")]
    if firstz > zones as i64 {
        return 1;
    }
    #[allow(clippy::cast_sign_loss, reason = "C mixes unsigned long and off_t")]
    if zmaps.wrapping_mul(MINIX_BLOCK_SIZE).wrapping_mul(8)
        < zones.wrapping_sub(firstz as u64).wrapping_add(1)
    {
        return 1;
    }
    // Parts of ext3 can pass for a minix superblock: not if ext's magic is
    // there.
    let Some(ext) = pr.get_buffer(0x400 + 0x38, 2) else {
        return err(pr);
    };
    if ext.span(0, 2) == b"\x53\xef" {
        return 1;
    }
    pr.sprintf_version(version.to_string());
    pr.set_fsblocksize(block_size);
    pr.set_block_size(block_size);
    pr.set_fsendianness(if swab {
        Endianness::Big
    } else {
        Endianness::Little
    });
    0
}

/// `minix_idinfo`.
pub static MINIX: IdInfo = IdInfo {
    name: "minix",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_minix),
    magics: &[
        // Version 1, little- then big-endian.
        IdMag::new(b"\x7f\x13", 1, 0x10),
        IdMag::new(b"\x8f\x13", 1, 0x10),
        IdMag::new(b"\x13\x7f", 1, 0x10),
        IdMag::new(b"\x13\x8f", 1, 0x10),
        // Version 2.
        IdMag::new(b"\x68\x24", 1, 0x10),
        IdMag::new(b"\x78\x24", 1, 0x10),
        IdMag::new(b"\x24\x68", 1, 0x10),
        IdMag::new(b"\x24\x78", 1, 0x10),
        // Version 3.
        IdMag::new(b"\x5a\x4d", 1, 0x18),
        IdMag::new(b"\x4d\x5a", 1, 0x18),
    ],
};
