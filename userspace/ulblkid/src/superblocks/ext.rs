//! `ext.c`: ext2, ext3, ext4, ext4dev and ext3/4 external journals (jbd) --
//! one superblock, told apart by its feature flags.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, SUBLKS_SECTYPE, USAGE_FILESYSTEM, USAGE_OTHER};

/// `EXT_SB_OFF`.
const EXT_SB_OFF: u64 = 0x400;
/// `offsetof(struct ext2_super_block, s_checksum)`.
const S_CHECKSUM: usize = 1020;
/// `EXT2_FLAGS_TEST_FILESYS`.
const EXT2_FLAGS_TEST_FILESYS: u32 = 0x0004;
/// `EXT3_FEATURE_COMPAT_HAS_JOURNAL`.
const HAS_JOURNAL: u32 = 0x0004;
/// `EXT4_FEATURE_RO_COMPAT_METADATA_CSUM`.
const METADATA_CSUM: u32 = 0x0400;
/// `EXT3_FEATURE_INCOMPAT_JOURNAL_DEV`.
const JOURNAL_DEV: u32 = 0x0008;
/// `EXT4_FEATURE_INCOMPAT_64BIT`.
const INCOMPAT_64BIT: u32 = 0x0080;
/// `EXT2_FEATURE_RO_COMPAT_SUPP` (ext3's is the same).
const RO_COMPAT_SUPP: u32 = 0x0001 | 0x0002 | 0x0004;
/// `EXT2_FEATURE_INCOMPAT_SUPP`.
const EXT2_INCOMPAT_SUPP: u32 = 0x0002 | 0x0010;
/// `EXT3_FEATURE_INCOMPAT_SUPP`.
const EXT3_INCOMPAT_SUPP: u32 = 0x0002 | 0x0004 | 0x0010;

/// The superblock and its three feature words.
struct Super {
    es: crate::Buf,
    fc: u32,
    fi: u32,
    frc: u32,
}

/// `ext_get_super`: the superblock at 1 KiB, its checksum verified where
/// the filesystem has one.
fn get_super(pr: &mut Probe) -> Option<Super> {
    let es = pr.get_buffer(EXT_SB_OFF, 1024)?;
    let frc = es.le32(100);
    if frc & METADATA_CSUM != 0 {
        let csum = crc32c::crc32c_raw(!0, es.span(0, S_CHECKSUM));
        if !pr.verify_csum(u64::from(csum), u64::from(es.le32(S_CHECKSUM))) {
            return None;
        }
    }
    Some(Super {
        fc: es.le32(92),
        fi: es.le32(96),
        frc,
        es,
    })
}

/// `ext_get_info(pr, ver, es)`.
fn get_info(pr: &mut Probe, ver: u32, es: &[u8]) {
    let incompat = es.le32(96);
    if es.u8_at(120) != 0 {
        pr.set_label(es.span(120, 16));
    }
    pr.set_uuid(es.span(104, 16));
    if es.le32(92) & HAS_JOURNAL != 0 {
        pr.set_uuid_as(es.span(208, 16), Some("EXT_JOURNAL"));
    }
    if ver != 2 && pr.chain_flags() & SUBLKS_SECTYPE != 0 && incompat & !EXT2_INCOMPAT_SUPP == 0 {
        pr.set_value("SEC_TYPE", b"ext2\0");
    }
    pr.sprintf_version(format!("{}.{}", es.le32(76), es.le16(62)));
    let log = es.le32(24);
    let mut block_size: u32 = 0;
    if log < 32 {
        block_size = 1024u32.wrapping_shl(log);
        pr.set_fsblocksize(block_size);
        pr.set_block_size(block_size);
    }
    let hi = if incompat & INCOMPAT_64BIT != 0 {
        u64::from(es.le32(336)) << 32
    } else {
        0
    };
    pr.set_fslastblock(u64::from(es.le32(4)) | hi);
    // The total, not less overhead: a little above what statfs says.
    pr.set_fssize(u64::from(block_size).wrapping_mul(u64::from(es.le32(4))));
}

/// A failed `ext_get_super`.
fn failed(pr: &Probe) -> i32 {
    pr.none_or_err()
}

/// `probe_jbd`.
fn probe_jbd(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(s) = get_super(pr) else {
        return failed(pr);
    };
    if s.fi & JOURNAL_DEV == 0 {
        return 1;
    }
    get_info(pr, 2, &s.es);
    pr.set_uuid_as(s.es.span(104, 16), Some("LOGUUID"));
    0
}

/// `probe_ext2`.
fn probe_ext2(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(s) = get_super(pr) else {
        return failed(pr);
    };
    if s.fc & HAS_JOURNAL != 0 {
        return 1;
    }
    if s.frc & !RO_COMPAT_SUPP != 0 || s.fi & !EXT2_INCOMPAT_SUPP != 0 {
        return 1;
    }
    get_info(pr, 2, &s.es);
    0
}

/// `probe_ext3`.
fn probe_ext3(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(s) = get_super(pr) else {
        return failed(pr);
    };
    if s.fc & HAS_JOURNAL == 0 {
        return 1;
    }
    if s.frc & !RO_COMPAT_SUPP != 0 || s.fi & !EXT3_INCOMPAT_SUPP != 0 {
        return 1;
    }
    get_info(pr, 3, &s.es);
    0
}

/// `probe_ext4dev`.
fn probe_ext4dev(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(s) = get_super(pr) else {
        return failed(pr);
    };
    if s.fi & JOURNAL_DEV != 0 {
        return 1;
    }
    if s.es.le32(352) & EXT2_FLAGS_TEST_FILESYS == 0 {
        return 1;
    }
    get_info(pr, 4, &s.es);
    0
}

/// `probe_ext4`: a feature ext3 does not understand, and not marked as a
/// test filesystem (which is ext4dev's).
fn probe_ext4(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(s) = get_super(pr) else {
        return failed(pr);
    };
    if s.fi & JOURNAL_DEV != 0 {
        return 1;
    }
    if s.frc & !RO_COMPAT_SUPP == 0 && s.fi & !EXT3_INCOMPAT_SUPP == 0 {
        return 1;
    }
    if s.es.le32(352) & EXT2_FLAGS_TEST_FILESYS != 0 {
        return 1;
    }
    get_info(pr, 4, &s.es);
    0
}

/// `BLKID_EXT_MAGICS`: 0xEF53 at 1 KiB + 0x38.
const EXT_MAGICS: &[IdMag] = &[IdMag::new(b"\x53\xef", 1, 0x38)];

/// `jbd_idinfo`.
pub static JBD: IdInfo = IdInfo {
    name: "jbd",
    usage: USAGE_OTHER,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_jbd),
    magics: EXT_MAGICS,
};

/// `ext2_idinfo`.
pub static EXT2: IdInfo = IdInfo {
    name: "ext2",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_ext2),
    magics: EXT_MAGICS,
};

/// `ext3_idinfo`.
pub static EXT3: IdInfo = IdInfo {
    name: "ext3",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_ext3),
    magics: EXT_MAGICS,
};

/// `ext4_idinfo`.
pub static EXT4: IdInfo = IdInfo {
    name: "ext4",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_ext4),
    magics: EXT_MAGICS,
};

/// `ext4dev_idinfo`.
pub static EXT4DEV: IdInfo = IdInfo {
    name: "ext4dev",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_ext4dev),
    magics: EXT_MAGICS,
};
