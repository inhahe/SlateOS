//! `cramfs.c`: cramfs, either byte order, its v2 CRC verified.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, Endianness, USAGE_FILESYSTEM};

/// `sizeof(struct cramfs_super)`.
const CRAMFS_SUPER: u64 = 64;
/// `offsetof(struct cramfs_super, info.crc)`.
const CRC_OFF: u64 = 32;
/// `CRAMFS_FLAG_FSID_VERSION_2`.
const FSID_VERSION_2: u32 = 0x0000_0001;

/// `cfs32_to_cpu(le, value)` of the word at `at`.
fn cfs32(le: bool, b: &[u8], at: usize) -> u32 {
    if le { b.le32(at) } else { b.be32(at) }
}

/// `cramfs_verify_csum`: CRC-32 of the image's first `size` bytes with the
/// CRC field zeroed -- zeroed in the cached buffer, as upstream zeroes it.
fn verify_csum(pr: &mut Probe, mag: Option<&'static IdMag>, cs: &[u8], le: bool) -> bool {
    let expected = cfs32(le, cs, 32);
    let csummed_size = cfs32(le, cs, 4);
    if csummed_size > 1 << 16 || u64::from(csummed_size) < CRAMFS_SUPER {
        return false;
    }
    let size = u64::from(csummed_size);
    if pr.get_sb_buffer(mag, size).is_none() {
        return false;
    }
    let base = mag.map_or(0, |m| u64::try_from(m.kboff << 10).unwrap_or(0));
    pr.zero_in_cache(base, size, base.wrapping_add(CRC_OFF), 4);
    let Some(csummed) = pr.get_sb_buffer(mag, size) else {
        return false;
    };
    let crc = !crc32::crc32_raw(!0, &csummed);
    pr.verify_csum(u64::from(crc), u64::from(expected))
}

/// `probe_cramfs`.
fn probe_cramfs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(cs) = pr.get_sb_buffer(mag, CRAMFS_SUPER) else {
        return pr.none_or_err();
    };
    let le = mag.is_some_and(|m| m.magic == b"\x45\x3d\xcd\x28");
    let v2 = cfs32(le, &cs, 8) & FSID_VERSION_2 != 0;
    if v2 && !verify_csum(pr, mag, &cs, le) {
        return 1;
    }
    pr.set_label(cs.span(48, 16));
    pr.set_fssize(u64::from(cfs32(le, &cs, 4)));
    pr.sprintf_version(if v2 { "2" } else { "1" });
    pr.set_fsendianness(if le {
        Endianness::Little
    } else {
        Endianness::Big
    });
    0
}

/// `cramfs_idinfo`.
pub static CRAMFS: IdInfo = IdInfo {
    name: "cramfs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_cramfs),
    magics: &[
        IdMag::new(b"\x45\x3d\xcd\x28", 0, 0),
        IdMag::new(b"\x28\xcd\x3d\x45", 0, 0),
    ],
};
