//! `via_raid.c`: VIA RAID members, metadata in the last sector.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_RAID};

/// `VIA_SIGNATURE`.
const VIA_SIGNATURE: u16 = 0xAA55;
/// `sizeof(struct via_metadata)`.
const VIA_SIZE: u64 = 51;

/// `probe_viaraid`.
fn probe_viaraid(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    if pr.size < 0x10000 {
        return 1;
    }
    if !pr.is_reg() && !pr.is_wholedisk() {
        return 1;
    }
    let off = (pr.size / 0x200).wrapping_sub(1).wrapping_mul(0x200);
    let Some(v) = pr.get_buffer(off, VIA_SIZE) else {
        return pr.none_or_err();
    };
    if v.le16(0) != VIA_SIGNATURE {
        return 1;
    }
    let version = v.u8_at(2);
    if version > 2 {
        return 1;
    }
    // `via_checksum`: the first 50 bytes summed into a byte.
    let cs = v.span(0, 50).iter().fold(0u8, |a, &b| a.wrapping_add(b));
    if !pr.verify_csum(u64::from(cs), u64::from(v.u8_at(50))) {
        return 1;
    }
    if pr.sprintf_version(version.to_string()) != 0 {
        return 1;
    }
    if pr.set_magic(off, v.span(0, 2)) != 0 {
        return 1;
    }
    0
}

/// `viaraid_idinfo`.
pub static VIARAID: IdInfo = IdInfo {
    name: "via_raid_member",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_viaraid),
    magics: &[],
};
