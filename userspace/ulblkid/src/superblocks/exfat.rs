//! `exfat.c`: exFAT -- the boot region's checksum verified, and the label
//! found by walking the root directory's cluster chain.

use crate::encode::ENCODE_UTF16LE;
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, PROBE_NONE, PROBE_OK, USAGE_FILESYSTEM};

/// `sizeof(struct exfat_super_block)`.
const EXFAT_SB: u64 = 512;
/// `EXFAT_FIRST_DATA_CLUSTER`.
const FIRST_DATA_CLUSTER: u32 = 2;
/// `EXFAT_LAST_DATA_CLUSTER`.
const LAST_DATA_CLUSTER: u32 = 0x0fff_fff6;
/// `EXFAT_ENTRY_SIZE`.
const ENTRY_SIZE: u64 = 32;
/// `EXFAT_ENTRY_EOD`.
const ENTRY_EOD: u8 = 0x00;
/// `EXFAT_ENTRY_LABEL`.
const ENTRY_LABEL: u8 = 0x83;
/// `EXFAT_MAX_DIR_SIZE`.
const MAX_DIR_SIZE: u64 = 256 * 1024 * 1024;

/// The superblock's geometry.
struct Sb<'a>(&'a [u8]);

impl Sb<'_> {
    fn bps_shift(&self) -> u32 {
        u32::from(self.0.u8_at(108))
    }
    fn spc_shift(&self) -> u32 {
        u32::from(self.0.u8_at(109))
    }
    /// `BLOCK_SIZE(sb)`.
    fn block_size(&self) -> u32 {
        if self.bps_shift() < 32 {
            1u32 << self.bps_shift()
        } else {
            0
        }
    }
    /// `CLUSTER_SIZE(sb)`.
    fn cluster_size(&self) -> u32 {
        if self.spc_shift() < 32 {
            self.block_size().wrapping_shl(self.spc_shift())
        } else {
            0
        }
    }
    /// `block_to_offset`.
    fn block_to_offset(&self, block: u64) -> u64 {
        block.wrapping_shl(self.bps_shift())
    }
    /// `cluster_to_offset`.
    fn cluster_to_offset(&self, cluster: u32) -> u64 {
        let block = u64::from(self.0.le32(88)).wrapping_add(
            u64::from(cluster.wrapping_sub(FIRST_DATA_CLUSTER)).wrapping_shl(self.spc_shift()),
        );
        self.block_to_offset(block)
    }
}

/// `next_cluster`: the FAT entry for `cluster`, 0 if it cannot be read.
fn next_cluster(pr: &mut Probe, sb: &Sb<'_>, cluster: u32) -> u32 {
    let fat_offset = sb
        .block_to_offset(u64::from(sb.0.le32(80)))
        .wrapping_add(u64::from(cluster) * 4);
    pr.get_buffer(fat_offset, 4).map_or(0, |b| b.le32(0))
}

/// `find_label`: the volume label entry of the root directory.
fn find_label(pr: &mut Probe, sb: &Sb<'_>) -> Option<Vec<u8>> {
    let mut cluster = sb.0.le32(96);
    let mut offset = sb.cluster_to_offset(cluster);
    for _ in 0..MAX_DIR_SIZE / ENTRY_SIZE {
        let entry = pr.get_buffer(offset, ENTRY_SIZE)?;
        match entry.u8_at(0) {
            ENTRY_EOD => return None,
            ENTRY_LABEL => return Some(entry.to_vec()),
            _ => {}
        }
        offset = offset.wrapping_add(ENTRY_SIZE);
        let cs = u64::from(sb.cluster_size());
        // Upstream divides by a cluster size of 0 too.
        if cs != 0 && offset.is_multiple_of(cs) {
            cluster = next_cluster(pr, sb, cluster);
            if !(FIRST_DATA_CLUSTER..=LAST_DATA_CLUSTER).contains(&cluster) {
                return None;
            }
            offset = sb.cluster_to_offset(cluster);
        }
    }
    None
}

