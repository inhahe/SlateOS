//! `promise_raid.c`: Promise FastTrack RAID members, a signature at one of
//! several distances from the end of the disk.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_RAID};

/// `PDC_SIGNATURE`.
const PDC_SIGNATURE: &[u8] = b"Promise Technology, Inc.";
/// The sectors from the end the metadata may start at.
const SECTORS: [u64; 13] = [
    63, 255, 256, 16, 399, 591, 675, 735, 911, 974, 991, 951, 3087,
];

/// `probe_pdcraid`.
fn probe_pdcraid(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    if pr.size < 0x40000 {
        return 1;
    }
    if !pr.is_reg() && !pr.is_wholedisk() {
        return 1;
    }
    let nsectors = pr.size >> 9;
    for s in SECTORS {
        if nsectors < s {
            return 1;
        }
        let off = nsectors.wrapping_sub(s) << 9;
        let Some(pdc) = pr.get_buffer(off, 24) else {
            return pr.none_or_err();
        };
        if pdc.span(0, 24) == PDC_SIGNATURE {
            if pr.set_magic(off, pdc.span(0, 24)) != 0 {
                return 1;
            }
            return 0;
        }
    }
    1
}

/// `pdcraid_idinfo`.
pub static PDCRAID: IdInfo = IdInfo {
    name: "promise_fasttrack_raid_member",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_pdcraid),
    magics: &[],
};
