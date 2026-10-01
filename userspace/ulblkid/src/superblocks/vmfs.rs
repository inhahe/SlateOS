//! `vmfs.c`: VMware VMFS filesystems and the volumes they live on.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM, USAGE_RAID};

/// VMFS's UUID layout: two little-endian words, then bytes as they are.
fn vmfs_uuid(u: &[u8]) -> String {
    let b = |k: usize| u.u8_at(k);
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b(3),
        b(2),
        b(1),
        b(0),
        b(7),
        b(6),
        b(5),
        b(4),
        b(9),
        b(8),
        b(10),
        b(11),
        b(12),
        b(13),
        b(14),
        b(15)
    )
}

/// `probe_vmfs_fs`.
fn probe_vmfs_fs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct vmfs_fs_info: magic, volume_version, version, uuid[16], mode,
    // label[128].
    let Some(h) = pr.get_sb_buffer(mag, 157) else {
        return pr.none_or_err();
    };
    let uuid = h.span(9, 16);
    pr.sprintf_uuid(uuid, vmfs_uuid(uuid));
    pr.set_label(h.span(29, 128));
    pr.sprintf_version(h.u8_at(8).to_string());
    0
}

/// `probe_vmfs_volume`.
fn probe_vmfs_volume(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct vmfs_volume_info: magic, ver, irrelevant[122], uuid[16].
    let Some(h) = pr.get_sb_buffer(mag, 146) else {
        return pr.none_or_err();
    };
    pr.set_value_str("UUID_SUB", vmfs_uuid(h.span(130, 16)).as_bytes());
    pr.sprintf_version(h.le32(4).to_string());
    // The LVM UUID: 20 bytes into the LVM info, 512 into the volume info at
    // 1 MiB.
    if let Some(lvm_uuid) = pr.get_buffer(1024 * 1024 + 512 + 20, 35) {
        pr.strncpy_uuid(&lvm_uuid);
    }
    0
}

/// `vmfs_fs_idinfo`.
pub static VMFS_FS: IdInfo = IdInfo {
    name: "VMFS",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_vmfs_fs),
    magics: &[IdMag::new(b"\x5e\xf1\xab\x2f", 2048, 0)],
};

/// `vmfs_volume_idinfo`.
pub static VMFS_VOLUME: IdInfo = IdInfo {
    name: "VMFS_volume_member",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_vmfs_volume),
    magics: &[IdMag::new(b"\x0d\xd0\x01\xc0", 1024, 0)],
};
