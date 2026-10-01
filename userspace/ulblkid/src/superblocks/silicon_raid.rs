//! `silicon_raid.c`: Silicon Image Medley RAID members, metadata in the last
//! sector.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_RAID};

/// `SILICON_MAGIC`.
const SILICON_MAGIC: u32 = 0x2F00_0000;
/// `sizeof(struct silicon_metadata)`.
const SIL_SIZE: u64 = 0x200;
/// `offsetof(struct silicon_metadata, magic)`.
const MAGIC_OFF: usize = 0x60;
/// `offsetof(struct silicon_metadata, checksum1)`.
const CHECKSUM1_OFF: usize = 0x13E;

/// `probe_silraid`.
fn probe_silraid(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    if pr.size < 0x10000 {
        return 1;
    }
    if !pr.is_reg() && !pr.is_wholedisk() {
        return 1;
    }
    let off = (pr.size / 0x200).wrapping_sub(1).wrapping_mul(0x200);
    let Some(sil) = pr.get_buffer(off, SIL_SIZE) else {
        return pr.none_or_err();
    };
    if sil.le32(MAGIC_OFF) != SILICON_MAGIC {
        return 1;
    }
    if sil.u8_at(0x116) >= 8 {
        return 1;
    }
    // `silraid_checksum`: the negated sum of the 16-bit words before it.
    let sum = (0..CHECKSUM1_OFF / 2).fold(0i32, |s, k| {
        s.wrapping_add(i32::from(sil.le16(k.saturating_mul(2))))
    });
    let csum = u64::from(u16::try_from(sum.wrapping_neg() & 0xFFFF).unwrap_or(0));
    if !pr.verify_csum(csum, u64::from(sil.le16(CHECKSUM1_OFF))) {
        return 1;
    }
    if pr.sprintf_version(format!("{}.{}", sil.le16(0x10A), sil.le16(0x108))) != 0 {
        return 1;
    }
    if pr.set_magic(off.wrapping_add(MAGIC_OFF as u64), sil.span(MAGIC_OFF, 4)) != 0 {
        return 1;
    }
    0
}

/// `silraid_idinfo`.
pub static SILRAID: IdInfo = IdInfo {
    name: "silicon_medley_raid_member",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_silraid),
    magics: &[],
};