/// `exfat_boot_checksum`: the rotating sum of the first eleven sectors,
/// less the volume flags and percent-in-use bytes.
fn boot_checksum(sectors: &[u8], sector_size: usize) -> u32 {
    let mut checksum: u32 = 0;
    for (i, &b) in sectors
        .iter()
        .take(sector_size.saturating_mul(11))
        .enumerate()
    {
        if i == 106 || i == 107 || i == 112 {
            continue;
        }
        checksum = (if checksum & 1 != 0 { 0x8000_0000u32 } else { 0 })
            .wrapping_add(checksum >> 1)
            .wrapping_add(u32::from(b));
    }
    checksum
}

/// `exfat_validate_checksum`: every copy in the twelfth sector must match.
fn validate_checksum(pr: &mut Probe, sb: &Sb<'_>) -> bool {
    let sector_size = sb.block_size() as usize;
    let Some(data) = pr.get_buffer(0, (sector_size as u64).wrapping_mul(12)) else {
        return false;
    };
    let checksum = boot_checksum(&data, sector_size);
    for i in 0..sector_size / 4 {
        let expected = data.le32(
            sector_size
                .saturating_mul(11)
                .saturating_add(i.saturating_mul(4)),
        );
        if !pr.verify_csum(u64::from(checksum), u64::from(expected)) {
            return false;
        }
    }
    true
}

/// `exfat_valid_superblock`.
fn valid_superblock(pr: &mut Probe, sb: &Sb<'_>) -> bool {
    if sb.0.le16(510) != 0xAA55 {
        return false;
    }
    if sb.cluster_size() == 0 {
        return false;
    }
    if sb.0.span(0, 3) != b"\xEB\x76\x90" {
        return false;
    }
    if sb.0.span(11, 53).iter().any(|&b| b != 0) {
        return false;
    }
    validate_checksum(pr, sb)
}

/// `blkid_probe_is_exfat`: found by vfat's magics, which include the boot
/// jump exFAT starts with.
pub(crate) fn is_exfat(pr: &mut Probe) -> i32 {
    let (rc, _, mag) = pr.get_idmag(&super::vfat::VFAT);
    if rc < 0 {
        return rc;
    }
    if rc != PROBE_OK || mag.is_none() {
        return 0;
    }
    let Some(sb) = pr.get_sb_buffer(mag, EXFAT_SB) else {
        return 0;
    };
    if sb.span(3, 8) != b"EXFAT   " {
        return 0;
    }
    i32::from(valid_superblock(pr, &Sb(&sb)))
}

/// `probe_exfat`.
fn probe_exfat(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(raw) = pr.get_sb_buffer(mag, EXFAT_SB) else {
        return pr.none_or_err();
    };
    let sb = Sb(&raw);
    if !valid_superblock(pr, &sb) {
        return PROBE_NONE;
    }
    match find_label(pr, &sb) {
        Some(label) => {
            let len = (usize::from(label.u8_at(1)) * 2).min(22);
            pr.set_utf8label(label.span(2, len), ENCODE_UTF16LE);
        }
        None => {
            if pr.errno != 0 {
                return pr.errno.wrapping_neg();
            }
        }
    }
    let s = raw.span(100, 4);
    let text = format!(
        "{:02X}{:02X}-{:02X}{:02X}",
        s.u8_at(3),
        s.u8_at(2),
        s.u8_at(1),
        s.u8_at(0)
    );
    pr.sprintf_uuid(s, text);
    pr.sprintf_version(format!("{}.{}", raw.u8_at(105), raw.u8_at(104)));
    let bs = sb.block_size();
    pr.set_fsblocksize(bs);
    pr.set_block_size(bs);
    pr.set_fssize(u64::from(bs).wrapping_mul(raw.le64(72)));
    PROBE_OK
}

/// `exfat_idinfo`.
pub static EXFAT: IdInfo = IdInfo {
    name: "exfat",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_exfat),
    magics: &[IdMag::new(b"EXFAT   ", 0, 3)],
};
