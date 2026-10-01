//! `ntfs.c`: NTFS -- the boot sector's BPB checked, the MFT found, and the
//! label read from the $Volume record's name attribute.

use crate::encode::ENCODE_UTF16LE;
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `MFT_RECORD_VOLUME`.
const MFT_RECORD_VOLUME: u64 = 3;
/// `NTFS_MAX_CLUSTER_SIZE`.
const NTFS_MAX_CLUSTER_SIZE: u32 = 2 * 1024 * 1024;
/// `MFT_RECORD_ATTR_VOLUME_NAME`.
const ATTR_VOLUME_NAME: u32 = 0x60;
/// `MFT_RECORD_ATTR_END`.
const ATTR_END: u32 = 0xffff_ffff;
/// `sizeof(struct file_attribute)`.
const FILE_ATTRIBUTE: u64 = 22;

/// `__probe_ntfs(pr, mag, save_info)`.
fn probe(pr: &mut Probe, mag: Option<&'static IdMag>, save_info: bool) -> i32 {
    let err = |pr: &Probe| pr.none_or_err();
    // struct ntfs_super_block: 84 bytes.
    let Some(ns) = pr.get_sb_buffer(mag, 84) else {
        return err(pr);
    };
    let sector_size = ns.le16(11);
    if !(256..=4096).contains(&sector_size) || !sector_size.is_power_of_two() {
        return 1;
    }
    let spc_raw = ns.u8_at(13);
    let sectors_per_cluster: u32 = match spc_raw {
        1 | 2 | 4 | 8 | 16 | 32 | 64 | 128 => u32::from(spc_raw),
        240..=249 => 1u32 << 256u32.wrapping_sub(u32::from(spc_raw)),
        _ => return 1,
    };
    if u32::from(sector_size).wrapping_mul(sectors_per_cluster) > NTFS_MAX_CLUSTER_SIZE {
        return 1;
    }
    // Unused fields must be zero.
    if ns.le16(14) != 0
        || ns.le16(17) != 0
        || ns.le16(19) != 0
        || ns.le16(22) != 0
        || ns.le32(32) != 0
        || ns.u8_at(16) != 0
    {
        return 1;
    }
    let cpmr_raw = ns.u8_at(64);
    let cpmr = i8::from_ne_bytes([cpmr_raw]);
    if !(0xe1..=0xf7).contains(&cpmr_raw) && !matches!(cpmr, 1 | 2 | 4 | 8 | 16 | 32 | 64) {
        return 1;
    }
    let mft_record_size: u32 = if cpmr > 0 {
        u32::from(cpmr.unsigned_abs())
            .wrapping_mul(sectors_per_cluster)
            .wrapping_mul(u32::from(sector_size))
    } else {
        let shift = 0i8.wrapping_sub(cpmr);
        if !(0..31).contains(&shift) {
            return 1;
        }
        1u32 << shift
    };
    // Not 0: one of the values matched above.
    let nr_clusters = ns
        .le64(40)
        .checked_div(u64::from(sectors_per_cluster))
        .unwrap_or(0);
    if ns.le64(48) > nr_clusters || ns.le64(56) > nr_clusters {
        return 1;
    }
    let mut off = ns
        .le64(48)
        .wrapping_mul(u64::from(sector_size))
        .wrapping_mul(u64::from(sectors_per_cluster));
    if mft_record_size < 4 {
        return 1;
    }
    let Some(buf_mft) = pr.get_buffer(off, u64::from(mft_record_size)) else {
        return err(pr);
    };
    if buf_mft.span(0, 4) != b"FILE" {
        return 1;
    }
    off = off.wrapping_add(MFT_RECORD_VOLUME * u64::from(mft_record_size));
    let Some(buf_mft) = pr.get_buffer(off, u64::from(mft_record_size)) else {
        return err(pr);
    };
    if buf_mft.span(0, 4) != b"FILE" {
        return 1;
    }
    if !save_info {
        return 0;
    }
    // $Volume's attributes, for its name.
    let mut attr_off = u64::from(buf_mft.le16(20));
    let bytes_allocated = u64::from(buf_mft.le32(28));
    let rec = u64::from(mft_record_size);
    while attr_off.wrapping_add(FILE_ATTRIBUTE) <= rec && attr_off <= bytes_allocated {
        let at = usize::try_from(attr_off).unwrap_or(usize::MAX);
        let attr = buf_mft.get(at..).unwrap_or_default();
        let attr_len = attr.le32(4);
        if attr_len == 0 {
            break;
        }
        let ty = attr.le32(0);
        if ty == ATTR_END {
            break;
        }
        if ty == ATTR_VOLUME_NAME {
            let val_off = u64::from(attr.le16(20));
            let val_len = u64::from(attr.le32(16));
            if attr_off.wrapping_add(val_off).wrapping_add(val_len) <= rec {
                let v = attr.span(val_off as usize, val_len as usize);
                pr.set_utf8label(v, ENCODE_UTF16LE);
            }
            break;
        }
        attr_off = attr_off.wrapping_add(u64::from(attr_len));
    }
    pr.set_fsblocksize(u32::from(sector_size).wrapping_mul(sectors_per_cluster));
    pr.set_block_size(u32::from(sector_size));
    pr.set_fssize(ns.le64(40).wrapping_mul(u64::from(sector_size)));
    pr.sprintf_uuid(ns.span(72, 8), format!("{:016X}", ns.le64(72)));
    0
}

/// `probe_ntfs`.
fn probe_ntfs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    probe(pr, mag, true)
}

/// `blkid_probe_is_ntfs`: 1 for NTFS, 0 not, `-errno` for a read error.
pub(crate) fn is_ntfs(pr: &mut Probe) -> i32 {
    let (rc, _, mag) = pr.get_idmag(&NTFS);
    if rc < 0 {
        return rc;
    }
    if rc != crate::PROBE_OK || mag.is_none() {
        return 0;
    }
    i32::from(probe(pr, mag, false) == 0)
}

/// `ntfs_idinfo`.
pub static NTFS: IdInfo = IdInfo {
    name: "ntfs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_ntfs),
    magics: &[IdMag::new(b"NTFS    ", 0, 3)],
};
