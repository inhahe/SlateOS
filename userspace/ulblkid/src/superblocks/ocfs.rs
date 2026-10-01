//! `ocfs.c`: Oracle's OCFS and OCFS2 cluster filesystems, and Oracle ASM
//! disks.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `probe_ocfs`: the volume header, and the volume label 512 bytes on.
fn probe_ocfs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let err = |pr: &Probe| pr.none_or_err();
    let base = mag.map_or(0, |m| u64::try_from(m.kboff << 10).unwrap_or(0));
    // struct ocfs_volume_header: 266 bytes.
    let Some(ovh) = pr.get_buffer(base, 266) else {
        return err(pr);
    };
    // struct ocfs_volume_label: 132 bytes.
    let Some(ovl) = pr.get_buffer(base.wrapping_add(512), 132) else {
        return err(pr);
    };
    let maj = ovh.le32(4);
    let min = ovh.le32(0);
    if maj == 1 {
        pr.set_value("SEC_TYPE", b"ocfs1\0");
    } else if maj >= 9 {
        pr.set_value("SEC_TYPE", b"ntocfs\0");
    }
    let label_len = usize::from(ovl.le16(112));
    if label_len < 64 {
        pr.set_label(ovl.span(48, label_len));
    }
    let mount_len = usize::from(ovh.le16(264));
    if mount_len < 128 {
        pr.set_value("MOUNT", ovh.span(136, mount_len));
    }
    pr.set_uuid(ovl.span(114, 16));
    pr.sprintf_version(format!("{maj}.{min}"));
    0
}

/// `probe_ocfs2`.
fn probe_ocfs2(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct ocfs2_super_block: 352 bytes.
    let Some(osb) = pr.get_sb_buffer(mag, 352) else {
        return pr.none_or_err();
    };
    pr.set_label(osb.span(272, 64));
    pr.set_uuid(osb.span(336, 16));
    pr.sprintf_version(format!("{}.{}", osb.le16(192), osb.le16(194)));
    let bits = osb.le32(248);
    if bits < 32 {
        pr.set_fsblocksize(1u32 << bits);
        pr.set_block_size(1u32 << bits);
    }
    0
}

/// `probe_oracleasm`.
fn probe_oracleasm(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct oracle_asm_disk_label: dummy[32], dl_tag[8], dl_id[24].
    let Some(dl) = pr.get_sb_buffer(mag, 64) else {
        return pr.none_or_err();
    };
    pr.set_label(dl.span(40, 24));
    0
}

/// `ocfs_idinfo`.
pub static OCFS: IdInfo = IdInfo {
    name: "ocfs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 14000 * 1024,
    probefunc: Some(probe_ocfs),
    magics: &[IdMag::new(b"OracleCFS", 8, 0)],
};

/// `ocfs2_idinfo`.
pub static OCFS2: IdInfo = IdInfo {
    name: "ocfs2",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 14000 * 1024,
    probefunc: Some(probe_ocfs2),
    magics: &[
        IdMag::new(b"OCFSV2", 1, 0),
        IdMag::new(b"OCFSV2", 2, 0),
        IdMag::new(b"OCFSV2", 4, 0),
        IdMag::new(b"OCFSV2", 8, 0),
    ],
};

/// `oracleasm_idinfo`.
pub static ORACLEASM: IdInfo = IdInfo {
    name: "oracleasm",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_oracleasm),
    magics: &[IdMag::new(b"ORCLDISK", 0, 32)],
};
