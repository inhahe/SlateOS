//! `bcache.c`: bcache devices (cache and backing) and bcachefs.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, PROBE_NONE, PROBE_OK, USAGE_FILESYSTEM, USAGE_OTHER};

/// `BCACHE_SB_MAGIC`.
const BCACHE_SB_MAGIC: &[u8] = b"\xc6\x85\x73\xf6\x4e\x1a\x45\xca\x82\x65\xf5\x7f\x48\xba\x6d\x81";
/// `BCACHEFS_SB_MAGIC`.
const BCACHEFS_SB_MAGIC: &[u8] =
    b"\xc6\x85\x73\xf6\x66\xce\x90\xa9\xd9\x6a\x60\xcf\x80\x3d\xf7\xef";
/// `BCACHE_SB_OFF`.
const BCACHE_SB_OFF: u64 = 0x1000;
/// `BCACHE_SB_MAGIC_OFF`: `offsetof(struct bcache_super_block, magic)`.
const BCACHE_SB_MAGIC_OFF: u32 = 24;
/// `sizeof(struct bcache_super_block)`.
const BCACHE_SB_SIZE: u64 = 2258;
/// `offsetof(struct bcache_super_block, d)`.
const BCACHE_D_OFF: u64 = 208;
/// `BCACHE_SB_CSUMMED_START`.
const BCACHE_SB_CSUMMED_START: usize = 8;
/// `BCACHEFS_SECTOR_SIZE`.
const BCACHEFS_SECTOR_SIZE: u64 = 512;
/// `BCACHEFS_SB_MAX_SIZE_SHIFT`.
const BCACHEFS_SB_MAX_SIZE_SHIFT: u8 = 0x10;
/// `BCACHEFS_SB_MAX_SIZE`.
const BCACHEFS_SB_MAX_SIZE: u64 = 1 << BCACHEFS_SB_MAX_SIZE_SHIFT;
/// `BCACHEFS_SB_FIELDS_OFF`: `offsetof(struct bcachefs_super_block, _start)`.
const BCACHEFS_SB_FIELDS_OFF: u64 = 752;
/// `BCACHEFS_SB_FIELD_TYPE_MEMBERS`.
const FIELD_TYPE_MEMBERS: u32 = 1;
/// `BCACHEFS_SB_FIELD_TYPE_DISK_GROUPS`.
const FIELD_TYPE_DISK_GROUPS: u32 = 5;
/// `sizeof(struct bcachefs_sb_member)`.
const MEMBER_SIZE: u64 = 56;
/// `sizeof(struct bcachefs_sb_disk_group)`.
const DISK_GROUP_SIZE: u64 = 48;

/// `bcache_verify_checksum`: CRC-64/WE of the superblock up to the end of
/// its journal buckets in use.
fn bcache_verify_checksum(pr: &mut Probe, mag: Option<&'static IdMag>, bcs: &[u8]) -> bool {
    let keys = u64::from(bcs.le16(206));
    if keys > 256 {
        return false;
    }
    let csummed_size = BCACHE_D_OFF.wrapping_add(keys.wrapping_mul(8));
    let Some(csummed) = pr.get_sb_buffer(mag, csummed_size) else {
        return false;
    };
    let data = csummed.get(BCACHE_SB_CSUMMED_START..).unwrap_or_default();
    let csum = crc64::crc64_we(data);
    pr.verify_csum(csum, bcs.le64(0))
}

/// `probe_bcache`.
fn probe_bcache(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(bcs) = pr.get_sb_buffer(mag, BCACHE_SB_SIZE) else {
        return pr.none_or_err();
    };
    if !bcache_verify_checksum(pr, mag, &bcs) {
        return PROBE_NONE;
    }
    if bcs.le64(8) != BCACHE_SB_OFF / 512 {
        return PROBE_NONE;
    }
    if pr.set_uuid(bcs.span(40, 16)) < 0 {
        return PROBE_NONE;
    }
    pr.set_wiper(0, BCACHE_SB_OFF);
    PROBE_OK
}

/// `probe_bcachefs_sb_members`: this device's UUID and the filesystem's
/// size from the members field.
fn sb_members(pr: &mut Probe, field: &[u8], nr_devices: u64, dev_idx: u64) {
    let bytes = u64::from(field.le32(0)).wrapping_mul(8);
    if bytes != MEMBER_SIZE.wrapping_mul(nr_devices).wrapping_add(8) {
        return;
    }
    let at =
        |i: u64| usize::try_from(MEMBER_SIZE.wrapping_mul(i).wrapping_add(8)).unwrap_or(usize::MAX);
    pr.set_uuid_as(field.span(at(dev_idx), 16), Some("UUID_SUB"));
    let mut sectors: u64 = 0;
    for i in 0..nr_devices {
        let m = at(i);
        let nbuckets = field.le64(m.saturating_add(16));
        let bucket_size = u64::from(field.le16(m.saturating_add(26)));
        sectors = sectors.wrapping_add(nbuckets.wrapping_mul(bucket_size));
    }
    pr.set_fssize(sectors.wrapping_mul(BCACHEFS_SECTOR_SIZE));
}

