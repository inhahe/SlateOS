//! `adaptec_raid.c`: Adaptec HostRAID members, metadata in the last sector.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, PROBE_NONE, PROBE_OK, USAGE_RAID};

/// `sizeof(struct adaptec_metadata)`.
const AD_SIZE: u64 = 512;

/// `probe_adraid`.
fn probe_adraid(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    if pr.size < 0x10000 {
        return PROBE_NONE;
    }
    if !pr.is_reg() && !pr.is_wholedisk() {
        return PROBE_NONE;
    }
    let off = (pr.size / 0x200).wrapping_sub(1).wrapping_mul(0x200);
    let Some(ad) = pr.get_buffer(off, AD_SIZE) else {
        return pr.none_or_err();
    };
    // `smagic` is AD_SIGNATURE, "DPTM", and `b0idcode` AD_MAGIC, both
    // stored big-endian.
    if ad.span(256, 4) != b"DPTM" {
        return PROBE_NONE;
    }
    if ad.span(0, 4) != [0x37, 0xFC, 0x4D, 0x1E] {
        return PROBE_NONE;
    }
    if pr.sprintf_version(ad.u8_at(63).to_string()) != 0 {
        return PROBE_NONE;
    }
    if pr.set_magic(off, ad.span(0, 4)) != 0 {
        return PROBE_NONE;
    }
    PROBE_OK
}

/// `adraid_idinfo`.
pub static ADRAID: IdInfo = IdInfo {
    name: "adaptec_raid_member",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_adraid),
    magics: &[],
};
