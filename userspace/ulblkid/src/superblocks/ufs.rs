//! `ufs.c`: UFS1 and UFS2 (and their vendor variants), either byte order.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, Endianness, USAGE_FILESYSTEM};

/// `sizeof(struct ufs_super_block)`.
const UFS_SB_SIZE: u64 = 1377;
/// `offsetof(struct ufs_super_block, fs_magic)`.
const FS_MAGIC: usize = 1372;
/// `UFS2_MAGIC`.
const UFS2_MAGIC: u32 = 0x1954_0119;
/// The magics, in upstream's order: UFS2, UFS, and the FEA, LFN, SEC and
/// 4GB variants.
const MAGS: [u32; 6] = [
    UFS2_MAGIC,
    0x0001_1954,
    0x0019_5612,
    0x0009_5014,
    0x0061_2195,
    0x0523_1994,
];

/// `probe_ufs`: the superblock at 0, 8, 64 or 256 KiB.
fn probe_ufs(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let mut hit = None;
    'outer: for kib in [0u64, 8, 64, 256] {
        let Some(ufs) = pr.get_buffer(kib.wrapping_mul(1024), UFS_SB_SIZE) else {
            return pr.none_or_err();
        };
        let (be, le) = (ufs.be32(FS_MAGIC), ufs.le32(FS_MAGIC));
        for m in MAGS {
            if le == m || be == m {
                hit = Some((kib, ufs, m, be == m));
                break 'outer;
            }
        }
    }
    let Some((kib, ufs, magic, is_be)) = hit else {
        return 1;
    };
    if magic == UFS2_MAGIC {
        pr.set_version(b"2");
        pr.set_label(ufs.span(680, 32));
    } else {
        pr.set_version(b"1");
    }
    let id = ufs.span(144, 8);
    if id.iter().any(|&b| b != 0) {
        let (a, b) = if is_be {
            (id.be32(0), id.be32(4))
        } else {
            (id.le32(0), id.le32(4))
        };
        pr.sprintf_uuid(id, format!("{a:08x}{b:08x}"));
    }
    if pr.set_magic(
        kib.wrapping_mul(1024).wrapping_add(FS_MAGIC as u64),
        ufs.span(FS_MAGIC, 4),
    ) != 0
    {
        return 1;
    }
    let bsize = if is_be { ufs.be32(52) } else { ufs.le32(52) };
    pr.set_fsblocksize(bsize);
    pr.set_block_size(bsize);
    pr.set_fsendianness(if is_be {
        Endianness::Big
    } else {
        Endianness::Little
    });
    0
}

/// `ufs_idinfo`.
pub static UFS: IdInfo = IdInfo {
    name: "ufs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_ufs),
    magics: &[],
};
