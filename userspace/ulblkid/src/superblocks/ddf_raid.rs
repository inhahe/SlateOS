//! `ddf_raid.c`: SNIA DDF RAID members, the anchor header at the end of the
//! device.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_RAID};

/// `sizeof(struct ddf_header)`.
const DDF_HEADER_SIZE: u64 = 512;

/// `probe_ddf`.
fn probe_ddf(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    const BE_MAGIC: [u8; 4] = [0xDE, 0x11, 0xDE, 0x11];
    const LE_MAGIC: [u8; 4] = [0x11, 0xDE, 0x11, 0xDE];
    if pr.size < 0x30000 {
        return 1;
    }
    let mut found = None;
    for hdr in [1u64, 257] {
        let off = (pr.size / 0x200).wrapping_sub(hdr).wrapping_mul(0x200);
        let Some(ddf) = pr.get_buffer(off, DDF_HEADER_SIZE) else {
            return pr.none_or_err();
        };
        let sig = ddf.span(0, 4);
        if sig == BE_MAGIC || sig == LE_MAGIC {
            found = Some((off, ddf));
            break;
        }
    }
    let Some((off, ddf)) = found else {
        return 1;
    };
    let lba = if ddf.span(0, 4) == BE_MAGIC {
        ddf.be64(96)
    } else {
        ddf.le64(96)
    };
    if lba > 0 {
        // The primary header must be where the anchor says.
        let Some(buf) = pr.get_buffer(lba << 9, 4) else {
            return pr.none_or_err();
        };
        if buf.span(0, 4) != ddf.span(0, 4) {
            return 1;
        }
    }
    pr.strncpy_uuid(ddf.span(8, 24));
    if pr.set_version(ddf.span(32, 8)) != 0 {
        return 1;
    }
    if pr.set_magic(off, ddf.span(0, 4)) != 0 {
        return 1;
    }
    0
}

/// `ddfraid_idinfo`.
pub static DDFRAID: IdInfo = IdInfo {
    name: "ddf_raid_member",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_ddf),
    magics: &[],
};
