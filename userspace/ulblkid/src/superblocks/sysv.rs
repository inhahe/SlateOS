//! `sysv.c`: System V and Xenix filesystems.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `SYSV_BLOCK_SIZE`.
const SYSV_BLOCK_SIZE: u64 = 1024;
/// `offsetof(struct sysv_super_block, s_magic)`.
const SYSV_MAGIC_OFF: usize = 504;

/// `probe_xenix`.
fn probe_xenix(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct xenix_super_block: 1024 bytes, s_fname at 632.
    let Some(sb) = pr.get_sb_buffer(mag, 1024) else {
        return pr.none_or_err();
    };
    pr.set_label(sb.span(632, 6));
    0
}

/// `probe_sysv`: the superblock half a block into block 0, 9, 15 or 18,
/// either byte order.
fn probe_sysv(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    for block in [0u64, 9, 15, 18] {
        let off = block
            .wrapping_mul(SYSV_BLOCK_SIZE)
            .wrapping_add(SYSV_BLOCK_SIZE / 2);
        // struct sysv_super_block: 512 bytes, naturally aligned.
        let Some(sb) = pr.get_buffer(off, 512) else {
            return pr.none_or_err();
        };
        let magic = sb.span(SYSV_MAGIC_OFF, 4);
        if magic == 0xfd18_7e20_u32.to_le_bytes() || magic == 0xfd18_7e20_u32.to_be_bytes() {
            if pr.set_label(sb.span(440, 6)) != 0 {
                return 1;
            }
            if pr.set_magic(off.wrapping_add(SYSV_MAGIC_OFF as u64), magic) != 0 {
                return 1;
            }
            return 0;
        }
    }
    1
}

/// `xenix_idinfo`.
pub static XENIX: IdInfo = IdInfo {
    name: "xenix",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_xenix),
    magics: &[
        IdMag::new(b"\x2b\x55\x44", 1, 0x400),
        IdMag::new(b"\x44\x55\x2b", 1, 0x400),
    ],
};

/// `sysv_idinfo`: both byte orders and four places, so the probe function
/// looks for the magic itself.
pub static SYSV: IdInfo = IdInfo {
    name: "sysv",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_sysv),
    magics: &[],
};
