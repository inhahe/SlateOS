//! `nvidia_raid.c`: NVIDIA MediaShield RAID members, metadata in the
//! second-to-last sector.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_RAID};

/// `NVIDIA_SIGNATURE`.
const NVIDIA_SIGNATURE: &[u8] = b"NVIDIA  ";
/// `NVIDIA_SUPERBLOCK_SIZE`.
const NVIDIA_SUPERBLOCK_SIZE: u64 = 120;

/// `probe_nvraid`.
fn probe_nvraid(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    if pr.size < 0x10000 {
        return 1;
    }
    if !pr.is_reg() && !pr.is_wholedisk() {
        return 1;
    }
    let off = (pr.size / 0x200).wrapping_sub(2).wrapping_mul(0x200);
    let Some(nv) = pr.get_buffer(off, NVIDIA_SUPERBLOCK_SIZE) else {
        return pr.none_or_err();
    };
    if nv.span(0, 8) != NVIDIA_SIGNATURE {
        return 1;
    }
    let size = nv.le32(8);
    if u64::from(size.wrapping_mul(4)) != NVIDIA_SUPERBLOCK_SIZE {
        return 1;
    }
    // `nvraid_verify_checksum`: the stored checksum plus every word
    // (itself included) must come back to the stored checksum.
    let stored = nv.le32(12);
    let csum = (0..size as usize).fold(stored, |c, i| c.wrapping_add(nv.le32(i.saturating_mul(4))));
    if !pr.verify_csum(u64::from(csum), u64::from(stored)) {
        return 1;
    }
    if pr.sprintf_version(nv.le16(16).to_string()) != 0 {
        return 1;
    }
    if pr.set_magic(off, nv.span(0, 8)) != 0 {
        return 1;
    }
    0
}

/// `nvraid_idinfo`.
pub static NVRAID: IdInfo = IdInfo {
    name: "nvidia_raid_member",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_nvraid),
    magics: &[],
};
