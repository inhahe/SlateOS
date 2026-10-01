//! `lvm.c`: LVM2 and LVM1 physical volumes, device-mapper snapshot COW
//! stores, dm-verity hash devices and dm-integrity devices.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_CRYPTO, USAGE_OTHER, USAGE_RAID, c_str};

/// `LVM2_ID_LEN`.
const LVM2_ID_LEN: usize = 32;
/// `LVM2_LABEL_SIZE`.
const LVM2_LABEL_SIZE: usize = 512;
/// `sizeof(struct lvm2_pv_label_header)`.
const LVM2_LABEL_HEADER: u64 = 64;
/// `offsetof(struct lvm2_pv_label_header, offset_xl)`.
const OFFSET_XL: usize = 20;

/// `lvm2_calc_crc(buf, size)`: LVM2's CRC-32 variant, a nibble at a time
/// from its own seed.
fn lvm2_calc_crc(data: &[u8]) -> u32 {
    const CRCTAB: [u32; 16] = [
        0x0000_0000,
        0x1db7_1064,
        0x3b6e_20c8,
        0x26d9_30ac,
        0x76dc_4190,
        0x6b6b_51f4,
        0x4db2_6158,
        0x5005_713c,
        0xedb8_8320,
        0xf00f_9344,
        0xd6d6_a3e8,
        0xcb61_b38c,
        0x9b64_c2b0,
        0x86d3_d2d4,
        0xa00a_e278,
        0xbdbd_f21c,
    ];
    let tab = |i: u32| {
        CRCTAB
            .get(usize::try_from(i & 0xf).unwrap_or(0))
            .copied()
            .unwrap_or(0)
    };
    data.iter().fold(0xf597_a6cf_u32, |mut crc, &b| {
        crc ^= u32::from(b);
        crc = (crc >> 4) ^ tab(crc);
        (crc >> 4) ^ tab(crc)
    })
}

/// `format_lvm_uuid`: the 32 characters in groups of 6-4-4-4-4-4-6, as a
/// C string (a NUL among them ends it).
fn format_lvm_uuid(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(LVM2_ID_LEN + 6);
    for (i, &c) in src.iter().take(LVM2_ID_LEN).enumerate() {
        if (1u32 << i) & 0x0444_4440 != 0 {
            out.push(b'-');
        }
        out.push(c);
    }
    c_str(&out).to_vec()
}

/// `probe_lvm2`: the label in the first or second sector of the matched
/// KiB, its sector number and CRC checked.
fn probe_lvm2(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(mag) = mag else {
        return 1;
    };
    let base = u64::try_from(mag.kboff << 10).unwrap_or(0);
    let mut sector = mag.kboff << 1;
    let Some(buf) = pr.get_buffer(base, 512 + LVM2_LABEL_HEADER) else {
        return pr.none_or_err();
    };
    let at = if buf.span(0, 8) == b"LABELONE" {
        0
    } else if buf.span(512, 8) == b"LABELONE" {
        sector = sector.wrapping_add(1);
        512
    } else {
        return 1;
    };
    let label = buf.span(at, 64);
    // `(unsigned) sector`.
    #[allow(clippy::cast_sign_loss, reason = "C's (unsigned) cast")]
    if label.le64(8) != u64::from(sector as u32) {
        return 1;
    }
    // The CRC runs from `offset_xl` to the end of the label's sector --
    // past the 576 bytes asked for, into the KiB the magic was matched in,
    // which upstream reads through the same pointer.
    let Some(kib) = pr.get_buffer(base, 1024) else {
        return 1;
    };
    let crc_data = kib.span(at.saturating_add(OFFSET_XL), LVM2_LABEL_SIZE - OFFSET_XL);
    if !pr.verify_csum(
        u64::from(lvm2_calc_crc(crc_data)),
        u64::from(label.le32(16)),
    ) {
        return 1;
    }
    let pv_uuid = label.span(32, LVM2_ID_LEN);
    let uuid = format_lvm_uuid(pv_uuid);
    pr.sprintf_uuid(pv_uuid, &uuid);
    // The magic is the label's type, "LVM2 001".
    pr.set_version(mag.magic);
    // pvcreate wipes the first 8 KiB: a partition table found there was
    // written later, and wins.
    pr.set_wiper(0, 8 * 1024);
    0
}

/// `probe_lvm1`.
fn probe_lvm1(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct lvm1_pv_label_header: id[2], version, _notused[10], pv_uuid[128].
    let Some(label) = pr.get_sb_buffer(mag, 172) else {
        return pr.none_or_err();
    };
    let version = label.le16(2);
    if version != 1 && version != 2 {
        return 1;
    }
    let uuid = format_lvm_uuid(label.span(44, 128));
    pr.sprintf_uuid(label.span(44, 128), &uuid);
    0
}

/// `probe_verity`.
fn probe_verity(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(sb) = pr.get_sb_buffer(mag, 512) else {
        return pr.none_or_err();
    };
    let version = sb.le32(8);
    if version != 1 {
        return 1;
    }
    pr.set_uuid(sb.span(16, 16));
    pr.sprintf_version(version.to_string());
    0
}

/// `probe_integrity`.
fn probe_integrity(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(sb) = pr.get_sb_buffer(mag, 29) else {
        return pr.none_or_err();
    };
    let version = sb.u8_at(8);
    if version == 0 {
        return 1;
    }
    pr.sprintf_version(version.to_string());
    0
}

/// `lvm2_idinfo`.
pub static LVM2: IdInfo = IdInfo {
    name: "LVM2_member",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_lvm2),
    magics: &[
        IdMag::new(b"LVM2 001", 0, 0x218),
        IdMag::new(b"LVM2 001", 0, 0x018),
        IdMag::new(b"LVM2 001", 1, 0x018),
        IdMag::new(b"LVM2 001", 1, 0x218),
    ],
};

/// `lvm1_idinfo`.
pub static LVM1: IdInfo = IdInfo {
    name: "LVM1_member",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_lvm1),
    magics: &[IdMag::new(b"HM", 0, 0)],
};

/// `snapcow_idinfo`: the magic is all.
pub static SNAPCOW: IdInfo = IdInfo {
    name: "DM_snapshot_cow",
    usage: USAGE_OTHER,
    flags: 0,
    minsz: 0,
    probefunc: None,
    magics: &[IdMag::new(b"SnAp", 0, 0)],
};

/// `verity_hash_idinfo`.
pub static VERITY_HASH: IdInfo = IdInfo {
    name: "DM_verity_hash",
    usage: USAGE_CRYPTO,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_verity),
    magics: &[IdMag::new(b"verity\0\0", 0, 0)],
};

/// `integrity_idinfo`.
pub static INTEGRITY: IdInfo = IdInfo {
    name: "DM_integrity",
    usage: USAGE_CRYPTO,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_integrity),
    magics: &[IdMag::new(b"integrt\0", 0, 0)],
};