/// `probe_bcachefs_sb_disk_groups`: this device's group label.
fn sb_disk_groups(pr: &mut Probe, field: &[u8], nr_devices: u64, dev_idx: u64) {
    let bytes = u64::from(field.le32(0)).wrapping_mul(8);
    if bytes != DISK_GROUP_SIZE.wrapping_mul(nr_devices).wrapping_add(8) {
        return;
    }
    let at = usize::try_from(DISK_GROUP_SIZE.wrapping_mul(dev_idx).wrapping_add(8))
        .unwrap_or(usize::MAX);
    pr.set_id_label("LABEL_SUB", field.span(at, 32));
}

/// `probe_bcachefs_sb_fields`: walk the variable fields after the fixed
/// superblock, as far as they stay inside it.
fn sb_fields(pr: &mut Probe, sb: &[u8], nr_devices: u64, dev_idx: u64) {
    let end = sb.len();
    let mut at = usize::try_from(BCACHEFS_SB_FIELDS_OFF).unwrap_or(usize::MAX);
    loop {
        // `is_within_range(field, sizeof(*field), sb_end)`.
        if at >= end || end.wrapping_sub(at) < 8 {
            break;
        }
        let field = sb.get(at..).unwrap_or_default();
        let field_size = u64::from(field.le32(0)).wrapping_mul(8);
        if field_size < 8 {
            break;
        }
        if u64::try_from(end.wrapping_sub(at)).unwrap_or(0) < field_size {
            break;
        }
        let ty = field.le32(4);
        if ty == 0 {
            break;
        }
        if ty == FIELD_TYPE_MEMBERS {
            sb_members(pr, field, nr_devices, dev_idx);
        }
        if ty == FIELD_TYPE_DISK_GROUPS {
            sb_disk_groups(pr, field, nr_devices, dev_idx);
        }
        at = at.saturating_add(usize::try_from(field_size).unwrap_or(usize::MAX));
    }
}

/// `bcachefs_validate_checksum`: by the type in the flags -- none, CRC-32C,
/// CRC-64/WE or XXH64; an unknown type is not held against it.
fn bcachefs_validate_checksum(pr: &mut Probe, bcs: &[u8], sb: &[u8]) -> bool {
    let checksum_type = bcs.be64(144) >> 58;
    let data = sb.get(16..).unwrap_or_default();
    match checksum_type {
        0 => true,
        1 => {
            let crc = crc32c::crc32c_raw(!0, data) ^ !0;
            pr.verify_csum(u64::from(crc), u64::from(bcs.le32(0)))
        }
        2 => {
            let crc = crc64::crc64_we(data);
            pr.verify_csum(crc, bcs.le64(0))
        }
        7 => {
            let h = xxhash64::xxh64(data, 0);
            pr.verify_csum(h, bcs.le64(0))
        }
        _ => true,
    }
}

/// `probe_bcachefs`.
fn probe_bcachefs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(bcs) = pr.get_sb_buffer(mag, BCACHEFS_SB_FIELDS_OFF) else {
        return pr.none_or_err();
    };
    if bcs.le64(104) != BCACHE_SB_OFF / BCACHEFS_SECTOR_SIZE {
        return PROBE_NONE;
    }
    let nr_devices = u64::from(bcs.u8_at(123));
    let dev_idx = u64::from(bcs.u8_at(122));
    if nr_devices == 0 || dev_idx >= nr_devices {
        return PROBE_NONE;
    }
    let sb_size = u64::from(bcs.le32(124))
        .wrapping_mul(8)
        .wrapping_add(BCACHEFS_SB_FIELDS_OFF);
    if sb_size > BCACHEFS_SB_MAX_SIZE {
        return PROBE_NONE;
    }
    let max_bits = bcs.u8_at(257);
    if max_bits > BCACHEFS_SB_MAX_SIZE_SHIFT {
        return PROBE_NONE;
    }
    if sb_size > (BCACHEFS_SECTOR_SIZE << max_bits) {
        return PROBE_NONE;
    }
    let Some(sb) = pr.get_sb_buffer(mag, sb_size) else {
        return PROBE_NONE;
    };
    if !bcachefs_validate_checksum(pr, &bcs, &sb) {
        return PROBE_NONE;
    }
    pr.set_uuid(bcs.span(56, 16));
    pr.set_label(bcs.span(72, 32));
    let version = bcs.le16(16);
    pr.sprintf_version(format!("{}.{}", version >> 10, version & 0x3ff));
    let blocksize = u32::from(bcs.le16(120)) * 512;
    pr.set_block_size(blocksize);
    pr.set_fsblocksize(blocksize);
    pr.set_wiper(0, BCACHE_SB_OFF);
    sb_fields(pr, &sb, nr_devices, dev_idx);
    PROBE_OK
}

/// `bcache_idinfo`.
pub static BCACHE: IdInfo = IdInfo {
    name: "bcache",
    usage: USAGE_OTHER,
    flags: 0,
    minsz: 8192,
    probefunc: Some(probe_bcache),
    magics: &[IdMag::new(BCACHE_SB_MAGIC, 4, BCACHE_SB_MAGIC_OFF)],
};

/// `bcachefs_idinfo`.
pub static BCACHEFS: IdInfo = IdInfo {
    name: "bcachefs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 256 * BCACHEFS_SECTOR_SIZE,
    probefunc: Some(probe_bcachefs),
    magics: &[
        IdMag::new(BCACHE_SB_MAGIC, 4, BCACHE_SB_MAGIC_OFF),
        IdMag::new(BCACHEFS_SB_MAGIC, 4, BCACHE_SB_MAGIC_OFF),
    ],
};
